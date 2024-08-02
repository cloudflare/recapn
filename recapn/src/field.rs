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
//! ```ignore
//! let foo = ty.foo_mut();
//! foo.set(foo.get() + 1);
//! ```
//!
//! Alternatively, we could implement traits over the accessor, letting users use arithmetic ops
//!
//! ```ignore
//! ty.foo_mut() += 1;
//! ```
//!
//! Or directly turn a list accessor into an iterator without calling another accessor function
//!
//! ```ignore
//! for i in ty.bar() {}
//! ```
//!
//! while still letting the user be strict on validation
//!
//! ```ignore
//! for i in ty.bar().try_get()? {}
//! ```
//!
//! In most of this API you'll see lots of explicit references to lifetimes `'b` and `'p`.
//! These lifetimes are short for 'borrow' and 'pointer' and are used for the borrow and pointer
//! lifetimes respectively. So `&'b ptr::StructReader<'p, T>` could be extended to
//! `&'borrow ptr::StructReader<'pointer, T>`.

use core::convert::TryFrom;
use core::marker::PhantomData;
use core::str::Utf8Error;

use crate::any::{AnyList, AnyPtr, AnyStruct};
use crate::internal::Sealed;
use crate::list::{self, ElementSize, InfalliblePtrs, List, TooManyElementsError};
use crate::orphan::{Orphan, Orphanage};
use crate::ptr::{self, Data, ElementCount, ErrorHandler, CopySize, IgnoreErrors, StructSize, UnwrapErrors};
use crate::rpc::{Capable, Empty, InsertableInto, Table};
use crate::ty::{self, EnumResult, ListValue, StructReader as _};
use crate::{any, data, text, Error, ErrorKind, Family, NotInSchema, Result};

mod internal {
    use super::*;

    pub trait Accessable {
        type Group<G: FieldGroup>;
        unsafe fn group<G: FieldGroup>(self) -> Self::Group<G>;
    
        type Data<D: Data>;
        unsafe fn data<D: Data>(self, info: &'static FieldInfo<D>) -> Self::Data<D>;

        type Enum<E: ty::Enum>;
        unsafe fn enum_value<E: ty::Enum>(self, info: &'static FieldInfo<Enum<E>>) -> Self::Enum<E>;
    }

    impl<'b, 'p, T: Table> Accessable for StructReader<'b, 'p, T> {
        type Group<G: FieldGroup> = G::Reader<'p, T>;
        unsafe fn group<G: FieldGroup>(self) -> Self::Group<G> {
            ty::StructReader::from_ptr(self.clone())
        }
        type Data<D: Data> = D;
        unsafe fn data<D: Data>(self, info: &'static FieldInfo<D>) -> Self::Data<D> {
            self.data_field_with_default(info.slot as usize, info.default)
        }
        type Enum<E: ty::Enum> = EnumResult<E>;
        unsafe fn enum_value<E: ty::Enum>(self, info: &'static FieldInfo<Enum<E>>) -> Self::Enum<E> {
            let &FieldInfo { slot, default } = info;
            let default: u16 = default.into();
            let value = self.data_field_with_default::<u16>(slot as usize, default);
            E::try_from(value)
        }
    }

    impl<'b, 'p, T: Table> Accessable for StructBuilder<'b, 'p, T> {
        type Group<G: FieldGroup> = G::Builder<'b, T>;
        unsafe fn group<G: FieldGroup>(self) -> Self::Group<G> {
            ty::StructBuilder::from_ptr(self.by_ref())
        }
        type Data<D: Data> = DataField<D, Self>;
        unsafe fn data<D: Data>(self, info: &'static FieldInfo<D>) -> Self::Data<D> {
            DataField { descriptor: info, repr: self }
        }
        type Enum<E: ty::Enum> = DataField<Enum<E>, Self>;
        unsafe fn enum_value<E: ty::Enum>(self, info: &'static FieldInfo<Enum<E>>) -> Self::Enum<E> {
            DataField { descriptor: info, repr: self }
        }
    }

    impl<'p, T: Table> Accessable for OwnedStructBuilder<'p, T> {
        type Group<G: FieldGroup> = G::Builder<'p, T>;
        unsafe fn group<G: FieldGroup>(self) -> Self::Group<G> {
            ty::StructBuilder::from_ptr(self)
        }
        type Data<D: Data> = DataField<D, Self>;
        unsafe fn data<D: Data>(self, info: &'static FieldInfo<D>) -> Self::Data<D> {
            DataField { descriptor: info, repr: self }
        }
        type Enum<E: ty::Enum> = DataField<Enum<E>, Self>;
        unsafe fn enum_value<E: ty::Enum>(self, info: &'static FieldInfo<Enum<E>>) -> Self::Enum<E> {
            DataField { descriptor: info, repr: self }
        }
    }
}

use internal::Accessable;

pub type StructReader<'b, 'p, T> = &'b ptr::StructReader<'p, T>;
pub type StructBuilder<'b, 'p, T> = &'b mut ptr::StructBuilder<'p, T>;
use ptr::StructBuilder as OwnedStructBuilder;

/// Describes a group of fields. This is primarily used for clearing unions and groups of fields within a struct.
pub trait FieldGroup: ty::StructView {
    unsafe fn clear<'b, 'p, T: Table>(a: StructBuilder<'b, 'p, T>);
}

/// Describes a type that can be a field in a Cap'n Proto struct type.
pub trait FieldType: Sized + 'static {
    /// A type used to describe the field type. This will likely be a type of FieldInfo, except
    /// for Void and groups, which have no descriptor.
    type Descriptor;

