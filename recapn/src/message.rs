//! An arena of segments containing Cap'n Proto data.

use crate::alloc::{
    Alloc, AllocLen, DynSpace, Global, Growing, ObjectLen, Scratch, Segment, SegmentLen,
    SegmentOffset, SignedSegmentOffset, Space, Word,
};
use crate::{any, ty, Result};
use crate::io::{StreamTable, StreamTableRef, TableReadError};
use crate::ptr::{PtrBuilder, PtrReader};
use crate::rpc::{Empty, InsertableInto};
use core::cell::{Cell, UnsafeCell};
use core::convert::TryFrom;
use core::fmt::{self, Debug};
use core::iter;
use core::marker::PhantomData;
use core::ptr::NonNull;
use core::slice;

use thiserror::Error;

/// An ID for a segment
pub type SegmentId = u32;

#[allow(dead_code)]
pub(crate) mod internal {
    use core::fmt::Debug;

    use super::*;

    /// An unchecked pointer into a segment. This can't be derefed normally. It must be checked and
    /// turned into a `SegmentRef`.
    #[derive(Clone, Copy, Debug, PartialEq, Eq, PartialOrd, Ord)]
    pub struct SegmentPtr<'a> {
        a: PhantomData<&'a Word>,
        ptr: *mut Word,
    }

    impl<'a> SegmentPtr<'a> {
        pub const fn null() -> Self {
            Self::new(core::ptr::null_mut())
        }

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
    pub struct SegmentRef<'a> {
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

        #[inline]
        pub unsafe fn iter_unchecked(self, offset: SegmentOffset) -> impl Iterator<Item = SegmentRef<'a>> {
            let range = 0..offset.get();
            range.into_iter().map(move |offset| unsafe {
                let offset = SegmentOffset::new_unchecked(offset);
                self.offset(offset).as_ref_unchecked()
            })
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

    #[derive(Clone)]
    pub struct SegmentReader<'a> {
        arena: &'a dyn SegmentSource,
        id: SegmentId,
        segment: Segment,
    }

    impl<'a> SegmentReader<'a> {
        pub fn from_source(source: &'a dyn SegmentSource, id: SegmentId) -> Option<Self> {
            Some(Self::new(source, id, source.get(id)?))
        }

        #[inline]
        pub fn new(arena: &'a dyn SegmentSource, id: SegmentId, segment: Segment) -> Self {
            Self { arena, id, segment }
        }

        /// Gets the start of the segment as a SegmentRef
        #[inline]
        pub fn start(&self) -> SegmentRef<'a> {
            SegmentRef {
                a: PhantomData,
                ptr: self.segment.data(),
            }
        }

        /// A pointer to just beyond the end of the segment.
        #[inline]
        pub fn end(&self) -> SegmentPtr<'a> {
            SegmentPtr::new(unsafe {
                self.segment
                    .data()
                    .as_ptr()
                    .add(self.segment.len().get() as usize)
            })
        }

        #[inline]
        pub fn id(&self) -> SegmentId {
            self.id
        }

        #[inline]
        fn contains(&self, ptr: SegmentPtr<'a>) -> bool {
            let start = self.start().as_segment_ptr();
            let end = self.end();

            start <= ptr && ptr < end
        }

        /// Checks if the pointer is a valid location in this segment, and if so,
        /// returns a ref for it.
        #[inline]
        pub fn try_get(&self, ptr: SegmentPtr<'a>) -> Option<SegmentRef<'a>> {
            if self.contains(ptr) {
                Some(unsafe { ptr.as_ref_unchecked() })
            } else {
                None
            }
        }

        #[inline]
        pub fn try_get_section(
            &self,
            start: SegmentPtr<'a>,
            len: ObjectLen,
        ) -> Option<SegmentRef<'a>> {
            let end = start.offset(len);
            if end < start {
                // the pointer wrapped around the address space? should only happen on 32 bit
                return None;
            }

            if end > self.end() {
                // the pointer went beyond the end of the segment
                return None;
            }

            self.try_get(start)
        }

        #[inline]
        pub fn try_get_section_offset(
            &self,
            offset: SegmentOffset,
            len: ObjectLen,
        ) -> Option<SegmentRef<'a>> {
            let start = self.start().offset(offset);
            self.try_get_section(start, len)
        }

        /// In release mode, converts a ptr to ref without checking if it's contained in this
        /// segment.
        ///
        /// In debug mode, this will assert that the ptr is contained in this segment.
        #[inline]
        pub unsafe fn get_unchecked(&self, ptr: SegmentPtr<'a>) -> SegmentRef<'a> {
            debug_assert!(self.contains(ptr));

            ptr.as_ref_unchecked()
        }

        /// Gets a segment reader for the segment with the specified ID, or None
        /// if the segment doesn't exist.
        #[inline]
        pub fn segment(&self, id: SegmentId) -> Option<SegmentReader<'a>> {
            Self::from_source(self.arena, id)
        }
    }

    impl fmt::Debug for SegmentReader<'_> {
        fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
            self.segment.fmt(f)
        }
    }

    pub struct ArenaSegment {
        segment: Segment,
        used_len: Option<Cell<AllocLen>>,
        segment_id: SegmentId,
    }

    impl ArenaSegment {
        #[inline]
        pub fn new(segment: Segment, used_len: Option<AllocLen>, id: SegmentId) -> Self {
            Self {
                segment,
                used_len: used_len.map(Cell::new),
                segment_id: id,
            }
        }

        #[inline]
        pub fn is_read_only(&self) -> bool {
            self.used_len.is_none()
        }

        #[inline]
        pub fn id(&self) -> SegmentId {
            self.segment_id
        }

        #[inline]
        pub fn segment(&self) -> &Segment {
            &self.segment
        }

        #[inline]
        pub fn try_use_len(&self, len: AllocLen) -> Option<SegmentOffset> {
            let used_len = self.used_len.as_ref()?;
            let old_used = used_len.get();
            let max = self.segment.len().get();

            let new_used = old_used.get().checked_add(len.get())?;
            if new_used > max {
                return None;
            }

            used_len.set(AllocLen::new(new_used).unwrap());
            Some(old_used.into())
        }

        #[inline]
        pub fn used_len(&self) -> SegmentOffset {
            if let Some(len) = &self.used_len {
                len.get().into()
            } else {
                self.segment().len().into()
            }
        }

        /// Returns a segment of the used data in this segment.
        ///
        /// If no data has been used, this returns None
        #[inline]
        pub fn used_segment(&self) -> Segment {
            Segment::new(self.segment.data(), self.used_len())
        }

        #[inline]
        pub fn as_slice(&self) -> &[Word] {
            let segment = self.used_segment();
            unsafe {
                core::slice::from_raw_parts(
                    segment.data().as_ptr().cast_const(),
                    segment.len().get() as usize,
                )
            }
        }
    }

    #[derive(Clone)]
    struct ArenaSegmentBuilder(NonNull<ArenaSegment>);

    impl ArenaSegmentBuilder {
        #[inline]
        pub const unsafe fn get(&self) -> &ArenaSegment {
            self.0.as_ref()
        }
    }

    /// An append-only arena for a set of message segments, managed by an Arena.
    ///
    /// This mostly serves as an optimization for the likely case where a message contains
    /// exactly one segment, in which case we don't want to have to allocate a vec.
    struct ArenaSegments {
        inner: UnsafeCell<ArenaSegmentsInner>,
    }

    struct ArenaSegmentsInner {
        /// Optimize for the first (and possibly only) segment
        first: ArenaSegment,
        /// An optional set of extra segments
        tail: Vec<NonNull<ArenaSegment>>,
    }

    impl ArenaSegments {
        const MAX_SEGMENTS: usize = (u32::MAX - 1) as usize;

        #[inline]
        pub fn new(first: Segment, used_size: AllocLen) -> Self {
            Self {
                inner: UnsafeCell::new(ArenaSegmentsInner {
                    first: ArenaSegment::new(first, Some(used_size), 0),
                    tail: Vec::new(),
                }),
            }
        }

        #[inline]
        fn inner(&self) -> &ArenaSegmentsInner {
            unsafe { &*self.inner.get() }
        }

        pub fn as_message_segments<'b>(&'b self) -> MessageSegments<'b> {
            let inner = self.inner();
            MessageSegments {
                first: inner.first.as_slice(),
                remainder: RemainderSegments { segments: inner.tail.as_slice() }
            }
        }

        #[inline]
        pub fn first(&self) -> NonNull<ArenaSegment> {
            NonNull::from(&self.inner().first)
        }

        #[inline]
        pub fn first_builder(&self) -> ArenaSegmentBuilder {
            ArenaSegmentBuilder(self.first())
        }

        #[inline]
        pub fn get(&self, id: SegmentId) -> Option<NonNull<ArenaSegment>> {
            match id {
                0 => Some(self.first()),
                _ => {
                    let idx = id as usize - 1;
                    self.inner().tail.get(idx).copied()
                },
            }
        }

        #[inline]
        pub fn builder(&self, id: SegmentId) -> Option<ArenaSegmentBuilder> {
            match id {
                0 => Some(self.first_builder()),
                _ => {
                    let idx = id as usize - 1;
                    let ptr = self.inner().tail[idx];
                    if unsafe { ptr.as_ref().is_read_only() } {
                        return None
                    }
        
                    Some(ArenaSegmentBuilder(ptr))
                },
            }
        }

        /// Add a new segment to the set, returning the next segment ID
        #[inline]
        pub fn push(&self, segment: Segment, used_len: AllocLen) -> (SegmentId, ArenaSegmentBuilder) {
            // Create a mutable reference directly to the tail vec. This prevents miri
            // saying we violated stacked borrows by taking a mutable borrow to the
            // whole ArenaSegmentsInner struct, which includes the ranges that the
            // first segment resides in.
            let tail = unsafe { &mut (*self.inner.get()).tail };

            if tail.len() > Self::MAX_SEGMENTS {
                panic!("Too many segments allocated in message!");
            }

            let id = (tail.len() + 1) as SegmentId;

            let arena_segment = ArenaSegment::new(segment, Some(used_len), id);
            let segment_ptr = NonNull::new(Box::into_raw(Box::new(arena_segment))).unwrap();
            tail.push(segment_ptr);

            (id, ArenaSegmentBuilder(segment_ptr))
        }

        #[inline]
        pub fn drain(self) -> impl Iterator<Item = ArenaSegment> {
            let ArenaSegmentsInner { first, tail } = self.inner.into_inner();

            let first = core::iter::once(first);
            let tail = tail.into_iter().map(|ptr| {
                let boxed = unsafe { Box::from_raw(ptr.as_ptr()) };
                *boxed
            });
            first.chain(tail)
        }
    }

    struct ArenaSegmentSet {
        segments: ArenaSegments,
        /// The last owned segment in the set
        last_owned: Cell<SegmentId>,
    }

    impl ArenaSegmentSet {
        pub fn new(first: Segment, used_size: AllocLen) -> Self {
            Self {
                segments: ArenaSegments::new(first, used_size),
                last_owned: Cell::new(0),
            }
        }
    }

    impl SegmentSource for ArenaSegmentSet {
        fn get(&self, id: SegmentId) -> Option<Segment> {
            let segment = self.segments.get(id)?;
            unsafe { Some(segment.as_ref().used_segment()) }
        }
    }

    pub(super) struct Arena<A: Alloc + ?Sized> {
        segments: Option<ArenaSegmentSet>,
        alloc: UnsafeCell<A>,
    }

    impl<A: Alloc> Arena<A> {
        pub const fn new(alloc: A) -> Self {
            Self {
                segments: None,
                alloc: UnsafeCell::new(alloc),
            }
        }

        pub fn root_builder<'b>(&'b mut self) -> SegmentBuilder<'b> {
            let alloc = self.alloc.get_mut();

            let alloc_len = AllocLen::MIN; // 1 Word for the root pointer
            let segments = self.segments.get_or_insert_with(|| unsafe {
                ArenaSegmentSet::new(alloc.alloc(alloc_len), alloc_len)
            });
            let segment = segments.segments.first_builder();
            let arena = BuilderArena { inner: unsafe { CovariantArenaStore::new(&*self) } };

            SegmentBuilder { segment, arena }
        }
    }

    impl<A: Alloc + ?Sized> Arena<A> {
        pub fn segments(&self) -> Option<MessageSegments> {
            Some(self.segments.as_ref()?.segments.as_message_segments())
        }

        pub fn alloc(&self) -> &A {
            // SAFETY: Since we're borrowing the arena fully with a shared reference, we know there
            // can't be any builders with interior mutability
            unsafe { &*self.alloc.get() }
        }

        pub fn get(&self, id: SegmentId) -> Option<Segment> {
            self.segments.as_ref()?.get(id)
        }

        pub fn clear(&mut self) {
            let alloc = self.alloc.get_mut();

            self.segments
                .take()
                .into_iter()
                .flat_map(|set| set.segments.drain())
                .filter(|s| !s.is_read_only())
                .for_each(|s| {
                    unsafe { alloc.dealloc(s.segment) }
                });
        }
    }

    impl<A: Alloc + ?Sized> Drop for Arena<A> {
        fn drop(&mut self) {
            self.clear()
        }
    }

    unsafe impl<A: Alloc + Send> Send for Arena<A> {}
    unsafe impl<A: Alloc + Sync> Sync for Arena<A> {}

    /// A reference to Arena<dyn Alloc> that doesn't make 'b invariant
    #[derive(Clone, Copy)]
    struct CovariantArenaStore<'b> {
        /// DON'T ACCESS THIS FIELD DIRECTLY, use get()
        inner: &'b Arena<dyn Alloc>,
    }

    impl<'b> CovariantArenaStore<'b> {
        unsafe fn new(borrow: &'b Arena<dyn Alloc + 'b>) -> Self {
            // VERY UNSAFE.
            // In order to prevent some very _unfortunate_ API choices made by other libraries,
            // we need to make 'b covariant. Normally due to the borrow below of
            // &'b ArenaInner<dyn Alloc + 'b>, this makes 'b invariant since it's
            // part of the traits's dynamic type of Alloc. This rule makes sense. If 'b was normally
            // allowed to automatically shorten, it would be possible to smuggle shorter variable
            // lifetimes into a possible cell. But in this _very specific case_, that doesn't apply.
            // We don't take anything out of the cell without giving it the lifetime 'b or shorter,
            // and most importantly, _we don't put anything into the cell with that lifetime_.
            // Unfortunately, we can't represent this in standard rust code. There is no
            // "UnsafeCovariant" type in the standard lib to make that possible, so we must
            // do it artificially by transmuting the reference into the one we actually want.
            Self {
                inner: core::mem::transmute::<_, &'b Arena<dyn Alloc>>(borrow)
            }
        }

        pub fn get(self) -> &'b Arena<dyn Alloc + 'b> {
            unsafe { core::mem::transmute::<_, &'b Arena<dyn Alloc>>(self.inner) }
        }
    }

    /// An arena builder that lets us add segments with interior mutability.
    #[derive(Clone)]
    pub struct BuilderArena<'b> {
        inner: CovariantArenaStore<'b>,
    }

    impl<'b> BuilderArena<'b> {
        fn parts(&self) -> (&mut dyn Alloc, &ArenaSegmentSet) {
            unsafe {
                let arena = self.inner.get();
                let alloc = &mut *arena.alloc.get();
                let segments = arena.segments.as_ref().unwrap_unchecked();
                (alloc, segments)
            }
        }

        fn segment_set(&self) -> &ArenaSegmentSet {
            let arena = self.inner.get();
            unsafe { arena.segments.as_ref().unwrap_unchecked() }
        }

        pub fn as_message_segments(&self) -> MessageSegments {
            self.segment_set().segments.as_message_segments()
        }

        /// Allocates a segment of at least `min_size` words. The newly allocated segment will
        /// already have the minimum size allocated within it, meaning you don't have to allocate
        /// again with the returned builder
        pub fn alloc_in_new_segment(
            &self,
            min_size: AllocLen,
        ) -> (SegmentRef<'b>, SegmentBuilder<'b>) {
            let (alloc, segments) = self.parts();

            let new_segment = unsafe { alloc.alloc(min_size) };
            let (new_id, ptr) = segments.segments.push(new_segment, min_size);

            segments.last_owned.set(new_id);

            let segment = SegmentBuilder {
                segment: ptr,
                arena: self.clone(),
            };
            (segment.start(), segment)
        }

        fn last_owned(&self) -> SegmentId {
            self.segment_set().last_owned.get()
        }

        /// Allocate an object of size somewhere in the message, returning a pointer to the
        /// allocated space and a segment builder for the segment the space was allocated in.
        pub fn alloc(&self, size: AllocLen) -> (SegmentRef<'b>, SegmentBuilder<'b>) {
            if let Some(builder) = self.segment_mut(self.last_owned()) {
                if let Some(space) = builder.alloc_in_segment(size) {
                    return (space, builder);
                }
            }

            self.alloc_in_new_segment(size)
        }

        pub fn segment_mut(&self, id: SegmentId) -> Option<SegmentBuilder<'b>> {
            let segments = self.segment_set();
            let segment = segments.segments.builder(id)?;

            Some(SegmentBuilder {
                segment,
                arena: self.clone(),
            })
        }
    }

    impl PartialEq<BuilderArena<'_>> for BuilderArena<'_> {
        #[inline]
        fn eq(&self, other: &BuilderArena<'_>) -> bool {
            core::ptr::eq(
                // compare by data pointers directly
                self.inner.get() as *const _ as *const (),
                other.inner.get() as *const _ as *const ()
            )
        }
    }

    #[derive(Clone)]
    pub struct SegmentBuilder<'b> {
        segment: ArenaSegmentBuilder,
        arena: BuilderArena<'b>,
    }

    impl<'b> SegmentBuilder<'b> {
        pub fn arena(&self) -> &BuilderArena<'b> {
            &self.arena
        }

        fn segment(&self) -> &ArenaSegment {
            unsafe { self.segment.get() }
        }

        pub fn as_reader<'c>(&'c self) -> SegmentReader<'c> {
            let arena = self.arena.segment_set();
            let arena_segment = self.segment();
            SegmentReader {
                arena,
                id: arena_segment.id(),
                segment: arena_segment.segment().clone(),
            }
        }

        #[inline]
        pub fn id(&self) -> SegmentId {
            self.segment().id()
        }

        #[inline]
        pub fn contains(&self, r: SegmentPtr<'b>) -> bool {
            self.segment()
                .segment()
                .to_ptr_range()
                .contains(&r.as_ptr())
        }

        #[inline]
        pub fn contains_section(&self, r: SegmentRef<'b>, offset: SegmentOffset) -> bool {
            self.contains_range(r.into(), r.offset(offset))
        }

        #[inline]
        pub fn contains_range(&self, start: SegmentPtr<'b>, end: SegmentPtr<'b>) -> bool {
            let segment_start = self.start().as_segment_ptr();
            let segment_end = self.end();

            segment_start <= start && start <= end && end < segment_end
        }

        #[inline]
        pub fn at_offset(&self, offset: SegmentOffset) -> SegmentRef<'b> {
            let r = self.start().offset(offset.into());
            debug_assert!(self.contains(r));
            unsafe { r.as_ref_unchecked() }
        }

        /// Gets the start of the segment as a SegmentRef
        #[inline]
        pub fn start(&self) -> SegmentRef<'b> {
            SegmentRef {
                a: PhantomData,
                ptr: self.segment().segment.data(),
            }
        }

        #[inline]
        pub fn end(&self) -> SegmentPtr<'b> {
            self.start().offset(self.segment().used_len().into())
        }

        #[inline]
        fn offset_from(&self, from: SegmentPtr<'b>, to: SegmentPtr<'b>) -> SignedSegmentOffset {
            debug_assert!(self.contains_range(from, to));
            unsafe {
                let offset = to.as_ptr().offset_from(from.as_ptr());
                SignedSegmentOffset::new_unchecked(offset as i32)
            }
        }

        /// Calculates the segment offset from the the end of `from` to the ptr `to`
        #[inline]
        pub fn offset_from_end_of(
            &self,
            from: SegmentPtr<'b>,
            to: SegmentPtr<'b>,
        ) -> SignedSegmentOffset {
            self.offset_from(from.offset(1u16.into()), to)
        }

        #[inline]
        pub fn offset_from_start(&self, to: SegmentPtr<'b>) -> SegmentOffset {
            debug_assert!(self.contains(to));

            let signed_offset = self.offset_from(self.start().into(), to);
            SegmentOffset::new(signed_offset.get() as u32).expect("offset from start was negative!")
        }

        /// Allocates space of the given size _somewhere_ in the message.
        #[inline]
        pub fn alloc(&self, size: AllocLen) -> (SegmentRef<'b>, SegmentBuilder<'b>) {
            if let Some(word) = self.alloc_in_segment(size) {
                (word, self.clone())
            } else {
                self.arena.alloc(size)
            }
        }

        /// Attempt to allocate the given space in this segment.
        #[inline]
        pub fn alloc_in_segment(&self, size: AllocLen) -> Option<SegmentRef<'b>> {
            unsafe {
                let start_alloc_offset = self.segment().try_use_len(size)?;
                let start = self
                    .start()
                    .offset(start_alloc_offset.into())
                    .as_ref_unchecked();
                Some(start)
            }
        }
    }

    impl Debug for SegmentBuilder<'_> {
        fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
            let segment = self.segment();
            f.debug_struct("SegmentBuilder")
                .field("id", &segment.id())
                .field("start", &segment.segment().data())
                .field("len", &segment.used_len())
                .finish()
        }
    }

    /// Controls how far a single message reader can read into a message.
    #[derive(Debug)]
    pub struct ReadLimiter {
        limit: Cell<u64>,
    }

    impl ReadLimiter {
        #[inline]
        pub const fn new(word_count: u64) -> Self {
            Self {
                limit: Cell::new(word_count),
            }
        }

        /// Attempts to read `words` from the limiter, returning a [`bool`](std::primitive::bool) indicating whether the read was successful.
        #[inline]
        pub fn try_read(&self, words: u64) -> bool {
            let current = self.limit.get();
            if words <= current {
                self.limit.set(current - words);
                return true;
            }

            false
        }

        #[inline]
        pub fn current_limit(&self) -> u64 {
            self.limit.get()
        }
    }

    pub trait SegmentSource {
        fn get(&self, id: SegmentId) -> Option<Segment>;

        fn size_in_words(&self) -> usize {
            (0u32..)
                .map_while(|id| self.get(id))
                .map(|s| s.len().get() as usize)
                .sum()
        }
    }

    impl<'a, T: SegmentSource> SegmentSource for &'a T {
        fn get(&self, id: SegmentId) -> Option<Segment> {
            T::get(*self, id)
        }
    }
}

