//! An RPC connection primitive.

use fnv::FnvHashMap;
use recapn::alloc::AllocLen;
use recapn::any::AnyPtr;
use recapn::arena::ReadArena;
use recapn::field::Struct;
use recapn::message::{BuilderParts, ReaderOptions};
use recapn::orphan::Orphan;
use recapn::ptr::{ElementSize, ReturnErrors};
use recapn::rpc::Capable;
use recapn::{list, message, BuilderOf, NotInSchema, ReaderOf};
use recapn_channel::request::{RequestUsage, ResponseReceiverFactory};
use recapn_channel::{mpsc, request, PipelineResolver};


use crate::chan::{
    self, ExternalMessage, LocalMessage, MessagePayload, Params, ParamsRoot, ResponseTarget,
    RpcChannel, RpcResults, SetPipeline,
};
use crate::client::Client;
use crate::gen::capnp_rpc_capnp as rpc_capnp;
use crate::pipeline::PipelineOp;
use crate::rt::{Receiver, Sender};
use crate::table::{CapTable, Table};
use crate::{Error, ErrorKind};
use rpc_capnp::promised_answer::Op;
use rpc_capnp::CapDescriptor;
use std::borrow::Cow;
use std::cmp::Reverse;
use std::collections::btree_map::Entry;
use std::collections::{BTreeMap, BinaryHeap};
use std::convert::TryFrom;
use std::fmt::Write;
use std::mem::replace;
use std::sync::Arc;

const MAX_OPS: usize = ElementSize::size_of::<Struct<Op>>().max_elements().get() as usize;
const MAX_CAPS: usize = ElementSize::size_of::<Struct<CapDescriptor>>()
    .max_elements()
    .get() as usize;

fn null_cap_error() -> Error {
    Error::failed("called null capability")
}

