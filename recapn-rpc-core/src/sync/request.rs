//! A request-response oneshot. This has the unique property that the request data and
//! response data are both part of the same allocation. This way you don't need to allocate
//! and track the request data separately.
//!
//! Request, Response, Pipeline, all in one allocation.

use core::cell::UnsafeCell;
use core::fmt::{self, Debug};
use core::future::{Future, IntoFuture};
use core::hash::Hash;
use core::mem::{ManuallyDrop, MaybeUninit};
use core::ops::Deref;
use core::pin::Pin;
use core::ptr::{addr_of_mut, NonNull};
use core::sync::atomic::AtomicUsize;
use core::sync::atomic::Ordering::{Acquire, Relaxed};
use alloc::sync::Arc;
use core::task::{Context, Poll};

use hashbrown::hash_map::RawEntryMut;
use hashbrown::{Equivalent, HashMap};
use parking_lot::Mutex;
use pin_project::{pin_project, pinned_drop};

use crate::sync::mpsc::{weak_channel, Sender};
use crate::sync::sharedshot::{self, RecvWaiter, ShotState, TryRecvError};
use crate::sync::util::Marc;

use super::mpsc::{self, SharedChannel, WeakChannel};
use super::sharedshot::Waiter;
use super::util::linked_list::{Link, Pointers};

/// An abstract channel. This is associated data stored in a mpsc channel that
/// also references associated types with the request-response system.
pub trait Chan: Sized {
    /// The parameters passed into the request.
    type Parameters;

    /// A key used to distinguish between different pipelines.
    type PipelineKey: Eq + Hash;

    /// An error that can be used to terminate channels.
    type Error: Clone + IntoResults<Self>;

    /// The results associated with this parameter type.
    type Results: PipelineResolver<Self>;

    /// A pipeline that can be configured separately via `set_pipeline`.
    type Pipeline: PipelineResolver<Self>;
}

/// Allows for the creation of `ResponseReceivers` from within a `PipelineResolver`.
///
/// This can be useful for when a separate pipeline is configured with `set_pipeline` that still
/// needs the original response. The factory can be used to create `ResponseReceiver` instances
/// and process the response when it comes in.
pub struct ResponseReceiverFactory<'a, C: Chan> {
    shared: &'a Arc<SharedRequest<C>>,
}

impl<'a, C: Chan> Clone for ResponseReceiverFactory<'a, C> {
    fn clone(&self) -> Self {
        Self {
            shared: self.shared,
        }
    }
}

impl<'a, C: Chan> Copy for ResponseReceiverFactory<'a, C> {}

pub trait PipelineResolver<C: Chan> {
    /// Resolves the pipeline channel with the given key.
    fn resolve(
        &self,
        recv: ResponseReceiverFactory<C>,
        key: C::PipelineKey,
        channel: mpsc::Receiver<C>,
    );

    /// Returns the pipeline channel with the given key. If the key doesn't match,
    /// this may return an already broken channel.
    fn pipeline(&self, recv: ResponseReceiverFactory<C>, key: C::PipelineKey) -> mpsc::Sender<C>;
}

/// Describes a conversion from a type into "results".
pub trait IntoResults<C: Chan> {
    fn into_results(self) -> C::Results;
}

/// A hint to how a request is going to be used.
///
/// Requests have a response and pipeline associated with them, but sometimes only one or the
/// other is used. By using different request constructors, a request's usage is set by what
/// parts are constructed. This is used as a hint by the RPC system to skip bookkeeping in
/// some cases.
///
/// This hint is not provided manually, instead it's provided depending on the constructor called
/// to create the request.
#[derive(Clone, Copy, PartialEq, Eq, Debug)]
pub enum RequestUsage {
    /// The request's response and pipeline both might be used.
    ResponseAndPipeline,
    /// Only the request response will be used. Pipelines will not be used.
    ///
    /// With this hint the callee doesn't keep results around for pipelining.
    Response,
    /// This will only be used for pipelining. The response will not be used.
    ///
    /// With this hint, the results aren't stored unless needed for pipelining.
    /// They also aren't sent over the network since the caller indicated it doesn't want them.
    Pipeline,
}

