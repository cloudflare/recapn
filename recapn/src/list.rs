//! A fixed-size, typed, list of values in a Cap'n Proto message.
//!
//! Where other libraries would implement distinct types for different types of lists,
//! `recapn` lists are different because all lists are represented as one type: `List`.
//! Like fields, we use wrapper types and indirection to implement distinct accessors
//! for different types of values. For example,
//!
//! * A list of primitives is a `List<V>` where V is the primitive value.
//! * A list of enums is a `List<field::Enum<T>>` where T is the enum type.
//! * A list of structs is a `List<field::Struct<T>>` where T is the struct type.
//! * A list of lists is a `List<List<T>>` where T is the inner list type.

use crate::alloc::ElementCount;
use crate::any::{self, AnyList, AnyPtr, AnyStruct};
use crate::data::{self, Data};
use crate::ptr::CopySize;
use crate::text::{self, Text};
use crate::field::{Enum, EnumResult, Struct};
use crate::internal::Sealed;
use crate::ptr::{internal::FieldData, PtrElementSize, StructSize, WriteNull, write_null};
use crate::rpc::{self, Capable, InsertableInto, Table};
use crate::{ty, Error, Family, IntoFamily, Result, ErrorKind};

use core::convert::{self, TryFrom, Infallible};
use core::marker::PhantomData;
use core::ops::{ControlFlow, Range};

pub use crate::ptr::ElementSize;

pub mod ptr {
    pub use crate::ptr::{
        ListBuilder as Builder,
        ListReader as Reader,
        PtrElementSize as ElementSize,
    };
}

pub type Reader<'a, V, T = rpc::Empty> = List<V, ptr::Reader<'a, T>>;
pub type Builder<'a, V, T = rpc::Empty> = List<V, ptr::Builder<'a, T>>;

/// A Cap'n Proto list.
pub struct List<V, T = Family> {
    repr: T,
    v: PhantomData<fn(u32) -> V>,
}

impl<V, T> List<V, T> {
    pub(crate) const fn new(repr: T) -> Self {
        Self { repr, v: PhantomData }
    }
}

impl<V, T> Sealed for List<V, T> {}

impl<T: Clone, V> Clone for List<V, T> {
    fn clone(&self) -> Self {
        Self::new(self.repr.clone())
    }
}

impl<V, T> IntoFamily for List<V, T> {
    type Family = List<V>;
}

impl<V, T: Capable> Capable for List<V, T> {
    type Table = T::Table;

    type Imbued = T::Imbued;
    type ImbuedWith<T2: Table> = List<V, T::ImbuedWith<T2>>;

    #[inline]
    fn imbued(&self) -> &Self::Imbued {
        self.repr.imbued()
    }

    #[inline]
    fn imbue_release<T2: Table>(
        self,
        new_table: <Self::ImbuedWith<T2> as Capable>::Imbued,
    ) -> (Self::ImbuedWith<T2>, T::Imbued) {
        let (new_ptr, old_table) = self.repr.imbue_release(new_table);
        (List::new(new_ptr), old_table)
    }

    #[inline]
    fn imbue_release_into<U>(&self, other: U) -> (U::ImbuedWith<Self::Table>, U::Imbued)
    where
        U: Capable,
        U::ImbuedWith<Self::Table>: Capable<Imbued = Self::Imbued>,
    {
        self.repr.imbue_release_into(other)
    }
}

// Reader traits and impls

impl<'a, V: ty::DynListValue, T: Table> ty::FromPtr<any::PtrReader<'a, T>> for List<V> {
    type Output = Reader<'a, V, T>;

    fn get(ptr: any::PtrReader<'a, T>) -> Self::Output {
        let inner = any::ptr::PtrReader::from(ptr);
        match inner.to_list(Some(ptr::ElementSize::size_of::<V>())) {
            Ok(Some(ptr)) => List::new(ptr),
            _ => Reader::empty().imbue_from(&inner),
        }
    }
}

impl<'a, V: ty::DynListValue, T: Table> ty::ReadPtr<any::PtrReader<'a, T>> for List<V> {
    fn try_get_option(ptr: any::PtrReader<'a, T>) -> Result<Option<Self::Output>> {
        let inner = any::ptr::PtrReader::from(ptr);
        match inner.to_list(Some(PtrElementSize::size_of::<V>())) {
            Ok(Some(ptr)) => Ok(Some(List::new(ptr))),
            Ok(None) => Ok(None),
            Err(err) => Err(err),
        }
    }
    fn try_get(ptr: any::PtrReader<'a, T>) -> Result<Self::Output> {
        let inner = any::ptr::PtrReader::from(ptr);
        match inner.to_list(Some(PtrElementSize::size_of::<V>())) {
            Ok(Some(ptr)) => Ok(List::new(ptr)),
            Ok(None) => Ok(Reader::empty().imbue_from(&inner)),
            Err(err) => Err(err),
        }
    }
    fn get_option(ptr: any::PtrReader<'a, T>) -> Option<Self::Output> {
        let inner = any::ptr::PtrReader::from(ptr);
        match inner.to_list(Some(PtrElementSize::size_of::<V>())) {
            Ok(Some(ptr)) => Some(List::new(ptr)),
            _ => None,
        }
    }
}

impl<'a, V: ty::DynListValue, T: Table> ty::FromPtr<any::ListReader<'a, T>> for List<V> {
    type Output = Reader<'a, V, T>;

    /// Converts from a AnyList reader to a List<V> reader. In the case of an invalid upgrade
    /// between types, this returns an empty list.
    fn get(ptr: any::ListReader<'a, T>) -> Self::Output {
        match ptr.try_typed_as::<V>() {
            Ok(ptr) => ptr,
            Err(old) => Reader::empty().imbue_from(&old),
        }
    }
}

impl<'a, V, T: Table> AsRef<ptr::Reader<'a, T>> for Reader<'a, V, T> {
    fn as_ref(&self) -> &ptr::Reader<'a, T> {
        &self.repr
    }
}

impl<'a, V: ty::DynListValue> Reader<'a, V, rpc::Empty> {
    pub const fn empty() -> Self {
        Self::new(ptr::Reader::empty(ElementSize::empty_size_of::<V>()))
    }
}

impl<'a, V, T: Table> Reader<'a, V, T> {
    /// Gets the length of the list
    #[inline]
    pub fn len(&self) -> u32 {
        self.repr.len().get()
    }

