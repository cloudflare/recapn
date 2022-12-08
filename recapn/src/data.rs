use crate::Family;

pub type Reader<'a> = Data<ptr::Reader<'a>>;
pub type Builder<'a> = Data<ptr::Builder<'a>>;

pub mod ptr {
    pub use crate::ptr::{
        DataReader as Reader,
        DataBuilder as Builder,
    };
}

pub struct Data<T = Family> {
    repr: T,
}

impl<'a> Reader<'a> {
    pub const fn empty() -> Self {
        Self { repr: ptr::Reader::null() }
    }
}

impl<'a> From<ptr::Reader<'a>> for Reader<'a> {
    fn from(repr: ptr::Reader<'a>) -> Self {
        Self { repr }
    }
}