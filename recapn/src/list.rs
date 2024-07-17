//! A fixed-size, typed, list of values in a Cap'n Proto message.
//!
//! Where other libraries would implement distinct types for different types of lists,
//! `recapn` lists are different because all lists are represented as one type: `List`.
//! Like fields, we use wrapper types and indirection to implement distinct accessors
//! for different types of values. For example,
//!
//! * A list of primitives is a `List<V>` where V is the primitive value.
//! * A list of enums is a `List<T>` where T is the enum type.
//! * A list of structs is a `List<T>` where T is the struct type.
//! * A list of lists is a `List<List<T>>` where T is the inner list type.

use crate::any::{self, AnyList, AnyPtr, AnyStruct};
use crate::data::{self, Data};
use crate::ptr::{CopySize, ErrorHandler, IgnoreErrors};
use crate::text::{self, Text};
use crate::internal::Sealed;
use crate::ptr::{Data as FieldData, PtrElementSize, StructSize};
use crate::rpc::{self, Capable, InsertableInto, Table};
use crate::ty::{self, kind, EnumResult};
use crate::{Error, Family, IntoFamily, Result, ErrorKind};

use core::convert::TryFrom;
use core::marker::PhantomData;
use core::ops::Range;

pub use crate::ptr::{ElementSize, ElementCount};

pub mod ptr {
    pub use crate::ptr::{
        ListBuilder as Builder,
        ListReader as Reader,
        PtrElementSize as ElementSize,
    };
}

pub struct TooManyElementsError(pub(crate) ());

pub type Reader<'p, V, T = rpc::Empty> = List<V, ptr::Reader<'p, T>>;
pub type Builder<'p, V, T = rpc::Empty> = List<V, ptr::Builder<'p, T>>;

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
        match ptr.try_get_as::<V>() {
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
    pub fn try_at<'b>(&'b self, index: u32) -> Option<ElementReader<'a, 'b, T, V>>
    where
        V: ListAccessable<&'b ptr::Reader<'a, T>>,
    {
        (index < self.len()).then(|| unsafe { self.at_unchecked(index) })
    }

    /// Get the element at the specified index, or None if the index is out of range.
    #[inline]
    pub fn at<'b>(&'b self, index: u32) -> ElementReader<'a, 'b, T, V>
    where
        V: ListAccessable<&'b ptr::Reader<'a, T>>,
    {
        self.try_at(index).expect("index out of bounds")
    }

    /// Get the element at the specified index without bounds checks.
    #[inline]
    pub unsafe fn at_unchecked<'b>(&'b self, index: u32) -> ElementReader<'a, 'b, T, V>
    where
        V: ListAccessable<&'b ptr::Reader<'a, T>>,
    {
        V::get(self.as_ref(), index)
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

impl<'p, V, T: Table> Builder<'p, V, T> {
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

    #[inline]
    pub fn try_copy_from<E>(
        &mut self,
        other: Reader<'_, V, impl InsertableInto<T>>,
        err_handler: E,
    ) -> Result<(), E::Error>
    where
        E: ErrorHandler,
    {
        self.as_mut().try_copy_from(other.as_ref(), err_handler)
    }

    /// Gets a mutable view of the element at the specified index, or None if the index is out of range.
    #[inline]
    pub fn try_at<'b>(&'b mut self, index: u32) -> Option<ElementBuilder<'b, 'p, T, V>>
    where
        V: ListAccessable<&'b mut ptr::Builder<'p, T>>,
    {
        (index < self.len().get()).then(move || unsafe { self.at_unchecked(index) })
    }

    /// Get the element at the specified index, or panics if out of range.
    #[inline]
    pub fn at<'b>(&'b mut self, index: u32) -> ElementBuilder<'b, 'p, T, V>
    where
        V: ListAccessable<&'b mut ptr::Builder<'p, T>>,
    {
        self.try_at(index).expect("index out of bounds")
    }

    /// Gets a mutable view of the element at the specified index without bounds checks.
    #[inline]
    pub unsafe fn at_unchecked<'b>(&'b mut self, index: u32) -> ElementBuilder<'b, 'p, T, V>
    where
        V: ListAccessable<&'b mut ptr::Builder<'p, T>>,
    {
        V::get(self.as_mut(), index)
    }

    #[inline]
    pub fn try_into_element(self, index: u32) -> Result<ElementOwner<'p, T, V>, Self>
    where
        V: ListAccessable<ptr::Builder<'p, T>>,
    {
        if index < self.len().get() {
            Ok(unsafe { self.into_element_unchecked(index) })
        } else {
            Err(self)
        }
    }

    #[inline]
    pub fn into_element(self, index: u32) -> ElementOwner<'p, T, V>
    where
        V: ListAccessable<ptr::Builder<'p, T>>,
    {
        self.try_into_element(index).ok().expect("index out of bounds")
    }

    #[inline]
    pub unsafe fn into_element_unchecked(self, index: u32) -> ElementOwner<'p, T, V>
    where
        V: ListAccessable<ptr::Builder<'p, T>>,
    {
        V::get(self.repr, index)
    }
}

/// An element in a list reader
pub type ElementReader<'b, 'p, T, V> =
    <V as ListAccessable<&'b ptr::Reader<'p, T>>>::View;
/// An element in a list builder with a mutable borrow
pub type ElementBuilder<'b, 'p, T, V> =
    <V as ListAccessable<&'b mut ptr::Builder<'p, T>>>::View;
pub type ElementOwner<'p, T, V> =
    <V as ListAccessable<ptr::Builder<'p, T>>>::View;

/// A "type kind" constraining trait that specializes list value type kinds.
pub trait TypeKind {
    type Kind;
}

impl<T> TypeKind for T
where
    T: ty::TypeKind,
{
    type Kind = T::Kind;
}

/// A marker trait for generic pointer element types. Note this does *not* include `AnyPtr` since it
/// has specialized implementations of `ListAccessable` that return the `any::PtrBuilder` itself.
pub trait Ptr: TypeKind<Kind = kind::PtrList<Self>> {}

