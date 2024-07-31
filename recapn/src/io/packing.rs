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

#[derive(Debug)]
pub struct PackResult {
    pub completed: bool,
    pub bytes_written: usize,
}

#[inline]
#[track_caller]
fn take_first<'a, T>(src: &mut &'a [T]) -> &'a T {
    try_take_first(src).unwrap()
}

#[inline]
fn try_take_first<'a, T>(src: &mut &'a [T]) -> Option<&'a T> {
    let (first, remainder) = src.split_first()?;
    *src = remainder;
    Some(first)
}

#[inline]
#[track_caller]
fn take_at<'a, T>(src: &mut &'a [T], idx: usize) -> &'a [T] {
    let (first, second) = src.split_at(idx);
    *src = second;
    first
}

#[inline]
#[track_caller]
fn take_first_mut<'a, T>(src: &mut &'a mut [T]) -> &'a mut T {
    assert!(!src.is_empty()); // checks the condition before modifying the environment
    let old = mem::take(src);
    let (first, remainder) = old.split_first_mut().unwrap();
    *src = remainder;
    first
}

#[inline]
#[track_caller]
fn take_at_mut<'a, T>(src: &mut &'a mut [T], idx: usize) -> &'a mut [T] {
    assert!(src.len() >= idx);
    let old = mem::take(src);
    let (first, second) = old.split_at_mut(idx);
    *src = second;
    first
}

use take_first_mut as take_byte;

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
        .unwrap_or(to_read);
    input.split_at(zero_words)
}

/// A packing primitive that implements the Cap'n Proto packing algorithm.
#[derive(Debug)]
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
        let in_output_len = output_buf.len();

        // Check if we have a copy to resume
        if let Some(active_copy) = &mut self.active_copy {
            match copy_and_advance_slices(active_copy, output_buf) {
                Ok(new_buf) => {
                    output_buf = new_buf;
                    self.active_copy = None;
                },
                Err(remainder) => {
                    *active_copy = remainder;
                    return PackResult { completed: false, bytes_written: in_output_len }
                }
            }
        }

        // At most we will write 10 bytes here if a word is all nonzeros.
        // 1 byte tag +
        // 8 byte word +
        // 1 byte uncompressed word count
        const MIN_BUF: usize = 10;

        let completed = loop {
            let Some((&word, remainder)) = self.input.split_first() else {
                break true
            };

            if output_buf.len() < MIN_BUF {
                break false
            }

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
            *tag = 0;
            macro_rules! handle_byte {
                ($idx:literal) => {
                    let b = word.0[$idx];
                    let bit = (b != 0) as u8;
                    *tag |= (bit << $idx);
                    if b != 0 {
                        write_byte(&mut output_buf, b);
                    }
                };
            }

            handle_byte!(0);
            handle_byte!(1);
            handle_byte!(2);
            handle_byte!(3);
            handle_byte!(4);
            handle_byte!(5);
            handle_byte!(6);
            handle_byte!(7);

            if *tag == 0xff {
                // An all-nonzero word is followed by a count of consecutive uncompressed
                // words, followed by the uncompressed words themselves.

                let (uncompressed, remainder) = split_at_filter(self.input, |&w| {
                    w.0.into_iter().filter(|&b| b == 0).count() >= 2
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
                        return PackResult {
                            completed: false,
                            bytes_written: in_output_len,
                        }
                    }
                }
            }
        };

        PackResult { completed, bytes_written: in_output_len - output_buf.len() }
    }
}

/// The result of calling `unpack()`.
#[derive(Debug)]
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
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum StopReason {
    /// The unpacker needs more input.
    NeedInput,
    /// The unpacker needs more output to write to.
    NeedOutput,
}

#[derive(Debug)]
pub struct IncompleteError {
    /// The number of bytes needed to finish unpacking.
    /// 
    /// This value can be 0 in the case that the unpacker had remaining null words to write.
    pub bytes_needed: usize,
}

impl fmt::Display for IncompleteError {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        if self.bytes_needed != 0 {
            write!(f, "incomplete packed input, {} bytes required to unpack", self.bytes_needed)
        } else {
            write!(f, "unfinished packed input, more data remaining in unpacker")
        }
    }
}

#[cfg(feature = "std")]
impl std::error::Error for IncompleteError {}

#[derive(Clone, Copy, Debug)]
enum WrittenBytes {
    Zero,
    One,
    Two,
    Three,
    Four,
    Five,
    Six,
    Seven,
}

