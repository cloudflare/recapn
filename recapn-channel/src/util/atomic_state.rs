//! Atomic state shared by channels and requests.

use std::sync::atomic::AtomicU8;
use std::sync::atomic::Ordering::{self, AcqRel, Acquire, Relaxed, Release};

const STATE_SET: u8 = 0b0000_0001;
const STATE_SEND_CLOSED: u8 = 0b0000_0010;
const STATE_RECV_CLOSED: u8 = 0b0000_0100;
const STATE_CLOSED_TASK_SET: u8 = 0b0000_1000;

const STATE_SET_PIPELINE: u8 = 0b0001_0000;

/// The state of the sharedshot.
#[derive(PartialEq, Eq, Clone, Copy)]
pub(crate) enum ShotState {
    Sent,
    Empty,
    Closed,
}

/// Atomic state used by sharedshot and request state.
#[derive(Debug)]
pub(crate) struct AtomicState(AtomicU8);

impl AtomicState {
    #[inline]
    pub const fn new() -> Self {
        Self(AtomicU8::new(0))
    }

    /// Make a new instance of the state but with the set bit already set.
    #[inline]
    pub const fn new_set() -> Self {
        Self(AtomicU8::new(STATE_SET))
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
