use crate::alloc::{AllocLen, ElementCount, ObjectLen, SegmentOffset, SignedSegmentOffset, Word};
use crate::list::Element;
use crate::message::internal::{ReadLimiter, SegmentBuilder, SegmentReader, SegmentRef};
use crate::message::SegmentId;
use crate::rpc::internal::CapPtrBuilder;
use crate::rpc::{BreakableCapSystem, CapSystem, CapTableReader, Empty, Table};
use crate::{Error, ErrorKind, Result};
use core::borrow::BorrowMut;
use core::cmp;
use core::convert::Infallible;
use core::fmt;
use core::marker::PhantomData;
use core::ops::ControlFlow;
use core::ptr::{self, NonNull};
use core::result::Result as CoreResult;

pub(crate) mod internal {
    use super::*;

    pub trait FieldData: Default + Copy + 'static {
        const ELEMENT_SIZE: ElementSize;

        unsafe fn read(ptr: *const u8, len: u32, slot: usize, default: Self) -> Self;
        unsafe fn read_unchecked(ptr: *const u8, slot: usize, default: Self) -> Self;

        unsafe fn write(ptr: *mut u8, len: u32, slot: usize, value: Self, default: Self);
        unsafe fn write_unchecked(ptr: *mut u8, slot: usize, value: Self, default: Self);
    }

    impl<T: FieldData> crate::internal::Sealed for T {}

    impl FieldData for Bool {
        const ELEMENT_SIZE: ElementSize = ElementSize::Bit;

        #[inline]
        unsafe fn read(ptr: *const u8, len_bytes: u32, slot: usize, default: Self) -> Self {
            let byte_offset = slot / 8;
            let value = if byte_offset < (len_bytes as usize) {
                Self::read_unchecked(ptr, slot, default)
            } else {
                false
            };
            value ^ default
        }
        #[inline]
        unsafe fn read_unchecked(ptr: *const u8, slot: usize, default: Self) -> Self {
            let (byte_offset, bit_num) = (slot / 8, slot % 8);
            let byte = core::ptr::read(ptr.add(byte_offset));
            let value = ((byte) & (1 << (bit_num))) != 0;
            value ^ default
        }

        #[inline]
        unsafe fn write(ptr: *mut u8, len: u32, slot: usize, value: Self, default: Self) {
            let byte_offset = slot / 8;
            if byte_offset < (len as usize) {
                Self::write_unchecked(ptr, slot, value, default)
            }
        }
        #[inline]
        unsafe fn write_unchecked(ptr: *mut u8, slot: usize, value: Self, default: Self) {
            let written_value = value ^ default;
            let (byte_offset, bit_num) = (slot / 8, slot % 8);
            let byte = ptr.add(byte_offset);
            *byte = (*byte & !(1 << bit_num)) | ((written_value as u8) << bit_num);
        }
    }

    macro_rules! impl_int {
        ($ty:ty, $size:ident) => {
            impl FieldData for $ty {
                const ELEMENT_SIZE: ElementSize = ElementSize::$size;

                #[inline]
                unsafe fn read(ptr: *const u8, len_bytes: u32, slot: usize, default: Self) -> Self {
                    let next_slot_byte_offset = (slot + 1) * core::mem::size_of::<Self>();
                    if next_slot_byte_offset < len_bytes as usize {
                        Self::read_unchecked(ptr, slot, default)
                    } else {
                        default
                    }
                }
                #[inline]
                unsafe fn read_unchecked(ptr: *const u8, slot: usize, default: Self) -> Self {
                    let data_ptr = ptr.cast::<WireValue<Self>>().add(slot);
                    let value = core::ptr::read(data_ptr).get();
                    value ^ default
                }

                #[inline]
                unsafe fn write(
                    ptr: *mut u8,
                    len_bytes: u32,
                    slot: usize,
                    value: Self,
                    default: Self,
                ) {
                    let next_slot_byte_offset = (slot + 1) * core::mem::size_of::<Self>();
                    if next_slot_byte_offset < len_bytes as usize {
                        Self::write_unchecked(ptr, slot, value, default)
                    }
                }
                #[inline]
                unsafe fn write_unchecked(ptr: *mut u8, slot: usize, value: Self, default: Self) {
                    let data_ptr = ptr.cast::<WireValue<Self>>().add(slot);
                    let write_value = value ^ default;
                    (&mut *data_ptr).set(write_value)
                }
            }
        };
    }

    impl_int!(UInt8, Byte);
    impl_int!(Int8, Byte);
    impl_int!(UInt16, TwoBytes);
    impl_int!(Int16, TwoBytes);
    impl_int!(UInt32, FourBytes);
    impl_int!(Int32, FourBytes);
    impl_int!(UInt64, EightBytes);
    impl_int!(Int64, EightBytes);
}

/// A void value, represented as a unit.
pub type Void = ();

pub type Bool = bool;

pub type Int8 = i8;
pub type Int16 = i16;
pub type Int32 = i32;
pub type Int64 = i64;

pub type UInt8 = u8;
pub type UInt16 = u16;
pub type UInt32 = u32;
pub type UInt64 = u64;

pub type Float32 = f32;
pub type Float64 = f64;

pub trait ToLittleEndian {
    fn to_le(self) -> Self;
}

macro_rules! to_le {
    ($($ty:ty: $i:ident => $ex:expr),+) => {
        $(impl ToLittleEndian for $ty {
            #[inline]
            fn to_le(self) -> Self {
                let $i = self;
                $ex
            }
        })+
    };
}

to_le! {
    u8:  i => i.to_le(),
    i8:  i => i.to_le(),
    u16: i => i.to_le(),
    i16: i => i.to_le(),
    u32: i => i.to_le(),
    i32: i => i.to_le(),
    u64: i => i.to_le(),
    i64: i => i.to_le()
}

#[derive(Clone, Copy, PartialEq, Eq)]
#[repr(transparent)]
struct WireValue<T>(T);
impl<T: ToLittleEndian + Copy> WireValue<T> {
    pub fn new(value: T) -> Self {
        Self(value.to_le())
    }

    pub fn get(&self) -> T {
        self.0.to_le()
    }

    pub fn set(&mut self, value: T) {
        self.0 = value.to_le();
    }
}

impl<T: ToLittleEndian + Copy> From<T> for WireValue<T> {
    fn from(value: T) -> Self {
        Self::new(value)
    }
}

#[derive(Default, Debug, Clone, PartialEq, Eq)]
pub struct MessageSize {
    pub words: u64,
    pub caps: u32,
}

impl core::ops::Add for MessageSize {
    type Output = MessageSize;

    fn add(self, rhs: Self) -> Self::Output {
        Self {
            words: self.words.add(rhs.words),
            caps: self.caps.add(rhs.caps),
        }
    }
}

impl core::ops::AddAssign for MessageSize {
    fn add_assign(&mut self, rhs: Self) {
        self.words += rhs.words;
        self.caps += rhs.caps;
    }
}

#[derive(Default, Debug, Clone, Copy, PartialEq, Eq)]
pub struct StructSize {
    /// The number of words of data in this struct
    pub data: u16,
    /// The number of pointers in this struct
    pub ptrs: u16,
}

impl StructSize {
    pub const EMPTY: StructSize = StructSize { data: 0, ptrs: 0 };

    /// Gets the total size of the struct in words
    pub const fn total(&self) -> u32 {
        self.data as u32 + self.ptrs as u32
    }

    /// Gets the max number of elements an struct list can contain of this struct
    pub const fn max_elements(&self) -> ElementCount {
        if matches!(*self, Self::EMPTY) {
            ElementCount::MAX
        } else {
            // subtract 1 for the tag ptr
            ElementCount::new((ElementCount::MAX_VALUE - 1) / (self.total())).unwrap()
        }
    }
}

#[derive(Debug, Copy, Clone, PartialEq, Eq)]
pub enum WireKind {
    Struct = 0,
    List = 1,
    Far = 2,
    Other = 3,
}

#[derive(Debug, Copy, Clone, PartialEq, Eq)]
pub enum PtrType {
    Null,
    Struct,
    List,
    Capability,
}

#[derive(Clone, Copy, PartialEq, Eq)]
struct Parts {
    pub lower: WireValue<u32>,
    pub upper: WireValue<u32>,
}

#[derive(Clone, Copy)]
pub union WirePtr {
    word: Word,
    parts: Parts,
    struct_ptr: StructPtr,
    list_ptr: ListPtr,
    far_ptr: FarPtr,
    cap_ptr: CapabilityPtr,
}

impl WirePtr {
    pub const NULL: WirePtr = WirePtr { word: Word::NULL };

    /// Creates a failed read error based on the expected data we intended to read
    fn fail_read(&self, expected: Option<ExpectedRead>) -> Error {
        Error::fail_read(expected, *self)
    }

    pub fn null() -> &'static WirePtr {
        &Self::NULL
    }

    /// Returns the kind of pointer
    #[inline]
    pub fn kind(&self) -> WireKind {
        unsafe {
            match self.parts.lower.get() as u8 & 3 {
                0 => WireKind::Struct,
                1 => WireKind::List,
                2 => WireKind::Far,
                3 => WireKind::Other,
                _ => unreachable!(),
            }
        }
    }
    #[inline]
    pub fn is_null(&self) -> bool {
        unsafe { self.word.is_null() }
    }
    #[inline]
    pub fn is_struct(&self) -> bool {
        matches!(self.kind(), WireKind::Struct)
    }
    #[inline]
    pub fn is_list(&self) -> bool {
        matches!(self.kind(), WireKind::List)
    }
    #[inline]
    pub fn is_capability(&self) -> bool {
        unsafe { self.parts.lower.get() == WireKind::Other as u32 }
    }
    #[inline]
    pub fn struct_ptr(&self) -> Option<&StructPtr> {
        self.is_struct().then(|| unsafe { &self.struct_ptr })
    }
    #[inline]
    pub fn try_struct_ptr(&self) -> Result<&StructPtr> {
        self.struct_ptr()
            .ok_or_else(|| self.fail_read(Some(ExpectedRead::Struct)))
    }
    #[inline]
    pub fn list_ptr(&self) -> Option<&ListPtr> {
        self.is_list().then(|| unsafe { &self.list_ptr })
    }
    #[inline]
    pub fn try_list_ptr(&self) -> Result<&ListPtr> {
        self.list_ptr()
            .ok_or_else(|| self.fail_read(Some(ExpectedRead::List)))
    }
    #[inline]
    pub fn far_ptr(&self) -> Option<&FarPtr> {
        (self.kind() == WireKind::Far).then(|| unsafe { &self.far_ptr })
    }
    #[inline]
    pub fn try_far_ptr(&self) -> Result<&FarPtr> {
        self.far_ptr()
            .ok_or_else(|| self.fail_read(Some(ExpectedRead::Far)))
    }
    #[inline]
    pub fn cap_ptr(&self) -> Option<&CapabilityPtr> {
        self.is_capability().then(|| unsafe { &self.cap_ptr })
    }
    #[inline]
    pub fn try_cap_ptr(&self) -> Result<&CapabilityPtr> {
        self.cap_ptr()
            .ok_or_else(|| self.fail_read(Some(ExpectedRead::Capability)))
    }
}

impl<'a> From<&'a Word> for &'a WirePtr {
    fn from(w: &'a Word) -> Self {
        unsafe { &*(w as *const Word as *const WirePtr) }
    }
}

impl fmt::Debug for WirePtr {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        write!(f, "{:?}", unsafe { self.word })
    }
}

impl PartialEq for WirePtr {
    fn eq(&self, other: &Self) -> bool {
        unsafe { self.word == other.word }
    }
}

#[repr(transparent)]
#[derive(Clone, Copy, PartialEq, Eq)]
pub struct StructPtr {
    parts: Parts,
}

impl StructPtr {
    pub const EMPTY: Self = Self::new(SignedSegmentOffset::new(-1).unwrap(), StructSize::EMPTY);

    #[inline]
    pub const fn new(offset: SignedSegmentOffset, StructSize { data, ptrs }: StructSize) -> Self {
        Self {
            parts: Parts {
                upper: WireValue((data as u32 | ((ptrs as u32) << 16)).to_le()),
                lower: WireValue(((offset.get() << 2) as u32).to_le()),
            },
        }
    }

    #[inline]
    pub fn offset(&self) -> SignedSegmentOffset {
        SignedSegmentOffset::new(self.parts.upper.get() as i32 >> 2).unwrap()
    }

    #[inline]
    pub fn inline_composite_element_count(&self) -> u32 {
        self.parts.upper.get() >> 2
    }

    /// Gets the size of the data section in words
    #[inline]
    pub fn data_size(&self) -> u16 {
        // truncate the upper bits
        self.parts.upper.get() as u16
    }

    /// Gets the number of pointers in the pointer section
    #[inline]
    pub fn ptr_count(&self) -> u16 {
        (self.parts.upper.get() >> 16) as u16
    }

    #[inline]
    pub fn size(&self) -> StructSize {
        StructSize {
            data: self.data_size(),
            ptrs: self.ptr_count(),
        }
    }

