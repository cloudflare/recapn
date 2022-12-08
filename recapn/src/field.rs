//! Types and traits associated with the simple accessor API.
//!
//! We want our generated code to be as simple as possible. Rather than have multiple accessors as
//! get_foo, set_foo, has_foo, disown_foo, adopt_foo, etc, we provide two accessor functions:
//! foo and foo_mut. foo returns an accessor suitable for reading a value, foo_mut returns an
//! accessor suitable for mutating a value. This is better since it lets us give users tons of
//! different ways of accessing values without having to make an equal number of accessors. It's
//! still as fast as having a standard accessor, but gives us (the library) more control, which
//! means we can offer greater flexibility. For example, these returned accessors can stored locally
//! which lets us do things like:
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

use core::convert::{Infallible, TryFrom};
use core::marker::PhantomData;
use core::ops::{Deref, DerefMut};

use crate::internal::Sealed;
use crate::list::{self, List};
use crate::ptr::{Capable, StructBuilder, StructReader};
use crate::rpc::{Empty, Table};
use crate::ty::{self, ListValue, Value};

use self::internal::{PtrAccessable, PtrAccessableMut};

/// A wrapper type used to implement methods for struct fields.
pub struct Struct<S: ty::Struct> {
    s: PhantomData<fn() -> S>,
}

impl<S: ty::Struct> Sealed for Struct<S> {}
impl<S: ty::Struct> Value for Struct<S> {
    type Default = StructReader<'static, Empty>;
}
impl<S: ty::Struct> ListValue for Struct<S> {
    const ELEMENT_SIZE: list::ElementSize = list::ElementSize::InlineComposite(S::SIZE);
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
    const ELEMENT_SIZE: list::ElementSize = list::ElementSize::TwoBytes;
}

/// Represents a group of fields in a Cap'n Proto struct.
pub struct Group<G: FieldGroup + 'static> {
    v: PhantomData<fn() -> G>,
}

mod internal {
    use crate::ptr::internal::FieldData;

    pub trait PtrAccessable {
        /// Reads a data field, possibly without checking bounds.
        unsafe fn data<D: FieldData>(&self, slot: usize, default: D) -> D;
    }

    pub trait PtrAccessableMut: PtrAccessable {
        unsafe fn set_data<D: FieldData>(&mut self, slot: usize, value: D, default: D);
    }
}

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
pub trait Viewable: core::ops::Deref {
    type ViewOf<V: FieldType>;
}

impl<'a, T: Accessable> Viewable for &'a T {
    type ViewOf<V: FieldType> = V::Ref<'a, T>;
}

impl<'a, T: AccessableMut> Viewable for &'a mut T {
    type ViewOf<V: FieldType> = V::Mut<'a, T>;
}

/// A structure that can be used to create an "accessor" for reading a field.
pub trait Accessable: internal::PtrAccessable + Sized {
    /// Creates an accessor for a field given the specified descriptor.
    unsafe fn get<'b, V: AccessableField<Self>>(
        &'b self,
        descriptor: &'b Descriptor<V>,
    ) -> Accessor<'b, Self, V>;

    /// Creates a variant accessor for a field given the specified descriptor.
    unsafe fn get_variant<'b, G: FieldGroup, V: AccessableField<Self>>(
        &'b self,
        descriptor: &'b VariantDescriptor<G, V>,
    ) -> Variant<'b, Self, G, V> {
        VariantField {
            a: PhantomData,
            descriptor,
            repr: self,
        }
    }
}

/// A structure that can be used to create a mutable "accessor" for reading and writing a field.
pub trait AccessableMut: Accessable + internal::PtrAccessableMut {
    /// Creates a mutable accessor for a field given the specified descriptor.
    unsafe fn get_mut<'b, V: AccessableField<Self>>(
        &'b mut self,
        descriptor: &'b Descriptor<V>,
    ) -> AccessorMut<'b, Self, V>;

    /// Creates a mutable variant accessor for a field given the specified descriptor.
    unsafe fn get_variant_mut<'b, G: FieldGroup, V: AccessableField<Self>>(
        &'b mut self,
        descriptor: &'b VariantDescriptor<G, V>,
    ) -> VariantMut<'b, Self, G, V> {
        VariantField {
            a: PhantomData,
            descriptor,
            repr: self,
        }
    }
}

