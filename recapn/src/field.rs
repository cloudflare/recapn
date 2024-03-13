//! Types and traits for accessing Cap'n Proto fields in a struct.
//!
//! We want our generated code to be as simple as possible. Rather than have multiple accessors as
//! get_foo, set_foo, has_foo, disown_foo, adopt_foo, etc, we provide two accessor functions:
//! foo and foo_mut. foo returns an accessor suitable for reading a value, foo_mut returns an
//! accessor suitable for mutating a value. This lets us give users tons of different ways of
//! accessing values without having to make an equal number of accessors. It's still as fast as
//! having a standard accessor, but gives us (the library) more control, which means we can offer
//! greater flexibility. For example, these returned accessors can stored locally which lets us do
//! things like:
//!
//! ```
//! let foo = ty.foo_mut();
//! foo.set(foo.get() + 1);
//! ```
//!
//! Alternatively, we could implement traits over the accessor, letting users use arithmetic ops
//!
//! ```
//! ty.foo_mut() += 1;
//! ```
//!
//! Or directly turn a list accessor into an iterator without calling another accessor function
//!
//! ```
//! for i in ty.bar() {}
//! ```
//!
//! while still letting the user be strict on validation
//!
//! ```
//! for i in ty.bar().try_get()? {}
//! ```
//!
//! In most of this API you'll see lots of explicit references to lifetimes `'b` and `'p`.
//! These lifetimes are short for 'borrow' and 'pointer' and are used for the borrow and pointer
//! lifetimes respectively. So `&'b ptr::StructReader<'p, T>` could be extended to
//! `&'borrow ptr::StructReader<'pointer, T>`.

use core::convert::{TryFrom, TryInto};
use core::marker::PhantomData;
use core::str::Utf8Error;

use crate::any::{AnyList, AnyPtr, AnyStruct};
use crate::data::Data;
use crate::internal::Sealed;
use crate::list::{self, ElementSize, List, InfalliblePtrs};
use crate::ptr::{self, ElementCount, ErrorHandler, CopySize, IgnoreErrors, StructSize, UnwrapErrors};
use crate::rpc::{Capable, Empty, InsertableInto, Table};
use crate::text::Text;
use crate::ty::{self, EnumResult, ListValue, Value, StructView, StructReader as _};
use crate::{any, data, text, NotInSchema, Error, Family, Result, ErrorKind};

pub type StructReader<'b, 'p, T> = &'b ptr::StructReader<'p, T>;
pub type StructBuilder<'b, 'p, T> = &'b mut ptr::StructBuilder<'p, T>;