    /// Gets the size of the struct in words as an [ObjectLen]
    #[inline]
    pub fn len(&self) -> ObjectLen {
        ObjectLen::new(self.size().total()).unwrap()
    }
}

/// A list element's size, with a struct size for inline composite values
#[derive(Clone, Copy, Debug, PartialEq)]
pub enum ElementSize {
    Void,
    Bit,
    Byte,
    TwoBytes,
    FourBytes,
    EightBytes,
    Pointer,
    InlineComposite(StructSize),
}

impl ElementSize {
    /// Returns the number of bits per element. This also returns the number of bits per pointer.
    pub fn bits(&self) -> u32 {
        use ElementSize::*;
        match self {
            Void => 0,
            Bit => 1,
            Byte => 8,
            TwoBytes => 8 * 2,
            FourBytes => 8 * 4,
            EightBytes | Pointer => 8 * 8,
            InlineComposite(size) => size.total() * Word::BITS as u32,
        }
    }

    /// Get the maximum number of elements a list of this element size can contain. This only
    /// really matters for struct elements, since structs can easily overflow the max segment
    /// size if they're not zero sized.
    pub const fn max_elements(&self) -> ElementCount {
        match self {
            Self::InlineComposite(size) => size.max_elements(),
            _ => ElementCount::MAX,
        }
    }
}

#[derive(Clone, Copy, Debug, PartialEq)]
pub enum PtrElementSize {
    Void = 0,
    Bit = 1,
    Byte = 2,
    TwoBytes = 3,
    FourBytes = 4,
    EightBytes = 5,
    Pointer = 6,
    InlineComposite = 7,
}

impl From<ElementSize> for PtrElementSize {
    fn from(size: ElementSize) -> Self {
        match size {
            ElementSize::Void => PtrElementSize::Void,
            ElementSize::Bit => PtrElementSize::Bit,
            ElementSize::Byte => PtrElementSize::Byte,
            ElementSize::TwoBytes => PtrElementSize::TwoBytes,
            ElementSize::FourBytes => PtrElementSize::FourBytes,
            ElementSize::EightBytes => PtrElementSize::EightBytes,
            ElementSize::Pointer => PtrElementSize::Pointer,
            ElementSize::InlineComposite(_) => PtrElementSize::InlineComposite,
        }
    }
}

impl PtrElementSize {
    /// Returns the number of bits per element. This also returns the number of bits per pointer.
    pub fn bits(&self) -> u32 {
        use PtrElementSize::*;
        match self {
            Void | InlineComposite => 0,
            Bit => 1,
            Byte => 8,
            TwoBytes => 8 * 2,
            FourBytes => 8 * 4,
            EightBytes | Pointer => 8 * 8,
        }
    }

    /// Returns the number of bits per data element.
    /// Pointers don't count as data elements and inline composite elements don't have
    /// a set number of bits, so they return 0.
    pub fn data_bits(&self) -> u32 {
        use PtrElementSize::*;
        match self {
            Void | InlineComposite | Pointer => 0,
            Bit => 1,
            Byte => 8,
            TwoBytes => 8 * 2,
            FourBytes => 8 * 4,
            EightBytes => 8 * 8,
        }
    }

    pub fn data_bytes(&self) -> u32 {
        use PtrElementSize::*;
        match self {
            Void | InlineComposite | Pointer | Bit => 0,
            Byte => 1,
            TwoBytes => 2,
            FourBytes => 4,
            EightBytes => 8,
        }
    }

    pub fn pointers(&self) -> u16 {
        match self {
            PtrElementSize::Pointer => 1,
            _ => 0,
        }
    }
}

#[repr(transparent)]
#[derive(Clone, Copy)]
pub struct ListPtr {
    parts: Parts,
}

impl ListPtr {
    #[inline]
    pub fn new(offset: SignedSegmentOffset, size: PtrElementSize, count: ElementCount) -> Self {
        Self {
            parts: Parts {
                upper: WireValue::new(count.get() << 3 | size as u32),
                lower: WireValue::new((offset.get() << 2) as u32 | WireKind::List as u32),
            },
        }
    }

    #[inline]
    pub fn element_size(&self) -> PtrElementSize {
        match self.parts.upper.get() as u8 & 3 {
            0 => PtrElementSize::Void,
            1 => PtrElementSize::Bit,
            2 => PtrElementSize::Byte,
            3 => PtrElementSize::TwoBytes,
            4 => PtrElementSize::FourBytes,
            5 => PtrElementSize::EightBytes,
            6 => PtrElementSize::Pointer,
            7 => PtrElementSize::InlineComposite,
            _ => unreachable!(),
        }
    }

    /// For most elements, this is the number of distinct elements in the list. For example, a
    /// count of 8 with element size Bit would be 8 bits. For inline composite elements, this is the
    /// word count of the list (not counting the tag).
    #[inline]
    pub fn element_count(&self) -> ElementCount {
        ElementCount::new(self.parts.upper.get() >> 3).unwrap()
    }

    #[inline]
    pub fn offset(&self) -> SignedSegmentOffset {
        SignedSegmentOffset::new(self.parts.lower.get() as i32 >> 2).unwrap()
    }
}

#[repr(transparent)]
#[derive(Clone, Copy)]
pub struct FarPtr {
    parts: Parts,
}

impl FarPtr {
    pub fn new(segment: SegmentId, offset: SegmentOffset, double_far: bool) -> Self {
        Self {
            parts: Parts {
                upper: WireValue::new(segment),
                lower: WireValue::new(
                    (offset.get() << 3) | ((double_far as u32) << 2) | WireKind::Far as u32,
                ),
            },
        }
    }

    #[inline]
    pub fn offset(&self) -> SegmentOffset {
        SegmentOffset::new(self.parts.lower.get() >> 3).unwrap()
    }

    #[inline]
    pub fn double_far(&self) -> bool {
        ((self.parts.lower.get() >> 2) & 1) != 0
    }

    #[inline]
    pub fn segment(&self) -> SegmentId {
        self.parts.upper.get()
    }
}

#[repr(transparent)]
#[derive(Clone, Copy)]
pub struct CapabilityPtr {
    parts: Parts,
}

impl CapabilityPtr {
    #[inline]
    pub fn capability_index(&self) -> u32 {
        self.parts.upper.get()
    }

    #[inline]
    pub fn new(index: u32) -> Self {
        Self {
            parts: Parts {
                lower: WireValue(WireKind::Other as u32),
                upper: WireValue(index),
            },
        }
    }
}

#[non_exhaustive]
#[derive(Debug)]
pub enum ExpectedRead {
    Struct,
    List,
    BitList,
    Text,
    Data,
    Far,
    Capability,
}

#[non_exhaustive]
#[derive(Debug)]
pub enum ActualRead {
    Null,
    Struct,
    List(PtrElementSize),
    Far,
    Other,
}

#[derive(Debug)]
pub struct FailedRead {
    /// The thing we expected to read from the pointer.
    pub expected: Option<ExpectedRead>,
    /// The actual pointer value.
    pub actual: ActualRead,
}

impl fmt::Display for FailedRead {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        let expected_str = |expected: &_| {
            use ExpectedRead::*;
            match expected {
                Struct => "struct",
                List => "list",
                BitList => "bit list",
                Text => "text",
                Data => "data",
                Far => "far pointer",
                Capability => "capability",
            }
        };
        let actual = {
            use ActualRead::*;
            use PtrElementSize::*;
            match self.actual {
                Null => "null",
                Struct => "struct",
                List(element) => match element {
                    Void => "void list",
                    Bit => "bit list",
                    Byte => "byte list",
                    TwoBytes => "two-byte list",
                    FourBytes => "four-byte list",
                    EightBytes => "eight-byte list",
                    Pointer => "pointer list",
                    InlineComposite => "struct list",
                },
                Far => "far pointer",
                Other => "other pointer",
            }
        };
        if let Some(expected) = &self.expected {
            write!(f, "expected {}, got {}", expected_str(expected), actual)
        } else {
            write!(f, "unexpected {}", actual)
        }
    }
}

/// A wrapper for a segment ref that indicates that the reference is to a WirePtr.
#[derive(Clone, Copy)]
struct WireRef<'a>(pub SegmentRef<'a>);

impl<'a> WireRef<'a> {
    pub fn deref_ptr(&self, reader: &ObjectReader<'a>) -> &'a WirePtr {
        reader.deref_word(self.0).into()
    }
}

/// A control flow struct used to signal that when an error occurs, the destination pointer
/// should be null.
///
/// A number of things can go wrong when copying from one list or struct to another. You could
/// exceed the nesting limit, the read limit, a pointer could be invalid, capability pointers
/// might've been accidentally set when writing a canonical struct, etc. In these cases, a likely
/// safe default is to not copy the value, and instead just write null at the destination.  This
/// struct signals that copying should continue, but should just write null to the destination
/// instead of erroring out.
#[derive(Default, Clone, Copy, Debug)]
pub struct WriteNull;

/// A struct to help with reading objects.
#[derive(Clone, Debug)]
struct ObjectReader<'a> {
    /// The segment reader associated with an object. If no segment is provided, we don't
    /// perform length checks on relative pointers since it's assumed to be always valid.
    segment: Option<SegmentReader<'a>>,
    /// The limiter associated with this pointer. If no limiter is provided, we don't perform
    /// read limiting.
    limiter: Option<&'a ReadLimiter>,
}

impl<'a> ObjectReader<'a> {
    pub const fn is_unchecked(&self) -> bool {
        matches!(
            self,
            ObjectReader {
                segment: None,
                limiter: None
            }
        )
    }

    fn deref_word(&self, r: SegmentRef<'a>) -> &'a Word {
        if let Some(segment) = self.segment.as_ref() {
            segment.deref_word(r)
        } else {
            unsafe { r.deref() }
        }
    }

    /// Attempts to read a section in the specified segment. If this is a valid offset, it returns
    /// a pointer to the section and a reader for the section.
    #[inline]
    pub fn try_read_object_in(
        self,
        segment: SegmentId,
        offset: SegmentOffset,
        len: ObjectLen,
    ) -> Result<(SegmentRef<'a>, Self)> {
        let reader = self
            .segment
            .as_ref()
            .and_then(|r| r.segment(segment))
            .ok_or_else(|| ErrorKind::MissingSegment(segment))?;

        let ptr = reader
            .try_get_section_offset(offset, len)
            .ok_or(ErrorKind::PointerOutOfBounds)?;

        if let Some(limiter) = self.limiter {
            if !limiter.try_read(len.get().into()) {
                return Err(ErrorKind::ReadLimitExceeded.into());
            }
        }

        Ok((
            ptr,
            Self {
                segment: Some(reader),
                limiter: self.limiter,
            },
        ))
    }

    pub fn try_read_object_from_end_of(
        self,
        WireRef(r): WireRef<'a>,
        offset: SignedSegmentOffset,
        len: ObjectLen,
    ) -> Result<(SegmentRef<'a>, Self)> {
        let r = unsafe { r.offset(1.into()) };
        let new_ptr = if let Some(segment) = &self.segment {
            segment
                .try_offset_from(r, offset, len)
                .ok_or(ErrorKind::PointerOutOfBounds)?
        } else {
            // the pointer is unchecked, which is unsafe to make anyway, so whoever made the
            // pointer originally upholds safety here
            unsafe { r.offset(offset) }
        };
        if let Some(limiter) = self.limiter {
            if !limiter.try_read(len.get().into()) {
                return Err(ErrorKind::ReadLimitExceeded.into());
            }
        }

        Ok((new_ptr, self))
    }

    #[inline]
    pub fn location_of(
        self,
        WireRef(r): WireRef<'a>,
    ) -> Result<Content<ObjectReader<'a>, WireRef<'a>>> {
        let ptr: &WirePtr = self.deref_word(r).into();
        if let Some(far) = ptr.far_ptr() {
            let segment = far.segment();
            let double_far = far.double_far();
            let landing_pad_size = ObjectLen::new(if double_far { 2 } else { 1 }).unwrap();
            let (ptr, reader) = self.try_read_object_in(segment, far.offset(), landing_pad_size)?;

            let pad: &WirePtr = reader.deref_word(ptr).into();

            if double_far {
                // The landing pad is another far pointer. The next pointer is a tag describing our
                // pointed-to object
                // SAFETY: We know from the above that this landing pad is two words, so this is
                //         safe
                const ONE: SignedSegmentOffset = SignedSegmentOffset::new(1).unwrap();
                let tag = unsafe { WireRef(ptr.offset(ONE)) };

                let far_ptr = pad.try_far_ptr()?;
                Ok(Content {
                    accessor: reader,
                    ptr: tag,
                    location: Location::DoubleFar {
                        segment: far_ptr.segment(),
                        offset: far_ptr.offset(),
                    },
                })
            } else {
                Ok(Content {
                    accessor: reader,
                    ptr: WireRef(ptr),
                    location: Location::Far,
                })
            }
        } else {
            Ok(Content {
                accessor: self,
                ptr: WireRef(r),
                location: Location::Near,
            })
        }
    }

    pub fn try_amplified_read(&self, words: u64) -> bool {
        if let Some(limiter) = self.limiter {
            return limiter.try_read(words);
        }
        true
    }
}

/// Describes the content at a location.
struct Content<A, P> {
    /// An accessor to get the content.
    pub accessor: A,
    /// The pointer that describes the content. This may not be in the same segment as
    /// the original pointer to this content.
    pub ptr: P,
    /// Information associated with the content. This will describe how to properly read
    /// the content.
    pub location: Location,
}

/// Describes the location at some content.
enum Location {
    /// A near location. The content is in the same segment as the original pointer to the content.
    Near,
    /// A far location. The pointer that describes the content is in the same segment as the
    /// content itself.
    Far,
    /// A double far location. The pointer that describes the content is in a different segment
    /// than the content itself.
    DoubleFar {
        /// A segment ID for the segment the content is in
        segment: SegmentId,
        /// The offset from the start of the segment to the start of the content
        offset: SegmentOffset,
    },
}

/// Describes a type which can be imbued with a cap system.
pub trait Capable: Sized {
    type Table: Table;
    type Imbued<T2: Table>: Capable<Table = T2>;
}

pub type TableIn<T> = <T as Capable>::Table;

/// Describes a reader type which can be imbued with a cap table reader.
pub trait CapableReader: Capable {
    fn imbue_release<T2: Table>(
        self,
        new_table: T2::Reader,
    ) -> (Self::Imbued<T2>, <Self::Table as Table>::Reader);

