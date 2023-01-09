use crate::alloc::{
    Alloc, AllocLen, DynSpace, Global, Growing, ObjectLen, Scratch, Segment, SegmentLen,
    SegmentOffset, Space, Word,
};
use crate::ptr::{PtrBuilder, PtrReader};
use crate::rpc::Empty;
use crate::any;
use core::cell::{Cell, UnsafeCell};
use core::convert::TryInto;
use core::fmt;
use core::iter;
use core::marker::PhantomData;
use core::pin::Pin;
use core::ptr::NonNull;
use core::slice;

pub type SegmentId = u32;

pub(crate) mod internal {
    use core::fmt::Debug;

    use crate::alloc::{SignedSegmentOffset, NonZeroU29};

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
            SegmentRef { a: PhantomData, ptr: NonNull::new_unchecked(self.ptr) }
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

    #[derive(Clone, Copy)]
    pub enum ArenaReader<'a> {
        FromBuilder(&'a Option<ArenaSegments>),
    }

    impl<'a> ArenaReader<'a> {
        pub fn get(&self, id: SegmentId) -> Option<SegmentReader<'a>> {
            match self {
                ArenaReader::FromBuilder(segments) => {
                    let segments = segments.as_ref()?;
                    let segment = match id {
                        0 => &segments.first,
                        id => unsafe {
                            let tail = &*segments.tail.get();
                            tail.get((id - 1) as usize)?.as_ref()

                        },
                    }
                    .used_segment();

                    Some(SegmentReader::new(*self, segment))
                }
            }
        }
    }

    #[derive(Clone)]
    pub struct SegmentReader<'a> {
        arena: ArenaReader<'a>,
        segment: Segment,
    }

    impl<'a> SegmentReader<'a> {
        #[inline]
        pub fn new(arena: ArenaReader<'a>, segment: Segment) -> Self {
            Self { arena, segment }
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
            self.arena.get(id)
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
            let len = SegmentLen::new(self.used_len().get()).unwrap();
            unsafe { Segment::new(self.segment.data(), len) }
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

    pub struct Arena<'e, A: Alloc + ?Sized> {
        /// A phantom lifetime from external readonly segments.
        e: PhantomData<&'e Segment>,
        /// The inner arena data, wrapped in an unsafe cell
        inner: UnsafeCell<ArenaInner<A>>,
    }

    struct ArenaInner<A: Alloc + ?Sized> {
        segments: UnsafeCell<Option<ArenaSegments>>,
        /// The last owned segment in this set
        last_owned: Cell<SegmentId>,
        alloc: UnsafeCell<A>,
    }

    /// An append-only arena for a set of message segments, managed by an Arena.
    ///
    /// This mostly serves as an optimization for the likely case where a message contains
    /// exactly one segment, in which case we don't want to have to allocate a vec.
    pub struct ArenaSegments {
        /// Optimize for the first (and possibly only) segment
        first: ArenaSegment,
        /// An optional set of extra segments
        tail: UnsafeCell<Vec<NonNull<ArenaSegment>>>,
    }

    impl<'e, A: Alloc> Arena<'e, A> {
        pub fn new(alloc: A) -> Self {
            Self {
                e: PhantomData,
                inner: UnsafeCell::new(ArenaInner {
                    alloc: UnsafeCell::new(alloc),
                    last_owned: Cell::new(0),
                    segments: UnsafeCell::new(None),
                }),
            }
        }
    }

    impl<'e, A: Alloc> Arena<'e, A> {
        pub fn segments(&self) -> Option<MessageSegments> {
            let inner = unsafe { &*self.inner.get() };
            let segments = unsafe { &*inner.segments.get() };

            segments.as_ref().map(|s| {
                MessageSegments {
                    first: s.first.as_slice(),
                    remainder: RemainderSegments {
                        segments: unsafe { &*s.tail.get() }
                    },
                }
            })
        }

        pub fn reader(&self) -> ArenaReader {
            let inner = unsafe { &*self.inner.get() };
            let segments = unsafe { &*inner.segments.get() };

            ArenaReader::FromBuilder(&segments)
        }

        pub fn builder<'b>(&'b mut self) -> BuilderArena<'b> {
            let borrow = unsafe {
                // VERY UNSAFE.
                // In order to prevent some very _unfortunate_ API choices made by other libraries,
                // we need to make 'b covariant. Normally due to the normal borrow below of
                // &'b UnsafeCell<ArenaInner<dyn Alloc + 'b>>, this makes 'b invariant since it's
                // part of the cell's type of T. This rule makes sense. If 'b was normally
                // allowed to automatically shorten, it would be possible to smuggle shorter variable
                // lifetimes into the cell. But in this _very specific case_, that doesn't apply.
                // We don't take anything out of the cell without giving it the lifetime 'b or shorter,
                // and most importantly, _we don't put anything into the cell with that lifetime_.
                // Unfortunately, we can't represent this in standard rust code. There is no
                // "UnsafeCovariant" type in the standard lib to make that possible, so we must
                // do it artificially by transmuting the reference into the one we actually want.
                let borrow: &'b UnsafeCell<ArenaInner<dyn Alloc + 'b>> = &self.inner;
                core::mem::transmute::<_, &'b UnsafeCell<ArenaInner<dyn Alloc>>>(borrow)
            };
            BuilderArena { inner: borrow }
        }
    }

    impl<A: Alloc + ?Sized> Drop for ArenaInner<A> {
        fn drop(&mut self) {
            let alloc = self.alloc.get_mut();
            if let Some(segments) = self.segments.get_mut().take() {
                unsafe {
                    alloc.dealloc(segments.first.segment().clone());
                }

                let segments = segments.tail
                    .into_inner()
                    .into_iter()
                    .map(|ptr| unsafe { *Box::from_raw(ptr.as_ptr()) });
                for segment in segments {
                    unsafe {
                        alloc.dealloc(segment.segment().clone());
                    }
                }
            }
        }
    }

    unsafe impl<'e, A: Alloc + Send> Send for Arena<'e, A> {}
    unsafe impl<'e, A: Alloc + Sync> Sync for Arena<'e, A> {}

    /// An arena builder that lets us add segments with interior mutability.
    #[derive(Clone, Debug)]
    pub struct BuilderArena<'b> {
        /// DON'T ACCESS THIS FIELD DIRECTLY. Use arena() or arena_mut() instead.
        inner: &'b UnsafeCell<ArenaInner<dyn Alloc>>,
    }

    impl<'b> BuilderArena<'b> {
        const MAX_SEGMENTS: usize = (u32::MAX - 1) as usize;

        /// Allocates a segment of at least `min_size` words. The newly allocated segment will
        /// already have the minimum size allocated within it, meaning you don't have to allocate
        /// again with the returned builder
        pub fn alloc_in_new_segment(&self, min_size: SegmentLen) -> (SegmentRef<'b>, SegmentBuilder<'b>) {
            unsafe {
                let inner = self.inner();
                let alloc = &mut *inner.alloc.get();
                let segments = &*inner.segments.get();

                let ptr = match segments {
                    None => {
                        let new_segment = alloc.alloc(min_size);
                        let segments_mut = &mut *inner.segments.get();
                        let segment = ArenaSegment::new(new_segment, Some(min_size), 0);
                        let segments = segments_mut.insert(ArenaSegments {
                            first: segment,
                            tail: UnsafeCell::new(Vec::new()),
                        });
                        // this pointer is fine, the reference will stick around at least as long
                        // as the associated arena borrow
                        NonNull::from(&segments.first)
                    }
                    Some(segments) => {
                        let tail = &mut *segments.tail.get();

                        if tail.len() > Self::MAX_SEGMENTS {
                            panic!("Too many segments allocated in message!");
                        }

                        let next_id = (tail.len() + 1) as u32;

                        inner.last_owned.set(next_id);

                        let new_segment = alloc.alloc(min_size);
                        let boxed = Box::into_raw(Box::new(ArenaSegment::new(new_segment, Some(min_size), next_id)));
                        let ptr = NonNull::new(boxed).unwrap();

                        tail.push(ptr);

                        ptr
                    }
                };
                let segment = SegmentBuilder {
                    segment: ptr,
                    arena: self.clone(),
                };
                (segment.start(), segment)
            }
        }

        fn inner(&self) -> &ArenaInner<dyn Alloc + 'b> {
            unsafe { core::mem::transmute(&*self.inner.get()) }
        }

        fn last_owned(&self) -> SegmentId {
            self.inner().last_owned.get()
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
            let segments = unsafe { (&*self.inner().segments.get()).as_ref()? };
            let segment = match id {
                0 => NonNull::from(&segments.first),
                id => {
                    let index = (id - 1) as usize;
                    let tail = unsafe { &*segments.tail.get() };
                    let segment = tail[index];
                    if unsafe { segment.as_ref().is_read_only() } {
                        return None;
                    }

                    segment
                }
            };
            Some(SegmentBuilder {
                segment,
                arena: self.clone(),
            })
        }
    }

    #[derive(Clone)]
    pub struct SegmentBuilder<'b> {
        segment: NonNull<ArenaSegment>,
        arena: BuilderArena<'b>,
    }

    impl<'b> SegmentBuilder<'b> {
        pub fn arena(&self) -> &BuilderArena<'b> {
            &self.arena
        }

        fn segment(&self) -> &ArenaSegment {
            unsafe { self.segment.as_ref() }
        }

        pub fn as_reader<'c>(&'c self) -> SegmentReader<'c> {
            let segments = unsafe { &*self.arena.inner().segments.get() };
            let arena = ArenaReader::FromBuilder(segments);
            SegmentReader {
                arena,
                segment: self.segment().segment().clone(),
            }
        }

        #[inline]
        pub fn id(&self) -> SegmentId {
            unsafe { self.segment.as_ref().id() }
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
                let start = self.start()
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
/// This type does not allocate by default. The first time you init the root of the message, a
/// segment is allocated.
pub struct Message<'e, A: Alloc> {
    arena: Arena<'e, A>,
}

