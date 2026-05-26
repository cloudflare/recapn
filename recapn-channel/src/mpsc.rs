//! An mpsc of requests and responses. This queue type is the core primitive behind
//! requests and responses in recapn-rpc. It supports many operations not found in
//! other primitives such as tokio mpsc and has many fewer allocations than building one
//! based on those primitives.
//!
//! A main benefit to this channel over a normal tokio mpsc channel is that channels of
//! this type can be forwarded to others at extremely low cost. This allows us to do
//! promise pipelining without needing to have forwarding tasks between queues. Instead
//! a pipelining queue can be made and then attached to the resolved queue when the
//! promise has resolved.
//!
//! Another benefit is that this channel doesn't need separate allocations for each step
//! in the request pipeline. Building off of tokio primitives would necessitate having
//! many allocations for sending requests and returning responses over one-shots, but
//! this set of types combines the channel queue with the oneshot mechanism and makes it
//! so one request makes one allocation.
//! 
//! To facilitate operations like embargoes, the mpsc also supports sending generic "events"
//! alongside standard requests. Events are custom data and have the same drop behavior as
//! requests, but instead of using response receivers as their indicator to stay alive,
//! events use EventRef instances. EventRefs can't access the event data, only keep the event
//! alive to be delivered.

use std::cell::UnsafeCell;
use std::fmt::{self, Debug};
use std::future::{poll_fn, Future};
use std::hash::{Hash, Hasher};
use std::marker::PhantomData;
use std::mem::{ManuallyDrop, MaybeUninit};
use std::pin::Pin;
use std::process::abort;
use std::ptr::{NonNull, addr_of, addr_of_mut};
use std::sync::atomic::AtomicUsize;
use std::sync::atomic::Ordering::{Acquire, Relaxed};
use std::sync::{Arc, Weak};
use std::task::{Context, Poll, Waker};

use parking_lot::{Mutex, MutexGuard};
use pin_project::{pin_project, pinned_drop};

use crate::request::{self, Request, RequestData};
use crate::util::array_vec::ArrayVec;
use crate::util::atomic_option_arc::AtomicOptionArc;
use crate::util::atomic_state::{AtomicState, ShotState};
use crate::util::closed_task::ClosedTask;
use crate::util::linked_list::{Link, LinkedList, Pointers};
use crate::util::wait_list::{RecvWaiter, WaitList};
use crate::{Chan, IntoResults};

#[derive(Clone, Copy, Debug)]
pub(crate) enum MessageKind {
    Request,
    Event,
}

/// The base message struct for an item in an mpsc channel.
/// 
/// This maintains 4 variables that all messages contain:
/// 
///  * The linked list pointers for the elements of the channel
///  * The parent channel the message is contained in
///  * The amount of interest in the message
///  * The data associated with the message
#[repr(C)]
pub(crate) struct SharedMessage<C: Chan, T: ?Sized> {
    /// Intrusive linked-list pointers for request channels
    ///
    /// In order to maintain "one allocation per request", we intrusively link
    /// requests together to build a request chain.
    pointers: Pointers<SharedMessage<C, ()>>,

    /// The parent channel of this link.
    parent: AtomicOptionArc<SharedChannel<C>>,

    /// The amount of "interest" in this message. When this value reaches zero, the message will be
    /// automatically released from its active channel and dropped. If it has no active channel,
    /// it will be dropped when it's put into one.
    ///
    /// Anything with interest includes things like response receivers, pipelines, and any
    /// pipelined clients that are waiting for the response to be received.
    interest_count: AtomicUsize,

    kind: MessageKind,

    pub data: T,
}

impl<C: Chan> request::SharedRequest<C> {
    pub fn new(
        request: C::Parameters,
        usage: request::RequestUsage,
        receivers: usize,
    ) -> Arc<Self> {
        Arc::new(Self {
            pointers: Pointers::new(),
            kind: MessageKind::Request,
            parent: AtomicOptionArc::none(),
            interest_count: AtomicUsize::new(receivers),
            data: RequestData::new(request, usage),
        })
    }
}

impl<C: Chan> SharedEvent<C> {
    pub fn new(data: C::Event, interest: usize) -> Arc<Self> {
        Arc::new(Self {
            pointers: Pointers::new(),
            kind: MessageKind::Event,
            parent: AtomicOptionArc::none(),
            interest_count: AtomicUsize::new(interest),
            data: EventData::new(data),
        })
    }
}

impl<C: Chan, T: ?Sized> SharedMessage<C, T> {
    /// Set the parent channel for this message.
    ///
    /// This should only be called while holding a lock on the specified channel.
    pub unsafe fn set_parent(&self, sender: Sender<C>) -> Option<Sender<C>> {
        self.parent.replace(Some(sender.into_shared())).map(Sender::from_shared)
    }

    /// Take back the parent sender for this message.
    ///
    /// This should only be called while holding a lock on the channel holding this message.
    pub unsafe fn take_parent(&self) -> Option<Sender<C>> {
        self.parent.take().map(Sender::from_shared)
    }

    pub fn add_interest(&self) {
        let old = self.interest_count.fetch_add(1, Relaxed);

        if old == usize::MAX {
            std::process::abort();
        }

        // Make sure I don't accidentally attempt to re-add interest.
        debug_assert_ne!(old, 0);
    }

    /// Removes interest in the message, returning a bool indicating if the message has any
    /// interest remaining.
    pub fn remove_interest(&self) -> bool {
        let old = self.interest_count.fetch_sub(1, Relaxed);
        old - 1 == 0
    }

    pub fn has_interest(&self) -> bool {
        self.interest_count.load(Relaxed) != 0
    }

    pub fn remove_self_from_channel(&self) {
        loop {
            let Some(parent) = self.parent.add_ref() else {
                // No parent!
                return
            };

            let mut lock = match parent.resolve_and_lock() {
                // We've locked the channel, now check to see if our parent was updated.
                Ok((_, lock)) if self.parent.same_as(Some(&parent)) => lock,
                // The parent was updated, let's retry.
                Ok(_) => continue,
                // The parent has resolved into a terminal resolution, which means the receiver
                // must be dropping all the messages from the channel anyway. So we can just
                // return now.
                Err(_) => return,
            };

            let ptr = NonNull::from_ref(self).cast::<SharedMessage<C, ()>>();
            let Some(link) = (unsafe { lock.messages.remove(ptr) }) else {
                panic!("message wasn't contained in locked parent")
            };

            drop(link.into_item());

            return
        }
    }
}

struct LinkPtr<C: Chan> {
    ptr: *const SharedMessage<C, ()>,
}

impl<C: Chan> LinkPtr<C> {
    /// Create a link pointer from a request that has already had the parent channel set.
    pub fn from_parented_request(req: Request<C>) -> Self {
        Self { ptr: Arc::into_raw(req.into_shared()).cast() }
    }

    /// Create a link pointer from an event that has already had the parent channel set.
    pub fn from_parented_event(event: Event<C>) -> Self {
        Self { ptr: Arc::into_raw(event.into_shared()).cast() }
    }

