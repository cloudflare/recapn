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

use std::borrow::Cow;
use std::fmt::Display;

pub(crate) mod chan;
pub mod client;
pub(crate) mod connection;
#[rustfmt::skip]
pub(crate) mod generated;
pub mod pipeline;
pub mod server;
pub mod table;

pub mod io {
    pub mod stream;
}

pub mod twoparty;

pub use connection::{
    IncomingMessage, Connection, ConnectionOptions, MessageFactory, MessageOutbound,
    OutboundMessage, OwnedIncomingMessage,
};

pub use chan::LocalMessage;
pub use client::Client;

pub(crate) use generated::capnp_rpc_capnp as rpc_capnp;
pub use rpc_capnp::exception::Type as ErrorKind;

#[derive(Clone, Debug)]
pub struct Error {
    kind: ErrorKind,
    description: Cow<'static, str>,
}

impl Error {
    pub fn new(kind: ErrorKind, description: Cow<'static, str>) -> Self {
        Error { kind, description }
    }
    pub fn failed(description: impl Into<Cow<'static, str>>) -> Self {
        Error {
            kind: ErrorKind::Failed,
            description: description.into(),
        }
    }
    pub fn overloaded(description: impl Into<Cow<'static, str>>) -> Self {
        Error {
            kind: ErrorKind::Overloaded,
            description: description.into(),
        }
    }
    pub fn disconnected(description: impl Into<Cow<'static, str>>) -> Self {
        Error {
            kind: ErrorKind::Disconnected,
            description: description.into(),
        }
    }
    pub fn unimplemented(description: impl Into<Cow<'static, str>>) -> Self {
        Error {
            kind: ErrorKind::Unimplemented,
            description: description.into(),
        }
    }

    #[inline]
    pub fn kind(&self) -> ErrorKind {
        self.kind
    }

    #[inline]
    pub fn description(&self) -> &str {
        &self.description
    }
}

impl From<recapn::Error> for Error {
    fn from(value: recapn::Error) -> Self {
        Error::failed(value.to_string())
    }
}

impl Display for ErrorKind {
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        f.write_str(match self {
            Self::Failed => "failed",
            Self::Disconnected => "disconnected",
            Self::Overloaded => "overloaded",
            Self::Unimplemented => "unimplemented",
        })
    }
}

impl Display for Error {
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        self.kind.fmt(f)?;
        f.write_str(": ")?;
        f.write_str(&self.description)
    }
}

pub type Result<T, E = Error> = core::result::Result<T, E>;