    /// Get the element at the specified index, or None if the index is out of range.
    #[inline]
    pub fn try_at<'b>(&'b self, index: u32) -> Option<ElementReader<'a, 'b, V, T>>
    where
        V: ListAccessable<&'b Self>
    {
        (index < self.len()).then(|| unsafe { self.at_unchecked(index) })
    }

    /// Get the element at the specified index, or None if the index is out of range.
    #[inline]
    pub fn at<'b>(&'b self, index: u32) -> ElementReader<'a, 'b, V, T>
    where
        V: ListAccessable<&'b Self>
    {
        self.try_at(index).expect("index out of bounds")
    }

    /// Get the element at the specified index without bounds checks.
    #[inline]
    pub unsafe fn at_unchecked<'b>(&'b self, index: u32) -> ElementReader<'a, 'b, V, T>
    where
        V: ListAccessable<&'b Self>
    {
        V::get(self, index)
    }
}

// Builder traits and impls

impl<'a, V, T: Table> AsRef<ptr::Builder<'a, T>> for Builder<'a, V, T> {
    fn as_ref(&self) -> &ptr::Builder<'a, T> {
        &self.repr
    }
}

impl<'a, V, T: Table> AsMut<ptr::Builder<'a, T>> for Builder<'a, V, T> {
    fn as_mut(&mut self) -> &mut ptr::Builder<'a, T> {
        &mut self.repr
    }
}

impl<'a, V, T: Table> Builder<'a, V, T> {
    /// Gets the length of the list
    #[inline]
    pub fn len(&self) -> ElementCount {
        self.repr.len()
    }

    #[inline]
    pub fn as_reader(&self) -> Reader<V, T> {
        List {
            v: PhantomData,
            repr: self.repr.as_reader(),
        }
    }

    /// Gets a mutable view of the element at the specified index, or None if the index is out of range.
    #[inline]
    pub fn try_at<'b>(&'b mut self, index: u32) -> Option<ElementBuilder<'a, 'b, V, T>>
    where
        V: ListAccessable<&'b mut Self>,
    {
        (index < self.len().get()).then(move || unsafe { self.at_unchecked(index) })
    }

    /// Get the element at the specified index, or panics if out of range.
    #[inline]
    pub fn at<'b>(&'b mut self, index: u32) -> ElementBuilder<'a, 'b, V, T>
    where
        V: ListAccessable<&'b mut Self>,
    {
        self.try_at(index).expect("index out of bounds")
    }

    /// Gets a mutable view of the element at the specified index without bounds checks.
    #[inline]
    pub unsafe fn at_unchecked<'b>(&'b mut self, index: u32) -> ElementBuilder<'a, 'b, V, T>
    where
        V: ListAccessable<&'b mut Self>,
    {
        V::get(self, index)
    }
}

/// A checked index view into a List.
pub struct Element<T> {
    list: T,
    idx: u32,
}

type ReaderElement<'a, 'b, V, T = rpc::Empty> = Element<&'b Reader<'a, V, T>>;
type BuilderElement<'a, 'b, V, T = rpc::Empty> = Element<&'b mut Builder<'a, V, T>>;

impl<T> Element<T> {
    fn new(list: T, idx: u32) -> Self {
        Self { list, idx }
    }
}

/// An element in a list reader
pub type ElementReader<'a, 'b, V, T = rpc::Empty> =
    <V as ListAccessable<&'b Reader<'a, V, T>>>::View;
/// An element in a list builder with a mutable borrow
pub type ElementBuilder<'a, 'b, V, T = rpc::Empty> =
    <V as ListAccessable<&'b mut Builder<'a, V, T>>>::View;

impl<'a, 'b, V, T> BuilderElement<'a, 'b, V, T>
where
    T: Table,
{
    /// Borrows an element, rather than consuming it.
    ///
    /// Allows calling multiple builder methods without consuming the element.
    ///
    /// # Example
    ///
    /// Basic usage:
    /// ```
    /// let mut element = list.at(0).unwrap();
    /// let mut sublist = element.by_ref().get_mut_or_init(2);
    ///
    /// // do some processing on the list. in this case, we don't actually know if the list is
    /// // large enough for our needs. so we use by_ref to keep the original element around and
    /// // resize if it's not big enough
    ///
    /// let mut sublist = element.by_ref().get_mut_or_init(4);
    #[inline]
    pub fn by_ref(&mut self) -> BuilderElement<'a, '_, V, T> {
        Element {
            list: &mut *self.list,
            idx: self.idx,
        }
    }
}

impl<'a, 'b, V: FieldData, T: Table> BuilderElement<'a, 'b, V, T> {
    /// A generic accessor for getting "field data", that is, primitive numeric and boolean types.
    #[inline]
    pub fn get(&self) -> V {
        unsafe { self.list.as_ref().data_unchecked(self.idx) }
    }
    /// A generic accessor for setting "field data", that is, primitive numeric and boolean types.
    #[inline]
    pub fn set(&mut self, value: V) {
        unsafe { self.list.as_mut().set_data_unchecked(self.idx, value) }
    }
}

impl<'a, 'b, T: Table> BuilderElement<'a, 'b, f32, T> {
    #[inline]
    /// Canonicalizes NaN values by blowing away the NaN payload.
    pub fn set_canonical(&mut self, value: f32) {
        const CANONICAL_NAN: u32 = 0x7fc00000u32;

        if value.is_nan() {
            unsafe { self.list.as_mut().set_data_unchecked(self.idx, CANONICAL_NAN) }
        } else {
            self.set(value)
        }
    }
}

impl<'a, 'b, T: Table> BuilderElement<'a, 'b, f64, T> {
    /// Canonicalizes NaN values by blowing away the NaN payload.
    #[inline]
    pub fn set_canonical(&mut self, value: f64) {
        const CANONICAL_NAN: u64 = 0x7ff8000000000000u64;

        if value.is_nan() {
            unsafe { self.list.as_mut().set_data_unchecked(self.idx, CANONICAL_NAN) }
        } else {
            self.set(value)
        }
    }
}

impl<'a, 'b, E: ty::Enum, T: Table> BuilderElement<'a, 'b, Enum<E>, T>
where
    T: Table,
    E: ty::Enum,
{
    #[inline]
    pub fn get(&self) -> EnumResult<E> {
        let value = unsafe { self.list.as_ref().data_unchecked::<u16>(self.idx) };
        E::try_from(value)
    }
    #[inline]
    pub fn set(&mut self, value: E) {
        let value = value.into();
        unsafe { self.list.as_mut().set_data_unchecked::<u16>(self.idx, value) }
    }
}