    pub fn into_item(self) -> ItemWithSender<C> {
        let sender;
        let item = unsafe {
            match *addr_of!((*self.ptr).kind) {
                MessageKind::Request => {
                    let shared = Arc::from_raw(self.ptr.cast::<request::SharedRequest<C>>());
                    sender = shared.take_parent().expect("missing parent channel from linked message");
                    Item::Request(Request::from_shared(shared))
                }
                MessageKind::Event => {
                    let shared = Arc::from_raw(self.ptr.cast::<SharedEvent<C>>());
                    sender = shared.take_parent().expect("missing parent channel from linked message");
                    Item::Event(Event::from_shared(shared))
                }
            }
        };
        ItemWithSender { item, sender }
    }
}

unsafe impl<C: Chan> Link for LinkPtr<C> {
    type Handle = Self;
    type Target = SharedMessage<C, ()>;

    fn as_raw(handle: &Self::Handle) -> NonNull<Self::Target> {
        NonNull::new(handle.ptr.cast_mut()).unwrap()
    }
    unsafe fn from_raw(ptr: NonNull<Self::Target>) -> Self::Handle {
        Self {
            ptr: ptr.as_ptr().cast_const(),
        }
    }
    unsafe fn pointers(target: NonNull<Self::Target>) -> NonNull<Pointers<Self::Target>> {
        let me = target.as_ptr();
        let field = addr_of_mut!((*me).pointers);
        NonNull::new_unchecked(field)
    }
}

pub(crate) struct GuardedChannel<C: Chan> {
    /// If this is a pipeline channel, this holds a reference back to the parent request to
    /// make sure it doesn't go out of scope. The parent request itself holds a weak pointer
    /// to this channel to make sure the reference cycle is broken.
    parent_request: Option<request::Receiver<C>>,

    messages: LinkedList<LinkPtr<C>, SharedMessage<C, ()>>,

    /// The waker set by the receiver to wake up the receiver task.
    waker: Option<Waker>,
}

enum ChannelResolution<C: Chan> {
    Forward(Sender<C>),
    Error(C::Error),
}

/// The shared state behind a request queue
pub(crate) struct SharedChannel<C: Chan> {
    /// Intrusive linked-list pointers for channels owned by a ReceiverSet. This field is owned
    /// by the parent set and can't be accessed without locking it first.
    set_link_pointers: Pointers<SharedChannel<C>>,

    /// An pointer back to the channel set that owns the channel.
    parent_set: AtomicOptionArc<SharedChannelSet<C>>,

    /// The mutex guarded state of the channel. This includes things like the request linked list,
    /// waker, and any possible owners of the channel, like a parent request or parent channel set.
    /// 
    /// Note: In the case that a channel is owned by a channel set and the set and the channel need
    /// to be locked at the same time, the channel's mutex should be locked *after* the set's mutex.
    /// When doing work on the channel, you'll likely find cases where you optimistically lock the
    /// channel first
    guarded_state: Mutex<GuardedChannel<C>>,

    /// The state of the channel.
    atomic_state: AtomicState,

    /// A set of waiters waiting to receive the value.
    waiters: WaitList,

    /// The number of senders waiting for resolution. When this value reaches zero,
    /// the channel is closed and the closed task is woken up. Note, all senders and requests in
    /// the channel are considered receivers of the resolution result. This prevents the sharedshot
    /// from prematurely closing if all senders are dropped but requests are active in the channel.
    ///
    /// Instances where all senders have dropped but requests are still active are common
    /// in pipeline clients, where a request is sent on the pipeline, then the sender is
    /// dropped. As long as the request has receivers for its reponse, it should stay active,
    /// instead of getting canceled because the channel closed and was dropped.
    sender_count: AtomicUsize,

    /// Tracks the receiver waiting for the channel to close (without pulling a value
    /// from the channel)
    closed_task: ClosedTask,

    /// The value of the channel resolution.
    resolution: UnsafeCell<MaybeUninit<ChannelResolution<C>>>,

    chan: C,
}

unsafe impl<C: Chan> Send for SharedChannel<C> {}
unsafe impl<C: Chan> Sync for SharedChannel<C> {}

impl<C: Chan + Debug> Debug for SharedChannel<C> {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        f.debug_struct("SharedChannel")
            .field("chan", &self.chan)
            .field("state", &"...")
            .field("resolution", &"...")
            .finish()
    }
}

impl<C: Chan> SharedChannel<C> {
    fn new(chan: C, senders: usize, parent: Option<request::Receiver<C>>) -> Self {
        Self {
            set_link_pointers: Pointers::new(),
            parent_set: AtomicOptionArc::none(),
            guarded_state: Mutex::new(GuardedChannel {
                parent_request: parent,
                messages: LinkedList::new(),
                waker: None,
            }),
            atomic_state: AtomicState::new(),
            waiters: WaitList::new(),
            sender_count: AtomicUsize::new(senders),
            closed_task: ClosedTask::new(),
            resolution: UnsafeCell::new(MaybeUninit::uninit()),
            chan,
        }
    }

    fn resolved(chan: C, senders: usize, resolution: ChannelResolution<C>) -> Self {
        Self {
            set_link_pointers: Pointers::new(),
            parent_set: AtomicOptionArc::none(),
            guarded_state: Mutex::new(GuardedChannel {
                parent_request: None,
                messages: LinkedList::new(),
                waker: None,
            }),
            atomic_state: AtomicState::new_set(),
            waiters: WaitList::new(),
            sender_count: AtomicUsize::new(senders),
            closed_task: ClosedTask::new(),
            resolution: UnsafeCell::new(MaybeUninit::new(resolution)),
            chan,
        }
    }

    fn dropped(chan: C) -> Self {
        Self {
            set_link_pointers: Pointers::new(),
            parent_set: AtomicOptionArc::none(),
            guarded_state: Mutex::new(GuardedChannel {
                parent_request: None,
                messages: LinkedList::new(),
                waker: None,
            }),
            atomic_state: AtomicState::new_send_closed(),
            waiters: WaitList::new(),
            sender_count: AtomicUsize::new(1),
            closed_task: ClosedTask::new(),
            resolution: UnsafeCell::new(MaybeUninit::uninit()),
            chan,
        }
    }

    fn state(&self) -> ShotState {
        self.atomic_state.state()
    }

    fn is_closed(&self) -> bool {
        self.atomic_state.load(Relaxed).is_recv_closed()
    }

    fn send_request(
        self: &Arc<Self>,
        req: Request<C>,
    ) -> Result<(), (Request<C>, Option<&C::Error>)> {
        let (real, mut channel) = match self.resolve_and_lock() {
            Ok(v) => v,
            Err(r) => {
                return match r {
                    MostResolved::Dropped => Err((req, None)),
                    MostResolved::Error(err) => return Err((req, Some(err))),
                }
            }
        };

        let req_shared = req.as_shared().clone();
        // SAFETY: We're holding the lock on this channel
        unsafe {
            req_shared.set_parent(self.clone().into_new_sender());
        }
        let link_ptr = LinkPtr::from_parented_request(req);

        // Insert the request into the list.
        channel.messages.push_back(link_ptr);

        if req_shared.is_finished() {
            // Just drop the request nobody wants the result
            let _ = channel.messages.pop_back().unwrap().into_item();
            return Ok(());
        }

        let waker = channel.waker.take();
        drop(channel);

        if let Some(w) = waker {
            w.wake();
        }

        if let Some(parent_set) = real.parent_set.add_ref() {
            parent_set.ready_channel(real);
        }

        Ok(())
    }