/// The shared state behind a request
///
/// This is an extremely complex structure that controls all of the details behind
/// request lifetime and management. It combines requests, responses, and pipelines
/// into one structure in order to save memory allocations as all of these effectively
/// have the same lifetime.
pub(crate) struct SharedRequest<C: Chan> {
    /// Intrusive linked-list pointers for request channels
    ///
    /// In order to maintain "one allocation per request", we intrusively link
    /// requests together to build a request chain.
    pointers: Pointers<SharedRequest<C>>,

    /// The channel this request is contained in. If all the receivers of this request
    /// drop, we attempt to automatically remove the request from whatever channel it's in.
    parent: Mutex<Option<Arc<SharedChannel<C>>>>,

    /// The request data. This should only be accessed by Request. When Request drops they
    /// will trigger a drop on these contents.
    request: UnsafeCell<ManuallyDrop<C::Parameters>>,

    /// Indicates how this request is intended to be used. This can be used as a hint when
    /// sending requests remotely to skip certain bookkeeping.
    usage: RequestUsage,

    /// The state of the request.
    state: sharedshot::AtomicState,

    /// The number of receivers waiting for the response. When this value reaches zero,
    /// the channel is closed and the closed task is woken up.
    ///
    /// A receiver includes both response receivers, pipelines, and any pipelined clients
    /// that are waiting for the response to be received.
    receivers_count: AtomicUsize,

    /// A set of waiters waiting to receive the value. Pipelined requests
    /// are not included here since they're fulfilled separately.
    waiters: sharedshot::WaitList,

    /// The close task, woken up when all the receivers have been dropped and the channel
    /// has been marked closed.
    closed_task: sharedshot::ClosedTask,

    /// The response value.
    response: UnsafeCell<MaybeUninit<C::Results>>,

    /// The map to queued clients waiting to be resolved.
    pipeline_map: Mutex<HashMap<C::PipelineKey, WeakChannel<C>>>,

    /// The destination pipeline value. If set to None, the response holds the pipeline
    pipeline_dest: UnsafeCell<MaybeUninit<Option<C::Pipeline>>>,
}

// TODO(NOW): Make this more accurate this is definitely wrong.
unsafe impl<C> Send for SharedRequest<C>
where
    C: Chan + Send,
    C::Parameters: Send,
    C::Error: Send,
    C::Results: Send,
    C::PipelineKey: Send,
    C::Pipeline: Send,
{
}
unsafe impl<C> Sync for SharedRequest<C>
where
    C: Chan + Send,
    C::Parameters: Send,
    C::Error: Send,
    C::Results: Send,
    C::PipelineKey: Send,
    C::Pipeline: Send,
{
}

unsafe impl<C: Chan> Link for SharedRequest<C> {
    type Handle = Arc<Self>;
    type Target = Self;

    fn into_raw(handle: Self::Handle) -> NonNull<Self::Target> {
        NonNull::new(Arc::into_raw(handle).cast_mut()).unwrap()
    }
    unsafe fn from_raw(ptr: NonNull<Self::Target>) -> Self::Handle {
        Arc::from_raw(ptr.as_ptr().cast_const())
    }
    unsafe fn pointers(target: NonNull<Self::Target>) -> NonNull<Pointers<Self::Target>> {
        let me = target.as_ptr();
        let field = addr_of_mut!((*me).pointers);
        NonNull::new_unchecked(field)
    }
}

impl<C: Chan> Drop for SharedRequest<C> {
    fn drop(&mut self) {
        // The request parameters don't need to be dropped since we would've had to drop
        // or otherwise destroy a Request in order to reach the point of dropping this.

        let state = self.state.get();

        if state.is_set() {
            unsafe { self.response.get_mut().assume_init_drop() }
        }

        if state.is_closed_task_set() {
            unsafe { self.closed_task.drop() }
        }

        if state.is_pipeline_set() {
            unsafe { self.pipeline_dest.get_mut().assume_init_drop() }
        }
    }
}

impl<C: Chan> SharedRequest<C> {
    pub fn new(request: C::Parameters, usage: RequestUsage, receivers: usize) -> Self {
        Self {
            parent: Mutex::new(None),
            pointers: Pointers::new(),
            request: UnsafeCell::new(ManuallyDrop::new(request)),
            usage,
            state: sharedshot::AtomicState::new(),
            waiters: sharedshot::WaitList::new(),
            receivers_count: AtomicUsize::new(receivers),
            closed_task: sharedshot::ClosedTask::new(),
            response: UnsafeCell::new(MaybeUninit::uninit()),
            pipeline_map: Mutex::new(HashMap::new()),
            pipeline_dest: UnsafeCell::new(MaybeUninit::uninit()),
        }
    }

