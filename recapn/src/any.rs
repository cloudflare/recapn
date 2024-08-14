//! Safe wrappers around "any" pointer, struct, or list value
//!
//! This module is distinct from the [`crate::ptr`] module in that it provides
//! safe, easy to use abstractions around "any" value, whereas the `ptr` module
//! provides all operations used by the rest of the library, including many unsafe
//! ones and ones not designed to be explicitly used by external code. Think many of
//! the operations in `core::ptr` vs `core::intrinsics`.

// This file is divided by types: AnyPtr, AnyStruct, AnyList.
// Each type defines:
//  * A set of aliases
//  * Impls for the following traits in the order of: Reader, Builder, Pipeline
//    * Sealed
//    * Capable
//    * IntoFamily
//    * ty::Value
//    * ty::(Dyn)ListValue
//   * In the order of: Reader, Builder
//    * AsRef (readers + builders)
//    * AsMut (builders)
//    * Type From ptr (readers + builders)
//    * Ptr From type (readers + builders)
//    * ty::FromPtr (readers)
//    * ty::ReadPtr (readers where applicable)
//    * other ty traits
//    * Main impl blocks
//    * Default
//    * PartialEq (readers + builders)

use crate::field::Struct;
use crate::internal::Sealed;
use crate::list::{self, ElementSize, List};
use crate::orphan::{Orphan, Orphanage};
use crate::ptr::{
    CopySize, ElementCount, ErrorHandler, IgnoreErrors, MessageSize, PtrElementSize, StructSize,
};
use crate::rpc::{
    self, BreakableCapSystem, CapTable, Capable, Empty, InsertableInto, PipelineBuilder, Pipelined,
    Table,
};
use crate::ty::{self, FromPtr, StructReader as _};
use crate::{data, text, Family, IntoFamily, Result};

pub mod ptr {
    pub use crate::ptr::{
        ListBuilder, ListReader, PtrBuilder, PtrReader, StructBuilder, StructReader,
    };
}

pub use crate::ptr::{PtrEquality, PtrType};

/// A safe wrapper around any pointer type.
///
/// You probably don't want to name this type this directly. Use one of the ptr
/// type aliases instead (e.g. [`any::PtrReader`](PtrReader)).
#[derive(Clone, Debug)]
pub struct AnyPtr<T = Family>(T);

/// A safe reader for a pointer of any type
pub type PtrReader<'a, T = Empty> = AnyPtr<ptr::PtrReader<'a, T>>;
/// A safe builder for a pointer of any type
pub type PtrBuilder<'a, T = Empty> = AnyPtr<ptr::PtrBuilder<'a, T>>;
/// A safe pipeline builder for a pointer of any type
pub type PtrPipeline<P> = AnyPtr<rpc::Pipeline<P>>;

impl Sealed for AnyPtr {}

impl<T: Capable> Capable for AnyPtr<T> {
    type Table = T::Table;

    type Imbued = T::Imbued;
    type ImbuedWith<T2: Table> = AnyPtr<T::ImbuedWith<T2>>;

    #[inline]
    fn imbued(&self) -> &Self::Imbued {
        self.0.imbued()
    }

    fn imbue_release<T2: Table>(
        self,
        new_table: <Self::ImbuedWith<T2> as Capable>::Imbued,
    ) -> (Self::ImbuedWith<T2>, T::Imbued) {
        let (new_ptr, old_table) = self.0.imbue_release(new_table);
        (AnyPtr(new_ptr), old_table)
    }

    fn imbue_release_into<U>(&self, other: U) -> (U::ImbuedWith<Self::Table>, U::Imbued)
    where
        U: Capable,
        U::ImbuedWith<Self::Table>: Capable<Imbued = Self::Imbued>,
    {
        self.0.imbue_release_into(other)
    }
}

impl<T> IntoFamily for AnyPtr<T> {
    type Family = AnyPtr;
}

impl ty::ListValue for AnyPtr {
    const ELEMENT_SIZE: list::ElementSize = list::ElementSize::Pointer;
}

// PtrReader impls

impl<'a, T: Table> AsRef<ptr::PtrReader<'a, T>> for PtrReader<'a, T> {
    #[inline]
    fn as_ref(&self) -> &ptr::PtrReader<'a, T> {
        &self.0
    }
}

impl<'a, T: Table> From<ptr::PtrReader<'a, T>> for PtrReader<'a, T> {
    #[inline]
    fn from(reader: ptr::PtrReader<'a, T>) -> Self {
        Self(reader)
    }
}

