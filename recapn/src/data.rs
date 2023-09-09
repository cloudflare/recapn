//! A fixed size blob of bytes contained in a Cap'n Proto message

use core::ops::{Deref, DerefMut};
use core::ptr::NonNull;

use crate::alloc::ElementCount;
use crate::list::ElementSize;
use crate::{ty, IntoFamily};
use crate::{internal::Sealed, Family};

pub mod ptr {
    use core::marker::PhantomData;
    use core::ptr::NonNull;
    use crate::alloc::ElementCount;

    #[derive(Clone, Copy)]
    pub struct Reader<'a> {
        a: PhantomData<&'a [u8]>,
        data: NonNull<u8>,
        len: ElementCount,
    }

    impl Reader<'_> {
        #[inline]
        pub const fn empty() -> Self {
            Self {
                a: PhantomData,
                data: NonNull::dangling(),
                len: ElementCount::MIN,
            }
        }

        #[inline]
        pub const unsafe fn new_unchecked(data: NonNull<u8>, len: ElementCount) -> Self {
            Self { a: PhantomData, data, len }
        }

        #[inline]
        pub const fn data(&self) -> NonNull<u8> {
            self.data
        }

        #[inline]
        pub const fn len(&self) -> ElementCount {
            self.len
        }
    }

    pub struct Builder<'a> {
        a: PhantomData<&'a mut [u8]>,
        data: NonNull<u8>,
        len: ElementCount,
    }

    impl Builder<'_> {
        #[inline]
        pub unsafe fn new_unchecked(data: NonNull<u8>, len: ElementCount) -> Self {
            Self { a: PhantomData, data, len }
        }

        #[inline]
        pub const fn data(&self) -> NonNull<u8> {
            self.data
        }

        #[inline]
        pub const fn len(&self) -> ElementCount {
            self.len
        }
    }
}

#[derive(Clone, Copy)]
pub struct Data<T = Family>(T);

impl Sealed for Data {}
impl<T> IntoFamily for Data<T> {
    type Family = Data;
}
impl ty::Value for Data {
    type Default = ptr::Reader<'static>;
}
impl ty::ListValue for Data {
    const ELEMENT_SIZE: ElementSize = ElementSize::Pointer;
}

pub type Reader<'a> = Data<ptr::Reader<'a>>;

impl<'a> From<ptr::Reader<'a>> for Reader<'a> {
    #[inline]
    fn from(repr: ptr::Reader<'a>) -> Self {
        Self(repr)
    }
}

impl<'a> From<Reader<'a>> for ptr::Reader<'a> {
    #[inline]
    fn from(value: Reader<'a>) -> Self {
        value.0
    }
}

impl<'a> Reader<'a> {
    /// Creates an empty data reader.
    #[inline]
    pub const fn empty() -> Self {
        Self(ptr::Reader::empty())
    }

    /// Creates a data reader from a slice of bytes, returning None if the slice is too large to
    /// be contained within a Cap'n Proto message.
    #[inline]
    pub const fn from_slice(slice: &'a [u8]) -> Option<Self> {
        let len = slice.len();
        if len > ElementCount::MAX_VALUE as usize {
            return None;
        }

        let count = ElementCount::new(len as u32).unwrap();
        unsafe {
            let ptr = NonNull::new_unchecked(slice.as_ptr().cast_mut());
            Some(Self(ptr::Reader::new_unchecked(ptr, count)))
        }
    }

    #[inline]
    pub const fn len(&self) -> u32 {
        self.0.len().get()
    }

    #[inline]
    pub const fn is_empty(&self) -> bool {
        self.len() == 0
    }

    #[inline]
    pub const fn as_slice(&self) -> &'a [u8] {
        let data = self.0.data().as_ptr().cast_const();
        let len = self.len() as usize;
        unsafe { core::slice::from_raw_parts(data, len) }
    }
}

impl Deref for Reader<'_> {
    type Target = [u8];

    #[inline]
    fn deref(&self) -> &Self::Target {
        self.as_slice()
    }
}

