use crate::internal::Sealed;
use crate::ptr::{Capable, CapableBuilder, CapableReader};
use crate::rpc::{self, Empty, PipelineBuilder, Pipelined, Table};
use crate::{list, ty, IntoFamily};
use crate::{Family, Result};

pub mod ptr {
    pub use crate::ptr::{
        ListBuilder, ListReader, PtrBuilder, PtrReader, StructBuilder, StructReader,
    };
}

pub use crate::ptr::PtrType;

/// A safe wrapper around any pointer type.
///
/// You probably don't want to name this type this directly. Use one of the ptr
/// type aliases instead (e.g. [`any::PtrReader`](PtrReader)).
#[derive(Clone, Debug)]
pub struct AnyPtr<T = Family> {
    pub(crate) repr: T,
}

/// A safe reader for a pointer of any type
pub type PtrReader<'a, T = Empty> = AnyPtr<ptr::PtrReader<'a, T>>;
/// A safe builder for a pointer of any type
pub type PtrBuilder<'a, T = Empty> = AnyPtr<ptr::PtrBuilder<'a, T>>;
/// A safe pipeline builder for a pointer of any type
pub type PtrPipeline<P> = AnyPtr<rpc::Pipeline<P>>;

impl<T: Capable> Capable for AnyPtr<T> {
    type Table = T::Table;
    type Imbued<T2: Table> = AnyPtr<T::Imbued<T2>>;
}

impl<T: CapableReader> CapableReader for AnyPtr<T> {
    fn imbue_release<T2: Table>(
        self,
        new_table: T2::Reader,
    ) -> (Self::Imbued<T2>, <Self::Table as Table>::Reader) {
        let (new_ptr, old_table) = self.repr.imbue_release(new_table);
        (AnyPtr { repr: new_ptr }, old_table)
    }
}

impl<T: CapableBuilder> CapableBuilder for AnyPtr<T> {
    fn imbue_release<T2: Table>(
        self,
        new_table: T2::Builder,
    ) -> (Self::Imbued<T2>, <Self::Table as Table>::Builder) {
        let (new_ptr, old_table) = self.repr.imbue_release(new_table);
        (AnyPtr { repr: new_ptr }, old_table)
    }
}

impl Sealed for AnyPtr {}
impl ty::Value for AnyPtr {
    type Default = PtrReader<'static, Empty>;
}
impl ty::ListValue for AnyPtr {
    const ELEMENT_SIZE: list::ElementSize = list::ElementSize::Pointer;
}

impl<'a, T: Table> PtrReader<'a, T> {
    #[inline]
    pub fn target_size(&self) {
        todo!()
    }

    #[inline]
    pub fn ptr_type(&self) -> Result<PtrType> {
        self.repr.ptr_type()
    }

    #[inline]
    pub fn is_null(&self) -> bool {
        matches!(self.ptr_type(), Ok(PtrType::Null))
    }

    #[inline]
    pub fn is_struct(&self) -> bool {
        matches!(self.ptr_type(), Ok(PtrType::Struct))
    }

    #[inline]
    pub fn is_list(&self) -> bool {
        matches!(self.ptr_type(), Ok(PtrType::List))
    }

    #[inline]
    pub fn is_capability(&self) -> bool {
        matches!(self.ptr_type(), Ok(PtrType::Capability))
    }
}

impl<'a> Default for PtrReader<'a, Empty> {
    fn default() -> Self {
        AnyPtr::from(ptr::PtrReader::null())
    }
}

impl<'a, T: Table> From<ptr::PtrReader<'a, T>> for PtrReader<'a, T> {
    fn from(reader: ptr::PtrReader<'a, T>) -> Self {
        Self { repr: reader }
    }
}

impl<'a, T: Table> PtrBuilder<'a, T> {
    pub fn as_reader(&self) -> PtrReader<T> {
        AnyPtr {
            repr: self.repr.as_reader(),
        }
    }

    pub fn by_ref(&mut self) -> PtrBuilder<T> {
        PtrBuilder { repr: self.repr.by_ref() }
    }

    #[inline]
    pub fn target_size(&self) {
        todo!()
    }

    #[inline]
    pub fn ptr_type(&self) -> Result<PtrType> {
        todo!()
    }

    #[inline]
    pub fn is_null(&self) -> bool {
        matches!(self.ptr_type(), Ok(PtrType::Null))
    }

    #[inline]
    pub fn is_struct(&self) -> bool {
        matches!(self.ptr_type(), Ok(PtrType::Struct))
    }

    #[inline]
    pub fn is_list(&self) -> bool {
        matches!(self.ptr_type(), Ok(PtrType::List))
    }

    #[inline]
    pub fn is_capability(&self) -> bool {
        matches!(self.ptr_type(), Ok(PtrType::Capability))
    }

    #[inline]
    pub fn clear(&mut self) {
        self.repr.clear()
    }
}

impl<'a, T: Table> From<ptr::PtrBuilder<'a, T>> for PtrBuilder<'a, T> {
    fn from(reader: ptr::PtrBuilder<'a, T>) -> Self {
        Self { repr: reader }
    }
}

impl<'a, T: Table> From<PtrBuilder<'a, T>> for ptr::PtrBuilder<'a, T> {
    fn from(value: PtrBuilder<'a, T>) -> Self {
        value.repr
    }
}

impl<'a, 'b, T: Table> From<&'b PtrBuilder<'a, T>> for PtrReader<'b, T> {
    fn from(builder: &'b PtrBuilder<'a, T>) -> Self {
        Self {
            repr: builder.repr.as_reader(),
        }
    }
}