impl<'a, T: Table> From<PtrReader<'a, T>> for ptr::PtrReader<'a, T> {
    #[inline]
    fn from(reader: PtrReader<'a, T>) -> Self {
        reader.0
    }
}

impl<'a, T: Table> ty::FromPtr<PtrReader<'a, T>> for AnyPtr {
    type Output = PtrReader<'a, T>;

    #[inline]
    fn get(ptr: PtrReader<'a, T>) -> Self::Output {
        ptr
    }
}

impl<'a, T: Table> PtrReader<'a, T> {
    #[inline]
    pub fn target_size(&self) -> Result<MessageSize> {
        self.0.target_size()
    }

    #[inline]
    pub fn ptr_type(&self) -> Result<PtrType> {
        self.0.ptr_type()
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
    pub fn equality(&self, other: &PtrReader<'_, impl Table>) -> Result<PtrEquality> {
        self.0.equality(&other.0)
    }

    #[inline]
    pub fn read_as<U: FromPtr<Self>>(self) -> U::Output {
        U::get(self)
    }

    #[inline]
    pub fn try_read_as<U: ty::ReadPtr<Self>>(self) -> Result<U::Output> {
        U::try_get(self)
    }

    #[inline]
    pub fn read_option_as<U: ty::ReadPtr<Self>>(self) -> Option<U::Output> {
        U::get_option(self)
    }

    #[inline]
    pub fn try_read_option_as<U: ty::ReadPtr<Self>>(self) -> Result<Option<U::Output>> {
        U::try_get_option(self)
    }

    #[inline]
    pub fn read_as_struct<S: ty::Struct>(self) -> <Struct<S> as FromPtr<Self>>::Output {
        self.read_as::<Struct<S>>()
    }

    #[inline]
    pub fn read_as_list_of<V: ty::DynListValue>(self) -> <List<V> as FromPtr<Self>>::Output {
        self.read_as::<List<V>>()
    }
}

impl<'a, T: CapTable> PtrReader<'a, T> {
    #[inline]
    pub fn try_read_option_as_client<C: ty::Capability<Client = T::Cap>>(
        &self,
    ) -> Result<Option<C>> {
        match self.0.try_to_capability() {
            Ok(Some(c)) => Ok(Some(C::from_client(c))),
            Ok(None) => Ok(None),
            Err(err) => Err(err),
        }
    }
}

impl<'a, T: CapTable + BreakableCapSystem> PtrReader<'a, T> {
    #[inline]
    pub fn try_read_as_client<C: ty::Capability<Client = T::Cap>>(&self) -> Result<C> {
        let cap = match self.0.try_to_capability() {
            Ok(Some(c)) => c,
            Ok(None) => T::null(),
            Err(err) => return Err(err),
        };
        Ok(C::from_client(cap))
    }

    #[inline]
    pub fn read_option_as_client<C: ty::Capability<Client = T::Cap>>(&self) -> Option<C> {
        let cap = match self.0.try_to_capability() {
            Ok(Some(c)) => c,
            Ok(None) => return None,
            Err(err) => T::broken(&err),
        };
        Some(C::from_client(cap))
    }

    #[inline]
    pub fn read_as_client<C: ty::Capability<Client = T::Cap>>(&self) -> C {
        let cap = match self.0.try_to_capability() {
            Ok(Some(c)) => c,
            Ok(None) => T::null(),
            Err(err) => T::broken(&err),
        };
        C::from_client(cap)
    }
}

impl<'a> Default for PtrReader<'a, Empty> {
    #[inline]
    fn default() -> Self {
        AnyPtr::from(ptr::PtrReader::null())
    }
}

impl<'a, T: Table, T2: Table> PartialEq<PtrReader<'a, T2>> for PtrReader<'a, T> {
    /// Returns whether the two pointers are equal. If the pointers contain capabilities, or an
    /// error occurs while reading the pointers this returns false.
    #[inline]
    fn eq(&self, other: &PtrReader<'a, T2>) -> bool {
        matches!(self.equality(other), Ok(PtrEquality::Equal))
    }
}

// PtrBuilder impls

impl<'a, T: Table> AsRef<ptr::PtrBuilder<'a, T>> for PtrBuilder<'a, T> {
    #[inline]
    fn as_ref(&self) -> &ptr::PtrBuilder<'a, T> {
        &self.0
    }
}

impl<'a, T: Table> AsMut<ptr::PtrBuilder<'a, T>> for PtrBuilder<'a, T> {
    #[inline]
    fn as_mut(&mut self) -> &mut ptr::PtrBuilder<'a, T> {
        &mut self.0
    }
}