impl<'e, A: Alloc> Message<'e, A> {
    /// Creates a new message backed by the specified allocator
    pub fn new(alloc: A) -> Self {
        Self {
            arena: Arena::new(alloc),
        }
    }

    /// Returns the segments of the message, or None if a root segment hasn't been allocated yet.
    pub fn segments(&self) -> Option<MessageSegments> {
        self.arena.segments()
    }

    /// Creates a new reader for this message.
    pub fn reader<'b>(&'b self) -> Reader<'b> {
        Reader {
            arena: self.arena.reader(),
            limiter: None,
            nesting_limit: u32::MAX,
        }
    }

    pub fn reader_with_options<'b>(&'b self, options: ReaderOptions) -> Reader<'b> {
        Reader {
            arena: self.arena.reader(),
            limiter: Some(ReadLimiter::new(options.traversal_limit)),
            nesting_limit: options.nesting_limit,
        }
    }

    /// Gets a builder for this message.
    pub fn builder<'b>(&'b mut self) -> Builder<'b, 'e> {
        let builder = self.arena.builder();
        let segment = builder
            .segment_mut(0)
            .unwrap_or_else(|| {
                builder.alloc_in_new_segment(SegmentLen::MIN).1
            });
        Builder {
            e: PhantomData,
            root: segment,
        }
    }
}

