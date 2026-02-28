use std::marker::PhantomData;
use std::num::NonZeroUsize;
use std::pin::Pin;
use std::ptr::NonNull;
use std::task::{Context, Poll};

use pin_project::pin_project;
use recapn::alloc::Word;
use recapn::io::{SegmentSet, SegmentSetTable, StreamOptions};
use recapn::io::stream::{StreamTableRef, TableReadError};
use tokio::io::AsyncRead;

use crate::chan::MessagePayload;
use crate::{IncomingMessage, OwnedIncomingMessage};

/// An incoming message possibly borrowing from a message stream.
pub struct StreamMessage<'a> {
    message: Inner<'a>,
}

enum Inner<'a> {
    Borrowed(recapn::io::SegmentSet<&'a [Word]>),
    Owned(recapn::io::SegmentSet<Box<[Word]>>),
}

impl<'a> IncomingMessage for StreamMessage<'a> {
    fn message(&self) -> &dyn recapn::arena::ReadArena {
        match &self.message {
            Inner::Borrowed(s) => s,
            Inner::Owned(s) => s,
        }
    }
    fn into_owned(self) -> OwnedIncomingMessage {
        let message = match self.message {
            Inner::Borrowed(s) => s.into_boxed_slice_set(),
            Inner::Owned(s) => s,
        };

        OwnedIncomingMessage { message: MessagePayload::External(Box::new(message)) }
    }
}

/// Reads messages from a stream.
/// 
/// This type contains an internal buffer for messages. It will attempt to use the buffer for
/// messages unless the incoming message is too large. This allows multiple small messages to
/// be read in one underlying read call.
#[pin_project]
pub struct ReadMessageBuf<R> {
    /// The main buffer used for small messages.
    buf: Box<[Word]>,
    state: State,
    options: StreamOptions,
    #[pin]
    stream: R,
}

enum State {
    /// We're reading a small value that fits in the shared `buf`.
    Small {
        /// The existing segment table.
        table: Option<SegmentSetTable>,
        /// The start of the data in the shared `buf`.
        words_start: usize,
        /// The number of bytes needed to transition to the next state of processing.
        /// If this is None, we're reading a new message.
        words_needed: Option<NonZeroUsize>,
        /// The number of bytes read
        bytes_read: usize,
    },
    /// We're reading a large value that doesn't fit in the common shared `buf`.
    Large {
        /// The table for the large value. If this is none, we're reading the table itself.
        table: Option<SegmentSetTable>,
        /// The buffer to use
        buf: Box<[Word]>,
        /// The amount of bytes already filled in the buffer.
        bytes_read: usize,
    },
}

impl State {
    const fn new() -> Self {
        Self::Small { table: None, words_start: 0, words_needed: None, bytes_read: 0 }
    }
}

impl<R> ReadMessageBuf<R> {
    pub fn new(stream: R, options: StreamOptions) -> Self {
        Self::with_capacity(stream, options, 256)
    }

    pub fn with_capacity(stream: R, options: StreamOptions, capacity: usize) -> Self {
        Self {
            buf: vec![Word::NULL; capacity].into(),
            state: State::new(),
            options,
            stream,
        }
    }
}

impl<R: AsyncRead + Unpin> ReadMessageBuf<R> {
    /// Read a message out of the stream.
    pub async fn read<'a>(&'a mut self) -> tokio::io::Result<Option<StreamMessage<'a>>> {
        Read::new(self).await
    }
}