impl<'a, T: Table> From<ptr::PtrBuilder<'a, T>> for PtrBuilder<'a, T> {
    #[inline]
    fn from(builder: ptr::PtrBuilder<'a, T>) -> Self {
        Self(builder)
    }
}

impl<'a, T: Table> From<PtrBuilder<'a, T>> for ptr::PtrBuilder<'a, T> {
    #[inline]
    fn from(value: PtrBuilder<'a, T>) -> Self {
        value.0
    }
}

impl<'a, 'b, T: Table> From<&'b PtrBuilder<'a, T>> for PtrReader<'b, T> {
    #[inline]
    fn from(builder: &'b PtrBuilder<'a, T>) -> Self {
        Self(builder.0.as_reader())
    }
}

impl<'a, T: Table> PtrBuilder<'a, T> {
    #[inline]
    pub fn as_reader(&self) -> PtrReader<T> {
        AnyPtr(self.0.as_reader())
    }

    #[inline]
    pub fn by_ref(&mut self) -> PtrBuilder<T> {
        AnyPtr(self.0.by_ref())
    }

    #[inline]
    pub fn target_size(&self) -> MessageSize {
        self.0.target_size()
    }

    #[inline]
    pub fn ptr_type(&self) -> PtrType {
        self.0.ptr_type()
    }

    #[inline]
    pub fn is_null(&self) -> bool {
        matches!(self.ptr_type(), PtrType::Null)
    }

    #[inline]
    pub fn is_struct(&self) -> bool {
        matches!(self.ptr_type(), PtrType::Struct)
    }

    #[inline]
    pub fn is_list(&self) -> bool {
        matches!(self.ptr_type(), PtrType::List)
    }

    #[inline]
    pub fn is_capability(&self) -> bool {
        matches!(self.ptr_type(), PtrType::Capability)
    }

    #[inline]
    pub fn equality(&self, other: &PtrReader<'_, impl Table>) -> Result<PtrEquality> {
        self.as_reader().equality(other)
    }

    #[inline]
    pub fn clear(&mut self) {
        self.0.clear()
    }

    #[inline]
    pub fn read_as<'b, U: FromPtr<PtrReader<'b, T>>>(&'b self) -> U::Output {
        U::get(self.as_reader())
    }

    #[inline]
    pub fn try_read_as<'b, U: ty::ReadPtr<PtrReader<'b, T>>>(&'b self) -> Result<U::Output> {
        U::try_get(self.as_reader())
    }

    #[inline]
    pub fn read_option_as<'b, U: ty::ReadPtr<PtrReader<'b, T>>>(&'b self) -> Option<U::Output> {
        U::get_option(self.as_reader())
    }

    #[inline]
    pub fn try_read_option_as<'b, U: ty::ReadPtr<PtrReader<'b, T>>>(
        &'b self,
    ) -> Result<Option<U::Output>> {
        U::try_get_option(self.as_reader())
    }

