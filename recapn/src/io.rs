//! Helpers for reading and writing Cap'n Proto messages in standard formats.

pub mod packing;
pub mod stream;

use {
    self::packing::StopReason,
    self::stream::{SegmentLenReader, StreamTableRef, TableReadError},
    crate::{
        alloc::{Segment, Word},
        arena::{ReadArena, SegmentId},
        message::ReaderOptions,
    },
    core::{
        fmt::{self, Debug, Display},
        ptr::NonNull,
    },
};

#[cfg(feature = "alloc")]
use {
    crate::alloc::SegmentLen,
    core::ops::Bound,
    rustalloc::{boxed::Box, vec::Vec},
};

#[cfg(feature = "std")]
use {
    crate::message::MessageSegments,
    core::iter,
    packing::{Packer, Unpacker},
    std::io,
    stream::StreamTable,
};

#[derive(Debug)]
pub enum ReadError {
    /// An error occured while reading the stream table
    Table(TableReadError),
    /// A segment in the table indicated it's too large to be read
    SegmentTooLarge {
        /// The ID of the segment
        segment: SegmentId,
    },
    /// The message was too large
    MessageTooLarge,
    /// The message was incomplete
    Incomplete,
}

impl Display for ReadError {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        match self {
            ReadError::Table(e) => Display::fmt(e, f),
            ReadError::SegmentTooLarge { .. } => write!(f, "segment too large"),
            ReadError::MessageTooLarge => write!(f, "message too large"),
            ReadError::Incomplete => write!(f, "incomplete message"),
        }
    }
}

impl From<TableReadError> for ReadError {
    #[inline]
    fn from(value: TableReadError) -> Self {
        ReadError::Table(value)
    }
}

#[cfg(feature = "std")]
impl std::error::Error for ReadError {}

/// A segment set that directly uses the stream table found in the message data without allocating.
///
/// Note: Because this uses the table directly, accessing segments is an O(n) operation where 'n'
/// is the segment ID.
pub struct TableSegmentSet<'a> {
    table: &'a StreamTableRef,
    data: &'a [Word],
}

impl<'a> TableSegmentSet<'a> {
    /// Read a message from the given slice
    pub fn new(words: &'a [Word]) -> Result<(Self, &'a [Word]), ReadError> {
        let (table, data) = StreamTableRef::try_read(words)?;

        // Validate the table and find the end
        let mut total_len = 0usize;
        for (len, id) in table.segments().iter().zip(0u32..) {
            let Some(len) = len.try_get() else {
                return Err(ReadError::SegmentTooLarge { segment: id });
            };
            let Some(new_total) = total_len.checked_add(len.get() as usize) else {
                // Return incomplete since the slice could never contain all the data if this
                // overflows
                return Err(ReadError::Incomplete);
            };
            total_len = new_total;
        }

        if total_len > data.len() {
            return Err(ReadError::Incomplete);
        }

        let (data, remainder) = data.split_at(total_len);
        Ok((Self { table, data }, remainder))
    }
}

unsafe impl ReadArena for TableSegmentSet<'_> {
    fn segment(&self, id: SegmentId) -> Option<Segment> {
        let (segment, leading_segments) = self
            .table
            .segments()
            .get(..=(id as usize))?
            .split_last()
            .unwrap();
        let start = leading_segments
            .iter()
            .map(|l| l.raw() as usize)
            .sum::<usize>();
        let data = NonNull::from(&self.data[start]);
        let len = segment.try_get().unwrap();
        Some(Segment { data, len })
    }

    #[inline]
    fn size_in_words(&self) -> usize {
        self.data.len()
    }
}

/// A generic table of segment locations where all segments are contiguously laid out in a
/// flat slice.
///
/// This is used in combination with `SegmentSet` to create a generic arena type for
/// reading messages from slices or streams.
#[derive(Debug)]
#[cfg(feature = "alloc")]
pub struct SegmentSetTable {
    /// The table. This contains the start positions of every segment except the first one
    /// (since it's always implied to be 0). The end of the last segment must be determined
    /// by the user by using the length of the whole slice.
    table: Box<[usize]>,
}

