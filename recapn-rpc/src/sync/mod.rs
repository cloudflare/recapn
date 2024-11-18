pub mod mpsc;
pub mod request;
mod util;

#[cfg(test)]
mod test;

#[derive(PartialEq, Eq, Clone, Copy, Debug)]
pub enum TryRecvError {
    Empty,
    Closed,
}