impl<T: Ptr + 'static> ty::ListValue for kind::PtrList<T> {
    const ELEMENT_SIZE: ElementSize = ElementSize::Pointer;
}

/// A checked index view into a List.
pub struct Element<V, T> {
    v: PhantomData<fn() -> V>,
    list: T,
    idx: u32,
}

impl<V, T> Element<V, T> {
    fn new(list: T, idx: u32) -> Self {
        Self { v: PhantomData, list, idx }
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

impl<T, U> ListAccessable<U> for T
where 
    T: TypeKind,
    T::Kind: ListAccessable<U>,
{
    type View = <T::Kind as ListAccessable<U>>::View;

    unsafe fn get(list: U, index: u32) -> Self::View {
        <T::Kind as ListAccessable<U>>::get(list, index)
    }
}

impl<'b, 'p, T: Table> ListAccessable<&'b ptr::Reader<'p, T>> for () {
    type View = ();

    #[inline]
    unsafe fn get(_: &'b ptr::Reader<'p, T>, _: u32) -> Self::View { () }
}

impl<'b, 'p, T: Table> ListAccessable<&'b mut ptr::Builder<'p, T>> for () {
    type View = ();

    #[inline]
    unsafe fn get(_: &'b mut ptr::Builder<'p, T>, _: u32) -> Self::View { () }
}

pub type DataElementBuilder<'b, 'p, T, D> = Element<kind::Data<D>, &'b mut ptr::Builder<'p, T>>;

impl<'b, 'p, T: Table, D: FieldData> ListAccessable<&'b ptr::Reader<'p, T>> for kind::Data<D> {
    type View = D;

    #[inline]
    unsafe fn get(list: &'b ptr::Reader<'p, T>, idx: u32) -> Self::View {
        list.data_unchecked(idx)
    }
}

impl<'b, 'p, T: Table, D: FieldData> ListAccessable<&'b mut ptr::Builder<'p, T>> for kind::Data<D> {
    type View = DataElementBuilder<'b, 'p, T, D>;

    #[inline]
    unsafe fn get(list: &'b mut ptr::Builder<'p, T>, idx: u32) -> Self::View {
        Element::new(list, idx)
    }
}

impl<'b, 'p, T: Table, D: FieldData> DataElementBuilder<'b, 'p, T, D> {
    /// A generic accessor for getting "field data", that is, primitive numeric and boolean types.
    #[inline]
    pub fn get(&self) -> D {
        unsafe { self.list.data_unchecked(self.idx) }
    }
    /// A generic accessor for setting "field data", that is, primitive numeric and boolean types.
    #[inline]
    pub fn set(&mut self, value: D) {
        unsafe { self.list.set_data_unchecked(self.idx, value) }
    }
}

impl<'b, 'p, T: Table> DataElementBuilder<'b, 'p, T, f32> {
    #[inline]
    /// Canonicalizes NaN values by blowing away the NaN payload.
    pub fn set_canonical(&mut self, value: f32) {
        const CANONICAL_NAN: u32 = 0x7fc00000u32;

        if value.is_nan() {
            unsafe { self.list.set_data_unchecked(self.idx, CANONICAL_NAN) }
        } else {
            self.set(value)
        }
    }
}

impl<'b, 'p, T: Table> DataElementBuilder<'b, 'p, T, f64> {
    /// Canonicalizes NaN values by blowing away the NaN payload.
    #[inline]
    pub fn set_canonical(&mut self, value: f64) {
        const CANONICAL_NAN: u64 = 0x7ff8000000000000u64;

        if value.is_nan() {
            unsafe { self.list.set_data_unchecked(self.idx, CANONICAL_NAN) }
        } else {
            self.set(value)
        }
    }
}

impl<E: ty::Enum> ty::ListValue for kind::Enum<E> {
    const ELEMENT_SIZE: ElementSize = ElementSize::TwoBytes;
}

pub type EnumElementBuilder<'b, 'p, T, E> = Element<kind::Enum<E>, &'b mut ptr::Builder<'p, T>>;

impl<'b, 'p, T: Table, E: ty::Enum> ListAccessable<&'b ptr::Reader<'p, T>> for kind::Enum<E> {
    type View = EnumResult<E>;

    #[inline]
    unsafe fn get(list: &'b ptr::Reader<'p, T>, idx: u32) -> Self::View {
        let value = list.data_unchecked(idx);
        E::try_from(value)
    }
}

impl<'b, 'p, T: Table, E: ty::Enum> ListAccessable<&'b mut ptr::Builder<'p, T>> for kind::Enum<E> {
    type View = EnumElementBuilder<'b, 'p, T, E>;

    #[inline]
    unsafe fn get(list: &'b mut ptr::Builder<'p, T>, idx: u32) -> Self::View {
        Element::new(list, idx)
    }
}

impl<'b, 'p, E: ty::Enum, T: Table> EnumElementBuilder<'b, 'p, T, E> {
    #[inline]
    pub fn get(&self) -> EnumResult<E> {
        let value = unsafe { self.list.data_unchecked::<u16>(self.idx) };
        E::try_from(value)
    }
    #[inline]
    pub fn set(&mut self, value: E) {
        let value = value.into();
        self.set_value(value)
    }
    #[inline]
    pub fn set_value(&mut self, value: u16) {
        unsafe { self.list.set_data_unchecked::<u16>(self.idx, value) }
    }
}

impl<S: ty::Struct> ty::ListValue for kind::Struct<S> {
    const ELEMENT_SIZE: ElementSize = ElementSize::InlineComposite(S::SIZE);
}

pub type StructElement<S, Repr> = Element<kind::Struct<S>, Repr>;

pub type StructElementBuilder<'b, 'p, T, S> = StructElement<S, &'b mut ptr::Builder<'p, T>>;
pub type StructElementOwner<'p, T, S> = StructElement<S, ptr::Builder<'p, T>>;

impl<'b, 'p, T: Table, S: ty::Struct> ListAccessable<&'b ptr::Reader<'p, T>> for kind::Struct<S> {
    type View = S::Reader<'p, T>;

    #[inline]
    unsafe fn get(list: &'b ptr::Reader<'p, T>, idx: u32) -> Self::View {
        ty::StructReader::from_ptr(list.struct_unchecked(idx))
    }
}

impl<'b, 'p, T: Table, S: ty::Struct> ListAccessable<&'b mut ptr::Builder<'p, T>> for kind::Struct<S> {
    type View = StructElementBuilder<'b, 'p, T, S>;

    #[inline]
    unsafe fn get(list: &'b mut ptr::Builder<'p, T>, idx: u32) -> Self::View {
        Element::new(list, idx)
    }
}

impl<'p, T: Table, S: ty::Struct> ListAccessable<ptr::Builder<'p, T>> for kind::Struct<S> {
    type View = StructElementOwner<'p, T, S>;

    #[inline]
    unsafe fn get(list: ptr::Builder<'p, T>, idx: u32) -> Self::View {
        Element::new(list, idx)
    }
}

impl<'b, 'p, T: Table, S: ty::Struct> StructElementBuilder<'b, 'p, T, S> {
    #[inline]
    pub fn get(self) -> S::Builder<'b, T> {
        unsafe { ty::StructBuilder::from_ptr(self.list.struct_mut_unchecked(self.idx)) }
    }

    #[inline]
    pub fn reader(&self) -> S::Reader<'_, T> {
        ty::StructReader::from_ptr(unsafe { self.list.struct_unchecked(self.idx) })
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
    pub fn set_with_caveats(self, reader: &S::Reader<'_, impl InsertableInto<T>>) -> S::Builder<'b, T> {
        self.try_set_with_caveats(reader, IgnoreErrors).unwrap()
    }

    #[inline]
    pub fn try_set_with_caveats<E: ErrorHandler>(
        self,
        reader: &S::Reader<'_, impl InsertableInto<T>>,
        err_handler: E,
    ) -> Result<S::Builder<'b, T>, E::Error> {
        let mut elem = self.get();
        elem.as_mut().try_copy_with_caveats(reader.as_ref(), false, err_handler)?;
        Ok(elem)
    }
}

pub type AnyStructElement<Repr> = Element<AnyStruct, Repr>;

pub type AnyStructElementBuilder<'b, 'p, T> = AnyStructElement<&'b mut ptr::Builder<'p, T>>;
pub type AnyStructElementOwner<'p, T> = AnyStructElement<ptr::Builder<'p, T>>;

impl<'b, 'p, T: Table> ListAccessable<&'b ptr::Reader<'p, T>> for AnyStruct {
    type View = any::StructReader<'p, T>;

