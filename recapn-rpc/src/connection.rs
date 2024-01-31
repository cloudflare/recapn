//! An RPC connection primitive.

use fnv::FnvHashMap;
use recapn::alloc::{AllocLen, ObjectLen};
use recapn::field::Struct;
use recapn::{any, list, BuilderOf, NotInSchema, ReaderOf};
use recapn::message::{Message, ReaderOptions, SegmentSource};
use tokio::select;
use tokio::sync::oneshot;
use tokio::sync::mpsc as tokio_mpsc;

use std::borrow::Cow;
use std::cmp::Reverse;
use std::collections::btree_map::{OccupiedEntry, VacantEntry, Entry};
use std::collections::{BTreeMap, BinaryHeap};
use std::convert::{TryFrom, Infallible as Never};
use std::sync::Arc;
use std::fmt::Write;
use crate::sync::request::{PipelineResolver, RequestUsage};
use crate::sync::{mpsc, request};
use crate::{Error, PipelineOp, ErrorKind};
use crate::client::{RpcClient, RpcCall, RpcResults, Client, ResponseTarget, LocalMessage, ExternalMessage, SetPipeline, RpcResponse};
use crate::gen::capnp_rpc_capnp as rpc_capnp;

type RpcSender = mpsc::Sender<RpcClient>;
type RpcReceiver = mpsc::Receiver<RpcClient>;