impl Message<'_, Growing<Global>> {
    /// Creates a message backed by the default growing global allocator configuration.
    pub fn global() -> Self {
        Self::default()
    }
}

impl<'s> Message<'_, Scratch<'s, Growing<Global>>> {
    /// Creates a message backed by the default growing global allocator configuration and the
    /// given scratch space.
    pub fn with_scratch<const N: usize>(space: &'s mut Space<N>) -> Self {
        Self::new(Scratch::with_space(space, Default::default()))
    }

    /// Creates a message backed by the default growing global allocator configuration and the
    /// given dynamically sized scratch space.
    pub fn with_dyn_scratch(space: &'s mut DynSpace) -> Self {
        Self::new(Scratch::with_dyn_space(space, Default::default()))
    }
}

impl<A: Alloc + Default> Default for Message<'_, A> {
    fn default() -> Self {
        Message::new(A::default())
    }
}

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

impl Default for ReaderOptions {
    fn default() -> Self {
        Self {
            traversal_limit: 8 * 1024 * 1024,
            nesting_limit: 64,
        }
    }
}

pub struct Reader<'a> {
    arena: internal::ArenaReader<'a>,
    limiter: Option<ReadLimiter>,
    nesting_limit: u32,
}

impl Reader<'_> {
    pub fn root(&self) -> PtrReader<Empty> {
        self.root_option().unwrap_or_default()
    }

    pub fn root_option(&self) -> Option<PtrReader<Empty>> {
        self.arena
            .get(0)
            .map(|s| PtrReader::root(s, self.limiter.as_ref(), self.nesting_limit))
    }
}

#[derive(Clone)]
pub struct MessageSegments<'b> {
    pub first: &'b [Word],
    pub remainder: RemainderSegments<'b>,
}

impl<'b> MessageSegments<'b> {
    pub fn first_bytes(&self) -> &'b [u8] {
        unsafe {
            // i'm very lazy
            let (_, bytes, _) = self.first.align_to();
            bytes
        }
    }

    pub fn len(&self) -> u32 {
        self.remainder.len() + 1
    }

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

#[derive(Clone)]
pub struct RemainderSegments<'b> {
    segments: &'b Vec<NonNull<ArenaSegment>>,
}

impl<'b> RemainderSegments<'b> {
    pub fn is_empty(&self) -> bool {
        self.segments.is_empty()
    }
    pub fn len(&self) -> u32 {
        self.segments.len() as u32
    }
    fn iter(&self) -> RemainderIter<'b> {
        self.segments
            .iter()
            .map(|s| unsafe { s.as_ref().as_slice() })
    }
}

type Iter<'b> = iter::Chain<iter::Once<&'b [Word]>, RemainderIter<'b>>;
type RemainderIter<'b> = iter::Map<
    slice::Iter<'b, NonNull<ArenaSegment>>,
    fn(&'b NonNull<ArenaSegment>) -> &'b [Word],
>;

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

pub struct Builder<'b, 'e: 'b> {
    e: PhantomData<&'e [Word]>,
    root: SegmentBuilder<'b>,
}