    #[inline]
    unsafe fn get(list: &'b ptr::Reader<'p, T>, idx: u32) -> Self::View {
        ty::StructReader::from_ptr(list.struct_unchecked(idx))
    }
}

impl<'b, 'p, T: Table> ListAccessable<&'b mut ptr::Builder<'p, T>> for AnyStruct {
    type View = AnyStructElementBuilder<'b, 'p, T>;

    #[inline]
    unsafe fn get(list: &'b mut ptr::Builder<'p, T>, idx: u32) -> Self::View {
        Element::new(list, idx)
    }
}

impl<'p, T: Table> ListAccessable<ptr::Builder<'p, T>> for AnyStruct {
    type View = AnyStructElementOwner<'p, T>;

    #[inline]
    unsafe fn get(list: ptr::Builder<'p, T>, idx: u32) -> Self::View {
        Element::new(list, idx)
    }
}

impl<'b, 'p, T: Table> AnyStructElementBuilder<'b, 'p, T> {
    #[inline]
    pub fn get(self) -> any::StructBuilder<'b, T> {
        unsafe { self.list.struct_mut_unchecked(self.idx).into() }
    }

    #[inline]
    pub fn reader(&self) -> any::StructReader<'_, T> {
        unsafe { self.list.struct_unchecked(self.idx).into() }
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
    pub fn set_with_caveats(self, reader: &any::StructReader<'_, impl InsertableInto<T>>) -> any::StructBuilder<'b, T> {
        self.try_set_with_caveats(reader, IgnoreErrors).unwrap()
    }

    #[inline]
    pub fn try_set_with_caveats<E: ErrorHandler>(
        self,
        reader: &any::StructReader<'_, impl InsertableInto<T>>,
        err_handler: E,
    ) -> Result<any::StructBuilder<'b, T>, E::Error> {
        let mut elem = self.get();
        elem.as_mut().try_copy_with_caveats(reader.as_ref(), false, err_handler)?;
        Ok(elem)
    }
}

pub type PtrElement<P, Repr> = Element<kind::PtrList<P>, Repr>;

pub type PtrElementReader<'b, 'p, T, P> = PtrElement<P, &'b ptr::Reader<'p, T>>;
pub type PtrElementBuilder<'b, 'p, T, P> = PtrElement<P, &'b mut ptr::Builder<'p, T>>;
pub type PtrElementOwner<'p, T, P> = PtrElement<P, ptr::Builder<'p, T>>;

impl<'b, 'p, T: Table, P: Ptr> ListAccessable<&'b ptr::Reader<'p, T>> for kind::PtrList<P> {
    type View = PtrElementReader<'b, 'p, T, P>;

    unsafe fn get(list: &'b ptr::Reader<'p, T>, idx: u32) -> Self::View {
        Element::new(list, idx)
    }
}

impl<'b, 'p, T: Table, P: Ptr> ListAccessable<&'b mut ptr::Builder<'p, T>> for kind::PtrList<P> {
    type View = PtrElementBuilder<'b, 'p, T, P>;