impl<R: AsyncRead> ReadMessageBuf<R> {
    pub fn poll_read<'a>(self: Pin<&'a mut Self>, cx: &mut Context<'_>) -> Poll<tokio::io::Result<Option<StreamMessage<'a>>>> {
        let mut this = self.project();

        // If a path needs to set up a large buffer for reading, it invalidates all our buffers.
        // So we set up an outer 'poll_loop to return to the buffer setup code.
        'poll_loop: loop {
            let (table, words_start, buf, mut words_needed, bytes_read) = match this.state {
                State::Small { table, words_start, words_needed, bytes_read  } => {
                    (table, words_start, &mut **this.buf, Some(words_needed), bytes_read)
                },
                State::Large { table, buf, bytes_read } => {
                    (table, &mut 0, &mut **buf, None, bytes_read)
                },
            };

            'read_loop: loop {
                // Set up a block so we can jump out and go straight to reading from the stream.
                'parse_message: {
                    // See if we can make progress on an existing message in the buffer
                    let bytes_needed = match &words_needed {
                        // We're reading a new message, but don't need to read one, so skip direct
                        // to the stream.
                        Some(None) => break 'parse_message,
                        // The bytes needed are equal to the size of the buffer.
                        None => buf.len(),
                        Some(Some(needed)) => needed.get() * Word::BYTES,
                    };

                    if *bytes_read < bytes_needed {
                        // We haven't read enough data, go to the stream.
                        break 'parse_message
                    }

                    // We have enough data!
                    if let Some(table) = table.take() {
                        // We have a table already, which means now we can read the message!
                        // We take the table because we know we have enough to read the message
                        // fully.

                        let message = match this.state {
                            State::Small { words_start, words_needed, bytes_read, .. } => {
                                let start = *words_start;
                                let len_words = words_needed.unwrap().get();
                                *words_start += len_words;

                                let end = start + len_words;

                                let bytes_remaining = *bytes_read - bytes_needed;
                                if bytes_remaining == 0 {
                                    // No more remaining bytes, so this might be the end of the
                                    // stream. Set our needed words to None to signal that.
                                    *words_needed = None;
                                } else {
                                    // At least one word for the start of the next message.
                                    *words_needed = NonZeroUsize::new(1);
                                }

                                *bytes_read = bytes_remaining;

                                Inner::Borrowed(
                                    SegmentSet::from_slice(table, &this.buf[start..end])
                                )
                            },
                            State::Large { buf, .. } => {
                                let buf = std::mem::take(buf);
                                *this.state = State::new();

                                Inner::Owned(SegmentSet::from_boxed_slice(table, buf))
                            },
                        };

                        return Poll::Ready(Ok(Some(StreamMessage { message })))
                    }

                    let space = &buf[*words_start..];
                    let space_bytes = Word::slice_to_bytes(space);

                    // We haven't read a table yet, so let's try doing that now.
                    let words_read = *bytes_read / Word::BYTES;
                    let (stream_table, remaining) = match StreamTableRef::try_read(&space[..words_read]) {
                        Ok(read) => read,
                        Err(TableReadError::Incomplete { count, required }) => {
                            if count > this.options.segment_limit {
                                return Poll::Ready(Err(
                                    tokio::io::Error::other(TableReadError::TooManySegments)
                                ))
                            }

                            // At this point we know we must be in the small buffer. So we can unwrap
                            // our words_needed here.
                            **words_needed.as_mut().unwrap() = NonZeroUsize::new(required);

                            if required <= space.len() {
                                // There's enough room to read the table in the rest of the buffer
                                // space so let's go forward to read the stream.
                                break 'parse_message
                            }

                            if required <= buf.len() {
                                // There's enough room to read the table in the buffer space, but not
                                // enough to continue reading to the end of the space here. So, let's
                                // figure out where we are in this buffer and move everything back to
                                // the start.

                                let start_bytes = *words_start * Word::BYTES;
                                let end_bytes = start_bytes + *bytes_read;
                                let buf_bytes = Word::slice_to_bytes_mut(buf);
                                buf_bytes.copy_within(start_bytes..end_bytes, 0);

                                *words_start = 0;

                                // Now go read from the stream
                                break 'parse_message
                            }

                            // The small buffer isn't big enough! We need to make a big buffer just for
                            // the stream table.

                            let mut new_big_buf = vec![Word::NULL; required].into_boxed_slice();
                            let new_big_buf_bytes = Word::slice_to_bytes_mut(&mut new_big_buf);
                            new_big_buf_bytes[..*bytes_read].copy_from_slice(&space_bytes[..*bytes_read]);

                            *this.state = State::Large { table: None, buf: new_big_buf, bytes_read: *bytes_read };

                            // Go back to the outer loop and restablish borrows.
                            continue 'poll_loop
                        },
                        Err(TableReadError::TooManySegments) =>
                            return Poll::Ready(Err(
                                tokio::io::Error::other(TableReadError::TooManySegments)
                            )),
                        Err(TableReadError::Empty) => unreachable!(),
                    };

                    if stream_table.count().get() > this.options.segment_limit {
                        return Poll::Ready(Err(tokio::io::Error::other(TableReadError::TooManySegments)))
                    }

                    // We have a new table that we can read. Let's convert it to a segment table
                    let (new_table, size) = match SegmentSetTable::from_stream(stream_table, this.options.read_limit) {
                        Ok(table) => table,
                        Err(err) => return Poll::Ready(Err(tokio::io::Error::other(err))),
                    };

                    *table = Some(new_table);

                    // Move our read amounts by the length of the table.
                    let table_len = words_read - remaining.len();
                    let table_len_bytes = table_len * Word::BYTES;
                    *words_start += table_len;
                    *bytes_read -= table_len_bytes;

                    // Now that we have a parsed table, update our state

                    if size == 0 {
                        // Size 0 check because most of the stuff down below assumes this won't
                        // happen. A size 0 message can fit in the remaining data for a large
                        // segment table read! So if this happens we just take everything out
                        // and reset to a small state. This is entirely unlikely to occur though.

                        let bytes_read = *bytes_read;
                        let words_start = if words_needed.is_none() {
                            // Use words_needed as a signal to what buffer we were using.
                            // If none, then we were using a large buffer, so are next starting
                            // position will be the start of the small buffer at
                            0
                        } else {
                            *words_start + table_len
                        };
                        let words_needed = if bytes_read == 0 {
                            None
                        } else {
                            NonZeroUsize::new(1)
                        };

                        let table = table.take().unwrap();
                        *this.state = State::Small {
                            table: None,
                            words_start,
                            words_needed,
                            bytes_read,
                        };

                        let message = StreamMessage {
                            message: Inner::Owned(
                                SegmentSet::from_boxed_slice(table, Box::default()),
                            ),
                        };

                        return Poll::Ready(Ok(Some(message)))
                    }

                    // At this point, like with the table code, we know we must be in the
                    // small buffer. So we can unwrap our words_needed here.
                    **words_needed.as_mut().unwrap() = NonZeroUsize::new(size);

                    if size <= remaining.len() {
                        // We read the message already! Jump back and reuse our logic above
                        continue 'read_loop
                    }

                    if size <= buf.len() {
                        // We haven't read the message, but we could read it in the small buffer.
                        // Let's move data around.

                        let start_bytes = *words_start * Word::BYTES;
                        let end_bytes = start_bytes + *bytes_read;
                        let buf_bytes = Word::slice_to_bytes_mut(buf);
                        buf_bytes.copy_within(start_bytes..end_bytes, 0);

                        *words_start = 0;

                        // Go and try to read more stuff.
                        break 'parse_message
                    }

                    // We haven't read the message and couldn't read it in the small buffer.
                    // We need to make a large buffer to read into.
                    let mut new_big_buf = vec![Word::NULL; size].into_boxed_slice();
                    let new_big_buf_bytes = Word::slice_to_bytes_mut(&mut new_big_buf);
                    new_big_buf_bytes[..*bytes_read].copy_from_slice(&space_bytes[table_len_bytes..*bytes_read]);

                    *this.state = State::Large { table: table.take(), buf: new_big_buf, bytes_read: *bytes_read };

                    // Now we can jump back to reuse all the logic we had earlier.
                    continue 'poll_loop
                }

