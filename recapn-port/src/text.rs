use core::fmt;

use recapn::{ptr::PtrReader, rpc::Table};

use crate::{DeserializePtr, Error, SerializePtr};

#[derive(Clone, PartialEq, Eq, Debug)]
pub struct TextFromBytesError(());

#[derive(Clone, PartialEq, Eq, Debug)]
pub enum TextFromBytesWithNulError {
    TextTooLarge,
    MissingNul,
}

#[derive(Clone)]
pub struct Text {
    // Invariant 1: the slice ends with a zero byte and has a length of at least one.
    // Invariant 2: the slice is within the max size of Cap'n Proto text.
    inner: Box<[u8]>,
}

impl fmt::Debug for Text {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        let bytes = self.to_bytes();

        // Copied from core::bstr
        write!(f, "\"")?;
        for chunk in bytes.utf8_chunks() {
            for c in chunk.valid().chars() {
                match c {
                    '\0' => write!(f, "\\0")?,
                    '\x01'..='\x7f' => write!(f, "{}", (c as u8).escape_ascii())?,
                    _ => write!(f, "{}", c.escape_debug())?,
                }
            }
            write!(f, "{}", chunk.invalid().escape_ascii())?;
        }
        write!(f, "\"")?;
        Ok(())
    }
}

impl fmt::Display for Text {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        let bytes = self.to_bytes();

        fn fmt_nopad(this: &[u8], f: &mut fmt::Formatter<'_>) -> fmt::Result {
            for chunk in this.utf8_chunks() {
                f.write_str(chunk.valid())?;
                if !chunk.invalid().is_empty() {
                    f.write_str("\u{FFFD}")?;
                }
            }
            Ok(())
        }

        let Some(align) = f.align() else {
            return fmt_nopad(bytes, f);
        };
        let nchars: usize = bytes
            .utf8_chunks()
            .map(|chunk| {
                chunk.valid().chars().count() + if chunk.invalid().is_empty() { 0 } else { 1 }
            })
            .sum();
        let padding = f.width().unwrap_or(0).saturating_sub(nchars);
        let fill = f.fill();
        let (lpad, rpad) = match align {
            fmt::Alignment::Left => (0, padding),
            fmt::Alignment::Right => (padding, 0),
            fmt::Alignment::Center => {
                let half = padding / 2;
                (half, half + padding % 2)
            }
        };
        for _ in 0..lpad {
            write!(f, "{fill}")?;
        }
        fmt_nopad(bytes, f)?;
        for _ in 0..rpad {
            write!(f, "{fill}")?;
        }

        Ok(())

    }

}

impl Text {
    pub fn try_from_bytes(b: &[u8]) -> Result<Self, TextFromBytesError> {
        let len = b.len() + 1; // add 1 for the null terminator
        if recapn::ptr::ElementCount::try_from(len).is_err() {
            return Err(TextFromBytesError(()))
        }
        let mut vec = Vec::with_capacity(len);
        vec.extend_from_slice(b);
        vec.push(0);
        Ok(Self { inner: vec.into_boxed_slice() })
    }
    #[inline]
    pub fn from_bytes(b: &[u8]) -> Self {
        Self::try_from_bytes(b).unwrap()
    }

    #[inline]
    pub fn try_new(b: impl AsRef<[u8]>) -> Result<Self, TextFromBytesError> {
        Self::try_from_bytes(b.as_ref())
    }
    #[inline]
    pub fn new(b: impl AsRef<[u8]>) -> Self {
        Self::from_bytes(b.as_ref())
    }

    pub fn try_from_bytes_with_nul(b: &[u8]) -> Result<Self, TextFromBytesWithNulError> {
        let Some(0) = b.last() else { return Err(TextFromBytesWithNulError::MissingNul) };
        let len = b.len();
        if recapn::ptr::ElementCount::try_from(len).is_err() {
            return Err(TextFromBytesWithNulError::TextTooLarge)
        }
        let mut vec = Vec::with_capacity(len);
        vec.extend_from_slice(b);
        vec.push(0);
        Ok(Self { inner: vec.into_boxed_slice() })
    }
    #[inline]
    pub fn from_bytes_with_nul(b: &[u8]) -> Self {
        Self::try_from_bytes_with_nul(b).unwrap()
    }

    #[inline]
    pub fn from_reader(r: recapn::text::Reader<'_>) -> Self {
        Self { inner: Box::from(r.as_bytes_with_nul()) }
    }

    #[inline]
    pub fn to_bytes(&self) -> &[u8] {
        match self.to_bytes_with_nul() {
            [r @ .., _] => r,
            r => {
                if cfg!(debug_assertions) {
                    unreachable!("Text slice must contain at least one element")
                }
                r
            },
        }
    }
    #[inline]
    pub fn to_bytes_with_nul(&self) -> &[u8] {
        &self.inner
    }

    #[inline]
    pub fn is_empty(&self) -> bool {
        self.to_bytes().is_empty()
    }
}

impl<T: Table> DeserializePtr<'_, T> for Text {
    fn deserialize<E: Error>(r: &PtrReader<'_, T>) -> Result<Self, E> {
        let text = match r.to_blob() {
            Ok(Some(b)) => match recapn::text::Reader::new(b) {
                Some(text) => text,
                None => return Err(E::protocol_error(recapn::Error::TextNotNulTerminated)),
            },
            Ok(None) => recapn::text::Reader::empty(),
            Err(err) => return Err(E::protocol_error(err)),
        };
        Ok(Self::from_reader(text))
    }
}

impl<T: Table> SerializePtr<'_, T> for Text {
    fn serialize<E: Error>(&self, b: recapn::ptr::PtrBuilder<'_, T>) -> Result<(), E> {
        let len = self.inner.len().try_into().unwrap();
        let mut blob = match b.try_init_blob(len) {
            Ok(blob) => recapn::data::Builder::from(blob),
            Err((err, _)) => return Err(E::protocol_error(err)),
        };
        blob.copy_from_slice(&self.inner);
        Ok(())
    }
}