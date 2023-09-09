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
use std::ptr::{NonNull, addr_of_mut};
use std::sync::Arc;
use std::sync::atomic::Ordering::{self, *};
use std::sync::atomic::{AtomicBool, AtomicU8, AtomicUsize};
use std::task::{Context, Poll, Waker};

use super::util::Marc;
use super::util::linked_list::{LinkedList, GuardedLinkedList, Link, Pointers};
use super::util::wake_list::WakeList;
use parking_lot::Mutex;
use pin_project::{pin_project, pinned_drop};

pub use super::TryRecvError;

type WaitList = Mutex<LinkedList<Waiter, Waiter>>;

/// List used in `Notify::notify_waiters`. It wraps a guarded linked list
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
    fn pop_back_locked(&mut self, _waiters: &mut LinkedList<Waiter, Waiter>) -> Option<NonNull<Waiter>> {
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
            let _lock_guard = self.waiters.lock();
            while let Some(waiter) = self.list.pop_back() {
                // Safety: we never make mutable references to waiters.
                let waiter = unsafe { waiter.as_ref() };
                waiter.notified.store(true, Release);
            }
        }
    }
}

#[derive(Debug)]
#[pin_project(project = RecvInnerProject)]
enum RecvInner<'a, T> {
    Init {
        /// The shot state
        shot: &'a Arc<State<T>>,
    },
    Waiting {
        /// The shot state
        shot: &'a Arc<State<T>>,

        /// The waiter, pinned in the Recv's linked list
        #[pin]
        waiter: Waiter,
    },
    /// The receive is done polling, so polling the future returns clones of the
    /// specified value.
    Done(Option<Value<T>>),
}

#[pin_project(PinnedDrop)]
pub struct Recv<'a, T> {
    #[pin]
    inner: RecvInner<'a, T>,
}

unsafe impl<T: Send> Send for Recv<'_, T> {}
unsafe impl<T: Sync> Sync for Recv<'_, T> {}

impl<'a, T> Recv<'a, T> {
    fn new(state: Option<&'a Arc<State<T>>>) -> Self {
        match state {
            Some(shot) => Recv { inner: RecvInner::Init { shot } },
            None => Recv { inner: RecvInner::Done(None) }
        }
    }
}

impl<'a, T> Future for Recv<'a, T> {
    type Output = Option<Value<T>>;

    fn poll(self: Pin<&mut Self>, ctx: &mut Context<'_>) -> Poll<Option<Value<T>>> {
        let mut this = self.project();

        match this.inner.as_mut().project() {
            RecvInnerProject::Init { shot } => {
                if let Some(value) = shot.try_recv_options() {
                    this.inner.set(RecvInner::Done(value.clone()));
                    return Poll::Ready(value)
                }

                // Clone the waker before locking, a waker clone can be triggering arbitrary code.
                let waker = ctx.waker().clone();

                // Acquire the lock and attempt to transition to the waiting state.
                let mut waiters = shot.waiters.lock();

                if let Some(value) = shot.try_recv_options() {
                    this.inner.set(RecvInner::Done(value.clone()));
                    return Poll::Ready(value)
                }

                let new_state = RecvInner::Waiting { shot, waiter: Waiter::new(Some(waker)) };
                this.inner.set(new_state);
                let RecvInnerProject::Waiting { waiter, .. } = this.inner.project() else {
                    unreachable!()
                };

                // Insert the waiter into the linked list
                waiters.push_front(NonNull::from(&*waiter));

                return Poll::Pending
            },
            RecvInnerProject::Waiting { shot, waiter } => {
                if waiter.notified.load(Acquire) {
                    // Safety: waiter is already unlinked and will not be shared again,
                    // so we have exclusive access `waker`.
                    unsafe {
                        let waker = &mut *waiter.waker.get();
                        drop(waker.take())
                    }

                    let value = shot.try_recv_options().unwrap();

                    this.inner.set(RecvInner::Done(value.clone()));
                    return Poll::Ready(value)
                }

                // We were not notified, implying it is still stored in a waiter list.
                // In order to access the waker fields, we must acquire the lock first.

                let mut old_waker = None;
                let mut waiters = shot.waiters.lock();

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

                    let value = shot.try_recv_options().unwrap();

                    this.inner.set(RecvInner::Done(value.clone()));
                    return Poll::Ready(value)
                }

                // Before we add a waiter to the list we check if the channel has closed.
                // If it's closed it means that there is a call to `notify_waiters` in
                // progress and this waiter must be contained by a guarded list used in
                // `notify_waiters`. So at this point we can treat the waiter as notified and
                // remove it from the list, as it would have been notified in the
                // `notify_waiters` call anyways.

                let Some(value) = shot.try_recv_options() else {
                    // The channel hasn't closed, let's update the waker
                    unsafe {
                        let current = &mut *waiter.waker.get();
                        let polling_waker = ctx.waker();
                        let should_update =
                            current.is_none() || matches!(current, Some(w) if w.will_wake(polling_waker));
                        if should_update {
                            old_waker = std::mem::replace(&mut *current, Some(polling_waker.clone()));
                        }
                    }

                    // Drop the old waker after releasing the lock.
                    drop(waiters);
                    drop(old_waker);

                    return Poll::Pending
                };

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

                this.inner.set(RecvInner::Done(value.clone()));
                return Poll::Ready(value)
            },
            RecvInnerProject::Done(result) => return Poll::Ready(result.clone()),
        }
    }
}

