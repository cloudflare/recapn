#![allow(unused, unsafe_code)]
use recapn::prelude::generated as _p;
use super::{__file, __imports};
#[derive(Clone)]
pub struct TestImport2<T = _p::Family>(T);
impl _p::ty::SchemaType for TestImport2 {
    const ID: u64 = 17779498780914921727u64;
}
impl<T> _p::IntoFamily for TestImport2<T> {
    type Family = TestImport2;
}
impl<T: _p::Capable> _p::Capable for TestImport2<T> {
    type Table = T::Table;
    type Imbued = T::Imbued;
    type ImbuedWith<T2: _p::rpc::Table> = TestImport2<T::ImbuedWith<T2>>;
    #[inline]
    fn imbued(&self) -> &Self::Imbued {
        self.0.imbued()
    }
    #[inline]
    fn imbue_release<T2: _p::rpc::Table>(
        self,
        new_table: <Self::ImbuedWith<T2> as _p::Capable>::Imbued,
    ) -> (Self::ImbuedWith<T2>, Self::Imbued) {
        let (imbued, old) = self.0.imbue_release(new_table);
        (TestImport2(imbued), old)
    }
    #[inline]
    fn imbue_release_into<U>(&self, other: U) -> (U::ImbuedWith<Self::Table>, U::Imbued)
    where
        U: _p::Capable,
        U::ImbuedWith<Self::Table>: _p::Capable<Imbued = Self::Imbued>,
    {
        self.0.imbue_release_into(other)
    }
}
impl<'a, T: _p::rpc::Table> _p::ty::TypedPtr for test_import2::Reader<'a, T> {
    type Ptr = _p::StructReader<'a, T>;
}
impl<'a, T: _p::rpc::Table> core::convert::From<_p::StructReader<'a, T>>
for test_import2::Reader<'a, T> {
    #[inline]
    fn from(ptr: _p::StructReader<'a, T>) -> Self {
        TestImport2(ptr)
    }
}
impl<'a, T: _p::rpc::Table> core::convert::From<test_import2::Reader<'a, T>>
for _p::StructReader<'a, T> {
    #[inline]
    fn from(reader: test_import2::Reader<'a, T>) -> Self {
        reader.0
    }
}
impl<'a, T: _p::rpc::Table> core::convert::AsRef<_p::StructReader<'a, T>>
for test_import2::Reader<'a, T> {
    #[inline]
    fn as_ref(&self) -> &_p::StructReader<'a, T> {
        &self.0
    }
}
impl<'a, T: _p::rpc::Table> _p::ty::StructReader for test_import2::Reader<'a, T> {}
impl<'a, T: _p::rpc::Table> _p::ty::TypedPtr for test_import2::Builder<'a, T> {
    type Ptr = _p::StructBuilder<'a, T>;
}
impl<'a, T: _p::rpc::Table> core::convert::From<test_import2::Builder<'a, T>>
for _p::StructBuilder<'a, T> {
    #[inline]
    fn from(reader: test_import2::Builder<'a, T>) -> Self {
        reader.0
    }
}
impl<'a, T: _p::rpc::Table> core::convert::AsRef<_p::StructBuilder<'a, T>>
for test_import2::Builder<'a, T> {
    #[inline]
    fn as_ref(&self) -> &_p::StructBuilder<'a, T> {
        &self.0
    }
}
impl<'a, T: _p::rpc::Table> core::convert::AsMut<_p::StructBuilder<'a, T>>
for test_import2::Builder<'a, T> {
    #[inline]
    fn as_mut(&mut self) -> &mut _p::StructBuilder<'a, T> {
        &mut self.0
    }
}
impl<'a, T: _p::rpc::Table> _p::ty::StructBuilder for test_import2::Builder<'a, T> {
    unsafe fn from_ptr(ptr: Self::Ptr) -> Self {
        Self(ptr)
    }
}
impl _p::StructView for TestImport2 {
    type Reader<'a, T: _p::rpc::Table> = test_import2::Reader<'a, T>;
    type Builder<'a, T: _p::rpc::Table> = test_import2::Builder<'a, T>;
}
impl _p::ty::Struct for TestImport2 {
    const SIZE: _p::StructSize = _p::StructSize {
        data: 0u16,
        ptrs: 2u16,
    };
}
impl TestImport2 {
    const FOO: _p::Descriptor<_p::Struct<__imports::capnp_test_capnp::TestAllTypes>> = _p::Descriptor::<
        _p::Struct<__imports::capnp_test_capnp::TestAllTypes>,
    > {
        slot: 0u32,
        default: ::core::option::Option::None,
    };
    const BAR: _p::Descriptor<
        _p::Struct<__imports::capnp_test_import_capnp::TestImport>,
    > = _p::Descriptor::<_p::Struct<__imports::capnp_test_import_capnp::TestImport>> {
        slot: 1u32,
        default: ::core::option::Option::None,
    };
}
impl<'p, T: _p::rpc::Table + 'p> test_import2::Reader<'p, T> {
    #[inline]
    pub fn foo(
        &self,
    ) -> _p::Accessor<'_, 'p, T, _p::Struct<__imports::capnp_test_capnp::TestAllTypes>> {
        unsafe {
            <_p::Struct<
                __imports::capnp_test_capnp::TestAllTypes,
            > as _p::field::FieldType>::accessor(&self.0, &TestImport2::FOO)
        }
    }
    #[inline]
    pub fn bar(
        &self,
    ) -> _p::Accessor<
        '_,
        'p,
        T,
        _p::Struct<__imports::capnp_test_import_capnp::TestImport>,
    > {
        unsafe {
            <_p::Struct<
                __imports::capnp_test_import_capnp::TestImport,
            > as _p::field::FieldType>::accessor(&self.0, &TestImport2::BAR)
        }
    }
}
impl<'p, T: _p::rpc::Table + 'p> test_import2::Builder<'p, T> {
    #[inline]
    pub fn foo(
        &mut self,
    ) -> _p::AccessorMut<
        '_,
        'p,
        T,
        _p::Struct<__imports::capnp_test_capnp::TestAllTypes>,
    > {
        unsafe {
            <_p::Struct<
                __imports::capnp_test_capnp::TestAllTypes,
            > as _p::field::FieldType>::accessor(&mut self.0, &TestImport2::FOO)
        }
    }
    #[inline]
    pub fn bar(
        &mut self,
    ) -> _p::AccessorMut<
        '_,
        'p,
        T,
        _p::Struct<__imports::capnp_test_import_capnp::TestImport>,
    > {
        unsafe {
            <_p::Struct<
                __imports::capnp_test_import_capnp::TestImport,
            > as _p::field::FieldType>::accessor(&mut self.0, &TestImport2::BAR)
        }
    }
    #[inline]
    pub fn into_foo(
        self,
    ) -> _p::AccessorOwned<
        'p,
        T,
        _p::Struct<__imports::capnp_test_capnp::TestAllTypes>,
    > {
        unsafe {
            <_p::Struct<
                __imports::capnp_test_capnp::TestAllTypes,
            > as _p::field::FieldType>::accessor(self.0, &TestImport2::FOO)
        }
    }
    #[inline]
    pub fn into_bar(
        self,
    ) -> _p::AccessorOwned<
        'p,
        T,
        _p::Struct<__imports::capnp_test_import_capnp::TestImport>,
    > {
        unsafe {
            <_p::Struct<
                __imports::capnp_test_import_capnp::TestImport,
            > as _p::field::FieldType>::accessor(self.0, &TestImport2::BAR)
        }
    }
}
pub mod test_import2 {
    use super::{__file, __imports, _p};
    pub type Reader<'a, T = _p::rpc::Empty> = super::TestImport2<
        _p::StructReader<'a, T>,
    >;
    pub type Builder<'a, T = _p::rpc::Empty> = super::TestImport2<
        _p::StructBuilder<'a, T>,
    >;
}
