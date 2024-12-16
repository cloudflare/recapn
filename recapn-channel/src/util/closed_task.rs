use core::cell::UnsafeCell;
use core::mem::MaybeUninit;
use core::sync::atomic::Ordering::Acquire;
use core::task::{Context, Poll, Waker};

use super::atomic_state::AtomicState;

#[derive(Debug)]
pub struct ClosedTask {
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
