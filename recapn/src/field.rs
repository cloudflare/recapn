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

use core::convert::{Infallible as Never, TryFrom};
use core::marker::PhantomData;
use core::str::Utf8Error;

use crate::any::{AnyList, AnyPtr, AnyStruct};
use crate::internal::Sealed;
use crate::list::{self, ElementSize, InfalliblePtrs, List, TooManyElementsError};
use crate::orphan::{Orphan, Orphanage};
use crate::ptr::{
    self, CopySize, Data, ElementCount, ErrorHandler, IgnoreErrors, StructSize, UnwrapErrors,
};
use crate::rpc::{BreakableCapSystem, CapTable, Capable, Empty, InsertableInto, Table};
use crate::ty::{self, EnumResult, ListValue, StructReader as _};
use crate::{any, data, text, Error, Family, NotInSchema, Result};

mod internal {
    use super::*;

    pub trait Accessable {
        type Table: Table;

        unsafe fn is_variant_set(&self, variant: &VariantInfo) -> bool;

        type Group<G: FieldGroup>;
        unsafe fn group<G: FieldGroup>(self) -> Self::Group<G>;

        type Data<D: Data>;
        unsafe fn data<D: Data>(self, info: &'static FieldInfo<D>) -> Self::Data<D>;

        type Enum<E: ty::Enum>;
        unsafe fn enum_value<E: ty::Enum>(self, info: &'static FieldInfo<Enum<E>>)
            -> Self::Enum<E>;

        unsafe fn ptr_reader(&self, slot: u16) -> ptr::PtrReader<'_, Self::Table>;

        #[inline]
        unsafe fn is_ptr_null(&self, slot: u16) -> bool {
            self.ptr_reader(slot).is_null()
        }
    }

    pub trait BuildAccessable: Accessable {
        unsafe fn set_variant(&mut self, variant: &VariantInfo);

        type ByRef<'b>
        where
            Self: 'b;
        fn by_ref<'b>(&'b mut self) -> Self::ByRef<'b>;
    }

    pub trait BuildAccessablePtr<'a>: BuildAccessable {
        unsafe fn into_ptr_field(self, slot: u16) -> ptr::PtrBuilder<'a, Self::Table>;
        unsafe fn ptr_field(&mut self, slot: u16) -> ptr::PtrBuilder<'_, Self::Table>;
    }

    impl<'b, 'p, T: Table> Accessable for StructReader<'b, 'p, T> {
        type Table = T;

        #[inline]
        unsafe fn is_variant_set(&self, variant: &VariantInfo) -> bool {
            self.data_field::<u16>(variant.slot as usize) == variant.case
        }

        type Group<G: FieldGroup> = G::Reader<'p, T>;
        #[inline]
        unsafe fn group<G: FieldGroup>(self) -> Self::Group<G> {
            ty::StructReader::from_ptr(self.clone())
        }
        type Data<D: Data> = D;
        #[inline]
        unsafe fn data<D: Data>(self, info: &'static FieldInfo<D>) -> Self::Data<D> {
            self.data_field_with_default(info.slot as usize, info.default)
        }
        type Enum<E: ty::Enum> = EnumResult<E>;
        #[inline]
        unsafe fn enum_value<E: ty::Enum>(
            self,
            info: &'static FieldInfo<Enum<E>>,
        ) -> Self::Enum<E> {
            let &FieldInfo { slot, default } = info;
            let default: u16 = default.into();
            let value = self.data_field_with_default::<u16>(slot as usize, default);
            E::try_from(value)
        }

        #[inline]
        unsafe fn ptr_reader(&self, slot: u16) -> ptr::PtrReader<'_, Self::Table> {
            self.ptr_field(slot)
        }
    }

    impl<'b, 'p, T: Table> Accessable for StructBuilder<'b, 'p, T> {
        type Table = T;

        #[inline]
        unsafe fn is_variant_set(&self, variant: &VariantInfo) -> bool {
            self.data_field_unchecked::<u16>(variant.slot as usize) == variant.case
        }
        type Group<G: FieldGroup> = G::Builder<'b, T>;
        #[inline]
        unsafe fn group<G: FieldGroup>(self) -> Self::Group<G> {
            ty::StructBuilder::from_ptr(self.by_ref())
        }
        type Data<D: Data> = DataField<D, Self>;
        #[inline]
        unsafe fn data<D: Data>(self, info: &'static FieldInfo<D>) -> Self::Data<D> {
            DataField {
                descriptor: info,
                repr: self,
            }
        }
        type Enum<E: ty::Enum> = DataField<Enum<E>, Self>;
        #[inline]
        unsafe fn enum_value<E: ty::Enum>(
            self,
            info: &'static FieldInfo<Enum<E>>,
        ) -> Self::Enum<E> {
            DataField {
                descriptor: info,
                repr: self,
            }
        }
        #[inline]
        unsafe fn ptr_reader(&self, slot: u16) -> ptr::PtrReader<'_, Self::Table> {
            self.ptr_field_unchecked(slot)
        }
    }
    impl<'b, 'p, T: Table> BuildAccessable for StructBuilder<'b, 'p, T> {
        #[inline]
        unsafe fn set_variant(&mut self, variant: &VariantInfo) {
            self.set_field_unchecked(variant.slot as usize, variant.case)
        }
        type ByRef<'c> = StructBuilder<'c, 'p, T> where Self: 'c;
        #[inline]
        fn by_ref<'c>(&'c mut self) -> Self::ByRef<'c> {
            &mut *self
        }
    }
    impl<'b, 'p, T: Table> BuildAccessablePtr<'b> for StructBuilder<'b, 'p, T> {
        #[inline]
        unsafe fn into_ptr_field(self, slot: u16) -> ptr::PtrBuilder<'b, Self::Table> {
            self.ptr_field_mut_unchecked(slot)
        }
        #[inline]
        unsafe fn ptr_field(&mut self, slot: u16) -> ptr::PtrBuilder<'_, Self::Table> {
            self.ptr_field_mut_unchecked(slot)
        }
    }

    impl<'p, T: Table> Accessable for OwnedStructBuilder<'p, T> {
        type Table = T;

        #[inline]
        unsafe fn is_variant_set(&self, variant: &VariantInfo) -> bool {
            self.data_field_unchecked::<u16>(variant.slot as usize) == variant.case
        }
        type Group<G: FieldGroup> = G::Builder<'p, T>;
        #[inline]
        unsafe fn group<G: FieldGroup>(self) -> Self::Group<G> {
            ty::StructBuilder::from_ptr(self)
        }
        type Data<D: Data> = DataField<D, Self>;
        #[inline]
        unsafe fn data<D: Data>(self, info: &'static FieldInfo<D>) -> Self::Data<D> {
            DataField {
                descriptor: info,
                repr: self,
            }
        }
        type Enum<E: ty::Enum> = DataField<Enum<E>, Self>;
        #[inline]
        unsafe fn enum_value<E: ty::Enum>(
            self,
            info: &'static FieldInfo<Enum<E>>,
        ) -> Self::Enum<E> {
            DataField {
                descriptor: info,
                repr: self,
            }
        }
        #[inline]
        unsafe fn ptr_reader(&self, slot: u16) -> ptr::PtrReader<'_, Self::Table> {
            self.ptr_field_unchecked(slot)
        }
    }
    impl<'p, T: Table> BuildAccessable for OwnedStructBuilder<'p, T> {
        #[inline]
        unsafe fn set_variant(&mut self, variant: &VariantInfo) {
            self.set_field_unchecked(variant.slot as usize, variant.case)
        }
        type ByRef<'b> = OwnedStructBuilder<'b, T> where Self: 'b;
        #[inline]
        fn by_ref<'b>(&'b mut self) -> Self::ByRef<'b> {
            self.by_ref()
        }
    }
    impl<'p, T: Table> BuildAccessablePtr<'p> for OwnedStructBuilder<'p, T> {
        #[inline]
        unsafe fn into_ptr_field(self, slot: u16) -> ptr::PtrBuilder<'p, Self::Table> {
            self.into_ptr_field_unchecked(slot)
        }
        #[inline]
        unsafe fn ptr_field(&mut self, slot: u16) -> ptr::PtrBuilder<'_, Self::Table> {
            self.ptr_field_mut_unchecked(slot)
        }
    }
}

use internal::{Accessable, BuildAccessable, BuildAccessablePtr};

pub type StructReader<'b, 'p, T> = &'b ptr::StructReader<'p, T>;
pub type StructBuilder<'b, 'p, T> = &'b mut ptr::StructBuilder<'p, T>;
use ptr::StructBuilder as OwnedStructBuilder;

/// Describes a group of fields. This is primarily used for clearing unions and groups of fields within a struct.
pub trait FieldGroup: ty::StructView {
    unsafe fn clear<T: Table>(a: StructBuilder<'_, '_, T>);
}

