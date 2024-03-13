//! Types implementing the Cap'n Proto packing compression algorithm.

use core::{cmp, fmt, mem, slice};

use crate::alloc::Word;

/// Copies from the source input slice to the destination and advances the slices.
/// If the source can be fully copied into the output buffer, the remaining output buffer
/// is returned in the Ok variant. If the source can't be fully copied, the remaining source
/// is returned the Err variant.
#[inline]
fn copy_and_advance_slices<'a, 'b, T: Copy>(
    src: &'a [T],
    dst: &'b mut [T],
) -> Result<&'b mut [T], &'a [T]> {
    if src.len() <= dst.len() {
        let (copy_space, new_dst) = dst.split_at_mut(src.len());
        copy_space.copy_from_slice(src);
        Ok(new_dst)
    } else {
        let (to_copy, remainder) = src.split_at(dst.len());
        dst.copy_from_slice(to_copy);
        Err(remainder)
    }
}

pub struct PackResult {
    pub completed: bool,
    pub bytes_written: usize,
}

#[inline]
fn take_byte<'a>(buf: &mut &'a mut [u8]) -> &'a mut u8 {
    let (first, remainder) = core::mem::take(buf).split_first_mut().unwrap();
    *buf = remainder;
    first
}

#[inline]
fn write_byte(buf: &mut &mut [u8], value: u8) {
    *take_byte(buf) = value;
}

#[inline]
fn split_at_filter(input: &[Word], func: impl FnMut(&Word) -> bool) -> (&[Word], &[Word]) {
    let to_read = cmp::min(input.len(), 255);
    let zero_words = input[..to_read]
        .iter()
        .position(func)
        .unwrap_or(255);
    input.split_at(zero_words)
}

/// A packing primitive that implements the Cap'n Proto packing algorithm.
pub struct Packer<'b> {
    input: &'b [Word],
    active_copy: Option<&'b [u8]>,
}

impl<'b> Packer<'b> {
    #[inline]
    pub fn new(input: &'b [Word]) -> Self {
        Self { input, active_copy: None }
    }

    /// Packs words into the specified buffer. This may not use the whole buffer.
    /// 
    /// At least 10 bytes are needed to write a word to the output buffer. If a buffer
    /// smaller than that is provided, no bytes will be written.
    pub fn pack(&mut self, mut output_buf: &mut [u8]) -> PackResult {
        let output_len = output_buf.len();
        let mut bytes_written = 0;

        // Check if we have a copy to resume
        if let Some(active_copy) = &mut self.active_copy {
            match copy_and_advance_slices(active_copy, output_buf) {
                Ok(new_buf) => {
                    output_buf = new_buf;
                    bytes_written += active_copy.len();
                    self.active_copy = None;
                },
                Err(remainder) => {
                    *active_copy = remainder;
                    return PackResult { completed: false, bytes_written: output_len }
                }
            }
        }

        // At most we will write 10 bytes here if a word is all nonzeros.
        // 1 byte tag +
        // 8 byte word +
        // 1 byte uncompressed word count
        const MIN_BUF: usize = 10;

        if output_buf.len() < MIN_BUF {
            return PackResult { completed: false, bytes_written }
        }

        let completed = loop {
            let Some((&word, remainder)) = self.input.split_first() else {
                break true
            };
            self.input = remainder;

            if word == Word::NULL {
                // An all-zero word is followed by a count of consecutive zero words (not
                // including the first one).
                let (zeros, remainder) = split_at_filter(self.input, |&w| w != Word::NULL);
                self.input = remainder;
                // Write the tag
                write_byte(&mut output_buf, 0);
                // Write the zero word count
                write_byte(&mut output_buf, zeros.len() as u8);
                continue;
            }

            let tag = take_byte(&mut output_buf);
            for byte in word.0.to_ne_bytes() {
                if byte != 0 {
                    *tag |= 1;
                    write_byte(&mut output_buf, byte);
                }
                *tag <<= 1;
            }

            if *tag == 0xff {
                // An all-nonzero word is followed by a count of consecutive uncompressed
                // words, followed by the uncompressed words themselves.

                let (uncompressed, remainder) = split_at_filter(self.input, |&w| {
                    w.0.to_ne_bytes().into_iter().filter(|&b| b == 0).count() >= 2
                });
                self.input = remainder;

                // Write the uncompressed word count
                write_byte(&mut output_buf, uncompressed.len() as u8);

                let bytes_to_copy = Word::slice_to_bytes(uncompressed);
                match copy_and_advance_slices(bytes_to_copy, output_buf) {
                    Ok(buf) => {
                        // We still have some space left. Just re-assign `out` since the 
                        // limit and end are going to stay the same
                        output_buf = buf;
                    },
                    Err(remainder) => {
                        self.active_copy = Some(remainder);
                        return PackResult { completed: false, bytes_written: output_len }
                    }
                }
            }

            if output_buf.len() < MIN_BUF {
                bytes_written += output_len - output_buf.len();
                break false
            }
        };

        PackResult { completed, bytes_written }
    }
}

