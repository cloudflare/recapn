//! Segment allocation APIs and other primitives.

use core::cmp;
use core::fmt::{self, Debug};
use core::marker::PhantomData;
use core::ops::Range;
use core::ptr::NonNull;
use crate::num::{u29, NonZeroU29, i30};

#[cfg(feature = "alloc")]
use rustalloc::{
    alloc::{self, Layout},
    boxed::Box,
    vec,
};

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
        bits.div_ceil(64) as u32
    }

    /// Round up a byte count to words.
    #[inline]
    pub const fn round_up_byte_count(bytes: u32) -> u32 {
        bytes.div_ceil(8)
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

/// The length of a segment in a message
pub type SegmentLen = u29;

/// A segment of Words. This can be conceived of as a slice of [`Word`]s but without an
/// attached lifetime. Many lower level APIs use this type and have different safety
/// requirements depending on the use-case.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct Segment {
    pub data: NonNull<Word>,
    pub len: SegmentLen,
}

impl Segment {
    #[inline]
    pub const fn to_ptr_range(&self) -> Range<*const Word> {
        let start = self.data.as_ptr() as *const Word;
        let end = start.wrapping_add(self.len.get() as usize);
        start..end
    }

    #[inline]
    pub const fn len_as_bytes(&self) -> usize {
        self.len.get() as usize * 8
    }

    #[inline]
    pub unsafe fn as_slice<'a>(&self) -> &'a [Word] {
        core::slice::from_raw_parts(self.data.as_ptr() as *const _, self.len.get() as usize)
    }

    #[inline]
    pub unsafe fn as_mut_slice<'a>(&mut self) -> &'a mut [Word] {
        core::slice::from_raw_parts_mut(self.data.as_ptr() as *mut _, self.len.get() as usize)
    }

    #[inline]
    pub unsafe fn as_bytes<'a>(&self) -> &'a [u8] {
        Word::slice_to_bytes(self.as_slice())
    }

    #[inline]
    pub unsafe fn as_mut_bytes<'a>(&mut self) -> &'a mut [u8] {
        Word::slice_to_bytes_mut(self.as_mut_slice())
    }
}

/// An offset from the start of a segment to an object within it.
pub type SegmentOffset = u29;
pub type SignedSegmentOffset = i30;

/// An unchecked pointer into a segment. This can't be derefed normally. It must be checked and
/// turned into a `SegmentRef`.
#[derive(Clone, Copy, Debug, PartialEq, Eq, PartialOrd, Ord)]
pub(crate) struct SegmentPtr<'a> {
    a: PhantomData<&'a Word>,
    ptr: *mut Word,
}

impl<'a> SegmentPtr<'a> {
    pub const fn new(ptr: *mut Word) -> Self {
        Self {
            a: PhantomData,
            ptr,
        }
    }

    pub const fn as_ptr(self) -> *const Word {
        self.ptr.cast_const()
    }

    pub const fn as_ptr_mut(self) -> *mut Word {
        self.ptr
    }

    pub const fn offset(self, offset: SegmentOffset) -> Self {
        Self::new(self.ptr.wrapping_add(offset.get() as usize))
    }

    pub const fn signed_offset(self, offset: SignedSegmentOffset) -> Self {
        Self::new(self.ptr.wrapping_offset(offset.get() as isize))
    }

    pub const fn signed_offset_from_end(self, offset: SignedSegmentOffset) -> Self {
        const ONE: SignedSegmentOffset = SignedSegmentOffset::new(1).unwrap();

        self.signed_offset(ONE).signed_offset(offset)
    }

    pub const unsafe fn as_ref_unchecked(self) -> SegmentRef<'a> {
        SegmentRef {
            a: PhantomData,
            ptr: NonNull::new_unchecked(self.ptr),
        }
    }
}

/// A checked reference to a location in a segment. This cannot be null.
#[derive(Clone, Copy, Debug, PartialEq, Eq, PartialOrd, Ord)]
pub(crate) struct SegmentRef<'a> {
    a: PhantomData<&'a Word>,
    ptr: NonNull<Word>,
}

