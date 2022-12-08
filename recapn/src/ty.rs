//! Types and traits implemented by foreign generated user types like enums and structs.

use crate::any;
use crate::field::FieldGroup;
use crate::internal::Sealed;
use crate::list::{self, List};
use crate::ptr::{self, internal::FieldData, StructSize};
use crate::ptr::{CapableBuilder, CapableReader};
use crate::rpc::{self, Table};
use crate::Result;
use core::marker::PhantomData;

/// An enum marker trait. Enums can be converted to and from shorts (u16), no matter if their value
/// is contained within the schema or not.
pub trait Enum: Clone + Copy + From<u16> + Into<u16> + 'static {}

pub trait Struct: 'static {
    const SIZE: StructSize;

    type Reader<'a, T: Table>: StructReader<'a, Table = T>;
    type Builder<'a, T: Table>: StructBuilder<'a, Table = T>;
}

pub trait StructReader<'a>: CapableReader {
    fn from_ptr(ptr: ptr::StructReader<'a, Self::Table>) -> Self;
}

pub trait StructBuilder<'a>: CapableBuilder {
    fn from_ptr(ptr: ptr::StructBuilder<'a, Self::Table>) -> Self;
}

/// A trait used to specify that a type is used to represent a Cap'n Proto value.
pub trait Value: Sealed + 'static {
    type Default;
}

/// A type representing a Cap'n Proto value that can be stored in a list.
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

impl<V: FieldData> Value for V {
    type Default = Self;
}
impl<V: FieldData> ListValue for V {
    const ELEMENT_SIZE: list::ElementSize = <V as FieldData>::ELEMENT_SIZE;
}

impl<V: Value> Value for List<V> {
    type Default = list::ptr::Reader<'static, rpc::Empty>;
}
impl<V: Value> ListValue for List<V> {
    const ELEMENT_SIZE: list::ElementSize = list::ElementSize::Pointer;
}