    fn imbue<T2: Table>(self, new_table: T2::Reader) -> Self::Imbued<T2> {
        self.imbue_release(new_table).0
    }
}

/// Describes a builder type which can be imbued with a cap table builder.
pub trait CapableBuilder: Capable {
    fn imbue_release<T2: Table>(
        self,
        new_table: T2::Builder,
    ) -> (Self::Imbued<T2>, <Self::Table as Table>::Builder);

    fn imbue<T2: Table>(self, new_table: T2::Builder) -> Self::Imbued<T2> {
        self.imbue_release(new_table).0
    }
}

pub struct PtrReader<'a, T: Table = Empty> {
    ptr: WireRef<'a>,
    reader: ObjectReader<'a>,
    table: T::Reader,
    nesting_limit: u32,
}

impl<'a> PtrReader<'a, Empty> {
    pub const unsafe fn new_unchecked(ptr: NonNull<Word>) -> Self {
        Self {
            ptr: unsafe { WireRef(SegmentRef::from_ptr(ptr)) },
            reader: ObjectReader {
                segment: None,
                limiter: None,
            },
            table: Empty,
            nesting_limit: u32::MAX,
        }
    }

    pub(crate) fn root(
        segment: SegmentReader<'a>,
        limiter: Option<&'a ReadLimiter>,
        nesting_limit: u32,
    ) -> Self {
        Self {
            ptr: WireRef(segment.start()),
            reader: ObjectReader {
                segment: Some(segment),
                limiter,
            },
            table: Empty,
            nesting_limit,
        }
    }

    pub fn null() -> Self {
        unsafe { Self::new_unchecked(NonNull::from(Word::null())) }
    }
}

impl<'a, T: Table> PtrReader<'a, T> {
    #[inline]
    fn locate(&self) -> Result<Content<ObjectReader<'a>, WireRef<'a>>> {
        self.reader.clone().location_of(self.ptr)
    }

    #[inline]
    fn ptr(&self) -> &'a WirePtr {
        self.reader.deref_word(self.ptr.0).into()
    }

    #[inline]
    pub fn ptr_type(&self) -> Result<PtrType> {
        if self.ptr().is_null() {
            return Ok(PtrType::Null);
        }

        let Content { ptr, .. } = self.locate()?;
        let ptr: &WirePtr = self.reader.deref_word(ptr.0).into();
        if ptr.is_struct() {
            Ok(PtrType::Struct)
        } else if ptr.is_list() {
            Ok(PtrType::List)
        } else if ptr.is_capability() {
            Ok(PtrType::Capability)
        } else {
            Err(ptr.fail_read(None))
        }
    }

    #[inline]
    pub fn is_null(&self) -> bool {
        self.ptr().is_null()
    }

    #[inline]
    pub fn to_struct(&self) -> Result<Option<StructReader<'a, T>>> {
        if self.is_null() {
            return Ok(None);
        }

        self.to_struct_inner().map(Some)
    }

    fn to_struct_inner(&self) -> Result<StructReader<'a, T>> {
        let nesting_limit = self
            .nesting_limit
            .checked_sub(1)
            .ok_or(ErrorKind::NestingLimitExceeded)?;

        let Content {
            accessor: reader,
            ptr,
            location,
        } = self.locate()?;

        let struct_ptr = *ptr.deref_ptr(&reader).try_struct_ptr()?;

        let len = struct_ptr.len();

        let (struct_start, reader) = match location {
            Location::Near | Location::Far => {
                reader.try_read_object_from_end_of(ptr, struct_ptr.offset(), len)
            }
            Location::DoubleFar { segment, offset } => {
                reader.try_read_object_in(segment, offset, len)
            }
        }?;

        let data_start = struct_start.as_nonnull().cast();
        let ptrs_start = unsafe { WireRef(struct_start.offset(struct_ptr.data_size().into())) };
        Ok(StructReader {
            data_start,
            ptrs_start,
            reader,
            table: self.table.clone(),
            nesting_limit,
            data_len: struct_ptr.data_size() as u32 * Word::BYTES as u32,
            ptrs_len: struct_ptr.ptr_count(),
        })
    }

    #[inline]
    pub fn to_list(
        &self,
        expected_element_size: Option<PtrElementSize>,
    ) -> Result<Option<ListReader<'a, T>>> {
        if self.is_null() {
            return Ok(None);
        }

        return self.to_list_inner(expected_element_size).map(Some);
    }

    fn to_list_inner(
        &self,
        expected_element_size: Option<PtrElementSize>,
    ) -> Result<ListReader<'a, T>> {
        let Some(nesting_limit) = self.nesting_limit.checked_sub(1) else {
            return Err(ErrorKind::NestingLimitExceeded.into())
        };

        let Content {
            accessor: reader,
            ptr,
            location,
        } = self.locate()?;

        let wire_ptr = *ptr.deref_ptr(&reader);
        let list_ptr = *wire_ptr.try_list_ptr()?;
        match list_ptr.element_size() {
            element_size @ PtrElementSize::InlineComposite => {
                let word_count = list_ptr.element_count().get();
                // add one for the tag pointer, because this could overflow the size of a segment,
                // we just assume on overflow the bounds check fails and is out of bounds.
                let len = ObjectLen::new(word_count + 1).ok_or(ErrorKind::PointerOutOfBounds)?;

                let (tag_ptr, reader) = match location {
                    Location::Near | Location::Far => {
                        reader.try_read_object_from_end_of(ptr, list_ptr.offset(), len)
                    }
                    Location::DoubleFar { segment, offset } => {
                        reader.try_read_object_in(segment, offset, len)
                    }
                }?;

                let tag = WireRef(tag_ptr)
                    .deref_ptr(&reader)
                    .struct_ptr()
                    .ok_or(ErrorKind::UnsupportedInlineCompositeElementTag)?;

                // move past the tag pointer to get the start of the list
                let mut first = unsafe { tag_ptr.offset(1.into()) };

                let element_count = tag.inline_composite_element_count();
                let data_size = tag.data_size();
                let ptr_count = tag.ptr_count();
                let words_per_element = tag.size().total();

                if element_count as u64 * words_per_element as u64 > word_count.into() {
                    // Make sure the tag reports a struct size that matches the reported word count
                    return Err(ErrorKind::InlineCompositeOverrun.into());
                }

                if words_per_element == 0 {
                    // watch out for zero-sized structs, which can claim to be arbitrarily
                    // large without having sent actual data.
                    if !reader.try_amplified_read(element_count as u64) {
                        return Err(ErrorKind::ReadLimitExceeded.into());
                    }
                }

                // If a struct list was not expected, then presumably a non-struct list was
                // upgraded to a struct list. We need to manipulate the pointer to point at
                // the first field of the struct. Together with the `step` field, this will
                // allow the struct list to be accessed as if it were a primitive list without
                // branching.

                // Check whether the size is compatible.
                use PtrElementSize::*;
                match expected_element_size {
                    None | Some(Void | InlineComposite) => {}
                    Some(Bit) => return Err(wire_ptr.fail_read(Some(ExpectedRead::BitList))),
                    Some(Pointer) => {
                        if ptr_count == 0 {
                            return Err(ErrorKind::IncompatibleUpgrade.into());
                        }

                        // We expected a list of pointers but got a list of structs. Assuming
                        // the first field in the struct is the pointer we were looking for, we
                        // want to munge the pointer to point at the first element's pointer
                        // section

                        first = unsafe { first.offset(tag.data_size().into()) };
                    }
                    Some(Byte | TwoBytes | FourBytes | EightBytes) => {
                        if data_size == 0 {
                            return Err(ErrorKind::IncompatibleUpgrade.into());
                        }
                    }
                }

                Ok(ListReader {
                    ptr: first.as_nonnull().cast(),
                    reader,
                    table: self.table.clone(),
                    element_count,
                    step: words_per_element * Word::BYTES as u32,
                    nesting_limit,
                    struct_data_len: data_size as u32 * Word::BYTES as u32,
                    struct_ptrs_len: ptr_count,
                    element_size,
                })
            }
            element_size => {
                // Verify that the elements are at least as large as the expected type.
                // Note that if we expected InlineComposite, the expected sizes here will
                // be zero, because bounds checking will be performed at field access time.
                // So this check here is for the case where we expected a list of some
                // primitive or pointer type.
                if let Some(expected) = expected_element_size {
                    match (element_size, expected) {
                        // bit lists can't be "upgraded" to other primitives
                        (PtrElementSize::Bit, PtrElementSize::Bit) => {}
                        // pointer lists can't either
                        (PtrElementSize::Pointer, PtrElementSize::Pointer) => {}
                        // but if the list's data is larger than the expected data,
                        // we can just truncate or perform bounds checks at runtime
                        (ptr_element, expected)
                            if ptr_element.data_bits() >= expected.data_bits() => {}
                        _ => return Err(ErrorKind::IncompatibleUpgrade.into()),
                    }
                }

                let element_count = list_ptr.element_count().get();
                if element_size == PtrElementSize::Void {
                    if reader.try_amplified_read(element_count as u64) {
                        return Err(ErrorKind::ReadLimitExceeded.into());
                    }
                }

                // This is a primitive or pointer list, but all (except `List(Bool)`) such lists
                // can also be interpreted as struct lists. We need to compute the data size and
                // pointer count for such structs. For `List(Bool)`, we instead interpret all read
                // structs as zero-sized.
                let step = element_size.bits();
                let word_count = Word::round_up_bit_count(element_count as u64 * step as u64);
                let len = ObjectLen::new(word_count).unwrap();
                let (ptr, reader) = match location {
                    Location::Near | Location::Far => {
                        reader.try_read_object_from_end_of(ptr, list_ptr.offset(), len)
                    }
                    Location::DoubleFar { segment, offset } => {
                        reader.try_read_object_in(segment, offset, len)
                    }
                }?;

                Ok(ListReader {
                    ptr: ptr.as_nonnull().cast(),
                    reader,
                    table: self.table.clone(),
                    element_count,
                    step,
                    nesting_limit,
                    struct_data_len: element_size.data_bytes(),
                    struct_ptrs_len: element_size.pointers(),
                    element_size,
                })
            }
        }
    }

    #[inline]
    pub fn to_text(&self) -> Result<Option<BlobReader<'a>>> {
        if self.is_null() {
            return Ok(None);
        }

        self.to_text_inner().map(Some)
    }

    fn to_text_inner(&self) -> Result<BlobReader<'a>> {
        let Content {
            accessor: reader,
            ptr,
            location,
        } = self.locate()?;
        let wire_ptr = ptr.deref_ptr(&reader);
        let list_ptr = wire_ptr
            .list_ptr()
            .filter(|l| l.element_size() == PtrElementSize::Byte)
            .ok_or_else(|| wire_ptr.fail_read(Some(ExpectedRead::Text)))?;

        let element_count = list_ptr.element_count();
        let Some(len) = ObjectLen::new(Word::round_up_byte_count(element_count.into())) else {
            return Err(ErrorKind::TextNotNulTerminated.into());
        };
        let (ptr, _) = match location {
            Location::Near | Location::Far => {
                reader.try_read_object_from_end_of(ptr, list_ptr.offset(), len)
            }
            Location::DoubleFar { segment, offset } => {
                reader.try_read_object_in(segment, offset, len)
            }
        }?;

        let data = ptr.as_nonnull().cast::<u8>();
        let last = unsafe { *data.as_ptr().add((u32::from(element_count) - 1) as usize) };
        if last != 0 {
            return Err(ErrorKind::TextNotNulTerminated.into());
        }

        Ok(BlobReader {
            a: PhantomData,
            ptr: data,
            len: element_count.into(),
        })
    }

    #[inline]
    pub fn to_data(&self) -> Result<Option<BlobReader<'a>>> {
        if self.is_null() {
            return Ok(None);
        }

        self.to_data_inner().map(Some)
    }

    fn to_data_inner(&self) -> Result<BlobReader<'a>> {
        let Content {
            accessor: reader,
            ptr,
            location,
        } = self.locate()?;

        let wire_ptr = ptr.deref_ptr(&reader);
        let list_ptr = wire_ptr
            .list_ptr()
            .filter(|l| l.element_size() == PtrElementSize::Byte)
            .ok_or_else(|| wire_ptr.fail_read(Some(ExpectedRead::Data)))?;

        let element_count = list_ptr.element_count().get();
        let Some(len) = ObjectLen::new(Word::round_up_byte_count(element_count)) else {
            return Ok(BlobReader::empty());
        };
        let (ptr, _) = match location {
            Location::Near | Location::Far => {
                reader.try_read_object_from_end_of(ptr, list_ptr.offset(), len)
            }
            Location::DoubleFar { segment, offset } => {
                reader.try_read_object_in(segment, offset, len)
            }
        }?;

        Ok(BlobReader {
            a: PhantomData,
            ptr: ptr.as_nonnull().cast(),
            len: element_count,
        })
    }
}