    /// Selects the type of accessor used by this type.
    type Accessor<A: Accessable>;
    unsafe fn accessor<A: Accessable>(a: A, descriptor: &'static Self::Descriptor) -> Self::Accessor<A>;

    type VariantAccessor<A: Accessable>;
    unsafe fn variant<A: Accessable>(a: A, descriptor: &'static VariantDescriptor<Self>) -> Self::VariantAccessor<A>;

    unsafe fn clear<'a, 'b, T: Table>(a: StructBuilder<'b, 'a, T>, descriptor: &'static Self::Descriptor);
}

impl FieldType for () {
    type Descriptor = ();
    type Accessor<A: Accessable> = ();
    #[inline]
    unsafe fn accessor<A: Accessable>(_: A, _: &'static Self::Descriptor) -> Self::Accessor<A> { () }
    type VariantAccessor<A: Accessable> = VoidVariant<Self, A>;
    #[inline]
    unsafe fn variant<A: Accessable>(a: A, descriptor: &'static VariantDescriptor<Self>) -> Self::VariantAccessor<A> {
        VoidVariant { t: PhantomData, variant: &descriptor.variant, repr: a }
    }

    #[inline]
    unsafe fn clear<'b, 'p, T: Table>(_: StructBuilder<'b, 'p, T>, _: &'static Self::Descriptor) {}
}

impl<D: Data> FieldType for D {
    type Descriptor = FieldInfo<D>;
    type Accessor<A: Accessable> = A::Data<D>;
    #[inline]
    unsafe fn accessor<A: Accessable>(a: A, descriptor: &'static Self::Descriptor) -> Self::Accessor<A> {
        a.data(descriptor)
    }
    type VariantAccessor<A: Accessable> = DataVariant<D, A>;
    #[inline]
    unsafe fn variant<A: Accessable>(a: A, descriptor: &'static VariantDescriptor<Self>) -> Self::VariantAccessor<A> {
        DataVariant { repr: a, descriptor: &descriptor.field, variant: &descriptor.variant }
    }
    #[inline]
    unsafe fn clear<'a, 'b, T: Table>(a: StructBuilder<'b, 'a, T>, descriptor: &'static Self::Descriptor) {
        a.set_field_with_default_unchecked(descriptor.slot as usize, descriptor.default, descriptor.default)
    }
}

/// An alias that makes it easier to name the descriptor type for a given type.
pub type Descriptor<V> = <V as FieldType>::Descriptor;

/// A trait used to abstract away how a field type is viewed. This is primarily used for unions,
/// where we want one type to represent the union in generated code which would be generic over
/// T where T is either a reference to an Accessable type or a mutable reference to an
/// AccessableMut type.
///
/// ```
/// # use recapn::prelude::gen::*;
/// pub enum Which<T: Viewable> {
///     Foo(ViewOf<T, u32>),
///     Bar(ViewOf<T, u64>),
/// }
/// ```
///
/// The trait Viewable effectively serves as a type selector. If T is a &'a impl Accessable, we
/// get a new effective definition of
///
/// ```ignore
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
/// ```ignore
/// pub enum Which<'a, T: AccessableMut> {
///     Foo(AccessorMut<'a, T, u32>),
///     Bar(AccessorMut<'a, T, u64>),
/// }
/// ```
///
/// which evaluates to
///
/// ```ignore
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

impl<'b, 'p, T: Table> Viewable for StructReader<'b, 'p, T> {
    type ViewOf<V: FieldType> = V::Accessor<Self>;
}

impl<'b, 'p, T: Table> Viewable for StructBuilder<'b, 'p, T> {
    type ViewOf<V: FieldType> = V::Accessor<Self>;
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

pub type Accessor<'b, 'p, T, V> = <V as FieldType>::Accessor<StructReader<'b, 'p, T>>;

pub type AccessorMut<'b, 'p, T, V> = <V as FieldType>::Accessor<StructBuilder<'b, 'p, T>>;

pub type AccessorOwned<'p, T, V> = <V as FieldType>::Accessor<ptr::StructBuilder<'p, T>>;

pub type Variant<'b, 'p, T, V> = <V as FieldType>::VariantAccessor<StructReader<'b, 'p, T>>;

pub type VariantMut<'b, 'p, T, V> = <V as FieldType>::VariantAccessor<StructBuilder<'b, 'p, T>>;

pub type VariantOwned<'p, T, V> = <V as FieldType>::VariantAccessor<ptr::StructBuilder<'p, T>>;

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
impl<S: ty::Struct> ty::Value for Struct<S> {
    type Default = ptr::StructReader<'static>;
}
impl<S: ty::Struct> ListValue for Struct<S> {
    const ELEMENT_SIZE: ElementSize = ElementSize::InlineComposite(S::SIZE);
}

/// Represents a group of fields in a Cap'n Proto struct.
pub struct Group<G: FieldGroup> {
    v: PhantomData<fn() -> G>,
}

impl<G: FieldGroup> FieldType for Group<G> {
    type Descriptor = ();
    type Accessor<A: Accessable> = A::Group<G>;
    #[inline]
    unsafe fn accessor<A: Accessable>(a: A, _: &'static Self::Descriptor) -> Self::Accessor<A> {
        unsafe { a.group::<G>() }
    }
    type VariantAccessor<A: Accessable> = VoidVariant<Self, A>;
    #[inline]
    unsafe fn variant<A: Accessable>(a: A, descriptor: &'static VariantDescriptor<Self>) -> Self::VariantAccessor<A> {
        VoidVariant { t: PhantomData, variant: &descriptor.variant, repr: a }
    }
    #[inline]
    unsafe fn clear<'a, 'b, T: Table>(a: StructBuilder<'b, 'a, T>, _: &'static Self::Descriptor) {
        G::clear(a)
    }
}

pub struct Capability<C: ty::Capability> {
    c: PhantomData<fn() -> C>,
}

impl<C: ty::Capability> Sealed for Capability<C> {}
impl<C: ty::Capability> ty::Value for Capability<C> {
    /// Capabilities don't have defaults
    type Default = ();
}
impl<C: ty::Capability> ListValue for Capability<C> {
    const ELEMENT_SIZE: ElementSize = ElementSize::Pointer;
}

/// A wrapper type used to implement methods for enum fields.
pub struct Enum<E: ty::Enum> {
    e: PhantomData<fn() -> E>,
}

impl<E: ty::Enum> Sealed for Enum<E> {}
impl<E: ty::Enum> ty::Value for Enum<E> {
    type Default = E;
}
impl<E: ty::Enum> ListValue for Enum<E> {
    const ELEMENT_SIZE: ElementSize = ElementSize::TwoBytes;
}

/// A variant accessor type for "void" types (`Void` and groups).
/// 
/// This includes not just the Void primitive, but also groups, which are kinda like voids in
/// the sense that they aren't actual "things" on the wire. Void fields take up no encoding space
/// and groups don't take up any encoding space either. They also have no associated data, so they
/// don't have an associated descriptor, just variant info.
pub struct VoidVariant<T, Repr> {
    t: PhantomData<fn() -> T>,
    variant: &'static VariantInfo,
    repr: Repr,
}

impl<'b, 'p, T: Table, U> VoidVariant<U, StructReader<'b, 'p, T>> {
    /// Returns a bool indicating whether or not this field is set in the union
    #[inline]
    pub fn is_set(&self) -> bool {
        let &VariantInfo { slot, case } = self.variant;
        self.repr.data_field::<u16>(slot as usize) == case
    }
}

impl<'b, 'p, T: Table, G: FieldGroup> VoidVariant<Group<G>, StructReader<'b, 'p, T>> {
    #[inline]
    pub fn get(&self) -> Option<G::Reader<'p, T>> {
        if self.is_set() {
            Some(ty::StructReader::from_ptr(self.repr.clone()))
        } else {
            None
        }
    }

    #[inline]
    pub unsafe fn get_unchecked(&self) -> G::Reader<'p, T> {
        ty::StructReader::from_ptr(self.repr.clone())
    }

    pub fn get_or_default(&self) -> G::Reader<'p, T> {
        match self.get() {
            Some(r) => r,
            None => {
                let ptr = ptr::StructReader::empty().imbue_from(self.repr);
                ty::StructReader::from_ptr(ptr)
            }
        }
    }
}

impl<'b, 'p, T: Table, U> VoidVariant<U, StructBuilder<'b, 'p, T>> {
    /// Returns a bool indicating whether or not this field is set in the union
    #[inline]
    pub fn is_set(&self) -> bool {
        let &VariantInfo { slot, case } = self.variant;
        unsafe { self.repr.data_field_unchecked::<u16>(slot as usize) == case }
    }

    #[inline]
    fn set_variant(&mut self) {
        let &VariantInfo { slot, case } = self.variant;
        unsafe { self.repr.set_field_unchecked(slot as usize, case) }
    }
}

impl<'b, 'p, T: Table> VoidVariant<(), StructBuilder<'b, 'p, T>> {
    #[inline]
    pub fn set(&mut self) {
        self.set_variant();
    }
}

impl<'b, 'p, T: Table, G: FieldGroup> VoidVariant<Group<G>, StructBuilder<'b, 'p, T>> {
    #[inline]
    pub fn get(self) -> Option<G::Builder<'b, T>> {
        if self.is_set() {
            unsafe { Some(ty::StructBuilder::from_ptr(self.repr.by_ref())) }
        } else {
            None
        }
    }

    #[inline]
    pub unsafe fn get_unchecked(self) -> G::Builder<'b, T> {
        ty::StructBuilder::from_ptr(self.repr.by_ref())
    }

    #[inline]
    pub fn set(mut self) -> G::Builder<'b, T> {
        self.set_variant();
        unsafe {
            G::clear(self.repr);
            ty::StructBuilder::from_ptr(self.repr.by_ref())
        }
    }
}

pub struct DataField<D: ty::Value, Repr> {
    descriptor: &'static FieldInfo<D>,
    repr: Repr,
}

impl<'b, 'p, T: Table, D: Data> DataField<D, StructBuilder<'b, 'p, T>> {
    #[inline]
    pub fn get(&self) -> D {
        unsafe { self.repr.data_field_with_default_unchecked(self.descriptor.slot as usize, self.descriptor.default) }
    }