#[inline]
fn segment_len_to_usize(len: &SegmentLenReader, id: SegmentId) -> Result<usize, ReadError> {
    match len.try_get() {
        Some(len) => Ok(len.get() as usize),
        None => Err(ReadError::SegmentTooLarge { segment: id }),
    }
}

#[cfg(feature = "alloc")]
impl SegmentSetTable {
    /// Reads a stream table of any size. This returns a table and the length of the whole
    /// message.
    pub fn from_stream(table: &StreamTableRef, limit: usize) -> Result<(Self, usize), ReadError> {
        let (first, remainder) = table.split_first();
        let mut current_pos = segment_len_to_usize(first, 0)?;
        if current_pos > limit {
            return Err(ReadError::MessageTooLarge);
        }

        let mut start_idxs = Vec::with_capacity(remainder.len());
        for (len, id) in remainder.iter().zip(1u32..) {
            let len = segment_len_to_usize(len, id)?;
            let Some(new_pos) = current_pos.checked_add(len).filter(|&new| new <= limit) else {
                return Err(ReadError::MessageTooLarge);
            };

            start_idxs.push(current_pos);
            current_pos = new_pos;
        }

        let table = start_idxs.into_boxed_slice();
        Ok((Self { table }, current_pos))
    }

    /// Returns the start and end bounds for the given segment.
    pub fn segment_bounds(&self, id: SegmentId) -> Option<(Bound<usize>, Bound<usize>)> {
        let idx = id as usize;
        let start = match id {
            0 => 0,
            _ => *self.table.get(idx - 1)?,
        };
        let end = match self.table.get(idx) {
            Some(end) => Bound::Excluded(*end),
            None => Bound::Unbounded,
        };

        Some((Bound::Included(start), end))
    }

    fn last_start(&self) -> usize {
        self.table.last().copied().unwrap_or(0)
    }
}

/// A set of read-only segments used by a message `Reader`.
///
/// This is a generic backing source type provided by the library.
#[derive(Debug)]
#[cfg(feature = "alloc")]
pub struct SegmentSet<S> {
    slice: S,
    table: SegmentSetTable,
}

#[cfg(feature = "alloc")]
impl SegmentSet<Box<[Word]>> {
    pub fn new(table: SegmentSetTable, data: Box<[Word]>) -> Self {
        assert!(table.last_start() <= data.len(), "incomplete data!");

        Self { slice: data, table }
    }
}

#[cfg(feature = "alloc")]
unsafe impl<'a> ReadArena for SegmentSet<&'a [Word]> {
    fn segment(&self, id: SegmentId) -> Option<Segment> {
        let bounds = self.table.segment_bounds(id)?;
        let slice = &self.slice[bounds];

        let len = SegmentLen::new(slice.len() as u32).unwrap();
        let data = NonNull::new(slice.as_ptr().cast_mut()).unwrap();
        Some(Segment { data, len })
    }
    fn size_in_words(&self) -> usize {
        self.slice.len()
    }
}

#[cfg(feature = "alloc")]
unsafe impl ReadArena for SegmentSet<Box<[Word]>> {
    fn segment(&self, id: SegmentId) -> Option<Segment> {
        let bounds = self.table.segment_bounds(id)?;
        let slice = &self.slice[bounds];

        let len = SegmentLen::new(slice.len() as u32).unwrap();
        let data = NonNull::new(slice.as_ptr().cast_mut()).unwrap();
        Some(Segment { data, len })
    }
    fn size_in_words(&self) -> usize {
        self.slice.len()
    }
}

#[derive(Debug)]
#[cfg(feature = "std")]
pub enum StreamError {
    /// An error occured while reading the stream table
    Table(TableReadError),
    /// A segment in the table indicated it's too large to be read
    SegmentTooLarge {
        /// The ID of the segment
        segment: SegmentId,
    },
    MessageTooLarge,
    Io(io::Error),
}

