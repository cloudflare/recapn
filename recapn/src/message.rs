//! An arena of segments containing Cap'n Proto data.

use crate::alloc::{
    AllocLen, Segment, SegmentOffset, Space, Word
};
use crate::arena::{SegmentId, ArenaSegment, BuildArena, ReadArena};
use crate::orphan::Orphanage;
use crate::{any, ty, ReaderOf};
use crate::ptr::{ObjectBuilder, PtrBuilder, PtrReader};
use crate::rpc::{Empty, InsertableInto};
use core::cell::{Cell, UnsafeCell};
use core::fmt::Debug;
use core::marker::PhantomData;

#[cfg(feature = "alloc")]
use crate::{
    alloc::{Alloc, DynSpace, Global, Growing, Scratch},
    arena::growing::Arena,
};

struct SingleSegmentArena {
    segment: UnsafeCell<ArenaSegment>,
}

impl SingleSegmentArena {
    fn new(segment: Segment) -> Self {
        Self {
            segment: UnsafeCell::new(unsafe {
                ArenaSegment::new(segment, AllocLen::ONE, 0)
            })
        }
    }

    fn segment(&self) -> &ArenaSegment {
        unsafe { &*self.segment.get() }
    }
}

unsafe impl BuildArena for SingleSegmentArena {
    fn alloc(&self, min_size: AllocLen) -> Option<(SegmentOffset, &ArenaSegment)> {
        let segment = self.segment();
        let place = segment.try_use_len(min_size)?;
        Some((place, segment))
    }
    fn segment(&self, id: SegmentId) -> Option<&ArenaSegment> {
        if id != 0 {
            return None
        }

        Some(self.segment())
    }
    fn insert_external_segment(&self, _: Segment) -> Option<SegmentId> {
        None
    }
    fn remove_external_segment(&self, _: SegmentId) {
        unimplemented!("external segments cannot be inserted, so they can't be removed")
    }
    fn as_read_arena(&self) -> &dyn ReadArena {
        self
    }
    fn size_in_words(&self) -> usize {
        self.segment().used_len().get() as usize
    }
    fn len(&self) -> u32 { 1 }
}

unsafe impl ReadArena for SingleSegmentArena {
    fn segment(&self, id: SegmentId) -> Option<Segment> {
        if id != 0 {
            return None
        }

        Some(self.segment().used_segment())
    }
    fn size_in_words(&self) -> usize {
        self.segment().used_len().get() as usize
    }
}

/// A fixed sized message that can write to one segment.
pub struct SingleSegmentMessage<'a> {
    a: PhantomData<&'a mut [Word]>,
    arena: SingleSegmentArena,
}

impl<'a> SingleSegmentMessage<'a> {
    pub fn with_space<const N: usize>(space: &'a mut Space<N>) -> Self {
        Self { a: PhantomData, arena: SingleSegmentArena::new(space.segment()) }
    }

    #[cfg(feature = "alloc")]
    pub fn with_dyn_space(space: &'a mut DynSpace) -> Self {
        Self { a: PhantomData, arena: SingleSegmentArena::new(space.segment()) }
    }

    /// Creates a new reader for this message. This reader has no limits placed on it.
    ///
    /// If you want a limited reader, use [`Message::reader_with_options`].
    #[inline]
    pub fn reader(&self) -> Reader {
        Reader::limitless(&self.arena)
    }

    /// Creates a new reader for this message with the specified reader options.
    #[inline]
    pub fn reader_with_options(&self, options: ReaderOptions) -> Reader {
        Reader::new(&self.arena, options)
    }

    /// Gets the builder for this message.
    #[inline]
    pub fn builder(&mut self) -> Builder<'_, 'static> {
        Builder {
            e: PhantomData,
            root: self.arena.segment(),
            arena: &self.arena,
        }
    }

    /// Return the used space of the message as a slice of [`Word`]s.
    pub fn as_words(&self) -> &[Word] {
        unsafe { self.arena.segment().used_segment().as_slice_unchecked() }
    }

    /// Return the used space of the message as a slice of bytes.
    pub fn as_bytes(&self) -> &[u8] {
        Word::slice_to_bytes(self.as_words())
    }
}

