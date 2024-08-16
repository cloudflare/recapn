//! Serialization integration with Cap'n Proto RPC.
//!
//! This may look a bit different from other libraries because everything is generic.
//! In order to avoid direct Dispatch Tax and allow for more flexible capability systems to be
//! used (like multi-threaded cap systems), everything is generic so any capability system can
//! be written and dropped in. Sadly this means users may have to explicitly think about what
//! RPC system their code is using, even if no RPC system is used.

use core::fmt::Display;

pub(crate) mod internal {
    use super::{
        CapSystem, CapTable, CapTableBuilder, CapTableReader, CapTranslator, Empty, Table,
    };
    use crate::{Error, Result};
    use core::convert::Infallible;

    pub trait CapPtrReader: Clone {
        type Table: Table;
        type Cap;
    }

    pub trait CapPtrBuilder: CapPtrReader {
        fn clear_cap(&self, index: u32) -> Result<()>;
        fn as_reader(&self) -> <Self::Table as Table>::Reader;
    }

    pub trait InsertableInto<T: Table>: Table {
        fn copy(reader: &Self::Reader, index: u32, builder: &T::Builder) -> Result<Option<u32>>;
    }

    impl CapPtrReader for Empty {
        type Table = Empty;
        type Cap = Infallible;
    }

    impl CapPtrBuilder for Empty {
        fn clear_cap(&self, _: u32) -> Result<()> {
            Err(Error::CapabilityNotAllowed)
        }
        fn as_reader(&self) -> Self {
            Self
        }
    }

    impl<T: Table> InsertableInto<T> for Empty {
        #[inline]
        fn copy(_: &Self::Reader, _: u32, _: &T::Builder) -> Result<Option<u32>> {
            Err(Error::CapabilityNotAllowed)
        }
    }

    // This impl is generic over CapTables instead of Table since it might conflict with the impl
    // above. But that's fine since no types other than Empty actually implement Table and not
    // CapTable.
    impl<C: CapTable> InsertableInto<Empty> for C {
        #[inline]
        fn copy(_: &Self::Reader, _: u32, _: &Empty) -> Result<Option<u32>> {
            Err(Error::CapabilityNotAllowed)
        }
    }

    impl<C, D> InsertableInto<D> for C
    where
        C: CapTable,
        D: CapTable,
        D::TableBuilder: CapTranslator<C::Cap>,
    {
        #[inline]
        fn copy(
            reader: &Self::Reader,
            index: u32,
            builder: &D::TableBuilder,
        ) -> Result<Option<u32>> {
            let cap = reader
                .extract_cap(index)
                .ok_or(Error::InvalidCapabilityPointer(index))?;
            let new_index = CapTranslator::inject_cap(builder, cap);
            Ok(new_index)
        }
    }

    impl<T: CapTableReader> CapPtrReader for T {
        type Table = T::System;
        type Cap = <T::System as CapSystem>::Cap;
    }

    impl<T: CapTableBuilder> CapPtrBuilder for T {
        fn clear_cap(&self, index: u32) -> Result<()> {
            self.drop_cap(index);
            Ok(())
        }
        fn as_reader(&self) -> <T::System as CapTable>::TableReader {
            self.as_reader()
        }
    }
}

/// A type used to represent a minimum cap table type system. It has no other constraints to allow
/// for empty tables to be used. If you want to use a capability system, use the CapSystem trait.
pub trait Table {
    /// A reader for a cap table.
    type Reader: internal::CapPtrReader<Table = Self>;
    /// A builder for a cap table.
    type Builder: internal::CapPtrBuilder<Table = Self>;
}

/// Describes a type which can be imbued with a cap system.
pub trait Capable: Sized {
    /// The table type imbued in this type.
    type Table: Table;

    /// The type imbued in this capable type. This is either `Table::Builder` or `Table::Reader`.
    type Imbued;
    /// The result of imbuing this type with a new table
    type ImbuedWith<T2: Table>: Capable<Table = T2>;