    pub unsafe fn request(&self) -> &C::Parameters {
        &*self.request.get()
    }

    /// Drop the request value inside the shared request
    pub unsafe fn drop_request(&self) {
        let request = &mut *self.request.get();
        ManuallyDrop::drop(request);
    }

    pub unsafe fn take_request(&self) -> C::Parameters {
        let request = &mut *self.request.get();
        ManuallyDrop::take(request)
    }

    pub fn close_sender(&self) {
        self.state.set_send_closed();
        self.waiters.wake_all();
    }

    pub fn finished(self: &Arc<Self>) -> Finished<C> {
        todo!()
    }

    pub fn is_finished(&self) -> bool {
        !self.state.load(Relaxed).is_recv_closed()
    }

    pub unsafe fn response(&self) -> &C::Results {
        (*self.response.get()).assume_init_ref()
    }

    /// Set the parent channel for this request.
    ///
    /// This should only be called while holding a lock on the specified channel.
    pub unsafe fn set_parent(&self, parent: Arc<SharedChannel<C>>) {
        let mut lock = self.parent.lock();
        debug_assert!(lock.is_none());
        *lock = Some(parent);
    }

    /// Take back the arc for this request's channel.
    ///
    /// This should only be called while holding a lock on the channel holding this request.
    pub unsafe fn take_parent(&self) -> Option<Arc<SharedChannel<C>>> {
        self.parent.lock().take()
    }

    /// Attempts to remove the request the current channel containing it. If this request
    /// isn't in a channel, this does nothing.
    pub fn try_remove_from_channel(&self) {
        let Some(parent) = self.parent.lock().clone() else {
            return;
        };

        let Some(most_resolved) = parent.most_unresolved() else {
            // If the channel is resolved by dropping or erroring permanently, we assume the
            // channel is actually in the middle of dropping requests, but just hasn't detached us
            // yet. We just return so that the channel can drop us itself.
            return;
        };

        todo!()
    }

    pub unsafe fn add_receiver(&self) {
        let old = self.receivers_count.fetch_add(1, Relaxed);

        if old == usize::MAX {
            std::process::abort();
        }

        // Make sure I don't accidentally attempt to re-open the channel.
        debug_assert_ne!(old, 0);
    }

    /// Remove a tracked receiver from the receiver count.
    ///
    /// If this is the last receiver (and the receiver count is zero), this closes the channel on
    /// the receiving side.
    ///
    /// Note: A channel cannot be re-opened by adding a receiver when the channel is closed.
    pub unsafe fn remove_receiver(&self) {
        let last_receiver = self.receivers_count.fetch_sub(1, Relaxed) == 0;
        if last_receiver {
            self.closed_task.close(&self.state);
            self.try_remove_from_channel();
        }
    }

    fn pipeline<K, F>(self: &Arc<Self>, key: K, chan: F) -> Sender<C>
    where
        K: Hash + Equivalent<C::PipelineKey>,
        C::PipelineKey: From<K>,
        F: FnOnce() -> C,
    {
        if !self.state.load(Acquire).is_pipeline_set() {
            // The pipeline hasn't been set yet. Lock the pipeline map so we can get a
            // pipelined client.
            let mut map = self.pipeline_map.lock();
            // Load relaxed since the mutex lock provides the synchronization we need
            // if the pipeline does get set.
            if !self.state.load(Relaxed).is_pipeline_set() {
                // It *still* hasn't been set, time to mutate the map
                let sender = match map.raw_entry_mut().from_key(&key) {
                    RawEntryMut::Occupied(mut occupied) => match occupied.get().sender() {
                        Some(sender) => sender,
                        None => {
                            let receiver = Receiver::track(self.clone());
                            let (sender, weak_channel) = weak_channel(chan(), receiver);
                            let _ = occupied.insert(weak_channel);
                            sender
                        }
                    },
                    RawEntryMut::Vacant(vacant) => {
                        let receiver = Receiver::track(self.clone());
                        let (sender, weak_channel) = weak_channel(chan(), receiver);
                        let _ = vacant.insert(C::PipelineKey::from(key), weak_channel);
                        sender
                    }
                };

                return sender;
            }
        }

        let factory = ResponseReceiverFactory { shared: self };

        let key = key.into();

        // Ah, a pipeline was set, so we can apply the ops and resolve directly into the
        // correct client.
        let pipeline = unsafe { (*self.pipeline_dest.get()).assume_init_ref() };
        match pipeline {
            Some(p) => p.pipeline(factory, key),
            None => unsafe { self.response().pipeline(factory, key) },
        }
    }

