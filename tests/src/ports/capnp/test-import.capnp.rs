#![allow(unused, unsafe_code)]
use super::{__file, __imports};
pub struct TestImport {
    pub field: Option<Box<__imports::capnp_test_capnp::TestAllTypes>>,
}
impl ::recapn::ty::SchemaType for TestImport {
    const ID: u64 = 13570947164928695703u64;
}
impl ::recapn_port::Deserialize<'_, ::recapn::rpc::Empty> for TestImport {
    fn deserialize<E: ::recapn_port::Error>(
        r: &::recapn::ptr::StructReader<'_, ::recapn::rpc::Empty>,
    ) -> Result<Self, E> {
        Ok(Self {
            field: ::recapn_port::deserialize(&r.ptr_field(0))?,
        })
    }
}
impl ::recapn_port::DeserializePtr<'_, ::recapn::rpc::Empty> for TestImport {
    #[inline]
    fn deserialize<E: ::recapn_port::Error>(
        r: &::recapn::ptr::PtrReader<'_, ::recapn::rpc::Empty>,
    ) -> Result<Self, E> {
        ::recapn_port::deserialize_struct(r)
    }
}
impl ::recapn_port::list::DeserializeList<'_, ::recapn::rpc::Empty> for TestImport {
    #[inline]
    fn deserialize<E: ::recapn_port::Error>(
        r: &::recapn::ptr::PtrReader<'_, ::recapn::rpc::Empty>,
    ) -> Result<::recapn_port::list::List<Self>, E> {
        ::recapn_port::list::deserialize_struct_list(r)
    }
}
impl ::recapn_port::Serialize<'_, ::recapn::rpc::Empty> for TestImport {
    const SIZE: ::recapn::ptr::StructSize = ::recapn::ptr::StructSize {
        data: 0,
        ptrs: 1,
    };
    fn serialize<E: ::recapn_port::Error>(
        &self,
        b: &mut ::recapn::ptr::StructBuilder<'_, ::recapn::rpc::Empty>,
    ) -> Result<(), E> {
        if let Some(mut ptr) = b.ptr_field_mut(0) {
            ::recapn_port::serialize(&self.field, ptr)?;
        }
        Ok(())
    }
}
impl ::recapn_port::SerializePtr<'_, ::recapn::rpc::Empty> for TestImport {
    #[inline]
    fn serialize<E: ::recapn_port::Error>(
        &self,
        b: ::recapn::ptr::PtrBuilder<'_, ::recapn::rpc::Empty>,
    ) -> Result<(), E> {
        ::recapn_port::serialize_struct(self, b)
    }
}
impl ::recapn_port::list::SerializeList<'_, ::recapn::rpc::Empty> for TestImport {
    #[inline]
    fn serialize<E: ::recapn_port::Error>(
        s: &[Self],
        b: recapn::ptr::PtrBuilder<'_, ::recapn::rpc::Empty>,
    ) -> Result<(), E> {
        ::recapn_port::list::serialize_struct_list(s, b)
    }
}