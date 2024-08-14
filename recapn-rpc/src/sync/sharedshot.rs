//! A oneshot that can have multiple waiters waiting for the result. This is a simple
//! multi-threaded primitive used as a replacement for forked promises in kj, but is
//! more efficient and flexible than using tokio's Notified for at-most-once notifications.

use std::borrow::Borrow;
use std::cell::UnsafeCell;
use std::future::{poll_fn, Future, IntoFuture};
use std::marker::PhantomPinned;
use std::mem::MaybeUninit;
use std::ops::Deref;
use std::pin::{pin, Pin};
use std::process::abort;
use std::ptr::{addr_of_mut, NonNull};
use std::sync::atomic::Ordering::{self, *};
use std::sync::atomic::{AtomicBool, AtomicU8, AtomicUsize};
use std::sync::Arc;
use std::task::{Context, Poll, Waker};

use super::util::array_vec::ArrayVec;
use super::util::linked_list::{GuardedLinkedList, Link, LinkedList, Pointers};
use super::util::Marc;
use parking_lot::Mutex;
use pin_project::{pin_project, pinned_drop};

pub use super::TryRecvError;

#[derive(Debug)]
pub(crate) struct Waiter {
    /// Intrusive linked-list pointers.
    pointers: Pointers<Waiter>,

    /// Waiting task's waker. Depending on the value of `notified`, this field
    /// is either protected by the `waiters` lock in `WaitList`, or it is exclusively
    /// owned by the enclosing `Waiter`.
    waker: UnsafeCell<Option<Waker>>,

    /// Indicates if this waiter has been notified or not.
    /// - If false, then `waker` is protected by the `waiters` lock.
    /// - If true, then `waker` is owned by the `Waiter` and can by accessed without locking.
    notified: AtomicBool,

    /// Should not be `Unpin`.
    _pinned: PhantomPinned,
}

impl Waiter {
    pub const fn new(waker: Option<Waker>) -> Self {
        Self {
            pointers: Pointers::new(),
            waker: UnsafeCell::new(waker),
            notified: AtomicBool::new(false),
            _pinned: PhantomPinned,
        }
    }
}

unsafe impl Link for Waiter {
    type Handle = NonNull<Self>;
    type Target = Self;

    fn into_raw(handle: Self::Handle) -> NonNull<Self::Target> {
        handle
    }

    unsafe fn from_raw(ptr: NonNull<Self::Target>) -> Self::Handle {
        ptr
    }

    unsafe fn pointers(target: NonNull<Self::Target>) -> NonNull<Pointers<Self::Target>> {
        let me = target.as_ptr();
        let field = addr_of_mut!((*me).pointers);
        NonNull::new_unchecked(field)
    }
}

/// List used in `WaitList::wake_all`. It wraps a guarded linked list
/// and gates the access to it on `notify.waiters` mutex. It also empties
/// the list on drop.
struct RecvWaitersList<'a> {
    list: GuardedLinkedList<Waiter, Waiter>,
    is_empty: bool,
    waiters: &'a WaitList,
}

impl<'a> RecvWaitersList<'a> {
    fn new(
        unguarded_list: LinkedList<Waiter, Waiter>,
        guard: Pin<&'a Waiter>,
        waiters: &'a WaitList,
    ) -> Self {
        let guard_ptr = NonNull::from(guard.get_ref());
        let list = unguarded_list.into_guarded(guard_ptr);
        Self {
            list,
            is_empty: false,
            waiters,
        }
    }

    /// Removes the last element from the guarded list. Modifying this list
    /// requires an exclusive access to the main list in `Notify`.
    fn pop_back_locked(
        &mut self,
        _waiters: &mut LinkedList<Waiter, Waiter>,
    ) -> Option<NonNull<Waiter>> {
        let result = self.list.pop_back();
        if result.is_none() {
            // Save information about emptiness to avoid waiting for lock
            // in the destructor.
            self.is_empty = true;
        }
        result
    }
}