impl<'a, T: Table> Capable for PtrReader<'a, T> {
    type Table = T;
    type Imbued<T2: Table> = PtrReader<'a, T2>;
}

impl<T: Table> CapableReader for PtrReader<'_, T> {
    #[inline]
    fn imbue_release<T2: Table>(self, new_table: T2::Reader) -> (Self::Imbued<T2>, T::Reader) {
        let old_table = self.table;
        let ptr = PtrReader {
            ptr: self.ptr,
            reader: self.reader,
            table: new_table,
            nesting_limit: self.nesting_limit,
        };
        (ptr, old_table)
    }
}

impl<T: CapSystem> PtrReader<'_, T> {
    /// Tries to read the pointer as a capability pointer.
    ///
    /// If the pointer is null, this returns None. If the pointer is not a capability or otherwise
    /// invalid, it returns an Err.
    #[inline]
    pub fn try_to_capability(&self) -> Result<Option<T::Cap>> {
        let ptr = self.ptr();

        if ptr.is_null() {
            return Ok(None);
        } else {
            let i = ptr.try_cap_ptr()?.capability_index();
            self.table
                .extract_cap(i)
                .ok_or_else(|| ErrorKind::InvalidCapabilityPointer(i).into())
                .map(Some)
        }
    }
}

impl<T: BreakableCapSystem> PtrReader<'_, T> {
    /// Reads the pointer as a capability pointer.
    #[inline]
    pub fn to_capability(&self) -> T::Cap {
        let ptr = self.ptr();
        if !ptr.is_null() {
            if let Some(cap) = ptr.cap_ptr() {
                if let Some(cap) = self.table.extract_cap(cap.capability_index()) {
                    cap
                } else {
                    T::broken("Read invalid capability pointer".to_owned())
                }
            } else {
                T::broken("Read non-capability pointer".to_owned())
            }
        } else {
            T::null()
        }
    }
}

impl Default for PtrReader<'_, Empty> {
    fn default() -> Self {
        Self::null()
    }
}

pub struct StructReader<'a, T: Table = Empty> {
    /// The start of the data section of the struct. This is a byte pointer in case of struct
    /// promotion
    data_start: NonNull<u8>,
    /// The start of the pointer section of the struct.
    ptrs_start: WireRef<'a>,
    /// The associated reader for the struct.
    reader: ObjectReader<'a>,
    /// The cap table for this struct
    table: T::Reader,
    /// The current nesting limit
    nesting_limit: u32,
    /// The size of the data section in bytes. We use bytes in case a primitive list is promoted
    /// into a struct list, where the primitive list element acts as the first data field in the
    /// struct. As such, this value must be in alignment with the alignment of the data pointer.
    /// If data_size_s is 8 (2 bytes), data_start must be two byte aligned.
    data_len: u32,
    /// The length of the ptr section in words.
    ptrs_len: u16,
}

impl<'a> StructReader<'a, Empty> {
    /// Makes a struct reader for a zero-sized struct.
    pub const fn empty() -> Self {
        Self {
            data_start: NonNull::dangling(),
            ptrs_start: unsafe { WireRef(SegmentRef::dangling()) },
            reader: ObjectReader {
                segment: None,
                limiter: None,
            },
            table: Empty,
            nesting_limit: u32::MAX,
            data_len: 0,
            ptrs_len: 0,
        }
    }

    /// Makes an unchecked struct reader. This is a struct reader suitable for reading default
    /// values in generated code.
    ///
    /// In generated code, a message is generated of all default values in a file and placed
    /// directly as raw words in a static array. This creates a struct reader that points at
    /// a given struct in that array, with no checks or limiter.
    ///
    /// As such any other use of this function is unsafe.
    pub const unsafe fn new_unchecked(ptr: NonNull<Word>, size: StructSize) -> Self {
        Self {
            data_start: ptr.cast(),
            ptrs_start: WireRef(
                SegmentRef::from_ptr(ptr)
                    .offset(SignedSegmentOffset::new(size.data as i32).unwrap()),
            ),
            reader: ObjectReader {
                segment: None,
                limiter: None,
            },
            table: Empty,
            nesting_limit: u32::MAX,
            data_len: size.data as u32 * Word::BYTES as u32,
            ptrs_len: size.ptrs,
        }
    }
}

impl<'a, T: Table> StructReader<'a, T> {
    /// Returns whether this struct can fit in the space allocated to a struct of the specified size
    #[inline]
    const fn fits(&self, size: StructSize) -> bool {
        let data_fits = Word::round_up_byte_count(self.data_len as u32) as u16 <= size.data;
        let ptrs_fit = self.ptrs_len <= size.ptrs;
        data_fits && ptrs_fit
    }

    #[inline]
    fn data(&self) -> *const u8 {
        self.data_start.as_ptr()
    }

    /// Returns the data section of this struct as a slice of bytes
    #[inline]
    pub fn data_section(&self) -> &'a [u8] {
        unsafe { core::slice::from_raw_parts(self.data(), self.data_len as usize) }
    }

    /// Returns the size of this struct's data section in bytes
    #[inline]
    pub fn data_size_bytes(&self) -> u32 {
        self.data_len
    }

    /// Returns the size of this struct's data section in words.
    ///
    /// In case of list struct promotion, this is rounded down rather than rounded up.
    #[inline]
    pub fn data_size(&self) -> u16 {
        (self.data_size_bytes() / Word::BYTES as u32) as u16
    }

    /// Returns the number of pointers in this struct
    #[inline]
    pub fn ptr_count(&self) -> u16 {
        self.ptrs_len
    }

    /// Returns the pointer section of this struct as a list reader
    #[inline]
    pub fn ptr_section(&self) -> ListReader<'a, T> {
        ListReader {
            reader: self.reader.clone(),
            ptr: self.ptrs_start.0.as_nonnull().cast(),
            table: self.table.clone(),
            element_count: self.ptrs_len as u32,
            step: 8,
            struct_data_len: 0,
            struct_ptrs_len: 1,
            element_size: PtrElementSize::Pointer,
            nesting_limit: self.nesting_limit,
        }
    }

    /// Reads a field in the data section from the specified slot
    #[inline]
    pub fn data_field<D: internal::FieldData>(&self, slot: usize) -> D {
        self.data_field_with_default(slot, D::default())
    }

    /// Reads a field in the data section with the specified slot and default value
    #[inline]
    pub fn data_field_with_default<D: internal::FieldData>(&self, slot: usize, default: D) -> D {
        unsafe { D::read(self.data(), self.data_len, slot, default) }
    }

    #[inline]
    pub fn ptr_field_option(&self, index: u16) -> Option<PtrReader<'a, T>> {
        if index < self.ptrs_len {
            Some(PtrReader {
                ptr: unsafe { WireRef(self.ptrs_start.0.offset(index.into())) },
                reader: self.reader.clone(),
                table: self.table.clone(),
                nesting_limit: self.nesting_limit,
            })
        } else {
            None
        }
    }

    #[inline]
    pub fn ptr_field(&self, index: u16) -> PtrReader<'a, T> {
        self.ptr_field_option(index)
            .unwrap_or_else(|| PtrReader::null().imbue(self.table.clone()))
    }
}

impl<'a, T: Table> Capable for StructReader<'a, T> {
    type Table = T;
    type Imbued<T2: Table> = StructReader<'a, T2>;
}

impl<T: Table> CapableReader for StructReader<'_, T> {
    fn imbue_release<T2: Table>(self, new_table: T2::Reader) -> (Self::Imbued<T2>, T::Reader) {
        let old_table = self.table;
        let ptr = StructReader {
            data_start: self.data_start,
            ptrs_start: self.ptrs_start,
            reader: self.reader,
            table: new_table,
            nesting_limit: self.nesting_limit,
            data_len: self.data_len,
            ptrs_len: self.ptrs_len,
        };
        (ptr, old_table)
    }
}

pub struct ListReader<'a, T: Table = Empty> {
    ptr: NonNull<u8>,
    reader: ObjectReader<'a>,
    /// A reader for the cap table associated with this list
    table: T::Reader,
    /// The length of this list
    element_count: u32,
    nesting_limit: u32,
    /// The number of *bytes* to multiply an index by to get the actual offset or element of an item
    step: u32,
    /// When reading an element as a struct, this is the number of bytes in the struct's data
    /// section
    struct_data_len: u32,
    /// When reading an element as a struct, this is the number of bytes in the struct's pointer
    /// section
    struct_ptrs_len: u16,
    element_size: PtrElementSize,
}

impl ListReader<'_, Empty> {
    /// Creates an empty list of void.
    pub fn empty() -> Self {
        Self::void_of_len(0.into())
    }

    /// Creates a void list of the specified length.
    pub fn void_of_len(len: ElementCount) -> Self {
        Self {
            ptr: NonNull::dangling(),
            reader: ObjectReader {
                segment: None,
                limiter: None,
            },
            table: Empty,
            element_count: len.into(),
            nesting_limit: u32::MAX,
            step: 0,
            struct_data_len: 0,
            struct_ptrs_len: 0,
            element_size: PtrElementSize::Void,
        }
    }
}

impl<'a, T: Table> ListReader<'a, T> {
    #[inline]
    pub(crate) fn clone_table_reader(&self) -> T::Reader {
        self.table.clone()
    }

    #[inline]
    pub fn len(&self) -> u32 {
        self.element_count
    }

    #[inline]
    pub fn ptr_element_size(&self) -> PtrElementSize {
        self.element_size
    }

    #[inline]
    pub fn element_size(&self) -> ElementSize {
        use PtrElementSize::*;
        match self.element_size {
            Void => ElementSize::Void,
            Bit => ElementSize::Bit,
            Byte => ElementSize::Byte,
            TwoBytes => ElementSize::TwoBytes,
            FourBytes => ElementSize::FourBytes,
            EightBytes => ElementSize::EightBytes,
            Pointer => ElementSize::Pointer,
            InlineComposite => ElementSize::InlineComposite(StructSize {
                data: (self.struct_data_len / Word::BYTES as u32) as u16,
                ptrs: self.struct_ptrs_len,
            }),
        }
    }

    /// Returns whether this list reader can be upgraded to an element of the specified size
    pub fn can_upgrade(&self, size: ElementSize) -> bool {
        todo!()
    }

    fn ptr(&self) -> *const u8 {
        self.ptr.as_ptr().cast_const()
    }

    #[inline]
    fn index_to_offset(&self, index: u32) -> usize {
        (self.step * index) as usize
    }

    /// Reads a primitive at the specified index.
    ///
    /// # Safety
    ///
    /// * The index must be within bounds
    /// * This is not a void or pointer list
    /// * If this is a bit list, D must be Bool
    /// * If this is a primitive list, D must have a size equal to or less than the element size
    ///   of the list
    /// * If this is a inline composite list, the struct must have a data section with at least
    ///   one word
    #[inline]
    pub unsafe fn data_unchecked<D: internal::FieldData>(&self, index: u32) -> D {
        use core::any::TypeId;
        if TypeId::of::<D>() == TypeId::of::<Bool>() {
            D::read_unchecked(self.ptr(), index as usize, D::default())
        } else {
            let ptr = self.ptr().add(self.index_to_offset(index));
            D::read_unchecked(ptr, 0, D::default())
        }
    }

    /// Gets a struct reader for the struct at the specified index.
    ///
    /// # Safety
    ///
    /// * The index must be within bounds
    /// * This must not be a bit list
    #[inline]
    pub unsafe fn struct_unchecked(&self, index: u32) -> StructReader<'a, T> {
        let struct_start = self.ptr().add(self.index_to_offset(index));
        let struct_data = struct_start;
        let struct_ptrs = struct_start
            .add(self.struct_data_len as usize)
            .cast::<Word>();

