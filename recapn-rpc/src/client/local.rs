//! Implements the local server side of RPC which consists of:
//! * Server dispatchers
//! * Request queueing
//! * Response handling

use std::mem::MaybeUninit;
use std::convert;
use std::future::Future;
use std::marker::{PhantomData, PhantomPinned};
use std::pin::{pin, Pin};
use tokio::task::JoinHandle;

use crate::{Result, Error};

use super::queue::PipelineResolver;
use super::{Client, Pipeline, RpcCall, RequestSender, RequestReceiver, RpcResponder, Params, RpcRequest};

#[derive(Clone)]
pub(super) struct LocalClient {
    pub sender: RequestSender,
}

/// The receiver side of a server that executes requests
pub struct Dispatcher<T: ?Sized> {
    broken: Option<Error>,
    receiver: RequestReceiver,
    inner: T,
}

impl<T: Dispatch + ?Sized> Dispatcher<T> {
    /// Run the dispatcher with the given executor and handle all
    /// requests until all connections to the server have been severed.
    /// 
    /// # Panics
    /// 
    /// This must be called within the context of a LocalSet. Calling this outside a LocalSet
    /// will result in a panic.
    pub async fn run(&mut self) -> Result<()> {
        while self.run_once().await? {}

        Ok(())
    }

    /// Handles exactly one message to the server. Returns a bool indicating whether a message
    /// was handled. If the server is broken or breaks while handling a blocking message, this
    /// returns an error.
    /// 
    /// # Panics
    /// 
    /// This must be called within the context of a LocalSet. Calling this outside a LocalSet
    /// will result in a panic.
    /// 
    /// # Drop safety
    /// 
    /// It is safe to drop the future returned by this function, but if the dispatcher is handling
    /// a blocking request, that request might be canceled prematurely. If this occurs, the
    /// dispatcher is automatically broken.
    pub async fn run_once(&mut self) -> Result<bool> {
        if let Some(err) = &self.broken {
            // The server is broken, return an error
            return Err(err.clone())
        }

        let Some(req) = self.receiver.recv().await else { return Ok(false) };
        let (request, responder) = req.split();
        let RpcCall { interface, method, params, client: _ } = request;

        let request = DispatchRequest { interface, method, params, responder, pipeline: None };
        let DispatchResponse(result) = self.inner.dispatch(request);
        match result {
            DispatchResult::Async => {} // do nothing, it has already been handled
            DispatchResult::Streaming(task) => {
                // Set up a guard so that the dispatcher becomes broken if we're dropped while
                // handling a blocking request.
                struct Bomb<'a>(Option<&'a mut Option<Error>>);
                impl Bomb<'_> {
                    fn defuse(mut self) {
                        self.0 = None;
                    }
                }
                impl Drop for Bomb<'_> {
                    fn drop(&mut self) {
                        if let Some(o) = self.0.take() {
                            *o = Some(Error::failed(
                                "a future handling a blocking request was dropped"
                            ));
                        }
                    }
                }
                let bomb = Bomb(Some(&mut self.broken));
                let result = task.await;
                bomb.defuse(); // counter-terrorists win
    
                let flattened = result
                    .map_err(|join_err| Error::failed(join_err.to_string()))
                    .and_then(convert::identity);
    
                if let Err(err) = flattened {
                    self.broken = Some(err.clone());
                    return Err(err)
                }
            }
        }

        Ok(true)
    }

    /// Returns a reference to the error indicating why this dispatcher is broken.
    /// 
    /// If an error is returned while handling a blocking request, the dispatcher becomes
    /// broken. In this case, this function returns the error and all future requests
    /// return the same error.
    pub fn broken_err(&self) -> Option<&Error> {
        self.broken.as_ref()
    }

    /// Gets a reference to the underlying server dispatcher
    pub fn get_ref(&self) -> &T {
        &self.inner
    }
}

