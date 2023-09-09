//! Support for generic multi-party systems.

use std::any::{Any, TypeId};
use std::cell::{RefCell, Cell};
use std::collections::HashMap;
use std::future::{Future, IntoFuture};
use std::marker::PhantomData;
use std::mem::replace;
use std::pin::Pin;
use std::task::{self, Waker, Context, Poll};
use std::rc::{Rc, Weak};

use recapn::alloc::{Growing, Global};
use recapn::any::{self, AnyPtr, AnyStruct};
use recapn::message::{self, Message};
use recapn::rpc::{Empty, CapSystem, Pipelined, PipelineBuilder};

use crate::{ClientOptions, PipelineOp, Result, Error};
use crate::node_set::{NodeSet, NodeIdx};

#[cold]
fn rpc_destroyed() -> Error {
    Error::disconnected("the RPC system was destroyed")
}

/// A server bound to a local RPC system
struct LocalServer<Srv: ?Sized> {
    /// The state of the server. When a server does a streaming call, it moves into the streaming
    /// state, and can become broken if any streaming calls return an error.
    state: RefCell<ServerState>,
    /// A reference to the system this server is attached to. It's weak since the RPC system
    /// holds a strong reference to it when making requests. If the RPC system is destroyed
    /// while local servers are still active, all future calls immediately fail with a disconnected
    /// error.
    system: Weak<RpcCell>,
    options: ClientOptions,
    /// The actual server, stored as the last field for automatic unsizing.
    server: Srv,
}

impl LocalServer<dyn Server> {
    /// Downcast a dynamic server into the concrete type.
    fn downcast_ref<T: Server>(&self) -> Option<&LocalServer<T>> {
        if TypeId::of::<T>() == self.server.type_id() {
            Some(unsafe { &*(self as *const LocalServer<dyn Server> as *const LocalServer<T>) })
        } else {
            None
        }
    }
}

#[derive(Clone)]
enum ClientKind {
    Local(Rc<LocalServer<dyn Server>>),
    /// A pipelined request client
    Pipeline(Rc<PipelineClient>),
    /// An error client that always just returns the given error
    Broken(Rc<Error>),
}

impl ClientKind {
    /// Resolve the client into a request client, unwrapping any known errors.
    pub fn to_request_client(&self) -> Result<RequestClient> {
        match self {
            ClientKind::Broken(err) => Err((**err).clone()),
            ClientKind::Local(local) => Ok(RequestClient::Local(local.clone())),
            ClientKind::Pipeline(pipeline) =>
                match &*pipeline.state.borrow() {
                    PipelineState::Resolved(ResolvedClient::Broken(err)) => Err((**err).clone()),
                    PipelineState::Resolved(ResolvedClient::Local(local)) => Ok(RequestClient::Local(local.clone())),
                    PipelineState::Blocked { .. } => Ok(RequestClient::Pipeline(pipeline.clone())),
                }
        }
    }
}

enum ServerState {
    /// The server is available to be called
    Available,
    /// The server is performing a streaming call, so future calls are blocked until the
    /// call is complete.
    Streaming {
    },
    /// The server returned an error while streaming, so we return this error for all future calls
    Broken(Error),
}

enum RequestClient {
    Local(Rc<LocalServer<dyn Server>>),
    /// A pipelined request client
    Pipeline(Rc<PipelineClient>),
}

impl RequestClient {
    /// Attempt to resolve the client. This automatically unwraps any broken clients so that we
    /// don't submit a request to the RPC system when the client has already broken.
    pub fn try_resolve(self) -> Result<Self> {
        todo!()
    }

    /// Unwraps and upgrades a handle to the RPC system
    pub fn rpc_handle(&self) -> Result<Rc<RpcCell>> {
        todo!()
    }
}

pub struct LocalRequest {
    interface_id: u64,
    method_id: u16,
    params: Message<'static, Growing<Global>>,
    /// The client this request corresponds to. This only contains directly unbreakable clients.
    client: RequestClient,
}

impl LocalRequest {
    /// Submit the request to the RPC system.
    fn submit(mut self) -> Result<(RequestIdx, Rc<RpcCell>)> {
        // Attempt to further resolve the client and extract the RPC handle. If either of these fail, we
        // simply return the given error. This saves some time and memory.
        let (client, handle) = self.client.try_resolve()
            .and_then(|client|
                client.rpc_handle().map(|handle| (client, handle)))?;

        self.client = client;

        let idx = handle.borrow_mut().requests.send(self);
        Ok((idx, handle))
    }

