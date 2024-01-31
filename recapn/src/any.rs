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

use crate::field::{Struct, Capability};
use crate::internal::Sealed;
use crate::list::{self, List};
use crate::ptr::{MessageSize, StructSize, PtrElementSize};
use crate::rpc::{self, Capable, Empty, PipelineBuilder, Pipelined, Table, CapTable, BreakableCapSystem};
use crate::ty::FromPtr;
use crate::{ty, Family, IntoFamily, Result};

pub mod ptr {
    pub use crate::ptr::{
        ListBuilder, ListReader, PtrBuilder, PtrReader, StructBuilder, StructReader,
    };
}

pub use crate::ptr::PtrType;

#[derive(PartialEq, Eq, Debug, Clone, Copy)]
pub enum PtrEquality {
    NotEqual,
    Equal,
    ContainsCaps,
}

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

impl ty::Value for AnyPtr {
    type Default = ptr::PtrReader<'static, Empty>;
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
        let ptr_type = self.ptr_type()?;
        if other.ptr_type()? != ptr_type {
            return Ok(PtrEquality::NotEqual)
        }

        todo!()

        /*
        match ptr_type {
            PtrType::Null => Ok(PtrEquality::Equal),
            PtrType::Struct => self.try_read_as::<AnyStruct>()?
                .equality(&other.try_read_as::<AnyStruct>()?),
            PtrType::List => self.try_read_as::<AnyList>()?
                .equality(&other.try_read_as::<AnyList>()?),
            PtrType::Capability => Ok(PtrEquality::ContainsCaps),
        }
        */
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
    pub fn try_read_option_as_client<C: ty::Capability<Client = T::Cap>>(&self) -> Result<Option<C>> {
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
            Err(err) => T::broken(err.to_string()),
        };
        Some(C::from_client(cap))
    }

    #[inline]
    pub fn read_as_client<C: ty::Capability<Client = T::Cap>>(&self) -> C {
        let cap = match self.0.try_to_capability() {
            Ok(Some(c)) => c,
            Ok(None) => T::null(),
            Err(err) => T::broken(err.to_string()),
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
    pub fn target_size(&self) {
        todo!()
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

    // TODO init, set, orphan APIs

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
    pub fn try_read_option_as<'b, U: ty::ReadPtr<PtrReader<'b, T>>>(&'b self) -> Result<Option<U::Output>> {
        U::try_get_option(self.as_reader())
    }

    #[inline]
    pub fn read_as_struct<'b, S: ty::Struct>(&'b self) -> <Struct<S> as FromPtr<PtrReader<'b, T>>>::Output {
        self.read_as::<Struct<S>>()
    }

    #[inline]
    pub fn read_as_list_of<'b, V: ty::DynListValue>(&'b self) -> <List<V> as FromPtr<PtrReader<'b, T>>>::Output {
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

impl ty::Value for AnyStruct {
    type Default = ptr::StructReader<'static, Empty>;
}

impl ty::DynListValue for AnyStruct {
    const PTR_ELEMENT_SIZE: crate::ptr::PtrElementSize = crate::ptr::PtrElementSize::InlineComposite;
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
    fn get(ptr: StructReader<'a, T>) -> Self::Output { ptr }
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
        todo!()
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

impl ty::Value for AnyList {
    type Default = ptr::ListReader<'static, Empty>;
}
impl ty::ListValue for AnyList {
    const ELEMENT_SIZE: list::ElementSize = list::ElementSize::Pointer;
}

// ListReader impls

impl<'a, T: Table> AsRef<ptr::ListReader<'a, T>> for ListReader<'a, T> {
    fn as_ref(&self) -> &ptr::ListReader<'a, T> {
        &self.0
    }
}

impl<'a, T: Table> ListReader<'a, T> {
    #[inline]
    pub fn len(&self) -> u32 {
        self.0.len().get()
    }

    /// Attempts to cast the list into a value of the specified type.
    pub fn try_typed_as<V: ty::DynListValue>(self) -> Result<list::Reader<'a, V, T>, Self> {
        let size = self.0.element_size();
        if size.upgradable_to(PtrElementSize::size_of::<V>()) {
            Ok(list::List::new(self.0))
        } else {
            Err(self)
        }
    }
}

// ListBuilder impls

impl<'a, T: Table> ListBuilder<'a, T> {
}

impl<'a, T: Table> From<ptr::ListReader<'a, T>> for ListReader<'a, T> {
    fn from(reader: ptr::ListReader<'a, T>) -> Self {
        Self(reader)
    }
}

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

impl<'a, T: Table> FromPtr<PtrReader<'a, T>> for AnyList {
    type Output = ListReader<'a, T>;

    fn get(reader: PtrReader<'a, T>) -> Self::Output {
        let ptr = match reader.0.to_list(None) {
            Ok(Some(ptr)) => ptr,
            _ => ptr::ListReader::empty(list::ElementSize::Void).imbue_from(&reader),
        };
        ptr.into()
    }
}