    #[inline]
    pub fn set(&mut self, value: D) {
        unsafe { self.repr.set_field_with_default_unchecked(self.descriptor.slot as usize, value, self.descriptor.default) }
    }
}

pub struct DataVariant<D: ty::Value, Repr> {
    descriptor: &'static FieldInfo<D>,
    variant: &'static VariantInfo,
    repr: Repr,
}

impl<'b, 'p, T: Table, D: Data> DataVariant<D, StructReader<'b, 'p, T>> {
    /// Returns a bool indicating whether or not this field is set in the union
    #[inline]
    pub fn is_set(&self) -> bool {
        let &VariantInfo { slot, case } = self.variant;
        self.repr.data_field::<u16>(slot as usize) == case
    }

    #[inline]
    pub fn get(&self) -> Option<D> {
        let &FieldInfo { slot, default } = self.descriptor;
        if self.is_set() {
            Some(self.repr.data_field_with_default(slot as usize, default))
        } else {
            None
        }
    }

    #[inline]
    pub fn get_or_default(&self) -> D {
        self.get().unwrap_or(self.descriptor.default)
    }
}

impl<'b, 'p, T: Table, D: Data> DataVariant<D, StructBuilder<'b, 'p, T>> {
    /// Returns a bool indicating whether or not this field is set in the union
    #[inline]
    pub fn is_set(&self) -> bool {
        let &VariantInfo { slot, case } = self.variant;
        self.repr.data_field::<u16>(slot as usize) == case
    }

    #[inline]
    pub fn get(&self) -> Option<D> {
        let &FieldInfo { slot, default } = self.descriptor;
        if self.is_set() {
            Some(unsafe { self.repr.data_field_with_default_unchecked(slot as usize, default) })
        } else {
            None
        }
    }

    #[inline]
    pub fn get_or_default(&self) -> D {
        self.get().unwrap_or(self.descriptor.default)
    }

    #[inline]
    pub fn set(&mut self, value: D) {
        let &VariantInfo { slot: variant_slot, case } = self.variant;
        let &FieldInfo { slot, default } = self.descriptor;
        unsafe {
            self.repr.set_field_unchecked(variant_slot as usize, case);
            self.repr.set_field_with_default_unchecked(slot as usize, value, default);
        }
    }
}

impl<E: ty::Enum> FieldType for Enum<E> {
    type Descriptor = FieldInfo<Self>;

