//! Types and traits implemented by foreign generated user types like enums and structs.

use crate::internal::Sealed;
use crate::list::{self, List};
use crate::ptr::{self, StructSize};
use crate::ptr::{CapableBuilder, CapableReader};
use crate::rpc::{self, Table};
use crate::{IntoFamily, Result};

/// An enum marker trait. Enums can be converted to and from shorts (u16), no matter if their value
/// is contained within the schema or not.
pub trait Enum: Clone + Copy + From<u16> + Into<u16> + 'static {}

pub trait Struct: 'static {
    const SIZE: StructSize;

    type Reader<'a, T: Table>: StructReader<Ptr = ptr::StructReader<'a, T>, Table = T>;
    type Builder<'a, T: Table>: StructBuilder<Ptr = ptr::StructBuilder<'a, T>, Table = T>;
}

pub trait StructReader: CapableReader + IntoFamily + Clone {
    type Ptr;

    fn from_ptr(ptr: Self::Ptr) -> Self;
    fn as_ptr(&self) -> &Self::Ptr;
    fn into_ptr(self) -> Self::Ptr;
}

pub trait StructBuilder: CapableBuilder + IntoFamily {
    type Ptr;

    fn from_ptr(ptr: Self::Ptr) -> Self;
    fn as_ptr(&self) -> &Self::Ptr;
    fn as_mut_ptr(&mut self) -> &mut Self::Ptr;
    fn into_ptr(self) -> Self::Ptr;
}

/// A trait used to specify that a type is used to represent a Cap'n Proto value.
pub trait Value: Sealed + 'static {
    type Default;
}

/// A type representing a fixed size Cap'n Proto value that can be stored in a list.
///
/// Note not every type that be stored in a list will implement this trait. Dynamically
/// sized types like AnyStruct can be stored in a list, but is implemented specially.
pub trait ListValue: Value {
    /// The size of elements of this value when stored in a list.
    const ELEMENT_SIZE: list::ElementSize;
}

impl Sealed for () {}
impl Value for () {
    type Default = ();
}
impl ListValue for () {
    const ELEMENT_SIZE: list::ElementSize = list::ElementSize::Void;
}

impl<V: Value> Value for List<V> {
    type Default = list::ptr::Reader<'static, rpc::Empty>;
}
impl<V: Value> ListValue for List<V> {
    const ELEMENT_SIZE: list::ElementSize = list::ElementSize::Pointer;
}