    unsafe fn respond_and_resolve(self: &Arc<Self>, result: C::Results) {
        // Write the response first
        (*self.response.get()).write(result);

        // Then set the pipeline to the response and resolve the pipeline.
        // If this panics we'll leak the response which kinda sucks
        // (since we won't set the flag saying the response is set)
        // so let's just make sure we don't panic.
        //
        // TODO: Use a scope guard to make sure we set the set value
        //       flag is set
        self.set_pipeline(None);

        // Now confirm the response being set
        self.state.set_value();

        // And wake up all the response receivers.
        self.waiters.wake_all();
    }

    unsafe fn respond(&self, result: C::Results) {
        (*self.response.get()).write(result);
        self.state.set_value();
        self.waiters.wake_all();
    }

    unsafe fn set_pipeline(self: &Arc<Self>, dest: Option<C::Pipeline>) {
        let dest = &*(*self.pipeline_dest.get()).write(dest);
        let factory = ResponseReceiverFactory { shared: self };

        match dest {
            None => {
                let r = self.response();
                self.resolve_pipeline_map(|k, c| r.resolve(factory, k, c));
            }
            Some(dest) => self.resolve_pipeline_map(|k, c| dest.resolve(factory, k, c)),
        }
    }

    fn resolve_pipeline_map<F: Fn(C::PipelineKey, mpsc::Receiver<C>)>(&self, resolver: F) {
        loop {
            let mut lock = self.pipeline_map.lock();
            if lock.is_empty() {
                // There's nothing in the map. We can now mark the pipeline as resolved.
                // Set the flag with relaxed ordering since when we return the unlock will
                // provide our synchronization.
                self.state.set_pipeline(Relaxed);
                return;
            }

            // Take the map from the mutex and unlock. This is to make sure we don't
            // somehow deadlock somewhere down the line and lets other threads make progress.
            let map = core::mem::take(&mut *lock);
            drop(lock);

            for (key, value) in map {
                let Some(receiver) = value.upgrade() else {
                    continue;
                };
                resolver(key, receiver);
            }
        }
    }
}

/// A tracked receiver type that manipulates the `receivers_count` of the shared
/// request state. This is intended to make it obvious what types count as receivers
/// and automate most of the work around tracking receivers.
pub(crate) struct Receiver<C: Chan>(Arc<SharedRequest<C>>);

impl<C: Chan> Receiver<C> {
    pub fn track(req: Arc<SharedRequest<C>>) -> Self {
        unsafe {
            req.add_receiver();
        }
        Self(req)
    }
}

impl<C: Chan> Clone for Receiver<C> {
    fn clone(&self) -> Self {
        Self::track(self.0.clone())
    }
}

impl<C: Chan> Drop for Receiver<C> {
    fn drop(&mut self) {
        unsafe { self.0.remove_receiver() }
    }
}

/// Create a new request and response channel with pipelining.
pub fn request_response_pipeline<C: Chan>(
    msg: C::Parameters,
) -> (Request<C>, ResponseReceiver<C>, PipelineBuilder<C>) {
    let request = Arc::new(SharedRequest::new(
        msg,
        RequestUsage::ResponseAndPipeline,
        2,
    ));
    (
        Request::new(request.clone()),
        ResponseReceiver {
            shared: Receiver(request.clone()),
        },
        PipelineBuilder {
            shared: Receiver(request),
        },
    )
}

/// Create a new request and response channel without pipelining.
pub fn request_response<C: Chan>(msg: C::Parameters) -> (Request<C>, ResponseReceiver<C>) {
    let request = Arc::new(SharedRequest::new(msg, RequestUsage::Response, 1));
    (
        Request::new(request.clone()),
        ResponseReceiver {
            shared: Receiver(request),
        },
    )
}

/// Create a new request for pipelining.
pub fn request_pipeline<C: Chan>(msg: C::Parameters) -> (Request<C>, PipelineBuilder<C>) {
    let request = Arc::new(SharedRequest::new(msg, RequestUsage::Pipeline, 1));
    (
        Request::new(request.clone()),
        PipelineBuilder {
            shared: Receiver(request),
        },
    )
}

