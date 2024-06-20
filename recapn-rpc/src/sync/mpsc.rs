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

use crate::sync::request::{SharedRequest, Request};
use crate::sync::util::array_vec::ArrayVec;
use std::fmt::{self, Debug};
use std::future::poll_fn;
use std::hash::{Hash, Hasher};
use std::sync::{Arc, Weak};
use std::task::{Waker, Context, Poll};

use parking_lot::{Mutex, MutexGuard};

use super::request::{self, Chan, IntoResults};
use super::sharedshot::{self, ShotState};
use super::util::linked_list::LinkedList;

pub use super::TryRecvError;

#[derive(Clone, Copy)]
pub enum Resolution<'a, C: Chan> {
    /// The channel was forwarded to another channel
    Forwarded(&'a Sender<C>),
    /// The receiving end was dropped
    Dropped,
    /// The channel was closed with the specified error
    Error(&'a C::Error),
}

#[derive(Clone, Copy)]
pub enum MostResolved<'a, E> {
    /// The receiving end was dropped
    Dropped,
    /// The channel was closed with the specified error
    Error(&'a E),
}

#[derive(Debug)]
pub struct Sender<C: Chan + ?Sized> {
    shared: Arc<SharedChannel<C>>,
}

impl<C: Chan> Clone for Sender<C> {
    fn clone(&self) -> Self {
        Self { shared: self.shared.clone() }
    }
}

impl<C: Chan> Sender<C> {
    pub fn send(&self, req: Request<C>) -> Result<(), Request<C>> {
        if req.is_finished() {
            // Just drop the request nobody wants the result
            return Ok(())
        }

        let (this, mut channel) = match self.shared.resolve_and_lock() {
            Ok(v) => v,
            Err(r) => return match r {
                MostResolved::Dropped => Err(req),
                MostResolved::Error(err) => {
                    let (_, responder) = req.respond();
                    responder.respond(err.clone().into_results());
                    Ok(())
                }
            }
        };

        let req_shared = req.take_inner();

        // Insert the request into the list.
        channel.requests.push_back(req_shared.clone());

        unsafe {
            req_shared.set_parent(this.clone());
        }

        let waker = channel.waker.take();

        // Drop the lock before waking the receiver since the waker could do literally
        // anything.
        drop(channel);

        if let Some(waker) = waker {
            waker.wake();
        }

        Ok(())
    }

    /// Returns whether the channel is resolved.
    pub fn is_resolved(&self) -> bool {
        self.try_resolved().is_some()
    }

    pub fn try_resolved(&self) -> Option<Resolution<C>> {
        self.shared.try_resolved()
    }

    pub fn most_resolved(mut self: &Self) -> (&Self, Option<MostResolved<C::Error>>) {
        loop {
            let resolution = self.try_resolved();
            return match resolution {
                None => (self, None),
                Some(Resolution::Dropped) => (self, Some(MostResolved::Dropped)),
                Some(Resolution::Error(err)) => (self, Some(MostResolved::Error(err))),
                Some(Resolution::Forwarded(channel)) => {
                    self = &channel;
                    continue
                },
            }
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
    pub async fn resolution(&self) -> Resolution<C> {
        todo!()
    }

    #[inline]
    pub fn chan(&self) -> &C {
        &self.shared.chan
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
        std::ptr::hash(Arc::as_ptr(&self.shared), state)
    }
}

#[derive(Debug)]
pub struct Receiver<C: Chan> {
    shared: Arc<SharedChannel<C>>,
}

unsafe impl<C: Chan> Send for Receiver<C>
where
    Request<C>: Send {}

unsafe impl<C: Chan> Sync for Receiver<C>
where
    Request<C>: Send {}

impl<C: Chan> Receiver<C> {
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
        let (other_shared, mut other_lock) = match other.shared.resolve_and_lock() {
            Ok(v) => v,
            Err(r) => {
                match r {
                    MostResolved::Dropped => drop(self),
                    MostResolved::Error(err) => self.close(err.clone()),
                }
                return Ok(())
            }
        };

        if Arc::ptr_eq(&self.shared, other_shared) {
            return Err(self)
        }

        let mut self_lock = self.shared.state.lock();
        if self_lock.requests.is_empty() {
            unsafe {
                let _ = self.shared.resolution.send(ChannelResolution::Forward(other.clone()));
            }
            return Ok(())
        }

        other_lock.requests.append_back(&mut self_lock.requests);

        let waker = other_lock.waker.take();

        drop(self_lock);
        drop(other_lock);

        if let Some(w) = waker {
            w.wake()
        }

        Ok(())
    }

    /// Await the closing of the channel, without taking any requesting from it.
    pub async fn closed(&mut self) {
        poll_fn(|cx| self.poll_closed(cx)).await
    }

    pub fn poll_closed(&mut self, cx: &mut Context) -> Poll<()> {
        self.shared.resolution.sender_poll_closed(cx)
    }

    pub fn is_closed(&self) -> bool {
        self.shared.resolution.is_receivers_closed()
    }

    pub async fn recv(&mut self) -> Option<Request<C>> {
        poll_fn(|cx| self.poll_recv(cx)).await
    }

    pub fn poll_recv(&mut self, cx: &mut Context) -> Poll<Option<Request<C>>> {
        if self.is_closed() {
            return Poll::Ready(None)
        }

        let waker = cx.waker().clone();

        let mut locked = self.shared.state.lock();
        let next = locked.requests.pop_front();

        if let Some(n) = next {
            unsafe { n.take_parent() };
            return Poll::Ready(Some(Request::new(n)))
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
    pub fn try_recv(&mut self) -> Option<Request<C>> {
        let mut locked = self.shared.state.lock();
        locked.requests.pop_front().map(Request::new)
    }

    /// Close this channel with the given error.
    pub fn close(self, err: C::Error) {
        let mut self_lock = self.shared.state.lock();
        drop(self_lock.waker.take());
        let _ = unsafe { self.shared.resolution.send(ChannelResolution::Error(err.clone())) };

        scopeguard::defer_on_unwind! {
            let mut self_lock = self.shared.state.lock();
            while let Some(r) = self_lock.requests.pop_front() {
                unsafe {
                    r.take_parent();
                    r.drop_request();
                    // TODO: Maybe signal that the request is over without waking everyone up?
                }
            }
        };

        let mut request_array = ArrayVec::<Request<C>, 32>::new();

        let respond_with_err = |r: Request<C>| {
            let (_, responder) = r.respond();
            responder.respond(err.clone().into_results());
        };

        'outer: loop {
            while request_array.can_push() {
                let Some(req) = self_lock.requests.pop_front() else { break 'outer };

                unsafe { req.take_parent(); }

                request_array.push(Request::new(req));
            }

            drop(self_lock);

            request_array.for_each(respond_with_err);

            self_lock = self.shared.state.lock();
        }

        drop(self_lock);

        request_array.for_each(respond_with_err);
    }
}

pub(crate) struct State<C: Chan> {
    /// If this is a pipeline channel, this holds a reference back to the parent request to
    /// make sure it doesn't go out of scope. The parent request itself holds a weak pointer
    /// to this channel to make sure the reference cycle is broken.
    parent_request: Option<request::Receiver<C>>,

    requests: LinkedList<SharedRequest<C>, SharedRequest<C>>,

    /// The waker set by the receiver to wake up the receiver task.
    waker: Option<Waker>,
}

enum ChannelResolution<C: Chan> {
    Forward(Sender<C>),
    Error(C::Error),
}

/// The shared state behind a request queue
pub(crate) struct SharedChannel<C: Chan + ?Sized> {
    state: Mutex<State<C>>,

    /// A sharedshot to track channel resolution.
    /// 
    /// In a channel, all senders and requests in the channel are considered receivers of
    /// the sharedshot result. This prevents the sharedshot from prematurely closing if all
    /// senders are dropped but requests are active in the channel.
    /// 
    /// Instances where all senders have dropped but requests are still active are common
    /// in pipeline clients, where a request is sent on the pipeline, then the sender is
    /// dropped. As long as the request has receivers for its reponse, it should stay active,
    /// instead of getting canceled because the channel closed and was dropped.
    resolution: sharedshot::State<ChannelResolution<C>>,

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
    pub fn try_resolved(&self) -> Option<Resolution<C>> {
        match self.resolution.state() {
            ShotState::Closed => Some(Resolution::Dropped),
            ShotState::Empty => None,
            ShotState::Sent => {
                let res = unsafe { self.resolution.get_ref() };
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
                    continue
                }
            }
        }
    }

    pub fn most_resolved<'a>(mut self: &'a Arc<Self>) -> (&'a Arc<Self>, Option<MostResolved<'a, C::Error>>) {
        loop {
            let resolution = self.try_resolved();
            return match resolution {
                None => (self, None),
                Some(Resolution::Dropped) => (self, Some(MostResolved::Dropped)),
                Some(Resolution::Error(err)) => (self, Some(MostResolved::Error(err))),
                Some(Resolution::Forwarded(channel)) => {
                    self = &channel.shared;
                    continue
                },
            }
        }
    }

    /// Resolve into the inner-most forwarded channel and acquire a lock to the channel
    pub fn resolve_and_lock<'a>(self: &'a Arc<Self>) -> Result<(&'a Arc<Self>, MutexGuard<'a, State<C>>), MostResolved<'a, C::Error>> {
        // First we need to find the most resolved version of this channel.
        let (mut this, mut resolution) = self.most_resolved();

        loop {
            if let Some(res) = resolution {
                return Err(res)
            }
    
            // Now we have the most resolved channel, lock it.
            let channel = this.state.lock();
    
            // But during that time spent locking it, it might've resolved further, so go through
            // the above steps again...
            (this, resolution) = this.most_resolved();

            if resolution.is_none() {
                // If it hasn't resolved, break from the loop.
                break Ok((this, channel))
            }
        }
    }

    pub fn lock_unresolved(&self) -> Option<MutexGuard<State<C>>> {
        todo!()
    }
}