/// The result of calling `unpack()`.
pub struct UnpackResult {
    /// The number of bytes read from the input. This may be less than the
    /// length of the input slice.
    pub bytes_read: usize,
    /// The number of words written to the output. This may be less than the
    /// length of the output slice.
    pub words_written: usize,
    /// The reason we stopped unpacking.
    pub reason: StopReason,
}

/// The reason the unpacker stopped unpacking.
pub enum StopReason {
    /// The unpacker needs more input.
    NeedInput {
        /// A partial unpack result. This can be used to resume unpacking with a new
        /// set of input or finish unpacking if no input is left.
        partial: PartialUnpack,
    },
    /// The unpacker needs more output to write to.
    NeedOutput,
}

#[derive(Debug)]
pub struct IncompleteError {
    pub bytes_needed: usize,
}

impl fmt::Display for IncompleteError {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        write!(f, "incomplete packed input, {} bytes required to unpack", self.bytes_needed)
    }
}

#[cfg(feature = "std")]
impl std::error::Error for IncompleteError {}

pub struct PartialUnpack {
    bytes_needed: usize,
    state: Option<PartialState>,
}

enum PartialState {
    WritingNulls(u8),
    WritingUncompressed(u8),
}

impl PartialUnpack {
    /// Resume a partial unpack by providing a new input slice.
    pub fn resume<'b>(self, input: &'b [u8]) -> Unpacker<'b> {
        Unpacker { input, state: self.state }
    }

    /// Finish the unpack, returning an error if bytes were needed to continue reading.
    pub fn finish(self) -> Result<(), IncompleteError> {
        if self.bytes_needed == 0 {
            Ok(())
        } else {
            Err(IncompleteError { bytes_needed: self.bytes_needed })
        }
    }
}

/// Progress the output by count. This doesn't actually write anything since we assume the input
/// consists entirely of NULL already.
#[inline]
fn write_nulls(output: &mut &mut [Word], count: u8) -> u8 {
    let count_usize = count as usize;
    if output.len() >= count_usize {
        *output = &mut mem::take(output)[count_usize..];
        0
    } else {
        let remaining = (count_usize - output.len()) as u8;
        *output = &mut [];
        remaining
    }
}

#[inline]
fn write_uncompressed(
    input: &mut &[u8],
    output: &mut &mut [Word],
    count: u8,
) -> Option<(u8, StopReason)> {
    let count_usize = count as usize;

    let full_words = input.len() / 8;
    let enough_input = full_words >= count_usize;
    let words_to_read = if enough_input { count_usize } else { full_words };
    let enough_output = words_to_read < output.len();

    let (out_words, result) = if !enough_output {
        // Takes the output buffer and replaces it with an empty slice.
        let out = mem::take(output);
        let remaining_words = (count_usize - out.len()) as u8;

        (out, Some((remaining_words, StopReason::NeedOutput)))
    } else {
        let (out, remainder) = mem::take(output).split_at_mut(words_to_read);
        *output = remainder;

        let partial = if !enough_input {
            let remaining_words = (count_usize - full_words) as u8;
            let reason = StopReason::NeedInput {
                partial: PartialUnpack {
                    bytes_needed: (remaining_words as usize) * 8,
                    state: Some(PartialState::WritingUncompressed(remaining_words)),
                },
            };
            Some((remaining_words, reason))
        } else {
            None
        };

        (out, partial)
    };

    let out_bytes = Word::slice_to_bytes_mut(out_words);
    let (to_copy, remainder) = input.split_at(out_bytes.len());
    *input = remainder;

    out_bytes.copy_from_slice(to_copy);

    result
}

