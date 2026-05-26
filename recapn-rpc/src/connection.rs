//! An RPC connection primitive.

use fnv::FnvHashMap;
use recapn::alloc::AllocLen;
use recapn::any::{self, AnyPtr};
use recapn::arena::ReadArena;
use recapn::message::{BuilderParts, ReaderOptions};
use recapn::orphan::Orphan;
use recapn::ptr::ReturnErrors;
use recapn::rpc::Capable;
use recapn::{list, message, BuilderOf, NotInSchema, ReaderOf};
use recapn_channel::request::{RequestUsage, ResponseReceiverFactory};
use recapn_channel::task::{DataRef, DataTask, DataTaskSet};
use recapn_channel::{mpsc, request, PipelineResolver};
use tokio::select;
use tokio::sync::mpsc as tokio_mpsc;

use crate::chan::{
    self, ExternalMessage, LocalMessage, MessagePayload, Params, ParamsRoot, ResponseTarget, RpcChannel, RpcResponse, RpcResults, SetPipeline
};
use crate::client::{dropped_cap, dropped_request, Client};
use crate::generated::capnp_rpc_capnp as rpc_capnp;
use rpc_capnp::r#return::Which as WhichReturn;
use crate::pipeline::PipelineOp;
use crate::table::{self, CapTable, Table};
use crate::{Result, Error, ErrorKind};
use std::borrow::Cow;
use std::cmp::Reverse;
use std::collections::{HashMap, hash_map};
use std::collections::btree_map;
use std::collections::{BTreeMap, BinaryHeap};
use std::convert::TryFrom;
use std::fmt::{Debug, Write};
use std::future::Future;
use std::mem::replace;
use std::pin::{Pin, pin};
use std::sync::Arc;

#[inline]
fn broken(err: Error) -> chan::Sender {
    mpsc::broken(RpcChannel::Broken, err)
}

#[inline]
pub(crate) fn null_cap_error() -> Error {
    Error::failed("called null capability")
}

#[inline]
fn null_cap() -> chan::Sender {
    mpsc::broken(RpcChannel::Broken, null_cap_error())
}

#[inline]
fn self_resolution_error() -> Error {
    Error::failed("pipeline resolved into itself")
}

#[inline]
fn no_pipeline_error() -> Error {
    Error::failed("attempted to pipeline on request without pipelining")
}

#[inline]
fn finished_answer_error() -> Error {
    Error::failed("attempted to pipeline on finished answer")
}

fn exception_to_error(ex: &ReaderOf<'_, rpc_capnp::Exception>) -> Error {
    let mut msg = String::new();
    let kind = match ex.r#type() {
        Ok(t) => t,
        Err(NotInSchema(num)) => {
            let _ = write!(msg, "(unknown exception type '{num}') ");
            ErrorKind::Failed
        }
    };

    match ex.reason().try_get_option() {
        // TODO(perf): Use UTF-8 chunks API to write to the msg without allocating
        Ok(Some(text)) => match String::from_utf8_lossy(text.as_bytes()) {
            Cow::Borrowed(s) => msg.push_str(s),
            Cow::Owned(s) => {
                msg.push_str("(malformed exception reason; utf-8 decoded lossy) ");
                msg.push_str(&s);
            }
        },
        Ok(None) => msg.push_str("(empty exception reason)"),
        Err(_) => msg.push_str("(malformed exception reason)"),
    }

    Error::new(kind, Cow::Owned(msg))
}

fn error_to_exception(err: &Error, ex: &mut BuilderOf<'_, rpc_capnp::Exception>) {
    ex.r#type().set(err.kind());

    // Truncate the string to the lower char boundary. This will probably be rare but we should
    // be safe than sorry.
    let len_to_use = err.description.floor_char_boundary(recapn::text::MAX_LENGTH as usize);
    let description = &err.description[..len_to_use];
    ex.reason().set_str(description);
}

type OpsList<'a> = list::StructListReader<'a, rpc_capnp::promised_answer::Op>;
fn to_pipeline_ops(list: OpsList<'_>) -> Result<Vec<PipelineOp>, Error> {
    list.into_iter()
        .filter_map(|op| {
            use rpc_capnp::promised_answer::op::Which;
            match op.which() {
                Ok(Which::Noop(())) => None, // does nothing
                Ok(Which::GetPointerField(field)) => Some(Ok(PipelineOp::PtrField(field))),
                Err(NotInSchema(variant)) => Some(Err(Error::failed(format!(
                    "Unsupported pipeline op: {variant}"
                )))),
            }
        })
        .collect()
}

fn write_promised_answer(
    id: QuestionId,
    ops: &[PipelineOp],
    proto: &mut BuilderOf<'_, rpc_capnp::PromisedAnswer>,
) -> Result<(), Error> {
    let Ok(count) = list::ElementCount::try_from(ops.len()) else {
        return Err(Error::failed("too many pipeline ops"))
    };

    proto.question_id().set(id);
    let mut list = proto.transform().init(count.get());
    for (op, idx) in ops.iter().zip(0..) {
        let mut b = list.at(idx).get();
        match op {
            PipelineOp::PtrField(ptr) => {
                b.get_pointer_field().set(*ptr);
            }
        }
    }

    Ok(())
}

pub(crate) type QuestionId = u32;
pub(crate) type AnswerId = QuestionId;
pub(crate) type ExportId = u32;
pub(crate) type ImportId = ExportId;
pub(crate) type EmbargoId = u32;

struct ExportTable<T> {
    slots: Vec<Option<T>>,
    free_slots: BinaryHeap<Reverse<u32>>,
}

impl<T> ExportTable<T> {
    pub fn new() -> Self {
        Self {
            slots: Vec::new(),
            free_slots: BinaryHeap::new(),
        }
    }

    pub fn is_empty(&self) -> bool {
        self.slots.len() == self.free_slots.len()
    }

    pub fn get_mut(&mut self, id: u32) -> Option<&mut T> {
        self.slots.get_mut(id as usize).and_then(Option::as_mut)
    }

    pub fn push(&mut self, value: T) -> (u32, &mut T) {
        let slot = match self.free_slots.pop() {
            Some(Reverse(free_slot)) => free_slot,
            None => {
                let len = self.slots.len();
                let slot = u32::try_from(len).expect("too many exports!");
                self.slots.push(None);
                slot
            }
        };
        let s = &mut self.slots[slot as usize];
        debug_assert!(s.is_none());
        let v = s.insert(value);
        (slot, v)
    }

    pub fn push_with<U>(&mut self, f: impl FnOnce(u32) -> (T, U)) -> (u32, U) {
        let slot = match self.free_slots.pop() {
            Some(Reverse(free_slot)) => free_slot,
            None => {
                let len = self.slots.len();
                let slot = u32::try_from(len).expect("too many exports!");
                self.slots.push(None);
                slot
            }
        };
        let s = &mut self.slots[slot as usize];
        debug_assert!(s.is_none());
        let (value, u) = f(slot);
        *s = Some(value);
        (slot, u)
    }

    pub fn remove(&mut self, id: u32) -> Option<T> {
        let result = self.slots.get_mut(id as usize).and_then(Option::take);
        if result.is_some() {
            self.free_slots.push(Reverse(id));
        }
        result
    }
}

const IMPORT_LOW_SLOTS: u32 = 16;

struct ImportTable<T> {
    low: [Option<T>; IMPORT_LOW_SLOTS as usize],
    high: BTreeMap<u32, T>,
}

impl<T> ImportTable<T> {
    #[inline]
    pub fn new() -> Self {
        Self {
            low: Default::default(),
            high: BTreeMap::new(),
        }
    }

    pub fn is_empty(&self) -> bool {
        self.low.iter().all(Option::is_none) && self.high.is_empty()
    }

    pub fn contains(&self, id: u32) -> bool {
        self.get(id).is_some()
    }

    #[inline]
    pub fn get(&self, id: u32) -> Option<&T> {
        if id < IMPORT_LOW_SLOTS {
            self.low[id as usize].as_ref()
        } else {
            self.high.get(&id)
        }
    }

    #[inline]
    pub fn get_mut(&mut self, id: u32) -> Option<&mut T> {
        if id < IMPORT_LOW_SLOTS {
            self.low[id as usize].as_mut()
        } else {
            self.high.get_mut(&id)
        }
    }

    #[inline]
    pub fn insert(&mut self, id: u32, value: T) -> &mut T {
        if id < IMPORT_LOW_SLOTS {
            let slot = &mut self.low[id as usize];
            assert!(slot.is_none());
            slot.get_or_insert(value)
        } else {
            match self.high.entry(id) {
                btree_map::Entry::Occupied(_) => panic!("value already exists"),
                btree_map::Entry::Vacant(v) => {
                    v.insert(value)
                }
            }
        }
    }

    pub fn remove(&mut self, id: u32) -> Option<T> {
        if id < IMPORT_LOW_SLOTS {
            self.low[id as usize].take()
        } else {
            self.high.remove(&id)
        }
    }
}

/// A source of messages for the RPC system.
///
/// Since messages can be created anywhere, we don't assume outbound messages have
/// a defined allocator or take any specific form. Instead, a message outbound consumes
/// a boxed message with any allocator.
pub trait MessageFactory {
    /// Creates a new message.
    fn new_message(&mut self) -> LocalMessage;

    /// Creates a new message, with the given estimated size.
    ///
    /// By default, this just calls `new()`
    fn new_estimated(&mut self, _: AllocLen) -> LocalMessage {
        self.new_message()
    }
}

#[derive(Debug)]
pub struct ConnectionId(EventSender);

impl PartialEq for ConnectionId {
    fn eq(&self, other: &Self) -> bool {
        self.0.same_channel(&other.0)
    }
}

impl Eq for ConnectionId {}

pub struct QuestionTarget {
    conn: ConnectionId,
    question: QuestionId,
}

#[non_exhaustive]
pub struct OutboundMessage {
    pub message: LocalMessage,
}

pub trait MessageOutbound: MessageFactory {
    /// Send a message, or queue it to send later.
    fn send(&mut self, msg: OutboundMessage);
}

pub struct OwnedIncomingMessage {
    pub message: MessagePayload,
}

pub trait IncomingMessage {
    fn message(&self) -> &dyn ReadArena;
    fn into_owned(self) -> OwnedIncomingMessage;
}

impl IncomingMessage for OutboundMessage {
    fn message(&self) -> &dyn ReadArena {
        self.message.as_read_arena()
    }
    fn into_owned(self) -> OwnedIncomingMessage {
        OwnedIncomingMessage { message: MessagePayload::Local(self.message) }
    }
}

pub struct QuestionPipeline {
    question: QuestionId,
    events: EventSender,
}

impl PipelineResolver<RpcChannel> for QuestionPipeline {
    fn resolve(
        &self,
        recv: chan::ResponseReceiver,
        key: Arc<[PipelineOp]>,
        channel: chan::Receiver,
    ) {
        // Create a new channel that indicates that this is a remote pipeline channel. This way
        // if we send the channel back over RPC we will fill in the CapDescriptor with a
        // PromisedAnswer.
        let (real_send, real_recv) = mpsc::channel(RpcChannel::PromisedAnswer {
            connection: ConnectionId(self.events.clone()),
            id: self.question,
            pipeline: key.clone(),
        });
        let _ = self.events.send(ConnectionEvent::PipelineStarted {
            id: self.question,
            ops: key,
            channel: real_recv,
            response: recv,
        });
        // Forward the original RPC channel so we can indicate that we're becoming a
        // promised answer channel.
        channel.forward_to(&real_send).unwrap();
    }
    fn pipeline(
        &self,
        recv: ResponseReceiverFactory<'_, RpcChannel>,
        key: Arc<[PipelineOp]>,
    ) -> chan::Sender {
        let (pipeline_send, pipeline_recv) = mpsc::channel(RpcChannel::Pipeline);
        self.resolve(recv.response(), key, pipeline_recv);
        pipeline_send
    }
}