/// Describes a type that can be a field in a Cap'n Proto struct type.
pub trait FieldType: 'static {
    /// A type used to describe the field type. This will likely be a type of FieldInfo, except
    /// for Void and groups, which have no descriptor.
    type Descriptor;

    /// The result of getting a read-only accessor to a field of this type.
    ///
    /// For void, this is a unit value.
    ///
    /// In the case of primitives, this will be the primitive value, since there's nothing
    /// else you can do when reading like error handling.
    ///
    /// For groups, this is the group type, since the group is just returned directly.
    ///
    /// For pointer fields this will be a wrapper "FieldReader".
    type Reader<'b, 'p: 'b, T: Table + 'p>;
    
    /// Returns a view of the value over the given representation given the specified descriptor.
    ///
    /// Note: this function is UNSAFE. The returned view makes a number of assumptions about the
    /// given descriptor in the name of performance. The descriptor:
    ///
    /// * Must contain a valid default value.
    /// * Must have a slot that is in a struct's data or pointer section bounds.
    ///
    /// Failure to follow these constraints will result in ***undefined behavior***.
    unsafe fn reader<'b, 'p: 'b, T: Table + 'p>(a: StructReader<'b, 'p, T>, descriptor: &'static Descriptor<Self>) -> Self::Reader<'b, 'p, T>;

    /// The result of getting a read-write accessor to a field of this type.
    type Builder<'b, 'p: 'b, T: Table + 'p>;

    /// Returns a view of the value over the given representation given the specified descriptor.
    ///
    /// Note: this function is UNSAFE. The returned view makes a number of assumptions about the
    /// given descriptor in the name of performance. The descriptor:
    ///
    /// * Must contain a valid default value.
    /// * Must have a slot that is in a struct's data or pointer section bounds.
    ///
    /// Failure to follow these constraints will result in ***undefined behavior***.
    unsafe fn builder<'b, 'p: 'b, T: Table + 'p>(a: StructBuilder<'b, 'p, T>, descriptor: &'static Descriptor<Self>) -> Self::Builder<'b, 'p, T>;
}

/// An alias that makes it easier to name the descriptor type for a given type.
pub type Descriptor<V> = <V as FieldType>::Descriptor;

/// A trait used to abstract away how a field type is viewed. This is primarily used for unions,
/// where we want one type to represent the union in generated code which would be generic over
/// T where T is either a reference to an Accessable type or a mutable reference to an
/// AccessableMut type.
///
/// ```
/// pub enum Which<T: Viewable> {
///     Foo(ViewOf<T, u32>),
///     Bar(ViewOf<T, u64>),
/// }
/// ```
///
/// The trait Viewable effectively serves as a type selector. If T is a &'a impl Accessable, we
/// get a new effective definition of
///
/// ```
/// pub enum Which<'a, T: Accessable> {
///     Foo(Accessor<'a, T, u32>),
///     Bar(Accessor<'a, T, u64>),
/// }
/// ```
///
/// and following this flow of logic, gives us an enum of
///
/// ```
/// pub enum Which {
///     Foo(u32),
///     Bar(u64),
/// }
/// ```
///
/// But it also works the other way. If T is a &'a mut impl AccessableMut, we get a definition of
///
/// ```
/// pub enum Which<'a, T: AccessableMut> {
///     Foo(AccessorMut<'a, T, u32>),
///     Bar(AccessorMut<'a, T, u64>),
/// }
/// ```
///
/// which evaluates to
///
/// ```
/// pub enum Which<'a, T: AccessableMut> {
///     Foo(Field<'a, u32, &'a mut T>),
///     Bar(Field<'a, u64, &'a mut T>),
/// }
/// ```
pub trait Viewable {
    type ViewOf<V: FieldType>;
}

impl Viewable for Family {
    type ViewOf<V: FieldType> = ();
}

impl<'b, 'p: 'b, T: Table + 'p> Viewable for StructReader<'b, 'p, T> {
    type ViewOf<V: FieldType> = V::Reader<'b, 'p, T>;
}

impl<'b, 'p: 'b, T: Table + 'p> Viewable for StructBuilder<'b, 'p, T> {
    type ViewOf<V: FieldType> = V::Builder<'b, 'p, T>;
}

impl<'a, T> Viewable for &'a T
where
    T: ty::TypedPtr,
    &'a T::Ptr: Viewable,
{
    type ViewOf<V: FieldType> = <&'a T::Ptr as Viewable>::ViewOf<V>;
}

impl<'a, T> Viewable for &'a mut T
where
    T: ty::TypedPtr,
    &'a mut T::Ptr: Viewable,
{
    type ViewOf<V: FieldType> = <&'a mut T::Ptr as Viewable>::ViewOf<V>;
}

/// Gets the view type of field type V over `Viewable` type T
pub type ViewOf<T, V> = <T as Viewable>::ViewOf<V>;

pub type Accessor<'b, 'p, T, V> = <V as FieldType>::Reader<'b, 'p, T>;

pub type AccessorMut<'b, 'p, T, V> = <V as FieldType>::Builder<'b, 'p, T>;

pub type Variant<'b, 'p, T, V> = VariantField<V, StructReader<'b, 'p, T>>;

pub type VariantMut<'b, 'p, T, V> = VariantField<V, StructBuilder<'b, 'p, T>>;

/// Describes a value in a "slot" in a struct. This does not include groups or void, which don't
/// have an associated default value or slot.
pub struct FieldInfo<V: ty::Value> {
    pub slot: u32,
    pub default: V::Default,
}

/// Describes a variant in a union in a struct. Because this is not attached to a value, it can
/// also be used as group info, as groups aren't values.
pub struct VariantInfo {
    pub slot: u32,
    pub case: u16,
}

/// Describes a value in a union variant in a struct.
pub struct VariantDescriptor<V: FieldType> {
    pub variant: VariantInfo,
    pub field: Descriptor<V>,
}

pub trait UnionViewer<A> {
    type View;

    unsafe fn get(accessable: A) -> Result<Self::View, NotInSchema>;
}

/// A wrapper type used to implement methods for struct fields.
pub struct Struct<S: ty::Struct> {
    s: PhantomData<fn() -> S>,
}

impl<S: ty::Struct> Sealed for Struct<S> {}
impl<S: ty::Struct> Value for Struct<S> {
    type Default = ptr::StructReader<'static>;
}
impl<S: ty::Struct> ListValue for Struct<S> {
    const ELEMENT_SIZE: ElementSize = ElementSize::InlineComposite(S::SIZE);
}

/// Describes a group of fields. This is primarily used for clearing unions and groups of fields within a struct.
pub trait FieldGroup: StructView {}

/// Represents a group of fields in a Cap'n Proto struct.
pub struct Group<G: FieldGroup> {
    v: PhantomData<fn() -> G>,
}

impl<G: FieldGroup> FieldType for Group<G> {
    type Descriptor = ();

    type Reader<'b, 'p: 'b, T: Table + 'p> = G::Reader<'p, T>;

    #[inline]
    unsafe fn reader<'b, 'p: 'b, T: Table + 'p>(a: StructReader<'b, 'p, T>, _: &'static Descriptor<Self>) -> G::Reader<'p, T> {
        ty::StructReader::from_ptr(a.clone())
    }

    type Builder<'b, 'p: 'b, T: Table + 'p> = G::Builder<'b, T>;

    #[inline]
    unsafe fn builder<'b, 'p: 'b, T: Table + 'p>(a: StructBuilder<'b, 'p, T>, _: &'static Descriptor<Self>) -> G::Builder<'b, T> {
        ty::StructBuilder::from_ptr(a.by_ref())
    }
}

pub struct Capability<C: ty::Capability> {
    c: PhantomData<fn() -> C>,
}

impl<C: ty::Capability> Sealed for Capability<C> {}
impl<C: ty::Capability> Value for Capability<C> {
    /// Capabilities don't have defaults
    type Default = ();
}
impl<C: ty::Capability> ListValue for Capability<C> {
    const ELEMENT_SIZE: ElementSize = ElementSize::Pointer;
}

pub struct Field<V: FieldType, Repr> {
    descriptor: &'static Descriptor<V>,
    repr: Repr,
}

pub type FieldReader<'b, 'p, T, V> = Field<V, StructReader<'b, 'p, T>>;
pub type FieldBuilder<'b, 'p, T, V> = Field<V, StructBuilder<'b, 'p, T>>;

impl<'b, 'p: 'b, T: Table + 'p, V> FieldBuilder<'b, 'p, T, V>
where
    V: FieldType,
{
    /// Create a new field builder "by reference". This allows a field builder to be reused
    /// as many builder functions consume the builder.
    #[inline]
    pub fn by_ref<'c>(&'c mut self) -> FieldBuilder<'c, 'p, T, V> {
        Field {
            descriptor: self.descriptor,
            repr: &mut *self.repr,
        }
    }
}

/// A base type for accessing union variant fields.
pub struct VariantField<V: FieldType, Repr> {
    descriptor: &'static VariantDescriptor<V>,
    repr: Repr,
}

impl<V: FieldType, Repr> VariantField<V, Repr> {
    pub unsafe fn new(repr: Repr, descriptor: &'static VariantDescriptor<V>) -> Self {
        Self { descriptor, repr }
    }
}

impl<'b, 'p: 'b, T: Table + 'p, V> Variant<'b, 'p, T, V>
where
    V: FieldType,
{
    /// Returns a bool indicating whether or not this field is set in the union
    #[inline]
    pub fn is_set(&self) -> bool {
        let VariantInfo { slot, case } = self.descriptor.variant;
        self.repr.data_field::<u16>(slot as usize) == case
    }

    /// Returns the underlying accessor for this field, or None if the field isn't set.
    #[inline]
    pub fn field(&self) -> Option<Accessor<'b, 'p, T, V>> {
        if self.is_set() {
            unsafe { Some(self.field_unchecked()) }
        } else {
            None
        }
    }

    /// Returns the underlying accessor for this field without checking if the field is set
    #[inline]
    pub unsafe fn field_unchecked(&self) -> Accessor<'b, 'p, T, V> {
        V::reader(&*self.repr, &self.descriptor.field)
    }
}

impl<'b, 'p: 'b, T: Table + 'p, V> VariantMut<'b, 'p, T, V>
where
    V: FieldType,
{
    /// Returns a bool indicating whether or not this field is set in the union
    #[inline]
    pub fn is_set(&self) -> bool {
        let VariantInfo { slot, case } = self.descriptor.variant;
        unsafe { self.repr.data_field_unchecked::<u16>(slot as usize) == case }
    }

    /// Borrows the variant field by reference, allowing the use of multiple builder methods
    /// without consuming the variant accessor.
    #[inline]
    pub fn by_ref<'c>(&'c mut self) -> VariantMut<'c, 'p, T, V> {
        VariantField {
            repr: &mut *self.repr,
            descriptor: self.descriptor,
        }
    }

    #[inline]
    pub fn field(self) -> Option<AccessorMut<'b, 'p, T, V>> {
        if self.is_set() {
            unsafe { Some(self.field_unchecked()) }
        } else {
            None
        }
    }

    /// Returns a field accessor for the field without checking if it's already set.
    #[inline]
    pub unsafe fn field_unchecked(self) -> AccessorMut<'b, 'p, T, V> {
        V::builder(self.repr, &self.descriptor.field)
    }

    #[inline]
    pub fn init_case(self) -> AccessorMut<'b, 'p, T, V> {
        let VariantInfo { slot, case } = self.descriptor.variant;
        unsafe {
            self.repr.set_field_unchecked(slot as usize, case);
            self.field_unchecked()
        }
    }
}

// impls for Void

impl FieldType for () {
    type Descriptor = ();

    type Reader<'b, 'p: 'b, T: Table + 'p> = ();
    #[inline]
    unsafe fn reader<'b, 'p: 'b, T: Table + 'p>(_: StructReader<'b, 'p, T>, _: &'static Descriptor<Self>) -> () { () }

    type Builder<'b, 'p: 'b, T: Table + 'p> = ();
    #[inline]
    unsafe fn builder<'b, 'p: 'b, T: Table + 'p>(_: StructBuilder<'b, 'p, T>, _: &'static Descriptor<Self>) -> () { () }
}

impl<'b, 'p: 'b, T: Table + 'p> VariantMut<'b, 'p, T, ()> {
    #[inline]
    pub fn set(&mut self) {
        self.by_ref().init_case()
    }
}

// impls data

macro_rules! data_accessable {
    ($($ty:ty),+) => {
        $(
            impl FieldType for $ty {
                type Descriptor = FieldInfo<Self>;

                type Reader<'b, 'p: 'b, T: Table + 'p> = $ty;

                #[inline]
                unsafe fn reader<'b, 'p: 'b, T: Table + 'p>(a: StructReader<'b, 'p, T>, d: &'static Descriptor<Self>) -> Self {
                    a.data_field_with_default(d.slot as usize, d.default)
                }

                type Builder<'b, 'p: 'b, T: Table + 'p> = Field<Self, StructBuilder<'b, 'p, T>>;

                #[inline]
                unsafe fn builder<'b, 'p: 'b, T: Table + 'p>(a: StructBuilder<'b, 'p, T>, d: &'static Descriptor<Self>) -> Field<Self, StructBuilder<'b, 'p, T>> {
                    Field { descriptor: d, repr: a }
                }
            }

            impl<'b, 'p: 'b, T: Table + 'p> FieldBuilder<'b, 'p, T, $ty> {
                #[inline]
                pub fn get(&self) -> $ty {
                    let FieldInfo { slot, default } = *self.descriptor;
                    unsafe { self.repr.data_field_with_default_unchecked(slot as usize, default) }
                }
                #[inline]
                pub fn set(&mut self, value: $ty) {
                    let FieldInfo { slot, default } = *self.descriptor;
                    unsafe { self.repr.set_field_with_default_unchecked(slot as usize, value, default) }
                }
            }

            impl<'b, 'p: 'b, T: Table + 'p> Variant<'b, 'p, T, $ty> {
                #[inline]
                pub fn get_or_default(&self) -> $ty {
                    self.field().unwrap_or(self.descriptor.field.default)
                }
            }

            impl<'b, 'p: 'b, T: Table + 'p> VariantMut<'b, 'p, T, $ty> {
                #[inline]
                pub fn get(&self) -> Option<$ty> {
                    let FieldInfo { slot, default } = self.descriptor.field;
                    if self.is_set() {
                        Some(unsafe { self.repr.data_field_with_default_unchecked(slot as usize, default) })
                    } else {
                        None
                    }
                }
                #[inline]
                pub fn get_or_default(&self) -> $ty {
                    self.get().unwrap_or(self.descriptor.field.default)
                }
                #[inline]
                pub fn set(&mut self, value: $ty) {
                    self.by_ref().init_case().set(value)
                }
            }
        )+
    };
}

data_accessable!(bool, u8, i8, u16, i16, u32, i32, u64, i64, f32, f64);

// impls for enum

/// A wrapper type used to implement methods for enum fields.
pub struct Enum<E: ty::Enum> {
    e: PhantomData<fn() -> E>,
}

impl<E: ty::Enum> Sealed for Enum<E> {}
impl<E: ty::Enum> Value for Enum<E> {
    type Default = E;
}
impl<E: ty::Enum> ListValue for Enum<E> {
    const ELEMENT_SIZE: ElementSize = ElementSize::TwoBytes;
}

impl<E: ty::Enum> FieldType for Enum<E> {
    type Descriptor = FieldInfo<Self>;

    type Reader<'b, 'p: 'b, T: Table + 'p> = EnumResult<E>;

    #[inline]
    unsafe fn reader<'b, 'p: 'b, T: Table + 'p>(s: StructReader<'b, 'p, T>, d: &'static Descriptor<Self>) -> Self::Reader<'b, 'p, T> {
        let FieldInfo { slot, default } = *d;
        s.data_field_with_default::<u16>(slot as usize, default.into()).try_into()
    }

    type Builder<'b, 'p: 'b, T: Table + 'p> = Field<Self, StructBuilder<'b, 'p, T>>;

    #[inline]
    unsafe fn builder<'b, 'p: 'b, T: Table + 'p>(s: StructBuilder<'b, 'p, T>, d: &'static Descriptor<Self>) -> Self::Builder<'b, 'p, T> {
        Field { repr: s, descriptor: d }
    }
}

impl<'b, 'p: 'b, T: Table + 'p, E> FieldBuilder<'b, 'p, T, Enum<E>>
where
    E: ty::Enum,
{
    #[inline]
    pub fn get(&self) -> EnumResult<E> {
        let FieldInfo { slot, default } = *self.descriptor;
        unsafe {
            self.repr
                .data_field_with_default_unchecked(slot as usize, default.into())
                .try_into()
        }
    }
    #[inline]
    pub fn set(&mut self, value: E) {
        let FieldInfo { slot, default } = *self.descriptor;
        let (value, default) = (value.into(), default.into());
        unsafe { self.repr.set_field_with_default_unchecked(slot as usize, value, default) }
    }
}

impl<'b, 'p: 'b, T: Table + 'p, E> Variant<'b, 'p, T, Enum<E>>
where
    E: ty::Enum,
{
    #[inline]
    pub fn get_or_default(&self) -> EnumResult<E> {
        self.field().unwrap_or(Ok(self.descriptor.field.default))
    }
}

impl<'b, 'p: 'b, T: Table + 'p, E> VariantMut<'b, 'p, T, Enum<E>>
where
    E: ty::Enum,
{
    #[inline]
    pub fn get(&self) -> Option<EnumResult<E>> {
        if self.is_set() {
            let FieldInfo { slot, default } = self.descriptor.field;
            Some(unsafe {
                self.repr
                    .data_field_with_default_unchecked::<u16>(slot as usize, default.into())
                    .try_into()
            })
        } else {
            None
        }
    }
    #[inline]
    pub fn get_or_default(&self) -> EnumResult<E> {
        self.get().unwrap_or(Ok(self.descriptor.field.default))
    }
    #[inline]
    pub fn set(&mut self, value: E) {
        self.by_ref().init_case().set(value)
    }
}

/// A macro to make implementing FieldType and AccessableField for non-generic ptr types easy
macro_rules! field_type_items {
    () => {
        type Descriptor = FieldInfo<Self>;

        type Reader<'b, 'p: 'b, T: Table + 'p> = Field<Self, StructReader<'b, 'p, T>>;

        #[inline]
        unsafe fn reader<'b, 'p: 'b, T: Table + 'p>(s: StructReader<'b, 'p, T>, d: &'static Descriptor<Self>) -> Self::Reader<'b, 'p, T> {
            Field { repr: s, descriptor: d }
        }

        type Builder<'b, 'p: 'b, T: Table + 'p> = Field<Self, StructBuilder<'b, 'p, T>>;

        #[inline]
        unsafe fn builder<'b, 'p: 'b, T: Table + 'p>(s: StructBuilder<'b, 'p, T>, d: &'static Descriptor<Self>) -> Self::Builder<'b, 'p, T> {
            Field { repr: s, descriptor: d }
        }
    };
}

// impls for list

impl<V: 'static> FieldType for List<V> {
    field_type_items!{}
}

impl<'b, 'p: 'b, T: Table + 'p, V> FieldReader<'b, 'p, T, List<V>>
where
    V: ty::DynListValue,
{
    #[inline]
    fn default(&self) -> list::Reader<'static, V, T> {
        self.descriptor.default.clone().imbue_from(self.repr)
    }

    #[inline]
    fn ptr(&self) -> ptr::PtrReader<'p, T> {
        self.repr.ptr_field(self.descriptor.slot as u16)
    }

    #[inline]
    pub fn is_null(&self) -> bool {
        self.ptr().is_null()
    }

    #[inline]
    pub fn get(&self) -> list::Reader<'p, V, T> {
        self.get_option().unwrap_or_else(|| self.default())
    }

    #[inline]
    pub fn get_option(&self) -> Option<list::Reader<'p, V, T>> {
        self.try_get_option().ok().flatten()
    }

    #[inline]
    pub fn try_get(&self) -> Result<list::Reader<'p, V, T>> {
        self.try_get_option()
            .map(|op| op.unwrap_or_else(|| self.default()))
    }

    #[inline]
    pub fn try_get_option(&self) -> Result<Option<list::Reader<'p, V, T>>> {
        match self
            .ptr()
            .to_list(Some(V::PTR_ELEMENT_SIZE))
        {
            Ok(Some(ptr)) => Ok(Some(List::new(ptr))),
            Ok(None) => Ok(None),
            Err(err) => Err(err),
        }
    }
}

