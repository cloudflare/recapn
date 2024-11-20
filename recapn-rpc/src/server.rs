//! Implements the local server side of RPC which consists of:
//! * Server dispatchers
//! * Request queueing
//! * Response handling

use recapn::message::Message;
use recapn::rpc::Capable;
use recapn::{any, message, ty, BuilderOf, ReaderOf};
use recapn_channel::mpsc;
use std::convert;
use std::future::Future;
use std::marker::PhantomData;
use std::mem;
use std::pin::Pin;
use tokio::task::JoinHandle;

use crate::chan::{self, LocalMessage, RpcCall, RpcChannel};
use crate::client::{self, Client};
use crate::table::{CapTable, Table};
use crate::{table, Error, Result};

pub fn new_server<T>(dispatcher: T) -> (Client, Dispatcher<T>) {
    let (sender, recv) = mpsc::channel(RpcChannel::Local);
    let dispatcher = Dispatcher {
        state: DispatcherState::Active(recv),
        inner: dispatcher,
    };
    (Client(sender), dispatcher)
}

/// Creates a client - dispatcher pair with the given server.
pub trait FromServer<T>: Sized {
    type Dispatcher: Dispatch;

    fn from_server(server: T) -> (Self, Dispatcher<Self::Dispatcher>);
}

enum DispatcherState {
    Broken(Error),
    Active(chan::Receiver),
}

use DispatcherState::*;

/// The receiver side of a server that executes requests
pub struct Dispatcher<T: ?Sized> {
    state: DispatcherState,
    inner: T,
}

impl<T: Dispatch + ?Sized> Dispatcher<T> {
    /// Run the dispatcher with the given executor and handle all
    /// requests until all connections to the server have been severed.
    ///
    /// # Panics
    ///
    /// This must be called within the context of a LocalSet. Calling this outside a LocalSet
    /// will result in a panic.
    pub async fn run(&mut self) -> Result<()> {
        while self.run_once().await? {}

        Ok(())
    }

    /// Handles exactly one message to the server. Returns a bool indicating whether a message
    /// was handled. If the server is broken or breaks while handling a blocking message, this
    /// returns an error.
    ///
    /// # Drop safety
    ///
    /// It is safe to drop the future returned by this function, but if the dispatcher is handling
    /// a blocking request, that request might be canceled prematurely. If this occurs, the
    /// dispatcher is automatically broken.
    pub async fn run_once(&mut self) -> Result<bool> {
        let receiver = match &mut self.state {
            Broken(err) => return Err(err.clone()),
            Active(r) => r,
        };

        let Some(req) = receiver.recv().await else {
            return Ok(false);
        };
        let req = match req {
            mpsc::Item::Request(req) => req,
            mpsc::Item::Event(event) => {
                // I want this to explode when I actually make an event value.
                let () = event.into_inner();
                return Ok(true)
            },
        };
        let (request, responder) = req.respond();
        let RpcCall {
            interface,
            method,
            params,
            target: _,
        } = request;

        let request = DispatchRequest {
            interface,
            method,
            params,
            responder,
        };
        let DispatchResponse(result) = self.inner.dispatch(request);
        match result {
            DispatchResult::Done => {} // do nothing, it has already been handled
            DispatchResult::Broken(err) => {
                // A streaming call returned an error! We're immediately broken
                match mem::replace(&mut self.state, Broken(err.clone())) {
                    Active(r) => r.close(err.clone()),
                    Broken(_) => unreachable!(),
                }
                return Err(err);
            }
            DispatchResult::Streaming(task) => {
                // Set up a guard so that the dispatcher becomes broken if we're dropped while
                // handling a blocking request.
                struct Bomb<'a>(Option<&'a mut DispatcherState>);
                impl Bomb<'_> {
                    fn defuse(mut self) {
                        self.0 = None;
                    }
                }
                impl Drop for Bomb<'_> {
                    fn drop(&mut self) {
                        if let Some(s) = self.0.take() {
                            let err =
                                Error::failed("a future handling a blocking request was dropped");
                            match mem::replace(s, Broken(err.clone())) {
                                Active(r) => r.close(err),
                                Broken(_) => unreachable!(),
                            }
                        }
                    }
                }
                let bomb = Bomb(Some(&mut self.state));
                let result = task.await;
                bomb.defuse(); // counter-terrorists win

                let flattened = result
                    .map_err(|join_err| Error::failed(join_err.to_string()))
                    .and_then(convert::identity);