enum ExportResolution {
    /// The export is hosted locally. It's not considered a promised export.
    Hosted,
    /// The export is a local promise. A connection event may cause the export to resolve.
    Promised(ConnectionTaskRef),
    /// The export is a local promise that has resolved into another cap with the given descriptor.
    Resolved {
        /// The export this export resolved to, released if the Resolve message is unimplemented.
        export: Option<ExportId>,
    },
}

struct Export {
    remote_ref_count: u32,
    resolution: ExportResolution,
    /// The client that should be used for referencing this capability locally
    client: chan::Sender,
}

struct Import {
    /// The number of times this import has been referenced on the connection.
    remote_ref_count: u32,
    /// Whether we should expect to see a Resolve message resolving this import.
    promised: bool,
    /// Whether we've sent a message to this import. If we have, we'll have to perform an embargo
    /// when it's resolved.
    sent_message: bool,
    /// The client that's given in incoming cap tables.
    client: chan::Sender,
}

struct Embargo {
    src: chan::Receiver,
    dst: chan::Sender,
}

struct CallPipeline {
    /// The weak sender associated with the existing channel for this pipeline.
    client: chan::WeakSender,
    /// The response receiver for the pipeline receiving the response.
    _response_receiver: chan::ResponseReceiver,
    // Whether a message was sent on this pipeline. If one was, when it resolves, we'll need to
    // perform a disembargo.
    sent_message: bool,
}

/// A question in an Export table.
enum Question {
    /// A highly simplified question for a Bootstrap request.
    ///
    /// Since we know this is for a bootstrap request, we know that outgoing pipelined requests must
    /// take a specific form and target the bootstrap capability stored for the connection.
    /// 
    /// When a `Return` is received targetting this question, the `ReceiverKey` is used to retreive
    /// the `Receiver` from the connection's `ReceiverSet` and resolve it to the returned
    /// capability. A `Finish` message is then sent for the question.
    /// 
    /// If the channel closes before a `Return` is received, a `Finish` message is sent for the
    /// question and the `Question` transitions to `Finished` to wait for a `Return`.
    Bootstrap {
        /// The channel associated with the bootstrap question pipeline.
        channel: chan::ReceiverKey,

        /// A flag indicating whether a disembargo is needed if the bootstrap resolves back to a
        /// capability hosted in our vat.
        sent_message: bool,
    },
    /// A normal Call in progress.
    Call {
        /// Holds a strong reference to the parent import. This will be released if the Call is
        /// finished prematurely.
        _parent: Option<chan::Sender>,

        /// A channel to send the response to.
        response_sender: Option<request::ResultsSender<RpcChannel>>,

        /// The response, if it was received.
        response: Option<chan::Response>,

        /// The active pipelines associated with this call
        pipelines: HashMap<Arc<[PipelineOp]>, CallPipeline>,

        /// The exports associated with this Question.
        exports: Vec<ExportId>,

        /// The finished task waiting for all references to the response of the request to be
        /// released.
        _finished: ConnectionTaskRef,
    },
    /// A gravestone for a finished question. Used when a question is `Finished` prematurely
    /// before a `Return` message is received.
    Finished {
        /// The exports for this question, if they exist. This is so that they can be released
        /// when we actually get the Return message.
        exports: Vec<ExportId>,
    },
}

impl Question {
    /// Replace this question with the finished state, moving the exports into it.
    pub fn finished(&mut self) -> Self {
        let exports = self.take_exports();
        replace(self, Self::Finished { exports })
    }

    /// Take the exports associated with this question.
    pub fn take_exports(&mut self) -> Vec<ExportId> {
        match self {
            Self::Bootstrap { .. } => Vec::new(),
            Self::Call { exports, .. } | Self::Finished { exports } =>
                replace(exports, Vec::new()),
        }
    }
}

/// The answer pipeline, which is set up in a very special way.
/// 
/// We need to make sure pipelined calls handle the Tribble 4-way Race.
/// Because of this, we can't have pipelined clients resolve immediately
/// to their most resolved client. If a pipelined client resolves into
/// something that is or has been a remote promised client, then we need to
/// resolve to the remote promised client. Here's an example
/// 
/// Given:
///   * Vat A: a request 'Q, capability \`1 and \`3
///   * Vat B: capability \`2
///   * \`1 is a promised capability resolved to \`2
///   * \`2 is a promised capability resolved to \`3
/// 
/// A pipelined request 'U is made based on 'Q to \`p1. \`p1 resolves to \`1.
/// The request 'U needs to go back over the connection to \`2 even we already know
/// \`2 resolved to \`3.
/// 
/// How it works:
/// 
/// For each pipeline we set up, we create a secondary pipeline to customize the resolution logic.
/// This also allows us to detect when an answer uses `set_pipeline()`.
/// 
/// When a request arrives, we check the `map` to see if there's an existing pipeline. Requests are
/// sent to the client specified in the `map`, not the `builder`.
/// 
/// If there is no existing client, we have to make one. The PipelineBuilder is used to build the
/// intermediary pipeline. Then, a new weak channel is made to hold the incoming requests. A task
/// is spawned that waits for the resolution of the intermediary pipeline and, once completed,
/// the results are returned and handled by `handle_answer_pipeline_resolution()`.
/// 
/// Note: because of the delayed resolution of `handle_answer_pipeline_resolution()`, we can't
/// return an answer when the response is processed by the connection.
struct AnswerPipeline {
    /// The pipeline builder, used to construct pipelines.
    /// 
    /// These pipelines are not used directly, instead they're used for their pipeline resolution.
    /// Because of `set_pipeline`, we can't simply make our own pipelines, we have to make pipelines
    /// which resolve to _other_ pipelines.
    builder: chan::PipelineBuilder,
    /// How many pipelines are waiting to be resolved? Once this reaches zero, a return message can
    /// be sent with the response.
    resolving_pipelines: usize,
    /// The memoized pipelines for this request.
    map: HashMap<Arc<[PipelineOp]>, ActivePipeline>,
}

struct ActivePipeline {
    resolve_task: Option<ConnectionTaskRef>,
    sender: chan::WeakSender,
}

impl AnswerPipeline {
    pub fn new(builder: chan::PipelineBuilder) -> Self {
        Self { builder, resolving_pipelines: 0, map: HashMap::new() }
    }

    pub fn is_resolving(&self) -> bool {
        self.resolving_pipelines != 0
    }

    pub fn build(&mut self, answer: AnswerId, ops: &[PipelineOp]) -> (chan::Sender, Option<ConnectionDataTask>) {
        if let Some(existing) = self.map.get(ops) {
            if let Some(existing) = existing.sender.upgrade() {
                return (existing, None)
            }

            // This pipeline is dead, remove it.
            self.map.remove(ops);
        }

        // Use the PipelineBuilder to construct a pipeline. This will always produce a new channel,
        // whether or not the pipeline has already resolved. This is so that when we attempt to
        // wait for resolution, we'll always get the first channel that was resolved to.
        let ops: Arc<[PipelineOp]> = Arc::from(ops);
        let pipeline = self.builder.build_or_resolve::<_, [_], _, _, _>(
            ops.clone(),
            || RpcChannel::Pipeline,
        |pipe, resp, key| {
            let (send, recv) = mpsc::channel(RpcChannel::Pipeline);
            pipe.resolve(resp.response(), key, recv);
            send
        },
        |pipe, resp, key| {
            let (send, recv) = mpsc::channel(RpcChannel::Pipeline);
            pipe.resolve(resp.response(), key, recv);
            send
        });

        // Now, set up the real channel. This is the channel that incoming requests will be sent to.
        let (real_send, real_recv) = mpsc::channel(RpcChannel::Pipeline);

        let task_ops = ops.clone();
        let resolve_task = async move {
            let ops = task_ops;
            let mut recv = real_recv;
            let pipeline = pipeline;
            let resolution = {
                let mut closed = pin!(recv.closed());
                let mut resolved = pin!(pipeline.resolution());
                tokio::select! {
                    _ = &mut closed => {
                        // The receiver closed, so we don't need to be waiting for resolution anymore.
                        AnswerPipelineResolution::PipelineDropped
                    }
                    res = &mut resolved => {
                        match res {
                            mpsc::Resolution::Forwarded(sender) =>
                                AnswerPipelineResolution::Resolved(sender.clone()),
                            mpsc::Resolution::Dropped =>
                                AnswerPipelineResolution::Error(dropped_cap()),
                            mpsc::Resolution::Error(err) =>
                                AnswerPipelineResolution::Error(err.clone()),
                        }
                    }
                }
            };
            ConnectionTaskResult::AnswerPipelineResolution { answer, ops, resolution, recv }
        };

        let task = new_task(resolve_task);
        let task_ref = task.data_ref();
        self.map.insert(ops, ActivePipeline { resolve_task: Some(task_ref), sender: real_send.downgrade() });
        self.resolving_pipelines += 1;

        (real_send, Some(task))
    }

    pub fn resolved(
        &mut self,
        ops: Arc<[PipelineOp]>,
        resolution: AnswerPipelineResolution,
        recv: chan::Receiver,
        finished: bool,
    ) -> ConnectionResult<()> {
        let hash_map::Entry::Occupied(mut entry) = self.map.entry(ops) else {
            return Err(Fatal(Error::failed("missing pipeline entry for answer")))
        };

        entry.get_mut().resolve_task = None;

        match resolution {
            AnswerPipelineResolution::Resolved(sender) => {
                // TODO: This probably needs embargoes or something
                if let Err(err) = recv.forward_to(&sender) {
                    err.close(self_resolution_error());
                }
            },
            AnswerPipelineResolution::Error(error) => {
                recv.close(error);
            },
            _ => {}
        }

        if finished {
            entry.remove();
        }

        self.resolving_pipelines -= 1;
        Ok(())
    }
}

enum Answer {
    /// A highly simplified answer for a bootstrap request.
    ///
    /// Since we know this is for a bootstrap request, we know
    /// that incoming pipelined requests must take a specific form
    /// and target the bootstrap capability stored for the connection.
    /// All that we need to track is the export ID for when the
    /// question is finished so the reference count can be possibly
    /// decremented.
    Bootstrap {
        /// The channel to direct pipelined requests to.
        client: chan::Sender,
        /// The export associated with this bootstrap answer
        export: Option<ExportId>,
    },
    /// An answer for an RPC Call.
    Call(CallAnswer),
}

struct CallAnswer {
    id: AnswerId,
    call_words: usize,

    reflected: bool,
    finished: bool,
    release_return_caps: bool,

    /// The pipeline for this call, if one was configured.
    pipeline: Option<AnswerPipeline>,
    /// The return task for the call. When this is None, the `response` can be read.
    return_task: Option<ConnectionTaskRef>,

    response: Option<chan::Response>,
    response_to_send: Option<OutboundMessage>,

    /// The exports to release.
    exports: Vec<ExportId>,
}

impl CallAnswer {
    fn new(id: AnswerId, call_words: usize, return_task: ConnectionTaskRef, reflected: bool, pipelines: Option<chan::PipelineBuilder>) -> Self {
        Self {
            id,
            call_words,
            reflected,
            finished: false,
            release_return_caps: false,
            pipeline: pipelines.map(AnswerPipeline::new),
            return_task: Some(return_task),
            response: None,
            response_to_send: None,
            exports: Vec::new(),
        }
    }

    fn is_answered(&self) -> bool {
        self.return_task.is_none()
    }

    fn pipeline(&mut self, ops: &[PipelineOp]) -> (chan::Sender, Option<ConnectionDataTask>) {
        let Some(pipeline) = self.pipeline.as_mut() else {
            return (mpsc::broken(chan::RpcChannel::Broken, no_pipeline_error()), None)
        };

        if self.finished {
            return (mpsc::broken(chan::RpcChannel::Broken, finished_answer_error()), None)
        }

        pipeline.build(self.id, ops)
    }