    type Accessor<A: Accessable> = A::Enum<E>;
    unsafe fn accessor<A: Accessable>(a: A, descriptor: &'static Self::Descriptor) -> Self::Accessor<A> {
        a.enum_value(descriptor)
    }

    type VariantAccessor<A: Accessable> = DataVariant<Self, A>;
    unsafe fn variant<A: Accessable>(a: A, descriptor: &'static VariantDescriptor<Self>) -> Self::VariantAccessor<A> {
        DataVariant { descriptor: &descriptor.field, variant: &descriptor.variant, repr: a }
    }

    #[inline]
    unsafe fn clear<'a, 'b, T: Table>(a: StructBuilder<'b, 'a, T>, descriptor: &'static Self::Descriptor) {
        a.set_field_unchecked::<u16>(descriptor.slot as usize, 0);
    }
}

impl<'b, 'p, T: Table, E: ty::Enum> DataField<Enum<E>, StructBuilder<'b, 'p, T>> {
    #[inline]
    pub fn get(&self) -> EnumResult<E> {
        let &FieldInfo { slot, default } = self.descriptor;
        let default: u16 = default.into();
        let value = unsafe { self.repr.data_field_with_default_unchecked::<u16>(slot as usize, default) };
        E::try_from(value)
    }

    #[inline]
    pub fn set(&mut self, value: E) {
        self.set_value(value.into())
    }

    /// Set a field to the given value, even if the value can't be represented by the enum type.
    #[inline]
    pub fn set_value(&mut self, value: u16) {
        let &FieldInfo { slot, default } = self.descriptor;
        let default: u16 = default.into();
        unsafe { self.repr.set_field_with_default_unchecked(slot as usize, value, default) }
    }
}

impl<'b, 'p, T: Table, E: ty::Enum> DataVariant<Enum<E>, StructReader<'b, 'p, T>> {
    /// Returns a bool indicating whether or not this field is set in the union
    #[inline]
    pub fn is_set(&self) -> bool {
        let &VariantInfo { slot, case } = self.variant;
        self.repr.data_field::<u16>(slot as usize) == case
    }

    #[inline]
    pub fn get(&self) -> Option<EnumResult<E>> {
        if self.is_set() {
            let &FieldInfo { slot, default } = self.descriptor;
            let default: u16 = default.into();
            let value = self.repr.data_field_with_default::<u16>(slot as usize, default);
            Some(E::try_from(value))
        } else {
            None
        }
    }

    #[inline]
    pub fn get_or_default(&self) -> EnumResult<E> {
        self.get().unwrap_or(Ok(self.descriptor.default))
    }
}

impl<'b, 'p, T: Table, E: ty::Enum> DataVariant<Enum<E>, StructBuilder<'b, 'p, T>> {
    /// Returns a bool indicating whether or not this field is set in the union
    #[inline]
    pub fn is_set(&self) -> bool {
        let &VariantInfo { slot, case } = self.variant;
        unsafe { self.repr.data_field_unchecked::<u16>(slot as usize) == case }
    }

    #[inline]
    pub fn get(&self) -> Option<EnumResult<E>> {
        if self.is_set() {
            let &FieldInfo { slot, default } = self.descriptor;
            let default: u16 = default.into();
            let value = unsafe { self.repr.data_field_with_default_unchecked(slot as usize, default) };
            Some(E::try_from(value))
        } else {
            None
        }
    }

    #[inline]
    pub fn get_or_default(&self) -> EnumResult<E> {
        self.get().unwrap_or(Ok(self.descriptor.default))
    }

    #[inline]
    pub fn set(&mut self, value: E) {
        let &VariantInfo { slot: variant_slot, case } = self.variant;
        let &FieldInfo { slot, default } = self.descriptor;
        let value: u16 = value.into();
        let default: u16 = default.into();
        unsafe {
            self.repr.set_field_unchecked(variant_slot as usize, case);
            self.repr.set_field_with_default_unchecked(slot as usize, value, default);
        }
    }
}

pub struct PtrField<V: ty::Value, Repr> {
    descriptor: &'static FieldInfo<V>,
    repr: Repr,
}

pub type PtrFieldReader<'b, 'p, T, V> = PtrField<V, StructReader<'b, 'p, T>>;
pub type PtrFieldBuilder<'b, 'p, T, V> = PtrField<V, StructBuilder<'b, 'p, T>>;
pub type PtrFieldOwner<'p, T, V> = PtrField<V, OwnedStructBuilder<'p, T>>;

impl<'b, 'p, T: Table, V: ty::Value> PtrFieldReader<'b, 'p, T, V> {
    #[inline]
    fn raw_ptr(&self) -> ptr::PtrReader<'p, T> {
        self.repr.ptr_field(self.descriptor.slot as u16)
    }

    #[inline]
    pub fn ptr(&self) -> any::PtrReader<'p, T> {
        self.raw_ptr().into()
    }

    #[inline]
    pub fn is_null(&self) -> bool {
        self.ptr().is_null()
    }
}

impl<'b, 'p, T: Table, V: ty::Value> PtrFieldBuilder<'b, 'p, T, V> {
    /// Create a new field builder "by reference". This allows a field builder to be reused
    /// as many builder functions consume the builder.
    #[inline]
    pub fn by_ref<'c>(&'c mut self) -> PtrFieldBuilder<'c, 'p, T, V> {
        PtrField {
            descriptor: self.descriptor,
            repr: &mut *self.repr,
        }
    }

    #[inline]
    fn raw_read_ptr(&self) -> ptr::PtrReader<T> {
        unsafe { self.repr.ptr_field_unchecked(self.descriptor.slot as u16) }
    }

    #[inline]
    fn raw_build_ptr(&mut self) -> ptr::PtrBuilder<T> {
        unsafe { self.repr.ptr_field_mut_unchecked(self.descriptor.slot as u16) }
    }

    #[inline]
    fn into_raw_build_ptr(self) -> ptr::PtrBuilder<'b, T> {
        unsafe { self.repr.ptr_field_mut_unchecked(self.descriptor.slot as u16) }
    }

    #[inline]
    pub fn is_null(&self) -> bool {
        self.raw_read_ptr().is_null()
    }

    /// Build the field as any pointer type.
    #[inline]
    pub fn ptr(self) -> any::PtrBuilder<'b, T> {
        self.into_raw_build_ptr().into()
    }

    #[inline]
    pub fn adopt(&mut self, orphan: Orphan<V, T>) {
        self.raw_build_ptr().adopt(orphan.into_inner());
    }

    #[inline]
    pub fn disown_into<'c>(&mut self, orphanage: &Orphanage<'c, T>) -> Orphan<'c, V, T> {
        Orphan::new(self.raw_build_ptr().disown_into(orphanage))
    }

    #[inline]
    pub fn try_clear<E: ErrorHandler>(&mut self, err_handler: E) -> Result<(), E::Error> {
        self.raw_build_ptr().try_clear(err_handler)
    }

    #[inline]
    pub fn clear(&mut self) {
        self.raw_build_ptr().clear()
    }
}

