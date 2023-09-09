//! Manually manage Cap'n Proto data through raw readers and builders

use crate::alloc::{
    u29, AllocLen, ElementCount, ObjectLen, SegmentOffset, SignedSegmentOffset, Word,
};
use crate::message::internal::{
    ReadLimiter, SegmentBuilder, SegmentPtr, SegmentReader, SegmentRef,
};
use crate::message::SegmentId;
use crate::rpc::internal::CapPtrBuilder;
use crate::rpc::{
    BreakableCapSystem, CapTable, CapTableReader, Capable, Empty, InsertableInto, Table,
};
use crate::text::ByteCount as TextByteCount;
use crate::ty;
use crate::{Error, ErrorKind, Result};
use core::borrow::BorrowMut;
use core::convert::{Infallible, TryFrom};
use core::fmt::Debug;
use core::ops::ControlFlow;
use core::ptr::{self, NonNull};
use core::{fmt, slice};

pub use crate::data::ptr::{Reader as DataReader, Builder as DataBuilder};
pub use crate::text::ptr::{Reader as TextReader, Builder as TextBuilder};

pub(crate) mod internal {
    use super::*;

    pub trait FieldData: ty::ListValue + Default + Copy + 'static {
        unsafe fn read(ptr: *const u8, len: u32, slot: usize, default: Self) -> Self;
        unsafe fn read_unchecked(ptr: *const u8, slot: usize, default: Self) -> Self;