        StructReader {
            data_start: NonNull::new_unchecked(struct_data.cast_mut()),
            ptrs_start: WireRef(SegmentRef::from_ptr(NonNull::new_unchecked(
                struct_ptrs.cast_mut(),
            ))),
            reader: self.reader.clone(),
            table: self.table.clone(),
            nesting_limit: self.nesting_limit.saturating_sub(1),
            data_len: self.struct_data_len,
            ptrs_len: self.struct_ptrs_len,
        }
    }

    /// Gets a pointer reader for the pointer at the specified index.
    ///
    /// # Safety
    ///
    /// * The index must be within bounds
    /// * This must be a pointer list or struct list with a struct that has at least one pointer
    ///   in its pointer section
    #[inline]
    pub unsafe fn ptr_unchecked(&self, index: u32) -> PtrReader<'a, T> {
        let ptr = self
            .ptr()
            .add(self.index_to_offset(index))
            .cast_mut()
            .cast();

        PtrReader {
            ptr: WireRef(SegmentRef::from_ptr(NonNull::new_unchecked(ptr))),
            reader: self.reader.clone(),
            table: self.table.clone(),
            nesting_limit: self.nesting_limit,
        }
    }
}

impl<'a, T: Table> Capable for ListReader<'a, T> {
    type Table = T;
    type Imbued<T2: Table> = ListReader<'a, T2>;
}

impl<T: Table> CapableReader for ListReader<'_, T> {
    fn imbue_release<T2: Table>(
        self,
        new_table: T2::Reader,
    ) -> (Self::Imbued<T2>, <Self::Table as Table>::Reader) {
        let old_table = self.table;
        let ptr = ListReader {
            ptr: self.ptr,
            reader: self.reader,
            table: new_table,
            element_count: self.element_count,
            step: self.step,
            nesting_limit: self.nesting_limit,
            struct_data_len: self.struct_data_len,
            struct_ptrs_len: self.struct_ptrs_len,
            element_size: self.element_size,
        };
        (ptr, old_table)
    }
}

/// A reader for the blob types Data and Text
pub struct BlobReader<'a> {
    a: PhantomData<&'a [u8]>,
    ptr: NonNull<u8>,
    len: u32,
}

impl<'a> BlobReader<'a> {
    pub const fn empty() -> Self {
        Self {
            a: PhantomData,
            ptr: NonNull::dangling(),
            len: 0,
        }
    }

    pub const fn ptr(&self) -> NonNull<u8> {
        self.ptr
    }

    pub const fn len(&self) -> u32 {
        self.len
    }
}

/// A helper struct for building objects in a message
#[derive(Clone)]
struct ObjectBuilder<'a> {
    segment: SegmentBuilder<'a>,
}

impl<'a> ObjectBuilder<'a> {
    pub fn as_reader<'b>(&'b self) -> ObjectReader<'b> {
        ObjectReader {
            segment: Some(self.segment.as_reader()),
            limiter: None,
        }
    }

    fn deref_word(&self, r: SegmentRef<'a>) -> &'a mut Word {
        debug_assert!(self.segment.contains(r));
        unsafe { &mut *r.as_ptr_mut() }
    }

    fn deref_ptr(&self, r: WireRef<'a>) -> &'a mut WirePtr {
        debug_assert!(self.segment.contains(r.0));
        unsafe { &mut *r.0.as_ptr_mut().cast() }
    }

    pub fn locate(&self, ptr: WireRef<'a>) -> Content<ObjectBuilder<'a>, WireRef<'a>> {
        todo!()
    }

    pub fn clear_ptrs(&self, ptr: WireRef<'a>) {
        todo!()
    }

    pub fn clear_section(&self, start: SegmentRef<'a>, offset: SegmentOffset) {
        debug_assert!(self.segment.contains_section(start, offset.into()));

        unsafe { ptr::write_bytes(start.as_ptr_mut(), 0, offset.get() as usize) }
    }

    /// Returns an object pointer and builder based on the given wire pointer and offset
    #[inline]
    pub fn build_object_from(
        &self,
        WireRef(ptr): WireRef<'a>,
        offset: SignedSegmentOffset,
    ) -> (SegmentRef<'a>, Self) {
        let start = unsafe { ptr.offset_from_end(offset) };
        debug_assert!(
            self.segment.contains(ptr),
            "builder contains invalid pointer"
        );
        (start, self.clone())
    }

    /// Attempts to get an object builder for an object in the specified segment, or None if the
    /// segment is read-only.
    pub fn build_object_in(
        &self,
        segment: SegmentId,
        offset: SegmentOffset,
    ) -> Option<(SegmentRef<'a>, Self)> {
        let segment = self.segment.arena().segment_mut(segment)?;
        let ptr = segment.at_offset(offset);
        Some((ptr, Self { segment }))
    }

    /// Allocates a struct in the message, then configures the given pointer to point to it.
    #[inline]
    pub fn alloc_struct(&self, r: WireRef<'a>, size: StructSize) -> (SegmentRef<'a>, Self) {
        let Some(len) = AllocLen::new(size.total()) else {
            *self.deref_ptr(r) = WirePtr { struct_ptr: StructPtr::EMPTY };
            return (r.0, self.clone())
        };

        let (start, segment) = if let Some(start) = self.segment.alloc_in_segment(len) {
            // the struct can be allocated in this segment, nice
            let offset = self.segment.offset_from_end_of(r.0, start);
            *self.deref_ptr(r) = WirePtr {
                struct_ptr: StructPtr::new(offset, size),
            };

            (start, self.segment.clone())
        } else {
            // the struct can't be allocated in this segment, so we need to allocate for
            // a struct + landing pad
            // this unwrap is ok since we know that u16::MAX + u16::MAX + 1 can never be
            // larger than a segment
            let len_with_pad = AllocLen::new(size.total() + 1).unwrap();
            let (pad, object_segment) = self.segment.arena().alloc(len_with_pad);
            let start = unsafe { pad.offset(1.into()) };

            // our pad is the actual pointer to the content
            let offset_to_pad = object_segment.offset_from_start(pad);
            *self.deref_ptr(r) = WirePtr {
                far_ptr: FarPtr::new(object_segment.id(), offset_to_pad, false),
            };

            *self.deref_ptr(WireRef(pad)) = WirePtr {
                struct_ptr: StructPtr::new(0.into(), size),
            };

            (start, object_segment)
        };

        (start, ObjectBuilder { segment })
    }

    /// Allocates a list in the message, then configures the given pointer to point to it.
    #[inline]
    pub fn alloc_list(
        &self,
        r: WireRef<'a>,
        element_size: ElementSize,
        element_count: ElementCount,
    ) -> Option<(SegmentRef<'a>, Self)> {
        let is_struct_list = matches!(element_size, ElementSize::InlineComposite(_));

        let element_bits = element_size.bits() as u64;
        let total_words = Word::round_up_bit_count((element_count.get() as u64) * element_bits);

        let tag_size = u32::from(is_struct_list); // 1 for true, 0 for false
        let total_size = total_words + tag_size;
        if total_size == 0 {
            let end = self.segment.end();
            let offset_to_end = self.segment.offset_from_end_of(r.0, end);
            *self.deref_ptr(r) = WirePtr {
                list_ptr: ListPtr::new(offset_to_end, element_size.into(), element_count),
            };
            return Some((end, self.clone()));
        }

        let element_count = if is_struct_list {
            ElementCount::new(total_words).unwrap()
        } else {
            element_count
        };

        let len = AllocLen::new(total_size)?;
        let (start, segment) = if let Some(alloc) = self.segment.alloc_in_segment(len) {
            // we were able to allocate in our current segment, nice

            let offset = self.segment.offset_from_end_of(r.0, alloc);
            *self.deref_ptr(r) = WirePtr {
                list_ptr: ListPtr::new(offset, element_size.into(), element_count),
            };

            (alloc, self.segment.clone())
        } else if let Some(len_with_landing_pad) = AllocLen::new(total_size + 1) {
            // we couldn't allocate in this segment, but we can probably allocate a new list
            // somewhere else with a far landing pad

            let (pad, segment) = self.segment.arena().alloc(len_with_landing_pad);
            let start = unsafe { pad.offset(1.into()) };

            *self.deref_ptr(r) = WirePtr {
                far_ptr: FarPtr::new(segment.id(), 0.into(), false),
            };
            *self.deref_ptr(WireRef(pad)) = WirePtr {
                list_ptr: ListPtr::new(0.into(), element_size.into(), element_count),
            };

            (start, segment)
        } else {
            // ok the list is just too big for even one more word to be added to its alloc size
            // so we're going to have to make a separate double far landing pad

            self.alloc_segment_of_list(r, element_size, element_count)
        };

        let start = if let ElementSize::InlineComposite(size) = element_size {
            *self.deref_ptr(WireRef(start)) = WirePtr {
                struct_ptr: StructPtr::new(0.into(), size),
            };

            unsafe { start.offset(1.into()) }
        } else {
            start
        };

        Some((start, ObjectBuilder { segment }))
    }

    /// Allocate a list where the total size in words is equal to that of a segment.
    ///
    /// This should be _very_ rare, so we're making it a separate cold function.
    #[cold]
    fn alloc_segment_of_list(
        &self,
        r: WireRef<'a>,
        element_size: ElementSize,
        element_count: ElementCount,
    ) -> (SegmentRef<'a>, SegmentBuilder<'a>) {
        // try to allocate the landing pad _first_ since it likely will fit in either this
        // segment or the last segment in the message.
        // if the allocations were flipped, we'd probably allocate the pad in a completely new
        // segment, which is just a waste of space.

        let (pad, pad_segment) = self.segment.alloc(AllocLen::new(2).unwrap());
        let tag = unsafe { pad.offset(1.into()) };

        let (start, list_segment) = self.segment.arena().alloc(AllocLen::MAX);

        let offset_to_pad = pad_segment.offset_from_start(pad);
        *self.deref_ptr(r) = WirePtr {
            far_ptr: FarPtr::new(pad_segment.id(), offset_to_pad, true),
        };

        // I'm pretty sure if we can't add even one more word to a list allocation then the list
        // takes up the whole segment, which means this has to be 0, but I'm not taking chances.
        // Change it if you think it actually really matters.
        let offset_to_list = list_segment.offset_from_start(start);
        *self.deref_ptr(WireRef(pad)) = WirePtr {
            far_ptr: FarPtr::new(list_segment.id(), offset_to_list, false),
        };

        *self.deref_ptr(WireRef(tag)) = WirePtr {
            list_ptr: ListPtr::new(0.into(), element_size.into(), element_count),
        };

        (start, list_segment)
    }
}

enum CopySize<T> {
    /// Size is determined by the input value's size. If canonical is true, we use the canonical
    /// size.
    FromValue { canonical: bool },
    /// The size is explicitly set to this value. We use this when setting to a default value.
    Explicit(T),
}

/// A result for consuming PtrBuilder functions.
pub type BuildResult<T, U, E = Error> = CoreResult<T, (Option<E>, U)>;

pub struct PtrBuilder<'a, T: Table = Empty> {
    builder: ObjectBuilder<'a>,
    ptr: WireRef<'a>,
    table: T::Builder,
}

impl<'a> PtrBuilder<'a, Empty> {
    pub(crate) fn root(segment: SegmentBuilder<'a>) -> Self {
        let ptr = WireRef(segment.at_offset(0.into()));
        Self {
            builder: ObjectBuilder { segment },
            ptr,
            table: Empty,
        }
    }
}

impl<'a, T: Table> PtrBuilder<'a, T> {
    /// Locate the content this builder points to.
    fn locate(&mut self) -> Content<ObjectBuilder<'a>, WireRef<'a>> {
        self.builder.locate(self.ptr)
    }

    pub fn is_null(&self) -> bool {
        *self.builder.deref_ptr(self.ptr) == WirePtr::NULL
    }