#[pinned_drop]
impl<T> PinnedDrop for Recv<'_, T> {
    fn drop(self: Pin<&mut Self>) {
        let this = self.project();

        if let RecvInnerProject::Waiting { shot, waiter } = this.inner.project() {
            let mut waiters = shot.waiters.lock();

            // remove the entry from the list (if not already removed)
            //
            // Safety: we hold the lock, so we have an exclusive access to every list the
            // waiter may be contained in. If the node is not contained in the `waiters`
            // list, then it is contained by a guarded list used by `notify_waiters`.
            unsafe { waiters.remove(NonNull::from(&*waiter)) };
        }
    }
}

#[derive(Debug)]
struct Waiter {
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
    pub fn new(waker: Option<Waker>) -> Self {
        Self {
            pointers: Pointers::new(),
            waker: UnsafeCell::new(waker),
            notified: AtomicBool::new(false),
            _pinned: PhantomPinned,
        }
    }
}

unsafe impl Link for Waiter {
    type Handle = NonNull<Waiter>;
    type Target = Waiter;

    fn as_raw(handle: &Self::Handle) -> NonNull<Self::Target> {
        *handle
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

pub fn channel<T>() -> (Sender<T>, Receiver<T>) {
    let state = Arc::new(State::new(1));
    let sender = Sender { shared: Marc::new(state.clone()) };
    let receiver = Receiver { shared: Some(state) };

    (sender, receiver)
}

#[derive(Clone, Copy)]
struct StateFlags(u8);

impl StateFlags {
    const SET: u8 =             0b0001;
    const SEND_CLOSED: u8 =     0b0010;
    const RECV_CLOSED: u8 =     0b0100;
    const CLOSED_TASK_SET: u8 = 0b1000;

    #[inline]
    pub fn load_from(cell: &AtomicU8, order: Ordering) -> Self {
        Self(cell.load(order))
    }

    #[inline]
    pub fn is_set(self) -> bool {
        self.0 & Self::SET != 0
    }

    #[inline]
    pub fn is_send_closed(self) -> bool {
        self.0 & Self::SEND_CLOSED != 0
    }

    #[inline]
    pub fn set_send_closed(cell: &AtomicU8) -> Self {
        let val = cell.fetch_or(Self::SEND_CLOSED, Release);
        Self(val | Self::SEND_CLOSED)
    }

    #[inline]
    pub fn is_recv_closed(self) -> bool {
        self.0 & Self::RECV_CLOSED != 0
    }

    #[inline]
    pub fn set_recv_closed(cell: &AtomicU8) -> Self {
        let val = cell.fetch_or(Self::RECV_CLOSED, AcqRel);
        Self(val | Self::RECV_CLOSED)
    }

    #[inline]
    pub fn is_closed_task_set(self) -> bool {
        self.0 & Self::CLOSED_TASK_SET != 0
    }

    #[inline]
    pub fn set_closed_task(cell: &AtomicU8) -> Self {
        let val = cell.fetch_or(Self::CLOSED_TASK_SET, AcqRel);
        Self(val | Self::CLOSED_TASK_SET)
    }

    #[inline]
    pub fn unset_closed_task(cell: &AtomicU8) -> Self {
        let val = cell.fetch_and(!Self::CLOSED_TASK_SET, AcqRel);
        Self(val & !Self::CLOSED_TASK_SET)
    }

    #[inline]
    pub fn set_value(cell: &AtomicU8) -> Self {
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
        let mut state = cell.load(Relaxed);
        loop {
            if Self(state).is_recv_closed() {
                break;
            }

            let exchange_result = cell.compare_exchange_weak(
                state, state | Self::SET, Release, Acquire);
            match exchange_result {
                Ok(_) => break,
                Err(actual) => state = actual,
            }
        }
        Self(state)
    }
}

/// The sharedshot state. This is public for the crate so that we can combine
/// allocations.
#[derive(Debug)]
pub(crate) struct State<T> {
    /// The state of the shot.
    state: AtomicU8,

    /// A set of waiters waiting to receive the value.
    waiters: WaitList,

    /// The number of receivers waiting for the response. When this value reaches zero,
    /// the channel is closed and the closed task is woken up.
    receivers_count: AtomicUsize,

    /// The close task, woken up when all the receivers have been dropped and the channel
    /// has been marked closed.
    closed_task: UnsafeCell<MaybeUninit<Waker>>,

    /// The value in the shot.
    value: UnsafeCell<MaybeUninit<T>>,
}

unsafe impl<T: Send> Send for State<T> {}
unsafe impl<T: Sync> Sync for State<T> {}

/// The state of the sharedshot.
pub(crate) enum ShotState {
    Sent,
    Empty,
    Closed,
}

impl ShotState {
    pub fn map_sent<T>(self, f: impl FnOnce() -> T) -> Result<T, TryRecvError> {
        match self {
            ShotState::Sent => Ok(f()),
            ShotState::Empty => Err(TryRecvError::Empty),
            ShotState::Closed => Err(TryRecvError::Closed),
        }
    }
}

impl<T> State<T> {
    pub const fn new(rcvs: usize) -> Self {
        Self {
            state: AtomicU8::new(0),
            waiters: WaitList::new(LinkedList::new()),
            receivers_count: AtomicUsize::new(rcvs),
            closed_task: UnsafeCell::new(MaybeUninit::uninit()),
            value: UnsafeCell::new(MaybeUninit::uninit()),
        }
    }

    /// Sends the value, or returns it if no receivers exist.
    pub unsafe fn send(&self, value: T) -> Result<(), T> {
        let value_store = &mut *self.value.get();
        value_store.write(value);

        let prev = StateFlags::set_value(&self.state);
        if prev.is_recv_closed() {
            return Err(value_store.assume_init_read());
        }

        self.wake_recvs();

        Ok(())
    }

    pub unsafe fn add_receiver(&self) {
        let old = self.receivers_count.fetch_add(1, Relaxed);

        // Make sure I don't accidentally attempt to re-open the channel.
        debug_assert_ne!(old, 0);
    }

    /// Remove a tracked receiver from the receiver count.
    /// 
    /// If this is the last receiver (and the receiver count is zero), this closes the channel on
    /// the receiving side.
    /// 
    /// Note: A channel cannot be re-opened by adding a receiver when the channel is closed.
    pub unsafe fn remove_receiver(&self) -> bool {
        let last_receiver = self.receivers_count.fetch_sub(1, Relaxed) == 0;
        if last_receiver {
            self.close_receiver()
        }
        last_receiver
    }

    /// Close the send half of the channel.
    pub unsafe fn close_sender(&self) {
        StateFlags::set_send_closed(&self.state);
        self.wake_recvs()
    }

    /// Close the receive half of the channel.
    fn close_receiver(&self) {
        let state = StateFlags::set_recv_closed(&self.state);
        if state.is_closed_task_set() && !(state.is_send_closed() || state.is_set()) {
            unsafe { 
                let closed_task_store = &*self.closed_task.get();
                closed_task_store.assume_init_ref().wake_by_ref();
            }
        }
    }

    pub fn is_receivers_closed(&self) -> bool {
        let state = StateFlags::load_from(&self.state, Acquire);
        state.is_recv_closed()
    }

    /// Takes the value out of the shared state. This assumes the value is initialized
    pub unsafe fn take(mut self) -> T {
        // Erase the set bit flag, since immediately after this the State is getting dropped.
        *self.state.get_mut() &= !StateFlags::SET;
        self.value.get_mut().assume_init_read()
    }

    pub fn try_recv(self: &Arc<Self>) -> Result<Value<T>, TryRecvError> {
        self.state().map_sent(|| Value { shared: self.clone() })
    }

    pub fn state(&self) -> ShotState {
        let state = StateFlags::load_from(&self.state, Acquire);
        if state.is_set() {
            ShotState::Sent
        } else if state.is_send_closed() {
            ShotState::Closed
        } else {
            ShotState::Empty
        }
    }

    pub unsafe fn get_ref(&self) -> &T {
        (&*self.value.get()).assume_init_ref()
    }

    pub fn sender_poll_closed(&self, cx: &mut Context<'_>) -> Poll<()> {
        let mut state = StateFlags::load_from(&self.state, Acquire);

        // The channel is closed. The waker will be destroyed when the sharedshot is dropped.
        if state.is_recv_closed() {
            return Poll::Ready(())
        }

        let waker = cx.waker();
        let closed_task_store = unsafe { &mut *self.closed_task.get() };
        if state.is_closed_task_set() {
            let closed_task = unsafe { closed_task_store.assume_init_ref() };
            if !closed_task.will_wake(waker) {
                // We have a new waker to insert. Unset the flag and drop the task.
                // This call will replace the local state with a state that doesn't have the flag
                // set so when we fall through we'll put the new waker in.
                state = StateFlags::unset_closed_task(&self.state);

                if state.is_recv_closed() {
                    // Oop, the task closed while we were doing this. Let's set the flag again
                    // so the waker is dropped when the sharedshot is destroyed.
                    StateFlags::set_closed_task(&self.state);
                    return Poll::Ready(())
                }

                unsafe { closed_task_store.assume_init_drop() }
            }
        }

        if !state.is_closed_task_set() {
            closed_task_store.write(waker.clone());
            state = StateFlags::set_closed_task(&self.state);

            if state.is_recv_closed() {
                return Poll::Ready(())
            }
        }

        Poll::Pending
    }

    fn wake_recvs(&self) {
        let mut waiters = self.waiters.lock();

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
        let mut list = RecvWaitersList::new(std::mem::take(&mut *waiters), pinned_guard.as_ref(), &self.waiters);

        let mut wakers = WakeList::new();
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
            wakers.wake_all();

            // Acquire the lock again.
            waiters = self.waiters.lock();
        }

        // Release the lock before notifying
        drop(waiters);

        wakers.wake_all();
    }

    /// Maps the result of try_recv into layered options, where None indicates the value hasn't
    /// been received, Some(None) indicates the channel was closed, and Some(Some) indicates the
    /// value was sent.
    fn try_recv_options(self: &Arc<Self>) -> Option<Option<Value<T>>> {
        match self.try_recv() {
            Ok(value) => Some(Some(value)),
            Err(TryRecvError::Closed) => Some(None),
            Err(TryRecvError::Empty) => None,
        }
    }
}

impl<T> Drop for State<T> {
    fn drop(&mut self) {
        let state = StateFlags(*self.state.get_mut());

        if state.is_set() {
            unsafe { self.value.get_mut().assume_init_drop() }
        }

        if state.is_closed_task_set() {
            unsafe { self.closed_task.get_mut().assume_init_drop() }
        }

        debug_assert!(self.waiters.get_mut().is_empty());
    }
}

pub struct Sender<T> {
    shared: Marc<State<T>>,
}

impl<T> Sender<T> {
    /// Send a value. If the channel is closed, this returns it back.
    pub fn send(mut self, value: T) -> Result<(), T> {
        unsafe {
            let state = self.shared.take();
            state.send(value)
        }
    }