    /// Send the request, returning a future for the response.
    pub async fn send(self) -> Result<LocalResponse> {
        self.send_future().await
    }

    fn send_future(self) -> LocalRequestFuture {
        match self.submit() {
            Ok((idx, handle)) =>
                LocalRequestFuture::waiting(idx, Rc::downgrade(&handle)),
            Err(err) => LocalRequestFuture::error(err),
        }
    }

    /// Send the request, returning a value for pipelining.
    pub fn pipeline(self) -> Result<any::PtrPipeline<LocalRequestPipeline>> {
        let (idx, handle) = self.submit()?;
        let pipeline = any::PtrPipeline::new(
            LocalRequestPipeline::new(idx, Rc::downgrade(&handle))
        );
        Ok(pipeline)
    }

    /// Send the request, returning a future to await the result and a pipeline for request
    /// pipelining.
    pub fn send_and_pipeline(self) -> Result<(LocalRequestFuture, any::PtrPipeline<LocalRequestPipeline>)> {
        let (idx, handle) = self.submit()?;
        let weak = Rc::downgrade(&handle);

        let future = LocalRequestFuture::waiting(idx, weak.clone());
        let pipeline = any::PtrPipeline::new(
            LocalRequestPipeline::new(idx, weak.clone())
        );
        Ok((future, pipeline))
    }
}

impl IntoFuture for LocalRequest {
    type IntoFuture = LocalRequestFuture;
    type Output = Result<LocalResponse>;

    fn into_future(self) -> Self::IntoFuture {
        self.send_future()
    }
}

pub struct LocalRequestFuture {
    state: LocalRequestFutureState,
}

impl LocalRequestFuture {
    fn waiting(idx: RequestIdx, handle: Weak<RpcCell>) -> Self {
        LocalRequestFuture {
            state: LocalRequestFutureState::Waiting {
                rpc: handle,
                request: idx,
            },
        }
    }

    fn error(err: Error) -> Self {
        LocalRequestFuture { state: LocalRequestFutureState::Ready(Err(err)) }
    }
}

enum LocalRequestFutureState {
    Waiting {
        rpc: Weak<RpcCell>,
        request: RequestIdx,
    },
    Ready(Result<LocalResponse>),
    Finished,
    Poisoned,
}

impl LocalRequestFutureState {
    #[inline]
    fn transition(self, ctx: &mut task::Context<'_>) -> (Self, Poll<Result<LocalResponse>>) {
        use LocalRequestFutureState::*;
        match self {
            Waiting { rpc, request } => {
                let Some(strong) = rpc.upgrade() else {
                    return (Finished, Poll::Ready(Err(rpc_destroyed())))
                };

                let mut state = strong.borrow_mut();
                match state.requests.take_response(request, ctx) {
                    Some(response) => (Finished, Poll::Ready(response)),
                    None => (Waiting { rpc, request }, Poll::Pending),
                }
            }
            Ready(result) => (Finished, Poll::Ready(result)),
            Finished => panic!("local request has been polled to completion"),
            Poisoned => panic!("local request has been poisoned"),
        }
    }
}

impl Future for LocalRequestFuture {
    type Output = Result<LocalResponse>;

    fn poll(self: Pin<&mut Self>, ctx: &mut task::Context<'_>) -> Poll<Self::Output> {
        let state = &mut self.get_mut().state;
        let old_state = replace(state, LocalRequestFutureState::Poisoned);
        let (new_state, result) = old_state.transition(ctx);
        *state = new_state;
        result
    }
}

#[derive(Clone)]
pub struct LocalRequestPipeline {
    rpc: Weak<RpcCell>,
    idx: RequestIdx,
    ops: Vec<PipelineOp>,
}

impl LocalRequestPipeline {
    fn new(idx: RequestIdx, handle: Weak<RpcCell>) -> Self {
        Self { rpc: handle, idx, ops: Vec::new() }
    }
}

impl CapSystem for LocalRequestPipeline {
    type Cap = Client;
}