use internal::*;

/// A message. Can be used to create multiple readers or one builder at a time.
///
/// This type exists to separate readers from builders in a way that's compatible with Rust's
/// lifetime system. Where in C++ you create message builders directly, in Rust we require you
/// to go through a separate "message" container to force the API to generate a lifetime that
/// constrains whether readers or a builder can exist. This way, we can apply safety guarntees
/// where they'd be difficult to implement if we started with simply a builder.
///
/// ```compile_fail
/// let mut message = recapn::Message::global();
/// let builder = message.builder();
///
/// let reader = message.reader(); // lifetime error here because builder exists
/// ```
///
/// It does have some other benefits however. In the previous version of Cap'n Proto Rust, since
/// everything was either a reader or a builder, there wasn't a way to share messages across
/// threads since readers and builders have a read limiter with interior mutability that is updated
/// as you move through the message. With a separate "message" holder at the top, we can avoid this
/// issue. Since the message can't read itself directly, it doesn't have interior mutability
/// (unless its allocator does), which means it can be send + sync. With send and sync, we can store
/// the message in an Arc, allowing us to read or build (with a lock) across threads.
///
/// This type does not allocate by default. The first time you get the builder for the message, a
/// segment is allocated.
pub struct Message<'e, A: Alloc> {
    /// A phantom lifetime from external readonly segments.
    e: PhantomData<&'e Segment>,
    arena: Arena<A>,
}