impl<'p, T: Table, V: ty::Value> PtrFieldOwner<'p, T, V> {
    /// Create a new field builder "by reference". This allows a field builder to be reused
    /// as many builder functions consume the builder.
    #[inline]
    pub fn by_ref<'c>(&'c mut self) -> PtrFieldOwner<'c, T, V> {
        PtrField {
            descriptor: self.descriptor,
            repr: self.repr.by_ref(),
        }
    }

    #[inline]
    fn raw_read_ptr(&self) -> ptr::PtrReader<T> {
        unsafe { self.repr.ptr_field_unchecked(self.descriptor.slot as u16) }
    }

    #[inline]
    fn raw_build_ptr(&mut self) -> ptr::PtrBuilder<T> {
        unsafe { self.repr.ptr_field_mut_unchecked(self.descriptor.slot as u16) }
    }

    #[inline]
    fn into_raw_build_ptr(self) -> ptr::PtrBuilder<'p, T> {
        unsafe { self.repr.into_ptr_field_unchecked(self.descriptor.slot as u16) }
    }

    #[inline]
    pub fn is_null(&self) -> bool {
        self.raw_read_ptr().is_null()
    }

    /// Build the field as any pointer type.
    #[inline]
    pub fn ptr(self) -> any::PtrBuilder<'p, T> {
        self.into_raw_build_ptr().into()
    }

    #[inline]
    pub fn adopt(&mut self, orphan: Orphan<V, T>) {
        self.raw_build_ptr().adopt(orphan.into_inner());
    }

    #[inline]
    pub fn disown_into<'c>(&mut self, orphanage: &Orphanage<'c, T>) -> Orphan<'c, V, T> {
        Orphan::new(self.raw_build_ptr().disown_into(orphanage))
    }

    #[inline]
    pub fn try_clear<E: ErrorHandler>(&mut self, err_handler: E) -> Result<(), E::Error> {
        self.raw_build_ptr().try_clear(err_handler)
    }

    #[inline]
    pub fn clear(&mut self) {
        self.raw_build_ptr().clear()
    }
}

/// A base type for accessing union variant fields.
pub struct PtrVariant<V: ty::Value, Repr> {
    descriptor: &'static FieldInfo<V>,
    variant: &'static VariantInfo,
    repr: Repr,
}

pub type PtrVariantReader<'b, 'p, T, V> = PtrVariant<V, StructReader<'b, 'p, T>>;
pub type PtrVariantBuilder<'b, 'p, T, V> = PtrVariant<V, StructBuilder<'b, 'p, T>>;
pub type PtrVariantOwner<'p, T, V> = PtrVariant<V, OwnedStructBuilder<'p, T>>;

impl<'b, 'p, T: Table, V: ty::Value> PtrVariantReader<'b, 'p, T, V> {
    /// Returns a bool indicating whether or not this field is set in the union
    #[inline]
    pub fn is_set(&self) -> bool {
        let &VariantInfo { slot, case } = self.variant;
        self.repr.data_field::<u16>(slot as usize) == case
    }

    #[inline]
    fn raw_ptr(&self) -> Option<ptr::PtrReader<'b, T>> {
        self.is_set().then(|| self.repr.ptr_field(self.descriptor.slot as u16))
    }

    #[inline]
    pub fn field(&self) -> Option<PtrFieldReader<'b, 'p, T, V>> {
        self.is_set().then(|| PtrField { descriptor: self.descriptor, repr: self.repr })
    }

    #[inline]
    pub fn is_null(&self) -> bool {
        if let Some(f) = self.field() {
            f.is_null()
        } else {
            true
        }
    }
}

impl<'b, 'p, T: Table, V: ty::Value> PtrVariantBuilder<'b, 'p, T, V> {
    /// Returns a bool indicating whether or not this field is set in the union
    #[inline]
    pub fn is_set(&self) -> bool {
        let &VariantInfo { slot, case } = self.variant;
        unsafe { self.repr.data_field_unchecked::<u16>(slot as usize) == case }
    }