impl<'b, 'p: 'b, T: Table + 'p, V> FieldReader<'b, 'p, T, List<V>>
where
    V: ty::DynListValue + for<'lb> list::ListAccessable<&'lb list::Reader<'p, V, T>>,
{
    #[inline]
    pub fn iter_by<S>(&self, strat: S) -> list::Iter<'p, V, S, T>
    where
        S: for<'lb> list::IterStrategy<V, list::ElementReader<'p, 'lb, V, T>>,
    {
        self.get().into_iter_by(strat)
    }
}

impl<'b, 'p: 'b, T: Table + 'p, V, Item> IntoIterator for FieldReader<'b, 'p, T, List<V>>
where
    V: ty::DynListValue + for<'lb> list::ListAccessable<&'lb list::Reader<'p, V, T>>,
    InfalliblePtrs: for<'lb> list::IterStrategy<V, list::ElementReader<'p, 'lb, V, T>, Item = Item>,
{
    type IntoIter = list::Iter<'p, V, InfalliblePtrs, T>;
    type Item = Item;

    fn into_iter(self) -> Self::IntoIter {
        self.get().into_iter()
    }
}

impl<'b, 'p: 'b, T: Table + 'p, V> Variant<'b, 'p, T, List<V>>
where
    V: ty::DynListValue,
{
    #[inline]
    fn default(&self) -> list::Reader<'static, V, T> {
        self.descriptor.field.default.clone().imbue_from(self.repr)
    }

    #[inline]
    pub fn is_null(&self) -> bool {
        self.field().map(|field| field.is_null()).unwrap_or(true)
    }

    #[inline]
    pub fn get_or_default(&self) -> list::Reader<'p, V, T> {
        self.try_get_option()
            .ok()
            .flatten()
            .unwrap_or_else(|| self.default())
    }

    #[inline]
    pub fn get_option(&self) -> Option<list::Reader<'p, V, T>> {
        self.try_get_option().ok().flatten()
    }

    #[inline]
    pub fn try_get(&self) -> Result<list::Reader<'p, V, T>> {
        self.try_get_option()
            .map(|op| op.unwrap_or_else(|| self.default()))
    }

    #[inline]
    pub fn try_get_option(&self) -> Result<Option<list::Reader<'p, V, T>>> {
        self.field()
            .map(|r| r.try_get_option())
            .transpose()
            .map(Option::flatten)
    }
}