fn apply_pipeline_ops<'a>(root: any::PtrReader<'a, Empty>, ops: &[PipelineOp]) -> Result<any::PtrReader<'a, Empty>> {
    let mut result = root;
    for op in ops {
        match op {
            PipelineOp::PtrField(idx) => {
                result = result.try_read_as::<AnyStruct>()
                    .map_err(|err| Error::failed(err))?
                    .ptr_field_or_default(*idx);
            }
        }
    }
    Ok(result)
}

impl Pipelined for LocalRequestPipeline {
    fn into_cap(self) -> Self::Cap {
        let Some(rpc) = self.rpc.upgrade() else {
            return Client::broken(rpc_destroyed());
        };

        todo!()
    }
}

impl PipelineBuilder<AnyPtr> for LocalRequestPipeline {
    type Operation = PipelineOp;

    fn push(mut self, op: Self::Operation) -> Self {
        self.ops.push(op);
        self
    }
}

impl PipelineBuilder<AnyStruct> for LocalRequestPipeline {
    type Operation = PipelineOp;

    fn push(mut self, op: Self::Operation) -> Self {
        self.ops.push(op);
        self
    }
}

#[derive(Clone)]
pub struct LocalResponse {
    response: Rc<Message<'static, Growing<Global>>>,
}

pub struct Client {
    kind: ClientKind,
}

impl Client {
    fn broken(err: Error) -> Self {
        Client { kind: ClientKind::Broken(Rc::new(err)) }
    }

    fn new_call(&self, interface_id: u64, method_id: u16) -> Result<LocalRequest> {
        Ok(LocalRequest {
            interface_id,
            method_id,
            params: Message::global(),
            client: self.kind.to_request_client()?,
        })
    }
}

struct LocalState {
    requests: RequestForest,
}

struct RequestRef {
    idx: NodeIdx<Request>,
    rpc: Weak<RpcCell>,
}

type RequestIdx = NodeIdx<Request>;

struct Request {
    state: RequestState,
    /// A waker used to wake the task in the future set.
    task_waker: Option<Waker>,
    /// The waker used to wake the task that's waiting for the response
    /// to this request.
    response_waker: Option<Waker>,
}

struct PipelineClient {
    state: RefCell<PipelineState>,
}

enum ResolvedClient {
    Local(Rc<LocalServer<dyn Server>>),
    Broken(Rc<Error>),
}

enum PipelineState {
    /// The pipeline is blocked, waiting for a response to be received.
    Blocked {
        system: Weak<RpcCell>,
        parent: NodeIdx<Request>,
        first: NodeIdx<Request>,
        last: NodeIdx<Request>,
    },
    /// The pipeline has resolved into the given client
    Resolved(ResolvedClient),
}

type PipelineMap = HashMap<Box<[PipelineOp]>, Rc<PipelineClient>>;

enum RequestState {
    /// The request is blocked.
    Blocked {
        request: LocalRequest,
        /// The next request in the block chain.
        next: RequestIdx,
        pipelines: PipelineMap,
    },
    /// The request is ready to run. Most requests start in this state, or transition
    /// to this state if the request has become immediately unblocked.
    /// 
    /// Note: it could turn out that when the request actually is polled to begin
    /// running that it moves to a blocked state, since another request started
    /// that put the local server into a streaming state.
    Ready {
        request: LocalRequest,
        /// The index of a blocked request.
        /// 
        /// Requests that become unblocked don't immediately unblock the whole
        /// blocked chain. Since often streaming calls are made exclusively on a
        /// server once they start, we hold off on unblocking the entire request chain
        /// until we make sure this request doesn't immediately block again. This
        /// prevents a thundering herd of queued blocked requests becoming ready
        /// and then immediately blocked again.
        blocking: RequestIdx,
        pipelines: PipelineMap,
    },
    /// The request is currently running.
    Running {
        /// The running future. This is optional to allow us to move the future out and
        /// release the ref cell borrow during polling, since subrequests might be made
        /// during the poll which will require a mutable borrow of the system state.
        /// If a poll is ever made and the future doesn't exist, the call will assume
        /// it's poisoned and lost forever.
        future: Option<Pin<Box<dyn Future<Output = LocalResponse>>>>,
        /// The server this request is running on. We hold a reference here so if the
        server: Rc<LocalServer<dyn Server>>,
        pipelines: PipelineMap,
    },
    /// The request is done. When moving this state, the response future will be waked
    /// if it has polled already.
    Done {
        response: Result<LocalResponse>,
    },
    /// The request is done and the response has been taken by its future. The task should
    /// be waked to delete the task node and free it again.
    Finished,
    /// An intermediate state when moving between states. If a state transition doesn't complete,
    /// this gravestone marks that a bug exists in the runtime. It should not be encountered in
    /// normal use.
    Poisoned,
}

