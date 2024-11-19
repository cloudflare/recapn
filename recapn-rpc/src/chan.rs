//! Implementations for RPC channels.
//!
//! The sync types provided by this library are generic for testing and extensability purposes.
//! This module defines the actual types

use std::sync::Arc;

use recapn::alloc::Alloc;
use recapn::any;
use recapn::arena::ReadArena;
use recapn::message::{self, Message, ReaderOptions};
use recapn::rpc::Capable;
use recapn_channel::{Chan, IntoResults, PipelineResolver};
use recapn_channel::mpsc;
use recapn_channel::request::{self, ResponseReceiverFactory};

use crate::client::Client;
use crate::connection;
use crate::pipeline::PipelineOp;
use crate::table::{CapTable, Table};
use crate::Error;

#[derive(Debug)]
pub(crate) enum RpcChannel {
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
    /// A local request pipeline
    Pipeline,
    /// A client to handle pipelined requests to a remote party.
    RemotePipeline,
    /// A client that was imported from another vat. When the client is dropped,
    /// the import is released.
    Import(connection::ImportClient),
}

pub type Sender = mpsc::Sender<RpcChannel>;
pub type Receiver = mpsc::Receiver<RpcChannel>;

pub type Request = request::Request<RpcChannel>;
pub type ResponseReceiver = request::ResponseReceiver<RpcChannel>;
pub type Recv = request::Recv<RpcChannel>;
pub type Responder = request::Responder<RpcChannel>;
pub type Response = request::Response<RpcChannel>;
pub type PipelineBuilder = request::PipelineBuilder<RpcChannel>;

impl Chan for RpcChannel {
    type Parameters = RpcCall;

    type PipelineKey = Arc<[PipelineOp]>;

    type Error = Error;

    type Pipeline = SetPipeline;
    type Results = RpcResults;
}

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

    pub fn reader(&self, options: message::ReaderOptions) -> message::Reader<'_> {
        message::Reader::new(self.segments(), options)
    }
}

/// Like [`ParamsRoot`], indicates what kind of structure exists in the root pointer of the
/// results message.
///
/// For most messages, like local or queued messages, this will be a pointer to the user defined
/// results structure itself. For incoming messages, this will be a Return structure.
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum ResultsRoot {
    /// The message root is the result parameters.
    Results,
}

pub struct RpcResponse {
    pub root: ResultsRoot,
    pub message: MessagePayload,
    /// The capability table
    pub table: Table,
}

impl RpcResponse {
    pub fn with_results<T>(&self, f: impl FnOnce(any::PtrReader<'_, CapTable<'_>>) -> T) -> T {
        let reader = self.message.reader(ReaderOptions::default());
        let results = match self.root {
            ResultsRoot::Results => reader.root().imbue(self.table.reader()),
        };
        f(results)
    }

    fn try_resolve_pipeline_ops(&self, ops: &[PipelineOp]) -> recapn::Result<Client> {
        self.with_results(|mut ptr| {
            for op in ops {
                match op {
                    PipelineOp::PtrField(index) => {
                        ptr = ptr
                            .try_read_as::<any::AnyStruct>()?
                            .ptr_field_or_default(*index);
                    }
                }
            }
            Ok(ptr.read_as_client::<Client>())
        })
    }
}

impl PipelineResolver<RpcChannel> for RpcResponse {
    fn resolve(
        &self,
        _: ResponseReceiverFactory<'_, RpcChannel>,
        key: Arc<[PipelineOp]>,
        channel: mpsc::Receiver<RpcChannel>,
    ) {
        match self.try_resolve_pipeline_ops(&key) {
            Ok(c) => {
                if let Err(recv) = channel.forward_to(&c.0) {
                    recv.close(Error::failed("attempted to resolve client into itself"))
                }
            }
            Err(err) => channel.close(Error::failed(err.to_string())),
        }
    }
    fn pipeline(
        &self,
        _: ResponseReceiverFactory<'_, RpcChannel>,
        key: Arc<[PipelineOp]>,
    ) -> mpsc::Sender<RpcChannel> {
        match self.try_resolve_pipeline_ops(&key) {
            Ok(c) => c.0,
            Err(err) => mpsc::broken(RpcChannel::Broken, Error::failed(err.to_string())),
        }
    }
}