impl WrittenBytes {
    #[inline]
    const fn needed_for_full(self) -> usize {
        use WrittenBytes::*;
        match self {
            Zero => 8,
            One => 7,
            Two => 6,
            Three => 5,
            Four => 4,
            Five => 3,
            Six => 2,
            Seven => 1,
        }
    }

    #[inline]
    const fn bytes(self) -> usize {
        use WrittenBytes::*;
        match self {
            Zero => 0,
            One => 1,
            Two => 2,
            Three => 3,
            Four => 4,
            Five => 5,
            Six => 6,
            Seven => 7,
        }
    }

    #[inline]
    const fn from_bytes(bytes: usize) -> Self {
        use WrittenBytes::*;
        match bytes {
            0 => Zero,
            1 => One,
            2 => Two,
            3 => Three,
            4 => Four,
            5 => Five,
            6 => Six,
            7 => Seven,
            _ => unreachable!(),
        }
    }
}

#[derive(Clone, Copy, Debug)]
struct PartialWord {
    buf: [u8; 8],
    written_bytes: WrittenBytes,
}

impl PartialWord {
    pub fn needed_bytes(&self) -> usize {
        self.written_bytes.needed_for_full()
    }

    pub fn start(src: &[u8]) -> Self {
        let written_bytes = WrittenBytes::from_bytes(src.len());
        let mut buf = [0u8; 8];
        buf[..src.len()].copy_from_slice(src);
        Self { buf, written_bytes }
    }

    pub fn finish(mut self, src: &mut &[u8], dst: &mut &mut [Word]) -> Result<(), PartialWord> {
        let needed = self.written_bytes.needed_for_full();
        let buf_start = self.written_bytes.bytes();
        if needed <= src.len() {
            let to_copy = take_at(src, needed);
            self.buf[buf_start..].copy_from_slice(to_copy);
            *take_first_mut(dst) = Word(self.buf);
            Ok(())
        } else {
            let to_copy = mem::take(src);
            let copy_len = to_copy.len();
            let buf_end = buf_start + copy_len;
            self.buf[buf_start..buf_end].copy_from_slice(to_copy);
            self.written_bytes = WrittenBytes::from_bytes(buf_end);
            Err(self)
        }
    }
}

#[derive(Clone, Copy, Debug)]
struct PartialTaggedWord {
    buf: [u8; 8],
    tag: u8,
    written_bytes: WrittenBytes,
}

impl PartialTaggedWord {
    pub fn needed_bytes(&self) -> usize {
        self.tag.count_ones() as usize
    }

    pub fn start(mut tag: u8, mut src: &[u8]) -> Self {
        let mut buf = [0u8; 8];
        let mut dst_idx = 0;
        while tag != 0 {
            if (tag & 1) != 0 {
                let Some(&byte) = try_take_first(&mut src) else { break };
                buf[dst_idx] = byte;
            }

            tag >>= 1;
            dst_idx += 1;
        }
        Self { buf, tag, written_bytes: WrittenBytes::from_bytes(dst_idx) }
    }

    pub fn finish(self, src: &mut &[u8], dst: &mut &mut [Word]) -> Result<(), PartialTaggedWord> {
        let Self { mut buf, mut tag, written_bytes } = self;
        let mut dst_idx = written_bytes.bytes();
        while tag != 0 {
            if (tag & 1) != 0 {
                let Some(&byte) = try_take_first(src) else { break };
                buf[dst_idx] = byte;
            }

            tag >>= 1;
            dst_idx += 1;
        }

        if tag == 0 {
            *take_first_mut(dst) = Word(buf);
            Ok(())
        } else {
            Err(PartialTaggedWord {
                buf,
                tag,
                written_bytes: WrittenBytes::from_bytes(dst_idx),
            })
        }
    }
}

#[derive(Clone, Copy, Debug)]
enum IncompleteState {
    /// The unpacker is reading a Word.
    PartialWord(PartialTaggedWord),
    /// The unpacker is reading a Word. This may transition into a `NeedUncompressedCount`
    /// after finishing the word.
    PartialUncompressed(PartialWord),
    /// The unpacker is expecting one byte for a null word count
    NeedNullCount,
    /// The unpacker is expecting one byte for an uncompressed word count
    NeedUncompressedCount,
    /// The unpacker is writing null words to the output
    WritingNulls(usize),
    /// The unpacker is writing uncompressed words to the output
    WritingUncompressed {
        remaining: usize,
        partial: Option<PartialWord>,
    },
}