/// Describes a type that can be a field in a Cap'n Proto struct type.
pub trait FieldType: Sized + 'static {
    /// A type used to describe the field type. This will likely be a type of FieldInfo, except
    /// for Void and groups, which have no descriptor.
    type Descriptor;

    /// Selects the type of accessor used by this type.
    type Accessor<A: Accessable>;
    unsafe fn accessor<A: Accessable>(
        a: A,
        descriptor: &'static Self::Descriptor,
    ) -> Self::Accessor<A>;

    type VariantAccessor<A: Accessable>;
    unsafe fn variant<A: Accessable>(
        a: A,
        descriptor: &'static VariantDescriptor<Self>,
    ) -> Self::VariantAccessor<A>;

    unsafe fn clear<T: Table>(a: StructBuilder<'_, '_, T>, descriptor: &'static Self::Descriptor);
}

impl FieldType for () {
    type Descriptor = ();
    type Accessor<A: Accessable> = ();
    #[inline]
    unsafe fn accessor<A: Accessable>(_: A, _: &'static Self::Descriptor) -> Self::Accessor<A> {}
    type VariantAccessor<A: Accessable> = VoidVariant<Self, A>;
    #[inline]
    unsafe fn variant<A: Accessable>(
        a: A,
        descriptor: &'static VariantDescriptor<Self>,
    ) -> Self::VariantAccessor<A> {
        VoidVariant {
            t: PhantomData,
            variant: &descriptor.variant,
            repr: a,
        }
    }

    #[inline]
    unsafe fn clear<T: Table>(_: StructBuilder<'_, '_, T>, _: &'static Self::Descriptor) {}
}

impl<D: Data> FieldType for D {
    type Descriptor = FieldInfo<D>;
    type Accessor<A: Accessable> = A::Data<D>;
    #[inline]
    unsafe fn accessor<A: Accessable>(
        a: A,
        descriptor: &'static Self::Descriptor,
    ) -> Self::Accessor<A> {
        a.data(descriptor)
    }
    type VariantAccessor<A: Accessable> = DataVariant<D, A>;
    #[inline]
    unsafe fn variant<A: Accessable>(
        a: A,
        descriptor: &'static VariantDescriptor<Self>,
    ) -> Self::VariantAccessor<A> {
        DataVariant {
            repr: a,
            descriptor: &descriptor.field,
            variant: &descriptor.variant,
        }
    }
    #[inline]
    unsafe fn clear<T: Table>(a: StructBuilder<'_, '_, T>, descriptor: &'static Self::Descriptor) {
        a.set_field_with_default_unchecked(
            descriptor.slot as usize,
            descriptor.default,
            descriptor.default,
        )
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

/// A trait used to specify that a type is used to represent a Cap'n Proto value.
pub trait Value: 'static {
    type Default;
}

/// A trait used to specify that a type is backed by a pointer field. This enables generic pointers
/// in generated code.
pub trait PtrValue: 'static {
    type Default;
}

impl<T: PtrValue> Value for T {
    type Default = Option<<T as PtrValue>::Default>;
}

/// Describes a value in a "slot" in a struct. This does not include groups or void, which don't
/// have an associated default value or slot.
pub struct FieldInfo<V: Value> {
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
impl<S: ty::Struct> PtrValue for Struct<S> {
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
    unsafe fn variant<A: Accessable>(
        a: A,
        descriptor: &'static VariantDescriptor<Self>,
    ) -> Self::VariantAccessor<A> {
        VoidVariant {
            t: PhantomData,
            variant: &descriptor.variant,
            repr: a,
        }
    }
    #[inline]
    unsafe fn clear<T: Table>(a: StructBuilder<'_, '_, T>, _: &'static Self::Descriptor) {
        G::clear(a)
    }
}

pub struct Capability<C: ty::Capability> {
    c: PhantomData<fn() -> C>,
}

impl<C: ty::Capability> Sealed for Capability<C> {}
impl<C: ty::Capability> PtrValue for Capability<C> {
    /// Capabilities don't have defaults
    type Default = Never;
}
impl<C: ty::Capability> ListValue for Capability<C> {
    const ELEMENT_SIZE: ElementSize = ElementSize::Pointer;
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

/// A variant accessor type for "void" types (`Void` and groups).
///
/// This includes not just the Void primitive, but also groups, which are kinda like voids in
/// the sense that they aren't actual "things" on the wire. Void fields take up no encoding space
/// and groups don't take up any encoding space either. They also have no associated data, so they
/// don't have an associated descriptor, just variant info.
#[must_use = "accessors don't do anything unless you call one of their functions"]
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

    #[inline]
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

#[must_use = "accessors don't do anything unless you call one of their functions"]
pub struct DataField<D: Value, Repr> {
    descriptor: &'static FieldInfo<D>,
    repr: Repr,
}

impl<'b, 'p, T: Table, D: Data> DataField<D, StructBuilder<'b, 'p, T>> {
    #[inline]
    pub fn get(&self) -> D {
        unsafe {
            self.repr.data_field_with_default_unchecked(
                self.descriptor.slot as usize,
                self.descriptor.default,
            )
        }
    }

    #[inline]
    pub fn set(&mut self, value: D) {
        unsafe {
            self.repr.set_field_with_default_unchecked(
                self.descriptor.slot as usize,
                value,
                self.descriptor.default,
            )
        }
    }
}

#[must_use = "accessors don't do anything unless you call one of their functions"]
pub struct DataVariant<D: Value, Repr> {
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
            Some(unsafe {
                self.repr
                    .data_field_with_default_unchecked(slot as usize, default)
            })
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
        let &VariantInfo {
            slot: variant_slot,
            case,
        } = self.variant;
        let &FieldInfo { slot, default } = self.descriptor;
        unsafe {
            self.repr.set_field_unchecked(variant_slot as usize, case);
            self.repr
                .set_field_with_default_unchecked(slot as usize, value, default);
        }
    }
}

impl<E: ty::Enum> FieldType for Enum<E> {
    type Descriptor = FieldInfo<Self>;

    type Accessor<A: Accessable> = A::Enum<E>;
    unsafe fn accessor<A: Accessable>(
        a: A,
        descriptor: &'static Self::Descriptor,
    ) -> Self::Accessor<A> {
        a.enum_value(descriptor)
    }

    type VariantAccessor<A: Accessable> = DataVariant<Self, A>;
    unsafe fn variant<A: Accessable>(
        a: A,
        descriptor: &'static VariantDescriptor<Self>,
    ) -> Self::VariantAccessor<A> {
        DataVariant {
            descriptor: &descriptor.field,
            variant: &descriptor.variant,
            repr: a,
        }
    }

    #[inline]
    unsafe fn clear<T: Table>(a: StructBuilder<'_, '_, T>, descriptor: &'static Self::Descriptor) {
        a.set_field_unchecked::<u16>(descriptor.slot as usize, 0);
    }
}

impl<'b, 'p, T: Table, E: ty::Enum> DataField<Enum<E>, StructBuilder<'b, 'p, T>> {
    #[inline]
    pub fn get(&self) -> EnumResult<E> {
        let &FieldInfo { slot, default } = self.descriptor;
        let default: u16 = default.into();
        let value = unsafe {
            self.repr
                .data_field_with_default_unchecked::<u16>(slot as usize, default)
        };
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
        unsafe {
            self.repr
                .set_field_with_default_unchecked(slot as usize, value, default)
        }
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
            let value = self
                .repr
                .data_field_with_default::<u16>(slot as usize, default);
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
            let value = unsafe {
                self.repr
                    .data_field_with_default_unchecked(slot as usize, default)
            };
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
        let &VariantInfo {
            slot: variant_slot,
            case,
        } = self.variant;
        let &FieldInfo { slot, default } = self.descriptor;
        let value: u16 = value.into();
        let default: u16 = default.into();
        unsafe {
            self.repr.set_field_unchecked(variant_slot as usize, case);
            self.repr
                .set_field_with_default_unchecked(slot as usize, value, default);
        }
    }
}

#[must_use = "accessors don't do anything unless you call one of their functions"]
pub struct PtrField<V: PtrValue, Repr> {
    descriptor: &'static FieldInfo<V>,
    repr: Repr,
}

impl<V: PtrValue, Repr: Accessable> PtrField<V, Repr> {
    #[inline]
    pub fn is_null(&self) -> bool {
        unsafe { self.repr.is_ptr_null(self.descriptor.slot as u16) }
    }
}

impl<V: PtrValue, Repr: BuildAccessable> PtrField<V, Repr> {
    /// Create a new field builder "by reference". This allows a field builder to be reused
    /// as many builder functions consume the builder.
    #[inline]
    pub fn by_ref<'c>(&'c mut self) -> PtrField<V, Repr::ByRef<'c>> {
        PtrField {
            descriptor: self.descriptor,
            repr: self.repr.by_ref(),
        }
    }
}

pub type PtrFieldReader<'b, 'p, T, V> = PtrField<V, StructReader<'b, 'p, T>>;
pub type PtrFieldBuilder<'b, 'p, T, V> = PtrField<V, StructBuilder<'b, 'p, T>>;
pub type PtrFieldOwner<'p, T, V> = PtrField<V, OwnedStructBuilder<'p, T>>;

impl<'b, 'p, T: Table, V: PtrValue> PtrFieldReader<'b, 'p, T, V> {
    fn raw_ptr(&self) -> ptr::PtrReader<'p, T> {
        self.repr.ptr_field(self.descriptor.slot as u16)
    }