    /// Set the answer state to finished. After this, no more messages can come in referencing the answer
    /// until after we've sent a Return message to indicate what happened with it.
    fn finish(
        &mut self,
        outbound: &mut dyn MessageOutbound,
        release_caps: bool,
    ) -> ConnectionResult<CallUpdateResults> {
        if replace(&mut self.finished, true) {
            return Err(Fatal(Error::failed("already received Finish for this answer")))
        }

        self.release_return_caps = release_caps;

        let pipelines_resolving = self.pipeline.as_ref().is_some_and(AnswerPipeline::is_resolving);
        if pipelines_resolving {
            // There's still pipelines resolving, so we can't do anything yet. Instead, when the
            // last pipeline resolves or the response is received, we'll do the return then.

            return Ok(CallUpdateResults::none())
        }

        // No pipelines! The answer can now really be finished
        self.finished(outbound)
    }

    /// Handle the response
    fn handle_response(
        &mut self,
        results: Option<chan::Response>,
        response_to_send: OutboundMessage,
        exports: Vec<ExportId>,
    ) -> ConnectionResult<CallUpdateResults> {
        self.response = results;
        self.response_to_send = Some(response_to_send);
        self.exports = exports;
        self.return_task = None;

        let pipelines_resolving = self.pipeline.as_ref().is_some_and(AnswerPipeline::is_resolving);
        if pipelines_resolving {
            // Can't do anything, still got pipelines to resolve
            return Ok(CallUpdateResults::none())
        }

        // Well we can't be finished, since we would've taken a different path.
        // The call would've been canceled already. Which means we can now send the response!

        Ok(CallUpdateResults {
            task_to_remove: None, // no task since it would've already been removed
            message_to_send: self.response_to_send.take(),
            finished: false,
            call_words_to_remove: self.call_words, // we're sending the response, the call is no longer in-flight
            exports_to_release: Vec::new(), // we don't know if we need to release exports yet
        })
    }

    fn handle_resolve(
        &mut self,
        outbound: &mut dyn MessageOutbound,
        ops: Arc<[PipelineOp]>,
        resolution: AnswerPipelineResolution,
        recv: chan::Receiver,
    ) -> ConnectionResult<CallUpdateResults> {
        let Some(pipeline) = &mut self.pipeline else {
            return Err(Fatal(Error::failed(
                "answer pipeline resolved for answer without pipelines"
            )))
        };

        pipeline.resolved(ops, resolution, recv, self.finished)?;

        if !self.finished || pipeline.is_resolving() {
            // Can't be finished yet, we haven't gotten the Finish message or we still have
            // pipelines resolving.
            return Ok(CallUpdateResults::none())
        }

        self.finished(outbound)
    }

    fn finished(&mut self, outbound: &mut dyn MessageOutbound) -> ConnectionResult<CallUpdateResults> {
        let task_to_remove = self.return_task.take();
        let message_to_send = if task_to_remove.is_some() {
            Some(self.response_to_send.take().unwrap_or_else(|| {
                let mut message = outbound.new_message();
                let mut ret = message
                    .builder()
                    .init_struct_root::<rpc_capnp::Message>()
                    .into_return()
                    .init();
                ret.answer_id().set(self.id);
                ret.release_param_caps().set(false);
                ret.canceled().set();
                OutboundMessage { message }
            }))
        } else {
            None
        };

        let mut exports_to_release = replace(&mut self.exports, Vec::new());
        if !self.release_return_caps {
            exports_to_release = Vec::new();
        }

        let call_words_to_remove = if message_to_send.is_some() { self.call_words } else { 0 };

        Ok(CallUpdateResults {
            message_to_send,
            exports_to_release,
            task_to_remove,
            call_words_to_remove,
            finished: true,
        })
    }
}

#[must_use]
struct CallUpdateResults {
    message_to_send: Option<OutboundMessage>,
    exports_to_release: Vec<ExportId>,
    task_to_remove: Option<ConnectionTaskRef>,
    call_words_to_remove: usize,
    finished: bool,
}

impl CallUpdateResults {
    fn none() -> Self {
        Self {
            message_to_send: None,
            exports_to_release: Vec::new(),
            task_to_remove: None,
            call_words_to_remove: 0,
            finished: false,
        }
    }
}

#[derive(Clone, Debug, PartialEq, Eq)]
pub struct PromisedAnswer {
    question: QuestionId,
    transform: Option<Arc<[PipelineOp]>>,
}

impl PromisedAnswer {
    fn write_proto(&self, proto: &mut BuilderOf<'_, rpc_capnp::PromisedAnswer>) -> Result<(), Error> {
        let ops = self.transform.as_deref().unwrap_or_default();
        let Ok(count) = list::ElementCount::try_from(ops.len()) else {
            return Err(Error::failed("too many pipeline ops"))
        };

        proto.question_id().set(self.question);
        let mut list = proto.transform().init(count.get());
        for (op, idx) in ops.iter().zip(0..) {
            let mut b = list.at(idx).get();
            match op {
                PipelineOp::PtrField(ptr) => {
                    b.get_pointer_field().set(*ptr);
                }
            }
        }

        Ok(())
    }
}

#[derive(Clone, Debug, PartialEq, Eq)]
pub enum CapTarget {
    Import(ImportId),
    PromisedAnswer(PromisedAnswer),
}

impl CapTarget {
    fn from_channel(c: &RpcChannel) -> Option<Self> {
        Some(match c {
            RpcChannel::Bootstrap { id, .. } => CapTarget::PromisedAnswer(
                PromisedAnswer { question: *id, transform: None },
            ),
            RpcChannel::PromisedAnswer { id, pipeline, .. } => CapTarget::PromisedAnswer(
                PromisedAnswer { question: *id, transform: Some(pipeline.clone()) }
            ),
            RpcChannel::Import { id, .. } => CapTarget::Import(*id),
            _ => return None,
        })
    }

    fn write_proto(
        &self,
        proto: &mut BuilderOf<'_, rpc_capnp::MessageTarget>,
    ) -> Result<(), Error> {
        match self {
            CapTarget::Import(import) => {
                proto.imported_cap().set(*import);
            },
            CapTarget::PromisedAnswer(promised) => {
                let mut msg_promised = proto.promised_answer().init();
                promised.write_proto(&mut msg_promised)?;
            },
        };
        Ok(())
    }
}

/// Connection events that don't need to be completed in a specific order relative to
/// outgoing messages.
pub enum ConnectionEvent {
    /// Used to resolve a promise pipelined client based on a question sent
    /// over an RPC connection.
    PipelineStarted {
        id: QuestionId,
        ops: Arc<[PipelineOp]>,
        channel: chan::Receiver,
        response: chan::ResponseReceiver,
    },
}

pub type EventSender = tokio_mpsc::UnboundedSender<ConnectionEvent>;
type EventReceiver = tokio_mpsc::UnboundedReceiver<ConnectionEvent>;

fn send_finish(outbound: &mut dyn MessageOutbound, question_id: QuestionId, release_caps: bool) {
    let mut message = outbound.new_message();
    let mut finished = message.builder()
        .init_struct_root::<rpc_capnp::Message>()
        .into_finish()
        .init();
    finished.question_id().set(question_id);
    finished.release_result_caps().set(release_caps);
    outbound.send(OutboundMessage { message });
}

fn send_release(outbound: &mut dyn MessageOutbound, import_id: ImportId, count: u32) {
    let mut message = outbound.new_message();
    let mut release = message.builder()
        .init_struct_root::<rpc_capnp::Message>()
        .into_release()
        .init();
    release.id().set(import_id);
    release.reference_count().set(count);
    outbound.send(OutboundMessage { message });
}

fn send_abort(outbound: &mut dyn MessageOutbound, err: &Error) {
    let mut message = outbound.new_message();
    let mut abort = message.builder()
        .init_struct_root::<rpc_capnp::Message>()
        .into_abort()
        .init();
    error_to_exception(err, &mut abort);
    outbound.send(OutboundMessage { message });
}

#[non_exhaustive]
#[derive(Default)]
pub struct ConnectionOptions {
    /// Controls whether pipeline optimization hints are used. If this is true
    /// the other party must also use pipeline hints as well.
    pub use_pipeline_hints: bool,

    /// The options to use when reading incoming messages
    pub reader_options: ReaderOptions,
}

enum ConnectionTaskResult {
    QuestionFinished {
        question: QuestionId,
    },
    AnswerReturned {
        answer: AnswerId,
        response: Option<chan::Response>,
    },
    AnswerPipelineResolution {
        answer: AnswerId,
        ops: Arc<[PipelineOp]>,
        resolution: AnswerPipelineResolution,
        recv: chan::Receiver,
    },
    ExportResolved {
        export: ExportId,
        result: Result<chan::Sender>,
    },
}

enum AnswerPipelineResolution {
    /// The pipeline was dropped because there were no more active requests.
    PipelineDropped,
    /// The answer pipeline resolved to the given sender
    Resolved(chan::Sender),
    /// The answer pipeline resolved to an error
    Error(Error),
}

type ConnectionFuture = Pin<Box<dyn Future<Output = ConnectionTaskResult> + Send + 'static>>;
type ConnectionTaskRef = DataRef<(), ConnectionFuture>;
type ConnectionDataTask = DataTask<(), ConnectionFuture>;
type ConnectionTaskSet = DataTaskSet<(), ConnectionFuture>;

fn new_task<F>(fut: F) -> ConnectionDataTask
where
    F: Future<Output = ConnectionTaskResult> + Send + 'static,
{
    DataTask::new((), Box::pin(fut) as _)
}

#[derive(Debug)]
enum ConnectionError {
    Recoverable(Error),
    Fatal(Error),
}

impl From<recapn::Error> for ConnectionError {
    fn from(value: recapn::Error) -> Self {
        ConnectionError::Fatal(Error::from(value))
    }
}

use ConnectionError::{Recoverable, Fatal};

type ConnectionResult<T> = Result<T, ConnectionError>;

/// Handles all the state for a single connection.
/// 
/// Functions are named according the following convention:
/// 
/// * 'handle' functions handle internal connection events such as tasks, external events, and
///   channel messages
/// * `send` functions handle sending outgoing messages on the connection
/// * `recv` functions handle receiving incoming messages on the connection
struct ConnectionState {
    exports: ExportTable<Export>,
    questions: ExportTable<Question>,
    answers: ImportTable<Answer>,
    imports: ImportTable<Import>,
    embargo: ExportTable<Embargo>,

    /// A back-ref from a client in the export table to the export ID of the client
    export_by_client: FnvHashMap<chan::Sender, ExportId>,

    channels: chan::ReceiverSet,

    call_words_in_flight: usize,

    conn_events_sender: EventSender,
    conn_events: EventReceiver,
    tasks: ConnectionTaskSet,

    options: ConnectionOptions,
    disconnect: Option<Error>,
}

impl ConnectionState {
    fn connection_id(&self) -> ConnectionId {
        ConnectionId(self.conn_events_sender.clone())
    }

    #[inline]
    fn is_idle(&self) -> bool {
        self.exports.is_empty() &&
        self.questions.is_empty() &&
        self.answers.is_empty() &&
        self.imports.is_empty()
    }

    #[inline]
    fn same_connection(&self, other: &ConnectionId) -> bool {
        self.conn_events_sender.same_channel(&other.0)
    }

    fn close(&mut self, outbound: &mut dyn MessageOutbound, err: Error) -> Error {
        if let Some(err) = self.disconnect.clone() {
            return err
        }

        self.disconnect = Some(err.clone());
        send_abort(outbound, &err);

        for c in self.channels.remove_all() {
            c.close(err.clone());
        }

        self.conn_events.close();
        self.tasks.remove_all();
        self.export_by_client.clear();

        self.call_words_in_flight = 0;

        err
    }

    fn handle_joined_task(
        &mut self,
        outbound: &mut dyn MessageOutbound,
        result: ConnectionTaskResult,
    ) -> ConnectionResult<()> {
        match result {
            ConnectionTaskResult::QuestionFinished { question } => self.handle_question_finished(outbound, question),
            ConnectionTaskResult::AnswerReturned { answer, response } => {
                self.handle_answer_returned(outbound, answer, response)
            }
            ConnectionTaskResult::AnswerPipelineResolution { answer, ops, resolution, recv } => {
                self.handle_answer_pipeline_resolution(outbound, answer, ops, resolution, recv)
            }
            ConnectionTaskResult::ExportResolved { export, result } => {
                self.handle_export_resolved(outbound, export, result)
            },
        }
    }

