pub trait Rt {
    type Sender<T: Send>: Sender<T>;
    type Receiver<T: Send>: Receiver<T>;

    fn channel<T: Send>(buffer: usize) -> (Self::Sender<T>, Self::Receiver<T>);
}

pub trait Sender<T: Send>: Clone + Send {
    fn send(&self, value: T) -> impl std::future::Future<Output = Result<(), SendError<T>>> + Send;
}

/// Error returned by the `Sender`.
#[derive(PartialEq, Eq, Clone, Copy)]
pub struct SendError<T: Send>(pub T);

impl<T: Send> core::fmt::Debug for SendError<T> {
    fn fmt(&self, f: &mut core::fmt::Formatter<'_>) -> core::fmt::Result {
        f.debug_struct("SendError").finish_non_exhaustive()
    }
}

impl<T: Send> core::fmt::Display for SendError<T> {
    fn fmt(&self, fmt: &mut core::fmt::Formatter<'_>) -> core::fmt::Result {
        write!(fmt, "channel closed")
    }
}

impl<T: Send> core::error::Error for SendError<T> {}

pub trait Receiver<T: Send> {
    async fn recv(&mut self) -> Option<T>;
}
