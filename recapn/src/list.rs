//! Types for typed Cap'n Proto lists.
//!
//! Where other libraries would implement distinct types for different types of lists,
//! `recapn` lists are different because all lists are represented as one type: `List`.
//! Like fields, we use wrapper types and indirection to implement distinct accessors
//! for different types of values. For example,
//!
//! A list of primitives is a List<V> where V is the primitive value.
//! A list of enums is a List<field::Enum<T>> where T is the enum type.
//! A list of structs is a List<field::Struct<T>> where T is the struct type.
//! A list of lists is a List<List<T>> where T is the inner list type.
//!

use crate::alloc::ElementCount;
use crate::any::{AnyStruct, self};
use crate::field::{Enum, Struct};
use crate::ptr::internal::FieldData;
use crate::ptr::{CapableReader, WriteNull};
use crate::rpc::{self, Table};
use crate::{ty, Error, Family, IntoFamily, Result};
use core::convert::Infallible;
use core::marker::PhantomData;
use core::ops::ControlFlow;

pub use crate::ptr::ElementSize;

mod internal {
    use crate::ptr::Capable;
    use crate::ptr::internal::FieldData;

    pub trait DataAccessable: Capable {
        unsafe fn data_unchecked<D: FieldData>(&self, index: u32) -> D;
    }

    pub trait DataAccessableMut: DataAccessable {
        unsafe fn set_data_unchecked<D: FieldData>(&mut self, index: u32, value: D);
    }
}

use internal::{DataAccessable, DataAccessableMut};

pub mod ptr {
    pub use crate::ptr::{ListBuilder as Builder, ListReader as Reader};
}

pub type Reader<'a, V, T> = List<V, ptr::Reader<'a, T>>;
pub type Builder<'a, V, T> = List<V, ptr::Builder<'a, T>>;

/// A Cap'n Proto list.
pub struct List<V, T = Family> {
    repr: T,
    v: PhantomData<fn(u32) -> V>,
}

impl<V, T> List<V, T> {
    fn new(repr: T) -> Self {
        Self {
            repr,
            v: PhantomData,
        }
    }
}

impl<V, T> crate::internal::Sealed for List<V, T> {}

impl<T: Clone, V> Clone for List<V, T> {
    fn clone(&self) -> Self {
        Self {
            repr: self.repr.clone(),
            v: PhantomData,
        }
    }
}

impl<V, T> IntoFamily for List<V, T> {
    type Family = List<V, Family>;
}

impl<'a, V, T: Table> Reader<'a, V, T> {
    /// Gets the length of the list
    pub fn len(&self) -> u32 {
        self.repr.len()
    }

    /// Get the element at the specified index, or None if the index is out of range.
    #[inline]
    pub fn at<'b>(&'b self, index: u32) -> Option<V::View>
    where
        V: ListElement<&'b ptr::Reader<'a, T>>,
    {
        (index < self.len()).then(|| unsafe { self.at_unchecked(index) })
    }

    /// Get the element at the specified index without bounds checks.
    #[inline]
    pub unsafe fn at_unchecked<'b>(&'b self, index: u32) -> V::View
    where
        V: ListElement<&'b ptr::Reader<'a, T>>,
    {
        V::get(&self.repr, index)
    }
}

impl<'a, V, T: Table> Builder<'a, V, T> {
    /// Gets the length of the list
    pub fn len(&self) -> u32 {
        self.repr.len()
    }

    /// Gets a read-only view of the element at the specified index, or None if the index is out of range.
    #[inline]
    pub fn at<'b>(&'b self, index: u32) -> Option<V::View>
    where
        V: ListElement<&'b ptr::Builder<'a, T>>,
    {
        (index < self.len()).then(|| unsafe { self.at_unchecked(index) })
    }

    /// Gets a read-only view of the element at the specified index without bounds checks.
    #[inline]
    pub unsafe fn at_unchecked<'b>(&'b self, index: u32) -> V::View
    where
        V: ListElement<&'b ptr::Builder<'a, T>>,
    {
        V::get(&self.repr, index)
    }

    /// Gets a mutable view of the element at the specified index, or None if the index is out of range.
    pub fn at_mut<'b>(&'b mut self, index: u32) -> Option<V::View>
    where
        V: ListElement<&'b mut ptr::Builder<'a, T>>,
    {
        (index < self.len()).then(move || unsafe { self.at_mut_unchecked(index) })
    }

    /// Gets a mutable view of the element at the specified index without bounds checks.
    pub unsafe fn at_mut_unchecked<'b>(&'b mut self, index: u32) -> V::View
    where
        V: ListElement<&'b mut ptr::Builder<'a, T>>,
    {
        V::get(&mut self.repr, index)
    }
}