    /// Handles when a Question we've sent has been finished because all receivers for the response
    /// were dropped locally.
    fn handle_question_finished(
        &mut self,
        outbound: &mut dyn MessageOutbound,
        question_id: QuestionId,
    ) -> ConnectionResult<()> {
        let Some(question) = self.questions.get_mut(question_id) else {
            return Err(Fatal(Error::failed("finished question that wasn't in table")))
        };

        match question.finished() {
            Question::Bootstrap { .. } => {
                // Bootstrap questions don't get finished on this path, they get finished through
                // `handle_bootstrap_question_close` when the bootstrap pipeline is closed.
                return Err(Fatal(Error::failed("expected call")))
            },
            Question::Finished { .. } => {
                return Err(Fatal(Error::failed("call already finished")))
            },
            Question::Call { response: Some(_), .. } => {
                // There should be no pipelines in the map since us receiving the Finished signal
                // must mean there's no response receivers in the map to begin with.

                // If we've received the response then that means we don't need to wait for
                // a return message, so we remove the question here.
                self.questions.remove(question_id);
            },
            Question::Call { response: None, .. } => {
                // We have to wait for the other side to return something.
            }
        }

        send_finish(outbound, question_id, false);

        Ok(())
    }

    /// Handles when an Answer we're waiting on returns successfully.
    /// 
    /// Note, this may not actually *return* an answer immediately, since we need to wait for all
    /// pipeline resolution tasks to occur. So this only configures the response.
    /// 
    /// A return is only sent once all promise pipelines have resolved. See AnswerPipeline for why
    /// this is complicated.
    fn handle_answer_returned(
        &mut self,
        outbound: &mut dyn MessageOutbound,
        answer_id: AnswerId,
        results: Option<chan::Response>,
    ) -> ConnectionResult<()> {
        let Some(answer) = self.answers.get(answer_id) else {
            return Err(Fatal(Error::failed("invalid answer")))
        };

        let answer = match answer {
            Answer::Bootstrap { .. } => {
                return Err(Fatal(Error::failed("answer returned for bootstrap")))
            },
            Answer::Call(a) => a,
        };

        if answer.is_answered() {
            return Err(Fatal(
                Error::failed("received response for answer we already responded to")
            ))
        }

        let mut exports = Vec::new();
        let mut message = outbound.new_message();
        let mut ret = message.builder()
            .init_struct_root::<rpc_capnp::Message>()
            .into_return()
            .init();
        ret.answer_id().set(answer_id);
        ret.release_param_caps().set(false);

        if answer.reflected {
            ret.results_sent_elsewhere().set();
        } else {
            // Coerce the response into a result
            let return_payload = match &results {
                Some(resp) => resp
                    .unwrap_results()
                    .as_ref()
                    .map_err(|e| e.clone()),
                None => Err(dropped_request()),
            };

            match return_payload {
                Ok(response) => {
                    // TODO(now): Don't copy the results, instead provide the destination message
                    // in the parameters and use refcounting to share it between the outbound and
                    // the stored result.
                    let mut payload = ret.results().init();
                    let table = {
                        let mut outgoing_table = Table::new(Vec::new());
                        let response_table = response.table.reader();
                        let response_reader = response.message.reader(
                            self.options.reader_options.clone(),
                        );
                        let response_payload = match response.root {
                            chan::ResultsRoot::Results => {
                                response_reader.root().imbue::<CapTable<'_>>(response_table)
                            },
                            chan::ResultsRoot::Return => {
                                response_reader
                                    .read_as_struct::<rpc_capnp::Message>()
                                    .r#return()
                                    .try_get()?
                                    .results()
                                    .try_get()?
                                    .content()
                                    .get()
                                    .imbue::<CapTable<'_>>(response_table)
                            },
                        };
                        payload
                            .content()
                            .get()
                            .imbue::<CapTable<'_>>(outgoing_table.builder())
                            .as_mut()
                            .try_copy_from(
                                response_payload.as_ref(),
                                false,
                                ReturnErrors,
                            )?;
                        outgoing_table.into_inner()
                    };
                    self.send_cap_table_with_exports(&mut exports, &table, &mut payload).map_err(Fatal)?;
                },
                Err(err) => {
                    let mut ex = ret.exception().init();
                    error_to_exception(&err, &mut ex);
                }
            }
        };

        let response_to_send = OutboundMessage { message };

        let Some(Answer::Call(answer)) = self.answers.get_mut(answer_id) else {
            unreachable!()
        };

        let results = answer.handle_response(results, response_to_send, exports)?;
        self.handle_call_update(outbound, answer_id, results)?;
        Ok(())
    }

    /// Handles when a pipeline for a question we're answering resolves.
    fn handle_answer_pipeline_resolution(
        &mut self,
        outbound: &mut dyn MessageOutbound,
        answer_id: AnswerId,
        ops: Arc<[PipelineOp]>,
        resolution: AnswerPipelineResolution,
        recv: chan::Receiver,
    ) -> ConnectionResult<()> {
        let Some(answer) = self.answers.get_mut(answer_id) else {
            return Err(Fatal(Error::failed("missing answer")))
        };

        let answer = match answer {
            Answer::Bootstrap { .. } => return Err(Fatal(Error::failed("invalid answer"))),
            Answer::Call(a) => a,
        };

        let results = answer.handle_resolve(outbound, ops, resolution, recv)?;
        self.handle_call_update(outbound, answer_id, results)?;

        Ok(())
    }

    fn handle_export_resolved(
        &mut self,
        outbound: &mut dyn MessageOutbound,
        export_id: ExportId,
        result: Result<chan::Sender>,
    ) -> ConnectionResult<()> {
        let mut message = outbound.new_message();
        let mut resolve = message
            .builder()
            .init_struct_root::<rpc_capnp::Message>()
            .into_resolve()
            .init();
        resolve.promise_id().set(export_id);
        let new_export = match result {
            Ok(cap) => {
                let mut proto = resolve.cap().init();
                self.send_cap(&cap, &mut proto).map_err(Fatal)?
            },
            Err(err) => {
                let mut ex = resolve.exception().init();
                error_to_exception(&err, &mut ex);

                None
            }
        };

        let Some(export) = self.exports.get_mut(export_id) else {
            return Err(Fatal(Error::failed("invalid resolving export")))
        };

        match export.resolution {
            ExportResolution::Hosted =>
                return Err(Fatal(Error::failed("hosted export resolved"))),
            ExportResolution::Promised(_) => {},
            ExportResolution::Resolved { .. } =>
                return Err(Fatal(Error::failed("resolved export resolved again"))),
        }

        export.resolution = ExportResolution::Resolved { export: new_export };

        outbound.send(OutboundMessage { message });

        Ok(())
    }
    
    fn recv_unimplemented_resolve(&mut self, export_id: ExportId) -> ConnectionResult<()> {
        let Some(resolved_export) = self.exports.get_mut(export_id) else {
            return Err(Fatal(Error::failed("unknown export in unimplemented resolve")))
        };

        let ExportResolution::Resolved { export } = resolved_export.resolution else {
            return Err(Fatal(Error::failed("export in unimplemented resolve is not resolved")))
        };

        if let Some(export) = export {
            self.recv_release(export, 1)?;
        }

        Ok(())
    }

    fn handle_conn_event(
        &mut self,
        _: &mut dyn MessageOutbound,
        incoming: ConnectionEvent,
    ) -> ConnectionResult<()> {
        match incoming {
            ConnectionEvent::PipelineStarted { id, ops, channel, response } => {
                self.handle_pipeline_started(id, ops, channel, response);
                Ok(())
            }
        }
    }

    fn handle_channel_close(
        &mut self,
        outbound: &mut dyn MessageOutbound,
        receiver: chan::Receiver,
    ) -> ConnectionResult<()> {
        match receiver.chan() {
            &RpcChannel::Bootstrap { id, .. } => {
                self.handle_closed_bootstrap_question(outbound, id)
            }
            &RpcChannel::Import { id, .. } => {
                self.handle_closed_import(outbound, id)
            }
            RpcChannel::PromisedAnswer { id, pipeline, .. } => {
                self.handle_closed_promised_answer(*id, pipeline)
            },
            _ => Err(Fatal(
                Error::failed("received unknown channel from outbound channel set")
            )),
        }
    }

    fn handle_channel_event(
        &mut self,
        outbound: &mut dyn MessageOutbound,
        _: CapTarget,
        _: chan::Sender,
        event: chan::RpcEvent,
    ) -> ConnectionResult<()> {
        let chan::RpcEvent::Embargo { connection, id, target } = event;

        if !self.same_connection(&connection) {
            return Err(Fatal(Error::failed("received disembargo event on a different connection")))
        }

        let mut message = outbound.new_message();
        let mut disembargo = message.builder()
            .init_struct_root::<rpc_capnp::Message>()
            .into_disembargo()
            .init();
        let mut target_proto = disembargo.target().init();
        target.write_proto(&mut target_proto).map_err(Fatal)?;
        disembargo.context().receiver_loopback().set(id);

        outbound.send(OutboundMessage { message });

        Ok(())
    }

    fn send_bootstrap(
        &mut self,
        outbound: &mut dyn MessageOutbound,
    ) -> chan::Sender {
        if let Some(err) = self.disconnect.clone() {
            return mpsc::broken(RpcChannel::Broken, err);
        }

        let connection_id = self.connection_id();
        let (id, sender) = self.questions.push_with(|id| {
            let (sender, receiver) = mpsc::channel(RpcChannel::Bootstrap {
                id,
                connection: connection_id,
            });
            let question_receiver = self.channels.insert(receiver);
            let question = Question::Bootstrap { channel: question_receiver, sent_message:  false };
            (question, sender)
        });

        let mut message = outbound.new_message();
        let mut bootstrap = message.builder()
            .init_struct_root::<rpc_capnp::Message>()
            .into_bootstrap()
            .init();
        bootstrap.question_id().set(id);
        outbound.send(OutboundMessage { message });

        sender
    }

    /// Handle closing a bootstrap question given that the channel has already been closed
    /// prematurely before receiving a Return.
    /// 
    /// This validates the connection state, sends a Finish message to the other party, and then
    /// removes the question from the table.
    fn handle_closed_bootstrap_question(
        &mut self,
        outbound: &mut dyn MessageOutbound,
        id: QuestionId,
    ) -> ConnectionResult<()> {
        let Some(question) = self.questions.get_mut(id) else {
            return Err(Fatal(
                Error::failed("question not in table")
            ))
        };

        match question.finished() {
            Question::Bootstrap { channel: _, .. } => {},
            Question::Call { .. } => {
                return Err(Fatal(
                    Error::failed("expected bootstrap question")
                ))
            }
            Question::Finished { .. } => {
                return Err(Fatal(
                    Error::failed("question already finished")
                ))
            }
        }

        send_finish(outbound, id, true);

        Ok(())
    }

    fn handle_closed_import(
        &mut self,
        outbound: &mut dyn MessageOutbound,
        id: ImportId,
    ) -> ConnectionResult<()> {
        let Some(import) = self.imports.remove(id) else {
            return Err(Fatal(
                Error::failed("import not in table")
            ))
        };

        send_release(outbound, id, import.remote_ref_count);

        Ok(())
    }

    /// Handle a Question pipeline closing. This will happen when all references to the channel
    /// have been dropped.
    /// 
    /// Note: Active requests on the channel have a reference to the channel, so this will only
    /// occur when all requests on the channel complete and all external references are released.
    fn handle_closed_promised_answer(
        &mut self,
        id: QuestionId,
        pipeline: &Arc<[PipelineOp]>,
    ) -> ConnectionResult<()> {
        let Some(question) = self.questions.get_mut(id) else {
            return Err(Fatal(Error::failed("promised answer closed for non-existent question")))
        };

        match question {
            Question::Bootstrap { .. } => return Err(Fatal(
                Error::failed("promised answer closed for bootstrap")
            )),
            Question::Call { pipelines, .. } => {
                drop(pipelines.remove(pipeline));
                Ok(())
            },
            Question::Finished { .. } => return Err(Fatal(
                Error::failed("question already finished")
            )),
        }
    }