    unsafe fn get(list: &'b mut ptr::Builder<'p, T>, idx: u32) -> Self::View {
        Element::new(list, idx)
    }
}

impl<'p, T: Table, P: Ptr> ListAccessable<ptr::Builder<'p, T>> for kind::PtrList<P> {
    type View = PtrElementOwner<'p, T, P>;

    unsafe fn get(list: ptr::Builder<'p, T>, idx: u32) -> Self::View {
        Element::new(list, idx)
    }
}

impl<V> TypeKind for List<V> {
    type Kind = kind::PtrList<Self>;
}
impl<V> Ptr for List<V> {}

pub type ListElement<V, Repr> = PtrElement<List<V>, Repr>;

pub type ListElementReader<'b, 'p, T, V> = ListElement<V, &'b ptr::Reader<'p, T>>;
pub type ListElementBuilder<'b, 'p, T, V> = ListElement<V, &'b mut ptr::Builder<'p, T>>;
pub type ListElementOwner<'p, T, V> = ListElement<V, ptr::Builder<'p, T>>;

impl<'b, 'p, T: Table, V: ty::DynListValue> ListElementReader<'b, 'p, T, V> {
    #[inline]
    fn ptr_reader(&self) -> crate::ptr::PtrReader<'p, T> {
        unsafe { self.list.ptr_unchecked(self.idx) }
    }

    #[inline]
    fn empty_list(&self) -> Reader<'p, V, T> {
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
    pub fn get(&self) -> Reader<'p, V, T> {
        match self.try_get_option() {
            Ok(Some(reader)) => reader,
            _ => self.empty_list(),
        }
    }

    /// Returns the list value in this element, or None if the list element is null or
    /// an error occurs while reading.
    #[inline]
    pub fn get_option(&self) -> Option<Reader<'p, V, T>> {
        match self.try_get_option() {
            Ok(Some(reader)) => Some(reader),
            _ => None,
        }
    }

    /// Returns the list value in this element. If the element is null, this returns an
    /// empty list. If an error occurs while reading it is returned.
    #[inline]
    pub fn try_get(&self) -> Result<Reader<'p, V, T>> {
        match self.try_get_option() {
            Ok(Some(reader)) => Ok(reader),
            Ok(None) => Ok(self.empty_list()),
            Err(err) => Err(err),
        }
    }

    /// Returns the list value in this element. If the element is null, this returns Ok(None).
    /// If an error occurs while reading it is returned.
    #[inline]
    pub fn try_get_option(&self) -> Result<Option<Reader<'p, V, T>>> {
        match self.ptr_reader().to_list(Some(PtrElementSize::size_of::<V>())) {
            Ok(Some(reader)) => Ok(Some(List::new(reader))),
            Ok(None) => Ok(None),
            Err(err) => Err(err),
        }
    }
}

impl<'b, 'p, T: Table, V: ty::ListValue> ListElementBuilder<'b, 'p, T, V> {
    #[inline]
    fn ptr_builder(&mut self) -> crate::ptr::PtrBuilder<T> {
        unsafe { self.list.ptr_mut_unchecked(self.idx) }
    }

    #[inline]
    fn into_ptr_builder(self) -> crate::ptr::PtrBuilder<'b, T> {
        unsafe { self.list.ptr_mut_unchecked(self.idx) }
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

    /// Initialize a new instance with the given element size.
    /// 
    /// The element size must be a valid upgrade from `V::ELEMENT_SIZE`. That is, calling
    /// `V::ELEMENT_SIZE.upgrade_to(size)` must yield `Some(size)`.
    #[inline]
    pub fn init_with_size(self, count: u32, size: ElementSize) -> Builder<'b, V, T> {
        assert_eq!(V::ELEMENT_SIZE.upgrade_to(size), Some(size));
        let count = ElementCount::new(count).expect("too many elements for list");
        List::new(self.into_ptr_builder().init_list(size, count))
    }

    #[inline]
    pub fn try_set<E: ErrorHandler>(
        &mut self,
        value: &Reader<V, impl InsertableInto<T>>,
        err_handler: E,
    ) -> Result<(), E::Error> {
        self.ptr_builder().try_set_list(&value.repr, CopySize::Minimum(V::ELEMENT_SIZE), err_handler)
    }

    #[inline]
    pub fn set(&mut self, value: &Reader<V, impl InsertableInto<T>>) {
        self.try_set(value, IgnoreErrors).unwrap()
    }

    #[inline]
    pub fn clear(&mut self) {
        self.ptr_builder().clear()
    }
}

impl<'b, 'p, T: Table> ListElementBuilder<'b, 'p, T, AnyStruct> {
    #[inline]
    fn ptr_builder(&mut self) -> crate::ptr::PtrBuilder<T> {
        unsafe { self.list.ptr_mut_unchecked(self.idx) }
    }

    #[inline]
    fn into_ptr_builder(self) -> crate::ptr::PtrBuilder<'b, T> {
        unsafe { self.list.ptr_mut_unchecked(self.idx) }
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

    /// Initializes the element as a list with the specified element count. This clears any
    /// pre-existing value.
    ///
    /// If the number of struct elements is too large for a Cap'n Proto message, this returns
    /// an Err.
    #[inline]
    pub fn try_init(
        self,
        size: StructSize,
        count: ElementCount,
    ) -> Result<Builder<'b, AnyStruct, T>> {
        self.into_ptr_builder()
            .try_init_list(ElementSize::InlineComposite(size), count)
            .map(List::new)
            .map_err(|(err, _)| err)
    }

    #[inline]
    pub fn try_set<E: ErrorHandler>(
        self,
        value: &Reader<AnyStruct, impl InsertableInto<T>>,
        desired_size: Option<StructSize>,
        err_handler: E,
    ) -> Result<(), E::Error> {
        let copy_size = match desired_size {
            Some(size) => CopySize::Minimum(ElementSize::InlineComposite(size)),
            None => CopySize::FromValue,
        };

        self.into_ptr_builder().try_set_list(&value.repr, copy_size, err_handler)
    }

    #[inline]
    pub fn set(self, value: &Reader<AnyStruct, impl InsertableInto<T>>, desired_size: Option<StructSize>) {
        self.try_set(value, desired_size, IgnoreErrors).unwrap()
    }

    #[inline]
    pub fn clear(&mut self) {
        self.ptr_builder().clear()
    }
}