pub type ViewOf<A, V> = <A as Viewable>::ViewOf<V>;

pub type Accessor<'a, A, V> = <V as FieldType>::Ref<'a, A>;

pub type AccessorMut<'a, A, V> = <V as FieldType>::Mut<'a, A>;

pub type Variant<'a, A, G, V> = VariantField<'a, G, V, &'a A>;

pub type VariantMut<'a, A, G, V> = VariantField<'a, G, V, &'a mut A>;

/// Describes a group of fields. This is primarily used for clearing unions and groups of fields within a struct.
pub trait FieldGroup {
    fn init<A: AccessableMut + ?Sized>(repr: &mut A);
}

/// Describes a value in a "slot" in a struct. This does not include groups or void, which don't
/// have an associated default value or slot.
pub struct FieldInfo<V: ty::Value> {
    pub slot: usize,
    pub default: V::Default,
}

/// Describes a type that can be a field in a Cap'n Proto struct type.
pub trait FieldType {
    /// A type used to describe the field type. This will likely be a type of FieldInfo, except for Void and groups,
    type Descriptor;

    /// The result of getting a read-only accessor to a field of this type.
    type Ref<'a, T>
    where
        T: 'a;
    /// The result of getting a read-write accessor to a field of this type.
    type Mut<'a, T>
    where
        T: 'a;
}

pub type Descriptor<V> = <V as FieldType>::Descriptor;

/// Describes a variant in a union in a struct. Because this is not attached to a value, it can
/// also be used as group info, as groups aren't values.
pub struct VariantInfo<G: FieldGroup> {
    g: PhantomData<fn() -> G>,
    slot: usize,
    case: u16,
}

/// Describes a value in a union variant in a struct.
pub struct VariantDescriptor<G: FieldGroup, V: FieldType> {
    variant: VariantInfo<G>,
    field: Descriptor<V>,
}

/// Provides a specialized view into a field value.
///
/// This type exists to allow us to control what type is returned from Accessable functions
/// depending on the value type. When reading primitive values, there's only one valid operation
/// users can perform: `get`. Forcing the user to call get on primitive values like this would be a
/// pain, so instead we return the value directly instead of going through some container.
///
/// Pointer types however need containers. A pointer value can have the special value "null", but
/// also have default values. Pointers can also return errors during validation, but users may
/// wish to have the accessor return a safe default for them. That's many more ways of accessing
/// values, and for that we need a container that can provide those functions.
///
/// Groups, while similar to structs, don't have any descriptor info. They simply act as views over
/// other structs, so our descriptor for these types can be a simple unit value, and get functions
/// can return the group directly.
pub trait AccessableField<Repr>: FieldType {
    /// Returns a view of the value over the given representation given the specified descriptor.
    ///
    /// Note: this function is UNSAFE. The returned view makes a number of assumptions about the
    /// given descriptor in the name of performance. The descriptor:
    ///
    /// * Must contain a valid default value.
    /// * Must have a slot that is in a struct's data or pointer section bounds.
    ///
    /// Failure to follow these constraints will result in ***undefined behavior***.
    unsafe fn get<'a>(repr: &'a Repr, descriptor: &'a Self::Descriptor) -> Self::Ref<'a, Repr>;

    /// Returns a view of the value over the given representation given the specified descriptor.
    ///
    /// Note: this function is UNSAFE. The returned view makes a number of assumptions about the
    /// given descriptor in the name of performance. The descriptor:
    ///
    /// * Must contain a valid default value.
    /// * Must have a slot that is in a struct's data or pointer section bounds.
    ///
    /// Failure to follow these constraints will result in ***undefined behavior***.
    unsafe fn get_mut<'a>(
        repr: &'a mut Repr,
        descriptor: &'a Self::Descriptor,
    ) -> Self::Mut<'a, Repr>;
}

