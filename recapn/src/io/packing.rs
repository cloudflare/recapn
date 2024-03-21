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
fn take_first<'a, T>(src: &mut &'a [T]) -> &'a T {
    let old = mem::take(src);
    let (first, remainder) = old.split_first().unwrap();
    *src = remainder;
    first
}

#[inline]
fn try_take_first<'a, T>(src: &mut &'a [T]) -> Option<&'a T> {
    let old = mem::take(src);
    let (first, remainder) = old.split_first()?;
    *src = remainder;
    Some(first)
}

#[inline]
fn take_at<'a, T>(src: &mut &'a [T], idx: usize) -> &'a [T] {
    let old = mem::take(src);
    let (first, second) = old.split_at(idx);
    *src = second;
    first
}

#[inline]
fn take_first_mut<'a, T>(src: &mut &'a mut [T]) -> &'a mut T {
    let old = mem::take(src);
    let (first, remainder) = old.split_first_mut().unwrap();
    *src = remainder;
    first
}

#[inline]
fn take_at_mut<'a, T>(src: &mut &'a mut [T], idx: usize) -> &'a mut [T] {
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
            for byte in word.0 {
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
            self.buf[buf_start..copy_len].copy_from_slice(to_copy);
            self.written_bytes = WrittenBytes::from_bytes(buf_start + copy_len);
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
            if (tag | 1) != 0 {
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
            if (tag | 1) != 0 {
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
    WritingNulls(u8),
    /// The unpacker is writing uncompressed words to the output
    WritingUncompressed {
        remaining: u8,
        partial: Option<PartialWord>,
    },
}

pub struct Unpacker {
    state: Option<IncompleteState>,
}

impl Unpacker {
    #[inline]
    pub const fn new() -> Self {
        Self { state: None }
    }

    /// Unpack words from the input into the specified buffer. The output buffer should
    /// be made up entirely of Word::NULL. If the output buffer is empty this immediately
    /// returns.
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
    fn write_nulls(&mut self, dst: &mut &mut [Word], count: u8) -> Result<(), StopReason> {
        let count_usize = count as usize;
        if dst.len() >= count_usize {
            *dst = &mut mem::take(dst)[count_usize..];
            Ok(())
        } else {
            let remaining = (count_usize - dst.len()) as u8;
            *dst = &mut [];
            self.state = Some(IncompleteState::WritingNulls(remaining));
            Err(StopReason::NeedOutput)
        }
    }

    #[inline]
    fn write_uncompressed(&mut self, src: &mut &[u8], dst: &mut &mut [Word], count: u8) -> Result<(), StopReason> {
        if count == 0 {
            return Ok(())
        }

        let count_usize = count as usize;

        let (full_words, partial_bytes) = (src.len() / 8, src.len() % 8);
        let enough_input = full_words >= count_usize;
        let words_to_read = if enough_input { count_usize } else { full_words };
        let enough_output = words_to_read < dst.len();

        if !enough_output {
            let out = mem::take(dst);
            let remaining = (count_usize - out.len()) as u8;

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
            let remaining = (count_usize - consumed_words) as u8;
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
        if dst.is_empty() {
            return StopReason::NeedOutput
        }

        if src.is_empty() {
            return StopReason::NeedInput
        }

        // Attempt to finish a previous read-write first
        match self.state.take() {
            Some(IncompleteState::PartialWord(partial)) => {
                if let Err(partial) = partial.finish(src, dst) {
                    self.state = Some(IncompleteState::PartialWord(partial))
                }
            }
            Some(IncompleteState::PartialUncompressed(partial)) => {
                if let Err(partial) = partial.finish(src, dst) {
                    self.state = Some(IncompleteState::PartialUncompressed(partial))
                }
                
                if src.is_empty() {
                    self.state = Some(IncompleteState::NeedUncompressedCount);
                    return StopReason::NeedInput
                }

                let count = *take_first(src);
                if let Err(stop) = self.write_uncompressed(src, dst, count) {
                    return stop
                }
            }
            Some(IncompleteState::NeedNullCount) => {
                let count = *take_first(src);
                if let Err(stop) = self.write_nulls(dst, count) {
                    return stop
                }
            }
            Some(IncompleteState::NeedUncompressedCount) => {
                let count = *take_first(src);
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
                        })
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
    
                    let count = *take_first(src);
                    if let Err(stop) = self.write_nulls(dst, count) {
                        break stop
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

                    let count = *take_first(src);
                    if let Err(stop) = self.write_uncompressed(src, dst, count) {
                        break stop
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

                    let mut input_pos = 0;
                    for word_byte in word_bytes.iter_mut() {
                        if (tag | 1) != 0 {
                            *word_byte = input_to_read[input_pos];
                            input_pos += 1;
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

}