                if let Err(err) = flattened {
                    match mem::replace(&mut self.state, Broken(err.clone())) {
                        Active(r) => r.close(err.clone()),
                        Broken(_) => unreachable!(),
                    }
                    return Err(err);
                }
            }
        }

        Ok(true)
    }

    /// Returns a reference to the error indicating why this dispatcher is broken.
    ///
    /// If an error is returned while handling a blocking request, the dispatcher becomes
    /// broken. In this case, this function returns the error and all future requests
    /// return the same error.
    pub fn broken_err(&self) -> Option<&Error> {
        match &self.state {
            Active(_) => None,
            Broken(err) => Some(err),
        }
    }

    /// Gets a reference to the underlying server dispatcher
    pub fn get_ref(&self) -> &T {
        &self.inner
    }
}

/// Dispatches requests to given interface methods by their IDs.
///
/// In generated code, a server dispatcher struct and server trait are generated for each
/// interface. Dispatcher structs wrap a type that implement their server trait and implement
/// the `Dispatch` trait to route requests by their interface and method ID.
///
/// # Example
///
/// Given this capnp interface
/// ```capnp
/// interface Foo {
///   bar @0 ();
///   baz @1 (bap :Data) -> stream;
/// }
/// ```
///
/// A struct and trait could be generated like so
///
/// ```ignore
/// // Server
/// pub trait FooServer {
///     fn bar(&mut self, CallContext<BarParams, BarResults> ctx) -> CallResult;
///     fn baz(&mut self, StreamContext<BazParams> ctx) -> StreamResult;
/// }
///
/// pub struct FooDispatcher<T>(pub T);
/// impl<T: FooServer> FooDispatcher<T> {
///     pub const INTERFACE_NAME: &str = "foobar.capnp:Foo";
///
///     #[doc(hidden)]
///     pub fn dispatch_foo(this: &T, request: DispatchRequest) -> DispatchResponse {
///         match request.method() {
///             0 => request.respond_with(|ctx| this.0.bar(ctx)),
///             _ => request.unimplemented_method(Self::INTERFACE_NAME),
///         }
///     }
/// }
///
/// impl<T: FooServer> Dispatch for FooDispatcher<T> {
///     fn dispatch(&self, request: DispatchRequest) -> DispatchResult {
///         match request.interface() {
///             0 => FooDispatcher::dispatch_foo(self, request),
///             _ => request.unimplemented_interface(Self::INTERFACE_NAME)
///         }
///     }
/// }
/// ```
pub trait Dispatch {
    fn dispatch(&mut self, request: DispatchRequest) -> DispatchResponse;
}

/// The params payload.
///
/// This contains methods to read the parameters of the call. Dropping this releases the
/// underlying params message and may allow the RPC system to free up buffer space to handle
/// other requests. Long-running asynchronous methods should try to drop this as early as is
/// convenient.
pub struct Parameters<P> {
    p: PhantomData<fn() -> P>,
    inner: chan::Params,
}

impl<P> Parameters<P> {
    pub fn reader(&self) -> ParametersReader<'_, P> {
        self.with_options(message::ReaderOptions::default())
    }

    pub fn with_options(&self, options: message::ReaderOptions) -> ParametersReader<'_, P> {
        ParametersReader {
            p: PhantomData,
            root: self.inner.root,
            reader: self.inner.message.reader(options),
            table: &self.inner.table,
        }
    }
}

pub struct ParametersReader<'a, P> {
    p: PhantomData<fn() -> P>,
    root: chan::ParamsRoot,
    reader: message::Reader<'a>,
    table: &'a table::Table,
}

impl<'a, P: ty::Struct> ParametersReader<'a, P> {
    pub fn get(&self) -> ReaderOf<'_, P, CapTable<'_>> {
        self.root().read_as_struct::<P>()
    }
}

impl<'a, P> ParametersReader<'a, P> {
    pub fn root(&self) -> any::PtrReader<'_, CapTable<'_>> {
        match self.root {
            chan::ParamsRoot::Params => self.reader.root().imbue(self.table.reader()),
        }
    }
}