/// A checked index view into a List.
pub struct Element<V, T> {
    repr: T,
    index: u32,
    v: PhantomData<fn(u32) -> V>,
}

/// An element in a list reader
pub type ReaderElementRef<'a, 'b, V, T = rpc::Empty> = Element<V, &'b ptr::Reader<'a, T>>;
/// An element in a list builder with a read-only borrow
pub type BuilderElementRef<'a, 'b, V, T = rpc::Empty> = Element<V, &'b ptr::Builder<'a, T>>;
/// An element in a list builder with a mutable borrow
pub type BuilderElementMut<'a, 'b, V, T = rpc::Empty> = Element<V, &'b mut ptr::Builder<'a, T>>;

impl<'b, T: DataAccessableMut, V: FieldData> Element<V, &'b mut T> {
    #[inline]
    pub fn get(&self) -> V {
        unsafe { self.repr.data_unchecked(self.index) }
    }
    #[inline]
    pub fn set(&mut self, value: V) {
        unsafe { self.repr.set_data_unchecked(self.index, value) }
    }
}

impl<'b, T: DataAccessableMut> Element<f32, &'b mut T> {
    #[inline]
    pub fn get(&self) -> f32 {
        let bits = unsafe { self.repr.data_unchecked(self.index) };
        f32::from_bits(bits)
    }
    #[inline]
    pub fn set(&mut self, value: f32) {
        let bits = value.to_bits();
        unsafe { self.repr.set_data_unchecked(self.index, bits) }
    }
    #[inline]
    /// Canonicalizes NaN values by blowing away the NaN payload.
    pub fn set_canonical(&mut self, value: f32) {
        if value.is_nan() {
            unsafe { self.repr.set_data_unchecked(self.index, 0x7fc00000u32) }
        } else {
            self.set(value)
        }
    }
}

impl<'b, T: DataAccessableMut> Element<f64, &'b mut T> {
    #[inline]
    pub fn get(&self) -> f64 {
        let bits = unsafe { self.repr.data_unchecked(self.index) };
        f64::from_bits(bits)
    }
    #[inline]
    pub fn set(&mut self, value: f64) {
        let bits = value.to_bits();
        unsafe { self.repr.set_data_unchecked(self.index, bits) }
    }
    /// Canonicalizes NaN values by blowing away the NaN payload.
    #[inline]
    pub fn set_canonical(&mut self, value: f64) {
        if value.is_nan() {
            unsafe {
                self.repr
                    .set_data_unchecked(self.index, 0x7ff8000000000000u64)
            }
        } else {
            self.set(value)
        }
    }
}

impl<'b, T: DataAccessableMut, E: ty::Enum> Element<Enum<E>, &'b mut T> {
    #[inline]
    pub fn get(&self) -> E {
        let value = unsafe { self.repr.data_unchecked::<u16>(self.index) };
        E::from(value)
    }
    #[inline]
    pub fn set(&mut self, value: E) {
        let value = value.into();
        unsafe { self.repr.set_data_unchecked::<u16>(self.index, value) }
    }
}