        unsafe fn write(ptr: *mut u8, len: u32, slot: usize, value: Self, default: Self);
        unsafe fn write_unchecked(ptr: *mut u8, slot: usize, value: Self, default: Self);
    }

    impl FieldData for bool {
        #[inline]
        unsafe fn read(ptr: *const u8, len_bytes: u32, slot: usize, default: Self) -> Self {
            let byte_offset = slot / 8;
            if byte_offset < (len_bytes as usize) {
                Self::read_unchecked(ptr, slot, default)
            } else {
                default
            }
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
        ($ty:ty) => {
            impl FieldData for $ty {
                #[inline]
                unsafe fn read(ptr: *const u8, len_bytes: u32, slot: usize, default: Self) -> Self {
                    let slot_byte_offset = slot * core::mem::size_of::<Self>();
                    if slot_byte_offset < len_bytes as usize {
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
                    let slot_byte_offset = slot * core::mem::size_of::<Self>();
                    if slot_byte_offset < len_bytes as usize {
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

    impl_int!(u8);
    impl_int!(i8);
    impl_int!(u16);
    impl_int!(i16);
    impl_int!(u32);
    impl_int!(i32);
    impl_int!(u64);
    impl_int!(i64);

    impl FieldData for f32 {
        #[inline]
        unsafe fn read(ptr: *const u8, len_bytes: u32, slot: usize, default: Self) -> Self {
            Self::from_bits(u32::read(ptr, len_bytes, slot, default.to_bits()))
        }
        #[inline]
        unsafe fn read_unchecked(ptr: *const u8, slot: usize, default: Self) -> Self {
            Self::from_bits(u32::read_unchecked(ptr, slot, default.to_bits()))
        }

        #[inline]
        unsafe fn write(ptr: *mut u8, len_bytes: u32, slot: usize, value: Self, default: Self) {
            u32::write(ptr, len_bytes, slot, value.to_bits(), default.to_bits())
        }
        #[inline]
        unsafe fn write_unchecked(ptr: *mut u8, slot: usize, value: Self, default: Self) {
            u32::write_unchecked(ptr, slot, value.to_bits(), default.to_bits())
        }
    }

    impl FieldData for f64 {
        #[inline]
        unsafe fn read(ptr: *const u8, len_bytes: u32, slot: usize, default: Self) -> Self {
            Self::from_bits(u64::read(ptr, len_bytes, slot, default.to_bits()))
        }
        #[inline]
        unsafe fn read_unchecked(ptr: *const u8, slot: usize, default: Self) -> Self {
            Self::from_bits(u64::read_unchecked(ptr, slot, default.to_bits()))
        }

        #[inline]
        unsafe fn write(ptr: *mut u8, len_bytes: u32, slot: usize, value: Self, default: Self) {
            u64::write(ptr, len_bytes, slot, value.to_bits(), default.to_bits())
        }
        #[inline]
        unsafe fn write_unchecked(ptr: *mut u8, slot: usize, value: Self, default: Self) {
            u64::write_unchecked(ptr, slot, value.to_bits(), default.to_bits())
        }
    }
}

#[derive(Clone, Copy, PartialEq, Eq, Debug)]
#[repr(transparent)]
pub struct WireValue<T>(T);

macro_rules! wire_value_type {
    ($($ty:ty),+) => {
        $(impl WireValue<$ty> {
            #[inline]
            pub const fn new(value: $ty) -> Self {
                Self(value.to_le())
            }

            #[inline]
            pub const fn get(&self) -> $ty {
                self.0.to_le()
            }

            #[inline]
            pub fn set(&mut self, value: $ty) {
                self.0 = value.to_le();
            }

            #[inline]
            pub fn from_word_slice<'a>(words: &'a [Word]) -> &'a [Self] {
                let (_, values, _) = unsafe { words.align_to() };
                values
            }

            #[inline]
            pub fn from_word_slice_mut<'a>(words: &'a mut [Word]) -> &'a mut [Self] {
                let (_, values, _) = unsafe { words.align_to_mut() };
                values
            }
        })+
    };
}

wire_value_type! {
    u8, i8, u16, i16, u32, i32, u64, i64
}

#[derive(Default, Debug, Clone, PartialEq, Eq, PartialOrd, Ord, Hash)]
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

#[derive(Default, Debug, Clone, Copy, PartialEq, Eq, PartialOrd, Ord, Hash)]
pub struct StructSize {
    /// The number of words of data in this struct
    pub data: u16,
    /// The number of pointers in this struct
    pub ptrs: u16,
}

impl StructSize {
    pub const EMPTY: StructSize = StructSize { data: 0, ptrs: 0 };

    #[inline]
    pub const fn is_empty(self) -> bool {
        matches!(self, Self::EMPTY)
    }

    /// Gets the total size of the struct in words
    #[inline]
    pub const fn total(self) -> u32 {
        self.data as u32 + self.ptrs as u32
    }

    /// Gets the max number of elements an struct list can contain of this struct
    #[inline]
    pub const fn max_elements(self) -> ElementCount {
        if self.is_empty() {
            ElementCount::MAX
        } else {
            // subtract 1 for the tag ptr
            ElementCount::new((ElementCount::MAX_VALUE - 1) / (self.total())).unwrap()
        }
    }

    /// Calculate the max struct size of two sizes. This chooses the max data and pointer section
    /// sizes from the given struct sizes.
    /// 
    /// # Example
    /// 
    /// ```
    /// use recapn::ptr::StructSize;
    /// 
    /// let a = StructSize { data: 2, ptrs: 0 };
    /// let b = StructSize { data: 0, ptrs: 5 };
    /// 
    /// let c = StructSize::max(a, b);
    /// assert_eq!(c, StructSize { data: 2, ptrs: 5 });
    /// ```
    #[inline]
    pub fn max(self, other: Self) -> Self {
        Self {
            data: self.data.max(other.data),
            ptrs: self.ptrs.max(other.ptrs),
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
#[repr(C)]
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

    fn parts(&self) -> &Parts {
        unsafe { &self.parts }
    }

    /// Returns the kind of pointer
    #[inline]
    pub fn kind(&self) -> WireKind {
        match self.parts().lower.get() as u8 & 3 {
            0 => WireKind::Struct,
            1 => WireKind::List,
            2 => WireKind::Far,
            3 => WireKind::Other,
            _ => unreachable!(),
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
        self.parts().lower.get() == WireKind::Other as u32
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

impl From<WirePtr> for Word {
    fn from(value: WirePtr) -> Self {
        unsafe { value.word }
    }
}

impl SegmentRef<'_> {
    pub fn as_wire_ptr(&self) -> &WirePtr {
        self.as_ref().into()
    }
}

impl fmt::Debug for WirePtr {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        match self.kind() {
            WireKind::Struct => self.struct_ptr().unwrap().fmt(f),
            WireKind::List => self.list_ptr().unwrap().fmt(f),
            WireKind::Far => self.far_ptr().unwrap().fmt(f),
            WireKind::Other => {
                if let Some(cap) = self.cap_ptr() {
                    cap.fmt(f)
                } else {
                    unsafe { self.word.fmt(f) }
                }
            }
        }
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
                upper: WireValue::<u32>::new(data as u32 | ((ptrs as u32) << 16)),
                lower: WireValue::<u32>::new((offset.get() << 2) as u32),
            },
        }
    }

    #[inline]
    pub const fn new_inline_composite_tag(
        count: ElementCount,
        StructSize { data, ptrs }: StructSize,
    ) -> Self {
        Self {
            parts: Parts {
                upper: WireValue((data as u32 | ((ptrs as u32) << 16)).to_le()),
                lower: WireValue((count.get() << 2).to_le()),
            },
        }
    }

    #[inline]
    pub fn offset(&self) -> SignedSegmentOffset {
        SignedSegmentOffset::new(self.parts.lower.get() as i32 >> 2).unwrap()
    }

    #[inline]
    pub fn inline_composite_element_count(&self) -> ElementCount {
        ElementCount::new((self.parts.lower.get() >> 2) & u29::MAX_VALUE).unwrap()
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

impl Debug for StructPtr {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        f.debug_struct("StructPtr")
            .field("offset", &self.offset())
            .field("data_size", &self.data_size())
            .field("ptrs_size", &self.ptr_count())
            .finish()
    }
}

impl From<StructPtr> for WirePtr {
    fn from(value: StructPtr) -> Self {
        WirePtr { struct_ptr: value }
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
    #[inline]
    pub const fn bits(&self) -> u32 {
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
    /// really matters for struct elements, since structs can overflow the max segment size
    /// if they're not zero sized.
    #[inline]
    pub const fn max_elements(&self) -> ElementCount {
        match self {
            Self::InlineComposite(size) => size.max_elements(),
            _ => ElementCount::MAX,
        }
    }

    /// Returns whether a list with elements of this size can be upgraded to a list of another
    /// element size.
    #[inline]
    pub fn upgradable_to(&self, other: PtrElementSize) -> bool {
        match (self, other) {
            // Structs can always upgrade to other inline composites
            (ElementSize::InlineComposite(_), PtrElementSize::InlineComposite) => true,
            // But can't be upgraded to bit lists
            (ElementSize::InlineComposite(_), PtrElementSize::Bit) => false,
            // Structs need one pointer field to upgrade to a pointer list
            (ElementSize::InlineComposite(StructSize { ptrs: 0, .. }), PtrElementSize::Pointer) => {
                false
            }
            // And need one data word to upgrade to a data list
            (
                ElementSize::InlineComposite(StructSize { data: 0, .. }),
                PtrElementSize::Byte
                | PtrElementSize::TwoBytes
                | PtrElementSize::FourBytes
                | PtrElementSize::EightBytes,
            ) => false,
            (ElementSize::Bit, PtrElementSize::Bit) => true,
            (ElementSize::Pointer, PtrElementSize::Pointer) => true,
            // This upgrade is valid as long as the new data is larger than the old
            (s, o) if s.bits() >= o.bits() => true,
            _ => false,
        }
    }

    /// Gets the ElementSize of the given static list value type.
    #[inline]
    pub const fn size_of<T: ty::ListValue>() -> ElementSize {
        <T as ty::ListValue>::ELEMENT_SIZE
    }

    /// Gets a suitable ElementSize for an empty list of the given list value type.
    /// 
    /// This is written to support empty default list readers, specifically empty lists
    /// of any struct, which need an element size for the inline composite elements.
    /// `AnyStruct` does not implement `ListValue` since it doesn't have a static
    /// list element size, so we use this and specify an empty inline composite element
    /// for empty lists.
    #[inline]
    pub const fn empty_size_of<T: ty::DynListValue>() -> ElementSize {
        match PtrElementSize::size_of::<T>() {
            PtrElementSize::Void => ElementSize::Void,
            PtrElementSize::Bit => ElementSize::Bit,
            PtrElementSize::Byte => ElementSize::Byte,
            PtrElementSize::TwoBytes => ElementSize::TwoBytes,
            PtrElementSize::FourBytes => ElementSize::FourBytes,
            PtrElementSize::EightBytes => ElementSize::EightBytes,
            PtrElementSize::Pointer => ElementSize::Pointer,
            PtrElementSize::InlineComposite => ElementSize::InlineComposite(StructSize::EMPTY),
        }
    }

    #[inline]
    pub const fn as_ptr_size(&self) -> PtrElementSize {
        match self {
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
        size.as_ptr_size()
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

    pub const fn size_of<T: ty::DynListValue>() -> Self {
        <T as ty::DynListValue>::PTR_ELEMENT_SIZE
    }
}

#[repr(transparent)]
#[derive(Clone, Copy, PartialEq)]
pub struct ListPtr {
    parts: Parts,
}

impl ListPtr {
    #[inline]
    pub const fn new(
        offset: SignedSegmentOffset,
        size: PtrElementSize,
        count: ElementCount,
    ) -> Self {
        Self {
            parts: Parts {
                upper: WireValue::<u32>::new(count.get() << 3 | size as u32),
                lower: WireValue::<u32>::new((offset.get() << 2) as u32 | WireKind::List as u32),
            },
        }
    }

    #[inline]
    pub fn element_size(&self) -> PtrElementSize {
        match self.parts.upper.get() as u8 & 0b111 {
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

impl Debug for ListPtr {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        f.debug_struct("ListPtr")
            .field("offset", &self.offset())
            .field("element_size", &self.element_size())
            .field("element_count", &self.element_count())
            .finish()
    }
}

impl From<ListPtr> for WirePtr {
    fn from(value: ListPtr) -> Self {
        WirePtr { list_ptr: value }
    }
}

#[repr(transparent)]
#[derive(Clone, Copy, PartialEq)]
pub struct FarPtr {
    parts: Parts,
}

impl FarPtr {
    pub const fn new(segment: SegmentId, offset: SegmentOffset, double_far: bool) -> Self {
        Self {
            parts: Parts {
                upper: WireValue::<u32>::new(segment),
                lower: WireValue::<u32>::new(
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

impl Debug for FarPtr {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        f.debug_struct("FarPtr")
            .field("segment", &self.segment())
            .field("double_far", &self.double_far())
            .field("offset", &self.offset())
            .finish()
    }
}

impl From<FarPtr> for WirePtr {
    fn from(value: FarPtr) -> Self {
        WirePtr { far_ptr: value }
    }
}

#[repr(transparent)]
#[derive(Clone, Copy, PartialEq)]
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

impl Debug for CapabilityPtr {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        f.debug_struct("CapabilityPtr")
            .field("capability_index", &self.capability_index())
            .finish()
    }
}

impl From<CapabilityPtr> for WirePtr {
    fn from(value: CapabilityPtr) -> Self {
        WirePtr { cap_ptr: value }
    }
}

#[non_exhaustive]
#[derive(Debug)]
pub enum ExpectedRead {
    Struct,
    List,
    Far,
    Capability,
}

#[non_exhaustive]
#[derive(Debug)]
pub enum ActualRead {
    Null,
    Struct,
    List,
    Far,
    Other,
}

#[derive(Debug)]
pub(crate) struct FailedRead {
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
                Far => "far pointer",
                Capability => "capability",
            }
        };
        let actual = {
            use ActualRead::*;
            match self.actual {
                Null => "null",
                List => "list",
                Struct => "struct",
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

#[derive(Debug)]
pub(crate) struct IncompatibleUpgrade {
    pub from: PtrElementSize,
    pub to: PtrElementSize,
}

impl fmt::Display for IncompatibleUpgrade {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        use PtrElementSize::*;
        let element_str = |e: _| match e {
            Void => "void",
            Bit => "bit",
            Byte => "byte",
            TwoBytes => "two byte",
            FourBytes => "four byte",
            EightBytes => "eight byte",
            Pointer => "pointer",
            InlineComposite => "struct",
        };
        write!(
            f,
            "incompatible upgrade from {} element list to {} element list",
            element_str(self.from),
            element_str(self.to)
        )
    }
}

/// Describes the location of an object in a Cap'n Proto message. This can be used to create
/// pointers directly into a message without needing to navigate through the message tree.
pub struct Address {
    pub segment: SegmentId,
    pub offset: SegmentOffset,
}

impl Address {
    /// The root address of a message.
    pub const ROOT: Self = Self { segment: 0, offset: SegmentOffset::MIN };
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
pub(crate) struct ObjectReader<'a> {
    /// The segment reader associated with an object. If no segment is provided, we don't
    /// perform length checks on relative pointers since it's assumed to be always valid.
    segment: Option<SegmentReader<'a>>,
    /// The limiter associated with this pointer. If no limiter is provided, we don't perform
    /// read limiting.
    limiter: Option<&'a ReadLimiter>,
}

impl<'a> ObjectReader<'a> {
    #[inline]
    pub unsafe fn section_slice(&self, ptr: SegmentRef<'a>, len: SegmentOffset) -> &[Word] {
        if let Some(segment) = &self.segment {
            debug_assert!(segment.try_get_section(ptr.into(), len).is_some());
        }

        slice::from_raw_parts(ptr.as_ptr(), len.get() as usize)
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

    #[inline]
    pub fn try_read_object_from_end_of(
        self,
        ptr: SegmentPtr<'a>,
        offset: SignedSegmentOffset,
        len: ObjectLen,
    ) -> Result<(SegmentRef<'a>, Self)> {
        let start = ptr.signed_offset_from_end(offset);
        let new_ptr = if let Some(segment) = &self.segment {
            segment
                .try_get_section(start, len)
                .ok_or(ErrorKind::PointerOutOfBounds)?
        } else {
            // the pointer is unchecked, which is unsafe to make anyway, so whoever made the
            // pointer originally upholds safety here
            unsafe { start.as_ref_unchecked() }
        };

        if let Some(limiter) = self.limiter {
            if !limiter.try_read(len.get().into()) {
                return Err(ErrorKind::ReadLimitExceeded.into());
            }
        }

        Ok((new_ptr, self))
    }

    #[inline(always)]
    pub fn location_of(self, ptr: SegmentRef<'a>) -> Result<Content<'a, ObjectReader<'a>>> {
        if let Some(far) = ptr.as_wire_ptr().far_ptr() {
            let segment = far.segment();
            let double_far = far.double_far();
            let landing_pad_size = ObjectLen::new(if double_far { 2 } else { 1 }).unwrap();
            let (ptr, reader) = self.try_read_object_in(segment, far.offset(), landing_pad_size)?;

            if double_far {
                // The landing pad is another far pointer. The next pointer is a tag describing our
                // pointed-to object
                // SAFETY: We know from the above that this landing pad is two words, so this is
                //         safe
                let tag = unsafe { ptr.offset(1.into()).as_ref_unchecked() };

                let far_ptr = ptr.as_wire_ptr().try_far_ptr()?;
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
                    ptr,
                    location: Location::Far,
                })
            }
        } else {
            Ok(Content {
                accessor: self,
                ptr,
                location: Location::Near,
            })
        }
    }

    #[inline]
    pub fn try_amplified_read(&self, words: u64) -> bool {
        if let Some(limiter) = self.limiter {
            return limiter.try_read(words);
        }
        true
    }
}

/// Describes the content at a location.
pub(crate) struct Content<'a, A> {
    /// An accessor to get the content.
    pub accessor: A,
    /// The pointer that describes the content. This may not be in the same segment as
    /// the original pointer to this content.
    pub ptr: SegmentRef<'a>,
    /// Information associated with the content. This will describe how to properly read
    /// the content.
    pub location: Location,
}

/// Describes the location at some content.
pub(crate) enum Location {
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

fn target_size(reader: &ObjectReader, ptr: SegmentRef, nesting_limit: u32) -> Result<MessageSize> {
    let Content { 
        accessor: reader,
        ptr,
        location,
    } = reader.clone().location_of(ptr)?;

    let wire_ptr = ptr.as_wire_ptr();
    match wire_ptr.kind() {
        WireKind::Struct => {
            let struct_ptr = wire_ptr.struct_ptr().unwrap();
            let len = struct_ptr.len();
            let (struct_start, reader) = match location {
                Location::Near | Location::Far => {
                    reader.try_read_object_from_end_of(ptr.into(), struct_ptr.offset(), len)
                }
                Location::DoubleFar { segment, offset } => {
                    reader.try_read_object_in(segment, offset, len)
                }
            }?;

            let ptrs_start = unsafe {
                struct_start
                    .offset(struct_ptr.data_size().into())
                    .as_ref_unchecked()
            };

            let struct_size = MessageSize { words: len.get() as u64, caps: 0 };
            let ptrs_total_size = total_struct_ptrs_size(
                &reader, ptrs_start, struct_ptr.ptr_count(), nesting_limit)?;

            Ok(struct_size + ptrs_total_size)
        }
        WireKind::List => {
            todo!()
        }
        WireKind::Other => {
            let _ = wire_ptr.try_cap_ptr()?;
            return Ok(MessageSize { words: 0, caps: 1 })
        }
        WireKind::Far => {
            return Err(wire_ptr.fail_read(None))
        }
    }
}

/// Calculate the sizes of all pointer targets in this struct
fn total_struct_ptrs_size(reader: &ObjectReader, ptrs_start: SegmentRef, ptrs_len: u16, nesting_limit: u32) -> Result<MessageSize> {
    let mut total_size = MessageSize { words: 0, caps: 0 };

    let iter = unsafe { ptrs_start.iter_unchecked(ptrs_len.into()) };
    for ptr in iter.filter(|w| !w.as_ref().is_null()) {
        let nesting_limit = nesting_limit.checked_sub(1)
            .ok_or_else(|| Error::from(ErrorKind::NestingLimitExceeded))?;

        total_size += target_size(reader, ptr, nesting_limit)?;
    }

    Ok(total_size)
}

pub struct PtrReader<'a, T: Table = Empty> {
    ptr: SegmentRef<'a>,
    reader: ObjectReader<'a>,
    table: T::Reader,
    nesting_limit: u32,
}

impl<'a, T: Table> Clone for PtrReader<'a, T> {
    fn clone(&self) -> Self {
        Self {
            ptr: self.ptr,
            reader: self.reader.clone(),
            table: self.table.clone(),
            nesting_limit: self.nesting_limit,
        }
    }
}

impl<'a> PtrReader<'a, Empty> {
    pub const unsafe fn new_unchecked(ptr: NonNull<Word>) -> Self {
        Self {
            ptr: SegmentRef::new_unchecked(ptr),
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
            ptr: segment.start(),
            reader: ObjectReader {
                segment: Some(segment),
                limiter,
            },
            table: Empty,
            nesting_limit,
        }
    }

    pub const fn null() -> Self {
        unsafe { Self::new_unchecked(NonNull::new_unchecked(Word::null() as *const _ as *mut _)) }
    }
}

impl<'a, T: Table> PtrReader<'a, T> {
    #[inline(always)]
    fn locate(&self) -> Result<Content<'a, ObjectReader<'a>>> {
        self.reader.clone().location_of(self.ptr)
    }

    #[inline]
    fn ptr(&self) -> &WirePtr {
        self.ptr.as_wire_ptr()
    }

    #[inline]
    pub fn target_size(&self) -> Result<MessageSize> {
        if self.ptr().is_null() {
            return Ok(MessageSize { words: 0, caps: 0 })
        }

        let nesting_limit = self.nesting_limit.checked_sub(1)
            .ok_or_else(|| ErrorKind::NestingLimitExceeded)?;

        target_size(&self.reader, self.ptr, nesting_limit)
    }

    #[inline]
    pub fn ptr_type(&self) -> Result<PtrType> {
        if self.ptr().is_null() {
            return Ok(PtrType::Null);
        }

        let Content { ptr, .. } = self.locate()?;
        let ptr = ptr.as_wire_ptr();
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

        let struct_ptr = ptr.as_wire_ptr().try_struct_ptr()?;
        let len = struct_ptr.len();

        let (struct_start, reader) = match location {
            Location::Near | Location::Far => {
                reader.try_read_object_from_end_of(ptr.into(), struct_ptr.offset(), len)
            }
            Location::DoubleFar { segment, offset } => {
                reader.try_read_object_in(segment, offset, len)
            }
        }?;

        let data_start = struct_start.as_inner().cast();
        let ptrs_start = unsafe {
            struct_start
                .offset(struct_ptr.data_size().into())
                .as_ref_unchecked()
        };

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

        let wire_ptr = ptr.as_wire_ptr();
        let list_ptr = *wire_ptr.try_list_ptr()?;
        match list_ptr.element_size() {
            element_size @ PtrElementSize::InlineComposite => {
                let word_count = list_ptr.element_count().get();
                // add one for the tag pointer, because this could overflow the size of a segment,
                // we just assume on overflow the bounds check fails and is out of bounds.
                let len = ObjectLen::new(word_count + 1).ok_or(ErrorKind::PointerOutOfBounds)?;

                let (tag_ptr, reader) = match location {
                    Location::Near | Location::Far => {
                        reader.try_read_object_from_end_of(ptr.into(), list_ptr.offset(), len)
                    }
                    Location::DoubleFar { segment, offset } => {
                        reader.try_read_object_in(segment, offset, len)
                    }
                }?;

                let tag = tag_ptr
                    .as_wire_ptr()
                    .struct_ptr()
                    .ok_or(ErrorKind::UnsupportedInlineCompositeElementTag)?;

                // move past the tag pointer to get the start of the list
                let first = unsafe { tag_ptr.offset(1.into()).as_ref_unchecked() };

                let element_count = tag.inline_composite_element_count();
                let data_size = tag.data_size();
                let ptr_count = tag.ptr_count();
                let words_per_element = tag.size().total();

                if element_count.get() as u64 * words_per_element as u64 > word_count.into() {
                    // Make sure the tag reports a struct size that matches the reported word count
                    return Err(ErrorKind::InlineCompositeOverrun.into());
                }

                if words_per_element == 0 {
                    // watch out for zero-sized structs, which can claim to be arbitrarily
                    // large without having sent actual data.
                    if !reader.try_amplified_read(element_count.get() as u64) {
                        return Err(ErrorKind::ReadLimitExceeded.into());
                    }
                }

                // Check whether the size is compatible.
                use PtrElementSize::*;
                match expected_element_size {
                    None | Some(Void | InlineComposite) => {}
                    Some(Bit) => return Err(Error::fail_upgrade(InlineComposite, Bit)),
                    Some(Pointer) => {
                        if ptr_count == 0 {
                            return Err(Error::fail_upgrade(InlineComposite, Pointer));
                        }
                    }
                    Some(expected @ (Byte | TwoBytes | FourBytes | EightBytes)) => {
                        if data_size == 0 {
                            return Err(Error::fail_upgrade(expected, Pointer));
                        }
                    }
                }

                Ok(ListReader {
                    ptr: first,
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
                        (actual, expected) => return Err(Error::fail_upgrade(actual, expected)),
                    }
                }

                let element_count = list_ptr.element_count();
                if element_size == PtrElementSize::Void {
                    if reader.try_amplified_read(element_count.get() as u64) {
                        return Err(ErrorKind::ReadLimitExceeded.into());
                    }
                }

                // This is a primitive or pointer list, but all (except `List(Bool)`) such lists
                // can also be interpreted as struct lists. We need to compute the data size and
                // pointer count for such structs. For `List(Bool)`, we instead interpret all read
                // structs as zero-sized.
                let step = element_size.bits() / 8;
                let word_count = Word::round_up_byte_count(element_count.get() * step);
                let len = ObjectLen::new(word_count).unwrap();
                let (ptr, reader) = match location {
                    Location::Near | Location::Far => {
                        reader.try_read_object_from_end_of(ptr.into(), list_ptr.offset(), len)
                    }
                    Location::DoubleFar { segment, offset } => {
                        reader.try_read_object_in(segment, offset, len)
                    }
                }?;

                Ok(ListReader {
                    ptr,
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
    pub fn to_text(&self) -> Result<Option<TextReader<'a>>> {
        if self.is_null() {
            return Ok(None);
        }

        self.to_text_inner().map(Some)
    }

    fn to_text_inner(&self) -> Result<TextReader<'a>> {
        let Content {
            accessor: reader,
            ptr,
            location,
        } = self.locate()?;

        let list_ptr = ptr.as_wire_ptr().try_list_ptr()?;
        let element_size = list_ptr.element_size();
        if element_size != PtrElementSize::Byte {
            return Err(Error::fail_upgrade(element_size, PtrElementSize::Byte));
        }

        let element_count = list_ptr.element_count();
        let Some(text_byte_count) = TextByteCount::new(element_count.into()) else {
            return Err(ErrorKind::TextNotNulTerminated.into());
        };
        let len = ObjectLen::new(Word::round_up_byte_count(element_count.into())).unwrap();
        let (ptr, _) = match location {
            Location::Near | Location::Far => {
                reader.try_read_object_from_end_of(ptr.into(), list_ptr.offset(), len)
            }
            Location::DoubleFar { segment, offset } => {
                reader.try_read_object_in(segment, offset, len)
            }
        }?;

        let data = ptr.as_inner().cast::<u8>();
        let last = unsafe { *data.as_ptr().add((u32::from(element_count) - 1) as usize) };
        if last != 0 {
            return Err(ErrorKind::TextNotNulTerminated.into());
        }

        Ok(unsafe { TextReader::new_unchecked(data, text_byte_count) })
    }

    #[inline]
    pub fn to_data(&self) -> Result<Option<DataReader<'a>>> {
        if self.is_null() {
            return Ok(None);
        }

        self.to_data_inner().map(Some)
    }

    fn to_data_inner(&self) -> Result<DataReader<'a>> {
        let Content {
            accessor: reader,
            ptr,
            location,
        } = self.locate()?;

        let list_ptr = ptr.as_wire_ptr().try_list_ptr()?;
        let element_size = list_ptr.element_size();
        if element_size != PtrElementSize::Byte {
            return Err(Error::fail_upgrade(element_size, PtrElementSize::Byte));
        }

        let element_count = list_ptr.element_count();
        let len = ObjectLen::new(Word::round_up_byte_count(element_count.into())).unwrap();
        let (ptr, _) = match location {
            Location::Near | Location::Far => {
                reader.try_read_object_from_end_of(ptr.into(), list_ptr.offset(), len)
            }
            Location::DoubleFar { segment, offset } => {
                reader.try_read_object_in(segment, offset, len)
            }
        }?;

        Ok(unsafe { DataReader::new_unchecked(ptr.as_inner().cast(), element_count) })
    }
}

impl<'a, T: Table> Capable for PtrReader<'a, T> {
    type Table = T;

    type Imbued = T::Reader;
    type ImbuedWith<T2: Table> = PtrReader<'a, T2>;

    #[inline]
    fn imbued(&self) -> &Self::Imbued { &self.table }

    #[inline]
    fn imbue_release<T2: Table>(self, new_table: T2::Reader) -> (Self::ImbuedWith<T2>, T::Reader) {
        let old_table = self.table;
        let ptr = PtrReader {
            ptr: self.ptr,
            reader: self.reader,
            table: new_table,
            nesting_limit: self.nesting_limit,
        };
        (ptr, old_table)
    }

    #[inline]
    fn imbue_release_into<U: Capable>(&self, other: U) -> (U::ImbuedWith<T>, U::Imbued)
    where
        U: Capable,
        U::ImbuedWith<Self::Table>: Capable<Imbued = Self::Imbued>,
    {
        other.imbue_release::<T>(self.table.clone())
    }
}

impl<T: CapTable> PtrReader<'_, T> {
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

impl<T: CapTable + BreakableCapSystem> PtrReader<'_, T> {
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

/// A forked pointer reader.
///
/// This reader effectively allows the user to control limits partway through a message without
/// having to rewind variables such as the read limiter, by instead forking off new readers
/// that don't effect old ones.
pub struct ForkedPtrReader<'a, T: Table = Empty> {
    ptr: SegmentRef<'a>,
    segment: Option<SegmentReader<'a>>,
    read_limiter: Option<ReadLimiter>,
    table: T::Reader,
    nesting_limit: u32,
}

impl<'a, T: Table> ForkedPtrReader<'a, T> {
    /// Gets the current limit of this reader in words, or none if no reader is present on the
    /// reader.
    pub fn read_limit(&self) -> Option<u64> {
        self.read_limiter.as_ref().map(|l| l.current_limit())
    }

    pub fn set_read_limit(&mut self, new_limit: u64) {
        self.read_limiter = Some(ReadLimiter::new(new_limit));
    }

    pub fn nesting_limit(&self) -> u32 {
        self.nesting_limit
    }

    pub fn set_nesting_limit(&mut self, new_limit: u32) {
        self.nesting_limit = new_limit
    }

    /// Returns a new pointer reader with the limits configured in this forked reader.
    pub fn read(&self) -> PtrReader<T> {
        PtrReader {
            ptr: self.ptr,
            reader: ObjectReader {
                segment: self.segment.clone(),
                limiter: self.read_limiter.as_ref(),
            },
            table: self.table.clone(),
            nesting_limit: self.nesting_limit,
        }
    }
}

pub struct StructReader<'a, T: Table = Empty> {
    /// The start of the data section of the struct. This is a byte pointer in case of struct
    /// promotion
    data_start: NonNull<u8>,
    /// The start of the pointer section of the struct.
    ptrs_start: SegmentRef<'a>,
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
            ptrs_start: unsafe { SegmentRef::dangling() },
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
        let ptrs_offset = SegmentOffset::new(size.data as u32).unwrap();

        Self {
            data_start: ptr.cast(),
            ptrs_start: SegmentRef::new_unchecked(ptr)
                .offset(ptrs_offset)
                .as_ref_unchecked(),
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
    fn size(&self, canonical: bool) -> (StructSize, &[u8]) {
        let mut data_section = self.data_section();
        let mut ptrs_len = self.ptrs_len;

        if canonical {
            // truncate data section
            while let Some((0, remainder)) = data_section.split_last() {
                data_section = remainder;
            }

            // truncate pointers
            let mut ptrs_section = self.ptr_section_slice();
            while let Some((&Word::NULL, remainder)) = ptrs_section.split_last() {
                ptrs_section = remainder;
            }
            ptrs_len = u16::try_from(ptrs_section.len()).unwrap();
        }

        (
            StructSize {
                data: Word::round_up_byte_count(data_section.len() as u32) as u16,
                ptrs: ptrs_len,
            },
            data_section,
        )
    }

    #[inline]
    pub fn total_size(&self) -> Result<MessageSize> {
        let struct_len = Word::round_up_byte_count(self.data_len) + self.ptrs_len as u32;
        let struct_size = MessageSize { words: struct_len as u64, caps: 0 };

        let ptrs_targets_size = total_struct_ptrs_size(
            &self.reader, self.ptrs_start, self.ptrs_len, self.nesting_limit)?;

        Ok(struct_size + ptrs_targets_size)
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

    #[inline]
    fn ptr_section_slice(&self) -> &[Word] {
        unsafe {
            self.reader
                .section_slice(self.ptrs_start, self.ptrs_len.into())
        }
    }

    /// Returns the pointer section of this struct as a list reader
    #[inline]
    pub fn ptr_section(&self) -> ListReader<'a, T> {
        ListReader {
            reader: self.reader.clone(),
            ptr: self.ptrs_start,
            table: self.table.clone(),
            element_count: self.ptrs_len.into(),
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
                ptr: unsafe { self.ptrs_start.offset(index.into()).as_ref_unchecked() },
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

impl<'a, T: Table> Clone for StructReader<'a, T> {
    fn clone(&self) -> Self {
        Self {
            data_start: self.data_start,
            ptrs_start: self.ptrs_start,
            reader: self.reader.clone(),
            table: self.table.clone(),
            nesting_limit: self.nesting_limit,
            data_len: self.data_len,
            ptrs_len: self.ptrs_len,
        }
    }
}

impl<'a, T: Table> Capable for StructReader<'a, T> {
    type Table = T;

    type Imbued = T::Reader;
    type ImbuedWith<T2: Table> = StructReader<'a, T2>;

    #[inline]
    fn imbued(&self) -> &Self::Imbued { &self.table }

    #[inline]
    fn imbue_release<T2: Table>(self, new_table: T2::Reader) -> (Self::ImbuedWith<T2>, T::Reader) {
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

    #[inline]
    fn imbue_release_into<U: Capable>(&self, other: U) -> (U::ImbuedWith<T>, U::Imbued)
    where
        U: Capable,
        U::ImbuedWith<Self::Table>: Capable<Imbued = Self::Imbued>,
    {
        other.imbue_release::<T>(self.table.clone())
    }
}

pub struct ListReader<'a, T: Table = Empty> {
    ptr: SegmentRef<'a>,
    reader: ObjectReader<'a>,
    /// A reader for the cap table associated with this list
    table: T::Reader,
    /// The length of this list
    element_count: ElementCount,
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

impl<T: Table> Clone for ListReader<'_, T> {
    fn clone(&self) -> Self {
        Self {
            ptr: self.ptr.clone(),
            reader: self.reader.clone(),
            table: self.table.clone(),
            element_count: self.element_count.clone(),
            nesting_limit: self.nesting_limit.clone(),
            step: self.step.clone(),
            struct_data_len: self.struct_data_len.clone(),
            struct_ptrs_len: self.struct_ptrs_len.clone(),
            element_size: self.element_size.clone(),
        }
    }
}

impl ListReader<'_, Empty> {
    /// Creates an empty list of the specific element size.
    pub const fn empty(element_size: ElementSize) -> Self {
        unsafe { Self::new_unchecked(NonNull::dangling(), ElementCount::MIN, element_size) }
    }

    pub const unsafe fn new_unchecked(
        ptr: NonNull<Word>,
        element_count: ElementCount,
        element_size: ElementSize,
    ) -> Self {
        let step = element_size.bits() / 8;
        let (struct_data_len, struct_ptrs_len) = match element_size {
            ElementSize::Pointer => (0, 1),
            ElementSize::InlineComposite(StructSize { data, ptrs }) => ((data as u32) * 8, ptrs),
            primitive => (primitive.bits() / 8, 0),
        };

        ListReader {
            ptr: SegmentRef::new_unchecked(ptr),
            reader: ObjectReader {
                segment: None,
                limiter: None,
            },
            table: Empty,
            element_count,
            nesting_limit: u32::MAX,
            step,
            struct_data_len,
            struct_ptrs_len,
            element_size: element_size.as_ptr_size(),
        }
    }
}

impl<'a, T: Table> ListReader<'a, T> {
    fn as_slice(&self) -> &[Word] {
        let elements = u64::from(self.element_count.get());
        let size_bits = u64::from(self.element_size.data_bits());
        let offset = Word::round_up_bit_count(elements * size_bits);

        unsafe {
            self.reader
                .section_slice(self.ptr, SegmentOffset::new(offset).unwrap())
        }
    }

    #[inline]
    pub fn len(&self) -> ElementCount {
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

    #[inline]
    fn ptr(&self) -> *const u8 {
        self.ptr.as_ptr().cast()
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

        debug_assert!(index < self.element_count.get(), "index out of bounds");
        debug_assert!({
            match self.element_size {
                PtrElementSize::Void | PtrElementSize::Pointer => false,
                PtrElementSize::Bit => TypeId::of::<D>() == TypeId::of::<bool>(),
                PtrElementSize::InlineComposite => self.struct_data_len != 0,
                size => PtrElementSize::from(D::ELEMENT_SIZE).data_bytes() <= size.data_bytes(),
            }
        }, "attempted to access invalid data for this list; list element size: {:?}, data type: {}",
        self.element_size, core::any::type_name::<D>());

        if TypeId::of::<D>() == TypeId::of::<bool>() {
            D::read_unchecked(self.ptr(), index as usize, D::default())
        } else {
            let ptr = self.ptr().add(self.index_to_offset(index));
            D::read_unchecked(ptr, 0, D::default())
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
        debug_assert!(index < self.element_count.get(), "index out of bounds");
        debug_assert!(
            (self.element_size == PtrElementSize::Pointer)
                || (self.element_size == PtrElementSize::InlineComposite
                    && self.struct_ptrs_len != 0),
            "attempted to read pointer from a list of something else"
        );

        let offset = ((self.step * index) + self.struct_data_len) as usize;
        let ptr = self.ptr().add(offset).cast_mut().cast();

        PtrReader {
            ptr: SegmentRef::new_unchecked(NonNull::new_unchecked(ptr)),
            reader: self.reader.clone(),
            table: self.table.clone(),
            nesting_limit: self.nesting_limit,
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
        debug_assert!(index < self.element_count.get(), "index out of bounds");
        debug_assert!(
            self.element_size != PtrElementSize::Bit,
            "attempted to read struct from bit list"
        );

        let struct_start = self.ptr().add(self.index_to_offset(index));
        let struct_data = struct_start;
        let struct_ptrs = struct_start
            .add(self.struct_data_len as usize)
            .cast::<Word>();

        StructReader {
            data_start: NonNull::new_unchecked(struct_data.cast_mut()),
            ptrs_start: SegmentRef::new_unchecked(NonNull::new_unchecked(struct_ptrs.cast_mut())),
            reader: self.reader.clone(),
            table: self.table.clone(),
            nesting_limit: self.nesting_limit.saturating_sub(1),
            data_len: self.struct_data_len,
            ptrs_len: self.struct_ptrs_len,
        }
    }
}

impl<'a, T: Table> Capable for ListReader<'a, T> {
    type Table = T;

    type Imbued = T::Reader;
    type ImbuedWith<T2: Table> = ListReader<'a, T2>;

    #[inline]
    fn imbued(&self) -> &Self::Imbued { &self.table }

    #[inline]
    fn imbue_release<T2: Table>(self, new_table: T2::Reader) -> (Self::ImbuedWith<T2>, T::Reader) {
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

    #[inline]
    fn imbue_release_into<U: Capable>(&self, other: U) -> (U::ImbuedWith<T>, U::Imbued)
    where
        U: Capable,
        U::ImbuedWith<Self::Table>: Capable<Imbued = Self::Imbued>,
    {
        other.imbue_release::<T>(self.table.clone())
    }
}

/// A helper struct for building objects in a message
#[derive(Clone, Debug)]
pub(crate) struct ObjectBuilder<'a> {
    segment: SegmentBuilder<'a>,
}

impl<'a> ObjectBuilder<'a> {
    #[inline]
    pub fn as_reader<'b>(&'b self) -> ObjectReader<'b> {
        ObjectReader {
            segment: Some(self.segment.as_reader()),
            limiter: None,
        }
    }

    #[inline]
    pub fn is_same_message(&self, other: &ObjectBuilder) -> bool {
        self.segment.arena() == other.segment.arena()
    }

    #[inline]
    pub fn id(&self) -> SegmentId {
        self.segment.id()
    }

    #[inline]
    pub fn locate(self, ptr: SegmentRef<'a>) -> Content<'a, ObjectBuilder<'a>> {
        if let Some(far) = ptr.as_wire_ptr().far_ptr() {
            let segment = far.segment();
            let double_far = far.double_far();
            let (ptr, reader) = self
                .build_object_in(segment, far.offset())
                .expect("far pointer cannot point to a read-only segment");

            if double_far {
                let tag = unsafe { ptr.offset(1.into()).as_ref_unchecked() };
                let far_ptr = ptr
                    .as_wire_ptr()
                    .try_far_ptr()
                    .expect("malformed double far in builder");
                Content {
                    accessor: reader,
                    ptr: tag,
                    location: Location::DoubleFar {
                        segment: far_ptr.segment(),
                        offset: far_ptr.offset(),
                    },
                }
            } else {
                Content {
                    accessor: reader,
                    ptr,
                    location: Location::Far,
                }
            }
        } else {
            Content {
                accessor: self,
                ptr,
                location: Location::Near,
            }
        }
    }

    /// Clear the given pointer. If it's a far pointer, clear the landing pads. This function ignores read errors.
    #[inline]
    pub fn clear_ptrs(&self, ptr: SegmentRef<'a>) {
        if let Some(far) = ptr.as_wire_ptr().far_ptr() {
            let result = self.build_object_in(far.segment(), far.offset());
            let Some((pad_ptr, builder)) = result else { return };

            builder.set_ptr(pad_ptr, WirePtr::NULL);
            if far.double_far() {
                let content_ptr = unsafe { pad_ptr.offset(1.into()).as_ref_unchecked() };
                builder.set_ptr(content_ptr, WirePtr::NULL);
            }
        }
        self.set_ptr(ptr, WirePtr::NULL);
    }

    #[inline]
    pub fn clear_section(&self, start: SegmentRef<'a>, offset: SegmentOffset) {
        debug_assert!(self.segment.contains_section(start, offset));

        unsafe { ptr::write_bytes(start.as_ptr_mut(), 0, offset.get() as usize) }
    }

    #[inline]
    pub fn set_ptr(&self, ptr: SegmentRef<'a>, value: impl Into<WirePtr>) {
        debug_assert!(
            self.segment.contains(ptr.into()),
            "segment does not contain ptr: {:?}; segment: {:?}",
            ptr.as_ptr(),
            &self.segment
        );

        let value = value.into();
        unsafe { *ptr.as_ptr_mut() = Word::from(value) }
    }

    #[inline]
    pub fn section_slice_mut(
        &mut self,
        start: SegmentRef<'a>,
        offset: SegmentOffset,
    ) -> &'a mut [Word] {
        debug_assert!(self.segment.contains_section(start, offset));

        unsafe { slice::from_raw_parts_mut(start.as_ptr_mut(), offset.get() as usize) }
    }

    /// Returns an object pointer and builder based on the given wire pointer and offset
    #[inline]
    pub fn build_object_from_end_of(
        &self,
        ptr: SegmentRef<'a>,
        offset: SignedSegmentOffset,
    ) -> (SegmentRef<'a>, Self) {
        let start = ptr.signed_offset_from_end(offset);
        debug_assert!(
            self.segment.contains(start),
            "builder contains invalid pointer"
        );
        (unsafe { start.as_ref_unchecked() }, self.clone())
    }

    /// Attempts to get an object builder for an object in the specified segment, or None if the
    /// segment is read-only.
    #[inline]
    pub fn build_object_in(
        &self,
        segment: SegmentId,
        offset: SegmentOffset,
    ) -> Option<(SegmentRef<'a>, Self)> {
        let segment = self.segment.arena().segment_mut(segment)?;
        let ptr = segment.at_offset(offset);
        Some((ptr, Self { segment }))
    }

    #[inline]
    pub fn alloc_in_arena(&self, size: AllocLen) -> (SegmentRef<'a>, Self) {
        let (r, s) = self.segment.arena().alloc(size);
        (r, Self { segment: s })
    }

    /// Allocates a struct in the message, then configures the given pointer to point to it.
    #[inline]
    pub fn alloc_struct(&self, ptr: SegmentRef<'a>, size: StructSize) -> (SegmentRef<'a>, Self) {
        let Some(len) = AllocLen::new(size.total()) else {
            self.set_ptr(ptr, StructPtr::EMPTY);
            return (ptr, self.clone());
        };

        let (start, segment) = if let Some(start) = self.segment.alloc_in_segment(len) {
            // the struct can be allocated in this segment, nice
            let offset = self.segment.offset_from_end_of(ptr.into(), start.into());
            self.set_ptr(ptr, StructPtr::new(offset, size));

            (start, self.clone())
        } else {
            // the struct can't be allocated in this segment, so we need to allocate for
            // a struct + landing pad
            // this unwrap is ok since we know that u16::MAX + u16::MAX + 1 can never be
            // larger than a segment
            let len_with_pad = AllocLen::new(size.total() + 1).unwrap();
            let (pad, object_segment) = self.alloc_in_arena(len_with_pad);
            let start = unsafe { pad.offset(1u16.into()).as_ref_unchecked() };

            // our pad is the actual pointer to the content
            let offset_to_pad = object_segment.segment.offset_from_start(pad.into());
            self.set_ptr(ptr, FarPtr::new(object_segment.id(), offset_to_pad, false));
            object_segment.set_ptr(pad, StructPtr::new(0i16.into(), size));

            (start, object_segment)
        };

        (start, segment)
    }

    /// Allocates a list in the message, then configures the given pointer to point to it.
    #[inline]
    pub fn alloc_list(
        &self,
        ptr: SegmentRef<'a>,
        element_size: ElementSize,
        element_count: ElementCount,
    ) -> Option<(SegmentRef<'a>, Self)> {
        let is_struct_list = matches!(element_size, ElementSize::InlineComposite(_));

        let element_bits = element_size.bits() as u64;
        let total_words = Word::round_up_bit_count((element_count.get() as u64) * element_bits);

        let tag_size = if is_struct_list { 1 } else { 0 };
        let total_size = total_words + tag_size;
        if total_size == 0 {
            let end = self.segment.end();
            let offset_to_end = self.segment.offset_from_end_of(ptr.into(), end);
            self.set_ptr(
                ptr,
                ListPtr::new(offset_to_end, element_size.into(), element_count),
            );
            // since the list has no size, any pointer is valid here, even one beyond the end of the segment
            return Some((unsafe { end.as_ref_unchecked() }, self.clone()));
        }

        let list_ptr_element_count = if is_struct_list {
            ElementCount::new(total_words)?
        } else {
            element_count
        };

        let len = AllocLen::new(total_size)?;
        let (start, segment) = if let Some(alloc) = self.segment.alloc_in_segment(len) {
            // we were able to allocate in our current segment, nice

            let offset = self.segment.offset_from_end_of(ptr.into(), alloc.into());
            self.set_ptr(
                ptr,
                ListPtr::new(offset, element_size.into(), list_ptr_element_count),
            );

            (alloc, self.clone())
        } else if let Some(len_with_landing_pad) = AllocLen::new(total_size + 1) {
            // we couldn't allocate in this segment, but we can probably allocate a new list
            // somewhere else with a far landing pad

            let (pad, segment) = self.alloc_in_arena(len_with_landing_pad);
            let start = unsafe { pad.offset(1u16.into()).as_ref_unchecked() };
            let offset_to_pad = segment.segment.offset_from_start(pad.into());

            self.set_ptr(ptr, FarPtr::new(segment.id(), offset_to_pad, false));
            segment.set_ptr(
                pad,
                ListPtr::new(0i16.into(), element_size.into(), list_ptr_element_count),
            );

            (start, segment)
        } else {
            // ok the list is just too big for even one more word to be added to its alloc size
            // so we're going to have to make a separate double far landing pad

            self.alloc_segment_of_list(ptr, element_size, element_count)
        };

        let start = if let ElementSize::InlineComposite(size) = element_size {
            segment.set_ptr(
                start,
                StructPtr::new_inline_composite_tag(element_count, size),
            );

            unsafe { start.offset(1u16.into()).as_ref_unchecked() }
        } else {
            start
        };

        Some((start, segment))
    }

    /// Allocate a list where the total size in words is equal to that of a segment.
    fn alloc_segment_of_list(
        &self,
        ptr: SegmentRef<'a>,
        element_size: ElementSize,
        element_count: ElementCount,
    ) -> (SegmentRef<'a>, ObjectBuilder<'a>) {
        // try to allocate the landing pad _first_ since it likely will fit in either this
        // segment or the last segment in the message.
        // if the allocations were flipped, we'd probably allocate the pad in a completely new
        // segment, which is just a waste of space.

        let (pad, pad_segment) = self.segment.alloc(AllocLen::new(2).unwrap());
        let tag = unsafe { pad.offset(1u16.into()).as_ref_unchecked() };

        let (start, list_segment) = self.alloc_in_arena(AllocLen::MAX);

        let offset_to_pad = pad_segment.offset_from_start(pad.into());
        self.set_ptr(ptr, FarPtr::new(pad_segment.id(), offset_to_pad, true));

        // I'm pretty sure if we can't add even one more word to a list allocation then the list
        // takes up the whole segment, which means this has to be 0, but I'm not taking chances.
        // Change it if you think it actually really matters.
        let pad_builder = ObjectBuilder {
            segment: pad_segment,
        };
        let offset_to_list = list_segment.segment.offset_from_start(start.into());
        pad_builder.set_ptr(pad, FarPtr::new(list_segment.id(), offset_to_list, false));
        pad_builder.set_ptr(
            tag,
            ListPtr::new(0i16.into(), element_size.into(), element_count),
        );

        (start, list_segment)
    }
}

pub enum CopySize<T> {
    /// Size is determined by the input value's size. If canonical is true, we use the canonical
    /// size.
    FromValue { canonical: bool },
    /// The size is explicitly set to at-least this value. We use this when setting a pointer to
    /// a copy of a value so that we can build it.
    Minimum(T),
}

/// A result for consuming PtrBuilder functions.
pub type BuildResult<T, U, E = Option<Error>> = Result<T, (E, U)>;

pub struct PtrBuilder<'a, T: Table = Empty> {
    builder: ObjectBuilder<'a>,
    ptr: SegmentRef<'a>,
    table: T::Builder,
}

impl<'a, T> Debug for PtrBuilder<'a, T>
where
    T: Table,
    T::Builder: Debug,
{
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        f.debug_struct("PtrBuilder")
            .field("builder", &self.builder)
            .field("ptr", &self.ptr)
            .field("table", &self.table as &dyn Debug)
            .finish()
    }
}

impl<'a> PtrBuilder<'a, Empty> {
    pub(crate) fn root(segment: SegmentBuilder<'a>) -> Self {
        let ptr = segment.start();
        Self {
            builder: ObjectBuilder { segment },
            ptr,
            table: Empty,
        }
    }
}

impl<'a, T: Table> PtrBuilder<'a, T> {
    /// Locate the content this builder points to.
    #[inline(always)]
    fn locate(&self) -> Content<'a, ObjectBuilder<'a>> {
        self.builder.clone().locate(self.ptr)
    }

    #[inline]
    pub fn is_null(&self) -> bool {
        *self.ptr.as_wire_ptr() == WirePtr::NULL
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
    #[inline]
    pub fn by_ref<'b>(&'b mut self) -> PtrBuilder<'b, T> {
        PtrBuilder {
            builder: self.builder.clone(),
            ptr: self.ptr,
            table: self.table.clone(),
        }
    }

    #[inline]
    pub fn ptr_type(&self) -> PtrType {
        if self.is_null() {
            return PtrType::Null;
        }

        let Content { ptr, .. } = self.locate();
        let ptr = ptr.as_wire_ptr();
        if ptr.is_struct() {
            PtrType::Struct
        } else if ptr.is_list() {
            PtrType::List
        } else if ptr.is_capability() {
            PtrType::Capability
        } else {
            unreachable!("{}", ptr.fail_read(None))
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
    ) -> BuildResult<StructBuilder<'a, T>, Self, Error> {
        let table = self.table.clone();
        let Content {
            accessor: builder,
            ptr,
            location,
        } = self.locate();
        let struct_ptr = match ptr.as_wire_ptr().try_struct_ptr() {
            Ok(ptr) => *ptr,
            Err(err) => return Err((err, self)),
        };

        let (existing_data_start, builder) = match location {
            Location::Near | Location::Far => {
                builder.build_object_from_end_of(ptr, struct_ptr.offset())
            }
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

        let existing_ptrs_start = unsafe {
            existing_data_start
                .offset(existing_data_size.into())
                .as_ref_unchecked()
        };

        let mut existing_builder = StructBuilder::<T> {
            builder,
            table,
            data_start: existing_data_start.as_inner().cast(),
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
        self.builder.clear_ptrs(self.ptr);
        let mut new_builder = self.init_struct_zeroed(expected);

        Self::transfer_struct(existing, &mut new_builder);

        existing.clear();
        new_builder
    }

    fn transfer_struct(old: &mut StructBuilder<T>, new: &mut StructBuilder<T>) {
        let old_data = old.data_section();
        new.data_section_mut()[..old_data.len()].copy_from_slice(old_data);

        let ptrs_to_copy = SegmentOffset::from(old.ptrs_len);
        let ptr_pairs = unsafe {
            let old_ptrs = old.ptrs_start.iter_unchecked(ptrs_to_copy);
            let new_ptrs = new.ptrs_start.iter_unchecked(ptrs_to_copy);
            old_ptrs.zip(new_ptrs)
        };

        for (old_ptr, new_ptr) in ptr_pairs {
            Self::transfer(old_ptr, &old.builder, new_ptr, &new.builder);
        }
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
    /// the provided reader isn't unchecked.
    ///
    /// If any far or other pointers are in the provided struct, this silently writes null instead.
    pub fn init_struct(
        mut self,
        size: StructSize,
    ) -> StructBuilder<'a, T> {
        self.clear();
        self.init_struct_zeroed(size)
    }

    /// Initializes the pointer as a struct with the specified size. This does not apply a default
    /// value to the struct.
    #[inline]
    fn init_struct_zeroed(&mut self, size: StructSize) -> StructBuilder<'a, T> {
        let (struct_start, builder) = self.builder.alloc_struct(self.ptr, size);
        let data_start = struct_start.as_inner().cast();
        let ptrs_start = unsafe { struct_start.offset(size.data.into()).as_ref_unchecked() };

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
    ) -> StructBuilder<'a, T> {
        match self.to_struct_mut(Some(size)) {
            Ok(builder) => builder,
            Err((_, this)) => this.init_struct(size),
        }
    }

    /// Sets the pointer to a struct copied from the specified reader.
    /// 
    /// This function allows you to set a pointer to a copy of the given struct data and handle
    /// struct promotion or writing the value canonically at the same time.
    /// 
    /// When calling to initialize a builder with a set size, pass `CopySize::Minimum` with the
    /// size of the struct type the copy is being made for. This will handle properly promoting
    /// the struct's size to be at least as large as the expected builder.
    /// 
    /// When calling to simply copy a value directly with or without canonicalization, use
    /// `CopySize::FromValue`.
    #[inline]
    pub fn try_set_struct<E, F>(
        mut self,
        value: &StructReader<impl InsertableInto<T>>,
        size: CopySize<StructSize>,
        mut err_handler: F,
    ) -> BuildResult<StructBuilder<'a, T>, Self, E>
    where
        F: FnMut(Error) -> ControlFlow<E, WriteNull>,
    {
        let (size, data_section, canonical) = match size {
            CopySize::Minimum(min_size) => {
                let (size, data_section) = value.size(false);
                (size.max(min_size), data_section, false)
            }
            CopySize::FromValue { canonical } => {
                let (size, data_section) = value.size(canonical);
                (size, data_section, canonical)
            }
        };

        self.clear();
        let mut builder = self.init_struct_zeroed(size);

        let builder_data = builder.data_section_mut();
        builder_data[..data_section.len()].copy_from_slice(data_section);

        let mut builder_ptrs = builder.ptr_section_mut();
        let ptr_section = value.ptr_section();
        for i in 0..(size.ptrs as u32) {
            unsafe {
                let value_ptr = ptr_section.ptr_unchecked(i);
                let mut builder_ptr = builder_ptrs.ptr_mut_unchecked(i);
                if let Err(e) =
                    builder_ptr.try_copy_from(&value_ptr, canonical, err_handler.borrow_mut())
                {
                    return Err((e, self));
                }
            }
        }

        Ok(builder)
    }

    pub fn set_struct(
        self,
        value: &StructReader<impl InsertableInto<T>>,
        size: CopySize<StructSize>,
    ) -> StructBuilder<'a, T> {
        self.try_set_struct::<Infallible, _>(value, size, |_| ControlFlow::Continue(WriteNull))
            .map_err(|(i, _)| i)
            .unwrap()
    }

    #[inline]
    pub fn to_list(
        &self,
        expected_element_size: Option<PtrElementSize>,
    ) -> Result<Option<ListReader<T>>> {
        self.as_reader().to_list(expected_element_size)
    }

    #[inline]
    pub fn to_list_mut(
        self,
        expected_element_size: Option<ElementSize>,
    ) -> BuildResult<ListBuilder<'a, T>, Self> {
        if self.is_null() {
            return Err((None, self));
        }

        self.to_list_mut_inner(expected_element_size)
            .map_err(|(err, s)| (Some(err), s))
    }

    fn to_list_mut_inner(
        self,
        expected_element_size: Option<ElementSize>,
    ) -> BuildResult<ListBuilder<'a, T>, Self, Error> {
        let Content {
            accessor: builder,
            ptr,
            location,
        } = self.locate();

        let wire_ptr = ptr.as_wire_ptr();
        let list_ptr = match wire_ptr.try_list_ptr() {
            Ok(ptr) => *ptr,
            Err(err) => return Err((err, self)),
        };

        let (existing_data_start, builder) = match location {
            Location::Near | Location::Far => {
                builder.build_object_from_end_of(ptr, list_ptr.offset())
            }
            Location::DoubleFar { segment, offset } => {
                match builder.build_object_in(segment, offset) {
                    Some(b) => b,
                    None => return Err((ErrorKind::WritingNotAllowed.into(), self)),
                }
            }
        };

        let existing_element_size = list_ptr.element_size();
        let mut existing_list = match existing_element_size {
            PtrElementSize::InlineComposite => {
                let tag = *existing_data_start
                    .as_wire_ptr()
                    .struct_ptr()
                    .expect("inline composite list with non-struct elements is not supported");

                let list_start = unsafe { existing_data_start.offset(1.into()).as_ref_unchecked() };
                let size = tag.size();
                let step = size.total() * (Word::BYTES as u32);

                ListBuilder::<T> {
                    builder,
                    ptr: list_start,
                    table: self.table.clone(),
                    element_count: tag.inline_composite_element_count(),
                    step,
                    struct_data_len: (size.data as u32) * (Word::BYTES as u32),
                    struct_ptrs_len: size.ptrs,
                    element_size: existing_element_size,
                }
            }
            PtrElementSize::Pointer => ListBuilder::<T> {
                builder,
                ptr: existing_data_start,
                table: self.table.clone(),
                element_count: list_ptr.element_count(),
                step: 8,
                struct_data_len: 0,
                struct_ptrs_len: 1,
                element_size: existing_element_size,
            },
            _ => ListBuilder::<T> {
                builder,
                ptr: existing_data_start,
                table: self.table.clone(),
                element_count: list_ptr.element_count(),
                step: existing_element_size.data_bytes(),
                struct_data_len: existing_element_size.data_bytes(),
                struct_ptrs_len: 0,
                element_size: existing_element_size,
            },
        };

        macro_rules! data_element {
            () => {
                ElementSize::Byte
                    | ElementSize::TwoBytes
                    | ElementSize::FourBytes
                    | ElementSize::EightBytes
            };
        }

        let list = match (existing_element_size, expected_element_size) {
            // No expected size and expected void can be made from anything
            (_, None | Some(ElementSize::Void)) => existing_list,
            (PtrElementSize::Bit, Some(ElementSize::Bit)) => existing_list,
            (PtrElementSize::Bit, Some(expected)) => {
                return Err((Error::fail_upgrade(PtrElementSize::Bit, expected.into()), self))
            }
            (actual, Some(ElementSize::Bit)) => {
                return Err((Error::fail_upgrade(actual, PtrElementSize::Bit), self))
            }
            (PtrElementSize::Void, Some(ElementSize::InlineComposite(StructSize::EMPTY)))
                // The existing pointer is void, but our expected struct size is empty? Just
                // return the existing list it'll be fine.
                => existing_list,
            (PtrElementSize::Void, Some(size @ ElementSize::InlineComposite(_))) => {
                // The existing pointer is void, so just allocate a new list
                // Note, we don't use init_list, which would panic if we had a max size list of
                // void
                self.try_init_list(size, existing_list.element_count)?
            }
            (_, Some(element_size @ ElementSize::InlineComposite(size))) => {
                // The existing pointer may or may not be an inline composite with the right size,
                // so let's check the size of everything first and see if we need to promote the
                // list.

                let expected_data_size_bytes = (size.data as u32) * (Word::BYTES as u32);
                let expected_data_larger = expected_data_size_bytes > existing_list.struct_data_len;
                let expected_ptrs_larger = size.ptrs > existing_list.struct_ptrs_len;
                if expected_data_larger || expected_ptrs_larger {
                    // One of the sections is bigger, so we need a promotion

                    self.builder.clear_ptrs(self.ptr);
                    let mut new_list = self.try_init_list(element_size, existing_list.element_count)?;
                    for i in 0..existing_list.len().get() {
                        unsafe {
                            let mut old_struct = existing_list.struct_mut_unchecked(i);
                            let mut new_struct = new_list.struct_mut_unchecked(i);
                            Self::transfer_struct(&mut old_struct, &mut new_struct);
                        }
                    }

                    new_list
                } else {
                    existing_list
                }
            }
            (existing, Some(ElementSize::Pointer)) => {
                if existing_list.struct_ptrs_len > 0 {
                    existing_list
                } else {
                    return Err((Error::fail_upgrade(existing, PtrElementSize::Pointer), self))
                }
            }
            (existing, Some(expected @ data_element!())) => {
                if existing_list.struct_data_len > 0 {
                    existing_list
                } else {
                    return Err((Error::fail_upgrade(existing, expected.into()), self))
                }
            }
        };

        Ok(list)
    }

    /// Gets a list builder for this pointer, or an empty list builder if the value is null or
    /// invalid.
    #[inline]
    pub fn to_list_mut_or_empty(
        self,
        expected_element_size: Option<ElementSize>,
    ) -> ListBuilder<'a, T> {
        match self.to_list_mut(expected_element_size) {
            Ok(builder) => builder,
            Err((_, this)) => ListBuilder::empty(
                this.builder,
                this.ptr,
                this.table.clone(),
                expected_element_size.unwrap_or(ElementSize::Void).into(),
            ),
        }
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
    ) -> BuildResult<ListBuilder<'a, T>, Self, Error> {
        self.clear();

        let Some((start, builder)) = self.builder.alloc_list(self.ptr, element_size, element_count) else {
            return Err((ErrorKind::AllocTooLarge.into(), self));
        };

        let step = element_size.bits() / 8;
        let (struct_data_len, struct_ptrs_len) = {
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
            table: self.table.clone(),
            element_count,
            step,
            struct_data_len,
            struct_ptrs_len,
            element_size: element_size.into(),
        })
    }

    #[inline]
    pub fn init_list(
        self,
        element_size: ElementSize,
        element_count: ElementCount,
    ) -> ListBuilder<'a, T> {
        self.try_init_list(element_size, element_count)
            .map_err(|(e, _)| e)
            .expect("too many elements for element size")
    }

    pub fn try_set_list<E, F, U>(
        mut self,
        value: &ListReader<U>,
        canonical: bool,
        mut err_handler: F,
    ) -> BuildResult<ListBuilder<'a, T>, Self, E>
    where
        F: FnMut(Error) -> ControlFlow<E, WriteNull>,
        U: InsertableInto<T>,
    {
        self.clear();

        let element_size = value.element_size();

        let (ptr, builder) = self
            .builder
            .alloc_list(self.ptr, element_size, value.element_count)
            .expect("input list value is too large to be contained in a message");

        let mut list = ListBuilder {
            builder,
            ptr,
            table: self.table.clone(),
            element_count: value.element_count,
            step: value.step,
            struct_data_len: value.struct_data_len,
            struct_ptrs_len: value.struct_ptrs_len,
            element_size: element_size.into(),
        };

        match element_size {
            ElementSize::InlineComposite(size) => {
                todo!()
            }
            ElementSize::Pointer => {
                let len = value.element_count.get();
                for i in 0..len {
                    let original = unsafe { value.ptr_unchecked(i) };
                    let mut new = unsafe { list.ptr_mut_unchecked(i) };
                    if let Err(err) =
                        new.try_copy_from(&original, canonical, err_handler.borrow_mut())
                    {
                        return Err((err, self));
                    }
                }
            }
            ElementSize::Void => {}
            _ => {
                list.as_slice_mut().copy_from_slice(value.as_slice());
            }
        }

        Ok(list)
    }

    #[inline]
    pub fn set_list(
        self,
        value: &ListReader<impl InsertableInto<T>>,
        canonical: bool,
    ) -> ListBuilder<'a, T> {
        self.try_set_list::<Infallible, _, _>(value, canonical, |_| {
            ControlFlow::Continue(WriteNull)
        })
        .map_err(|e| e.0)
        .unwrap()
    }

    #[inline]
    pub fn to_data(&self) -> Result<Option<DataReader>> {
        self.as_reader().to_data()
    }

    #[inline]
    pub fn to_data_mut(self) -> BuildResult<DataBuilder<'a>, Self> {
        todo!()
    }

    #[inline]
    pub fn init_data(
        self,
        element_count: ElementCount,
    ) -> DataBuilder<'a> {
        todo!()
    }

    #[inline]
    pub fn set_data(
        self,
        value: &DataReader,
    ) -> DataBuilder<'a> {
        let len = value.len();
        let builder = self.init_data(len);
        unsafe {
            builder.data()
                .as_ptr()
                .copy_from_nonoverlapping(value.data().as_ptr() as *const _, len.get() as usize)
        }
        builder
    }

    #[inline]
    pub fn to_text(&self) -> Result<Option<TextReader>> {
        self.as_reader().to_text()
    }

    #[inline]
    pub fn to_text_mut(self) -> BuildResult<TextBuilder<'a>, Self> {
        todo!()
    }

    /// Initializes a text blob for this pointer. Note: the provided element count _includes_ the null-terminator, so it
    /// cannot be zero.
    #[inline]
    pub fn init_text(
        self,
        element_count: TextByteCount,
    ) -> TextBuilder<'a> {
        todo!()
    }

    #[inline]
    pub fn set_text(
        self,
        value: &TextReader,
    ) -> TextBuilder<'a> {
        todo!()
    }

    /// Transfer a pointer from one place to another.
    fn transfer_into(&mut self, other: &mut PtrBuilder<'a, T>) {
        debug_assert!(self.builder.is_same_message(&other.builder));

        other.clear();

        Self::transfer(self.ptr, &self.builder, other.ptr, &other.builder)
    }

    fn transfer(
        existing_ptr: SegmentRef,
        existing_builder: &ObjectBuilder,
        new_ptr: SegmentRef,
        new_builder: &ObjectBuilder,
    ) {
        debug_assert!(new_ptr.as_ref().is_null());

        if existing_ptr.as_ref().is_null() {
            unsafe { new_ptr.as_ptr_mut().write(Word::NULL) }
        } else {
            match existing_ptr.as_wire_ptr().kind() {
                WireKind::Struct | WireKind::List => {}
                WireKind::Far | WireKind::Other => {
                    unsafe { new_ptr.as_ptr_mut().write(*existing_ptr.as_ref()) }
                }
            }
            
        }
    }

    /// Clears the pointer, passing any errors to a given error handler.
    ///
    /// The handler can choose to break with an error E, or continue and write null instead.
    ///
    /// You probably don't want this function, as writing null is a perfectly reasonable default
    /// to use. In which case, `clear` does this for you. You should only use this if you want
    /// strict validation or to customize error handling somehow.
    #[inline]
    pub fn try_clear<E, F>(&mut self, mut err_handler: F) -> Result<(), E>
    where
        F: FnMut(Error) -> ControlFlow<E, WriteNull>,
    {
        Self::try_clear_inner(
            &self.builder,
            self.ptr,
            &self.table,
            err_handler.borrow_mut(),
        )
    }

    fn try_clear_inner<E, F>(
        builder: &ObjectBuilder<'a>,
        ptr: SegmentRef<'a>,
        table: &T::Builder,
        err_handler: &mut F,
    ) -> Result<(), E>
    where
        F: FnMut(Error) -> ControlFlow<E, WriteNull>,
    {
        if *ptr.as_ref() == Word::NULL {
            return Ok(());
        }

        let Content {
            accessor: object_builder,
            ptr: content_ptr,
            location,
        } = builder.clone().locate(ptr);
        let content = content_ptr.as_wire_ptr();

        match content.kind() {
            WireKind::Struct => {
                let struct_ptr = content.struct_ptr().unwrap();
                let size = StructSize {
                    data: struct_ptr.data_size(),
                    ptrs: struct_ptr.ptr_count(),
                };
                if size != StructSize::EMPTY {
                    let (start, mut struct_builder) = match location {
                        Location::Near | Location::Far => object_builder
                            .build_object_from_end_of(content_ptr, struct_ptr.offset()),
                        Location::DoubleFar { segment, offset } => object_builder
                            .build_object_in(segment, offset)
                            .expect("struct pointers cannot refer to read-only segments"),
                    };

                    Self::try_clear_struct_inner(
                        &mut struct_builder,
                        start,
                        size,
                        table,
                        err_handler,
                    )?;
                }

                builder.clear_ptrs(ptr);
            }
            WireKind::List => {
                use PtrElementSize::*;

                let list_ptr = content.list_ptr().unwrap();
                let (start, mut list_builder) = match location {
                    Location::Near | Location::Far => {
                        object_builder.build_object_from_end_of(content_ptr, list_ptr.offset())
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
                        let section = list_builder
                            .section_slice_mut(start, count)
                            .iter_mut()
                            .map(|w| unsafe { SegmentRef::new_unchecked(NonNull::from(w)) });
                        for ptr in section {
                            Self::try_clear_inner(&list_builder, ptr, table, err_handler)?;
                        }
                    }
                    InlineComposite => {
                        let tag = start
                            .as_wire_ptr()
                            .struct_ptr()
                            .expect("inline composite tag must be struct");

                        let list_start = unsafe { start.offset(1u16.into()).as_ref_unchecked() };

                        let size = StructSize {
                            data: tag.data_size(),
                            ptrs: tag.ptr_count(),
                        };

                        if size != StructSize::EMPTY {
                            let step = size.total();
                            for i in 0..tag.inline_composite_element_count().get() {
                                let offset = SegmentOffset::new(i * step).unwrap();
                                let struct_start =
                                    unsafe { list_start.offset(offset.into()).as_ref_unchecked() };
                                Self::try_clear_struct_inner(
                                    &mut list_builder,
                                    struct_start,
                                    size,
                                    table,
                                    &mut *err_handler,
                                )?;
                            }
                        }

                        list_builder.set_ptr(start, WirePtr::NULL);
                    }
                    primitive => {
                        let primitive_bits = u64::from(primitive.bits());
                        let element_count = u64::from(count.get());

                        let words = Word::round_up_bit_count(primitive_bits * element_count);
                        let offset = SegmentOffset::new(words).unwrap();

                        list_builder.clear_section(start, offset);
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
            }
            WireKind::Far => unreachable!("invalid far pointer in builder"),
        }

        builder.clear_ptrs(ptr);

        Ok(())
    }

    /// Clears a struct (including inline composite structs)
    fn try_clear_struct_inner<E, F>(
        builder: &mut ObjectBuilder<'a>,
        start: SegmentRef<'a>,
        size: StructSize,
        table: &T::Builder,
        err_handler: &mut F,
    ) -> Result<(), E>
    where
        F: FnMut(Error) -> ControlFlow<E, WriteNull>,
    {
        builder.clear_section(start, size.data.into());

        let iter = unsafe {
            let ptrs_start = start.offset(size.data.into()).as_ref_unchecked();
            ptrs_start.iter_unchecked(size.ptrs.into())
        };
        for ptr in iter.filter(|r| !r.as_ref().is_null()) {
            Self::try_clear_inner(&builder, ptr, table, err_handler)?;
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

    fn try_copy_from_inner<E, F, U>(
        &mut self,
        ptr: SegmentRef<'_>,
        reader: ObjectReader<'_>,
        table: &U::Reader,
        canonical: bool,
        err_handler: &mut F,
    ) -> Result<(), E>
    where
        F: FnMut(Error) -> ControlFlow<E, WriteNull>,
        U: InsertableInto<T>,
    {
        let location_result = reader.location_of(ptr).map_err(err_handler.borrow_mut());
        let Content {
            accessor: reader,
            ptr: content_ptr,
            location,
        } = match location_result {
            Ok(content) => content,
            Err(ControlFlow::Break(err)) => return Err(err),
            Err(ControlFlow::Continue(WriteNull)) => return Ok(()),
        };

        let content_wire_ptr = content_ptr.as_wire_ptr();
        match content_wire_ptr.kind() {
            WireKind::Struct => {
                let struct_ptr = content_wire_ptr.struct_ptr().unwrap();

                todo!()
            }
            WireKind::List => {
                todo!()
            }
            WireKind::Other => {
                let result = content_wire_ptr
                    .try_cap_ptr()
                    .and_then(|cap| {
                        if canonical {
                            Err(ErrorKind::CapabilityNotAllowed.into())
                        } else {
                            Ok(cap)
                        }
                    })
                    .map_err(err_handler.borrow_mut())
                    .and_then(|cap| {
                        let index = cap.capability_index();
                        U::copy(&table, index, &self.table).map_err(err_handler.borrow_mut())
                    });
                let new_ptr = match result {
                    Ok(index) => CapabilityPtr::new(index).into(),
                    Err(ControlFlow::Continue(WriteNull)) => WirePtr::NULL,
                    Err(ControlFlow::Break(err)) => return Err(err),
                };
                self.builder.set_ptr(self.ptr, new_ptr);
            }
            WireKind::Far => {
                let flow = err_handler(Error::fail_read(None, *content_wire_ptr));
                if let ControlFlow::Break(err) = flow {
                    return Err(err);
                }
                // the handler requested we write null, but we're assuming that has already
                // happened or isn't needed
            }
        }

        Ok(())
    }

    /// Performs a deep copy of the given pointer, optionally canonicalizing it.
    ///
    /// If any errors occur while reading from the given pointer, the error is passed to the given
    /// error handler function. The handler can choose to break with an error E, or continue and
    /// write null instead.
    ///
    /// This function will clear the pointer before copying, but will use an infallible clear and not
    /// pass the error handler provided here to the clearing function. If you want to handle clearing
    /// errors, do the clear separately before calling this function.
    ///
    /// You probably don't want this function, as writing null is a perfectly reasonable default
    /// to use in all cases. In which case `copy_from` does this for you. You should only use this
    /// if you want strict validation or to customize error handling somehow.
    #[inline]
    pub fn try_copy_from<E, F, U>(
        &mut self,
        other: &PtrReader<U>,
        canonical: bool,
        mut err_handler: F,
    ) -> Result<(), E>
    where
        F: FnMut(Error) -> ControlFlow<E, WriteNull>,
        U: InsertableInto<T>,
    {
        self.clear();

        self.try_copy_from_inner::<E, F, U>(
            other.ptr,
            other.reader.clone(),
            &other.table,
            canonical,
            &mut err_handler,
        )
    }

    /// Performs a deep copy of the given pointer, optionally canonicalizing it.
    ///
    /// If any errors occur while copying, this writes null. If you want a fallible copy or want
    /// to customize error handling behavior, use `try_copy_from`.
    #[inline]
    pub fn copy_from(&mut self, other: &PtrReader<impl InsertableInto<T>>, canonical: bool) {
        self.try_copy_from::<Infallible, _, _>(other, canonical, |_| {
            ControlFlow::Continue(WriteNull)
        })
        .unwrap()
    }
}

impl<'a, T: Table> Capable for PtrBuilder<'a, T> {
    type Table = T;

    type Imbued = T::Builder;
    type ImbuedWith<T2: Table> = PtrBuilder<'a, T2>;

    #[inline]
    fn imbued(&self) -> &Self::Imbued { &self.table }

    #[inline]
    fn imbue_release<T2: Table>(
        self,
        new_table: T2::Builder,
    ) -> (Self::ImbuedWith<T2>, <Self::Table as Table>::Builder) {
        let old_table = self.table;
        let ptr = PtrBuilder {
            builder: self.builder,
            ptr: self.ptr,
            table: new_table,
        };
        (ptr, old_table)
    }

    #[inline]
    fn imbue_release_into<U: Capable>(&self, other: U) -> (U::ImbuedWith<T>, U::Imbued)
    where
        U: Capable,
        U::ImbuedWith<Self::Table>: Capable<Imbued = Self::Imbued>,
    {
        other.imbue_release::<T>(self.table.clone())
    }
}

pub struct StructBuilder<'a, T: Table = Empty> {
    builder: ObjectBuilder<'a>,
    data_start: NonNull<u8>,
    ptrs_start: SegmentRef<'a>,
    table: T::Builder,
    data_len: u32,
    ptrs_len: u16,
}

impl<'a, T: Table> Capable for StructBuilder<'a, T> {
    type Table = T;

    type Imbued = T::Builder;
    type ImbuedWith<T2: Table> = StructBuilder<'a, T2>;

    #[inline]
    fn imbued(&self) -> &Self::Imbued { &self.table }

    #[inline]
    fn imbue_release<T2: Table>(
        self,
        new_table: T2::Builder,
    ) -> (Self::ImbuedWith<T2>, <Self::Table as Table>::Builder) {
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

    #[inline]
    fn imbue_release_into<U: Capable>(&self, other: U) -> (U::ImbuedWith<T>, U::Imbued)
    where
        U: Capable,
        U::ImbuedWith<Self::Table>: Capable<Imbued = Self::Imbued>,
    {
        other.imbue_release::<T>(self.table.clone())
    }
}

impl<'a, T: Table> StructBuilder<'a, T> {
    #[inline]
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
    pub fn size(&self) -> StructSize {
        StructSize {
            data: Word::round_up_byte_count(self.data_len) as u16,
            ptrs: self.ptrs_len,
        }
    }

    #[inline]
    pub fn total_size(&self) -> Result<MessageSize> {
        todo!()
    }

    #[inline]
    pub fn data_section(&self) -> &[u8] {
        unsafe { core::slice::from_raw_parts(self.data_const(), self.data_len as usize) }
    }

    #[inline]
    pub fn data_section_mut(&mut self) -> &mut [u8] {
        unsafe { core::slice::from_raw_parts_mut(self.data(), self.data_len as usize) }
    }

    #[inline]
    pub fn by_ref<'b>(&'b mut self) -> StructBuilder<'b, T> {
        StructBuilder {
            builder: self.builder.clone(),
            data_start: self.data_start,
            ptrs_start: self.ptrs_start,
            table: self.table.clone(),
            data_len: self.data_len,
            ptrs_len: self.ptrs_len,
        }
    }

    /// Clears all data in the struct
    #[inline]
    fn clear(&mut self) {
        let total_len = ((self.data_len) + ((self.ptrs_len as u32) * 8)) as usize;
        unsafe { core::ptr::write_bytes(self.data_start.as_ptr(), 0, total_len) }
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
    pub fn ptr_section<'b>(&'b self) -> ListReader<'b, T> {
        ListReader {
            reader: self.builder.as_reader(),
            ptr: self.ptrs_start,
            table: self.table.as_reader(),
            element_count: self.ptrs_len.into(),
            step: Word::BYTES as u32,
            struct_data_len: 0,
            struct_ptrs_len: 1,
            element_size: PtrElementSize::Pointer,
            nesting_limit: u32::MAX,
        }
    }

    #[inline]
    pub fn ptr_section_mut<'b>(&'b mut self) -> ListBuilder<'b, T> {
        ListBuilder {
            builder: self.builder.clone(),
            ptr: self.ptrs_start,
            table: self.table.clone(),
            element_count: self.ptrs_len.into(),
            step: Word::BYTES as u32,
            struct_data_len: 0,
            struct_ptrs_len: 1,
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
            ptr: self.ptrs_start.offset(slot.into()).as_ref_unchecked(),
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
            ptr: self.ptrs_start.offset(slot.into()).as_ref_unchecked(),
            builder: self.builder.clone(),
            table: self.table.clone(),
        }
    }
}

