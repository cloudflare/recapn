use tokio::sync::mpsc;

pub use mpsc::unbounded_channel;

pub type UnboundedSender<T> = mpsc::UnboundedSender<T>;
pub type UnboundedReceiver<T> = mpsc::UnboundedReceiver<T>;

pub use tokio::sync::oneshot;

pub use tokio::select;

pub use tokio::task::JoinHandle;

pub use tokio::task::spawn;
pub use tokio::task::spawn_local;

impl<T: Send> crate::rt::Sender<T> for mpsc::Sender<T> {
    async fn send(&self, value: T) -> Result<(), crate::rt::SendError<T>> {
        self.send(value).await.map_err(|err| err.into())
    }
}

impl<T: Send> crate::rt::Receiver<T> for mpsc::Receiver<T> {
    async fn recv(&mut self) -> Option<T> {
        self.recv().await
    }
}

struct Rt;

impl crate::rt::Rt for Rt {
    type Sender<T: Send> = mpsc::Sender<T>;
    type Receiver<T: Send> = mpsc::Receiver<T>;

    fn channel<T: Send>(buffer: usize) -> (Self::Sender<T>, Self::Receiver<T>) {
        mpsc::channel(buffer)
    }
}

impl<T: Send> From<mpsc::error::SendError<T>> for crate::rt::SendError<T> {
    fn from(value: mpsc::error::SendError<T>) -> Self {
        Self(value.0)
    }
}