macro_rules! list_of_list_reader_accessors {
    ($ptrlt:lifetime, $brrwlt:lifetime, $rtrnlt:lifetime, $table:ident, $repr:ty) => {
        impl<$ptrlt, $brrwlt, $table, V> Element<List<V>, $repr>
        where
            $table: Table,
            V: ty::ListValue,
        {
            #[inline]
            fn ptr_reader(&self) -> crate::ptr::PtrReader<$rtrnlt, $table> {
                unsafe { self.repr.ptr_unchecked(self.index) }
            }

            #[inline]
            fn empty_list(&self) -> Reader<$rtrnlt, V, $table> {
                List::new(ptr::Reader::empty().imbue(self.repr.clone_table_reader()))
            }

            /// Returns whether this list element is a null pointer.
            #[inline]
            pub fn is_null(&self) -> bool {
                self.ptr_reader().is_null()
            }

            /// Returns the list value in this element, or an empty list if the list is null
            /// or an error occurs while reading.
            #[inline]
            pub fn get(&self) -> Reader<$rtrnlt, V, $table> {
                match self.try_get_option() {
                    Ok(Some(reader)) => reader,
                    _ => self.empty_list(),
                }
            }

            /// Returns the list value in this element, or None if the list element is null or
            /// an error occurs while reading.
            #[inline]
            pub fn get_option(&self) -> Option<Reader<$rtrnlt, V, $table>> {
                match self.try_get_option() {
                    Ok(Some(reader)) => Some(reader),
                    _ => None,
                }
            }

            /// Returns the list value in this element. If the element is null, this returns an
            /// empty list. If an error occurs while reading it is returned.
            #[inline]
            pub fn try_get(&self) -> Result<Reader<$rtrnlt, V, $table>> {
                match self.try_get_option() {
                    Ok(Some(reader)) => Ok(reader),
                    Ok(None) => Ok(self.empty_list()),
                    Err(err) => Err(err),
                }
            }

            /// Returns the list value in this element. If the element is null, this returns Ok(None).
            /// If an error occurs while reading it is returned.
            #[inline]
            pub fn try_get_option(&self) -> Result<Option<Reader<$rtrnlt, V, $table>>> {
                match self.ptr_reader().to_list(Some(V::ELEMENT_SIZE.into())) {
                    Ok(Some(reader)) => Ok(Some(List::new(reader))),
                    Ok(None) => Ok(None),
                    Err(err) => Err(err),
                }
            }
        }
    };
}

// All accessor methods return Lists with the lifetime of 'a
list_of_list_reader_accessors!('a, 'b, 'a, T, &'b ptr::Reader<'a, T>);
// All accessor methods return Lists with the lifetime of 'b
list_of_list_reader_accessors!('a, 'b, 'b, T, &'b ptr::Builder<'a, T>);
// All accessor methods return Lists with an infered lifetime based on the accessor's borrow of
// the element
list_of_list_reader_accessors!('a, 'b, '_, T, &'b mut ptr::Builder<'a, T>);

impl<'a, 'b, T, V> Element<List<V>, &'b mut ptr::Builder<'a, T>>
where
    T: Table,
    V: ty::ListValue,
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
    pub fn by_ref(&mut self) -> Element<List<V>, &mut ptr::Builder<'a, T>> {
        Element {
            repr: &mut *self.repr,
            index: self.index,
            v: PhantomData,
        }
    }

    #[inline]
    fn ptr_builder(&mut self) -> crate::ptr::PtrBuilder<T> {
        unsafe { self.repr.ptr_mut_unchecked(self.index) }
    }

    #[inline]
    fn into_ptr_builder(self) -> crate::ptr::PtrBuilder<'b, T> {
        unsafe { self.repr.ptr_mut_unchecked(self.index) }
    }

    /// Gets the value of the element in the list as a builder. If the value is null or invalid
    /// in some way, an empty list builder is returned.
    #[inline]
    pub fn get_mut(self) -> Builder<'b, V, T> {
        List::new(
            self.into_ptr_builder()
                .to_list_mut_or_empty(Some(V::ELEMENT_SIZE)),
        )
    }

    #[inline]
    pub fn try_get_mut_option(self) -> Result<Option<Builder<'b, V, T>>> {
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
    pub fn get_mut_or_init(self, count: ElementCount) -> Builder<'b, V, T> {
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
        value: &Reader<V, impl Table>,
        err_handler: F,
    ) -> Result<Builder<'b, V, T>, E>
    where
        F: FnMut(Error) -> ControlFlow<E, WriteNull>,
    {
        self.into_ptr_builder()
            .try_set_list(&value.repr, false, err_handler)
            .map(List::new)
    }

    #[inline]
    pub fn set(self, value: &Reader<V, impl Table>) -> Builder<'b, V, T> {
        List::new(self.into_ptr_builder().set_list(&value.repr, false))
    }

    #[inline]
    pub fn clear(&mut self) {
        self.ptr_builder().clear()
    }
}