pub struct ListBuilder<'a, T: Table = Empty> {
    builder: ObjectBuilder<'a>,
    ptr: SegmentRef<'a>,
    table: T::Builder,
    element_count: ElementCount,
    step: u32,
    struct_data_len: u32,
    struct_ptrs_len: u16,
    element_size: PtrElementSize,
}

impl<'a, T: Table> ListBuilder<'a, T> {
    #[inline]
    fn empty(
        builder: ObjectBuilder<'a>,
        ptr: SegmentRef<'a>,
        table: T::Builder,
        element_size: PtrElementSize,
    ) -> Self {
        Self {
            builder,
            ptr,
            table,
            element_count: 0.into(),
            step: 0,
            struct_data_len: 0,
            struct_ptrs_len: 0,
            element_size,
        }
    }

    fn as_slice_mut(&mut self) -> &mut [Word] {
        let elements = u64::from(self.element_count.get());
        let size_bits = u64::from(self.element_size.data_bits());
        let offset = Word::round_up_bit_count(elements * size_bits);

        self.builder
            .section_slice_mut(self.ptr, SegmentOffset::new(offset).unwrap())
    }

    #[inline]
    pub fn as_reader(&self) -> ListReader<T> {
        ListReader {
            ptr: self.ptr,
            reader: self.builder.as_reader(),
            table: self.table.as_reader(),
            element_count: self.element_count,
            nesting_limit: u32::MAX,
            step: self.step,
            struct_data_len: self.struct_data_len,
            struct_ptrs_len: self.struct_ptrs_len,
            element_size: self.element_size,
        }
    }