    #[inline]
    pub fn read_as_struct<S: ty::Struct>(
        &self,
    ) -> <Struct<S> as FromPtr<PtrReader<'_, T>>>::Output {
        self.read_as::<Struct<S>>()
    }

    #[inline]
    pub fn read_as_list_of<V: ty::DynListValue>(
        &self,
    ) -> <List<V> as FromPtr<PtrReader<'_, T>>>::Output {
        self.read_as::<List<V>>()
    }

    #[inline]
    pub fn init_struct<S: ty::Struct>(self) -> S::Builder<'a, T> {
        let ptr = self.0.init_struct(S::SIZE);
        unsafe { ty::StructBuilder::from_ptr(ptr) }
    }

    #[inline]
    pub fn init_any_struct(self, size: StructSize) -> StructBuilder<'a, T> {
        let ptr = self.0.init_struct(size);
        AnyStruct(ptr)
    }

    #[inline]
    pub fn init_list<V: ty::ListValue>(self, count: u32) -> list::Builder<'a, V, T> {
        let count = ElementCount::new(count).expect("too many elements for list");
        list::Builder::new(self.0.init_list(V::ELEMENT_SIZE, count))
    }

    #[inline]
    pub fn init_any_struct_list(
        self,
        size: StructSize,
        count: u32,
    ) -> list::Builder<'a, AnyStruct, T> {
        let count = ElementCount::new(count).expect("too many elements for list");
        list::Builder::new(self.0.init_list(ElementSize::InlineComposite(size), count))
    }

    #[inline]
    pub fn init_any_list(self, element_size: ElementSize, count: u32) -> ListBuilder<'a, T> {
        let count = ElementCount::new(count).expect("too many elements for list");
        AnyList(self.0.init_list(element_size, count))
    }

    #[inline]
    pub fn init_data(self, count: u32) -> data::Builder<'a> {
        let count = ElementCount::new(count).expect("too many elements for list");
        self.0.init_blob(count).into()
    }

    #[inline]
    pub fn init_text(self, count: u32) -> text::Builder<'a> {
        let count = ElementCount::new(count).expect("too many elements for list");
        text::Builder::new_unchecked(self.0.init_blob(count))
    }

    #[inline]
    pub fn try_set_struct<S: ty::Struct, T2, E>(
        &mut self,
        reader: &S::Reader<'_, T2>,
        err_handler: E,
    ) -> Result<(), E::Error>
    where
        T2: InsertableInto<T>,
        E: ErrorHandler,
    {
        self.0
            .try_set_struct(reader.as_ptr(), CopySize::Minimum(S::SIZE), err_handler)
    }

    #[inline]
    pub fn try_set_any_struct<T2, E>(
        &mut self,
        reader: &StructReader<'_, T2>,
        err_handler: E,
    ) -> Result<(), E::Error>
    where
        T2: InsertableInto<T>,
        E: ErrorHandler,
    {
        self.0
            .try_set_struct(reader.as_ptr(), CopySize::FromValue, err_handler)
    }

    #[inline]
    pub fn try_set_list<V, T2, E>(
        &mut self,
        reader: &list::Reader<'_, V, T2>,
        err_handler: E,
    ) -> Result<(), E::Error>
    where
        V: ty::DynListValue,
        T2: InsertableInto<T>,
        E: ErrorHandler,
    {
        self.0
            .try_set_list(reader.as_ref(), CopySize::FromValue, err_handler)
    }

    #[inline]
    pub fn try_set_any_list<T2, E>(
        &mut self,
        reader: &ListReader<'_, T2>,
        err_handler: E,
    ) -> Result<(), E::Error>
    where
        T2: InsertableInto<T>,
        E: ErrorHandler,
    {
        self.0
            .try_set_list(reader.as_ref(), CopySize::FromValue, err_handler)
    }

    #[inline]
    pub fn set_data(self, value: data::Reader) -> data::Builder<'a> {
        self.0.set_blob(value.into()).into()
    }

    #[inline]
    pub fn set_text(self, value: text::Reader) -> text::Builder<'a> {
        text::Builder::new_unchecked(self.0.set_blob(value.into()))
    }

    #[inline]
    pub fn try_set<T2, E>(
        &mut self,
        reader: &PtrReader<'_, T2>,
        canonical: bool,
        err_handler: E,
    ) -> Result<(), E::Error>
    where
        T2: InsertableInto<T>,
        E: ErrorHandler,
    {
        self.0
            .try_copy_from(reader.as_ref(), canonical, err_handler)
    }

    #[inline]
    pub fn set<T2>(&mut self, reader: &PtrReader<'_, T2>, canonical: bool)
    where
        T2: InsertableInto<T>,
    {
        self.try_set(reader, canonical, IgnoreErrors).unwrap()
    }

    #[inline]
    pub fn disown_into<'b>(&mut self, orphanage: &Orphanage<'b, T>) -> Orphan<'b, AnyPtr, T> {
        Orphan::new(self.0.disown_into(orphanage))
    }

    #[inline]
    pub fn adopt(&mut self, orphan: Orphan<'_, AnyPtr, T>) {
        self.0.adopt(orphan.into_inner());
    }
}

impl<'a, T: Table, T2: Table> PartialEq<PtrReader<'a, T2>> for PtrBuilder<'a, T> {
    #[inline]
    fn eq(&self, other: &PtrReader<'a, T2>) -> bool {
        matches!(self.equality(other), Ok(PtrEquality::Equal))
    }
}

impl<P: Pipelined + PipelineBuilder<AnyPtr>> PtrPipeline<P> {
    pub fn new(pipeline: P) -> Self {
        Self(rpc::Pipeline::new(pipeline))
    }
    pub fn push(self, op: P::Operation) -> Self {
        Self(self.0.push(op))
    }
    pub fn into_cap(self) -> P::Cap {
        self.0.into_cap()
    }
    pub fn to_cap(self) -> P::Cap
    where
        P: Clone,
    {
        self.0.to_cap()
    }
}

#[derive(Clone, Debug)]
pub struct AnyStruct<T = Family>(pub(crate) T);

