//! Types and traits implemented by foreign generated user types like enums and structs.

use core::convert::TryFrom;

use crate::any;
use crate::field;
use crate::internal::Sealed;
use crate::list::{self, List, ElementSize};
use crate::ptr::{self, StructSize, MessageSize};
use crate::rpc::{Capable, Table};
use crate::ReaderOf;
use crate::{NotInSchema, IntoFamily, Result};

/// An enum marker trait.
pub trait Enum: TypeKind<Kind = kind::Enum<Self>> + Copy + TryFrom<u16, Error = NotInSchema> + Into<u16> + Default + 'static {}

pub type EnumResult<E> = Result<E, NotInSchema>;

/// A capability marker trait.
pub trait Capability: TypeKind<Kind = kind::Capability<Self>> + 'static {
    /// The typeless client this type is a wrapper around.
    type Client;

    /// Convert from a typeless client into an instance of this type.
    fn from_client(c: Self::Client) -> Self;

    /// Unwrap the inner typeless client.
    fn into_inner(self) -> Self::Client;
}

/// Provides associated types for readers and builders of a struct with the given type.
/// 
/// Note, this is applied to both structs and *groups* which are not values that you can read
/// from a pointer.
pub trait StructView: 'static {
    type Reader<'a, T: Table>: StructReader<Ptr = ptr::StructReader<'a, T>> + IntoFamily<Family = Self>;
    type Builder<'a, T: Table>: StructBuilder<Ptr = ptr::StructBuilder<'a, T>> + IntoFamily<Family = Self>;
}

/// A marker that indicates that a type represents a struct.
/// 
/// This provides the struct's size, along with the reader and builder associated types.
pub trait Struct: TypeKind<Kind = kind::Struct<Self>> + StructView {
    const SIZE: StructSize;
}

/// Gets the size of a struct type. This is effectively the same as `<S as Struct>::SIZE`
#[inline]
pub const fn size_of<S: Struct>() -> StructSize {
    S::SIZE
}

/// Creates a reader for the default value of the given struct type.
#[inline]
pub fn struct_default<S: Struct>() -> ReaderOf<'static, S> {
    let empty = ptr::StructReader::empty();
    StructReader::from_ptr(empty)
}

/// A safely typed wrapper around a raw reader or builder.
pub trait TypedPtr {
    /// The underlying pointer type for this type.
    type Ptr;
}

/// A helper trait for easy reader conversions between typed to untyped structs.
pub trait StructReader: TypedPtr + From<Self::Ptr> + Into<Self::Ptr> + AsRef<Self::Ptr> {
    #[inline]
    fn from_ptr(ptr: Self::Ptr) -> Self {
        Self::from(ptr)
    }
    #[inline]
    fn as_ptr(&self) -> &Self::Ptr {
        self.as_ref()
    }
    #[inline]
    fn into_ptr(self) -> Self::Ptr {
        self.into()
    }
}

/// A trait exposing the total_size function for calculating the size of an object (struct, list)
/// and all of its subobjects.
pub trait TotalSize {
    /// Calculates the size of the object.
    fn total_size(&self) -> Result<MessageSize>;
}

impl<'a, Type> TotalSize for Type
where
    Type: TypedPtr + AsRef<Type::Ptr>,
    Type::Ptr: TotalSize,
{
    #[inline]
    fn total_size(&self) -> Result<MessageSize> {
        self.as_ref().total_size()
    }
}

pub trait StructBuilder: TypedPtr + Into<Self::Ptr> + AsRef<Self::Ptr> + AsMut<Self::Ptr> {
    /// Interprets the raw builder as this type.
    /// 
    /// # Safety
    /// 
    /// The specified builder must have a size greater than or equal to the size of this
    /// struct type.
    unsafe fn from_ptr(ptr: Self::Ptr) -> Self;

    #[inline]
    fn into_ptr(self) -> Self::Ptr {
        self.into()
    }
    #[inline]
    fn as_ptr(&self) -> &Self::Ptr {
        self.as_ref()
    }
    #[inline]
    fn as_ptr_mut(&mut self) -> &mut Self::Ptr {
        self.as_mut()
    }
}

/// An extension trait to implement an `as_reader` function for types including foreign
/// struct builders
pub trait AsReader {
    /// The resulting reader type
    type Reader;

    /// Performs the conversion
    fn as_reader(self) -> Self::Reader;
}

impl<'builder, 'borrow, Type, T> AsReader for &'borrow Type
where
    Type: StructBuilder<Ptr = ptr::StructBuilder<'builder, T>> + IntoFamily + Capable<Table = T>,
    Type::Family: StructView,
    T: Table + 'borrow,
    'builder: 'borrow,
{
    type Reader = ReaderOf<'borrow, Type::Family, T>;

    #[inline]
    fn as_reader(self) -> Self::Reader {
        self.as_ref().as_reader().into()
    }
}

/// A trait used to describe infallible conversions to different pointer types.
pub trait FromPtr<T> {
    /// The type returned by the conversion.
    type Output;

    /// Converts the pointer into the output type.
    fn get(ptr: T) -> Self::Output;
}

/// Inherits all kind implementations. This way we can 
impl<T, U> FromPtr<U> for T
where
    T: TypeKind,
    T::Kind: FromPtr<U>,
{
    type Output = <T::Kind as FromPtr<U>>::Output;

    fn get(ptr: U) -> Self::Output {
        <T::Kind as FromPtr<U>>::get(ptr)
    }
}