    pub fn as_reader<'b>(&'b self) -> PtrReader<'b, T> {
        PtrReader {
            ptr: self.ptr,
            reader: self.builder.as_reader(),
            table: self.table.as_reader(),
            nesting_limit: u32::MAX,
        }
    }

    /// Borrows the pointer, rather than consuming it.
    pub fn by_ref<'b>(&'b mut self) -> PtrBuilder<'b, T> {
        PtrBuilder {
            builder: self.builder.clone(),
            ptr: self.ptr,
            table: self.table.clone(),
        }
    }

    pub fn to_struct(&self) -> Result<Option<StructReader<T>>> {
        self.as_reader().to_struct()
    }

    /// Gets a builder for a struct from this pointer.
    ///
    /// If the pointer is null this returns `Err((None, self))`. If an error occurs it's returned. If you don't
    /// care about either of these cases, use `struct_mut_or_init`, which will init the pointer in
    /// those cases.
    ///
    /// If struct of a specific size is expected and the existing struct is too small to fit, a
    /// copy will be made of the data to match the expected struct size.
    #[inline]
    pub fn to_struct_mut(
        self,
        expected_size: Option<StructSize>,
    ) -> BuildResult<StructBuilder<'a, T>, Self> {
        if self.is_null() {
            return Err((None, self));
        }

        self.to_struct_mut_inner(expected_size)
            .map_err(|(err, s)| (Some(err), s))
    }

    fn to_struct_mut_inner(
        mut self,
        expected_size: Option<StructSize>,
    ) -> CoreResult<StructBuilder<'a, T>, (Error, Self)> {
        let table = self.table.clone();
        let Content {
            accessor: builder,
            ptr,
            location,
        } = self.locate();
        let struct_ptr = match builder.deref_ptr(ptr).try_struct_ptr() {
            Ok(ptr) => ptr,
            Err(err) => return Err((err, self)),
        };

        let (existing_data_start, builder) = match location {
            Location::Near | Location::Far => builder.build_object_from(ptr, struct_ptr.offset()),
            Location::DoubleFar { segment, offset } => {
                match builder.build_object_in(segment, offset) {
                    Some(b) => b,
                    None => return Err((ErrorKind::WritingNotAllowed.into(), self)),
                }
            }
        };

        let StructSize {
            data: existing_data_size,
            ptrs: existing_ptr_count,
        } = struct_ptr.size();
        let existing_ptrs_start = unsafe { WireRef(ptr.0.offset(existing_data_size.into())) };

        let mut existing_builder = StructBuilder::<T> {
            builder,
            table,
            data_start: existing_data_start.as_nonnull().cast(),
            ptrs_start: existing_ptrs_start,
            data_len: existing_data_size as u32 * Word::BYTES as u32,
            ptrs_len: existing_ptr_count,
        };

        let builder = match expected_size {
            Some(size) if existing_data_size < size.data || existing_ptr_count < size.ptrs => {
                // The space allocated for this struct is too small.  Unlike with readers, we can't just
                // run with it and do bounds checks at access time, because how would we handle writes?
                // Instead, we have to copy the struct to a new space now.

                self.promote_struct(&mut existing_builder, size)
            }
            _ => existing_builder,
        };

        Ok(builder)
    }

    /// Promotes an existing struct to a larger size. We pass the existing struct as a struct
    /// builder because it keeps the parameters nice and neat.
    fn promote_struct(
        &mut self,
        existing: &mut StructBuilder<'a, T>,
        expected: StructSize,
    ) -> StructBuilder<'a, T> {
        todo!()
    }

    /// Initializes the pointer as a struct with the specified size and default value.
    ///
    /// This will replace any pre-existing pointer value here and zero its memory. Invalid data
    /// such as unknown pointers are simply zeroed. If a default is specified, it will be copied
    /// here.
    ///
    /// # Panics
    ///
    /// This code expects the provided reader is a valid default value. This code will panic if
    /// the provided reader
    ///
    ///   * Isn't unchecked
    ///   * Is larger than the provided struct size
    ///
    /// If any far or other pointers are in the provided struct, this silently writes null instead.
    pub fn init_struct(
        mut self,
        size: StructSize,
        default: Option<&StructReader<Empty>>,
    ) -> StructBuilder<'a, T> {
        if let Some(default) = default {
            assert!(
                default.reader.is_unchecked(),
                "default struct value isn't unchecked!"
            );
            assert!(default.fits(size), "default struct value is too large!");

            self.try_set_struct_with_size::<Infallible, _>(default, CopySize::Explicit(size), |err| {
                if cfg!(debug_assertions) {
                    panic!("encountered read error while copying default value during struct init: {}", err);
                }

                ControlFlow::Continue(WriteNull)
            }).map_err(|(i, _)| i).unwrap()
        } else {
            self.clear();
            self.init_struct_zeroed(size)
        }
    }

    /// Initializes the pointer as a struct with the specified size. This does not apply a default
    /// value to the struct.
    fn init_struct_zeroed(&mut self, size: StructSize) -> StructBuilder<'a, T> {
        let (struct_start, builder) = self.builder.alloc_struct(self.ptr, size);
        let data_start = struct_start.as_nonnull().cast();
        let ptrs_start = unsafe { WireRef(struct_start.offset(size.data.into())) };

        StructBuilder::<T> {
            builder,
            data_start,
            ptrs_start,
            table: self.table.clone(),
            data_len: (size.data as u32) * Word::BYTES as u32,
            ptrs_len: size.ptrs,
        }
    }

    /// Gets a builder for a struct from this pointer of the specified size.
    ///
    /// If the pointer is null or otherwise invalid this will initialize a new struct of the
    /// specified size with the given default value.
    ///
    /// # Panics
    ///
    /// This code expects the provided reader is a valid default value. This code will panic if
    /// the provided reader
    ///
    ///   * Isn't unchecked
    ///   * Is larger than the provided struct size
    pub fn to_struct_mut_or_init(
        self,
        size: StructSize,
        default: Option<&StructReader<Empty>>,
    ) -> StructBuilder<'a, T> {
        match self.to_struct_mut(Some(size)) {
            Ok(builder) => builder,
            Err((_, this)) => this.init_struct(size, default),
        }
    }

    /// Sets the pointer to a struct copied from the specified reader. If a size is provided, we
    /// allocate that much space no matter the actual size of the specified value. The value must
    /// fit in the given struct size.
    #[inline]
    fn try_set_struct_with_size<E, F>(
        mut self,
        value: &StructReader<impl Table>,
        size: CopySize<StructSize>,
        mut err_handler: F,
    ) -> CoreResult<StructBuilder<'a, T>, (E, Self)>
    where
        F: FnMut(Error) -> ControlFlow<E, WriteNull>,
    {
        let mut data_section = value.data_section();
        let ptr_section = value.ptr_section();

        let (size, canonical) = match size {
            CopySize::Explicit(size) => {
                debug_assert!(value.fits(size));
                (size, false)
            }
            CopySize::FromValue { canonical } => {
                let mut ptrs_len = value.ptrs_len;

                if canonical {
                    // truncate data section
                    while let Some((0, remainder)) = data_section.split_last() {
                        data_section = remainder;
                    }
                    for i in (0..(ptrs_len - 1)).rev() {
                        let ptr = unsafe { ptr_section.ptr_unchecked(i as u32) };
                        if ptr.is_null() {
                            ptrs_len = i;
                        } else {
                            break;
                        }
                    }
                }

                (
                    StructSize {
                        data: Word::round_up_byte_count(data_section.len() as u32) as u16,
                        ptrs: ptrs_len,
                    },
                    canonical,
                )
            }
        };

        self.clear();
        let mut builder = self.init_struct_zeroed(size);

        let builder_data = builder.data_section_mut();
        builder_data[..data_section.len()].copy_from_slice(data_section);

        let mut builder_ptrs = builder.ptr_section_mut();
        for i in 0..(size.ptrs as u32) {
            unsafe {
                let value_ptr = ptr_section.ptr_unchecked(i);
                let mut builder_ptr = builder_ptrs.ptr_mut_unchecked(i);
                if let Err(e) = builder_ptr.try_copy_from(&value_ptr, canonical, &mut err_handler) {
                    return Err((e, self));
                }
            }
        }

        Ok(builder)
    }

    pub fn try_set_struct<E, F>(
        self,
        value: &StructReader<impl Table>,
        canonical: bool,
        err_handler: F,
    ) -> CoreResult<StructBuilder<'a, T>, (E, Self)>
    where
        F: FnMut(Error) -> ControlFlow<E, WriteNull>,
    {
        self.try_set_struct_with_size(value, CopySize::FromValue { canonical }, err_handler)
    }

    pub fn set_struct(
        self,
        value: &StructReader<impl Table>,
        canonical: bool,
    ) -> StructBuilder<'a, T> {
        self.try_set_struct::<Infallible, _>(value, canonical, |_| ControlFlow::Continue(WriteNull))
            .map_err(|(i, _)| i)
            .unwrap()
    }

    pub fn to_list(
        &self,
        expected_element_size: Option<ElementSize>,
    ) -> Result<Option<ListReader<T>>> {
        self.as_reader()
            .to_list(expected_element_size.map(Into::into))
    }

    pub fn to_list_mut(
        self,
        expected_element_size: Option<ElementSize>,
    ) -> BuildResult<ListBuilder<'a, T>, Self> {
        todo!()
    }

    pub fn to_list_mut_or_empty(
        self,
        expected_element_size: Option<ElementSize>,
    ) -> ListBuilder<'a, T> {
        todo!()
    }

    /// Attempts to initialize this pointer as a new list.
    ///
    /// If the list is too large to fit in a Cap'n Proto message, this function returns an Err.
    ///
    /// If you're reasonably sure that this function won't fail, you can use `init_list`, which
    /// will panic on Err.
    pub fn try_init_list(
        mut self,
        element_size: ElementSize,
        element_count: ElementCount,
    ) -> CoreResult<ListBuilder<'a, T>, (Error, Self)> {
        self.clear();

        let Some((start, builder)) = self.builder.alloc_list(self.ptr, element_size, element_count) else {
            return Err((ErrorKind::AllocTooLarge.into(), self));
        };

        let step = element_size.bits() / 8;
        let (struct_data_size, struct_ptr_count) = {
            use ElementSize::*;
            match element_size {
                Pointer => (0, 1),
                InlineComposite(StructSize { data, ptrs }) => (data as u32 * 8, ptrs),
                primitive => (primitive.bits() / 8, 0),
            }
        };

        Ok(ListBuilder {
            builder,
            ptr: start,
            table: self.table,
            len: element_count.get(),
            step,
            struct_data_size,
            struct_ptr_count,
            element_size: element_size.into(),
        })
    }

    pub fn init_list(
        self,
        element_size: ElementSize,
        element_count: ElementCount,
    ) -> ListBuilder<'a, T> {
        self.try_init_list(element_size, element_count)
            .map_err(|(e, _)| e)
            .expect("too many elements for element size")
    }

    pub fn try_set_list<E, F: FnMut(Error) -> ControlFlow<E, WriteNull>>(
        self,
        value: &ListReader<impl Table>,
        canonical: bool,
        err_handler: F,
    ) -> CoreResult<ListBuilder<'a, T>, E> {
        todo!()
    }

    pub fn set_list(self, value: &ListReader<impl Table>, canonical: bool) -> ListBuilder<'a, T> {
        self.try_set_list::<Infallible, _>(value, canonical, |_| ControlFlow::Continue(WriteNull))
            .unwrap()
    }

    /// Transfer a pointer from one place to another.
    #[inline]
    fn transfer(&mut self, other: &mut PtrBuilder<impl Table>) {
        todo!()
    }

    /// Clears the pointer, passing any errors to a given error handler.
    ///
    /// The handler can choose to break with an error E, or continue and write null instead.
    ///
    /// You probably don't want this function, as writing null is a perfectly reasonable default
    /// to use. In which case, `clear` does this for you. You should only use this if you want
    /// strict validation or to customize error handling somehow.
    #[inline]
    pub fn try_clear<E, F>(&mut self, err_handler: F) -> CoreResult<(), E>
    where
        F: FnMut(Error) -> ControlFlow<E, WriteNull>,
    {
        Self::try_clear_inner(&self.builder, self.ptr, &self.table, err_handler)
    }

    fn try_clear_inner<E, F>(
        builder: &ObjectBuilder<'a>,
        ptr: WireRef<'a>,
        table: &T::Builder,
        mut err_handler: F,
    ) -> CoreResult<(), E>
    where
        F: FnMut(Error) -> ControlFlow<E, WriteNull>,
    {
        if *builder.deref_ptr(ptr) == WirePtr::NULL {
            return Ok(());
        }

        let Content {
            accessor: object_builder,
            ptr: content_ptr,
            location,
        } = builder.locate(ptr);
        let content = builder.deref_ptr(content_ptr);

        match content.kind() {
            WireKind::Struct => {
                let struct_ptr = content.struct_ptr().unwrap();
                let size = StructSize {
                    data: struct_ptr.data_size(),
                    ptrs: struct_ptr.ptr_count(),
                };
                if size != StructSize::EMPTY {
                    let (start, struct_builder) = match location {
                        Location::Near | Location::Far => {
                            object_builder.build_object_from(content_ptr, struct_ptr.offset())
                        }
                        Location::DoubleFar { segment, offset } => object_builder
                            .build_object_in(segment, offset)
                            .expect("struct pointers cannot refer to read-only segments"),
                    };

                    Self::try_clear_struct_inner(
                        &struct_builder,
                        start,
                        size,
                        table,
                        err_handler.borrow_mut(),
                    )?;
                }

                builder.clear_ptrs(ptr);
            }
            WireKind::List => {
                use PtrElementSize::*;

                let list_ptr = content.list_ptr().unwrap();
                let (start, list_builder) = match location {
                    Location::Near | Location::Far => {
                        object_builder.build_object_from(content_ptr, list_ptr.offset())
                    }
                    Location::DoubleFar { segment, offset } => match object_builder
                        .build_object_in(segment, offset)
                    {
                        Some(place) => place,
                        None if list_ptr.element_size() == Byte => todo!("delete external data"),
                        None => {
                            unreachable!("lists not of bytes cannot refer to read-only segments")
                        }
                    },
                };
                let count = list_ptr.element_count();
                match list_ptr.element_size() {
                    Pointer => {
                        for i in 0..count.get() {
                            let i = unsafe { ElementCount::new_unchecked(i) };
                            let ptr = unsafe { start.offset(i.into()) };
                            Self::try_clear_inner(
                                &list_builder,
                                WireRef(ptr),
                                table,
                                err_handler.borrow_mut(),
                            )?;
                        }
                    }
                    InlineComposite => {
                        let tag = *list_builder
                            .deref_ptr(WireRef(start))
                            .struct_ptr()
                            .expect("inline composite tag must be struct");

                        let list_start = unsafe { start.offset(1.into()) };

                        let size = StructSize {
                            data: tag.data_size(),
                            ptrs: tag.ptr_count(),
                        };

                        if size != StructSize::EMPTY {
                            let step = size.total();
                            for i in 0..tag.inline_composite_element_count() {
                                let offset = SegmentOffset::new(i * step).unwrap();
                                let struct_start = unsafe { list_start.offset(offset.into()) };
                                Self::try_clear_struct_inner(
                                    &list_builder,
                                    struct_start,
                                    size,
                                    table,
                                    err_handler.borrow_mut(),
                                )?;
                            }
                        }

                        *list_builder.deref_ptr(WireRef(start)) = WirePtr::NULL;
                    }
                    primitive => {
                        let len = SegmentOffset::new(Word::round_up_bit_count(
                            (primitive.bits() as u64) * (count.get() as u64),
                        ))
                        .unwrap();
                        list_builder.clear_section(start, len.into());
                    }
                }
                builder.clear_ptrs(ptr);
            }
            WireKind::Other => {
                let result = content
                    .cap_ptr()
                    .ok_or(Error::fail_read(Some(ExpectedRead::Capability), *content))
                    .and_then(|p| table.clear_cap(p.capability_index()));
                if let Err(err) = result {
                    if let ControlFlow::Break(e) = err_handler(err) {
                        return Err(e);
                    }
                }
                *builder.deref_ptr(ptr) = WirePtr::NULL;
            }
            WireKind::Far => unreachable!("invalid far pointer in builder"),
        }

        Ok(())
    }

    /// Clears a struct (including inline composite structs)
    fn try_clear_struct_inner<E, F>(
        builder: &ObjectBuilder<'a>,
        start: SegmentRef<'a>,
        size: StructSize,
        table: &T::Builder,
        mut err_handler: F,
    ) -> CoreResult<(), E>
    where
        F: FnMut(Error) -> ControlFlow<E, WriteNull>,
    {
        builder.clear_section(start, size.data.into());

        let ptrs_start = unsafe { start.offset(size.ptrs.into()) };
        for i in 0..size.ptrs {
            let ptr = unsafe { ptrs_start.offset(i.into()) };
            Self::try_clear_inner(&builder, WireRef(ptr), table, err_handler.borrow_mut())?;
        }

        Ok(())
    }

    /// Clears the pointer. If any errors occur while clearing any child objects, null is written.
    /// If you want a fallible clear or want to customize error handling behavior, use `try_clear`.
    #[inline]
    pub fn clear(&mut self) {
        self.try_clear::<Infallible, _>(|_| ControlFlow::Continue(WriteNull))
            .unwrap()
    }

    /// Performs a deep copy of the given pointer, optionally canonicalizing it.
    ///
    /// If any errors occur while reading from the given pointer, the error is passed to the given
    /// error handler function. The handler can choose to break with an error E, or continue and
    /// write null instead.
    ///
    /// You probably don't want this function, as writing null is a perfectly reasonable default
    /// to use in all cases. In which case `copy_from` does this for you. You should only use this
    /// if you want strict validation or to customize error handling somehow.
    #[inline]
    pub fn try_copy_from<E, F>(
        &mut self,
        other: &PtrReader<impl Table>,
        canonical: bool,
        mut err_handler: F,
    ) -> CoreResult<(), E>
    where
        F: FnMut(Error) -> ControlFlow<E, WriteNull>,
    {
        self.try_clear(err_handler.borrow_mut())?;

        let location_result = other.locate().map_err(err_handler.borrow_mut());
        let Content {
            accessor: reader,
            ptr,
            location,
        } = match location_result {
            Ok(content) => content,
            Err(ControlFlow::Break(err)) => return Err(err),
            Err(ControlFlow::Continue(WriteNull)) => return Ok(()),
        };

        let ptr_val = ptr.deref_ptr(&reader);
        match ptr_val.kind() {
            WireKind::Struct => {
                let struct_ptr = ptr_val.struct_ptr().unwrap();
            }
            WireKind::List => {
                let list_ptr = ptr_val.list_ptr().unwrap();
            }
            WireKind::Other => {}
            WireKind::Far => {
                if let ControlFlow::Break(err) = err_handler(Error::fail_read(None, *ptr_val)) {
                    return Err(err);
                }
            }
        }

        Ok(())
    }

    /// Performs a deep copy of the given pointer, optionally canonicalizing it.
    ///
    /// If any errors occur while copying, this writes null. If you want a fallible copy or want
    /// to customize error handling behavior, use `try_copy_from`.
    #[inline]
    pub fn copy_from(&mut self, other: &PtrReader<impl Table>, canonical: bool) {
        self.try_copy_from::<Infallible, _>(other, canonical, |_| ControlFlow::Continue(WriteNull))
            .unwrap()
    }
}

