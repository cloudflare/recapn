use crate::sync::request::{PipelineResolver, ResponseReceiverFactory};
use crate::sync::{mpsc, request};
use crate::table::{CapTable, Table};
use crate::{connection, Error, PipelineOp, Result};
use recapn::alloc::Alloc;
use recapn::any::{self};
use recapn::arena::ReadArena;
use recapn::message::{self, Message};
use recapn::{ty, ReaderOf};
use std::future::Future;
use std::marker::PhantomData;
use std::sync::Arc;

mod local;
mod queue;

pub use local::{Dispatch, Dispatcher};

pub type LocalMessage = Box<Message<'static, dyn Alloc + Send>>;
pub type ExternalMessage = Box<dyn ReadArena + Send + Sync>;

/// An RPC message which can refer to either a local message or an external message
pub enum MessagePayload {
    /// A message that was created locally. Local messages can be modified in place if necessary
    /// without copying the entire message content to a new message.
    Local(LocalMessage),
    /// An external message. Since this message hasn't been checked, we will have to check and
    /// copy it if we want to modify it (but this should be rare).
    External(ExternalMessage),
}

impl MessagePayload {
    pub fn segments(&self) -> &dyn ReadArena {
        match self {
            MessagePayload::Local(m) => m.as_read_arena(),
            MessagePayload::External(m) => m,
        }
    }

    pub fn reader(&self, options: message::ReaderOptions) -> message::Reader {
        message::Reader::new(self.segments(), options)
    }
}

/// Indicates what kind of structure exists in the root pointer of the params message.
///
/// For most messages, like local or queued messages, this will be a pointer to the user defined
/// params structure itself. For outgoing messages, this will be a Call structure.
///
/// As the message flows through the RPC system, what exists in the root pointer might change.
/// If a local queue resolves into a remote capability, the parameters at the root will be
/// disowned so a Call structure can be made there instead. The parameters will then be accessable
/// from within the Payload located within the Call.
pub(crate) enum ParamsRoot {
    /// The message root is the request parameters.
    Params,
}

pub struct Params {
    pub(crate) root: ParamsRoot,
    pub(crate) message: MessagePayload,
    /// The capability table
    pub(crate) table: Table,
}

impl Params {
    fn builder(&mut self) -> any::PtrBuilder<'_, CapTable<'_>> {
        todo!()
    }
}

/// Like [`ParamsRoot`], indicates what kind of structure exists in the root pointer of the
/// results message.
///
/// For most messages, like local or queued messages, this will be a pointer to the user defined
/// results structure itself. For incoming messages, this will be a Return structure.
enum ResultsRoot {
    /// The message root is the result parameters.
    Results,
}

pub struct RpcResponse {
    root: ResultsRoot,
    message: MessagePayload,
    /// The capability table
    table: Table,
}

impl PipelineResolver<RpcClient> for RpcResponse {
    fn resolve(
        &self,
        recv: ResponseReceiverFactory<RpcClient>,
        key: Arc<[PipelineOp]>,
        channel: mpsc::Receiver<RpcClient>,
    ) {
        todo!()
    }
    fn pipeline(
        &self,
        recv: ResponseReceiverFactory<RpcClient>,
        key: Arc<[PipelineOp]>,
    ) -> mpsc::Sender<RpcClient> {
        todo!()
    }
}

impl PipelineResolver<RpcClient> for Result<RpcResponse, Error> {
    fn resolve(
        &self,
        recv: ResponseReceiverFactory<RpcClient>,
        key: Arc<[PipelineOp]>,
        channel: mpsc::Receiver<RpcClient>,
    ) {
        match self {
            Ok(r) => r.resolve(recv, key, channel),
            Err(err) => channel.close(err.clone()),
        }
    }
    fn pipeline(
        &self,
        recv: ResponseReceiverFactory<RpcClient>,
        key: Arc<[PipelineOp]>,
    ) -> mpsc::Sender<RpcClient> {
        match self {
            Ok(r) => r.pipeline(recv, key),
            Err(err) => mpsc::broken(RpcClient::Broken, err.clone()),
        }
    }
}

impl PipelineResolver<RpcClient> for RpcResults {
    fn resolve(
        &self,
        recv: ResponseReceiverFactory<RpcClient>,
        key: Arc<[PipelineOp]>,
        channel: mpsc::Receiver<RpcClient>,
    ) {
        match self {
            Owned(r) => r.resolve(recv, key, channel),
            OtherResponse(r) => r.resolve(recv, key, channel),
            Shared(r) => r.resolve(recv, key, channel),
        }
    }
    fn pipeline(
        &self,
        recv: ResponseReceiverFactory<RpcClient>,
        key: Arc<[PipelineOp]>,
    ) -> mpsc::Sender<RpcClient> {
        match self {
            Owned(r) => r.pipeline(recv, key),
            OtherResponse(r) => r.pipeline(recv, key),
            Shared(r) => r.pipeline(recv, key),
        }
    }
}