pub struct Unpacker<'b> {
    input: &'b [u8],
    state: Option<PartialState>,
}

impl<'b> Unpacker<'b> {
    #[inline]
    pub fn new(input: &'b [u8]) -> Self {
        Self { input, state: None }
    }

    /// Unpack words from the input into the specified buffer. The buffer should
    /// be made up entirely of Word::NULL.
    pub fn unpack(&mut self, mut buf: &mut [Word]) -> UnpackResult {
        let original_input_len = self.input.len();
        let original_buf_len = buf.len();

        // Attempt to resume a previous write operation
        match self.state.take() {
            Some(PartialState::WritingNulls(count)) => {
                let remaining = write_nulls(&mut buf, count);
                if remaining != 0 {
                    self.state = Some(PartialState::WritingNulls(remaining));
                    return UnpackResult {
                        bytes_read: 0,
                        words_written: original_buf_len - buf.len(),
                        reason: StopReason::NeedOutput,
                    }
                }
            }
            Some(PartialState::WritingUncompressed(count)) => {
                let result = write_uncompressed(
                    &mut self.input,
                    &mut buf,
                    count,
                );
                if let Some((remaining, stop)) = result {
                    if remaining != 0 {
                        self.state = Some(PartialState::WritingUncompressed(remaining));
                    }
                    return UnpackResult {
                        bytes_read: original_input_len - self.input.len(),
                        words_written: original_buf_len - buf.len(),
                        reason: stop,
                    }
                }
            }
            None => {}
        }

        // Maintain separate variables to work around weird lifetime issues.
        let mut curr_input_len = self.input.len();
        let mut curr_buf_len = buf.len();

        // Now run our normal unpacking loop
        let stop = loop {
            let [tag, input_remainder @ ..] = self.input else {
                break StopReason::NeedInput {
                    partial: PartialUnpack {
                        bytes_needed: 0,
                        state: None,
                    }
                }
            };

            let [word, buf_remainder @ ..] = buf else {
                break StopReason::NeedOutput
            };
            let word_bytes = Word::slice_to_bytes_mut(slice::from_mut(word));

            let mut bytes_needed = tag.count_ones() as usize;
            if bytes_needed == 0 || bytes_needed == 8 {
                bytes_needed += 1;
            }

            if input_remainder.len() < bytes_needed {
                break StopReason::NeedInput {
                    partial: PartialUnpack {
                        bytes_needed,
                        state: None,
                    },
                }
            }

            let (input_to_read, input_remainder) = input_remainder.split_at(bytes_needed);

            self.input = input_remainder;
            curr_input_len = self.input.len();

            buf = buf_remainder;
            curr_buf_len = buf.len();

            match tag {
                0x00 => {
                    // A 0 tag followed by the number of zero words.
                    let num_null_words = input_to_read[0];
                    let remaining = write_nulls(&mut buf, num_null_words);
                    if remaining != 0 {
                        self.state = Some(PartialState::WritingNulls(remaining));
                        break StopReason::NeedOutput
                    }
                }
                0xff => {
                    // A full word followed by the number of uncompressed words.
                    let [bytes_to_copy @ .., num_uncompressed_words] = input_to_read else { unreachable!() };
                    word_bytes.copy_from_slice(bytes_to_copy);
                    let result = write_uncompressed(
                        &mut self.input,
                        &mut buf,
                        *num_uncompressed_words,
                    );
                    if let Some((remaining, stop)) = result {
                        match &stop {
                            StopReason::NeedOutput => {
                                self.state = Some(PartialState::WritingUncompressed(remaining));
                            },
                            StopReason::NeedInput { .. } => {},
                        }
                        break stop
                    }
                }
                &(mut tag) => {
                    let mut input_pos = 0;
                    for word_pos in 0..8 {
                        if (tag | 1) != 0 {
                            word_bytes[word_pos] = input_to_read[input_pos];
                            input_pos += 1;
                        }

                        tag >>= 1;

                        if tag == 0 {
                            break;
                        }
                    }
                }
            }
        };

        UnpackResult {
            bytes_read: original_input_len - curr_input_len,
            words_written: original_buf_len - curr_buf_len,
            reason: stop,
        }
    }
}