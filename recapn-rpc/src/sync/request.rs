//! A request-response oneshot. This has the unique property that the request data and
//! response data are both part of the same allocation. This way you don't need to allocate
//! and track the request data separately.

use std::cell::UnsafeCell;
use std::future::{Future, poll_fn};
use std::marker::PhantomData;
use std::mem::{ManuallyDrop, MaybeUninit};
use std::ops::Deref;
use std::pin::Pin;
use std::ptr::NonNull;
use std::sync::atomic::{AtomicPtr, AtomicUsize};
use std::sync::Arc;
use std::task;

use super::sharedshot;
use super::util::linked_list::Pointers;
use crate::Error;
use crate::sync::util::Marc;

/// Create a new request and response channel.
pub fn channel<T, U>(req: T) -> (Request<T, U>, ResponseReceiver<T, U>) {
    todo!()
}

/// 
pub struct Request<T, U> {
    shared: Marc<SharedRequest<T, U>>,
}

impl<T, U> Request<T, U> {
    /// Get the inner request value.
    pub fn get(&self) -> &T {
        self.shared.get().request()
    }

    pub fn respond(self, value: U) -> Result<(), U> {
        todo!()
    }

    /// Split the request into an ActiveRequest and a Responder.
    pub fn split(self) -> (T, Responder<T, U>) {
        todo!()
    }
}

impl<T, U> Drop for Request<T, U> {
    fn drop(&mut self) {
        if let Some(shared) = self.shared.try_take() {
            unsafe { shared.drop_request() }
        }
    }
}

/// Allows sending a response for a request.
pub struct Responder<T, U> {
    shared: Arc<SharedRequest<T, U>>,
}

impl<T, U> Responder<T, U> {
    pub fn send(self, resp: U) -> Result<(), U> {
        todo!()
    }

    pub async fn closed(&mut self) {
        let closed = poll_fn(|ctx| self.poll_closed(ctx));
        closed.await
    }

    pub fn is_closed(&self) -> bool {
        todo!()
    }

    pub fn poll_closed(&mut self, ctx: &mut task::Context) -> task::Poll<()> {
        todo!()
    }
}

pub struct ResponseReceiver<T, U> {
    shared: Option<Arc<SharedRequest<T, U>>>,
}

unsafe impl<T: Send, U: Send> Send for ResponseReceiver<T, U> {}
unsafe impl<T: Sync, U: Sync> Sync for ResponseReceiver<T, U> {}

impl<T, U> Future for ResponseReceiver<T, U> {
    type Output = Option<U>;

    fn poll(self: Pin<&mut Self>, cx: &mut task::Context<'_>) -> task::Poll<Self::Output> {
        todo!()
    }
}

pub struct Response<T, U> {
    shared: Arc<SharedRequest<T, U>>,
}

unsafe impl<T: Send, U: Send> Send for Response<T, U> {}
unsafe impl<T: Sync, U: Sync> Sync for Response<T, U> {}

impl<T, U> Clone for Response<T, U> {
    fn clone(&self) -> Self {
        Self { shared: self.shared.clone() }
    }
}

/// The shared state behind a request
pub(crate) struct SharedRequest<T, U> {
    /// Pointers to requests in a mpsc linked list.
    pointers: Pointers<SharedRequest<T, U>>,

    /// The original mpsc linked list that owned this request. Note, this may not be the
    /// current owner of the list, as the channel may have been forwarded, in which case
    /// the channel will have a chain of pointers pointing at the true owner of the linked
    /// list.
    owner: Option<Arc<()>>,

    /// The request data. This should only be accessed by Request. When Request drop they
    /// will trigger a drop on these contents.
    request: UnsafeCell<ManuallyDrop<T>>,

    /// The response sharedshot.
    response: sharedshot::State<U>,
}

impl<T, U> SharedRequest<T, U> {
    pub fn new(request: T) -> Self {
        Self {
            pointers: Pointers::new(),
            owner: None,
            request: UnsafeCell::new(ManuallyDrop::new(request)),
            response: sharedshot::State::new(1),
        }
    }

    pub fn request(&self) -> &T {
        // SAFETY: Request data can be accessed through a shared reference by anyone
        unsafe { &**self.request.get() }
    }

    /// Drop the request value inside the shared request
    /// 
    /// SAFETY: This may only be called once and is primarily used by the Drop implementations of
    /// Request and ActiveRequest
    pub unsafe fn drop_request(&self) {
        let request = &mut *self.request.get();
        ManuallyDrop::drop(request)
    }
}

#[cfg(test)]
mod test {
    use super::channel;

    #[tokio::test]
    async fn request_response() {
        // Requests and responses are a core primitive to RPC. Since we make a lot of them, it
        // doesn't make sense to do many allocations when we always are going to have both at once.
        // Request combines the request and response data into a single allocation and exposes an
        // sharedshot API to allow for the receipt of the response.

        // Requests are created similar to a oneshot with the request function which returns a
        // new request and response receiver.
        let (request, receiver) = channel(1);

        // Requests can be split into an active request and a responder to allow dropping the
        // request data separately from the responder.
        let (request, responder) = request.split();
        assert_eq!(request, 1);

        // Responder can return the response back to the sender if nobody is listening for the response.
        responder.send(2).unwrap();

        // Receivers can await the response.
        let response = receiver.await.unwrap();
        assert_eq!(response, 2);

        // When all handles to the request are dropped, the whole allocation is released.
    }
}