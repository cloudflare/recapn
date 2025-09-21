use core::ops::{Deref, DerefMut};

use recapn::ptr::{PtrBuilder, PtrReader};
use recapn::rpc::Table;

use crate::{DeserializePtr, Error, SerializePtr};

/// A boxed slice of bytes that's guaranteed to fit in a Cap'n Proto message.
#[derive(Clone, PartialEq, Eq, PartialOrd, Ord, Hash, Debug, Default)]
pub struct Data {
    // Invariant 1: the slice is within the max size of Cap'n Proto data.
    inner: Box<[u8]>,
}

impl Data {
    #[inline]
    pub fn new(b: Box<[u8]>) -> Result<Self, Box<[u8]>> {
        if recapn::ptr::ElementCount::try_from(b.len()).is_err() {
            return Err(b)
        }

        Ok(Self { inner: b })
    }
    #[inline]
    pub fn into_inner(this: Self) -> Box<[u8]> {
        this.inner
    }

    #[inline]
    pub fn from_reader(r: recapn::data::Reader<'_>) -> Self {
        Self { inner: Box::from(&*r) }
    }
    #[inline]
    pub fn as_reader(this: &Self) -> recapn::data::Reader<'_> {
        recapn::data::Reader::from_slice(&this)
    }
}

impl From<recapn::data::Reader<'_>> for Data {
    #[inline]
    fn from(value: recapn::data::Reader<'_>) -> Self {
        Self::from_reader(value)
    }
}
impl From<Data> for Box<[u8]> {
    #[inline]
    fn from(value: Data) -> Self {
        Data::into_inner(value)
    }
}

impl Deref for Data {
    type Target = [u8];

    #[inline]
    fn deref(&self) -> &Self::Target {
        &self.inner
    }
}

impl DerefMut for Data {
    #[inline]
    fn deref_mut(&mut self) -> &mut Self::Target {
        &mut self.inner
    }
}

impl<T: Table> DeserializePtr<'_, T> for Data {
    fn deserialize<E: Error>(r: &PtrReader<'_, T>) -> Result<Self, E> {
        let text = match r.to_blob() {
            Ok(Some(b)) => recapn::data::Reader::from(b),
            Ok(None) => recapn::data::Reader::empty(),
            Err(err) => return Err(E::protocol_error(err)),
        };
        Ok(Self::from_reader(text))
    }
}

impl<T: Table> SerializePtr<'_, T> for Data {
    fn serialize<E: Error>(&self, b: PtrBuilder<'_, T>) -> Result<(), E> {
        let len = self.inner.len().try_into().unwrap();
        let mut blob = match b.try_init_blob(len) {
            Ok(blob) => recapn::data::Builder::from(blob),
            Err((err, _)) => return Err(E::protocol_error(err)),
        };
        blob.copy_from_slice(&self.inner);
        Ok(())
    }
}