    /// Get a reference to the type imbued in this type.
    fn imbued(&self) -> &Self::Imbued;

    /// Imbues this type with a table, returning the resulting value and the old table.
    fn imbue_release<T2: Table>(
        self,
        new_table: <Self::ImbuedWith<T2> as Capable>::Imbued,
    ) -> (Self::ImbuedWith<T2>, Self::Imbued);

    /// Imbues this type with a table, returning the resulting value and discarding the old table.
    #[inline]
    fn imbue<T2: Table>(
        self,
        new_table: <Self::ImbuedWith<T2> as Capable>::Imbued,
    ) -> Self::ImbuedWith<T2> {
        self.imbue_release(new_table).0
    }

    /// Imbue another type with the table of this type, returning the resulting value and the old table.
    fn imbue_release_into<T>(&self, other: T) -> (T::ImbuedWith<Self::Table>, T::Imbued)
    where
        T: Capable,
        T::ImbuedWith<Self::Table>: Capable<Imbued = Self::Imbued>;

    /// Imbue another type with the table of this type, returning the resulting value and discarding the old table.
    #[inline]
    fn imbue_into<T>(&self, other: T) -> T::ImbuedWith<Self::Table>
    where
        T: Capable,
        T::ImbuedWith<Self::Table>: Capable<Imbued = Self::Imbued>,
    {
        self.imbue_release_into(other).0
    }

    #[inline]
    fn imbue_release_from<T>(self, other: &T) -> (Self::ImbuedWith<T::Table>, Self::Imbued)
    where
        T: Capable,
        Self::ImbuedWith<T::Table>: Capable<Imbued = T::Imbued>,
    {
        other.imbue_release_into(self)
    }

    #[inline]
    fn imbue_from<T>(self, other: &T) -> Self::ImbuedWith<T::Table>
    where
        T: Capable,
        Self::ImbuedWith<T::Table>: Capable<Imbued = T::Imbued>,
    {
        other.imbue_into(self)
    }
}

pub type TableIn<T> = <T as Capable>::Table;

/// A trait that can be used to constrain tables that can be used to copy between pointers with
/// different tables.
///
/// This trait includes copying capabilities into empty tables. If you attempt to copy a capability
/// from a pointer with an empty table or if you attempt to copy a capability from a pointer with a
/// cap table into a pointer with an empty table, an error will occur.
///
/// You don't implement this trait directly. Instead, to support capabilities from external tables,
/// implement `CapTranslator<T>` on your TableBuilder type.
pub trait InsertableInto<T: Table>: internal::InsertableInto<T> {}

impl<T: Table, U: internal::InsertableInto<T>> InsertableInto<T> for U {}

/// An empty cap table type. Capabilities cannot be used in this system (since it doesn't implement
/// any system traits). Use this in place of a table in situations where no RPC system is required.
#[derive(Clone, Copy, PartialEq, Eq, Debug, Default)]
pub struct Empty;

impl Table for Empty {
    type Reader = Empty;
    type Builder = Empty;
}

/// A basic type that provides a "cap" type for capabilities.
pub trait CapSystem {
    /// The type used to represent a capability. This should be a safe dynamic type that can be
    /// exposed to user code in the same way as other typeless dynamic types like AnyPtr. For
    /// example, the type used for this in C++ is Capability::Client.
    type Cap;
}

/// A basic RPC system. Contains all types required to build proper code-gen and accessors that
/// access cap tables.
pub trait CapTable: CapSystem {
    /// The type used to read the cap table.
    type TableReader: CapTableReader<System = Self>;
    /// The type used to build the cap table.
    type TableBuilder: CapTableBuilder<System = Self>;
}

impl<T: CapTable> Table for T {
    type Reader = T::TableReader;
    type Builder = T::TableBuilder;
}