/// A trait used to describe types which can be read from other pointer types (fallible).
pub trait ReadPtr<T>: FromPtr<T> {
    fn try_get_option(ptr: T) -> Result<Option<Self::Output>>;
    fn try_get(ptr: T) -> Result<Self::Output>;
    fn get_option(ptr: T) -> Option<Self::Output>;
}

impl<'a, S: Struct, T: Table> FromPtr<any::StructReader<'a, T>> for kind::Struct<S> {
    type Output = ReaderOf<'a, S, T>;

    fn get(ptr: any::StructReader<'a, T>) -> Self::Output {
        ptr.read_as::<S>()
    }
}

impl<'a, S: Struct, T: Table> FromPtr<any::PtrReader<'a, T>> for kind::Struct<S> {
    type Output = ReaderOf<'a, S, T>;

    fn get(reader: any::PtrReader<'a, T>) -> Self::Output {
        match reader.as_ref().to_struct() {
            Ok(Some(ptr)) => ptr,
            _ => reader.imbue_into(ptr::StructReader::empty()),
        }.into()
    }
}

impl<'a, S: Struct, T: Table> ReadPtr<any::PtrReader<'a, T>> for kind::Struct<S> {
    fn try_get_option(reader: any::PtrReader<'a, T>) -> Result<Option<Self::Output>> {
        match reader.as_ref().to_struct() {
            Ok(Some(ptr)) => Ok(Some(ptr.into())),
            Ok(None) => Ok(None),
            Err(err) => Err(err),
        }
    }
    fn try_get(reader: any::PtrReader<'a, T>) -> Result<Self::Output> {
        Ok(match reader.as_ref().to_struct() {
            Ok(Some(ptr)) => ptr.into(),
            Ok(None) => reader.imbue_into(ptr::StructReader::empty()).into(),
            Err(err) => return Err(err),
        })
    }
    fn get_option(reader: any::PtrReader<'a, T>) -> Option<Self::Output> {
        reader.as_ref().to_struct().ok().flatten().map(Into::into)
    }
}

/// A type representing a fixed size Cap'n Proto value that can be stored in a list.
///
/// Note not every type that be stored in a list will implement this trait. Dynamically
/// sized types like AnyStruct can be stored in a list, but is implemented specially.
pub trait ListValue: DynListValue {
    /// The size of elements of this value when stored in a list.
    const ELEMENT_SIZE: ElementSize;
}
impl<T> ListValue for T
where
    T: list::TypeKind + 'static,
    T::Kind: ListValue,
{
    const ELEMENT_SIZE: ElementSize = <T::Kind as ListValue>::ELEMENT_SIZE;
}

impl<D: ptr::Data> ListValue for kind::Data<D> {
    const ELEMENT_SIZE: ElementSize = <D as ptr::Data>::ELEMENT_SIZE;
}

pub trait DynListValue: 'static {
    const PTR_ELEMENT_SIZE: ptr::PtrElementSize;
}
impl<T: ListValue> DynListValue for T {
    const PTR_ELEMENT_SIZE: ptr::PtrElementSize = T::ELEMENT_SIZE.as_ptr_size();
}

pub type Void = ();

pub type Bool = bool;

pub type Int8 = i8;
pub type Int16 = i16;
pub type Int32 = i32;
pub type Int64 = i64;

pub type UInt8 = u8;
pub type UInt16 = u16;
pub type UInt32 = u32;
pub type UInt64 = u64;

pub type Float32 = f32;
pub type Float64 = f64;

/// A generic "type kind" constraining trait.
/// 
/// Type kinds allow us to write generic trait implementations over non-overlapping types. This
/// pattern of implementing over type kinds is used heavily in this library and primarily starts
/// here.
/// 
/// As you might know, you cannot have multiple implementation blocks with overlapping definitions.
/// This generally means that you can't have more than one generic implementation over a generic
/// type. In this library, we need that! Most of the library is generic in some way.
pub trait TypeKind {
    /// The kind this type is.
    type Kind;
}

/// Contains "kind" marker types used for writting blanket implementations over non-overlapping
/// types.
pub mod kind {
    use core::marker::PhantomData;

    use crate::field;
    use crate::list;
    use crate::ptr;
    use crate::ty;

    type Fantom<T> = PhantomData<fn() -> T>;

    /// A type kind for primitive data types like uint8, int32, f64, and bool. See [`ptr::Data`].
    pub struct Data<D: ?Sized + ptr::Data>(Fantom<D>);
    /// A type kind for enum types. See [`ty::Enum`].
    pub struct Enum<E: ?Sized + ty::Enum>(Fantom<E>);
    /// A type kind for capability types. See [`ty::Capability`].
    pub struct Capability<C: ?Sized + ty::Capability>(Fantom<C>);
    /// A type kind for user struct types. See [`ty::Struct`].
    pub struct Struct<S: ?Sized + ty::Struct>(Fantom<S>);
    /// A type kind for user group types. See [`field::Group`].
    pub struct Group<G: ?Sized + field::Group>(Fantom<G>);
    /// A type kind for pointer field types. This is not the same as pointer list types which use
    /// [`PtrList`] instead. Most notably, [`Struct`] kinds are not considered [`PtrList`] kinds,
    /// but are considered [`PtrField`] kinds.
    pub struct PtrField<P: ?Sized + field::Ptr>(Fantom<P>);
    /// A type kind for pointer list types. This is not the same as pointer field types which use
    /// [`PtrField`] instead. Most notably, [`Struct`] kinds are considered [`PtrField`] kinds,
    /// but are not considered [`PtrList`] kinds.
    pub struct PtrList<P: ?Sized + list::Ptr>(Fantom<P>);
}