impl Display for StreamError {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        match self {
            StreamError::Table(err) => Display::fmt(err, f),
            StreamError::SegmentTooLarge { .. } => f.write_str("segment too large"),
            StreamError::MessageTooLarge => f.write_str("message too large"),
            StreamError::Io(err) => Display::fmt(err, f),
        }
    }
}

impl std::error::Error for StreamError {
    fn source(&self) -> Option<&(dyn std::error::Error + 'static)> {
        match self {
            StreamError::Table(err) => Some(err),
            StreamError::SegmentTooLarge { .. } => None,
            StreamError::MessageTooLarge => None,
            StreamError::Io(err) => Some(err),
        }
    }
}

impl From<io::Error> for StreamError {
    fn from(value: io::Error) -> Self {
        StreamError::Io(value)
    }
}

impl From<TableReadError> for StreamError {
    fn from(value: TableReadError) -> Self {
        StreamError::Table(value)
    }
}

pub struct StreamOptions {
    /// The max number of segments that are allowed to be contained in the message.
    pub segment_limit: u32,
    /// The max amount of words that can be read from the input stream
    pub read_limit: usize,
}

impl StreamOptions {
    pub const DEFAULT: Self = Self {
        segment_limit: 512,
        read_limit: ReaderOptions::DEFAULT.traversal_limit as usize,
    };
}

impl Default for StreamOptions {
    fn default() -> Self {
        Self::DEFAULT
    }
}

/// Parse a message from a flat array.
///
/// This returns both the message reader and the remainder of the slice beyond the end of
/// the message.
///
/// If the content is incomplete or invalid, this returns Err
#[cfg(feature = "alloc")]
pub fn read_from_slice(slice: &[Word]) -> Result<(SegmentSet<&[Word]>, &[Word]), ReadError> {
    let (table, content) = StreamTableRef::try_read(slice)?;
    let (table, message_len) = SegmentSetTable::from_stream(table, content.len())?;
    if content.len() < message_len {
        return Err(ReadError::Incomplete);
    }

    let (content, remainder) = content.split_at(message_len);
    let set = SegmentSet {
        slice: content,
        table,
    };
    Ok((set, remainder))
}

#[inline]
fn map_read_err(err: ReadError) -> StreamError {
    match err {
        ReadError::Table(err) => StreamError::Table(err),
        ReadError::SegmentTooLarge { segment } => StreamError::SegmentTooLarge { segment },
        ReadError::MessageTooLarge => StreamError::MessageTooLarge,
        ReadError::Incomplete => unreachable!(),
    }
}

#[cfg(feature = "std")]
pub fn read_from_stream<R: io::Read>(
    mut r: R,
    options: StreamOptions,
) -> Result<SegmentSet<Box<[Word]>>, StreamError> {
    let mut first = [Word::NULL; 1];

    const TABLE_STACK_BUF_LEN: usize = 32;
    let mut table_stack_buf: [Word; TABLE_STACK_BUF_LEN];
    let mut table_heap_buf: Box<[Word]>;

    r.read_exact(Word::slice_to_bytes_mut(&mut first))?;

    let segment_table = match StreamTableRef::try_read(&first) {
        Ok((table, _)) => table,
        Err(TableReadError::Incomplete { count, required }) => {
            if count >= options.segment_limit {
                return Err(StreamError::Table(TableReadError::TooManySegments));
            }

            let buffer = if count > stream::max_table_size_from_len(TABLE_STACK_BUF_LEN) {
                // There's too many segments for our stack buffer, so we need to allocate
                // a buffer on the heap

                table_heap_buf = vec![Word::NULL; required + 1].into_boxed_slice();
                &mut *table_heap_buf
            } else {
                table_stack_buf = [Word::NULL; TABLE_STACK_BUF_LEN];
                &mut table_stack_buf[..(required + 1)]
            };

            let (buffer_first, rest) = buffer.split_first_mut().unwrap();
            *buffer_first = first[0];

            r.read_exact(Word::slice_to_bytes_mut(rest))?;

            StreamTableRef::try_read(buffer)
                .expect("failed to read segment table")
                .0
        }
        Err(err @ TableReadError::TooManySegments) => return Err(StreamError::Table(err)),
        Err(TableReadError::Empty) => unreachable!(),
    };
    let (table, message_len) =
        SegmentSetTable::from_stream(segment_table, options.read_limit).map_err(map_read_err)?;

    let mut data = vec![Word::NULL; message_len].into_boxed_slice();
    r.read_exact(Word::slice_to_bytes_mut(&mut data))?;

    Ok(SegmentSet::new(table, data))
}