impl<'b, 'p: 'b, T: Table + 'p, V> FieldBuilder<'b, 'p, T, List<V>>
where
    V: ty::DynListValue,
{
    #[inline]
    fn ptr(&self) -> ptr::PtrReader<T> {
        unsafe { self.repr.ptr_field_unchecked(self.descriptor.slot as u16) }
    }

    #[inline]
    fn ptr_mut(&mut self) -> ptr::PtrBuilder<T> {
        unsafe {
            self.repr
                .ptr_field_mut_unchecked(self.descriptor.slot as u16)
        }
    }

    #[inline]
    fn into_ptr(self) -> ptr::PtrBuilder<'b, T> {
        unsafe {
            self.repr
                .ptr_field_mut_unchecked(self.descriptor.slot as u16)
        }
    }

    /// Returns whether the list has a value
    #[inline]
    pub fn is_null(&self) -> bool {
        self.ptr().is_null()
    }

    #[inline]
    pub fn try_clear<E: ErrorHandler>(&mut self, err_handler: E) -> Result<(), E::Error> {
        self.ptr_mut().try_clear(err_handler)
    }

    #[inline]
    pub fn clear(&mut self) {
        self.ptr_mut().clear()
    }
}

impl<'b, 'p: 'b, T: Table + 'p, V> FieldBuilder<'b, 'p, T, List<V>>
where
    V: ty::ListValue,
{
    /// Returns a builder for the field. If it's not set or a error occurs while reading the
    /// existing value, the default is set instead.
    #[inline]
    pub fn get(self) -> list::Builder<'b, V, T> {
        let default = &self.descriptor.default;
        match self.into_ptr().to_list_mut(Some(V::ELEMENT_SIZE)) {
            Ok(ptr) => List::new(ptr),
            Err((_, ptr)) => {
                assert_eq!(default.as_ref().element_size(), V::ELEMENT_SIZE);
                let mut new_list = ptr.init_list(V::ELEMENT_SIZE, default.as_ref().len());
                new_list.try_copy_from(default.as_ref(), UnwrapErrors).unwrap();
                List::new(new_list)
            }
        }
    }

    #[inline]
    pub fn try_set<E>(
        &mut self,
        value: &list::Reader<V, impl InsertableInto<T>>,
        err_handler: E,
    ) -> Result<(), E::Error>
    where
        E: ErrorHandler,
    {
        self.ptr_mut().try_set_list(value.as_ref(), CopySize::Minimum(V::ELEMENT_SIZE), err_handler)
    }

    #[inline]
    pub fn set(&mut self, value: &list::Reader<V, impl InsertableInto<T>>) {
        self.try_set(value, IgnoreErrors).unwrap()
    }

    #[inline]
    pub fn init(self, count: u32) -> list::Builder<'b, V, T> {
        let count = ElementCount::new(count).expect("too many elements for list");
        List::new(self.into_ptr().init_list(V::ELEMENT_SIZE, count))
    }

    /// Initialize a new instance with the given element size.
    /// 
    /// The element size must be a valid upgrade from `V::ELEMENT_SIZE`. That is, calling
    /// `V::ELEMENT_SIZE.upgrade_to(size)` must yield `Some(size)`.
    #[inline]
    pub fn init_with_size(self, count: u32, size: ElementSize) -> list::Builder<'b, V, T> {
        assert_eq!(V::ELEMENT_SIZE.upgrade_to(size), Some(size));
        let count = ElementCount::new(count).expect("too many elements for list");
        List::new(self.into_ptr().init_list(size, count))
    }
}