#[derive(Debug)]
pub struct Unpacker {
    state: Option<IncompleteState>,
}

impl Unpacker {
    #[inline]
    pub const fn new() -> Self {
        Self { state: None }
    }

    /// Unpack words from the input into the specified buffer. The output buffer should
    /// be made up entirely of Word::NULL.
    #[inline]
    pub fn unpack(&mut self, mut src: &[u8], mut dst: &mut [Word]) -> UnpackResult {
        let original_src_len = src.len();
        let original_dst_len = dst.len();

        let reason = self.unpack_inner(&mut src, &mut dst);

        UnpackResult {
            bytes_read: original_src_len - src.len(),
            words_written: original_dst_len - dst.len(),
            reason,
        }
    }

    /// Progress the output by count. This doesn't actually write anything since we assume the input
    /// consists entirely of NULL already.
    #[inline]
    fn write_nulls(&mut self, dst: &mut &mut [Word], count: usize) -> Result<(), StopReason> {
        if dst.len() >= count {
            *dst = &mut mem::take(dst)[count..];
            Ok(())
        } else {
            let remaining = count - dst.len();
            *dst = &mut [];
            self.state = Some(IncompleteState::WritingNulls(remaining));
            Err(StopReason::NeedOutput)
        }
    }

    #[inline]
    fn write_uncompressed(&mut self, src: &mut &[u8], dst: &mut &mut [Word], count: usize) -> Result<(), StopReason> {
        let (full_words, partial_bytes) = (src.len() / 8, src.len() % 8);
        let enough_input = full_words >= count;
        let words_to_read = if enough_input { count } else { full_words };
        let enough_output = words_to_read <= dst.len();

        if !enough_output {
            let out = mem::take(dst);
            let remaining = count - out.len();

            let out_bytes = Word::slice_to_bytes_mut(out);
            let in_bytes = take_at(src, out_bytes.len());
            out_bytes.copy_from_slice(in_bytes);

            self.state = Some(IncompleteState::WritingUncompressed { remaining, partial: None });

            return Err(StopReason::NeedOutput)
        }

        let out = take_at_mut(dst, words_to_read);
        let out_bytes = Word::slice_to_bytes_mut(out);

        if !enough_input {
            let has_partial = partial_bytes != 0;
            let consumed_words = full_words + (has_partial as usize);
            let remaining = count - consumed_words;
            let full_word_bytes = take_at(src, full_words * 8);
            out_bytes.copy_from_slice(full_word_bytes);

            let partial = has_partial.then(|| PartialWord::start(mem::take(src)));
            self.state = Some(IncompleteState::WritingUncompressed { remaining, partial });

            return Err(StopReason::NeedInput)
        }

        let full_word_bytes = take_at(src, words_to_read * 8);
        out_bytes.copy_from_slice(full_word_bytes);

        Ok(())
    }

    fn unpack_inner<'a>(&mut self, src: &mut &[u8], dst: &mut &mut [Word]) -> StopReason {
        if src.is_empty() {
            return StopReason::NeedInput
        }

        // Attempt to finish a previous read-write first
        match self.state.take() {
            Some(IncompleteState::PartialWord(partial)) => {
                if let Err(partial) = partial.finish(src, dst) {
                    self.state = Some(IncompleteState::PartialWord(partial));
                    return StopReason::NeedInput
                }
            }
            Some(IncompleteState::PartialUncompressed(partial)) => {
                if let Err(partial) = partial.finish(src, dst) {
                    self.state = Some(IncompleteState::PartialUncompressed(partial));
                    return StopReason::NeedInput
                }

                if src.is_empty() {
                    self.state = Some(IncompleteState::NeedUncompressedCount);
                    return StopReason::NeedInput
                }

                let count = *take_first(src) as usize;
                if let Err(stop) = self.write_uncompressed(src, dst, count) {
                    return stop
                }
            }
            Some(IncompleteState::NeedNullCount) => {
                let count = *take_first(src) as usize;
                if let Err(stop) = self.write_nulls(dst, count) {
                    return stop
                }
            }
            Some(IncompleteState::NeedUncompressedCount) => {
                let count = *take_first(src) as usize;
                if let Err(stop) = self.write_uncompressed(src, dst, count) {
                    return stop
                }
            }
            Some(IncompleteState::WritingNulls(count)) => {
                if let Err(stop) = self.write_nulls(dst, count) {
                    return stop
                }
            }
            Some(IncompleteState::WritingUncompressed { remaining, partial }) => {
                if let Some(partial) = partial {
                    if let Err(partial) = partial.finish(src, dst) {
                        self.state = Some(IncompleteState::WritingUncompressed {
                            remaining,
                            partial: Some(partial),
                        });
                        return StopReason::NeedInput
                    }
                }

                if let Err(stop) = self.write_uncompressed(src, dst, remaining) {
                    return stop
                }
            }
            None => {}
        }