// These are safe since, even though this type has interior mutability, we don't expose any of
// it from this level of the API. So a user can pass instances of this type between threads
// as much as they want (though they probably won't).
unsafe impl Send for SingleSegmentMessage<'_> {}
unsafe impl Sync for SingleSegmentMessage<'_> {}

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
#[cfg(feature = "alloc")]
pub struct Message<'e, A: Alloc + ?Sized> {
    /// A phantom lifetime from external readonly segments.
    e: PhantomData<&'e [Word]>,
    arena: Arena<A>,
}

#[cfg(feature = "alloc")]
impl<'e, A: Alloc> Message<'e, A> {
    /// Creates a new message backed by the specified allocator
    #[inline]
    pub const fn new(alloc: A) -> Self {
        Self {
            e: PhantomData,
            arena: Arena::new(alloc),
        }
    }
}

#[cfg(feature = "alloc")]
impl<'e, A: Alloc + ?Sized> Message<'e, A> {
    /// Returns the segments of the message, or None if a root segment hasn't been allocated yet.
    #[inline]
    pub fn segments(&self) -> Option<MessageSegments> {
        let (first, arena) = self.arena.try_root_and_build()?;
        Some(MessageSegments { first, arena })
    }

    /// Return the message's internal arena as a `ReadArena` trait object reference.
    #[inline]
    pub fn as_read_arena(&self) -> &dyn ReadArena {
        self.arena.as_read()
    }

    /// Creates a new reader for this message. This reader has no limits placed on it.
    ///
    /// If you want a limited reader, use [`Message::reader_with_options`].
    #[inline]
    pub fn reader(&self) -> Reader {
        Reader::limitless(self.arena.as_read())
    }

    /// Creates a new reader for this message with the specified reader options.
    #[inline]
    pub fn reader_with_options(&self, options: ReaderOptions) -> Reader {
        Reader::new(self.arena.as_read(), options)
    }

    /// Gets the builder for this message. This will allocate the root segment if it doesn't exist.
    #[inline]
    pub fn builder<'b>(&'b mut self) -> Builder<'b, 'e> {
        let (root, arena) = self.arena.root_and_build();
        Builder { e: PhantomData, root, arena }
    }

    /// Clears the message, deallocating all the segments within it.
    pub fn clear(&mut self) {
        self.arena.clear();
    }
}

#[cfg(feature = "alloc")]
impl Message<'_, Growing<Global>> {
    /// Creates a message backed by the default growing global allocator configuration. This is
    /// the configuration similar to the default configuration of the `MallocMessageBuilder` in
    /// Cap'n Proto C++.
    #[inline]
    pub fn global() -> Self {
        Self::new(Growing::new(
            Growing::<()>::DEFAULT_FIRST_SEGMENT_LEN,
            Global,
        ))
    }
}

#[cfg(feature = "alloc")]
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

#[cfg(feature = "alloc")]
impl<A: Alloc + Default> Default for Message<'_, A> {
    /// Creates a default instance of the allocator and returns a new message backed by it.
    #[inline]
    fn default() -> Self {
        Message::new(A::default())
    }
}

#[cfg(feature = "alloc")]
unsafe impl<A: Alloc + Send + ?Sized> Send for Message<'_, A> {}
#[cfg(feature = "alloc")]
unsafe impl<A: Alloc + ?Sized> Sync for Message<'_, A> {}

pub struct Builder<'b, 'e: 'b> {
    e: PhantomData<&'e [Word]>,
    root: &'b ArenaSegment,
    arena: &'b dyn BuildArena,
}

impl<'b, 'e: 'b> Builder<'b, 'e> {
    pub fn by_ref<'c>(&'c mut self) -> Builder<'c, 'e> {
        Builder {
            e: PhantomData,
            root: self.root,
            arena: self.arena,
        }
    }

    pub fn init_struct_root<S: ty::Struct>(self) -> S::Builder<'b, Empty> {
        let root = self.into_root();
        let root_raw = crate::ptr::PtrBuilder::from(root);
        let builder = root_raw.init_struct(<S as ty::Struct>::SIZE);
        unsafe { ty::StructBuilder::from_ptr(builder) }
    }

    pub fn set_struct_root<S, T>(self, reader: &S::Reader<'_, impl InsertableInto<Empty>>)
    where
        S: ty::Struct,
    {
        let root = self.into_root();
        let mut root_raw = crate::ptr::PtrBuilder::from(root);
        root_raw.set_struct(reader.as_ref(), crate::ptr::CopySize::Minimum(S::SIZE));
    }

    /// Returns the root pointer for the message.
    pub fn into_root(self) -> any::PtrBuilder<'b> {
        self.into_parts().root
    }

    pub fn into_parts(self) -> BuilderParts<'b> {
        BuilderParts {
            root: PtrBuilder::root(self.root, self.arena).into(),
            orphanage: Orphanage::new(ObjectBuilder::new(self.root, self.arena)),
        }
    }

    pub fn segments(&self) -> MessageSegments {
        MessageSegments {
            first: self.root,
            arena: self.arena,
        }
    }

    pub fn size_in_words(&self) -> usize {
        self.arena.size_in_words()
    }
}