impl<'a, 'b, S, T> BuilderElement<'a, 'b, Struct<S>, T>
where
    S: ty::Struct,
    T: Table,
{
    #[inline]
    pub fn get(self) -> S::Builder<'b, T> {
        unsafe { ty::StructBuilder::from_ptr(self.list.as_mut().struct_mut_unchecked(self.idx)) }
    }

    #[inline]
    pub fn reader(&self) -> S::Reader<'_, T> {
        ty::StructReader::from_ptr(unsafe { self.list.as_ref().struct_unchecked(self.idx) })
    }

    /// Mostly behaves like you'd expect `set` to behave, but with a caveat originating from
    /// the fact that structs in a struct list are allocated inline rather than by pointer:
    /// If the source struct is larger than the target struct -- say, because the source was built
    /// using a newer version of the schema that has additional fields -- it will be truncated,
    /// losing data.
    ///
    /// If an error occurs while reading the struct, null is written instead. If you want a falible
    /// set, use `try_set_with_caveats`.
    #[inline]
    pub fn set_with_caveats(&mut self, reader: &S::Reader<'_, impl InsertableInto<T>>) {
        self.try_set_with_caveats::<convert::Infallible, _>(reader, |_| ControlFlow::Continue(WriteNull))
            .unwrap()
    }

    #[inline]
    pub fn try_set_with_caveats<E, F>(
        &mut self,
        reader: &S::Reader<'_, impl InsertableInto<T>>,
        err_handler: F,
    ) -> Result<(), E>
    where
        F: FnMut(Error) -> ControlFlow<E, WriteNull>,
    {
        todo!()
    }
}

impl<'a, 'b, T> BuilderElement<'a, 'b, AnyStruct, T>
where
    T: Table,
{
    #[inline]
    pub fn get(self) -> any::StructBuilder<'b, T> {
        unsafe { self.list.repr.struct_mut_unchecked(self.idx).into() }
    }

    #[inline]
    pub fn reader(&self) -> any::StructReader<'_, T> {
        unsafe { self.list.as_ref().struct_unchecked(self.idx).into() }
    }

    /// Mostly behaves like you'd expect `set` to behave, but with a caveat originating from
    /// the fact that structs in a struct list are allocated inline rather than by pointer:
    /// If the source struct is larger than the target struct -- say, because the source was built
    /// using a newer version of the schema that has additional fields -- it will be truncated,
    /// losing data.
    ///
    /// If an error occurs while reading the struct, null is written instead. If you want a falible
    /// set, use `try_set_with_caveats`.
    #[inline]
    pub fn set_with_caveats(&mut self, reader: &any::StructReader<'_, impl Table>) {
        self.try_set_with_caveats::<convert::Infallible, _>(reader, |_| ControlFlow::Continue(WriteNull))
            .unwrap()
    }

    #[inline]
    pub fn try_set_with_caveats<E, F>(
        &mut self,
        reader: &any::StructReader<'_, impl Table>,
        err_handler: F,
    ) -> Result<(), E>
    where
        F: FnMut(Error) -> ControlFlow<E, WriteNull>,
    {
        todo!()
    }
}

impl<'a, 'b, V, T> ReaderElement<'a, 'b, List<V>, T>
where
    T: Table,
    V: ty::DynListValue,
{
    #[inline]
    fn ptr_reader(&self) -> crate::ptr::PtrReader<'a, T> {
        unsafe { self.list.as_ref().ptr_unchecked(self.idx) }
    }

    #[inline]
    fn empty_list(&self) -> Reader<'a, V, T> {
        List::new(ptr::Reader::empty(ElementSize::Pointer).imbue_from(self.list))
    }

    /// Returns whether this list element is a null pointer.
    #[inline]
    pub fn is_null(&self) -> bool {
        self.ptr_reader().is_null()
    }

    /// Returns the list value in this element, or an empty list if the list is null
    /// or an error occurs while reading.
    #[inline]
    pub fn get(&self) -> Reader<'a, V, T> {
        match self.try_get_option() {
            Ok(Some(reader)) => reader,
            _ => self.empty_list(),
        }
    }

    /// Returns the list value in this element, or None if the list element is null or
    /// an error occurs while reading.
    #[inline]
    pub fn get_option(&self) -> Option<Reader<'a, V, T>> {
        match self.try_get_option() {
            Ok(Some(reader)) => Some(reader),
            _ => None,
        }
    }

    /// Returns the list value in this element. If the element is null, this returns an
    /// empty list. If an error occurs while reading it is returned.
    #[inline]
    pub fn try_get(&self) -> Result<Reader<'a, V, T>> {
        match self.try_get_option() {
            Ok(Some(reader)) => Ok(reader),
            Ok(None) => Ok(self.empty_list()),
            Err(err) => Err(err),
        }
    }

    /// Returns the list value in this element. If the element is null, this returns Ok(None).
    /// If an error occurs while reading it is returned.
    #[inline]
    pub fn try_get_option(&self) -> Result<Option<Reader<'a, V, T>>> {
        match self.ptr_reader().to_list(Some(PtrElementSize::size_of::<V>())) {
            Ok(Some(reader)) => Ok(Some(List::new(reader))),
            Ok(None) => Ok(None),
            Err(err) => Err(err),
        }
    }
}

impl<'a, 'b, T, V> BuilderElement<'a, 'b, List<V>, T>
where
    T: Table,
    V: ty::ListValue,
{
    #[inline]
    fn ptr_builder(&mut self) -> crate::ptr::PtrBuilder<T> {
        unsafe { self.list.as_mut().ptr_mut_unchecked(self.idx) }
    }

    #[inline]
    fn into_ptr_builder(self) -> crate::ptr::PtrBuilder<'b, T> {
        unsafe { self.list.as_mut().ptr_mut_unchecked(self.idx) }
    }

    /// Gets the value of the element in the list as a builder. If the value is null or invalid
    /// in some way, an empty list builder is returned.
    #[inline]
    pub fn get(self) -> Builder<'b, V, T> {
        List::new(
            self.into_ptr_builder()
                .to_list_mut_or_empty(Some(V::ELEMENT_SIZE)),
        )
    }

    #[inline]
    pub fn try_get_option(self) -> Result<Option<Builder<'b, V, T>>> {
        match self.into_ptr_builder().to_list_mut(Some(V::ELEMENT_SIZE)) {
            Ok(ptr) => Ok(Some(List::new(ptr))),
            Err((None, _)) => Ok(None),
            Err((Some(err), _)) => Err(err),
        }
    }

    /// Gets the value of the element in the list as a builder. If the value is null or invalid
    /// in some way, it's reinitialized as a list with the specified element count.
    ///
    /// # Panics
    ///
    /// If the number of elements in the list causes the allocation size to exceed the max
    /// segment length, this function will panic. Use `V`'s `ELEMENT_SIZE` constant to
    /// check the max number of elements a list can contain of the value to check this ahead of
    /// time.
    #[inline]
    pub fn get_or_init(self, count: ElementCount) -> Builder<'b, V, T> {
        let builder = match self.into_ptr_builder().to_list_mut(Some(V::ELEMENT_SIZE)) {
            Ok(ptr) => ptr,
            Err((_, original)) => original.init_list(V::ELEMENT_SIZE, count),
        };
        List::new(builder)
    }

    /// Initializes the element as a list with the specified element count. This clears any
    /// pre-existing value.
    ///
    /// # Panics
    ///
    /// If the number of elements in the list causes the allocation size to exceed the max
    /// segment length, this function will panic. Use `V`'s `ELEMENT_SIZE` constant to
    /// check the max number of elements a list can contain of the value to check this ahead of
    /// time.
    ///
    /// This is only an issue for struct lists, so struct lists have a `try_init` method that
    /// returns an error on failure instead.
    #[inline]
    pub fn init(self, count: ElementCount) -> Builder<'b, V, T> {
        List::new(self.into_ptr_builder().init_list(V::ELEMENT_SIZE, count))
    }

    #[inline]
    pub fn try_set<F, E>(
        self,
        value: &Reader<V, impl InsertableInto<T>>,
        err_handler: F,
    ) -> Result<Builder<'b, V, T>, E>
    where
        F: FnMut(Error) -> ControlFlow<E, WriteNull>,
    {
        self.into_ptr_builder()
            .try_set_list(&value.repr, V::ELEMENT_SIZE, err_handler)
            .map(List::new)
            .map_err(|(err, _)| err)
    }

    #[inline]
    pub fn set(self, value: &Reader<V, impl InsertableInto<T>>) -> Builder<'b, V, T> {
        self.try_set(value, write_null).unwrap()
    }

    #[inline]
    pub fn clear(&mut self) {
        self.ptr_builder().clear()
    }
}

