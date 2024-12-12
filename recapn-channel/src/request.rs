//! A request-response oneshot. This has the unique property that the request data and
//! response data are both part of the same allocation. This way you don't need to allocate
//! and track the request data separately.
//!
//! Request, Response, Pipeline, all in one allocation.

use alloc::sync::Arc;
use core::cell::UnsafeCell;
use core::fmt::{self, Debug};
use core::future::{Future, IntoFuture};
use core::hash::Hash;
use core::mem::{ManuallyDrop, MaybeUninit};
use core::ops::Deref;
use core::pin::Pin;
use core::sync::atomic::Ordering::{Acquire, Relaxed};
use core::sync::atomic::{AtomicUsize, Ordering};
use core::task::{Context, Poll};

use hashbrown::hash_map::RawEntryMut;
use hashbrown::{Equivalent, HashMap};
use parking_lot::Mutex;
use pin_project::{pin_project, pinned_drop};

use crate::mpsc::{self, weak_channel, Sender, SharedChannel, SharedLink, WeakChannel};
use crate::util::atomic_state::{AtomicState, ShotState};
use crate::util::wait_list::{ClosedWaiter, RecvWaiter, WaitList};
use crate::util::Marc;
use crate::{Chan, PipelineResolver};

#[derive(PartialEq, Eq, Clone, Copy, Debug)]
pub enum TryRecvError {
    Empty,
    Closed,
}

/// Allows for the creation of `ResponseReceivers` from within a `PipelineResolver`.
///
/// This can be useful for when a separate pipeline is configured with `set_pipeline` that still
/// needs the original response. The factory can be used to create `ResponseReceiver` instances
/// and process the response when it comes in.
pub struct ResponseReceiverFactory<'a, C: Chan> {
    shared: &'a Arc<RequestInner<C>>,
}

impl<'a, C: Chan> Clone for ResponseReceiverFactory<'a, C> {
    fn clone(&self) -> Self {
        Self {
            shared: self.shared,
        }
    }
}

impl<'a, C: Chan> Copy for ResponseReceiverFactory<'a, C> {}

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
    state: AtomicState,

    /// The number of receivers waiting for the response. When this value reaches zero,
    /// the channel is closed and the closed task is woken up.
    ///
    /// A receiver includes both response receivers, pipelines, and any pipelined clients
    /// that are waiting for the response to be received.
    receivers_count: AtomicUsize,

    /// A set of waiters waiting to receive the value.
    /// Pipelined requests are not included here since they're fulfilled separately.
    waiters: WaitList,

    /// A set of waiters waiting for the request to be finished. Finished waiters are not contained
    /// within the receiving waiters since we want to make sure we only wake up finished tasks when
    /// all receivers leave scope.
    finished_waiters: WaitList,

    /// The response value.
    response: UnsafeCell<MaybeUninit<C::Results>>,

    /// The map to queued clients waiting to be resolved.
    pipeline_map: Mutex<HashMap<C::PipelineKey, WeakChannel<C>>>,

    /// The destination pipeline value. If set to None, the response holds the pipeline
    pipeline_dest: UnsafeCell<MaybeUninit<Option<C::Pipeline>>>,
}

type RequestInner<C> = SharedLink<SharedRequest<C>>;

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

impl<C: Chan> Drop for SharedRequest<C> {
    fn drop(&mut self) {
        // The request parameters don't need to be dropped since we would've had to drop
        // or otherwise destroy a Request in order to reach the point of dropping this.

        let state = self.state.get();

        if state.is_set() {
            unsafe { self.response.get_mut().assume_init_drop() }
        }

        if state.is_pipeline_set() {
            unsafe { self.pipeline_dest.get_mut().assume_init_drop() }
        }
    }
}