    #[inline]
    pub fn ptr(&self) -> any::PtrReader<'p, T> {
        self.raw_ptr().into()
    }
}

impl<'b, B: BuildAccessablePtr<'b>, V: PtrValue> PtrField<V, B> {
    #[inline]
    fn raw_build_ptr(&mut self) -> ptr::PtrBuilder<'_, B::Table> {
        unsafe { self.repr.ptr_field(self.descriptor.slot as u16) }
    }

    #[inline]
    fn into_raw_build_ptr(self) -> ptr::PtrBuilder<'b, B::Table> {
        unsafe { self.repr.into_ptr_field(self.descriptor.slot as u16) }
    }

    /// Build the field as any pointer type.
    #[inline]
    pub fn ptr(self) -> any::PtrBuilder<'b, B::Table> {
        self.into_raw_build_ptr().into()
    }

    #[inline]
    pub fn adopt(&mut self, orphan: Orphan<'_, V, B::Table>) {
        self.raw_build_ptr().adopt(orphan.into_inner());
    }

    #[inline]
    pub fn disown_into<'c>(
        &mut self,
        orphanage: &Orphanage<'c, B::Table>,
    ) -> Orphan<'c, V, B::Table> {
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
#[derive(Clone)]
pub struct PtrVariant<V: PtrValue, Repr> {
    descriptor: &'static FieldInfo<V>,
    variant: &'static VariantInfo,
    repr: Repr,
}

impl<V: PtrValue, Repr: Accessable> PtrVariant<V, Repr> {
    /// Returns a bool indicating whether or not this field is set in the union
    #[inline]
    pub fn is_set(&self) -> bool {
        unsafe { self.repr.is_variant_set(&self.variant) }
    }

    #[inline]
    pub fn is_null(&self) -> bool {
        if self.is_set() {
            unsafe { self.repr.is_ptr_null(self.descriptor.slot as u16) }
        } else {
            true
        }
    }
}

impl<V: PtrValue, Repr: BuildAccessable> PtrVariant<V, Repr> {
    /// Create a new field builder "by reference". This allows a field builder to be reused
    /// as many builder functions consume the builder.
    #[inline]
    pub fn by_ref<'c>(&'c mut self) -> PtrVariant<V, Repr::ByRef<'c>> {
        PtrVariant {
            variant: self.variant,
            descriptor: self.descriptor,
            repr: self.repr.by_ref(),
        }
    }
}

pub type PtrVariantReader<'b, 'p, T, V> = PtrVariant<V, StructReader<'b, 'p, T>>;
pub type PtrVariantBuilder<'b, 'p, T, V> = PtrVariant<V, StructBuilder<'b, 'p, T>>;
pub type PtrVariantOwner<'p, T, V> = PtrVariant<V, OwnedStructBuilder<'p, T>>;

impl<'b, 'p, T: Table, V: PtrValue> PtrVariantReader<'b, 'p, T, V> {
    #[inline]
    fn raw_ptr_or_null(&self) -> ptr::PtrReader<'p, T> {
        if self.is_set() {
            self.repr.ptr_field(self.descriptor.slot as u16)
        } else {
            ptr::PtrReader::null().imbue_from(self.repr)
        }
    }

    #[inline]
    pub fn field(&self) -> Option<PtrFieldReader<'b, 'p, T, V>> {
        self.is_set().then_some(PtrField {
            descriptor: self.descriptor,
            repr: self.repr,
        })
    }
}

impl<'b, B: BuildAccessablePtr<'b>, V: PtrValue> PtrVariant<V, B> {
    #[inline]
    fn raw_build_ptr(&mut self) -> Option<ptr::PtrBuilder<'_, B::Table>> {
        self.is_set().then(|| self.raw_build_field_ptr())
    }

    /// Get a pointer build for this field without checking to make sure it's set.
    #[inline]
    fn raw_build_field_ptr(&mut self) -> ptr::PtrBuilder<'_, B::Table> {
        unsafe { self.repr.ptr_field(self.descriptor.slot as u16) }
    }

    #[inline]
    fn set_case(&mut self) -> bool {
        let is_set = self.is_set();
        if !is_set {
            unsafe {
                self.repr.set_variant(self.variant);
            }
        }
        is_set
    }

    #[inline]
    fn set_case_and_build_ptr(&mut self) -> ptr::PtrBuilder<'_, B::Table> {
        let was_set = self.set_case();
        let mut ptr = self.raw_build_field_ptr();
        if !was_set {
            ptr.clear();
        }
        ptr
    }

    #[inline]
    pub fn init_case(mut self) -> PtrField<V, B> {
        let was_set = self.set_case();
        let mut field = PtrField {
            descriptor: self.descriptor,
            repr: self.repr,
        };
        if !was_set {
            field.clear();
        }
        field
    }

    #[inline]
    pub fn field(self) -> Option<PtrField<V, B>> {
        self.is_set().then_some(PtrField {
            descriptor: self.descriptor,
            repr: self.repr,
        })
    }

    #[inline]
    pub fn adopt(&mut self, orphan: Orphan<'_, V, B::Table>) {
        self.set_case();
        unsafe {
            self.repr
                .ptr_field(self.descriptor.slot as u16)
                .adopt(orphan.into_inner())
        }
    }

    #[inline]
    pub fn disown_into<'c>(
        &mut self,
        orphanage: &Orphanage<'c, B::Table>,
    ) -> Option<Orphan<'c, V, B::Table>> {
        self.raw_build_ptr()
            .map(|mut p| Orphan::new(p.disown_into(orphanage)))
    }

    #[inline]
    pub fn try_clear<E: ErrorHandler>(&mut self, err_handler: E) -> Result<(), E::Error> {
        match self.raw_build_ptr() {
            Some(mut ptr) => ptr.try_clear(err_handler),
            None => Ok(()),
        }
    }

    #[inline]
    pub fn clear(&mut self) {
        if let Some(mut ptr) = self.raw_build_ptr() {
            ptr.clear();
        }
    }
}

/// A macro to make implementing FieldType and AccessableField for non-generic ptr types easy
macro_rules! field_type_items {
    () => {
        type Descriptor = FieldInfo<Self>;

        type Accessor<A: Accessable> = PtrField<Self, A>;
        #[inline]
        unsafe fn accessor<A: Accessable>(
            a: A,
            descriptor: &'static Self::Descriptor,
        ) -> Self::Accessor<A> {
            PtrField {
                descriptor,
                repr: a,
            }
        }

        type VariantAccessor<A: Accessable> = PtrVariant<Self, A>;
        #[inline]
        unsafe fn variant<A: Accessable>(
            a: A,
            descriptor: &'static VariantDescriptor<Self>,
        ) -> Self::VariantAccessor<A> {
            PtrVariant {
                descriptor: &descriptor.field,
                variant: &descriptor.variant,
                repr: a,
            }
        }

        #[inline]
        unsafe fn clear<T: Table>(
            a: StructBuilder<'_, '_, T>,
            descriptor: &'static Self::Descriptor,
        ) {
            a.ptr_field_mut_unchecked(descriptor.slot as u16).clear();
        }
    };
}

// impls for list

impl<V: 'static> PtrValue for List<V> {
    type Default = ptr::ListReader<'static>;
}

impl<V: 'static> FieldType for List<V> {
    field_type_items! {}
}

impl<'p, T: Table, V: ty::DynListValue> PtrFieldReader<'_, 'p, T, List<V>> {
    #[inline]
    fn default(&self) -> list::Reader<'static, V, T> {
        match &self.descriptor.default {
            Some(default) => {
                debug_assert!(default.element_size().upgradable_to(V::PTR_ELEMENT_SIZE));

                List::new(default.clone().imbue_from(self.repr))
            }
            None => List::empty().imbue_from(self.repr),
        }
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
        match self.raw_ptr().to_list(Some(V::PTR_ELEMENT_SIZE)) {
            Ok(Some(ptr)) => Ok(Some(List::new(ptr))),
            Ok(None) => Ok(None),
            Err(err) => Err(err),
        }
    }
}