impl<'e, A: Alloc> Message<'e, A> {
    /// Creates a new message backed by the specified allocator
    #[inline]
    pub const fn new(alloc: A) -> Self {
        Self {
            e: PhantomData,
            arena: Arena::new(alloc),
        }
    }

    /// Returns the segments of the message, or None if a root segment hasn't been allocated yet.
    #[inline]
    pub fn segments(&self) -> Option<MessageSegments> {
        self.arena.segments()
    }

    /// Creates a new reader for this message. This reader has no limits placed on it.
    ///
    /// If you want a limited reader, use [`Message::reader_with_options`].
    #[inline]
    pub fn reader(&self) -> Reader<&Self> {
        Reader::limitless(self)
    }

    /// Creates a new reader for this message with the specified reader options.
    #[inline]
    pub fn reader_with_options(&self, options: ReaderOptions) -> Reader<&Self> {
        Reader::new(self, options)
    }

    /// Gets the builder for this message. This will allocate the root segment if it doesn't exist.
    #[inline]
    pub fn builder<'b>(&'b mut self) -> Builder<'b, 'e> {
        Builder {
            e: PhantomData,
            root: self.arena.root_builder(),
        }
    }

    /// Clears the message, deallocating all the segments within it.
    pub fn clear(&mut self) {
        self.arena.clear()
    }
}

