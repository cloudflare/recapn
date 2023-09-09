//! A fast, safe, feature complete Cap'n Proto implementation for modern Rust.

#![cfg_attr(not(feature = "std"), no_std)]
#![feature(const_option)]
#![feature(slice_from_ptr_range)]

use core::fmt::{self, Display};
use ptr::{PtrElementSize, WirePtr};
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
// RPC layers may be implemented later on in other modules, or not at all due to feature flags.
// But essentially, RPC is not implemented here (beyond what is essential for handling pointers).

// Layer 0 : Messages and segments

pub mod alloc;
pub mod message;
pub mod io;

// Layer 1 : Typeless primitives (structs, lists, caps)

pub mod ptr;
pub mod data;
pub mod text;
pub mod rpc;

// Layer 2 : Types and abstractions

pub mod ty;
pub mod list;
pub mod any;
pub mod field;

// Layer 3 : Extensions

// pub mod orphan;
// pub mod schema;
// pub mod compiler;
// pub mod dynamic;

/// A type that is the reader of a struct type.
pub type ReaderOf<'a, T, Table = rpc::Empty> = <T as ty::StructView>::Reader<'a, Table>;
/// A type that is the builder of a struct type.
pub type BuilderOf<'a, T, Table = rpc::Empty> = <T as ty::StructView>::Builder<'a, Table>;

pub mod prelude {
    pub mod gen {
        pub use recapn::any::{self, AnyList, AnyPtr, AnyStruct};
        pub use recapn::data::{self, Data};
        pub use recapn::field::{
            self, Accessor, AccessorMut, Descriptor, Enum, FieldGroup, Group, Struct,
            UnionViewer, Variant, VariantInfo, VariantDescriptor, VariantMut, ViewOf, Viewable,
        };
        pub use recapn::list::{self, List};
        pub use recapn::ptr::{
            self, StructBuilder, StructReader, StructSize
        };
        pub use recapn::rpc::{self, Capable, Table};
        pub use recapn::text::{self, Text};
        pub use recapn::ty::{self, StructView};
        pub use recapn::{BuilderOf, Family, IntoFamily, ReaderOf, Result, NotInSchema};
    }
}

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
    #[error("{0}")]
    IncompatibleUpgrade(ptr::IncompatibleUpgrade),
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
                            List => ptr::ActualRead::List,
                        }
                    }
                },
            }),
        }
    }

    #[inline]
    pub(crate) fn fail_upgrade(from: PtrElementSize, to: PtrElementSize) -> Self {
        Self {
            kind: ErrorKind::IncompatibleUpgrade(ptr::IncompatibleUpgrade { from, to }),
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

/// A type representing a union or enum variant that doesn't exist in the Cap'n Proto schema.
#[derive(Error, Debug)]
pub struct NotInSchema(pub u16);

impl Display for NotInSchema {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        write!(f, "variant {} was not present in the schema", self.0)
    }
}
