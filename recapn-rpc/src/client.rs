use crate::chan::{self, LocalMessage, Receiver, RpcCall, RpcChannel, Sender};
use crate::pipeline::{Pipeline, PipelineOf};
use crate::table::{CapTable, Table};
use crate::{Error, Result};
use pin_project::pin_project;
use recapn::any;
use recapn::message::{Message, ReaderOptions};
use recapn::rpc::{Capable, Pipelinable, TypedPipeline};
use recapn::{message, rpc, ty, BuilderOf, ReaderOf};
use recapn_channel::{mpsc, request_response, request_pipeline, request_response_pipeline};
use std::future::{Future, IntoFuture};
use std::marker::PhantomData;
use std::pin::Pin;
use std::task;
use tokio::select;

async fn wait_and_resolve(future: impl Future<Output = Client>, mut receiver: Receiver) {
    select! {
        // if all the senders and requests drop, just cancel
        _ = receiver.closed() => {},
        client = future => {
            if let Err(r) = receiver.forward_to(&client.0) {
                // Woops, it resolved into itself. Resolve it to an error instead
                r.close(Error::failed("attempted to resolve client into itself"))
            }
        },
    }
}

/// A typeless capability client.
#[derive(Clone, Debug)]
pub struct Client(pub(crate) Sender);

impl Client {
    /// Create a new client which is "broken". This client when called will always
    /// return the error passed in here.
    pub fn broken(err: Error) -> Self {
        Self(mpsc::broken(RpcChannel::Broken, err))
    }

    /// Returns a Client that queues up calls until `future` is ready, then forwards them
    /// to the new client.
    ///
    /// The future is spawned using the tokio `spawn()` function. It begins running automatically
    /// in the background.
    ///
    /// # Cancelation
    ///
    /// The future will be dropped if all client references are dropped, including those kept
    /// transitively through active requests.
    pub fn spawn<F>(future: F) -> Self
    where
        F: Future<Output = Self> + Send + 'static,
    {
        let (client, resolver) = mpsc::channel(RpcChannel::Spawned);

        let _ = tokio::task::spawn(wait_and_resolve(future, resolver));

        Self(client)
    }

    /// Returns a Client that queues up calls until `future` is ready, then forwards them
    /// to the new client.
    ///
    /// The future is spawned using the tokio `spawn_local()` function. It begins running automatically
    /// in the background.
    ///
    /// # Cancelation
    ///
    /// The future will be canceled if all client references are dropped, including those kept
    /// transitively through active requests.
    pub fn spawn_local<F>(future: F) -> Self
    where
        F: Future<Output = Self> + 'static,
    {
        let (client, resolver) = mpsc::channel(RpcChannel::Spawned);

        let _ = tokio::task::spawn_local(wait_and_resolve(future, resolver));

        Self(client)
    }

    #[inline]
    pub fn call<P, R>(&self, interface_id: u64, method_id: u16) -> Request<P, R> {
        Request {
            params: PhantomData,
            results: PhantomData,
            client: self.clone(),
            request: CallBuilder::new(interface_id, method_id),
        }
    }

    #[inline]
    pub fn streaming_call<P>(&self, interface_id: u64, method_id: u16) -> StreamingRequest<P> {
        StreamingRequest {
            params: PhantomData,
            client: self.clone(),
            request: CallBuilder::new(interface_id, method_id),
        }
    }

    pub async fn when_resolved(&self) -> Result<(), Error> {
        todo!()
    }
}

impl ty::Capability for Client {
    type Client = Self;

    #[inline]
    fn from_client(c: Self::Client) -> Self {
        c
    }
    #[inline]
    fn into_inner(self) -> Self::Client {
        self
    }
}

pub(crate) struct CallBuilder {
    interface: u64,
    method: u16,
    params: LocalMessage,
    table: Table,
}

impl CallBuilder {
    fn new(interface: u64, method: u16) -> Self {
        Self {
            interface,
            method,
            params: Box::new(Message::global()),
            table: Table::new(Vec::new()),
        }
    }

    pub fn params(&mut self) -> any::PtrBuilder<'_, CapTable<'_>> {
        let table = self.table.builder();
        self.params.builder().into_root().imbue(table)
    }

    pub fn build(self) -> RpcCall {
        RpcCall {
            interface: self.interface,
            method: self.method,
            params: chan::Params {
                root: chan::ParamsRoot::Params,
                message: chan::MessagePayload::Local(self.params),
                table: self.table,
            },
            target: chan::ResponseTarget::Local,
        }
    }
}

pub(crate) fn dropped_cap() -> Error {
    Error::disconnected("capability was dropped")
}

fn dropped_request() -> Error {
    Error::disconnected("request was dropped before it could be responded to")
}

pub struct Request<P, R> {
    params: PhantomData<fn() -> P>,
    results: PhantomData<fn() -> R>,
    client: Client,
    request: CallBuilder,
}

impl<P: ty::Struct, R> Request<P, R> {
    pub fn params(&mut self) -> BuilderOf<'_, P, CapTable<'_>> {
        self.params_ptr().init_struct::<P>()
    }
}

impl<P, R> Request<P, R> {
    pub fn params_ptr(&mut self) -> any::PtrBuilder<'_, CapTable<'_>> {
        self.request.params()
    }

