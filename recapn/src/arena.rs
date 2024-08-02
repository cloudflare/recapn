//! Types and traits for implementing custom message arenas.

use crate::alloc::{Word, AllocLen, Segment, SegmentLen, SegmentRef, SegmentOffset};
use core::cell::Cell;
use core::ptr::NonNull;

#[cfg(feature = "alloc")]
use rustalloc::boxed::Box;

/// An ID for a segment
pub type SegmentId = u32;

/// A trait for reading segments from an arena. This trait is unsafe since the returned
/// [`Segment`] is not validated and must conform to special safety constraints.
///
/// # Safety
///
/// The segments returned by this function have many of the same constraints as a slice of
/// [`Word`]s. In particular:
/// 
/// * `data` must be valid for reads for `len * 8` bytes, and it must be properly aligned.
///   This means in particular:
///   * The entire memory range of this slice must be contained within a single allocated object!
///     Slices can never span across multiple allocated objects.
/// * `data` must point to `len` consecutive properly initialized values of [`Word`].
/// * The memory referenced by the segment must have a lifetime at least as long as the arena itself.
/// * The memory referenced by the segment must not be mutated for the lifetime of the arena
///   by the arena itself. Shared references can be passed as segments through [`ReadArena`] though,
///   since the library promises to never attempt to write to a segment obtained through a
///   [`ReadArena`] instance.
/// * The total size `len * 8` of the slice must be no larger than `isize::MAX`,
///   and adding that size to data must not “wrap around” the address space. See the safety
///   documentation of `pointer::offset`.
pub unsafe trait ReadArena {
    fn segment(&self, id: SegmentId) -> Option<Segment>;
    fn size_in_words(&self) -> usize;
}

unsafe impl<'a, T: ReadArena + ?Sized> ReadArena for &'a T {
    fn segment(&self, id: SegmentId) -> Option<Segment> {
        T::segment(*self, id)
    }
    fn size_in_words(&self) -> usize {
        T::size_in_words(*self)
    }
}

#[cfg(feature = "alloc")]
unsafe impl<T: ReadArena + ?Sized> ReadArena for Box<T> {
    fn segment(&self, id: SegmentId) -> Option<Segment> {
        T::segment(&*self, id)
    }
    fn size_in_words(&self) -> usize {
        T::size_in_words(&*self)
    }
}

/// Packs a segment with an ID. This is used to more efficiently pack segment data on 64-bit
/// platforms.
#[derive(Clone, Copy, Debug)]
pub(crate) struct SegmentWithId {
    pub(crate) data: NonNull<Word>,
    pub(crate) len: SegmentLen,
    pub(crate) id: SegmentId,
}

impl SegmentWithId {
    pub fn segment(&self) -> Segment {
        Segment { data: self.data, len: self.len }
    }
}

#[derive(Debug, Clone, Copy)]
enum SegmentState {
    /// The segment is used by this amount
    Used(AllocLen),
    /// The segment is read-only and cannot be modified
    ReadOnly,
    /// The segment is deleted and should be represented as empty
    Deleted,
}

/// A segment stored in an arena. Arenas store instances of these structs to track segments
/// and hand out references to them to be built by the rest of the library.
#[derive(Debug)]
pub struct ArenaSegment {
    /// The segment packed with the ID.
    segment: SegmentWithId,
    state: Cell<SegmentState>,
}

impl ArenaSegment {
    /// Create a new arena segment.
    #[inline]
    pub unsafe fn new(segment: Segment, used_len: AllocLen, id: SegmentId) -> Self {
        Self {
            segment: SegmentWithId {
                data: segment.data,
                len: segment.len,
                id,
            },
            state: Cell::new(SegmentState::Used(used_len)),
        }
    }

    /// Create an arena segment for an external segment.
    #[inline]
    pub unsafe fn external(segment: Segment, id: SegmentId) -> Self {
        Self {
            segment: SegmentWithId {
                data: segment.data,
                len: segment.len,
                id,
            },
            state: Cell::new(SegmentState::ReadOnly),
        }
    }

    #[inline]
    pub fn is_read_only(&self) -> bool {
        matches!(self.state.get(), SegmentState::ReadOnly)
    }

    #[inline]
    fn delete(&self) {
        self.state.set(SegmentState::Deleted)
    }

    #[inline]
    pub(crate) fn segment_with_id(&self) -> SegmentWithId {
        self.segment
    }

    #[inline]
    pub fn id(&self) -> SegmentId {
        self.segment.id
    }

    #[inline]
    pub(crate) fn segment(&self) -> Segment {
        self.segment.segment()
    }

    #[inline]
    pub(crate) fn start(&self) -> SegmentRef {
        unsafe { SegmentRef::new_unchecked(self.segment.data) }
    }

