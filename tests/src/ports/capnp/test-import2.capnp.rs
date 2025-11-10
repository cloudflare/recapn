#![allow(unused, unsafe_code)]
use super::{__file, __imports};
pub struct TestImport2 {
    pub foo: Option<Box<__imports::capnp_test_capnp::TestAllTypes>>,
    pub bar: Option<Box<__imports::capnp_test_import_capnp::TestImport>>,
}
impl ::recapn::ty::SchemaType for TestImport2 {
    const ID: u64 = 17779498780914921727u64;
}
impl ::recapn_port::Deserialize<'_, ::recapn::rpc::Empty> for TestImport2 {
    fn deserialize<E: ::recapn_port::Error>(
        r: &::recapn::ptr::StructReader<'_, ::recapn::rpc::Empty>,
    ) -> Result<Self, E> {
        Ok(Self {
            foo: ::recapn_port::deserialize(&r.ptr_field(0))?,
            bar: ::recapn_port::deserialize(&r.ptr_field(1))?,
        })
    }
}
impl ::recapn_port::Serialize<'_, ::recapn::rpc::Empty> for TestImport2 {
    const SIZE: ::recapn::ptr::StructSize = ::recapn::ptr::StructSize {
        data: 0u16,
        ptrs: 2u16,
    };
    fn serialize<E: ::recapn_port::Error>(
        &self,
        b: &mut ::recapn::ptr::StructBuilder<'_, ::recapn::rpc::Empty>,
    ) -> Result<(), E> {
        if let Some(mut ptr) = b.ptr_field_mut(0) {
            ::recapn_port::serialize(&self.foo, ptr)?;
        }
        if let Some(mut ptr) = b.ptr_field_mut(1) {
            ::recapn_port::serialize(&self.bar, ptr)?;
        }
        Ok(())
    }
}