    /// Send the request without setting up pipelining.
    pub fn send(self) -> Result<ResponseReceiver<R>> {
        let call = self.request.build();
        let (req, resp) = request_response::<RpcChannel>(call);

        if let Err((_, err)) = self.client.0.send(req) {
            return Err(err.map_or_else(dropped_cap, |e| e.clone()));
        }

        Ok(ResponseReceiver {
            results: PhantomData,
            recv: resp,
        })
    }

    pub(crate) fn tail_call(self, resp: chan::Responder) {
        let req = resp.tail_call(self.request.build());

        if let Err((req, err)) = self.client.0.send(req) {
            let err = err.map_or_else(dropped_cap, |e| e.clone());
            req.respond().1.respond(chan::RpcResults::Owned(Err(err)));
        }
    }
}

impl<P, R: Pipelinable> Request<P, R> {
    /// Send the request.
    pub fn send_and_pipeline(self) -> Result<(ResponseReceiver<R>, PipelineOf<R>)> {
        let call = self.request.build();
        let (req, resp, pipeline) = request_response_pipeline::<RpcChannel>(call);

        if let Err((_, err)) = self.client.0.send(req) {
            return Err(err.map_or_else(dropped_cap, |e| e.clone()));
        }

        let response = ResponseReceiver {
            results: PhantomData,
            recv: resp,
        };
        let pipe = TypedPipeline::from_pipeline(rpc::Pipeline::new(Pipeline::new(pipeline)));
        Ok((response, pipe))
    }

    /// Send the request without a response. This only supports pipelining operations.
    pub fn pipeline(self) -> Result<PipelineOf<R>> {
        let call = self.request.build();
        let (req, pipeline) = request_pipeline::<RpcChannel>(call);

        if let Err((_, err)) = self.client.0.send(req) {
            return Err(err.map_or_else(dropped_cap, |e| e.clone()));
        }

        Ok(TypedPipeline::from_pipeline(rpc::Pipeline::new(
            Pipeline::new(pipeline),
        )))
    }
}

pub struct StreamingRequest<P> {
    params: PhantomData<fn() -> P>,
    client: Client,
    request: CallBuilder,
}

impl<P: ty::Struct> StreamingRequest<P> {
    pub fn params(&mut self) -> BuilderOf<'_, P, CapTable<'_>> {
        self.request.params().init_struct::<P>()
    }
}

impl<P> StreamingRequest<P> {
    pub async fn send(self) -> Result<()> {
        let call = self.request.build();
        let (req, resp) = request_response::<RpcChannel>(call);

        // TODO handle flow control.

        if let Err((_, err)) = self.client.0.send(req) {
            return Err(err.map_or_else(dropped_cap, |e| e.clone()));
        }

        let response = resp.await.ok_or_else(dropped_request)?;
        if let Err(err) = response.unwrap_results() {
            return Err(err.clone());
        }

        Ok(())
    }
}

pub struct ResponseReceiver<R> {
    results: PhantomData<fn() -> R>,
    recv: chan::ResponseReceiver,
}

impl<R> IntoFuture for ResponseReceiver<R> {
    type IntoFuture = Recv<R>;
    type Output = Response<R>;

    #[inline]
    fn into_future(self) -> Self::IntoFuture {
        Recv {
            results: PhantomData,
            recv: self.recv.into_future(),
        }
    }
}

#[pin_project]
pub struct Recv<R> {
    results: PhantomData<fn() -> R>,
    #[pin]
    recv: chan::Recv,
}

impl<R> Future for Recv<R> {
    type Output = Response<R>;

    #[inline]
    fn poll(self: Pin<&mut Self>, cx: &mut task::Context<'_>) -> task::Poll<Self::Output> {
        self.project().recv.poll(cx).map(|inner| Response {
            results: PhantomData,
            inner,
        })
    }
}

/// An RPC response. This can be cloned at low cost and shared between threads.
pub struct Response<R> {
    results: PhantomData<fn() -> R>,
    inner: Option<chan::Response>,
}

impl<R> Response<R> {
    /// Get a new reader for the results of this message.
    pub fn results(&self) -> Result<Results<'_, R>> {
        self.results_with_options(ReaderOptions::default())
    }

    pub fn results_with_options(&self, ops: ReaderOptions) -> Result<Results<'_, R>> {
        let Some(inner) = &self.inner else {
            return Err(dropped_request());
        };

        match inner.unwrap_results() {
            Ok(r) => Ok(Results {
                t: PhantomData,
                root: r.root,
                reader: r.message.reader(ops),
                table: &r.table,
            }),
            Err(err) => Err(err.clone()),
        }
    }
}

impl<R> Clone for Response<R> {
    fn clone(&self) -> Self {
        Self {
            results: PhantomData,
            inner: self.inner.clone(),
        }
    }
}

/// A reader for the results message
pub struct Results<'a, T> {
    t: PhantomData<fn() -> T>,
    root: chan::ResultsRoot,
    reader: message::Reader<'a>,
    table: &'a Table,
}

impl<T: ty::Struct> Results<'_, T> {
    /// Gets the root structure of the results.
    pub fn get(&self) -> ReaderOf<'_, T, CapTable<'_>> {
        match self.root {
            chan::ResultsRoot::Results => self
                .reader
                .root()
                .imbue(self.table.reader())
                .read_as_struct::<T>(),
        }
    }
}
