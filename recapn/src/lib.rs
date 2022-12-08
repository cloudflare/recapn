//! A fast, safe, feature complete Cap'n Proto implementation for modern Rust.

#![cfg_attr(not(feature = "std"), no_std)]
#![feature(const_option)]
#![feature(slice_from_ptr_range)]
#![feature(slice_ptr_get)]

use ptr::WirePtr;
use thiserror::Error;

#[doc(hidden)]
extern crate self as recapn;

#[cfg(feature = "std")]
extern crate std as alloc_crate;

#[cfg(not(feature = "std"))]
extern crate alloc as alloc_crate;

mod internal {
    pub trait Sealed {}
}

// Modules defined in this part of the lib are for the serialization layers.
// RPC layers may be defined later on in other modules, or not at all due to feature flags.
// But essentially, RPC is not defined here (beyond what is essential for handling pointers).

// Layer 0 : Messages and segments

/// Types and primitives for the allocation of segments of [Words].
pub mod alloc;
/// Types for the creation of Cap'n Proto messages.
pub mod message;

// Layer 1 : Typeless primitives (structs, lists, caps)

/// Types and primitives for the reading and building of objects in a
/// message (structs, lists, and pointers)
pub mod ptr;
/// Types and primitives for interacting with Cap'n Proto RPC.
///
/// This may look a bit different from other libraries because everything is generic.
/// In order to avoid direct Dispatch Tax and allow for more flexible capability systems to be
/// used (like multi-threaded cap systems), everything is generic so any capability system can
/// be written and dropped in. Sadly this means users may have to explicitly think about what
/// RPC system their code is using, even if no RPC system is used.
pub mod rpc;

// Layer 2 : Types and abstractions
pub mod any;
pub mod list;
pub mod ty;
// pub mod data;
// pub mod text;
pub mod field;

// Layer 3 : Extensions
// pub mod schema;
pub mod compiler;
// pub mod dynamic;

#[doc(hidden)]
pub mod prelude {
    pub mod v1 {
        pub use crate::field::{
            self as f, Accessable, AccessableMut, Accessor, AccessorMut, UnionSlot, Variant,
            VariantMut,
        };
        pub use crate::{list, Family, Result};
        pub mod t {
            pub use crate::ptr::{
                Bool, Int16, Int32, Int64, Int8, UInt16, UInt32, UInt64, UInt8, Void,
            };
            pub use crate::ty::{Enum, Struct};
        }
        pub use crate::rpc;
    }
}

/// A type that is the reader of a struct type.
pub type ReaderOf<'a, T, Table = rpc::Empty> = <T as ty::Struct>::Reader<'a, Table>;
/// A type that is the builder of a struct type.
pub type BuilderOf<'a, T, Table = rpc::Empty> = <T as ty::Struct>::Builder<'a, Table>;

/*
pub use any::Any;
pub use data::Data;
pub use list::List;
pub use text::Text;

/// Includes a file as a reference to a Word array.
///
/// The file will be padded with 0-bytes if its length in bytes is not divisible by 8.
///
/// The file is located relative to the current file (similarly to how modules are found).
/// The provided path is interpreted in a platform-specific way at compile time. So, for instance,
/// an invocation with a Windows path containing backslashes \ would not compile correctly on Unix.
///
/// This macro will yield an expression of type &'static [Word; N] which is the contents of the file.
#[macro_export]
macro_rules! include_words {
    ($file:expr $(,)?) => {{
        const FILE: &[u8] = include_bytes!($file);
        const FILE_LEN_WORDS: usize = Word::round_up_byte_count(FILE.len());
        const fn make_aligned_words_of_file() -> [Word; FILE_LEN_WORDS] {
            let file_aligned = [Word::NULL; FILE_LEN_WORDS];
            unsafe {
                core::ptr::copy_nonoverlapping(
                    FILE.as_ptr(),
                    file_aligned.as_ptr() as *mut u8, // LMAO
                    FILE.len(),
                )
            };
            file_aligned
        }
        &make_aligned_words_of_file()
    }};
}
*/