impl<'p, T: Table, V: ty::DynListValue> PtrFieldReader<'_, 'p, T, List<V>>
where
    V: for<'lb> list::ListAccessable<&'lb list::Reader<'p, V, T>>,
{
    #[inline]
    pub fn iter_by<S>(&self, strat: S) -> list::Iter<'p, V, S, T>
    where
        S: for<'lb> list::IterStrategy<V, list::ElementReader<'p, 'lb, V, T>>,
    {
        self.get().into_iter_by(strat)
    }
}

impl<'p, T: Table, V: ty::DynListValue, Item> IntoIterator for PtrFieldReader<'_, 'p, T, List<V>>
where
    V: for<'lb> list::ListAccessable<&'lb list::Reader<'p, V, T>>,
    InfalliblePtrs: for<'lb> list::IterStrategy<V, list::ElementReader<'p, 'lb, V, T>, Item = Item>,
{
    type IntoIter = list::Iter<'p, V, InfalliblePtrs, T>;
    type Item = Item;

    #[inline]
    fn into_iter(self) -> Self::IntoIter {
        self.get().into_iter()
    }
}

impl<'p, T: Table, V: ty::DynListValue> PtrVariantReader<'_, 'p, T, List<V>> {
    #[inline]
    fn default(&self) -> list::Reader<'static, V, T> {
        match &self.descriptor.default {
            Some(default) => {
                debug_assert!(default.element_size().upgradable_to(V::PTR_ELEMENT_SIZE));

                List::new(default.clone().imbue_from(self.repr))
            }
            None => List::empty().imbue_from(self.repr),
        }
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

impl<'b, T: Table, B: BuildAccessablePtr<'b, Table = T>, V: ty::ListValue> PtrField<List<V>, B> {
    /// Returns a builder for the field. If it's not set or a error occurs while reading the
    /// existing value, the default is set instead.
    #[inline]
    pub fn get(self) -> list::Builder<'b, V, T> {
        let default = &self.descriptor.default;
        match self.into_raw_build_ptr().to_list_mut(Some(V::ELEMENT_SIZE)) {
            Ok(ptr) => List::new(ptr),
            Err((_, ptr)) => {
                let new_list = if let Some(default) = default {
                    debug_assert_eq!(default.element_size(), V::ELEMENT_SIZE);
                    let mut new_list = ptr.init_list(V::ELEMENT_SIZE, default.len());
                    new_list.try_copy_from(default, UnwrapErrors).unwrap();
                    new_list
                } else {
                    ptr.init_list(V::ELEMENT_SIZE, ElementCount::ZERO)
                };
                List::new(new_list)
            }
        }
    }

    #[inline]
    pub fn try_set<E: ErrorHandler>(
        &mut self,
        value: &list::Reader<'_, V, impl InsertableInto<T>>,
        err_handler: E,
    ) -> Result<(), E::Error> {
        self.raw_build_ptr().try_set_list(
            value.as_ref(),
            CopySize::Minimum(V::ELEMENT_SIZE),
            err_handler,
        )
    }

    #[inline]
    pub fn set(&mut self, value: &list::Reader<'_, V, impl InsertableInto<T>>) {
        self.try_set(value, IgnoreErrors).unwrap()
    }

    #[inline]
    pub fn init(self, count: u32) -> list::Builder<'b, V, T> {
        self.try_init(count).expect("too many elements for list")
    }

    #[inline]
    pub fn try_init(self, count: u32) -> Result<list::Builder<'b, V, T>, TooManyElementsError> {
        let size = V::ELEMENT_SIZE;
        if count > size.max_elements().get() {
            return Err(TooManyElementsError(()));
        }
        let count = ElementCount::new(count).unwrap();

        Ok(List::new(self.into_raw_build_ptr().init_list(size, count)))
    }

    /// Initialize a new instance with the given element size.
    ///
    /// The element size must be a valid upgrade from `V::ELEMENT_SIZE`. That is, calling
    /// `V::ELEMENT_SIZE.upgrade_to(size)` must yield `Some(size)`.
    #[inline]
    pub fn init_with_size(self, count: u32, size: ElementSize) -> list::Builder<'b, V, T> {
        assert_eq!(V::ELEMENT_SIZE.upgrade_to(size), Some(size));
        assert!(
            count < size.max_elements().get(),
            "too many elements for list"
        );
        let count = ElementCount::new(count).unwrap();
        List::new(self.into_raw_build_ptr().init_list(size, count))
    }
}

impl<'b, T: Table, B: BuildAccessablePtr<'b, Table = T>> PtrField<List<AnyStruct>, B> {
    /// Returns a builder for the field. If it's not set or a error occurs while reading the
    /// existing value, the default is set instead.
    #[inline]
    pub fn get(self, size: StructSize) -> list::Builder<'b, AnyStruct, T> {
        let size = ElementSize::InlineComposite(size);
        let default = &self.descriptor.default;
        let ptr = match self.into_raw_build_ptr().to_list_mut(Some(size)) {
            Ok(ptr) => ptr,
            Err((_, ptr)) => {
                if let Some(default) = default {
                    let default_size = default.element_size();
                    let size = default_size
                        .upgrade_to(size)
                        .expect("default value should be upgradable to struct list");
                    let mut builder = ptr.init_list(size, default.len());
                    builder.try_copy_from(default, UnwrapErrors).unwrap();
                    builder
                } else {
                    ptr.init_list(size, ElementCount::ZERO)
                }
            }
        };
        List::new(ptr)
    }

    #[inline]
    pub fn try_set<E: ErrorHandler>(
        &mut self,
        value: &list::Reader<'_, AnyStruct, impl InsertableInto<T>>,
        err_handler: E,
    ) -> Result<(), E::Error> {
        self.raw_build_ptr()
            .try_set_list(value.as_ref(), CopySize::FromValue, err_handler)
    }

    #[inline]
    pub fn set(&mut self, value: &list::Reader<'_, AnyStruct, impl InsertableInto<T>>) {
        self.try_set(value, IgnoreErrors).unwrap()
    }

    #[inline]
    pub fn init(self, size: StructSize, count: u32) -> list::Builder<'b, AnyStruct, T> {
        self.try_init(size, count)
            .expect("too many elements for list")
    }

    #[inline]
    pub fn try_init(
        self,
        size: StructSize,
        count: u32,
    ) -> Result<list::Builder<'b, AnyStruct, T>, TooManyElementsError> {
        let size = ElementSize::InlineComposite(size);
        if count > size.max_elements().get() {
            return Err(TooManyElementsError(()));
        }
        let count = ElementCount::new(count).unwrap();

        Ok(List::new(self.into_raw_build_ptr().init_list(size, count)))
    }
}

impl<'b, T, B, V> PtrVariant<List<V>, B>
where
    T: Table,
    B: BuildAccessablePtr<'b, Table = T>,
    V: ty::ListValue,
{
    #[inline]
    pub fn try_set<E: ErrorHandler>(
        &mut self,
        value: &list::Reader<'_, V, impl InsertableInto<T>>,
        err_handler: E,
    ) -> Result<(), E::Error> {
        self.set_case_and_build_ptr()
            .try_set_list(value.as_ref(), CopySize::FromValue, err_handler)
    }

    #[inline]
    pub fn set(&mut self, value: &list::Reader<'_, V, impl InsertableInto<T>>) {
        self.try_set(value, IgnoreErrors).unwrap()
    }

    #[inline]
    pub fn init(self, count: u32) -> list::Builder<'b, V, T> {
        self.try_init(count).expect("too many elements for list")
    }

    #[inline]
    pub fn try_init(self, count: u32) -> Result<list::Builder<'b, V, T>, TooManyElementsError> {
        let size = V::ELEMENT_SIZE;
        if count > size.max_elements().get() {
            return Err(TooManyElementsError(()));
        }

        Ok(self.init_case().init(count))
    }
}

impl<'b, T, B> PtrVariant<List<AnyStruct>, B>
where
    T: Table,
    B: BuildAccessablePtr<'b, Table = T>,
{
    #[inline]
    pub fn try_set<E: ErrorHandler>(
        &mut self,
        value: &list::Reader<'_, AnyStruct, impl InsertableInto<T>>,
        err_handler: E,
    ) -> Result<(), E::Error> {
        self.set_case_and_build_ptr()
            .try_set_list(value.as_ref(), CopySize::FromValue, err_handler)
    }

    #[inline]
    pub fn set(&mut self, value: &list::Reader<'_, AnyStruct, impl InsertableInto<T>>) {
        self.try_set(value, IgnoreErrors).unwrap()
    }

    #[inline]
    pub fn init(self, size: StructSize, count: u32) -> list::Builder<'b, AnyStruct, T> {
        self.try_init(size, count)
            .expect("too many elements for list")
    }

    #[inline]
    pub fn try_init(
        self,
        size: StructSize,
        count: u32,
    ) -> Result<list::Builder<'b, AnyStruct, T>, TooManyElementsError> {
        if count > ElementSize::InlineComposite(size).max_elements().get() {
            return Err(TooManyElementsError(()));
        }

        Ok(self.init_case().init(size, count))
    }
}