impl Drop for RecvWaitersList<'_> {
    fn drop(&mut self) {
        // If the list is not empty, we unlink all waiters from it.
        // We do not wake the waiters to avoid double panics.
        if !self.is_empty {
            let _lock_guard = self.waiters.list.lock();
            while let Some(waiter) = self.list.pop_back() {
                // Safety: we never make mutable references to waiters.
                let waiter = unsafe { waiter.as_ref() };
                waiter.notified.store(true, Release);
            }
        }
    }
}

#[derive(Debug)]
pub(crate) struct WaitList {
    list: Mutex<LinkedList<Waiter, Waiter>>,
}

impl WaitList {
    pub const fn new() -> Self {
        Self {
            list: Mutex::new(LinkedList::new()),
        }
    }

    pub fn wake_all(&self) {
        let mut waiters = self.list.lock();

        // It is critical for `GuardedLinkedList` safety that the guard node is
        // pinned in memory and is not dropped until the guarded list is dropped.
        let guard = Waiter::new(None);
        let pinned_guard = pin!(guard);

        // We move all waiters to a secondary list. It uses a `GuardedLinkedList`
        // underneath to allow every waiter to safely remove itself from it.
        //
        // * This list will be still guarded by the `waiters` lock.
        //   `NotifyWaitersList` wrapper makes sure we hold the lock to modify it.
        // * This wrapper will empty the list on drop. It is critical for safety
        //   that we will not leave any list entry with a pointer to the local
        //   guard node after this function returns / panics.
        let mut list =
            RecvWaitersList::new(std::mem::take(&mut *waiters), pinned_guard.as_ref(), self);

        const NUM_WAKERS: usize = 32;

        let mut wakers = ArrayVec::<Waker, NUM_WAKERS>::new();
        'outer: loop {
            while wakers.can_push() {
                match list.pop_back_locked(&mut waiters) {
                    Some(waiter) => {
                        // Safety: we never make mutable references to waiters.
                        let waiter = unsafe { waiter.as_ref() };

                        // Safety: we hold the lock, so we can access the waker.
                        let waker = unsafe {
                            let waker = &mut *waiter.waker.get();
                            waker.take()
                        };
                        if let Some(waker) = waker {
                            wakers.push(waker);
                        }

                        // This waiter is unlinked and will not be shared ever again, release it.
                        waiter.notified.store(true, Release);
                    }
                    None => {
                        break 'outer;
                    }
                }
            }

            // Release the lock before notifying.
            drop(waiters);

            // One of the wakers may panic, but the remaining waiters will still
            // be unlinked from the list in `NotifyWaitersList` destructor.
            wakers.for_each(Waker::wake);

            // Acquire the lock again.
            waiters = self.list.lock();
        }

        // Release the lock before notifying
        drop(waiters);

        wakers.for_each(Waker::wake);
    }
}

/// A waiter helper to make the poll logic behind sharedshot components reusable.
///
/// This contains the state of the waiter and nothing else. Custom pollers call into this
/// via the poll function and provide the atomic state and wait list that the waiter puts
/// itself in.
#[derive(Debug)]
#[pin_project]
pub(crate) struct RecvWaiter {
    #[pin]
    waiter: Option<Waiter>,
}

impl RecvWaiter {
    /// Creates a new recv waiter for waiting on the result of a sharedshot.
    pub const fn new() -> Self {
        Self { waiter: None }
    }

