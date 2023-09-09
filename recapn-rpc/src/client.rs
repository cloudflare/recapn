use crate::table::{Table, CapTable};
use crate::{Result, Error, PipelineOp};
use crate::sync::{request, mpsc};
use std::borrow::Cow;
use std::future::Future;
use std::marker::PhantomData;
use std::sync::Arc;
use recapn::any::{AnyStruct, self};
use recapn::message::Message;
use recapn::rpc::Capable;

mod error;
mod local;
mod queue;

use self::queue::PipelineResolver;

pub use local::{Dispatch, Dispatcher};

type RpcMessage = Message<'static, recapn::alloc::Growing<recapn::alloc::Global>>;

/// Indicates what kind of structure exists in the root pointer of the params message.
/// 
/// For most messages, like local or queued messages, this will be a pointer to the user defined
/// params structure itself. For outgoing messages, this will be a Call structure.
/// 
/// As the message flows through the RPC system, what exists in the root pointer might change.
/// If a local queue resolves into a remote capability, the parameters at the root will be
/// disowned so a Call structure can be made there instead. The parameters will then be accessable
/// from within the Payload located within the Call.
enum ParamsRoot {
    /// The message root is the request parameters.
    Params,
}

struct Params {
    root: ParamsRoot,
    message: RpcMessage,
    /// The capability table
    table: Table,
}

impl Params {
    fn builder(&mut self) -> any::PtrBuilder<'_, CapTable<'_>> {
        let ptr = match self.root {
            ParamsRoot::Params => self.message.builder().into_root(),
        };

        ptr.imbue(self.table.builder())
    }
}

struct RpcCall {
    /// The client that the call was made on. This is intended to keep the client
    /// alive for the duration that the request is in holding.
    /// 
    /// This is primarily intended for queue clients which cancel the future resolving
    /// the client to call if all references to the queue client go out of scope.
    pub client: UnbrokenClient,

    /// The interface ID for the call
    pub interface: u64,
    /// The method ID for the call
    pub method: u16,

    /// The parameters of the call contained in a message.
    pub params: Params,
}

struct RpcResponse {
}

type RpcResult = Result<RpcResponse>;

type RpcRequest = request::Request<RpcCall, RpcResult>;
type RpcResponseReceiver = request::ResponseReceiver<RpcCall, RpcResult>;
type RpcResponder = request::Responder<RpcCall, RpcResult>;

type RequestSender = mpsc::Sender<RpcCall, RpcResult>;
type RequestReceiver = mpsc::Receiver<RpcCall, RpcResult>;

#[derive(Clone)]
enum UnbrokenClient {
    Queue(Arc<queue::QueueClient>),
    Local(local::LocalClient),
}

use UnbrokenClient::{Queue, Local};

impl From<UnbrokenClient> for ClientKind {
    #[inline]
    fn from(value: UnbrokenClient) -> Self {
        ClientKind::Unbroken(value)
    }
}

impl From<UnbrokenClient> for Client {
    #[inline]
    fn from(value: UnbrokenClient) -> Self {
        ClientKind::from(value).into()
    }
}

#[derive(Clone)]
enum ClientKind {
    Unbroken(UnbrokenClient),
    Broken(Arc<error::BrokenClient>),
}

use ClientKind::{Broken, Unbroken};

impl From<ClientKind> for Client {
    #[inline]
    fn from(value: ClientKind) -> Self {
        Client { kind: value }
    }
}

/// A typeless capability client.
#[derive(Clone)]
pub struct Client {
    kind: ClientKind,
}

impl Client {
    /// Create a new client which is "broken". This client when called will always
    /// return the error passed in here.
    pub fn broken(err: Error) -> Client {
        todo!()
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
    pub fn spawn<F>(future: F) -> Client
    where
        F: Future<Output = Client> + Send + 'static,
    {
        Queue(queue::spawn(future)).into()
    }

    /// Returns a Client that queues up calls until `future` is ready, then forwards them
    /// to the new client.
    /// 
    /// The future is spawned using the tokio `spawn_local()` function. It begins running automatically
    /// in the background.
    /// 
    /// # Cancelation
    /// 
    /// The future will be dropped if all client references are dropped, including those kept
    /// transitively through active requests.
    pub fn spawn_local<F>(future: F) -> Client
    where
        F: Future<Output = Client> + 'static,
    {
        Queue(queue::spawn_local(future)).into()
    }

    pub fn call(&self, interface_id: u64, method_id: u16) -> Result<Request> {
        match &self.kind {
            Unbroken(unbroken) => {
                Ok(Request::new(unbroken.clone(), interface_id, method_id))
            },
            Broken(_) => todo!(),
        }
    }

    pub fn err(&self) -> Option<&Error> {
        todo!()
    }

    // If this Client is a promise that has already resolved, returns the inner, resolved version
    // of the capability.  The caller may permanently replace this client with the resolved one if
    // desired.  Returns null if the client isn't a promise or hasn't resolved yet -- use
    // `more_resolved()` to distinguish between them.
    pub fn resolved(&self) -> Option<Self> {
        todo!()
    }

    // If this Client is a promise that has already resolved, returns the inner, resolved version
    // of the capability. If it resolved into an error, or is an error itself, return the error.
    pub fn try_resolved(&self) -> Result<Option<Self>> {
        todo!()
    }

    /// If this client is a settled reference (not a promise), return None.
    /// Otherwise, wait for when it resolves into a new client that is closer to
    /// being the final, settled client. Calling this repeatedly should eventually
    /// produce a settled client. `when_resolved` does this for you.
    pub async fn more_resolved(&self) -> Option<Self> {
        todo!()
    }

    pub async fn when_resolved(&self) -> Self {
        todo!()
    }
}

pub struct Request {
    call: RpcCall,
}

impl Request {
    fn new(client: UnbrokenClient, interface: u64, method: u16) -> Self {
        Request {
            call: RpcCall {
                client,
                interface,
                method,
                params: Params {
                    root: ParamsRoot::Params,
                    message: Message::global(),
                    table: Table::new(),
                },
            }
        }
    }

