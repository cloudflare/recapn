//! Segment allocation APIs and other primitives.

use alloc_crate::alloc::{self, Layout};
use core::cmp;
use core::fmt::{self, Debug};
use core::marker::PhantomData;
use core::num::NonZeroU32;
use core::ops::Range;
use core::ptr::{self, NonNull};

/// An aligned 64-bit word type.
#[derive(Copy, Clone, PartialEq, Eq, PartialOrd, Ord, Hash)]
#[repr(transparent)]
pub struct Word(pub u64);

impl Word {
    /// A null value
    pub const NULL: Self = Self(0);

    /// The number of bytes in a Word (8)
    pub const BYTES: usize = 8;
    /// The number of bits in a word (64)
    pub const BITS: usize = Word::BYTES * 8;

    /// Returns whether this word is null
    #[inline]
    pub const fn is_null(self) -> bool {
        matches!(self, Self::NULL)
    }

    /// Returns a static reference to a null word.
    #[inline]
    pub const fn null() -> &'static Word {
        &Word::NULL
    }

    /// Round up a bit count to words.
    #[inline]
    pub const fn round_up_bit_count(bits: u64) -> u32 {
        // The maximum object size is 4GB - 1 byte. If measured in bits,
        // this would overflow a 32-bit counter, so we need to accept
        // u64. However, 32 bits is enough for the returned
        // byte counts and word counts.
        ((bits + 63) / 64u64) as u32
    }

    /// Round up a byte count to words.
    #[inline]
    pub const fn round_up_byte_count(bytes: u32) -> u32 {
        (bytes + 7) / 8
    }

    /// Converts a slice of Words into a slice of bytes
    #[inline]
    pub fn slice_to_bytes<'a>(slice: &'a [Word]) -> &'a [u8] {
        let (_, bytes, _) = unsafe { slice.align_to() };
        bytes
    }

    /// Converts a slice of Words into a slice of bytes
    #[inline]
    pub fn slice_to_bytes_mut<'a>(slice: &'a mut [Word]) -> &'a mut [u8] {
        let (_, bytes, _) = unsafe { slice.align_to_mut() };
        bytes
    }
}

impl Debug for Word {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        write!(f, "{:#016X}", self.0)
    }
}

impl Default for Word {
    fn default() -> Self {
        Word::NULL
    }
}

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

/// A 29 bit integer. It cannot be zero.
#[derive(Debug, Clone, Copy, PartialEq, Eq, PartialOrd, Ord)]
pub struct NonZeroU29(NonZeroU32);

impl NonZeroU29 {
    pub const MIN_VALUE: u32 = 1;
    pub const MAX_VALUE: u32 = 2u32.pow(29) - 1;

    pub const MIN: Self = Self(NonZeroU32::new(Self::MIN_VALUE).unwrap());
    pub const MAX: Self = Self(NonZeroU32::new(Self::MAX_VALUE).unwrap());

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

/// The length of an allocation made in a message. This cannot be zero.
pub type AllocLen = NonZeroU29;

/// A 29 bit integer.
#[derive(Debug, Clone, Copy, PartialEq, Eq, PartialOrd, Ord)]
#[allow(non_camel_case_types)]
pub struct u29(u32);

impl u29 {
    pub const MIN_VALUE: u32 = 0;
    pub const MAX_VALUE: u32 = 2u32.pow(29) - 1;

    pub const MIN: Self = Self(Self::MIN_VALUE);
    pub const MAX: Self = Self(Self::MAX_VALUE);

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

/// The length of a segment in a message
pub type SegmentLen = u29;

/// The length of an object (pointer, struct, list) within a message
pub type ObjectLen = u29;
pub type ObjectLenBytes = u32;

/// An offset from the start of a segment to an object within it.
pub type SegmentOffset = u29;
pub type SegmentOffsetBytes = u32;

pub type ElementCount = u29;

get_cmp!(NonZeroU29, u29);
get_cmp!(u29, NonZeroU29);

/// A 30 bit signed integer describing the offset of data in a segment in words.
#[derive(Debug, Clone, Copy, PartialEq, Eq, PartialOrd, Ord)]
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

pub type SignedSegmentOffset = i30;

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

/// A segment of Words.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct Segment {
    data: NonNull<Word>,
    len: SegmentLen,
}

impl Segment {
    #[inline]
    pub const fn new(data: NonNull<Word>, len: SegmentLen) -> Self {
        Self { data, len }
    }