impl Message<'_, Growing<Global>> {
    /// Creates a message backed by the default growing global allocator configuration. This is
    /// the configuration similar to the default configuration of the `MallocMessageBuilder` in
    /// Cap'n Proto C++.
    #[inline]
    pub const fn global() -> Self {
        Self::new(Growing::new(
            Growing::<()>::DEFAULT_FIRST_SEGMENT_LEN,
            Global,
        ))
    }
}

impl<'s> Message<'_, Scratch<'s, Growing<Global>>> {
    /// Creates a message backed by the default growing global allocator configuration and the
    /// given scratch space.
    #[inline]
    pub fn with_scratch<const N: usize>(space: &'s mut Space<N>) -> Self {
        Self::new(Scratch::with_space(space, Default::default()))
    }

    /// Creates a message backed by the default growing global allocator configuration and the
    /// given dynamically sized scratch space.
    #[inline]
    pub fn with_dyn_scratch(space: &'s mut DynSpace) -> Self {
        Self::new(Scratch::with_dyn_space(space, Default::default()))
    }
}

impl<A: Alloc + Default> Default for Message<'_, A> {
    /// Creates a default instance of the allocator and returns a new message backed by it.
    #[inline]
    fn default() -> Self {
        Message::new(A::default())
    }
}

impl<A: Alloc> SegmentSource for Message<'_, A> {
    fn get(&self, id: SegmentId) -> Option<Segment> {
        self.arena.get(id)
    }
}