impl<'a, 'b, T> BuilderElement<'a, 'b, List<AnyStruct>, T>
where
    T: Table,
{
    #[inline]
    fn ptr_builder(&mut self) -> crate::ptr::PtrBuilder<T> {
        unsafe { self.list.as_mut().ptr_mut_unchecked(self.idx) }
    }

    #[inline]
    fn into_ptr_builder(self) -> crate::ptr::PtrBuilder<'b, T> {
        unsafe { self.list.as_mut().ptr_mut_unchecked(self.idx) }
    }

    /// Gets the value of the element in the list as a builder. If the value is null or invalid
    /// in some way, an empty list builder is returned.
    #[inline]
    pub fn get(self, expected_size: StructSize) -> Builder<'b, AnyStruct, T> {
        let expected = ElementSize::InlineComposite(expected_size);
        List::new(
            self.into_ptr_builder()
                .to_list_mut_or_empty(Some(expected)),
        )
    }

    #[inline]
    pub fn try_get_option(self, expected_size: StructSize) -> Result<Option<Builder<'b, AnyStruct, T>>> {
        let expected = ElementSize::InlineComposite(expected_size);
        match self.into_ptr_builder().to_list_mut(Some(expected)) {
            Ok(ptr) => Ok(Some(List::new(ptr))),
            Err((None, _)) => Ok(None),
            Err((Some(err), _)) => Err(err),
        }
    }

    /// Gets the value of the element in the list as a builder. If the value is null or invalid
    /// in some way, it's reinitialized as a list with the specified element count.
    ///
    /// # Panics
    ///
    /// If the number of elements in the list causes the allocation size to exceed the max
    /// segment length, this function will panic. Use `V`'s `ELEMENT_SIZE` constant to
    /// check the max number of elements a list can contain of the value to check this ahead of
    /// time.
    #[inline]
    pub fn get_or_init(self, size: StructSize, count: ElementCount) -> Builder<'b, AnyStruct, T> {
        let expected = ElementSize::InlineComposite(size);
        let builder = match self.into_ptr_builder().to_list_mut(Some(expected)) {
            Ok(ptr) => ptr,
            Err((_, original)) => original.init_list(expected, count),
        };
        List::new(builder)
    }

    /// Initializes the element as a list with the specified element count. This clears any
    /// pre-existing value.
    ///
    /// # Panics
    ///
    /// If the number of elements in the list causes the allocation size to exceed the max
    /// segment length, this function will panic. Use `V`'s `ELEMENT_SIZE` constant to
    /// check the max number of elements a list can contain of the value to check this ahead of
    /// time.
    ///
    /// This is only an issue for struct lists, so struct lists have a `try_init` method that
    /// returns an error on failure instead.
    #[inline]
    pub fn init(self, size: StructSize, count: ElementCount) -> Builder<'b, AnyStruct, T> {
        List::new(self.into_ptr_builder()
            .init_list(ElementSize::InlineComposite(size), count))
    }

    #[inline]
    pub fn try_set<F, E>(
        self,
        value: &Reader<AnyStruct, impl InsertableInto<T>>,
        desired_size: Option<StructSize>,
        err_handler: F,
    ) -> Result<Builder<'b, AnyStruct, T>, E>
    where
        F: FnMut(Error) -> ControlFlow<E, WriteNull>,
    {
        let copy_size = match desired_size {
            Some(size) => ElementSize::InlineComposite(size),
            None => value.repr.element_size(),
        };

        self.into_ptr_builder()
            .try_set_list(&value.repr, copy_size, err_handler)
            .map(List::new)
            .map_err(|(err, _)| err)
    }

    #[inline]
    pub fn set(self, value: &Reader<AnyStruct, impl InsertableInto<T>>, desired_size: Option<StructSize>) -> Builder<'b, AnyStruct, T> {
        self.try_set(value, desired_size, write_null).unwrap()
    }

    #[inline]
    pub fn clear(&mut self) {
        self.ptr_builder().clear()
    }
}

impl<'a, 'b, T> ReaderElement<'a, 'b, AnyList, T>
where
    T: Table,
{
    #[inline]
    fn ptr_reader(&self) -> crate::ptr::PtrReader<'a, T> {
        unsafe { self.list.as_ref().ptr_unchecked(self.idx) }
    }

    #[inline]
    fn empty_list(&self) -> any::ListReader<'a, T> {
        any::ListReader::from(ptr::Reader::empty(ElementSize::Pointer).imbue_from(self.list))
    }

    /// Returns whether this list element is a null pointer.
    #[inline]
    pub fn is_null(&self) -> bool {
        self.ptr_reader().is_null()
    }

    /// Returns the list value in this element, or an empty list if the list is null
    /// or an error occurs while reading.
    #[inline]
    pub fn get(&self) -> any::ListReader<'a, T> {
        match self.try_get_option() {
            Ok(Some(reader)) => reader,
            _ => self.empty_list(),
        }
    }

    /// Returns the list value in this element, or None if the list element is null or
    /// an error occurs while reading.
    #[inline]
    pub fn get_option(&self) -> Option<any::ListReader<'a, T>> {
        match self.try_get_option() {
            Ok(Some(reader)) => Some(reader),
            _ => None,
        }
    }

    /// Returns the list value in this element. If the element is null, this returns an
    /// empty list. If an error occurs while reading it is returned.
    #[inline]
    pub fn try_get(&self) -> Result<any::ListReader<'a, T>> {
        match self.try_get_option() {
            Ok(Some(reader)) => Ok(reader),
            Ok(None) => Ok(self.empty_list()),
            Err(err) => Err(err),
        }
    }

    /// Returns the list value in this element. If the element is null, this returns Ok(None).
    /// If an error occurs while reading it is returned.
    #[inline]
    pub fn try_get_option(&self) -> Result<Option<any::ListReader<'a, T>>> {
        match self.ptr_reader().to_list(None) {
            Ok(Some(reader)) => Ok(Some(any::ListReader::from(reader))),
            Ok(None) => Ok(None),
            Err(err) => Err(err),
        }
    }
}