/// The raw components of the builder, including the root pointer, the root lifetime orphanage,
/// and the external segment inserter.
pub struct BuilderParts<'b> {
    pub root: any::PtrBuilder<'b>,
    pub orphanage: Orphanage<'b>,
}

/// The internal segments of a built message. This always contains at least one segment.
#[derive(Clone)]
pub struct MessageSegments<'b> {
    first: &'b ArenaSegment,
    arena: &'b dyn BuildArena,
}

impl<'b> MessageSegments<'b> {
    #[inline]
    pub fn first(&self) -> MessageSegment<'b> {
        MessageSegment(self.first)
    }

    #[inline]
    pub fn len(&self) -> u32 {
        self.arena.len()
    }
}

pub struct MessageSegment<'b>(&'b ArenaSegment);

impl<'b> MessageSegment<'b> {
    #[inline]
    pub fn len(&self) -> u32 {
        self.0.used_len().get()
    }

    #[inline]
    pub fn as_words(&self) -> &'b [Word] {
        unsafe { self.0.used_segment().as_slice_unchecked() }
    }

    #[inline]
    pub fn as_bytes(&self) -> &'b [u8] {
        Word::slice_to_bytes(self.as_words())
    }
}

impl<'b> IntoIterator for MessageSegments<'b> {
    type IntoIter = SegmentsIter<'b>;
    type Item = MessageSegment<'b>;

    fn into_iter(self) -> Self::IntoIter {
        SegmentsIter { arena: Some(self.arena), curr: 0 }
    }
}

pub struct SegmentsIter<'b> {
    arena: Option<&'b dyn BuildArena>,
    curr: SegmentId,
}

impl<'b> Iterator for SegmentsIter<'b> {
    type Item = MessageSegment<'b>;

    fn next(&mut self) -> Option<Self::Item> {
        let arena = self.arena?;
        let segment = arena.segment(self.curr);
        if segment.is_none() {
            self.arena = None;
        }
        self.curr += 1;
        segment.map(MessageSegment)
    }
}

/// Options controlling how data is read
#[derive(Debug, Clone)]
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

/// A type used to read a message on a single thread.
pub struct Reader<'a> {
    limiter: Option<ReadLimiter>,
    nesting_limit: u32,
    message: &'a dyn ReadArena,
}

impl<'a> Reader<'a> {
    /// Creates a message reader with the specified limits.
    pub const fn new(message: &'a dyn ReadArena, options: ReaderOptions) -> Self {
        Self {
            limiter: Some(ReadLimiter::new(options.traversal_limit)),
            nesting_limit: options.nesting_limit,
            message,
        }
    }

    /// Creates a message reader with no limits applied.
    /// 
    /// Note: You should only use this if you trust the source of this data! Untrusted
    /// data can perform amplification attacks and stack overflow DoS attacks while
    /// reading the data, so you should only use this method if you want to read some
    /// data that you know is safe. For instance: data built yourself using Message
    /// is generally trusted, so Message::reader uses this constructor.
    pub const fn limitless(message: &'a dyn ReadArena) -> Self {
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
        let ptr = PtrReader::root(&self.message, self.limiter.as_ref(), self.nesting_limit)?;
        Some(ptr.into())
    }

    /// Returns the root and interprets it as the specified pointer type.
    #[inline]
    pub fn read_as<'b, T: ty::FromPtr<any::PtrReader<'b>>>(&'b self) -> T::Output {
        self.root().read_as::<T>()
    }

    #[inline]
    pub fn read_as_struct<S: ty::Struct>(&self) -> ReaderOf<'_, S> {
        self.root().read_as_struct::<S>()
    }

    /// Calculates the message's total size in words.
    #[inline]
    pub fn size_in_words(&self) -> usize {
        self.message.size_in_words()
    }
}