impl<'a> SegmentRef<'a> {
    /// Creates a dangling ref. This does not refer to any valid word, so it must not be
    /// derefed. Useful for zero-sized types such as empty lists or empty structs.
    #[inline]
    pub const unsafe fn dangling() -> Self {
        Self::new_unchecked(NonNull::dangling())
    }

    #[inline]
    pub const unsafe fn new_unchecked(ptr: NonNull<Word>) -> Self {
        Self {
            a: PhantomData,
            ptr,
        }
    }

    #[inline]
    pub const fn as_segment_ptr(self) -> SegmentPtr<'a> {
        SegmentPtr::new(self.as_ptr_mut())
    }

    #[inline]
    pub const fn as_ptr(self) -> *const Word {
        self.ptr.as_ptr().cast_const()
    }

    #[inline]
    pub const fn as_ptr_mut(self) -> *mut Word {
        self.ptr.as_ptr()
    }

    #[inline]
    pub const fn as_inner(self) -> NonNull<Word> {
        self.ptr
    }

    #[inline]
    pub const fn offset(self, offset: SegmentOffset) -> SegmentPtr<'a> {
        self.as_segment_ptr().offset(offset)
    }

    #[inline]
    pub const fn signed_offset(self, offset: SignedSegmentOffset) -> SegmentPtr<'a> {
        self.as_segment_ptr().signed_offset(offset)
    }

    #[inline]
    pub const fn signed_offset_from_end(self, offset: SignedSegmentOffset) -> SegmentPtr<'a> {
        self.as_segment_ptr().signed_offset_from_end(offset)
    }
}

impl AsRef<Word> for SegmentRef<'_> {
    #[inline]
    fn as_ref(&self) -> &Word {
        unsafe { self.as_inner().as_ref() }
    }
}

impl From<SegmentRef<'_>> for SegmentPtr<'_> {
    #[inline]
    fn from(value: SegmentRef<'_>) -> Self {
        Self::new(value.as_ptr_mut())
    }
}

/// The length of an allocation made in a message. This cannot be zero.
pub type AllocLen = NonZeroU29;

/// An allocation trait that can be used to get a builder to allocate segments for an arena.
pub unsafe trait Alloc {
    /// Allocates a [`Segment`] of memory that can fit at least `size` [`Word`]s.
    /// The returned segment must be zeroed.
    ///
    /// If the segment cannot be allocated, `None` is returned instead.
    unsafe fn alloc(&mut self, size: AllocLen) -> Option<Segment>;

    /// Deallocs the segment. This segment must be previously allocated by this allocator.
    unsafe fn dealloc(&mut self, segment: Segment);
}

/// An allocator that never allocates, it always returns None.
///
/// This can be useful as a backing allocator to a Scratch allocator to make sure
/// nothing is ever allocated.
#[derive(Default, Clone, Copy, Debug)]
pub struct Never;

unsafe impl Alloc for Never {
    #[inline]
    unsafe fn alloc(&mut self, _: AllocLen) -> Option<Segment> {
        None
    }
    #[inline]
    unsafe fn dealloc(&mut self, _: Segment) {
        unimplemented!()
    }
}

/// An allocator that uses the global allocator to allocate segments.
///
/// You probably don't want to use this allocator directly, since it will
/// always allocate the minimum size requested. Instead you probably want
/// to layer another allocator on top, like a Growing or Fixed allocator.
#[derive(Default, Clone, Copy, Debug)]
#[cfg(feature = "alloc")]
pub struct Global;