impl<'a, 'b, T> BuilderElement<'a, 'b, AnyList, T>
where
    T: Table,
{
    #[inline]
    fn ptr_builder(&mut self) -> crate::ptr::PtrBuilder<T> {
        unsafe { self.list.as_mut().ptr_mut_unchecked(self.idx) }
    }

    #[inline]
    fn into_ptr_builder(self) -> crate::ptr::PtrBuilder<'b, T> {
        unsafe { self.list.as_mut().ptr_mut_unchecked(self.idx) }
    }

    /// Gets the value of the element in the list as a builder. If the value is null or invalid
    /// in some way, an empty list builder is returned.
    #[inline]
    pub fn get(self) -> any::ListBuilder<'b, T> {
        any::ListBuilder::from(self.into_ptr_builder().to_list_mut_or_empty(None))
    }

    #[inline]
    pub fn try_get_option(self) -> Result<Option<any::ListBuilder<'b, T>>> {
        match self.into_ptr_builder().to_list_mut(None) {
            Ok(ptr) => Ok(Some(any::ListBuilder::from(ptr))),
            Err((None, _)) => Ok(None),
            Err((Some(err), _)) => Err(err),
        }
    }

    /// Initializes the element as a list with the specified element count. This clears any
    /// pre-existing value.
    ///
    /// # Panics
    ///
    /// If the number of elements in the list causes the allocation size to exceed the max
    /// segment length, this function will panic. Use `V`'s `ELEMENT_SIZE` constant to
    /// check the max number of elements a list can contain of the value to check this ahead of
    /// time.
    #[inline]
    pub fn init(self, size: ElementSize, count: ElementCount) -> any::ListBuilder<'b, T> {
        any::ListBuilder::from(self.into_ptr_builder().init_list(size, count))
    }

    /// Initializes the element as a list with the specified element count. This clears any
    /// pre-existing value.
    ///
    /// If the number of struct elements is too large for a Cap'n Proto message, this returns
    /// an Err.
    #[inline]
    pub fn try_init(
        self,
        size: ElementSize,
        count: ElementCount,
    ) -> Result<any::ListBuilder<'b, T>> {
        self.into_ptr_builder()
            .try_init_list(size, count)
            .map(any::ListBuilder::from)
            .map_err(|(err, _)| err)
    }

    #[inline]
    pub fn try_set<F, E>(
        self,
        value: &any::ListReader<impl InsertableInto<T>>,
        err_handler: F,
    ) -> Result<any::ListBuilder<'b, T>, E>
    where
        F: FnMut(Error) -> ControlFlow<E, WriteNull>,
    {
        self.into_ptr_builder()
            .try_set_list(value.as_ref(), value.as_ref().element_size(), err_handler)
            .map(any::ListBuilder::from)
            .map_err(|(err, _)| err)
    }

    #[inline]
    pub fn set(self, value: &any::ListReader<impl InsertableInto<T>>) -> any::ListBuilder<'b, T> {
        self.try_set(value, write_null).unwrap()
    }

    #[inline]
    pub fn clear(&mut self) {
        self.ptr_builder().clear()
    }
}

impl<'a, 'b, T> ReaderElement<'a, 'b, Data, T>
where
    T: Table,
{
    #[inline]
    fn ptr_reader(&self) -> crate::ptr::PtrReader<'a, T> {
        unsafe { self.list.as_ref().ptr_unchecked(self.idx) }
    }

    /// Returns whether this list element is a null pointer.
    #[inline]
    pub fn is_null(&self) -> bool {
        self.ptr_reader().is_null()
    }

    /// Returns the list value in this element, or an empty list if the list is null
    /// or an error occurs while reading.
    #[inline]
    pub fn get(&self) -> data::Reader<'a> {
        match self.try_get_option() {
            Ok(Some(reader)) => reader,
            _ => data::Reader::empty(),
        }
    }

    /// Returns the list value in this element, or None if the list element is null or
    /// an error occurs while reading.
    #[inline]
    pub fn get_option(&self) -> Option<data::Reader<'a>> {
        match self.try_get_option() {
            Ok(Some(reader)) => Some(reader),
            _ => None,
        }
    }

    /// Returns the list value in this element. If the element is null, this returns an
    /// empty list. If an error occurs while reading it is returned.
    #[inline]
    pub fn try_get(&self) -> Result<data::Reader<'a>> {
        match self.try_get_option() {
            Ok(Some(reader)) => Ok(reader),
            Ok(None) => Ok(data::Reader::empty()),
            Err(err) => Err(err),
        }
    }

    /// Returns the list value in this element. If the element is null, this returns Ok(None).
    /// If an error occurs while reading it is returned.
    #[inline]
    pub fn try_get_option(&self) -> Result<Option<data::Reader<'a>>> {
        match self.ptr_reader().to_blob() {
            Ok(Some(reader)) => Ok(Some(data::Reader::from(reader))),
            Ok(None) => Ok(None),
            Err(err) => Err(err),
        }
    }
}

impl<'a, 'b, T> BuilderElement<'a, 'b, Data, T>
where
    T: Table,
{
    #[inline]
    fn ptr_builder(&mut self) -> crate::ptr::PtrBuilder<T> {
        unsafe { self.list.as_mut().ptr_mut_unchecked(self.idx) }
    }

    #[inline]
    fn into_ptr_builder(self) -> crate::ptr::PtrBuilder<'b, T> {
        unsafe { self.list.as_mut().ptr_mut_unchecked(self.idx) }
    }

    #[inline]
    pub fn get(self) -> data::Builder<'b> {
        match self.into_ptr_builder().to_blob_mut() {
            Ok(b) => b.into(),
            Err(_) => todo!(),
        }
    }

    #[inline]
    pub fn try_get_option(self) -> Result<Option<data::Builder<'b>>> {
        match self.into_ptr_builder().to_blob_mut() {
            Ok(b) => Ok(Some(b.into())),
            Err((None, _)) => Ok(None),
            Err((Some(e), _)) => Err(e),
        }
    }

    #[inline]
    pub fn init(self, count: ElementCount) -> data::Builder<'b> {
        self.into_ptr_builder().init_blob(count).into()
    }

    #[inline]
    pub fn set(self, value: &data::Reader) -> data::Builder<'b> {
        self.into_ptr_builder().set_blob(value.as_ref()).into()
    }

    #[inline]
    pub fn clear(&mut self) {
        self.ptr_builder().clear()
    }
}