impl<'a, 'b, S, T> Element<List<Struct<S>>, &'b mut ptr::Builder<'a, T>>
where
    S: ty::Struct,
    T: Table,
{
    /// Initializes the element as a list with the specified element count. This clears any
    /// pre-existing value.
    ///
    /// If the number of struct elements is too large for a Cap'n Proto message, this returns
    /// an Err.
    #[inline]
    pub fn try_init(&mut self, count: ElementCount) -> Result<Builder<Struct<S>, T>> {
        self.ptr_builder()
            .try_init_list(<Struct<S> as ty::ListValue>::ELEMENT_SIZE, count)
            .map(List::new)
            .map_err(|(err, _)| err)
    }
}

impl<'a, 'b, S, T> Element<Struct<S>, &'b mut ptr::Builder<'a, T>>
where
    S: ty::Struct,
    T: Table,
{
    /// Borrows an element, rather than consuming it.
    ///
    /// Allows calling multiple builder methods without consuming the element.
    #[inline]
    pub fn by_ref(&mut self) -> Element<Struct<S>, &mut ptr::Builder<'a, T>> {
        Element {
            repr: &mut *self.repr,
            index: self.index,
            v: PhantomData,
        }
    }

    #[inline]
    pub fn get(&self) -> S::Reader<'_, T> {
        ty::StructReader::from_ptr(unsafe { self.repr.struct_unchecked(self.index) })
    }

    #[inline]
    pub fn get_mut(self) -> S::Builder<'b, T> {
        ty::StructBuilder::from_ptr(unsafe { self.repr.struct_mut_unchecked(self.index) })
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
    pub fn set_with_caveats(&mut self, reader: &S::Reader<'_, impl Table>) {
        self.try_set_with_caveats::<Infallible, _>(reader, |_| ControlFlow::Continue(WriteNull))
            .unwrap()
    }

    #[inline]
    pub fn try_set_with_caveats<E, F>(
        &mut self,
        reader: &S::Reader<'_, impl Table>,
        err_handler: F,
    ) -> Result<(), E>
    where
        F: FnMut(Error) -> ControlFlow<E, WriteNull>,
    {
        todo!()
    }
}

impl<'a, 'b, T> Element<AnyStruct, &'b mut ptr::Builder<'a, T>>
where
    T: Table,
{
    /// Borrows an element, rather than consuming it.
    ///
    /// Allows calling multiple builder methods without consuming the element.
    #[inline]
    pub fn by_ref(&mut self) -> Element<AnyStruct, &mut ptr::Builder<'a, T>> {
        Element {
            repr: &mut *self.repr,
            index: self.index,
            v: PhantomData,
        }
    }

    #[inline]
    pub fn get(&self) -> any::StructReader<'_, T> {
        ty::StructReader::from_ptr(unsafe { self.repr.struct_unchecked(self.index) })
    }

    #[inline]
    pub fn get_mut(self) -> any::StructBuilder<'b, T> {
        todo!()
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
        self.try_set_with_caveats::<Infallible, _>(reader, |_| ControlFlow::Continue(WriteNull))
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
pub trait ListElement<Repr>: crate::internal::Sealed {
    /// The resulting view of this value
    type View;

    /// Gets the value
    unsafe fn get(repr: Repr, index: u32) -> Self::View;
}

impl<Repr> ListElement<Repr> for () {
    type View = ();

    #[inline]
    unsafe fn get(_: Repr, _: u32) -> Self::View {
        ()
    }
}

impl<'b, T: DataAccessable, V: FieldData> ListElement<&'b T> for V {
    type View = V;

    #[inline]
    unsafe fn get(repr: &'b T, index: u32) -> Self::View {
        repr.data_unchecked(index)
    }
}

impl<'b, T: DataAccessableMut, V: FieldData> ListElement<&'b mut T> for V {
    type View = Element<V, &'b mut T>;

    #[inline]
    unsafe fn get(repr: &'b mut T, index: u32) -> Self::View {
        Element {
            repr,
            index,
            v: PhantomData,
        }
    }
}

impl crate::internal::Sealed for f32 {}
impl<'b, T: DataAccessable> ListElement<&'b T> for f32 {
    type View = Self;

    #[inline]
    unsafe fn get(repr: &'b T, index: u32) -> Self::View {
        let bits = repr.data_unchecked(index);
        f32::from_bits(bits)
    }
}

impl<'b, T: DataAccessableMut> ListElement<&'b mut T> for f32 {
    type View = Element<Self, &'b mut T>;