impl<'b, 'p: 'b, T: Table + 'p,> FieldBuilder<'b, 'p, T, List<AnyStruct>> {
    /// Returns a builder for the field. If it's not set or a error occurs while reading the
    /// existing value, the default is set instead.
    #[inline]
    pub fn get(self, expected_size: Option<StructSize>) -> list::Builder<'b, AnyStruct, T> {
        let default = self.descriptor.default.as_ref();
        let ptr = match self
            .into_ptr()
            .to_list_mut(expected_size.map(ElementSize::InlineComposite))
        {
            Ok(ptr) => ptr,
            Err((_, ptr)) => {
                let default_size = default.element_size();
                let size = match expected_size {
                    Some(e) => {
                        let expected = ElementSize::InlineComposite(e);
                        default_size.upgrade_to(expected)
                            .expect("default value can't be upgraded to struct list!")
                    },
                    None => default_size,
                };
                let mut builder = ptr.init_list(size, default.len());
                builder.try_copy_from(default, UnwrapErrors).unwrap();
                builder
            }
        };
        List::new(ptr)
    }

    #[inline]
    pub fn init(self, size: StructSize, count: u32) -> list::Builder<'b, AnyStruct, T> {
        let count = ElementCount::new(count).expect("too many elements for list");
        List::new(
            self.into_ptr()
                .init_list(ElementSize::InlineComposite(size), count),
        )
    }
    
    // TODO the rest of the accessors
}

impl<'b, 'p: 'b, T: Table + 'p, V> VariantMut<'b, 'p, T, List<V>>
where
    V: ty::DynListValue,
{
    // TODO acceessors
}

impl<S: ty::Struct> FieldType for Struct<S> {
    field_type_items!{}
}

impl<S: ty::Struct, Repr> Field<Struct<S>, Repr> {
    #[inline]
    fn default_ptr(&self) -> ptr::StructReader<'static> {
        self.descriptor.default.clone()
    }

    /// Returns the default value of the field
    #[inline]
    pub fn default(&self) -> S::Reader<'static, Empty> {
        ty::StructReader::from_ptr(self.default_ptr())
    }
}

impl<'b, 'p: 'b, T: Table + 'p, S> FieldReader<'b, 'p, T, Struct<S>>
where
    S: ty::Struct,
{
    #[inline]
    fn default_imbued_ptr(&self) -> ptr::StructReader<'static, T> {
        self.default_ptr().imbue_from(self.repr)
    }

    #[inline]
    fn ptr(&self) -> ptr::PtrReader<'b, T> {
        self.repr.ptr_field(self.descriptor.slot as u16)
    }

    #[inline]
    pub fn is_null(&self) -> bool {
        self.ptr().is_null()
    }

    #[inline]
    pub fn get(&self) -> S::Reader<'b, T> {
        ty::StructReader::from_ptr(match self.ptr().to_struct() {
            Ok(Some(ptr)) => ptr,
            _ => self.default_imbued_ptr(),
        })
    }

    #[inline]
    pub fn get_option(&self) -> Option<S::Reader<'b, T>> {
        self.try_get_option().ok().flatten()
    }

    #[inline]
    pub fn try_get(&self) -> Result<S::Reader<'b, T>> {
        Ok(ty::StructReader::from_ptr(match self.ptr().to_struct() {
            Ok(Some(ptr)) => ptr,
            Ok(None) => self.default_imbued_ptr(),
            Err(err) => return Err(err),
        }))
    }

    #[inline]
    pub fn try_get_option(&self) -> Result<Option<S::Reader<'b, T>>> {
        match self.ptr().to_struct() {
            Ok(Some(ptr)) => Ok(Some(ty::StructReader::from_ptr(ptr))),
            Ok(None) => Ok(None),
            Err(err) => Err(err),
        }
    }
}