    /// Sends a Call from a message received from a local RPC channel. The 
    fn send_call(
        &mut self,
        outbound: &mut dyn MessageOutbound,
        target: CapTarget,
        parent: chan::Sender,
        req: chan::Request,
    ) -> ConnectionResult<()> {
        // If the request is already canceled just drop it
        if req.is_finished() {
            return Ok(());
        }

        // Set up our piplining hints.
        let usage = req.usage();
        let no_promise_pipelining =
            usage == RequestUsage::Response && self.options.use_pipeline_hints;
        let only_promise_pipelining =
            usage == RequestUsage::Pipeline && self.options.use_pipeline_hints;

        let (request, responder) = req.respond();

        if let Some(err) = &self.disconnect {
            responder.respond(RpcResults::Owned(Err(err.clone())));
            return Ok(())
        }

        let Params {
            root,
            message,
            table,
        } = request.params;

        enum ParamsToFill<'b> {
            /// The params to fill are an external message where the root is the params themselves.
            ExternalParams(ParamsRoot, ExternalMessage),
            /// The params are an orphan from elsewhere in the message.
            LocalOrphan(Orphan<'b, AnyPtr>),
        }

        // First, we need to set up our outgoing message. We do 4 different things depending on
        // whether this is a local or external message and whether the parameters of the
        // call are in the root of the message or contained within a Call payload.
        // External messages must always be copied. Local messages can be manipulated in-place
        // to add in new details
        let mut outgoing;
        let (params, mut message) = match (root, message) {
            (root, MessagePayload::External(external)) => {
                outgoing = outbound.new_message();
                let message = outgoing.builder().init_struct_root::<rpc_capnp::Message>();
                (ParamsToFill::ExternalParams(root, external), message)
            }
            (ParamsRoot::Params, MessagePayload::Local(local)) => {
                outgoing = local;
                let BuilderParts {
                    mut root,
                    orphanage,
                } = outgoing.builder().into_parts();
                let orphan = root.disown_into(&orphanage);
                let message = root.init_struct::<rpc_capnp::Message>();
                (ParamsToFill::LocalOrphan(orphan), message)
            },
            (ParamsRoot::RpcCall, MessagePayload::Local(_)) => unreachable!(),
        };

        // Set up all the common Call fields like interface, method, target, payload, etc.
        let mut call = message.call().init();

        call.interface_id().set(request.interface);
        call.method_id().set(request.method);
        call.no_promise_pipelining().set(no_promise_pipelining);
        call.only_promise_pipeline().set(only_promise_pipelining);

        let mut msg_target = call.target().init();
        if let Err(err) = target.write_proto(&mut msg_target) {
            return Err(Fatal(err))
        }

        let mut payload = call.params().init();

        let mut table_to_write;
        match params {
            ParamsToFill::ExternalParams(root, params) => {
                table_to_write = Table::new(Vec::with_capacity(table.len()));

                fn copy_payload(
                    root: ParamsRoot,
                    src: any::PtrReader<'_>,
                    src_table: table::TableReader<'_>,
                    dst: any::PtrBuilder<'_>,
                    dst_table: table::TableBuilder<'_>,
                ) -> recapn::Result<()> {
                    let params = match root {
                        ParamsRoot::Params => src.clone(),
                        ParamsRoot::RpcCall => src.read_as_struct::<rpc_capnp::Message>()
                            .call()
                            .try_get()?
                            .params()
                            .try_get()? // todo: surface these errors through the 
                            .content()
                            .get()
                    }.imbue::<CapTable<'_>>(src_table);

                    dst.imbue::<CapTable<'_>>(dst_table)
                        .try_set(&params, false, ReturnErrors)
                }

                let params_reader =
                    message::Reader::new(&*params, self.options.reader_options.clone());
                let table_reader = table.reader();
                let table_builder = table_to_write.builder();
                if let Err(err) = copy_payload(
                    root,
                    params_reader.root(),
                    table_reader,
                    payload.content().ptr(),
                    table_builder,
                ) {
                    return Err(Fatal(Error::failed(format!(
                        "failed to copy params into call: {err:}"
                    ))));
                }
            }
            ParamsToFill::LocalOrphan(orphan) => {
                table_to_write = table;
                payload.content().adopt(orphan);
            }
        }

        let cap_table = table_to_write.into_inner();
        let mut exports = Vec::new();
        self.send_cap_table_with_exports(&mut exports, &cap_table, &mut payload).map_err(Fatal)?;

        if let ResponseTarget::Remote(response_to) = &request.target {
            if self.same_connection(&response_to.conn) {
                // This is a call being made to answer an answer we received. This means we can
                // set up the call to have the other vat send the result to themselves and fulfill
                // our Answer to skip needing to serialize the response.
                call.send_results_to().yourself().set();

                todo!()
            }

            // todo(level 3): support sendResultsTo.thirdParty
        }

        // This is a call we will have to handle locally.

        let (id, finished) = self.questions.push_with(|id| {
            let pipeline = QuestionPipeline {
                question: id,
                events: self.conn_events_sender.clone(),
            };
            let results = responder.set_pipeline(SetPipeline::RemotePipeline(pipeline));
            let finished = results.finished();
            let finished_task = new_task(async move {
                finished.await;
                ConnectionTaskResult::QuestionFinished { question: id }
            });
            let question = Question::Call {
                _parent: {
                    let mut parent = parent;
                    parent.resolve_in_place();
                    Some(parent)
                },
                response_sender: Some(results),
                response: None,
                pipelines: HashMap::default(),
                exports,
                _finished: finished_task.data_ref(),
            };
            (question, finished_task)
        });

        call.question_id().set(id);
        outbound.send(OutboundMessage { message: outgoing });

        self.tasks.insert(finished);

        Ok(())
    }

    /// Receive a list of caps from an incoming Call or Return, adjusting state accordingly. This
    /// table must be released later through `release_cap_table`.
    fn recv_cap_table(
        &mut self,
        outbound: &mut dyn MessageOutbound,
        table: list::StructListReader<'_, rpc_capnp::CapDescriptor>,
    ) -> Result<Vec<Option<Client>>> {
        let mut caps = Vec::with_capacity(table.len() as usize);
        for c in table {
            caps.push(self.recv_cap(outbound, &c)?);
        }
        Ok(caps)
    }

    fn recv_cap(
        &mut self,
        outbound: &mut dyn MessageOutbound,
        cap: &ReaderOf<'_, rpc_capnp::CapDescriptor>,
    ) -> Result<Option<Client>> {
        use rpc_capnp::cap_descriptor::Which;
        Ok(Some(Client(match cap.which() {
            Ok(Which::None(())) => return Ok(None),
            Ok(Which::SenderHosted(id)) => {
                self.recv_import_cap(outbound, id, false)?
            },
            Ok(Which::SenderPromise(id)) => {
                self.recv_import_cap(outbound, id, true)?
            }
            Ok(Which::ReceiverHosted(id)) => {
                self.recv_export_cap(id)?
            }
            Ok(Which::ReceiverAnswer(answer)) => {
                let answer = answer.try_get()?;
                let transform = to_pipeline_ops(answer.transform().try_get()?)?;
                self.recv_answer_cap(answer.question_id(), Arc::from(transform))?
            }
            Ok(Which::ThirdPartyHosted(third_party)) => {
                let third_party = third_party.try_get()?;
                self.recv_import_cap(outbound, third_party.vine_id(), false)?
            }
            Err(err) => return Err(Error::failed(format!("unknown cap: {}", err.0))),
        })))
    }

    /// Receive an import capability.
    /// 
    /// This sets up outgoing RPC channel for the import if it doesn't exist. If it does exist, we add
    /// a reference to the existing import.
    fn recv_import_cap(
        &mut self,
        outbound: &mut dyn MessageOutbound,
        id: ImportId,
        promise: bool,
    ) -> Result<chan::Sender> {
        let import = match self.imports.get_mut(id) {
            Some(import) => import,
            None => {
                let (client, recv) = mpsc::channel(RpcChannel::Import {
                    connection: self.connection_id(),
                    id,
                });
                let new_import = Import {
                    remote_ref_count: 0,
                    promised: promise,
                    sent_message: false,
                    client,
                };
                self.channels.insert(recv);
                self.imports.insert(id, new_import)
            },
        };

        let new_ref_count = match import.remote_ref_count.checked_add(1) {
            Some(new) => new,
            None => {
                // There's too many references to this import. Let's send a Release message
                // and reset our count back to 1
                send_release(outbound, id, u32::MAX - 1);

                1
            },
        };

        import.remote_ref_count = new_ref_count;
        Ok(import.client.clone())
    }

    fn recv_export_cap(
        &mut self,
        id: ExportId,
    ) -> Result<chan::Sender> {
        let Some(export) = self.exports.get_mut(id) else {
            return Err(Error::failed("unknown export"))
        };

        let Some(new_count) = export.remote_ref_count.checked_add(1) else {
            return Err(Error::failed("too many references for export"))
        };

        export.remote_ref_count = new_count;

        Ok(export.client.clone())
    }

    /// Receive a capability for a locally promised answer pipeline. This takes a slightly
    /// different path than `recv_target_from_promised_answer` since this refers to a capability
    /// that will be referenced locally, so we don't need to go through the answer pipeline.
    /// Instead we can bypass it and access the PipelineBuilder directly.
    fn recv_answer_cap(
        &mut self,
        id: AnswerId,
        ops: Arc<[PipelineOp]>,
    ) -> Result<chan::Sender> {
        let Some(answer) = self.answers.get_mut(id) else {
            return Err(Error::failed("unknown answer"))
        };

        let result = match answer {
            Answer::Bootstrap { client, .. } if ops.is_empty() => {
                Ok(client.clone())
            }
            Answer::Bootstrap { .. } => {
                Err(Recoverable(Error::failed("invalid pipeline ops for bootstrap")))
            },
            Answer::Call(CallAnswer { pipeline: None, .. }) => {
                Err(Recoverable(no_pipeline_error()))
            },
            Answer::Call(CallAnswer { pipeline: Some(pipeline), .. }) => {
                Ok(pipeline.builder.build::<Arc<[_]>, [_], _>(ops, || RpcChannel::Pipeline))
            },
        };

        match result {
            Ok(client) => Ok(client),
            Err(Recoverable(err)) => Ok(mpsc::broken(RpcChannel::Broken, err)),
            Err(Fatal(err)) => Err(err),
        }
    }

    /// Resolve a channel receiver to the given cap descriptor received from the other side of the
    /// connection.
    /// 
    /// This is for remote capabilities and promises (which we handle as channels that we're
    /// listening to the receivers of) resolving to capabilities. These can be either local or
    /// remote capabilities. So a promised bootstrap may resolve to an import, or a promised answer
    /// might resolve to a locally held capability.
    fn recv_import_resolve(
        connection_events: &EventSender,
        embargos: &mut ExportTable<Embargo>,
        outbound: &mut dyn MessageOutbound,
        import: chan::Receiver,
        sent_message: bool,
        new_cap: &chan::Sender,
    ) -> Result<()> {
        match import.chan() {
            RpcChannel::Bootstrap { .. } |
            RpcChannel::Import { .. } |
            RpcChannel::PromisedAnswer { .. } => {},
            _ => panic!("attempted to resolve channel that wasn't an import")
        }

        if !sent_message {
            // We didn't send any messages to this capability, so we don't need to do
            // any embargos. Just resolve directly to the destination.
            if let Err(import) = import.forward_to(new_cap) {
                import.close(self_resolution_error());
            }
            return Ok(())
        }

        // We sent a message, so we may have to set up an embargo if the resolution
        // returns to us.
        match new_cap.chan() {
            RpcChannel::Broken => {
                // Broken channels always resolve directly, since we know they're broken.
                import.forward_to(new_cap).unwrap();
                return Ok(())
            },
            RpcChannel::Local |
            RpcChannel::LocalShortening  |
            RpcChannel::Spawned |
            RpcChannel::Pipeline => {
                // This channel resolved back to us! So we'll need to put up an embargo.
                // Note we don't need to worry about this resolved cap being resolved to
                // something else since if it is, the other connection will handle their
                // own separate embargo and everything will work out.
            },
            RpcChannel::Bootstrap { connection, .. } |
            RpcChannel::PromisedAnswer { connection, .. } |
            RpcChannel::Import { connection, .. }
                if !connection_events.same_channel(&connection.0) => {
                    // Channels on a separate connection effectively resolve to us, so same as above,
                    // we need an embargo.
                },
            _ => {
                // This is the same connection, so let's resolve the channel directly. We know the
                // other party won't resolve to something which they already resolved, so we can
                // resolve directly.
                import.forward_to(new_cap).unwrap();
                return Ok(())
            }
        }

        Self::send_import_disembargo(embargos, outbound, import, new_cap.clone())
    }