    pub fn poll(
        self: Pin<&mut Self>,
        ctx: &mut Context<'_>,
        atom: &AtomicState,
        waiters: &WaitList,
    ) -> ShotState {
        let mut this = self.project();

        match this.waiter.as_mut().as_pin_mut() {
            None => {
                if let ShotState::Sent = atom.state() {
                    return ShotState::Sent;
                }

                // Clone the waker before locking, a waker clone can be triggering arbitrary code.
                let waker = ctx.waker().clone();

                // Acquire the lock and attempt to transition to the waiting state.
                let mut waiters = waiters.list.lock();

                if let ShotState::Sent = atom.state() {
                    return ShotState::Sent;
                }

                let waiter =
                    unsafe { Pin::get_unchecked_mut(this.waiter).insert(Waiter::new(Some(waker))) };

                // Insert the waiter into the linked list
                waiters.push_front(NonNull::from(&*waiter));

                ShotState::Empty
            }
            Some(waiter) => {
                if waiter.notified.load(Acquire) {
                    // Safety: waiter is already unlinked and will not be shared again,
                    // so we have exclusive access `waker`.
                    unsafe {
                        let waker = &mut *waiter.waker.get();
                        drop(waker.take());
                    }

                    this.waiter.set(None);
                    return atom.state();
                }

                // We were not notified, implying it is still stored in a waiter list.
                // In order to access the waker fields, we must acquire the lock first.

                let mut old_waker = None;
                let mut waiters = waiters.list.lock();

                // We hold the lock and notifications are set only with the lock held,
                // so this can be relaxed, because the happens-before relationship is
                // established through the mutex.
                if waiter.notified.load(Relaxed) {
                    // Safety: waiter is already unlinked and will not be shared again,
                    // so we have an exclusive access to `waker`.
                    old_waker = unsafe {
                        let waker = &mut *waiter.waker.get();
                        waker.take()
                    };

                    drop(waiters);
                    drop(old_waker);

                    this.waiter.set(None);
                    return atom.state();
                }

                // Before we add a waiter to the list we check if the channel has closed.
                // If it's closed it means that there is a call to `notify_waiters` in
                // progress and this waiter must be contained by a guarded list used in
                // `notify_waiters`. So at this point we can treat the waiter as notified and
                // remove it from the list, as it would have been notified in the
                // `notify_waiters` call anyways.

                let state = atom.state();
                match atom.state() {
                    ShotState::Empty => {
                        // The channel hasn't closed, let's update the waker
                        unsafe {
                            let current = &mut *waiter.waker.get();
                            let polling_waker = ctx.waker();
                            let should_update = current.is_none()
                                || matches!(current, Some(w) if w.will_wake(polling_waker));
                            if should_update {
                                old_waker =
                                    std::mem::replace(&mut *current, Some(polling_waker.clone()));
                            }
                        }

                        // Drop the old waker after releasing the lock.
                        drop(waiters);
                        drop(old_waker);
                    }
                    ShotState::Sent | ShotState::Closed => {
                        // Safety: we hold the lock, so we can modify the waker.
                        old_waker = unsafe {
                            let waker = &mut *waiter.waker.get();
                            waker.take()
                        };

                        // Safety: we hold the lock, so we have an exclusive access to the list.
                        // The list is used in `notify_waiters`, so it must be guarded.
                        unsafe { waiters.remove(NonNull::from(&*waiter)) };

                        // Explicit drop of the lock to indicate the scope that the
                        // lock is held. Because holding the lock is required to
                        // ensure safe access to fields not held within the lock, it
                        // is helpful to visualize the scope of the critical
                        // section.
                        drop(waiters);

                        // Drop the old waker after releasing the lock.
                        drop(old_waker);

                        this.waiter.set(None);
                    }
                }

                state
            }
        }
    }

    pub fn pinned_drop(self: Pin<&mut Self>, waiters: &WaitList) {
        let this = self.project();

        if let Some(waiter) = this.waiter.as_pin_mut() {
            let mut waiters = waiters.list.lock();

            // remove the entry from the list (if not already removed)
            //
            // Safety: we hold the lock, so we have an exclusive access to every list the
            // waiter may be contained in. If the node is not contained in the `waiters`
            // list, then it is contained by a guarded list used by `notify_waiters`.
            unsafe { waiters.remove(NonNull::from(&*waiter)) };
        }
    }
}

pub fn channel<T>() -> (Sender<T>, Receiver<T>) {
    let state = Arc::new(State::new(1));
    let sender = Sender {
        shared: Marc::new(state.clone()),
    };
    let receiver = Receiver { shared: state };

    (sender, receiver)
}