impl<'b, 'p: 'b, T: Table + 'p, S> FieldBuilder<'b, 'p, T, Struct<S>>
where
    S: ty::Struct,
{
    #[inline]
    fn ptr_mut(&mut self) -> ptr::PtrBuilder<T> {
        unsafe {
            self.repr
                .ptr_field_mut_unchecked(self.descriptor.slot as u16)
        }
    }

    #[inline]
    fn into_ptr(self) -> ptr::PtrBuilder<'b, T> {
        unsafe { self.repr.ptr_field_mut_unchecked(self.descriptor.slot as u16) }
    }

    /// Returns a builder for the field. If it's not set or a error occurs while reading the
    /// existing value, the field default is set in its place instead.
    #[inline]
    pub fn get(self) -> S::Builder<'b, T> {
        let default = &self.descriptor.default;
        let ptr = match self.into_ptr().to_struct_mut(Some(S::SIZE)) {
            Ok(ptr) => ptr,
            Err((_, ptr)) => {
                let mut builder = ptr.init_struct(S::SIZE);
                builder.copy_with_caveats(default, false);
                builder
            }
        };
        unsafe { ty::StructBuilder::from_ptr(ptr) }
    }

    #[inline]
    pub fn init(self) -> S::Builder<'b, T> {
        unsafe { ty::StructBuilder::from_ptr(self.into_ptr().init_struct(S::SIZE)) }
    }

    /// Returns a builder for the field. If it's not set or a error occurs while reading the
    /// existing value, the struct default is set instead.
    #[inline]
    pub fn get_or_init(self) -> S::Builder<'b, T> {
        let ptr = self.into_ptr().to_struct_mut_or_init(S::SIZE);
        unsafe { ty::StructBuilder::from_ptr(ptr) }
    }

    /// Try to set this field to a copy of the given value.
    ///
    /// If an error occurs while reading the input value, it's passed to the error handler, which
    /// can choose to write null instead or return an error.
    #[inline]
    pub fn try_set<T2, E>(
        self,
        value: &S::Reader<'_, T2>,
        err_handler: E,
    ) -> Result<(), E::Error>
    where
        T2: InsertableInto<T>,
        E: ErrorHandler,
    {
        self.into_ptr().try_set_struct(value.as_ptr(), ptr::CopySize::Minimum(S::SIZE), err_handler)
    }

    #[inline]
    pub fn set<T2>(self, value: &S::Reader<'_, T2>)
    where
        T2: InsertableInto<T>,
    {
        self.try_set(value, IgnoreErrors).unwrap()
    }

    #[inline]
    pub fn try_clear<E: ErrorHandler>(&mut self, err_handler: E) -> Result<(), E::Error> {
        self.ptr_mut().try_clear(err_handler)
    }

    #[inline]
    pub fn clear(&mut self) {
        self.try_clear(IgnoreErrors).unwrap()
    }
}

impl<'b, 'p: 'b, T: Table + 'p, S> Variant<'b, 'p, T, Struct<S>>
where
    S: ty::Struct,
{
    #[inline]
    fn default(&self) -> S::Reader<'b, T> {
        self.descriptor.field.default.clone().imbue_from(self.repr).into()
    }

    #[inline]
    pub fn is_null(&self) -> bool {
        self.field().map(|field| field.is_null()).unwrap_or(true)
    }

    #[inline]
    pub fn get_or_default(&self) -> S::Reader<'b, T> {
        self.try_get_option()
            .ok()
            .flatten()
            .unwrap_or_else(|| self.default())
    }

    #[inline]
    pub fn get_option(&self) -> Option<S::Reader<'b, T>> {
        self.try_get_option().ok().flatten()
    }

    #[inline]
    pub fn try_get(&self) -> Result<S::Reader<'b, T>> {
        self.try_get_option()
            .map(|op| op.unwrap_or_else(|| self.default()))
    }

    #[inline]
    pub fn try_get_option(&self) -> Result<Option<S::Reader<'b, T>>> {
        self.field()
            .map(|r| r.try_get_option())
            .transpose()
            .map(Option::flatten)
    }
}

impl<'b, 'p: 'b, T: Table + 'p, S> VariantMut<'b, 'p, T, Struct<S>>
where
    S: ty::Struct,
{
    /// Returns a builder for the field. If it's not set or a error occurs while reading the
    /// existing value, the field default is set in its place instead.
    #[inline]
    pub fn get(self) -> S::Builder<'b, T> {
        self.init_case().get()
    }

    #[inline]
    pub fn init(self) -> S::Builder<'b, T> {
        self.init_case().init()
    }

    /// Returns a builder for the field. If it's not set or a error occurs while reading the
    /// existing value, the struct default is set instead.
    #[inline]
    pub fn get_or_init(self) -> S::Builder<'b, T> {
        self.init_case().get_or_init()
    }

    #[inline]
    pub fn try_set<E>(
        self,
        value: &S::Reader<'_, impl InsertableInto<T>>,
        err_handler: E,
    ) -> Result<(), E::Error>
    where
        E: ErrorHandler,
    {
        self.init_case().try_set(value, err_handler)
    }

    #[inline]
    pub fn set(self, value: &S::Reader<'_, impl InsertableInto<T>>) {
        self.init_case().set(value)
    }

    #[inline]
    pub fn try_clear<E: ErrorHandler>(&mut self, err_handler: E) -> Result<(), E::Error> {
        let Some(mut field) = self.by_ref().field() else { return Ok(()) };
        field.try_clear(err_handler)
    }

    #[inline]
    pub fn clear(&mut self) {
        self.try_clear(IgnoreErrors).unwrap()
    }
}

impl FieldType for Text {
    field_type_items!{}
}

impl<Repr> Field<Text, Repr> {
    /// Returns the default value of the field
    #[inline]
    pub fn default(&self) -> text::Reader<'static> {
        text::Reader::new_unchecked(self.descriptor.default)
    }
}

impl<'b, 'p: 'b, T: Table + 'p> FieldReader<'b, 'p, T, Text> {
    #[inline]
    fn ptr(&self) -> ptr::PtrReader<'b, T> {
        self.repr.ptr_field(self.descriptor.slot as u16)
    }

    #[inline]
    pub fn is_null(&self) -> bool {
        self.ptr().is_null()
    }

    #[inline]
    pub fn get(&self) -> text::Reader<'b> {
        self.get_option().unwrap_or_else(|| self.default())
    }

    /// Gets the text field as a string, returning an error if the value isn't valid UTF-8 text.
    /// This is shorthand for `get().as_str()`
    /// 
    /// If the field is null or an error occurs while reading the pointer to the text itself, this
    /// returns the default value.
    #[inline]
    pub fn as_str(&self) -> Result<&'b str, Utf8Error> {
        self.get().as_str()
    }

    #[inline]
    pub fn get_option(&self) -> Option<text::Reader<'b>> {
        self.try_get_option().ok().flatten()
    }

    #[inline]
    pub fn try_get(&self) -> Result<text::Reader<'b>> {
        self.try_get_option()
            .map(|op| op.unwrap_or_else(|| self.default()))
    }

    #[inline]
    pub fn try_get_option(&self) -> Result<Option<text::Reader<'b>>> {
        match self.ptr().to_blob() {
            Ok(Some(ptr)) => {
                let text = text::Reader::new(ptr)
                    .ok_or(Error::from(ErrorKind::TextNotNulTerminated))?;

                Ok(Some(text))
            },
            Ok(None) => Ok(None),
            Err(err) => Err(err),
        }
    }
}

