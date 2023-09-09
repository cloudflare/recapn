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
//! These wouldn't be possible with standard accessor methods (without bloating generated code).
//! But how do we implement them? Well, with a trait of course. We need an abstract way to represent
//! our "accessable" struct implementations. While a Reader is different from a Builder, a reference
//! to a Builder should be functionally the same as a Reader. Like having a &&mut.
//!
//! To that end, the Accessable trait is born (and sibling trait: AccessableMut).
//! Accessable is used to provide a central entrypoint into the library's type system for creating
//! accessors for fields in user structs.
//!
//! Through these types we only need to specify two impl blocks in generated code. One for read-only
//! accessors, and one for read-write.
//!
//! ```
//! impl Foo {
//!     pub const fn bar_descriptor() -> &'static Descriptor<u32> {
//!         &Descriptor<u32> {
//!             slot: 0,
//!             default: 0,
//!         }
//!     }
//! }
//!
//! impl<T: Accessable> Foo<T> {
//!     pub fn bar(&self) -> Accessor<A, u32> {
//!         unsafe { u32::get(self, Foo::bar_descriptor()) }
//!     }
//! }
//!
//! impl<T: AccessableMut> Foo<T> {
//!     pub fn bar_mut(&mut self) -> AccessorMut<A, u32> {
//!         unsafe { u32::get(self, Foo::bar_descriptor()) }
//!     }
//! }
//! ```
//!
//! Oneofs act in the same way, we need one enum to describe a oneof.
//!
//! Given the following defintion
//! ```text
//! struct Foo {
//!     union {
//!         bar @0 :UInt32;
//!         baz @1 :Uint64;
//!     }
//! }
//! ```
//!
//! We get a definition like this:
//! ```
//! pub enum Which<T: Viewable> {
//!     Bar(ViewOf<T, u32>),
//!     Baz(ViewOf<T, u64>),
//! }
//! ```
//! 
//! In most of this API you'll see lots of explicit references to lifetimes `'b` and `'p`.
//! These lifetimes are short for 'borrow' and 'pointer' and are used for the borrow and pointer
//! lifetimes respectively. So `&'b ptr::StructReader<'p, T>` could be extended to
//! `&'borrow ptr::StructReader<'pointer, T>`.

use core::convert::TryInto;
use core::marker::PhantomData;
use core::ops::ControlFlow;
use core::str::Utf8Error;

use crate::alloc::ElementCount;
use crate::any::{AnyList, AnyPtr, AnyStruct};
use crate::data::Data;
use crate::internal::Sealed;
use crate::list::{self, ElementSize, List, ListAccessable, InfalliblePtrs};
use crate::ptr::{self, StructSize, WriteNull};
use crate::rpc::{Capable, Empty, InsertableInto, Table};
use crate::text::Text;
use crate::ty::{self, ListValue, Value, StructView, StructReader as _};
use crate::{any, data, text, NotInSchema, Error, Family, Result};

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
///     Foo(Field<'a, u64, &'a mut T>),
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

pub type EnumResult<E> = Result<E, NotInSchema>;

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

pub struct Field<V: FieldType, Repr> {
    descriptor: &'static Descriptor<V>,
    repr: Repr,
}

pub type FieldReader<'b, 'p, T, V> = Field<V, StructReader<'b, 'p, T>>;
pub type FieldBuilder<'b, 'p, T, V> = Field<V, StructBuilder<'b, 'p, T>>;

impl<'b, 'p: 'b, V, T> FieldBuilder<'b, 'p, T, V>
where
    V: FieldType,
    T: Table + 'p,
{
    #[inline]
    pub fn by_ref<'c>(&'c mut self) -> FieldBuilder<'c, 'p, T, V> {
        Field {
            descriptor: self.descriptor,
            repr: &mut *self.repr,
        }
    }
}

pub struct VariantField<V: FieldType, Repr> {
    descriptor: &'static VariantDescriptor<V>,
    repr: Repr,
}

impl<V: FieldType, Repr> VariantField<V, Repr> {
    pub unsafe fn new(repr: Repr, descriptor: &'static VariantDescriptor<V>) -> Self {
        Self { descriptor, repr }
    }
}

impl<'b, 'p: 'b, T, V> Variant<'b, 'p, T, V>
where
    T: Table + 'p,
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
    pub fn get(&self) -> Option<Accessor<'b, 'p, T, V>> {
        if self.is_set() {
            unsafe { Some(self.get_unchecked()) }
        } else {
            None
        }
    }

    /// Returns the underlying accessor for this field without checking if the field is set
    #[inline]
    pub unsafe fn get_unchecked(&self) -> Accessor<'b, 'p, T, V> {
        V::reader(&*self.repr, &self.descriptor.field)
    }
}