impl<S: ty::Struct> FieldType for Struct<S> {
    field_type_items! {}
}

impl<S: ty::Struct, Repr> PtrField<Struct<S>, Repr> {
    #[inline]
    fn default_ptr(&self) -> ptr::StructReader<'static> {
        self.descriptor
            .default
            .clone()
            .unwrap_or_else(ptr::StructReader::empty)
    }

    /// Returns the default value of the field
    #[inline]
    pub fn default(&self) -> S::Reader<'static, Empty> {
        ty::StructReader::from_ptr(self.default_ptr())
    }
}

impl<'p, T: Table, S: ty::Struct> PtrFieldReader<'_, 'p, T, Struct<S>> {
    #[inline]
    fn default_imbued_ptr(&self) -> ptr::StructReader<'static, T> {
        self.default_ptr().imbue_from(self.repr)
    }

    #[inline]
    pub fn get(&self) -> S::Reader<'p, T> {
        ty::StructReader::from_ptr(match self.raw_ptr().to_struct() {
            Ok(Some(ptr)) => ptr,
            _ => self.default_imbued_ptr(),
        })
    }

    #[inline]
    pub fn get_option(&self) -> Option<S::Reader<'p, T>> {
        self.try_get_option().ok().flatten()
    }

    #[inline]
    pub fn try_get(&self) -> Result<S::Reader<'p, T>> {
        Ok(ty::StructReader::from_ptr(
            match self.raw_ptr().to_struct() {
                Ok(Some(ptr)) => ptr,
                Ok(None) => self.default_imbued_ptr(),
                Err(err) => return Err(err),
            },
        ))
    }

    #[inline]
    pub fn try_get_option(&self) -> Result<Option<S::Reader<'p, T>>> {
        match self.raw_ptr().to_struct() {
            Ok(Some(ptr)) => Ok(Some(ty::StructReader::from_ptr(ptr))),
            Ok(None) => Ok(None),
            Err(err) => Err(err),
        }
    }
}

impl<'b, T, B, S> PtrField<Struct<S>, B>
where
    T: Table,
    B: BuildAccessablePtr<'b, Table = T>,
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
                if let Some(default) = default {
                    builder.copy_with_caveats(default, false);
                }
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
    pub fn try_set<E: ErrorHandler>(
        &mut self,
        value: &S::Reader<'_, impl InsertableInto<T>>,
        err_handler: E,
    ) -> Result<(), E::Error> {
        self.raw_build_ptr().try_set_struct(
            value.as_ptr(),
            ptr::CopySize::Minimum(S::SIZE),
            err_handler,
        )
    }

    #[inline]
    pub fn set(&mut self, value: &S::Reader<'_, impl InsertableInto<T>>) {
        self.try_set(value, IgnoreErrors).unwrap()
    }
}

impl<S: ty::Struct, Repr> PtrVariant<Struct<S>, Repr> {
    #[inline]
    fn default_ptr(&self) -> ptr::StructReader<'static> {
        self.descriptor
            .default
            .clone()
            .unwrap_or_else(ptr::StructReader::empty)
    }

    /// Returns the default value of the field
    #[inline]
    pub fn default(&self) -> S::Reader<'static, Empty> {
        ty::StructReader::from_ptr(self.default_ptr())
    }
}

impl<'p, T: Table, S: ty::Struct> PtrVariantReader<'_, 'p, T, Struct<S>> {
    #[inline]
    fn default_imbued_ptr(&self) -> ptr::StructReader<'static, T> {
        self.descriptor
            .default
            .clone()
            .unwrap_or_else(ptr::StructReader::empty)
            .imbue_from(self.repr)
    }

    #[inline]
    pub fn get_or_default(&self) -> S::Reader<'p, T> {
        let ptr = match self.raw_ptr_or_null().to_struct() {
            Ok(Some(ptr)) => ptr,
            _ => self.default_imbued_ptr(),
        };
        ty::StructReader::from_ptr(ptr)
    }

    #[inline]
    pub fn get_option(&self) -> Option<S::Reader<'p, T>> {
        self.try_get_option().ok().flatten()
    }

    #[inline]
    pub fn try_get(&self) -> Result<S::Reader<'p, T>> {
        let ptr = match self.raw_ptr_or_null().to_struct() {
            Ok(Some(ptr)) => ptr,
            Ok(None) => self.default_imbued_ptr(),
            Err(err) => return Err(err),
        };
        Ok(ty::StructReader::from_ptr(ptr))
    }

    #[inline]
    pub fn try_get_option(&self) -> Result<Option<S::Reader<'p, T>>> {
        self.field()
            .map(|r| r.try_get_option())
            .transpose()
            .map(Option::flatten)
    }
}

impl<'b, T, B, S> PtrVariant<Struct<S>, B>
where
    T: Table,
    B: BuildAccessablePtr<'b, Table = T>,
    S: ty::Struct,
{
    #[inline]
    pub fn init(self) -> S::Builder<'b, T> {
        self.init_case().init()
    }

    #[inline]
    pub fn try_set<E: ErrorHandler>(
        &mut self,
        value: &S::Reader<'_, impl InsertableInto<T>>,
        err_handler: E,
    ) -> Result<(), E::Error> {
        self.set_case_and_build_ptr().try_set_struct(
            value.as_ptr(),
            CopySize::Minimum(S::SIZE),
            err_handler,
        )
    }

    #[inline]
    pub fn set(&mut self, value: &S::Reader<'_, impl InsertableInto<T>>) {
        self.try_set(value, IgnoreErrors).unwrap()
    }
}

impl PtrValue for text::Text {
    type Default = text::Reader<'static>;
}

impl FieldType for text::Text {
    field_type_items! {}
}

impl<Repr> PtrField<text::Text, Repr> {
    /// Returns the default value of the field
    #[inline]
    pub fn default(&self) -> text::Reader<'static> {
        self.descriptor.default.unwrap_or_else(text::Reader::empty)
    }
}

impl<'p, T: Table> PtrFieldReader<'_, 'p, T, text::Text> {
    #[inline]
    pub fn get(&self) -> text::Reader<'p> {
        self.get_option().unwrap_or_else(|| self.default())
    }

    /// Gets the text field as a string, returning an error if the value isn't valid UTF-8 text.
    /// This is shorthand for `get().as_str()`
    ///
    /// If the field is null or an error occurs while reading the pointer to the text itself, this
    /// returns the default value.
    #[inline]
    pub fn as_str(&self) -> Result<&'p str, Utf8Error> {
        self.get().as_str()
    }

    #[inline]
    pub fn get_option(&self) -> Option<text::Reader<'p>> {
        self.try_get_option().ok().flatten()
    }

    #[inline]
    pub fn try_get(&self) -> Result<text::Reader<'p>> {
        self.try_get_option()
            .map(|op| op.unwrap_or_else(|| self.default()))
    }

    #[inline]
    pub fn try_get_option(&self) -> Result<Option<text::Reader<'p>>> {
        match self.raw_ptr().to_blob() {
            Ok(Some(ptr)) => {
                let text = text::Reader::new(ptr).ok_or(Error::TextNotNulTerminated)?;

                Ok(Some(text))
            }
            Ok(None) => Ok(None),
            Err(err) => Err(err),
        }
    }
}

impl<'b, B: BuildAccessablePtr<'b>> PtrField<text::Text, B> {
    /// Returns a builder for the field. If it's not set or a error occurs while reading the
    /// existing value, the default is set instead.
    #[inline]
    pub fn get(self) -> text::Builder<'b> {
        let default = self.descriptor.default.unwrap_or_else(text::Reader::empty);
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
    pub fn set(&mut self, value: text::Reader<'_>) {
        self.raw_build_ptr().set_blob(value.into());
    }

    /// Set the text element to a copy of the given string.
    ///
    /// # Panics
    ///
    /// If the string is too large to fit in a Cap'n Proto message, this function will
    /// panic.
    #[inline]
    pub fn set_str(&mut self, value: &str) {
        self.try_set_str(value)
            .ok()
            .expect("str is too large to fit in a Cap'n Proto message")
    }

    #[inline]
    pub fn try_set_str(&mut self, value: &str) -> Result<(), text::TryFromStrError> {
        let Some(len) = u32::try_from(value.len() + 1)
            .ok()
            .and_then(text::ByteCount::new)
        else {
            return Err(text::TryFromStrError(()));
        };

        let blob = self.raw_build_ptr().init_blob(len.into());
        let mut builder = text::Builder::new_unchecked(blob);
        builder.as_bytes_mut().copy_from_slice(value.as_bytes());
        Ok(())
    }
}