impl<'b, 'p: 'b, T: Table + 'p> FieldBuilder<'b, 'p, T, Text> {
    #[inline]
    fn ptr(&self) -> ptr::PtrReader<T> {
        unsafe { self.repr.ptr_field_unchecked(self.descriptor.slot as u16) }
    }

    #[inline]
    fn ptr_mut(&mut self) -> ptr::PtrBuilder<T> {
        unsafe {
            self.repr
                .ptr_field_mut_unchecked(self.descriptor.slot as u16)
        }
    }

    #[inline]
    fn into_ptr(self) -> ptr::PtrBuilder<'b, T> {
        unsafe { self.repr.ptr_field_mut_unchecked(self.descriptor.slot as u16) }
    }

    #[inline]
    pub fn is_null(&self) -> bool {
        self.ptr().is_null()
    }

    /// Returns a builder for the field. If it's not set or a error occurs while reading the
    /// existing value, the default is set instead.
    #[inline]
    pub fn get(self) -> text::Builder<'b> {
        let default = self.descriptor.default;
        let blob = match self.into_ptr().to_blob_mut() {
            Ok(b) => b,
            Err((_, ptr)) => ptr.set_blob(default),
        };

        text::Builder::new_unchecked(blob)
    }

    /// Creates a text builder with the given length in bytes including the null terminator.
    /// 
    /// # Panics
    /// 
    /// If the length is zero or greater than text::ByteCount::MAX this function will
    /// panic.
    #[inline]
    pub fn init(self, len: u32) -> text::Builder<'b> {
        self.init_byte_count(text::ByteCount::new(len).expect("invalid text field size"))
    }

    #[inline]
    pub fn init_byte_count(self, len: text::ByteCount) -> text::Builder<'b> {
        text::Builder::new_unchecked(self.into_ptr().init_blob(len.into()))
    }

    #[inline]
    pub fn set(self, value: text::Reader) -> text::Builder<'b> {
        text::Builder::new_unchecked(self.into_ptr().set_blob(value.into()))
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
        let Some(len) = u32::try_from(value.len() + 1).ok().and_then(text::ByteCount::new) else {
            return Err(self)
        };

        let mut builder = self.init_byte_count(len);
        builder.as_bytes_mut().copy_from_slice(value.as_bytes());
        Ok(builder)
    }

    #[inline]
    pub fn try_clear<E: ErrorHandler>(&mut self, err_handler: E) -> Result<(), E::Error> {
        self.ptr_mut().try_clear(err_handler)
    }

    #[inline]
    pub fn clear(&mut self) {
        self.ptr_mut().clear()
    }
}

impl<'b, 'p: 'b, T: Table + 'p> Variant<'b, 'p, T, Text> {

}

impl<'b, 'p: 'b, T: Table + 'p> VariantMut<'b, 'p, T, Text> {

}

impl FieldType for Data {
    field_type_items!{}
}

impl<Repr> Field<Data, Repr> {
    /// Returns the default value of the field
    #[inline]
    pub fn default(&self) -> data::Reader<'static> {
        self.descriptor.default.clone().into()
    }
}

impl<'b, 'p, T: Table + 'p> FieldReader<'b, 'p, T, Data> {
    #[inline]
    fn ptr(&self) -> ptr::PtrReader<'b, T> {
        self.repr.ptr_field(self.descriptor.slot as u16)
    }

    #[inline]
    pub fn is_null(&self) -> bool {
        self.ptr().is_null()
    }

    #[inline]
    pub fn get(&self) -> data::Reader<'b> {
        self.get_option().unwrap_or_else(|| self.default())
    }

    #[inline]
    pub fn get_option(&self) -> Option<data::Reader<'b>> {
        self.try_get_option().ok().flatten()
    }

    #[inline]
    pub fn try_get(&self) -> Result<data::Reader<'b>> {
        self.try_get_option()
            .map(|op| op.unwrap_or_else(|| self.default()))
    }

    #[inline]
    pub fn try_get_option(&self) -> Result<Option<data::Reader<'b>>> {
        match self.ptr().to_blob() {
            Ok(Some(ptr)) => Ok(Some(ptr.into())),
            Ok(None) => Ok(None),
            Err(err) => Err(err),
        }
    }
}

impl<'b, 'p: 'b, T: Table + 'p> FieldBuilder<'b, 'p, T, Data> {
    #[inline]
    fn ptr(&self) -> ptr::PtrReader<T> {
        unsafe { self.repr.ptr_field_unchecked(self.descriptor.slot as u16) }
    }

    #[inline]
    fn ptr_mut(&mut self) -> ptr::PtrBuilder<T> {
        unsafe {
            self.repr
                .ptr_field_mut_unchecked(self.descriptor.slot as u16)
        }
    }

    #[inline]
    fn into_ptr(self) -> ptr::PtrBuilder<'b, T> {
        unsafe { self.repr.ptr_field_mut_unchecked(self.descriptor.slot as u16) }
    }

    #[inline]
    pub fn is_null(&self) -> bool {
        self.ptr().is_null()
    }

    /// Returns a builder for the field. If it's not set or a error occurs while reading the
    /// existing value, the default is set instead.
    #[inline]
    pub fn get(self) -> data::Builder<'b> {
        let default = self.descriptor.default;
        match self.into_ptr().to_blob_mut() {
            Ok(data) => data,
            Err((_, ptr)) => ptr.set_blob(default),
        }.into()
    }

    #[inline]
    pub fn init(self, count: ElementCount) -> data::Builder<'b> {
        self.into_ptr().init_blob(count).into()
    }

    #[inline]
    pub fn set(self, value: data::Reader) -> data::Builder<'b> {
        self.into_ptr().set_blob(value.into()).into()
    }

    /// Set the data element to a copy of the given slice.
    /// 
    /// # Panics
    /// 
    /// If the slice is too large to fit in a Cap'n Proto message, this function will
    /// panic.
    #[inline]
    pub fn set_slice(self, value: &[u8]) -> data::Builder<'b> {
        self.try_set_slice(value)
            .ok()
            .expect("slice is too large to fit in a Cap'n Proto message")
    }

    #[inline]
    pub fn try_set_slice(self, value: &[u8]) -> Result<data::Builder<'b>, Self> {
        let len = u32::try_from(value.len()).ok().and_then(ElementCount::new);
        let Some(len) = len else {
            return Err(self)
        };

        let mut builder = self.init(len);
        builder.copy_from_slice(value);
        Ok(builder)
    }

    #[inline]
    pub fn try_clear<E: ErrorHandler>(&mut self, err_handler: E) -> Result<(), E::Error> {
        self.ptr_mut().try_clear(err_handler)
    }

    #[inline]
    pub fn clear(&mut self) {
        self.ptr_mut().clear()
    }
}