unsafe impl<A: Alloc + Send> Send for Message<'_, A> {}
unsafe impl<A: Alloc + Sync> Sync for Message<'_, A> {}

/// Options controlling how data is read
pub struct ReaderOptions {
    /// Limits how many total words of data are allowed to be traversed.  Traversal is counted when
    /// a new struct or list builder is obtained, e.g. from a get() accessor.  This means that calling
    /// the getter for the same sub-struct multiple times will cause it to be double-counted.  Once
    /// the traversal limit is reached, an error will be reported.
    ///
    /// This limit exists for security reasons.  It is possible for an attacker to construct a message
    /// in which multiple pointers point at the same location.  This is technically invalid, but hard
    /// to detect.  Using such a message, an attacker could cause a message which is small on the wire
    /// to appear much larger when actually traversed, possibly exhausting server resources leading to
    /// denial-of-service.
    ///
    /// It makes sense to set a traversal limit that is much larger than the underlying message.
    /// Together with sensible coding practices (e.g. trying to avoid calling sub-object getters
    /// multiple times, which is expensive anyway), this should provide adequate protection without
    /// inconvenience.
    ///
    /// The default limit is 64 MiB.  This may or may not be a sensible number for any given use case,
    /// but probably at least prevents easy exploitation while also avoiding causing problems in most
    /// typical cases.
    pub traversal_limit: u64,
    /// Limits how deeply-nested a message structure can be, e.g. structs containing other structs or
    /// lists of structs.
    ///
    /// Like the traversal limit, this limit exists for security reasons.  Since it is common to use
    /// recursive code to traverse recursive data structures, an attacker could easily cause a stack
    /// overflow by sending a very-deeply-nested (or even cyclic) message, without the message even
    /// being very large.  The default limit of 64 is probably low enough to prevent any chance of
    /// stack overflow, yet high enough that it is never a problem in practice.
    pub nesting_limit: u32,
}