    #[inline]
    pub const fn data(&self) -> NonNull<Word> {
        self.data
    }

    #[inline]
    pub const fn to_ptr_range(&self) -> Range<*const Word> {
        let start = self.data.as_ptr() as *const Word;
        let end = unsafe { start.add(self.len.get() as usize) };
        start..end
    }

    #[inline]
    pub const fn len(&self) -> SegmentLen {
        self.len
    }

    #[inline]
    pub const fn len_as_bytes(&self) -> usize {
        self.len.get() as usize * 8
    }

    #[inline]
    pub unsafe fn as_slice(&self) -> &[Word] {
        core::slice::from_raw_parts(self.data.as_ptr() as *const _, self.len.get() as usize)
    }

    #[inline]
    pub unsafe fn as_mut_slice(&mut self) -> &mut [Word] {
        core::slice::from_raw_parts_mut(self.data.as_ptr() as *mut _, self.len.get() as usize)
    }

    #[inline]
    pub unsafe fn as_bytes(&self) -> &[u8] {
        Word::slice_to_bytes(self.as_slice())
    }

    #[inline]
    pub unsafe fn as_mut_bytes(&mut self) -> &mut [u8] {
        Word::slice_to_bytes_mut(self.as_mut_slice())
    }
}

/// An allocation trait that can be used to get a builder to allocate segments for an arena.
pub unsafe trait Alloc {
    /// Allocates a [`Segment`] of memory that can fit at least `size` [`Word`]s.
    /// The returned segment must be zeroed.
    ///
    /// [`Word`]: Word
    /// [`Segment`]: Segment
    unsafe fn alloc(&mut self, size: AllocLen) -> Segment;

    /// Deallocs the segment. This segment must be previously allocated by this allocator.
    unsafe fn dealloc(&mut self, segment: Segment);
}

/// An allocator that uses the global allocator to allocate segments.
///
/// You probably don't want to use this allocator directly, since it will
/// always allocate the minimum size requested. Instead you probably want
/// to layer another allocator on top, like a Growing or Fixed allocator.
#[derive(Default, Clone, Copy, Debug)]
pub struct Global;

unsafe impl Alloc for Global {
    #[inline]
    unsafe fn alloc(&mut self, size: AllocLen) -> Segment {
        let layout = Layout::array::<Word>(size.get() as usize).expect("Segment too large!");
        let ptr = alloc::alloc_zeroed(layout);
        if let Some(ptr) = NonNull::new(ptr) {
            Segment::new(ptr.cast(), size.into())
        } else {
            alloc::handle_alloc_error(layout);
        }
    }
    #[inline]
    unsafe fn dealloc(&mut self, segment: Segment) {
        let layout =
            Layout::array::<Word>(segment.len().get() as usize).expect("Segment too large!");
        alloc::dealloc(segment.data.as_ptr().cast(), layout)
    }
}

/// An allocation layer that prefers to allocate the same amount of space for each
/// segment (unless larger segments are required).
#[derive(Debug)]
pub struct Fixed<A> {
    size: AllocLen,
    inner: A,
}

impl<A> Fixed<A> {
    #[inline]
    pub const fn new(size: AllocLen, inner: A) -> Self {
        Self { size, inner }
    }
}

unsafe impl<A: Alloc> Alloc for Fixed<A> {
    #[inline]
    unsafe fn alloc(&mut self, size: AllocLen) -> Segment {
        self.inner.alloc(cmp::max(size, self.size))
    }
    #[inline]
    unsafe fn dealloc(&mut self, segment: Segment) {
        self.inner.dealloc(segment)
    }
}

/// An allocation layer that will grow the allocation size for each segment on the
/// assumption that message sizes are exponentially distributed.
#[derive(Debug)]
pub struct Growing<A> {
    next_segment: AllocLen,
    inner: A,
}

impl<A> Growing<A> {
    /// The default length used for the first segment of 1024 words (8 kibibytes).
    pub const DEFAULT_FIRST_SEGMENT_LEN: AllocLen = AllocLen::new(1024).unwrap();