    fn send_event(
        self: &Arc<Self>,
        event: Event<C>,
    ) -> Result<(), (Event<C>, Option<&C::Error>)> {
        let (_, mut channel) = match self.resolve_and_lock() {
            Ok(v) => v,
            Err(r) => {
                return match r {
                    MostResolved::Dropped => Err((event, None)),
                    MostResolved::Error(err) => return Err((event, Some(err))),
                }
            }
        };

        let shared_event = event.as_shared().clone();
        unsafe {
            event.as_shared().set_parent(self.clone().into_new_sender());
        }

        channel
            .messages
            .push_back(LinkPtr::from_parented_event(event));

        if !shared_event.has_interest() {
            let _ = channel.messages.pop_back().unwrap().into_item();
            return Ok(())
        }

        let waker = channel.waker.take();
        drop(channel);

        if let Some(w) = waker {
            w.wake();
        }

        if let Some(parent_set) = self.parent_set.add_ref() {
            parent_set.ready_channel(self);
        }

        Ok(())
    }

    unsafe fn add_sender(&self) {
        let old = self.sender_count.fetch_add(1, Relaxed);
        if old == usize::MAX {
            abort();
        }

        // Make sure I don't accidentally attempt to re-open the channel.
        debug_assert_ne!(old, 0);
    }

    fn try_add_sender(&self) -> bool {
        self.sender_count.fetch_update(Acquire, Relaxed, |c| {
            if c == 0 {
                return None
            }

            Some(c + 1)
        }).is_ok()
    }

    /// Remove a tracked receiver from the receiver count.
    ///
    /// If this is the last receiver (and the receiver count is zero), this closes the channel on
    /// the receiving side.
    ///
    /// Note: A channel cannot be re-opened by adding a receiver when the channel is closed.
    unsafe fn remove_sender(&self) {
        let last_receiver = (self.sender_count.fetch_sub(1, Relaxed) - 1) == 0;
        if last_receiver {
            self.closed_task.close(&self.atomic_state);

            // Wake up the receiver so it can notice the channel is closed.
            let waker = self.guarded_state.lock().waker.take();
            if let Some(waker) = waker {
                waker.wake();
            }
        }
    }

    fn into_new_sender(self: Arc<Self>) -> Sender<C> {
        Sender::new(self)
    }

    fn into_existing_sender(self: Arc<Self>) -> Sender<C> {
        Sender::from_shared(self)
    }

    fn sender_poll_closed(&self, cx: &mut Context<'_>) -> Poll<()> {
        self.closed_task.poll(&self.atomic_state, cx)
    }

    /// Close the receiver without resolving.
    unsafe fn close_receiver(&self) {
        self.atomic_state.set_send_closed();
        self.waiters.wake_all();
    }

    unsafe fn get_ref_unchecked(&self) -> &ChannelResolution<C> {
        (*self.resolution.get()).assume_init_ref()
    }

    unsafe fn resolve(&self, value: ChannelResolution<C>) {
        let value_store = &mut *self.resolution.get();
        value_store.write(value);

        let prev = self.atomic_state.try_set_value();
        if prev.is_recv_closed() {
            value_store.assume_init_drop();

            return;
        }

        self.waiters.wake_all();
    }

    fn try_resolved(&self) -> Option<Resolution<'_, C>> {
        match self.state() {
            ShotState::Closed => Some(Resolution::Dropped),
            ShotState::Empty => None,
            ShotState::Sent => {
                let res = unsafe { self.get_ref_unchecked() };
                Some(match res {
                    ChannelResolution::Forward(channel) => Resolution::Forwarded(channel),
                    ChannelResolution::Error(err) => Resolution::Error(err),
                })
            }
        }
    }

    pub fn most_resolved<'a>(
        mut self: &'a Arc<Self>,
    ) -> (&'a Arc<Self>, Option<MostResolved<'a, C::Error>>) {
        loop {
            let resolution = self.try_resolved();
            return match resolution {
                None => (self, None),
                Some(Resolution::Dropped) => (self, Some(MostResolved::Dropped)),
                Some(Resolution::Error(err)) => (self, Some(MostResolved::Error(err))),
                Some(Resolution::Forwarded(channel)) => {
                    self = &channel.shared;
                    continue;
                }
            };
        }
    }

    /// Resolve into the inner-most forwarded channel and acquire a lock to the channel
    pub fn resolve_and_lock<'a>(
        self: &'a Arc<Self>,
    ) -> Result<(&'a Arc<Self>, MutexGuard<'a, GuardedChannel<C>>), MostResolved<'a, C::Error>> {
        // First we need to find the most resolved version of this channel.
        let (mut this, mut resolution) = self.most_resolved();

        loop {
            if let Some(res) = resolution {
                return Err(res);
            }

            // Now we have the most resolved channel, lock it.
            let channel = this.guarded_state.lock();

            // But during that time spent locking it, it might've resolved further, so go through
            // the above steps again...
            (this, resolution) = this.most_resolved();

            if resolution.is_none() {
                // If it hasn't resolved, break from the loop.
                break Ok((this, channel));
            }
        }
    }

    fn forward_to(self: &Arc<Self>, mut other: &Arc<Self>) -> Result<(), ()> {
        // Keep track of the original so that we can determine resolution chains.
        let original = other;
        let mut resolution;
        (other, resolution) = other.most_resolved();
        let waker;
        loop {
            if let Some(r) = resolution {
                let err = match r {
                    MostResolved::Dropped => None,
                    MostResolved::Error(err) => Some(err.clone()),
                };
                self.resolve_and_close(err);
                return Ok(());
            }

            // Declare the lock variables separately from the named locks. This is done to make sure
            // the locks are also *dropped* in a consistent order, as Rust invokes Drop on variables
            // in reverse declaration order. This way, lock_b is always unlocked first, followed
            // by lock_a. self_lock and other_lock are always mutable borrows to their proper locks.
            let mut lock_a;
            let mut lock_b;
            let self_lock;
            let other_lock;

            let self_ptr = core::ptr::from_ref(self.as_ref());
            let other_ptr = core::ptr::from_ref(other.as_ref());
            match self_ptr.cmp(&other_ptr) {
                std::cmp::Ordering::Less => {
                    lock_a = self.guarded_state.lock();
                    lock_b = other.guarded_state.lock();
                    self_lock = &mut lock_a;
                    other_lock = &mut lock_b;
                }
                std::cmp::Ordering::Greater => {
                    lock_a = other.guarded_state.lock();
                    lock_b = self.guarded_state.lock();
                    self_lock = &mut lock_b;
                    other_lock = &mut lock_a;
                }
                std::cmp::Ordering::Equal => return Err(()),
            };

            (other, resolution) = other.most_resolved();

            // Continue the loop if other resolved while we were acquiring locks.
            if resolution.is_some() {
                continue;
            }

            self_lock.waker = None;

            unsafe {
                // Use the original channel so we can maintain resolution chains.
                self.resolve(ChannelResolution::Forward(original.clone().into_new_sender()));
            }

            // There's no requests to forward, so just return early. This way we won't wake up the
            // other receiver for no reason.
            if self_lock.messages.is_empty() {
                return Ok(());
            }

            other_lock.messages.append_back(&mut self_lock.messages);
            waker = other_lock.waker.take();
            break;
        }

        if let Some(waker) = waker {
            waker.wake();
        }

        if let Some(parent_set) = other.parent_set.add_ref() {
            parent_set.ready_channel(other);
        }

        Ok(())
    }

    fn resolve_and_close(&self, err: Option<C::Error>) {
        let mut self_lock = self.guarded_state.lock();
        drop(self_lock.waker.take());
        unsafe {
            if let Some(err) = &err {
                self.resolve(ChannelResolution::Error(err.clone()));
            } else {
                self.close_receiver();
            }
        }

        scopeguard::defer_on_unwind! {
            let mut self_lock = self.guarded_state.lock();
            while let Some(r) = self_lock.messages.pop_front() {
                drop(r.into_item());
            }
        };

        let mut request_array = ArrayVec::<Item<C>, 32>::new();

        let respond_with_err = |i: Item<C>| {
            if let Some(err) = &err {
                if let Item::Request(request) = i {
                    let (_, responder) = request.respond();
                    responder.respond(err.clone().into_results());
                }
            }
        };

        'outer: loop {
            while request_array.can_push() {
                let Some(ptr) = self_lock.messages.pop_front() else {
                    break 'outer;
                };

                request_array.push(ptr.into_item().item);
            }

            drop(self_lock);

            request_array.for_each(respond_with_err);

            self_lock = self.guarded_state.lock();
        }

        drop(self_lock);

        request_array.for_each(respond_with_err);
    }

    fn drop_receiver(&self) {
        let state = self.atomic_state.load(Relaxed);
        if !state.is_set() {
            unsafe {
                self.close_receiver();
            }
        }
    }
}