/// The results payload and the response pipeline.
///
/// This contains methods to manipulate the results payload, the response pipeline,
/// or perform a tail call. Dropping this causes an empty response to be sent.
///
/// There are three primary ways to manipulate the response:
///
/// - `respond()`
/// - `tail_call()`
/// - `pipeline_and_results()`
///
/// `respond()` returns an instance of `Results` for manipulating the results payload.
///
/// `tail_call()` accepts a new Request that should be sent and have the response sent
/// in place for this call. With a tail call, the RPC implementation may be able to optimize
/// the tail call to another machine such that the results never actually pass through this
/// machine. Even if no such optimization is possible, `tail_call()` may allow pipelined calls
/// to be forwarded optimistically to the new call site.
///
/// `pipeline_and_results()` allows configuring the response pipeline and results separately.
/// This is analogous to `setPipeline` in C++, allowing you to tell the RPC system where the
/// capabilities in the response will resolve to, before making the response, allowing requests
/// that are promise-pipelined on this call's results to continue their journey to the final
/// destination before this call itself has completed.
///
/// # Cancellation
///
/// Request cancellation will not automatically cancel spawned futures handling requests. Instead,
/// Response and ResponseSender have functions that can be used to hook into the cancellation
/// future, allowing them to listen for cancellation to occur if the user requests it.
pub struct Response<'a, R> {
    r: PhantomData<fn() -> R>,
    responder: &'a mut Option<chan::Responder>,
    results: &'a mut Option<ResultsBuilder>,
}

impl<'a, R> Response<'a, R> {
    fn responder(self) -> chan::Responder {
        self.responder.take().unwrap()
    }

    /// Accepts a new Request that should be sent and have the response sent in place for this call.
    ///
    /// With a tail call, the RPC implementation may be able to optimize the tail call to another
    /// machine such that the results never actually pass through this machine. Even if no such
    /// optimization is possible, `tail_call()` may allow pipelined calls to be forwarded
    /// optimistically to the new call site.
    #[inline]
    pub fn tail_call<P>(self, req: client::Request<P, R>) {
        req.tail_call(self.responder())
    }
}

impl<'a, R: ty::Struct> Response<'a, R> {
    /// Returns a builder for the results of this call.
    #[inline]
    pub fn results(self) -> BuilderOf<'a, R, CapTable<'a>> {
        let builder = self.results.get_or_insert_with(|| ResultsBuilder {
            message: Box::new(Message::global()),
            table: Table::new(Vec::new()),
        });
        let b = builder.message.builder();
        let table = builder.table.builder();
        b.into_root().imbue(table).init_struct::<R>()
    }
}

struct ResultsBuilder {
    message: LocalMessage,
    table: Table,
}

pub struct CallResult(DispatchResult);

impl From<CallResult> for DispatchResponse {
    fn from(value: CallResult) -> Self {
        DispatchResponse(value.0)
    }
}

#[non_exhaustive]
pub struct CallContext<P, R> {
    pub params: Parameters<P>,
    pub response: Responder<R>,
}

pub struct Responder<R> {
    r: PhantomData<fn() -> R>,
    responder: chan::Responder,
}

macro_rules! with_context {
    ($handler:ident, |$ctx:ident| $expr:expr) => {{
        let mut responder = Some($handler.responder);
        let mut results = None;
        let $ctx = Response {
            r: PhantomData,
            responder: &mut responder,
            results: &mut results,
        };
        let result = $expr;
        if let Some(responder) = responder {
            // If the user didn't fill in a response and returned ok, fill in the RPC response
            // with an empty message.
            let response = result
                .map(|()| {
                    if let Some(results) = results {
                        chan::RpcResponse {
                            root: chan::ResultsRoot::Results,
                            message: chan::MessagePayload::Local(results.message),
                            table: results.table,
                        }
                    } else {
                        chan::RpcResponse {
                            root: chan::ResultsRoot::Results,
                            message: chan::MessagePayload::Local(Box::new(Message::global())),
                            table: table::Table::new(Vec::new()),
                        }
                    }
                })
                .into();
            responder.respond(response);
        }
    }};
}