impl TypeKind for AnyList {
    type Kind = kind::PtrList<Self>;
}
impl Ptr for AnyList {}

pub type AnyListElement<Repr> = PtrElement<AnyList, Repr>;

pub type AnyListElementReader<'b, 'p, T> = AnyListElement<&'b ptr::Reader<'p, T>>;
pub type AnyListElementBuilder<'b, 'p, T> = AnyListElement<&'b mut ptr::Builder<'p, T>>;
pub type AnyListElementOwner<'p, T> = AnyListElement<ptr::Builder<'p, T>>;

impl<'b, 'p, T: Table> AnyListElementReader<'b, 'p, T> {
    #[inline]
    fn ptr_reader(&self) -> crate::ptr::PtrReader<'p, T> {
        unsafe { self.list.ptr_unchecked(self.idx) }
    }

    #[inline]
    fn empty_list(&self) -> any::ListReader<'p, T> {
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
    pub fn get(&self) -> any::ListReader<'p, T> {
        match self.try_get_option() {
            Ok(Some(reader)) => reader,
            _ => self.empty_list(),
        }
    }

    /// Returns the list value in this element, or None if the list element is null or
    /// an error occurs while reading.
    #[inline]
    pub fn get_option(&self) -> Option<any::ListReader<'p, T>> {
        match self.try_get_option() {
            Ok(Some(reader)) => Some(reader),
            _ => None,
        }
    }

    /// Returns the list value in this element. If the element is null, this returns an
    /// empty list. If an error occurs while reading it is returned.
    #[inline]
    pub fn try_get(&self) -> Result<any::ListReader<'p, T>> {
        match self.try_get_option() {
            Ok(Some(reader)) => Ok(reader),
            Ok(None) => Ok(self.empty_list()),
            Err(err) => Err(err),
        }
    }

    /// Returns the list value in this element. If the element is null, this returns Ok(None).
    /// If an error occurs while reading it is returned.
    #[inline]
    pub fn try_get_option(&self) -> Result<Option<any::ListReader<'p, T>>> {
        match self.ptr_reader().to_list(None) {
            Ok(Some(reader)) => Ok(Some(any::ListReader::from(reader))),
            Ok(None) => Ok(None),
            Err(err) => Err(err),
        }
    }
}

impl<'b, 'p, T: Table> AnyListElementBuilder<'b, 'p, T> {
    #[inline]
    fn ptr_builder(&mut self) -> crate::ptr::PtrBuilder<T> {
        unsafe { self.list.ptr_mut_unchecked(self.idx) }
    }

    #[inline]
    fn into_ptr_builder(self) -> crate::ptr::PtrBuilder<'b, T> {
        unsafe { self.list.ptr_mut_unchecked(self.idx) }
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
    pub fn try_set<E: ErrorHandler>(
        self,
        value: &any::ListReader<impl InsertableInto<T>>,
        err_handler: E,
    ) -> Result<(), E::Error> {
        self.into_ptr_builder()
            .try_set_list(value.as_ref(), CopySize::Minimum(ElementSize::Pointer), err_handler)
    }

    #[inline]
    pub fn set(self, value: &any::ListReader<impl InsertableInto<T>>) {
        self.try_set(value, IgnoreErrors).unwrap()
    }

    #[inline]
    pub fn clear(&mut self) {
        self.ptr_builder().clear()
    }
}

impl TypeKind for Data {
    type Kind = kind::PtrList<Self>;
}
impl Ptr for Data {}

pub type DataBlobElement<Repr> = PtrElement<Data, Repr>;

pub type DataBlobElementReader<'b, 'p, T> = DataBlobElement<&'b ptr::Reader<'p, T>>;
pub type DataBlobElementBuilder<'b, 'p, T> = DataBlobElement<&'b mut ptr::Builder<'p, T>>;

impl<'b, 'p, T: Table> DataBlobElementReader<'b, 'p, T> {
    #[inline]
    fn ptr_reader(&self) -> crate::ptr::PtrReader<'p, T> {
        unsafe { self.list.ptr_unchecked(self.idx) }
    }

    /// Returns whether this list element is a null pointer.
    #[inline]
    pub fn is_null(&self) -> bool {
        self.ptr_reader().is_null()
    }

    /// Returns the list value in this element, or an empty list if the list is null
    /// or an error occurs while reading.
    #[inline]
    pub fn get(&self) -> data::Reader<'p> {
        match self.try_get_option() {
            Ok(Some(reader)) => reader,
            _ => data::Reader::empty(),
        }
    }

    /// Returns the list value in this element, or None if the list element is null or
    /// an error occurs while reading.
    #[inline]
    pub fn get_option(&self) -> Option<data::Reader<'p>> {
        match self.try_get_option() {
            Ok(Some(reader)) => Some(reader),
            _ => None,
        }
    }

    /// Returns the list value in this element. If the element is null, this returns an
    /// empty list. If an error occurs while reading it is returned.
    #[inline]
    pub fn try_get(&self) -> Result<data::Reader<'p>> {
        match self.try_get_option() {
            Ok(Some(reader)) => Ok(reader),
            Ok(None) => Ok(data::Reader::empty()),
            Err(err) => Err(err),
        }
    }

    /// Returns the list value in this element. If the element is null, this returns Ok(None).
    /// If an error occurs while reading it is returned.
    #[inline]
    pub fn try_get_option(&self) -> Result<Option<data::Reader<'p>>> {
        match self.ptr_reader().to_blob() {
            Ok(Some(reader)) => Ok(Some(data::Reader::from(reader))),
            Ok(None) => Ok(None),
            Err(err) => Err(err),
        }
    }
}