impl<'a, 'b, T> ReaderElement<'a, 'b, Text, T>
where
    T: Table,
{
    #[inline]
    fn ptr_reader(&self) -> crate::ptr::PtrReader<'a, T> {
        unsafe { self.list.as_ref().ptr_unchecked(self.idx) }
    }

    /// Returns whether this list element is a null pointer.
    #[inline]
    pub fn is_null(&self) -> bool {
        self.ptr_reader().is_null()
    }

    /// Returns the list value in this element, or an empty list if the list is null
    /// or an error occurs while reading.
    #[inline]
    pub fn get(&self) -> text::Reader<'a> {
        match self.try_get_option() {
            Ok(Some(reader)) => reader,
            _ => text::Reader::empty(),
        }
    }

    /// Returns the list value in this element, or None if the list element is null or
    /// an error occurs while reading.
    #[inline]
    pub fn get_option(&self) -> Option<text::Reader<'a>> {
        match self.try_get_option() {
            Ok(Some(reader)) => Some(reader),
            _ => None,
        }
    }

    /// Returns the list value in this element. If the element is null, this returns an
    /// empty list. If an error occurs while reading it is returned.
    #[inline]
    pub fn try_get(&self) -> Result<text::Reader<'a>> {
        match self.try_get_option() {
            Ok(Some(reader)) => Ok(reader),
            Ok(None) => Ok(text::Reader::empty()),
            Err(err) => Err(err),
        }
    }

    /// Returns the list value in this element. If the element is null, this returns Ok(None).
    /// If an error occurs while reading it is returned.
    #[inline]
    pub fn try_get_option(&self) -> Result<Option<text::Reader<'a>>> {
        match self.ptr_reader().to_blob() {
            Ok(Some(b)) => text::Reader::new(b)
                .ok_or_else(|| Error::from(ErrorKind::TextNotNulTerminated))
                .map(Some),
            Ok(None) => Ok(None),
            Err(err) => Err(err),
        }
    }
}

impl<'a, 'b, T> BuilderElement<'a, 'b, Text, T>
where
    T: Table,
{
    #[inline]
    fn ptr_builder(&mut self) -> crate::ptr::PtrBuilder<T> {
        unsafe { self.list.as_mut().ptr_mut_unchecked(self.idx) }
    }

    #[inline]
    fn into_ptr_builder(self) -> crate::ptr::PtrBuilder<'b, T> {
        unsafe { self.list.as_mut().ptr_mut_unchecked(self.idx) }
    }

    #[inline]
    pub fn get(self) -> text::Builder<'b> {
        todo!()
    }

    #[inline]
    pub fn try_get_option(self) -> Result<Option<text::Builder<'b>>> {
        todo!()
    }

    #[inline]
    pub fn init(self, count: text::ByteCount) -> text::Builder<'b> {
        todo!()
    }

    #[inline]
    pub fn set(self, value: &text::Reader) -> text::Builder<'b> {
        todo!()
    }

    /// Set the text element to a copy of the given string.
    /// 
    /// # Panics
    /// 
    /// If the string is too large to fit in a Cap'n Proto message, this function will
    /// panic.
    #[inline]
    pub fn set_str(self, value: &str) -> text::Builder<'b> {
        self.try_set_str(value)
            .ok()
            .expect("str is too large to fit in a Cap'n Proto message")
    }

    #[inline]
    pub fn try_set_str(self, value: &str) -> Result<text::Builder<'b>, Self> {
        let len = u32::try_from(value.len() + 1).ok().and_then(text::ByteCount::new);
        let Some(len) = len else {
            return Err(self)
        };

        let mut builder = self.init(len);
        builder.as_bytes_mut().copy_from_slice(value.as_bytes());
        Ok(builder)
    }

    #[inline]
    pub fn clear(&mut self) {
        self.ptr_builder().clear()
    }
}

/// A helper used to provide element views into a list. Depending on the value type, this may simply
/// return the value itself, or an element view which can be used to access one of the other getters
/// or setters.
///
/// Some types have multiple ways of reading values which account for error handling or provide safe
/// defaults. For example, reading a pointer field can return an error, but many don't want to
/// handle that error and would accept a safe default. But to hide the error completely would be
/// taking it too far, since some might want to perform strict validation and error out if any
/// incorrect data is read. So we provide both.
///
/// But other types can't return errors and are simple arithmetic, so we don't bother returning a
/// view with multiple possible accessors, since really only one exists.
pub trait ListAccessable<T> {
    type View;

    unsafe fn get(list: T, index: u32) -> Self::View;
}

impl<'a, 'b, T: Table> ListAccessable<&'b Reader<'a, Self, T>> for () {
    type View = ();

    #[inline]
    unsafe fn get(_: &'b Reader<'a, Self, T>, _: u32) -> Self::View { () }
}

impl<'a, 'b, T: Table> ListAccessable<&'b mut Builder<'a, Self, T>> for () {
    type View = ();

    #[inline]
    unsafe fn get(_: &'b mut Builder<'a, Self, T>, _: u32) -> Self::View { () }
}

impl<'a, 'b, T: Table, V: FieldData> ListAccessable<&'b Reader<'a, Self, T>> for V {
    type View = V;

    #[inline]
    unsafe fn get(list: &'b Reader<'a, Self, T>, idx: u32) -> Self::View {
        list.as_ref().data_unchecked(idx)
    }
}

impl<'a, 'b, T: Table, V: FieldData> ListAccessable<&'b mut Builder<'a, Self, T>> for V {
    type View = BuilderElement<'a, 'b, Self, T>;

    #[inline]
    unsafe fn get(list: &'b mut Builder<'a, Self, T>, idx: u32) -> Self::View {
        Element { list, idx }
    }
}

impl<'a, 'b, T: Table, E: ty::Enum> ListAccessable<&'b Reader<'a, Self, T>> for Enum<E> {
    type View = EnumResult<E>;

    #[inline]
    unsafe fn get(list: &'b Reader<'a, Self, T>, idx: u32) -> Self::View {
        let value = list.as_ref().data_unchecked(idx);
        E::try_from(value)
    }
}

