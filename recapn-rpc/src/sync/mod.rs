pub mod sharedshot;
pub mod request;
pub mod mpsc;
mod util;

pub enum TryRecvError {
    Empty,
    Closed,
}