impl<'b, 'p: 'b, T, V> VariantMut<'b, 'p, T, V>
where
    T: Table + 'p,
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
    pub fn accessor(self) -> Option<AccessorMut<'b, 'p, T, V>> {
        if self.is_set() {
            unsafe { Some(self.accessor_unchecked()) }
        } else {
            None
        }
    }

    /// Returns a field accessor for the field without checking if it's already set.
    #[inline]
    pub unsafe fn accessor_unchecked(self) -> AccessorMut<'b, 'p, T, V> {
        V::builder(self.repr, &self.descriptor.field)
    }

    fn init_case(self) -> AccessorMut<'b, 'p, T, V> {
        let VariantInfo { slot, case } = self.descriptor.variant;
        unsafe {
            self.repr.set_field_unchecked(slot as usize, case);
            self.accessor_unchecked()
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
                    self.get().unwrap_or(self.descriptor.field.default)
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

impl<'b, 'p: 'b, E: ty::Enum, T: Table + 'p> FieldBuilder<'b, 'p, T, Enum<E>> {
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

impl<'b, 'p: 'b, E: ty::Enum, T: Table + 'p> Variant<'b, 'p, T, Enum<E>> {
    #[inline]
    pub fn get_or_default(&self) -> EnumResult<E> {
        self.get().unwrap_or(Ok(self.descriptor.field.default))
    }
}

impl<'b, 'p: 'b, E: ty::Enum, T: Table + 'p> VariantMut<'b, 'p, T, Enum<E>> {
    #[inline]
    pub fn get(&self) -> Option<EnumResult<E>> {
        let FieldInfo { slot, default } = self.descriptor.field;
        if self.is_set() {
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

impl<'b, 'p, V, T> FieldReader<'b, 'p, T, List<V>>
where
    V: ty::DynListValue,
    T: Table,
{
    /// Returns the default value of the field
    #[inline]
    pub fn default(&self) -> list::Reader<'static, V, T> {
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
            .to_list(Some(list::ptr::ElementSize::size_of::<V>()))
        {
            Ok(Some(ptr)) => Ok(Some(List::new(ptr))),
            Ok(None) => Ok(None),
            Err(err) => Err(err),
        }
    }
}

impl<'b, 'p, T, V> FieldReader<'b, 'p, T, List<V>>
where
    T: Table,
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

impl<'b, 'p, T, V, Item> IntoIterator for Field<List<V>, StructReader<'b, 'p, T>>
where
    V: ty::DynListValue + for<'lb> list::ListAccessable<&'lb list::Reader<'p, V, T>>,
    InfalliblePtrs: for<'lb> list::IterStrategy<V, list::ElementReader<'p, 'lb, V, T>, Item = Item>,
    T: Table,
{
    type IntoIter = list::Iter<'p, V, InfalliblePtrs, T>;
    type Item = Item;

    fn into_iter(self) -> Self::IntoIter {
        self.get().into_iter()
    }
}

impl<'b, 'p, T, V> Variant<'b, 'p, T, List<V>>
where
    T: Table + 'p,
    V: ty::DynListValue,
{
    #[inline]
    fn default(&self) -> list::Reader<'static, V, T> {
        self.descriptor.field.default.clone().imbue_from(self.repr)
    }

    #[inline]
    pub fn is_null(&self) -> bool {
        self.get().map(|field| field.is_null()).unwrap_or(true)
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
        self.get()
            .map(|r| r.try_get_option())
            .transpose()
            .map(Option::flatten)
    }
}

impl<'b, 'p, T, V> FieldBuilder<'b, 'p, T, List<V>>
where
    T: Table + 'p,
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
    pub fn try_set<T2, F, E>(
        self,
        value: &list::Reader<V, T2>,
        err_handler: F,
    ) -> Result<list::Builder<'b, V, T>, E>
    where
        T2: InsertableInto<T>,
        F: FnMut(Error) -> ControlFlow<E, WriteNull>,
    {
        match self
            .into_ptr()
            .try_set_list(value.as_ref(), false, err_handler)
        {
            Ok(ptr) => Ok(List::new(ptr)),
            Err((err, _)) => Err(err),
        }
    }

    #[inline]
    pub fn set<T2>(self, value: &list::Reader<V, T2>) -> list::Builder<'b, V, T>
    where
        T2: InsertableInto<T>,
    {
        List::new(self.into_ptr().set_list(value.as_ref(), false))
    }

    #[inline]
    pub fn try_clear<F, E>(&mut self, err_handler: F) -> Result<(), E>
    where
        F: FnMut(Error) -> ControlFlow<E, WriteNull>,
    {
        self.ptr_mut().try_clear(err_handler)
    }

    #[inline]
    pub fn clear(&mut self) {
        self.ptr_mut().clear()
    }
}

impl<'b, 'p, T, V> FieldBuilder<'b, 'p, T, List<V>>
where
    T: Table + 'p,
    V: ty::ListValue,
{
    /// Returns a builder for the field. If it's not set or a error occurs while reading the
    /// existing value, the default is set instead.
    #[inline]
    pub fn get(self) -> list::Builder<'b, V, T> {
        let default = &self.descriptor.default;
        let ptr = match self.into_ptr().to_list_mut(Some(V::ELEMENT_SIZE)) {
            Ok(ptr) => ptr,
            Err((_, ptr)) => ptr.set_list(default.as_ref(), false),
        };
        List::new(ptr)
    }

    #[inline]
    pub fn init(self, count: u32) -> list::Builder<'b, V, T> {
        let count = ElementCount::new(count).expect("too many elements for list");
        List::new(self.into_ptr().init_list(V::ELEMENT_SIZE, count))
    }
}

impl<'b, 'p, T> FieldBuilder<'b, 'p, T, List<AnyStruct>>
where
    T: Table + 'p,
{
    /// Returns a builder for the field. If it's not set or a error occurs while reading the
    /// existing value, the default is set instead.
    #[inline]
    pub fn get(self, expected_size: Option<StructSize>) -> list::Builder<'b, AnyStruct, T> {
        let default = &self.descriptor.default;
        let ptr = match self
            .into_ptr()
            .to_list_mut(expected_size.map(ElementSize::InlineComposite))
        {
            Ok(ptr) => ptr,
            Err((_, ptr)) => ptr.set_list(default.as_ref(), false),
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

impl<'b, 'p, T, S> FieldReader<'b, 'p, T, Struct<S>>
where
    T: Table + 'b,
    S: ty::Struct,
{
    #[inline]
    fn default_ptr_imbued(&self) -> ptr::StructReader<'static, T> {
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
        let ptr = self
            .ptr()
            .to_struct()
            .ok()
            .flatten()
            .unwrap_or_else(|| self.default_ptr_imbued());
        ty::StructReader::from_ptr(ptr)
    }

    #[inline]
    pub fn get_option(&self) -> Option<S::Reader<'b, T>> {
        self.try_get_option().ok().flatten()
    }

    #[inline]
    pub fn try_get(&self) -> Result<S::Reader<'b, T>> {
        let ptr = match self.ptr().to_struct() {
            Ok(Some(ptr)) => ptr,
            Ok(None) => self.default_ptr_imbued(),
            Err(err) => return Err(err),
        };
        Ok(ty::StructReader::from_ptr(ptr))
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

impl<'b, 'p, T, S> FieldBuilder<'b, 'p, T, Struct<S>>
where
    T: Table + 'p,
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
            Err((_, ptr)) => ptr.set_struct(default, ptr::CopySize::Minimum(S::SIZE)),
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

    #[inline]
    pub fn try_set<T2, F, E>(
        self,
        value: &S::Reader<'_, T2>,
        err_handler: F,
    ) -> Result<S::Builder<'b, T>, E>
    where
        T2: InsertableInto<T>,
        F: FnMut(Error) -> ControlFlow<E, WriteNull>,
    {
        match self
            .into_ptr()
            .try_set_struct(value.as_ptr(), ptr::CopySize::Minimum(S::SIZE), err_handler)
        {
            Ok(ptr) => Ok(unsafe { ty::StructBuilder::from_ptr(ptr) }),
            Err((err, _)) => Err(err),
        }
    }

    #[inline]
    pub fn set<T2>(self, value: &S::Reader<'_, T2>) -> S::Builder<'b, T>
    where
        T2: InsertableInto<T>,
    {
        unsafe { ty::StructBuilder::from_ptr(self.into_ptr().set_struct(value.as_ptr(), ptr::CopySize::Minimum(S::SIZE))) }
    }

    #[inline]
    pub fn try_clear<F, E>(&mut self, err_handler: F) -> Result<(), E>
    where
        F: FnMut(Error) -> ControlFlow<E, WriteNull>,
    {
        self.ptr_mut().try_clear(err_handler)
    }

    #[inline]
    pub fn clear(&mut self) {
        self.ptr_mut().clear()
    }
}

impl FieldType for Text {
    field_type_items!{}
}

impl<Repr> Field<Text, Repr> {
    /// Returns the default value of the field
    #[inline]
    pub fn default(&self) -> text::Reader<'static> {
        self.descriptor.default
    }
}

impl<'b, 'p, T> FieldReader<'b, 'p, T, Text>
where
    T: Table,
{
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
        match self.ptr().to_text() {
            Ok(Some(ptr)) => Ok(Some(ptr.into())),
            Ok(None) => Ok(None),
            Err(err) => Err(err),
        }
    }
}