/// A future that signals when the request is "finished".
///
/// A request is finished when there are no more receivers waiting for responses. A receiver can be
///
///  * A response receiver that consumes the response directly.
///  * A pending pipeline that hasn't been resolved.
///  * Or a pipeline builder that can construct new pipelines.
///
/// When there are no more receivers for the response of a request, the request is considered
/// "finished" and can be thrown away. If a request is actively in a channel, it will remove
/// itself from the channel and drop itself. Servers can use this to cancel an operation by
/// selecting either a "finished" future or a future processing the request. Multiple futures
/// can exist simultaniously and wait on the same request.
#[pin_project(PinnedDrop)]
pub struct Finished<C: Chan> {
    shared: Arc<SharedRequest<C>>,

    #[pin]
    waiter: Option<Waiter>,

    state: Poll<()>,
}

impl<C: Chan> Future for Finished<C> {
    type Output = ();

    fn poll(self: Pin<&mut Self>, cx: &mut Context<'_>) -> Poll<Self::Output> {
        todo!()
    }
}

#[pinned_drop]
impl<C: Chan> PinnedDrop for Finished<C> {
    fn drop(self: Pin<&mut Self>) {
        todo!()
    }
}

pub struct Request<C: Chan> {
    shared: Marc<SharedRequest<C>>,
}

impl<C: Chan> Request<C> {
    pub(crate) const fn new(inner: Arc<SharedRequest<C>>) -> Self {
        Self {
            shared: Marc::new(inner),
        }
    }

    /// Get the inner request value.
    pub fn get(&self) -> &C::Parameters {
        unsafe { self.shared.get().request() }
    }

    pub fn usage(&self) -> RequestUsage {
        self.shared.get().usage
    }

    /// Respond to the request using a responder derived from it, taking the parameters in
    /// the process.
    pub fn respond(mut self) -> (C::Parameters, Responder<C>) {
        unsafe {
            let shared = self.shared.take();
            let params = shared.take_request();
            let responder = Responder {
                shared: Marc::new(shared),
            };
            (params, responder)
        }
    }

    pub fn finished(&self) -> Finished<C> {
        self.shared.inner().finished()
    }

    pub fn is_finished(&self) -> bool {
        self.shared.get().is_finished()
    }

    pub(crate) fn take_inner(mut self) -> Arc<SharedRequest<C>> {
        unsafe { self.shared.take() }
    }
}

impl<C: Chan> Drop for Request<C> {
    fn drop(&mut self) {
        if let Some(shared) = self.shared.try_take() {
            unsafe {
                shared.drop_request();
                shared.close_sender();
            }
        }
    }
}

/// Allows sending a response for a request.
pub struct Responder<C: Chan> {
    shared: Marc<SharedRequest<C>>,
}

impl<C: Chan> Responder<C> {
    /// Returns the given response.
    pub fn respond(mut self, resp: C::Results) {
        unsafe {
            let shared = self.shared.take();
            shared.respond_and_resolve(resp);
        }
    }

    /// Reuse the request to perform a tail-call, turning the responder back into a request.
    pub fn tail_call(mut self, msg: C::Parameters) -> Request<C> {
        unsafe {
            let shared = self.shared.take();
            *shared.request.get() = ManuallyDrop::new(msg);
            Request {
                shared: Marc::new(shared),
            }
        }
    }

    /// Fulfill the pipeline portion of the request, allowing pipelined requests to flow though
    /// without waiting for the call to complete.
    pub fn set_pipeline(mut self, dst: C::Pipeline) -> ResultsSender<C> {
        unsafe {
            let shared = self.shared.take();
            shared.set_pipeline(Some(dst));
            ResultsSender {
                shared: Marc::new(shared),
            }
        }
    }

    pub fn finished(&self) -> Finished<C> {
        self.shared.inner().finished()
    }

    pub fn is_finished(&self) -> bool {
        self.shared.get().is_finished()
    }
}

impl<C: Chan> Drop for Responder<C> {
    fn drop(&mut self) {
        if let Some(shared) = self.shared.try_take() {
            shared.close_sender()
        }
    }
}

/// The result of calling `Responder::set_pipeline`, a type that can only be used to send the
/// results of a call.
pub struct ResultsSender<C: Chan> {
    shared: Marc<SharedRequest<C>>,
}