impl<'a, T: Table> Capable for PtrBuilder<'a, T> {
    type Table = T;
    type Imbued<T2: Table> = PtrBuilder<'a, T2>;
}

impl<T: Table> CapableBuilder for PtrBuilder<'_, T> {
    fn imbue_release<T2: Table>(
        self,
        new_table: T2::Builder,
    ) -> (Self::Imbued<T2>, <Self::Table as Table>::Builder) {
        let old_table = self.table;
        let ptr = PtrBuilder {
            builder: self.builder,
            ptr: self.ptr,
            table: new_table,
        };
        (ptr, old_table)
    }
}

pub struct StructBuilder<'a, T: Table = Empty> {
    builder: ObjectBuilder<'a>,
    data_start: NonNull<u8>,
    ptrs_start: WireRef<'a>,
    table: T::Builder,
    data_len: u32,
    ptrs_len: u16,
}

impl<'a, T: Table> Capable for StructBuilder<'a, T> {
    type Table = T;
    type Imbued<T2: Table> = StructBuilder<'a, T2>;
}

impl<T: Table> CapableBuilder for StructBuilder<'_, T> {
    fn imbue_release<T2: Table>(
        self,
        new_table: T2::Builder,
    ) -> (Self::Imbued<T2>, <Self::Table as Table>::Builder) {
        let old_table = self.table;
        let ptr = StructBuilder {
            builder: self.builder,
            data_start: self.data_start,
            ptrs_start: self.ptrs_start,
            table: new_table,
            data_len: self.data_len,
            ptrs_len: self.ptrs_len,
        };
        (ptr, old_table)
    }
}

impl<'a, T: Table> StructBuilder<'a, T> {
    pub fn as_reader<'b>(&'b self) -> StructReader<'b, T> {
        StructReader {
            reader: self.builder.as_reader(),
            data_start: self.data_start,
            ptrs_start: self.ptrs_start,
            table: self.table.as_reader(),
            data_len: self.data_len,
            ptrs_len: self.ptrs_len,
            nesting_limit: u32::MAX,
        }
    }

    #[inline]
    fn data_const(&self) -> *const u8 {
        self.data().cast_const()
    }

    #[inline]
    fn data(&self) -> *mut u8 {
        self.data_start.as_ptr()
    }

    #[inline]
    fn data_section_mut(&mut self) -> &mut [u8] {
        unsafe { core::slice::from_raw_parts_mut(self.data(), self.data_len as usize) }
    }

    /// Reads a field in the data section from the specified slot
    #[inline]
    pub fn data_field<D: internal::FieldData>(&self, slot: usize) -> D {
        self.data_field_with_default(slot, D::default())
    }

    /// Reads a field in the data section with the specified slot and default value
    #[inline]
    pub fn data_field_with_default<D: internal::FieldData>(&self, slot: usize, default: D) -> D {
        unsafe { D::read(self.data_const(), self.data_len, slot, default) }
    }

    /// Reads a field in the data section from the specified slot. This assumes the slot is valid
    /// and does not perform any bounds checking.
    #[inline]
    pub unsafe fn data_field_unchecked<D: internal::FieldData>(&self, slot: usize) -> D {
        self.data_field_with_default_unchecked(slot, D::default())
    }

    /// Reads a field in the data section with the specified slot and default value. This assumes
    /// the slot is valid and does not perform any bounds checking.
    #[inline]
    pub unsafe fn data_field_with_default_unchecked<D: internal::FieldData>(
        &self,
        slot: usize,
        default: D,
    ) -> D {
        D::read_unchecked(self.data_const(), slot, default)
    }

    /// Sets a field in the data section in the slot to the specified value. If the slot is outside
    /// the data section of this struct, this does nothing.
    #[inline]
    pub fn set_field<D: internal::FieldData>(&mut self, slot: usize, value: D) {
        unsafe { D::write(self.data(), self.data_len, slot, value, D::default()) }
    }

    /// Sets a field in the data section in the slot to the specified value. This assumes the slot
    /// is valid and does not perform any bounds checking.
    #[inline]
    pub unsafe fn set_field_unchecked<D: internal::FieldData>(&mut self, slot: usize, value: D) {
        self.set_field_with_default_unchecked(slot, value, D::default())
    }

    #[inline]
    pub fn set_field_with_default<D: internal::FieldData>(
        &mut self,
        slot: usize,
        value: D,
        default: D,
    ) {
        unsafe { D::write(self.data(), self.data_len, slot, value, default) }
    }

    #[inline]
    pub unsafe fn set_field_with_default_unchecked<D: internal::FieldData>(
        &mut self,
        slot: usize,
        value: D,
        default: D,
    ) {
        D::write_unchecked(self.data(), slot, value, default)
    }

    /// The number of pointers in this struct's pointer section
    #[inline]
    pub fn ptr_count(&self) -> u16 {
        self.ptrs_len
    }

    #[inline]
    fn ptr_section_mut<'b>(&'b mut self) -> ListBuilder<'b, T> {
        ListBuilder {
            builder: self.builder.clone(),
            ptr: self.ptrs_start.0,
            table: self.table.clone(),
            len: self.ptrs_len as u32,
            step: Word::BYTES as u32,
            struct_data_size: 0,
            struct_ptr_count: 1,
            element_size: PtrElementSize::Pointer,
        }
    }

    #[inline]
    pub fn ptr_field<'b>(&'b self, slot: u16) -> Option<PtrReader<'b, T>> {
        (slot < self.ptrs_len).then(move || unsafe { self.ptr_field_unchecked(slot) })
    }

    #[inline]
    pub unsafe fn ptr_field_unchecked<'b>(&'b self, slot: u16) -> PtrReader<'b, T> {
        PtrReader {
            ptr: WireRef(self.ptrs_start.0.offset(slot.into())),
            reader: self.builder.as_reader(),
            table: self.table.as_reader(),
            nesting_limit: u32::MAX,
        }
    }

    #[inline]
    pub fn ptr_field_mut<'b>(&'b mut self, slot: u16) -> Option<PtrBuilder<'b, T>> {
        (slot < self.ptrs_len).then(move || unsafe { self.ptr_field_mut_unchecked(slot) })
    }

    #[inline]
    pub unsafe fn ptr_field_mut_unchecked<'b>(&'b mut self, slot: u16) -> PtrBuilder<'b, T> {
        PtrBuilder {
            ptr: WireRef(self.ptrs_start.0.offset(slot.into())),
            builder: self.builder.clone(),
            table: self.table.clone(),
        }
    }
}

pub struct ListBuilder<'a, T: Table = Empty> {
    builder: ObjectBuilder<'a>,
    ptr: SegmentRef<'a>,
    table: T::Builder,
    len: u32,
    step: u32,
    struct_data_size: u32,
    struct_ptr_count: u16,
    element_size: PtrElementSize,
}

impl<'a, T: Table> ListBuilder<'a, T> {
    pub(crate) fn clone_table_reader(&self) -> T::Reader {
        self.table.as_reader()
    }

    pub fn len(&self) -> u32 {
        self.len
    }

    /// Reads a primitive at the specified index.
    ///
    /// # Safety
    ///
    /// * The index must be within bounds
    /// * This is not a void or pointer list
    /// * If this is a bit list, D must be Bool
    /// * If this is a primitive list, D must have a size equal to or less than the element size
    ///   of the list
    /// * If this is a inline composite list, the struct must have a data section with at least
    ///   one word
    #[inline]
    pub unsafe fn data_unchecked<D: internal::FieldData>(&self, index: u32) -> D {
        todo!()
    }
    #[inline]
    pub unsafe fn set_data_unchecked<D: internal::FieldData>(&mut self, index: u32, value: D) {
        todo!()
    }