impl<'b, 'p, T: Table> DataBlobElementBuilder<'b, 'p, T> {
    #[inline]
    fn ptr_builder(&mut self) -> crate::ptr::PtrBuilder<T> {
        unsafe { self.list.ptr_mut_unchecked(self.idx) }
    }

    #[inline]
    fn into_ptr_builder(self) -> crate::ptr::PtrBuilder<'b, T> {
        unsafe { self.list.ptr_mut_unchecked(self.idx) }
    }

    #[inline]
    pub fn get(self) -> data::Builder<'b> {
        match self.into_ptr_builder().to_blob_mut() {
            Ok(b) => b.into(),
            Err(_) => data::Builder::empty(),
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
    pub fn set(self, value: data::Reader) -> data::Builder<'b> {
        self.into_ptr_builder().set_blob(value.into()).into()
    }

    #[inline]
    pub fn clear(&mut self) {
        self.ptr_builder().clear()
    }
}

impl TypeKind for Text {
    type Kind = kind::PtrList<Self>;
}
impl Ptr for Text {}

pub type TextElement<Repr> = PtrElement<Text, Repr>;

pub type TextElementReader<'b, 'p, T> = TextElement<&'b ptr::Reader<'p, T>>;
pub type TextElementBuilder<'b, 'p, T> = TextElement<&'b mut ptr::Builder<'p, T>>;

impl<'b, 'p, T: Table> TextElementReader<'b, 'p, T> {
    #[inline]
    fn ptr_reader(&self) -> crate::ptr::PtrReader<'p, T> {
        unsafe { self.list.ptr_unchecked(self.idx) }
    }

    /// Returns whether this list element is a null pointer.
    #[inline]
    pub fn is_null(&self) -> bool {
        self.ptr_reader().is_null()
    }

    /// Returns the list value in this element, or an empty list if the list is null
    /// or an error occurs while reading.
    #[inline]
    pub fn get(&self) -> text::Reader<'p> {
        match self.try_get_option() {
            Ok(Some(reader)) => reader,
            _ => text::Reader::empty(),
        }
    }

    /// Returns the list value in this element, or None if the list element is null or
    /// an error occurs while reading.
    #[inline]
    pub fn get_option(&self) -> Option<text::Reader<'p>> {
        match self.try_get_option() {
            Ok(Some(reader)) => Some(reader),
            _ => None,
        }
    }

    /// Returns the list value in this element. If the element is null, this returns an
    /// empty list. If an error occurs while reading it is returned.
    #[inline]
    pub fn try_get(&self) -> Result<text::Reader<'p>> {
        match self.try_get_option() {
            Ok(Some(reader)) => Ok(reader),
            Ok(None) => Ok(text::Reader::empty()),
            Err(err) => Err(err),
        }
    }

    /// Returns the list value in this element. If the element is null, this returns Ok(None).
    /// If an error occurs while reading it is returned.
    #[inline]
    pub fn try_get_option(&self) -> Result<Option<text::Reader<'p>>> {
        match self.ptr_reader().to_blob() {
            Ok(Some(b)) => text::Reader::new(b)
                .ok_or_else(|| Error::from(ErrorKind::TextNotNulTerminated))
                .map(Some),
            Ok(None) => Ok(None),
            Err(err) => Err(err),
        }
    }
}

impl<'b, 'p, T: Table> TextElementBuilder<'b, 'p, T> {
    #[inline]
    fn ptr_builder(&mut self) -> crate::ptr::PtrBuilder<T> {
        unsafe { self.list.ptr_mut_unchecked(self.idx) }
    }

    #[inline]
    fn into_ptr_builder(self) -> crate::ptr::PtrBuilder<'b, T> {
        unsafe { self.list.ptr_mut_unchecked(self.idx) }
    }

    #[inline]
    pub fn get(self) -> text::Builder<'b> {
        match self.try_get_option() {
            Ok(Some(text)) => text,
            _ => text::Builder::empty(),
        }
    }

    #[inline]
    pub fn try_get_option(self) -> Result<Option<text::Builder<'b>>> {
        match self.into_ptr_builder().to_blob_mut() {
            Ok(blob) => match text::Builder::new(blob) {
                Some(text) => Ok(Some(text)),
                None => Err(ErrorKind::TextNotNulTerminated.into()),
            },
            Err((None, _)) => Ok(None),
            Err((Some(err), _)) => Err(err),
        }
    }

    #[inline]
    pub fn init(self, count: text::ByteCount) -> text::Builder<'b> {
        let blob = self.into_ptr_builder().init_blob(count.into());
        text::Builder::new_unchecked(blob)
    }

    #[inline]
    pub fn set(self, value: &text::Reader) -> text::Builder<'b> {
        let mut new = self.init(value.byte_count());
        new.as_bytes_mut().copy_from_slice(value.as_bytes());
        new
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

impl ty::ListValue for AnyPtr {
    const ELEMENT_SIZE: ElementSize = ElementSize::Pointer;
}

impl<'a, 'b, T: Table> ListAccessable<&'b ptr::Reader<'a, T>> for AnyPtr {
    type View = any::PtrReader<'a, T>;

    #[inline]
    unsafe fn get(list: &'b ptr::Reader<'a, T>, idx: u32) -> Self::View {
        list.ptr_unchecked(idx).into()
    }
}

impl<'a, 'b, T: Table> ListAccessable<&'b mut ptr::Builder<'a, T>> for AnyPtr {
    type View = any::PtrBuilder<'b, T>;

    #[inline]
    unsafe fn get(list: &'b mut ptr::Builder<'a, T>, idx: u32) -> Self::View {
        list.ptr_mut_unchecked(idx).into()
    }
}

impl<C: ty::Capability> TypeKind for kind::Capability<C> {
    type Kind = kind::PtrList<Self>;
}
impl<C: ty::Capability> Ptr for kind::Capability<C> {}