/// The state of the sharedshot.
#[derive(PartialEq, Eq, Clone, Copy)]
pub(crate) enum ShotState {
    Sent,
    Empty,
    Closed,
}

impl ShotState {
    pub fn map_sent<T>(self, f: impl FnOnce() -> T) -> Result<T, TryRecvError> {
        match self {
            Self::Sent => Ok(f()),
            Self::Empty => Err(TryRecvError::Empty),
            Self::Closed => Err(TryRecvError::Closed),
        }
    }
}

const STATE_SET: u8 = 0b0000_0001;
const STATE_SEND_CLOSED: u8 = 0b0000_0010;
const STATE_RECV_CLOSED: u8 = 0b0000_0100;
const STATE_CLOSED_TASK_SET: u8 = 0b0000_1000;

const STATE_SET_PIPELINE: u8 = 0b0001_0000;

/// Atomic state used by sharedshot and request state.
#[derive(Debug)]
pub(crate) struct AtomicState(AtomicU8);

impl AtomicState {
    pub const fn new() -> Self {
        Self(AtomicU8::new(0))
    }

    #[inline]
    pub fn get(&mut self) -> StateFlags {
        StateFlags(*self.0.get_mut())
    }

    #[inline]
    pub fn load(&self, order: Ordering) -> StateFlags {
        StateFlags(self.0.load(order))
    }

    #[inline]
    pub fn state(&self) -> ShotState {
        self.load(Acquire).state()
    }

    #[inline]
    pub fn set_send_closed(&self) -> StateFlags {
        let val = self.0.fetch_or(STATE_SEND_CLOSED, Release);
        StateFlags(val | STATE_SEND_CLOSED)
    }

    #[inline]
    pub fn set_recv_closed(&self) -> StateFlags {
        let val = self.0.fetch_or(STATE_RECV_CLOSED, AcqRel);
        StateFlags(val | STATE_RECV_CLOSED)
    }

    #[inline]
    pub fn set_closed_task(&self) -> StateFlags {
        let val = self.0.fetch_or(STATE_CLOSED_TASK_SET, AcqRel);
        StateFlags(val | STATE_CLOSED_TASK_SET)
    }

    #[inline]
    pub fn clear_closed_task(&self) -> StateFlags {
        let val = self.0.fetch_and(!STATE_CLOSED_TASK_SET, AcqRel);
        StateFlags(val & !STATE_CLOSED_TASK_SET)
    }

    #[inline]
    pub fn set_value(&self) -> StateFlags {
        let val = self.0.fetch_or(STATE_SET, AcqRel);
        StateFlags(val | STATE_SET)
    }

    #[inline]
    pub fn try_set_value(&self) -> StateFlags {
        // This method is a compare-and-swap loop rather than a fetch-or like
        // other `set_$WHATEVER` methods on `StateFlags`. This is because we must
        // check if the state has been closed before setting the `SET`
        // bit.
        //
        // We don't want to set both the `SET` bit if the `RECV_CLOSED`
        // bit is already set, because `SET` will tell the receiver that
        // it's okay to access the inner `UnsafeCell`. Immediately after calling
        // `set_value`, if the channel was closed, the sender will _also_
        // access the `UnsafeCell` to take the value back out, so if a
        // `poll_recv` or `try_recv` call is occurring concurrently, both
        // threads may try to access the `UnsafeCell` if we were to set the
        // `SET` bit on a closed channel.
        let mut state = self.0.load(Relaxed);
        loop {
            if StateFlags(state).is_recv_closed() {
                break;
            }

            let exchange_result =
                self.0
                    .compare_exchange_weak(state, state | STATE_SET, Release, Acquire);
            match exchange_result {
                Ok(_) => break,
                Err(actual) => state = actual,
            }
        }
        StateFlags(state)
    }

    #[inline]
    pub fn clear_set_value(&mut self) {
        *self.0.get_mut() &= !STATE_SET;
    }

