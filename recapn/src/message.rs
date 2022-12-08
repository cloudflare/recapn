use crate::alloc::{
    Alloc, AllocLen, DynSpace, Global, Growing, ObjectLen, Scratch, Segment, SegmentLen,
    SegmentOffset, Space, Word,
};
use crate::ptr::{PtrBuilder, PtrReader};
use crate::rpc::Empty;
use core::cell::UnsafeCell;
use core::convert::TryInto;
use core::fmt;
use core::iter;
use core::marker::PhantomData;
use core::pin::Pin;
use core::ptr::NonNull;
use core::slice;
use core::sync::atomic::{AtomicU64, Ordering};

pub type SegmentId = u32;

pub(crate) mod internal {
    use crate::alloc::SignedSegmentOffset;

    use super::*;

    /// A reference to a location in a segment.
    #[derive(Clone, Copy, PartialEq, Eq, Debug)]
    pub struct SegmentRef<'a> {
        a: PhantomData<&'a Word>,
        ptr: NonNull<Word>,
    }

    impl<'a> SegmentRef<'a> {
        /// Creates a dangling ref. This does not refer to any valid word, so it must not be
        /// derefed. Useful for zero-sized types such as empty lists or empty structs.
        pub const unsafe fn dangling() -> Self {
            Self::from_ptr(NonNull::dangling())
        }

        pub const unsafe fn from_ptr(ptr: NonNull<Word>) -> Self {
            Self {
                a: PhantomData,
                ptr,
            }
        }

        pub fn as_ptr(&self) -> *const Word {
            self.ptr.as_ptr().cast_const()
        }

        pub fn as_ptr_mut(&self) -> *mut Word {
            self.ptr.as_ptr()
        }

        pub fn as_nonnull(&self) -> NonNull<Word> {
            self.ptr
        }

        pub const unsafe fn offset(self, offset: SignedSegmentOffset) -> Self {
            Self::from_ptr(NonNull::new_unchecked(
                self.ptr.as_ptr().offset(offset.get() as isize),
            ))
        }

        pub unsafe fn offset_from_end(self, offset: SignedSegmentOffset) -> Self {
            self.offset(1.into()).offset(offset)
        }

        pub unsafe fn deref(&self) -> &'a Word {
            self.ptr.as_ref()
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
                        id => segments.tail.get((id - 1) as usize)?,
                    }
                    .used_segment()?;

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

        #[inline]
        fn contains_ref(&self, r: SegmentRef) -> bool {
            self.contains(r.ptr.as_ptr().cast_const(), ObjectLen::new(1).unwrap())
        }

        #[inline]
        pub fn deref_word<'b>(&self, r: SegmentRef<'b>) -> &'b Word {
            debug_assert!(self.contains_ref(r));
            unsafe { r.ptr.as_ref() }
        }

        #[inline]
        pub fn try_offset_from<'b>(
            &self,
            r: SegmentRef<'b>,
            offset: SignedSegmentOffset,
            len: ObjectLen,
        ) -> Option<SegmentRef<'b>> {
            debug_assert!(self.contains_ref(r));

            let ptr = r.ptr.as_ptr().cast_const();
            let offset_ptr = ptr.wrapping_offset(offset.get() as isize);
            self.contains(offset_ptr, len).then(|| SegmentRef {
                a: PhantomData,
                ptr: unsafe { NonNull::new_unchecked(offset_ptr.cast_mut()) },
            })
        }

        /// Gets a segment reader for the segment with the specified ID, or None
        /// if the segment doesn't exist.
        #[inline]
        pub fn segment(&self, id: SegmentId) -> Option<SegmentReader<'a>> {
            self.arena.get(id)
        }

        #[inline]
        pub fn contains(&self, ptr: *const Word, len: ObjectLen) -> bool {
            let range = self.segment.to_ptr_range();
            let end = ptr.wrapping_offset(len.get() as isize);
            range.contains(&ptr) && range.contains(&end)
        }

        #[inline]
        pub fn try_get_section_offset(
            &self,
            offset: SegmentOffset,
            len: ObjectLen,
        ) -> Option<SegmentRef<'a>> {
            // find out where the end off the section is supposed to be and make sure it's not beyond
            // the length of the segment
            let end_offset = offset
                .get()
                .checked_add(len.get())
                .and_then(SegmentOffset::new)?;
            if end_offset > self.segment.len() {
                return None;
            }

            Some(SegmentRef {
                a: PhantomData,
                ptr: unsafe { self.segment.add_offset(offset) },
            })
        }
    }

    impl fmt::Debug for SegmentReader<'_> {
        fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
            self.segment.fmt(f)
        }
    }

    pub struct ArenaSegment {
        segment: Segment,
        /// A segment offset indicating the used space in the segment,
        /// or some flags. READ_ONLY_FLAG is used to indicate it's a
        /// read-only segment, DELETED_FLAG is used to indicate the
        /// segment was read-only, but has now been deleted.
        used_len_or_flags: u32,
        segment_id: SegmentId,
    }

    impl ArenaSegment {
        const READ_ONLY_FLAG: u32 = u32::MAX;

        pub fn new(segment: Segment, read_only: bool, id: SegmentId) -> Self {
            Self {
                segment,
                used_len_or_flags: if read_only { Self::READ_ONLY_FLAG } else { 0 },
                segment_id: id,
            }
        }

        pub fn is_read_only(&self) -> bool {
            self.used_len_or_flags > SegmentOffset::MAX_VALUE
        }

        pub fn id(&self) -> SegmentId {
            self.segment_id
        }

        pub fn segment(&self) -> &Segment {
            &self.segment
        }

        pub fn used_len(&self) -> SegmentOffset {
            if self.is_read_only() {
                self.segment().len().into()
            } else {
                SegmentOffset::new(self.used_len_or_flags).unwrap()
            }
        }

        /// Returns a segment of the used data in this segment.
        ///
        /// If no data has been used, this returns None
        pub fn used_segment(&self) -> Option<Segment> {
            let len = SegmentLen::new(self.used_len().get())?;
            unsafe { Some(Segment::new(self.segment.data(), len)) }
        }

        pub fn as_slice(&self) -> &[Word] {
            if let Some(segment) = self.used_segment() {
                unsafe {
                    core::slice::from_raw_parts(
                        segment.data().as_ptr().cast_const(),
                        segment.len().get() as usize,
                    )
                }
            } else {
                &[]
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
        segments: Option<ArenaSegments>,
        /// The last owned segment in this set
        last_owned: SegmentId,
        alloc: A,
    }

    /// An append-only arena for a set of message segments, managed by an Arena.
    ///
    /// This mostly serves as an optimization for the likely case where a message contains
    /// exactly one segment, in which case we don't want to have to allocate a vec.
    pub struct ArenaSegments {
        /// Optimize for the first (and possibly only) segment
        first: ArenaSegment,
        /// An optional set of extra segments
        tail: Vec<Pin<Box<ArenaSegment>>>,
    }

    impl<'e, A: Alloc> Arena<'e, A> {
        pub fn new(alloc: A) -> Self {
            Self {
                e: PhantomData,
                inner: UnsafeCell::new(ArenaInner {
                    alloc,
                    last_owned: 0,
                    segments: None,
                }),
            }
        }
    }

    impl<'e, A: Alloc> Arena<'e, A> {
        pub fn reader(&self) -> ArenaReader {
            let borrow = unsafe { &*self.inner.get() };

            ArenaReader::FromBuilder(&borrow.segments)
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
            let alloc = &mut self.alloc;
            if let Some(segments) = self.segments.as_mut() {
                let first = &segments.first;
                unsafe {
                    alloc.dealloc(first.segment().clone());
                }

                for segment in segments.tail.drain(..) {
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
    #[derive(Clone)]
    pub struct BuilderArena<'b> {
        /// DON'T ACCESS THIS FIELD DIRECTLY. Use arena() or arena_mut() instead.
        inner: &'b UnsafeCell<ArenaInner<dyn Alloc>>,
    }

    impl<'b> BuilderArena<'b> {
        const MAX_SEGMENTS: usize = (u32::MAX - 1) as usize;

        /// Allocates a segment of at least `min_size` words.
        pub fn alloc_segment(&self, min_size: SegmentLen) -> SegmentBuilder<'b> {
            unsafe {
                let inner = self.arena_mut();
                let new_segment = inner.alloc.alloc(min_size);
                let ptr = match inner.segments {
                    ref mut s @ None => {
                        let segment = ArenaSegment::new(new_segment, false, 0);
                        let segments = s.insert(ArenaSegments {
                            first: segment,
                            tail: Vec::new(),
                        });
                        // this pointer is fine, the reference will stick around at least as long
                        // as the associated arena borrow
                        NonNull::from(&segments.first)
                    }
                    Some(ref mut segments) => {
                        if segments.tail.len() > Self::MAX_SEGMENTS {
                            panic!("Too many segments allocated in message!");
                        }

                        let next_id = (segments.tail.len() + 1) as u32;

                        inner.last_owned = next_id;

                        let boxed = Box::pin(ArenaSegment::new(new_segment, false, next_id));
                        let ptr = NonNull::from(boxed.as_ref().get_ref());

                        segments.tail.push(boxed);

                        ptr
                    }
                };
                SegmentBuilder {
                    segment: ptr,
                    arena: self.clone(),
                }
            }
        }

        fn arena(&self) -> &ArenaInner<dyn Alloc + 'b> {
            unsafe { core::mem::transmute(&*self.inner.get()) }
        }

        unsafe fn arena_mut(&self) -> &mut ArenaInner<dyn Alloc + 'b> {
            unsafe { core::mem::transmute(&mut *self.inner.get()) }
        }

        fn last_owned(&self) -> SegmentId {
            self.arena().last_owned
        }

        /// Allocate an object of size somewhere in the message, returning a pointer to the
        /// allocated space and a segment builder for the segment the space was allocated in.
        pub fn alloc(&self, size: AllocLen) -> (SegmentRef<'b>, SegmentBuilder<'b>) {
            if let Some(builder) = self.segment_mut(self.last_owned()) {
                if let Some(space) = builder.alloc_in_segment(size) {
                    return (space, builder);
                }
            }

            // If "last_segment_id" turned up nothing, we must be the first segment in the message
            // If no space could be allocated in the segment, we need a new one
            let builder = self.alloc_segment(size);
            let space = builder
                .alloc_in_segment(size)
                .expect("failed to allocate enough space in new segment");
            (space, builder)
        }

        pub fn segment_mut(&self, id: SegmentId) -> Option<SegmentBuilder<'b>> {
            let inner = unsafe { &mut *self.inner.get() };
            let segments = inner.segments.as_mut()?;
            let segment = match id {
                0 => NonNull::from(&mut segments.first),
                id => {
                    let index = (id - 1) as usize;
                    let segment = &mut segments.tail[index];
                    if segment.is_read_only() {
                        return None;
                    }

                    let inner_ref = segment.as_mut();
                    NonNull::from(inner_ref.get_mut())
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
        fn segment(&self) -> &ArenaSegment {
            unsafe { self.segment.as_ref() }
        }

        pub fn as_reader<'c>(&'c self) -> SegmentReader<'c> {
            let arena = ArenaReader::FromBuilder(&self.arena.arena().segments);
            SegmentReader {
                arena,
                segment: self.segment().segment().clone(),
            }
        }

        pub fn id(&self) -> SegmentId {
            unsafe { self.segment.as_ref().id() }
        }

        pub fn contains(&self, r: SegmentRef<'b>) -> bool {
            self.segment()
                .segment()
                .to_ptr_range()
                .contains(&r.as_ptr())
        }

        pub fn contains_section(&self, r: SegmentRef<'b>, offset: SignedSegmentOffset) -> bool {
            todo!()
        }

        pub fn contains_range(&self, start: SegmentRef<'b>, end: SegmentRef<'b>) -> bool {
            todo!()
        }

        pub fn arena(&self) -> &BuilderArena<'b> {
            &self.arena
        }

        pub fn at_offset(&self, offset: SegmentOffset) -> SegmentRef<'b> {
            let r = unsafe { self.start().offset(offset.into()) };
            debug_assert!(self.contains(r));
            r
        }

        pub fn offset_from_start(&self, to: SegmentRef<'b>) -> SegmentOffset {
            let signed_offset = self.offset_from(self.start(), to);
            SegmentOffset::new(signed_offset.get() as u32).unwrap()
        }

        #[inline]
        pub fn start(&self) -> SegmentRef<'b> {
            unsafe { SegmentRef::from_ptr(self.segment().segment().data()) }
        }

        #[inline]
        pub fn end(&self) -> SegmentRef<'b> {
            unsafe { self.start().offset(self.segment().used_len().into()) }
        }

        #[inline]
        fn offset_from(&self, from: SegmentRef<'b>, to: SegmentRef<'b>) -> SignedSegmentOffset {
            debug_assert!(self.contains_range(from, to));
            unsafe {
                let offset = to.as_ptr().offset_from(from.as_ptr().add(1));
                SignedSegmentOffset::new_unchecked(offset as i32)
            }
        }

        /// Calculates the segment offset from the the end of `from` to the ptr `to`
        #[inline]
        pub fn offset_from_end_of(
            &self,
            from: SegmentRef<'b>,
            to: SegmentRef<'b>,
        ) -> SignedSegmentOffset {
            self.offset_from(unsafe { from.offset(1.into()) }, to)
        }

        /// Allocates space of the given size _somewhere_ in the message.
        pub fn alloc(&self, size: AllocLen) -> (SegmentRef<'b>, SegmentBuilder<'b>) {
            if let Some(word) = self.alloc_in_segment(size) {
                (word, self.clone())
            } else {
                self.arena.alloc(size)
            }
        }

        /// Attempt to allocate the given space in this segment.
        pub fn alloc_in_segment(&self, size: AllocLen) -> Option<SegmentRef<'b>> {
            let segment = self.segment();
            if segment.is_read_only() {
                return None;
            }

            let start_alloc_offset = segment.used_len();
            let new_used_len = start_alloc_offset.get().checked_add(size.get())?;
            if new_used_len > segment.segment().len().get() {
                return None;
            }

            let start = unsafe { self.start().offset(start_alloc_offset.into()) };
            Some(start)
        }
    }

    /// Controls how far a single message reader can read into a message.
    #[derive(Debug)]
    pub struct ReadLimiter {
        limit: AtomicU64,
    }

    impl ReadLimiter {
        #[inline]
        pub const fn unlimited() -> Self {
            Self {
                limit: AtomicU64::new(u64::max_value()),
            }
        }

        #[inline]
        pub const fn new(word_count: u64) -> Self {
            Self {
                limit: AtomicU64::new(word_count),
            }
        }

        #[inline]
        pub fn reset(&self, word_count: u64) {
            self.limit.store(word_count, Ordering::Relaxed)
        }

        /// Attempts to read `words` from the limiter, returning a [`bool`](std::primitive::bool) indicating whether the read was successful.
        #[inline]
        pub fn try_read(&self, words: u64) -> bool {
            let current = self.limit.load(Ordering::Relaxed);
            if words <= current {
                self.limit.store(current - words, Ordering::Relaxed);
                return true;
            }

            false
        }

        /// Adds some words back onto the limit, useful if you know you're going to double read some data.
        #[inline]
        pub fn unread(&self, words: u64) {
            let old = self.limit.load(Ordering::Relaxed);
            let new = old.saturating_add(words);
            self.limit.store(new, Ordering::Relaxed);
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
        todo!()
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
        Builder {
            e: PhantomData,
            arena: self.arena.builder(),
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
    segments: &'b Vec<Pin<Box<ArenaSegment>>>,
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
            .map(|s| s.as_ref().get_ref().as_slice())
    }
}

type Iter<'b> = iter::Chain<iter::Once<&'b [Word]>, RemainderIter<'b>>;
type RemainderIter<'b> = iter::Map<
    slice::Iter<'b, Pin<Box<ArenaSegment>>>,
    fn(&'b Pin<Box<ArenaSegment>>) -> &'b [Word],
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
    fn fold<B, F>(self, init: B, f: F) -> B
    where
        Self: Sized,
        F: FnMut(B, Self::Item) -> B,
    {
        self.inner.fold(init, f)
    }
    #[inline]
    fn nth(&mut self, n: usize) -> Option<Self::Item> {
        self.inner.nth(n)
    }
    #[inline]
    fn find<P>(&mut self, predicate: P) -> Option<Self::Item>
    where
        Self: Sized,
        P: FnMut(&Self::Item) -> bool,
    {
        self.inner.find(predicate)
    }
    #[inline]
    fn last(self) -> Option<Self::Item>
    where
        Self: Sized,
    {
        self.inner.last()
    }
    #[inline]
    fn size_hint(&self) -> (usize, Option<usize>) {
        self.inner.size_hint()
    }
}

pub struct Builder<'b, 'e: 'b> {
    e: PhantomData<&'e [Word]>,
    arena: BuilderArena<'b>,
}

impl<'b, 'e: 'b> Builder<'b, 'e> {
    pub fn segment_inserter(&self) -> ExternalSegmentInserter<'e, 'b> {
        ExternalSegmentInserter {
            e: PhantomData,
            arena: self.arena.clone(),
        }
    }

    /// Returns the root pointer for the message.
    pub fn into_root(self) -> PtrBuilder<'b, Empty> {
        PtrBuilder::root(self.arena.segment_mut(0).unwrap())
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