                let space = &mut buf[*words_start..];
                let space_bytes = Word::slice_to_bytes_mut(space);
                let mut read_buf = tokio::io::ReadBuf::new(space_bytes);
                read_buf.set_filled(*bytes_read);

                match this.stream.as_mut().poll_read(cx, &mut read_buf) {
                    Poll::Pending => return Poll::Pending,
                    Poll::Ready(Err(err)) => return Poll::Ready(Err(err)),
                    Poll::Ready(Ok(())) => {},
                }

                let prev_read = std::mem::replace(bytes_read, read_buf.filled().len());
                let bytes_added = *bytes_read - prev_read;

                if bytes_added == 0 {
                    // We didn't read anything.
                    return Poll::Ready({
                        if matches!(&words_needed, Some(None)) {
                            // But that's ok, we didn't need anything
                            Ok(None)
                        } else {
                            // Oh, we wanted something...
                            Err(tokio::io::ErrorKind::UnexpectedEof.into())
                        }
                    })
                }

                // We read something. If we didn't have anything we needed
                // to read, we definitely do now.
                if let Some(empty @ None) = &mut words_needed {
                    **empty = NonZeroUsize::new(1);
                }
            }
        }
    }
}

struct Read<'a, R> {
    read: Option<NonNull<R>>,
    p: PhantomData<&'a mut R>,
}

impl<'a, R> Read<'a, R> {
    fn new(inner: &'a mut R) -> Self {
        Self { read: Some(NonNull::from_mut(inner)), p: PhantomData }
    }
}

unsafe impl<'a, R> Send for Read<'a, R> where &'a mut R: Send {}
unsafe impl<'a, R> Sync for Read<'a, R> where &'a mut R: Sync {}

impl<'a, R: AsyncRead + Unpin> Future for Read<'a, ReadMessageBuf<R>> {
    type Output = tokio::io::Result<Option<StreamMessage<'a>>>;

    fn poll(mut self: std::pin::Pin<&mut Self>, cx: &mut Context<'_>) -> Poll<Self::Output> {
        let Some(mut read) = self.read else {
            panic!("`Read` polled after completion")
        };
        let read = Pin::new(unsafe { read.as_mut() });

        let poll = read.poll_read(cx);
        if poll.is_ready() {
            self.read = None;
        }
        poll
    }
}
