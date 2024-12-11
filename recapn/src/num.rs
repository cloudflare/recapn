//! 29-bit and 30-bit integers used throughout the library.

use core::cmp;
use core::fmt;
use core::hint::unreachable_unchecked;
use core::num::NonZeroU32;

#[derive(Debug, Copy, Clone, PartialEq, Eq)]
pub struct TryFromIntError(pub(crate) ());

impl fmt::Display for TryFromIntError {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        f.write_str("out of range integral type conversion attempted")
    }
}

impl core::error::Error for TryFromIntError {}

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
            fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
                fmt::$trait::fmt(&self.get(), f)
            }
        }
    };
}

macro_rules! from_int_impl {
    ($dst:ty: $in:ty => $($src:ty),*) => {$(
        impl From<$src> for $dst {
            #[inline]
            fn from(value: $src) -> $dst {
                let inner = <$in as From<$src>>::from(value);
                <$dst>::new_unwrap(inner)
            }
        }
    )*};
}

macro_rules! try_from_int_impl {
    ($dst:ty: $in:ty => $($src:ty),*) => {$(
        impl TryFrom<$src> for $dst {
            type Error = TryFromIntError;

            #[inline]
            fn try_from(value: $src) -> Result<$dst, TryFromIntError> {
                let inner = <$in as TryFrom<$src>>::try_from(value)
                    .map_err(|_| TryFromIntError(()))?;
                <$dst>::new(inner).ok_or(TryFromIntError(()))
            }
        }
    )*};
}

/// A 29 bit integer. It cannot be zero.
#[derive(Clone, Copy, PartialEq, Eq, PartialOrd, Ord)]
pub struct NonZeroU29(NonZeroU32);

impl NonZeroU29 {
    pub const MIN_VALUE: u32 = 1;
    pub const MAX_VALUE: u32 = 2u32.pow(29) - 1;

    pub const MIN: Self = Self(unsafe { NonZeroU32::new_unchecked(Self::MIN_VALUE) });
    pub const MAX: Self = Self(unsafe { NonZeroU32::new_unchecked(Self::MAX_VALUE) });

    pub const ONE: Self = Self::MIN;

    #[inline]
    pub const fn new(n: u32) -> Option<Self> {
        if n >= Self::MIN_VALUE && n <= Self::MAX_VALUE {
            match NonZeroU32::new(n) {
                Some(n) => Some(Self(n)),
                None => None,
            }
        } else {
            None
        }
    }

    #[inline]
    #[track_caller]
    pub const fn new_unwrap(n: u32) -> Self {
        let Some(s) = Self::new(n) else {
            panic!("integer value out of range")
        };
        s
    }

    #[inline]
    #[track_caller]
    pub const unsafe fn new_unchecked(n: u32) -> Self {
        match Self::new(n) {
            Some(n) => n,
            _ => {
                // actually checked with debug_assertions on
                unreachable_unchecked()
            }
        }
    }

    #[inline]
    pub const fn get(self) -> u32 {
        let val = self.0.get();
        if val >= Self::MIN_VALUE && val <= Self::MAX_VALUE {
            val
        } else {
            // hint for the optimizer
            unsafe { unreachable_unchecked() }
        }
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
            Some(Self(n))
        } else {
            None
        }
    }

    #[inline]
    #[track_caller]
    pub const fn new_unwrap(n: u32) -> Self {
        let Some(s) = Self::new(n) else {
            panic!("integer value out of range")
        };
        s
    }

    #[inline]
    #[track_caller]
    pub const unsafe fn new_unchecked(n: u32) -> Self {
        match Self::new(n) {
            Some(n) => n,
            _ => unreachable_unchecked(),
        }
    }

    #[inline]
    pub const fn get(self) -> u32 {
        if self.0 >= Self::MIN_VALUE && self.0 <= Self::MAX_VALUE {
            self.0
        } else {
            unsafe { unreachable_unchecked() }
        }
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

impl From<u29> for i30 {
    fn from(u29(v): u29) -> Self {
        Self(v as i32)
    }
}

from_int_impl!(u29: u32 => u8, u16);
try_from_int_impl!(u29: u32 => i8, i16, u32, i32, u64, i64, u128, i128, usize, isize);

get_cmp!(NonZeroU29, u29);
get_cmp!(u29, NonZeroU29);

/// A 30 bit signed integer describing the offset of data in a segment in words.
#[derive(Clone, Copy, PartialEq, Eq, PartialOrd, Ord)]
#[allow(non_camel_case_types)]
pub struct i30(i32);

impl i30 {
    pub const MIN_VALUE: i32 = -(2i32.pow(30) / 2);
    pub const MAX_VALUE: i32 = (2i32.pow(30) / 2) - 1;

    pub const MIN: Self = Self(Self::MIN_VALUE);
    pub const MAX: Self = Self(Self::MAX_VALUE);

    #[inline]
    pub const fn new(n: i32) -> Option<Self> {
        if n >= Self::MIN_VALUE && n <= Self::MAX_VALUE {
            Some(Self(n))
        } else {
            None
        }
    }

    #[inline]
    #[track_caller]
    pub const fn new_unwrap(n: i32) -> Self {
        let Some(s) = Self::new(n) else {
            panic!("integer value out of range")
        };
        s
    }

    #[inline]
    #[track_caller]
    pub const unsafe fn new_unchecked(n: i32) -> Self {
        match Self::new(n) {
            Some(n) => n,
            _ => unreachable_unchecked(),
        }
    }

    #[inline]
    pub const fn get(self) -> i32 {
        if self.0 >= Self::MIN_VALUE && self.0 <= Self::MAX_VALUE {
            self.0
        } else {
            unsafe { unreachable_unchecked() }
        }
    }
}

fwd_fmt_traits!(i30);

from_int_impl!(i30: i32 => u8, i8, u16, i16);
try_from_int_impl!(i30: i32 => u32, i32, u64, i64, u128, i128, usize, isize);
