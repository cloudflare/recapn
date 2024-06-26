#![allow(unused, unsafe_code)]
use recapn::prelude::gen as _p;
use super::{__file, __imports};
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
impl core::convert::TryFrom<u16> for TestEnum {
    type Error = _p::NotInSchema;
    #[inline]
    fn try_from(value: u16) -> Result<Self, Self::Error> {
        match value {
            0u16 => Ok(Self::Foo),
            1u16 => Ok(Self::Bar),
            2u16 => Ok(Self::Baz),
            3u16 => Ok(Self::Qux),
            4u16 => Ok(Self::Quux),
            5u16 => Ok(Self::Corge),
            6u16 => Ok(Self::Grault),
            7u16 => Ok(Self::Garply),
            value => Err(_p::NotInSchema(value)),
        }
    }
}
impl core::convert::From<TestEnum> for u16 {
    #[inline]
    fn from(value: TestEnum) -> Self {
        value as u16
    }
}
impl _p::ty::Enum for TestEnum {}
#[derive(Clone)]
pub struct TestAllTypes<T = _p::Family>(T);
impl<T> _p::IntoFamily for TestAllTypes<T> {
    type Family = TestAllTypes;
}
impl<T: _p::Capable> _p::Capable for TestAllTypes<T> {
    type Table = T::Table;
    type Imbued = T::Imbued;
    type ImbuedWith<T2: _p::rpc::Table> = TestAllTypes<T::ImbuedWith<T2>>;
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
        (TestAllTypes(imbued), old)
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
impl<'a, T: _p::rpc::Table> _p::ty::TypedPtr for test_all_types::Reader<'a, T> {
    type Ptr = _p::StructReader<'a, T>;
}
impl<'a, T: _p::rpc::Table> core::convert::From<_p::StructReader<'a, T>>
for test_all_types::Reader<'a, T> {
    #[inline]
    fn from(ptr: _p::StructReader<'a, T>) -> Self {
        TestAllTypes(ptr)
    }
}
impl<'a, T: _p::rpc::Table> core::convert::From<test_all_types::Reader<'a, T>>
for _p::StructReader<'a, T> {
    #[inline]
    fn from(reader: test_all_types::Reader<'a, T>) -> Self {
        reader.0
    }
}
impl<'a, T: _p::rpc::Table> core::convert::AsRef<_p::StructReader<'a, T>>
for test_all_types::Reader<'a, T> {
    #[inline]
    fn as_ref(&self) -> &_p::StructReader<'a, T> {
        &self.0
    }
}
impl<'a, T: _p::rpc::Table> _p::ty::StructReader for test_all_types::Reader<'a, T> {}
impl<'a, T: _p::rpc::Table> _p::ty::TypedPtr for test_all_types::Builder<'a, T> {
    type Ptr = _p::StructBuilder<'a, T>;
}
impl<'a, T: _p::rpc::Table> core::convert::From<test_all_types::Builder<'a, T>>
for _p::StructBuilder<'a, T> {
    #[inline]
    fn from(reader: test_all_types::Builder<'a, T>) -> Self {
        reader.0
    }
}
impl<'a, T: _p::rpc::Table> core::convert::AsRef<_p::StructBuilder<'a, T>>
for test_all_types::Builder<'a, T> {
    #[inline]
    fn as_ref(&self) -> &_p::StructBuilder<'a, T> {
        &self.0
    }
}
impl<'a, T: _p::rpc::Table> core::convert::AsMut<_p::StructBuilder<'a, T>>
for test_all_types::Builder<'a, T> {
    #[inline]
    fn as_mut(&mut self) -> &mut _p::StructBuilder<'a, T> {
        &mut self.0
    }
}
impl<'a, T: _p::rpc::Table> _p::ty::StructBuilder for test_all_types::Builder<'a, T> {
    unsafe fn from_ptr(ptr: Self::Ptr) -> Self {
        Self(ptr)
    }
}
impl _p::StructView for TestAllTypes {
    type Reader<'a, T: _p::rpc::Table> = test_all_types::Reader<'a, T>;
    type Builder<'a, T: _p::rpc::Table> = test_all_types::Builder<'a, T>;
}
impl _p::ty::Struct for TestAllTypes {
    const SIZE: _p::StructSize = _p::StructSize {
        data: 6u16,
        ptrs: 20u16,
    };
}
impl TestAllTypes {
    const VOID_FIELD: _p::Descriptor<()> = ();
    const BOOL_FIELD: _p::Descriptor<bool> = _p::Descriptor::<bool> {
        slot: 0u32,
        default: false,
    };
    const INT8_FIELD: _p::Descriptor<i8> = _p::Descriptor::<i8> {
        slot: 1u32,
        default: 0i8,
    };
    const INT16_FIELD: _p::Descriptor<i16> = _p::Descriptor::<i16> {
        slot: 1u32,
        default: 0i16,
    };
    const INT32_FIELD: _p::Descriptor<i32> = _p::Descriptor::<i32> {
        slot: 1u32,
        default: 0i32,
    };
    const INT64_FIELD: _p::Descriptor<i64> = _p::Descriptor::<i64> {
        slot: 1u32,
        default: 0i64,
    };
    const U_INT8_FIELD: _p::Descriptor<u8> = _p::Descriptor::<u8> {
        slot: 16u32,
        default: 0u8,
    };
    const U_INT16_FIELD: _p::Descriptor<u16> = _p::Descriptor::<u16> {
        slot: 9u32,
        default: 0u16,
    };
    const U_INT32_FIELD: _p::Descriptor<u32> = _p::Descriptor::<u32> {
        slot: 5u32,
        default: 0u32,
    };
    const U_INT64_FIELD: _p::Descriptor<u64> = _p::Descriptor::<u64> {
        slot: 3u32,
        default: 0u64,
    };
    const FLOAT32_FIELD: _p::Descriptor<f32> = _p::Descriptor::<f32> {
        slot: 8u32,
        default: 0f32,
    };
    const FLOAT64_FIELD: _p::Descriptor<f64> = _p::Descriptor::<f64> {
        slot: 5u32,
        default: 0f64,
    };
    const TEXT_FIELD: _p::Descriptor<_p::Text> = _p::Descriptor::<_p::Text> {
        slot: 0u32,
        default: _p::text::Reader::empty(),
    };
    const DATA_FIELD: _p::Descriptor<_p::Data> = _p::Descriptor::<_p::Data> {
        slot: 1u32,
        default: _p::data::Reader::empty(),
    };
    const STRUCT_FIELD: _p::Descriptor<_p::Struct<TestAllTypes>> = _p::Descriptor::<
        _p::Struct<TestAllTypes>,
    > {
        slot: 2u32,
        default: _p::StructReader::empty(),
    };
    const ENUM_FIELD: _p::Descriptor<_p::Enum<TestEnum>> = _p::Descriptor::<
        _p::Enum<TestEnum>,
    > {
        slot: 18u32,
        default: TestEnum::Foo,
    };
    const INTERFACE_FIELD: _p::Descriptor<()> = ();
    const VOID_LIST: _p::Descriptor<_p::List<()>> = _p::Descriptor::<_p::List<()>> {
        slot: 3u32,
        default: _p::ListReader::empty(_p::ElementSize::size_of::<()>()),
    };
    const BOOL_LIST: _p::Descriptor<_p::List<bool>> = _p::Descriptor::<_p::List<bool>> {
        slot: 4u32,
        default: _p::ListReader::empty(_p::ElementSize::size_of::<bool>()),
    };
    const INT8_LIST: _p::Descriptor<_p::List<i8>> = _p::Descriptor::<_p::List<i8>> {
        slot: 5u32,
        default: _p::ListReader::empty(_p::ElementSize::size_of::<i8>()),
    };
    const INT16_LIST: _p::Descriptor<_p::List<i16>> = _p::Descriptor::<_p::List<i16>> {
        slot: 6u32,
        default: _p::ListReader::empty(_p::ElementSize::size_of::<i16>()),
    };
    const INT32_LIST: _p::Descriptor<_p::List<i32>> = _p::Descriptor::<_p::List<i32>> {
        slot: 7u32,
        default: _p::ListReader::empty(_p::ElementSize::size_of::<i32>()),
    };
    const INT64_LIST: _p::Descriptor<_p::List<i64>> = _p::Descriptor::<_p::List<i64>> {
        slot: 8u32,
        default: _p::ListReader::empty(_p::ElementSize::size_of::<i64>()),
    };
    const U_INT8_LIST: _p::Descriptor<_p::List<u8>> = _p::Descriptor::<_p::List<u8>> {
        slot: 9u32,
        default: _p::ListReader::empty(_p::ElementSize::size_of::<u8>()),
    };
    const U_INT16_LIST: _p::Descriptor<_p::List<u16>> = _p::Descriptor::<_p::List<u16>> {
        slot: 10u32,
        default: _p::ListReader::empty(_p::ElementSize::size_of::<u16>()),
    };
    const U_INT32_LIST: _p::Descriptor<_p::List<u32>> = _p::Descriptor::<_p::List<u32>> {
        slot: 11u32,
        default: _p::ListReader::empty(_p::ElementSize::size_of::<u32>()),
    };
    const U_INT64_LIST: _p::Descriptor<_p::List<u64>> = _p::Descriptor::<_p::List<u64>> {
        slot: 12u32,
        default: _p::ListReader::empty(_p::ElementSize::size_of::<u64>()),
    };
    const FLOAT32_LIST: _p::Descriptor<_p::List<f32>> = _p::Descriptor::<_p::List<f32>> {
        slot: 13u32,
        default: _p::ListReader::empty(_p::ElementSize::size_of::<f32>()),
    };
    const FLOAT64_LIST: _p::Descriptor<_p::List<f64>> = _p::Descriptor::<_p::List<f64>> {
        slot: 14u32,
        default: _p::ListReader::empty(_p::ElementSize::size_of::<f64>()),
    };
    const TEXT_LIST: _p::Descriptor<_p::List<_p::Text>> = _p::Descriptor::<
        _p::List<_p::Text>,
    > {
        slot: 15u32,
        default: _p::ListReader::empty(_p::ElementSize::size_of::<_p::Text>()),
    };
    const DATA_LIST: _p::Descriptor<_p::List<_p::Data>> = _p::Descriptor::<
        _p::List<_p::Data>,
    > {
        slot: 16u32,
        default: _p::ListReader::empty(_p::ElementSize::size_of::<_p::Data>()),
    };
    const STRUCT_LIST: _p::Descriptor<_p::List<_p::Struct<TestAllTypes>>> = _p::Descriptor::<
        _p::List<_p::Struct<TestAllTypes>>,
    > {
        slot: 17u32,
        default: _p::ListReader::empty(
            _p::ElementSize::size_of::<_p::Struct<TestAllTypes>>(),
        ),
    };
    const ENUM_LIST: _p::Descriptor<_p::List<_p::Enum<TestEnum>>> = _p::Descriptor::<
        _p::List<_p::Enum<TestEnum>>,
    > {
        slot: 18u32,
        default: _p::ListReader::empty(_p::ElementSize::size_of::<_p::Enum<TestEnum>>()),
    };
    const INTERFACE_LIST: _p::Descriptor<_p::List<()>> = _p::Descriptor::<_p::List<()>> {
        slot: 19u32,
        default: _p::ListReader::empty(_p::ElementSize::size_of::<()>()),
    };
}
impl<'p, T: _p::rpc::Table + 'p> test_all_types::Reader<'p, T> {
    #[inline]
    pub fn void_field(&self) -> _p::Accessor<'_, 'p, T, ()> {
        unsafe {
            <() as _p::field::FieldType>::accessor(&self.0, &TestAllTypes::VOID_FIELD)
        }
    }
    #[inline]
    pub fn bool_field(&self) -> _p::Accessor<'_, 'p, T, bool> {
        unsafe {
            <bool as _p::field::FieldType>::accessor(&self.0, &TestAllTypes::BOOL_FIELD)
        }
    }
    #[inline]
    pub fn int8_field(&self) -> _p::Accessor<'_, 'p, T, i8> {
        unsafe {
            <i8 as _p::field::FieldType>::accessor(&self.0, &TestAllTypes::INT8_FIELD)
        }
    }
    #[inline]
    pub fn int16_field(&self) -> _p::Accessor<'_, 'p, T, i16> {
        unsafe {
            <i16 as _p::field::FieldType>::accessor(&self.0, &TestAllTypes::INT16_FIELD)
        }
    }
    #[inline]
    pub fn int32_field(&self) -> _p::Accessor<'_, 'p, T, i32> {
        unsafe {
            <i32 as _p::field::FieldType>::accessor(&self.0, &TestAllTypes::INT32_FIELD)
        }
    }
    #[inline]
    pub fn int64_field(&self) -> _p::Accessor<'_, 'p, T, i64> {
        unsafe {
            <i64 as _p::field::FieldType>::accessor(&self.0, &TestAllTypes::INT64_FIELD)
        }
    }
    #[inline]
    pub fn u_int8_field(&self) -> _p::Accessor<'_, 'p, T, u8> {
        unsafe {
            <u8 as _p::field::FieldType>::accessor(&self.0, &TestAllTypes::U_INT8_FIELD)
        }
    }
    #[inline]
    pub fn u_int16_field(&self) -> _p::Accessor<'_, 'p, T, u16> {
        unsafe {
            <u16 as _p::field::FieldType>::accessor(
                &self.0,
                &TestAllTypes::U_INT16_FIELD,
            )
        }
    }
    #[inline]
    pub fn u_int32_field(&self) -> _p::Accessor<'_, 'p, T, u32> {
        unsafe {
            <u32 as _p::field::FieldType>::accessor(
                &self.0,
                &TestAllTypes::U_INT32_FIELD,
            )
        }
    }
    #[inline]
    pub fn u_int64_field(&self) -> _p::Accessor<'_, 'p, T, u64> {
        unsafe {
            <u64 as _p::field::FieldType>::accessor(
                &self.0,
                &TestAllTypes::U_INT64_FIELD,
            )
        }
    }
    #[inline]
    pub fn float32_field(&self) -> _p::Accessor<'_, 'p, T, f32> {
        unsafe {
            <f32 as _p::field::FieldType>::accessor(
                &self.0,
                &TestAllTypes::FLOAT32_FIELD,
            )
        }
    }
    #[inline]
    pub fn float64_field(&self) -> _p::Accessor<'_, 'p, T, f64> {
        unsafe {
            <f64 as _p::field::FieldType>::accessor(
                &self.0,
                &TestAllTypes::FLOAT64_FIELD,
            )
        }
    }
    #[inline]
    pub fn text_field(&self) -> _p::Accessor<'_, 'p, T, _p::Text> {
        unsafe {
            <_p::Text as _p::field::FieldType>::accessor(
                &self.0,
                &TestAllTypes::TEXT_FIELD,
            )
        }
    }
    #[inline]
    pub fn data_field(&self) -> _p::Accessor<'_, 'p, T, _p::Data> {
        unsafe {
            <_p::Data as _p::field::FieldType>::accessor(
                &self.0,
                &TestAllTypes::DATA_FIELD,
            )
        }
    }
    #[inline]
    pub fn struct_field(&self) -> _p::Accessor<'_, 'p, T, _p::Struct<TestAllTypes>> {
        unsafe {
            <_p::Struct<
                TestAllTypes,
            > as _p::field::FieldType>::accessor(&self.0, &TestAllTypes::STRUCT_FIELD)
        }
    }
    #[inline]
    pub fn enum_field(&self) -> _p::Accessor<'_, 'p, T, _p::Enum<TestEnum>> {
        unsafe {
            <_p::Enum<
                TestEnum,
            > as _p::field::FieldType>::accessor(&self.0, &TestAllTypes::ENUM_FIELD)
        }
    }
    #[inline]
    pub fn interface_field(&self) -> _p::Accessor<'_, 'p, T, ()> {
        unsafe {
            <() as _p::field::FieldType>::accessor(
                &self.0,
                &TestAllTypes::INTERFACE_FIELD,
            )
        }
    }
    #[inline]
    pub fn void_list(&self) -> _p::Accessor<'_, 'p, T, _p::List<()>> {
        unsafe {
            <_p::List<
                (),
            > as _p::field::FieldType>::accessor(&self.0, &TestAllTypes::VOID_LIST)
        }
    }
    #[inline]
    pub fn bool_list(&self) -> _p::Accessor<'_, 'p, T, _p::List<bool>> {
        unsafe {
            <_p::List<
                bool,
            > as _p::field::FieldType>::accessor(&self.0, &TestAllTypes::BOOL_LIST)
        }
    }
    #[inline]
    pub fn int8_list(&self) -> _p::Accessor<'_, 'p, T, _p::List<i8>> {
        unsafe {
            <_p::List<
                i8,
            > as _p::field::FieldType>::accessor(&self.0, &TestAllTypes::INT8_LIST)
        }
    }
    #[inline]
    pub fn int16_list(&self) -> _p::Accessor<'_, 'p, T, _p::List<i16>> {
        unsafe {
            <_p::List<
                i16,
            > as _p::field::FieldType>::accessor(&self.0, &TestAllTypes::INT16_LIST)
        }
    }
    #[inline]
    pub fn int32_list(&self) -> _p::Accessor<'_, 'p, T, _p::List<i32>> {
        unsafe {
            <_p::List<
                i32,
            > as _p::field::FieldType>::accessor(&self.0, &TestAllTypes::INT32_LIST)
        }
    }
    #[inline]
    pub fn int64_list(&self) -> _p::Accessor<'_, 'p, T, _p::List<i64>> {
        unsafe {
            <_p::List<
                i64,
            > as _p::field::FieldType>::accessor(&self.0, &TestAllTypes::INT64_LIST)
        }
    }
    #[inline]
    pub fn u_int8_list(&self) -> _p::Accessor<'_, 'p, T, _p::List<u8>> {
        unsafe {
            <_p::List<
                u8,
            > as _p::field::FieldType>::accessor(&self.0, &TestAllTypes::U_INT8_LIST)
        }
    }
    #[inline]
    pub fn u_int16_list(&self) -> _p::Accessor<'_, 'p, T, _p::List<u16>> {
        unsafe {
            <_p::List<
                u16,
            > as _p::field::FieldType>::accessor(&self.0, &TestAllTypes::U_INT16_LIST)
        }
    }
    #[inline]
    pub fn u_int32_list(&self) -> _p::Accessor<'_, 'p, T, _p::List<u32>> {
        unsafe {
            <_p::List<
                u32,
            > as _p::field::FieldType>::accessor(&self.0, &TestAllTypes::U_INT32_LIST)
        }
    }
    #[inline]
    pub fn u_int64_list(&self) -> _p::Accessor<'_, 'p, T, _p::List<u64>> {
        unsafe {
            <_p::List<
                u64,
            > as _p::field::FieldType>::accessor(&self.0, &TestAllTypes::U_INT64_LIST)
        }
    }
    #[inline]
    pub fn float32_list(&self) -> _p::Accessor<'_, 'p, T, _p::List<f32>> {
        unsafe {
            <_p::List<
                f32,
            > as _p::field::FieldType>::accessor(&self.0, &TestAllTypes::FLOAT32_LIST)
        }
    }
    #[inline]
    pub fn float64_list(&self) -> _p::Accessor<'_, 'p, T, _p::List<f64>> {
        unsafe {
            <_p::List<
                f64,
            > as _p::field::FieldType>::accessor(&self.0, &TestAllTypes::FLOAT64_LIST)
        }
    }
    #[inline]
    pub fn text_list(&self) -> _p::Accessor<'_, 'p, T, _p::List<_p::Text>> {
        unsafe {
            <_p::List<
                _p::Text,
            > as _p::field::FieldType>::accessor(&self.0, &TestAllTypes::TEXT_LIST)
        }
    }
    #[inline]
    pub fn data_list(&self) -> _p::Accessor<'_, 'p, T, _p::List<_p::Data>> {
        unsafe {
            <_p::List<
                _p::Data,
            > as _p::field::FieldType>::accessor(&self.0, &TestAllTypes::DATA_LIST)
        }
    }
    #[inline]
    pub fn struct_list(
        &self,
    ) -> _p::Accessor<'_, 'p, T, _p::List<_p::Struct<TestAllTypes>>> {
        unsafe {
            <_p::List<
                _p::Struct<TestAllTypes>,
            > as _p::field::FieldType>::accessor(&self.0, &TestAllTypes::STRUCT_LIST)
        }
    }
    #[inline]
    pub fn enum_list(&self) -> _p::Accessor<'_, 'p, T, _p::List<_p::Enum<TestEnum>>> {
        unsafe {
            <_p::List<
                _p::Enum<TestEnum>,
            > as _p::field::FieldType>::accessor(&self.0, &TestAllTypes::ENUM_LIST)
        }
    }
    #[inline]
    pub fn interface_list(&self) -> _p::Accessor<'_, 'p, T, _p::List<()>> {
        unsafe {
            <_p::List<
                (),
            > as _p::field::FieldType>::accessor(&self.0, &TestAllTypes::INTERFACE_LIST)
        }
    }
}
impl<'p, T: _p::rpc::Table + 'p> test_all_types::Builder<'p, T> {
    #[inline]
    pub fn void_field(&mut self) -> _p::AccessorMut<'_, 'p, T, ()> {
        unsafe {
            <() as _p::field::FieldType>::accessor(
                &mut self.0,
                &TestAllTypes::VOID_FIELD,
            )
        }
    }
    #[inline]
    pub fn bool_field(&mut self) -> _p::AccessorMut<'_, 'p, T, bool> {
        unsafe {
            <bool as _p::field::FieldType>::accessor(
                &mut self.0,
                &TestAllTypes::BOOL_FIELD,
            )
        }
    }
    #[inline]
    pub fn int8_field(&mut self) -> _p::AccessorMut<'_, 'p, T, i8> {
        unsafe {
            <i8 as _p::field::FieldType>::accessor(
                &mut self.0,
                &TestAllTypes::INT8_FIELD,
            )
        }
    }
    #[inline]
    pub fn int16_field(&mut self) -> _p::AccessorMut<'_, 'p, T, i16> {
        unsafe {
            <i16 as _p::field::FieldType>::accessor(
                &mut self.0,
                &TestAllTypes::INT16_FIELD,
            )
        }
    }
    #[inline]
    pub fn int32_field(&mut self) -> _p::AccessorMut<'_, 'p, T, i32> {
        unsafe {
            <i32 as _p::field::FieldType>::accessor(
                &mut self.0,
                &TestAllTypes::INT32_FIELD,
            )
        }
    }
    #[inline]
    pub fn int64_field(&mut self) -> _p::AccessorMut<'_, 'p, T, i64> {
        unsafe {
            <i64 as _p::field::FieldType>::accessor(
                &mut self.0,
                &TestAllTypes::INT64_FIELD,
            )
        }
    }
    #[inline]
    pub fn u_int8_field(&mut self) -> _p::AccessorMut<'_, 'p, T, u8> {
        unsafe {
            <u8 as _p::field::FieldType>::accessor(
                &mut self.0,
                &TestAllTypes::U_INT8_FIELD,
            )
        }
    }
    #[inline]
    pub fn u_int16_field(&mut self) -> _p::AccessorMut<'_, 'p, T, u16> {
        unsafe {
            <u16 as _p::field::FieldType>::accessor(
                &mut self.0,
                &TestAllTypes::U_INT16_FIELD,
            )
        }
    }
    #[inline]
    pub fn u_int32_field(&mut self) -> _p::AccessorMut<'_, 'p, T, u32> {
        unsafe {
            <u32 as _p::field::FieldType>::accessor(
                &mut self.0,
                &TestAllTypes::U_INT32_FIELD,
            )
        }
    }
    #[inline]
    pub fn u_int64_field(&mut self) -> _p::AccessorMut<'_, 'p, T, u64> {
        unsafe {
            <u64 as _p::field::FieldType>::accessor(
                &mut self.0,
                &TestAllTypes::U_INT64_FIELD,
            )
        }
    }
    #[inline]
    pub fn float32_field(&mut self) -> _p::AccessorMut<'_, 'p, T, f32> {
        unsafe {
            <f32 as _p::field::FieldType>::accessor(
                &mut self.0,
                &TestAllTypes::FLOAT32_FIELD,
            )
        }
    }
    #[inline]
    pub fn float64_field(&mut self) -> _p::AccessorMut<'_, 'p, T, f64> {
        unsafe {
            <f64 as _p::field::FieldType>::accessor(
                &mut self.0,
                &TestAllTypes::FLOAT64_FIELD,
            )
        }
    }
    #[inline]
    pub fn text_field(&mut self) -> _p::AccessorMut<'_, 'p, T, _p::Text> {
        unsafe {
            <_p::Text as _p::field::FieldType>::accessor(
                &mut self.0,
                &TestAllTypes::TEXT_FIELD,
            )
        }
    }
    #[inline]
    pub fn data_field(&mut self) -> _p::AccessorMut<'_, 'p, T, _p::Data> {
        unsafe {
            <_p::Data as _p::field::FieldType>::accessor(
                &mut self.0,
                &TestAllTypes::DATA_FIELD,
            )
        }
    }
    #[inline]
    pub fn struct_field(
        &mut self,
    ) -> _p::AccessorMut<'_, 'p, T, _p::Struct<TestAllTypes>> {
        unsafe {
            <_p::Struct<
                TestAllTypes,
            > as _p::field::FieldType>::accessor(
                &mut self.0,
                &TestAllTypes::STRUCT_FIELD,
            )
        }
    }
    #[inline]
    pub fn enum_field(&mut self) -> _p::AccessorMut<'_, 'p, T, _p::Enum<TestEnum>> {
        unsafe {
            <_p::Enum<
                TestEnum,
            > as _p::field::FieldType>::accessor(&mut self.0, &TestAllTypes::ENUM_FIELD)
        }
    }
    #[inline]
    pub fn interface_field(&mut self) -> _p::AccessorMut<'_, 'p, T, ()> {
        unsafe {
            <() as _p::field::FieldType>::accessor(
                &mut self.0,
                &TestAllTypes::INTERFACE_FIELD,
            )
        }
    }
    #[inline]
    pub fn void_list(&mut self) -> _p::AccessorMut<'_, 'p, T, _p::List<()>> {
        unsafe {
            <_p::List<
                (),
            > as _p::field::FieldType>::accessor(&mut self.0, &TestAllTypes::VOID_LIST)
        }
    }
    #[inline]
    pub fn bool_list(&mut self) -> _p::AccessorMut<'_, 'p, T, _p::List<bool>> {
        unsafe {
            <_p::List<
                bool,
            > as _p::field::FieldType>::accessor(&mut self.0, &TestAllTypes::BOOL_LIST)
        }
    }
    #[inline]
    pub fn int8_list(&mut self) -> _p::AccessorMut<'_, 'p, T, _p::List<i8>> {
        unsafe {
            <_p::List<
                i8,
            > as _p::field::FieldType>::accessor(&mut self.0, &TestAllTypes::INT8_LIST)
        }
    }
    #[inline]
    pub fn int16_list(&mut self) -> _p::AccessorMut<'_, 'p, T, _p::List<i16>> {
        unsafe {
            <_p::List<
                i16,
            > as _p::field::FieldType>::accessor(&mut self.0, &TestAllTypes::INT16_LIST)
        }
    }
    #[inline]
    pub fn int32_list(&mut self) -> _p::AccessorMut<'_, 'p, T, _p::List<i32>> {
        unsafe {
            <_p::List<
                i32,
            > as _p::field::FieldType>::accessor(&mut self.0, &TestAllTypes::INT32_LIST)
        }
    }
    #[inline]
    pub fn int64_list(&mut self) -> _p::AccessorMut<'_, 'p, T, _p::List<i64>> {
        unsafe {
            <_p::List<
                i64,
            > as _p::field::FieldType>::accessor(&mut self.0, &TestAllTypes::INT64_LIST)
        }
    }
    #[inline]
    pub fn u_int8_list(&mut self) -> _p::AccessorMut<'_, 'p, T, _p::List<u8>> {
        unsafe {
            <_p::List<
                u8,
            > as _p::field::FieldType>::accessor(&mut self.0, &TestAllTypes::U_INT8_LIST)
        }
    }
    #[inline]
    pub fn u_int16_list(&mut self) -> _p::AccessorMut<'_, 'p, T, _p::List<u16>> {
        unsafe {
            <_p::List<
                u16,
            > as _p::field::FieldType>::accessor(
                &mut self.0,
                &TestAllTypes::U_INT16_LIST,
            )
        }
    }
    #[inline]
    pub fn u_int32_list(&mut self) -> _p::AccessorMut<'_, 'p, T, _p::List<u32>> {
        unsafe {
            <_p::List<
                u32,
            > as _p::field::FieldType>::accessor(
                &mut self.0,
                &TestAllTypes::U_INT32_LIST,
            )
        }
    }
    #[inline]
    pub fn u_int64_list(&mut self) -> _p::AccessorMut<'_, 'p, T, _p::List<u64>> {
        unsafe {
            <_p::List<
                u64,
            > as _p::field::FieldType>::accessor(
                &mut self.0,
                &TestAllTypes::U_INT64_LIST,
            )
        }
    }
    #[inline]
    pub fn float32_list(&mut self) -> _p::AccessorMut<'_, 'p, T, _p::List<f32>> {
        unsafe {
            <_p::List<
                f32,
            > as _p::field::FieldType>::accessor(
                &mut self.0,
                &TestAllTypes::FLOAT32_LIST,
            )
        }
    }
    #[inline]
    pub fn float64_list(&mut self) -> _p::AccessorMut<'_, 'p, T, _p::List<f64>> {
        unsafe {
            <_p::List<
                f64,
            > as _p::field::FieldType>::accessor(
                &mut self.0,
                &TestAllTypes::FLOAT64_LIST,
            )
        }
    }
    #[inline]
    pub fn text_list(&mut self) -> _p::AccessorMut<'_, 'p, T, _p::List<_p::Text>> {
        unsafe {
            <_p::List<
                _p::Text,
            > as _p::field::FieldType>::accessor(&mut self.0, &TestAllTypes::TEXT_LIST)
        }
    }
    #[inline]
    pub fn data_list(&mut self) -> _p::AccessorMut<'_, 'p, T, _p::List<_p::Data>> {
        unsafe {
            <_p::List<
                _p::Data,
            > as _p::field::FieldType>::accessor(&mut self.0, &TestAllTypes::DATA_LIST)
        }
    }
    #[inline]
    pub fn struct_list(
        &mut self,
    ) -> _p::AccessorMut<'_, 'p, T, _p::List<_p::Struct<TestAllTypes>>> {
        unsafe {
            <_p::List<
                _p::Struct<TestAllTypes>,
            > as _p::field::FieldType>::accessor(&mut self.0, &TestAllTypes::STRUCT_LIST)
        }
    }
    #[inline]
    pub fn enum_list(
        &mut self,
    ) -> _p::AccessorMut<'_, 'p, T, _p::List<_p::Enum<TestEnum>>> {
        unsafe {
            <_p::List<
                _p::Enum<TestEnum>,
            > as _p::field::FieldType>::accessor(&mut self.0, &TestAllTypes::ENUM_LIST)
        }
    }
    #[inline]
    pub fn interface_list(&mut self) -> _p::AccessorMut<'_, 'p, T, _p::List<()>> {
        unsafe {
            <_p::List<
                (),
            > as _p::field::FieldType>::accessor(
                &mut self.0,
                &TestAllTypes::INTERFACE_LIST,
            )
        }
    }
    #[inline]
    pub fn into_text_field(self) -> _p::AccessorOwned<'p, T, _p::Text> {
        unsafe {
            <_p::Text as _p::field::FieldType>::accessor(
                self.0,
                &TestAllTypes::TEXT_FIELD,
            )
        }
    }
    #[inline]
    pub fn into_data_field(self) -> _p::AccessorOwned<'p, T, _p::Data> {
        unsafe {
            <_p::Data as _p::field::FieldType>::accessor(
                self.0,
                &TestAllTypes::DATA_FIELD,
            )
        }
    }
    #[inline]
    pub fn into_struct_field(
        self,
    ) -> _p::AccessorOwned<'p, T, _p::Struct<TestAllTypes>> {
        unsafe {
            <_p::Struct<
                TestAllTypes,
            > as _p::field::FieldType>::accessor(self.0, &TestAllTypes::STRUCT_FIELD)
        }
    }
    #[inline]
    pub fn into_void_list(self) -> _p::AccessorOwned<'p, T, _p::List<()>> {
        unsafe {
            <_p::List<
                (),
            > as _p::field::FieldType>::accessor(self.0, &TestAllTypes::VOID_LIST)
        }
    }
    #[inline]
    pub fn into_bool_list(self) -> _p::AccessorOwned<'p, T, _p::List<bool>> {
        unsafe {
            <_p::List<
                bool,
            > as _p::field::FieldType>::accessor(self.0, &TestAllTypes::BOOL_LIST)
        }
    }
    #[inline]
    pub fn into_int8_list(self) -> _p::AccessorOwned<'p, T, _p::List<i8>> {
        unsafe {
            <_p::List<
                i8,
            > as _p::field::FieldType>::accessor(self.0, &TestAllTypes::INT8_LIST)
        }
    }
    #[inline]
    pub fn into_int16_list(self) -> _p::AccessorOwned<'p, T, _p::List<i16>> {
        unsafe {
            <_p::List<
                i16,
            > as _p::field::FieldType>::accessor(self.0, &TestAllTypes::INT16_LIST)
        }
    }
    #[inline]
    pub fn into_int32_list(self) -> _p::AccessorOwned<'p, T, _p::List<i32>> {
        unsafe {
            <_p::List<
                i32,
            > as _p::field::FieldType>::accessor(self.0, &TestAllTypes::INT32_LIST)
        }
    }
    #[inline]
    pub fn into_int64_list(self) -> _p::AccessorOwned<'p, T, _p::List<i64>> {
        unsafe {
            <_p::List<
                i64,
            > as _p::field::FieldType>::accessor(self.0, &TestAllTypes::INT64_LIST)
        }
    }
    #[inline]
    pub fn into_u_int8_list(self) -> _p::AccessorOwned<'p, T, _p::List<u8>> {
        unsafe {
            <_p::List<
                u8,
            > as _p::field::FieldType>::accessor(self.0, &TestAllTypes::U_INT8_LIST)
        }
    }
    #[inline]
    pub fn into_u_int16_list(self) -> _p::AccessorOwned<'p, T, _p::List<u16>> {
        unsafe {
            <_p::List<
                u16,
            > as _p::field::FieldType>::accessor(self.0, &TestAllTypes::U_INT16_LIST)
        }
    }
    #[inline]
    pub fn into_u_int32_list(self) -> _p::AccessorOwned<'p, T, _p::List<u32>> {
        unsafe {
            <_p::List<
                u32,
            > as _p::field::FieldType>::accessor(self.0, &TestAllTypes::U_INT32_LIST)
        }
    }
    #[inline]
    pub fn into_u_int64_list(self) -> _p::AccessorOwned<'p, T, _p::List<u64>> {
        unsafe {
            <_p::List<
                u64,
            > as _p::field::FieldType>::accessor(self.0, &TestAllTypes::U_INT64_LIST)
        }
    }
    #[inline]
    pub fn into_float32_list(self) -> _p::AccessorOwned<'p, T, _p::List<f32>> {
        unsafe {
            <_p::List<
                f32,
            > as _p::field::FieldType>::accessor(self.0, &TestAllTypes::FLOAT32_LIST)
        }
    }
    #[inline]
    pub fn into_float64_list(self) -> _p::AccessorOwned<'p, T, _p::List<f64>> {
        unsafe {
            <_p::List<
                f64,
            > as _p::field::FieldType>::accessor(self.0, &TestAllTypes::FLOAT64_LIST)
        }
    }
    #[inline]
    pub fn into_text_list(self) -> _p::AccessorOwned<'p, T, _p::List<_p::Text>> {
        unsafe {
            <_p::List<
                _p::Text,
            > as _p::field::FieldType>::accessor(self.0, &TestAllTypes::TEXT_LIST)
        }
    }
    #[inline]
    pub fn into_data_list(self) -> _p::AccessorOwned<'p, T, _p::List<_p::Data>> {
        unsafe {
            <_p::List<
                _p::Data,
            > as _p::field::FieldType>::accessor(self.0, &TestAllTypes::DATA_LIST)
        }
    }
    #[inline]
    pub fn into_struct_list(
        self,
    ) -> _p::AccessorOwned<'p, T, _p::List<_p::Struct<TestAllTypes>>> {
        unsafe {
            <_p::List<
                _p::Struct<TestAllTypes>,
            > as _p::field::FieldType>::accessor(self.0, &TestAllTypes::STRUCT_LIST)
        }
    }
    #[inline]
    pub fn into_enum_list(
        self,
    ) -> _p::AccessorOwned<'p, T, _p::List<_p::Enum<TestEnum>>> {
        unsafe {
            <_p::List<
                _p::Enum<TestEnum>,
            > as _p::field::FieldType>::accessor(self.0, &TestAllTypes::ENUM_LIST)
        }
    }
    #[inline]
    pub fn into_interface_list(self) -> _p::AccessorOwned<'p, T, _p::List<()>> {
        unsafe {
            <_p::List<
                (),
            > as _p::field::FieldType>::accessor(self.0, &TestAllTypes::INTERFACE_LIST)
        }
    }
}
pub mod test_all_types {
    use super::{__file, _p};
    pub type Reader<'a, T = _p::rpc::Empty> = super::TestAllTypes<
        _p::StructReader<'a, T>,
    >;
    pub type Builder<'a, T = _p::rpc::Empty> = super::TestAllTypes<
        _p::StructBuilder<'a, T>,
    >;
}