impl ReaderOptions {
    /// The default reader options returned by the default implementation
    pub const DEFAULT: Self = Self {
        traversal_limit: 8 * 1024 * 1024,
        nesting_limit: 64,
    };
}

impl Default for ReaderOptions {
    fn default() -> Self {
        Self::DEFAULT
    }
}

/// A type used to read a message on a single thread.
pub struct Reader<M> {
    message: M,
    limiter: Option<ReadLimiter>,
    nesting_limit: u32,
}

impl<M: SegmentSource> Reader<M> {
    /// Creates a message reader with the specified limits.
    pub const fn new(message: M, options: ReaderOptions) -> Self {
        Self {
            message,
            limiter: Some(ReadLimiter::new(options.traversal_limit)),
            nesting_limit: options.nesting_limit,
        }
    }

    /// Creates a message reader with no limits applied.
    /// 
    /// Note: You should only use this if you trust the source of this data! Untrusted
    /// data can perform amplification attacks and stack overflow DoS attacks while
    /// reading the data, so you should only use this method if you want to read some
    /// data that you know is safe.For instance: data built yourself using Message
    /// is generally trusted, so Message::reader uses this constructor.
    pub const fn limitless(message: M) -> Self {
        Self {
            message,
            limiter: None,
            nesting_limit: u32::MAX,
        }
    }

    /// Returns the root as any pointer. If no root segment exists, this is a default null pointer.
    #[inline]
    pub fn root(&self) -> any::PtrReader {
        self.root_option().unwrap_or_default()
    }

    /// Returns the root as any pointer. If no root segment exists, this returns None.
    #[inline]
    pub fn root_option(&self) -> Option<any::PtrReader> {
        let segment = SegmentReader::from_source(&self.message, 0)?;
        Some(PtrReader::root(segment, self.limiter.as_ref(), self.nesting_limit).into())
    }

    /// Returns the root and interprets it as the specified pointer type.
    #[inline]
    pub fn read_as<'b, T: ty::FromPtr<any::PtrReader<'b>>>(&'b self) -> T::Output {
        self.root().read_as::<T>()
    }

    /// Calculates the message's total size in words.
    #[inline]
    pub fn size_in_words(&self) -> usize {
        self.message.size_in_words()
    }
}

pub struct Builder<'b, 'e: 'b> {
    e: PhantomData<&'e [Word]>,
    root: SegmentBuilder<'b>,
}

impl<'b, 'e: 'b> Builder<'b, 'e> {
    pub fn by_ref<'c>(&'c mut self) -> Builder<'c, 'e> {
        Builder {
            e: PhantomData,
            root: self.root.clone(),
        }
    }

    pub fn init_struct_root<S: ty::Struct>(self) -> S::Builder<'b, Empty> {
        let root = self.into_root();
        let root_raw = crate::ptr::PtrBuilder::from(root);
        let builder = root_raw.init_struct(<S as ty::Struct>::SIZE);
        unsafe { ty::StructBuilder::from_ptr(builder) }
    }

    pub fn set_struct_root<S, T>(self, reader: &S::Reader<'_, T>) -> S::Builder<'b, Empty>
    where
        S: ty::Struct,
        T: InsertableInto<Empty>,
    {
        let root = self.into_root();
        let root_raw = crate::ptr::PtrBuilder::from(root);
        let builder = root_raw.set_struct(reader.as_ref(), crate::ptr::CopySize::Minimum(S::SIZE));
        unsafe { ty::StructBuilder::from_ptr(builder) }
    }

    /// Returns the root pointer for the message.
    pub fn into_root(self) -> any::PtrBuilder<'b> {
        PtrBuilder::root(self.root).into()
    }

    pub fn into_parts(self) -> BuilderParts<'b> {
        BuilderParts {
            root: PtrBuilder::root(self.root).into(),
        }
    }

    pub fn segments(&self) -> MessageSegments {
        self.root.arena().as_message_segments()
    }

    pub fn size_in_words(&self) -> usize {
        self.segments().iter().map(|s| s.len()).sum()
    }
}

/// The raw components of the builder, including the root pointer, the root lifetime orphanage,
/// and the external segment inserter.
pub struct BuilderParts<'b> {
    pub root: any::PtrBuilder<'b>,
}

/// The internal segments of a built message. This always contains at least one segment.
#[derive(Clone)]
pub struct MessageSegments<'b> {
    pub first: &'b [Word],
    pub remainder: RemainderSegments<'b>,
}

impl<'b> MessageSegments<'b> {
    #[inline]
    pub fn first_bytes(&self) -> &'b [u8] {
        Word::slice_to_bytes(self.first)
    }

    #[inline]
    pub fn len(&self) -> u32 {
        self.remainder.len() + 1
    }

    #[inline]
    pub fn iter(&self) -> SegmentsIter<'b> {
        SegmentsIter {
            inner: core::iter::once(self.first).chain(self.remainder.iter()),
        }
    }
}