/// A safe reader for a struct of any type
pub type StructReader<'a, T = Empty> = AnyStruct<ptr::StructReader<'a, T>>;
/// A safe builder for a struct of any type
pub type StructBuilder<'a, T = Empty> = AnyStruct<ptr::StructBuilder<'a, T>>;
/// A safe pipeline builder for a struct of any type
pub type StructPipeline<P> = AnyStruct<rpc::Pipeline<P>>;

impl Sealed for AnyStruct {}

impl<T: Capable> Capable for AnyStruct<T> {
    type Table = T::Table;

    type Imbued = T::Imbued;
    type ImbuedWith<T2: Table> = AnyStruct<T::ImbuedWith<T2>>;

    #[inline]
    fn imbued(&self) -> &Self::Imbued {
        self.0.imbued()
    }

    fn imbue_release<T2: Table>(
        self,
        new_table: <Self::ImbuedWith<T2> as Capable>::Imbued,
    ) -> (Self::ImbuedWith<T2>, T::Imbued) {
        let (new_ptr, old_table) = self.0.imbue_release(new_table);
        (AnyStruct(new_ptr), old_table)
    }

    fn imbue_release_into<U>(&self, other: U) -> (U::ImbuedWith<Self::Table>, U::Imbued)
    where
        U: Capable,
        U::ImbuedWith<Self::Table>: Capable<Imbued = Self::Imbued>,
    {
        self.0.imbue_release_into(other)
    }
}

impl<T> IntoFamily for AnyStruct<T> {
    type Family = AnyStruct;
}

impl ty::DynListValue for AnyStruct {
    const PTR_ELEMENT_SIZE: crate::ptr::PtrElementSize =
        crate::ptr::PtrElementSize::InlineComposite;
}

// StructReader impls

impl<'a, T: Table> AsRef<ptr::StructReader<'a, T>> for StructReader<'a, T> {
    #[inline]
    fn as_ref(&self) -> &ptr::StructReader<'a, T> {
        &self.0
    }
}

impl<'a, T: Table> From<ptr::StructReader<'a, T>> for StructReader<'a, T> {
    #[inline]
    fn from(reader: ptr::StructReader<'a, T>) -> Self {
        Self(reader)
    }
}

impl<'a, T: Table> From<StructReader<'a, T>> for ptr::StructReader<'a, T> {
    #[inline]
    fn from(reader: StructReader<'a, T>) -> Self {
        reader.0
    }
}

impl<'a, T: Table> ty::FromPtr<StructReader<'a, T>> for AnyStruct {
    type Output = StructReader<'a, T>;

    #[inline]
    fn get(ptr: StructReader<'a, T>) -> Self::Output {
        ptr
    }
}

impl<'a, T: Table> ty::FromPtr<PtrReader<'a, T>> for AnyStruct {
    type Output = StructReader<'a, T>;

    #[inline]
    fn get(AnyPtr(reader): PtrReader<'a, T>) -> Self::Output {
        match reader.to_struct() {
            Ok(Some(s)) => AnyStruct(s),
            _ => StructReader::empty().imbue_from(&reader),
        }
    }
}

impl<'a, T: Table> ty::ReadPtr<PtrReader<'a, T>> for AnyStruct {
    #[inline]
    fn try_get_option(AnyPtr(reader): PtrReader<'a, T>) -> Result<Option<Self::Output>> {
        match reader.to_struct() {
            Ok(Some(s)) => Ok(Some(AnyStruct(s))),
            Ok(None) => Ok(None),
            Err(err) => Err(err),
        }
    }
    #[inline]
    fn try_get(AnyPtr(reader): PtrReader<'a, T>) -> Result<Self::Output> {
        match reader.to_struct() {
            Ok(Some(s)) => Ok(AnyStruct(s)),
            Ok(None) => Ok(StructReader::empty().imbue_from(&reader)),
            Err(err) => Err(err),
        }
    }
    #[inline]
    fn get_option(AnyPtr(reader): PtrReader<'a, T>) -> Option<Self::Output> {
        match reader.to_struct() {
            Ok(Some(s)) => Some(AnyStruct(s)),
            _ => None,
        }
    }
}

impl<'a, T: Table> ty::TypedPtr for StructReader<'a, T> {
    type Ptr = ptr::StructReader<'a, T>;
}

impl<'a, T: Table> ty::StructReader for StructReader<'a, T> {}

impl<'a> StructReader<'a, rpc::Empty> {
    #[inline]
    pub const fn empty() -> Self {
        Self(ptr::StructReader::empty())
    }
}