impl<'b, 'p, T> FieldBuilder<'b, 'p, T, Text>
where
    T: Table,
{
    #[inline]
    fn ptr_mut(&mut self) -> ptr::PtrBuilder<T> {
        unsafe {
            self.repr
                .ptr_field_mut_unchecked(self.descriptor.slot as u16)
        }
    }

    /// Returns a builder for the field. If it's not set or a error occurs while reading the
    /// existing value, the default is set instead.
    #[inline]
    pub fn get(self) -> text::Builder<'b> {
        todo!()
    }

    #[inline]
    pub fn init(self, count: u32) -> text::Builder<'b> {
        assert!(count < ElementCount::MAX_VALUE, "text too long");
        let count = ElementCount::new(count).unwrap();

        todo!()
    }

    #[inline]
    pub fn set(self, value: &text::Reader) -> text::Builder<'b> {
        todo!()
    }

    #[inline]
    pub fn try_clear<F, E>(&mut self, err_handler: F) -> Result<(), E>
    where
        F: FnMut(Error) -> ControlFlow<E, WriteNull>,
    {
        self.ptr_mut().try_clear(err_handler)
    }

    #[inline]
    pub fn clear(&mut self) {
        self.ptr_mut().clear()
    }
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

impl<'b, 'p, T> FieldReader<'b, 'p, T, Data>
where
    T: Table,
{
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
        match self.ptr().to_data() {
            Ok(Some(ptr)) => Ok(Some(ptr.into())),
            Ok(None) => Ok(None),
            Err(err) => Err(err),
        }
    }
}