    /// Gets a pointer reader for the pointer at the specified index.
    ///
    /// # Safety
    ///
    /// * The index must be within bounds
    /// * This must be a pointer list or struct list with a struct that has at least one pointer
    ///   in its pointer section
    #[inline]
    pub unsafe fn ptr_unchecked<'b>(&'b self, index: u32) -> PtrReader<'b, T> {
        todo!()
    }
    #[inline]
    pub unsafe fn ptr_mut_unchecked<'b>(&'b mut self, index: u32) -> PtrBuilder<'b, T> {
        todo!()
    }

    /// Gets a struct reader for the struct at the specified index.
    ///
    /// # Safety
    ///
    /// * The index must be within bounds
    /// * This must not be a bit list
    #[inline]
    pub unsafe fn struct_unchecked<'b>(&'b self, index: u32) -> StructReader<'b, T> {
        todo!()
    }
    #[inline]
    pub unsafe fn struct_mut_unchecked<'b>(&'b mut self, index: u32) -> StructBuilder<'b, T> {
        todo!()
    }
}

impl<'a, T: Table> Capable for ListBuilder<'a, T> {
    type Table = T;
    type Imbued<T2: Table> = ListBuilder<'a, T2>;
}

impl<T: Table> CapableBuilder for ListBuilder<'_, T> {
    fn imbue_release<T2: Table>(
        self,
        new_table: T2::Builder,
    ) -> (Self::Imbued<T2>, <Self::Table as Table>::Builder) {
        let old_table = self.table;
        let ptr = ListBuilder {
            builder: self.builder,
            ptr: self.ptr,
            table: new_table,
            len: self.len,
            step: self.step,
            struct_data_size: self.struct_data_size,
            struct_ptr_count: self.struct_ptr_count,
            element_size: self.element_size,
        };
        (ptr, old_table)
    }
}

pub struct BlobBuilder<'a> {
    a: PhantomData<&'a mut [u8]>,
    ptr: NonNull<u8>,
    len: u32,
}

impl<'a> BlobBuilder<'a> {
    pub fn ptr(&self) -> NonNull<u8> {
        self.ptr
    }

    pub fn len(&self) -> u32 {
        self.len
    }
}

#[cfg(test)]
mod tests {
    use crate::{
        alloc::{Alloc, AllocLen, Fixed, Global, Scratch, Space, Word},
        message::{Message, ReaderOptions},
        rpc::Empty,
    };
    use core::ptr::NonNull;

    use super::{ElementCount, ElementSize, PtrReader, StructBuilder, StructReader, StructSize};

    #[test]
    fn simple_struct() {
        // making AlignedData hard, easier to just write the bytes as numbers
        // and swap the bytes with u64::swap_bytes()
        let data = [
            Word(0x_00_00_00_00_01_00_00_00_u64.swap_bytes()),
            Word(0x_01_23_45_67_89_ab_cd_ef_u64.swap_bytes()),
        ];

        let ptr = unsafe { PtrReader::new_unchecked(NonNull::from(&data[0])) };
        let reader = ptr.to_struct().unwrap().unwrap();

        assert_eq!(0xefcdab8967452301, reader.data_field::<u64>(0));
        assert_eq!(0, reader.data_field::<u64>(1));
        assert_eq!(0x67452301, reader.data_field::<u32>(0));
        assert_eq!(0xefcdab89, reader.data_field::<u32>(1));
        assert_eq!(0, reader.data_field::<u32>(2));
        assert_eq!(0x2301, reader.data_field::<u16>(0));
        assert_eq!(0x6745, reader.data_field::<u16>(1));
        assert_eq!(0xab89, reader.data_field::<u16>(2));
        assert_eq!(0xefcd, reader.data_field::<u16>(3));
        assert_eq!(0, reader.data_field::<u16>(4));

        assert_eq!(
            321 ^ 0xefcdab8967452301,
            reader.data_field_with_default::<u64>(0, 321)
        );
        assert_eq!(
            321 ^ 0x67452301,
            reader.data_field_with_default::<u32>(0, 321)
        );
        assert_eq!(321 ^ 0x2301, reader.data_field_with_default::<u16>(0, 321));
        assert_eq!(321, reader.data_field_with_default::<u64>(1, 321));
        assert_eq!(321, reader.data_field_with_default::<u32>(2, 321));
        assert_eq!(321, reader.data_field_with_default::<u16>(4, 321));

        // Bits
        assert_eq!(true, reader.data_field(0));
        assert_eq!(false, reader.data_field(1));
        assert_eq!(false, reader.data_field(2));
        assert_eq!(false, reader.data_field(3));
        assert_eq!(false, reader.data_field(4));
        assert_eq!(false, reader.data_field(5));
        assert_eq!(false, reader.data_field(6));
        assert_eq!(false, reader.data_field(7));

        assert_eq!(true, reader.data_field(8));
        assert_eq!(true, reader.data_field(9));
        assert_eq!(false, reader.data_field(10));
        assert_eq!(false, reader.data_field(11));
        assert_eq!(false, reader.data_field(12));
        assert_eq!(true, reader.data_field(13));
        assert_eq!(false, reader.data_field(14));
        assert_eq!(false, reader.data_field(15));

        assert_eq!(true, reader.data_field(63));
        assert_eq!(false, reader.data_field(64));

        assert_eq!(true, reader.data_field_with_default(0, false));
        assert_eq!(false, reader.data_field_with_default(1, false));
        assert_eq!(true, reader.data_field_with_default(63, false));
        assert_eq!(false, reader.data_field_with_default(64, false));
        assert_eq!(false, reader.data_field_with_default(0, true));
        assert_eq!(true, reader.data_field_with_default(1, true));
        assert_eq!(false, reader.data_field_with_default(63, true));
        assert_eq!(true, reader.data_field_with_default(64, true));
    }

    fn setup_struct(builder: &mut StructBuilder<Empty>) {
        builder.set_field::<u64>(0, 0x1011121314151617);
        builder.set_field::<u32>(2, 0x20212223);
        builder.set_field::<u16>(6, 0x3031);
        builder.set_field::<u8>(14, 0x40);
        builder.set_field::<bool>(120, false);
        builder.set_field::<bool>(121, false);
        builder.set_field::<bool>(122, true);
        builder.set_field::<bool>(123, false);
        builder.set_field::<bool>(124, true);
        builder.set_field::<bool>(125, true);
        builder.set_field::<bool>(126, true);
        builder.set_field::<bool>(127, false);

        {
            let mut sub_struct = builder
                .ptr_field_mut(0)
                .unwrap()
                .init_struct(StructSize { data: 1, ptrs: 0 }, None);
            sub_struct.set_field::<u32>(0, 123);
        }

        {
            let mut list = builder
                .ptr_field_mut(1)
                .unwrap()
                .init_list(ElementSize::FourBytes, 3.into());
            assert_eq!(list.len(), 3);
            unsafe {
                list.set_data_unchecked::<i32>(0, 200);
                list.set_data_unchecked::<i32>(1, 201);
                list.set_data_unchecked::<i32>(2, 202);
            }
        }

        {
            let mut list = builder.ptr_field_mut(2).unwrap().init_list(
                ElementSize::InlineComposite(StructSize { data: 1, ptrs: 1 }),
                4.into(),
            );
            assert_eq!(list.len(), 4);
            for i in 0..4 {
                let mut builder = unsafe { list.struct_mut_unchecked(i) };
                builder.set_field(0, 300 + i);
                builder
                    .ptr_field_mut(0)
                    .unwrap()
                    .init_struct(StructSize { data: 1, ptrs: 0 }, None)
                    .set_field(0, 400 + i);
            }
        }

        {
            let mut list = builder
                .ptr_field_mut(3)
                .unwrap()
                .init_list(ElementSize::Pointer, 5.into());
            assert_eq!(list.len(), 5);
            for i in 0..5 {
                let mut element = unsafe {
                    list.ptr_mut_unchecked(i)
                        .init_list(ElementSize::TwoBytes, ElementCount::new(i + 1).unwrap())
                };
                assert_eq!(element.len(), i + 1);
                for j in 0..i {
                    unsafe {
                        element.set_data_unchecked::<u16>(j, 500 + j as u16);
                    }
                }
            }
        }
    }

    fn check_struct_reader(reader: &StructReader<Empty>) {
        assert_eq!(0x1011121314151617, reader.data_field::<u64>(0));
        assert_eq!(0x20212223, reader.data_field::<u32>(2));
        assert_eq!(0x3031, reader.data_field::<u16>(6));
        assert_eq!(0x40, reader.data_field(14));
        assert_eq!(false, reader.data_field(120));
        assert_eq!(false, reader.data_field(121));
        assert_eq!(true, reader.data_field(122));
        assert_eq!(false, reader.data_field(123));
        assert_eq!(true, reader.data_field(124));
        assert_eq!(true, reader.data_field(125));
        assert_eq!(true, reader.data_field(126));
        assert_eq!(false, reader.data_field(127));

        {
            let substruct = reader.ptr_field(0).to_struct().unwrap().unwrap();
            assert_eq!(123, substruct.data_field::<u32>(0));
        }

        {
            let list = reader
                .ptr_field(1)
                .to_list(Some(ElementSize::FourBytes.into()))
                .unwrap()
                .unwrap();
            assert_eq!(3, list.len());
            for i in 0..3 {
                unsafe {
                    assert_eq!(200 + i, list.data_unchecked(i));
                }
            }
        }

        {
            let list = reader
                .ptr_field(2)
                .to_list(Some(crate::ptr::PtrElementSize::InlineComposite))
                .unwrap()
                .unwrap();
        }
    }

    fn check_struct_builder(builder: &StructBuilder<Empty>) {
        todo!()
    }

    fn struct_round_trip(message: &mut Message<impl Alloc>) {
        {
            let mut builder = message
                .builder()
                .into_root()
                .init_struct(StructSize { data: 2, ptrs: 4 }, None);
            setup_struct(&mut builder);

            check_struct_builder(&builder);
            check_struct_reader(&builder.as_reader());
        }

        let reader = message.reader();
        let root_reader = reader.root().to_struct().unwrap().unwrap();

        check_struct_reader(&root_reader);

        let limited_reader = message.reader_with_options(ReaderOptions {
            nesting_limit: 4,
            traversal_limit: u64::MAX,
        });
        let root_reader = limited_reader.root().to_struct().unwrap().unwrap();

        check_struct_reader(&root_reader);
    }

    #[test]
    fn struct_round_trip_one_segment() {
        // word count:
        //    1  root pointer
        //    6  root struct
        //    1  sub message
        //    2  3-element int32 list
        //   13  struct list
        //         1 tag
        //        12 4x struct
        //           1 data section
        //           1 pointer section
        //           1 sub-struct
        //   11  list list
        //         5 pointers to sub-lists
        //         6 sub-lists (4x 1 word, 1x 2 words)
        // -----
        //   34
        let mut space = Space::<34>::VALUE;
        let alloc = Scratch::with_space(&mut space, Global);
        let mut message = Message::new(alloc);
        struct_round_trip(&mut message);

        let segments = message.segments().unwrap();
        assert_eq!(1, segments.len());
        assert_eq!(34, segments.first.len());
    }

    #[test]
    fn struct_round_trip_one_segment_per_allocation() {
        let alloc = Global;
        let mut message = Message::new(alloc);
        struct_round_trip(&mut message);

        let segments: Vec<_> = message.segments().into_iter().flatten().collect();
        assert_eq!(15, segments.len());

        // Check each segment size
        assert_eq!(1, segments[0].len());
        assert_eq!(7, segments[1].len());
        assert_eq!(2, segments[2].len());
        assert_eq!(3, segments[3].len());
        assert_eq!(10, segments[4].len());
        assert_eq!(2, segments[5].len());
        assert_eq!(2, segments[6].len());
        assert_eq!(2, segments[7].len());
        assert_eq!(2, segments[8].len());
        assert_eq!(6, segments[9].len());
        assert_eq!(2, segments[10].len());
        assert_eq!(2, segments[11].len());
        assert_eq!(2, segments[12].len());
        assert_eq!(2, segments[13].len());
        assert_eq!(3, segments[14].len());
    }

    #[test]
    fn struct_round_trip_multiple_segments_with_multiple_allocations() {
        let alloc = Fixed::new(AllocLen::new(8).unwrap(), Global);
        let mut message = Message::new(alloc);
        struct_round_trip(&mut message);

        let segments: Vec<_> = message.segments().into_iter().flatten().collect();
        assert_eq!(6, segments.len());

        // Check each segment size
        assert_eq!(8, segments[0].len());
        assert_eq!(3, segments[1].len());
        assert_eq!(10, segments[2].len());
        assert_eq!(8, segments[3].len());
        assert_eq!(8, segments[4].len());
        assert_eq!(7, segments[5].len());
    }
}