pub type CapabilityElement<C, Repr> = PtrElement<kind::Capability<C>, Repr>;

pub type CapabilityElementReader<'b, 'p, T, C> = CapabilityElement<C, &'b ptr::Reader<'p, T>>;
pub type CapabilityElementBuilder<'b, 'p, T, C> = CapabilityElement<C, &'b mut ptr::Builder<'p, T>>;
pub type CapabilityElementOwner<'p, T, C> = CapabilityElement<C, ptr::Builder<'p, T>>;

impl<'b, 'p, T, C, Client> CapabilityElementReader<'b, 'p, T, C>
where
    C: ty::Capability<Client = Client>,
    T: rpc::CapTable<Cap = Client>,
{
    #[inline]
    fn ptr_reader(&self) -> crate::ptr::PtrReader<'p, T> {
        unsafe { self.list.ptr_unchecked(self.idx) }
    }

    #[inline]
    pub fn is_null(&self) -> bool {
        self.ptr_reader().is_null()
    }

    #[inline]
    pub fn try_get_option(&self) -> Result<Option<C>> {
        match self.ptr_reader().try_to_capability() {
            Ok(Some(cap)) => Ok(Some(C::from_client(cap))),
            Ok(None) => Ok(None),
            Err(err) => Err(err),
        }
    }
}

impl<'b, 'p, T, C, Client> CapabilityElementReader<'b, 'p, T, C>
where
    C: ty::Capability<Client = Client>,
    T: rpc::CapTable<Cap = Client> + rpc::BreakableCapSystem,
{
    #[inline]
    pub fn get(&self) -> C {
        match self.try_get_option() {
            Ok(Some(client)) => client,
            Ok(None) => C::from_client(T::null()),
            Err(err) => C::from_client(T::broken(&err)),
        }
    }

    #[inline]
    pub fn get_option(&self) -> Option<C> {
        match self.try_get_option() {
            Ok(Some(client)) => Some(client),
            _ => None,
        }
    }

    #[inline]
    pub fn try_get(&self) -> Result<C> {
        match self.try_get_option() {
            Ok(Some(client)) => Ok(client),
            Ok(None) => Ok(C::from_client(T::null())),
            Err(err) => Err(err),
        }
    }
}

impl<'b, 'p, T, C, Client> CapabilityElementBuilder<'b, 'p, T, C>
where
    C: ty::Capability<Client = Client>,
    T: rpc::CapTable<Cap = Client>,
{
    #[inline]
    fn ptr_reader(&self) -> crate::ptr::PtrReader<T> {
        unsafe { self.list.ptr_unchecked(self.idx) }
    }

    #[inline]
    fn ptr_builder(&mut self) -> crate::ptr::PtrBuilder<T> {
        unsafe { self.list.ptr_mut_unchecked(self.idx) }
    }

    #[inline]
    pub fn is_null(&self) -> bool {
        self.ptr_reader().is_null()
    }

    #[inline]
    pub fn try_get_option(&self) -> Result<Option<C>> {
        match self.ptr_reader().try_to_capability() {
            Ok(Some(cap)) => Ok(Some(C::from_client(cap))),
            Ok(None) => Ok(None),
            Err(err) => Err(err),
        }
    }

    #[inline]
    pub fn set(&mut self, client: C) {
        self.ptr_builder().set_cap(client.into_inner());
    }
}

impl<'b, 'p, T, C, Client> CapabilityElementBuilder<'b, 'p, T, C>
where
    C: ty::Capability<Client = Client>,
    T: rpc::CapTable<Cap = Client> + rpc::BreakableCapSystem,
{
    #[inline]
    pub fn get(&self) -> C {
        match self.try_get_option() {
            Ok(Some(client)) => client,
            Ok(None) => C::from_client(T::null()),
            Err(err) => C::from_client(T::broken(&err)),
        }
    }
}

// Iterators. WARNING! HELL BELOW
// Ideally one day we'll get rid of all this crap and just use lending iterators.

/// Walks the type kind tree to find what type is actually supposed to be used for the iterator.
pub trait IterKind {
    type Iter;
}

impl<T> IterKind for T
where
    T: TypeKind,
    T::Kind: IterKind,
{
    type Iter = T::Kind;
}

impl IterKind for () {
    type Iter = ();
}

impl<S: ty::Struct> IterKind for kind::Struct<S> {
    type Iter = Self;
}

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
pub trait IterStrategy<T, V: ListAccessable<T>> {
    /// The resulting type derived from this strategy
    type Item;

    /// Applies the strategy to get the output
    fn get<'b>(&mut self, element: V::View) -> Self::Item;
}

impl<T, V, F, Output> IterStrategy<T, V> for F
where
    V: ListAccessable<T>,
    F: FnMut(V::View) -> Output,
{
    type Item = Output;

    #[inline]
    fn get(&mut self, element: V::View) -> Self::Item {
        (self)(element)
    }
}