fn exception_to_error(ex: &ReaderOf<rpc_capnp::Exception>) -> Error {
    let mut msg = String::new();
    let kind = match ex.r#type() {
        Ok(t) => t,
        Err(NotInSchema(num)) => {
            let _ = write!(msg, "(unknown exception type '{num}') ");
            ErrorKind::Failed
        },
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

type OpsList<'a> = list::Reader<'a, recapn::field::Struct<rpc_capnp::promised_answer::Op>>;
fn to_pipeline_ops(list: OpsList) -> Result<Vec<PipelineOp>, Error> {
    list.into_iter().filter_map(|op| {
        use rpc_capnp::promised_answer::op::Which;
        match op.which() {
            Ok(Which::Noop(())) => None, // does nothing
            Ok(Which::GetPointerField(field)) => Some(Ok(PipelineOp::PtrField(field))),
            Err(NotInSchema(variant)) =>
                Some(Err(Error::failed(format!("Unsupported pipeline op: {variant}")))),
        }
    }).collect()
}

type QuestionId = u32;
type AnswerId = QuestionId;
type ExportId = u32;
type ImportId = ExportId;

struct ExportTable<T> {
    slots: Vec<Option<T>>,
    free_slots: BinaryHeap<Reverse<u32>>,
}

impl<T> ExportTable<T> {
    pub fn get(&self, id: u32) -> Option<&T> {
        self.slots.get(id as usize).map(Option::as_ref).flatten()
    }

    pub fn get_mut(&mut self, id: u32) -> Option<&mut T> {
        self.slots.get_mut(id as usize).map(Option::as_mut).flatten()
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

    pub fn remove(&mut self, id: u32) -> Option<T> {
        let result = self.slots.get_mut(id as usize).map(Option::take).flatten();
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
    pub fn insert(&mut self, id: u32, value: T) -> Result<(), T> {
        if id < IMPORT_LOW_SLOTS {
            let slot = &mut self.low[id as usize];
            if slot.is_none() {
                *slot = Some(value);
            } else {
                return Err(value)
            }
        } else {
            match self.high.entry(id) {
                Entry::Occupied(_) => return Err(value),
                Entry::Vacant(v) => {
                    v.insert(value);
                }
            }
        }

        Ok(())
    }

    pub fn remove(&mut self, id: u32) -> Option<T> {
        todo!()
    }
}

/// A source of messages for the RPC system.
/// 
/// Since messages can be created anywhere, we don't assume outbound messages have
/// a defined allocator or take any specific form. Instead, a message outbound consumes
/// a boxed message with any allocator.
pub trait MessageFactory {
    /// Creates a new message.
    fn new(&self) -> LocalMessage;

    /// Creates a new message, with the given estimated size.
    /// 
    /// By default, this just calls `new()`
    fn new_estimated(&self, _: AllocLen) -> LocalMessage { self.new() }
}

pub(crate) struct ConnectionId(EventSender);

impl PartialEq for ConnectionId {
    fn eq(&self, other: &Self) -> bool {
        self.0.same_channel(&other.0)
    }
}

impl Eq for ConnectionId {}

pub(crate) struct QuestionTarget {
    conn: ConnectionId,
    question: QuestionId,
}

#[non_exhaustive]
pub struct OutboundMessage {
    pub message: LocalMessage,
}

pub trait MessageOutbound: MessageFactory {
    /// Send a message, or queue it to send later.
    fn send(&self, msg: OutboundMessage);
}

pub struct OwnedIncomingMessage {
    pub message: ExternalMessage,
}

pub trait IncomingMessage: SegmentSource {
    fn total_size(&self) -> ObjectLen;
    fn into_owned(self) -> OwnedIncomingMessage;
}

pub(crate) struct QuestionPipeline {
    question: QuestionId,
    events: EventSender,
}

impl PipelineResolver<RpcClient> for QuestionPipeline {
    fn resolve(&self, key: Box<[PipelineOp]>, channel: mpsc::Receiver<RpcClient>) {
        let _ = self.events.send(ConnectionEvent::Pipeline {
            question: self.question,
            ops: key,
            receiver: channel,
        });
    }
    fn pipeline(&self, key: Box<[PipelineOp]>) -> mpsc::Sender<RpcClient> {
        let (sender, channel) = mpsc::channel(RpcClient::RemotePipeline);
        self.resolve(key, channel);
        sender
    }
}

struct Export {
    ref_count: u32,

    client: RpcSender,

    resolve: Option<()>,
}

enum Question {
    /// A highly simplified question for a bootstrap request.
    /// 
    /// Since we know this is for a bootstrap request, we know
    /// that outgoing pipelined requests must take a specific form
    /// and target the bootstrap capability stored for the connection.
    Bootstrap {
        resolver: oneshot::Sender<CapResolution>,
    },
    /// An answered bootstrap request
    AnsweredBootstrap {
        client: Client,
    },
    /// A normal call.
    Call {
        /// A channel to send the response
        response: Option<oneshot::Sender<Result<RpcResponse, Error>>>,
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
        /// The export associated with this answer (if any)
        export: Option<ExportId>,

        /// The client associated with this answer
        client: RpcSender,
    },
}

/// Connection events that don't need to be completed in a specific order relative to
/// outgoing messages. This includes things like creating new pipeline clients, marking
/// questions as finished, or releasing imports.
pub(crate) enum ConnectionEvent {
    /// A question event used to make a new pipeline client.
    Pipeline {
        question: QuestionId,
        ops: Box<[PipelineOp]>,
        receiver: RpcReceiver,
    },
    /// Used to indicate that a question isn't needed anymore because
    /// all the pipeline builders and response receivers were dropped.
    /// 
    /// This may not immediately mark the question as finished remotely.
    /// If any pipelines are still active the question will stay up.
    QuestionFinished(QuestionId),
}

pub(crate) type EventSender = tokio_mpsc::UnboundedSender<ConnectionEvent>;
type EventReceiver = tokio_mpsc::UnboundedReceiver<ConnectionEvent>;

#[non_exhaustive]
pub struct ConnectionOptions {
    /// Controls the size of the internal connection event buffer.
    /// 
    /// Larger buffers can handle more events at once, but may starve
    /// other connection operations in the event of contention.
    /// 
    /// The default is 8.
    pub connection_event_buffer_size: usize,

    /// Controls the size of the internal cap message buffer.
    /// 
    /// Larger buffers can handle more outgoing cap messages at once, but
    /// may starve other connection operations in the event of contention.
    /// 
    /// The default is 16.
    pub cap_message_buffer_size: usize,

    /// Controls whether pipeline optimization hints are used. If this is true
    /// the other party must also use pipeline hints as well.
    pub use_pipeline_hints: bool,

    /// The options to use when reading incoming messages
    pub reader_options: ReaderOptions,
}

impl Default for ConnectionOptions {
    fn default() -> Self {
        Self {
            connection_event_buffer_size: 8,
            cap_message_buffer_size: 16,
            use_pipeline_hints: false,
            reader_options: ReaderOptions::default(),
        }
    }
}

pub struct Connection<T: ?Sized> {
    exports: ExportTable<Export>,
    questions: ExportTable<Question>,
    answers: ImportTable<Answer>,
    imports: ImportTable<Import>,

    /// A back-ref from a client in the export table to the export ID of the client
    export_by_client: FnvHashMap<RpcSender, ExportId>,

    conn_events_sender: EventSender,
    conn_events: EventReceiver,
    conn_event_buf: Vec<ConnectionEvent>,

    cap_messages_sender: tokio_mpsc::Sender<CapMessage>,
    cap_messages: tokio_mpsc::Receiver<CapMessage>,
    cap_message_buf: Vec<CapMessage>,

    call_words_in_flight: usize,

    disconnect: Option<Error>,

    bootstrap: RpcSender,
    options: ConnectionOptions,
    outbound: T,
}

impl<T> Connection<T> {
    pub fn new(outbound: T, bootstrap: Client, options: ConnectionOptions) -> Self {
        todo!()
    }
}

impl<T: MessageOutbound + ?Sized> Connection<T> {
    /// Returns a client for interacting with the bootstrap capability of the connection.
    pub fn bootstrap(&mut self) -> Client {
        if let Some(err) = &self.disconnect {
            return Client::broken(err.clone())
        }

        let (resolver, resolve_receiver) = oneshot::channel();
        let (id, _) = self.questions.push(Question::Bootstrap { resolver });

        // Send our bootstrap message.
        let mut message = self.outbound.new();
        let mut builder = message.builder().init_struct_root::<rpc_capnp::Message>();
        let mut bootstrap = builder.bootstrap().init();
        bootstrap.question_id().set(id);

        self.outbound.send(OutboundMessage { message });

        // Now configure our cap handler to forward pipelined messages.
        let (client, rpc_receiver) = Client::new(RpcClient::Bootstrap);
        let cap_handler = ReceivedCap {
            target: CapTarget::bootstrap(id),
            resolution: resolve_receiver,

            inbound: rpc_receiver,
            outbound: self.cap_messages_sender.clone(),
        };

        tokio::spawn(cap_handler.handle());

        client
    }

    /// Gets the number of Words involved in incoming active calls being processed by this
    /// connection.
    /// 
    /// This can be used to implement a control flow limit on the connection to avoid being
    /// overwhelmed by requests from the other party.
    pub fn call_words_in_flight(&self) -> usize {
        self.call_words_in_flight
    }

    /// Handles a set of internal events and outgoing messages, then returns. If an error is
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
    pub async fn handle_events(&mut self) -> Result<(), Error> {
        assert!(self.conn_event_buf.is_empty());
        assert!(self.cap_message_buf.is_empty());

        let conn_limit = self.options.connection_event_buffer_size;
        let cap_limit = self.options.cap_message_buffer_size;

        let conn_buf = &mut self.conn_event_buf;
        let cap_buf = &mut self.cap_message_buf;

        select! {
            _ = self.conn_events.recv_many(conn_buf, conn_limit) => {
                // Take the connection event buffer away while we drain it to retain capacity
                let mut conn_buf = std::mem::take(conn_buf);
                for event in conn_buf.drain(..) {
                    self.handle_event(event)
                }
                // Put the buffer back so it'll get reused.
                self.conn_event_buf = conn_buf;
            }
            _ = self.cap_messages.recv_many(cap_buf, cap_limit) => {
                let mut cap_buf = std::mem::take(cap_buf);
                for msg in cap_buf.drain(..) {
                    if let Err(err) = self.handle_cap_message(msg) {
                        // Only track the first error
                        if self.disconnect.is_none() {
                            self.disconnect = Some(err)
                        }
                    }
                }
                self.cap_message_buf = cap_buf;
            }
        }

        self.handle_close()
    }

    fn handle_cap_message(&mut self, msg: CapMessage) -> Result<(), Error> {
        match msg.event {
            CapEvent::Call(call) => self.send_call(msg.target, call),
            CapEvent::Disembargo { inbound, outbound } => {

            }
            CapEvent::Finished => {

            }
        }
        Ok(())
    }

    fn send_call(&mut self, target: CapTarget, req: request::Request<RpcClient>) {
        // If the request is already canceled just drop it
        if req.is_closed() {
            return
        }

        // Set up our piplining hints.
        let usage = req.usage();
        let no_promise_pipelining = usage == RequestUsage::Response && self.options.use_pipeline_hints;
        let only_promise_pipelining = usage == RequestUsage::Pipeline && self.options.use_pipeline_hints;

        let (request, responder) = req.respond();
        if let Some(err) = &self.disconnect {
            responder.respond(RpcResults::Owned(Err(err.clone())));
            return
        }

        // Set up all the common Call fields like interface, method, target, payload, etc.
        let mut outgoing = self.outbound.new();
        let mut message = outgoing.builder().init_struct_root::<rpc_capnp::Message>();
        let mut call = message.call().init();

        call.interface_id().set(request.interface);
        call.method_id().set(request.method);
        call.no_promise_pipelining().set(no_promise_pipelining);
        call.only_promise_pipeline().set(only_promise_pipelining);

        let mut msg_target = call.target().init();
        match target {
            CapTarget::Import(import) => msg_target.imported_cap().set(import),
            CapTarget::PromisedAnswer(question_id, ops_id) => {
                let mut msg_promised = msg_target.promised_answer().init();
                let local_question = self.questions.get(question_id);

                todo!()
            }
        }

        // todo: fill in params

        if let ResponseTarget::Remote(response_to) = &request.target {
            if response_to.conn.0.same_channel(&self.conn_events_sender) {
                // This is a reflected call! The call is being sent back over the connection to
                // where the results are needed. This means we can optimize this operation to make
                // sure the other end doesn't send us the results, which would require reflecting
                // those too.

                // Configure the results to be sent to "yourself" (them) instead of "caller" (us).
                call.send_results_to().yourself().set();

                // todo Set up the local question

                self.outbound.send(OutboundMessage { message: outgoing });

                // Now, configure the request from the inbound call to to say we've already
                // returned and send a return message for it.

                let mut outgoing = self.outbound.new();
                let mut message = outgoing.builder().init_struct_root::<rpc_capnp::Message>();
                let mut ret = message.r#return().init();

                ret.answer_id().set(response_to.question);
                ret.release_param_caps().set(false);
                ret.no_finish_needed().set(true);
                ret.take_from_other_question().set(0);

                self.outbound.send(OutboundMessage { message: outgoing });

                return
            }

            // todo(level 3): support sendResultsTo.thirdParty
        }

        // This is a call we will have to handle locally.

        let events = self.conn_events_sender.clone();
        let (result_send_shot, result_recv_shot) = oneshot::channel();

        let (id, _) = self.questions.push(Question::Call { response: Some(result_send_shot) });

        let pipeline = QuestionPipeline { question: id, events: events.clone() };
        let results = responder.set_pipeline(SetPipeline::RemotePipeline(pipeline));

        tokio::spawn(async move {
            let mut r = results;
            let events = events.clone();
            select! {
                Ok(result) = result_recv_shot => {
                    r.respond(RpcResults::Owned(result));
                }
                _ = r.closed() => {}
            }
            let _ = events.send(ConnectionEvent::QuestionFinished(id));
        });
    }

    fn handle_event(&mut self, incoming: ConnectionEvent) {
        todo!()
    }

    /// Handles cleaning up the connection after it has been closed.
    /// 
    /// If it hasn't been closed, this does nothing.
    fn handle_close(&mut self) -> Result<(), Error> {
        todo!()
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
    pub fn try_handle_message(&mut self, incoming: impl IncomingMessage) -> Result<(), Error> {
        let reader = recapn::message::Reader::new(&incoming, self.options.reader_options.clone());
        let reader = reader.read_as_struct::<rpc_capnp::Message>();

        use rpc_capnp::message::Which;
        match reader.which() {
            Ok(Which::Unimplemented(unimplemented)) => {
                todo!()
            }
            Ok(Which::Abort(abort)) => {
                Err(exception_to_error(&abort.try_get()?))
            },
            Ok(Which::Bootstrap(bootstrap)) => {
                self.handle_bootstrap(bootstrap.try_get()?.question_id())
            },
            Ok(Which::Call(call)) => {
                self.handle_call(incoming.into_owned().message)
            }
            Ok(Which::Return(ret)) => {
                todo!()
            }
            Ok(Which::Finish(finish)) => {
                todo!()
            }
            Ok(Which::Resolve(resolve)) => {
                todo!()
            }
            Ok(Which::Release(release))=> {
                todo!()
            }
            Ok(Which::Disembargo(disembargo)) => {
                todo!()
            }
            _ => {
                let size = AllocLen::new(incoming.total_size().get()).unwrap_or(AllocLen::MIN);
                let mut message = self.outbound.new_estimated(size);
                let mut builder = message.builder().init_struct_root::<rpc_capnp::Message>();
                builder.unimplemented().set(&reader);
    
                self.outbound.send(OutboundMessage { message });
    
                todo!()
            }
        }
    }

    fn handle_bootstrap(&mut self, answer: AnswerId) -> Result<(), Error> {
        let mut response = self.outbound.new();
        let mut message = response.builder().init_struct_root::<rpc_capnp::Message>();
        let mut ret = message.r#return().init();
        ret.answer_id().set(answer);

        let mut results = ret.results().init();
        let mut table = results.cap_table().init(1);
        let mut descriptor = table.at(0).get();
        let client = self.bootstrap.most_resolved().0.clone();
        let export = self.write_cap_descriptor(&client, &mut descriptor);

        results.content().as_mut().set_capability(0);

        let answer_value = Answer::Bootstrap { export, client };

        let Ok(()) = self.answers.insert(answer, answer_value) else {
            return Err(Error::failed("questionId is already in use"))
        };

        self.outbound.send(OutboundMessage { message: response });

        Ok(())
    }

    fn handle_call(&mut self, message: ExternalMessage) -> Result<(), Error> {
        let reader = recapn::message::Reader::new(&*message, self.options.reader_options.clone());
        let message = reader.read_as_struct::<rpc_capnp::Message>();
        let call = message.call().field().unwrap().try_get()?;

        let target = self.read_message_target(&call.target().try_get()?)?;

        let payload = call.params().try_get()?;
        let cap_table = self.read_cap_descriptors(payload.cap_table().try_get()?);

        let interface_id = call.interface_id();
        let method_id = call.method_id();

        use rpc_capnp::call::send_results_to::Which;
        let redirect_results = match call.send_results_to().which() {
            Ok(Which::Caller(())) => false,
            Ok(Which::Yourself(())) => true,
            _ => return Err(Error::failed("Unsupported `Call.sendResultsTo`.")),
        };

        todo!()
    }

    fn handle_return(&mut self, message: OwnedIncomingMessage) -> Result<(), Error> {
        todo!()
    }

    fn write_cap_descriptor(&mut self, cap: &RpcSender, descriptor: &mut BuilderOf<rpc_capnp::CapDescriptor>) -> Option<ExportId> {
        let (id, export) = if let Some(&existing) = self.export_by_client.get(cap) {
            let export = self.exports.get_mut(existing)
                .expect("missing export from by client table");
            export.ref_count += 1;
            (existing, export)
        } else {
            // This is the first time we've seen this capability.
            self.exports.push(Export { ref_count: 1, client: cap.clone(), resolve: None })
        };

        todo!()
    }

    fn read_cap_descriptor(&mut self, descriptor: &ReaderOf<rpc_capnp::CapDescriptor>) -> Option<RpcSender> {
        use rpc_capnp::cap_descriptor::Which;
        match descriptor.which() {
            Ok(Which::None(())) => None,
            Ok(Which::SenderHosted(id)) => Some(self.import(id, false)),
            Ok(Which::SenderPromise(id)) => Some(self.import(id, true)),
            Ok(Which::ReceiverHosted(hosted)) => {
                todo!()
            }
            Ok(Which::ReceiverAnswer(answer)) => {
                todo!()
            }
            Ok(Which::ThirdPartyHosted(third_party)) => {
                todo!()
            }
            Err(NotInSchema(variant)) => {
                todo!()
            }
        }
    }

    fn read_cap_descriptors(&mut self, table: list::Reader<Struct<rpc_capnp::CapDescriptor>>) -> Vec<Option<Client>> {
        table.into_iter()
            .map(|d| self.read_cap_descriptor(&d).map(|s| Client { sender: s }))
            .collect()
    }

    fn import(&mut self, id: ImportId, promise: bool) -> RpcSender {
        todo!()
    }

    fn read_message_target(&mut self, target: &ReaderOf<rpc_capnp::MessageTarget>) -> Result<Client, Error> {
        let which = target.which()
            .map_err(|NotInSchema(v)| Error::failed(format!("Unknown message target type: {v}")))?;
        use rpc_capnp::message_target::Which;
        match which {
            Which::ImportedCap(cap) =>
                if let Some(export) = self.exports.get(cap) {
                    Ok(Client { sender: export.client.clone() })
                } else {
                    Err(Error::failed("Message target is not a current export ID."))
                },
            Which::PromisedAnswer(promised_answer) => {
                let promised_answer = promised_answer.try_get()?;
                let ops = promised_answer.transform().try_get()?;

                let client = match self.answers.get(promised_answer.question_id()) {
                    Some(Answer::Bootstrap { client, .. }) if ops.len() == 0 => {
                        // This is targeting their bootstrap request! Make sure there are no ops in
                        // the ops list and just return the client specified by the export.

                        Client { sender: client.clone() }
                    }
                    _ => Client::broken(Error::failed("Invalid pipeline call"))
                };

                Ok(client)
            }
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
        if self.disconnect.is_none() {
            self.disconnect = Some(err);
        }

        self.handle_close().unwrap_err()
    }

    /// Closes the connection but don't attempt to clean up active state.
    pub fn close_fast(&mut self, err: Error) -> Error {
        self.disconnect.get_or_insert(err).clone()
    }
}

type PipelineId = usize;

#[derive(Clone, Copy, Debug, PartialEq, Eq)]
enum CapTarget {
    Import(ImportId),
    PromisedAnswer(QuestionId, PipelineId),
}

impl CapTarget {
    pub fn bootstrap(question: QuestionId) -> Self {
        Self::PromisedAnswer(question, 0)
    }
}

/// A capability event.
enum CapEvent {
    /// A call forwarded to the connection.
    Call(request::Request<RpcClient>),
    /// The cap handler sent a message to the connection so it couldn't automatically resolve
    /// the channel. When the connection receives this, it might send a disembargo to the other
    /// party, or just handle the resolution itself.
    Disembargo {
        inbound: RpcReceiver,
        outbound: RpcSender,
    },
    /// The underlying channel has closed and all messages have been sent.
    Finished,
}

/// An outbound cap event for the connection to handle
struct CapMessage {
    target: CapTarget,
    event: CapEvent,
}

enum CapResolution {
    /// The cap resolved into an error
    Broken(Error),
    Resolve(RpcSender),
}

/// Handles forwarding messages from an internal mpsc into a connection mpsc.
struct ReceivedCap {
    target: CapTarget,
    resolution: oneshot::Receiver<CapResolution>,

    inbound: RpcReceiver,
    outbound: tokio_mpsc::Sender<CapMessage>,
}

impl ReceivedCap {
    pub async fn handle(mut self) {
        let mut handled_message = false;
        loop {
            select! {
                Ok(resolve) = &mut self.resolution => {
                    match resolve {
                        CapResolution::Broken(err) => self.inbound.close(err),
                        CapResolution::Resolve(client) if !handled_message => {
                            // We haven't handled a message yet, so we can just forward directly
                            // to the new client
                            if let Err(this) = self.inbound.forward_to(&client) {
                                this.close(Error::failed("resolved a capability into itself"))
                            }
                        }
                        CapResolution::Resolve(client) => {
                            let msg = CapMessage {
                                target: self.target,
                                event: CapEvent::Disembargo {
                                    inbound: self.inbound,
                                    outbound: client,
                                },
                            };
                            if let Err(err) = self.outbound.send(msg).await {
                                // If the connection event channel is closed, assume the resolved
                                // client is broken too, so resolve to it immediately.
                                //
                                // This should communicate state better than if we closed with a
                                // generic error, since the resolved client should have the error
                                // of the closed connection.
                                let CapEvent::Disembargo { inbound, outbound } = err.0.event else {
                                    unreachable!()
                                };
                                if let Err(this) = inbound.forward_to(&outbound) {
                                    this.close(Error::failed("resolved a capability into itself"))
                                }
                            }
                        }
                    }
                    break
                }
                msg = self.inbound.recv() => match msg {
                    Some(msg) => {
                        handled_message = true;
                        let cap_msg = CapMessage {
                            target: self.target,
                            event: CapEvent::Call(msg),
                        };
                        if let Err(err) = self.outbound.send(cap_msg).await {
                            todo!()
                        }
                    }
                    None => {
                        let _ = self.outbound.send(CapMessage {
                            target: self.target,
                            event: CapEvent::Finished,
                        }).await;
                        break
                    }
                },
                // If we don't receive a resolution, we assume we've been disconnected
                // and just need to exit.
                else => break,
            }
        }
    }
}

struct Import {
    resolver: oneshot::Sender<CapResolution>,
    client: mpsc::Sender<RpcClient>,
}