    #[inline]
    pub fn set_pipeline(&self, order: Ordering) -> StateFlags {
        let val = self.0.fetch_or(STATE_SET_PIPELINE, order);
        StateFlags(val | STATE_SET_PIPELINE)
    }
}

#[derive(Clone, Copy)]
pub(crate) struct StateFlags(u8);

impl StateFlags {
    #[inline]
    pub fn is_set(self) -> bool {
        self.0 & STATE_SET != 0
    }

    #[inline]
    pub fn is_send_closed(self) -> bool {
        self.0 & STATE_SEND_CLOSED != 0
    }

    #[inline]
    pub fn is_recv_closed(self) -> bool {
        self.0 & STATE_RECV_CLOSED != 0
    }

    #[inline]
    pub fn is_closed_task_set(self) -> bool {
        self.0 & STATE_CLOSED_TASK_SET != 0
    }

    #[inline]
    pub fn is_pipeline_set(self) -> bool {
        self.0 & STATE_SET_PIPELINE != 0
    }

    #[inline]
    pub fn state(self) -> ShotState {
        if self.is_set() {
            ShotState::Sent
        } else if self.is_send_closed() {
            ShotState::Closed
        } else {
            ShotState::Empty
        }
    }
}

#[derive(Debug)]
pub(crate) struct ClosedTask {
    waker: UnsafeCell<MaybeUninit<Waker>>,
}

impl ClosedTask {
    #[inline]
    pub const fn new() -> Self {
        Self {
            waker: UnsafeCell::new(MaybeUninit::uninit()),
        }
    }

    #[inline]
    pub fn poll(&self, atom: &AtomicState, cx: &mut Context<'_>) -> Poll<()> {
        let mut state = atom.load(Acquire);

        // The channel is closed. The waker will be destroyed when the sharedshot is dropped.
        if state.is_recv_closed() {
            return Poll::Ready(());
        }

        let waker = cx.waker();
        let closed_task_store = unsafe { &mut *self.waker.get() };
        if state.is_closed_task_set() {
            let closed_task = unsafe { closed_task_store.assume_init_ref() };
            if !closed_task.will_wake(waker) {
                // We have a new waker to insert. Unset the flag and drop the task.
                // This call will replace the local state with a state that doesn't have the flag
                // set so when we fall through we'll put the new waker in.
                state = atom.clear_closed_task();

                if state.is_recv_closed() {
                    // Oop, the task closed while we were doing this. Let's set the flag again
                    // so the waker is dropped when the sharedshot is destroyed.
                    atom.set_closed_task();
                    return Poll::Ready(());
                }

                unsafe { closed_task_store.assume_init_drop() }
            }
        }

        if !state.is_closed_task_set() {
            closed_task_store.write(waker.clone());
            state = atom.set_closed_task();

            if state.is_recv_closed() {
                return Poll::Ready(());
            }
        }

        Poll::Pending
    }

    #[inline]
    pub fn close(&self, atom: &AtomicState) {
        let state = atom.set_recv_closed();
        if state.is_closed_task_set() && !(state.is_send_closed() || state.is_set()) {
            unsafe {
                let closed_task_store = &*self.waker.get();
                closed_task_store.assume_init_ref().wake_by_ref();
            }
        }
    }

    #[inline]
    pub unsafe fn drop(&mut self) {
        self.waker.get_mut().assume_init_drop()
    }
}

/// The sharedshot state. This is public for the crate so that we can combine
/// allocations.
#[derive(Debug)]
pub(crate) struct State<T> {
    /// The state of the shot.
    state: AtomicState,

    /// A set of waiters waiting to receive the value.
    waiters: WaitList,

    /// The number of receivers waiting for the response. When this value reaches zero,
    /// the channel is closed and the closed task is woken up.
    receivers_count: AtomicUsize,

    /// The close task, woken up when all the receivers have been dropped and the channel
    /// has been marked closed.
    closed_task: ClosedTask,

    /// The value in the shot.
    value: UnsafeCell<MaybeUninit<T>>,
}

unsafe impl<T: Send> Send for State<T> {}
unsafe impl<T: Sync> Sync for State<T> {}