pub(crate) enum SetPipeline {
    Response(RpcResponse),
    RemotePipeline(connection::QuestionPipeline),
}

impl PipelineResolver<RpcClient> for SetPipeline {
    fn resolve(
        &self,
        recv: ResponseReceiverFactory<RpcClient>,
        key: Arc<[PipelineOp]>,
        channel: mpsc::Receiver<RpcClient>,
    ) {
        match self {
            Self::Response(r) => r.resolve(recv, key, channel),
            Self::RemotePipeline(r) => r.resolve(recv, key, channel),
        }
    }
    fn pipeline(
        &self,
        recv: ResponseReceiverFactory<RpcClient>,
        key: Arc<[PipelineOp]>,
    ) -> mpsc::Sender<RpcClient> {
        match self {
            Self::Response(r) => r.pipeline(recv, key),
            Self::RemotePipeline(r) => r.pipeline(recv, key),
        }
    }
}

impl request::IntoResults<RpcClient> for Error {
    fn into_results(self) -> <RpcClient as request::Chan>::Results {
        Owned(Err(self))
    }
}

/// Tracks where the response to this call is expected to go.
///
/// This is used to detect when a question is reflected back to the original
/// caller. When a reflected call is detected the RPC system sets up the call
/// back so that the caller doesn't send the response through us and instead
/// handles it local.
pub(crate) enum ResponseTarget {
    Local,
    Remote(connection::QuestionTarget),
}

pub(crate) struct RpcCall {
    /// The interface ID for the call
    pub interface: u64,
    /// The method ID for the call
    pub method: u16,

    /// The parameters of the call contained in a message.
    pub params: Params,

    pub target: ResponseTarget,
}

pub(crate) enum RpcClient {
    /// A client that sends requests to a local request handler.
    Local,
    /// A client that always returns the same error when requests are made.
    Broken,
    /// A client that sends requests to a local request handler, but will resolve
    /// in the future to another client. This causes the client to be advertised
    /// as a promise in the RPC protocol.
    LocalShortening,
    /// A client from spawning a task to fulfill it later.
    Spawned,
    /// A client to handle bootstrap requests.
    Bootstrap(connection::QuestionId),
    /// A client to handle pipelined requests to a remote party.
    RemotePipeline,
    /// A client that was imported from another vat. When the client is dropped,
    /// the import is released.
    Import(connection::ImportClient),
}

pub(crate) enum RpcResults {
    /// A response owned by this request.
    Owned(Result<RpcResponse, Error>),
    /// A response shared from another request.
    ///
    /// This is useful if a call is sent to ourselves and another question needs to pull the
    /// response from it.
    OtherResponse(request::Response<RpcClient>),
    /// A response put in an Arc. Connections use this to hold a copy of the response for future
    /// possible pipeline calls.
    Shared(Arc<Result<RpcResponse, Error>>),
}

use RpcResults::*;

impl request::Chan for RpcClient {
    type Parameters = RpcCall;

    type PipelineKey = Arc<[PipelineOp]>;

    type Error = Error;

    type Pipeline = SetPipeline;
    type Results = RpcResults;
}

/// A typeless capability client.
#[derive(Clone)]
pub struct Client {
    pub(crate) sender: mpsc::Sender<RpcClient>,
}

impl Client {
    pub(crate) fn new(client: RpcClient) -> (Client, mpsc::Receiver<RpcClient>) {
        let (sender, receiver) = mpsc::channel(client);
        (Self { sender }, receiver)
    }

    /// Create a new client which is "broken". This client when called will always
    /// return the error passed in here.
    pub fn broken(err: Error) -> Client {
        Client {
            sender: mpsc::broken(RpcClient::Broken, err),
        }
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
        Client {
            sender: queue::spawn(future),
        }
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
    pub fn spawn_local<F>(future: F) -> Client
    where
        F: Future<Output = Client> + 'static,
    {
        Client {
            sender: queue::spawn_local(future),
        }
    }

    pub fn call(&self, interface_id: u64, method_id: u16) -> Result<Request> {
        todo!()
    }
}

pub struct Request {
    client: Client,
    call: RpcCall,
}

impl Request {
    fn new(client: Client, interface: u64, method: u16) -> Self {
        Request {
            client,
            call: RpcCall {
                interface,
                method,
                params: Params {
                    root: ParamsRoot::Params,
                    message: MessagePayload::Local(Box::new(Message::global())),
                    table: Table::new(Vec::new()),
                },
                target: ResponseTarget::Local,
            },
        }
    }

    pub fn params(&mut self) -> any::PtrBuilder<CapTable> {
        self.call.params.builder()
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

impl<T: ty::StructView> Results<'_, T> {
    /// Gets the root structure of the results.
    pub fn get(&self) -> ReaderOf<T, CapTable> {
        todo!()
    }
}

pub fn server<T>(dispatcher: T) -> (Client, Dispatcher<T>) {
    todo!()
}