impl<Repr> PtrVariant<text::Text, Repr> {
    /// Returns the default value of the field
    #[inline]
    pub fn default(&self) -> text::Reader<'static> {
        self.descriptor.default.unwrap_or_else(text::Reader::empty)
    }
}

impl<'p, T: Table> PtrVariantReader<'_, 'p, T, text::Text> {
    #[inline]
    pub fn get(&self) -> text::Reader<'p> {
        self.get_option().unwrap_or_else(|| self.default())
    }

    /// Gets the text field as a string, returning an error if the value isn't valid UTF-8 text.
    /// This is shorthand for `get().as_str()`
    ///
    /// If the field is null or an error occurs while reading the pointer to the text itself, this
    /// returns the default value.
    #[inline]
    pub fn as_str(&self) -> Result<&'p str, Utf8Error> {
        self.get().as_str()
    }

    #[inline]
    pub fn get_option(&self) -> Option<text::Reader<'p>> {
        self.try_get_option().ok().flatten()
    }

    #[inline]
    pub fn try_get(&self) -> Result<text::Reader<'p>> {
        self.try_get_option()
            .map(|op| op.unwrap_or_else(|| self.default()))
    }

    #[inline]
    pub fn try_get_option(&self) -> Result<Option<text::Reader<'p>>> {
        match self.raw_ptr_or_null().to_blob() {
            Ok(Some(ptr)) => {
                let text = text::Reader::new(ptr).ok_or(Error::TextNotNulTerminated)?;

                Ok(Some(text))
            }
            Ok(None) => Ok(None),
            Err(err) => Err(err),
        }
    }
}

impl<'b, B: BuildAccessablePtr<'b>> PtrVariant<text::Text, B> {
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
        self.init_case().init_byte_count(len)
    }

    #[inline]
    pub fn set(&mut self, value: text::Reader<'_>) {
        self.set_case_and_build_ptr().set_blob(value.into());
    }

    /// Set the text element to a copy of the given string.
    ///
    /// # Panics
    ///
    /// If the string is too large to fit in a Cap'n Proto message, this function will
    /// panic.
    #[inline]
    pub fn set_str(&mut self, value: &str) {
        self.try_set_str(value)
            .ok()
            .expect("str is too large to fit in a Cap'n Proto message")
    }

    #[inline]
    pub fn try_set_str(&mut self, value: &str) -> Result<(), text::TryFromStrError> {
        let Some(len) = u32::try_from(value.len() + 1)
            .ok()
            .and_then(text::ByteCount::new)
        else {
            return Err(text::TryFromStrError(()));
        };

        let blob = self.set_case_and_build_ptr().init_blob(len.into());
        let mut builder = text::Builder::new_unchecked(blob);
        builder.as_bytes_mut().copy_from_slice(value.as_bytes());
        Ok(())
    }
}

impl PtrValue for data::Data {
    type Default = data::Reader<'static>;
}

impl FieldType for data::Data {
    field_type_items! {}
}

impl<Repr> PtrField<data::Data, Repr> {
    /// Returns the default value of the field
    #[inline]
    pub fn default(&self) -> data::Reader<'static> {
        self.descriptor.default.unwrap_or_else(data::Reader::empty)
    }
}

impl<'p, T: Table> PtrFieldReader<'_, 'p, T, data::Data> {
    #[inline]
    pub fn get(&self) -> data::Reader<'p> {
        self.get_option().unwrap_or_else(|| self.default())
    }

    #[inline]
    pub fn get_option(&self) -> Option<data::Reader<'p>> {
        self.try_get_option().ok().flatten()
    }

    #[inline]
    pub fn try_get(&self) -> Result<data::Reader<'p>> {
        self.try_get_option()
            .map(|op| op.unwrap_or_else(|| self.default()))
    }

    #[inline]
    pub fn try_get_option(&self) -> Result<Option<data::Reader<'p>>> {
        match self.raw_ptr().to_blob() {
            Ok(Some(ptr)) => Ok(Some(ptr.into())),
            Ok(None) => Ok(None),
            Err(err) => Err(err),
        }
    }
}

impl<'b, B: BuildAccessablePtr<'b>> PtrField<data::Data, B> {
    /// Returns a builder for the field. If it's not set or a error occurs while reading the
    /// existing value, the default is set instead.
    #[inline]
    pub fn get(self) -> data::Builder<'b> {
        let default = self.descriptor.default.unwrap_or_else(data::Reader::empty);
        match self.into_raw_build_ptr().to_blob_mut() {
            Ok(data) => data,
            Err((_, ptr)) => ptr.set_blob(default.into()),
        }
        .into()
    }

    #[inline]
    pub fn init(self, count: ElementCount) -> data::Builder<'b> {
        self.into_raw_build_ptr().init_blob(count).into()
    }

    #[inline]
    pub fn set(&mut self, value: data::Reader<'_>) {
        self.raw_build_ptr().set_blob(value.into());
    }

    /// Set the data element to a copy of the given slice.
    ///
    /// # Panics
    ///
    /// If the slice is too large to fit in a Cap'n Proto message, this function will
    /// panic.
    #[inline]
    pub fn set_slice(&mut self, value: &[u8]) {
        self.try_set_slice(value).unwrap()
    }

    #[inline]
    pub fn try_set_slice(&mut self, value: &[u8]) -> Result<(), data::TryFromSliceError> {
        let value = data::Reader::try_from_slice(value)?;
        self.set(value);
        Ok(())
    }
}

impl<Repr> PtrVariant<data::Data, Repr> {
    /// Returns the default value of the field
    #[inline]
    pub fn default(&self) -> data::Reader<'static> {
        self.descriptor.default.unwrap_or_else(data::Reader::empty)
    }
}

impl<'p, T: Table> PtrVariantReader<'_, 'p, T, data::Data> {
    #[inline]
    pub fn get(&self) -> data::Reader<'p> {
        self.get_option().unwrap_or_else(|| self.default())
    }

    #[inline]
    pub fn get_option(&self) -> Option<data::Reader<'p>> {
        self.try_get_option().ok().flatten()
    }

    #[inline]
    pub fn try_get(&self) -> Result<data::Reader<'p>> {
        self.try_get_option()
            .map(|op| op.unwrap_or_else(|| self.default()))
    }

    #[inline]
    pub fn try_get_option(&self) -> Result<Option<data::Reader<'p>>> {
        match self.raw_ptr_or_null().to_blob() {
            Ok(Some(ptr)) => Ok(Some(ptr.into())),
            Ok(None) => Ok(None),
            Err(err) => Err(err),
        }
    }
}

impl<'b, T: Table> PtrVariantBuilder<'b, '_, T, data::Data> {
    #[inline]
    pub fn init(self, count: ElementCount) -> data::Builder<'b> {
        self.init_case().init(count)
    }

    #[inline]
    pub fn set(&mut self, value: data::Reader<'_>) {
        self.set_case_and_build_ptr().set_blob(value.into());
    }

    /// Set the data element to a copy of the given slice.
    ///
    /// # Panics
    ///
    /// If the slice is too large to fit in a Cap'n Proto message, this function will
    /// panic.
    #[inline]
    pub fn set_slice(&mut self, value: &[u8]) {
        self.try_set_slice(value).unwrap()
    }

    #[inline]
    pub fn try_set_slice(&mut self, value: &[u8]) -> Result<(), data::TryFromSliceError> {
        let value = data::Reader::try_from_slice(value)?;
        self.set(value);
        Ok(())
    }
}

impl PtrValue for AnyPtr {
    type Default = ptr::PtrReader<'static, Empty>;
}

impl FieldType for AnyPtr {
    field_type_items!();
}

impl<Repr> PtrField<AnyPtr, Repr> {
    /// Returns the default value of the field
    #[inline]
    pub fn default(&self) -> any::PtrReader<'static> {
        AnyPtr::from(self.descriptor.default.clone().unwrap_or_default())
    }
}

impl<'p, T: Table> PtrFieldReader<'_, 'p, T, AnyPtr> {
    /// Get the value of the field. An alias for `ptr()`
    #[inline]
    pub fn get(&self) -> any::PtrReader<'p, T> {
        self.ptr()
    }

    /// Get the value of the field. If the pointer value is null, this returns the default value.
    #[inline]
    pub fn get_or_default(&self) -> any::PtrReader<'p, T> {
        let ptr = self.ptr();
        if ptr.is_null() {
            self.default().imbue_from(&ptr)
        } else {
            ptr
        }
    }
}

impl<'b, B: BuildAccessablePtr<'b>> PtrField<AnyPtr, B> {
    /// Get the value of the field. An alias for `ptr()`
    #[inline]
    pub fn get(self) -> any::PtrBuilder<'b, B::Table> {
        self.ptr()
    }
}