/// A channel for sending messages out to another vat. Messages are created with
/// [`new_message`] and sent with [`OutgoingMessage::send`].
pub trait OutgoingChannel: 'static {
    /// The message type used for sending out messages.
    type Message: OutgoingMessage;

    /// Create a new message to be sent out on this channel.
    fn new_message(&self) -> Self::Message;

    /// Checks if the channel is already closed, possibly due to a sending error.
    /// 
    /// If this is true, the RPC system may call [`close`] with no error to gracefully
    /// cleanup the connection.
    fn is_closed(&self) -> bool;

    /// Gracefully close the channel, possibly with an error.
    fn close(self, err: Option<Error>);
}

/// A message to be sent on an RPC connection.
pub trait OutgoingMessage: 'static {
    fn root(&mut self) -> any::PtrBuilder<Empty>;

    /// Send the message.
    /// 
    /// Note: If the channel has been closed, the implementation may consume the message
    /// and not send it.
    fn send(self);
}

/// A forest of request trees.
/// 
/// In Cap'n Proto RPC, a request's state can be represented as a tree, where the root
/// of a tree is an active request, and the child nodes are blocked and pipelined requests.
/// 
/// When a new request is made, it's added to the node set. If the local server is
/// serving a streaming request, the request is blocked, and added to the end of the
/// list of requests blocked on streaming.
/// 
/// If the request is pipelined it's added to the list of pipelined requests for the given
/// pipelined capability.
struct RequestForest {
    requests: NodeSet<Request>,
    first_request: RequestIdx,
}

impl RequestForest {
    pub fn new() -> Self {
        Self {
            requests: NodeSet::new(),
            first_request: NodeIdx::END,
        }
    }

    pub fn send(&mut self, req: LocalRequest) -> RequestIdx {
        todo!()
    }

    // Take the result of a request that is done.
    pub fn take_response(&mut self, idx: RequestIdx, ctx: &mut task::Context<'_>) -> Option<Result<LocalResponse>> {
        let mut request = self.requests.get_mut(idx);

        if matches!(request.state, RequestState::Done { .. }) {
            let RequestState::Done { response } =
                replace(&mut request.state, RequestState::Finished) else { unreachable!() };

            // Wake up the task to cleanup the request later.
            if let Some(waker) = request.task_waker.take() {
                waker.wake();
            }

            Some(response)
        } else {
            // Set the response waker here since we call this from the ResponseFuture type. When the
            // request moves into the done state, this will be woken up.
            request.response_waker = Some(ctx.waker().clone());
            None
        }
    }
}

struct RpcState {
    requests: RequestForest,
    /// A weak pointer identifying this system
    this: Weak<RpcCell>,
}

impl RpcState {
    pub fn new() -> Rc<RpcCell> {
        Rc::new_cyclic(|this| {
            RefCell::new(RpcState {
                requests: RequestForest::new(),
                this: this.clone(),
            })
        })
    }
}

type RpcCell = RefCell<RpcState>;

enum ProvisionState {
    Start,
}

pub struct Provision {
    state: Rc<Cell<ProvisionState>>,
}

impl Provision {
    /// Creates a new Provision to be used by an RpcSystem.
    pub fn new() -> Self {
        todo!()
    }
}

/// Types used to implement network-specific parameters and methods.
pub trait VatNetwork: Sized {
    /// Connection specific data for sending data out to another vat.
    type Connection: OutgoingChannel;

    /// Information that must be sent in an `Accept` message to identify objects being accepted.
    type ProvisionId;
    /// Information that must be sent in a `Provide` message to identity the recipient of the
    /// capability.
    type RecipientId;
    /// Information needed to connect to a third party and accept a capability from it.
    type ThirdPartyCapId;

    /// Create a recipient ID and third-party cap ID suitable for performing a third-party
    /// handoff from one connection to another. This recipient ID is sent to the `from`
    /// connection in the form of a `Provide` message and the third-party cap ID is sent
    /// to the `to` connection in the form of a CapDescriptor.thirdPartyHosted variant.
    fn new_provision(&mut self, from: &VatConnection<Self>, to: &VatConnection<Self>)
        -> Result<(Self::RecipientId, Self::ThirdPartyCapId)>;