    /// Attempts to use the given `len` [`Word`]s in the segment, returning
    /// a [`SegmentRef`] to the start of the area if successful.
    #[inline]
    pub(crate) fn try_use_len(&self, len: AllocLen) -> Option<SegmentOffset> {
        let SegmentState::Used(old_used) = self.state.get() else { unreachable!() };
        let max = self.segment.len.get();

        let new_used = old_used.get() + len.get();
        if new_used > max {
            return None;
        }

        self.state.set(SegmentState::Used(AllocLen::new(new_used).unwrap()));
        Some(old_used.into())
    }

    #[inline]
    pub fn used_len(&self) -> SegmentOffset {
        match self.state.get() {
            SegmentState::Used(len) => len.into(),
            SegmentState::ReadOnly => self.segment.len,
            SegmentState::Deleted => SegmentOffset::ZERO,
        }
    }

    /// Returns a segment of the used data in this segment.
    #[inline]
    pub fn used_segment(&self) -> Segment {
        Segment { data: self.segment.data, len: self.used_len() }
    }
}

pub unsafe trait BuildArena {
    fn alloc(&self, min_size: AllocLen) -> Option<(SegmentOffset, &ArenaSegment)>;
    fn segment(&self, id: SegmentId) -> Option<&ArenaSegment>;

    fn insert_external_segment(&self, segment: Segment) -> Option<SegmentId>;
    fn remove_external_segment(&self, segment: SegmentId);

    fn as_read_arena(&self) -> &dyn ReadArena;

    /// The number of segments in the arena.
    fn len(&self) -> u32;
    fn size_in_words(&self) -> usize;
}

#[cfg(feature = "alloc")]
pub(crate) mod growing {
    use super::*;
    use core::cell::UnsafeCell;
    use core::ptr::{addr_of, addr_of_mut, from_mut, from_ref};
    use rustalloc::vec::Vec;
    use crate::alloc::Alloc;

    pub struct ArenaSegments {
        /// Optimize for the first (and possibly only) segment
        first: Option<ArenaSegment>,
        /// An optional set of extra segments
        tail: Vec<NonNull<ArenaSegment>>,
    }

    /// A raw vtable for doing arena operations as a `Message`. This is done with a raw vtable
    /// instead of a normal dynamic trait because we want the `Message` type to accept an `Alloc`
    /// implementation while still being unsizable. However, unsized types cannot be coerced into
    /// other unsized types, so there's no way for us get a `ReadArena` or a `BuildArena` out of
    /// an `Arena` with an unsized alloc type.
    struct ArenaVtable {
        as_read: unsafe fn(*const ()) -> *const dyn ReadArena,
        try_root_and_build: unsafe fn(*const ()) -> Option<(*const ArenaSegment, *const dyn BuildArena)>,
        root_and_build: unsafe fn(*mut ()) -> (*const ArenaSegment, *const dyn BuildArena),
        clear: unsafe fn(*mut ()),
    }

    pub struct Arena<A: Alloc + ?Sized> {
        vtable: &'static ArenaVtable,
        segments: UnsafeCell<ArenaSegments>,
        /// The last owned segment in the set. Attempting to allocate from the arena will
        /// try to allocate from this segment first before allocating a new one.
        last_owned: Cell<SegmentId>,
        alloc: UnsafeCell<A>,
    }

    impl<A: Alloc> Arena<A> {
        const MAX_SEGMENTS: usize = (u32::MAX - 1) as usize;

        pub const fn new(alloc: A) -> Self {
            let vtable = &ArenaVtable {
                as_read: |ptr| unsafe {
                    let obj: *const dyn ReadArena = &*ptr.cast::<Self>();
                    // Transmute the implied lifetime attached to the obj dyn ReadArena.
                    // This is safe since our caller will add back in the lifetime and
                    // make an equivalent type.
                    core::mem::transmute(obj)
                },
                try_root_and_build: |ptr| unsafe {
                    let this = &*ptr.cast::<Self>();
                    let root = this.root()?;
                    let obj: *const dyn BuildArena = this;
                    let arena = core::mem::transmute(obj);
                    Some((root, arena))
                },
                root_and_build: |ptr| unsafe {
                    let this = &*ptr.cast::<Self>();
                    let root = this.alloc_root();
                    let obj: *const dyn BuildArena = this;
                    let arena = core::mem::transmute(obj);
                    (root, arena)
                },
                clear: |ptr| unsafe {
                    (*ptr.cast::<Self>()).clear_inner()
                },
            };

            Self {
                vtable,
                segments: UnsafeCell::new(ArenaSegments {
                    first: None,
                    tail: Vec::new(),
                }),
                last_owned: Cell::new(0),
                alloc: UnsafeCell::new(alloc),
            }
        }

