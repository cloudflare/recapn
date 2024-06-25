//! A fast, safe, feature complete Cap'n Proto implementation for modern Rust.

#![cfg_attr(not(feature = "std"), no_std)]
#![feature(const_option)]
#![feature(slice_from_ptr_range)]
#![feature(ptr_sub_ptr)]
#![cfg_attr(feature = "std", feature(write_all_vectored))]

use core::fmt::{self, Display};
use crate::alloc::AllocLen;

#[doc(hidden)]
extern crate self as recapn;

#[cfg(feature = "alloc")]
extern crate alloc as rustalloc;

mod internal {
    pub trait Sealed {}
}

// Modules defined in this part of the lib are for the serialization layers.
// RPC layers may be implemented later on in other modules, or not at all due to feature flags.
// But essentially, RPC is not implemented here (beyond what is essential for handling pointers).

// Layer 0 : Messages and segments

pub mod num;
pub mod alloc;
pub mod arena;
pub mod message;
pub mod io;

// Layer 1 : Typeless primitives (structs, lists, caps)

pub mod ptr;
pub mod orphan;
pub mod data;
pub mod text;
pub mod rpc;

// Layer 2 : Types and abstractions

pub mod ty;
pub mod list;
pub mod any;
pub mod field;

// Layer 3 : Extensions

// pub mod schema;
// pub mod dynamic;
// pub mod compiler;

/// A type that is the reader of a struct type.
pub type ReaderOf<'a, T, Table = rpc::Empty> = <T as ty::StructView>::Reader<'a, Table>;
/// A type that is the builder of a struct type.
pub type BuilderOf<'a, T, Table = rpc::Empty> = <T as ty::StructView>::Builder<'a, Table>;

pub mod prelude {
    pub mod gen {
        pub use recapn::alloc::Word;
        pub use recapn::any::{self, AnyList, AnyPtr, AnyStruct};
        pub use recapn::data::{self, Data};
        pub use recapn::field::{
            self, Accessor, AccessorMut, AccessorOwned, Descriptor, Enum, FieldGroup, Group, Struct,
            UnionViewer, VariantInfo, VariantDescriptor, Variant, VariantMut, VariantOwned,
            ViewOf, Viewable,
        };
        pub use recapn::list::{self, List};
        pub use recapn::ptr::{
            self, StructBuilder, StructReader, StructSize, ListReader, ElementSize, PtrReader
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
#[derive(Debug)]
pub(crate) enum ErrorKind {
    /// The nesting limit has been exceeded, either because the message is too deeply-nested or
    /// it contains cycles. See [message::ReaderOptions].
    NestingLimitExceeded,
    /// A pointer points to a value outside the bounds of the message or segment.
    PointerOutOfBounds,
    /// The read limit has been exceeded.
    ReadLimitExceeded,
    /// A list of inline composite elements overran its word count.
    InlineCompositeOverrun,
    /// An inline composite element tag was a pointer to something other than a struct.
    UnsupportedInlineCompositeElementTag,
    /// We expected to read an pointer of a specific type, but found something else instead.
    UnexpectedRead(ptr::FailedRead),
    /// The existing list value is incompatible with the expected type. This can happen if
    /// we validate a list pointer which has upgraded to an inline composite, but has no data
    /// section when the list element size is one, two, four, or eight bytes, or has no pointer
    /// section when the list element is a pointer.
    IncompatibleUpgrade(ptr::IncompatibleUpgrade),
    /// Capabilities are not allowed in this context, likely because it is canonical or it has an empty cap table.
    CapabilityNotAllowed,
    /// A capability pointer in the message pointed to a capability in the cap table that doesn't
    /// exist.
    InvalidCapabilityPointer(u32),
    /// The message contained text that is not NUL-terminated, or wasn't large enough to contain one.
    TextNotNulTerminated,
    /// When reading a message, we followed a far pointer to another segment, but the segment
    /// didn't exist.
    MissingSegment(u32),
    /// We followed a far pointer to build something, but it pointed to a read-only segment.
    WritingNotAllowed,
    /// An attempt was made to allocate something that cannot be represented in a Cap'n Proto message.
    AllocTooLarge,
    /// An attempt was made to allocate something of the specified size, but the underlying allocator failed.
    AllocFailed(AllocLen),
    /// An attempt was made to adopt an orphan from a different message.
    OrphanFromDifferentMessage,
}

#[derive(Debug)]
pub struct Error {
    kind: ErrorKind,
}

impl Display for Error {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        match &self.kind {
            ErrorKind::NestingLimitExceeded => write!(f, "nesting limit exceeded"),
            ErrorKind::PointerOutOfBounds => write!(f, "pointer out of bounds"),
            ErrorKind::ReadLimitExceeded => write!(f, "read limit exceeded"),
            ErrorKind::InlineCompositeOverrun => write!(f, "inline composite word count overrun"),
            ErrorKind::UnsupportedInlineCompositeElementTag => write!(f, "unsupported inline composite element tag"),
            ErrorKind::UnexpectedRead(read) => Display::fmt(read, f),
            ErrorKind::IncompatibleUpgrade(upgrade) => Display::fmt(upgrade, f),
            ErrorKind::CapabilityNotAllowed => write!(f, "capability not allowed in this context"),
            ErrorKind::InvalidCapabilityPointer(index) => write!(f, "invalid capability pointer ({index})"),
            ErrorKind::TextNotNulTerminated => write!(f, "text wasn't NUL terminated"),
            ErrorKind::MissingSegment(id) => write!(f, "missing segment {id}"),
            ErrorKind::WritingNotAllowed => write!(f, "attempted to write to read-only segment"),
            ErrorKind::AllocTooLarge => write!(f, "allocation too large"),
            ErrorKind::AllocFailed(size) => write!(f, "failed to allocate {size} words in message"),
            ErrorKind::OrphanFromDifferentMessage => write!(f, "orphan from different message"),
        }
    }
}

impl From<ErrorKind> for Error {
    #[cold]
    fn from(kind: ErrorKind) -> Self {
        Error { kind }
    }
}

#[cfg(feature = "std")]
impl std::error::Error for Error {}

pub type Result<T, E = Error> = core::result::Result<T, E>;

/// A type representing a union or enum variant that doesn't exist in the Cap'n Proto schema.
#[derive(Debug)]
pub struct NotInSchema(pub u16);

impl Display for NotInSchema {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        write!(f, "variant {} was not present in the schema", self.0)
    }
}

#[cfg(feature = "std")]
impl std::error::Error for NotInSchema {}