    #[inline]
    pub fn len(&self) -> ElementCount {
        self.element_count
    }

    #[inline]
    pub fn is_empty(&self) -> bool {
        self.len().get() == 0
    }

    #[inline]
    fn ptr(&self) -> *const u8 {
        self.ptr.as_ptr().cast()
    }

    #[inline]
    fn ptr_mut(&mut self) -> *mut u8 {
        self.ptr.as_ptr_mut().cast()
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
    /// * This is not a void, pointer, or inline composite list
    /// * If this is a bit list, D must be Bool
    /// * If this is a primitive list, D must have a size equal to or less than the element size
    ///   of the list
    #[inline]
    pub unsafe fn data_unchecked<D: internal::FieldData>(&self, index: u32) -> D {
        use core::any::TypeId;

        debug_assert!(index < self.element_count.get(), "index out of bounds");
        debug_assert!({
            match self.element_size {
                PtrElementSize::Void | PtrElementSize::Pointer => false,
                PtrElementSize::Bit => TypeId::of::<D>() == TypeId::of::<bool>(),
                PtrElementSize::InlineComposite => self.struct_data_len != 0,
                size => PtrElementSize::from(D::ELEMENT_SIZE).data_bytes() <= size.data_bytes(),
            }
        }, "attempted to access invalid data for this list; list element size: {:?}, data type: {}",
        self.element_size, core::any::type_name::<D>());

        if TypeId::of::<D>() == TypeId::of::<bool>() {
            D::read_unchecked(self.ptr(), index as usize, D::default())
        } else {
            let ptr = self.ptr().add(self.index_to_offset(index));
            D::read_unchecked(ptr, 0, D::default())
        }
    }
    #[inline]
    pub unsafe fn set_data_unchecked<D: internal::FieldData>(&mut self, index: u32, value: D) {
        use core::any::TypeId;

        debug_assert!(index < self.element_count.get(), "index out of bounds");
        debug_assert!({
            match self.element_size {
                PtrElementSize::Void | PtrElementSize::Pointer => false,
                PtrElementSize::Bit => TypeId::of::<D>() == TypeId::of::<bool>(),
                PtrElementSize::InlineComposite => self.struct_data_len != 0,
                size => PtrElementSize::from(D::ELEMENT_SIZE).data_bytes() <= size.data_bytes(),
            }
        }, "attempted to access invalid data for this list; list element size: {:?}, data type: {}",
        self.element_size, core::any::type_name::<D>());

        if TypeId::of::<D>() == TypeId::of::<bool>() {
            D::write_unchecked(self.ptr_mut(), index as usize, value, D::default())
        } else {
            let ptr = self.ptr_mut().add(self.index_to_offset(index));
            D::write_unchecked(ptr, 0, value, D::default())
        }
    }

    /// Gets a pointer reader for the pointer at the specified index.
    ///
    /// # Safety
    ///
    /// * The index must be within bounds
    /// * This must be a pointer list
    #[inline]
    pub unsafe fn ptr_unchecked<'b>(&'b self, index: u32) -> PtrReader<'b, T> {
        debug_assert!(index < self.element_count.get(), "index out of bounds");
        debug_assert!(
            (self.element_size == PtrElementSize::Pointer)
                || (self.element_size == PtrElementSize::InlineComposite
                    && self.struct_ptrs_len != 0),
            "attempted to read pointer from a list of something else"
        );

