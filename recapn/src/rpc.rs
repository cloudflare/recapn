pub(crate) mod internal {
    use super::{
        CapSystem, CapTable, CapTableBuilder, CapTableReader, CapTranslator, Empty, Table,
    };
    use crate::{ErrorKind, Result};
    use core::convert::Infallible;

    pub trait CapPtrReader: Clone {
        type Table: Table;
        type Cap;

        fn read_cap(&self, index: u32) -> Result<Self::Cap>;
    }

    pub trait CapPtrBuilder: CapPtrReader {
        fn set_cap(&self, cap: Self::Cap) -> Result<u32>;
        fn clear_cap(&self, index: u32) -> Result<()>;
        fn as_reader(&self) -> <Self::Table as Table>::Reader;
    }

    pub trait InsertableInto<T: Table>: Table {
        fn copy(reader: &Self::Reader, index: u32, builder: &T::Builder) -> Result<u32>;
    }

    impl CapPtrReader for Empty {
        type Table = Empty;
        type Cap = Infallible;

        fn read_cap(&self, _: u32) -> Result<Infallible> {
            Err(ErrorKind::CapabilityNotAllowed.into())
        }
    }

    impl CapPtrBuilder for Empty {
        fn set_cap(&self, cap: Infallible) -> Result<u32> {
            match cap {}
        }
        fn clear_cap(&self, _: u32) -> Result<()> {
            Err(ErrorKind::CapabilityNotAllowed.into())
        }
        fn as_reader(&self) -> Self {
            Self
        }
    }

    impl<T: Table> InsertableInto<T> for Empty {
        fn copy(_: &Self::Reader, _: u32, _: &T::Builder) -> Result<u32> {
            Err(ErrorKind::CapabilityNotAllowed.into())
        }
    }

    // This impl is generic over CapTables instead of Table since it might conflict with the impl
    // above. But that's fine since no types other than Empty actually implement Table and not
    // CapTable.
    impl<C: CapTable> InsertableInto<Empty> for C {
        fn copy(_: &Self::Reader, _: u32, _: &Empty) -> Result<u32> {
            Err(ErrorKind::CapabilityNotAllowed.into())
        }
    }

    impl<C, D> InsertableInto<D> for C
    where
        C: CapTable,
        D: CapTable,
        D::TableBuilder: CapTranslator<C::Cap>,
    {
        fn copy(reader: &Self::Reader, index: u32, builder: &D::TableBuilder) -> Result<u32> {
            let cap = reader
                .extract_cap(index)
                .ok_or_else(|| ErrorKind::InvalidCapabilityPointer(index))?;
            let new_index = CapTranslator::inject_cap(builder, cap);
            Ok(new_index)
        }
    }

    impl<T: CapTableReader> CapPtrReader for T {
        type Table = T::System;
        type Cap = <T::System as CapSystem>::Cap;

        fn read_cap(&self, index: u32) -> Result<Self::Cap> {
            self.extract_cap(index)
                .ok_or(ErrorKind::InvalidCapabilityPointer(index).into())
        }
    }

    impl<T: CapTableBuilder> CapPtrBuilder for T {
        fn set_cap(&self, cap: Self::Cap) -> Result<u32> {
            Ok(self.inject_cap(cap))
        }
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
    fn broken(reason: String) -> Self::Cap;

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
    fn inject_cap(&self, cap: <Self::System as CapSystem>::Cap) -> u32;

    /// Remove a capability injected earlier. Called when the pointer is overwritten or zero'd out.
    fn drop_cap(&self, index: u32);

    fn as_reader(&self) -> <Self::System as CapTable>::TableReader;
}

/// Represents a cap table that can inject external capabilities of type T into this table.
pub trait CapTranslator<T>: CapTableBuilder {
    fn inject_cap(&self, cap: T) -> u32;
}

impl<C, S, T> CapTranslator<C> for T
where
    S: CapSystem<Cap = C>,
    T: CapTableBuilder<System = S>,
{
    fn inject_cap(&self, cap: C) -> u32 {
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