impl<C: Chan> Drop for SharedChannel<C> {
    fn drop(&mut self) {
        let state = self.atomic_state.get();

        if state.is_set() {
            unsafe { self.resolution.get_mut().assume_init_drop() }
        }

        if state.is_closed_task_set() {
            unsafe {
                self.closed_task.drop();
            }
        }
    }
}

struct ChannelInSet<C: Chan> {
    ptr: NonNull<SharedChannel<C>>,
}

unsafe impl<C: Chan> Link for ChannelInSet<C> {
    type Handle = Self;
    type Target = SharedChannel<C>;

    fn as_raw(handle: &Self::Handle) -> NonNull<Self::Target> {
        handle.ptr
    }
    unsafe fn from_raw(ptr: NonNull<Self::Target>) -> Self::Handle {
        Self { ptr }
    }
    unsafe fn pointers(target: NonNull<Self::Target>) -> NonNull<Pointers<Self::Target>> {
        let me = target.as_ptr();
        let field = addr_of_mut!((*me).set_link_pointers);
        NonNull::new_unchecked(field)
    }
}

struct GuardedChannelSet<C: Chan> {
    idle: LinkedList<ChannelInSet<C>, SharedChannel<C>>,
    ready: LinkedList<ChannelInSet<C>, SharedChannel<C>>,
    waker: Option<Waker>,
}

impl<C: Chan> GuardedChannelSet<C> {
    fn wake_up(&self) {
        if let Some(waker) = &self.waker {
            waker.wake_by_ref();
        }
    }

    fn pop_receiver(&mut self) -> Option<Receiver<C>> {
        let channel = self.ready.pop_front().or_else(|| self.idle.pop_front())?;
        let channel = unsafe { Arc::from_raw(channel.ptr.as_ptr().cast_const()) };
        let guarded_channel = channel.guarded_state.lock();
        channel.parent_set.replace(None);
        channel.atomic_state.clear_ready(Relaxed);
        drop(guarded_channel);
        Some(Receiver { shared: ManuallyDrop::new(channel) })
    }

    /// Pop the ready list until it yields an item or a closed channel.
    fn pop_ready(&mut self) -> Option<(NonNull<SharedChannel<C>>, Option<ItemWithSender<C>>)> {
        loop {
            let channel = self.ready.pop_front()?;
            let channel_ptr = channel.ptr;
            let channel_ref = unsafe { channel_ptr.as_ref() };
            let mut guarded_channel = channel_ref.guarded_state.lock();
            let Some(next_request) = guarded_channel.messages.pop_front() else {
                if channel_ref.is_closed() {
                    // The channel is closed, so we can tear it down and remove it.
                    channel_ref.parent_set.replace(None);
                    channel_ref.atomic_state.clear_ready(Relaxed);
                    return Some((channel_ptr, None))
                } else {
                    // Oh, it's not closed, but it's empty? Put it back in the idle list,
                    // must be a fluke.
                    channel_ref.atomic_state.clear_ready(Relaxed);
                    self.idle.push_back(channel);
                    continue;
                }
            };
            if !guarded_channel.messages.is_empty() || channel_ref.is_closed() {
                // If we're ready still, put it back into the ready list.
                self.ready.push_back(channel);
                self.wake_up();
            } else {
                channel_ref.atomic_state.clear_ready(Relaxed);
            }
            return Some((channel_ptr, Some(next_request.into_item())));
        }
    }
}

struct SharedChannelSet<C: Chan> {
    guarded: Mutex<GuardedChannelSet<C>>,
}

impl<C: Chan> SharedChannelSet<C> {
    fn insert(self: &Arc<Self>, channel: Arc<SharedChannel<C>>) {
        let channel_ptr = NonNull::new(Arc::into_raw(channel).cast_mut()).unwrap();
        // SAFETY: We just got this pointer from Arc::into_raw
        let channel_ref = unsafe { channel_ptr.as_ref() };

        let self_strong = Arc::clone(self);

        let mut set_guard = self.guarded.lock();
        let mut channel_guard = channel_ref.guarded_state.lock();

        // Clean up our channel state by removing any old wakers, setting the parent set to our
        // weak pointer, and writing our atomic parent set pointer.
        channel_guard.waker = None;
        channel_ref.parent_set.replace(Some(self_strong));

        // If the channel is already in a state to be actioned, put it
        // immediately in the ready state.
        if !channel_guard.messages.is_empty() || channel_ref.is_closed() {
            set_guard.ready.push_back(ChannelInSet { ptr: channel_ptr });
            channel_ref.atomic_state.set_ready(Relaxed);
            set_guard.wake_up();
        } else {
            set_guard.idle.push_back(ChannelInSet { ptr: channel_ptr });
        }
    }

    fn remove_by_ref(self: &Arc<Self>, channel: &SharedChannel<C>) -> bool {
        let channel_ptr = NonNull::from(channel);

        let mut set_guard = self.guarded.lock();

        if !channel.parent_set.same_as(Some(self)) {
            return false
        }

        let _channel_guard = channel.guarded_state.lock();
        channel.parent_set.replace(None);
        let old_flags = channel.atomic_state.clear_ready(Relaxed);

        unsafe {
            if old_flags.is_ready_set() {
                set_guard.ready.remove(channel_ptr);
            } else {
                set_guard.idle.remove(channel_ptr);
            }
        }

        true
    }