    #[inline]
    pub const fn new(first_segment: AllocLen, alloc: A) -> Self {
        Self {
            next_segment: first_segment,
            inner: alloc,
        }
    }
}

impl<A: Default> Default for Growing<A> {
    fn default() -> Self {
        Self::new(Self::DEFAULT_FIRST_SEGMENT_LEN, A::default())
    }
}

unsafe impl<A: Alloc> Alloc for Growing<A> {
    #[inline]
    unsafe fn alloc(&mut self, size: AllocLen) -> Segment {
        let alloc_size = cmp::max(size, self.next_segment);

        let next_size = self.next_segment.get().saturating_add(alloc_size.get());
        self.next_segment = AllocLen::new(next_size).unwrap_or(AllocLen::MAX);
        self.inner.alloc(alloc_size)
    }
    #[inline]
    unsafe fn dealloc(&mut self, segment: Segment) {
        self.inner.dealloc(segment)
    }
}

/// Scratch space that can be passed to a [Scratch] allocator.
pub struct Space<const N: usize>([Word; N]);

impl<const N: usize> Space<N> {
    #[inline]
    pub const fn new() -> Self {
        assert!(N > 0, "Scratch space is empty!");
        assert!(
            N <= SegmentLen::MAX_VALUE as usize,
            "Scratch space is too large!"
        );

        Self([Word::NULL; N])
    }
}

/// A simple alias that makes it easy to create scratch space.
/// 
/// # Example
/// 
/// ```
/// use recapn::alloc;
/// use recapn::message::Message;
/// 
/// let mut space = alloc::space::<16>();
/// let message = Message::with_scratch(&mut space);
/// # drop(message);
/// ```
#[inline]
pub const fn space<const N: usize>() -> Space<N> { Space::new() }

/// Scratch space with a dynamic length that can be passed to a [Scratch] allocator.
pub struct DynSpace(Box<[Word]>);

impl DynSpace {
    #[inline]
    pub fn new(len: SegmentLen) -> Self {
        DynSpace(vec![Word::NULL; len.get() as usize].into_boxed_slice())
    }
}

/// An allocation layer that first attempts to allocate to some borrowed scratch space before
/// falling back to another allocator.
#[derive(Debug)]
pub struct Scratch<'s, A> {
    s: PhantomData<&'s mut [Word]>,
    segment: Segment,
    used: bool,
    next: A,
}

impl<'s, A> Scratch<'s, A> {
    /// Creates a new scratch space allocator for the given statically allocated space.
    #[inline]
    pub fn with_space<const N: usize>(space: &'s mut Space<N>, alloc: A) -> Self {
        let size = AllocLen::new(N as u32).unwrap();
        let start = NonNull::new(space.0.as_mut_ptr()).unwrap();

        Self {
            s: PhantomData,
            segment: Segment::new(start, size.into()),
            used: false,
            next: alloc,
        }
    }

    /// Creates a new scratch space allocator for the given dynamically allocated space.
    #[inline]
    pub fn with_dyn_space(space: &'s mut DynSpace, alloc: A) -> Self {
        let size = AllocLen::new(space.0.len() as u32).unwrap();
        let start = NonNull::new(space.0.as_mut_ptr()).unwrap();

        Self {
            s: PhantomData,
            segment: Segment::new(start, size.into()),
            used: false,
            next: alloc,
        }
    }

    /// Creates a new scratch space allocator with the given segment. This effectively allows
    /// you to allocate and manage scratch space anywhere.
    /// 
    /// # Safety
    /// 
    /// The segment data must last as long as the lifetime of the scratch allocator, otherwise
    /// this allocator may exhibit **undefined behavior**.
    #[inline]
    pub unsafe fn with_segment(segment: Segment, alloc: A) -> Self {
        Self {
            s: PhantomData,
            segment,
            used: false,
            next: alloc,
        }
    }
}

