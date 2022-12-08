use core::str::Utf8Error;

use crate::Family;

pub mod ptr {
    pub use crate::ptr::{
        TextReader as Reader,
        TextBuilder as Builder,
    };
}

pub type Reader<'a> = Text<ptr::Reader<'a>>;
pub type Builder<'a> = Text<ptr::Builder<'a>>;

pub struct Text<T = Family> {
    repr: T,
}

impl<'a> Reader<'a> {
    pub const fn empty() -> Self {
        Self { repr: raw::Reader::null() }
    }

    pub fn as_bytes(&self) -> &[u8] {
        unsafe {
            core::slice::from_raw_parts(self.repr.ptr(), (self.repr.len() - 1) as usize)
        }
    }

    pub fn as_bytes_with_nul(&self) -> &[u8] {
        unsafe {
            core::slice::from_raw_parts(self.repr.ptr(), self.repr.len() as usize)
        }
    }

    pub fn as_str(&self) -> Result<&str, Utf8Error> {
        core::str::from_utf8(self.as_bytes())
    }

    pub unsafe fn as_str_unchecked(&self) -> &str {
        core::str::from_utf8_unchecked(self.as_bytes())
    }
}

impl<'a> From<raw::Reader<'a>> for Reader<'a> {
    fn from(repr: raw::Reader<'a>) -> Self {
        Self { repr }
    }
}