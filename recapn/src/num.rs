//! 29-bit and 30-bit integers used throughout the library.

use core::cmp;
use core::fmt;
use core::num::NonZeroU32;

/// A simple macro to implement cmp traits using the inner type gotten through a get() function
macro_rules! get_cmp {
    ($ty1:ty, $ty2:ty) => {
        impl PartialEq<$ty1> for $ty2 {
            fn eq(&self, other: &$ty1) -> bool {
                self.get().eq(&other.get())
            }
        }

        impl PartialOrd<$ty1> for $ty2 {
            fn partial_cmp(&self, other: &$ty1) -> Option<cmp::Ordering> {
                self.get().partial_cmp(&other.get())
            }
        }
    };
}

macro_rules! fwd_fmt_traits {
    ($ty:ty) => {
        fwd_fmt_traits!(Binary, $ty);
        fwd_fmt_traits!(Debug, $ty);
        fwd_fmt_traits!(Display, $ty);
        fwd_fmt_traits!(LowerExp, $ty);
        fwd_fmt_traits!(LowerHex, $ty);
        fwd_fmt_traits!(Octal, $ty);
        fwd_fmt_traits!(UpperExp, $ty);
        fwd_fmt_traits!(UpperHex, $ty);
    };
    ($trait:ident, $ty:ty) => {
        impl fmt::$trait for $ty {
            fn fmt(&self, f: &mut fmt::Formatter) -> fmt::Result {
                fmt::$trait::fmt(&self.get(), f)
            }
        }
    }
}

/// A 29 bit integer. It cannot be zero.
#[derive(Clone, Copy, PartialEq, Eq, PartialOrd, Ord)]
pub struct NonZeroU29(NonZeroU32);

impl NonZeroU29 {
    pub const MIN_VALUE: u32 = 1;
    pub const MAX_VALUE: u32 = 2u32.pow(29) - 1;

    pub const MIN: Self = Self(NonZeroU32::new(Self::MIN_VALUE).unwrap());
    pub const MAX: Self = Self(NonZeroU32::new(Self::MAX_VALUE).unwrap());

    pub const ONE: Self = Self::MIN;

    #[inline]
    pub const fn new(n: u32) -> Option<Self> {
        if n >= Self::MIN_VALUE && n <= Self::MAX_VALUE {
            Some(unsafe { Self::new_unchecked(n) })
        } else {
            None
        }
    }

    #[inline]
    pub const unsafe fn new_unchecked(n: u32) -> Self {
        Self(NonZeroU32::new_unchecked(n))
    }

    #[inline]
    pub const fn get(self) -> u32 {
        self.0.get()
    }
}

fwd_fmt_traits!(NonZeroU29);

/// A 29 bit integer.
#[derive(Clone, Copy, PartialEq, Eq, PartialOrd, Ord)]
#[allow(non_camel_case_types)]
pub struct u29(u32);

impl u29 {
    pub const MIN_VALUE: u32 = 0;
    pub const MAX_VALUE: u32 = 2u32.pow(29) - 1;

    pub const MIN: Self = Self(Self::MIN_VALUE);
    pub const MAX: Self = Self(Self::MAX_VALUE);

    pub const ZERO: Self = Self::MIN;

    #[inline]
    pub const fn new(n: u32) -> Option<Self> {
        if n >= Self::MIN_VALUE && n <= Self::MAX_VALUE {
            Some(unsafe { Self::new_unchecked(n) })
        } else {
            None
        }
    }

    #[inline]
    pub const unsafe fn new_unchecked(n: u32) -> Self {
        Self(n)
    }

    #[inline]
    pub const fn get(self) -> u32 {
        self.0
    }
}

fwd_fmt_traits!(u29);

impl From<NonZeroU29> for u29 {
    fn from(value: NonZeroU29) -> Self {
        Self(value.get())
    }
}

impl From<u29> for u32 {
    fn from(v: u29) -> Self {
        v.get()
    }
}

impl From<Option<NonZeroU29>> for u29 {
    fn from(value: Option<NonZeroU29>) -> Self {
        match value {
            Some(value) => Self(value.get()),
            None => Self(0),
        }
    }
}

impl From<u16> for u29 {
    fn from(v: u16) -> Self {
        Self(v as u32)
    }
}

impl From<u29> for i30 {
    fn from(u29(v): u29) -> Self {
        Self(v as i32)
    }
}

get_cmp!(NonZeroU29, u29);
get_cmp!(u29, NonZeroU29);

/// A 30 bit signed integer describing the offset of data in a segment in words.
#[derive(Clone, Copy, PartialEq, Eq, PartialOrd, Ord)]
#[allow(non_camel_case_types)]
pub struct i30(i32);

impl i30 {
    pub const MIN_VALUE: i32 = (2i32.pow(30) / 2) * -1;
    pub const MAX_VALUE: i32 = (2i32.pow(30) / 2) - 1;

    pub const MIN: Self = Self(Self::MIN_VALUE);
    pub const MAX: Self = Self(Self::MAX_VALUE);

    #[inline]
    pub const fn new(n: i32) -> Option<Self> {
        if n >= Self::MIN_VALUE && n <= Self::MAX_VALUE {
            Some(unsafe { Self::new_unchecked(n) })
        } else {
            None
        }
    }

    #[inline]
    pub const unsafe fn new_unchecked(n: i32) -> Self {
        Self(n)
    }

    #[inline]
    pub const fn get(self) -> i32 {
        self.0
    }
}

fwd_fmt_traits!(i30);

impl From<u16> for i30 {
    fn from(v: u16) -> Self {
        Self(v as i32)
    }
}

impl From<i16> for i30 {
    fn from(v: i16) -> Self {
        Self(v as i32)
    }
}
