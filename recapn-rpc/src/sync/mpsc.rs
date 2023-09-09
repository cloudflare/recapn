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

use crate::Error;
use crate::sync::request::{SharedRequest, Request};
use std::marker::PhantomData;
use std::ptr::NonNull;
use std::sync::Arc;
use std::sync::atomic::AtomicPtr;

use parking_lot::Mutex;

use super::util::linked_list::LinkedList;

pub use super::TryRecvError;

pub struct Sender<T, U> {
    inner: Arc<SharedQueue<T, U>>,
}

unsafe impl<T: Send, U: Send> Send for Sender<T, U> {}
unsafe impl<T: Sync, U: Sync> Sync for Sender<T, U> {}

impl<T, U> Sender<T, U> {
    pub fn send(&self, req: Request<T, U>) {
        todo!()
    }

    /// Returns a new sender for the inner most forwarded queue. This can save some time
    /// needing to resolve the current queue when sending requests.
    pub fn resolved(&self) -> Option<Sender<T, U>> {
        todo!()
    }

    /// Returns whether the channel is closed.
    pub fn is_closed(&self) -> bool {
        todo!()
    }

    /// Waits until the channel is closed
    pub async fn closed(&self) {
        todo!()
    }
}

impl<T, U> Clone for Sender<T, U> {
    fn clone(&self) -> Self {
        todo!()
    }
}

pub struct Receiver<T, U> {
    tu: PhantomData<(T, U)>,
}

unsafe impl<T: Send, U: Send> Send for Receiver<T, U> {}
unsafe impl<T: Send, U: Send> Sync for Receiver<T, U> {}

impl<T, U> Receiver<T, U> {
    /// Forward all the requests from this receiver to the given sender in one operation
    /// while keeping request ordering. After this operation the receiver is consumed,
    /// but senders that were originally associated with this receiver will automatically
    /// begin refering to the channel associated with the forwarded channel.
    /// 
    /// Forwarding to a sender on this same channel will result in the receiver being returned.
    pub fn forward_to(self, other: &Sender<T, U>) -> Result<(), Self> {
        todo!()
    }

    pub async fn recv(&mut self) -> Option<Request<T, U>> {
        todo!()
    }

    /// Tries to receive the next value for this receiver.
    pub fn try_recv(&mut self) -> Result<T, TryRecvError> {
        todo!()
    }

    pub fn close(&mut self) {
        todo!()
    }
}

/// The shared state behind a request queue
struct SharedQueue<T, U> {
    /// The owner of this queue. This points at the current queue if it hasn't been forwarded.
    /// When a queue has been forwarded to another, this points at the other queue requests
    /// have been forwarded to.
    owner: AtomicPtr<SharedQueue<T, U>>,

    ptrs: Mutex<LinkedList<(), ()>>,
}

impl<T, U> SharedQueue<T, U> {

}

pub fn channel<T, U>() -> (Sender<T, U>, Receiver<T, U>) {
    todo!()
}