impl<P: Pipelined + PipelineBuilder<AnyPtr>> PtrPipeline<P> {
    pub fn new(pipeline: rpc::Pipeline<P>) -> Self {
        Self { repr: pipeline }
    }
    pub fn push(self, op: P::Operation) -> Self {
        Self {
            repr: self.repr.push(op),
        }
    }
    pub fn into_cap(self) -> P::Cap {
        self.repr.into_cap()
    }
    pub fn to_cap(self) -> P::Cap
    where
        P: Clone,
    {
        self.repr.to_cap()
    }
}

#[derive(Clone, Debug)]
pub struct AnyStruct<T = Family> {
    pub(crate) repr: T,
}

pub type StructReader<'a, T = Empty> = AnyStruct<ptr::StructReader<'a, T>>;
pub type StructBuilder<'a, T = Empty> = AnyStruct<ptr::StructBuilder<'a, T>>;
pub type StructPipeline<P> = AnyStruct<rpc::Pipeline<P>>;

impl<T> Sealed for AnyStruct<T> {}
impl ty::Value for AnyStruct {
    type Default = StructReader<'static, Empty>;
}
// Note: AnyStruct does not implement ty::ListValue since the size the user wants for the structs
// is unknown at compile time

impl<T> IntoFamily for AnyStruct<T> {
    type Family = AnyStruct;
}

impl<T: Capable> Capable for AnyStruct<T> {
    type Table = T::Table;
    type Imbued<T2: Table> = AnyStruct<T::Imbued<T2>>;
}

impl<T: CapableReader> CapableReader for AnyStruct<T> {
    fn imbue_release<T2: Table>(
        self,
        new_table: T2::Reader,
    ) -> (Self::Imbued<T2>, <Self::Table as Table>::Reader) {
        let (new_ptr, old_table) = self.repr.imbue_release(new_table);
        (AnyStruct { repr: new_ptr }, old_table)
    }
}

impl<T: CapableBuilder> CapableBuilder for AnyStruct<T> {
    fn imbue_release<T2: Table>(
        self,
        new_table: T2::Builder,
    ) -> (Self::Imbued<T2>, <Self::Table as Table>::Builder) {
        let (new_ptr, old_table) = self.repr.imbue_release(new_table);
        (AnyStruct { repr: new_ptr }, old_table)
    }
}

impl<'a, T: Table> ty::StructReader for StructReader<'a, T> {
    type Ptr = ptr::StructReader<'a, Self::Table>;

    fn from_ptr(ptr: ptr::StructReader<'a, Self::Table>) -> Self {
        Self { repr: ptr }
    }
    fn as_ptr(&self) -> &ptr::StructReader<'a, Self::Table> {
        &self.repr
    }
    fn into_ptr(self) -> crate::ptr::StructReader<'a, Self::Table> {
        self.repr
    }
}

impl<P: Pipelined + PipelineBuilder<AnyStruct>> StructPipeline<P> {
    pub fn new(pipeline: rpc::Pipeline<P>) -> Self {
        Self { repr: pipeline }
    }
    pub fn push(self, op: P::Operation) -> Self {
        Self {
            repr: self.repr.push(op),
        }
    }
    pub fn into_cap(self) -> P::Cap {
        self.repr.into_cap()
    }
    pub fn to_cap(self) -> P::Cap
    where
        P: Clone,
    {
        self.repr.to_cap()
    }
}

#[derive(Clone, Debug)]
pub struct AnyList<T = Family> {
    pub(crate) repr: T,
}

impl<T: Capable> Capable for AnyList<T> {
    type Table = T::Table;
    type Imbued<T2: Table> = AnyList<T::Imbued<T2>>;
}

impl<T: CapableReader> CapableReader for AnyList<T> {
    fn imbue_release<T2: Table>(
        self,
        new_table: T2::Reader,
    ) -> (Self::Imbued<T2>, <Self::Table as Table>::Reader) {
        let (new_ptr, old_table) = self.repr.imbue_release(new_table);
        (AnyList { repr: new_ptr }, old_table)
    }
}

impl<T: CapableBuilder> CapableBuilder for AnyList<T> {
    fn imbue_release<T2: Table>(
        self,
        new_table: T2::Builder,
    ) -> (Self::Imbued<T2>, <Self::Table as Table>::Builder) {
        let (new_ptr, old_table) = self.repr.imbue_release(new_table);
        (AnyList { repr: new_ptr }, old_table)
    }
}

impl<T> Sealed for AnyList<T> {}

pub type ListReader<'a, T = Empty> = AnyList<ptr::ListReader<'a, T>>;
pub type ListBuilder<'a, T = Empty> = AnyList<ptr::ListBuilder<'a, T>>;

impl<'a, T: Table> ListReader<'a, T> {
    pub fn new(ptr: ptr::ListReader<'a, T>) -> Self {
        Self { repr: ptr }
    }
    pub fn into_inner(self) -> ptr::ListReader<'a, T> {
        self.repr
    }
}

impl<'a, T: Table> ListBuilder<'a, T> {
    pub fn new(ptr: ptr::ListBuilder<'a, T>) -> Self {
        Self { repr: ptr }
    }
    pub fn into_inner(self) -> ptr::ListBuilder<'a, T> {
        self.repr
    }
}

impl<'a, T: Table> AsRef<ptr::ListBuilder<'a, T>> for ListBuilder<'a, T> {
    fn as_ref(&self) -> &ptr::ListBuilder<'a, T> {
        &self.repr
    }
}

impl<'a, T: Table> AsMut<ptr::ListBuilder<'a, T>> for ListBuilder<'a, T> {
    fn as_mut(&mut self) -> &mut ptr::ListBuilder<'a, T> {
        &mut self.repr
    }
}