impl<'b> IntoIterator for MessageSegments<'b> {
    type IntoIter = SegmentsIter<'b>;
    type Item = &'b [Word];

    fn into_iter(self) -> Self::IntoIter {
        self.iter()
    }
}

impl<'a, 'b> IntoIterator for &'b MessageSegments<'a> {
    type IntoIter = SegmentsIter<'a>;
    type Item = &'a [Word];

    fn into_iter(self) -> Self::IntoIter {
        self.iter()
    }
}

#[derive(Clone)]
pub struct RemainderSegments<'b> {
    segments: &'b [NonNull<ArenaSegment>],
}

impl<'b> RemainderSegments<'b> {
    #[inline]
    pub fn is_empty(&self) -> bool {
        self.segments.is_empty()
    }
    #[inline]
    pub fn len(&self) -> u32 {
        self.segments.len() as u32
    }
    #[inline]
    fn iter(&self) -> RemainderIter<'b> {
        self.segments
            .iter()
            .map(|s| unsafe { s.as_ref().as_slice() })
    }
}

type Iter<'b> = iter::Chain<iter::Once<&'b [Word]>, RemainderIter<'b>>;
type RemainderIter<'b> =
    iter::Map<slice::Iter<'b, NonNull<ArenaSegment>>, fn(&'b NonNull<ArenaSegment>) -> &'b [Word]>;

#[derive(Clone)]
pub struct SegmentsIter<'b> {
    inner: Iter<'b>,
}

impl<'b> Iterator for SegmentsIter<'b> {
    type Item = &'b [Word];

    #[inline]
    fn next(&mut self) -> Option<Self::Item> {
        self.inner.next()
    }
    #[inline]
    fn count(self) -> usize
    where
        Self: Sized,
    {
        self.inner.count()
    }
    #[inline]
    fn size_hint(&self) -> (usize, Option<usize>) {
        self.inner.size_hint()
    }
}

enum RawKind {
    Slice,
    Box,
}

struct RawSegment {
    segment: Segment,
    kind: RawKind,
}

impl RawSegment {
    pub fn from_slice(slice: &[Word]) -> Self {
        let len_u32 = u32::try_from(slice.len()).expect("segment too large");
        let len = SegmentLen::new(len_u32).expect("segment too large");
        let ptr = NonNull::from(&slice[0]);
        let segment = Segment::new(ptr, len);

        Self {
            segment,
            kind: RawKind::Slice,
        }
    }

    pub fn from_box(boxed: Box<[Word]>) -> Self {
        let len_u32 = u32::try_from(boxed.len()).expect("segment too large");
        let len = SegmentLen::new(len_u32).expect("segment too large");
        let ptr = NonNull::new(Box::leak(boxed).as_mut_ptr()).unwrap();
        let segment = Segment::new(ptr, len);

        Self {
            segment,
            kind: RawKind::Box,
        }
    }
}

impl Drop for RawSegment {
    fn drop(&mut self) {
        if let RawKind::Box = self.kind {
            let ptr = self.segment.data().as_ptr();
            let len = self.segment.len().get() as usize;
            unsafe {
                let slice = core::slice::from_raw_parts_mut(ptr, len);
                drop(Box::from_raw(slice));
            }
        }
    }
}

unsafe impl Send for RawSegment {}
unsafe impl Sync for RawSegment {}

/// A set of read-only segments used by a MessageReader.
///
/// This is a generic backing source type provided by the library.
pub struct SegmentSet<'a> {
    a: PhantomData<&'a Segment>,
    first: RawSegment,
    tail: Vec<RawSegment>,
}

impl SegmentSource for SegmentSet<'_> {
    fn get(&self, id: SegmentId) -> Option<Segment> {
        let segment = match id {
            0 => self.first.segment.clone(),
            id => self
                .tail
                .get((id - 1) as usize)
                .map(|r| r.segment.clone())?,
        };
        Some(segment)
    }
}

impl<'a> SegmentSet<'a> {
    pub fn empty() -> Self {
        Self::of_slice(&[Word::NULL])
    }

    fn from_raw(raw: RawSegment) -> Self {
        Self {
            a: PhantomData,
            first: raw,
            tail: Vec::new(),
        }
    }

    pub fn of_slice(slice: &'a [Word]) -> Self {
        Self::from_raw(RawSegment::from_slice(slice))
    }

    pub fn of_box(boxed: Box<[Word]>) -> Self {
        Self::from_raw(RawSegment::from_box(boxed))
    }
}

impl Debug for SegmentSet<'_> {
    fn fmt(&self, f: &mut fmt::Formatter) -> fmt::Result {
        f.debug_list()
            .entry(&self.first.segment)
            .entries(self.tail.iter().map(|s| &s.segment))
            .finish()
    }
}

#[derive(Debug)]
pub struct SegmentSetBuilder<'a> {
    segments: Option<SegmentSet<'a>>,
}

impl<'a> SegmentSetBuilder<'a> {
    pub fn new() -> Self {
        Self { segments: None }
    }

    fn push_raw(&mut self, raw: RawSegment) {
        match &mut self.segments {
            Some(segments) => {
                assert!(segments.tail.len() < (u32::MAX as usize));

                segments.tail.push(raw);
            }
            segments @ None => *segments = Some(SegmentSet::from_raw(raw)),
        }
    }

    /// Adds a segment slice to the set.
    ///
    /// # Panics
    ///
    /// Panics if the slice is larger than the max segment size (4 GB)
    pub fn push_slice(&mut self, segment: &'a [Word]) {
        self.push_raw(RawSegment::from_slice(segment))
    }