/// A marker type used in place of a concrete generic implementation for a message's representation.
#[derive(Clone, Copy, Debug, Default, PartialEq, Eq)]
pub struct Family;

/// A trait used to get the family type of a structure from a concrete implementation's type.
pub trait IntoFamily {
    /// The family type for this type.
    type Family;
}

/// Errors that can occur while reading and writing in Cap'n Proto serialization format.
#[non_exhaustive]
#[derive(Debug, Error)]
pub(crate) enum ErrorKind {
    /// The nesting limit has been exceeded, either because the message is too deeply-nested or
    /// it contains cycles. See [message::ReaderOptions].
    #[error("nesting limit exceeded")]
    NestingLimitExceeded,
    /// A pointer points to a value outside the bounds of the message or segment.
    #[error("pointer out of bounds")]
    PointerOutOfBounds,
    /// The read limit has been exceeded.
    #[error("read limit exceeded")]
    ReadLimitExceeded,
    /// A list of inline composite elements overran its word count.
    #[error("inline composite word count overrun")]
    InlineCompositeOverrun,
    /// An inline composite element tag was a pointer to something other than a struct.
    #[error("unsupported inline composite element tag")]
    UnsupportedInlineCompositeElementTag,
    /// We expected to read an pointer of a specific type, but found something else instead.
    #[error("{0}")]
    UnexpectedRead(ptr::FailedRead),
    /// The existing list value is incompatible with the expected type. This can happen if
    /// we validate a list pointer which has upgraded to an inline composite, but has no data
    /// section when the list element size is one, two, four, or eight bytes, or has no pointer
    /// section when the list element is a pointer.
    #[error("incompatible list upgrade")]
    IncompatibleUpgrade,
    /// Capabilities are not allowed in this context, likely because it is canonical or it has an empty cap table.
    #[error("capability not allowed in this context")]
    CapabilityNotAllowed,
    /// A capability pointer in the message pointed to a capability in the cap table that doesn't
    /// exist.
    #[error("invalid capability pointer ({0})")]
    InvalidCapabilityPointer(u32),
    /// The message contained text that is not NUL-terminated, or wasn't large enough to contain one.
    #[error("text wasn't NUL terminated")]
    TextNotNulTerminated,
    /// We can't copy a struct from a struct reader because one of its sections is pointing to one
    /// of our sections and the other one isn't.
    #[error("invalid struct overlap")]
    InvalidStructOverlap,
    /// When reading a message, we followed a far pointer to another segment, but the segment
    /// didn't exist.
    #[error("missing segment {0}")]
    MissingSegment(u32),
    /// We followed a far pointer to build something, but it pointed to a read-only segment.
    #[error("read-only segment")]
    WritingNotAllowed,
    /// An attempt was made to allocate something that cannot be represented in a Cap'n Proto message.
    #[error("allocation too large")]
    AllocTooLarge,
}

#[derive(Debug, Error)]
#[error(transparent)]
pub struct Error {
    kind: ErrorKind,
}

impl Error {
    #[cold]
    pub(crate) fn fail_read(expected: Option<ptr::ExpectedRead>, actual: WirePtr) -> Self {
        Self {
            kind: ErrorKind::UnexpectedRead(ptr::FailedRead {
                expected,
                actual: {
                    if actual.is_null() {
                        ptr::ActualRead::Null
                    } else {
                        use ptr::WireKind::*;
                        match actual.kind() {
                            Struct => ptr::ActualRead::Struct,
                            Far => ptr::ActualRead::Far,
                            Other => ptr::ActualRead::Other,
                            List => {
                                ptr::ActualRead::List(actual.list_ptr().unwrap().element_size())
                            }
                        }
                    }
                },
            }),
        }
    }
}

impl From<ErrorKind> for Error {
    #[cold]
    fn from(kind: ErrorKind) -> Self {
        Error { kind }
    }
}

pub type Result<T, E = Error> = core::result::Result<T, E>;