        let offset = ((self.step * index) + self.struct_data_len) as usize;
        let ptr = self.ptr().add(offset).cast_mut().cast();

        PtrReader {
            ptr: SegmentRef::new_unchecked(NonNull::new_unchecked(ptr)),
            reader: self.builder.as_reader(),
            table: self.table.as_reader(),
            nesting_limit: u32::MAX,
        }
    }
    #[inline]
    pub unsafe fn ptr_mut_unchecked<'b>(&'b mut self, index: u32) -> PtrBuilder<'b, T> {
        debug_assert!(index < self.element_count.get(), "index out of bounds");
        debug_assert!(
            (self.element_size == PtrElementSize::Pointer)
                || (self.element_size == PtrElementSize::InlineComposite
                    && self.struct_ptrs_len != 0),
            "attempted to read pointer from a list of something else"
        );

        let offset = ((self.step * index) + self.struct_data_len) as usize;
        let ptr = self.ptr().add(offset).cast_mut().cast();

        PtrBuilder {
            ptr: SegmentRef::new_unchecked(NonNull::new_unchecked(ptr)),
            builder: self.builder.clone(),
            table: self.table.clone(),
        }
    }

    /// Gets a struct reader for the struct at the specified index.
    ///
    /// # Safety
    ///
    /// * The index must be within bounds
    /// * This must be an inline composite list
    #[inline]
    pub unsafe fn struct_unchecked<'b>(&'b self, index: u32) -> StructReader<'b, T> {
        debug_assert!(index < self.element_count.get(), "index out of bounds");
        debug_assert!(
            self.element_size != PtrElementSize::Bit,
            "attempted to read struct from bit list"
        );

        let struct_start = self.ptr().add(self.index_to_offset(index));
        let struct_data = struct_start;
        let struct_ptrs = struct_start
            .add(self.struct_data_len as usize)
            .cast::<Word>();

        StructReader {
            data_start: NonNull::new_unchecked(struct_data.cast_mut()),
            ptrs_start: SegmentRef::new_unchecked(NonNull::new_unchecked(struct_ptrs.cast_mut())),
            reader: self.builder.as_reader().clone(),
            table: self.table.as_reader(),
            data_len: self.struct_data_len,
            ptrs_len: self.struct_ptrs_len,
            nesting_limit: u32::MAX,
        }
    }
    #[inline]
    pub unsafe fn struct_mut_unchecked<'b>(&'b mut self, index: u32) -> StructBuilder<'b, T> {
        debug_assert!(index < self.element_count.get(), "index out of bounds");
        debug_assert!(
            self.element_size != PtrElementSize::Bit,
            "attempted to read struct from bit list"
        );

        let struct_start = self.ptr().add(self.index_to_offset(index));
        let struct_data = struct_start;
        let struct_ptrs = struct_start
            .add(self.struct_data_len as usize)
            .cast::<Word>();

        StructBuilder {
            data_start: NonNull::new_unchecked(struct_data.cast_mut()),
            ptrs_start: SegmentRef::new_unchecked(NonNull::new_unchecked(struct_ptrs.cast_mut())),
            builder: self.builder.clone(),
            table: self.table.clone(),
            data_len: self.struct_data_len,
            ptrs_len: self.struct_ptrs_len,
        }
    }
}

