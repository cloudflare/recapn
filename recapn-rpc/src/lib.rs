//! A multi-threaded Cap'n Proto RPC library.
//! 
//! This implements Cap'n Proto RPC with support for multi-threaded systems. Where other native
//! implementations of RPC are single-threaded, this implementation has been specifically designed
//! for splitting apart systems and running on multiple threads.
//! 
//! There are two main components of RPC: Local servers and connection handlers. These expose
//! handles for communicating with a server or capability in an opaque way using Cap'n Proto
//! messages. In this library, local servers and connections are primitives you can use to build
//! your own system with whatever threading system you want. You can put everything on a single
//! thread, put your connection handling on one or more threads, put local servers on one thread,
//! or any number of threads, or any combination of the above.

#![feature(hash_raw_entry)]

use client::Client;

pub mod gen;
pub mod table;
pub mod sync;
pub mod client;
pub mod connection;
//mod system;
//pub mod twoparty;

#[derive(Clone, Debug)]
pub enum ErrorKind {
    Failed,
    Overloaded,
    Disconnected,
    Unimplemented,
}

#[derive(Clone, Debug)]
pub struct Error {
    kind: ErrorKind,
    description: Box<str>,
}

impl Error {
    pub fn failed(description: impl Into<Box<str>>) -> Self {
        Error { kind: ErrorKind::Failed, description: description.into() }
    }
    pub fn overloaded(description: impl Into<Box<str>>) -> Self {
        Error { kind: ErrorKind::Overloaded, description: description.into() }
    }
    pub fn disconnected(description: impl Into<Box<str>>) -> Self {
        Error { kind: ErrorKind::Disconnected, description: description.into() }
    }
    pub fn unimplemented(description: impl Into<Box<str>>) -> Self {
        Error { kind: ErrorKind::Unimplemented, description: description.into() }
    }
}

pub type Result<T, E = Error> = core::result::Result<T, E>;

#[derive(Clone, PartialEq, Eq, Hash)]
pub enum PipelineOp {
    // We don't include PipelineOp::None because it's pointless and a waste of space.
    PtrField(u16),
}

pub struct ClientOptions {
    #[cfg(any(unix, target_os = "wasi"))]
    pub fd: Option<std::os::fd::RawFd>,
}