impl FieldType for () {
    type Descriptor = ();

    type Ref<'a, T> = () where T: 'a;
    type Mut<'a, T> = () where T: 'a;
}

impl<T: Accessable> AccessableField<T> for () {
    #[inline]
    unsafe fn get<'a>(_: &'a T, _: &'a Self::Descriptor) -> Self::Ref<'a, T> {
        ()
    }
    #[inline]
    unsafe fn get_mut<'a>(_: &'a mut T, _: &'a Self::Descriptor) -> Self::Mut<'a, T> {
        ()
    }
}

/// A type representing a union variant that doesn't exist in the Cap'n Proto schema.
pub struct NotInSchema {
    pub case: u16,
}

pub struct UnionSlot<Repr> {
    repr: Repr,
    case: u16,
}

pub struct Field<'a, V: FieldType, Repr> {
    a: PhantomData<&'a ()>,
    repr: Repr,
    descriptor: &'a Descriptor<V>,
}

pub struct VariantField<'a, G: FieldGroup, V: FieldType, Repr: Viewable> {
    a: PhantomData<&'a ()>,
    descriptor: &'a VariantDescriptor<G, V>,
    repr: Repr,
}

impl<'a, G, V, Repr> VariantField<'a, G, V, Repr>
where
    G: FieldGroup,
    V: AccessableField<Repr::Target>,
    Repr: Viewable,
    Repr::Target: Accessable,
{
    pub fn is_set(&self) -> bool {
        let variant = &self.descriptor.variant;
        unsafe { self.repr.data(variant.slot, 0) == variant.case }
    }

    pub fn field(&self) -> Option<Accessor<'_, Repr::Target, V>> {
        if self.is_set() {
            unsafe { Some(self.field_unchecked()) }
        } else {
            None
        }
    }

    pub unsafe fn field_unchecked(&self) -> Accessor<'_, Repr::Target, V> {
        V::get(&*self.repr, &self.descriptor.field)
    }
}

impl<'a, G, V, Repr> VariantField<'a, G, V, Repr>
where
    G: FieldGroup,
    V: AccessableField<Repr::Target>,
    Repr: Viewable + DerefMut,
    Repr::Target: AccessableMut,
{
    pub fn field_mut(&mut self) -> Option<AccessorMut<'_, Repr::Target, V>> {
        if self.is_set() {
            unsafe { Some(self.field_mut_unchecked()) }
        } else {
            None
        }
    }

    /// Returns a field accessor for the field without checking if it's already set.
    pub unsafe fn field_mut_unchecked(&mut self) -> AccessorMut<'_, Repr::Target, V> {
        V::get_mut(&mut *self.repr, &self.descriptor.field)
    }

    /// Clears the union and sets it to this variant, returning an accessor to mutate the field
    pub fn init(&mut self) -> AccessorMut<'_, Repr::Target, V> {
        G::init(&mut *self.repr);
        unsafe {
            let variant = &self.descriptor.variant;
            self.repr.set_data(variant.slot, variant.case, 0);
            self.field_mut_unchecked()
        }
    }

    /// Returns the field if it's set, or inits it if it's not set.
    pub fn field_or_init(&mut self) -> AccessorMut<'_, Repr::Target, V> {
        if self.is_set() {
            unsafe { self.field_mut_unchecked() }
        } else {
            self.init()
        }
    }
}

impl<'a, G, Repr> VariantField<'a, G, (), Repr>
where
    G: FieldGroup,
    Repr: Viewable + DerefMut,
    Repr::Target: AccessableMut,
{
    /// An alias for `init()`.
    pub fn set(&mut self) {
        self.init()
    }
}