/// A `BufRead` paired with an unpacker.
///
/// This allows for easily reading words from a packed input stream.
#[cfg(feature = "std")]
#[derive(Debug)]
pub struct PackedStream<R> {
    unpacker: Unpacker,
    stream: R,
}

#[cfg(feature = "std")]
impl<R: io::BufRead> PackedStream<R> {
    pub const fn new(stream: R) -> Self {
        Self {
            unpacker: Unpacker::new(),
            stream,
        }
    }

    pub fn read(&mut self, out: &mut [Word]) -> Result<usize, io::Error> {
        if out.is_empty() {
            return Ok(0);
        }

        loop {
            let buf = self.stream.fill_buf()?;
            if buf.is_empty() {
                return match self.unpacker.finish() {
                    Ok(()) => Ok(0),
                    Err(err) => Err(io::Error::new(io::ErrorKind::UnexpectedEof, err)),
                };
            }

            let result = self.unpacker.unpack(buf, out);
            self.stream.consume(result.bytes_read);

            match result.reason {
                StopReason::NeedInput if result.words_written == 0 => continue,
                _ => break Ok(result.words_written),
            }
        }
    }

    pub fn read_exact(&mut self, mut out: &mut [Word]) -> Result<(), io::Error> {
        while !out.is_empty() {
            match self.read(out) {
                Ok(0) => break,
                Ok(n) => {
                    out = &mut out[n..];
                }
                Err(ref e) if e.kind() == io::ErrorKind::Interrupted => {}
                Err(e) => return Err(e),
            }
        }
        if !out.is_empty() {
            Err(io::Error::from(io::ErrorKind::UnexpectedEof))
        } else {
            Ok(())
        }
    }

    /// Finishes the stream, flushing the unpacker state. If more data needs to be unpacked, this
    /// returns an error.
    pub fn finish(&mut self) -> Result<(), io::Error> {
        loop {
            let buf = self.stream.fill_buf()?;
            if buf.is_empty() {
                return self
                    .unpacker
                    .finish()
                    .map_err(|e| io::Error::new(io::ErrorKind::UnexpectedEof, e));
            }

            let result = self.unpacker.unpack(buf, &mut []);
            self.stream.consume(result.bytes_read);

            match result.reason {
                StopReason::NeedInput => continue,
                StopReason::NeedOutput => {
                    return self
                        .unpacker
                        .finish()
                        .map_err(|e| io::Error::new(io::ErrorKind::WriteZero, e))
                }
            }
        }
    }

    /// Deconstruct the packed stream into its unpacker and backing `BufRead` instance.
    pub fn into_parts(self) -> (Unpacker, R) {
        (self.unpacker, self.stream)
    }
}