/// Dispatches requests to given interface methods by their IDs.
/// 
/// In generated code, a server dispatcher struct and server trait are generated for each
/// interface. Dispatcher structs wrap a type that implement their server trait and implement
/// the `Dispatch` trait to route requests by their interface and method ID.
/// 
/// # Example
/// 
/// Given this capnp interface
/// ```capnp
/// interface Foo {
///   bar @0 (baz :Text);
/// }
/// ```
/// 
/// A struct and trait could be generated like so
/// ```
/// // Server
/// pub trait Foo {
///     type BarFuture: Future<Output = Result<()>> + 'static;
///     fn bar(&self, BarContext ctx) -> Self::BarFuture;
/// }
/// 
/// pub struct FooDispatcher<T>(T);
/// impl<T> FooDispatcher<T>
/// where
///     T: Foo,
/// {
///     pub const INTERFACE_NAME: &str = "foobar.capnp:Foo";
/// 
///     #[doc(hidden)]
///     pub fn dispatch_method<E>(this: &T, request: Request<E>) -> DispatchResponse
///     where
///         E: Executor
///     {
///         match request.method() {
///             0 => request.respond_with(|ctx| this.0.bar(ctx)),
///             _ => request.unimplemented_method(Self::INTERFACE_NAME),
///         }
///     }
/// }
/// 
/// impl<T, E> Dispatch<E> for FooDispatcher<T>
/// where
///     T: Foo,
///     E: Executor,
/// {
///     fn dispatch(&self, request: Request<E>) -> DispatchResult {
///         match request.interface() {
///             0 => FooDispatcher::dispatch_method(self, request),
///             _ => request.unimplemented_interface(Self::INTERFACE_NAME)
///         }
///     }
/// }
pub trait Dispatch {
    fn dispatch(&mut self, request: DispatchRequest) -> DispatchResponse;
}

/// A helper type for constructing a `Parameters<'a, P>` in-place, pinned, on the stack.
/// 
/// # Example
/// 
/// ```text
/// let mut in_place = ParametersInPlace::new(params);
/// let mut pinned = pin!(in_place);
/// 
/// // pass to call via pinned.get()
/// 
/// pinned.drop();
/// ```
struct ParametersInPlace {
    params: Option<Params>,

    _pinned: PhantomPinned,
}

impl ParametersInPlace {
    pub fn new(params: Params) -> Self {
        todo!()
    }

    pub fn get<'a, P>(self: Pin<&'a mut Self>) -> Parameters<'a, P> {
        todo!()
    }

    unsafe fn root(self: Pin<&Self>) -> recapn::any::PtrReader<'_> {
        todo!()
    }

    /// Drop the parameters. If the parameters have already been dropped, this does nothing.
    pub fn drop(self: Pin<&mut Self>) {
        todo!()
    }
}

/// The params payload.
/// 
/// This contains methods to read the parameters of the call. Dropping this releases the
/// underlying params message and may allow the RPC system to free up buffer space to handle
/// other requests. Long-running asynchronous methods should try to drop this as early as is
/// convenient.
pub struct Parameters<'a, P> {
    inner: Pin<&'a mut ParametersInPlace>,
    p: PhantomData<fn() -> P>,
}

impl<'a, P> Parameters<'a, P> {
    pub fn get(&self) -> P {
        let _ptr =  unsafe { self.inner.as_ref().root() };
        todo!()
    }
}

impl<'a, P> Drop for Parameters<'a, P> {
    fn drop(&mut self) {
        self.inner.as_mut().drop()
    }
}

/// The results payload and the response pipeline.
/// 
/// This contains methods to manipulate the results payload, the response pipeline,
/// or perform a tail call. Dropping this causes an empty response to be sent.
/// 
/// There are three primary ways to manipulate the response:
/// 
/// - `respond()`
/// - `tail_call()`
/// - `pipeline_and_results()`
/// 
/// `respond()` returns an instance of `Results` for manipulating the results payload.
/// 
/// `tail_call()` accepts a new Request that should be sent and have the response sent
/// in place for this call. With a tail call, the RPC implementation may be able to optimize
/// the tail call to another machine such that the results never actually pass through this
/// machine. Even if no such optimization is possible, `tail_call()` may allow pipelined calls
/// to be forwarded optimistically to the new call site.
/// 
/// `pipeline_and_results()` allows configuring the response pipeline and results separately.
/// This is analogous to `setPipeline` in C++, allowing you to tell the RPC system where the
/// capabilities in the response will resolve to, before making the response, allowing requests
/// that are promise-pipelined on this call's results to continue their journey to the final
/// destination before this call itself has completed.
pub struct Response<'a, R> {
    r: PhantomData<(&'a (), R)>,
}