    /// Send a disembargo message for an import we've received.
    fn send_import_disembargo(
        embargos: &mut ExportTable<Embargo>,
        outbound: &mut dyn MessageOutbound,
        src: chan::Receiver,
        dst: chan::Sender,
    ) -> Result<()> {
        let mut message = outbound.new_message();
        let mut disembargo = message.builder()
            .init_struct_root::<rpc_capnp::Message>()
            .into_disembargo()
            .init();
        let mut target = disembargo.target().init();
        match src.chan() {
            RpcChannel::Bootstrap { id, .. } => {
                let mut promised = target.promised_answer().init();
                promised.question_id().set(*id);
            }
            RpcChannel::PromisedAnswer { id, pipeline, .. } => {
                let mut promised = target.promised_answer().init();
                write_promised_answer(*id, pipeline, &mut promised)?;
            }
            RpcChannel::Import { id, .. } => {
                target.imported_cap().set(*id);
            }
            _ => unreachable!(),
        }

        let (id, _) = embargos.push(Embargo { src, dst });
        disembargo.context().sender_loopback().set(id);

        outbound.send(OutboundMessage { message });

        Ok(())
    }

    fn send_cap_table_with_exports(
        &mut self,
        exports: &mut Vec<ExportId>,
        cap_table: &[Option<Client>],
        payload: &mut BuilderOf<'_, rpc_capnp::Payload>,
    ) -> Result<()> {
        let Ok(table_len) = list::ElementCount::try_from(cap_table.len()) else {
            return Err(Error::failed("too many caps in message"))
        };
        let mut payload_table = payload.cap_table().init(table_len.get());
        for (i, cap) in (0..table_len.get()).zip(cap_table.iter()) {
            let Some(cap) = cap else { continue };
            let mut descriptor = payload_table.at(i).get();
            exports.extend(self.send_cap(&cap.0, &mut descriptor)?);
        }
        Ok(())
    }

    /// Send a resolved capability. This assumes the capability is in its currently most resolved
    /// form.
    fn send_cap_resolved(
        &mut self,
        cap: &chan::Sender,
        proto: &mut BuilderOf<'_, rpc_capnp::CapDescriptor>,
    ) -> Result<Option<ExportId>> {
        if let Some(&existing_export) = self.export_by_client.get(cap) {
            let Some(export) = self.exports.get_mut(existing_export) else {
                return Err(Error::failed("invalid export in client table"))
            };

            let Some(new_count) = export.remote_ref_count.checked_add(1) else {
                return Err(Error::failed("too many references on export"))
            };

            export.remote_ref_count = new_count;
            return Ok(Some(existing_export))
        }

        match cap.chan() {
            RpcChannel::Local => {
                // This is our only "hosted" cap type (for now)
                self.send_cap_as_new_export(cap, false, proto).map(Some)
            }
            RpcChannel::Broken |
            RpcChannel::LocalShortening |
            RpcChannel::Spawned |
            RpcChannel::Pipeline => {
                // These are effectively locally hosted promises, including the broken one!
                self.send_cap_as_new_export(cap, true, proto).map(Some)
            }
            RpcChannel::Bootstrap { connection, .. } |
            RpcChannel::PromisedAnswer { connection, .. } |
            RpcChannel::Import { connection, .. } if !self.same_connection(connection) => {
                // This are capabilities to other connections that aren't our own, so we treat them
                // as exports.
                self.send_cap_as_new_export(cap, true, proto).map(Some)
            },
            RpcChannel::Bootstrap { id, .. } => {
                let mut answer = proto.receiver_answer().init();
                answer.question_id().set(*id);

                Ok(None)
            }
            RpcChannel::PromisedAnswer { id, pipeline, .. } => {
                let mut answer = proto.receiver_answer().init();
                write_promised_answer(*id, pipeline, &mut answer)?;

                Ok(None)
            }
            RpcChannel::Import { id, .. } => {
                proto.receiver_hosted().set(*id);

                Ok(None)
            },
        }
    }

    fn send_cap(
        &mut self,
        cap: &chan::Sender,
        proto: &mut BuilderOf<'_, rpc_capnp::CapDescriptor>,
    ) -> Result<Option<ExportId>> {
        self.send_cap_resolved(cap.most_resolved().0, proto)
    }

    fn send_cap_as_new_export(
        &mut self,
        cap: &chan::Sender,
        is_promise: bool,
        proto: &mut BuilderOf<'_, rpc_capnp::CapDescriptor>,
    ) -> Result<ExportId> {
        let (id, export) = self.exports.push(Export {
            remote_ref_count: 1,
            resolution: ExportResolution::Hosted,
            client: cap.clone(),
        });

        self.export_by_client.insert(cap.clone(), id);

        if is_promise {
            let resolve_sender = cap.clone();
            let task = new_task(async move {
                ConnectionTaskResult::ExportResolved {
                    export: id,
                    result: match resolve_sender.resolution().await {
                        mpsc::Resolution::Forwarded(new) => Ok(new.clone()),
                        mpsc::Resolution::Dropped => Err(dropped_cap()),
                        mpsc::Resolution::Error(err) => Err(err.clone()),
                    },
                }
            });
            let task_ref = task.data_ref();
            self.tasks.insert(task);
            export.resolution = ExportResolution::Promised(task_ref);
            proto.sender_promise().set(id);
        } else {
            proto.sender_hosted().set(id);
        }

        Ok(id)
    }

    /// Begin handling a pipeline for a Question we sent.
    fn handle_pipeline_started(
        &mut self,
        question_id: QuestionId,
        ops: Arc<[PipelineOp]>,
        receiver: chan::Receiver,
        response_receiver: chan::ResponseReceiver,
    ) {
        let Some(question) = self.questions.get_mut(question_id) else {
            receiver.close(Error::disconnected("question not in table"));
            return;
        };

        let Question::Call { pipelines, response, .. } = question else {
            if cfg!(debug_assertions) {
                panic!("cannot handle pipeline events for question {}", question_id)
            }

            receiver.close(Error::failed("cannot handle pipeline events for this question"));
            return
        };

        if let Some(response) = &response {
            // We already got the repsonse for this call! We can now simply resolve the
            // pipeline immediately.
            let resolved = response.resolve_ops_to_sender(&ops);
            if let Err(receiver) = receiver.forward_to(&resolved) {
                receiver.close(self_resolution_error());
            }
            return
        }

        // We are currently waiting for this call to complete. We need to put the pipeline's
        // receiver into our receiver set to send pipelined calls out, but we also need to
        // track the receivers and their keys through the call's pipelines map.
        match pipelines.entry(ops) {
            // We've already got a pipeline for this set of ops, let's reuse the channel by
            // forwarding our new receiver to it.
            hash_map::Entry::Occupied(mut o) => {
                let pipeline = o.get_mut();
                if let Some(existing) = pipeline.client.upgrade() {
                    if let Err(receiver) = receiver.forward_to(&existing) {
                        // This should never happen.
                        receiver.close(self_resolution_error());
                        return
                    }
                } else {
                    // Oh, the existing client was already closed. In this case, we're
                    // going to receive an event for the channel closing in the future as
                    // there are no senders left for the channel and no messages in it.
                    // That means we can clean up this channel now and replace it with our
                    // new one.
                    let _ = self.channels.remove_by_weak_sender(&pipeline.client);
                    pipeline.client = self.channels.insert(receiver).into_weak_sender();
                }
            },
            hash_map::Entry::Vacant(v) => {
                let client = self.channels.insert(receiver).into_weak_sender();
                v.insert(CallPipeline {
                    client,
                    sent_message: false,
                    _response_receiver: response_receiver,
                });
            },
        }
    }

    fn recv_message_target(
        &mut self,
        target: &ReaderOf<'_, rpc_capnp::MessageTarget>,
    ) -> ConnectionResult<chan::Sender> {
        use rpc_capnp::message_target::Which;
        let target = match target.which() {
            Ok(Which::ImportedCap(cap)) => self.recv_target_from_import(cap).map(Some),
            Ok(Which::PromisedAnswer(promised_answer)) => {
                let promised_answer = promised_answer.try_get()?;
                let ops = promised_answer.transform().try_get()?;
                let ops = to_pipeline_ops(ops).map_err(Fatal)?;

                self.recv_target_from_promised_answer(promised_answer.question_id(), &ops)
            }
            Err(NotInSchema(v)) => return Err(Fatal(
                Error::failed(format!("unknown message target type: {v}"))
            ))
        };

        let sender = match target {
            Ok(Some(client)) => client,
            Ok(None) => null_cap(),
            Err(Recoverable(err)) => broken(err),
            Err(Fatal(err)) => return Err(Fatal(err)),
        };

        Ok(sender)
    }

    /// Get the Sender for the given incoming import ID.
    /// 
    /// This adds a references to the given export.
    fn recv_target_from_import(
        &mut self,
        import: ImportId,
    ) -> ConnectionResult<chan::Sender> {
        let Some(export) = self.exports.get_mut(import) else {
            return Err(Fatal(
                Error::failed("broken export descriptor")
            ))
        };

        match export.remote_ref_count.checked_add(1) {
            Some(new) => export.remote_ref_count = new,
            None => return Err(Fatal(
                Error::failed("too many references to export")
            ))
        }

        Ok(export.client.clone())
    }

    fn recv_target_from_promised_answer(
        &mut self,
        id: AnswerId,
        ops: &[PipelineOp],
    ) -> ConnectionResult<Option<chan::Sender>> {
        let Some(answer) = self.answers.get_mut(id) else {
            return Err(Recoverable(
                Error::failed("invalid promised answer")
            ));
        };

        match answer {
            Answer::Bootstrap { client, .. } => {
                if !ops.is_empty() {
                    return Err(Recoverable(Error::failed("invalid pipeline ops for bootstrap")))
                }

                Ok(Some(client.clone()))
            },
            Answer::Call(CallAnswer { pipeline: None, .. }) => {
                Err(Recoverable(Error::failed("attempted to pipeline on request without pipelining")))
            },
            Answer::Call(answer) => {
                let (sender, task) = answer.pipeline(ops);
                if let Some(task) = task {
                    self.tasks.insert(task);
                }
                Ok(Some(sender))
            },
        }
    }

    fn recv_unimplemented(
        &mut self,
        msg: &ReaderOf<'_, rpc_capnp::Message>,
    ) -> ConnectionResult<()> {
        use rpc_capnp::message::Which;

        match msg.which() {
            Ok(Which::Resolve(resolve)) => {
                let resolve = resolve.try_get()?;
                // Resolve is unimplemented. In this case we can just release the new Export.
                self.recv_unimplemented_resolve(resolve.promise_id())?;
            },
            _ => return Err(Fatal(Error::failed(format!(
                "Peer did not implement required RPC message type: {}",
                msg.as_ref().data_field::<u16>(0), // TODO: clean up magic number
            ))))
        }

        Ok(())
    }