impl<'a, T: Table> StructReader<'a, T> {
    #[inline]
    pub fn total_size(&self) -> Result<MessageSize> {
        self.0.total_size()
    }

    #[inline]
    pub fn data_section(&self) -> &[u8] {
        self.0.data_section()
    }

    #[inline]
    pub fn ptr_count(&self) -> u16 {
        self.0.ptr_count()
    }

    #[inline]
    pub fn ptr_section(&self) -> list::Reader<'a, AnyPtr, T> {
        list::Reader::new(self.0.ptr_section())
    }

    #[inline]
    pub fn ptr_field(&self, index: u16) -> Option<PtrReader<'a, T>> {
        self.0.ptr_field_option(index).map(AnyPtr)
    }

    #[inline]
    pub fn ptr_field_or_default(&self, index: u16) -> PtrReader<'a, T> {
        AnyPtr(self.0.ptr_field(index))
    }

    #[inline]
    pub fn equality(&self, other: &StructReader<'_, impl Table>) -> Result<PtrEquality> {
        self.0.equality(&other.0)
    }

    /// Interprets the struct as the given type
    #[inline]
    pub fn read_as<S: ty::Struct>(self) -> S::Reader<'a, T> {
        ty::StructReader::from_ptr(self.0)
    }
}

impl<'a> Default for StructReader<'a, Empty> {
    fn default() -> Self {
        Self(ptr::StructReader::empty())
    }
}

impl<'a, T: Table, T2: Table> PartialEq<StructReader<'a, T2>> for StructReader<'a, T> {
    #[inline]
    fn eq(&self, other: &StructReader<'a, T2>) -> bool {
        matches!(self.equality(other), Ok(PtrEquality::Equal))
    }
}

// StructBuilder impls

impl<'a, T: Table> AsRef<ptr::StructBuilder<'a, T>> for StructBuilder<'a, T> {
    #[inline]
    fn as_ref(&self) -> &ptr::StructBuilder<'a, T> {
        &self.0
    }
}

impl<'a, T: Table> AsMut<ptr::StructBuilder<'a, T>> for StructBuilder<'a, T> {
    #[inline]
    fn as_mut(&mut self) -> &mut ptr::StructBuilder<'a, T> {
        &mut self.0
    }
}

impl<'a, T: Table> From<ptr::StructBuilder<'a, T>> for StructBuilder<'a, T> {
    #[inline]
    fn from(builder: ptr::StructBuilder<'a, T>) -> Self {
        Self(builder)
    }
}

impl<'a, T: Table> From<StructBuilder<'a, T>> for ptr::StructBuilder<'a, T> {
    #[inline]
    fn from(builder: StructBuilder<'a, T>) -> Self {
        builder.0
    }
}

impl<'a, T: Table> ty::TypedPtr for StructBuilder<'a, T> {
    type Ptr = ptr::StructBuilder<'a, T>;
}

impl<'a, T: Table> ty::StructBuilder for StructBuilder<'a, T> {
    unsafe fn from_ptr(ptr: Self::Ptr) -> Self {
        // This is always allowed since the type represents any sized struct
        Self(ptr)
    }
}

impl<'a, T: Table> StructBuilder<'a, T> {
    #[inline]
    pub fn as_reader(&self) -> StructReader<T> {
        AnyStruct(self.0.as_reader())
    }

    #[inline]
    pub fn by_ref(&mut self) -> StructBuilder<T> {
        AnyStruct(self.0.by_ref())
    }

    #[inline]
    pub fn total_size(&self) -> MessageSize {
        self.0.total_size()
    }

    #[inline]
    pub fn size(&self) -> StructSize {
        self.0.size()
    }

    #[inline]
    pub fn data_section(&self) -> &[u8] {
        self.0.data_section()
    }

    #[inline]
    pub fn data_section_mut(&mut self) -> &mut [u8] {
        self.0.data_section_mut()
    }

    #[inline]
    pub fn ptr_section(&self) -> list::Reader<AnyPtr, T> {
        List::new(self.0.ptr_section())
    }

    #[inline]
    pub fn ptr_section_mut(&mut self) -> list::Builder<AnyPtr, T> {
        List::new(self.0.ptr_section_mut())
    }

    #[inline]
    pub fn equality(&self, other: &StructReader<'_, impl Table>) -> Result<PtrEquality> {
        self.as_reader().equality(other)
    }

    #[inline]
    pub fn try_get_as<S: ty::Struct>(self) -> Result<S::Builder<'a, T>, Self> {
        if S::SIZE.fits_inside(self.size()) {
            Ok(unsafe { ty::StructBuilder::from_ptr(self.0) })
        } else {
            Err(self)
        }
    }
}

