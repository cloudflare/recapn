//! UTF-8 encoded data with a null terminator

use core::fmt;
use core::ptr::NonNull;
use core::slice;
use core::str::Utf8Error;

use crate::list::ElementSize;
use crate::ptr::ElementCount;
use crate::ty;
use crate::{internal::Sealed, Family};

pub type ByteCount = crate::num::NonZeroU29;

pub mod ptr {
    pub use crate::ptr::{BlobBuilder as Builder, BlobReader as Reader};
}

/// Concatenates literals into a static text reader.
///
/// Internally this uses `core::concat!` and adds an extra nul byte. The resulting bytes
/// are passed directly to `text::Reader::from_slice`.
#[macro_export]
macro_rules! text {
    ($($e:expr),* $(,)?) => {
        $crate::text::Reader::from_slice(core::concat!($($e),*, '\0').as_bytes())
    };
}

const EMPTY_SLICE: &[u8] = &[0];

/// An error returned when trying to set a text field to a string value if the value is too large.
#[derive(Debug, Copy, Clone, PartialEq, Eq)]
pub struct TryFromStrError(pub(crate) ());

impl fmt::Display for TryFromStrError {
    fn fmt(&self, fmt: &mut fmt::Formatter<'_>) -> fmt::Result {
        fmt.write_str("attempted to create a text blob from too large a string")
    }
}

impl core::error::Error for TryFromStrError {}

#[derive(Clone, Copy)]
pub struct Text<T = Family>(T);

pub type Reader<'a> = Text<ptr::Reader<'a>>;
pub type Builder<'a> = Text<ptr::Builder<'a>>;

impl Sealed for Text {}
impl ty::ListValue for Text {
    const ELEMENT_SIZE: ElementSize = ElementSize::Pointer;
}

impl<'a> Reader<'a> {
    pub const EMPTY: Self = Self::empty();

    #[inline]
    pub const fn empty() -> Self {
        Self::from_slice(EMPTY_SLICE)
    }

    /// Interprets a given blob reader as text. This requires that the blob has at least one byte
    /// and that the last byte is empty.
    #[inline]
    pub const fn new(blob: ptr::Reader<'a>) -> Option<Self> {
        match blob.as_slice() {
            [.., 0] => Some(Self(blob)),
            _ => None,
        }
    }

    /// Converts from the given byte slice to a text reader. This asserts
    /// that the slice is a valid text blob and can be used in a constant context.
    #[inline]
    pub const fn from_slice(s: &'a [u8]) -> Self {
        if let [.., 0] = s { if let Some(r) = ptr::Reader::new(s) { return Self(r) } }

        panic!("attempted to make invalid text blob from slice")
    }

    pub const fn byte_count(&self) -> ByteCount {
        ByteCount::new_unwrap(self.0.len().get())
    }

    /// The length of the text (including the null terminator)
    #[inline]
    pub const fn len(&self) -> u32 {
        self.0.len().get()
    }

    #[inline]
    pub const fn is_empty(&self) -> bool {
        self.len() == 1
    }

    /// Returns the bytes of the text field without the null terminator
    #[inline]
    pub const fn as_bytes(&self) -> &'a [u8] {
        match self.as_bytes_with_nul().split_last() {
            Some((_, remainder)) => remainder,
            _ => {
                debug_assert!(
                    false,
                    "this shouldn't happen, it's to avoid panic code in release"
                );
                EMPTY_SLICE
            }
        }
    }

    /// Returns the bytes of the text field with the null terminator
    #[inline]
    pub const fn as_bytes_with_nul(&self) -> &'a [u8] {
        self.0.as_slice()
    }

    #[inline]
    pub const fn as_str(&self) -> Result<&'a str, Utf8Error> {
        core::str::from_utf8(self.as_bytes())
    }

    #[inline]
    pub const unsafe fn as_str_unchecked(&self) -> &'a str {
        core::str::from_utf8_unchecked(self.as_bytes())
    }
}

impl fmt::Debug for Reader<'_> {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        self.as_bytes_with_nul().fmt(f)
    }
}

impl<'a> From<Reader<'a>> for ptr::Reader<'a> {
    fn from(value: Reader<'a>) -> Self {
        value.0
    }
}

