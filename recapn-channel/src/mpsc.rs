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

use std::cell::UnsafeCell;
use std::fmt::{self, Debug};
use std::future::{poll_fn, Future};
use std::hash::{Hash, Hasher};
use std::marker::PhantomData;
use std::mem::{ManuallyDrop, MaybeUninit};
use std::pin::Pin;
use std::process::abort;
use std::ptr::{addr_of_mut, NonNull};
use std::sync::atomic::{AtomicPtr, AtomicUsize};
use std::sync::atomic::Ordering::{Acquire, Relaxed};
use std::sync::{Arc, Weak};
use std::task::{Context, Poll, Waker};

use parking_lot::{Mutex, MutexGuard};
use pin_project::{pin_project, pinned_drop};

use crate::request::{self, Request, SharedRequest};
use crate::util::array_vec::ArrayVec;
use crate::util::atomic_state::{AtomicState, ShotState};
use crate::util::closed_task::ClosedTask;
use crate::util::linked_list::{Link, LinkedList, Pointers};
use crate::util::wait_list::{RecvWaiter, WaitList};
use crate::{Chan, IntoResults};

#[derive(Clone, Copy, Debug)]
pub(crate) enum LinkKind {
    Request,
    Event,
}

#[repr(C)]
pub(crate) struct SharedLink<T: ?Sized> {
    /// Intrusive linked-list pointers for request channels
    ///
    /// In order to maintain "one allocation per request", we intrusively link
    /// requests together to build a request chain.
    pointers: Pointers<SharedLink<()>>,

    kind: LinkKind,

    pub data: T,
}

impl<C: Chan> SharedLink<SharedRequest<C>> {
    pub fn new(data: SharedRequest<C>) -> Arc<Self> {
        Arc::new(Self {
            pointers: Pointers::new(),
            kind: LinkKind::Request,
            data,
        })
    }
}

enum LinkItem<C: Chan> {
    Request(Arc<SharedLink<SharedRequest<C>>>),
    Event(Box<SharedLink<C::Event>>),
}

impl<C: Chan> LinkItem<C> {
    #[inline]
    pub fn into_item(self) -> Item<C> {
        match self {
            LinkItem::Request(r) => Item::Request(Request::new(r)),
            LinkItem::Event(e) => Item::Event(Event { inner: e }),
        }
    }
}

struct LinkPtr<C: Chan> {
    p: PhantomData<fn() -> C>,
    ptr: *const SharedLink<()>,
}

impl<C: Chan> LinkPtr<C> {
    pub fn from_item(item: LinkItem<C>) -> Self {
        Self {
            p: PhantomData,
            ptr: match item {
                LinkItem::Request(r) => Arc::into_raw(r).cast(),
                LinkItem::Event(e) => Box::into_raw(e).cast(),
            },
        }
    }

    pub fn into_link_item(self) -> LinkItem<C> {
        let kind = unsafe { (*self.ptr).kind };
        match kind {
            LinkKind::Request => {
                let cast_ptr = self.ptr.cast::<SharedLink<SharedRequest<C>>>();
                LinkItem::Request(unsafe { Arc::from_raw(cast_ptr) })
            }
            LinkKind::Event => {
                let cast_ptr = self.ptr.cast::<SharedLink<C::Event>>().cast_mut();
                LinkItem::Event(unsafe { Box::from_raw(cast_ptr) })
            }
        }
    }

    pub fn into_item(self) -> Item<C> {
        self.into_link_item().into_item()
    }
}

unsafe impl<C: Chan> Link for LinkPtr<C> {
    type Handle = Self;
    type Target = SharedLink<()>;