fn exception_to_error(ex: &ReaderOf<rpc_capnp::Exception>) -> Error {
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

type OpsList<'a> = list::StructListReader<'a, rpc_capnp::promised_answer::Op>;
fn to_pipeline_ops(list: OpsList) -> Result<Vec<PipelineOp>, Error> {
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

pub(crate) type QuestionId = u32;
pub(crate) type AnswerId = QuestionId;
pub(crate) type ExportId = u32;
pub(crate) type ImportId = ExportId;

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

    pub fn get(&self, id: u32) -> Option<&T> {
        self.slots.get(id as usize).and_then(Option::as_ref)
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
                return Err(value);
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

struct FreeVec<T> {
    slots: Vec<Option<T>>,
    free_slots: BinaryHeap<Reverse<usize>>,
}

impl<T> FreeVec<T> {
    pub fn new() -> Self {
        Self {
            slots: Vec::new(),
            free_slots: BinaryHeap::new(),
        }
    }

    pub fn get(&self, idx: usize) -> Option<&T> {
        self.slots.get(idx).and_then(Option::as_ref)
    }

    pub fn get_mut(&mut self, idx: usize) -> Option<&mut T> {
        self.slots.get_mut(idx).and_then(Option::as_mut)
    }

    pub fn push(&mut self, value: T) -> (usize, &mut T) {
        let slot = match self.free_slots.pop() {
            Some(Reverse(free_slot)) => free_slot,
            None => {
                let len = self.slots.len();
                self.slots.push(None);
                len
            }
        };
        let s = &mut self.slots[slot];
        debug_assert!(s.is_none());
        let v = s.insert(value);
        (slot, v)
    }

    pub fn len(&self) -> usize {
        self.slots.len() - self.free_slots.len()
    }

    pub fn is_empty(&self) -> bool {
        self.len() == 0
    }

    pub fn remove(&mut self, idx: usize) -> Option<T> {
        let result = self.slots.get_mut(idx).and_then(Option::take);
        if result.is_some() {
            self.free_slots.push(Reverse(idx));
        }
        result
    }

    pub fn take_all(self) -> impl Iterator<Item = T> {
        self.slots.into_iter().flatten()
    }
}

/// A source of messages for the RPC system.
///
/// Since messages can be created anywhere, we don't assume outbound messages have
/// a defined allocator or take any specific form. Instead, a message outbound consumes
/// a boxed message with any allocator.
pub trait MessageFactory {
    /// Creates a new message.
    #[allow(clippy::wrong_self_convention)] // object safety
    fn new(&self) -> LocalMessage;

    /// Creates a new message, with the given estimated size.
    ///
    /// By default, this just calls `new()`
    fn new_estimated(&self, _: AllocLen) -> LocalMessage {
        self.new()
    }
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

pub trait IncomingMessage {
    fn message(&self) -> &dyn ReadArena;
    fn into_owned(self) -> OwnedIncomingMessage;
}

pub(crate) struct QuestionPipeline {
    question: QuestionId,
    events: EventSender,
}

impl PipelineResolver<RpcChannel> for QuestionPipeline {
    fn resolve(
        &self,
        recv: ResponseReceiverFactory<RpcChannel>,
        key: Arc<[PipelineOp]>,
        channel: chan::Receiver,
    ) {
        todo!()
    }
    fn pipeline(
        &self,
        recv: ResponseReceiverFactory<RpcChannel>,
        key: Arc<[PipelineOp]>,
    ) -> chan::Sender {
        let (sender, channel) = mpsc::channel(RpcChannel::RemotePipeline);
        self.resolve(recv, key, channel);
        sender
    }
}

struct Export {
    ref_count: u32,

    client: chan::Sender,

    resolve: Option<()>,
}

#[derive(Debug)]
pub(crate) struct ImportClient {
    id: ImportId,
    sender: EventSender,
}

impl Drop for ImportClient {
    fn drop(&mut self) {
        let _ = self.sender.send(ConnectionEvent::ImportReleased(self.id));
    }
}

enum Question {
    /// An incomplete question that isn't finished getting built yet or is being modified.
    /// This can be encountered if the task processing messages ignores RPC errors and doesn't
    /// close the connection.
    Incomplete,
    /// A highly simplified question for a bootstrap request.
    ///
    /// Since we know this is for a bootstrap request, we know
    /// that outgoing pipelined requests must take a specific form
    /// and target the bootstrap capability stored for the connection.
    Bootstrap {
        resolver: crate::tokio::oneshot::Sender<CapResolution>,
    },
    /// An answered bootstrap request
    AnsweredBootstrap,
    /// A normal call in progress.
    Call {
        /// A channel to send the response to.
        response: request::ResultsSender<RpcChannel>,
        /// The number of active pipelines with this call
        pipelines_count: usize,
        /// Indicates whether all response receivers have been dropped for this request.
        finished: bool,
    },
    /// A call that has returned from the other side
    ReturnedCall,
    /// A call which was made back to the caller. We won't receive the response
    /// for it, instead we'll get a return saying "results sent elsewhere".
    Reflected,
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
        client: chan::Sender,
    },
}

/// Connection events that don't need to be completed in a specific order relative to
/// outgoing messages. This includes things like marking questions as finished or releasing
/// imports.
pub(crate) enum ConnectionEvent {
    /// Used to indicate that a question isn't needed anymore because
    /// all the pipeline builders and response receivers were dropped.
    ///
    /// This may not immediately mark the question as finished remotely.
    /// If any pipelines are still active the question will stay up.
    QuestionFinished(QuestionId),
    /// Indicates an import client has been dropped.
    ImportReleased(ImportId),
}

pub(crate) type EventSender = crate::tokio::UnboundedSender<ConnectionEvent>;
type EventReceiver = crate::tokio::UnboundedReceiver<ConnectionEvent>;

#[non_exhaustive]
#[derive(Default)]
pub struct ConnectionOptions {
    /// Controls whether pipeline optimization hints are used. If this is true
    /// the other party must also use pipeline hints as well.
    pub use_pipeline_hints: bool,

    /// The options to use when reading incoming messages
    pub reader_options: ReaderOptions,
}

pub struct Connection<T: ?Sized, Rt: crate::rt::Rt> {
    exports: ExportTable<Export>,
    questions: ExportTable<Question>,
    answers: ImportTable<Answer>,
    imports: ImportTable<Import>,

    /// A back-ref from a client in the export table to the export ID of the client
    export_by_client: FnvHashMap<chan::Sender, ExportId>,

    conn_events_sender: EventSender,
    conn_events: EventReceiver,

    cap_messages_sender: Rt::Sender<CapMessage>,
    cap_messages: Rt::Receiver<CapMessage>,

    call_words_in_flight: usize,

    disconnect: Option<Error>,

    bootstrap: chan::Sender,
    options: ConnectionOptions,
    outbound: T,
}

impl<T, Rt: crate::rt::Rt> Connection<T, Rt> {
    pub fn new(outbound: T, bootstrap: Client, options: ConnectionOptions) -> Self {
        let (conn_events_sender, conn_events) = crate::tokio::unbounded_channel();
        let (cap_messages_sender, cap_messages) = Rt::channel(8);
        Connection {
            exports: ExportTable::new(),
            questions: ExportTable::new(),
            answers: ImportTable::new(),
            imports: ImportTable::new(),
            export_by_client: FnvHashMap::default(),
            conn_events_sender,
            conn_events,
            cap_messages_sender,
            cap_messages,
            call_words_in_flight: 0,
            disconnect: None,
            bootstrap: bootstrap.0,
            options,
            outbound,
        }
    }
}

impl<T: MessageOutbound + ?Sized, Rt: crate::rt::Rt + 'static> Connection<T, Rt> {
    #[inline]
    fn same_connection(&self, id: &ConnectionId) -> bool {
        id.0.same_channel(&self.conn_events_sender)
    }

    /// Returns a client for interacting with the bootstrap capability of the connection.
    pub fn bootstrap(&mut self) -> Client {
        if let Some(err) = &self.disconnect {
            return Client::broken(err.clone());
        }

        let (resolver, resolve_receiver) = crate::tokio::oneshot::channel();
        let (id, _) = self.questions.push(Question::Bootstrap { resolver });

        // Send our bootstrap message.
        let mut message = self.outbound.new();
        let mut builder = message.builder().init_struct_root::<rpc_capnp::Message>();
        let mut bootstrap = builder.bootstrap().init();
        bootstrap.question_id().set(id);

        self.outbound.send(OutboundMessage { message });

        let (sender, receiver) = mpsc::channel(chan::RpcChannel::Bootstrap(id));
        let cap_handler = ReceivedCap::<Rt> {
            target: CapTarget::bootstrap(id),
            resolution: resolve_receiver,

            inbound: receiver,
            outbound: self.cap_messages_sender.clone(),
        };

        let fut = cap_handler.handle();
        crate::tokio::spawn(fut);

        Client(sender)
    }

    /// Gets the number of Words involved in incoming active calls being processed by this
    /// connection.
    ///
    /// This can be used to implement a control flow limit on the connection to avoid being
    /// overwhelmed by requests from the other party.
    pub fn call_words_in_flight(&self) -> usize {
        self.call_words_in_flight
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
    /// This function is cancel safe. If `handle_events` is used as the future in a crate::tokio::select!
    /// statement and some other branch completes first, it is guaranteed that no messages
    /// were received and dropped.
    pub async fn handle_event(&mut self) -> Result<(), Error> {
        crate::tokio::select! {
            Some(event) = self.conn_events.recv() => {
                self.handle_conn_event(event)
            }
            Some(msg) = self.cap_messages.recv() => {
                if let Err(err) = self.handle_cap_message(msg) {
                    // Only track the first error
                    if self.disconnect.is_none() {
                        self.disconnect = Some(err)
                    }
                }
            }
        }

        self.handle_close()
    }

    fn handle_cap_message(&mut self, msg: CapMessage) -> Result<(), Error> {
        match msg.event {
            CapEvent::Call(chan::Item::Request(call)) => self.send_call(msg.target, call),
            CapEvent::Call(chan::Item::Event(_)) => todo!(),
            CapEvent::Disembargo { inbound, outbound } => {}
            CapEvent::Finished => {}
        }
        Ok(())
    }

    fn send_call(&mut self, target: CapTarget, req: chan::Request) {
        // If the request is already canceled just drop it
        if req.is_finished() {
            return;
        }

        // Set up our piplining hints.
        let usage = req.usage();
        let no_promise_pipelining =
            usage == RequestUsage::Response && self.options.use_pipeline_hints;
        let only_promise_pipelining =
            usage == RequestUsage::Pipeline && self.options.use_pipeline_hints;

        let (request, responder) = req.respond();
        macro_rules! err_response {
            ($expr:expr) => {
                responder.respond(RpcResults::Owned(Err($expr)));
            };
        }

        if let Some(err) = &self.disconnect {
            err_response!(err.clone());
            return;
        }

        let Params {
            root,
            message,
            table,
        } = request.params;

        enum ParamsToFill<'b> {
            /// The params to fill are an external message where the root is the params themselves.
            ExternalParams(ExternalMessage),
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
            (ParamsRoot::Params, MessagePayload::External(external)) => {
                outgoing = self.outbound.new();
                let message = outgoing.builder().init_struct_root::<rpc_capnp::Message>();
                (ParamsToFill::ExternalParams(external), message)
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
            }
        };

        // Set up all the common Call fields like interface, method, target, payload, etc.
        let mut call = message.call().init();

        call.interface_id().set(request.interface);
        call.method_id().set(request.method);
        call.no_promise_pipelining().set(no_promise_pipelining);
        call.only_promise_pipeline().set(only_promise_pipelining);

        let mut msg_target = call.target().init();
        match target {
            CapTarget::Import(import) => msg_target.imported_cap().set(import),
            CapTarget::PromisedAnswer(question_id, ops) => {
                let mut msg_promised = msg_target.promised_answer().init();
                msg_promised.question_id().set(question_id);

                let Ok(mut transform) = msg_promised.transform().try_init(ops.len() as u32) else {
                    responder.respond(RpcResults::Owned(Err(Error::failed(
                        "too many pipeline ops",
                    ))));
                    return;
                };

                for (op, idx) in ops.iter().zip(0..) {
                    let mut b = transform.at(idx).get();
                    match op {
                        PipelineOp::PtrField(ptr) => {
                            b.get_pointer_field().set(*ptr);
                        }
                    }
                }
            }
        }

        let mut payload = call.params().init();

        let mut table_to_write;
        match params {
            ParamsToFill::ExternalParams(params) => {
                table_to_write = Table::new(Vec::with_capacity(table.len()));

                let params_reader =
                    message::Reader::new(&*params, self.options.reader_options.clone());
                let table_reader = table.reader();
                let params = params_reader.root().imbue::<CapTable>(table_reader);

                let table_builder = table_to_write.builder();
                let mut content = payload.content().ptr().imbue::<CapTable>(table_builder);

                if let Err(err) = content.try_set(&params, false, ReturnErrors) {
                    err_response!(Error::failed(format!(
                        "failed to copy params into call: {err:}"
                    )));
                    return;
                }
            }
            ParamsToFill::LocalOrphan(orphan) => {
                table_to_write = table;
                payload.content().adopt(orphan);
            }
        }

        let cap_table = table_to_write.into_inner();
        let table_len = cap_table.len() as u32;
        let Ok(mut payload_table) = payload.cap_table().try_init(table_len) else {
            err_response!(Error::failed("too many caps in message"));
            return;
        };
        for (i, cap) in (0..table_len).zip(cap_table.iter()) {
            let Some(cap) = cap else { continue };
            let mut descriptor = payload_table.at(i).get();
            self.write_cap_descriptor(&cap.0, &mut descriptor);
        }

        if let ResponseTarget::Remote(response_to) = &request.target {
            if self.same_connection(&response_to.conn) {
                // This is a reflected call! The call is being sent back over the connection to
                // where the results are needed. This means we can optimize this operation to make
                // sure the other end doesn't send us the results, which would require reflecting
                // those too.

                todo!()
            }

            // todo(level 3): support sendResultsTo.thirdParty
        }

        // This is a call we will have to handle locally.

        let events = self.conn_events_sender.clone();
        let (id, question) = self.questions.push(Question::Incomplete);

        call.question_id().set(id);

        let pipeline = QuestionPipeline {
            question: id,
            events: events.clone(),
        };
        let results = responder.set_pipeline(SetPipeline::RemotePipeline(pipeline));
        let finished = results.finished();
        *question = Question::Call {
            response: results,
            pipelines_count: 0,
            finished: false,
        };

        crate::tokio::spawn(async move {
            finished.await;
            let _ = events.send(ConnectionEvent::QuestionFinished(id));
        });
    }

    fn handle_conn_event(&mut self, incoming: ConnectionEvent) {
        match incoming {
            ConnectionEvent::QuestionFinished(id) => self.handle_finished(id),
            ConnectionEvent::ImportReleased(import) => todo!(),
        }
    }

    fn handle_pipeline(
        &mut self,
        question_id: QuestionId,
        ops: Arc<[PipelineOp]>,
        receiver: chan::Receiver,
    ) {
        let Some(question) = self.questions.get_mut(question_id) else {
            receiver.close(Error::disconnected("invalid question ID"));
            return;
        };

        todo!()
    }

    fn handle_finished(&mut self, question_id: QuestionId) {
        let Some(question) = self.questions.get_mut(question_id) else {
            self.close(Error::failed("invalid question ID"));
            return;
        };

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
        let message = incoming.message();
        let reader = recapn::message::Reader::new(message, self.options.reader_options.clone());
        let reader = reader.read_as_struct::<rpc_capnp::Message>();

        use rpc_capnp::message::Which;
        match reader.which() {
            Ok(Which::Unimplemented(unimplemented)) => {
                todo!()
            }
            Ok(Which::Abort(abort)) => Err(exception_to_error(&abort.try_get()?)),
            Ok(Which::Bootstrap(bootstrap)) => {
                self.handle_bootstrap(bootstrap.try_get()?.question_id())
            }
            Ok(Which::Call(_)) => self.handle_call(incoming.into_owned().message),
            Ok(Which::Return(_)) => self.handle_return(incoming.into_owned().message),
            Ok(Which::Finish(finish)) => {
                todo!()
            }
            Ok(Which::Resolve(resolve)) => {
                todo!()
            }
            Ok(Which::Release(release)) => {
                todo!()
            }
            Ok(Which::Disembargo(disembargo)) => {
                todo!()
            }
            _ => {
                let size = AllocLen::new(message.size_in_words() as u32).unwrap_or(AllocLen::MIN);
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

        results.content().ptr().as_mut().set_capability_ptr(0);

        let answer_value = Answer::Bootstrap { export, client };

        let Ok(()) = self.answers.insert(answer, answer_value) else {
            return Err(Error::failed("questionId is already in use"));
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

    fn handle_return(&mut self, message: ExternalMessage) -> Result<(), Error> {
        let reader = recapn::message::Reader::new(&*message, self.options.reader_options.clone());
        let message = reader.read_as_struct::<rpc_capnp::Message>();
        let ret: ReaderOf<rpc_capnp::Return> = message.r#return().field().unwrap().try_get()?;

        let question_id = ret.answer_id();
        let question = self
            .questions
            .get_mut(question_id)
            .ok_or_else(|| Error::failed("invalid answerId"))?;

        use Question::*;
        match replace(question, Question::Incomplete) {
            Bootstrap { resolver } => match self.resolve_bootstrap_question(&ret) {
                Ok(res) => todo!(),
                Err(err) => {}
            },
            Call {
                response,
                pipelines_count,
                finished,
            } => {
                todo!()
            }
            Reflected => {
                todo!()
            }
            Incomplete | AnsweredBootstrap { .. } | ReturnedCall { .. } => {
                return Err(Error::failed("invalid answer target"))
            }
        }

        todo!()
    }

    fn resolve_bootstrap_question(
        &mut self,
        ret: &ReaderOf<rpc_capnp::Return>,
    ) -> Result<CapResolution, Error> {
        let which = ret.which().map_err(|NotInSchema(variant)| {
            Error::failed(format!("unknown return variant {variant}"))
        })?;

        use rpc_capnp::r#return::Which as WhichReturn;
        match which {
            WhichReturn::Results(payload) => {
                let payload = payload.try_get()?;
                match payload.content().ptr().as_ref().try_to_capability_index()? {
                    Some(idx) => {
                        let table = payload.cap_table().try_get()?;
                        let Some(descriptor) = table.try_at(idx) else {
                            return Err(Error::failed("missing bootstrap capability"));
                        };
                        let cap = self.read_cap_descriptor(&descriptor)?;
                        match cap {
                            Some(sender) => Ok(CapResolution::Resolve(sender)),
                            None => Ok(CapResolution::Broken(null_cap_error())),
                        }
                    }
                    None => Ok(CapResolution::Broken(null_cap_error())),
                }
            }
            WhichReturn::Exception(ex) => {
                Ok(CapResolution::Broken(exception_to_error(&ex.try_get()?)))
            }
            WhichReturn::AcceptFromThirdParty(_) => todo!(),
            WhichReturn::Canceled(_) => Err(Error::failed("bootstrap was not canceled")),
            WhichReturn::ResultsSentElsewhere(_) => {
                Err(Error::failed("bootstrap results cannot be sent elsewhere"))
            }
            WhichReturn::TakeFromOtherQuestion(_) => Err(Error::failed(
                "bootstrap results cannot be taken from another previous question",
            )),
        }
    }

    fn write_cap_descriptor(
        &mut self,
        cap: &chan::Sender,
        descriptor: &mut BuilderOf<rpc_capnp::CapDescriptor>,
    ) -> Option<ExportId> {
        let (cap, res) = cap.most_resolved();
        match cap.chan() {
            RpcChannel::Import(import) => {
                descriptor.receiver_hosted().set(import.id);
            }
            RpcChannel::RemotePipeline => {
                todo!()
            }
            &RpcChannel::Bootstrap(question) => {}
            _ => {}
        }
        if let Some(err) = res {
            // The channel terminated with an error or was dropped. We can't actually write it here
            // as an error though. Instead we export it as a promised capability that we immediately
            // resolve as an error. But we don't want to export it every time.
        }

        let (id, export) = if let Some(&existing) = self.export_by_client.get(cap) {
            let export = self
                .exports
                .get_mut(existing)
                .expect("missing export from by client table");
            export.ref_count += 1;
            (existing, export)
        } else {
            // This is the first time we've seen this capability.
            self.exports.push(Export {
                ref_count: 1,
                client: cap.clone(),
                resolve: None,
            })
        };

        todo!()
    }

    fn read_cap_descriptor(
        &mut self,
        descriptor: &ReaderOf<rpc_capnp::CapDescriptor>,
    ) -> Result<Option<chan::Sender>, Error> {
        use rpc_capnp::cap_descriptor::Which;
        match descriptor.which() {
            Ok(Which::None(())) => Ok(None),
            Ok(Which::SenderHosted(id)) => Ok(Some(self.import(id, false))),
            Ok(Which::SenderPromise(id)) => Ok(Some(self.import(id, true))),
            Ok(Which::ReceiverHosted(hosted)) => {
                todo!()
            }
            Ok(Which::ReceiverAnswer(answer)) => {
                todo!()
            }
            Ok(Which::ThirdPartyHosted(third_party)) => {
                let id = third_party.try_get()?.vine_id();
                Ok(Some(self.import(id, false)))
            }
            Err(NotInSchema(variant)) => Err(Error::failed("unknown cap descriptor type")),
        }
    }

    fn read_cap_descriptors(
        &mut self,
        table: list::StructListReader<rpc_capnp::CapDescriptor>,
    ) -> Result<Vec<Option<Client>>, Error> {
        let mut descriptors = Vec::with_capacity(table.len() as usize);
        for d in table {
            let client = self.read_cap_descriptor(&d)?;
            descriptors.push(client.map(Client));
        }
        Ok(descriptors)
    }

    fn import(&mut self, id: ImportId, promise: bool) -> chan::Sender {
        todo!()
    }

    fn read_message_target(
        &mut self,
        target: &ReaderOf<rpc_capnp::MessageTarget>,
    ) -> Result<Client, Error> {
        let which = target
            .which()
            .map_err(|NotInSchema(v)| Error::failed(format!("Unknown message target type: {v}")))?;
        use rpc_capnp::message_target::Which;
        match which {
            Which::ImportedCap(cap) => {
                if let Some(export) = self.exports.get(cap) {
                    Ok(Client(export.client.clone()))
                } else {
                    Err(Error::failed("Message target is not a current export ID."))
                }
            }
            Which::PromisedAnswer(promised_answer) => {
                let promised_answer = promised_answer.try_get()?;
                let ops = promised_answer.transform().try_get()?;

                let client = match self.answers.get(promised_answer.question_id()) {
                    Some(Answer::Bootstrap { client, .. }) if ops.len() == 0 => {
                        // This is targeting their bootstrap request! Make sure there are no ops in
                        // the ops list and just return the client specified by the export.

                        Client(client.clone())
                    }
                    Some(Answer::Bootstrap { .. }) | None => {
                        Client::broken(Error::failed("Invalid pipeline call"))
                    }
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

#[derive(Clone, Debug, PartialEq, Eq)]
enum CapTarget {
    Import(ImportId),
    PromisedAnswer(QuestionId, Arc<[PipelineOp]>),
}

impl CapTarget {
    pub fn bootstrap(question: QuestionId) -> Self {
        Self::PromisedAnswer(question, Arc::new([]))
    }
}

/// A capability event.
enum CapEvent {
    /// A call forwarded to the connection.
    Call(chan::Item),
    /// The cap handler sent a message to the connection so it couldn't automatically resolve
    /// the channel. When the connection receives this, it might send a disembargo to the other
    /// party, or just handle the resolution itself.
    Disembargo {
        inbound: chan::Receiver,
        outbound: chan::Sender,
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
    /// The cap resolved into another channel
    Resolve(chan::Sender),
}

/// Handles forwarding messages from an internal mpsc into a connection mpsc.
struct ReceivedCap<Rt: crate::rt::Rt> {
    target: CapTarget,
    resolution: crate::tokio::oneshot::Receiver<CapResolution>,

    inbound: chan::Receiver,
    outbound: Rt::Sender<CapMessage>,
}

impl<Rt: crate::rt::Rt> ReceivedCap<Rt> {
    pub async fn handle(mut self) -> () {
        let mut handled_message = false;
        loop {
            crate::tokio::select! {
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
                                target: self.target.clone(),
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
                            target: self.target.clone(),
                            event: CapEvent::Call(msg),
                        };
                        let _ = self.outbound.send(cap_msg).await;
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
    resolver: crate::tokio::oneshot::Sender<CapResolution>,
    client: chan::Sender,
}
