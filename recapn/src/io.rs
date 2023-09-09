//! Types for serializing and deserializing Cap'n Proto messages in standard framing format

use alloc_crate::borrow::Borrow;
use core::fmt::{self, Debug};
use core::num::NonZeroU32;

use thiserror::Error;

use crate::alloc::{SegmentLen, Word};
use crate::message::MessageSegments;
use crate::ptr::WireValue;

const INLINE_TABLE_SIZE: usize = 3;

#[derive(Clone)]
enum Table {
    /// An inline table that can contain a table with 4 segments
    Inline {
        len: u8,
        array: [Word; INLINE_TABLE_SIZE],
    },
    Heap(Vec<Word>),
}

impl Table {
    pub const fn empty() -> Self {
        Table::Inline {
            len: 1,
            array: [Word::NULL; INLINE_TABLE_SIZE],
        }
    }

    /// Resize the table to have at least `len` words
    #[inline]
    pub fn resize_to(&mut self, len: usize) {
        match self {
            Table::Inline { len: current_len, .. } if len <= INLINE_TABLE_SIZE => {
                let current = *current_len as usize;
                if current < len {
                    *current_len = len as u8;
                }
            }
            Table::Inline { .. } => {
                *self = Table::Heap(vec![Word::NULL; len]);
            } 
            Table::Heap(vec) => {
                if vec.len() < len {
                    vec.resize(len, Word::NULL);
                }
            }
        }
    }

    #[inline]
    pub fn as_slice(&self) -> &[Word] {
        match self {
            Table::Inline { array, len } => &array[..(*len as usize)],
            Table::Heap(boxed) => boxed.as_ref(),
        }
    }

    #[inline]
    pub fn as_slice_mut(&mut self) -> &mut [Word] {
        match self {
            Table::Inline { array, len } => &mut array[..(*len as usize)],
            Table::Heap(boxed) => boxed.as_mut(),
        }
    }
}

/// A buffer that can be used to build a segment table for streaming.
#[derive(Clone)]
pub struct StreamTable {
    table: Table,
}

impl StreamTable {
    /// Creates an "empty" stream table.
    /// 
    /// Though this table is considered "empty", it isn't actually empty to match the invariants
    /// of StreamTableRef, which requires that the table have at least one Word. This Word is
    /// NULL, which makes the table have one empty segment.
    #[inline]
    pub const fn empty() -> Self {
        Self { table: Table::empty(), }
    }

    /// Returns the size of the streaming table in [`Word`]s
    #[inline]
    pub fn size_of(segments: &MessageSegments) -> usize {
        Self::size_from_count(segments.len())
    }

    /// Returns the size of the streaming table in [`Words`]s given the number of segments in the
    /// message
    #[inline]
    pub const fn size_from_count(count: u32) -> usize {
        let count = count as usize;
        (count / 2) + 1
    }

    /// Writes the stream framing table to the specified slice.
    ///
    /// The slice size must match the table size returned by [`size_of`].
    #[inline]
    pub fn write_to(segments: &MessageSegments, slice: &mut [Word]) {
        assert!(
            slice.len() == Self::size_of(segments),
            "provided slice does not match segment table size"
        );

        // Clear the last word first just in case the user decided to not clear the slice before
        // use. This is to clear the 4 padding bytes if they exist.
        slice[slice.len() - 1] = Word::NULL;

        let (count, lens) = WireValue::<u32>::from_word_slice_mut(slice)
            .split_first_mut()
            .unwrap();
        count.set(segments.len() - 1);

        lens.into_iter()
            .zip(segments)
            .for_each(|(len_value, segment)| len_value.set(segment.len() as u32));
    }

    #[inline]
    pub fn new(segments: &MessageSegments) -> Self {
        let mut table = Self::empty();
        table.write_segments(segments);
        table
    }

    /// Writes a new segments table to this instance, reusing the existing allocation, or
    /// reallocating if required.
    #[inline]
    pub fn write_segments(&mut self, segments: &MessageSegments) {
        let size = Self::size_of(segments);
        self.table.resize_to(size);

        Self::write_to(segments, self.table.as_slice_mut());
    }

    #[inline]
    pub fn as_slice(&self) -> &[Word] {
        self.table.as_slice()
    }

    #[inline]
    pub fn as_bytes(&self) -> &[u8] {
        Word::slice_to_bytes(self.as_slice())
    }
}