    fn remove_all(self: &Arc<Self>) -> Vec<Receiver<C>> {
        let mut receivers = Vec::new();

        'outer: loop {
            let mut guarded = self.guarded.lock();
            // Only pull 10 items from the set at a time to give an opportunity for other operations
            // to occur on the set.
            for _ in 0..10 {
                let Some(recv) = guarded.pop_receiver() else {
                    break 'outer;
                };
                receivers.push(recv);
            }
        }

        receivers
    }

    fn poll_recv<'a>(self: &'a Arc<Self>, cx: &mut Context<'_>) -> Poll<SetRecvResult<'a, C>> {
        let mut guarded = self.guarded.lock();
        let Some((channel, item)) = guarded.pop_ready() else {
            let cx_waker = cx.waker();
            if let Some(w) = &mut guarded.waker {
                w.clone_from(cx_waker);
            } else {
                guarded.waker = Some(cx_waker.clone());
            };
            return Poll::Pending
        };
        let result = match item {
            Some(ItemWithSender { item, sender }) => SetRecvResult::Item {
                receiver: ReceiverKeyRef {
                    channel: channel.as_ptr().cast_const(),
                    set: self,
                },
                item,
                sender,
            },
            None => SetRecvResult::Closed {
                receiver: Receiver {
                    shared: ManuallyDrop::new(unsafe {
                        Arc::from_raw(channel.as_ptr().cast_const())
                    })
                }
            }
        };

        Poll::Ready(result)
    }

    fn try_recv(self: &Arc<Self>) -> Option<SetRecvResult<'_, C>> {
        let (channel, item) = self.guarded.lock().pop_ready()?;
        let result = match item {
            Some(ItemWithSender { item, sender }) => SetRecvResult::Item {
                receiver: ReceiverKeyRef {
                    channel: channel.as_ptr().cast_const(),
                    set: self,
                },
                item,
                sender,
            },
            None => SetRecvResult::Closed {
                receiver: Receiver {
                    shared: ManuallyDrop::new(unsafe {
                        Arc::from_raw(channel.as_ptr().cast_const())
                    })
                }
            }
        };
        Some(result)
    }

    fn ready_channel(mut self: Arc<Self>, channel: &SharedChannel<C>) {
        let channel_ptr = NonNull::from(channel);

        let mut set_guard = loop {
            let set_guard = self.guarded.lock();

            if !channel.parent_set.same_as(Some(&self)) {
                drop(set_guard);
                let _channel_guard = channel.guarded_state.lock();
                let new_set = channel.parent_set.add_ref();
                let Some(new_self) = new_set else {
                    return;
                };

                self = new_self;
                continue
            }

            break set_guard
        };

        let old_state = channel.atomic_state.set_ready(Relaxed);
        if !old_state.is_ready_set() {
            unsafe {
                set_guard.idle.remove(channel_ptr);
            }
            set_guard.ready.push_back(ChannelInSet { ptr: channel_ptr });
        }

        set_guard.wake_up();

    }
}

struct EventData<E> {
    data: UnsafeCell<ManuallyDrop<E>>,
}

impl<E> EventData<E> {
    fn new(data: E) -> Self {
        Self { data: UnsafeCell::new(ManuallyDrop::new(data)) }
    }

    unsafe fn get(&self) -> &E {
        &*self.data.get()
    }
    unsafe fn get_mut(&self) -> &mut E {
        &mut *self.data.get()
    }
    unsafe fn take_data(&self) -> E {
        ManuallyDrop::take(&mut *self.data.get())
    }

    unsafe fn drop_data(&self) {
        ManuallyDrop::drop(&mut *self.data.get())
    }
}

type SharedEvent<C> = SharedMessage<C, EventData<<C as Chan>::Event>>;

/// An message that can be sent on a mpsc channel. This follows the same ordering as requests, but
/// without the extra data of requests, like responses and pipelines, allowing simple messages to
/// be sent along the request path.
pub struct Event<C: Chan> {
    inner: ManuallyDrop<Arc<SharedEvent<C>>>,
}

impl<C: Chan> Event<C> {
    fn from_shared(shared: Arc<SharedEvent<C>>) -> Self {
        Self { inner: ManuallyDrop::new(shared) }
    }
    fn into_shared(mut self) -> Arc<SharedEvent<C>> {
        let inner = unsafe { ManuallyDrop::take(&mut self.inner) };
        std::mem::forget(self);
        inner
    }
    fn as_shared(&self) -> &Arc<SharedEvent<C>> {
        &self.inner
    }

    /// Create an event with interest. When this interest is dropped, the event is automatically
    /// released from its current channel.
    #[inline]
    pub fn new(data: C::Event) -> (Self, EventInterest<C>) {
        let shared = SharedEvent::new(data, 1);
        let interest = EventInterest { inner: shared.clone() };
        let event = Self::from_shared(shared);
        (event, interest)
    }

    /// Create an event with inherent interest. This will never be released automatically
    /// from whatever channel it's placed in.
    #[inline]
    pub fn with_inherent_interest(data: C::Event) -> Self {
        Self::from_shared(SharedEvent::new(data, 1))
    }

    /// Returns whether the event has any interest associated with it. If this is false,
    /// the event will be immediately dropped when sent on a channel.
    pub fn has_interest(&self) -> bool {
        self.inner.interest_count.load(Relaxed) != 0
    }

    /// Consume the event and return the underlying value.
    #[inline]
    pub fn into_inner(self) -> C::Event {
        let message = self.into_shared();
        unsafe { message.data.take_data() }
    }

    /// Get a shared reference to the underlying event value.
    #[inline]
    pub fn get(&self) -> &C::Event {
        unsafe { self.inner.data.get() }
    }

    /// Get a mutable reference to the underlying event value.
    #[inline]
    pub fn get_mut(&mut self) -> &mut C::Event {
        unsafe { self.inner.data.get_mut() }
    }
}

impl<C: Chan> Debug for Event<C>
where
    C::Event: Debug,
{
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        f.debug_struct("Event")
            .field("inner", &self.get())
            .finish()
    }
}

impl<C: Chan> Drop for Event<C> {
    fn drop(&mut self) {
        unsafe {
            self.inner.data.drop_data();
            ManuallyDrop::drop(&mut self.inner);
        }
    }
}

unsafe impl<C: Chan> Send for Event<C>
where
    C::Event: Send
{}

unsafe impl<C: Chan> Sync for Event<C>
where
    C::Event: Sync
{}

/// A reference to an event in a channel. This can only be used to keep the event alive. When all
/// interest is dropped the event is released automatically.
pub struct EventInterest<C: Chan> {
    inner: Arc<SharedEvent<C>>,
}

impl<C: Chan> Clone for EventInterest<C> {
    fn clone(&self) -> Self {
        let inner = self.inner.clone();
        inner.add_interest();
        Self { inner }
    }
}

impl<C: Chan> Drop for EventInterest<C> {
    fn drop(&mut self) {
        if self.inner.remove_interest() {
            self.inner.remove_self_from_channel();
        }
    }
}