pub(crate) struct ResolutionReceiver<C: Chan>(Arc<SharedChannel<C>>);

impl<C: Chan> ResolutionReceiver<C> {
    pub fn track(s: Arc<SharedChannel<C>>) -> Self {
        unsafe { s.resolution.add_receiver() }
        Self(s)
    }
}

impl<C: Chan> Drop for ResolutionReceiver<C> {
    fn drop(&mut self) {
        unsafe { self.0.resolution.remove_receiver() }
    }
}

pub fn channel<C: Chan>(chan: C) -> (Sender<C>, Receiver<C>) {
    let channel = Arc::new(SharedChannel {
        state: Mutex::new(State {
            parent_request: None,
            requests: LinkedList::new(),
            waker: None,
        }),
        // 1 receiver for the sender
        resolution: sharedshot::State::new(1),
        chan,
    });
    let sender = Sender { shared: channel.clone() };
    let receiver = Receiver { shared: channel };
    (sender, receiver)
}

/// Creates a sender to a broken channel with the given error
pub fn broken<C: Chan>(chan: C, err: C::Error) -> Sender<C> {
    let channel = Arc::new(SharedChannel {
        state: Mutex::new(State {
            parent_request: None,
            requests: LinkedList::new(),
            waker: None,
        }),
        // 1 receiver for the sender
        resolution: sharedshot::State::new(1),
        chan,
    });
    let _ = unsafe { channel.resolution.send(ChannelResolution::Error(err)) };
    let sender = Sender { shared: channel };
    sender
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
pub struct WeakChannel<C: Chan> {
    shared: Weak<SharedChannel<C>>,
}

impl<C: Chan> WeakChannel<C> {
    pub fn sender(&self) -> Option<Sender<C>> {
        let shared = self.shared.upgrade()?;
        Some(Sender { shared })
    }

    /// Upgrade the channel into a receiver
    pub fn upgrade(self) -> Option<Receiver<C>> {
        let shared = self.shared.upgrade()?;
        let _ = shared.state.lock().parent_request.take();
        Some(Receiver { shared })
    }
}

pub(crate) fn weak_channel<C: Chan>(chan: C, parent: request::Receiver<C>) -> (Sender<C>, WeakChannel<C>) {
    let channel = Arc::new(SharedChannel {
        state: Mutex::new(State {
            parent_request: Some(parent),
            requests: LinkedList::new(),
            waker: None,
        }),
        // 1 receiver for the sender
        resolution: sharedshot::State::new(1),
        chan,
    });
    let weak_channel = WeakChannel { shared: Arc::downgrade(&channel) };
    let sender = Sender { shared: channel };
    (sender, weak_channel)
}

#[cfg(test)]
mod test {

}