macro_rules! infallible_strategies {
    ($ty:ty) => {
        impl<'b, 'p, T: Table> IterStrategy<&'b ptr::Reader<'p, T>, ()> for $ty {
            type Item = ();

            #[inline]
            fn get(&mut self, _: Self::Item) -> Self::Item {}
        }

        impl<'b, 'p, T: Table, D: FieldData> IterStrategy<&'b ptr::Reader<'p, T>, kind::Data<D>> for $ty {
            type Item = D;

            #[inline]
            fn get(&mut self, element: Self::Item) -> Self::Item { element }
        }

        impl<'b, 'p, T: Table, E: ty::Enum> IterStrategy<&'b ptr::Reader<'p, T>, kind::Enum<E>> for $ty {
            type Item = EnumResult<E>;

            #[inline]
            fn get(&mut self, element: Self::Item) -> Self::Item { element }
        }

        impl<'b, 'p, T: Table, S: ty::Struct> IterStrategy<&'b ptr::Reader<'p, T>, kind::Struct<S>> for $ty {
            type Item = S::Reader<'p, T>;

            #[inline]
            fn get(&mut self, element: Self::Item) -> Self::Item { element }
        }

        impl<'b, 'p, T: Table> IterStrategy<&'b ptr::Reader<'p, T>, AnyStruct> for $ty {
            type Item = any::StructReader<'p, T>;

            #[inline]
            fn get(&mut self, element: Self::Item) -> Self::Item { element }
        }

        impl<'b, 'p, T: Table> IterStrategy<&'b ptr::Reader<'p, T>, AnyPtr> for $ty {
            type Item = any::PtrReader<'p, T>;

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

impl<'b, 'p, T: Table, V: ty::DynListValue> IterStrategy<&'b ptr::Reader<'p, T>, List<V>> for InfalliblePtrs {
    type Item = Reader<'p, V, T>;

    fn get(&mut self, element: ListElementReader<'b, 'p, T, V>) -> Self::Item {
        element.get()
    }
}

impl<'b, 'p, T: Table> IterStrategy<&'b ptr::Reader<'p, T>, AnyList> for InfalliblePtrs {
    type Item = any::ListReader<'p, T>;

    fn get(&mut self, element: AnyListElementReader<'b, 'p, T>) -> Self::Item {
        element.get()
    }
}

impl<'b, 'p, T: Table> IterStrategy<&'b ptr::Reader<'p, T>, Data> for InfalliblePtrs {
    type Item = data::Reader<'p>;

    fn get(&mut self, element: DataBlobElementReader<'b, 'p, T>) -> Self::Item {
        element.get()
    }
}

impl<'b, 'p, T: Table> IterStrategy<&'b ptr::Reader<'p, T>, Text> for InfalliblePtrs {
    type Item = text::Reader<'p>;

    fn get(&mut self, element: TextElementReader<'b, 'p, T>) -> Self::Item {
        element.get()
    }
}

/// A fallible iteration strategy for many element types. This matches the behavior of capnproto-rust
/// by returning results for pointer reads.
pub struct Fallible;

infallible_strategies!(Fallible);

impl<'b, 'p, T: Table, V: ty::DynListValue> IterStrategy<&'b ptr::Reader<'p, T>, List<V>> for Fallible {
    type Item = Result<Reader<'p, V, T>>;

    fn get(&mut self, element: ListElementReader<'b, 'p, T, V>) -> Self::Item {
        element.try_get()
    }
}

impl<'b, 'p, T: Table> IterStrategy<&'b ptr::Reader<'p, T>, AnyList> for Fallible {
    type Item = Result<any::ListReader<'p, T>>;

    fn get(&mut self, element: AnyListElementReader<'b, 'p, T>) -> Self::Item {
        element.try_get()
    }
}

impl<'b, 'p, T: Table> IterStrategy<&'b ptr::Reader<'p, T>, Data> for Fallible {
    type Item = Result<data::Reader<'p>>;

    fn get(&mut self, element: DataBlobElementReader<'b, 'p, T>) -> Self::Item {
        element.try_get()
    }
}

impl<'b, 'p, T: Table> IterStrategy<&'b ptr::Reader<'p, T>, Text> for Fallible {
    type Item = Result<text::Reader<'p>>;

    fn get(&mut self, element: TextElementReader<'b, 'p, T>) -> Self::Item {
        element.try_get()
    }
}

/// An iterator through a list that maps the element view into a new output.
pub struct Iter<'a, V, S = InfalliblePtrs, T: Table = rpc::Empty> {
    v: PhantomData<fn() -> V>,
    list: ptr::Reader<'a, T>,
    range: Range<u32>,
    strategy: S,
}

impl<'p, V, S, T, Item> Iterator for Iter<'p, V, S, T>
where
    V: IterKind,
    V::Iter: for<'b> ListAccessable<&'b ptr::Reader<'p, T>>,
    S: for<'b> IterStrategy<&'b ptr::Reader<'p, T>, V::Iter, Item = Item>,
    T: Table,
{
    type Item = Item;

    #[inline]
    fn next(&mut self) -> Option<Self::Item> {
        let element = unsafe { <V::Iter as ListAccessable<_>>::get(&self.list, self.range.next()?) };
        Some(self.strategy.get(element))
    }
    #[inline]
    fn size_hint(&self) -> (usize, Option<usize>) {
        self.range.size_hint()
    }
}

impl<'p, V, S, T, Item> DoubleEndedIterator for Iter<'p, V, S, T>
where
    V: IterKind,
    V::Iter: for<'b> ListAccessable<&'b ptr::Reader<'p, T>>,
    S: for<'b> IterStrategy<&'b ptr::Reader<'p, T>, V::Iter, Item = Item>,
    T: Table,
{
    #[inline]
    fn next_back(&mut self) -> Option<Self::Item> {
        let element = unsafe { <V::Iter as ListAccessable<_>>::get(&self.list, self.range.next_back()?) };
        Some(self.strategy.get(element))
    }
}

impl<'a, V, T: Table> Reader<'a, V, T> {
    pub fn into_iter_by<S>(self, strat: S) -> Iter<'a, V, S, T> {
        let range = 0..self.len();
        Iter { v: PhantomData, list: self.repr, range, strategy: strat }
    }
}

impl<'p, V, T, Item> IntoIterator for Reader<'p, V, T>
where
    V: IterKind,
    V::Iter: for<'b> ListAccessable<&'b ptr::Reader<'p, T>>,
    InfalliblePtrs: for<'b> IterStrategy<&'b ptr::Reader<'p, T>, V::Iter, Item = Item>,
    T: Table,
{
    type IntoIter = Iter<'p, V, InfalliblePtrs, T>;
    type Item = Item;

    fn into_iter(self) -> Self::IntoIter {
        self.into_iter_by(InfalliblePtrs)
    }
}