impl PipelineResolver<RpcChannel> for Result<RpcResponse, Error> {
    fn resolve(
        &self,
        recv: ResponseReceiverFactory<'_, RpcChannel>,
        key: Arc<[PipelineOp]>,
        channel: mpsc::Receiver<RpcChannel>,
    ) {
        match self {
            Ok(r) => r.resolve(recv, key, channel),
            Err(err) => channel.close(err.clone()),
        }
    }
    fn pipeline(
        &self,
        recv: ResponseReceiverFactory<'_, RpcChannel>,
        key: Arc<[PipelineOp]>,
    ) -> mpsc::Sender<RpcChannel> {
        match self {
            Ok(r) => r.pipeline(recv, key),
            Err(err) => mpsc::broken(RpcChannel::Broken, err.clone()),
        }
    }
}

impl PipelineResolver<RpcChannel> for RpcResults {
    fn resolve(
        &self,
        recv: ResponseReceiverFactory<'_, RpcChannel>,
        key: Arc<[PipelineOp]>,
        channel: mpsc::Receiver<RpcChannel>,
    ) {
        match self {
            RpcResults::Owned(r) => r.resolve(recv, key, channel),
            RpcResults::OtherResponse(r) => r.resolve(recv, key, channel),
        }
    }
    fn pipeline(
        &self,
        recv: ResponseReceiverFactory<'_, RpcChannel>,
        key: Arc<[PipelineOp]>,
    ) -> mpsc::Sender<RpcChannel> {
        match self {
            RpcResults::Owned(r) => r.pipeline(recv, key),
            RpcResults::OtherResponse(r) => r.pipeline(recv, key),
        }
    }
}

pub enum RpcResults {
    /// A response owned by this request.
    Owned(Result<RpcResponse, Error>),
    /// A response shared from another request.
    ///
    /// This is useful if a call is sent to ourselves and another question needs to pull the
    /// response from it.
    OtherResponse(request::Response<RpcChannel>),
}

impl RpcResults {
    /// Find the inner most result value
    pub fn unwrap_results(mut self: &Self) -> &Result<RpcResponse, Error> {
        loop {
            match self {
                RpcResults::Owned(owned) => break owned,
                RpcResults::OtherResponse(resp) => self = resp.as_ref(),
            }
        }
    }
}

impl From<Result<RpcResponse, Error>> for RpcResults {
    fn from(value: Result<RpcResponse, Error>) -> Self {
        RpcResults::Owned(value)
    }
}

pub enum SetPipeline {
    Response(RpcResults),
    RemotePipeline(connection::QuestionPipeline),
}

impl PipelineResolver<RpcChannel> for SetPipeline {
    fn resolve(
        &self,
        recv: ResponseReceiverFactory<'_, RpcChannel>,
        key: Arc<[PipelineOp]>,
        channel: Receiver,
    ) {
        match self {
            Self::Response(r) => r.resolve(recv, key, channel),
            Self::RemotePipeline(r) => r.resolve(recv, key, channel),
        }
    }
    fn pipeline(
        &self,
        recv: ResponseReceiverFactory<'_, RpcChannel>,
        key: Arc<[PipelineOp]>,
    ) -> Sender {
        match self {
            Self::Response(r) => r.pipeline(recv, key),
            Self::RemotePipeline(r) => r.pipeline(recv, key),
        }
    }
}

impl IntoResults<RpcChannel> for Error {
    fn into_results(self) -> <RpcChannel as Chan>::Results {
        RpcResults::Owned(Err(self))
    }
}

/// Tracks where the response to this call is expected to go.
///
/// This is used to detect when a question is reflected back to the original
/// caller. When a reflected call is detected the RPC system sets up the call
/// back so that the caller doesn't send the response through us and instead
/// handles it local.
pub enum ResponseTarget {
    Local,
    Remote(connection::QuestionTarget),
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
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum ParamsRoot {
    /// The message root is the request parameters.
    Params,
}

pub struct Params {
    pub root: ParamsRoot,
    pub message: MessagePayload,
    /// The capability table
    pub table: Table,
}

pub struct RpcCall {
    /// The interface ID for the call
    pub interface: u64,
    /// The method ID for the call
    pub method: u16,

    /// The parameters of the call contained in a message.
    pub params: Params,

    pub target: ResponseTarget,
}