    fn recv_bootstrap(
        &mut self,
        outbound: &mut dyn MessageOutbound,
        id: AnswerId,
        client: &chan::Sender,
    ) -> ConnectionResult<()> {
        if self.answers.contains(id) {
            return Err(Fatal(Error::failed("received bootstrap request for existing answer")))
        }

        let mut response = outbound.new_message();
        let mut ret = response.builder()
            .init_struct_root::<rpc_capnp::Message>()
            .into_return()
            .init();
        ret.answer_id().set(id);

        let mut results = ret.results().init();
        let mut table = results.cap_table().init(1);
        let mut descriptor = table.at(0).get();

        let export = self.send_cap(client, &mut descriptor).map_err(Fatal)?;
        self.answers.insert(id, Answer::Bootstrap { client: client.clone(), export });

        results.content().ptr().as_mut().set_capability_ptr(0);

        outbound.send(OutboundMessage { message: response });

        Ok(())
    }

    fn recv_call(
        &mut self,
        outbound: &mut dyn MessageOutbound,
        message: MessagePayload,
    ) -> ConnectionResult<()> {
        let reader = message.reader(self.options.reader_options.clone());
        let call_words = reader.size_in_words();

        let m = reader.read_as_struct::<rpc_capnp::Message>();
        let call = m.call().field().unwrap().try_get()?;

        let answer_id = call.question_id();

        let target = self.recv_message_target(&call.target().try_get()?)?;

        let payload = call.params().try_get()?;
        let cap_table = self.recv_cap_table(outbound, payload.cap_table().try_get()?).map_err(Fatal)?;

        let interface_id = call.interface_id();
        let method_id = call.method_id();

        let no_promise_pipelining = call.no_promise_pipelining();
        let only_promise_pipelining = call.only_promise_pipeline();
        if no_promise_pipelining && only_promise_pipelining {
            return Err(Fatal(Error::failed("Call cannot be no pipelining and only pipelining")))
        }

        use rpc_capnp::call::send_results_to::Which;
        let redirect_results = match call.send_results_to().which() {
            Ok(Which::Caller(())) => false,
            Ok(Which::Yourself(())) => true,
            _ => return Err(Fatal(Error::failed("Unsupported `Call.sendResultsTo`."))),
        };

        let msg = chan::RpcCall {
            interface: interface_id,
            method: method_id,
            target: ResponseTarget::Remote(QuestionTarget {
                conn: ConnectionId(self.conn_events_sender.clone()),
                question: answer_id,
            }),
            params: Params {
                root: ParamsRoot::RpcCall,
                message,
                table: Table::new(cap_table),
            },
        };
        let (req, response, pipeline) = request::request_response_pipeline::<RpcChannel>(msg);
        let task = new_task(async move {
            ConnectionTaskResult::AnswerReturned {
                answer: answer_id,
                response: response.await,
            }
        });
        let task_ref = task.data_ref();
        self.answers.insert(answer_id, Answer::Call(CallAnswer::new(
            answer_id,
            call_words,
            task_ref,
            redirect_results,
            Some(pipeline),
        )));

        self.tasks.insert(task);
        self.call_words_in_flight += call_words;

        if let Err((req, err)) = target.send(req) {
            let err = err.cloned().unwrap_or_else(dropped_cap);
            req.respond().1.respond(RpcResults::Owned(Err(err)));
        }

        Ok(())
    }

    /// Handles a Return message received from the other party.
    fn recv_return(
        &mut self,
        outbound: &mut dyn MessageOutbound,
        message: MessagePayload,
    ) -> ConnectionResult<()> {
        let reader = message.reader(self.options.reader_options.clone());
        let message_proto = reader.read_as_struct::<rpc_capnp::Message>();
        let ret = message_proto.r#return().field().unwrap().try_get()?;

        let question_id = ret.answer_id();
        let release_param_caps = ret.release_param_caps();

        enum Return<'a> {
            Results {
                table: Vec<Option<Client>>,
                content: any::PtrReader<'a>,
            },
            Exception(Error),
            Canceled,
            ResultsSentElsewhere,
            TakeFromOtherQuestion(chan::Response),
        }

        let deserialized_return = match ret.which() {
            Ok(WhichReturn::Results(res)) => {
                let payload = res.try_get()?;
                let table = payload.cap_table().try_get()?;

                Return::Results {
                    table: self.recv_cap_table(outbound, table).map_err(Fatal)?,
                    content: payload.content().get(),
                }
            }
            Ok(WhichReturn::Exception(ex)) => {
                let ex = ex.try_get()?;
                Return::Exception(exception_to_error(&ex))
            }
            Ok(WhichReturn::Canceled(())) => Return::Canceled,
            Ok(WhichReturn::ResultsSentElsewhere(())) => Return::ResultsSentElsewhere,
            Ok(WhichReturn::TakeFromOtherQuestion(id)) => Return::TakeFromOtherQuestion(todo!()),
            _ => return Err(Fatal(Error::failed("Unknown 'Return' type")))
        };

        let Some(question) = self.questions.get_mut(question_id) else {
            return Err(Fatal(Error::failed("invalid answerId")))
        };

        let exports = question.take_exports();
        match question {
            Question::Bootstrap { channel, sent_message } => {
                let Some(chan) = self.channels.remove(&channel) else {
                    return Err(Fatal(Error::failed("channel already removed")))
                };

                let sent_message = *sent_message;

                let cap = match deserialized_return {
                    Return::Results { ref table, content } =>
                        match content.as_ref().try_to_capability_index() {
                            Ok(Some(i)) => match table.get(i as usize) {
                                Some(Some(cap)) => Ok(cap),
                                Some(None) => Err(null_cap_error()),
                                None => Err(Error::from(recapn::Error::InvalidCapabilityPointer(i))),
                            },
                            Ok(None) => Err(null_cap_error()),
                            Err(err) => Err(Error::from(err)),
                        }
                    Return::Exception(ex) => Err(ex),
                    Return::Canceled => {
                        return Err(Fatal(Error::failed("Return message falsely claims call was canceled")))
                    }
                    Return::ResultsSentElsewhere => {
                        return Err(Fatal(Error::failed("Bootstrap results were sent elsewhere")))
                    }
                    Return::TakeFromOtherQuestion(_) => {
                        return Err(Fatal(Error::failed("Cannot takeFromOtherQuestion for Bootstrap results")))
                    }
                };

                match cap {
                    Ok(cap) => Self::recv_import_resolve(
                        &self.conn_events_sender,
                        &mut self.embargo,
                        outbound,
                        chan,
                        sent_message,
                        &cap.0,
                    ).map_err(Fatal)?,
                    Err(err) => chan.close(err),
                }

                let _ = self.questions.remove(question_id);

                send_finish(outbound, question_id, false);
            },
            Question::Call { response_sender, response, pipelines, .. } => 'breakout: {
                let results = match deserialized_return {
                    Return::Results { table, .. } => {
                        RpcResults::Owned(Ok(RpcResponse {
                            root: chan::ResultsRoot::Return,
                            message,
                            table: Table::new(table),
                        }))
                    }
                    Return::Exception(ex) => RpcResults::Owned(Err(ex)),
                    Return::Canceled => {
                        return Err(Fatal(Error::failed("Return message falsely claims call was canceled")))
                    }
                    Return::ResultsSentElsewhere if response_sender.is_some() => {
                        return Err(Fatal(
                            Error::failed("Expected results, but results were sent elsewhere")
                        ))
                    }
                    Return::ResultsSentElsewhere => {
                        // Nothing to do
                        break 'breakout
                    }
                    Return::TakeFromOtherQuestion(other) => RpcResults::OtherResponse(other),
                };

                let Some(response_sender) = response_sender.take() else {
                    return Err(Fatal(Error::failed("Sent results elsewhere, but got them here")))
                };

                let response = &*response.insert(response_sender.respond(results));
                for (ops, pipeline) in pipelines.drain() {
                    let Some(src) = self.channels.remove_by_weak_sender(&pipeline.client) else {
                        // No channel, move on to the next one
                        continue;
                    };

                    let dst = response.resolve_ops_to_sender(&ops);
                    Self::recv_import_resolve(
                        &self.conn_events_sender,
                        &mut self.embargo,
                        outbound,
                        src,
                        pipeline.sent_message,
                        &dst,
                    ).map_err(Fatal)?
                }
            },
            Question::Finished { .. } => {
                // Nothing to do
                self.questions.remove(question_id);
            }
        }

        if release_param_caps {
            for id in exports {
                self.recv_release(id, 1)?;
            }
        }

        Ok(())
    }

    fn recv_finish(
        &mut self,
        outbound: &mut dyn MessageOutbound,
        answer_id: AnswerId,
        release_caps: bool,
    ) -> ConnectionResult<()> {
        let Some(answer) = self.answers.get_mut(answer_id) else {
            return Err(Fatal(Error::failed("unknown answer ID")))
        };

        let answer = match answer {
            Answer::Bootstrap { export, .. } => {
                let export = export.take();
                if release_caps && let Some(export) = export {
                    self.recv_release(export, 1)?;
                }

                self.answers.remove(answer_id);

                return Ok(())
            },
            Answer::Call(answer) => answer,
        };

        let results = answer.finish(outbound, release_caps)?;
        self.handle_call_update(outbound, answer_id, results)?;

        Ok(())
    }

    fn handle_call_update(
        &mut self,
        outbound: &mut dyn MessageOutbound,
        answer_id: AnswerId,
        results: CallUpdateResults,
    ) -> ConnectionResult<()> {
        let CallUpdateResults {
            message_to_send,
            exports_to_release,
            task_to_remove,
            call_words_to_remove,
            finished,
        } = results;

        if let Some(message) = message_to_send {
            outbound.send(message);
        }
        for export in exports_to_release {
            self.recv_release(export, 1)?;
        }
        if let Some(task) = task_to_remove {
            let _ = self.tasks.remove(&task);
        }
        if finished {
            self.answers.remove(answer_id);
        }
        self.call_words_in_flight -= call_words_to_remove;
        Ok(())
    }

    fn recv_resolve(
        &mut self,
        outbound: &mut dyn MessageOutbound,
        resolve: &ReaderOf<'_, rpc_capnp::Resolve>,
    ) -> ConnectionResult<()> {
        use rpc_capnp::resolve::Which as WhichResolve;

        let result = match resolve.which() {
            Ok(WhichResolve::Cap(cap)) => {
                let cap = cap.try_get()?;
                let client = self.recv_cap(outbound, &cap)
                    .map_err(Fatal)?
                    .map(|c| c.0)
                    .unwrap_or_else(|| null_cap());
                Ok(client)
            },
            Ok(WhichResolve::Exception(ex)) => {
                let ex = ex.try_get()?;
                Err(exception_to_error(&ex))
            },
            Err(NotInSchema(v)) => return Err(Fatal(Error::failed(format!("unknown Resolve result: {v}")))),
        };

        let id = resolve.promise_id();
        let Some(import) = self.imports.get_mut(id) else {
            // We must've released this import already, so this is just a delayed Resolve message.
            return Ok(())
        };

        if !import.promised {
            return Err(Fatal(Error::failed("attempted to resolve hosted Import")))
        }

        let Some(src) = self.channels.remove_by_sender(&import.client) else {
            return Err(Fatal(Error::failed("missing channel for Import")))
        };

        match result {
            Ok(dst) => Self::recv_import_resolve(
                &self.conn_events_sender,
                &mut self.embargo,
                outbound,
                src,
                import.sent_message,
                &dst,
            ).map_err(Fatal)?,
            Err(err) => src.close(err),
        }

        Ok(())
    }

    fn recv_release(
        &mut self,
        export_id: ExportId,
        count: u32,
    ) -> ConnectionResult<()> {
        let Some(export) = self.exports.get_mut(export_id) else {
            return Err(Fatal(Error::failed("export does not exist")))
        };

        let Some(new_count) = export.remote_ref_count.checked_sub(count) else {
            return Err(Fatal(Error::failed("attempted to release an import past 0 references")))
        };

        if new_count != 0 {
            export.remote_ref_count = new_count;
            return Ok(())
        }

        // The new ref count is 0, so the export is released, we now need to clean up all the state
        // associated with the export, including the awaiting resolution task (if it exists) and
        // the client export map entry.
        let export = self.exports.remove(export_id).unwrap();
        assert_eq!(self.export_by_client.remove(&export.client), Some(export_id));

        match export.resolution {
            ExportResolution::Hosted | ExportResolution::Resolved { .. } => {
                // This export was hosted, or is already resolved, so there's no resolution task
                // for us to remove.
            },
            ExportResolution::Promised(task_ref) => {
                // There's a task! Let's remove it.
                assert!(self.tasks.remove(&task_ref).is_some());
            }
        }

        Ok(())
    }

    fn recv_disembargo(
        &mut self,
        disembargo: &ReaderOf<'_, rpc_capnp::Disembargo>,
    ) -> ConnectionResult<()> {
        use rpc_capnp::disembargo::context::Which as WhichContext;
        let target_proto = disembargo.target().try_get()?;
        let target = self.recv_message_target(&target_proto)?;
        let cap_target = CapTarget::from_channel(target.chan()).unwrap();
        match disembargo.context().which() {
            Ok(WhichContext::SenderLoopback(id)) => {
                // todo: Store this interest somewhere
                let event = chan::Event::with_inherent_interest(chan::RpcEvent::Embargo {
                    connection: self.connection_id(),
                    id,
                    target: cap_target,
                });
                target.send_event(event).unwrap();
            },
            Ok(WhichContext::ReceiverLoopback(id)) => {
                let Some(embargo) = self.embargo.remove(id) else {
                    return Err(Fatal(Error::failed("missing embargo")))
                };

                if let Err(src) = embargo.src.forward_to(&embargo.dst) {
                    src.close(self_resolution_error());
                }
            },
            _ => return Err(Fatal(Error::failed("unknown disembargo context"))),
        }

        Ok(())
    }
}