impl<'a, T: Table, T2: Table> PartialEq<StructReader<'a, T2>> for StructBuilder<'a, T> {
    #[inline]
    fn eq(&self, other: &StructReader<'a, T2>) -> bool {
        matches!(self.equality(other), Ok(PtrEquality::Equal))
    }
}

impl<'a, T: Table, T2: Table> PartialEq<StructBuilder<'a, T2>> for StructReader<'a, T> {
    #[inline]
    fn eq(&self, other: &StructBuilder<'a, T2>) -> bool {
        matches!(other.equality(self), Ok(PtrEquality::Equal))
    }
}

impl<P: Pipelined + PipelineBuilder<AnyStruct>> StructPipeline<P> {
    pub fn new(pipeline: rpc::Pipeline<P>) -> Self {
        Self(pipeline)
    }
    pub fn push(self, op: P::Operation) -> Self {
        Self(self.0.push(op))
    }
    pub fn into_cap(self) -> P::Cap {
        self.0.into_cap()
    }
    pub fn to_cap(self) -> P::Cap
    where
        P: Clone,
    {
        self.0.to_cap()
    }
}

#[derive(Clone, Debug)]
pub struct AnyList<T = Family>(T);

pub type ListReader<'a, T = Empty> = AnyList<ptr::ListReader<'a, T>>;
pub type ListBuilder<'a, T = Empty> = AnyList<ptr::ListBuilder<'a, T>>;
pub type ListPipeline<P> = AnyList<rpc::Pipeline<P>>;

impl<T> Sealed for AnyList<T> {}

impl<T: Capable> Capable for AnyList<T> {
    type Table = T::Table;

    type Imbued = T::Imbued;
    type ImbuedWith<T2: Table> = AnyList<T::ImbuedWith<T2>>;

    #[inline]
    fn imbued(&self) -> &Self::Imbued {
        self.0.imbued()
    }

    #[inline]
    fn imbue_release<T2: Table>(
        self,
        new_table: <Self::ImbuedWith<T2> as Capable>::Imbued,
    ) -> (Self::ImbuedWith<T2>, T::Imbued) {
        let (new_ptr, old_table) = self.0.imbue_release(new_table);
        (AnyList(new_ptr), old_table)
    }

    #[inline]
    fn imbue_release_into<U>(&self, other: U) -> (U::ImbuedWith<Self::Table>, U::Imbued)
    where
        U: Capable,
        U::ImbuedWith<Self::Table>: Capable<Imbued = Self::Imbued>,
    {
        self.0.imbue_release_into(other)
    }
}

impl<T> IntoFamily for AnyList<T> {
    type Family = AnyList;
}

impl ty::ListValue for AnyList {
    const ELEMENT_SIZE: list::ElementSize = list::ElementSize::Pointer;
}

// ListReader impls

impl<'a, T: Table> AsRef<ptr::ListReader<'a, T>> for ListReader<'a, T> {
    #[inline]
    fn as_ref(&self) -> &ptr::ListReader<'a, T> {
        &self.0
    }
}

impl<'a, T: Table> From<ptr::ListReader<'a, T>> for ListReader<'a, T> {
    #[inline]
    fn from(reader: ptr::ListReader<'a, T>) -> Self {
        Self(reader)
    }
}

impl<'a, T: Table> From<ListReader<'a, T>> for ptr::ListReader<'a, T> {
    #[inline]
    fn from(reader: ListReader<'a, T>) -> Self {
        reader.0
    }
}

impl<'a, T: Table> ty::FromPtr<ListReader<'a, T>> for AnyList {
    type Output = ListReader<'a, T>;

    #[inline]
    fn get(ptr: ListReader<'a, T>) -> Self::Output {
        ptr
    }
}

impl<'a, T: Table> ty::FromPtr<PtrReader<'a, T>> for AnyList {
    type Output = ListReader<'a, T>;

    fn get(AnyPtr(reader): PtrReader<'a, T>) -> Self::Output {
        match reader.to_list(None) {
            Ok(Some(s)) => AnyList(s),
            _ => ListReader::empty(ElementSize::Void).imbue_from(&reader),
        }
    }
}