impl<T> State<T> {
    pub const fn new(rcvs: usize) -> Self {
        Self {
            state: AtomicState::new(),
            waiters: WaitList::new(),
            receivers_count: AtomicUsize::new(rcvs),
            closed_task: ClosedTask::new(),
            value: UnsafeCell::new(MaybeUninit::uninit()),
        }
    }

    pub fn state(&self) -> ShotState {
        self.state.state()
    }

    pub unsafe fn add_receiver(&self) {
        let old = self.receivers_count.fetch_add(1, Relaxed);

        if old == usize::MAX {
            abort();
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
            self.closed_task.close(&self.state)
        }
    }

    /// Close the send half of the channel.
    pub unsafe fn close_sender(&self) {
        self.state.set_send_closed();
        self.waiters.wake_all();
    }

    pub fn is_receivers_closed(&self) -> bool {
        self.state.load(Relaxed).is_recv_closed()
    }

    pub fn sender_poll_closed(&self, cx: &mut Context<'_>) -> Poll<()> {
        self.closed_task.poll(&self.state, cx)
    }

    pub unsafe fn send(&self, value: T) -> Result<(), T> {
        let value_store = &mut *self.value.get();
        value_store.write(value);

        let prev = self.state.try_set_value();
        if prev.is_recv_closed() {
            return Err(value_store.assume_init_read());
        }

        self.waiters.wake_all();

        Ok(())
    }

    /// Takes the value out of the shared state. This assumes the value is initialized
    pub unsafe fn take(mut self) -> T {
        // Erase the set bit flag, since immediately after this the State is getting dropped.
        self.state.clear_set_value();
        self.value.get_mut().assume_init_read()
    }

    pub unsafe fn get_ref(&self) -> &T {
        (*self.value.get()).assume_init_ref()
    }
}

impl<T> Drop for State<T> {
    fn drop(&mut self) {
        let state = self.state.get();

        if state.is_set() {
            unsafe { self.value.get_mut().assume_init_drop() }
        }

        if state.is_closed_task_set() {
            unsafe { self.closed_task.drop() }
        }

        debug_assert!(self.waiters.list.get_mut().is_empty());
    }
}

pub struct Sender<T> {
    shared: Marc<State<T>>,
}

impl<T> Sender<T> {
    /// Send a value. If the channel is closed, this returns it back.
    #[inline]
    pub fn send(mut self, value: T) -> Result<(), T> {
        unsafe { self.shared.take().send(value) }
    }

    #[inline]
    pub async fn closed(&mut self) {
        poll_fn(|cx| self.poll_closed(cx)).await
    }

    #[inline]
    pub fn poll_closed(&mut self, cx: &mut Context<'_>) -> Poll<()> {
        self.shared.get().sender_poll_closed(cx)
    }

    #[inline]
    pub fn is_closed(&self) -> bool {
        !self.shared.get().is_receivers_closed()
    }
}

impl<T> Drop for Sender<T> {
    fn drop(&mut self) {
        if let Some(shared) = self.shared.try_take() {
            unsafe { shared.close_sender() }
        }
    }
}

#[pin_project(PinnedDrop)]
pub struct Recv<T> {
    shot: Receiver<T>,

    #[pin]
    waiter: RecvWaiter,

    state: Poll<Option<Value<T>>>,
}

unsafe impl<T: Send> Send for Recv<T> {}
unsafe impl<T: Sync> Sync for Recv<T> {}

impl<T> Recv<T> {
    fn new(shot: Receiver<T>) -> Self {
        Recv {
            shot,
            waiter: RecvWaiter::new(),
            state: Poll::Pending,
        }
    }
}

impl<T> Future for Recv<T> {
    type Output = Option<Value<T>>;

