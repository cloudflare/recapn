pub mod sharedshot;
pub mod request;
pub mod mpsc;
mod util;

#[derive(PartialEq, Eq, Clone, Copy, Debug)]
pub enum TryRecvError {
    Empty,
    Closed,
}