#[cfg(feature = "std")]
pub fn read_packed_from_stream<R: io::BufRead>(
    stream: &mut PackedStream<R>,
    options: StreamOptions,
) -> Result<SegmentSet<Box<[Word]>>, StreamError> {
    const TABLE_STACK_BUF_LEN: usize = 32;
    let mut table_stack_buf: [Word; TABLE_STACK_BUF_LEN];
    let mut table_heap_buf: Box<[Word]>;

    let mut first = [Word::NULL; 1];
    stream.read_exact(&mut first)?;

    let table = match StreamTableRef::try_read(&first) {
        Ok((table, _)) => table,
        Err(TableReadError::Empty) => unreachable!(),
        Err(err @ TableReadError::TooManySegments) => return Err(StreamError::Table(err)),
        Err(TableReadError::Incomplete { count, required }) => {
            if count >= options.segment_limit {
                return Err(StreamError::Table(TableReadError::TooManySegments));
            }

            let buffer = if count > stream::max_table_size_from_len(TABLE_STACK_BUF_LEN) {
                // There's too many segments for our stack buffer, so we need to allocate
                // a buffer on the heap

                table_heap_buf = vec![Word::NULL; required + 1].into_boxed_slice();
                &mut *table_heap_buf
            } else {
                table_stack_buf = [Word::NULL; TABLE_STACK_BUF_LEN];
                &mut table_stack_buf[..(required + 1)]
            };

            let (buffer_first, rest) = buffer.split_first_mut().unwrap();
            *buffer_first = first[0];

            stream.read_exact(rest)?;

            StreamTableRef::try_read(buffer)
                .expect("failed to read segment table")
                .0
        }
    };

    let (table, message_len) =
        SegmentSetTable::from_stream(table, options.read_limit).map_err(map_read_err)?;

    let mut data = vec![Word::NULL; message_len].into_boxed_slice();
    stream.read_exact(&mut data)?;

    Ok(SegmentSet::new(table, data))
}

#[cfg(feature = "std")]
pub fn write_message<W: io::Write>(mut w: W, segments: &MessageSegments<'_>) -> Result<(), io::Error> {
    use std::io::IoSlice;

    let stream_table = StreamTable::from_segments(segments);
    let message_segment_bytes = segments.clone().into_iter().map(|s| s.as_bytes());
    let mut io_slice_box = iter::once(stream_table.as_bytes())
        .chain(message_segment_bytes)
        .map(std::io::IoSlice::new)
        .collect::<Box<[_]>>();

    // TODO(someday): This is literally a copy of write_all_vectored.
    // Use it when it becomes stable.
    let mut bufs = &mut *io_slice_box;

    IoSlice::advance_slices(&mut bufs, 0);
    while !bufs.is_empty() {
        match w.write_vectored(bufs) {
            Ok(0) => {
                return Err(io::Error::new(io::ErrorKind::WriteZero, "failed to write whole buffer"));
            }
            Ok(n) => IoSlice::advance_slices(&mut bufs, n),
            Err(ref e) if e.kind() == io::ErrorKind::Interrupted => {}
            Err(e) => return Err(e),
        }
    }
    Ok(())
}

#[cfg(feature = "std")]
pub fn write_message_packed<W: io::Write>(
    mut w: W,
    segments: &MessageSegments<'_>,
) -> Result<(), io::Error> {
    let mut buffer = [0u8; 256];

    let stream_table = StreamTable::from_segments(segments);
    let message_segment_words = segments.clone().into_iter().map(|s| s.as_words());
    let segments = iter::once(stream_table.as_slice()).chain(message_segment_words);
    for segment in segments {
        let mut packer = Packer::new(segment);
        loop {
            let result = packer.pack(&mut buffer);
            w.write_all(&buffer[..result.bytes_written])?;
            if result.completed {
                break;
            }
        }
    }

    Ok(())
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    #[cfg(feature = "std")]
    fn message_with_zero_size_final_segment() {
        let msg = [
            Word([
                1, 0, 0, 0, // A message with 2 segments
                2, 0, 0, 0, // where the first segment is 2 words
            ]),
            Word::NULL, // The last segment is 0 words.
            Word::NULL,
            Word::NULL,
        ];
        let slice = Word::slice_to_bytes(msg.as_slice());

        let _ = read_from_stream(slice, StreamOptions::DEFAULT).unwrap();
    }
}