impl AsRef<[u8]> for Reader<'_> {
    #[inline]
    fn as_ref(&self) -> &[u8] {
        self.as_slice()
    }
}

impl<'a> AsRef<ptr::Reader<'a>> for Reader<'a> {
    #[inline]
    fn as_ref(&self) -> &ptr::Reader<'a> {
        &self.0
    }
}

impl Default for Reader<'_> {
    #[inline]
    fn default() -> Self {
        Self::empty()
    }
}

impl PartialEq<Reader<'_>> for Reader<'_> {
    #[inline]
    fn eq(&self, other: &Reader<'_>) -> bool {
        self.as_slice() == other.as_slice()
    }
}

impl PartialEq<[u8]> for Reader<'_> {
    #[inline]
    fn eq(&self, other: &[u8]) -> bool {
        self.as_slice() == other
    }
}

impl PartialEq<Reader<'_>> for [u8] {
    #[inline]
    fn eq(&self, other: &Reader<'_>) -> bool {
        self == other.as_slice()
    }
}

pub type Builder<'a> = Data<ptr::Builder<'a>>;

impl<'a> From<ptr::Builder<'a>> for Builder<'a> {
    #[inline]
    fn from(repr: ptr::Builder<'a>) -> Self {
        Self(repr)
    }
}

impl<'a> From<Builder<'a>> for ptr::Builder<'a> {
    #[inline]
    fn from(value: Builder<'a>) -> Self {
        value.0
    }
}

impl<'a> Builder<'a> {
    #[inline]
    pub fn as_reader<'b>(&'b self) -> Reader<'b> {
        Data(unsafe { ptr::Reader::new_unchecked(self.0.data(), self.0.len()) })
    }

    #[inline]
    pub const fn len(&self) -> u32 {
        self.0.len().get()
    }

    #[inline]
    pub const fn is_empty(&self) -> bool {
        self.len() == 0
    }

    #[inline]
    pub const fn as_slice(&self) -> &'a [u8] {
        let data = self.0.data().as_ptr().cast_const();
        let len = self.len() as usize;
        unsafe { core::slice::from_raw_parts(data, len) }
    }

    #[inline]
    pub fn as_slice_mut(&mut self) -> &'a mut [u8] {
        let data = self.0.data().as_ptr();
        let len = self.len() as usize;
        unsafe { core::slice::from_raw_parts_mut(data, len) }
    }
}

impl<'a> AsRef<ptr::Builder<'a>> for Builder<'a> {
    fn as_ref(&self) -> &ptr::Builder<'a> {
        &self.0
    }
}

impl<'a> AsRef<[u8]> for Builder<'a> {
    #[inline]
    fn as_ref(&self) -> &[u8] {
        self.as_slice()
    }
}

impl PartialEq<Builder<'_>> for Builder<'_> {
    fn eq(&self, other: &Builder<'_>) -> bool {
        **self == **other
    }
}

impl PartialEq<Reader<'_>> for Builder<'_> {
    fn eq(&self, other: &Reader<'_>) -> bool {
        **self == **other
    }
}

impl PartialEq<Builder<'_>> for Reader<'_> {
    fn eq(&self, other: &Builder<'_>) -> bool {
        **self == **other
    }
}

impl PartialEq<[u8]> for Builder<'_> {
    fn eq(&self, other: &[u8]) -> bool {
        **self == *other
    }
}

impl PartialEq<Builder<'_>> for [u8] {
    fn eq(&self, other: &Builder<'_>) -> bool {
        *self == **other
    }
}

impl Deref for Builder<'_> {
    type Target = [u8];

    #[inline]
    fn deref(&self) -> &Self::Target {
        self.as_slice()
    }
}

impl DerefMut for Builder<'_> {
    #[inline]
    fn deref_mut(&mut self) -> &mut Self::Target {
        self.as_slice_mut()
    }
}