impl<'a, T: Table> Capable for ListBuilder<'a, T> {
    type Table = T;

    type Imbued = T::Builder;
    type ImbuedWith<T2: Table> = ListBuilder<'a, T2>;

    #[inline]
    fn imbued(&self) -> &Self::Imbued { &self.table }

    #[inline]
    fn imbue_release<T2: Table>(
        self,
        new_table: T2::Builder,
    ) -> (Self::ImbuedWith<T2>, T::Builder) {
        let old_table = self.table;
        let ptr = ListBuilder {
            builder: self.builder,
            ptr: self.ptr,
            table: new_table,
            element_count: self.element_count,
            step: self.step,
            struct_data_len: self.struct_data_len,
            struct_ptrs_len: self.struct_ptrs_len,
            element_size: self.element_size,
        };
        (ptr, old_table)
    }

    #[inline]
    fn imbue_release_into<U: Capable>(&self, other: U) -> (U::ImbuedWith<T>, U::Imbued)
    where
        U: Capable,
        U::ImbuedWith<Self::Table>: Capable<Imbued = Self::Imbued>,
    {
        other.imbue_release::<T>(self.table.clone())
    }
}

#[cfg(test)]
mod tests {
    use crate::{
        alloc::{Alloc, AllocLen, Fixed, Global, Scratch, Space, Word},
        any::AnyStruct,
        message::{Message, ReaderOptions},
        rpc::Empty,
    };
    use core::ptr::NonNull;