pub struct Connection<T: ?Sized> {
    state: ConnectionState,

    bootstrap: chan::Sender,
    outbound: T,
}

impl<T> Connection<T> {
    pub fn new(outbound: T, bootstrap: Client, options: ConnectionOptions) -> Self {
        let (conn_events_sender, conn_events) = tokio_mpsc::unbounded_channel();
        Connection {
            state: ConnectionState {
                exports: ExportTable::new(),
                questions: ExportTable::new(),
                answers: ImportTable::new(),
                imports: ImportTable::new(),
                embargo: ExportTable::new(),
                export_by_client: FnvHashMap::default(),
                channels: chan::ReceiverSet::new(),
                call_words_in_flight: 0,
                conn_events_sender,
                conn_events,
                tasks: DataTaskSet::new(),
                options,
                disconnect: None,
            },
            bootstrap: bootstrap.0,
            outbound,
        }
    }
}

impl<T: MessageOutbound> Connection<T> {
    /// Returns a client for interacting with the bootstrap capability of the connection.
    pub fn bootstrap(&mut self) -> Client {
        Client(self.state.send_bootstrap(&mut self.outbound))
    }

    /// Gets the number of Words involved in incoming active calls being processed by this
    /// connection.
    ///
    /// This can be used to implement a control flow limit on the connection to avoid being
    /// overwhelmed by requests from the other party.
    #[inline]
    pub fn call_words_in_flight(&self) -> usize {
        self.state.call_words_in_flight
    }

    /// Returns whether the connection may be considered idle.
    /// 
    /// A connection is condered idle if there's no active imports, exports, questions, or answers.
    /// The only thing that would make the connection active again would be a new incoming
    /// bootstrap message.
    #[inline]
    pub fn is_idle(&self) -> bool {
        self.state.is_idle()
    }

    #[inline]
    pub fn disconnected(&self) -> Option<&Error> {
        self.state.disconnect.as_ref()
    }

    pub fn poll_tasks(&mut self, cx: &mut std::task::Context<'_>) -> std::task::Poll<Error> {
        use std::task::{Poll::*, ready};
        if let Some(err) = self.state.disconnect.clone() {
            return Ready(err)
        }

        let (_, result) = ready!(self.state.tasks.poll_next(cx));
        let result = self.state.handle_joined_task(&mut self.outbound, result);

        match result {
            Ok(()) => {
                cx.waker().wake_by_ref();
                Pending
            },
            Err(Fatal(err) | Recoverable(err)) => Ready(self.close(err)),
        }
    }

    pub fn poll_events(&mut self, cx: &mut std::task::Context<'_>) -> std::task::Poll<Error> {
        use std::task::{Poll::*, ready};
        if let Some(err) = self.state.disconnect.clone() {
            return Ready(err)
        }

        let next = ready!(self.state.conn_events.poll_recv(cx))
            .expect("connection events channel closed prematurely");
        let result = self.state.handle_conn_event(&mut self.outbound, next);

        match result {
            Ok(()) => {
                cx.waker().wake_by_ref();
                Pending
            },
            Err(Fatal(err) | Recoverable(err)) => Ready(self.close(err)),
        }
    }

    pub fn poll_channels(&mut self, cx: &mut std::task::Context<'_>) -> std::task::Poll<Error> {
        use std::task::{Poll::*, ready};
        if let Some(err) = self.state.disconnect.clone() {
            return Ready(err)
        }

        let msg = ready!(self.state.channels.poll_recv(cx));
        let result = match msg {
            chan::SetRecvResult::Closed { receiver } =>
                self.state.handle_channel_close(&mut self.outbound, receiver),
            chan::SetRecvResult::Item {
                item: chan::Item::Request(request),
                receiver,
                sender,
            } => {
                let target = CapTarget::from_channel(receiver.chan()).expect("invalid target");
                self.state.send_call(&mut self.outbound, target, sender, request)
            },
            chan::SetRecvResult::Item {
                item: chan::Item::Event(event),
                receiver,
                sender,
            } => {
                let target = CapTarget::from_channel(receiver.chan()).expect("invalid target");
                self.state.handle_channel_event(&mut self.outbound, target, sender, event.into_inner())
            },
        };

        match result {
            Ok(()) => {
                cx.waker().wake_by_ref();
                Pending
            },
            Err(Fatal(err) | Recoverable(err)) => Ready(self.close(err)),
        }
    }

    /// Handles an internal event or outgoing message, then returns. If an error is
    /// returned, the connection has closed.
    ///
    /// This can be ran in a loop alongside whatever code is necessary to handle incoming
    /// messages.
    ///
    /// Since this handles one set of events, it can also be used alongside `call_words_in_flight`
    /// to implement a control flow limit. When the limit is exceeded after a call to
    /// `handle_message`, the connection loop can run `handle_events` until the limit is no longer
    /// exceeded.
    ///
    /// This function is cancel safe. If `handle_events` is used as the future in a tokio::select!
    /// statement and some other branch completes first, it is guaranteed that no messages
    /// were received and dropped.
    pub async fn handle_event(&mut self) -> Result<(), Error> {
        if let Some(err) = self.state.disconnect.clone() {
            return Err(err)
        }

        let result = select! {
            Some((_, result)) = self.state.tasks.join_next() => self.state.handle_joined_task(&mut self.outbound, result),
            Some(event) = self.state.conn_events.recv() => self.state.handle_conn_event(&mut self.outbound, event),
            Some(msg) = self.state.channels.recv() => {
                match msg {
                    chan::SetRecvResult::Closed { receiver } => self.state.handle_channel_close(&mut self.outbound, receiver),
                    chan::SetRecvResult::Item {
                        item: chan::Item::Request(request),
                        receiver,
                        sender,
                    } => {
                        let target = CapTarget::from_channel(receiver.chan())
                            .ok_or_else(|| Error::failed("invalid target"))?;

                        self.state.send_call(&mut self.outbound, target, sender, request)
                    },
                    chan::SetRecvResult::Item {
                        item: chan::Item::Event(event),
                        receiver,
                        sender,
                    } => {
                        let target = CapTarget::from_channel(receiver.chan())
                            .ok_or_else(|| Error::failed("invalid target"))?;

                        self.state.handle_channel_event(&mut self.outbound, target, sender, event.into_inner())
                    },
                }
            }
        };

        match result {
            Ok(()) => Ok(()),
            Err(Fatal(err) | Recoverable(err)) => Err(self.close(err)),
        }
    }

    /// Handle an incoming message. If an error occurs, close the connection.
    pub fn handle_message(&mut self, incoming: impl IncomingMessage) -> Result<(), Error> {
        let result = self.try_handle_message(incoming);
        if let Err(err) = &result {
            let _ = self.close(err.clone());
        }
        result
    }

    /// Handle an incoming message, but return any immediate errors intead of closing
    /// the connection.
    ///
    /// If an error is returned, that error should eventually be passed to `close()`.
    fn try_handle_message(
        &mut self,
        incoming: impl IncomingMessage,
    ) -> Result<(), Error> {
        let message = incoming.message();
        let reader = recapn::message::Reader::new(message, self.state.options.reader_options.clone());
        let reader = reader.read_as_struct::<rpc_capnp::Message>();

        use rpc_capnp::message::Which;
        let res = match reader.which() {
            Ok(Which::Unimplemented(unimplemented)) => {
                self.state.recv_unimplemented(&unimplemented.try_get()?)
            }
            Ok(Which::Abort(abort)) => {
                Err(Fatal(exception_to_error(&abort.try_get()?)))
            },
            Ok(Which::Bootstrap(bootstrap)) => {
                self.state.recv_bootstrap(&mut self.outbound, bootstrap.try_get()?.question_id(), &self.bootstrap)
            }
            Ok(Which::Call(_)) => {
                self.state.recv_call(&mut self.outbound, incoming.into_owned().message)
            },
            Ok(Which::Return(_)) => {
                self.state.recv_return(&mut self.outbound, incoming.into_owned().message)
            },
            Ok(Which::Finish(finish)) => {
                let finish = finish.try_get()?;
                self.state.recv_finish(&mut self.outbound, finish.question_id(), finish.release_result_caps())
            }
            Ok(Which::Resolve(resolve)) => {
                let resolve = resolve.try_get()?;
                self.state.recv_resolve(&mut self.outbound, &resolve)
            }
            Ok(Which::Release(release)) => {
                let release = release.try_get()?;
                self.state.recv_release(release.id(), release.reference_count())
            }
            Ok(Which::Disembargo(disembargo)) => {
                let disembargo = disembargo.try_get()?;
                self.state.recv_disembargo(&disembargo)
            }
            _ => {
                let size = AllocLen::new(message.size_in_words() as u32 + 2).unwrap_or(AllocLen::MIN);
                let mut message = self.outbound.new_estimated(size);
                message.builder().init_struct_root::<rpc_capnp::Message>()
                    .into_unimplemented()
                    .try_set(&reader, ReturnErrors)?;

                self.outbound.send(OutboundMessage { message });

                Ok(())
            }
        };

        match res {
            Ok(()) => Ok(()),
            Err(Fatal(err) | Recoverable(err)) => Err(self.close(err)),
        }
    }

    /// Closes the connection and forces all dependencies to fail.
    ///
    /// This will be called if any tasks have a fatal error, or can be called
    /// by the network if a message fails to be read.
    ///
    /// If the connection has already been closed, this returns an Err with the
    /// original error that caused it to close.
    ///
    /// After calling this, messages may still be sent out on the connection.
    /// If possible, the system should attempt to send these messages, as the
    /// last message sent will be an Abort to the other side. However, it's acceptable
    /// if this fails.
    pub fn close(&mut self, err: Error) -> Error {
        self.state.close(&mut self.outbound, err)
    }
}

/// A consumed closed connection. This allows you to handle processing vestigial events that may
/// be received after the connection is closed.
pub struct ClosedConnection {
    err: Error,
    conn_events: EventReceiver,
}

impl ClosedConnection {
    pub fn error(&self) -> &Error {
        &self.err
    }

    pub async fn cleanup(mut self) {
        while let Some(event) = self.conn_events.recv().await {
            match event {
                ConnectionEvent::PipelineStarted { channel, .. }
                    => channel.close(self.err.clone()),
            }
        }
    }
}

#[cfg(test)]
mod test {
}