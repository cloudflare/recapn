//! Types and traits implemented by foreign generated user types like enums and structs.

use core::convert::TryFrom;

use crate::any;
use crate::field;
use crate::internal::Sealed;
use crate::list::{self, ElementSize, List};
use crate::ptr::{self, MessageSize, StructSize};
use crate::rpc::{Capable, Table};
use crate::ReaderOf;
use crate::{IntoFamily, NotInSchema, Result};

/// An enum marker trait.
pub trait Enum: Copy + TryFrom<u16, Error = NotInSchema> + Into<u16> + Default + 'static {}

pub type EnumResult<E> = Result<E, NotInSchema>;

/// A capability marker trait.
pub trait Capability: 'static {
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
    type Reader<'a, T: Table>: StructReader<Ptr = ptr::StructReader<'a, T>>
        + IntoFamily<Family = Self>;
    type Builder<'a, T: Table>: StructBuilder<Ptr = ptr::StructBuilder<'a, T>>
        + IntoFamily<Family = Self>;
}

/// A marker that indicates that a type represents a struct.
///
/// This provides the struct's size, along with the reader and builder associated types.
pub trait Struct: StructView {
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

/// A trait used to describe types which can be read from other pointer types (fallible).
pub trait ReadPtr<T>: FromPtr<T> {
    fn try_get_option(ptr: T) -> Result<Option<Self::Output>>;
    fn try_get(ptr: T) -> Result<Self::Output>;
    fn get_option(ptr: T) -> Option<Self::Output>;
}

impl<'a, S: Struct, T: Table> FromPtr<any::StructReader<'a, T>> for field::Struct<S> {
    type Output = ReaderOf<'a, S, T>;

    fn get(ptr: any::StructReader<'a, T>) -> Self::Output {
        ptr.read_as::<S>()
    }
}

impl<'a, S: Struct, T: Table> FromPtr<any::PtrReader<'a, T>> for field::Struct<S> {
    type Output = ReaderOf<'a, S, T>;

    fn get(reader: any::PtrReader<'a, T>) -> Self::Output {
        match reader.as_ref().to_struct() {
            Ok(Some(ptr)) => ptr,
            _ => reader.imbue_into(ptr::StructReader::empty()),
        }
        .into()
    }
}

impl<'a, S: Struct, T: Table> ReadPtr<any::PtrReader<'a, T>> for field::Struct<S> {
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

macro_rules! impl_value {
    ($ty:ty, $size:ident) => {
        impl Sealed for $ty {}
        impl field::Value for $ty {
            type Default = Self;
        }
        impl ListValue for $ty {
            const ELEMENT_SIZE: ElementSize = ElementSize::$size;
        }
    };
}

impl_value!(Void, Void);
impl_value!(Bool, Bit);
impl_value!(UInt8, Byte);
impl_value!(Int8, Byte);
impl_value!(UInt16, TwoBytes);
impl_value!(Int16, TwoBytes);
impl_value!(UInt32, FourBytes);
impl_value!(Int32, FourBytes);
impl_value!(UInt64, EightBytes);
impl_value!(Int64, EightBytes);
impl_value!(Float32, FourBytes);
impl_value!(Float64, EightBytes);

impl<V: 'static> ListValue for List<V> {
    const ELEMENT_SIZE: list::ElementSize = list::ElementSize::Pointer;
}