impl<'a> AsRef<ptr::Reader<'a>> for Reader<'a> {
    #[inline]
    fn as_ref(&self) -> &ptr::Reader<'a> {
        &self.0
    }
}

impl PartialEq<Reader<'_>> for Reader<'_> {
    fn eq(&self, other: &Reader<'_>) -> bool {
        self.as_bytes() == other.as_bytes()
    }
}

impl PartialEq<str> for Reader<'_> {
    fn eq(&self, other: &str) -> bool {
        self.as_bytes() == other.as_bytes()
    }
}

impl PartialEq<Reader<'_>> for str {
    fn eq(&self, other: &Reader<'_>) -> bool {
        self.as_bytes() == other.as_bytes()
    }
}

impl<'a> Builder<'a> {
    pub(crate) const fn new_unchecked(blob: ptr::Builder<'a>) -> Self {
        Self(blob)
    }

    /// Interprets a given blob builder as text. This requires that the blob has at least one byte
    /// and that the last byte is NUL.
    #[inline]
    pub const fn new(blob: ptr::Builder<'a>) -> Option<Self> {
        match blob.as_slice() {
            [.., 0] => Some(Self(blob)),
            _ => None,
        }
    }

    #[inline]
    pub fn empty() -> Self {
        // This is safe since we never expose the NUL byte for mutation. All functions that
        // return mutable slices for mutating the buffer take one byte off the end to make sure
        // the NUL byte is left untouched.
        let ptr = NonNull::new(EMPTY_SLICE.as_ptr().cast_mut()).unwrap();
        let len = ElementCount::new(1).unwrap();
        Self(ptr::Builder::new(ptr, len))
    }

    #[inline]
    pub fn as_reader(&self) -> Reader<'_> {
        Text(self.0.as_reader())
    }

    /// The length of the text (including the null terminator)
    #[inline]
    pub const fn len(&self) -> u32 {
        self.0.len().get()
    }

    #[inline]
    pub const fn is_empty(&self) -> bool {
        self.len() == 1
    }

    /// Returns the bytes of the text field without the null terminator
    #[inline]
    pub const fn as_bytes(&self) -> &[u8] {
        let Some((_, remainder)) = self.as_bytes_with_nul().split_last() else {
            // Originally we would panic here, but really anything is valid
            return &[];
        };
        remainder
    }

    #[inline]
    pub fn as_bytes_mut(&mut self) -> &mut [u8] {
        unsafe { slice::from_raw_parts_mut(self.0.data().as_ptr(), (self.len() - 1) as usize) }
    }

    /// Returns the bytes of the text field with the null terminator
    #[inline]
    pub const fn as_bytes_with_nul(&self) -> &[u8] {
        unsafe { slice::from_raw_parts(self.0.data().as_ptr().cast_const(), self.len() as usize) }
    }

    #[inline]
    pub const fn as_str(&self) -> Result<&str, Utf8Error> {
        core::str::from_utf8(self.as_bytes())
    }

    #[inline]
    pub const unsafe fn as_str_unchecked(&self) -> &str {
        core::str::from_utf8_unchecked(self.as_bytes())
    }

    #[inline]
    pub fn as_str_mut(&mut self) -> Result<&mut str, Utf8Error> {
        core::str::from_utf8_mut(self.as_bytes_mut())
    }

    #[inline]
    pub unsafe fn as_str_unchecked_mut(&mut self) -> &mut str {
        core::str::from_utf8_unchecked_mut(self.as_bytes_mut())
    }
}

impl<'a> AsRef<ptr::Builder<'a>> for Builder<'a> {
    #[inline]
    fn as_ref(&self) -> &ptr::Builder<'a> {
        &self.0
    }
}

impl PartialEq<Builder<'_>> for Builder<'_> {
    fn eq(&self, other: &Builder<'_>) -> bool {
        self.as_bytes() == other.as_bytes()
    }
}

impl PartialEq<str> for Builder<'_> {
    fn eq(&self, other: &str) -> bool {
        self.as_bytes() == other.as_bytes()
    }
}

impl PartialEq<Builder<'_>> for str {
    fn eq(&self, other: &Builder<'_>) -> bool {
        self.as_bytes() == other.as_bytes()
    }
}