impl<'b, 'e: 'b> Builder<'b, 'e> {
    pub fn segment_inserter(&self) -> ExternalSegmentInserter<'e, 'b> {
        ExternalSegmentInserter {
            e: PhantomData,
            arena: self.root.arena().clone(),
        }
    }

    /// Returns the root pointer for the message.
    pub fn into_root(self) -> any::PtrBuilder<'b, Empty> {
        PtrBuilder::root(self.root).into()
    }
}

/// A type that can be used to create external read-only segments in a message.
pub struct ExternalSegment<'e> {
    e: PhantomData<&'e [Word]>,
    segment: Segment,
}

impl<'e> ExternalSegment<'e> {
    pub unsafe fn from_raw_parts(ptr: NonNull<Word>, len: SegmentLen) -> Self {
        Self {
            e: PhantomData,
            segment: Segment::new(ptr, len),
        }
    }

    /// Attempts to make an external segment from the given bytes. This requires that:
    ///
    /// * The data be aligned to Words
    /// * The length is divisible by 8
    /// * The length (in words) is a valid segment length
    ///
    /// Otherwise, the function returns None.
    pub fn from_bytes(b: &'e [u8]) -> Option<Self> {
        let ([], words, []) = (unsafe { b.align_to::<Word>() }) else { return None };
        let slen32: u32 = words.len().try_into().ok()?;
        let len = SegmentLen::new(slen32)?;
        Some(unsafe { Self::from_raw_parts(NonNull::from(&words[0]), len) })
    }
}

/// Allows for the insertion of borrowed external segments into a message builder in the form of
/// data orphans.
///
/// This is kept separate from Orphanages since external segments require a separate lifetime
/// (normally refered to as `'e`). But bringing that lifetime around just for orphanages and only
/// for external segments is a massive cognitive drain for users of the library.
///
/// Note, this only applies to *borrowed* external segments. External segments with a static
/// lifetime can be inserted through a normal orphanage. This is only needed for external segments
/// without a static lifetime.
pub struct ExternalSegmentInserter<'b, 'e: 'b> {
    e: PhantomData<&'e [Word]>,
    arena: BuilderArena<'b>,
}

impl<'b, 'e: 'b> ExternalSegmentInserter<'b, 'e> {
    pub fn insert(&self, segment: ExternalSegment<'e>) -> PhantomData<&'b ()> {
        todo!()
    }
}

/*
struct ReaderSegment<'a> {
    a: PhantomData<&'a [Word]>,
    segment: Segment,
    drop: fn(Segment),
}

impl ReaderSegment<'static> {
    pub fn from_box(b: Box<[Word]>) -> Result<Self, Box<[Word]>> {
        let len = if let Some(len) = u32::try_from(b.len()).ok().and_then(SegmentLen::new) {
            len
        } else {
            return Err(b);
        };

        let raw = Box::into_raw(b);

        let segment = unsafe { Segment::new(NonNull::new_unchecked(raw.as_mut_ptr()), len) };
        Ok(Self {
            a: PhantomData,
            segment,
            drop: |s| unsafe {
                let slice = slice::from_raw_parts_mut(s.data().as_ptr(), s.len().get() as usize);
                drop(Box::<[Word]>::from_raw(slice))
            },
        })
    }
}

impl Drop for ReaderSegment<'_> {
    fn drop(&mut self) {
        drop(self.segment.clone())
    }
}

unsafe impl Send for ReaderSegment<'_> {}
unsafe impl Sync for ReaderSegment<'_> {}

/// A set of read-only segments used by a MessageReader.
struct SegmentSet<'a> {
    first: ReaderSegment<'a>,
    tail: Vec<Option<ReaderSegment<'a>>>,
}

impl ArenaReader for SegmentSet<'_> {
    fn get(&self, id: SegmentId) -> Option<SegmentReader> {
        let segment = match id {
            0 => &self.first,
            id => self.tail.get((id - 1) as usize)?.as_ref()?,
        }
        .segment
        .clone();

        Some(SegmentReader::new(self, segment))
    }
}

/// A readable message. Can be used to create multiple readers.
///
/// Like [Message], this type exists to remove the need for interior mutability in a root "reader"
/// type. This type cannot be used to read the underlying message, instead it can only be used to
/// create readers, which are locked to a single thread from interior mutability requirements.
pub struct MessageReader<'a> {
    a: PhantomData<&'a [Word]>,
    segments: SegmentSet<'a>,
}

impl<'a> MessageReader<'a> {
    /// Creates a new reader for this message.
    pub fn reader<'b>(&'b self, options: ReaderOptions) -> Reader<'b> {
        todo!()
    }
}

pub struct MessageReaderBuilder<'a> {
    a: PhantomData<&'a [Word]>,
}
*/
