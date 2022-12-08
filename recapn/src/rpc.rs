pub(crate) mod internal {
    use super::{CapSystem, CapTableBuilder, CapTableReader, Empty, Table};
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
        fn as_reader(&self) -> <T::System as CapSystem>::TableReader {
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

/// An empty cap table type. Capabilities cannot be used in this system (since it doesn't implement
/// any system traits). Use this in place of a table in situations where no RPC system is required.
#[derive(Clone, Copy, PartialEq, Eq, Debug, Default)]
pub struct Empty;

impl Table for Empty {
    type Reader = Empty;
    type Builder = Empty;
}

/// A basic RPC system. Contains all types required to build proper code-gen and accessors.
pub trait CapSystem {
    /// The type used to represent a capability.
    type Cap;

    /// The type used to read the cap table.
    type TableReader: CapTableReader<System = Self>;
    /// The type used to build the cap table.
    type TableBuilder: CapTableBuilder<System = Self>;
}

impl<T: CapSystem> Table for T {
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
    type System: CapSystem;

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

    fn as_reader(&self) -> <Self::System as CapSystem>::TableReader;
}