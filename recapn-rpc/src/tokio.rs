use tokio::sync::mpsc;

pub use mpsc::channel;
pub use mpsc::unbounded_channel;

pub type UnboundedSender<T> = mpsc::UnboundedSender<T>;
pub type UnboundedReceiver<T> = mpsc::UnboundedReceiver<T>;

pub type Sender<T> = mpsc::Sender<T>;
pub type Receiver<T> = mpsc::Receiver<T>;

pub use tokio::sync::oneshot;

pub use tokio::select;

pub use tokio::task::JoinHandle;

pub use tokio::task::spawn;
pub use tokio::task::spawn_local;