    #[inline]
    unsafe fn get(repr: &'b mut T, index: u32) -> Self::View {
        Element {
            repr,
            index,
            v: PhantomData,
        }
    }
}

impl crate::internal::Sealed for f64 {}
impl<'b, T: DataAccessable> ListElement<&'b T> for f64 {
    type View = Self;

    #[inline]
    unsafe fn get(repr: &'b T, index: u32) -> Self::View {
        let bits = repr.data_unchecked(index);
        f64::from_bits(bits)
    }
}

impl<'b, T: DataAccessableMut> ListElement<&'b mut T> for f64 {
    type View = Element<Self, &'b mut T>;

    #[inline]
    unsafe fn get(repr: &'b mut T, index: u32) -> Self::View {
        Element {
            repr,
            index,
            v: PhantomData,
        }
    }
}

impl<'b, T: DataAccessable, E: ty::Enum> ListElement<&'b T> for Enum<E> {
    type View = E;

    #[inline]
    unsafe fn get(repr: &'b T, index: u32) -> Self::View {
        let value = repr.data_unchecked(index);
        E::from(value)
    }
}

impl<'b, T: DataAccessableMut, E: ty::Enum> ListElement<&'b mut T> for Enum<E> {
    type View = Element<Self, &'b mut T>;

    #[inline]
    unsafe fn get(repr: &'b mut T, index: u32) -> Self::View {
        Element {
            repr,
            index,
            v: PhantomData,
        }
    }
}

impl<Repr, V> ListElement<Repr> for List<V> {
    type View = Element<Self, Repr>;

    #[inline]
    unsafe fn get(repr: Repr, index: u32) -> Self::View {
        Element {
            repr,
            index,
            v: PhantomData,
        }
    }
}

impl<'a, 'b, T, S> ListElement<&'b ptr::Reader<'a, T>> for Struct<S>
where
    T: rpc::Table,
    S: ty::Struct,
{
    type View = S::Reader<'a, T>;

    #[inline]
    unsafe fn get(repr: &'b ptr::Reader<'a, T>, index: u32) -> Self::View {
        ty::StructReader::from_ptr(unsafe { repr.struct_unchecked(index) })
    }
}

impl<'a, 'b, T, S> ListElement<&'b ptr::Builder<'a, T>> for Struct<S>
where
    T: rpc::Table,
    S: ty::Struct,
{
    type View = S::Reader<'b, T>;

    #[inline]
    unsafe fn get(repr: &'b ptr::Builder<'a, T>, index: u32) -> Self::View {
        ty::StructReader::from_ptr(unsafe { repr.struct_unchecked(index) })
    }
}

impl<'a, 'b, T, S> ListElement<&'b mut ptr::Builder<'a, T>> for Struct<S>
where
    T: rpc::Table,
    S: ty::Struct,
{
    type View = Element<Self, &'b mut ptr::Builder<'a, T>>;

    #[inline]
    unsafe fn get(repr: &'b mut ptr::Builder<'a, T>, index: u32) -> Self::View {
        Element {
            repr,
            index,
            v: PhantomData,
        }
    }
}

impl<'a, 'b, T> ListElement<&'b ptr::Reader<'a, T>> for AnyStruct
where
    T: rpc::Table,
{
    type View = any::StructReader<'a, T>;

    #[inline]
    unsafe fn get(repr: &'b ptr::Reader<'a, T>, index: u32) -> Self::View {
        ty::StructReader::from_ptr(unsafe { repr.struct_unchecked(index) })
    }
}

impl<'a, 'b, T> ListElement<&'b ptr::Builder<'a, T>> for AnyStruct
where
    T: rpc::Table,
{
    type View = any::StructReader<'b, T>;

    #[inline]
    unsafe fn get(repr: &'b ptr::Builder<'a, T>, index: u32) -> Self::View {
        ty::StructReader::from_ptr(unsafe { repr.struct_unchecked(index) })
    }
}

impl<'a, 'b, T> ListElement<&'b mut ptr::Builder<'a, T>> for AnyStruct
where
    T: rpc::Table,
{
    type View = Element<Self, &'b mut ptr::Builder<'a, T>>;

    #[inline]
    unsafe fn get(repr: &'b mut ptr::Builder<'a, T>, index: u32) -> Self::View {
        Element {
            repr,
            index,
            v: PhantomData,
        }
    }
}
