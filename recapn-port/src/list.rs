use core::ops::{Deref, DerefMut};

use recapn::ptr::{ElementSize, PtrBuilder, PtrElementSize, PtrReader};
use recapn::rpc::Table;
use recapn::ty;

use crate::data::Data;
use crate::text::Text;
use crate::{Deserialize, DeserializePtr, Enum, Error, Serialize, SerializePtr};

pub struct List<T>(pub Vec<T>);

impl<T> Deref for List<T> {
    type Target = Vec<T>;

    #[inline]
    fn deref(&self) -> &Self::Target {
        &self.0
    }
}

impl<T> DerefMut for List<T> {
    #[inline]
    fn deref_mut(&mut self) -> &mut Self::Target {
        &mut self.0
    }
}

impl<T> From<Vec<T>> for List<T> {
    #[inline]
    fn from(value: Vec<T>) -> Self {
        List(value)
    }
}

impl<T> From<List<T>> for Vec<T> {
    #[inline]
    fn from(value: List<T>) -> Self {
        value.0
    }
}

pub trait DeserializeList<'de, T: Table>: Sized {
    fn deserialize<E: Error>(r: &PtrReader<'de, T>) -> Result<List<Self>, E>;
}

macro_rules! deserialize_list_ptr {
    ($ptr:expr, $size:expr, |$list:pat_param| $block:block) => {
        match $ptr.to_list($size) {
            Ok(Some($list)) => $block,
            Ok(None) => Ok(List(Vec::new())),
            Err(err) => Err(E::protocol_error(err)),
        }
    };
}

macro_rules! deserialize_list {
    ($ty:ty, $size:expr, |$list:pat_param| $block:block) => {
        impl<T: Table> DeserializeList<'_, T> for $ty {
            fn deserialize<E: Error>(r: &PtrReader<'_, T>) -> Result<List<Self>, E> {
                deserialize_list_ptr!(r, $size, |$list| $block)
            }
        }
    };
}

macro_rules! deserialize_data_list {
    ($ty:ty, $size:expr) => {
        deserialize_list!($ty, Some($size), |l| {
            let len = l.len().get();
            let mut v = Vec::with_capacity(len as usize);
            for i in 0..len {
                v.push(unsafe { l.data_unchecked(i) });
            }
            Ok(List(v))
        });
    };
}

deserialize_list!((), Some(PtrElementSize::Void), |l| {
    Ok(List(vec![(); l.len().get() as usize]))
});

deserialize_data_list!(bool, PtrElementSize::Bit);

deserialize_data_list!(u8, PtrElementSize::Byte);
deserialize_data_list!(i8, PtrElementSize::Byte);

deserialize_data_list!(u16, PtrElementSize::TwoBytes);
deserialize_data_list!(i16, PtrElementSize::TwoBytes);

deserialize_data_list!(u32, PtrElementSize::FourBytes);
deserialize_data_list!(i32, PtrElementSize::FourBytes);
deserialize_data_list!(f32, PtrElementSize::FourBytes);

deserialize_data_list!(u64, PtrElementSize::EightBytes);
deserialize_data_list!(i64, PtrElementSize::EightBytes);
deserialize_data_list!(f64, PtrElementSize::EightBytes);

impl<En: ty::Enum, T: Table> DeserializeList<'_, T> for Enum<En> {
    fn deserialize<E: Error>(r: &PtrReader<'_, T>) -> Result<List<Self>, E> {
        deserialize_list_ptr!(r, Some(PtrElementSize::TwoBytes), |l| {
            let len = l.len().get();
            let mut v = Vec::with_capacity(len as usize);
            for i in 0..len {
                v.push(Enum::from_value(unsafe { l.data_unchecked::<u16>(i) }));
            }
            Ok(List(v))
        })
    }
}