        // Now begin our normal unpacking loop
        loop {
            if src.is_empty() {
                break StopReason::NeedInput
            }

            if dst.is_empty() {
                break StopReason::NeedOutput
            }

            let tag = *take_first(src);
            match tag {
                0x00 => {
                    // A 0 tag followed by the number of zero words.

                    // Skip the first word, since the count byte is the number of *additional*
                    // null words
                    take_first_mut(dst);

                    if src.is_empty() {
                        self.state = Some(IncompleteState::NeedNullCount);
                        break StopReason::NeedInput
                    }
    
                    let count = *take_first(src) as usize;
                    if count != 0 {
                        if let Err(stop) = self.write_nulls(dst, count) {
                            break stop
                        }
                    }
                }
                0xff => {
                    // A full word followed by the number of uncompressed words.
                    if src.len() < 8 {
                        let partial = PartialWord::start(mem::take(src));
                        self.state = Some(IncompleteState::PartialUncompressed(partial));
                        break StopReason::NeedInput
                    }

                    let word = take_first_mut(dst);
                    let word_bytes = Word::slice_to_bytes_mut(slice::from_mut(word));
                    let src_bytes = take_at(src, 8);
                    word_bytes.copy_from_slice(src_bytes);

                    if src.is_empty() {
                        self.state = Some(IncompleteState::NeedUncompressedCount);
                        break StopReason::NeedInput
                    }

                    let count = *take_first(src) as usize;
                    if count != 0 {
                        if let Err(stop) = self.write_uncompressed(src, dst, count) {
                            break stop
                        }
                    }
                }
                mut tag => {
                    let bytes_needed = tag.count_ones() as usize;
                    if src.len() < bytes_needed {
                        let partial = PartialTaggedWord::start(tag, mem::take(src));
                        self.state = Some(IncompleteState::PartialWord(partial));
                        break StopReason::NeedInput
                    }

                    let input_to_read = take_at(src, bytes_needed);
                    let word = take_first_mut(dst);
                    let word_bytes = Word::slice_to_bytes_mut(slice::from_mut(word));

                    let mut i = input_to_read.into_iter();
                    for word_byte in word_bytes.iter_mut() {
                        if (tag & 1) != 0 {
                            *word_byte = *i.next().unwrap();
                        }

                        tag >>= 1;

                        if tag == 0 {
                            break;
                        }
                    }
                }
            }
        }
    }

    /// Finish unpacking. This returns a result with an error if the unpacker wasn't finished and
    /// had partial state left over.
    #[inline]
    pub fn finish(&self) -> Result<(), IncompleteError> {
        let bytes_needed = match self.state {
            Some(IncompleteState::PartialWord(partial)) => partial.needed_bytes(),
            Some(IncompleteState::PartialUncompressed(partial)) => partial.needed_bytes() + 1,
            Some(IncompleteState::NeedNullCount) => 1,
            Some(IncompleteState::NeedUncompressedCount) => 1,
            Some(IncompleteState::WritingNulls(_)) => 0, // Not sure what else to put here?
            Some(IncompleteState::WritingUncompressed { remaining, partial }) => {
                let remaining_word_bytes = remaining as usize * 8;
                let partial = partial.map(|p| p.needed_bytes()).unwrap_or(0);
                remaining_word_bytes + partial
            }
            None => return Ok(())
        };

        Err(IncompleteError { bytes_needed })
    }
}

#[cfg(test)]
mod tests {
    use std::io::BufReader;

    use crate::alloc::Word;
    use crate::io::PackedStream;
    use super::Packer;