impl<'b, 'p: 'b, T: Table + 'p> Variant<'b, 'p, T, Data> {
    // TODO accessors
}

impl<'b, 'p: 'b, T: Table + 'p> VariantMut<'b, 'p, T, Data> {
    // TODO accessors
}

impl FieldType for AnyPtr {
    type Descriptor = FieldInfo<Self>;

    type Reader<'b, 'p: 'b, T: Table + 'p> = any::PtrReader<'b, T>;
    #[inline]
    unsafe fn reader<'b, 'p: 'b, T: Table + 'p>(s: StructReader<'b, 'p, T>, d: &'static Descriptor<Self>) -> Self::Reader<'b, 'p, T> {
        s.ptr_field(d.slot as u16).into()
    }

    type Builder<'b, 'p: 'b, T: Table + 'p> = any::PtrBuilder<'b, T>;
    #[inline]
    unsafe fn builder<'b, 'p: 'b, T: Table + 'p>(s: StructBuilder<'b, 'p, T>, d: &'static Descriptor<Self>) -> Self::Builder<'b, 'p, T> {
        unsafe { s.ptr_field_mut_unchecked(d.slot as u16).into() }
    }
}

impl<'b, 'p: 'b, T: Table + 'p> Variant<'b, 'p, T, AnyPtr> {
    // TODO accessors
}

impl<'b, 'p: 'b, T: Table + 'p> VariantMut<'b, 'p, T, AnyPtr> {
    // TODO accessors
}

impl FieldType for AnyStruct {
    field_type_items!{}
}

impl<Repr> Field<AnyStruct, Repr> {
    /// Returns the default value of the field
    #[inline]
    fn default_ptr(&self) -> ptr::StructReader<'static> {
        self.descriptor.default.clone()
    }

    /// Returns the default value of the field
    #[inline]
    pub fn default(&self) -> any::StructReader<'static> {
        AnyStruct(self.default_ptr())
    }
}

impl<'b, 'p: 'b, T: Table + 'p> FieldReader<'b, 'p, T, AnyStruct> {
    #[inline]
    fn default_imbued(&self) -> any::StructReader<'static, T> {
        self.default().imbue_from(self.repr)
    }

    #[inline]
    fn ptr(&self) -> ptr::PtrReader<'b, T> {
        self.repr.ptr_field(self.descriptor.slot as u16)
    }

    #[inline]
    pub fn is_null(&self) -> bool {
        self.ptr().is_null()
    }

    #[inline]
    pub fn get(&self) -> any::StructReader<'b, T> {
        self.get_option().unwrap_or_else(|| self.default_imbued())
    }

    #[inline]
    pub fn get_option(&self) -> Option<any::StructReader<'b, T>> {
        self.try_get_option().ok().flatten()
    }

    #[inline]
    pub fn try_get(&self) -> Result<any::StructReader<'b, T>> {
        self.try_get_option()
            .map(|op| op.unwrap_or_else(|| self.default_imbued()))
    }

    #[inline]
    pub fn try_get_option(&self) -> Result<Option<any::StructReader<'b, T>>> {
        match self.ptr().to_struct() {
            Ok(Some(ptr)) => Ok(Some(ptr.into())),
            Ok(None) => Ok(None),
            Err(err) => Err(err),
        }
    }
}

impl<'b, 'p: 'b, T: Table + 'p> FieldBuilder<'b, 'p, T, AnyStruct> {
    // TODO accessors
}

impl<'b, 'p: 'b, T: Table + 'p> Variant<'b, 'p, T, AnyStruct> {
    // TODO accessors
}

impl<'b, 'p: 'b, T: Table + 'p> VariantMut<'b, 'p, T, AnyStruct> {
    // TODO accessors
}

impl FieldType for AnyList {
    field_type_items!{}
}

impl<Repr> Field<AnyList, Repr> {
    /// Returns the default value of the field
    #[inline]
    fn default_ptr(&self) -> ptr::ListReader<'static> {
        self.descriptor.default.clone()
    }

    /// Returns the default value of the field
    #[inline]
    pub fn default(&self) -> any::ListReader<'static> {
        self.default_ptr().into()
    }
}

impl<'b, 'p: 'b, T: Table + 'p> FieldReader<'b, 'p, T, AnyList> {
    #[inline]
    fn default_imbued(&self) -> any::ListReader<'static, T> {
        self.default().imbue_from(self.repr)
    }

    #[inline]
    fn ptr(&self) -> ptr::PtrReader<'b, T> {
        self.repr.ptr_field(self.descriptor.slot as u16)
    }

    #[inline]
    pub fn is_null(&self) -> bool {
        self.ptr().is_null()
    }

    #[inline]
    pub fn get(&self) -> any::ListReader<'b, T> {
        self.get_option().unwrap_or_else(|| self.default_imbued())
    }

    #[inline]
    pub fn get_option(&self) -> Option<any::ListReader<'b, T>> {
        self.try_get_option().ok().flatten()
    }

    #[inline]
    pub fn try_get(&self) -> Result<any::ListReader<'b, T>> {
        self.try_get_option()
            .map(|op| op.unwrap_or_else(|| self.default_imbued()))
    }

    #[inline]
    pub fn try_get_option(&self) -> Result<Option<any::ListReader<'b, T>>> {
        match self.ptr().to_list(None) {
            Ok(Some(ptr)) => Ok(Some(ptr.into())),
            Ok(None) => Ok(None),
            Err(err) => Err(err),
        }
    }
}

impl<'b, 'p: 'b, T: Table + 'p> FieldBuilder<'b, 'p, T, AnyList> {
    // TODO accessors
}

impl<'b, 'p: 'b, T: Table + 'p> Variant<'b, 'p, T, AnyList> {
    // TODO accessors
}

impl<'b, 'p: 'b, T: Table + 'p> VariantMut<'b, 'p, T, AnyList> {
    // TODO accessors
}

impl<C: ty::Capability> FieldType for Capability<C> {
    field_type_items!{}
}

impl<'b, 'p, T, C> FieldReader<'b, 'p, T, Capability<C>>
where
    T: Table,
    C: ty::Capability,
{
    // TODO accessors
}

impl<'b, 'p, T, C> FieldBuilder<'b, 'p, T, Capability<C>>
where
    T: Table,
    C: ty::Capability,
{
    // TODO accessors
}

impl<'b, 'p, T, C> Variant<'b, 'p, T, Capability<C>>
where
    T: Table,
    C: ty::Capability,
{
    // TODO accessors
}

impl<'b, 'p, T, C> VariantMut<'b, 'p, T, Capability<C>>
where
    T: Table,
    C: ty::Capability,
{
    // TODO accessors
}
