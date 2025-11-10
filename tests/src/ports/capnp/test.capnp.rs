#![allow(unused, unsafe_code)]
use super::{__file, __imports};
#[repr(u16)]
#[derive(Clone, Copy, PartialEq, Eq, PartialOrd, Ord, Hash, Debug, Default)]
pub enum TestEnum {
    #[default]
    Foo,
    Bar,
    Baz,
    Qux,
    Quux,
    Corge,
    Grault,
    Garply,
}
impl ::recapn::ty::SchemaType for TestEnum {
    const ID: u64 = 11281115850894843091u64;
}
impl core::convert::TryFrom<u16> for TestEnum {
    type Error = ::recapn::NotInSchema;
    #[inline]
    fn try_from(value: u16) -> Result<Self, ::recapn::NotInSchema> {
        match value {
            0u16 => Ok(Self::Foo),
            1u16 => Ok(Self::Bar),
            2u16 => Ok(Self::Baz),
            3u16 => Ok(Self::Qux),
            4u16 => Ok(Self::Quux),
            5u16 => Ok(Self::Corge),
            6u16 => Ok(Self::Grault),
            7u16 => Ok(Self::Garply),
            value => Err(::recapn::NotInSchema(value)),
        }
    }
}
impl core::convert::From<TestEnum> for u16 {
    #[inline]
    fn from(value: TestEnum) -> Self {
        value as u16
    }
}
impl ::recapn::ty::Enum for TestEnum {}
pub struct TestAllTypes {
    pub void_field: (),
    pub bool_field: bool,
    pub int8_field: i8,
    pub int16_field: i16,
    pub int32_field: i32,
    pub int64_field: i64,
    pub u_int8_field: u8,
    pub u_int16_field: u16,
    pub u_int32_field: u32,
    pub u_int64_field: u64,
    pub float32_field: f32,
    pub float64_field: f64,
    pub text_field: Option<::recapn_port::text::Text>,
    pub data_field: Option<::recapn_port::data::Data>,
    pub struct_field: Option<Box<TestAllTypes>>,
    pub enum_field: ::recapn_port::Enum<TestEnum>,
    pub void_list: Option<::recapn_port::list::List<()>>,
    pub bool_list: Option<::recapn_port::list::List<bool>>,
    pub int8_list: Option<::recapn_port::list::List<i8>>,
    pub int16_list: Option<::recapn_port::list::List<i16>>,
    pub int32_list: Option<::recapn_port::list::List<i32>>,
    pub int64_list: Option<::recapn_port::list::List<i64>>,
    pub u_int8_list: Option<::recapn_port::list::List<u8>>,
    pub u_int16_list: Option<::recapn_port::list::List<u16>>,
    pub u_int32_list: Option<::recapn_port::list::List<u32>>,
    pub u_int64_list: Option<::recapn_port::list::List<u64>>,
    pub float32_list: Option<::recapn_port::list::List<f32>>,
    pub float64_list: Option<::recapn_port::list::List<f64>>,
    pub text_list: Option<::recapn_port::list::List<::recapn_port::text::Text>>,
    pub data_list: Option<::recapn_port::list::List<::recapn_port::data::Data>>,
    pub struct_list: Option<::recapn_port::list::List<TestAllTypes>>,
    pub enum_list: Option<::recapn_port::list::List<::recapn_port::Enum<TestEnum>>>,
}
impl Default for TestAllTypes {
    fn default() -> Self {
        Self {
            void_field: (),
            bool_field: false,
            int8_field: 0,
            int16_field: 0,
            int32_field: 0,
            int64_field: 0,
            u_int8_field: 0,
            u_int16_field: 0,
            u_int32_field: 0,
            u_int64_field: 0,
            float32_field: 0.,
            float64_field: 0.,
            text_field: None,
            data_field: None,
            struct_field: None,
            enum_field: ::recapn_port::Enum::from_value(0),
            void_list: None,
            bool_list: None,
            int8_list: None,
            int16_list: None,
            int32_list: None,
            int64_list: None,
            u_int8_list: None,
            u_int16_list: None,
            u_int32_list: None,
            u_int64_list: None,
            float32_list: None,
            float64_list: None,
            text_list: None,
            data_list: None,
            struct_list: None,
            enum_list: None,
        }
    }
}
impl ::recapn::ty::SchemaType for TestAllTypes {
    const ID: u64 = 11576770112468509693u64;
}
impl ::recapn_port::Deserialize<'_, ::recapn::rpc::Empty> for TestAllTypes {
    fn deserialize<E: ::recapn_port::Error>(
        r: &::recapn::ptr::StructReader<'_, ::recapn::rpc::Empty>,
    ) -> Result<Self, E> {
        Ok(Self {
            void_field: {},
            bool_field: { r.data_field_with_default(0, false) },
            int8_field: { r.data_field_with_default(1, 0) },
            int16_field: { r.data_field_with_default(1, 0) },
            int32_field: { r.data_field_with_default(1, 0) },
            int64_field: { r.data_field_with_default(1, 0) },
            u_int8_field: { r.data_field_with_default(16, 0) },
            u_int16_field: { r.data_field_with_default(9, 0) },
            u_int32_field: { r.data_field_with_default(5, 0) },
            u_int64_field: { r.data_field_with_default(3, 0) },
            float32_field: { r.data_field_with_default(8, 0.) },
            float64_field: { r.data_field_with_default(5, 0.) },
            text_field: { ::recapn_port::deserialize(&r.ptr_field(0))? },
            data_field: { ::recapn_port::deserialize(&r.ptr_field(1))? },
            struct_field: { ::recapn_port::deserialize(&r.ptr_field(2))? },
            enum_field: { ::recapn_port::Enum::from_value(r.data_field_with_default(18, 0)) },
            void_list: { ::recapn_port::deserialize(&r.ptr_field(3))? },
            bool_list: { ::recapn_port::deserialize(&r.ptr_field(4))? },
            int8_list: { ::recapn_port::deserialize(&r.ptr_field(5))? },
            int16_list: { ::recapn_port::deserialize(&r.ptr_field(6))? },
            int32_list: { ::recapn_port::deserialize(&r.ptr_field(7))? },
            int64_list: { ::recapn_port::deserialize(&r.ptr_field(8))? },
            u_int8_list: { ::recapn_port::deserialize(&r.ptr_field(9))? },
            u_int16_list: { ::recapn_port::deserialize(&r.ptr_field(10))? },
            u_int32_list: { ::recapn_port::deserialize(&r.ptr_field(11))? },
            u_int64_list: { ::recapn_port::deserialize(&r.ptr_field(12))? },
            float32_list: { ::recapn_port::deserialize(&r.ptr_field(13))? },
            float64_list: { ::recapn_port::deserialize(&r.ptr_field(14))? },
            text_list: { ::recapn_port::deserialize(&r.ptr_field(15))? },
            data_list: { ::recapn_port::deserialize(&r.ptr_field(16))? },
            struct_list: { ::recapn_port::deserialize(&r.ptr_field(17))? },
            enum_list: { ::recapn_port::deserialize(&r.ptr_field(18))? },
        })
    }
}
impl ::recapn_port::DeserializePtr<'_, ::recapn::rpc::Empty> for TestAllTypes {
    #[inline]
    fn deserialize<E: ::recapn_port::Error>(
        r: &::recapn::ptr::PtrReader<'_, ::recapn::rpc::Empty>,
    ) -> Result<Self, E> {
        ::recapn_port::deserialize_struct(r)
    }
}
impl ::recapn_port::list::DeserializeList<'_, ::recapn::rpc::Empty> for TestAllTypes {
    #[inline]
    fn deserialize<E: ::recapn_port::Error>(
        r: &::recapn::ptr::PtrReader<'_, ::recapn::rpc::Empty>,
    ) -> Result<::recapn_port::list::List<Self>, E> {
        ::recapn_port::list::deserialize_struct_list(r)
    }
}
impl ::recapn_port::Serialize<'_, ::recapn::rpc::Empty> for TestAllTypes {
    const SIZE: ::recapn::ptr::StructSize = ::recapn::ptr::StructSize {
        data: 6u16,
        ptrs: 20u16,
    };
    fn serialize<E: ::recapn_port::Error>(
        &self,
        b: &mut ::recapn::ptr::StructBuilder<'_, ::recapn::rpc::Empty>,
    ) -> Result<(), E> {
        b.set_field_with_default(0, self.bool_field, false);
        b.set_field_with_default(1, self.int8_field, 0);
        b.set_field_with_default(1, self.int16_field, 0);
        b.set_field_with_default(1, self.int32_field, 0);
        b.set_field_with_default(1, self.int64_field, 0);
        b.set_field_with_default(16, self.u_int8_field, 0);
        b.set_field_with_default(9, self.u_int16_field, 0);
        b.set_field_with_default(5, self.u_int32_field, 0);
        b.set_field_with_default(3, self.u_int64_field, 0);
        b.set_field_with_default(8, self.float32_field, 0.);
        b.set_field_with_default(5, self.float64_field, 0.);
        if let Some(ptr) = b.ptr_field_mut(0) {
            ::recapn_port::serialize(&self.text_field, ptr)?;
        }
        if let Some(ptr) = b.ptr_field_mut(1) {
            ::recapn_port::serialize(&self.data_field, ptr)?;
        }
        if let Some(ptr) = b.ptr_field_mut(2) {
            ::recapn_port::serialize(&self.struct_field, ptr)?;
        }
        b.set_field_with_default(18, self.enum_field.into_value(), 0);
        if let Some(ptr) = b.ptr_field_mut(3) {
            ::recapn_port::serialize(&self.void_list, ptr)?;
        }
        if let Some(ptr) = b.ptr_field_mut(4) {
            ::recapn_port::serialize(&self.bool_list, ptr)?;
        }
        if let Some(ptr) = b.ptr_field_mut(5) {
            ::recapn_port::serialize(&self.int8_list, ptr)?;
        }
        if let Some(ptr) = b.ptr_field_mut(6) {
            ::recapn_port::serialize(&self.int16_list, ptr)?;
        }
        if let Some(ptr) = b.ptr_field_mut(7) {
            ::recapn_port::serialize(&self.int32_list, ptr)?;
        }
        if let Some(ptr) = b.ptr_field_mut(8) {
            ::recapn_port::serialize(&self.int64_list, ptr)?;
        }
        if let Some(ptr) = b.ptr_field_mut(9) {
            ::recapn_port::serialize(&self.u_int8_list, ptr)?;
        }
        if let Some(ptr) = b.ptr_field_mut(10) {
            ::recapn_port::serialize(&self.u_int16_list, ptr)?;
        }
        if let Some(ptr) = b.ptr_field_mut(11) {
            ::recapn_port::serialize(&self.u_int32_list, ptr)?;
        }
        if let Some(ptr) = b.ptr_field_mut(12) {
            ::recapn_port::serialize(&self.u_int64_list, ptr)?;
        }
        if let Some(ptr) = b.ptr_field_mut(13) {
            ::recapn_port::serialize(&self.float32_list, ptr)?;
        }
        if let Some(ptr) = b.ptr_field_mut(14) {
            ::recapn_port::serialize(&self.float64_list, ptr)?;
        }
        if let Some(ptr) = b.ptr_field_mut(15) {
            ::recapn_port::serialize(&self.text_list, ptr)?;
        }
        if let Some(ptr) = b.ptr_field_mut(16) {
            ::recapn_port::serialize(&self.data_list, ptr)?;
        }
        if let Some(ptr) = b.ptr_field_mut(17) {
            ::recapn_port::serialize(&self.struct_list, ptr)?;
        }
        if let Some(ptr) = b.ptr_field_mut(18) {
            ::recapn_port::serialize(&self.enum_list, ptr)?;
        }
        Ok(())
    }
}
impl ::recapn_port::SerializePtr<'_, ::recapn::rpc::Empty> for TestAllTypes {
    #[inline]
    fn serialize<E: ::recapn_port::Error>(
        &self,
        b: ::recapn::ptr::PtrBuilder<'_, ::recapn::rpc::Empty>,
    ) -> Result<(), E> {
        ::recapn_port::serialize_struct(self, b)
    }
}
impl ::recapn_port::list::SerializeList<'_, ::recapn::rpc::Empty> for TestAllTypes {
    #[inline]
    fn serialize<E: ::recapn_port::Error>(
        this: &[Self],
        b: ::recapn::ptr::PtrBuilder<'_, ::recapn::rpc::Empty>,
    ) -> Result<(), E> {
        ::recapn_port::list::serialize_struct_list(this, b)
    }
}