/// The resolution of a channel.
/// 
/// A channel can be resolved into another channel, where all requests sent to this one are
/// forwarded to the next one, or closed, where all requests return the given terminal error.
/// This will also indicate if the Receiver was dropped without doing any resolution.
pub enum Resolution<'a, C: Chan> {
    /// The channel was forwarded to another channel
    Forwarded(&'a Sender<C>),
    /// The receiving end was dropped
    Dropped,
    /// The channel was closed with the specified error
    Error(&'a C::Error),
}

impl<'a, C: Chan> Resolution<'a, C> {
    /// Returns the forwarded channel, if the channel was forwarded to another.
    pub fn forwarded(self) -> Option<&'a Sender<C>> {
        let Self::Forwarded(f) = self else {
            return None;
        };
        Some(f)
    }

    /// Returns whether the channel was forwarded to another.
    pub fn is_forwarded(&self) -> bool {
        matches!(self, Self::Forwarded(_))
    }

    /// Returns whether the channel was dropped.
    pub fn is_dropped(&self) -> bool {
        matches!(self, Self::Dropped)
    }

    /// Returns the close error, if the channel was closed with an error.
    pub fn error(self) -> Option<&'a C::Error> {
        let Self::Error(e) = self else { return None };
        Some(e)
    }

    /// Returns whether the channel was closed with an error.
    pub fn is_error(&self) -> bool {
        matches!(self, Self::Error(_))
    }
}

impl<'a, C: Chan> Debug for Resolution<'a, C>
where
    Sender<C>: Debug,
    C::Error: Debug,
{
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        match self {
            Self::Forwarded(sender) =>
                f.debug_tuple("Forwarded")
                    .field(sender)
                    .finish(),
            Self::Dropped => f.write_str("Dropped"),
            Self::Error(err) =>
                f.debug_tuple("Error")
                    .field(err)
                    .finish()
        }
    }
}

impl<C: Chan> Clone for Resolution<'_, C> {
    #[inline]
    fn clone(&self) -> Self {
        match self {
            Resolution::Forwarded(s) => Resolution::Forwarded(s),
            Resolution::Dropped => Resolution::Dropped,
            Resolution::Error(e) => Resolution::Error(*e),
        }
    }
}
impl<C: Chan> Copy for Resolution<'_, C> {}

/// The most resolved
pub enum MostResolved<'a, E> {
    /// The receiving end was dropped
    Dropped,
    /// The channel was closed with the specified error
    Error(&'a E),
}

impl<C: Chan> Clone for MostResolved<'_, C> {
    #[inline]
    fn clone(&self) -> Self {
        match self {
            MostResolved::Dropped => MostResolved::Dropped,
            MostResolved::Error(e) => MostResolved::Error(*e),
        }
    }
}
impl<C: Chan> Copy for MostResolved<'_, C> {}

#[derive(Debug)]
pub struct Sender<C: Chan> {
    shared: ManuallyDrop<Arc<SharedChannel<C>>>,
}

impl<C: Chan> Clone for Sender<C> {
    fn clone(&self) -> Self {
        Self::new(Arc::clone(&self.shared))
    }
}

impl<C: Chan> Drop for Sender<C> {
    fn drop(&mut self) {
        unsafe {
            self.shared.remove_sender();
        }
    }
}

impl<C: Chan> Sender<C> {
    /// Create a new sender, incrementing the channel's sender count.
    fn new(shared: Arc<SharedChannel<C>>) -> Self {
        unsafe {
            shared.add_sender();
        }
        Self::from_shared(shared)
    }

    fn into_shared(mut self) -> Arc<SharedChannel<C>> {
        let inner = unsafe { ManuallyDrop::take(&mut self.shared) };
        std::mem::forget(self);
        inner
    }

    fn from_shared(shared: Arc<SharedChannel<C>>) -> Self {
        Self { shared: ManuallyDrop::new(shared) }
    }

    pub fn send(&self, req: Request<C>) -> Result<(), (Request<C>, Option<&C::Error>)> {
        self.shared.send_request(req)
    }

    pub fn send_event(
        &self,
        event: Event<C>,
    ) -> Result<(), (Event<C>, Option<&C::Error>)> {
        self.shared.send_event(event)
    }

    /// Returns whether the channel is resolved.
    pub fn is_resolved(&self) -> bool {
        self.try_resolved().is_some()
    }

    pub fn try_resolved(&self) -> Option<Resolution<'_, C>> {
        self.shared.try_resolved()
    }

    pub fn most_resolved(mut self: &Self) -> (&Self, Option<MostResolved<'_, C::Error>>) {
        loop {
            let resolution = self.try_resolved();
            return match resolution {
                None => (self, None),
                Some(Resolution::Dropped) => (self, Some(MostResolved::Dropped)),
                Some(Resolution::Error(err)) => (self, Some(MostResolved::Error(err))),
                Some(Resolution::Forwarded(channel)) => {
                    self = channel;
                    continue;
                }
            };
        }
    }

    /// Replace this sender with the most resolved sender for this channel.
    ///
    /// This does not indicate if the channel has been terminated, but can be useful in the
    /// context of RPC. Exports can't be exported already broken, they have to be exported
    /// as a promise that is immediately broken by a resolve.
    ///
    /// By calling `resolve_in_place` and only checking if the channel isn't a terminal client,
    /// a future can be made to deliver the brokenness later. This future immediately resolves
    /// and puts a event back into the connection, but generally simplifies a lot of the handling
    /// around brokenness.
    pub fn resolve_in_place(&mut self) {
        let (resolved, _) = self.most_resolved();
        *self = resolved.clone();
    }

    /// Wait for the channel to be resolved.
    pub fn resolution(&self) -> Resolved<'_, C> {
        Resolved {
            shared: &self.shared,
            waiter: RecvWaiter::new(),
            state: Poll::Pending,
        }
    }

    #[inline]
    pub fn downgrade(&self) -> WeakSender<C> {
        WeakSender {
            shared: Arc::downgrade(&self.shared)
        }
    }

    #[inline]
    pub fn chan(&self) -> &C {
        &self.shared.chan
    }
}

#[pin_project(PinnedDrop)]
pub struct Resolved<'a, C: Chan> {
    shared: &'a SharedChannel<C>,

    #[pin]
    waiter: RecvWaiter,

    state: Poll<Resolution<'a, C>>,
}

impl<'a, C: Chan> Future for Resolved<'a, C> {
    type Output = Resolution<'a, C>;

    fn poll(self: Pin<&mut Self>, ctx: &mut Context<'_>) -> Poll<Self::Output> {
        let this = self.project();

        if this.state.is_ready() {
            return *this.state;
        }

        let poll = this.waiter.poll(
            ctx,
            &this.shared.atomic_state,
            &this.shared.waiters,
        );

        let value = match poll {
            ShotState::Empty => return Poll::Pending,
            ShotState::Closed => Resolution::Dropped,
            ShotState::Sent => {
                let value = unsafe { this.shared.get_ref_unchecked() };
                match value {
                    ChannelResolution::Error(err) => Resolution::Error(err),
                    ChannelResolution::Forward(fwd) => Resolution::Forwarded(fwd),
                }
            }
        };

        *this.state = Poll::Ready(value);

        Poll::Ready(value)
    }
}