/// A cap system that can provide "broken" and "null" capabilities. Used when reading capabilities
/// from messages for when a default is requested.
pub trait BreakableCapSystem: CapSystem {
    /// Gets a capability that is considered broken by the capability system.
    fn broken<T: Display + ?Sized>(reason: &T) -> Self::Cap;

    /// Gets a capability that can be used in place of a null pointer when reading
    /// a capnp message.
    fn null() -> Self::Cap;
}

pub trait CapTableReader: Clone {
    type System: CapTable;

    /// Extract the capability at the given index. If the index is invalid, returns None.
    fn extract_cap(&self, index: u32) -> Option<<Self::System as CapSystem>::Cap>;
}

pub trait CapTableBuilder: CapTableReader {
    /// Add the capability to the message and return its index. If the same cap is injected
    /// twice, this may return the same index both times, but in this case drop_cap() needs to be
    /// called an equal number of times to actually remove the cap.
    fn inject_cap(&self, cap: <Self::System as CapSystem>::Cap) -> Option<u32>;

    /// Remove a capability injected earlier. Called when the pointer is overwritten or zero'd out.
    fn drop_cap(&self, index: u32);

    fn as_reader(&self) -> <Self::System as CapTable>::TableReader;
}

/// Represents a cap table that can inject external capabilities of type T into this table.
pub trait CapTranslator<T>: CapTableBuilder {
    fn inject_cap(&self, cap: T) -> Option<u32>;
}

impl<C, S, T> CapTranslator<C> for T
where
    S: CapSystem<Cap = C>,
    T: CapTableBuilder<System = S>,
{
    fn inject_cap(&self, cap: C) -> Option<u32> {
        CapTableBuilder::inject_cap(self, cap)
    }
}

/// A pipelined pointer. This can be converted into a capability to that pointer.
pub trait Pipelined: CapSystem {
    /// Expect that the result is a capability and construct a pipelined version of it now.
    fn into_cap(self) -> Self::Cap;
    /// Expect that the result is a capability and construct a pipelined version of it now without
    /// taking ownership.
    ///
    /// The default implementation of this clones the pipeline and calls `into_cap`.
    fn to_cap(&self) -> Self::Cap
    where
        Self: Clone,
    {
        self.clone().into_cap()
    }
}

/// A pipeline builder allowing for pushing new operations to the pipeline based on the type
/// of T.
pub trait PipelineBuilder<T> {
    /// The type used to represent an pipeline operation like getting a pointer field by index.
    type Operation;

    /// Takes ownership of the pipeline, adds a new operation to it, and returns a new pipeline.
    fn push(self, op: Self::Operation) -> Self;
}

/// A typeless pipeline. This is the type all typed pipelines wrap around.
#[derive(Clone)]
pub struct Pipeline<P: Pipelined>(P);

impl<P: Pipelined> Pipeline<P> {
    pub fn new(pipeline: P) -> Self {
        Pipeline(pipeline)
    }

    pub fn push<T>(self, op: <P as PipelineBuilder<T>>::Operation) -> Self
    where
        P: PipelineBuilder<T>,
    {
        Self(self.0.push(op))
    }
    pub fn into_cap(self) -> P::Cap {
        self.0.into_cap()
    }
    pub fn to_cap(&self) -> P::Cap
    where
        P: Clone,
    {
        self.0.to_cap()
    }
}

/// A pipelinable type marker trait. This is primarily implemented for struct types but could be
/// used in the future for lists and other types.
pub trait Pipelinable {
    type Pipeline<P: Pipelined>: TypedPipeline<Pipeline = P>;
}

pub type PipelineOf<T, P> = <T as Pipelinable>::Pipeline<P>;

/// A typed pipeline wrapping an untyped pipeline builder.
pub trait TypedPipeline {
    type Pipeline: Pipelined;

    /// Convert from a typeless pipeline into a pipeline of this type.
    fn from_pipeline(p: Pipeline<Self::Pipeline>) -> Self;

    /// Unwrap the inner typeless pipeline.
    fn into_inner(self) -> Pipeline<Self::Pipeline>;
}