impl<'b, 'p, T> FieldBuilder<'b, 'p, T, Data>
where
    T: Table,
{
    #[inline]
    fn ptr_mut(&mut self) -> ptr::PtrBuilder<T> {
        unsafe {
            self.repr
                .ptr_field_mut_unchecked(self.descriptor.slot as u16)
        }
    }

    /// Returns a builder for the field. If it's not set or a error occurs while reading the
    /// existing value, the default is set and returned instead.
    #[inline]
    pub fn get(self) -> data::Builder<'b> {
        todo!()
    }

    #[inline]
    pub fn init(self, count: u32) -> data::Builder<'b> {
        assert!(count < ElementCount::MAX_VALUE, "text too long");
        let count = ElementCount::new(count).unwrap();

        todo!()
    }

    #[inline]
    pub fn set(self, value: &data::Reader) -> data::Builder<'b> {
        todo!()
    }

    #[inline]
    pub fn try_clear<F, E>(&mut self, err_handler: F) -> Result<(), E>
    where
        F: FnMut(Error) -> ControlFlow<E, WriteNull>,
    {
        self.ptr_mut().try_clear(err_handler)
    }

    #[inline]
    pub fn clear(&mut self) {
        self.ptr_mut().clear()
    }
}

impl FieldType for AnyPtr {
    field_type_items!{}
}

impl<Repr> Field<AnyPtr, Repr> {
    /// Returns the default value of the field
    #[inline]
    fn default_ptr(&self) -> ptr::PtrReader<'static> {
        self.descriptor.default.clone()
    }

    /// Returns the default value of the field
    #[inline]
    pub fn default(&self) -> any::PtrReader<'static> {
        self.default_ptr().into()
    }
}

impl<'b, 'p, T> FieldReader<'b, 'p, T, AnyPtr>
where
    T: Table,
{
    #[inline]
    fn default_imbued(&self) -> any::PtrReader<'static, T> {
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

    /// Get the value of the pointer field.
    #[inline]
    pub fn get(&self) -> any::PtrReader<'b, T> {
        self.ptr().into()
    }

    /// Get the value of the pointer field or the default if it's null.
    #[inline]
    pub fn get_or_default(&self) -> any::PtrReader<'b, T> {
        let ptr = self.ptr();
        if ptr.is_null() {
            self.default_imbued()
        } else {
            ptr.into()
        }
    }
}

impl<'b, 'p, T> FieldBuilder<'b, 'p, T, AnyPtr>
where
    T: Table,
{
    
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

impl<'b, 'p, T> FieldReader<'b, 'p, T, AnyStruct>
where
    T: Table,
{
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

impl<'b, 'p, T> FieldBuilder<'b, 'p, T, AnyStruct>
where
    T: Table,
{
    
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

impl<'b, 'p, T> FieldReader<'b, 'p, T, AnyList>
where
    T: Table,
{
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

impl<'b, 'p, T> FieldBuilder<'b, 'p, T, AnyList>
where
    T: Table,
{
    
}

// TODO(soon): Add support for capability fields