    fn poll(self: Pin<&mut Self>, ctx: &mut Context<'_>) -> Poll<Self::Output> {
        let this = self.project();

        if this.state.is_ready() {
            return this.state.clone();
        }

        let shared = &this.shot.shared;
        let value = match this.waiter.poll(ctx, &shared.state, &shared.waiters) {
            ShotState::Empty => return Poll::Pending,
            ShotState::Sent => Some(Value {
                shared: shared.clone(),
            }),
            ShotState::Closed => None,
        };
        *this.state = Poll::Ready(value.clone());

        Poll::Ready(value)
    }
}

#[pinned_drop]
impl<T> PinnedDrop for Recv<T> {
    fn drop(self: Pin<&mut Self>) {
        let this = self.project();

        this.waiter.pinned_drop(&this.shot.shared.waiters);
    }
}

pub struct Receiver<T> {
    shared: Arc<State<T>>,
}

impl<T> Receiver<T> {
    /// Attempts to receive the value. This returns an owned copy of the value that can be used separately from the receiver.
    pub fn try_recv(&self) -> Result<Value<T>, TryRecvError> {
        match self.shared.state.state() {
            ShotState::Closed => Err(TryRecvError::Closed),
            ShotState::Empty => Err(TryRecvError::Empty),
            ShotState::Sent => Ok(Value {
                shared: self.shared.clone(),
            }),
        }
    }

    pub fn try_ref(&self) -> Result<&T, TryRecvError> {
        self.shared
            .state
            .state()
            .map_sent(|| unsafe { self.shared.get_ref() })
    }

    pub fn recv(self) -> Recv<T> {
        Recv::new(self)
    }
}

impl<T> Clone for Receiver<T> {
    fn clone(&self) -> Self {
        unsafe { self.shared.add_receiver() }

        Self {
            shared: self.shared.clone(),
        }
    }
}

impl<T> Drop for Receiver<T> {
    fn drop(&mut self) {
        unsafe { self.shared.remove_receiver() }
    }
}

impl<T> IntoFuture for Receiver<T> {
    type IntoFuture = Recv<T>;
    type Output = Option<Value<T>>;

    fn into_future(self) -> Self::IntoFuture {
        self.recv()
    }
}

/// A reference to a sharedshot result.
#[derive(Debug)]
pub struct Value<T> {
    shared: Arc<State<T>>,
}

impl<T> Value<T> {
    fn get(&self) -> &T {
        // SAFETY: We only construct a Value when we know the inner value is set.
        unsafe { self.shared.get_ref() }
    }

    // Returns the inner value, if the Value has exactly one reference.
    pub fn try_unwrap(this: Self) -> Result<T, Value<T>> {
        Arc::try_unwrap(this.shared)
            .map(|shared| unsafe { shared.take() })
            .map_err(|shared| Value { shared })
    }
}

impl<T> Clone for Value<T> {
    fn clone(&self) -> Self {
        Self {
            shared: Arc::clone(&self.shared),
        }
    }
}

impl<T> AsRef<T> for Value<T> {
    fn as_ref(&self) -> &T {
        Value::get(self)
    }
}

impl<T> Borrow<T> for Value<T> {
    fn borrow(&self) -> &T {
        Value::get(self)
    }
}

impl<T> Deref for Value<T> {
    type Target = T;

    fn deref(&self) -> &Self::Target {
        Value::get(self)
    }
}

#[cfg(test)]
mod test {
    use super::*;
    use tokio_test::*;

    #[test]
    fn send_recv() {
        let (send, recv) = channel();
        let mut recv1 = task::spawn(recv.clone().recv());
        let mut recv2 = task::spawn(recv.clone().recv());

        assert_pending!(recv1.poll());
        assert_pending!(recv2.poll());

        assert_ok!(send.send(123));

        assert!(recv1.is_woken());
        assert!(recv2.is_woken());

        let val1 = assert_ready!(recv1.poll()).unwrap();
        let val2 = assert_ready!(recv2.poll()).unwrap();

        assert_eq!(*val1, *val2);
    }

    #[tokio::test]
    async fn async_send_recv() {
        let (send, recv) = channel();
        assert_ok!(send.send(123));
        assert_eq!(*(recv.await.unwrap()), 123);
    }
}