    /// Adds a boxed segment slice to the set.
    ///
    /// # Panics
    ///
    /// Panics if the slice is larger than the max segment size (4 GB)
    pub fn push_box(&mut self, boxed: Box<[Word]>) {
        self.push_raw(RawSegment::from_box(boxed))
    }

    /// Builds the reader.
    ///
    /// If no segments were inserted into the builder, dummy empty message reader is returned.
    pub fn build(self) -> SegmentSet<'a> {
        self.segments.unwrap_or_else(|| SegmentSet::empty())
    }
}

#[derive(Error, Debug)]
pub enum ReadError<'a> {
    /// An error occured while reading the stream table
    #[error(transparent)]
    Table(#[from] TableReadError),
    /// A segment in the table indicated it's too large to be read
    #[error("segment too large")]
    SegmentTooLarge {
        /// The ID of the segment
        segment: SegmentId,
        /// The stream table reader
        table: &'a StreamTableRef,
    },
    /// The message was incomplete
    #[error("incomplete message")]
    Incomplete(Box<Partial<'a>>),
}

#[cfg(feature = "std")]
#[derive(Error, Debug)]
pub enum StreamError {
    /// An error occured while reading the stream table
    #[error(transparent)]
    Table(#[from] TableReadError),
    /// A segment in the table indicated it's too large to be read
    #[error("segment too large")]
    SegmentTooLarge {
        /// The ID of the segment
        segment: SegmentId,
        /// The stream table reader
        table: StreamTable,
    },
    #[error("message too large")]
    MessageTooLarge,
    #[error(transparent)]
    Io(#[from] std::io::Error),
}

pub struct StreamOptions {
    /// The max number of segments that are allowed to be contained in the message. If the limit is 0 or 1
    pub segment_limit: u32,
    /// The max amount of words that can be read from the input stream
    pub read_limit: u64,
}

impl StreamOptions {
    pub const DEFAULT: Self = Self {
        segment_limit: 512,
        read_limit: ReaderOptions::DEFAULT.traversal_limit,
    };
}

impl Default for StreamOptions {
    fn default() -> Self {
        Self::DEFAULT
    }
}

#[derive(Debug)]
pub struct Partial<'a> {
    /// A slice of the existing segment data
    pub existing: &'a [Word],
    /// How many bytes are required to read the rest of the current segment
    pub remaining: usize,
    /// The current segment ID we were reading
    pub current: SegmentId,
    /// The stream table reader
    pub table: &'a StreamTableRef,
    /// The segment set builder
    pub builder: SegmentSetBuilder<'a>,
}

impl<'a> SegmentSet<'a> {
    /// Parse a message from a flat array.
    ///
    /// This returns both the message reader and the remainder of the slice beyond the end of
    /// the message.
    ///
    /// If the content is incomplete or invalid, this returns an error
    #[inline]
    pub fn from_slice(slice: &'a [Word]) -> Result<(Self, &'a [Word]), ReadError<'a>> {
        let (table, mut content) = StreamTableRef::try_read(slice)?;
        let mut builder = SegmentSetBuilder::new();

        for (len, id) in table.segments().into_iter().zip(0u32..) {
            let Some(len) = len.try_get() else {
                return Err(ReadError::SegmentTooLarge {
                    segment: id, table,
                })
            };
            let len = len.get() as usize;

            let Some((segment, remainder)) = content.get(..len).zip(content.get(len..)) else {
                return Err(ReadError::Incomplete(Box::new(Partial {
                    existing: content,
                    remaining: len - content.len(),
                    current: id,
                    table,
                    builder,
                })))
            };

            builder.push_slice(segment);
            content = remainder;
        }

        Ok((builder.build(), content))
    }

    #[cfg(feature = "std")]
    pub fn from_read<R: std::io::Read>(
        mut reader: R,
        options: StreamOptions,
    ) -> Result<Self, StreamError> {
        let mut first = [Word::NULL; 1];
        let segment_table_buffer: tinyvec::TinyVec<[Word; 32]>;

        reader.read_exact(Word::slice_to_bytes_mut(&mut first))?;

        let segment_table = match StreamTableRef::try_read(&first) {
            Ok((table, _)) => table,
            Err(TableReadError::Incomplete { count, required }) => {
                if count >= options.segment_limit {
                    return Err(StreamError::Table(TableReadError::TooManySegments));
                }

                let mut buffer = tinyvec::tiny_vec!();
                buffer.resize(required + 1, Word::NULL);
                let (buffer_first, rest) = buffer.split_first_mut().unwrap();
                *buffer_first = first[0];

                reader.read_exact(Word::slice_to_bytes_mut(rest))?;

                segment_table_buffer = buffer;

                StreamTableRef::try_read(&segment_table_buffer)
                    .expect("failed to read segment table")
                    .0
            }
            Err(err @ TableReadError::TooManySegments) => return Err(StreamError::Table(err)),
            Err(TableReadError::Empty) => unreachable!(),
        };

        let mut total_words = 0u64;
        for (id, size) in segment_table.segments().into_iter().enumerate() {
            let Some(size) = size.try_get() else {
                return Err(StreamError::SegmentTooLarge {
                    segment: id as u32,
                    table: segment_table.to_owned(),
                })
            };

            total_words += size.get() as u64;

            if total_words > options.read_limit {
                return Err(StreamError::MessageTooLarge);
            }    
        }

        let mut segments = SegmentSetBuilder::new();

        for size in segment_table
            .segments()
            .into_iter()
            .map(|size| size.try_get().unwrap())
        {
            let mut segment = vec![Word::NULL; size.get() as usize].into_boxed_slice();
            reader.read_exact(Word::slice_to_bytes_mut(segment.as_mut()))?;

            segments.push_box(segment);
        }

        Ok(segments.build())
    }
}