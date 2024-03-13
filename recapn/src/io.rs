//! Helpers for reading and writing Cap'n Proto messages in standard formats.

pub mod stream;
pub mod packing;

use {
    core::{
        fmt::{self, Display, Debug},
        ptr::NonNull,
    },
    crate::{
        alloc::{Segment, Word},
        arena::{SegmentId, ReadArena},
        message::ReaderOptions,
    },
    stream::{StreamTableRef, TableReadError}
};

#[cfg(feature = "alloc")]
use {
    rustalloc::{
        boxed::Box,
        vec::Vec,
    },
    crate::alloc::SegmentLen,
};

#[cfg(feature = "std")]
use {
    core::iter,
    std::io,
    crate::message::MessageSegments,
    stream::StreamTable,
    packing::{Packer, PackResult},
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
    /// The message was incomplete
    Incomplete,
}

impl Display for ReadError {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        match self {
            ReadError::Table(e) => Display::fmt(e, f),
            ReadError::SegmentTooLarge { .. } => write!(f, "segment too large"),
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
        for (len, id) in table.segments().into_iter().zip(0u32..) {
            let Some(len) = len.try_get() else {
                return Err(ReadError::SegmentTooLarge { segment: id, })
            };
            let Some(new_total) = total_len.checked_add(len.get() as usize) else {
                // Return incomplete since the slice could never contain all the data if this
                // overflows
                return Err(ReadError::Incomplete)
            };
            total_len = new_total;
        }

        if total_len > data.len() {
            return Err(ReadError::Incomplete)
        }

        let (data, remainder) = data.split_at(total_len);
        Ok((Self { table, data }, remainder))
    }
}

unsafe impl ReadArena for TableSegmentSet<'_> {
    fn segment(&self, id: SegmentId) -> Option<Segment> {
        let (segment, leading_segments) = self.table.segments()
            .get(..=(id as usize))?
            .split_last()
            .unwrap();
        let start = leading_segments.into_iter().map(|l| l.raw() as usize).sum::<usize>();
        let data = NonNull::from(&self.data[start]);
        let len = segment.try_get().unwrap();
        Some(Segment { data, len })
    }

    #[inline]
    fn size_in_words(&self) -> usize {
        self.data.len()
    }
}

/// A set of read-only segments used by a message `Reader`.
///
/// This is a generic backing source type provided by the library.
#[derive(Debug)]
#[cfg(feature = "alloc")]
pub struct SegmentSet<S> {
    slice: S,
    /// The location of the first segment. For borrowed slices this is always 0.
    first_segment: usize,
    segment_starts: Box<[usize]>,
}