impl<C: Chan> ResultsSender<C> {
    /// Returns the given response.
    pub fn respond(mut self, resp: C::Results) {
        unsafe {
            let shared = self.shared.take();
            shared.respond(resp);
        }
    }

    pub fn finished(&self) -> Finished<C> {
        self.shared.inner().finished()
    }

    pub fn is_finished(&self) -> bool {
        self.shared.get().is_finished()
    }
}

impl<C: Chan> Drop for ResultsSender<C> {
    fn drop(&mut self) {
        if let Some(shared) = self.shared.try_take() {
            shared.close_sender();
        }
    }
}

/// A response receiver that can be used to await the response.
#[derive(Clone)]
pub struct ResponseReceiver<C: Chan> {
    shared: Receiver<C>,
}

impl<C: Chan> ResponseReceiver<C> {
    /// Attempts to receive the value. This returns an owned copy of the value that can be used separately from the receiver.
    pub fn try_recv(&self) -> Result<Response<C>, TryRecvError> {
        match self.shared.0.state.state() {
            ShotState::Closed => Err(TryRecvError::Closed),
            ShotState::Empty => Err(TryRecvError::Empty),
            ShotState::Sent => Ok(Response {
                shared: self.shared.0.clone(),
            }),
        }
    }

    pub fn try_ref(&self) -> Result<&C::Results, TryRecvError> {
        match self.shared.0.state.state() {
            ShotState::Closed => Err(TryRecvError::Closed),
            ShotState::Empty => Err(TryRecvError::Empty),
            ShotState::Sent => Ok(unsafe { self.shared.0.response() }),
        }
    }

    pub fn recv(self) -> Recv<C> {
        Recv::new(self.shared)
    }
}

impl<C: Chan> IntoFuture for ResponseReceiver<C> {
    type IntoFuture = Recv<C>;
    type Output = Option<Response<C>>;

    fn into_future(self) -> Self::IntoFuture {
        self.recv()
    }
}

#[pin_project(PinnedDrop)]
pub struct Recv<C: Chan> {
    shot: Receiver<C>,

    #[pin]
    waiter: RecvWaiter,

    state: Poll<Option<Response<C>>>,
}

unsafe impl<C: Chan + Send> Send for Recv<C> {}
unsafe impl<C: Chan + Sync> Sync for Recv<C> {}

impl<C: Chan> Recv<C> {
    fn new(shot: Receiver<C>) -> Self {
        Recv {
            shot,
            waiter: RecvWaiter::new(),
            state: Poll::Pending,
        }
    }
}

impl<C: Chan> Future for Recv<C> {
    type Output = Option<Response<C>>;

    fn poll(self: Pin<&mut Self>, ctx: &mut Context<'_>) -> Poll<Self::Output> {
        let this = self.project();

        if this.state.is_ready() {
            return this.state.clone();
        }

        let value = match this
            .waiter
            .poll(ctx, &this.shot.0.state, &this.shot.0.waiters)
        {
            ShotState::Empty => return Poll::Pending,
            ShotState::Sent => Some(Response {
                shared: this.shot.0.clone(),
            }),
            ShotState::Closed => None,
        };
        *this.state = Poll::Ready(value.clone());

        Poll::Ready(value)
    }
}

#[pinned_drop]
impl<C: Chan> PinnedDrop for Recv<C> {
    fn drop(self: Pin<&mut Self>) {
        let this = self.project();

        this.waiter.pinned_drop(&this.shot.0.waiters);
    }
}

pub struct Response<C: Chan> {
    shared: Arc<SharedRequest<C>>,
}

impl<C: Chan> Response<C> {
    fn get(&self) -> &C::Results {
        // SAFETY: We only construct a Response when we know the inner value is set.
        unsafe { (*self.shared.response.get()).assume_init_ref() }
    }
}

impl<C: Chan> Clone for Response<C> {
    fn clone(&self) -> Self {
        Self {
            shared: self.shared.clone(),
        }
    }
}

impl<C> Debug for Response<C>
where
    C: Chan,
    C::Results: Debug,
    C::Error: Debug,
{
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        self.get().fmt(f)
    }
}

impl<C: Chan> AsRef<C::Results> for Response<C> {
    fn as_ref(&self) -> &C::Results {
        Response::get(self)
    }
}

impl<C: Chan> Deref for Response<C> {
    type Target = C::Results;