impl<'a, R> Response<'a, R> {
    fn new(responder: RpcResponder, pipeline: Option<PipelineResolver>) -> Self {
        todo!()
    }
}

#[non_exhaustive]
pub struct CallContext<'a, P, R> {
    pub params: Parameters<'a, P>,
    pub response: Response<'a, R>,
}

pub struct DispatchRequest {
    interface: u64,
    method: u16,
    params: Params,
    responder: RpcResponder,
    pipeline: Option<PipelineResolver>,
}

impl DispatchRequest {
    #[inline]
    pub fn interface(&self) -> u64 { self.interface }

    #[inline]
    pub fn method(&self) -> u16 { self.method }

    pub fn respond_with<P, R, Res>(self, responder: Res) -> DispatchResponse
    where
        Res: for<'a> Responder<'a, P, R>,
    {
        let _ = self.spawn_handler(responder);
        DispatchResponse(DispatchResult::Async)
    }

    pub fn respond_blocking_with<P, R, Res>(self, responder: Res) -> DispatchResponse
    where
        Res: for<'a> Responder<'a, P, R>,
    {
        let handler = self.spawn_handler(responder);
        DispatchResponse(DispatchResult::Streaming(handler))
    }

    fn spawn_handler<P, R, Res>(self, responder: Res) -> JoinHandle<Result<()>>
    where
        Res: for<'a> Responder<'a, P, R>,
    {
        let DispatchRequest {
            params,
            responder: call_responder,
            pipeline,
            ..
        } = self;
        let future = async move {
            // First, let's set up our parameters. In order to prevent allocations, we're going
            // to try and allocate as much as we can in-place here in this future.
            let in_place = ParametersInPlace::new(params);
            let mut params_pinned = pin!(in_place);
            let parameters = params_pinned.as_mut().get();

            // Now let's set up our response type.
            let pipeline = pipeline;
            let response = Response::new(call_responder, pipeline);

            let context = CallContext {
                params: parameters,
                response,
            };

            let result = responder.respond(context).await;

            match result {
                Ok(()) => todo!("send off response"),
                Err(err) => todo!("return error for response"),
            }

            params_pinned.drop();

            result
        };

        tokio::task::spawn_local(future)
    }

    /// Respond with the given error.
    #[inline]
    pub fn error(self, err: Error) -> DispatchResponse {
        if let Some(pipeline) = self.pipeline {
            todo!()
        }

        let _ = self.responder.send(Err(err));
        DispatchResponse(DispatchResult::Async)
    }

    /// Respond with a generic error indicating that the interface is not implemented
    /// by the dispatcher.
    #[inline]
    pub fn unimplemented_interface(self, interface_name: &str) -> DispatchResponse {
        let err = Error::unimplemented(format!(
            "Requested interface (@{:0<#16x}) not implemented for {}",
            self.interface, interface_name,
        ));
        self.error(err)
    }

    /// Respond with a generic error indicating that the method is not implemented
    /// by the dispatcher.
    #[inline]
    pub fn unimplemented_method(self, interface_name: &str) -> DispatchResponse {
        let err = Error::unimplemented(format!(
            "Method (@{}) not implemented for interface \"{}\" (@{:0<#16x})",
            self.method, interface_name, self.interface,
        ));
        self.error(err)
    }
}

enum DispatchResult {
    /// An async task was spawned to handle the request
    Async,
    /// The request is streaming, so the task must be waited to completion before
    /// handling the next request
    Streaming(JoinHandle<Result<()>>),
}

pub struct DispatchResponse(DispatchResult);

pub trait Responder<'a, P, R>: 'static {
    type Future: Future<Output = Result<()>> + 'a;

    fn respond(self, ctx: CallContext<'a, P, R>) -> Self::Future;
}

impl<'a, P, R, F, Fut> Responder<'a, P, R> for F
where
    F: FnOnce(CallContext<'a, P, R>) -> Fut + 'static,
    Fut: Future<Output = Result<()>> + 'a,
{
    type Future = Fut;

    #[inline]
    fn respond(self, ctx: CallContext<'a, P, R>) -> Self::Future {
        (self)(ctx)
    }
}