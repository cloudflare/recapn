pub mod mpsc;
pub mod request;
pub mod sharedshot;
mod util;

#[derive(PartialEq, Eq, Clone, Copy, Debug)]
pub enum TryRecvError {
    Empty,
    Closed,
}
