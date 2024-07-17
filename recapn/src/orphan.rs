use core::marker::PhantomData;

use crate::any::AnyStruct;
use crate::data::Data;
use crate::list::List;
use crate::ptr::{ElementCount, ElementSize, ObjectBuilder, OrphanBuilder, StructSize};
use crate::rpc::{self, Capable, Table};
use crate::text::{self, Text};
use crate::ty;
use crate::ty::kind::{Struct, Capability};
use crate::Result;

pub struct Orphanage<'b, T: Table = rpc::Empty> {
    builder: ObjectBuilder<'b>,
    table: T::Builder,
}

impl<'b, T: Table> Capable for Orphanage<'b, T> {
    type Table = T;

    type Imbued = T::Builder;
    type ImbuedWith<T2: Table> = Orphanage<'b, T2>;

    #[inline]
    fn imbued(&self) -> &Self::Imbued { &self.table }

    #[inline]
    fn imbue_release<T2: Table>(
        self,
        new_table: <Self::ImbuedWith<T2> as Capable>::Imbued,
    ) -> (Self::ImbuedWith<T2>, Self::Imbued) {
        let old_table = self.table;
        let orphanage = Orphanage { builder: self.builder, table: new_table };
        (orphanage, old_table)
    }

    #[inline]
    fn imbue_release_into<U: Capable>(&self, other: U) -> (U::ImbuedWith<T>, U::Imbued)
    where
        U: Capable,
        U::ImbuedWith<Self::Table>: Capable<Imbued = Self::Imbued>,
    {
        other.imbue_release::<T>(self.table.clone())
    }
}

impl<'b> Orphanage<'b, rpc::Empty> {
    #[inline]
    pub(crate) fn new(builder: ObjectBuilder<'b>) -> Self {
        Self { builder, table: rpc::Empty }
    }
}

impl<'a, T: Table> Orphanage<'a, T> {
    #[inline]
    pub(crate) fn builder(&self) -> &ObjectBuilder<'a> {
        &self.builder
    }

    #[inline]
    fn try_new_struct_orphan(&self, size: StructSize) -> Result<OrphanBuilder<'a, T>> {
        let (object, builder) = self.builder.alloc_struct_orphan(size)?;
        Ok(OrphanBuilder::new(builder, object, self.table.clone()))
    }

    #[inline]
    fn try_new_list_orphan(&self, size: ElementSize, count: ElementCount) -> Result<OrphanBuilder<'a, T>> {
        let (object, builder) = self.builder.alloc_list_orphan(size, count)?;
        Ok(OrphanBuilder::new(builder, object, self.table.clone()))
    }

    #[inline]
    pub fn try_new_struct<S: ty::Struct>(&self) -> Result<Orphan<'a, Struct<S>, T>> {
        self.try_new_struct_orphan(S::SIZE).map(Orphan::new)
    }

    #[inline]
    pub fn new_struct<S: ty::Struct>(&self) -> Orphan<'a, Struct<S>, T> {
        self.try_new_struct::<S>().expect("failed to allocate struct")
    }

    #[inline]
    pub fn try_new_any_struct(&self, size: StructSize) -> Result<Orphan<'a, AnyStruct, T>> {
        self.try_new_struct_orphan(size).map(Orphan::new)
    }

    #[inline]
    pub fn new_any_struct(&self, size: StructSize) -> Orphan<'a, AnyStruct, T> {
        self.try_new_any_struct(size).expect("failed to allocate struct")
    }

    #[inline]
    pub fn try_new_list_of<V: ty::ListValue>(
        &self,
        count: ElementCount,
    ) -> Result<Orphan<'a, List<V>, T>> {
        self.try_new_list_orphan(V::ELEMENT_SIZE, count).map(Orphan::new)
    }

    #[inline]
    pub fn new_list_of<V: ty::ListValue>(&self, count: ElementCount) -> Orphan<'a, List<V>, T> {
        self.try_new_list_of::<V>(count).expect("failed to allocate list")
    }

    #[inline]
    pub fn try_new_list_of_any_struct(
        &self,
        size: StructSize,
        count: ElementCount,
    ) -> Result<Orphan<'a, List<AnyStruct>, T>> {
        self.try_new_list_orphan(ElementSize::InlineComposite(size), count).map(Orphan::new)
    }

    #[inline]
    pub fn new_list_of_any_struct(
        &self,
        size: StructSize,
        count: ElementCount,
    ) -> Orphan<'a, List<AnyStruct>, T> {
        self.try_new_list_of_any_struct(size, count).expect("failed to allocate list")
    }

    #[inline]
    pub fn try_new_data(&self, size: ElementCount) -> Result<Orphan<'a, Data, T>> {
        self.try_new_list_orphan(ElementSize::Byte, size).map(Orphan::new)
    }

    #[inline]
    pub fn new_data(&self, size: ElementCount) -> Orphan<'a, Data, T> {
        self.try_new_data(size).expect("failed to allocate data blob")
    }

    #[inline]
    pub fn try_new_text(&self, size: text::ByteCount) -> Result<Orphan<'a, Text, T>> {
        self.try_new_list_orphan(ElementSize::Byte, size.into()).map(Orphan::new)
    }

    #[inline]
    pub fn new_text(&self, size: text::ByteCount) -> Orphan<'a, Text, T> {
        self.try_new_text(size).expect("failed to allocate text blob")
    }
}

pub struct Orphan<'a, T, Table: rpc::Table = rpc::Empty> {
    t: PhantomData<fn() -> T>,
    builder: OrphanBuilder<'a, Table>,
}

/// An alias for an orphan of a struct type.
pub type StructOrphan<'a, S, Table = rpc::Empty> = Orphan<'a, Struct<S>, Table>;
/// An alias for an orphan of a list type.
pub type ListOrphan<'a, V, Table = rpc::Empty> = Orphan<'a, List<V>, Table>;
/// An alias for an orphan of a capability type.
pub type CapabilityOrphan<'a, C, Table = rpc::Empty> = Orphan<'a, Capability<C>, Table>;

impl<'a, T, Table: rpc::Table> Capable for Orphan<'a, T, Table> {
    type Table = Table;

    type Imbued = Table::Builder;
    type ImbuedWith<Table2: rpc::Table> = Orphan<'a, T, Table2>;

    #[inline]
    fn imbued(&self) -> &Self::Imbued { self.builder.imbued() }

    #[inline]
    fn imbue_release<T2: rpc::Table>(
        self,
        new_table: T2::Builder,
    ) -> (Self::ImbuedWith<T2>, <Self::Table as rpc::Table>::Builder) {
        let (ptr, old_table) = self.builder.imbue_release(new_table);
        (Orphan { t: PhantomData, builder: ptr }, old_table)
    }

    #[inline]
    fn imbue_release_into<U: Capable>(&self, other: U) -> (U::ImbuedWith<Table>, U::Imbued)
    where
        U: Capable,
        U::ImbuedWith<Self::Table>: Capable<Imbued = Self::Imbued>,
    {
        other.imbue_release::<Table>(self.imbued().clone())
    }
}

impl<'a, T, Table: rpc::Table> Orphan<'a, T, Table> {
    #[inline]
    pub(crate) fn new(builder: OrphanBuilder<'a, Table>) -> Self {
        Self { t: PhantomData, builder }
    }

    #[inline]
    pub(crate) fn into_inner(self) -> OrphanBuilder<'a, Table> {
        self.builder
    }
}

impl<'a, S, Table> Orphan<'a, Struct<S>, Table>
where
    S: ty::Struct,
    Table: rpc::Table,
{

}

// TODO accessors