impl<Repr> PtrVariant<AnyPtr, Repr> {
    /// Returns the default value of the field
    #[inline]
    pub fn default(&self) -> any::PtrReader<'static> {
        AnyPtr::from(self.descriptor.default.clone().unwrap_or_default())
    }
}

impl<'p, T: Table> PtrVariantReader<'_, 'p, T, AnyPtr> {
    /// Get the value of the field or a null pointer if it's not set.
    #[inline]
    pub fn get_or_null(&self) -> any::PtrReader<'p, T> {
        match self.field() {
            Some(f) => f.get(),
            None => AnyPtr::null().imbue_from(self.repr),
        }
    }

    /// Get the value of the field. If the pointer value is null, this returns the default value.
    #[inline]
    pub fn get_or_default(&self) -> any::PtrReader<'p, T> {
        let ptr = self.get_or_null();
        if ptr.is_null() {
            self.default().imbue_from(&ptr)
        } else {
            ptr
        }
    }
}

impl<'b, B: BuildAccessablePtr<'b>> PtrVariant<AnyPtr, B> {
    /// Set the case of the union to this field and get a pointer builder for the field.
    #[inline]
    pub fn get(self) -> any::PtrBuilder<'b, B::Table> {
        self.init_case().get()
    }
}

impl PtrValue for AnyStruct {
    type Default = ptr::StructReader<'static, Empty>;
}

impl FieldType for AnyStruct {
    field_type_items! {}
}

impl<Repr> PtrField<AnyStruct, Repr> {
    /// Returns the default value of the field
    #[inline]
    fn default_ptr(&self) -> ptr::StructReader<'static> {
        self.descriptor
            .default
            .clone()
            .unwrap_or_else(ptr::StructReader::empty)
    }

    /// Returns the default value of the field
    #[inline]
    pub fn default(&self) -> any::StructReader<'static> {
        AnyStruct(self.default_ptr())
    }
}

impl<'p, T: Table> PtrFieldReader<'_, 'p, T, AnyStruct> {
    #[inline]
    fn default_imbued(&self) -> any::StructReader<'static, T> {
        self.default().imbue_from(self.repr)
    }

    #[inline]
    pub fn get(&self) -> any::StructReader<'p, T> {
        self.get_option().unwrap_or_else(|| self.default_imbued())
    }

    #[inline]
    pub fn get_option(&self) -> Option<any::StructReader<'p, T>> {
        self.try_get_option().ok().flatten()
    }

    #[inline]
    pub fn try_get(&self) -> Result<any::StructReader<'p, T>> {
        self.try_get_option()
            .map(|op| op.unwrap_or_else(|| self.default_imbued()))
    }

    #[inline]
    pub fn try_get_option(&self) -> Result<Option<any::StructReader<'p, T>>> {
        match self.raw_ptr().to_struct() {
            Ok(Some(ptr)) => Ok(Some(ptr.into())),
            Ok(None) => Ok(None),
            Err(err) => Err(err),
        }
    }
}

impl<'b, T: Table, B: BuildAccessablePtr<'b, Table = T>> PtrField<AnyStruct, B> {
    /// Returns a builder for the field. If it's not set or a error occurs while reading the
    /// existing value, the field default is set in its place instead.
    #[inline]
    pub fn get(self, size: Option<StructSize>) -> any::StructBuilder<'b, T> {
        let default = &self.descriptor.default;
        let ptr = match self.into_raw_build_ptr().to_struct_mut(size) {
            Ok(ptr) => ptr,
            Err((_, ptr)) => match default {
                Some(default) => {
                    let mut new_size = default.size();
                    if let Some(size) = size {
                        new_size = new_size.max(size);
                    }
                    let mut builder = ptr.init_struct(new_size);
                    builder.copy_with_caveats(default, false);
                    builder
                }
                None => ptr.init_struct(size.unwrap_or_default()),
            },
        };
        unsafe { ty::StructBuilder::from_ptr(ptr) }
    }

    #[inline]
    pub fn init(self, size: StructSize) -> any::StructBuilder<'b, T> {
        unsafe { ty::StructBuilder::from_ptr(self.into_raw_build_ptr().init_struct(size)) }
    }

    /// Returns a builder for the field. If it's not set or a error occurs while reading the
    /// existing value, the struct default is set instead.
    #[inline]
    pub fn get_or_init(self, size: StructSize) -> any::StructBuilder<'b, T> {
        let ptr = self.into_raw_build_ptr().to_struct_mut_or_init(size);
        unsafe { ty::StructBuilder::from_ptr(ptr) }
    }

    /// Try to set this field to a copy of the given value.
    ///
    /// If an error occurs while reading the input value, it's passed to the error handler, which
    /// can choose to write null instead or return an error.
    #[inline]
    pub fn try_set<E: ErrorHandler>(
        &mut self,
        value: &any::StructReader<'_, impl InsertableInto<T>>,
        size: Option<StructSize>,
        err_handler: E,
    ) -> Result<(), E::Error> {
        let size = size
            .map(ptr::CopySize::Minimum)
            .unwrap_or(CopySize::FromValue);
        self.raw_build_ptr()
            .try_set_struct(value.as_ptr(), size, err_handler)
    }

    #[inline]
    pub fn set(
        &mut self,
        value: &any::StructReader<'_, impl InsertableInto<T>>,
        size: Option<StructSize>,
    ) {
        self.try_set(value, size, IgnoreErrors).unwrap()
    }
}

impl<'p, T: Table> PtrVariantReader<'_, 'p, T, AnyStruct> {
    #[inline]
    fn default_imbued_ptr(&self) -> ptr::StructReader<'static, T> {
        self.descriptor
            .default
            .clone()
            .unwrap_or_else(ptr::StructReader::empty)
            .imbue_from(self.repr)
    }

    #[inline]
    pub fn get_or_default(&self) -> any::StructReader<'p, T> {
        let ptr = match self.raw_ptr_or_null().to_struct() {
            Ok(Some(ptr)) => ptr,
            _ => self.default_imbued_ptr(),
        };
        ty::StructReader::from_ptr(ptr)
    }

    #[inline]
    pub fn get_option(&self) -> Option<any::StructReader<'p, T>> {
        self.try_get_option().ok().flatten()
    }

    #[inline]
    pub fn try_get(&self) -> Result<any::StructReader<'p, T>> {
        let ptr = match self.raw_ptr_or_null().to_struct() {
            Ok(Some(ptr)) => ptr,
            Ok(None) => self.default_imbued_ptr(),
            Err(err) => return Err(err),
        };
        Ok(ty::StructReader::from_ptr(ptr))
    }

    #[inline]
    pub fn try_get_option(&self) -> Result<Option<any::StructReader<'p, T>>> {
        self.field()
            .map(|r| r.try_get_option())
            .transpose()
            .map(Option::flatten)
    }
}

impl<'b, T: Table, B: BuildAccessablePtr<'b, Table = T>> PtrVariant<AnyStruct, B> {
    #[inline]
    pub fn init(self, size: StructSize) -> any::StructBuilder<'b, T> {
        self.init_case().init(size)
    }

    #[inline]
    pub fn try_set<E: ErrorHandler>(
        &mut self,
        value: &any::StructReader<'_, impl InsertableInto<T>>,
        size: Option<StructSize>,
        err_handler: E,
    ) -> Result<(), E::Error> {
        let size = size
            .map(ptr::CopySize::Minimum)
            .unwrap_or(CopySize::FromValue);
        self.set_case_and_build_ptr()
            .try_set_struct(value.as_ref(), size, err_handler)
    }

    #[inline]
    pub fn set(
        &mut self,
        value: &any::StructReader<'_, impl InsertableInto<T>>,
        size: Option<StructSize>,
    ) {
        self.try_set(value, size, IgnoreErrors).unwrap()
    }
}

impl PtrValue for AnyList {
    type Default = ptr::ListReader<'static, Empty>;
}

impl FieldType for AnyList {
    field_type_items! {}
}

impl<Repr> PtrField<AnyList, Repr> {
    /// Returns the default value of the field
    #[inline]
    fn default_ptr(&self) -> ptr::ListReader<'static> {
        self.descriptor
            .default
            .clone()
            .unwrap_or_else(|| ptr::ListReader::empty(ElementSize::Void))
    }

    /// Returns the default value of the field
    #[inline]
    pub fn default(&self) -> any::ListReader<'static> {
        self.default_ptr().into()
    }
}