    fn deref(&self) -> &Self::Target {
        Response::get(self)
    }
}

#[derive(Clone)]
pub struct PipelineBuilder<C: Chan> {
    shared: Receiver<C>,
}

impl<C: Chan> PipelineBuilder<C> {
    pub fn build<K, F>(&self, key: K, chan: F) -> Sender<C>
    where
        K: Hash + Equivalent<C::PipelineKey>,
        C::PipelineKey: From<K>,
        F: FnOnce() -> C,
    {
        self.shared.0.pipeline(key, chan)
    }
}

#[cfg(test)]
mod test {
    use assert_matches2::assert_matches;
    use hashbrown::HashMap;
    use mpsc::TryRecvError;

    use crate::{
        sync::{mpsc, request::RequestUsage},
        Error,
    };

    use super::{request_response, Chan, IntoResults, PipelineResolver, ResponseReceiverFactory};

    #[derive(Debug)]
    struct TestChannel;

    #[derive(Clone, Copy, Eq, PartialEq, Debug)]
    struct IntRequest(i32);

    #[derive(Clone, Debug)]
    struct IntsResponse(HashMap<i32, mpsc::Sender<TestChannel>>);

    #[derive(PartialEq, Eq, Hash, Debug)]
    struct IntPipelineKey(i32);

    impl IntoResults<TestChannel> for Error {
        #[inline]
        fn into_results(self) -> <TestChannel as Chan>::Results {
            Err(self)
        }
    }

    impl Chan for TestChannel {
        type Parameters = IntRequest;
        type Error = Error;
        type Results = Result<IntsResponse, Error>;
        type PipelineKey = IntPipelineKey;
        type Pipeline = IntsResponse;
    }

    impl PipelineResolver<TestChannel> for IntsResponse {
        fn resolve(
            &self,
            _: ResponseReceiverFactory<TestChannel>,
            key: IntPipelineKey,
            channel: mpsc::Receiver<TestChannel>,
        ) {
            let Some(results) = self.0.get(&key.0) else {
                channel.close(Error::failed("unknown pipeline"));
                return;
            };

            if let Err(c) = channel.forward_to(results) {
                // If it's an infinite loop, close it with an error
                c.close(Error::failed("infinite loop"));
            }
        }

        fn pipeline(
            &self,
            _: ResponseReceiverFactory<TestChannel>,
            key: IntPipelineKey,
        ) -> mpsc::Sender<TestChannel> {
            match self.0.get(&key.0) {
                Some(c) => c.clone(),
                None => mpsc::broken(TestChannel, Error::failed("unknown pipeline")),
            }
        }
    }

    impl PipelineResolver<TestChannel> for Result<IntsResponse, Error> {
        fn resolve(
            &self,
            recv: ResponseReceiverFactory<TestChannel>,
            key: IntPipelineKey,
            channel: mpsc::Receiver<TestChannel>,
        ) {
            match self {
                Ok(r) => r.resolve(recv, key, channel),
                Err(err) => channel.close(err.clone()),
            }
        }

        fn pipeline(
            &self,
            _: ResponseReceiverFactory<TestChannel>,
            key: IntPipelineKey,
        ) -> mpsc::Sender<TestChannel> {
            match self {
                Ok(r) => match r.0.get(&key.0) {
                    Some(c) => c.clone(),
                    None => mpsc::broken(TestChannel, Error::failed("unknown pipeline")),
                },
                Err(err) => mpsc::broken(TestChannel, err.clone()),
            }
        }
    }

    #[test]
    #[ignore = "borked"]
    fn sync_request_response() {
        // Make a request and respond, then receive the result and check it.

        let input = IntRequest(1);
        let (req, resp) = request_response::<TestChannel>(input);
        assert_eq!(req.usage(), RequestUsage::Response);

        let (params, responder) = req.respond();
        assert_eq!(input, params);

        assert!(!responder.is_finished());
        responder.respond(Ok(IntsResponse(HashMap::new())));

        let results = resp.try_recv().unwrap();
        let response = (*results).as_ref().unwrap();

        assert!(response.0.is_empty());
    }

    #[test]
    fn sync_close_request() {
        // Make a request, but drop the request, and make sure the receiver becomes aware of it.

        let input = IntRequest(1);
        let (req, resp) = request_response::<TestChannel>(input);
        drop(req);

        assert_matches!(resp.try_recv(), Err(TryRecvError::Closed));
    }
}