macro_rules! data_accessable {
    ($($ty:ty),+) => {
        $(
            impl FieldType for $ty {
                type Descriptor = FieldInfo<Self>;

                type Ref<'a, T> = $ty
                where
                    T: 'a;

                type Mut<'a, T> = Field<'a, Self, &'a mut T>
                where
                    T: 'a;
            }

            impl<T: Accessable> AccessableField<T> for $ty {
                unsafe fn get<'a>(repr: &'a T, descriptor: &'a Self::Descriptor) -> Self::Ref<'a, T> {
                    repr.data(descriptor.slot, descriptor.default)
                }
                unsafe fn get_mut<'a>(repr: &'a mut T, descriptor: &'a Self::Descriptor) -> Self::Mut<'a, T> {
                    Field { a: PhantomData, repr, descriptor }
                }
            }

            impl<'a, T: AccessableMut> Field<'a, $ty, &'a mut T> {
                pub fn get(&self) -> $ty {
                    unsafe { self.repr.data(self.descriptor.slot, self.descriptor.default) }
                }
                pub fn set(&mut self, value: $ty) {
                    unsafe { self.repr.set_data(self.descriptor.slot, value, self.descriptor.default) }
                }
            }

            impl<'a, G, Repr> VariantField<'a, G, $ty, Repr>
            where
                G: FieldGroup,
                Repr: Viewable,
                Repr::Target: Accessable,
            {
                pub fn get(&self) -> $ty {
                    self.field().unwrap_or(self.descriptor.field.default)
                }
                /// An alias for `field()`
                pub fn get_option(&self) -> Option<$ty> {
                    self.field()
                }
            }

            impl<'a, G, Repr> VariantField<'a, G, $ty, Repr>
            where
                G: FieldGroup,
                Repr: Viewable + DerefMut,
                Repr::Target: AccessableMut,
            {
                pub fn set(&mut self, value: $ty) {
                    self.init().set(value)
                }
            }
        )+
    };
}

data_accessable!(bool, u8, i8, u16, i16, u32, i32, u64, i64);

impl<E: ty::Enum> FieldType for Enum<E> {
    type Descriptor = FieldInfo<Self>;

    type Ref<'a, T> = E
    where
        T: 'a;

    type Mut<'a, T> = Field<'a, Self, &'a mut T>
    where
        T: 'a;
}

impl<T: Accessable, E: ty::Enum> AccessableField<T> for Enum<E> {
    unsafe fn get<'a>(repr: &'a T, descriptor: &'a Self::Descriptor) -> Self::Ref<'a, T> {
        repr.data::<u16>(descriptor.slot, descriptor.default.into())
            .into()
    }
    unsafe fn get_mut<'a>(repr: &'a mut T, descriptor: &'a Self::Descriptor) -> Self::Mut<'a, T> {
        Field {
            a: PhantomData,
            repr,
            descriptor,
        }
    }
}

impl<'a, T: AccessableMut, E: ty::Enum> Field<'a, Enum<E>, &'a mut T> {
    pub fn get(&self) -> E {
        unsafe {
            self.repr
                .data(self.descriptor.slot, self.descriptor.default.into())
                .into()
        }
    }
    pub fn set(&mut self, value: E) {
        let (value, default) = (value.into(), self.descriptor.default.into());
        unsafe { self.repr.set_data(self.descriptor.slot, value, default) }
    }
}

impl<'a, G, E, Repr> VariantField<'a, G, Enum<E>, Repr>
where
    G: FieldGroup,
    E: ty::Enum,
    Repr: Viewable,
    Repr::Target: Accessable,
{
    pub fn get(&self) -> E {
        self.field().unwrap_or(self.descriptor.field.default)
    }
    /// An alias for field()
    pub fn get_option(&self) -> Option<E> {
        self.field()
    }
}

impl<'a, G, E, Repr> VariantField<'a, G, Enum<E>, Repr>
where
    G: FieldGroup,
    E: ty::Enum,
    Repr: Viewable + DerefMut,
    Repr::Target: AccessableMut,
{
    pub fn set(&mut self, value: E) {
        self.init().set(value)
    }
}