#[pinned_drop]
impl<C: Chan> PinnedDrop for Resolved<'_, C> {
    fn drop(self: Pin<&mut Self>) {
        let this = self.project();

        this.waiter.pinned_drop(&this.shared.waiters);
    }
}

unsafe impl<C: Chan> Send for Resolved<'_, C> where Request<C>: Send {}
unsafe impl<C: Chan> Sync for Resolved<'_, C> where Request<C>: Send {}

impl<C: Chan + ?Sized> PartialEq for Sender<C> {
    fn eq(&self, other: &Self) -> bool {
        Arc::ptr_eq(&self.shared, &other.shared)
    }
}
impl<C: Chan + ?Sized> Eq for Sender<C> {}

impl<C: Chan + ?Sized> Hash for Sender<C> {
    fn hash<H: Hasher>(&self, state: &mut H) {
        std::ptr::hash(Arc::as_ptr(&self.shared), state);
    }
}

unsafe impl<C: Chan> Send for Sender<C> where Request<C>: Send {}
unsafe impl<C: Chan> Sync for Sender<C> where Request<C>: Send {}

/// An item that can be sent along an mpsc channel.
pub enum Item<C: Chan> {
    Request(Request<C>),
    Event(Event<C>),
}

pub struct ItemWithSender<C: Chan> {
    pub item: Item<C>,
    pub sender: Sender<C>,
}

#[derive(Debug)]
pub struct Receiver<C: Chan> {
    shared: ManuallyDrop<Arc<SharedChannel<C>>>,
}

unsafe impl<C: Chan> Send for Receiver<C> where Request<C>: Send {}
unsafe impl<C: Chan> Sync for Receiver<C> where Request<C>: Send {}

impl<C: Chan> Receiver<C> {
    #[inline]
    fn into_inner(mut self) -> Arc<SharedChannel<C>> {
        let inner = unsafe { ManuallyDrop::take(&mut self.shared) };
        std::mem::forget(self);
        inner
    }

    #[inline]
    pub fn sender(&self) -> Option<Sender<C>> {
        if !self.shared.try_add_sender() {
            return None
        }

        Some(Sender::from_shared(Arc::clone(&self.shared)))
    }

    #[inline]
    pub fn chan(&self) -> &C {
        &self.shared.chan
    }

    /// Forward all the requests from this receiver to the given sender in one operation
    /// while keeping request ordering. After this operation the receiver is consumed,
    /// but senders that were originally associated with this receiver will automatically
    /// begin refering to the channel associated with the forwarded channel.
    ///
    /// Forwarding to a sender on this same channel will result in the receiver being returned.
    pub fn forward_to(self, other: &Sender<C>) -> Result<(), Self> {
        let inner = self.into_inner();
        if let Err(()) = inner.forward_to(&other.shared) {
            return Err(Receiver { shared: ManuallyDrop::new(inner) })
        }

        Ok(())
    }

    /// Await the closing of the channel, without taking any requesting from it.
    pub async fn closed(&mut self) {
        poll_fn(|cx| self.poll_closed(cx)).await;
    }

    pub fn poll_closed(&mut self, cx: &mut Context<'_>) -> Poll<()> {
        self.shared.sender_poll_closed(cx)
    }

    pub fn is_closed(&self) -> bool {
        self.shared.is_closed()
    }

    /// Receive the next item in the channel.
    pub async fn recv(&mut self) -> Option<ItemWithSender<C>> {
        poll_fn(|cx| self.poll_recv(cx)).await
    }

    pub fn poll_recv(&mut self, cx: &mut Context<'_>) -> Poll<Option<ItemWithSender<C>>> {
        if self.is_closed() {
            return Poll::Ready(None);
        }

        let waker = cx.waker().clone();

        let mut locked = self.shared.guarded_state.lock();
        let next = locked.messages.pop_front();

        if let Some(n) = next {
            return Poll::Ready(Some(n.into_item()));
        }

        if let Some(w) = &locked.waker {
            if !w.will_wake(&waker) {
                locked.waker = None
            }
        }

        if locked.waker.is_none() {
            locked.waker = Some(waker)
        }

        Poll::Pending
    }

    /// Tries to receive the next value for this receiver.
    pub fn try_recv(&mut self) -> Option<ItemWithSender<C>> {
        let mut locked = self.shared.guarded_state.lock();
        let link = locked.messages.pop_front()?;
        Some(link.into_item())
    }

    /// Close this channel with the given error.
    pub fn close(self, err: C::Error) {
        let inner = self.into_inner();
        inner.resolve_and_close(Some(err));
    }
}

impl<C: Chan> Drop for Receiver<C> {
    fn drop(&mut self) {
        self.shared.drop_receiver();
        unsafe {
            ManuallyDrop::drop(&mut self.shared);
        }
    }
}

/// Create a new mpsc channel with the given channel data.
pub fn channel<C: Chan>(chan: C) -> (Sender<C>, Receiver<C>) {
    let channel = Arc::new(SharedChannel::new(chan, 1, None));
    let sender = channel.clone().into_existing_sender();
    let receiver = Receiver { shared: ManuallyDrop::new(channel) };
    (sender, receiver)
}

/// Creates a sender to a broken channel with the given error
pub fn broken<C: Chan>(chan: C, err: C::Error) -> Sender<C> {
    Arc::new(SharedChannel::resolved(
        chan,
        1,
        ChannelResolution::Error(err),
    )).into_existing_sender()
}

/// Creates a sender to a broken channel with a dropped receiver
pub fn dropped<C: Chan>(chan: C) -> Sender<C> {
    Arc::new(SharedChannel::dropped(chan)).into_existing_sender()
}

/// A version of Receiver that drops the channel if all the senders are destroyed and the
/// channel is empty.
///
/// This is used for pipeline channels to break circular references, where the channel itself
/// holds a reference back to the parent request to indicate that the channel is somehow receiving
/// the response from it, and the request holds the weak channel so that it can send requests to it
/// for pipelining.
///
/// When the pipeline is resolved, a weak channel is upgraded into a strong Receiver.
#[derive(Debug)]
pub(crate) struct WeakReceiver<C: Chan> {
    shared: Weak<SharedChannel<C>>,
}

impl<C: Chan> WeakReceiver<C> {
    pub fn sender(&self) -> Option<Sender<C>> {
        let shared = self.shared.upgrade()?;
        if !shared.try_add_sender() {
            return None
        }
        Some(shared.into_existing_sender())
    }

    /// Upgrade the channel into a receiver
    pub fn upgrade(self) -> Option<(Receiver<C>, request::Receiver<C>)> {
        let shared = self.shared.upgrade()?;
        let response = shared.guarded_state.lock().parent_request.take().unwrap();
        Some((Receiver { shared: ManuallyDrop::new(shared) }, response))
    }
}

impl<C: Chan> Drop for WeakReceiver<C> {
    fn drop(&mut self) {
        let Some(shared) = self.shared.upgrade() else { return };
        let _ = shared.guarded_state.lock().parent_request.take();
        shared.drop_receiver();
        drop(shared);
    }
}