    /// Resolve a third-party cap ID into the connection for the vat and a provision ID that
    /// can be used to `Accept` the cap.
    fn resolve_third_party_cap(&mut self, cap_id: &Self::ThirdPartyCapId)
        -> Result<(VatConnection<Self>, Self::ProvisionId)>;

    /// Get a provision based on a recipient ID. This is commonly executed first as part of receiving
    /// a `Provide` message.
    /// 
    /// If this is received after an `Accept` message, the network is free to clean up any state
    /// related to the provision.
    /// 
    /// Note: Vat networks should account for the fact that an `Accept` can be received _before_ a
    /// `Provide` in the case of network congestion or other delivery delays.
    fn receive_provision(&mut self, id: &Self::RecipientId) -> Result<Provision>;

    /// Get a provision based on a provision ID. This is commonly executed second as part of receiving
    /// an `Accept` message.
    /// 
    /// If this is received after a `Provide` message, the network is free to clean up any state
    /// related to the provision.
    /// 
    /// Note: Vat networks should account for the fact that an `Accept` can be received _before_ a
    /// `Provide` in the case of network congestion or other delivery delays.
    fn resolve_provision(&mut self, id: &Self::ProvisionId) -> Result<Provision>;

    /// Cancel the provision, cleaning up any network state. If no state exists for the provision,
    /// this does nothing.
    /// 
    /// This exists for the case where a party drops out of a handoff (because of disconnection) and
    /// can't send their part of the handoff.
    fn cancel_provision(&mut self, provision: &Provision);
}

/// An incoming message sent to the local vat.
/// 
/// Note while this trait does not require the message to be clonable,
/// most interfaces that use it do.
pub trait IncomingMessage {
    fn root(&self) -> any::PtrReader<Empty>;
}

/// The base primitive for local RPC. This can be used to create a local RPC system
/// without any external connections.
pub struct RpcSystem {
    state: Rc<RpcCell>,
}

impl RpcSystem {
    pub fn new_client(&self) -> Client {
        todo!()
    }

    /// Polls the system, driving all the requests within it.
    pub fn poll(&mut self, ctx: &mut Context) -> Poll<()> {
        todo!()
    }
}

/// A base primitive for multi-vat RPC. This can be used to create an RPC system
/// between any number of vats.
/// 
/// The Network type provides network specific parameters for messaging and transport.
pub struct VatSystem<N: VatNetwork> {
    state: Rc<RpcCell>,
    network: N,
}

impl<N: VatNetwork> VatSystem<N> {
    pub fn new(network: N) -> Self {
        VatSystem {
            state: RpcState::new(),
            network,
        }
    }

    /// Creates a new connection bound to this RPC system.
    pub fn new_connection(&self, conn: N::Connection) -> VatConnection<N> {
        VatConnection {
            system: Rc::downgrade(&self.state),
            state: Rc::new(ConnectionCell {
                state: RefCell::new(ConnectionState::new()),
                conn,
            }),
        }
    }

    /// Process an incoming message on the given connection.
    pub fn recv<M>(&mut self, conn: &VatConnection<N>, msg: M) -> Result<()>
    where
        M: IncomingMessage + ToOwned,
        M::Owned: IncomingMessage + 'static,
    {
        todo!()
    }
}

pub struct VatConnection<N: VatNetwork> {
    system: Weak<RpcCell>,
    state: Rc<ConnectionCell<N::Connection>>,
}

impl<N: VatNetwork> VatConnection<N> {
    pub fn conn(&self) -> &N::Connection {
        &self.state.conn
    }
}

struct ConnectionState {
    answers: HashMap<u32, AnswerState>,
}

impl ConnectionState {
    fn new() -> Self {
        Self { answers: HashMap::new() }
    }
}

struct ConnectionCell<T: ?Sized + 'static> {
    state: RefCell<ConnectionState>,
    conn: T,
}

struct Question {

}

/// A request sent to a remote vat waiting for an answer.
enum QuestionState {

}

struct QuestionSet {
    nodes: NodeSet<Question>,
}

type QuestionId = NodeIdx<Question>;

/// A request running locally to answer a question made by a remote vat.
enum AnswerState {

}