#[cfg(feature = "alloc")]
unsafe impl<'a> ReadArena for SegmentSet<&'a [Word]> {
    fn segment(&self, id: SegmentId) -> Option<Segment> {
        let slice = match id {
            0 => if let Some(&end) = self.segment_starts.first() {
                &self.slice[self.first_segment..end]
            } else {
                self.slice
            },
            _ => match self.segment_starts.get((id as usize - 1)..)? {
                [start, end, ..] => &self.slice[*start..*end],
                [start] => &self.slice[*start..],
                [] => return None,
            }
        };

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
        let slice = match id {
            0 => if let Some(&end) = self.segment_starts.first() {
                &self.slice[self.first_segment..end]
            } else {
                &*self.slice
            },
            _ => match self.segment_starts.get((id as usize - 1)..)? {
                [start, end, ..] => &self.slice[*start..*end],
                [start] => &self.slice[*start..],
                [] => return None,
            }
        };

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

/// Parse a message from a flat array.
///
/// This returns both the message reader and the remainder of the slice beyond the end of
/// the message.
///
/// If the content is incomplete or invalid, this returns Err
#[inline]
#[cfg(feature = "alloc")]
pub fn read_from_slice<'a>(slice: &'a [Word]) -> Result<(SegmentSet<&'a [Word]>, &'a [Word]), ReadError> {
    let (table, content) = StreamTableRef::try_read(slice)?;
    let segments = table.segments();

    let mut remainder = content;
    let mut current_pos = 0;
    let mut start_idxs = Vec::with_capacity(segments.len() - 1);
    for (len, id) in segments.into_iter().zip(1u32..) {
        let Some(len) = len.try_get() else {
            return Err(ReadError::SegmentTooLarge {
                segment: id,
            })
        };
        let len = len.get() as usize;
        let new_pos = current_pos + len;
        if content.len() < new_pos {
            return Err(ReadError::Incomplete)
        }

        start_idxs.push(current_pos);
        current_pos = new_pos;
        remainder = &content[current_pos..];
    }

    let set = SegmentSet {
        slice: &content[..current_pos],
        first_segment: 0,
        segment_starts: start_idxs.into_boxed_slice(),
    };
    Ok((set, remainder))
}

/// Read a message from a boxed slice of words. This consumes the whole slice and does not return
/// any trailing words after the message itself.
/// 
/// The table is separate from the actual message content. This is intended to be used by reading
/// the whole segment table into a seperate allocation, calculating the total size of the message,
/// then allocating a space large enough to contain the whole message.
#[inline]
#[cfg(feature = "alloc")]
pub fn read_from_box(table: &StreamTableRef, slice: Box<[Word]>) -> Result<SegmentSet<Box<[Word]>>, ReadError> {
    let segments = table.segments();

    let mut current_pos = 0;
    let mut start_idxs = Vec::with_capacity(segments.len() - 1);
    for (len, id) in segments.into_iter().zip(1u32..) {
        let Some(len) = len.try_get() else {
            return Err(ReadError::SegmentTooLarge {
                segment: id,
            })
        };
        let len = len.get() as usize;
        let new_pos = current_pos + len;
        if slice.len() < new_pos {
            return Err(ReadError::Incomplete)
        }

        start_idxs.push(current_pos);
        current_pos = new_pos;
    }

    let set = SegmentSet {
        slice,
        first_segment: 0,
        segment_starts: start_idxs.into_boxed_slice(),
    };
    Ok(set)
}

#[cfg(feature = "std")]
pub fn read_from_stream<R: io::Read>(r: &mut R, options: StreamOptions) -> Result<SegmentSet<Box<[Word]>>, StreamError> {
    let mut first = [Word::NULL; 1];
    let segment_table_buffer: tinyvec::TinyVec<[Word; 32]>;

    r.read_exact(Word::slice_to_bytes_mut(&mut first))?;

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

            r.read_exact(Word::slice_to_bytes_mut(rest))?;

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
            })
        };

        total_words += size.get() as u64;

        if total_words > options.read_limit {
            return Err(StreamError::MessageTooLarge);
        }    
    }

    let mut data = vec![Word::NULL; total_words as usize].into_boxed_slice();
    r.read_exact(Word::slice_to_bytes_mut(data.as_mut()))?;

    let set = read_from_box(segment_table, data)
        .expect("failed to read message from slice");

    Ok(set)
}

#[cfg(feature = "std")]
pub fn read_packed_from_stream<R: io::Read>(r: &mut R) -> Result<SegmentSet<Box<[Word]>>, io::Error> {
    todo!()
}

#[cfg(feature = "std")]
pub fn write_message<W: io::Write>(w: &mut W, segments: &MessageSegments) -> Result<(), io::Error> {
    let stream_table = StreamTable::from_segments(segments);
    let message_segment_bytes = segments.clone().into_iter().map(|s| s.as_bytes());
    let mut io_slice_box = iter::once(stream_table.as_bytes())
        .chain(message_segment_bytes)
        .map(std::io::IoSlice::new)
        .collect::<Box<[_]>>();

    w.write_all_vectored(&mut io_slice_box)
}

#[cfg(feature = "std")]
pub fn write_message_packed<W: io::Write>(w: &mut W, segments: &MessageSegments) -> Result<(), io::Error> {
    let mut buffer = [0u8; 256];

    let stream_table = StreamTable::from_segments(segments);
    let message_segment_words = segments.clone().into_iter().map(|s| s.as_words());
    let segments = iter::once(stream_table.as_slice()).chain(message_segment_words);
    for segment in segments {
        let mut packer = Packer::new(segment);
        loop {
            let PackResult { completed, bytes_written } = packer.pack(&mut buffer);
            w.write_all(&buffer[..bytes_written])?;
            if completed {
                break
            }
        }
    }

    Ok(())
}