impl<C: Chan> SharedRequest<C> {
    pub fn new(
        request: C::Parameters,
        usage: RequestUsage,
        receivers: usize,
    ) -> Arc<SharedLink<Self>> {
        SharedLink::new(Self {
            parent: Mutex::new(None),
            request: UnsafeCell::new(ManuallyDrop::new(request)),
            usage,
            state: AtomicState::new(),
            waiters: WaitList::new(),
            finished_waiters: WaitList::new(),
            receivers_count: AtomicUsize::new(receivers),
            response: UnsafeCell::new(MaybeUninit::uninit()),
            pipeline_map: Mutex::new(HashMap::new()),
            pipeline_dest: UnsafeCell::new(MaybeUninit::uninit()),
        })
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

    pub fn is_finished(&self) -> bool {
        self.state.load(Relaxed).is_recv_closed()
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
            panic!("out of receiver counters");
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
        let old = self.receivers_count.fetch_sub(1, Relaxed);
        let last_receiver = old - 1 == 0;
        if last_receiver {
            self.state.set_recv_closed();
            self.finished_waiters.wake_all();

            self.try_remove_from_channel();
        }
    }

    fn resolve_pipeline_map(&self, resolver: impl Fn(C::PipelineKey, mpsc::Receiver<C>)) {
        // Declare the lock first so that it's dropped last.
        let mut lock;
        // Set the flag with relaxed ordering since when we return the unlock will
        // provide our synchronization.
        scopeguard::defer!({
            self.state.set_pipeline(Ordering::Relaxed);
        });

        loop {
            lock = self.pipeline_map.lock();
            if lock.is_empty() {
                // There's nothing in the map. We can now mark the pipeline as resolved.
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
pub(crate) struct Receiver<C: Chan>(Arc<RequestInner<C>>);

impl<C: Chan> Receiver<C> {
    pub fn track(req: Arc<RequestInner<C>>) -> Self {
        unsafe {
            req.data.add_receiver();
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
        unsafe { self.0.data.remove_receiver() }
    }
}

/// Create a new request and response channel with pipelining.
pub fn request_response_pipeline<C: Chan>(
    msg: C::Parameters,
) -> (Request<C>, ResponseReceiver<C>, PipelineBuilder<C>) {
    let request = SharedRequest::new(msg, RequestUsage::ResponseAndPipeline, 2);
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
    let request = SharedRequest::new(msg, RequestUsage::Response, 1);
    (
        Request::new(request.clone()),
        ResponseReceiver {
            shared: Receiver(request),
        },
    )
}

/// Create a new request for pipelining.
pub fn request_pipeline<C: Chan>(msg: C::Parameters) -> (Request<C>, PipelineBuilder<C>) {
    let request = SharedRequest::new(msg, RequestUsage::Pipeline, 1);
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
    shared: Arc<RequestInner<C>>,

    #[pin]
    waiter: ClosedWaiter,

    state: Poll<()>,
}

impl<C: Chan> Finished<C> {
    fn new(shared: Arc<RequestInner<C>>) -> Self {
        Self {
            shared,
            waiter: ClosedWaiter::new(),
            state: Poll::Pending,
        }
    }
}

impl<C: Chan> Future for Finished<C> {
    type Output = ();

    fn poll(self: Pin<&mut Self>, ctx: &mut Context<'_>) -> Poll<Self::Output> {
        let this = self.project();

        if this.state.is_ready() {
            return *this.state;
        }

        let finished = this.waiter.poll(
            ctx,
            &this.shared.data.state,
            &this.shared.data.finished_waiters,
        );
        if !finished {
            return Poll::Pending;
        }

        *this.state = Poll::Ready(());

        Poll::Ready(())
    }
}

#[pinned_drop]
impl<C: Chan> PinnedDrop for Finished<C> {
    fn drop(self: Pin<&mut Self>) {
        let this = self.project();

        this.waiter.pinned_drop(&this.shared.data.finished_waiters);
    }
}

pub struct Request<C: Chan> {
    shared: Marc<RequestInner<C>>,
}

impl<C: Chan> Request<C> {
    pub(crate) const fn new(inner: Arc<RequestInner<C>>) -> Self {
        Self {
            shared: Marc::new(inner),
        }
    }

    /// Get the inner request value.
    pub fn get(&self) -> &C::Parameters {
        unsafe { self.shared.get().data.request() }
    }

    pub fn usage(&self) -> RequestUsage {
        self.shared.get().data.usage
    }

    /// Respond to the request using a responder derived from it, taking the parameters in
    /// the process.
    pub fn respond(mut self) -> (C::Parameters, Responder<C>) {
        unsafe {
            let shared = self.shared.take();
            let params = shared.data.take_request();
            let responder = Responder {
                shared: Marc::new(shared),
            };
            (params, responder)
        }
    }

    pub fn finished(&self) -> Finished<C> {
        Finished::new(self.shared.get().clone())
    }

    pub fn is_finished(&self) -> bool {
        self.shared.get().data.is_finished()
    }

    pub(crate) fn take_inner(mut self) -> Arc<RequestInner<C>> {
        unsafe { self.shared.take() }
    }
}

impl<C: Chan> Drop for Request<C> {
    fn drop(&mut self) {
        if let Some(shared) = self.shared.try_take() {
            unsafe {
                shared.data.drop_request();
                shared.data.close_sender();
            }
        }
    }
}

/// Allows sending a response for a request.
pub struct Responder<C: Chan> {
    shared: Marc<RequestInner<C>>,
}

impl<C: Chan> Responder<C> {
    /// Returns the given response.
    pub fn respond(mut self, resp: C::Results) {
        // SAFETY: We're consuming the Responder and aren't in Drop.
        let shared = unsafe { self.shared.take() };

        // SAFETY: The responder owns this field until we mark that the response is set.
        unsafe {
            (*shared.data.response.get()).write(resp);
        }

        // Mark the response is set. We can't touch response anymore except to read it.
        shared.data.state.set_value();

        // Wake everyone up. If there's lots of pipelines to resolve, it might take a bit.
        // Under tokio multi-threading this might mean some work can happen on other threads.
        shared.data.waiters.wake_all();

        // SAFETY: The responder owns this field until we mark that the pipeline has been written.
        unsafe {
            let _ = &*(*shared.data.pipeline_dest.get()).write(None);
        }

        // SAFETY: We're reading the response data we just wrote above.
        let resp = unsafe { shared.data.response() };
        let factory = ResponseReceiverFactory { shared: &shared };
        shared
            .data
            .resolve_pipeline_map(|k, c| resp.resolve(factory, k, c));

        // Pipeline resolution will mark the pipeline as written for us, even if we panic.
    }

    /// Reuse the request to perform a tail-call, turning the responder back into a request.
    pub fn tail_call(mut self, msg: C::Parameters) -> Request<C> {
        // SAFETY: We're consuming the Responder and aren't in Drop.
        let shared = unsafe { self.shared.take() };

        // SAFETY: The responder owns this field.
        unsafe {
            *shared.data.request.get() = ManuallyDrop::new(msg);
        }

        Request {
            shared: Marc::new(shared),
        }
    }

    /// Fulfill the pipeline portion of the request, allowing pipelined requests to flow though
    /// without waiting for the call to complete.
    pub fn set_pipeline(mut self, dst: C::Pipeline) -> ResultsSender<C> {
        // SAFETY: We're consuming the Responder and aren't in Drop.
        let shared = unsafe { self.shared.take() };

        // SAFETY: The responder owns this field until we mark that the pipeline has been written.
        unsafe {
            let _ = &*(*shared.data.pipeline_dest.get()).write(Some(dst));
        }

        // SAFETY: We're reading the response data we just wrote above.
        let resp = unsafe { shared.data.response() };
        let factory = ResponseReceiverFactory { shared: &shared };
        shared
            .data
            .resolve_pipeline_map(|k, c| resp.resolve(factory, k, c));

        // Pipeline resolution will mark the pipeline as written for us, even if we panic.

        ResultsSender {
            shared: Marc::new(shared),
        }
    }

    pub fn finished(&self) -> Finished<C> {
        Finished::new(self.shared.get().clone())
    }

    pub fn is_finished(&self) -> bool {
        self.shared.get().data.is_finished()
    }
}

impl<C: Chan> Drop for Responder<C> {
    fn drop(&mut self) {
        if let Some(shared) = self.shared.try_take() {
            shared.data.close_sender()
        }
    }
}

/// The result of calling `Responder::set_pipeline`, a type that can only be used to send the
/// results of a call.
pub struct ResultsSender<C: Chan> {
    shared: Marc<RequestInner<C>>,
}

impl<C: Chan> ResultsSender<C> {
    /// Returns the given response.
    pub fn respond(mut self, resp: C::Results) {
        // SAFETY: We're consuming the ResultsSender and aren't in Drop.
        let shared = unsafe { self.shared.take() };

        // SAFETY: The ResultsSender owns this field until we mark that the response is set.
        unsafe {
            (*shared.data.response.get()).write(resp);
        }

        shared.data.state.set_value();
        shared.data.waiters.wake_all();
    }

    pub fn finished(&self) -> Finished<C> {
        Finished::new(self.shared.get().clone())
    }

    pub fn is_finished(&self) -> bool {
        self.shared.get().data.is_finished()
    }
}

impl<C: Chan> Drop for ResultsSender<C> {
    fn drop(&mut self) {
        if let Some(shared) = self.shared.try_take() {
            shared.data.close_sender();
        }
    }
}

/// A response receiver that can be used to await the response.
pub struct ResponseReceiver<C: Chan> {
    shared: Receiver<C>,
}

impl<C: Chan> ResponseReceiver<C> {
    /// Attempts to receive the value. This returns an owned copy of the value that can be used separately from the receiver.
    pub fn try_recv(&self) -> Result<Response<C>, TryRecvError> {
        match self.shared.0.data.state.state() {
            ShotState::Closed => Err(TryRecvError::Closed),
            ShotState::Empty => Err(TryRecvError::Empty),
            ShotState::Sent => Ok(Response {
                shared: self.shared.0.clone(),
            }),
        }
    }

    pub fn try_ref(&self) -> Result<&C::Results, TryRecvError> {
        match self.shared.0.data.state.state() {
            ShotState::Closed => Err(TryRecvError::Closed),
            ShotState::Empty => Err(TryRecvError::Empty),
            ShotState::Sent => Ok(unsafe { self.shared.0.data.response() }),
        }
    }

    pub fn recv(self) -> Recv<C> {
        Recv::new(self.shared)
    }
}

impl<C: Chan> Clone for ResponseReceiver<C> {
    fn clone(&self) -> Self {
        Self {
            shared: self.shared.clone(),
        }
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
        let shared = &this.shot.0;
        let req = &shared.data;

        if this.state.is_ready() {
            return this.state.clone();
        }

        let value = match this.waiter.poll(ctx, &req.state, &req.waiters) {
            ShotState::Empty => return Poll::Pending,
            ShotState::Sent => Some(Response {
                shared: shared.clone(),
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

        this.waiter.pinned_drop(&this.shot.0.data.waiters);
    }
}

pub struct Response<C: Chan> {
    shared: Arc<RequestInner<C>>,
}

impl<C: Chan> Response<C> {
    fn get(&self) -> &C::Results {
        // SAFETY: We only construct a Response when we know the inner value is set.
        unsafe { (*self.shared.data.response.get()).assume_init_ref() }
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
        let shared = &self.shared.0;
        let req = &shared.data;

        if !req.state.load(Acquire).is_pipeline_set() {
            // The pipeline hasn't been set yet. Lock the pipeline map so we can get a
            // pipelined client.
            let mut map = req.pipeline_map.lock();
            // Load relaxed since the mutex lock provides the synchronization we need
            // if the pipeline does get set.
            if !req.state.load(Relaxed).is_pipeline_set() {
                // It *still* hasn't been set, time to mutate the map
                let sender = match map.raw_entry_mut().from_key(&key) {
                    RawEntryMut::Occupied(mut occupied) => match occupied.get().sender() {
                        Some(sender) => sender,
                        None => {
                            let receiver = Receiver::track(shared.clone());
                            let (sender, weak_channel) = weak_channel(chan(), receiver);
                            let _ = occupied.insert(weak_channel);
                            sender
                        }
                    },
                    RawEntryMut::Vacant(vacant) => {
                        let receiver = Receiver::track(shared.clone());
                        let (sender, weak_channel) = weak_channel(chan(), receiver);
                        let _ = vacant.insert(C::PipelineKey::from(key), weak_channel);
                        sender
                    }
                };

                return sender;
            }
        }

        let factory = ResponseReceiverFactory { shared };

        let key = key.into();

        // Ah, a pipeline was set, so we can apply the ops and resolve directly into the
        // correct client.
        let pipeline = unsafe { (*req.pipeline_dest.get()).assume_init_ref() };
        match pipeline {
            Some(p) => p.pipeline(factory, key),
            None => unsafe { req.response().pipeline(factory, key) },
        }
    }
}

impl<C: Chan> Clone for PipelineBuilder<C> {
    fn clone(&self) -> Self {
        Self {
            shared: self.shared.clone(),
        }
    }
}
