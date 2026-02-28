//! # recapn-channel
//! 
//! This crate contains async primitives used by recapn-rpc. These primitives are customized
//! specially to the needs of recapn-rpc and as such are likely not going to be that useful outside
//! of that context (like pipelines).
//! 
//! # Channels
//! 
//! The main use of this crate is the custom mpsc channel type. This mpsc channel has lots of
//! special behaviors used by recapn-rpc. Some of these include:
//! 
//! * Channel resolution: mpsc channels can be resolved into other channels, causing all requests
//!   sent to the channel to be forwarded to another at low cost.
//! * Channel termination: Channels can resolve into errors, causing future requests to be
//!   immediately returned with an error.
//! * Channel mux: Channels can be multiplexed together so you can receive requests from many
//!   channels at once.
//! 
//! Channels are not generalized to sending any kind of data. Instead, channels send special
//! requests and events which are also part of this crate.
//! 
//! # Requests
//! 
//! Requests encapsulate the request, response, and pipeline of a recapn-rpc request. You can
//! imagine a request as some parameters, paired with a oneshot for a shared response, and you
//! can setup pipelines based on the data.
//! 
//! A request can be passed through a channel. Requests can be received from a channel and then
//! passed to other channels without responding to them. When you want to respond to a request,
//! you're given a `Responder` along with the original `Parameters` associated with the request.
//! `Parameters` can then be passed into a `Responder` to create a new request as a `tail_call`.
//! A response can be sent to all receivers by passing `Results` to `respond`. 
//! 
//! ## Pipelines
//! 
//! A promise pipeline in Cap'n Proto is a capability which has been derived from a set of
//! operations to be performed on the response of a given request. When the response promise
//! resolves, the operations are performed to get the real capability and the promised capability
//! resolves to the real one.
//! 
//! We generalize this so that every request has all the necessary state required to support
//! pipelines using a hash map. Custom pipeline handlers can be set through `set_pipeline`.

use std::hash::Hash;

pub mod mpsc;
pub mod request;
pub mod task;
mod util;

#[cfg(test)]
mod test;

pub use mpsc::channel;
pub use request::{request_pipeline, request_response, request_response_pipeline};

/// An abstract channel. This is associated data stored in a mpsc channel that
/// also references associated types with the request-response system.
pub trait Chan: Sized {
    /// Event data that can be sent through a channel. This allows you to pass arbitrary data
    /// through a channel on the request path without it behaving like a request.
    type Event;

    /// The parameters passed into the request.
    type Parameters;

    /// A key used to distinguish between different pipelines.
    type PipelineKey: Eq + Hash;

    /// An error that can be used to terminate channels.
    type Error: Clone + IntoResults<Self>;

    /// The results associated with this parameter type.
    type Results: PipelineResolver<Self>;

    /// A pipeline that can be configured separately via `set_pipeline`.
    type Pipeline: PipelineResolver<Self>;
}

/// Defines a type which can resolve or return an existing pipeline channel.
pub trait PipelineResolver<C: Chan> {
    /// Resolves the pipeline channel with the given key.
    fn resolve(
        &self,
        recv: request::ResponseReceiver<C>,
        key: C::PipelineKey,
        channel: mpsc::Receiver<C>,
    );

    /// Returns the pipeline channel with the given key. If the key doesn't match,
    /// this may return an already broken channel.
    fn pipeline(
        &self,
        recv: request::ResponseReceiverFactory<'_, C>,
        key: C::PipelineKey,
    ) -> mpsc::Sender<C>;
}

/// Describes a conversion from a type into "results".
pub trait IntoResults<C: Chan> {
    /// Convert this value into channel results.
    fn into_results(self) -> C::Results;
}