    /// Create a new field builder "by reference". This allows a field builder to be reused
    /// as many builder functions consume the builder.
    #[inline]
    pub fn by_ref<'c>(&'c mut self) -> PtrVariantBuilder<'c, 'p, T, V> {
        PtrVariant {
            descriptor: self.descriptor,
            variant: self.variant,
            repr: &mut *self.repr,
        }
    }

    #[inline]
    fn raw_read_ptr(&self) -> Option<ptr::PtrReader<T>> {
        self.is_set().then(|| unsafe { self.repr.ptr_field_unchecked(self.descriptor.slot as u16) })
    }

    #[inline]
    fn raw_build_ptr(&mut self) -> Option<ptr::PtrBuilder<T>> {
        self.is_set().then(|| unsafe { self.repr.ptr_field_mut_unchecked(self.descriptor.slot as u16) })
    }

    #[inline]
    pub fn init_field(self) -> PtrFieldBuilder<'b, 'p, T, V> {
        if !self.is_set() {
            let &VariantInfo { slot, case } = self.variant;
            unsafe {
                self.repr.set_field_unchecked::<u16>(slot as usize, case);
                self.repr.ptr_field_mut_unchecked(self.descriptor.slot as u16).clear();
            }
        }
        PtrField { descriptor: self.descriptor, repr: self.repr }
    }

    #[inline]
    pub fn field(self) -> Option<PtrFieldBuilder<'b, 'p, T, V>> {
        self.is_set().then(|| PtrField { descriptor: self.descriptor, repr: self.repr })
    }

    #[inline]
    pub fn is_null(&self) -> bool {
        if let Some(f) = self.raw_read_ptr() {
            f.is_null()
        } else {
            true
        }
    }

    #[inline]
    pub fn adopt(&mut self, orphan: Orphan<V, T>) {
        if !self.is_set() {
            let &VariantInfo { slot, case } = self.variant;
            unsafe {
                self.repr.set_field_unchecked::<u16>(slot as usize, case);
            }
        }
        let mut ptr = unsafe { self.repr.ptr_field_mut_unchecked(self.descriptor.slot as u16) };
        ptr.adopt(orphan.into_inner());
    }

    #[inline]
    pub fn disown_into<'c>(&mut self, orphanage: &Orphanage<'c, T>) -> Option<Orphan<'c, V, T>> {
        self.raw_build_ptr().map(|mut p| Orphan::new(p.disown_into(orphanage)))
    }

    #[inline]
    pub fn try_clear<E: ErrorHandler>(&mut self, err_handler: E) -> Result<(), E::Error> {
        let Some(mut ptr) = self.raw_build_ptr() else { return Ok(()) };
        ptr.try_clear(err_handler)
    }

    #[inline]
    pub fn clear(&mut self) {
        let Some(mut ptr) = self.raw_build_ptr() else { return };
        ptr.clear()
    }
}

impl<'p, T: Table, V: ty::Value> PtrVariantOwner<'p, T, V> {
    /// Returns a bool indicating whether or not this field is set in the union
    #[inline]
    pub fn is_set(&self) -> bool {
        let &VariantInfo { slot, case } = self.variant;
        unsafe { self.repr.data_field_unchecked::<u16>(slot as usize) == case }
    }
}

/// A macro to make implementing FieldType and AccessableField for non-generic ptr types easy
macro_rules! field_type_items {
    () => {
        type Descriptor = FieldInfo<Self>;

        type Accessor<A: Accessable> = PtrField<Self, A>;
        #[inline]
        unsafe fn accessor<A: Accessable>(a: A, descriptor: &'static Self::Descriptor) -> Self::Accessor<A> {
            PtrField { descriptor, repr: a }
        }

        type VariantAccessor<A: Accessable> = PtrVariant<Self, A>;
        #[inline]
        unsafe fn variant<A: Accessable>(a: A, descriptor: &'static VariantDescriptor<Self>) -> Self::VariantAccessor<A> {
            PtrVariant { descriptor: &descriptor.field, variant: &descriptor.variant, repr: a }
        }

        #[inline]
        unsafe fn clear<'a, 'b, T: Table>(a: StructBuilder<'b, 'a, T>, descriptor: &'static Self::Descriptor) {
            a.ptr_field_mut_unchecked(descriptor.slot as u16).clear();
        }
    };
}

// impls for list

impl<V: 'static> FieldType for List<V> {
    field_type_items!{}
}

impl<'b, 'p, T: Table, V> PtrFieldReader<'b, 'p, T, List<V>>
where
    V: ty::DynListValue,
{
    #[inline]
    fn default(&self) -> list::Reader<'static, V, T> {
        let default = &self.descriptor.default;
        assert!(default.element_size().upgradable_to(V::PTR_ELEMENT_SIZE));

        List::new(default.clone().imbue_from(self.repr))
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
            .raw_ptr()
            .to_list(Some(V::PTR_ELEMENT_SIZE))
        {
            Ok(Some(ptr)) => Ok(Some(List::new(ptr))),
            Ok(None) => Ok(None),
            Err(err) => Err(err),
        }
    }
}

impl<'b, 'p, T: Table, V> PtrFieldReader<'b, 'p, T, List<V>>
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

impl<'b, 'p, T: Table, V, Item> IntoIterator for PtrFieldReader<'b, 'p, T, List<V>>
where
    V: ty::DynListValue + for<'lb> list::ListAccessable<&'lb list::Reader<'p, V, T>>,
    InfalliblePtrs: for<'lb> list::IterStrategy<V, list::ElementReader<'p, 'lb, V, T>, Item = Item>,
{
    type IntoIter = list::Iter<'p, V, InfalliblePtrs, T>;
    type Item = Item;

    #[inline]
    fn into_iter(self) -> Self::IntoIter {
        self.get().into_iter()
    }
}

impl<'b, 'p, T: Table, V: ty::DynListValue> PtrVariantReader<'b, 'p, T, List<V>> {
    #[inline]
    fn default(&self) -> list::Reader<'static, V, T> {
        let default = &self.descriptor.default;
        assert!(default.element_size().upgradable_to(V::PTR_ELEMENT_SIZE));

        List::new(default.clone().imbue_from(self.repr))
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

impl<'b, 'p, T: Table, V: ty::ListValue> PtrFieldBuilder<'b, 'p, T, List<V>> {
    /// Returns a builder for the field. If it's not set or a error occurs while reading the
    /// existing value, the default is set instead.
    #[inline]
    pub fn get(self) -> list::Builder<'b, V, T> {
        let default = &self.descriptor.default;
        match self.into_raw_build_ptr().to_list_mut(Some(V::ELEMENT_SIZE)) {
            Ok(ptr) => List::new(ptr),
            Err((_, ptr)) => {
                assert_eq!(default.element_size(), V::ELEMENT_SIZE);
                let mut new_list = ptr.init_list(V::ELEMENT_SIZE, default.len());
                new_list.try_copy_from(default, UnwrapErrors).unwrap();
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
        self.raw_build_ptr().try_set_list(value.as_ref(), CopySize::Minimum(V::ELEMENT_SIZE), err_handler)
    }

    #[inline]
    pub fn set(&mut self, value: &list::Reader<V, impl InsertableInto<T>>) {
        self.try_set(value, IgnoreErrors).unwrap()
    }

    #[inline]
    pub fn init(self, count: u32) -> list::Builder<'b, V, T> {
        let count = ElementCount::new(count).expect("too many elements for list");
        List::new(self.into_raw_build_ptr().init_list(V::ELEMENT_SIZE, count))
    }