unsafe impl<A: Alloc> Alloc for Scratch<'_, A> {
    #[inline]
    unsafe fn alloc(&mut self, size: AllocLen) -> Segment {
        if self.used || self.segment.len() < size {
            self.next.alloc(size)
        } else {
            self.used = true;
            self.segment.clone()
        }
    }
    #[inline]
    unsafe fn dealloc(&mut self, mut segment: Segment) {
        if segment != self.segment {
            self.next.dealloc(segment)
        } else {
            let bytes = segment.as_mut_bytes();
            ptr::write_bytes(bytes.as_mut_ptr(), 0, bytes.len())
        }
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn word_rounding() {
        // bit rounding
        assert_eq!(Word::round_up_bit_count(1), 1);
        assert_eq!(Word::round_up_bit_count(63), 1);
        assert_eq!(Word::round_up_bit_count(64), 1);
        assert_eq!(Word::round_up_bit_count(65), 2);
        assert_eq!(Word::round_up_bit_count(64 * 2), 2);
        assert_eq!(Word::round_up_bit_count(64 * 2 + 1), 3);
        assert_eq!(
            Word::round_up_bit_count((ObjectLen::MAX.get() as u64) * (Word::BITS as u64)),
            536870911
        );

        // byte rounding
        assert_eq!(Word::round_up_byte_count(1), 1);
        assert_eq!(Word::round_up_byte_count(7), 1);
        assert_eq!(Word::round_up_byte_count(8), 1);
        assert_eq!(Word::round_up_byte_count(9), 2);
        assert_eq!(Word::round_up_byte_count(8 * 2), 2);
        assert_eq!(Word::round_up_byte_count(8 * 2 + 1), 3);
    }

    macro_rules! alloc {
        ($alloc:expr, $size:expr, $func:expr) => {{
            let alloc = &mut $alloc;
            unsafe {
                let segment = alloc.alloc(AllocLen::new($size).unwrap());
                $func(segment.clone());
                alloc.dealloc(segment);
            }
        }};
    }

    #[test]
    fn global_alloc() {
        let mut alloc = Global;
        alloc!(alloc, 5, |segment: Segment| {
            assert_eq!(segment.as_slice(), &[Word::NULL; 5]);
        });
    }

    #[test]
    fn fixed_alloc() {
        let mut fixed = Fixed::new(AllocLen::new(5).unwrap(), Global);
        alloc!(fixed, 1, |segment: Segment| {
            assert_eq!(segment.as_slice(), &[Word::NULL; 5]);
        });
        alloc!(fixed, 7, |segment: Segment| {
            assert_eq!(segment.as_slice(), &[Word::NULL; 7]);
        });
    }

    #[test]
    fn growing_alloc() {
        let mut growing = Growing::new(AllocLen::MIN, Global);
        alloc!(growing, 1, |segment: Segment| {
            assert_eq!(segment.as_slice(), &[Word::NULL; 1]);
        });
        // next one will be 2
        alloc!(growing, 1, |segment: Segment| {
            assert_eq!(segment.as_slice(), &[Word::NULL; 2]);
        });
        // next one would be 4, but we pass in 5, and since the requested segment
        // is bigger, that's what it will use
        alloc!(growing, 5, |segment: Segment| {
            assert_eq!(segment.as_slice(), &[Word::NULL; 5]);
        });
        // next one would be 9
        alloc!(growing, 1, |segment: Segment| {
            assert_eq!(segment.as_slice(), &[Word::NULL; 9]);
        });
    }

    #[test]
    fn scratch_alloc() {
        let mut static_space = space::<5>();
        let mut addr = 0;
        {
            let mut scratch = Scratch::with_space(&mut static_space, Global);
            alloc!(scratch, 1, |mut segment: Segment| {
                addr = segment.data().as_ptr() as usize;
                assert_eq!(segment.as_slice(), &[Word::NULL; 5]);
                segment.as_mut_bytes().fill(255);
            });
            // next one won't use scratch space
            alloc!(scratch, 1, |segment: Segment| {
                assert_eq!(segment.as_slice(), &[Word::NULL; 1]);
            });
        }
        assert_eq!(&static_space as *const _ as usize, addr);
        // make sure the scratch allocator cleared the scratch space
        assert_eq!(static_space.0, [Word::NULL; 5]);
    }
}
