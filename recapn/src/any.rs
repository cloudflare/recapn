use crate::internal::Sealed;
use crate::ptr::{Capable, CapableBuilder, CapableReader, PtrType};
use crate::rpc::{Empty, Table};
use crate::ty;
use crate::{Error, Family, Result};
use core::convert::{TryFrom, TryInto};

pub mod ptr {
    pub use crate::ptr::{
        ListBuilder, ListReader, PtrBuilder, PtrReader, StructBuilder, StructReader,
    };
}

/// A safe wrapper around any pointer type.
///
/// You probably don't want to name this type this directly. Use one of the ptr
/// type aliases instead (e.g. [`any::PtrReader`](PtrReader)).
pub struct AnyPtr<T = Family> {
    repr: T,
}

impl<T: Capable> Capable for AnyPtr<T> {
    type Table = T::Table;
    type Imbued<T2: Table> = AnyPtr<T::Imbued<T2>>;
}

impl<T: CapableReader> CapableReader for AnyPtr<T> {
    fn imbue_release<T2: Table>(
        self,
        new_table: T2::Reader,
    ) -> (Self::Imbued<T2>, <Self::Table as Table>::Reader) {
        let (new_ptr, old_table) = self.repr.imbue_release(new_table);
        (AnyPtr { repr: new_ptr }, old_table)
    }
}

impl<T: CapableBuilder> CapableBuilder for AnyPtr<T> {
    fn imbue_release<T2: Table>(
        self,
        new_table: T2::Builder,
    ) -> (Self::Imbued<T2>, <Self::Table as Table>::Builder) {
        let (new_ptr, old_table) = self.repr.imbue_release(new_table);
        (AnyPtr { repr: new_ptr }, old_table)
    }
}

/// A safe reader for a pointer of any type
pub type PtrReader<'a, T = Empty> = AnyPtr<ptr::PtrReader<'a, T>>;
/// A safe builder for a pointer of any type
pub type PtrBuilder<'a, T = Empty> = AnyPtr<ptr::PtrBuilder<'a, T>>;

impl<'a, T: Table> PtrReader<'a, T> {
    #[inline]
    pub fn target_size(&self) {
        todo!()
    }

    #[inline]
    pub fn ptr_type(&self) -> Result<PtrType> {
        todo!()
    }

    #[inline]
    pub fn is_null(&self) -> bool {
        matches!(self.ptr_type(), Ok(PtrType::Null))
    }

    #[inline]
    pub fn is_struct(&self) -> bool {
        matches!(self.ptr_type(), Ok(PtrType::Struct))
    }

    #[inline]
    pub fn is_list(&self) -> bool {
        matches!(self.ptr_type(), Ok(PtrType::List))
    }

    #[inline]
    pub fn is_capability(&self) -> bool {
        matches!(self.ptr_type(), Ok(PtrType::Capability))
    }

    /// Attempts to resolve the pointer as the specified type. If an error occurs while reading,
    /// this returns the error. If the pointer is null, this returns Ok(None).
    #[inline]
    pub fn try_read_into_option<U>(self) -> Result<Option<U>>
    where
        Option<U>: TryFrom<ptr::PtrReader<'a, T>, Error = Error>,
    {
        self.repr.try_into()
    }
}

impl<'a> Default for PtrReader<'a, Empty> {
    fn default() -> Self {
        AnyPtr::from(ptr::PtrReader::null())
    }
}

impl<'a, T: Table> From<ptr::PtrReader<'a, T>> for PtrReader<'a, T> {
    fn from(reader: ptr::PtrReader<'a, T>) -> Self {
        Self { repr: reader }
    }
}

impl<'a, T: Table> PtrBuilder<'a, T> {
    pub fn as_reader(&self) -> PtrReader<T> {
        AnyPtr {
            repr: self.repr.as_reader(),
        }
    }

    #[inline]
    pub fn target_size(&self) {
        todo!()
    }

    #[inline]
    pub fn ptr_type(&self) -> Result<PtrType> {
        todo!()
    }

    #[inline]
    pub fn is_null(&self) -> bool {
        matches!(self.ptr_type(), Ok(PtrType::Null))
    }

    #[inline]
    pub fn is_struct(&self) -> bool {
        matches!(self.ptr_type(), Ok(PtrType::Struct))
    }

    #[inline]
    pub fn is_list(&self) -> bool {
        matches!(self.ptr_type(), Ok(PtrType::List))
    }

    #[inline]
    pub fn is_capability(&self) -> bool {
        matches!(self.ptr_type(), Ok(PtrType::Capability))
    }

    #[inline]
    pub fn clear(&mut self) {
        self.repr.clear()
    }
}

impl<'a, T: Table> From<ptr::PtrBuilder<'a, T>> for PtrBuilder<'a, T> {
    fn from(reader: ptr::PtrBuilder<'a, T>) -> Self {
        Self { repr: reader }
    }
}

impl<'a, 'b, T: Table> From<&'b PtrBuilder<'a, T>> for PtrReader<'b, T> {
    fn from(builder: &'b PtrBuilder<'a, T>) -> Self {
        Self {
            repr: builder.repr.as_reader(),
        }
    }
}

pub struct AnyStruct<T = Family> {
    repr: T,
}

impl<T: Capable> Capable for AnyStruct<T> {
    type Table = T::Table;
    type Imbued<T2: Table> = AnyStruct<T::Imbued<T2>>;
}

impl<T: CapableReader> CapableReader for AnyStruct<T> {
    fn imbue_release<T2: Table>(
        self,
        new_table: T2::Reader,
    ) -> (Self::Imbued<T2>, <Self::Table as Table>::Reader) {
        let (new_ptr, old_table) = self.repr.imbue_release(new_table);
        (AnyStruct { repr: new_ptr }, old_table)
    }
}

impl<T: CapableBuilder> CapableBuilder for AnyStruct<T> {
    fn imbue_release<T2: Table>(
        self,
        new_table: T2::Builder,
    ) -> (Self::Imbued<T2>, <Self::Table as Table>::Builder) {
        let (new_ptr, old_table) = self.repr.imbue_release(new_table);
        (AnyStruct { repr: new_ptr }, old_table)
    }
}

impl<T> Sealed for AnyStruct<T> {}

pub type StructReader<'a, T = Empty> = AnyStruct<ptr::StructReader<'a, T>>;
pub type StructBuilder<'a, T = Empty> = AnyStruct<ptr::StructBuilder<'a, T>>;

impl<'a, T: Table> ty::StructReader<'a> for StructReader<'a, T> {
    fn from_ptr(ptr: crate::ptr::StructReader<'a, Self::Table>) -> Self {
        Self { repr: ptr }
    }
}

pub struct AnyList<T = Family> {
    repr: T,
}

impl<T> Sealed for AnyList<T> {}

pub type ListReader<'a, T = Empty> = AnyList<ptr::ListReader<'a, T>>;
pub type ListBuilder<'a, T = Empty> = AnyList<ptr::ListBuilder<'a, T>>;