    fn into_raw(handle: Self::Handle) -> NonNull<Self::Target> {
        NonNull::new(handle.ptr.cast_mut()).unwrap()
    }
    unsafe fn from_raw(ptr: NonNull<Self::Target>) -> Self::Handle {
        Self {
            p: PhantomData,
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

    requests: LinkedList<LinkPtr<C>, SharedLink<()>>,

    /// The waker set by the receiver to wake up the receiver task.
    waker: Option<Waker>,

    /// If this is a channel that's part of a ReceiverSet, this holds a strong reference
    /// back to the parent.
    parent_set: Option<Arc<SharedChannelSet<C>>>,
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

    /// An pointer back to the channel set that owns the channel. This can be checked to assert
    /// that the channel hasn't been moved to another set in the time between locks without having
    /// to lock the channel again.
    parent_set_ptr: AtomicPtr<SharedChannelSet<C>>,

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
            parent_set_ptr: AtomicPtr::new(core::ptr::null_mut()),
            guarded_state: Mutex::new(GuardedChannel {
                parent_request: parent,
                requests: LinkedList::new(),
                waker: None,
                parent_set: None,
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
            parent_set_ptr: AtomicPtr::new(core::ptr::null_mut()),
            guarded_state: Mutex::new(GuardedChannel {
                parent_request: None,
                requests: LinkedList::new(),
                waker: None,
                parent_set: None,
            }),
            atomic_state: AtomicState::new_set(),
            waiters: WaitList::new(),
            sender_count: AtomicUsize::new(senders),
            closed_task: ClosedTask::new(),
            resolution: UnsafeCell::new(MaybeUninit::new(resolution)),
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
        let (this, mut channel) = match self.resolve_and_lock() {
            Ok(v) => v,
            Err(r) => {
                return match r {
                    MostResolved::Dropped => Err((req, None)),
                    MostResolved::Error(err) => return Err((req, Some(err))),
                }
            }
        };

        let req_shared = req.into_inner();
        let link_ptr = LinkPtr::from_item(LinkItem::Request(req_shared.clone()));

        // Insert the request into the list.
        channel.requests.push_back(link_ptr);

        unsafe {
            req_shared.data.set_parent(this.clone());
        }

        if req_shared.data.is_finished() {
            // Just drop the request nobody wants the result
            let _ = channel.requests.pop_back().unwrap().into_item();
            unsafe {
                req_shared.data.take_parent();
            }
            return Ok(());
        }

        if let Some(w) = &channel.waker {
            w.wake_by_ref();
        }

        if let Some(parent_set) = channel.parent_set.clone() {
            drop(channel);
            parent_set.ready_channel(self);
        }

        Ok(())
    }

    fn send_event(
        self: &Arc<Self>,
        event: Event<C::Event>,
    ) -> Result<(), (Event<C::Event>, Option<&C::Error>)> {
        let (_, mut channel) = match self.resolve_and_lock() {
            Ok(v) => v,
            Err(r) => {
                return match r {
                    MostResolved::Dropped => Err((event, None)),
                    MostResolved::Error(err) => return Err((event, Some(err))),
                }
            }
        };

        channel
            .requests
            .push_back(LinkPtr::from_item(LinkItem::Event(event.inner)));

        if let Some(w) = &channel.waker {
            w.wake_by_ref();
        }

        if let Some(parent_set) = channel.parent_set.clone() {
            drop(channel);
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

    fn sender_poll_closed(&self, cx: &mut Context<'_>) -> Poll<()> {
        self.closed_task.poll(&self.atomic_state, cx)
    }

    /// Remove a tracked receiver from the receiver count.
    ///
    /// If this is the last receiver (and the receiver count is zero), this closes the channel on
    /// the receiving side.
    ///
    /// Note: A channel cannot be re-opened by adding a receiver when the channel is closed.
    unsafe fn remove_sender(&self) {
        let last_receiver = self.sender_count.fetch_sub(1, Relaxed) == 0;
        if last_receiver {
            self.closed_task.close(&self.atomic_state)
        }
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

    /// Like most_resolved, but returns None if the channel has permanently resolved.
    pub fn most_unresolved(mut self: &Self) -> Option<&Self> {
        loop {
            let resolution = self.try_resolved();
            break match resolution {
                None => Some(self),
                Some(Resolution::Dropped | Resolution::Error(_)) => None,
                Some(Resolution::Forwarded(channel)) => {
                    self = &*channel.shared;
                    continue;
                }
            };
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
        let mut resolution;
        (other, resolution) = other.most_resolved();
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
                self.resolve(ChannelResolution::Forward(Sender::new(other.clone())));
            }

            // There's no requests to forward, so just return early. This way we won't wake up the
            // other receiver for no reason.
            if self_lock.requests.is_empty() {
                return Ok(());
            }

            other_lock.requests.append_back(&mut self_lock.requests);

            if let Some(waker) = &other_lock.waker {
                waker.wake_by_ref();
            }

            if let Some(parent_set) = other_lock.parent_set.clone() {
                drop(lock_b);
                drop(lock_a);
                parent_set.ready_channel(other);
            }


            return Ok(());
        }
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
            while let Some(r) = self_lock.requests.pop_front() {
                if let LinkItem::Request(r) = r.into_link_item() {
                    unsafe {
                        r.data.take_parent();
                        r.data.drop_request();
                        // TODO: Maybe signal that the request is over without waking everyone up?
                    }
                }
            }
        };

        let mut request_array = ArrayVec::<Item<C>, 32>::new();

        let respond_with_err = |i: Item<C>| {
            if let Some(err) = &err {
                if let Item::Request(r) = i {
                    let (_, responder) = r.respond();
                    responder.respond(err.clone().into_results());
                }
            }
        };

        'outer: loop {
            while request_array.can_push() {
                let Some(ptr) = self_lock.requests.pop_front() else {
                    break 'outer;
                };

                let item = ptr.into_link_item();

                if let LinkItem::Request(req) = &item {
                    unsafe {
                        req.data.take_parent();
                    }
                }

                request_array.push(item.into_item());
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

    fn into_raw(handle: Self::Handle) -> NonNull<Self::Target> {
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
    fn pop_receiver(&mut self) -> Option<Receiver<C>> {
        let channel = self.ready.pop_front().or_else(|| self.idle.pop_front())?;
        let channel = unsafe { Arc::from_raw(channel.ptr.as_ptr().cast_const()) };
        let mut guarded_channel = channel.guarded_state.lock();
        guarded_channel.parent_set = None;
        channel.parent_set_ptr.store(std::ptr::null_mut(), Relaxed);
        channel.atomic_state.clear_ready(Relaxed);
        drop(guarded_channel);
        Some(Receiver { shared: ManuallyDrop::new(channel) })
    }

    fn has_idle(&self) -> bool {
        self.idle.is_empty()
    }

    /// Pop the ready list until it yields an item or a closed channel.
    fn pop_ready(&mut self) -> Option<(NonNull<SharedChannel<C>>, Option<Item<C>>)> {
        loop {
            let channel = self.ready.pop_front()?;
            let channel_ptr = channel.ptr;
            let channel_ref = unsafe { channel_ptr.as_ref() };
            let mut guarded_channel = channel_ref.guarded_state.lock();
            let Some(next_request) = guarded_channel.requests.pop_front() else {
                if channel_ref.is_closed() {
                    // The channel is closed, so we can tear it down and remove it.
                    guarded_channel.parent_set = None;
                    channel_ref.parent_set_ptr.store(std::ptr::null_mut(), Relaxed);
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
            if guarded_channel.requests.is_empty() || channel_ref.is_closed() {
                // If we're ready still, put it back into the ready list.
                self.ready.push_back(channel);
            } else {
                channel_ref.atomic_state.clear_ready(Relaxed);
            }
            let link = next_request.into_link_item();
            if let LinkItem::Request(req) = &link {
                unsafe { req.data.take_parent() };
            }
            return Some((channel_ptr, Some(link.into_item())));
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
        let self_strong_ptr = Arc::as_ptr(self);

        let mut set_guard = self.guarded.lock();
        let mut channel_guard = channel_ref.guarded_state.lock();

        // Clean up our channel state by removing any old wakers, setting the parent set to our
        // weak pointer, and writing our atomic parent set pointer.
        channel_guard.waker = None;
        channel_guard.parent_set = Some(self_strong);
        channel_ref.parent_set_ptr.store(self_strong_ptr.cast_mut(), Relaxed);

        // If the channel is already in a state to be actioned, put it
        // immediately in the ready state.
        if !channel_guard.requests.is_empty() || channel_ref.is_closed() {
            set_guard.ready.push_back(ChannelInSet { ptr: channel_ptr });
            channel_ref.atomic_state.set_ready(Relaxed);

            if let Some(waker) = &set_guard.waker {
                waker.wake_by_ref();
            }
        } else {
            set_guard.idle.push_back(ChannelInSet { ptr: channel_ptr });
        }
    }

    fn remove_by_ref(self: &Arc<Self>, channel: &SharedChannel<C>) -> bool {
        let channel_ptr = NonNull::from(channel);

        let mut set_guard = self.guarded.lock();

        let channel_parent_set_ptr = channel.parent_set_ptr.load(Relaxed);
        if !std::ptr::addr_eq(channel_parent_set_ptr, self.as_ref()) {
            return false
        }

        let mut channel_guard = channel.guarded_state.lock();
        channel_guard.parent_set = None;
        channel.parent_set_ptr.store(std::ptr::null_mut(), Relaxed);
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
        let next = guarded.pop_ready();
        match next {
            Some((channel, None)) => {
                let strong_channel = unsafe { Arc::from_raw(channel.as_ptr().cast_const()) };
                Poll::Ready(SetRecvResult::Closed {
                    receiver: Receiver {
                        shared: ManuallyDrop::new(strong_channel)
                    }
                })
            }
            Some((channel, Some(item))) => {
                Poll::Ready(SetRecvResult::Item {
                    receiver: ReceiverKeyRef {
                        channel: channel.as_ptr().cast_const(),
                        set: self,
                    },
                    item,
                })
            }
            None if guarded.has_idle() => {
                let cx_waker = cx.waker();
                if let Some(w) = &mut guarded.waker {
                    w.clone_from(cx_waker);
                } else {
                    guarded.waker = Some(cx_waker.clone());
                };
                Poll::Pending
            }
            None => Poll::Ready(SetRecvResult::None)
        }
    }

    fn try_recv(self: &Arc<Self>) -> SetRecvResult<'_, C> {
        let next = self.guarded.lock().pop_ready();
        match next {
            Some((channel, None)) => {
                let strong_channel = unsafe { Arc::from_raw(channel.as_ptr().cast_const()) };
                SetRecvResult::Closed {
                    receiver: Receiver {
                        shared: ManuallyDrop::new(strong_channel)
                    }
                }
            }
            Some((channel, Some(item))) => {
                SetRecvResult::Item {
                    receiver: ReceiverKeyRef {
                        channel: channel.as_ptr().cast_const(),
                        set: self,
                    },
                    item,
                }
            }
            None => SetRecvResult::None
        }
    }

    fn ready_channel(mut self: Arc<Self>, channel: &SharedChannel<C>) {
        let channel_ptr = NonNull::from(channel);

        let mut set_guard = loop {
            let set_guard = self.guarded.lock();

            let channel_parent_set_ptr = channel.parent_set_ptr.load(Relaxed);
            if !std::ptr::addr_eq(channel_parent_set_ptr, self.as_ref()) {
                drop(set_guard);
                let channel_guard = channel.guarded_state.lock();
                let new_set = channel_guard.parent_set.clone();
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

        if let Some(waker) = &set_guard.waker {
            waker.wake_by_ref();
        }

    }
}

/// An message that can be sent on a mpsc channel. This follows the same ordering as requests, but
/// without the drop behavior of requests, allowing simple messages to be sent along the
/// request path.
pub struct Event<E> {
    inner: Box<SharedLink<E>>,
}

impl<E> Event<E> {
    #[inline]
    pub fn new(data: E) -> Self {
        Self {
            inner: Box::new(SharedLink {
                pointers: Pointers::new(),
                kind: LinkKind::Event,
                data,
            }),
        }
    }

    #[inline]
    pub fn into_inner(self) -> E {
        self.inner.data
    }
}

pub enum Resolution<'a, C: Chan> {
    /// The channel was forwarded to another channel
    Forwarded(&'a Sender<C>),
    /// The receiving end was dropped
    Dropped,
    /// The channel was closed with the specified error
    Error(&'a C::Error),
}

impl<'a, C: Chan> Resolution<'a, C> {
    pub fn forwarded(self) -> Option<&'a Sender<C>> {
        let Self::Forwarded(f) = self else {
            return None;
        };
        Some(f)
    }

    pub fn is_forwarded(&self) -> bool {
        matches!(self, Self::Forwarded(_))
    }

    pub fn is_dropped(&self) -> bool {
        matches!(self, Self::Dropped)
    }

    pub fn error(self) -> Option<&'a C::Error> {
        let Self::Error(e) = self else { return None };
        Some(e)
    }

    pub fn is_error(&self) -> bool {
        matches!(self, Self::Error(_))
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
    shared: Arc<SharedChannel<C>>,
}

impl<C: Chan> Clone for Sender<C> {
    fn clone(&self) -> Self {
        Self::new(self.shared.clone())
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
        Self { shared }
    }

    pub fn send(&self, req: Request<C>) -> Result<(), (Request<C>, Option<&C::Error>)> {
        self.shared.send_request(req)
    }

    pub fn send_event(
        &self,
        event: Event<C::Event>,
    ) -> Result<(), (Event<C::Event>, Option<&C::Error>)> {
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

pub enum Item<C: Chan> {
    Request(Request<C>),
    Event(Event<C::Event>),
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

    pub async fn recv(&mut self) -> Option<Item<C>> {
        poll_fn(|cx| self.poll_recv(cx)).await
    }

    pub fn poll_recv(&mut self, cx: &mut Context<'_>) -> Poll<Option<Item<C>>> {
        if self.is_closed() {
            return Poll::Ready(None);
        }

        let waker = cx.waker().clone();

        let mut locked = self.shared.guarded_state.lock();
        let next = locked.requests.pop_front().map(LinkPtr::into_link_item);

        if let Some(n) = next {
            if let LinkItem::Request(req) = &n {
                unsafe { req.data.take_parent() };
            }
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
    pub fn try_recv(&mut self) -> Option<Item<C>> {
        let mut locked = self.shared.guarded_state.lock();
        let link = locked.requests.pop_front()?;
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

pub fn channel<C: Chan>(chan: C) -> (Sender<C>, Receiver<C>) {
    let channel = Arc::new(SharedChannel::new(chan, 1, None));
    let sender = Sender {
        shared: channel.clone(),
    };
    let receiver = Receiver { shared: ManuallyDrop::new(channel) };
    (sender, receiver)
}

/// Creates a sender to a broken channel with the given error
pub fn broken<C: Chan>(chan: C, err: C::Error) -> Sender<C> {
    Sender {
        shared: Arc::new(SharedChannel::resolved(chan, 1, ChannelResolution::Error(err)))
    }
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
        Some(Sender { shared })
    }

    /// Upgrade the channel into a receiver
    pub fn upgrade(self) -> Option<Receiver<C>> {
        let shared = self.shared.upgrade()?;
        let _ = shared.guarded_state.lock().parent_request.take();
        Some(Receiver { shared: ManuallyDrop::new(shared) })
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
        Some(Sender { shared })
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
    let sender = Sender { shared: channel };
    (sender, weak_channel)
}

/// A key to remove a receiver from a ReceiverSet.
pub struct ReceiverKey<C: Chan> {
    shared: Weak<SharedChannel<C>>,
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
    /// No item was received.
    None,
    /// An item was received from a channel.
    Item {
        /// The receiver key this item was received for.
        receiver: ReceiverKeyRef<'a, C>,
        /// The item received.
        item: Item<C>,
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
        }
    }

    #[inline]
    pub fn insert(&mut self, recv: Receiver<C>) -> ReceiverKey<C> {
        let channel = recv.into_inner();
        let key = ReceiverKey { shared: Arc::downgrade(&channel) };
        self.shared.insert(channel);
        key
    }

    #[inline]
    pub fn remove(&mut self, key: &ReceiverKey<C>) -> Option<Receiver<C>> {
        let channel = key.shared.upgrade()?;
        if !self.shared.remove_by_ref(&channel) {
            return None
        }

        Some(Receiver { shared: ManuallyDrop::new(channel) })
    }

    /// Returns all receivers in the set, clearing the set.
    #[inline]
    pub fn remove_all(&mut self) -> Vec<Receiver<C>> {
        self.shared.remove_all()
    }

    /// Receive the next value for any channel in this set. If there are no channels in this set,
    /// this returns None.
    #[inline]
    pub async fn recv(&mut self) -> SetRecvResult<'_, C> {
        pub struct SetRecv<'a, C: Chan> {
            ptr: Option<NonNull<ReceiverSet<C>>>,
            a: PhantomData<&'a mut ReceiverSet<C>>,
        }

        impl<'a, C: Chan> Future for SetRecv<'a, C> {
            type Output = SetRecvResult<'a, C>;

            fn poll(self: Pin<&mut Self>, cx: &mut Context<'_>) -> Poll<Self::Output> {
                let this = self.get_mut();
                let mut ptr = this.ptr.expect("future already consumed");
                let ptr = unsafe { ptr.as_mut() };

                let result = ptr.poll_recv(cx);
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
        self.shared.poll_recv(cx)
    }

    /// Tries to receive the next value for this set.
    #[inline]
    pub fn try_recv(&mut self) -> SetRecvResult<'_, C> {
        self.shared.try_recv()
    }
}

impl<C: Chan> Drop for ReceiverSet<C> {
    fn drop(&mut self) {
        drop(self.remove_all());
    }
}