impl<'p, T: Table> PtrFieldReader<'_, 'p, T, AnyList> {
    #[inline]
    fn default_imbued(&self) -> any::ListReader<'static, T> {
        self.default().imbue_from(self.repr)
    }

    #[inline]
    pub fn get(&self) -> any::ListReader<'p, T> {
        self.get_option().unwrap_or_else(|| self.default_imbued())
    }

    #[inline]
    pub fn get_option(&self) -> Option<any::ListReader<'p, T>> {
        self.try_get_option().ok().flatten()
    }

    #[inline]
    pub fn try_get(&self) -> Result<any::ListReader<'p, T>> {
        self.try_get_option()
            .map(|op| op.unwrap_or_else(|| self.default_imbued()))
    }

    #[inline]
    pub fn try_get_option(&self) -> Result<Option<any::ListReader<'p, T>>> {
        match self.raw_ptr().to_list(None) {
            Ok(Some(ptr)) => Ok(Some(ptr.into())),
            Ok(None) => Ok(None),
            Err(err) => Err(err),
        }
    }
}

impl<'b, T: Table, B: BuildAccessablePtr<'b, Table = T>> PtrField<AnyList, B> {
    /// Returns a builder for the field. If it's not set or a error occurs while reading the
    /// existing value, the default is set instead.
    #[inline]
    pub fn get(self) -> any::ListBuilder<'b, T> {
        let default = &self.descriptor.default;
        match self.into_raw_build_ptr().to_list_mut(None) {
            Ok(ptr) => ptr.into(),
            Err((_, ptr)) => {
                let new_list = if let Some(default) = default {
                    let mut new_list = ptr.init_list(default.element_size(), default.len());
                    new_list.try_copy_from(default, UnwrapErrors).unwrap();
                    new_list
                } else {
                    ptr.init_list(ElementSize::Void, ElementCount::ZERO)
                };
                AnyList::from(new_list)
            }
        }
    }

    #[inline]
    pub fn try_set<E: ErrorHandler>(
        &mut self,
        value: &any::ListReader<'_, impl InsertableInto<T>>,
        err_handler: E,
    ) -> Result<(), E::Error> {
        self.raw_build_ptr()
            .try_set_list(value.as_ref(), CopySize::FromValue, err_handler)
    }

    #[inline]
    pub fn set(&mut self, value: &any::ListReader<'_, impl InsertableInto<T>>) {
        self.try_set(value, IgnoreErrors).unwrap()
    }

    #[inline]
    pub fn init(self, size: ElementSize, count: u32) -> any::ListBuilder<'b, T> {
        self.try_init(size, count)
            .expect("too many elements for list")
    }

    #[inline]
    pub fn try_init(
        self,
        size: ElementSize,
        count: u32,
    ) -> Result<any::ListBuilder<'b, T>, TooManyElementsError> {
        if count > size.max_elements().get() {
            return Err(TooManyElementsError(()));
        }
        let count = ElementCount::new(count).unwrap();

        Ok(AnyList::from(
            self.into_raw_build_ptr().init_list(size, count),
        ))
    }
}

impl<'b, 'p, T: Table> PtrVariantReader<'b, 'p, T, AnyList> {
    #[inline]
    fn default_imbued_ptr(&self) -> ptr::ListReader<'static, T> {
        self.descriptor
            .default
            .clone()
            .unwrap_or_else(|| ptr::ListReader::empty(ElementSize::Void))
            .imbue_from(self.repr)
    }

    #[inline]
    pub fn get_or_default(&self) -> any::ListReader<'p, T> {
        let ptr = match self.raw_ptr_or_null().to_list(None) {
            Ok(Some(ptr)) => ptr,
            _ => self.default_imbued_ptr(),
        };
        AnyList::from(ptr)
    }

    #[inline]
    pub fn get_option(&self) -> Option<any::ListReader<'p, T>> {
        self.try_get_option().ok().flatten()
    }

    #[inline]
    pub fn try_get(&self) -> Result<any::ListReader<'p, T>> {
        let ptr = match self.raw_ptr_or_null().to_list(None) {
            Ok(Some(ptr)) => ptr,
            Ok(None) => self.default_imbued_ptr(),
            Err(err) => return Err(err),
        };
        Ok(AnyList::from(ptr))
    }

    #[inline]
    pub fn try_get_option(&self) -> Result<Option<any::ListReader<'p, T>>> {
        self.field()
            .map(|r| r.try_get_option())
            .transpose()
            .map(Option::flatten)
    }
}

impl<'b, T: Table, B: BuildAccessablePtr<'b, Table = T>> PtrVariant<AnyList, B> {
    #[inline]
    pub fn try_set<E: ErrorHandler>(
        &mut self,
        value: &any::ListReader<'_, impl InsertableInto<T>>,
        err_handler: E,
    ) -> Result<(), E::Error> {
        self.set_case_and_build_ptr()
            .try_set_list(value.as_ref(), CopySize::FromValue, err_handler)
    }

    #[inline]
    pub fn set(&mut self, value: &any::ListReader<'_, impl InsertableInto<T>>) {
        self.try_set(value, IgnoreErrors).unwrap()
    }

    #[inline]
    pub fn init(self, size: ElementSize, count: u32) -> any::ListBuilder<'b, T> {
        self.try_init(size, count)
            .expect("too many elements for list")
    }

    #[inline]
    pub fn try_init(
        self,
        size: ElementSize,
        count: u32,
    ) -> Result<any::ListBuilder<'b, T>, TooManyElementsError> {
        if count > size.max_elements().get() {
            return Err(TooManyElementsError(()));
        }
        let count = ElementCount::new(count).unwrap();

        Ok(AnyList::from(
            self.init_case().into_raw_build_ptr().init_list(size, count),
        ))
    }
}

impl<C: ty::Capability> FieldType for Capability<C> {
    field_type_items! {}
}

impl<'b, 'p, T, C> PtrFieldReader<'b, 'p, T, Capability<C>>
where
    T: CapTable,
    C: ty::Capability<Client = T::Cap>,
{
    #[inline]
    pub fn try_get_option(&self) -> Result<Option<C>> {
        match self.raw_ptr().try_to_capability() {
            Ok(Some(c)) => Ok(Some(C::from_client(c))),
            Ok(None) => Ok(None),
            Err(err) => Err(err),
        }
    }
}

impl<'b, 'p, T, C> PtrFieldReader<'b, 'p, T, Capability<C>>
where
    T: CapTable + BreakableCapSystem,
    C: ty::Capability<Client = T::Cap>,
{
    #[inline]
    pub fn get(&self) -> C {
        let cap = match self.raw_ptr().try_to_capability() {
            Ok(Some(c)) => c,
            Ok(None) => T::null(),
            Err(err) => T::broken(&err),
        };
        C::from_client(cap)
    }

    #[inline]
    pub fn get_option(&self) -> Option<C> {
        self.try_get_option().ok().flatten()
    }

    #[inline]
    pub fn try_get(&self) -> Result<C> {
        let cap = self.raw_ptr().try_to_capability()?.unwrap_or_else(T::null);
        Ok(C::from_client(cap))
    }
}

impl<'b, 'p, T, C> PtrFieldBuilder<'b, 'p, T, Capability<C>>
where
    T: CapTable,
    C: ty::Capability<Client = T::Cap>,
{
    #[inline]
    pub fn set(&mut self, client: C) {
        self.raw_build_ptr().set_cap(client.into_inner())
    }
}

impl<'b, 'p, T, C> PtrVariantReader<'b, 'p, T, Capability<C>>
where
    T: CapTable,
    C: ty::Capability<Client = T::Cap>,
{
    #[inline]
    pub fn try_get_option(&self) -> Result<Option<C>> {
        match self.raw_ptr_or_null().try_to_capability() {
            Ok(Some(c)) => Ok(Some(C::from_client(c))),
            Ok(None) => Ok(None),
            Err(err) => Err(err),
        }
    }
}

impl<'b, 'p, T, C> PtrVariantReader<'b, 'p, T, Capability<C>>
where
    T: CapTable + BreakableCapSystem,
    C: ty::Capability<Client = T::Cap>,
{
    #[inline]
    pub fn get(&self) -> C {
        let cap = match self.raw_ptr_or_null().try_to_capability() {
            Ok(Some(c)) => c,
            Ok(None) => T::null(),
            Err(err) => T::broken(&err),
        };
        C::from_client(cap)
    }

    #[inline]
    pub fn get_option(&self) -> Option<C> {
        self.try_get_option().ok().flatten()
    }

    #[inline]
    pub fn try_get(&self) -> Result<C> {
        let cap = self
            .raw_ptr_or_null()
            .try_to_capability()?
            .unwrap_or_else(T::null);
        Ok(C::from_client(cap))
    }
}

impl<'b, 'p, T, C> PtrVariantBuilder<'b, 'p, T, Capability<C>>
where
    T: CapTable,
    C: ty::Capability<Client = T::Cap>,
{
    #[inline]
    pub fn set(&mut self, client: C) {
        self.set_case_and_build_ptr().set_cap(client.into_inner())
    }
}