    pub async fn closed(&mut self) {
        let closed = poll_fn(|cx| self.shared.get().sender_poll_closed(cx));
        closed.await
    }

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

pub struct Receiver<T> {
    shared: Option<Arc<State<T>>>,
}

impl<T> Receiver<T> {
    /// Attempts to receive the value. This returns an owned copy of the value that can be used separately from the receiver.
    pub fn try_recv(&self) -> Result<Value<T>, TryRecvError> {
        self.shared
            .as_ref()
            .ok_or(TryRecvError::Closed)
            .and_then(State::try_recv)
    }

    pub fn try_ref(&self) -> Result<&T, TryRecvError> {
        let Some(shared) = &self.shared else {
            return Err(TryRecvError::Closed)
        };

        shared.state().map_sent(|| unsafe { shared.get_ref() })
    }

    pub fn recv(&self) -> Recv<'_, T> {
        Recv::new(self.shared.as_ref())
    }
}

impl<T> Clone for Receiver<T> {
    fn clone(&self) -> Self {
        let Some(shared) = &self.shared else {
            return Self { shared: None }
        };

        unsafe { shared.add_receiver() }

        Self { shared: Some(shared.clone()) }
    }
}

impl<T> Drop for Receiver<T> {
    fn drop(&mut self) {
        if let Some(shared) = &self.shared {
            unsafe { shared.remove_receiver(); }
        }
    }
}

impl<'a, T> IntoFuture for &'a Receiver<T> {
    type IntoFuture = Recv<'a, T>;
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
        Self { shared: Arc::clone(&self.shared) }
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

}