deserialize_list!(Text, Some(PtrElementSize::Pointer), |l| {
    let len = l.len().get();
    let mut v = Vec::with_capacity(len as usize);
    for i in 0..len {
        let ptr = unsafe { l.ptr_unchecked(i) };
        v.push(<Text as DeserializePtr<'_, _>>::deserialize(&ptr)?);
    }
    Ok(List(v))
});

deserialize_list!(Data, Some(PtrElementSize::Pointer), |l| {
    let len = l.len().get();
    let mut v = Vec::with_capacity(len as usize);
    for i in 0..len {
        let ptr = unsafe { l.ptr_unchecked(i) };
        v.push(<Data as DeserializePtr<'_, _>>::deserialize(&ptr)?);
    }
    Ok(List(v))
});

impl<'de, V, T> DeserializeList<'de, T> for List<V>
where
    V: DeserializeList<'de, T>,
    T: Table,
{
    fn deserialize<E: Error>(r: &PtrReader<'de, T>) -> Result<List<Self>, E> {
        deserialize_list_ptr!(r, Some(PtrElementSize::Pointer), |l| {
            let len = l.len().get();
            let mut v = Vec::with_capacity(len as usize);
            for i in 0..len {
                let ptr = unsafe { l.ptr_unchecked(i) };
                v.push(<V as DeserializeList<'de, T>>::deserialize(&ptr)?.into());
            }
            Ok(List(v))
        })
    }
}

#[inline]
pub fn deserialize_struct_list<'de, S, T, E>(r: &PtrReader<'de, T>) -> Result<List<S>, E>
where
    S: Deserialize<'de, T>,
    T: Table,
    E: Error,
{
    deserialize_list_ptr!(r, Some(PtrElementSize::InlineComposite), |l| {
        let len = l.len().get();
        let mut v = Vec::with_capacity(len as usize);
        for i in 0..len {
            let s = unsafe { l.struct_unchecked(i) };
            v.push(<S as Deserialize<'de, T>>::deserialize(&s)?);
        }
        Ok(List(v))
    })
}

pub trait SerializeList<'se, T: Table>: Sized {
    fn serialize<E: Error>(s: &[Self], b: PtrBuilder<'se, T>) -> Result<(), E>;
}

macro_rules! serialize_list_ptr {
    ($slice:expr, $ptr:expr, $size:expr, |$slice_pat:pat_param, $list:pat_param| $block:block) => {
        'm: {
            let slice = $slice;
            let Ok(len) = recapn::ptr::ElementCount::try_from(slice.len()) else {
                break 'm Err(E::protocol_error(recapn::Error::AllocTooLarge))
            };
            let $slice_pat = slice;
            match $ptr.try_init_list($size, len) {
                Ok($list) => $block,
                Err((err, _)) => Err(E::protocol_error(err)),
            }
        }
    };
}

macro_rules! serialize_list {
    ($ty:ty, $size:expr, |$slice:pat_param, $list:pat_param| $block:block) => {
        impl<T: Table> SerializeList<'_, T> for $ty {
            fn serialize<E: Error>(s: &[Self], b: PtrBuilder<'_, T>) -> Result<(), E> {
                serialize_list_ptr!(s, b, $size, |$slice, $list| $block)
            }
        }
    };
}

macro_rules! serialize_data_list {
    ($ty:ty, $size:expr) => {
        serialize_list!($ty, $size, |slice, mut l| {
            for (i, &value) in slice.iter().enumerate() {
                unsafe { l.set_data_unchecked(i as u32, value) }
            }
            Ok(())
        });
    };
}

serialize_list!((), ElementSize::Void, |_, _| { Ok(()) });

serialize_data_list!(bool, ElementSize::Bit);

serialize_data_list!(u8, ElementSize::Byte);
serialize_data_list!(i8, ElementSize::Byte);

serialize_data_list!(u16, ElementSize::TwoBytes);
serialize_data_list!(i16, ElementSize::TwoBytes);

serialize_data_list!(u32, ElementSize::FourBytes);
serialize_data_list!(i32, ElementSize::FourBytes);
serialize_data_list!(f32, ElementSize::FourBytes);

serialize_data_list!(u64, ElementSize::EightBytes);
serialize_data_list!(i64, ElementSize::EightBytes);
serialize_data_list!(f64, ElementSize::EightBytes);

impl<En: ty::Enum, T: Table> SerializeList<'_, T> for Enum<En> {
    fn serialize<E: Error>(s: &[Self], b: PtrBuilder<'_, T>) -> Result<(), E> {
        serialize_list_ptr!(s, b, ElementSize::TwoBytes, |s, mut l| {
            for (i, &value) in s.iter().enumerate() {
                unsafe { l.set_data_unchecked(i as u32, value.into_value()) }
            }
            Ok(())
        })
    }
}

serialize_list!(Text, ElementSize::Pointer, |s, mut l| {
    for (i, value) in s.iter().enumerate() {
        let ptr = unsafe { l.ptr_mut_unchecked(i as u32) };
        <Text as SerializePtr<'_, _>>::serialize(value, ptr)?
    }
    Ok(())
});

serialize_list!(Data, ElementSize::Pointer, |s, mut l| {
    for (i, value) in s.iter().enumerate() {
        let ptr = unsafe { l.ptr_mut_unchecked(i as u32) };
        <Data as SerializePtr<'_, _>>::serialize(value, ptr)?
    }
    Ok(())
});

impl<'se, V, T> SerializeList<'se, T> for List<V>
where
    V: for<'se2> SerializeList<'se2, T>,
    T: Table,
{
    fn serialize<E: Error>(s: &[Self], b: PtrBuilder<'se, T>) -> Result<(), E> {
        serialize_list_ptr!(s, b, ElementSize::Pointer, |s, mut l| {
            for (i, value) in s.iter().enumerate() {
                let ptr = unsafe { l.ptr_mut_unchecked(i as u32) };
                <V as SerializeList<'_, _>>::serialize(value, ptr)?;
            }
            Ok(())
        })
    }
}

#[inline]
pub fn serialize_struct_list<S, T, E>(s: &[S], b: PtrBuilder<'_, T>) -> Result<(), E>
where
    S: for<'se> Serialize<'se, T>,
    T: Table,
    E: Error,
{
    serialize_list_ptr!(s, b, ElementSize::InlineComposite(S::SIZE), |s, mut l| {
        for (i, src) in s.iter().enumerate() {
            let mut dst = unsafe { l.struct_mut_unchecked(i as u32) };
            src.serialize(&mut dst)?;
        }
        Ok(())
    })
}

impl<'de, T, V> DeserializePtr<'de, T> for List<V>
where
    T: Table,
    V: DeserializeList<'de, T>,
{
    fn deserialize<E: Error>(r: &PtrReader<'de, T>) -> Result<Self, E> {
        <V as DeserializeList<'de, T>>::deserialize(r)
    }
}

impl<'se, T, V> SerializePtr<'se, T> for List<V>
where
    T: Table,
    V: SerializeList<'se, T>,
{
    fn serialize<E: Error>(&self, b: PtrBuilder<'se, T>) -> Result<(), E> {
        <V as SerializeList<'se, T>>::serialize(self, b)
    }
}