impl<'a, T: Table> ty::ReadPtr<PtrReader<'a, T>> for AnyList {
    #[inline]
    fn try_get_option(AnyPtr(reader): PtrReader<'a, T>) -> Result<Option<Self::Output>> {
        match reader.to_list(None) {
            Ok(Some(r)) => Ok(Some(AnyList(r))),
            Ok(None) => Ok(None),
            Err(err) => Err(err),
        }
    }
    #[inline]
    fn try_get(AnyPtr(reader): PtrReader<'a, T>) -> Result<Self::Output> {
        match reader.to_list(None) {
            Ok(Some(r)) => Ok(AnyList(r)),
            Ok(None) => Ok(ListReader::empty(ElementSize::Void).imbue_from(&reader)),
            Err(err) => Err(err),
        }
    }
    #[inline]
    fn get_option(AnyPtr(reader): PtrReader<'a, T>) -> Option<Self::Output> {
        match reader.to_list(None) {
            Ok(Some(r)) => Some(AnyList(r)),
            _ => None,
        }
    }
}

impl<'a, T: Table> ty::TypedPtr for ListReader<'a, T> {
    type Ptr = ptr::ListReader<'a, T>;
}

impl ListReader<'_> {
    #[inline]
    pub fn empty(element_size: ElementSize) -> Self {
        Self(ptr::ListReader::empty(element_size))
    }
}

impl<'a, T: Table> ListReader<'a, T> {
    #[inline]
    pub fn total_size(&self) -> Result<MessageSize> {
        self.0.total_size()
    }

    #[inline]
    pub fn len(&self) -> u32 {
        self.0.len().get()
    }

    #[inline]
    pub fn is_empty(&self) -> bool {
        self.len() == 0
    }

    #[inline]
    pub fn element_size(&self) -> ElementSize {
        self.0.element_size()
    }

    #[inline]
    pub fn equality(&self, other: &ListReader<'_, impl Table>) -> Result<PtrEquality> {
        self.0.equality(&other.0)
    }

    /// Attempts to cast the list into a value of the specified type.
    #[inline]
    pub fn try_get_as<V: ty::DynListValue>(self) -> Result<list::Reader<'a, V, T>, Self> {
        let size = self.0.element_size();
        if size.upgradable_to(PtrElementSize::size_of::<V>()) {
            Ok(list::List::new(self.0))
        } else {
            Err(self)
        }
    }
}

// ListBuilder impls

impl<'a, T: Table> AsRef<ptr::ListBuilder<'a, T>> for ListBuilder<'a, T> {
    fn as_ref(&self) -> &ptr::ListBuilder<'a, T> {
        &self.0
    }
}

impl<'a, T: Table> AsMut<ptr::ListBuilder<'a, T>> for ListBuilder<'a, T> {
    fn as_mut(&mut self) -> &mut ptr::ListBuilder<'a, T> {
        &mut self.0
    }
}

impl<'a, T: Table> From<ptr::ListBuilder<'a, T>> for ListBuilder<'a, T> {
    fn from(builder: ptr::ListBuilder<'a, T>) -> Self {
        Self(builder)
    }
}

impl<'a, T: Table> From<ListBuilder<'a, T>> for ptr::ListBuilder<'a, T> {
    fn from(builder: ListBuilder<'a, T>) -> Self {
        builder.0
    }
}

impl<'a, T: Table> ListBuilder<'a, T> {
    #[inline]
    pub fn as_reader(&self) -> ListReader<T> {
        self.0.as_reader().into()
    }

    #[inline]
    pub fn by_ref(&mut self) -> ListBuilder<T> {
        self.0.by_ref().into()
    }

    #[inline]
    pub fn total_size(&self) -> MessageSize {
        self.0.total_size()
    }

    #[inline]
    pub fn len(&self) -> u32 {
        self.0.len().get()
    }

    #[inline]
    pub fn is_empty(&self) -> bool {
        self.len() == 0
    }

    #[inline]
    pub fn element_size(&self) -> ElementSize {
        self.0.element_size()
    }

    #[inline]
    pub fn equality(&self, other: &ListReader<'_, impl Table>) -> Result<PtrEquality> {
        self.as_reader().equality(other)
    }
}

impl<'a, T: Table, T2: Table> PartialEq<ListReader<'a, T2>> for ListBuilder<'a, T> {
    #[inline]
    fn eq(&self, other: &ListReader<'a, T2>) -> bool {
        matches!(self.equality(other), Ok(PtrEquality::Equal))
    }
}

impl<'a, T: Table, T2: Table> PartialEq<ListBuilder<'a, T2>> for ListReader<'a, T> {
    #[inline]
    fn eq(&self, other: &ListBuilder<'a, T2>) -> bool {
        matches!(other.equality(self), Ok(PtrEquality::Equal))
    }
}