    #[inline]
    pub fn try_init(self, count: u32) -> Result<list::Builder<'b, V, T>, TooManyElementsError> {
        let max = V::ELEMENT_SIZE.max_elements().get();
        if count > max {
            return Err(TooManyElementsError(()))
        }

        Ok(self.init(count))
    }

    /// Initialize a new instance with the given element size.
    /// 
    /// The element size must be a valid upgrade from `V::ELEMENT_SIZE`. That is, calling
    /// `V::ELEMENT_SIZE.upgrade_to(size)` must yield `Some(size)`.
    #[inline]
    pub fn init_with_size(self, count: u32, size: ElementSize) -> list::Builder<'b, V, T> {
        assert_eq!(V::ELEMENT_SIZE.upgrade_to(size), Some(size));
        let count = ElementCount::new(count).expect("too many elements for list");
        List::new(self.into_raw_build_ptr().init_list(size, count))
    }
}

impl<'b, 'p, T: Table> PtrFieldBuilder<'b, 'p, T, List<AnyStruct>> {
    /// Returns a builder for the field. If it's not set or a error occurs while reading the
    /// existing value, the default is set instead.
    #[inline]
    pub fn get(self, expected_size: Option<StructSize>) -> list::Builder<'b, AnyStruct, T> {
        let default = &self.descriptor.default;
        let ptr = match self
            .into_raw_build_ptr()
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
            self.into_raw_build_ptr()
                .init_list(ElementSize::InlineComposite(size), count),
        )
    }
    
    // TODO the rest of the accessors
}

impl<'b, 'p, T: Table, V: ty::DynListValue> PtrVariantBuilder<'b, 'p, T, List<V>> {
    // TODO acceessors
}

impl<S: ty::Struct> FieldType for Struct<S> {
    field_type_items!{}
}

impl<S: ty::Struct, Repr> PtrField<Struct<S>, Repr> {
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

impl<'b, 'p, T: Table, S> PtrFieldReader<'b, 'p, T, Struct<S>>
where
    S: ty::Struct,
{
    #[inline]
    fn default_imbued_ptr(&self) -> ptr::StructReader<'static, T> {
        self.default_ptr().imbue_from(self.repr)
    }

    #[inline]
    pub fn get(&self) -> S::Reader<'b, T> {
        ty::StructReader::from_ptr(match self.raw_ptr().to_struct() {
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
        Ok(ty::StructReader::from_ptr(match self.raw_ptr().to_struct() {
            Ok(Some(ptr)) => ptr,
            Ok(None) => self.default_imbued_ptr(),
            Err(err) => return Err(err),
        }))
    }

    #[inline]
    pub fn try_get_option(&self) -> Result<Option<S::Reader<'b, T>>> {
        match self.raw_ptr().to_struct() {
            Ok(Some(ptr)) => Ok(Some(ty::StructReader::from_ptr(ptr))),
            Ok(None) => Ok(None),
            Err(err) => Err(err),
        }
    }
}

impl<'b, 'p: 'b, T: Table + 'p, S> PtrFieldBuilder<'b, 'p, T, Struct<S>>
where
    S: ty::Struct,
{
    /// Returns a builder for the field. If it's not set or a error occurs while reading the
    /// existing value, the field default is set in its place instead.
    #[inline]
    pub fn get(self) -> S::Builder<'b, T> {
        let default = &self.descriptor.default;
        let ptr = match self.into_raw_build_ptr().to_struct_mut(Some(S::SIZE)) {
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
        unsafe { ty::StructBuilder::from_ptr(self.into_raw_build_ptr().init_struct(S::SIZE)) }
    }

    /// Returns a builder for the field. If it's not set or a error occurs while reading the
    /// existing value, the struct default is set instead.
    #[inline]
    pub fn get_or_init(self) -> S::Builder<'b, T> {
        let ptr = self.into_raw_build_ptr().to_struct_mut_or_init(S::SIZE);
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
        self.into_raw_build_ptr().try_set_struct(value.as_ptr(), ptr::CopySize::Minimum(S::SIZE), err_handler)
    }

    #[inline]
    pub fn set<T2>(self, value: &S::Reader<'_, T2>)
    where
        T2: InsertableInto<T>,
    {
        self.try_set(value, IgnoreErrors).unwrap()
    }
}

impl<'b, 'p: 'b, T: Table + 'p, S> PtrVariantReader<'b, 'p, T, Struct<S>>
where
    S: ty::Struct,
{
    #[inline]
    fn default(&self) -> S::Reader<'b, T> {
        self.descriptor.default.clone().imbue_from(self.repr).into()
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

impl<'b, 'p: 'b, T: Table + 'p, S> PtrVariantBuilder<'b, 'p, T, Struct<S>>
where
    S: ty::Struct,
{
    /// Returns a builder for the field. If it's not set or a error occurs while reading the
    /// existing value, the field default is set in its place instead.
    #[inline]
    pub fn get(self) -> S::Builder<'b, T> {
        self.init_field().get()
    }

    #[inline]
    pub fn init(self) -> S::Builder<'b, T> {
        self.init_field().init()
    }

    /// Returns a builder for the field. If it's not set or a error occurs while reading the
    /// existing value, the struct default is set instead.
    #[inline]
    pub fn get_or_init(self) -> S::Builder<'b, T> {
        self.init_field().get_or_init()
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
        self.init_field().try_set(value, err_handler)
    }

    #[inline]
    pub fn set(self, value: &S::Reader<'_, impl InsertableInto<T>>) {
        self.init_field().set(value)
    }
}

impl FieldType for text::Text {
    field_type_items!{}
}

impl<Repr> PtrField<text::Text, Repr> {
    /// Returns the default value of the field
    #[inline]
    pub fn default(&self) -> text::Reader<'static> {
        self.descriptor.default
    }
}