    use super::{
        ElementCount, ElementSize, PtrBuilder, PtrElementSize, PtrReader, StructBuilder,
        StructReader, StructSize,
    };

    #[test]
    fn simple_struct() {
        // making AlignedData hard, easier to just write the bytes as numbers
        // and swap the bytes with u64::to_be()
        let data = [
            Word(0x_00_00_00_00_01_00_00_00_u64.to_be()),
            Word(0x_01_23_45_67_89_ab_cd_ef_u64.to_be()),
        ];

        let ptr =
            unsafe { PtrReader::new_unchecked(NonNull::new(data.as_ptr().cast_mut()).unwrap()) };
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
                .init_struct(StructSize { data: 1, ptrs: 0 });
            sub_struct.set_field::<u32>(0, 123);
        }

        {
            let mut list = builder
                .ptr_field_mut(1)
                .unwrap()
                .init_list(ElementSize::FourBytes, 3.into());
            assert_eq!(list.len().get(), 3);
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
            assert_eq!(list.len().get(), 4);
            for i in 0..4 {
                let mut builder = unsafe { list.struct_mut_unchecked(i) };
                builder.set_field(0, 300 + i);
                builder
                    .ptr_field_mut(0)
                    .unwrap()
                    .init_struct(StructSize { data: 1, ptrs: 0 })
                    .set_field(0, 400 + i);
            }
        }