impl Debug for StreamTable {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        let borrowed: &StreamTableRef = self.borrow();
        borrowed.fmt(f)
    }
}

impl Borrow<StreamTableRef> for StreamTable {
    fn borrow(&self) -> &StreamTableRef {
        let words = self.table.as_slice();
        let wire_values = WireValue::<u32>::from_word_slice(words);
        StreamTableRef::new(wire_values)
    }
}

#[derive(Debug)]
#[repr(transparent)]
pub struct StreamTableRef {
    table: [SegmentLenReader],
}

impl StreamTableRef {
    fn new<'a>(slice: &'a [WireValue<u32>]) -> &'a Self {
        assert!(!slice.is_empty());

        unsafe { &*(slice as *const [WireValue<u32>] as *const StreamTableRef) }
    }

    fn as_wire_values<'a>(&'a self) -> &'a [WireValue<u32>] {
        unsafe { &*(self as *const StreamTableRef as *const [WireValue<u32>]) }
    }

    /// Attempts to read a stream table from the specified slice, returning a reader for the table
    /// and the remainder of the slice.
    ///
    /// If the table cannot be fully read, this returns Err with a usize that indicates how many
    /// more words need to be read to make progress.
    #[inline]
    pub fn try_read<'a>(slice: &'a [Word]) -> Result<(&'a Self, &'a [Word]), TableReadError> {
        let (count, table) = WireValue::<u32>::from_word_slice(slice)
            .split_first()
            .ok_or(TableReadError::Empty)?;

        let count = count
            .get()
            .checked_add(1)
            .ok_or(TableReadError::TooManySegments)?;
        let end_of_table = StreamTable::size_from_count(count);

        let Some(remainder) = slice.get(end_of_table..) else {
            return Err(TableReadError::Incomplete {
                count, required: end_of_table - slice.len()
            })
        };

        let table = &table[..(count as usize)];
        Ok((Self::new(table), remainder))
    }

    #[inline]
    pub fn count(&self) -> NonZeroU32 {
        NonZeroU32::new(self.table.len() as u32).unwrap()
    }

    #[inline]
    pub fn segments(&self) -> &[SegmentLenReader] {
        &self.table
    }

    #[inline]
    pub fn split_first(&self) -> (&SegmentLenReader, &[SegmentLenReader]) {
        self.segments().split_first().unwrap()
    }
}

impl ToOwned for StreamTableRef {
    type Owned = StreamTable;

    fn to_owned(&self) -> Self::Owned {
        let mut table = StreamTable::empty();
        self.clone_into(&mut table);
        table
    }
    /// Clones into the stream table, possibly reusing the allocation that exists.
    fn clone_into(&self, target: &mut Self::Owned) {
        let ref_count = self.count().get();
        let size = StreamTable::size_from_count(ref_count);
        target.table.resize_to(size);

        let (count, target) = WireValue::<u32>::from_word_slice_mut(target.table.as_slice_mut())
            .split_first_mut()
            .unwrap();
        count.set(ref_count - 1);

        let table_wire_values = self.as_wire_values();

        // The slice from the table might contain the 4 byte padding value, but the
        // stream table ref slice won't, which means the lengths won't match which
        // would cause `copy_from_slice` to fail. So we reslice it to make sure we
        // discard that set of padding.
        let target_lens = &mut target[..table_wire_values.len()];
        target_lens.copy_from_slice(table_wire_values);
    }
}

#[repr(transparent)]
pub struct SegmentLenReader(WireValue<u32>);

impl SegmentLenReader {
    #[inline]
    pub fn get_raw(&self) -> u32 {
        self.0.get()
    }

    #[inline]
    pub fn try_get(&self) -> Option<SegmentLen> {
        SegmentLen::new(self.0.get())
    }
}

impl Debug for SegmentLenReader {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        self.0.get().fmt(f)
    }
}

#[derive(Error, Debug)]
pub enum TableReadError {
    /// The segment table indicated it had more segments than can be represented in a message
    /// (`u32::MAX + 1`). This may also be used downstream to indicate that the parser is rejecting
    /// the stream for security reasons.
    #[error("too many segments")]
    TooManySegments,
    /// The input was empty. At least one Word is required.
    #[error("empty input")]
    Empty,
    /// The table was incomplete, the value indicates how many words are required to make progress
    /// when reading the table
    #[error("incomplete table")]
    Incomplete {
        /// The number of segments in the table
        count: u32,
        /// The number of Words required to read the rest of the table
        required: usize,
    },
}