use core::marker::PhantomData;

use recapn::ptr::{PtrBuilder, PtrReader, StructBuilder, StructReader, StructSize};
use recapn::rpc::{Capable, Table};

pub mod data;
pub mod list;
pub mod text;

pub trait Error: Sized {
    fn custom<T: core::error::Error + 'static>(msg: T) -> Self;

    fn protocol_error(err: recapn::Error) -> Self {
        Self::custom(err)
    }
}

impl Error for Box<dyn core::error::Error> {
    fn custom<T: core::error::Error + 'static>(msg: T) -> Self {
        Box::new(msg)
    }

    fn protocol_error(err: recapn::Error) -> Self {
        Box::new(err)
    }
}

/// An enum value that can be converted into a native typed enum.
#[derive(Clone, Copy, Debug, Default, PartialEq, Eq, PartialOrd, Ord, Hash)]
pub struct Enum<E: recapn::ty::Enum> {
    e: PhantomData<fn() -> E>,
    value: u16,
}

impl<E: recapn::ty::Enum> Enum<E> {
    #[inline]
    pub const fn from_value(value: u16) -> Self {
        Self { e: PhantomData, value }
    }
    #[inline]
    pub fn from_type(value: E) -> Self {
        Self::from_value(value.into())
    }

    #[inline]
    pub fn get(self) -> Result<E, recapn::NotInSchema> {
        self.value.try_into()
    }
    #[inline]
    pub fn into_value(self) -> u16 {
        self.value
    }
}

pub trait Deserialize<'de, T: Table>: Sized {
    fn deserialize<E: Error>(r: &StructReader<'de, T>) -> Result<Self, E>;
}

impl<'de, T, S> Deserialize<'de, T> for Box<S>
where
    T: Table,
    S: Deserialize<'de, T>,
{
    #[inline]
    fn deserialize<E: Error>(r: &StructReader<'de, T>) -> Result<Self, E> {
        S::deserialize(r).map(Box::new)
    }
}

pub trait DeserializePtr<'de, T: Table>: Sized {
    fn deserialize<E: Error>(r: &PtrReader<'de, T>) -> Result<Self, E>;
}

impl<'de, T, P> DeserializePtr<'de, T> for Box<P>
where
    T: Table,
    P: DeserializePtr<'de, T>,
{
    #[inline]
    fn deserialize<E: Error>(r: &PtrReader<'de, T>) -> Result<Self, E> {
        P::deserialize(r).map(Box::new)
    }
}

impl<'de, T, P> DeserializePtr<'de, T> for Option<P>
where
    T: Table,
    P: DeserializePtr<'de, T>,
{
    #[inline]
    fn deserialize<E: Error>(r: &PtrReader<'de, T>) -> Result<Self, E> {
        if r.is_null() {
            return Ok(None)
        }

        P::deserialize(r).map(Some)
    }
}

pub trait Serialize<'se, T: Table> {
    const SIZE: StructSize;

    fn serialize<E: Error>(&self, b: &mut StructBuilder<'se, T>) -> Result<(), E>;
}

pub trait SerializePtr<'se, T: Table> {
    fn serialize<E: Error>(&self, b: PtrBuilder<'se, T>) -> Result<(), E>;
}

impl<'se, T, P> SerializePtr<'se, T> for Box<P>
where
    T: Table,
    P: SerializePtr<'se, T>,
{
    #[inline]
    fn serialize<E: Error>(&self, b: PtrBuilder<'se, T>) -> Result<(), E> {
        <P as SerializePtr<'se, T>>::serialize(self, b)
    }
}

impl<'se, T, P> SerializePtr<'se, T> for Option<P>
where
    T: Table,
    P: SerializePtr<'se, T>,
{
    #[inline]
    fn serialize<E: Error>(&self, b: PtrBuilder<'se, T>) -> Result<(), E> {
        let Some(value) = self else { return Ok(()) };
        value.serialize(b)
    }
}

/// Invokes the given types `DeserializePtr` implementation.
#[inline]
pub fn deserialize<'de, T, E, P>(t: &PtrReader<'de, T>) -> Result<P, E>
where
    T: Table,
    E: Error,
    P: DeserializePtr<'de, T>,
{
    P::deserialize(t)
}

/// Invokes the given types `DeserializePtr` implementation.
#[inline]
pub fn serialize<'se, T, E, P>(value: &P, t: PtrBuilder<'se, T>) -> Result<(), E>
where
    T: Table,
    E: Error,
    P: SerializePtr<'se, T>,
{
    P::serialize(value, t)
}

/// Deserializes a struct directly from a pointer reader. If the pointer is null, it deserializes
/// from an empty struct instead.
#[inline]
pub fn deserialize_struct<'de, T, E, S>(t: &PtrReader<'de, T>) -> Result<S, E>
where
    T: Table,
    E: Error,
    S: Deserialize<'de, T>,
{
    let r = match t.to_struct() {
        Ok(Some(s)) => s,
        Ok(None) => StructReader::empty().imbue_from(t),
        Err(err) => return Err(E::protocol_error(err)),
    };
    S::deserialize(&r)
}

#[inline]
pub fn serialize_struct<'se, T, E, S>(s: &S, b: PtrBuilder<'se, T>) -> Result<(), E>
where
    T: Table,
    E: Error,
    S: Serialize<'se, T>,
{
    match b.try_init_struct(S::SIZE) {
        Ok(mut b) => s.serialize(&mut b),
        Err((err, _)) => Err(E::protocol_error(err)),
    }
}

pub trait DeserializeAnyPtrExt<'de, T: Table> {
    fn deserialize<P, E>(&self) -> Result<P, E>
    where
        P: DeserializePtr<'de, T>,
        E: Error;
}

impl<'de, T: Table> DeserializeAnyPtrExt<'de, T> for recapn::any::PtrReader<'de, T> {
    fn deserialize<P, E>(&self) -> Result<P, E>
    where
        P: DeserializePtr<'de, T>,
        E: Error,
    {
        P::deserialize(&self.as_ref())
    }
}

pub trait SerializeAnyPtrExt<T: Table> {
    fn serialize<P, E>(&mut self, value: &P) -> Result<(), E>
    where
        P: for<'se> SerializePtr<'se, T>,
        E: Error;
}

impl<T: Table> SerializeAnyPtrExt<T> for recapn::any::PtrBuilder<'_, T> {
    fn serialize<P, E>(&mut self, value: &P) -> Result<(), E>
    where
        P: for<'se> SerializePtr<'se, T>,
        E: Error,
    {
        P::serialize(value, self.as_mut().by_ref())
    }
}

#[cfg(test)]
mod tests {
}