        {
            let mut list = builder
                .ptr_field_mut(3)
                .unwrap()
                .init_list(ElementSize::Pointer, 5.into());
            assert_eq!(list.len().get(), 5);
            for i in 0..5 {
                let mut element = unsafe {
                    list.ptr_mut_unchecked(i)
                        .init_list(ElementSize::TwoBytes, ElementCount::new(i + 1).unwrap())
                };
                for j in 0..=i {
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
        assert_eq!(0x40, reader.data_field::<u8>(14));
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
            assert_eq!(3, list.len().get());
            unsafe {
                assert_eq!(200, list.data_unchecked::<i32>(0));
                assert_eq!(201, list.data_unchecked::<i32>(1));
                assert_eq!(202, list.data_unchecked::<i32>(2));
            }
        }

        {
            let list = reader
                .ptr_field(2)
                .to_list(Some(PtrElementSize::InlineComposite))
                .unwrap()
                .unwrap();
            assert_eq!(4, list.len().get());
            for i in 0..4 {
                let element = unsafe { list.struct_unchecked(i) };
                assert_eq!(300 + (i as i32), element.data_field::<i32>(0));
                let element_struct = element.ptr_field(0).to_struct().unwrap().unwrap();
                assert_eq!(400 + (i as i32), element_struct.data_field::<i32>(0));
            }
        }

        {
            let list = reader
                .ptr_field(3)
                .to_list(Some(PtrElementSize::Pointer))
                .unwrap()
                .unwrap();
            assert_eq!(5, list.len().get());
            for i in 0..5 {
                let element = unsafe {
                    list.ptr_unchecked(i)
                        .to_list(Some(PtrElementSize::TwoBytes))
                        .unwrap()
                        .unwrap()
                };
                assert_eq!(i + 1, element.len().get());
                for j in 0..=i {
                    assert_eq!(500 + (j as u16), unsafe {
                        element.data_unchecked::<u16>(j)
                    });
                }
            }
        }
    }

    fn check_struct_builder(builder: &mut StructBuilder<Empty>) {
        assert_eq!(0x1011121314151617, builder.data_field::<u64>(0));
        assert_eq!(0x20212223, builder.data_field::<u32>(2));
        assert_eq!(0x3031, builder.data_field::<u16>(6));
        assert_eq!(0x40, builder.data_field::<u8>(14));
        assert_eq!(false, builder.data_field(120));
        assert_eq!(false, builder.data_field(121));
        assert_eq!(true, builder.data_field(122));
        assert_eq!(false, builder.data_field(123));
        assert_eq!(true, builder.data_field(124));
        assert_eq!(true, builder.data_field(125));
        assert_eq!(true, builder.data_field(126));
        assert_eq!(false, builder.data_field(127));

        {
            let substruct = builder
                .ptr_field_mut(0)
                .unwrap()
                .to_struct_mut(Some(StructSize { data: 1, ptrs: 0 }))
                .unwrap();
            assert_eq!(123, substruct.data_field::<u32>(0));
        }

        {
            let list = builder
                .ptr_field_mut(1)
                .unwrap()
                .to_list_mut(Some(ElementSize::FourBytes))
                .unwrap();
            assert_eq!(3, list.len().get());
            unsafe {
                assert_eq!(200, list.data_unchecked::<i32>(0));
                assert_eq!(201, list.data_unchecked::<i32>(1));
                assert_eq!(202, list.data_unchecked::<i32>(2));
            }
        }

        {
            let list = builder
                .ptr_field_mut(2)
                .unwrap()
                .to_list_mut(Some(ElementSize::InlineComposite(StructSize {
                    data: 1,
                    ptrs: 1,
                })))
                .unwrap();
            assert_eq!(4, list.len().get());
            for i in 0..4 {
                let element = unsafe { list.struct_unchecked(i) };
                assert_eq!(300 + (i as i32), element.data_field::<i32>(0));
                let element_struct = element.ptr_field(0).to_struct().unwrap().unwrap();
                assert_eq!(400 + (i as i32), element_struct.data_field::<i32>(0));
            }
        }

        {
            let mut list = builder
                .ptr_field_mut(3)
                .unwrap()
                .to_list_mut(Some(ElementSize::Pointer))
                .unwrap();
            assert_eq!(5, list.len().get());
            for i in 0..5 {
                let element = unsafe {
                    list.ptr_mut_unchecked(i)
                        .to_list_mut(Some(ElementSize::TwoBytes))
                        .unwrap()
                };
                assert_eq!(i + 1, element.len().get());
                for j in 0..=i {
                    assert_eq!(500 + (j as u16), unsafe {
                        element.data_unchecked::<u16>(j)
                    });
                }
            }
        }
    }

    fn struct_round_trip(message: &mut Message<impl Alloc>) {
        {
            let ptr: PtrBuilder = message.builder().into_root().into();
            let mut builder = ptr.init_struct(StructSize { data: 2, ptrs: 4 });
            setup_struct(&mut builder);

            check_struct_builder(&mut builder);
            check_struct_reader(&builder.as_reader());
        }

        let reader = message.reader();
        let root_reader = reader.read_as::<AnyStruct>().into();

        check_struct_reader(&root_reader);

        let limited_reader = message.reader_with_options(ReaderOptions {
            nesting_limit: 4,
            traversal_limit: u64::MAX,
        });
        let root_reader = limited_reader.read_as::<AnyStruct>().into();

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
        let mut space = Space::<34>::new();
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
