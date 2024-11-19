use std::hash::Hash;

pub mod mpsc;
pub mod request;
mod util;

#[cfg(test)]
mod test;

pub use mpsc::channel;
pub use request::{request_response, request_pipeline, request_response_pipeline};

/// An abstract channel. This is associated data stored in a mpsc channel that
/// also references associated types with the request-response system.
pub trait Chan: Sized {
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

pub trait PipelineResolver<C: Chan> {
    /// Resolves the pipeline channel with the given key.
    fn resolve(
        &self,
        recv: request::ResponseReceiverFactory<'_, C>,
        key: C::PipelineKey,
        channel: mpsc::Receiver<C>,
    );

    /// Returns the pipeline channel with the given key. If the key doesn't match,
    /// this may return an already broken channel.
    fn pipeline(&self, recv: request::ResponseReceiverFactory<'_, C>, key: C::PipelineKey) -> mpsc::Sender<C>;
}

/// Describes a conversion from a type into "results".
pub trait IntoResults<C: Chan> {
    fn into_results(self) -> C::Results;
}