#[cfg(feature = "alloc")]
unsafe impl Alloc for Global {
    #[inline]
    unsafe fn alloc(&mut self, size: AllocLen) -> Option<Segment> {
        let layout = Layout::array::<Word>(size.get() as usize).ok()?;
        let ptr = alloc::alloc_zeroed(layout);
        let ptr = NonNull::new(ptr)?;
        Some(Segment { data: ptr.cast(), len: size.into() })
    }
    #[inline]
    unsafe fn dealloc(&mut self, segment: Segment) {
        let layout =
            Layout::array::<Word>(segment.len.get() as usize).expect("Segment too large!");
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
    unsafe fn alloc(&mut self, size: AllocLen) -> Option<Segment> {
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
    unsafe fn alloc(&mut self, size: AllocLen) -> Option<Segment> {
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
pub struct Space<const N: usize> {
    /// Indicates if this space has been used and needs to be cleared.
    dirty: bool,
    space: [Word; N],
}

impl<const N: usize> Space<N> {
    #[inline]
    pub const fn new() -> Self {
        assert!(N > 0, "Scratch space is empty!");
        assert!(
            N <= SegmentLen::MAX_VALUE as usize,
            "Scratch space is too large!"
        );

        Self {
            dirty: false,
            space: [Word::NULL; N],
        }
    }

    #[inline]
    pub(crate) fn segment(&mut self) -> Segment {
        if self.dirty {
            self.space.fill(Word::NULL);
        }

        self.dirty = true;
        let len = AllocLen::new(N as u32).unwrap().into();
        let data = NonNull::new(self.space.as_mut_ptr()).unwrap();

        Segment { data, len }
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
#[cfg(feature = "alloc")]
pub struct DynSpace {
    dirty: bool,
    space: Box<[Word]>,
}

#[cfg(feature = "alloc")]
impl DynSpace {
    #[inline]
    pub fn new(len: AllocLen) -> Self {
        let space = vec![Word::NULL; len.get() as usize].into_boxed_slice();
        DynSpace { dirty: false, space }
    }

    #[inline]
    pub(crate) fn segment(&mut self) -> Segment {
        if self.dirty {
            self.space.fill(Word::NULL);
        }

        self.dirty = true;
        let len = AllocLen::new(self.space.len() as u32).unwrap().into();
        let data = NonNull::new(self.space.as_mut_ptr()).unwrap();

        Segment { data, len }
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
        Self {
            s: PhantomData,
            segment: space.segment(),
            used: false,
            next: alloc,
        }
    }

    /// Creates a new scratch space allocator for the given dynamically allocated space.
    #[inline]
    #[cfg(feature = "alloc")]
    pub fn with_dyn_space(space: &'s mut DynSpace, alloc: A) -> Self {
        Self {
            s: PhantomData,
            segment: space.segment(),
            used: false,
            next: alloc,
        }
    }

    /// Creates a new scratch space allocator with the given segment. This effectively allows
    /// you to allocate and manage scratch space anywhere.
    /// 
    /// # Safety
    /// 
    /// The segment data must last as long as the lifetime of the scratch allocator and be
    /// completely zeroed, otherwise this allocator may exhibit **undefined behavior**.
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
    unsafe fn alloc(&mut self, size: AllocLen) -> Option<Segment> {
        if self.used || self.segment.len < size {
            self.next.alloc(size)
        } else {
            self.used = true;
            Some(self.segment.clone())
        }
    }
    #[inline]
    unsafe fn dealloc(&mut self, segment: Segment) {
        if segment != self.segment {
            self.next.dealloc(segment)
        }
    }
}

#[cfg(test)]
#[cfg(feature = "alloc")]
mod tests {
    use super::*;
    use crate::ptr::ObjectLen;

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
                let segment = alloc.alloc(AllocLen::new($size).unwrap()).unwrap();
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
        let mut growing = Growing::new(AllocLen::ONE, Global);
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
                addr = segment.data.as_ptr() as usize;
                assert_eq!(segment.as_slice(), &[Word::NULL; 5]);
                segment.as_mut_bytes().fill(255);
            });
            // next one won't use scratch space
            alloc!(scratch, 1, |segment: Segment| {
                assert_eq!(segment.as_slice(), &[Word::NULL; 1]);
            });
        }
        assert_eq!(&static_space as *const _ as usize, addr);
    }
}