impl<'b, 'p: 'b, T: Table + 'p> PtrFieldReader<'b, 'p, T, text::Text> {
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
        match self.raw_ptr().to_blob() {
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

impl<'b, 'p: 'b, T: Table + 'p> PtrFieldBuilder<'b, 'p, T, text::Text> {
    /// Returns a builder for the field. If it's not set or a error occurs while reading the
    /// existing value, the default is set instead.
    #[inline]
    pub fn get(self) -> text::Builder<'b> {
        let default = self.descriptor.default;
        let blob = match self.into_raw_build_ptr().to_blob_mut() {
            Ok(b) => b,
            Err((_, ptr)) => ptr.set_blob(default.into()),
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
        text::Builder::new_unchecked(self.into_raw_build_ptr().init_blob(len.into()))
    }

    #[inline]
    pub fn set(self, value: text::Reader) -> text::Builder<'b> {
        text::Builder::new_unchecked(self.into_raw_build_ptr().set_blob(value.into()))
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
}

impl<'b, 'p: 'b, T: Table + 'p> PtrVariantReader<'b, 'p, T, text::Text> {

}

impl<'b, 'p: 'b, T: Table + 'p> PtrVariantBuilder<'b, 'p, T, text::Text> {

}

impl FieldType for data::Data {
    field_type_items!{}
}

impl<Repr> PtrField<data::Data, Repr> {
    /// Returns the default value of the field
    #[inline]
    pub fn default(&self) -> data::Reader<'static> {
        self.descriptor.default.clone().into()
    }
}

impl<'b, 'p, T: Table + 'p> PtrFieldReader<'b, 'p, T, data::Data> {
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
        match self.raw_ptr().to_blob() {
            Ok(Some(ptr)) => Ok(Some(ptr.into())),
            Ok(None) => Ok(None),
            Err(err) => Err(err),
        }
    }
}

impl<'b, 'p: 'b, T: Table + 'p> PtrFieldBuilder<'b, 'p, T, data::Data> {
    /// Returns a builder for the field. If it's not set or a error occurs while reading the
    /// existing value, the default is set instead.
    #[inline]
    pub fn get(self) -> data::Builder<'b> {
        let default = self.descriptor.default;
        match self.into_raw_build_ptr().to_blob_mut() {
            Ok(data) => data,
            Err((_, ptr)) => ptr.set_blob(default.into()),
        }.into()
    }

    #[inline]
    pub fn init(self, count: ElementCount) -> data::Builder<'b> {
        self.into_raw_build_ptr().init_blob(count).into()
    }

    #[inline]
    pub fn set(self, value: data::Reader) -> data::Builder<'b> {
        self.into_raw_build_ptr().set_blob(value.into()).into()
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
}

impl<'b, 'p: 'b, T: Table + 'p> PtrVariantReader<'b, 'p, T, data::Data> {
    // TODO accessors
}

impl<'b, 'p: 'b, T: Table + 'p> PtrVariantBuilder<'b, 'p, T, data::Data> {
    // TODO accessors
}

impl FieldType for AnyPtr {
    field_type_items!();
}

impl<'b, 'p: 'b, T: Table + 'p> PtrVariantReader<'b, 'p, T, AnyPtr> {
    // TODO accessors
}

impl<'b, 'p: 'b, T: Table + 'p> PtrVariantBuilder<'b, 'p, T, AnyPtr> {
    // TODO accessors
}

impl FieldType for AnyStruct {
    field_type_items!{}
}

impl<Repr> PtrField<AnyStruct, Repr> {
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

impl<'b, 'p: 'b, T: Table + 'p> PtrFieldReader<'b, 'p, T, AnyStruct> {
    #[inline]
    fn default_imbued(&self) -> any::StructReader<'static, T> {
        self.default().imbue_from(self.repr)
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
        match self.raw_ptr().to_struct() {
            Ok(Some(ptr)) => Ok(Some(ptr.into())),
            Ok(None) => Ok(None),
            Err(err) => Err(err),
        }
    }
}

impl<'b, 'p: 'b, T: Table + 'p> PtrFieldBuilder<'b, 'p, T, AnyStruct> {
    // TODO accessors
}

impl<'b, 'p: 'b, T: Table + 'p> PtrVariantReader<'b, 'p, T, AnyStruct> {
    // TODO accessors
}

impl<'b, 'p: 'b, T: Table + 'p> PtrVariantBuilder<'b, 'p, T, AnyStruct> {
    // TODO accessors
}

impl FieldType for AnyList {
    field_type_items!{}
}

impl<Repr> PtrField<AnyList, Repr> {
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

impl<'b, 'p: 'b, T: Table + 'p> PtrFieldReader<'b, 'p, T, AnyList> {
    #[inline]
    fn default_imbued(&self) -> any::ListReader<'static, T> {
        self.default().imbue_from(self.repr)
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
        match self.raw_ptr().to_list(None) {
            Ok(Some(ptr)) => Ok(Some(ptr.into())),
            Ok(None) => Ok(None),
            Err(err) => Err(err),
        }
    }
}

impl<'b, 'p: 'b, T: Table + 'p> PtrFieldBuilder<'b, 'p, T, AnyList> {
    // TODO accessors
}

impl<'b, 'p: 'b, T: Table + 'p> PtrVariantReader<'b, 'p, T, AnyList> {
    // TODO accessors
}

impl<'b, 'p: 'b, T: Table + 'p> PtrVariantBuilder<'b, 'p, T, AnyList> {
    // TODO accessors
}

impl<C: ty::Capability> FieldType for Capability<C> {
    field_type_items!{}
}

impl<'b, 'p, T, C> PtrFieldReader<'b, 'p, T, Capability<C>>
where
    T: Table,
    C: ty::Capability,
{
    // TODO accessors
}

impl<'b, 'p, T, C> PtrFieldBuilder<'b, 'p, T, Capability<C>>
where
    T: Table,
    C: ty::Capability,
{
    // TODO accessors
}

impl<'b, 'p, T: Table, C: ty::Capability> PtrVariantReader<'b, 'p, T, Capability<C>> {
    // TODO accessors
}

impl<'b, 'p, T: Table, C: ty::Capability> PtrVariantBuilder<'b, 'p, T, Capability<C>> {
    // TODO accessors
}