    fn expect_packs_to(unpacked: &[Word], packed: &[u8]) {
        // Use a buffer of 0xDE to make sure we're properly writting 0x00 bytes.
        let mut buf = [0xde; 512];
        let mut packer = Packer::new(unpacked);
        let mut out = Vec::new();

        loop {
            let r = packer.pack(&mut buf);
            out.extend_from_slice(&buf[0..r.bytes_written]);
            if r.completed {
                break
            }
        }

        assert_eq!(packed, out.as_slice(), "expected != result");

        {
            let mut words = vec![Word::NULL; unpacked.len()].into_boxed_slice();
            let mut stream = PackedStream::new(packed);
            stream.read_exact(&mut *words).expect("expected to read packed data");
            stream.finish().expect("expected end of stream");
    
            assert_eq!(unpacked, &*words);
        }

        // Test some silly buffers
        for size in [1, 2, 3, 5, 7, 10] {
            let mut words = vec![Word::NULL; unpacked.len()].into_boxed_slice();
            let reader = BufReader::with_capacity(size, packed);
            let mut stream = PackedStream::new(reader);
            stream.read_exact(&mut *words).expect("expected to read packed data");
            stream.finish().expect("expected end of stream");
    
            assert_eq!(unpacked, &*words);
        }
    }

    #[test]
    fn simple_packing() {
        expect_packs_to(&[], &[]);
        expect_packs_to(&[Word([0, 0, 0, 0, 0, 0, 0, 0])], &[0, 0]);
        expect_packs_to(&[
            Word([0, 0, 12, 0, 0, 34, 0, 0]),
        ], &[
            0b00100100, 12, 34,
        ]);
        expect_packs_to(&[
            Word([1, 3, 2, 4, 5, 7, 6, 8]),
        ], &[
            0b11111111, 1, 3, 2, 4, 5, 7, 6, 8, 0,
        ]);
        expect_packs_to(&[
            Word([0, 0, 0, 0, 0, 0, 0, 0]),
            Word([1, 3, 2, 4, 5, 7, 6, 8]),
        ], &[
            0, 0,
            0b11111111, 1, 3, 2, 4, 5, 7, 6, 8, 0,
        ]);
        expect_packs_to(&[
            Word([0, 0, 12, 0, 0, 34, 0, 0]),
            Word([1, 3, 2, 4, 5, 7, 6, 8]),
        ], &[
            0b00100100, 12, 34,
            0b11111111, 1, 3, 2, 4, 5, 7, 6, 8, 0,
        ]);
        expect_packs_to(&[
            Word([1, 3, 2, 4, 5, 7, 6, 8]),
            Word([8, 6, 7, 5, 4, 2, 3, 1]),
        ], &[
            0b11111111, 1, 3, 2, 4, 5, 7, 6, 8, 1,
            8, 6, 7, 5, 4, 2, 3, 1,
        ]);
        expect_packs_to(&[
            Word([1, 2, 3, 4, 5, 6, 7, 8]),
            Word([1, 2, 3, 4, 5, 6, 7, 8]),
            Word([1, 2, 3, 4, 5, 6, 7, 8]),
            Word([1, 2, 3, 4, 5, 6, 7, 8]),
            Word([0, 2, 4, 0, 9, 0, 5, 1]),
        ], &[
            0b11111111, 1, 2, 3, 4, 5, 6, 7, 8, 3,
            1, 2, 3, 4, 5, 6, 7, 8,
            1, 2, 3, 4, 5, 6, 7, 8,
            1, 2, 3, 4, 5, 6, 7, 8,
            0b11010110, 2, 4, 9, 5, 1,
        ]);
        expect_packs_to(&[
            Word([1, 2, 3, 4, 5, 6, 7, 8]),
            Word([1, 2, 3, 4, 5, 6, 7, 8]),
            Word([6, 2, 4, 3, 9, 0, 5, 1]),
            Word([1, 2, 3, 4, 5, 6, 7, 8]),
            Word([0, 2, 4, 0, 9, 0, 5, 1]),
        ], &[
            0b11111111, 1, 2, 3, 4, 5, 6, 7, 8, 3,
            1, 2, 3, 4, 5, 6, 7, 8,
            6, 2, 4, 3, 9, 0, 5, 1,
            1, 2, 3, 4, 5, 6, 7, 8,
            0b11010110, 2, 4, 9, 5, 1,
        ]);
        expect_packs_to(&[
            Word([8, 0, 100, 6, 0, 1, 1, 2]),
            Word([0, 0, 0, 0, 0, 0, 0, 0]),
            Word([0, 0, 0, 0, 0, 0, 0, 0]),
            Word([0, 0, 0, 0, 0, 0, 0, 0]),
            Word([0, 0, 1, 0, 2, 0, 3, 1]),
        ], &[
            0b11101101, 8, 100, 6, 1, 1, 2,
            0, 2,
            0b11010100, 1, 2, 3, 1,
        ]);
    }
}