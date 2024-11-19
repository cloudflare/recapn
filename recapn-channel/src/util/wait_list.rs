use std::cell::UnsafeCell;
use std::marker::PhantomPinned;
use std::pin::{pin, Pin};
use std::ptr::{addr_of_mut, NonNull};
use std::sync::atomic::AtomicBool;
use std::sync::atomic::Ordering::{Acquire, Release, Relaxed};
use std::task::{Context, Waker};

use parking_lot::Mutex;
use pin_project::pin_project;

use super::array_vec::ArrayVec;
use super::atomic_state::{AtomicState, ShotState};
use super::linked_list::{GuardedLinkedList, Link, LinkedList, Pointers};

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

impl Drop for WaitList {
    fn drop(&mut self) {
        debug_assert!(self.list.get_mut().is_empty());
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
                match state {
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

/// A waiter helper to make the poll logic behind sharedshot components reusable.
///
/// This contains the state of the waiter and nothing else. Custom pollers call into this
/// via the poll function and provide the atomic state and wait list that the waiter puts
/// itself in.
#[derive(Debug)]
#[pin_project]
pub(crate) struct ClosedWaiter {
    #[pin]
    waiter: Option<Waiter>,
}

impl ClosedWaiter {
    /// Creates a new recv waiter for waiting on the result of a sharedshot.
    pub const fn new() -> Self {
        Self { waiter: None }
    }

    pub fn poll(
        self: Pin<&mut Self>,
        ctx: &mut Context<'_>,
        atom: &AtomicState,
        waiters: &WaitList,
    ) -> bool {
        let mut this = self.project();

        match this.waiter.as_mut().as_pin_mut() {
            None => {
                if atom.load(Acquire).is_recv_closed() {
                    return true;
                }

                // Clone the waker before locking, a waker clone can be triggering arbitrary code.
                let waker = ctx.waker().clone();

                // Acquire the lock and attempt to transition to the waiting state.
                let mut waiters = waiters.list.lock();

                if atom.load(Acquire).is_recv_closed() {
                    return true;
                }

                let waiter =
                    unsafe { Pin::get_unchecked_mut(this.waiter).insert(Waiter::new(Some(waker))) };

                // Insert the waiter into the linked list
                waiters.push_front(NonNull::from(&*waiter));

                false
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
                    return atom.load(Acquire).is_recv_closed();
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
                    return atom.load(Acquire).is_recv_closed();
                }

                // Before we add a waiter to the list we check if the channel has closed.
                // If it's closed it means that there is a call to `notify_waiters` in
                // progress and this waiter must be contained by a guarded list used in
                // `notify_waiters`. So at this point we can treat the waiter as notified and
                // remove it from the list, as it would have been notified in the
                // `notify_waiters` call anyways.

                let is_closed = atom.load(Acquire).is_recv_closed();
                if is_closed {
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
                } else {
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

                is_closed
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