        pub fn root(&self) -> Option<&ArenaSegment> {
            unsafe { (*self.segments.get()).first.as_ref() }
        }
        pub fn alloc_root(&self) -> &ArenaSegment {
            unsafe {
                let first = &mut (*self.segments.get()).first;
                first.get_or_insert_with(|| {
                    self.alloc(AllocLen::ONE, 0).expect("failed to allocate root segment")
                })
            }
        }
        fn alloc(&self, min_size: AllocLen, id: SegmentId) -> Option<ArenaSegment> {
            unsafe {
                let alloc = &mut *self.alloc.get();
                let segment = alloc.alloc(min_size)?;
                Some(ArenaSegment::new(segment, min_size, id))
            }
        }
        fn tail(&self) -> &Vec<NonNull<ArenaSegment>> {
            unsafe { &*addr_of!((*self.segments.get()).tail) }
        }
        fn tail_mut(&self) -> &mut Vec<NonNull<ArenaSegment>> {
            unsafe { &mut *addr_of_mut!((*self.segments.get()).tail) }
        }
        fn segment(&self, id: SegmentId) -> Option<&ArenaSegment> {
            if id == 0 {
                self.root()
            } else {
                let segment = self.tail().get((id - 1) as usize)?;
                Some(unsafe { segment.as_ref() })
            }
        }
        fn size_in_words(&self) -> usize {
            let Some(root) = self.root() else { return 0 };
            let root_len = root.used_len().get() as usize;
            let tail_sum = self.tail().iter().map(|ptr| unsafe {
                ptr.as_ref().used_len().get() as usize
            }).sum::<usize>();
            root_len + tail_sum
        }
        fn clear_inner(&mut self) {
            self.last_owned.set(0);
            let alloc = self.alloc.get_mut();
            let segments = self.segments.get_mut();
            let first = segments.first.take();
            let tail = segments.tail.drain(..);
            if let Some(first) = first {
                unsafe { alloc.dealloc(first.segment()) };
            }
            for ptr in tail {
                unsafe {
                    let b = Box::from_raw(ptr.as_ptr());
                    alloc.dealloc(b.segment());
                }
            }
        }
    }

    impl<A: Alloc + ?Sized> Arena<A> {
        pub fn as_read(&self) -> &dyn ReadArena {
            unsafe { &*(self.vtable.as_read)(from_ref(self).cast::<()>()) }
        }
        pub fn try_root_and_build(&self) -> Option<(&ArenaSegment, &dyn BuildArena)> {
            unsafe {
                let s = from_ref(self).cast::<()>();
                let (segment, arena) = (self.vtable.try_root_and_build)(s)?;
                Some((&*segment, &*arena))
            }
        }
        pub fn root_and_build(&mut self) -> (&ArenaSegment, &dyn BuildArena) {
            unsafe {
                let s = from_mut(self).cast::<()>();
                let (segment, arena) = (self.vtable.root_and_build)(s);
                (&*segment, &*arena)
            }
        }
        pub fn clear(&mut self) {
            unsafe { (self.vtable.clear)(from_mut(self).cast::<()>()) }
        }
    }

    impl<A: Alloc + ?Sized> Drop for Arena<A> {
        fn drop(&mut self) {
            self.clear();
        }
    }

    unsafe impl<A: Alloc> ReadArena for Arena<A> {
        fn segment(&self, id: SegmentId) -> Option<Segment> {
            self.segment(id).map(|s| s.used_segment())
        }
        fn size_in_words(&self) -> usize {
            self.size_in_words()
        }
    }

    unsafe impl<A: Alloc> BuildArena for Arena<A> {
        fn alloc(&self, min_size: AllocLen) -> Option<(SegmentOffset, &ArenaSegment)> {
            let last_owned = self.segment(self.last_owned.get()).unwrap();
            if let Some(rf) = last_owned.try_use_len(min_size) {
                return Some((rf, last_owned))
            }

            let tail = self.tail_mut();
            if tail.len() >= Self::MAX_SEGMENTS {
                return None
            }

            let id = tail.len() as SegmentId + 1;
            let segment = self.alloc(min_size, id)?;
            let ptr = box_to_nonnull(Box::new(segment));
            let segment = unsafe { ptr.as_ref() };
            tail.push(ptr);
            self.last_owned.set(id);
            Some((0u16.into(), segment))
        }
        fn segment(&self, id: SegmentId) -> Option<&ArenaSegment> {
            self.segment(id)
        }

        fn insert_external_segment(&self, segment: Segment) -> Option<SegmentId> {
            let tail = self.tail_mut();
            if tail.len() >= Self::MAX_SEGMENTS {
                return None
            }

            let id = tail.len() as SegmentId;
            let segment = unsafe { ArenaSegment::external(segment, id) };
            let ptr = box_to_nonnull(Box::new(segment));
            tail.push(ptr);
            Some(id)
        }
        fn remove_external_segment(&self, id: SegmentId) {
            self.segment(id).expect("cannot remove segment that doesn't exist").delete();
        }

        fn as_read_arena(&self) -> &dyn ReadArena { self }
        fn len(&self) -> u32 {
            let has_first = u32::from(self.root().is_some());
            let tail_len = self.tail().len() as u32;
            has_first + tail_len
        }
        fn size_in_words(&self) -> usize {
            self.size_in_words()
        }
    }

    fn box_to_nonnull<T>(b: Box<T>) -> NonNull<T> {
        let p = Box::into_raw(b);
        NonNull::new(p).unwrap()
    }
}