impl<R> Responder<R> {
    pub fn spawn_with<F>(self, f: F) -> CallResult
    where
        F: FnOnce(Response<'_, R>) -> Pin<Box<dyn Future<Output = Result<()>> + Send + '_>>,
        F: Send + 'static,
    {
        let fut = async move { with_context!(self, |ctx| (f)(ctx).await) };
        tokio::task::spawn(fut);
        CallResult(DispatchResult::Done)
    }

    pub fn spawn_local_with<F>(self, f: F) -> CallResult
    where
        F: FnOnce(Response<'_, R>) -> Pin<Box<dyn Future<Output = Result<()>> + '_>>,
        F: 'static,
    {
        let fut = async move { with_context!(self, |ctx| (f)(ctx).await) };
        tokio::task::spawn_local(fut);
        CallResult(DispatchResult::Done)
    }

    pub fn respond_with<Fn>(self, f: Fn) -> CallResult
    where
        Fn: FnOnce(Response<'_, R>) -> Result<()>,
    {
        with_context!(self, |ctx| (f)(ctx));
        CallResult(DispatchResult::Done)
    }

    #[inline]
    pub fn error(self, err: Error) -> CallResult {
        self.responder.respond(chan::RpcResults::Owned(Err(err)));
        CallResult(DispatchResult::Done)
    }
}

#[non_exhaustive]
pub struct StreamContext<P> {
    pub params: Parameters<P>,
    pub response: StreamResponder,
}

pub struct StreamResponder {
    inner: chan::Responder,
}

impl StreamResponder {
    /// Respond to the call when the given future completes. If an error is returned, this breaks
    /// the stream and all future calls to this capability will return the same error.
    pub fn spawn<Fu>(self, f: Fu) -> CallResult
    where
        Fu: Future<Output = Result<()>> + Send + 'static,
    {
        let task = tokio::task::spawn(async move {
            let result = f.await;
            let response = match &result {
                Ok(()) => Ok(chan::RpcResponse {
                    root: chan::ResultsRoot::Results,
                    message: chan::MessagePayload::Local(Box::new(Message::global())),
                    table: table::Table::new(Vec::new()),
                }),
                Err(err) => Err(err.clone()),
            }
            .into();
            self.inner.respond(response);
            result
        });
        CallResult(DispatchResult::Streaming(task))
    }

    /// Respond to the call when the given local future completes. If an error is returned, this
    /// breaks the stream and all future calls to this capability will return the same error.
    pub fn spawn_local<Fu>(self, f: Fu) -> CallResult
    where
        Fu: Future<Output = Result<()>> + 'static,
    {
        CallResult(DispatchResult::Streaming(tokio::task::spawn_local(f)))
    }

    /// Respond to the call when the given function completes. If an error is returned, this
    /// breaks the stream and all future calls to this capability will return the same error.
    pub fn respond_with<Fn>(self, f: Fn) -> CallResult
    where
        Fn: FnOnce() -> Result<()>,
    {
        let r = match f() {
            Ok(()) => DispatchResult::Done,
            Err(err) => DispatchResult::Broken(err),
        };
        CallResult(r)
    }
}

pub struct DispatchRequest {
    interface: u64,
    method: u16,
    params: chan::Params,
    responder: chan::Responder,
}

impl DispatchRequest {
    #[inline]
    pub fn interface(&self) -> u64 {
        self.interface
    }

    #[inline]
    pub fn method(&self) -> u16 {
        self.method
    }

    #[inline]
    pub fn into_call_context<P, R>(self) -> CallContext<P, R> {
        CallContext {
            params: Parameters {
                p: PhantomData,
                inner: self.params,
            },
            response: Responder {
                r: PhantomData,
                responder: self.responder,
            },
        }
    }

    #[inline]
    pub fn into_stream_context<P>(self) -> StreamContext<P> {
        StreamContext {
            params: Parameters {
                p: PhantomData,
                inner: self.params,
            },
            response: StreamResponder {
                inner: self.responder,
            },
        }
    }

    /// Respond with the given error.
    #[inline]
    pub fn error(self, err: Error) -> DispatchResponse {
        self.responder.respond(chan::RpcResults::Owned(Err(err)));
        DispatchResponse(DispatchResult::Done)
    }

    /// Respond with a generic error indicating that the interface is not implemented
    /// by the dispatcher.
    #[inline]
    pub fn unimplemented_interface(self, interface_name: &str) -> DispatchResponse {
        let err = Error::unimplemented(format!(
            "Requested interface (@{:0<#16x}) not implemented for \"{}\"",
            self.interface, interface_name,
        ));
        self.error(err)
    }

    /// Respond with a generic error indicating that the method is not implemented
    /// by the dispatcher.
    #[inline]
    pub fn unimplemented_method(self, interface_name: &str) -> DispatchResponse {
        let err = Error::unimplemented(format!(
            "Method (@{}) not implemented for interface \"{}\" (@{:0<#16x})",
            self.method, interface_name, self.interface,
        ));
        self.error(err)
    }
}

enum DispatchResult {
    /// The request has been dispatched and no more work needs to be done by the dispatcher.
    Done,
    /// The request failed with the given error.
    Broken(Error),
    /// The request is streaming, so the task must be waited to completion before handling
    /// the next request
    Streaming(JoinHandle<Result<()>>),
}

pub struct DispatchResponse(DispatchResult);
