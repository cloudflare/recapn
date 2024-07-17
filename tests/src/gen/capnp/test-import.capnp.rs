#![allow(unused, unsafe_code)]
use recapn::prelude::gen as _p;
use super::{__file, __imports};
#[derive(Clone)]
pub struct TestImport<T = _p::Family>(T);
impl<T> _p::IntoFamily for TestImport<T> {
    type Family = TestImport;
}
impl<T: _p::Capable> _p::Capable for TestImport<T> {
    type Table = T::Table;
    type Imbued = T::Imbued;
    type ImbuedWith<T2: _p::rpc::Table> = TestImport<T::ImbuedWith<T2>>;
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
        (TestImport(imbued), old)
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
impl<'a, T: _p::rpc::Table> _p::ty::TypedPtr for test_import::Reader<'a, T> {
    type Ptr = _p::StructReader<'a, T>;
}
impl<'a, T: _p::rpc::Table> core::convert::From<_p::StructReader<'a, T>>
for test_import::Reader<'a, T> {
    #[inline]
    fn from(ptr: _p::StructReader<'a, T>) -> Self {
        TestImport(ptr)
    }
}
impl<'a, T: _p::rpc::Table> core::convert::From<test_import::Reader<'a, T>>
for _p::StructReader<'a, T> {
    #[inline]
    fn from(reader: test_import::Reader<'a, T>) -> Self {
        reader.0
    }
}
impl<'a, T: _p::rpc::Table> core::convert::AsRef<_p::StructReader<'a, T>>
for test_import::Reader<'a, T> {
    #[inline]
    fn as_ref(&self) -> &_p::StructReader<'a, T> {
        &self.0
    }
}
impl<'a, T: _p::rpc::Table> _p::ty::StructReader for test_import::Reader<'a, T> {}
impl<'a, T: _p::rpc::Table> _p::ty::TypedPtr for test_import::Builder<'a, T> {
    type Ptr = _p::StructBuilder<'a, T>;
}
impl<'a, T: _p::rpc::Table> core::convert::From<test_import::Builder<'a, T>>
for _p::StructBuilder<'a, T> {
    #[inline]
    fn from(reader: test_import::Builder<'a, T>) -> Self {
        reader.0
    }
}
impl<'a, T: _p::rpc::Table> core::convert::AsRef<_p::StructBuilder<'a, T>>
for test_import::Builder<'a, T> {
    #[inline]
    fn as_ref(&self) -> &_p::StructBuilder<'a, T> {
        &self.0
    }
}
impl<'a, T: _p::rpc::Table> core::convert::AsMut<_p::StructBuilder<'a, T>>
for test_import::Builder<'a, T> {
    #[inline]
    fn as_mut(&mut self) -> &mut _p::StructBuilder<'a, T> {
        &mut self.0
    }
}
impl<'a, T: _p::rpc::Table> _p::ty::StructBuilder for test_import::Builder<'a, T> {
    unsafe fn from_ptr(ptr: Self::Ptr) -> Self {
        Self(ptr)
    }
}
impl _p::StructView for TestImport {
    type Reader<'a, T: _p::rpc::Table> = test_import::Reader<'a, T>;
    type Builder<'a, T: _p::rpc::Table> = test_import::Builder<'a, T>;
}
impl _p::ty::TypeKind for TestImport {
    type Kind = _p::ty::kind::Struct<Self>;
}
impl _p::ty::Struct for TestImport {
    const SIZE: _p::StructSize = _p::StructSize {
        data: 0u16,
        ptrs: 1u16,
    };
}
impl TestImport {
    const FIELD: _p::Descriptor<__imports::capnp_test_capnp::TestAllTypes> = _p::Descriptor::<
        __imports::capnp_test_capnp::TestAllTypes,
    > {
        slot: 0u32,
        default: None,
    };
}
impl<'p, T: _p::rpc::Table + 'p> test_import::Reader<'p, T> {
    #[inline]
    pub fn field(
        &self,
    ) -> _p::Accessor<'_, 'p, T, __imports::capnp_test_capnp::TestAllTypes> {
        unsafe {
            <__imports::capnp_test_capnp::TestAllTypes as _p::field::FieldType>::accessor(
                &self.0,
                &TestImport::FIELD,
            )
        }
    }
}
impl<'p, T: _p::rpc::Table + 'p> test_import::Builder<'p, T> {
    #[inline]
    pub fn field(
        &mut self,
    ) -> _p::AccessorMut<'_, 'p, T, __imports::capnp_test_capnp::TestAllTypes> {
        unsafe {
            <__imports::capnp_test_capnp::TestAllTypes as _p::field::FieldType>::accessor(
                &mut self.0,
                &TestImport::FIELD,
            )
        }
    }
    #[inline]
    pub fn into_field(
        self,
    ) -> _p::AccessorOwned<'p, T, __imports::capnp_test_capnp::TestAllTypes> {
        unsafe {
            <__imports::capnp_test_capnp::TestAllTypes as _p::field::FieldType>::accessor(
                self.0,
                &TestImport::FIELD,
            )
        }
    }
}
pub mod test_import {
    use super::{__file, _p};
    pub type Reader<'a, T = _p::rpc::Empty> = super::TestImport<_p::StructReader<'a, T>>;
    pub type Builder<'a, T = _p::rpc::Empty> = super::TestImport<
        _p::StructBuilder<'a, T>,
    >;
}