impl<'a, 'b, T: Table, E: ty::Enum> ListAccessable<&'b mut Builder<'a, Self, T>> for Enum<E> {
    type View = BuilderElement<'a, 'b, Self, T>;

    #[inline]
    unsafe fn get(list: &'b mut Builder<'a, Self, T>, idx: u32) -> Self::View {
        Element::new(list, idx)
    }
}

impl<'a, 'b, T: Table, S: ty::Struct> ListAccessable<&'b Reader<'a, Self, T>> for Struct<S> {
    type View = S::Reader<'a, T>;

    #[inline]
    unsafe fn get(list: &'b Reader<'a, Self, T>, idx: u32) -> Self::View {
        ty::StructReader::from_ptr(list.as_ref().struct_unchecked(idx))
    }
}

impl<'a, 'b, T: Table, S: ty::Struct> ListAccessable<&'b mut Builder<'a, Self, T>> for Struct<S> {
    type View = BuilderElement<'a, 'b, Self, T>;

    #[inline]
    unsafe fn get(list: &'b mut Builder<'a, Self, T>, idx: u32) -> Self::View {
        Element::new(list, idx)
    }
}

impl<'a, 'b, T: Table> ListAccessable<&'b Reader<'a, Self, T>> for AnyStruct {
    type View = any::StructReader<'b, T>;

    #[inline]
    unsafe fn get(list: &'b Reader<'a, Self, T>, idx: u32) -> Self::View {
        ty::StructReader::from_ptr(list.as_ref().struct_unchecked(idx))
    }
}

impl<'a, 'b, T: Table> ListAccessable<&'b mut Builder<'a, Self, T>> for AnyStruct {
    type View = BuilderElement<'a, 'b, AnyStruct, T>;

    #[inline]
    unsafe fn get(list: &'b mut Builder<'a, Self, T>, idx: u32) -> Self::View {
        Element::new(list, idx)
    }
}

impl<'a, 'b, T: Table, V: ty::ListValue> ListAccessable<&'b Reader<'a, Self, T>> for List<V> {
    type View = ReaderElement<'a, 'b, Self, T>;

    #[inline]
    unsafe fn get(list: &'b Reader<'a, Self, T>, idx: u32) -> Self::View {
        Element::new(list, idx)
    }
}

impl<'a, 'b, T: Table, V: ty::ListValue> ListAccessable<&'b mut Builder<'a, Self, T>> for List<V> {
    type View = BuilderElement<'a, 'b, Self, T>;

    #[inline]
    unsafe fn get(list: &'b mut Builder<'a, Self, T>, idx: u32) -> Self::View {
        Element::new(list, idx)
    }
}

impl<'a, 'b, T: Table> ListAccessable<&'b Reader<'a, Self, T>> for AnyList {
    type View = ReaderElement<'a, 'b, Self, T>;

    #[inline]
    unsafe fn get(list: &'b Reader<'a, Self, T>, idx: u32) -> Self::View {
        Element::new(list, idx)
    }
}

impl<'a, 'b, T: Table> ListAccessable<&'b mut Builder<'a, Self, T>> for AnyList {
    type View = BuilderElement<'a, 'b, Self, T>;

    #[inline]
    unsafe fn get(list: &'b mut Builder<'a, Self, T>, idx: u32) -> Self::View {
        Element::new(list, idx)
    }
}

impl<'a, 'b, T: Table> ListAccessable<&'b Reader<'a, Self, T>> for Data {
    type View = ReaderElement<'a, 'b, Self, T>;

    #[inline]
    unsafe fn get(list: &'b Reader<'a, Self, T>, idx: u32) -> Self::View {
        Element::new(list, idx)
    }
}

impl<'a, 'b, T: Table> ListAccessable<&'b mut Builder<'a, Self, T>> for Data {
    type View = BuilderElement<'a, 'b, Self, T>;

    #[inline]
    unsafe fn get(list: &'b mut Builder<'a, Self, T>, idx: u32) -> Self::View {
        Element::new(list, idx)
    }
}

impl<'a, 'b, T: Table> ListAccessable<&'b Reader<'a, Self, T>> for Text {
    type View = ReaderElement<'a, 'b, Self, T>;

    #[inline]
    unsafe fn get(list: &'b Reader<'a, Self, T>, idx: u32) -> Self::View {
        Element::new(list, idx)
    }
}

impl<'a, 'b, T: Table> ListAccessable<&'b mut Builder<'a, Self, T>> for Text {
    type View = BuilderElement<'a, 'b, Self, T>;

    #[inline]
    unsafe fn get(list: &'b mut Builder<'a, Self, T>, idx: u32) -> Self::View {
        Element::new(list, idx)
    }
}

impl<'a, 'b, T: Table> ListAccessable<&'b Reader<'a, Self, T>> for AnyPtr {
    type View = any::PtrReader<'a, T>;

    #[inline]
    unsafe fn get(list: &'b Reader<'a, Self, T>, idx: u32) -> Self::View {
        list.repr.ptr_unchecked(idx).into()
    }
}

impl<'a, 'b, T: Table> ListAccessable<&'b mut Builder<'a, Self, T>> for AnyPtr {
    type View = any::PtrBuilder<'b, T>;

    #[inline]
    unsafe fn get(list: &'b mut Builder<'a, Self, T>, idx: u32) -> Self::View {
        list.repr.ptr_mut_unchecked(idx).into()
    }
}

// Iterators. WARNING! HELL BELOW

/// Describes the conversion from an element reader to a value which isn't dependent
/// on the reader itself.
/// 
/// This is necessary since lending iterators don't exist in Rust yet, so for pointer
/// fields which would borrow the list for the element reader, we need to provide
/// a conversion ahead of time that properly reads the value.
/// 
/// For an infallible iterator that always returns a value (except for enums), even for
/// pointer fields that return errors, use the default iterator with the `InfalliblePtrs`
/// strategy.
/// 
/// For a fallible iterator like in capnproto-rust which return a result for errors while
/// reading, use the `Fallible` strategy.
/// 
/// Custom strategies can be used by providing a closure to `into_iter_by` which will be
/// called for every element in the list.
pub trait IterStrategy<T, E> {
    /// The resulting type derived from this strategy
    type Item;

    /// Applies the strategy to get the output
    fn get(&mut self, element: E) -> Self::Item;
}

impl<T, E, F, Output> IterStrategy<T, E> for F
where
    F: FnMut(E) -> Output,
{
    type Item = Output;

    #[inline]
    fn get(&mut self, element: E) -> Self::Item {
        (self)(element)
    }
}