pub struct WeakSender<C: Chan> {
    shared: Weak<SharedChannel<C>>,
}

impl<C: Chan> WeakSender<C> {
    pub fn upgrade(&self) -> Option<Sender<C>> {
        let shared = self.shared.upgrade()?;
        if !shared.try_add_sender() {
            return None
        }
        Some(shared.into_existing_sender())
    }
}

pub(crate) fn weak_channel<C: Chan>(
    chan: C,
    parent: request::Receiver<C>,
) -> (Sender<C>, WeakReceiver<C>) {
    let channel = Arc::new(SharedChannel::new(chan, 1, Some(parent)));
    let weak_channel = WeakReceiver {
        shared: Arc::downgrade(&channel),
    };
    let sender = channel.into_existing_sender();
    (sender, weak_channel)
}

/// A key to remove a receiver from a ReceiverSet.
pub struct ReceiverKey<C: Chan> {
    shared: Weak<SharedChannel<C>>,
}

impl<C: Chan> ReceiverKey<C> {
    pub fn into_weak_sender(self) -> WeakSender<C> {
        WeakSender { shared: self.shared }
    }
}

pub struct ReceiverKeyRef<'a, C: Chan> {
    channel: *const SharedChannel<C>,
    set: &'a Arc<SharedChannelSet<C>>,
}

impl<'a, C: Chan> ReceiverKeyRef<'a, C> {
    /// Get an owned copy of the receiver key.
    pub fn key(&self) -> ReceiverKey<C> {
        // TODO(someday): Kinda sucks that we have to increment the strong count just so we can
        // increment the weak count. Maybe they'll add a "increment weak count" function someday.
        let strong = unsafe {
            Arc::increment_strong_count(self.channel);
            Arc::from_raw(self.channel)
        };
        ReceiverKey { shared: Arc::downgrade(&strong) }
    }

    /// Get the associated channel data for the receiver.
    pub fn chan(&self) -> &C {
        unsafe { &(*self.channel).chan }
    }

    /// Remove the receiver from the set.
    pub fn take(self) -> Receiver<C> {
        let strong = unsafe {
            Arc::increment_strong_count(self.channel);
            Arc::from_raw(self.channel)
        };
        assert!(self.set.remove_by_ref(&strong));
        Receiver { shared: ManuallyDrop::new(strong) }
    }
}

pub enum SetRecvResult<'a, C: Chan> {
    /// An item was received from a channel.
    Item {
        /// The receiver key this item was received for.
        receiver: ReceiverKeyRef<'a, C>,
        /// The item received.
        item: Item<C>,
        /// The sender this item was sent to.
        sender: Sender<C>,
    },
    /// A receiver was closed, indicating that it will never receive any messages. The receiver
    /// for the channel is removed from the set and returned to the caller.
    Closed {
        /// The receiver that was closed.
        receiver: Receiver<C>,
    },
}

/// A set of receivers that can all be received from simultaneously.
pub struct ReceiverSet<C: Chan> {
    shared: Arc<SharedChannelSet<C>>,
    len: usize,
}

impl<C: Chan> ReceiverSet<C> {
    #[inline]
    pub fn new() -> Self {
        Self {
            shared: Arc::new(SharedChannelSet {
                guarded: Mutex::new(GuardedChannelSet {
                    idle: LinkedList::new(),
                    ready: LinkedList::new(),
                    waker: None,
                })
            }),
            len: 0,
        }
    }

    #[inline]
    pub fn insert(&mut self, recv: Receiver<C>) -> ReceiverKey<C> {
        let channel = recv.into_inner();
        let key = ReceiverKey { shared: Arc::downgrade(&channel) };
        self.shared.insert(channel);
        self.len += 1;
        key
    }

    #[inline]
    pub fn len(&self) -> usize {
        self.len
    }

    #[inline]
    pub fn is_empty(&self) -> bool {
        self.len == 0
    }

    #[inline]
    pub fn remove(&mut self, key: &ReceiverKey<C>) -> Option<Receiver<C>> {
        let channel = key.shared.upgrade()?;
        if !self.shared.remove_by_ref(&channel) {
            return None
        }

        self.len -= 1;
        Some(Receiver { shared: ManuallyDrop::new(channel) })
    }

    #[inline]
    pub fn remove_by_sender(&mut self, key: &Sender<C>) -> Option<Receiver<C>> {
        if !self.shared.remove_by_ref(&key.shared) {
            return None
        }

        self.len -= 1;
        Some(Receiver { shared: key.shared.clone() })
    }

    #[inline]
    pub fn remove_by_weak_sender(&mut self, key: &WeakSender<C>) -> Option<Receiver<C>> {
        let channel = key.shared.upgrade()?;
        if !self.shared.remove_by_ref(&channel) {
            return None
        }

        self.len -= 1;
        Some(Receiver { shared: ManuallyDrop::new(channel) })
    }

    /// Returns all receivers in the set, clearing the set.
    #[inline]
    pub fn remove_all(&mut self) -> Vec<Receiver<C>> {
        let vec = self.shared.remove_all();
        self.len = 0;
        vec
    }

    /// Receive the next value for any channel in this set. If there are no channels in this set,
    /// this returns None.
    #[inline]
    pub async fn recv(&mut self) -> Option<SetRecvResult<'_, C>> {
        pub struct SetRecv<'a, C: Chan> {
            ptr: Option<NonNull<ReceiverSet<C>>>,
            a: PhantomData<&'a mut ReceiverSet<C>>,
        }

        unsafe impl<'a, C: Chan> Send for SetRecv<'a, C> where &'a mut ReceiverSet<C>: Send {}

        impl<'a, C: Chan> Future for SetRecv<'a, C> {
            type Output = Option<SetRecvResult<'a, C>>;

            fn poll(self: Pin<&mut Self>, cx: &mut Context<'_>) -> Poll<Self::Output> {
                let this = self.get_mut();
                let mut ptr = this.ptr.expect("future already consumed");
                let ptr = unsafe { ptr.as_mut() };

                let result = {
                    if ptr.is_empty() {
                        Poll::Ready(None)
                    } else {
                        ptr.poll_recv(cx).map(Some)
                    }
                };

                if result.is_ready() {
                    this.ptr = None;
                }

                result
            }
        }

        SetRecv { ptr: Some(NonNull::from(self)), a: PhantomData }.await
    }

    #[inline]
    pub fn poll_recv<'a>(&'a mut self, cx: &mut Context<'_>) -> Poll<SetRecvResult<'a, C>> {
        let result = self.shared.poll_recv(cx);
        if let Poll::Ready(SetRecvResult::Closed { .. }) = &result {
            self.len -= 1;
        }
        result
    }

    /// Tries to receive the next value for this set.
    #[inline]
    pub fn try_recv(&mut self) -> Option<SetRecvResult<'_, C>> {
        let result = self.shared.try_recv();
        if let Some(SetRecvResult::Closed { .. }) = &result {
            self.len -= 1;
        }
        result
    }
}

impl<C: Chan> Drop for ReceiverSet<C> {
    fn drop(&mut self) {
        drop(self.remove_all());
    }
}