    pub fn params(&mut self) -> any::PtrBuilder<CapTable> {
        self.call.params.builder()
    }

    /// Send the request and receive the response. This does not set up pipelining.
    /// To use pipelining, use `pipeline()` or `send_and_pipeline()`
    pub async fn send(self) -> Result<Response> {
        let client = self.call.client.clone();
        let (req, response) = request::channel(self.call);
        match client {
            Queue(queue) => queue.call_forwarding.send(req),
            Local(local) => local.sender.send(req),
        }

        // Since we don't need to set up pipelining, we can just await the response here.
        response.await
            .unwrap_or_else(|| Err(Error::failed("failed to receive response")))
            .map(|resp| Response { inner: resp })
    }

    /// Send the request and return a pipeline.
    pub fn pipeline(self) -> Pipeline {
        let (mut pipeline, resolver) = queue::pipeline();
        let client = self.call.client.clone();
        let (req, response) = request::channel(self.call);
        match client {
            Queue(queue) => queue.call_forwarding.send(req),
            Local(local) => local.sender.send(req),
        }

        pipeline.response = Some(response);
        Pipeline { kind: PipelineKind::Unresolved(Arc::new(pipeline)) }
    }

    /// Send the request and return a pipeline and a future to receive the response.
    pub fn send_and_pipeline(self) -> ((), Pipeline) {
        todo!()
    }
}

/// An RPC response. This can be cloned at low cost and shared between threads.
pub struct Response {
    inner: RpcResponse,
}

impl Response {
    /// Get a new reader for the results of this message.
    pub fn results(&self) -> Results<()> {
        todo!()
    }
}

/// A reader for the results message
pub struct Results<'a, T> {
    p: PhantomData<&'a T>,
}

impl<T> Results<'_, T> {
    /// Gets the root structure of the results.
    pub fn get(&self) -> () {
        todo!()
    }
}

#[derive(Clone)]
enum ResolvedPipeline {
    Local(request::Response<RpcCall, RpcResult>),
}

impl ResolvedPipeline {
    #[inline]
    fn apply(&self, ops: Cow<[PipelineOp]>) -> Client {
        todo!()
    }
}

#[derive(Clone)]
enum PipelineKind {
    Resolved(ResolvedPipeline),
    Unresolved(Arc<queue::QueuePipeline>),
}

#[derive(Clone)]
pub struct Pipeline {
    kind: PipelineKind,
}

impl Pipeline {
    #[inline]
    fn apply(&self, ops: Cow<[PipelineOp]>) -> Client {
        match &self.kind {
            PipelineKind::Unresolved(queue) => queue.apply(ops),
            PipelineKind::Resolved(resolved) => resolved.apply(ops),
        }
    }
}

#[derive(Clone)]
pub struct PipelineBuilder {
    pipeline: Pipeline,
    ops: Vec<PipelineOp>,
}

impl recapn::rpc::CapSystem for PipelineBuilder {
    type Cap = Client;
}

impl recapn::rpc::PipelineBuilder<AnyStruct> for PipelineBuilder {
    type Operation = PipelineOp;

    #[inline]
    fn push(mut self, op: Self::Operation) -> Self {
        self.ops.push(op);
        self
    }
}

impl recapn::rpc::Pipelined for PipelineBuilder {
    #[inline]
    fn into_cap(self) -> Self::Cap {
        self.pipeline.apply(Cow::Owned(self.ops))
    }
    #[inline]
    fn to_cap(&self) -> Self::Cap {
        self.pipeline.apply(Cow::Borrowed(&self.ops))
    }
}

pub fn server<T>(dispatcher: T) -> (Client, Dispatcher<T>) {
    todo!()
}