macro_rules! infallible_strategies {
    ($ty:ty) => {
        impl IterStrategy<(), ()> for $ty {
            type Item = ();

            #[inline]
            fn get(&mut self, _: Self::Item) -> Self::Item {}
        }

        impl<V: FieldData> IterStrategy<V, V> for $ty {
            type Item = V;

            #[inline]
            fn get(&mut self, element: Self::Item) -> Self::Item { element }
        }

        impl<E: ty::Enum> IterStrategy<Enum<E>, EnumResult<E>> for $ty {
            type Item = EnumResult<E>;

            #[inline]
            fn get(&mut self, element: Self::Item) -> Self::Item { element }
        }

        impl<S: ty::Struct, R: ty::StructReader> IterStrategy<Struct<S>, R> for $ty {
            type Item = R;

            #[inline]
            fn get(&mut self, element: Self::Item) -> Self::Item { element }
        }

        impl<'a, T: Table> IterStrategy<AnyStruct, any::StructReader<'a, T>> for $ty {
            type Item = any::StructReader<'a, T>;

            #[inline]
            fn get(&mut self, element: Self::Item) -> Self::Item { element }
        }

        impl<'a, T: Table> IterStrategy<AnyPtr, any::PtrReader<'a, T>> for $ty {
            type Item = any::PtrReader<'a, T>;

            #[inline]
            fn get(&mut self, element: Self::Item) -> Self::Item { element }
        }
    };
}

/// An infallible iteration strategy for most element types. This is the default type used
/// for iteration. Pointer fields that return null or an error will instead return a default
/// value. Lists of enum will iterate over results with errors indicating unknown enumerants.
pub struct InfalliblePtrs;

infallible_strategies!(InfalliblePtrs);

impl<'a, 'b, T, V> IterStrategy<List<V>, ReaderElement<'a, 'b, List<V>, T>> for InfalliblePtrs
where
    T: Table,
    V: ty::DynListValue,
{
    type Item = Reader<'a, V, T>;

    fn get(&mut self, element: ReaderElement<'a, 'b, List<V>, T>) -> Self::Item {
        element.get()
    }
}

impl<'a, 'b, T> IterStrategy<AnyList, ReaderElement<'a, 'b, AnyList, T>> for InfalliblePtrs
where
    T: Table,
{
    type Item = any::ListReader<'a, T>;

    fn get(&mut self, element: ReaderElement<'a, 'b, AnyList, T>) -> Self::Item {
        element.get()
    }
}

impl<'a, 'b, T> IterStrategy<Data, ReaderElement<'a, 'b, Data, T>> for InfalliblePtrs
where
    T: Table,
{
    type Item = data::Reader<'a>;

    fn get(&mut self, element: ReaderElement<'a, 'b, Data, T>) -> Self::Item {
        element.get()
    }
}

impl<'a, 'b, T> IterStrategy<Text, ReaderElement<'a, 'b, Text, T>> for InfalliblePtrs
where
    T: Table,
{
    type Item = text::Reader<'a>;

    fn get(&mut self, element: ReaderElement<'a, 'b, Text, T>) -> Self::Item {
        element.get()
    }
}

/// A fallible iteration strategy for many element types. This matches the behavior of capnproto-rust
/// by returning results for pointer reads.
pub struct Fallible;

infallible_strategies!(Fallible);

impl<'a, 'b, T, V> IterStrategy<List<V>, ReaderElement<'a, 'b, List<V>, T>> for Fallible
where
    T: Table,
    V: ty::DynListValue,
{
    type Item = Result<Reader<'a, V, T>>;

    fn get(&mut self, element: ReaderElement<'a, 'b, List<V>, T>) -> Self::Item {
        element.try_get()
    }
}

impl<'a, 'b, T> IterStrategy<AnyList, ReaderElement<'a, 'b, AnyList, T>> for Fallible
where
    T: Table,
{
    type Item = Result<any::ListReader<'a, T>>;

    fn get(&mut self, element: ReaderElement<'a, 'b, AnyList, T>) -> Self::Item {
        element.try_get()
    }
}

impl<'a, 'b, T> IterStrategy<Data, ReaderElement<'a, 'b, Data, T>> for Fallible
where
    T: Table,
{
    type Item = Result<data::Reader<'a>>;

    fn get(&mut self, element: ReaderElement<'a, 'b, Data, T>) -> Self::Item {
        element.try_get()
    }
}

impl<'a, 'b, T> IterStrategy<Text, ReaderElement<'a, 'b, Text, T>> for Fallible
where
    T: Table,
{
    type Item = Result<text::Reader<'a>>;

    fn get(&mut self, element: ReaderElement<'a, 'b, Text, T>) -> Self::Item {
        element.try_get()
    }
}

/// An iterator through a list that maps the element view into a new output.
pub struct Iter<'a, V, S = InfalliblePtrs, T: Table = rpc::Empty> {
    list: Reader<'a, V, T>,
    range: Range<u32>,
    strategy: S,
}

impl<'a, V, S, T, Item> Iterator for Iter<'a, V, S, T>
where
    V: for<'b> ListAccessable<&'b Reader<'a, V, T>>,
    S: for<'b> IterStrategy<V, ElementReader<'a, 'b, V, T>, Item = Item>,
    T: Table,
{
    type Item = Item;

    #[inline]
    fn next(&mut self) -> Option<Self::Item> {
        let element = unsafe { self.list.at_unchecked(self.range.next()?) };
        Some(self.strategy.get(element))
    }
    #[inline]
    fn size_hint(&self) -> (usize, Option<usize>) {
        self.range.size_hint()
    }
}

impl<'a, V, S, T, Item> DoubleEndedIterator for Iter<'a, V, S, T>
where
    V: for<'b> ListAccessable<&'b Reader<'a, V, T>>,
    S: for<'b> IterStrategy<V, <V as ListAccessable<&'b Reader<'a, V, T>>>::View, Item = Item>,
    T: Table,
{
    #[inline]
    fn next_back(&mut self) -> Option<Self::Item> {
        let element = unsafe { self.list.at_unchecked(self.range.next_back()?) };
        Some(self.strategy.get(element))
    }
}

impl<'a, V, T> Reader<'a, V, T>
where
    T: Table,
{
    pub fn into_iter_by<S>(self, strat: S) -> Iter<'a, V, S, T> {
        let range = 0..self.len();
        Iter { list: self, range, strategy: strat }
    }
}

impl<'a, V, T, Item> IntoIterator for Reader<'a, V, T>
where
    V: for<'b> ListAccessable<&'b Reader<'a, V, T>>,
    T: Table,
    InfalliblePtrs: for<'lb> IterStrategy<V, ElementReader<'a, 'lb, V, T>, Item = Item>,
{
    type IntoIter = Iter<'a, V, InfalliblePtrs, T>;
    type Item = Item;

    fn into_iter(self) -> Self::IntoIter {
        self.into_iter_by(InfalliblePtrs)
    }
}