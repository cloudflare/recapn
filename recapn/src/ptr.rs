//! Manually manage Cap'n Proto data through raw readers and builders

use crate::alloc::{
    AllocLen, Segment, SegmentLen, SegmentOffset, SegmentPtr, SegmentRef, SignedSegmentOffset, Word,
};
use crate::arena::{ArenaSegment, BuildArena, ReadArena, SegmentId, SegmentWithId};
use crate::field;
use crate::internal::Sealed;
use crate::message::ReadLimiter;
use crate::num::u29;
use crate::orphan::Orphanage;
use crate::rpc::internal::CapPtrBuilder;
use crate::rpc::{
    BreakableCapSystem, CapTable, CapTableBuilder, CapTableReader, Capable, Empty, InsertableInto,
    Table,
};
use crate::ty;
use crate::{Error, Result};
use core::convert::Infallible;
use core::fmt::Debug;
use core::marker::PhantomData;
use core::ops::ControlFlow;
use core::ptr::{self, NonNull};
use core::{cmp, fmt, slice};

pub use crate::data::ptr::{Builder as DataBuilder, Reader as DataReader};
pub use crate::text::ptr::{Builder as TextBuilder, Reader as TextReader};

/// A field type that can be found in the data section of a struct.
pub trait Data:
    field::Value<Default = Self> + ty::ListValue + Default + Copy + Sealed + 'static
{
    unsafe fn read(ptr: *const u8, len: u32, slot: usize, default: Self) -> Self;
    unsafe fn read_unchecked(ptr: *const u8, slot: usize, default: Self) -> Self;

    unsafe fn write(ptr: *mut u8, len: u32, slot: usize, value: Self, default: Self);
    unsafe fn write_unchecked(ptr: *mut u8, slot: usize, value: Self, default: Self);
}

impl Data for bool {
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
        *byte = (*byte & !(1 << bit_num)) | (u8::from(written_value) << bit_num);
    }
}

macro_rules! impl_int {
    ($ty:ty) => {
        impl Data for $ty {
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
            unsafe fn write(ptr: *mut u8, len_bytes: u32, slot: usize, value: Self, default: Self) {
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

impl Data for f32 {
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

impl Data for f64 {
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
            pub fn from_word_slice(words: &[Word]) -> &[Self] {
                let (_, values, _) = unsafe { words.align_to() };
                values
            }

            #[inline]
            pub fn from_word_slice_mut(words: &mut [Word]) -> &mut [Self] {
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

/// The size of a struct in data words and pointers.
///
/// A struct in Cap'n Proto is made up of a "data" section and a pointer section. Space in each
/// section is allocated according to the capnp compiler according to the field order based on
/// each field's ordinal, allowing for backwards compatibility.
#[derive(Default, Debug, Clone, Copy, PartialEq, Eq, PartialOrd, Ord, Hash)]
pub struct StructSize {
    /// The number of words of data in this struct
    pub data: u16,
    /// The number of pointers in this struct
    pub ptrs: u16,
}

impl StructSize {
    /// An empty struct with 0 data words and 0 pointers. Structs of this size take up
    /// no space on the wire and canonical pointers to said structs must have a special -1 offset
    /// to their location.
    pub const EMPTY: StructSize = StructSize { data: 0, ptrs: 0 };

    /// Matches self against `EMPTY`, indicating if the struct has no data or pointer words.
    ///
    /// # Example
    ///
    /// ```
    /// use recapn::ptr::StructSize;
    ///
    /// assert!(StructSize::EMPTY.is_empty());
    /// assert!(StructSize { data: 0, ptrs: 0 }.is_empty());
    ///
    /// let one_ptrs = StructSize { data: 0, ptrs: 1 };
    /// assert!(!one_ptrs.is_empty());
    ///
    /// let one_data = StructSize { data: 1, ptrs: 0 };
    /// assert!(!one_data.is_empty());
    /// ```
    #[inline]
    pub const fn is_empty(self) -> bool {
        matches!(self, Self::EMPTY)
    }

    /// Gets the total size of the struct in words
    ///
    /// # Example
    ///
    /// ```
    /// use recapn::ptr::StructSize;
    ///
    /// assert_eq!(StructSize::EMPTY.total(), 0);
    ///
    /// let a = StructSize { data: 2, ptrs: 5 };
    /// assert_eq!(a.total(), 7);
    ///
    /// let max = StructSize { data: u16::MAX, ptrs: u16::MAX };
    /// assert_eq!(max.total(), 131_070);
    /// ```
    #[inline]
    pub const fn total(self) -> u32 {
        self.data as u32 + self.ptrs as u32
    }

    /// Gets the total size of the struct in words as an [`ObjectLen`].
    ///
    /// This is the same as [`total`] but it just wraps the result in [`ObjectLen`].
    ///
    /// # Example
    ///
    /// ```
    /// use recapn::ptr::StructSize;
    ///
    /// assert_eq!(StructSize::EMPTY.len().get(), 0);
    ///
    /// let a = StructSize { data: 2, ptrs: 5 };
    /// assert_eq!(a.len().get(), 7);
    ///
    /// let max = StructSize { data: u16::MAX, ptrs: u16::MAX };
    /// assert_eq!(max.len().get(), 131_070);
    /// ```
    #[inline]
    pub const fn len(self) -> ObjectLen {
        ObjectLen::new(self.total()).unwrap()
    }

    /// Gets the max number of elements an struct list can contain of this struct.
    ///
    /// Struct lists have a limit on how many elements they can contain of a given struct.
    /// Internally the list pointer to a struct list has the number of [`Word`]s in the list,
    /// which can't be larger than a segment (max 4GB). Struct lists also have a "tag" that
    /// declares the size of the struct itself, included with the list content. Thus, a list of
    /// non-empty structs cannot have [`ElementCount::MAX`] struct elements in it, and the max
    /// number of elements a struct list can have strinks as the struct grows larger.
    ///
    /// # Example
    ///
    /// ```
    /// use recapn::ptr::{StructSize, ElementCount};
    ///
    /// // An empty struct takes up no space and can have max elements.
    /// assert_eq!(StructSize::EMPTY.max_elements(), ElementCount::MAX);
    ///
    /// // A one word struct can have MAX - 1.
    /// let one_word = StructSize { data: 1, ptrs: 0 };
    /// assert_eq!(one_word.max_elements().get(), ElementCount::MAX_VALUE - 1);
    ///
    /// // A max size struct list can only be 4,096 elements long
    /// let max = StructSize { data: u16::MAX, ptrs: u16::MAX };
    /// assert_eq!(max.max_elements().get(), 4096);
    /// ```
    #[inline]
    pub const fn max_elements(self) -> ElementCount {
        if self.is_empty() {
            ElementCount::MAX
        } else {
            // subtract 1 for the tag ptr
            ElementCount::new((ElementCount::MAX_VALUE - 1) / (self.total())).unwrap()
        }
    }

    /// Returns whether a struct of this size can fit inside a struct of the given size.
    ///
    /// This is effectively shorthand for checking if this struct size's data and ptrs
    /// is less than or equal to the data and ptrs of `outer`.
    ///
    /// # Example
    ///
    /// ```
    /// use recapn::ptr::StructSize;
    ///
    /// let a = StructSize { data: 2, ptrs: 1 };
    /// let b = StructSize { data: 1, ptrs: 0 };
    ///
    /// // `b` fits inside `a` since both sections of `b` are smaller than the sections of `a`
    /// assert!(b.fits_inside(a));
    /// assert!(!a.fits_inside(b));
    /// ```
    #[inline]
    pub const fn fits_inside(self, outer: Self) -> bool {
        self.data <= outer.data && self.ptrs <= outer.ptrs
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

/// The length of an object (pointer, struct, list) within a message in Words.
pub type ObjectLen = u29;
/// The number of elements in a Cap'n Proto list.
pub type ElementCount = u29;

#[derive(Debug, Copy, Clone, PartialEq, Eq)]
enum WireKind {
    Struct = 0,
    List = 1,
    Far = 2,
    Other = 3,
}

/// The type of data a pointer refers to.
#[derive(Debug, Copy, Clone, PartialEq, Eq)]
pub enum PtrType {
    /// The pointer is null (it doesn't refer to anything).
    Null,
    /// The pointer refers to a struct.
    Struct,
    /// The pointer refers to a list. This includes `Data` and `Text` which are represented as
    /// lists of bytes on the wire.
    List,
    /// The pointer refers to a capability.
    Capability,
}

#[derive(Clone, Copy, PartialEq, Eq)]
#[repr(C)]
struct Parts {
    pub lower: WireValue<u32>,
    pub upper: WireValue<u32>,
}

impl Parts {
    /// Gets the segment offset of some content. This is only valid for struct and list pointers.
    pub const fn content_offset(&self) -> SignedSegmentOffset {
        SignedSegmentOffset::new(self.lower.get() as i32 >> 2).unwrap()
    }

    /// Replace the segment offset of some content
    pub const fn set_content_offset(&self, offset: SignedSegmentOffset) -> Self {
        let old_kind = self.lower.get() & 3;
        let new_lower = (offset.get() << 2) as u32 | old_kind;
        Self {
            lower: WireValue(new_lower),
            upper: self.upper,
        }
    }
}

#[inline]
fn fail_upgrade(from: PtrElementSize, to: PtrElementSize) -> Error {
    Error::IncompatibleUpgrade(IncompatibleUpgrade { from, to })
}

#[derive(Clone, Copy)]
union WirePtr {
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
    #[cold]
    fn fail_read(&self, expected: Option<ExpectedRead>) -> Error {
        Error::UnexpectedRead(FailedRead {
            expected,
            actual: {
                if self.is_null() {
                    ActualRead::Null
                } else {
                    use WireKind::*;
                    match self.kind() {
                        Struct => ActualRead::Struct,
                        Far => ActualRead::Far,
                        Other => ActualRead::Other,
                        List => ActualRead::List,
                    }
                }
            },
        })
        .into()
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
        unsafe { &*std::ptr::from_ref::<Word>(w).cast::<WirePtr>() }
    }
}

impl From<WirePtr> for Word {
    fn from(value: WirePtr) -> Self {
        unsafe { value.word }
    }
}

impl crate::alloc::SegmentRef<'_> {
    fn as_wire_ptr(&self) -> &WirePtr {
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
struct StructPtr {
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
        self.parts.content_offset()
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

/// A list element's size, with a struct size for inline composite values.
#[derive(Clone, Copy, Debug, PartialEq)]
pub enum ElementSize {
    /// A void element. This takes up no space on the wire.
    Void,
    /// A bit element. Used by bool lists. Bits are packed together to save space but can't be
    /// upgraded to a struct later.
    Bit,
    /// A byte element. `Data` and `Text` are lists of byte elements.
    Byte,
    /// A two-byte element. Enums are represented by this size.
    TwoBytes,
    /// A four-byte element.
    FourBytes,
    /// A eight-byte element.
    EightBytes,
    /// A pointer element. Lists of lists or capabilities use this element size.
    Pointer,
    /// An inline composite element. Struct lists use this element size.
    InlineComposite(StructSize),
}

impl ElementSize {
    /// Returns whether this element size has no data or pointers.
    ///
    /// # Example
    ///
    /// ```
    /// use recapn::ptr::{ElementSize, StructSize};
    ///
    /// assert!(ElementSize::Void.is_empty());
    /// assert!(ElementSize::InlineComposite(StructSize::EMPTY).is_empty());
    ///
    /// assert!(!ElementSize::Bit.is_empty());
    /// assert!(!ElementSize::EightBytes.is_empty());
    /// assert!(!ElementSize::Pointer.is_empty());
    /// assert!(!ElementSize::InlineComposite(StructSize { data: 1, ptrs: 0 }).is_empty());
    /// ```
    #[inline]
    pub const fn is_empty(self) -> bool {
        matches!(self, Self::Void | Self::InlineComposite(StructSize::EMPTY))
    }

    /// Returns whether this element size consists entirely of pointers.
    ///
    /// # Example
    ///
    /// ```
    /// use recapn::ptr::{ElementSize, StructSize};
    ///
    /// assert!(ElementSize::Pointer.is_ptrs());
    /// assert!(ElementSize::InlineComposite(StructSize { ptrs: 3, data: 0 }).is_ptrs());
    ///
    /// assert!(!ElementSize::Void.is_ptrs());
    /// assert!(!ElementSize::EightBytes.is_ptrs());
    /// assert!(!ElementSize::InlineComposite(StructSize { ptrs: 3, data: 1 }).is_ptrs());
    /// ```
    #[inline]
    pub const fn is_ptrs(self) -> bool {
        use ElementSize::*;
        let not_empty = !self.is_empty();
        // This does not need to check if ptrs is 0 since it would've been caught by
        // the `is_empty` check
        let has_ptrs = matches!(
            self,
            Pointer | InlineComposite(StructSize { data: 0, ptrs: _ })
        );
        not_empty && has_ptrs
    }

    /// Returns whether this element size is a composite of data *and* pointers.
    ///
    /// # Example
    ///
    /// ```
    /// use recapn::ptr::{ElementSize, StructSize};
    ///
    /// assert!(ElementSize::InlineComposite(StructSize { ptrs: 3, data: 5 }).is_composite());
    ///
    /// assert!(!ElementSize::Void.is_composite());
    /// assert!(!ElementSize::EightBytes.is_composite());
    /// assert!(!ElementSize::Pointer.is_composite());
    /// assert!(!ElementSize::InlineComposite(StructSize { ptrs: 3, data: 0 }).is_composite());
    /// assert!(!ElementSize::InlineComposite(StructSize { ptrs: 0, data: 2 }).is_composite());
    /// ```
    #[inline]
    pub const fn is_composite(self) -> bool {
        use ElementSize::InlineComposite;
        matches!(self, InlineComposite(StructSize { data, ptrs }) if data != 0 && ptrs != 0)
    }

    /// Returns whether this element size consists entirely of data.
    ///
    /// # Example
    ///
    /// ```
    /// use recapn::ptr::{ElementSize, StructSize};
    ///
    /// assert!(ElementSize::Bit.is_data());
    /// assert!(ElementSize::EightBytes.is_data());
    /// assert!(ElementSize::InlineComposite(StructSize { ptrs: 0, data: 3 }).is_data());
    ///
    /// assert!(!ElementSize::Void.is_data());
    /// assert!(!ElementSize::Pointer.is_data());
    /// assert!(!ElementSize::InlineComposite(StructSize { ptrs: 3, data: 1 }).is_data());
    /// ```
    #[inline]
    pub const fn is_data(self) -> bool {
        let not_empty = !self.is_empty();
        let not_ptrs = !self.is_ptrs();
        let not_composite = !self.is_composite();
        not_empty && not_ptrs && not_composite
    }

    /// Returns whether this is an inline composite element size.
    ///
    /// Unlike with `is_composite` this does not consider the size of the elements themselves,
    /// just whether it's an "inline composite".
    ///
    /// # Example
    ///
    /// ```
    /// use recapn::ptr::{ElementSize, StructSize};
    ///
    /// let size = ElementSize::InlineComposite(StructSize::EMPTY);
    /// assert!(size.is_inline_composite());
    /// ```
    #[inline]
    pub const fn is_inline_composite(self) -> bool {
        matches!(self, ElementSize::InlineComposite(_))
    }

    /// Returns the number of bits per element. This also returns the number of bits per pointer.
    ///
    /// # Example
    ///
    /// ```
    /// use recapn::ptr::ElementSize;
    ///
    /// assert_eq!(ElementSize::Void.bits(), 0);
    /// assert_eq!(ElementSize::Bit.bits(), 1);
    /// assert_eq!(ElementSize::TwoBytes.bits(), 16);
    /// assert_eq!(ElementSize::Pointer.bits(), 64);
    /// ```
    #[inline]
    pub const fn bits(self) -> u32 {
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

    /// Return the number of bytes and pointers per element. Bit element return 0 bytes.
    ///
    /// # Example
    ///
    /// ```
    /// use recapn::ptr::{ElementSize, StructSize};
    ///
    /// assert_eq!(ElementSize::TwoBytes.bytes_and_ptrs(), (2, 0));
    /// assert_eq!(ElementSize::Pointer.bytes_and_ptrs(), (0, 1));
    ///
    /// let example = StructSize { data: 3, ptrs: 2 };
    /// assert_eq!(ElementSize::InlineComposite(example).bytes_and_ptrs(), (3 * 8, 2));
    /// ```
    #[inline]
    pub const fn bytes_and_ptrs(self) -> (u32, u16) {
        use ElementSize::*;
        match self {
            Void => (0, 0),
            Bit => (0, 0),
            Byte => (1, 0),
            TwoBytes => (2, 0),
            FourBytes => (4, 0),
            EightBytes => (8, 0),
            Pointer => (0, 1),
            InlineComposite(size) => (size.data as u32 * 8, size.ptrs),
        }
    }

    /// Return the struct size if upgrading this element size to a struct.
    ///
    /// Bit elements cannot be upgraded and return an empty struct.
    #[inline]
    pub const fn struct_upgrade(self) -> StructSize {
        use ElementSize::*;
        match self {
            Void | Bit => StructSize::EMPTY,
            Byte | TwoBytes | FourBytes | EightBytes => StructSize { data: 1, ptrs: 0 },
            Pointer => StructSize { data: 0, ptrs: 1 },
            InlineComposite(size) => size,
        }
    }

    /// Get the maximum number of elements a list of this element size can contain. This only
    /// really matters for struct elements, since structs can overflow the max segment size
    /// if they're not zero sized.
    ///
    /// # Example
    ///
    /// ```
    /// use recapn::ptr::{ElementSize, ElementCount, StructSize};
    ///
    /// assert_eq!(ElementSize::FourBytes.max_elements(), ElementCount::MAX);
    ///
    /// let one_data = ElementSize::InlineComposite(StructSize { data: 1, ptrs: 0 });
    /// assert_eq!(one_data.max_elements().get(), ElementCount::MAX_VALUE - 1);
    /// ```
    #[inline]
    pub const fn max_elements(self) -> ElementCount {
        match self {
            Self::InlineComposite(size) => size.max_elements(),
            _ => ElementCount::MAX,
        }
    }

    /// Get the total number of bytes required to hold all the data in a list of this element.
    /// This is padded to the nearest byte for bit elements, but otherwise contains no other data.
    #[inline]
    pub const fn total_bytes(self, count: ElementCount) -> u32 {
        let count = count.get() as u64;
        let element_bits = self.bits() as u64;
        (element_bits * count).div_ceil(8) as u32
    }

    /// Get the total number of words required to hold all the data in a list of this element.
    /// This does not include any other data like the tag pointer for inline composites.
    #[inline]
    pub const fn total_words(self, count: ElementCount) -> u32 {
        let count = count.get() as u64;
        let element_bits = self.bits() as u64;
        Word::round_up_bit_count(element_bits * count)
    }

    /// Gets the total number of words required to allocate a list of this element. This includes
    /// all data including the tag pointer.
    #[inline]
    pub const fn object_words(self, count: ElementCount) -> u32 {
        let tag_size = if self.is_inline_composite() { 1 } else { 0 };
        self.total_words(count) + tag_size
    }

    /// Returns whether a list with elements of this size can be upgraded to a list of another
    /// element size.
    #[inline]
    pub fn upgradable_to(self, other: PtrElementSize) -> bool {
        match (self, other) {
            // Structs can always upgrade to other inline composites
            (ElementSize::InlineComposite(_), PtrElementSize::InlineComposite) => true,
            // But can't be upgraded to bit lists
            (ElementSize::Bit, PtrElementSize::Bit) => true,
            (ElementSize::Bit, _) | (_, PtrElementSize::Bit) => false,
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
            // Most of everything can upgrade to inline composite
            (_, PtrElementSize::InlineComposite) => true,
            (s, o) => s.as_ptr_size() == o,
        }
    }

    /// Creates a new element size based on a possible upgrade between the two sizes.
    ///
    /// This is commutative, and will produce the same upgrade no matter the order
    /// of the args. For example, an upgrade from a Pointer element list to an
    /// InlineComposite list with a pointer element will yield the same result
    /// as "upgrading" an InlineComposite list to a Pointer element list. Either order
    /// will yield an InlineComposite element size.
    ///
    /// ```
    /// # use recapn::ptr::{ElementSize, StructSize};
    /// let a = ElementSize::Pointer;
    /// let b = ElementSize::InlineComposite(StructSize { ptrs: 2, data: 0 });
    ///
    /// let c = a.upgrade_to(b).unwrap();
    /// let d = b.upgrade_to(a).unwrap();
    /// assert_eq!(c, d);
    /// ```
    ///
    /// Attempting to upgrade the result of this function to itself or one of the original
    /// inputs will return the same result. Thus, this can be used to determine if a
    /// list builder has the correct upgraded size when performing a copy.
    ///
    /// ```
    /// # use recapn::ptr::{ElementSize, StructSize};
    /// let a = ElementSize::Pointer;
    /// let b = ElementSize::InlineComposite(StructSize { ptrs: 2, data: 0 });
    ///
    /// let c = a.upgrade_to(b).unwrap();
    /// let d = c.upgrade_to(a).unwrap();
    /// assert_eq!(c, d);
    ///
    /// let e = c.upgrade_to(b).unwrap();
    /// assert_eq!(c, e);
    /// ```
    ///
    /// If the upgrade is invalid, `None` will be returned.
    ///
    /// ```
    /// # use recapn::ptr::{ElementSize, StructSize};
    /// // Bit lists cannot be upgraded
    /// let a = ElementSize::Bit;
    /// let b = ElementSize::InlineComposite(StructSize { data: 1, ptrs: 0 });
    /// assert_eq!(a.upgrade_to(b), None);
    /// ```
    #[inline]
    pub fn upgrade_to(self, other: ElementSize) -> Option<ElementSize> {
        use ElementSize::*;
        match (self, other) {
            // Equal sized elements can obviously work together
            (src, min) if src == min => Some(min),
            // Two inline composites get the max struct size required to fit both
            (InlineComposite(src), InlineComposite(min)) => Some(InlineComposite(src.max(min))),
            // Bit can't be upgraded to anything but itself
            (Bit, _) | (_, Bit) => None,

            // A struct upgrade from a pointer list requires one pointer in the struct
            (InlineComposite(StructSize { ptrs: 0, .. }), Pointer) => None,
            (src @ InlineComposite(_), Pointer) => Some(src),
            // The same condition in reverse
            (Pointer, InlineComposite(StructSize { ptrs: 0, .. })) => None,
            (Pointer, min @ InlineComposite(_)) => Some(min),

            // A struct upgrade from a data list requires one data word in the struct (unless void)
            (src @ InlineComposite(StructSize { data: 0, .. }), Void) => Some(src),
            (InlineComposite(StructSize { data: 0, .. }), _) => None,
            (src @ InlineComposite(_), _) => Some(src),
            // The same condition in reverse
            (Void, min @ InlineComposite(StructSize { data: 0, .. })) => Some(min),
            (_, InlineComposite(StructSize { data: 0, .. })) => None,
            (_, min @ InlineComposite(_)) => Some(min),

            // Everything else is invalid
            _ => None,
        }
    }

    /// Gets the ElementSize of the given static list value type.
    ///
    /// # Example
    ///
    /// ```
    /// use recapn::ptr::ElementSize;
    ///
    /// let size = ElementSize::size_of::<u16>();
    /// assert_eq!(size, ElementSize::TwoBytes);
    /// ```
    #[inline]
    pub const fn size_of<T: ty::ListValue>() -> ElementSize {
        <T as ty::ListValue>::ELEMENT_SIZE
    }

    /// Gets a suitable ElementSize for an empty list of the given list value type.
    ///
    /// This is written to support empty default list readers, specifically empty lists
    /// of any struct, which need an element size for the inline composite elements.
    ///
    /// `AnyStruct` does not implement `ListValue` since it doesn't have a static
    /// list element size, so we use this and specify an empty inline composite element
    /// for empty lists.
    ///
    /// # Example
    ///
    /// ```
    /// use recapn::any::AnyStruct;
    /// use recapn::ptr::{ElementSize, StructSize};
    ///
    /// let size = ElementSize::empty_size_of::<u16>();
    /// assert_eq!(size, ElementSize::TwoBytes);
    ///
    /// let size = ElementSize::empty_size_of::<AnyStruct>();
    /// assert_eq!(size, ElementSize::InlineComposite(StructSize::EMPTY));
    /// ```
    #[inline]
    pub const fn empty_size_of<T: ty::DynListValue>() -> ElementSize {
        PtrElementSize::size_of::<T>().to_element_size()
    }

    /// Convert this `ElementSize` into its `PtrElementSize` counterpart.
    ///
    /// # Example
    ///
    /// ```
    /// use recapn::ptr::{ElementSize, PtrElementSize};
    ///
    /// assert_eq!(ElementSize::TwoBytes.as_ptr_size(), PtrElementSize::TwoBytes);
    /// ```
    #[inline]
    pub const fn as_ptr_size(self) -> PtrElementSize {
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

/// A simplified element size that only indicates the variant of the element.
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum PtrElementSize {
    /// A void element.
    Void = 0,
    /// A bit element.
    Bit = 1,
    /// A byte element.
    Byte = 2,
    /// A two-byte element.
    TwoBytes = 3,
    /// A four-byte element.
    FourBytes = 4,
    /// A eight-byte element.
    EightBytes = 5,
    /// A pointer element.
    Pointer = 6,
    /// An inline composite element.
    InlineComposite = 7,
}

impl From<ElementSize> for PtrElementSize {
    fn from(size: ElementSize) -> Self {
        size.as_ptr_size()
    }
}

impl PtrElementSize {
    /// Gets the `PtrElementSize` of the given static list element type.
    ///
    /// # Example
    ///
    /// ```
    /// use recapn::ptr::PtrElementSize;
    ///
    /// let size = PtrElementSize::size_of::<u16>();
    /// assert_eq!(size, PtrElementSize::TwoBytes);
    /// ```
    ///
    /// This works with elements without a fixed `ElementSize`, like `AnyStruct`.
    ///
    /// ```
    /// # use recapn::ptr::PtrElementSize;
    /// use recapn::any::AnyStruct;
    ///
    /// let size = PtrElementSize::size_of::<AnyStruct>();
    /// assert_eq!(size, PtrElementSize::InlineComposite);
    /// ```
    pub const fn size_of<T: ty::DynListValue>() -> Self {
        <T as ty::DynListValue>::PTR_ELEMENT_SIZE
    }

    /// Converts to a full element size.
    ///
    /// Because an inline composite element size isn't provided, it's assumed to be empty.
    /// This makes this method good for converting to `ElementSize` when you know it's not
    /// an inline composite as it was already handled separately.
    ///
    /// # Example
    ///
    /// ```
    /// use recapn::ptr::{PtrElementSize, ElementSize, StructSize};
    ///
    /// assert_eq!(PtrElementSize::TwoBytes.to_element_size(), ElementSize::TwoBytes);
    /// assert_eq!(
    ///     PtrElementSize::InlineComposite.to_element_size(),
    ///     ElementSize::InlineComposite(StructSize::EMPTY),
    /// );
    ///
    /// // This is useful if you've already handled the InlineComposite case separately
    /// # let size = PtrElementSize::Byte;
    /// let size = if size == PtrElementSize::InlineComposite {
    ///     let struct_size = StructSize { data: 3, ptrs: 2 };
    ///     ElementSize::InlineComposite(struct_size)
    /// } else {
    ///     size.to_element_size()
    /// };
    /// ```
    #[inline]
    pub const fn to_element_size(self) -> ElementSize {
        match self {
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
}

#[repr(transparent)]
#[derive(Clone, Copy, PartialEq)]
struct ListPtr {
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
        self.parts.content_offset()
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
struct FarPtr {
    parts: Parts,
}

impl FarPtr {
    pub fn new(segment: SegmentId, offset: SegmentOffset, double_far: bool) -> Self {
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
struct CapabilityPtr {
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

#[derive(Debug)]
pub enum ExpectedRead {
    Struct,
    List,
    Far,
    Capability,
}

#[derive(Debug)]
pub enum ActualRead {
    Null,
    Struct,
    List,
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

impl core::error::Error for FailedRead {}

/// An error returned when attempting to perform an incompatible upgrade to another list type.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub struct IncompatibleUpgrade {
    /// The original element size we tried to upgrade from.
    pub from: PtrElementSize,
    /// The new element size we tried to upgrade into.
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

impl core::error::Error for IncompatibleUpgrade {}

/// Describes the location of an object in a Cap'n Proto message. This can be used to create
/// pointers directly into a message without needing to navigate through the message tree.
///
/// This is primarily used by code generators to figure out where a default value is located
/// in the schema message that's embeded in the generated code.
pub struct Address {
    /// The segment the object is in.
    pub segment: SegmentId,
    /// The offset into the segment the object is at.
    pub offset: SegmentOffset,
}

impl Address {
    /// The root address of a message.
    pub const ROOT: Self = Self {
        segment: 0,
        offset: SegmentOffset::ZERO,
    };
}

/// A control flow struct used to signal that when an error occurs, the destination pointer
/// should be null.
///
/// A number of things can go wrong when copying from one list or struct to another. You could
/// exceed the nesting limit, the read limit, a pointer could be invalid, capability pointers
/// might've been accidentally set when writing a canonical struct, etc. In these cases, a likely
/// safe default is to not copy the value, and instead just write null at the destination. This
/// struct signals that copying should continue, but should just write null to the destination
/// instead of erroring out.
#[derive(Default, Clone, Copy, Debug)]
pub struct WriteNull;

/// Describes an error handler type that can be used when setting copies of values.
pub trait ErrorHandler {
    /// The error returned. This can be any type, but most provided handlers will either return
    /// [`crate::Error`] or [`core::convert::Infallible`].
    type Error;

    /// Returns a [`ControlFlow`] to signal how to handle the error.
    fn handle_err(&mut self, err: Error) -> ControlFlow<Self::Error, WriteNull>;
}

/// An error handler that simply ignores all errors and always writes null instead.
pub struct IgnoreErrors;
impl ErrorHandler for IgnoreErrors {
    type Error = Infallible;
    #[inline]
    fn handle_err(&mut self, _: Error) -> ControlFlow<Self::Error, WriteNull> {
        ControlFlow::Continue(WriteNull)
    }
}

/// An error handler that panics if any errors occur.
pub struct UnwrapErrors;
impl ErrorHandler for UnwrapErrors {
    type Error = Infallible;
    #[inline]
    fn handle_err(&mut self, err: Error) -> ControlFlow<Self::Error, WriteNull> {
        panic!("failed to build value: {}", err)
    }
}

/// An error handler that breaks after the first error.
pub struct ReturnErrors;
impl ErrorHandler for ReturnErrors {
    type Error = Error;
    #[inline]
    fn handle_err(&mut self, err: Error) -> ControlFlow<Self::Error, WriteNull> {
        ControlFlow::Break(err)
    }
}

impl<F, E> ErrorHandler for F
where
    F: FnMut(Error) -> ControlFlow<E, WriteNull>,
{
    type Error = E;
    #[inline]
    fn handle_err(&mut self, err: Error) -> ControlFlow<E, WriteNull> {
        (self)(err)
    }
}

#[inline]
fn map_control_flow<B>(flow: ControlFlow<B, WriteNull>) -> Result<(), B> {
    match flow {
        ControlFlow::Break(b) => Err(b),
        ControlFlow::Continue(WriteNull) => Ok(()),
    }
}

#[derive(Clone)]
struct SegmentReader<'a> {
    arena: &'a dyn ReadArena,
    segment: SegmentWithId,
}

impl<'a> SegmentReader<'a> {
    pub fn from_source(arena: &'a dyn ReadArena, id: SegmentId) -> Option<Self> {
        arena
            .segment(id)
            .map(|segment| Self::new(arena, id, segment))
    }

    #[inline]
    pub fn new(arena: &'a dyn ReadArena, id: SegmentId, segment: Segment) -> Self {
        Self {
            arena,
            segment: SegmentWithId {
                data: segment.data,
                len: segment.len,
                id,
            },
        }
    }

    /// Gets the start of the segment as a SegmentRef. If the segment is empty, this returns None.
    #[inline]
    pub fn start(&self) -> Option<SegmentRef<'a>> {
        if self.segment.len == SegmentLen::ZERO {
            return None;
        }

        unsafe { Some(SegmentRef::new_unchecked(self.segment.data)) }
    }

    #[inline]
    pub fn start_ptr(&self) -> SegmentPtr<'a> {
        SegmentPtr::new(self.segment.data.as_ptr())
    }

    /// A pointer to just beyond the end of the segment.
    #[inline]
    pub fn end(&self) -> SegmentPtr<'a> {
        SegmentPtr::new(unsafe {
            self.segment
                .data
                .as_ptr()
                .add(self.segment.len.get() as usize)
        })
    }

    #[inline]
    fn contains(&self, ptr: SegmentPtr<'a>) -> bool {
        let start = self.start_ptr();
        let end = self.end();

        start <= ptr && ptr < end
    }

    /// Checks if the pointer is a valid location in this segment, and if so,
    /// returns a ref for it.
    #[inline]
    pub fn try_get(&self, ptr: SegmentPtr<'a>) -> Option<SegmentRef<'a>> {
        if self.contains(ptr) {
            Some(unsafe { ptr.as_ref_unchecked() })
        } else {
            None
        }
    }

    #[inline]
    pub fn try_get_section(&self, start: SegmentPtr<'a>, len: ObjectLen) -> Option<SegmentRef<'a>> {
        let end = start.offset(len);
        if end < start {
            // the pointer wrapped around the address space? should only happen on 32 bit
            return None;
        }

        if end > self.end() {
            // the pointer went beyond the end of the segment
            return None;
        }

        self.try_get(start)
    }

    #[inline]
    pub fn try_get_section_offset(
        &self,
        offset: SegmentOffset,
        len: ObjectLen,
    ) -> Option<SegmentRef<'a>> {
        let start = self.start_ptr().offset(offset);
        self.try_get_section(start, len)
    }

    /// Gets a segment reader for the segment with the specified ID, or None
    /// if the segment doesn't exist.
    #[inline]
    pub fn segment(&self, id: SegmentId) -> Option<SegmentReader<'a>> {
        Self::from_source(self.arena, id)
    }
}

impl fmt::Debug for SegmentReader<'_> {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        self.segment.fmt(f)
    }
}

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

    /// Get the length of the given section of Words but without any null words on the end.
    #[inline]
    pub unsafe fn trim_end_null_section(
        &self,
        ptr: SegmentRef<'a>,
        len: SegmentOffset,
    ) -> SegmentOffset {
        let mut slice = self.section_slice(ptr, len);
        while let [remainder @ .., Word::NULL] = slice {
            slice = remainder;
        }
        SegmentOffset::new(slice.len() as u32).unwrap()
    }

    /// Attempts to read a section in the specified segment. If this is a valid offset, it returns
    /// a pointer to the section and updates the current reader to point at the segment.
    pub fn try_read_object_in(
        &mut self,
        segment: SegmentId,
        offset: SegmentOffset,
        len: ObjectLen,
    ) -> Result<SegmentRef<'a>> {
        let reader = self
            .segment
            .as_ref()
            .and_then(|r| r.segment(segment))
            .ok_or(Error::MissingSegment(segment))?;

        let ptr = reader
            .try_get_section_offset(offset, len)
            .ok_or(Error::PointerOutOfBounds)?;

        if let Some(limiter) = self.limiter {
            if !limiter.try_read(len.get().into()) {
                return Err(Error::ReadLimitExceeded.into());
            }
        }

        self.segment = Some(reader);

        Ok(ptr)
    }

    pub fn try_read_object_from_end_of(
        &self,
        ptr: SegmentPtr<'a>,
        offset: SignedSegmentOffset,
        len: ObjectLen,
    ) -> Result<SegmentRef<'a>> {
        let start = ptr.signed_offset_from_end(offset);
        let new_ptr = if let Some(segment) = &self.segment {
            segment
                .try_get_section(start, len)
                .ok_or(Error::PointerOutOfBounds)?
        } else {
            // SAFETY: the pointer is unchecked, which is unsafe to make anyway, so whoever made
            // the pointer originally upholds safety here
            unsafe { start.as_ref_unchecked() }
        };

        if let Some(limiter) = self.limiter {
            if !limiter.try_read(len.get().into()) {
                return Err(Error::ReadLimitExceeded.into());
            }
        }

        Ok(new_ptr)
    }

    fn location_of(&mut self, src: SegmentRef<'a>) -> Result<Content<'a>> {
        let ptr = *src.as_wire_ptr();
        if let Some(far) = ptr.far_ptr() {
            let segment = far.segment();
            let double_far = far.double_far();
            let landing_pad_size = ObjectLen::new(if double_far { 2 } else { 1 }).unwrap();
            let pad = self.try_read_object_in(segment, far.offset(), landing_pad_size)?;
            let pad_ptr = *pad.as_wire_ptr();

            if double_far {
                // The landing pad is another far pointer. The next pointer is a tag describing our
                // pointed-to object
                // SAFETY: We know from the above that this landing pad is two words, so this is safe
                let tag = unsafe { *pad.offset(1.into()).as_ref_unchecked().as_wire_ptr() };

                let far_ptr = pad_ptr.try_far_ptr()?;
                Ok(Content {
                    ptr: tag,
                    location: Location::DoubleFar {
                        segment: far_ptr.segment(),
                        offset: far_ptr.offset(),
                    },
                })
            } else {
                Ok(Content {
                    ptr: pad_ptr,
                    location: Location::Far { origin: pad.into() },
                })
            }
        } else {
            Ok(Content {
                ptr,
                location: Location::Near { origin: src.into() },
            })
        }
    }

    fn try_read_typed(&mut self, ptr: SegmentRef<'a>) -> Result<TypedContent<'a>> {
        let Content { location, ptr } = self.location_of(ptr)?;
        match ptr.kind() {
            WireKind::Struct => self
                .read_as_struct_content(location, *ptr.struct_ptr().unwrap())
                .map(TypedContent::Struct),
            WireKind::List => self
                .read_as_list_content(location, *ptr.list_ptr().unwrap(), None)
                .map(TypedContent::List),
            WireKind::Other => {
                let cap = ptr.try_cap_ptr()?.capability_index();
                Ok(TypedContent::Capability(cap))
            }
            WireKind::Far => Err(ptr.fail_read(None)),
        }
    }

    fn try_read_struct(&mut self, ptr: SegmentRef<'a>) -> Result<StructContent<'a>> {
        let content = self.location_of(ptr)?;
        self.try_read_as_struct_content(content)
    }

    fn try_read_as_struct_content(&mut self, content: Content<'a>) -> Result<StructContent<'a>> {
        let ptr = *content.ptr.try_struct_ptr()?;
        self.read_as_struct_content(content.location, ptr)
    }

    fn read_as_struct_content(
        &mut self,
        location: Location<'a>,
        struct_ptr: StructPtr,
    ) -> Result<StructContent<'a>> {
        let size = struct_ptr.size();
        let len = size.len();
        let struct_start = match location {
            Location::Near { origin } | Location::Far { origin } => {
                self.try_read_object_from_end_of(origin, struct_ptr.offset(), len)
            }
            Location::DoubleFar { segment, offset } => {
                self.try_read_object_in(segment, offset, len)
            }
        }?;
        Ok(StructContent {
            ptr: struct_start,
            size,
        })
    }

    fn try_read_list(
        &mut self,
        ptr: SegmentRef<'a>,
        expected_element_size: Option<PtrElementSize>,
    ) -> Result<ListContent<'a>> {
        let content = self.location_of(ptr)?;
        self.try_read_as_list_content(content, expected_element_size)
    }

    fn try_read_as_list_content(
        &mut self,
        content: Content<'a>,
        expected: Option<PtrElementSize>,
    ) -> Result<ListContent<'a>> {
        let ptr = *content.ptr.try_list_ptr()?;
        self.read_as_list_content(content.location, ptr, expected)
    }

    fn read_as_list_content(
        &mut self,
        location: Location<'a>,
        list_ptr: ListPtr,
        expected_element_size: Option<PtrElementSize>,
    ) -> Result<ListContent<'a>> {
        let ptr_element = list_ptr.element_size();
        if ptr_element == PtrElementSize::InlineComposite {
            let word_count = list_ptr.element_count().get();
            // add one for the tag pointer, because this could overflow the size of a segment,
            // we just assume on overflow the bounds check fails and is out of bounds.
            let len = ObjectLen::new(word_count + 1).ok_or(Error::PointerOutOfBounds)?;

            let tag_ptr = match location {
                Location::Near { origin } | Location::Far { origin } => {
                    self.try_read_object_from_end_of(origin, list_ptr.offset(), len)
                }
                Location::DoubleFar { segment, offset } => {
                    self.try_read_object_in(segment, offset, len)
                }
            }?;

            let tag = tag_ptr
                .as_wire_ptr()
                .struct_ptr()
                .ok_or(Error::UnsupportedInlineCompositeElementTag)?;

            // move past the tag pointer to get the start of the list
            let first = unsafe { tag_ptr.offset(1.into()).as_ref_unchecked() };

            let element_count = tag.inline_composite_element_count();

            let struct_size = tag.size();
            let element_size = ElementSize::InlineComposite(struct_size);
            if let Some(expected) = expected_element_size {
                if !element_size.upgradable_to(expected) {
                    return Err(fail_upgrade(ptr_element, expected));
                }
            }

            let words_per_element = struct_size.total();

            if u64::from(element_count.get()) * u64::from(words_per_element) > word_count.into() {
                // Make sure the tag reports a struct size that matches the reported word count
                return Err(Error::InlineCompositeOverrun.into());
            }

            if words_per_element == 0 {
                // watch out for zero-sized structs, which can claim to be arbitrarily
                // large without having sent actual data.
                if !self.try_amplified_read(u64::from(element_count.get())) {
                    return Err(Error::ReadLimitExceeded.into());
                }
            }

            Ok(ListContent {
                ptr: first,
                element_size: ElementSize::InlineComposite(struct_size),
                element_count,
            })
        } else {
            let element_size = ptr_element.to_element_size();

            if let Some(expected) = expected_element_size {
                if !element_size.upgradable_to(expected) {
                    return Err(fail_upgrade(ptr_element, expected));
                }
            }

            let element_count = list_ptr.element_count();
            if element_size == ElementSize::Void
                && !self.try_amplified_read(element_count.get() as u64)
            {
                return Err(Error::ReadLimitExceeded.into());
            }

            let element_bits = element_size.bits();
            let word_count =
                Word::round_up_bit_count(u64::from(element_count.get()) * u64::from(element_bits));
            let len = ObjectLen::new(word_count).unwrap();
            let ptr = match location {
                Location::Near { origin } | Location::Far { origin } => {
                    self.try_read_object_from_end_of(origin, list_ptr.offset(), len)
                }
                Location::DoubleFar { segment, offset } => {
                    self.try_read_object_in(segment, offset, len)
                }
            }?;

            Ok(ListContent {
                ptr,
                element_size,
                element_count,
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

/// Describes the location at some object in the message.
#[derive(Debug)]
struct Content<'a> {
    /// The wire value that describes the content
    pub ptr: WirePtr,
    /// A location that can be used to find the content
    pub location: Location<'a>,
}

#[derive(Debug)]
enum Location<'a> {
    /// A near location. The content is in the same segment as the original pointer to the content.
    Near {
        /// The origin an offset can be applied to to find the content in the segment.
        origin: SegmentPtr<'a>,
    },
    /// A far location. The pointer that describes the content is in the same segment as the
    /// content itself.
    Far {
        /// The origin an offset can be applied to to find the content in the segment.
        origin: SegmentPtr<'a>,
    },
    /// A double far location. The pointer that describes the content is in a different segment
    /// than the content itself.
    DoubleFar {
        /// A segment ID for the segment the content is in
        segment: SegmentId,
        /// The offset from the start of the segment to the start of the content
        offset: SegmentOffset,
    },
}

struct StructContent<'a> {
    pub ptr: SegmentRef<'a>,
    pub size: StructSize,
}

impl<'a> StructContent<'a> {
    pub fn data_start(&self) -> NonNull<u8> {
        self.ptr.as_inner().cast()
    }
    pub fn ptrs_start(&self) -> SegmentRef<'a> {
        unsafe { self.ptr.offset(self.size.data.into()).as_ref_unchecked() }
    }
}

struct ListContent<'a> {
    pub ptr: SegmentRef<'a>,
    pub element_size: ElementSize,
    pub element_count: ElementCount,
}

enum TypedContent<'a> {
    Struct(StructContent<'a>),
    List(ListContent<'a>),
    Capability(u32),
}

#[inline]
unsafe fn iter_unchecked(
    place: SegmentRef<'_>,
    offset: SegmentOffset,
) -> impl Iterator<Item = SegmentRef<'_>> {
    let range = 0..offset.get();
    range.into_iter().map(move |offset| unsafe {
        let offset = SegmentOffset::new_unchecked(offset);
        place.offset(offset).as_ref_unchecked()
    })
}

#[inline]
unsafe fn step_by_unchecked(
    place: SegmentRef<'_>,
    offset: SegmentOffset,
    len: SegmentOffset,
) -> impl Iterator<Item = SegmentRef<'_>> {
    assert!(offset
        .get()
        .checked_mul(len.get())
        .is_some_and(|size| size <= SegmentOffset::MAX_VALUE));

    let range = 0..len.get();
    range.into_iter().map(move |idx| unsafe {
        let new_offset = SegmentOffset::new_unchecked(offset.get() * idx);
        place.offset(new_offset).as_ref_unchecked()
    })
}

fn target_size(reader: &ObjectReader, ptr: SegmentRef, nesting_limit: u32) -> Result<MessageSize> {
    let mut reader = reader.clone();
    let target_size = match reader.try_read_typed(ptr)? {
        TypedContent::Struct(content) => {
            let nesting_limit = nesting_limit
                .checked_sub(1)
                .ok_or_else(|| Error::from(Error::NestingLimitExceeded))?;

            total_struct_size(&reader, content, nesting_limit)?
        }
        TypedContent::List(ListContent {
            ptr: start,
            element_size,
            element_count,
        }) => {
            let content_size = MessageSize {
                words: u64::from(element_size.total_words(element_count)),
                caps: 0,
            };
            let targets_size = match element_size {
                ElementSize::InlineComposite(size) => {
                    let nesting_limit = nesting_limit
                        .checked_sub(1)
                        .ok_or_else(|| Error::from(Error::NestingLimitExceeded))?;

                    let mut composites = total_inline_composites_targets_size(
                        &reader,
                        start,
                        element_count,
                        size,
                        nesting_limit,
                    )?;
                    composites.words += 1;
                    composites
                }
                ElementSize::Pointer => {
                    let nesting_limit = nesting_limit
                        .checked_sub(1)
                        .ok_or_else(|| Error::from(Error::NestingLimitExceeded))?;

                    total_ptrs_size(&reader, start, element_count, nesting_limit)?
                }
                _ => MessageSize::default(),
            };
            content_size + targets_size
        }
        TypedContent::Capability(_) => MessageSize { words: 0, caps: 1 },
    };
    Ok(target_size)
}

fn total_struct_size(
    reader: &ObjectReader,
    content: StructContent,
    nesting_limit: u32,
) -> Result<MessageSize> {
    let struct_size = MessageSize {
        words: u64::from(content.size.total()),
        caps: 0,
    };
    let ptrs_total_size = total_ptrs_size(
        reader,
        content.ptrs_start(),
        content.size.ptrs.into(),
        nesting_limit,
    )?;

    Ok(struct_size + ptrs_total_size)
}

fn total_inline_composites_targets_size(
    reader: &ObjectReader,
    start: SegmentRef,
    len: ElementCount,
    size: StructSize,
    nesting_limit: u32,
) -> Result<MessageSize> {
    let mut total_size = MessageSize::default();

    for (_, ptrs) in iter_inline_composites(start, size, len) {
        total_size += total_ptrs_size(reader, ptrs, size.ptrs.into(), nesting_limit)?;
    }

    Ok(total_size)
}

/// Calculate the sizes of pointer targets in a set
fn total_ptrs_size(
    reader: &ObjectReader,
    start: SegmentRef,
    len: SegmentOffset,
    nesting_limit: u32,
) -> Result<MessageSize> {
    let mut total_size = MessageSize { words: 0, caps: 0 };

    let iter = unsafe { iter_unchecked(start, len) };

    for ptr in iter.filter(|w| !w.as_ref().is_null()) {
        total_size += target_size(reader, ptr, nesting_limit)?;
    }

    Ok(total_size)
}

#[inline]
fn trim_end_null_words(mut slice: &[Word]) -> &[Word] {
    while let [remainder @ .., Word::NULL] = slice {
        slice = remainder
    }
    slice
}

#[inline]
fn trim_end_null_bytes(mut slice: &[u8]) -> &[u8] {
    while let [remainder @ .., 0] = slice {
        slice = remainder
    }
    slice
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

    pub const unsafe fn slice_unchecked(slice: &'a [Word]) -> Self {
        Self::new_unchecked(NonNull::new_unchecked(slice.as_ptr().cast_mut()))
    }

    pub fn root(
        arena: &'a dyn ReadArena,
        limiter: Option<&'a ReadLimiter>,
        nesting_limit: u32,
    ) -> Option<Self> {
        let root = SegmentReader::from_source(arena, 0)?;
        Some(Self {
            ptr: root.start()?,
            reader: ObjectReader {
                segment: Some(root),
                limiter,
            },
            table: Empty,
            nesting_limit,
        })
    }

    pub const fn null() -> Self {
        unsafe {
            Self::new_unchecked(NonNull::new_unchecked(
                std::ptr::from_ref(Word::null()).cast_mut(),
            ))
        }
    }
}

impl<'a, T: Table> PtrReader<'a, T> {
    #[inline]
    fn ptr(&self) -> &WirePtr {
        self.ptr.as_wire_ptr()
    }

    #[inline]
    pub fn target_size(&self) -> Result<MessageSize> {
        if self.ptr().is_null() {
            return Ok(MessageSize { words: 0, caps: 0 });
        }

        let nesting_limit = self
            .nesting_limit
            .checked_sub(1)
            .ok_or(Error::NestingLimitExceeded)?;

        target_size(&self.reader, self.ptr, nesting_limit)
    }

    #[inline]
    pub fn ptr_type(&self) -> Result<PtrType> {
        if self.ptr().is_null() {
            return Ok(PtrType::Null);
        }

        let mut reader = self.reader.clone();
        let Content { ptr, .. } = reader.location_of(self.ptr)?;
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
    pub fn equality<T2: Table>(&self, other: &PtrReader<T2>) -> Result<PtrEquality> {
        cmp_ptr(self.ptr, &self.reader, other.ptr, &other.reader)
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

        let nesting_limit = self
            .nesting_limit
            .checked_sub(1)
            .ok_or(Error::NestingLimitExceeded)?;

        let mut reader = self.reader.clone();
        let content = reader.try_read_struct(self.ptr)?;
        let size = content.size;

        Ok(Some(StructReader {
            data_start: content.data_start(),
            ptrs_start: content.ptrs_start(),
            reader,
            table: self.table.clone(),
            nesting_limit,
            data_len: u32::from(size.data) * Word::BYTES as u32,
            ptrs_len: size.ptrs,
        }))
    }

    #[inline]
    pub fn to_list(
        &self,
        expected_element_size: Option<PtrElementSize>,
    ) -> Result<Option<ListReader<'a, T>>> {
        if self.is_null() {
            return Ok(None);
        }

        let Some(nesting_limit) = self.nesting_limit.checked_sub(1) else {
            return Err(Error::NestingLimitExceeded.into());
        };

        let mut reader = self.reader.clone();
        let content = reader.try_read_list(self.ptr, expected_element_size)?;

        Ok(Some(ListReader {
            ptr: content.ptr,
            reader,
            table: self.table.clone(),
            element_count: content.element_count,
            nesting_limit,
            element_size: content.element_size,
        }))
    }

    #[inline]
    pub fn to_blob(&self) -> Result<Option<BlobReader<'a>>> {
        if self.is_null() {
            return Ok(None);
        }

        self.to_blob_inner().map(Some)
    }

    fn to_blob_inner(&self) -> Result<BlobReader<'a>> {
        let mut reader = self.reader.clone();
        let Content { location, ptr } = reader.location_of(self.ptr)?;

        let list_ptr = ptr.try_list_ptr()?;
        let element_size = list_ptr.element_size();
        if element_size != PtrElementSize::Byte {
            return Err(fail_upgrade(element_size, PtrElementSize::Byte));
        }

        let element_count = list_ptr.element_count();
        let len = ObjectLen::new(Word::round_up_byte_count(element_count.into())).unwrap();
        let ptr = match location {
            Location::Near { origin } | Location::Far { origin } => {
                reader.try_read_object_from_end_of(origin, list_ptr.offset(), len)
            }
            Location::DoubleFar { segment, offset } => {
                reader.try_read_object_in(segment, offset, len)
            }
        }?;

        Ok(unsafe { BlobReader::new_unchecked(ptr.as_inner().cast(), element_count) })
    }

    #[inline]
    pub fn try_to_capability_index(&self) -> Result<Option<u32>> {
        let ptr = self.ptr();

        if ptr.is_null() {
            Ok(None)
        } else {
            let i = ptr.try_cap_ptr()?.capability_index();
            Ok(Some(i))
        }
    }
}

impl<'a, T: Table> Capable for PtrReader<'a, T> {
    type Table = T;

    type Imbued = T::Reader;
    type ImbuedWith<T2: Table> = PtrReader<'a, T2>;

    #[inline]
    fn imbued(&self) -> &Self::Imbued {
        &self.table
    }

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
        match self.try_to_capability_index() {
            Ok(Some(i)) => self
                .table
                .extract_cap(i)
                .ok_or_else(|| Error::InvalidCapabilityPointer(i).into())
                .map(Some),
            Ok(None) => Ok(None),
            Err(err) => Err(err),
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
                    T::broken("Read invalid capability pointer")
                }
            } else {
                T::broken("Read non-capability pointer")
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

    pub const unsafe fn slice_unchecked(slice: &'a [Word], size: StructSize) -> Self {
        let ptr = NonNull::new_unchecked(slice.as_ptr().cast_mut());
        Self::new_unchecked(ptr, size)
    }
}

impl<'a, T: Table> StructReader<'a, T> {
    /// Calculate the size of this struct if written in it's canonical form.
    #[inline]
    pub fn canonical_size(&self) -> StructSize {
        let data_section = if self.data_len < 8 {
            // If the data section is less than 8 bytes, we assume it's a struct promotion from a
            // primitive list element. So we do the faster route by comparing the whole thing
            // against 0.
            let data_section = self.data_section();
            if data_section.iter().all(|&b| b == 0) {
                // All the bytes are 0, return an empty data section
                &[]
            } else {
                data_section
            }
        } else {
            // Otherwise, we use trim end null words, which should end up faster since we
            // compare by whole 64 bit blocks
            let trimmed = trim_end_null_words(self.data_section_words());
            Word::slice_to_bytes(trimmed)
        };

        // truncate pointers
        let ptrs_len = trim_end_null_words(self.ptr_section_slice()).len() as u16;

        StructSize {
            data: Word::round_up_byte_count(data_section.len() as u32) as u16,
            ptrs: ptrs_len,
        }
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
        let struct_len = Word::round_up_byte_count(self.data_len) + u32::from(self.ptrs_len);
        let struct_size = MessageSize {
            words: u64::from(struct_len),
            caps: 0,
        };

        let ptrs_targets_size = total_ptrs_size(
            &self.reader,
            self.ptrs_start,
            self.ptrs_len.into(),
            self.nesting_limit,
        )?;

        Ok(struct_size + ptrs_targets_size)
    }

    #[inline]
    pub fn equality<T2: Table>(&self, other: &StructReader<T2>) -> Result<PtrEquality> {
        let our_data = trim_end_null_bytes(self.data_section());
        let their_data = trim_end_null_bytes(other.data_section());
        if our_data != their_data {
            return Ok(PtrEquality::NotEqual);
        }

        cmp_ptr_sections(
            self.ptrs_start,
            self.ptrs_len.into(),
            &self.reader,
            other.ptrs_start,
            other.ptrs_len.into(),
            &other.reader,
        )
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

    /// Returns the data section of this struct
    #[inline]
    pub fn data_section_words(&self) -> &'a [Word] {
        unsafe { core::slice::from_raw_parts(self.data().cast(), (self.data_len as usize) / 8) }
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
            element_size: ElementSize::Pointer,
            nesting_limit: self.nesting_limit,
        }
    }

    /// Reads a field in the data section from the specified slot
    #[inline]
    pub fn data_field<D: Data>(&self, slot: usize) -> D {
        self.data_field_with_default(slot, D::default())
    }

    /// Reads a field in the data section with the specified slot and default value
    #[inline]
    pub fn data_field_with_default<D: Data>(&self, slot: usize, default: D) -> D {
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
    fn imbued(&self) -> &Self::Imbued {
        &self.table
    }

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
    element_size: ElementSize,
}

impl<T: Table> Clone for ListReader<'_, T> {
    fn clone(&self) -> Self {
        Self {
            ptr: self.ptr,
            reader: self.reader.clone(),
            table: self.table.clone(),
            element_count: self.element_count,
            nesting_limit: self.nesting_limit,
            element_size: self.element_size,
        }
    }
}

impl<'a> ListReader<'a, Empty> {
    /// Creates an empty list of the specific element size.
    pub const fn empty(element_size: ElementSize) -> Self {
        unsafe { Self::new_unchecked(NonNull::dangling(), ElementCount::ZERO, element_size) }
    }

    pub const unsafe fn new_unchecked(
        ptr: NonNull<Word>,
        element_count: ElementCount,
        element_size: ElementSize,
    ) -> Self {
        ListReader {
            ptr: SegmentRef::new_unchecked(ptr),
            reader: ObjectReader {
                segment: None,
                limiter: None,
            },
            table: Empty,
            element_count,
            nesting_limit: u32::MAX,
            element_size,
        }
    }

    pub const unsafe fn slice_unchecked(
        slice: &'a [Word],
        element_count: u32,
        element_size: ElementSize,
    ) -> Self {
        let ptr = NonNull::new_unchecked(slice.as_ptr().cast_mut());
        Self::new_unchecked(ptr, u29::new(element_count).unwrap(), element_size)
    }
}

impl<'a, T: Table> ListReader<'a, T> {
    #[inline]
    pub fn len(&self) -> ElementCount {
        self.element_count
    }

    #[inline]
    pub fn element_size(&self) -> ElementSize {
        self.element_size
    }

    #[inline]
    pub fn canonical_element_size(&self) -> ElementSize {
        match self.element_size {
            ElementSize::InlineComposite(size) => {
                ElementSize::InlineComposite(calculate_canonical_inline_composite_size(
                    &self.reader,
                    self.ptr,
                    size,
                    self.element_count,
                ))
            }
            other => other,
        }
    }

    #[inline]
    pub fn total_size(&self) -> Result<MessageSize> {
        let len = self.element_count;
        let mut word_count = u64::from(self.element_size.total_words(len));
        if self.element_size.is_inline_composite() {
            // Add a word for the tag pointer.
            word_count += 1;
        }
        let list_size = MessageSize {
            words: word_count,
            caps: 0,
        };

        match self.element_size {
            ElementSize::Pointer => {
                let target_sizes =
                    total_ptrs_size(&self.reader, self.ptr, len, self.nesting_limit)?;

                Ok(list_size + target_sizes)
            }
            ElementSize::InlineComposite(size) => {
                let target_sizes = total_inline_composites_targets_size(
                    &self.reader,
                    self.ptr,
                    len,
                    size,
                    self.nesting_limit,
                )?;

                Ok(list_size + target_sizes)
            }
            _ => Ok(list_size),
        }
    }

    #[inline]
    pub fn equality<T2: Table>(&self, other: &ListReader<T2>) -> Result<PtrEquality> {
        cmp_list(
            &ListContent {
                ptr: self.ptr,
                element_size: self.element_size,
                element_count: self.element_count,
            },
            &self.reader,
            &ListContent {
                ptr: other.ptr,
                element_size: other.element_size,
                element_count: other.element_count,
            },
            &other.reader,
        )
    }

    #[inline]
    fn ptr(&self) -> *const u8 {
        self.ptr.as_ptr().cast()
    }

    #[inline]
    fn index_to_offset(&self, index: u32) -> usize {
        let step = u64::from(self.element_size.bits());
        let index = u64::from(index);
        let byte_offset = (step * index) / 8;
        byte_offset as usize
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
    pub unsafe fn data_unchecked<D: Data>(&self, index: u32) -> D {
        use core::any::TypeId;

        debug_assert!(index < self.element_count.get(), "index out of bounds");
        debug_assert!({
            let is_inline_composite_with_data = matches!(
                self.element_size,
                ElementSize::InlineComposite(size) if size.data != 0
            );
            self.element_size == D::ELEMENT_SIZE || is_inline_composite_with_data
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
            {
                let is_inline_composite_with_ptr = matches!(
                    self.element_size, ElementSize::InlineComposite(size) if size.ptrs != 0
                );
                self.element_size == ElementSize::Pointer || is_inline_composite_with_ptr
            },
            "attempted to read pointer from a list of something else"
        );

        let base_offset = self.index_to_offset(index);
        let data_offset = if let ElementSize::InlineComposite(size) = self.element_size {
            size.data as usize * Word::BYTES
        } else {
            0
        };

        let offset = base_offset + data_offset;
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
            self.element_size != ElementSize::Bit,
            "attempted to read struct from bit list"
        );

        let (data_len, ptrs_len) = self.element_size.bytes_and_ptrs();
        let offset = self.index_to_offset(index);
        let struct_start = self.ptr().add(offset);
        let struct_data = struct_start;
        let struct_ptrs = struct_start.add(data_len as usize).cast::<Word>();

        StructReader {
            data_start: NonNull::new_unchecked(struct_data.cast_mut()),
            ptrs_start: SegmentRef::new_unchecked(NonNull::new_unchecked(struct_ptrs.cast_mut())),
            reader: self.reader.clone(),
            table: self.table.clone(),
            nesting_limit: self.nesting_limit.saturating_sub(1),
            data_len,
            ptrs_len,
        }
    }
}

impl<'a, T: Table> Capable for ListReader<'a, T> {
    type Table = T;

    type Imbued = T::Reader;
    type ImbuedWith<T2: Table> = ListReader<'a, T2>;

    #[inline]
    fn imbued(&self) -> &Self::Imbued {
        &self.table
    }

    #[inline]
    fn imbue_release<T2: Table>(self, new_table: T2::Reader) -> (Self::ImbuedWith<T2>, T::Reader) {
        let old_table = self.table;
        let ptr = ListReader {
            ptr: self.ptr,
            reader: self.reader,
            table: new_table,
            element_count: self.element_count,
            nesting_limit: self.nesting_limit,
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

#[derive(Clone, Copy)]
pub struct BlobReader<'a> {
    slice: &'a [u8],
}

impl<'a> BlobReader<'a> {
    #[inline]
    pub(crate) const fn new(slice: &'a [u8]) -> Option<Self> {
        if slice.len() < ElementCount::MAX_VALUE as usize {
            Some(Self { slice })
        } else {
            None
        }
    }

    pub(crate) const unsafe fn new_unchecked(ptr: NonNull<u8>, len: ElementCount) -> Self {
        Self {
            slice: std::slice::from_raw_parts(ptr.as_ptr(), len.get() as usize),
        }
    }

    pub const fn empty() -> Self {
        Self { slice: &[] }
    }

    pub const fn data(&self) -> NonNull<u8> {
        unsafe { NonNull::new_unchecked(self.slice.as_ptr().cast_mut()) }
    }

    pub const fn len(&self) -> ElementCount {
        unsafe { ElementCount::new_unchecked(self.slice.len() as _) }
    }

    pub const fn as_slice(&self) -> &'a [u8] {
        self.slice
    }
}

const LANDING_PAD_LEN: AllocLen = AllocLen::new(2).unwrap();

pub(crate) enum OrphanObject<'a> {
    Struct {
        location: SegmentRef<'a>,
        size: StructSize,
    },
    List {
        location: SegmentRef<'a>,
        element_size: PtrElementSize,
        element_count: ElementCount,
    },
    Far {
        location: SegmentRef<'a>,
        double_far: bool,
    },
    Capability(u32),
}

/// A helper struct for building objects in a message
#[derive(Clone)]
pub(crate) struct ObjectBuilder<'a> {
    segment: &'a ArenaSegment,
    arena: &'a dyn BuildArena,
}

impl Debug for ObjectBuilder<'_> {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        f.debug_struct("ObjectBuilder")
            .field("segment", self.segment)
            .field("arena", &"...")
            .finish()
    }
}

impl<'a> ObjectBuilder<'a> {
    #[inline]
    pub fn new(segment: &'a ArenaSegment, arena: &'a dyn BuildArena) -> Self {
        Self { segment, arena }
    }

    #[inline]
    pub fn as_reader(&self) -> ObjectReader<'_> {
        ObjectReader {
            segment: Some(SegmentReader {
                segment: self.segment.segment_with_id(),
                arena: self.arena.as_read_arena(),
            }),
            limiter: None,
        }
    }

    #[inline]
    pub fn id(&self) -> SegmentId {
        self.segment.id()
    }

    #[inline]
    pub fn contains(&self, r: SegmentPtr<'a>) -> bool {
        self.segment.segment().to_ptr_range().contains(&r.as_ptr())
    }

    #[inline]
    pub fn check_ref(&self, r: SegmentPtr<'a>) -> SegmentRef<'a> {
        debug_assert!(self.contains(r));
        unsafe { r.as_ref_unchecked() }
    }

    #[inline]
    pub fn contains_section(&self, r: SegmentRef<'a>, offset: SegmentOffset) -> bool {
        self.contains_range(r.into(), r.offset(offset))
    }

    #[inline]
    pub fn contains_range(&self, start: SegmentPtr<'a>, end: SegmentPtr<'a>) -> bool {
        let segment_start = self.start().as_segment_ptr();
        let segment_end = self.end();

        segment_start <= start && start <= end && end <= segment_end
    }

    #[inline]
    pub fn at_offset(&self, offset: SegmentOffset) -> SegmentRef<'a> {
        let r = self.start().offset(offset);
        debug_assert!(self.contains(r));
        unsafe { r.as_ref_unchecked() }
    }

    /// Gets the start of the segment as a SegmentRef
    #[inline]
    pub fn start(&self) -> SegmentRef<'a> {
        self.segment.start()
    }

    #[inline]
    pub fn end(&self) -> SegmentPtr<'a> {
        self.start().offset(self.segment.used_len())
    }

    #[inline]
    fn offset_from(&self, from: SegmentPtr<'a>, to: SegmentPtr<'a>) -> SignedSegmentOffset {
        debug_assert!(self.contains_range(from, to));
        unsafe {
            let offset = to.as_ptr().offset_from(from.as_ptr());
            SignedSegmentOffset::new_unchecked(offset as i32)
        }
    }

    /// Calculates the segment offset from the the end of `from` to the ptr `to`
    #[inline]
    pub fn offset_from_end_of(
        &self,
        from: SegmentPtr<'a>,
        to: SegmentPtr<'a>,
    ) -> SignedSegmentOffset {
        self.offset_from(from.offset(1u16.into()), to)
    }

    #[inline]
    pub fn offset_from_start(&self, to: SegmentPtr<'a>) -> SegmentOffset {
        debug_assert!(self.contains(to));

        let signed_offset = self.offset_from(self.start().into(), to);
        SegmentOffset::new(signed_offset.get() as u32).expect("offset from start was negative!")
    }

    /// Allocates space of the given size _somewhere_ in the message.
    #[inline]
    pub fn alloc(&self, size: AllocLen) -> Option<(SegmentRef<'a>, ObjectBuilder<'a>)> {
        if let Some(word) = self.alloc_in_segment(size) {
            Some((word, self.clone()))
        } else {
            let (place, segment) = self.arena.alloc(size)?;
            let builder = ObjectBuilder {
                segment,
                arena: self.arena,
            };
            let rf = builder.at_offset(place);
            Some((rf, builder))
        }
    }

    #[inline]
    pub fn alloc_double_far_landing_pad(
        &self,
    ) -> Option<(SegmentRef<'a>, SegmentRef<'a>, ObjectBuilder<'a>)> {
        let (pad, segment) = self.alloc(LANDING_PAD_LEN)?;
        let tag = unsafe { pad.offset(1u16.into()).as_ref_unchecked() };
        Some((pad, tag, segment))
    }

    /// Attempt to allocate the given space in this segment.
    #[inline]
    pub fn alloc_in_segment(&self, size: AllocLen) -> Option<SegmentRef<'a>> {
        let offset = self.segment.try_use_len(size)?;
        Some(self.at_offset(offset))
    }

    #[inline]
    pub fn is_same_message(&self, other: &ObjectBuilder) -> bool {
        ptr::addr_eq(self.arena, other.arena)
    }

    #[inline]
    pub fn is_same_segment(&self, other: &ObjectBuilder) -> bool {
        self.id() == other.id()
    }

    #[inline]
    fn locate(self, ptr: SegmentRef<'a>) -> (Content<'a>, Self) {
        self.locate_ptr(ptr, *ptr.as_wire_ptr())
    }

    /// Locate content based on the given WirePtr "set" in the given place.
    ///
    /// This allows content to be relocated if the set pointer value in the place is different
    /// than the other value.
    #[inline]
    fn locate_ptr(self, place: SegmentRef<'a>, ptr: WirePtr) -> (Content<'a>, Self) {
        if let Some(far) = ptr.far_ptr() {
            let segment = far.segment();
            let double_far = far.double_far();
            let (ptr, builder) = self
                .build_object_in(segment, far.offset())
                .expect("far pointer cannot point to a read-only segment");

            let content = if double_far {
                let tag = unsafe { ptr.offset(1.into()).as_ref_unchecked() };
                let far_ptr = ptr
                    .as_wire_ptr()
                    .try_far_ptr()
                    .expect("malformed double far in builder");
                Content {
                    ptr: *tag.as_wire_ptr(),
                    location: Location::DoubleFar {
                        segment: far_ptr.segment(),
                        offset: far_ptr.offset(),
                    },
                }
            } else {
                Content {
                    ptr: *ptr.as_wire_ptr(),
                    location: Location::Far { origin: ptr.into() },
                }
            };

            (content, builder)
        } else {
            let content = Content {
                ptr,
                location: Location::Near {
                    origin: place.into(),
                },
            };

            (content, self)
        }
    }

    /// Clear the given pointer. If it's a far pointer, clear the landing pads. This function ignores read errors.
    #[inline]
    pub fn clear_ptrs(&self, ptr: SegmentRef<'a>) {
        if let Some(far) = ptr.as_wire_ptr().far_ptr() {
            let result = self.build_object_in(far.segment(), far.offset());
            let Some((pad_ptr, builder)) = result else {
                return;
            };

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
        debug_assert!(self.contains_section(start, offset));

        unsafe { ptr::write_bytes(start.as_ptr_mut(), 0, offset.get() as usize) }
    }

    #[inline]
    fn set_ptr(&self, ptr: SegmentRef<'a>, value: impl Into<WirePtr>) {
        debug_assert!(
            self.contains(ptr.into()),
            "segment does not contain ptr: {:?}; segment: {:?}",
            ptr.as_ptr(),
            &self.segment
        );

        let value = value.into();
        unsafe { *ptr.as_ptr_mut() = Word::from(value) }
    }

    #[inline]
    pub unsafe fn section_slice_mut(
        &self,
        start: SegmentRef<'a>,
        offset: SegmentOffset,
    ) -> &'a mut [Word] {
        debug_assert!(self.contains_section(start, offset));

        slice::from_raw_parts_mut(start.as_ptr_mut(), offset.get() as usize)
    }

    /// Returns an object pointer and builder based on the given wire pointer and offset
    #[inline]
    pub fn build_object_from_end_of<'b>(
        &self,
        ptr: SegmentPtr<'b>,
        offset: SignedSegmentOffset,
    ) -> (SegmentRef<'a>, Self) {
        let ptr = SegmentPtr::new(ptr.as_ptr_mut());
        let start = self.check_ref(ptr.signed_offset_from_end(offset));
        (start, self.clone())
    }

    #[inline]
    pub fn build_segment(&self, segment: SegmentId) -> Option<Self> {
        let segment = self.arena.segment(segment)?;
        let builder = ObjectBuilder {
            segment,
            arena: self.arena,
        };
        Some(builder)
    }

    /// Attempts to get an object builder for an object in the specified segment, or None if the
    /// segment is read-only.
    #[inline]
    pub fn build_object_in(
        &self,
        segment: SegmentId,
        offset: SegmentOffset,
    ) -> Option<(SegmentRef<'a>, Self)> {
        let builder = self.build_segment(segment)?;
        let place = builder.at_offset(offset);
        Some((place, builder))
    }

    #[inline]
    pub fn build_object_from_end_in<'b>(
        &self,
        segment: SegmentId,
        ptr: SegmentPtr<'b>,
        offset: SignedSegmentOffset,
    ) -> Option<(SegmentRef<'a>, Self)> {
        let ptr = SegmentPtr::new(ptr.as_ptr_mut());
        let builder = self.build_segment(segment)?;
        let start = self.check_ref(ptr.signed_offset_from_end(offset));
        Some((start, builder))
    }

    #[inline]
    pub fn alloc_in_arena(&self, size: AllocLen) -> Option<(SegmentRef<'a>, Self)> {
        let (offset, segment) = self.arena.alloc(size)?;
        let builder = ObjectBuilder {
            segment,
            arena: self.arena,
        };
        let rf = builder.at_offset(offset);
        Some((rf, builder))
    }

    /// Allocates a struct in the message, then configures the given pointer to point to it.
    #[inline]
    pub fn alloc_struct(
        &self,
        ptr: SegmentRef<'a>,
        size: StructSize,
    ) -> Result<(SegmentRef<'a>, Self)> {
        let Some(len) = AllocLen::new(size.total()) else {
            self.set_ptr(ptr, StructPtr::EMPTY);
            return Ok((ptr, self.clone()));
        };

        let (start, segment) = if let Some(start) = self.alloc_in_segment(len) {
            // the struct can be allocated in this segment, nice
            let offset = self.offset_from_end_of(ptr.into(), start.into());
            self.set_ptr(ptr, StructPtr::new(offset, size));

            (start, self.clone())
        } else {
            // the struct can't be allocated in this segment, so we need to allocate for
            // a struct + landing pad
            // this unwrap is ok since we know that u16::MAX + u16::MAX + 1 can never be
            // larger than a segment
            let len_with_pad = AllocLen::new(size.total() + 1).unwrap();
            let (pad, object_segment) = self
                .alloc_in_arena(len_with_pad)
                .ok_or(Error::AllocFailed(len_with_pad))?;
            let start = unsafe { pad.offset(1u16.into()).as_ref_unchecked() };

            // our pad is the actual pointer to the content
            let offset_to_pad = object_segment.offset_from_start(pad.into());
            self.set_ptr(ptr, FarPtr::new(object_segment.id(), offset_to_pad, false));
            object_segment.set_ptr(pad, StructPtr::new(0i16.into(), size));

            (start, object_segment)
        };

        Ok((start, segment))
    }

    #[inline]
    pub fn alloc_struct_orphan(&self, size: StructSize) -> Result<(OrphanObject<'a>, Self)> {
        let Some(len) = AllocLen::new(size.total()) else {
            let object = OrphanObject::Struct {
                location: self.segment.start(),
                size: StructSize::EMPTY,
            };
            return Ok((object, self.clone()));
        };

        let (start, segment) = self.alloc(len).ok_or(Error::AllocFailed(len))?;
        Ok((
            OrphanObject::Struct {
                location: start,
                size,
            },
            segment,
        ))
    }

    #[inline]
    fn build_struct(&self, origin: SegmentRef<'a>) -> Result<(StructContent<'a>, Self)> {
        let (Content { ptr, location }, builder) = self.clone().locate(origin);
        let struct_ptr = *ptr.try_struct_ptr()?;

        let (start, builder) = match location {
            Location::Near { origin } | Location::Far { origin } => {
                builder.build_object_from_end_of(origin, struct_ptr.offset())
            }
            Location::DoubleFar { segment, offset } => {
                match builder.build_object_in(segment, offset) {
                    Some(b) => b,
                    None => return Err(Error::WritingNotAllowed.into()),
                }
            }
        };

        let content = StructContent {
            ptr: start,
            size: struct_ptr.size(),
        };

        Ok((content, builder))
    }

    #[inline]
    fn clear_struct_section(&self, content: StructContent<'a>) {
        self.clear_section(content.ptr, content.size.len());
    }

    /// Allocates a list in the message, configures the given pointer to point to it, and returns
    /// a pointer to the start of the list content with an associated object length and builder.
    #[inline]
    pub fn alloc_list(
        &self,
        ptr: SegmentRef<'a>,
        element_size: ElementSize,
        element_count: ElementCount,
    ) -> Result<(SegmentRef<'a>, ObjectLen, Self)> {
        let is_struct_list = element_size.is_inline_composite();
        let tag_size = if is_struct_list { 1 } else { 0 };
        let size = element_size.total_words(element_count);
        let total_size = size + tag_size;
        if total_size == 0 {
            self.set_ptr(
                ptr,
                ListPtr::new(0u16.into(), element_size.into(), element_count),
            );
            // since the list has no size, any pointer is valid here, even one beyond the end of the segment
            return Ok((
                unsafe { ptr.offset(1.into()).as_ref_unchecked() },
                ObjectLen::ZERO,
                self.clone(),
            ));
        }
        let len = AllocLen::new(total_size).ok_or(Error::AllocTooLarge)?;

        let list_ptr_element_count = if is_struct_list {
            SegmentOffset::new(size).unwrap()
        } else {
            element_count
        };
        let (start, segment) = if let Some(alloc) = self.alloc_in_segment(len) {
            // we were able to allocate in our current segment, nice

            let offset = self.offset_from_end_of(ptr.into(), alloc.into());
            self.set_ptr(
                ptr,
                ListPtr::new(offset, element_size.into(), list_ptr_element_count),
            );

            (alloc, self.clone())
        } else if let Some(len_with_landing_pad) = AllocLen::new(total_size + 1) {
            // we couldn't allocate in this segment, but we can probably allocate a new list
            // somewhere else with a far landing pad

            let (pad, segment) = self
                .alloc_in_arena(len_with_landing_pad)
                .ok_or(Error::AllocFailed(len_with_landing_pad))?;
            let start = unsafe { pad.offset(1u16.into()).as_ref_unchecked() };
            let offset_to_pad = segment.offset_from_start(pad.into());

            self.set_ptr(ptr, FarPtr::new(segment.id(), offset_to_pad, false));
            segment.set_ptr(
                pad,
                ListPtr::new(0i16.into(), element_size.into(), list_ptr_element_count),
            );

            (start, segment)
        } else {
            // ok the list is just too big for even one more word to be added to its alloc size
            // so we're going to have to make a separate double far landing pad

            self.alloc_segment_of_list(ptr, element_size, element_count)?
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

        Ok((start, len.into(), segment))
    }

    /// Allocate a list where the total size in words is equal to that of a segment.
    #[cold]
    fn alloc_segment_of_list(
        &self,
        ptr: SegmentRef<'a>,
        element_size: ElementSize,
        element_count: ElementCount,
    ) -> Result<(SegmentRef<'a>, ObjectBuilder<'a>)> {
        // try to allocate the landing pad _first_ since it likely will fit in either this
        // segment or the last segment in the message.
        // if the allocations were flipped, we'd probably allocate the pad in a completely new
        // segment, which is just a waste of space.

        let Some((pad, tag, pad_builder)) = self.alloc_double_far_landing_pad() else {
            return Err(Error::AllocFailed(LANDING_PAD_LEN).into());
        };

        let (start, list_segment) = self
            .alloc_in_arena(AllocLen::MAX)
            .ok_or(Error::AllocFailed(AllocLen::MAX))?;

        let offset_to_pad = pad_builder.offset_from_start(pad.into());
        self.set_ptr(ptr, FarPtr::new(pad_builder.id(), offset_to_pad, true));

        // I'm pretty sure if we can't add even one more word to a list allocation then the list
        // takes up the whole segment, which means this has to be 0, but I'm not taking chances.
        // Change it if you think it actually really matters.
        let offset_to_list = list_segment.offset_from_start(start.into());
        pad_builder.set_ptr(pad, FarPtr::new(list_segment.id(), offset_to_list, false));
        pad_builder.set_ptr(
            tag,
            ListPtr::new(0i16.into(), element_size.into(), element_count),
        );

        Ok((start, list_segment))
    }

    /// Like alloc_list, but doesn't configure a pointer to point at the resulting data. Instead
    /// an OrphanObject is returned describing the content.
    ///
    /// Because it allocates an orphan it also isn't as optimized for forward traversal. Assuming
    /// the orphan could be adopted anywhere, it doesn't make sense to allocate a far landing pad
    /// if we fail to allocate in this segment. Thus, if we don't adopt the list in this segment
    /// a landing pad will be allocated _after_ the list itself.
    pub fn alloc_list_orphan(
        &self,
        element_size: ElementSize,
        element_count: ElementCount,
    ) -> Result<(OrphanObject<'a>, ObjectBuilder<'a>)> {
        let total_size = element_size.object_words(element_count);
        if total_size == 0 {
            let object = OrphanObject::List {
                location: self.segment.start(),
                element_size: element_size.into(),
                element_count,
            };
            return Ok((object, self.clone()));
        }

        let len = AllocLen::new(total_size).ok_or(Error::AllocTooLarge)?;
        let (alloc_start, segment) = self.alloc(len).ok_or(Error::AllocFailed(len))?;
        if let ElementSize::InlineComposite(size) = element_size {
            segment.set_ptr(
                alloc_start,
                StructPtr::new_inline_composite_tag(element_count, size),
            );
        }

        let object = OrphanObject::List {
            location: alloc_start,
            element_size: element_size.into(),
            element_count,
        };
        Ok((object, segment))
    }

    #[inline]
    fn build_list(&self, origin: SegmentRef<'a>) -> Result<(ListContent<'a>, Self)> {
        let (Content { ptr, location }, builder) = self.clone().locate(origin);
        let list_ptr = *ptr.try_list_ptr()?;

        let (start, builder) = match location {
            Location::Near { origin } | Location::Far { origin } => {
                builder.build_object_from_end_of(origin, list_ptr.offset())
            }
            Location::DoubleFar { segment, offset } => {
                match builder.build_object_in(segment, offset) {
                    Some(b) => b,
                    None => return Err(Error::WritingNotAllowed.into()),
                }
            }
        };

        let element_size = list_ptr.element_size();
        let content = match element_size {
            PtrElementSize::InlineComposite => {
                let tag = *start
                    .as_wire_ptr()
                    .struct_ptr()
                    .expect("inline composite list with non-struct elements is not supported");

                let list_start = unsafe { start.offset(1.into()).as_ref_unchecked() };
                let size = tag.size();

                ListContent {
                    ptr: list_start,
                    element_size: ElementSize::InlineComposite(size),
                    element_count: tag.inline_composite_element_count(),
                }
            }
            _ => ListContent {
                ptr: start,
                element_count: list_ptr.element_count(),
                element_size: element_size.to_element_size(),
            },
        };

        Ok((content, builder))
    }

    #[inline]
    fn clear_list_section(&self, content: ListContent<'a>) {
        let Some(size) = AllocLen::new(content.element_size.object_words(content.element_count))
        else {
            return;
        };
        let obj_start = if content.element_size.is_inline_composite() {
            // In this case the content pointer is just after the start of the list object.
            // So we have to subtract 1 to get the tag pointer.
            let obj_start = content.ptr.signed_offset(SignedSegmentOffset::from(-1i16));
            self.check_ref(obj_start)
        } else {
            content.ptr
        };
        self.clear_section(obj_start, size.into());
    }
}

/// Controls how a type is copied into a message.
///
/// This allows you to control struct or list promotions when copying from an existing value.
/// In other Cap'n Proto implementations, promotions must be done *after* a struct or list
/// has been copied. In this crate, you can promote a struct or list during the initial copy,
/// saving some time and message space.
#[derive(Clone, Copy, PartialEq, Eq, Debug)]
pub enum CopySize<T> {
    /// Size is copied from the input value's size.
    FromValue,
    /// Size is determined by the canonical size of the value.
    ///
    /// Canonical size is primarily a feature for structs. Null words in the data and pointer
    /// sections of the struct are removed. For lists of structs, the largest canonical struct
    /// size is used for the whole list. Capability pointers are not allowed in canonical values.
    Canonical,
    /// The size is explicitly set to at-least this value. We use this when setting a pointer to
    /// a copy of a value so that we can build it.
    ///
    /// In the case of lists, this serves to indicate what element size we're going to upgrade to,
    /// which means the set function can fail if an attempt is made to perform an invalid upgrade.
    Minimum(T),
}

impl<T> CopySize<T> {
    pub fn is_canonical(&self) -> bool {
        matches!(self, CopySize::Canonical)
    }
}

impl CopySize<StructSize> {
    /// Gets the required builder size to copy the struct in the given form.
    pub fn builder_size(&self, value: &StructReader<'_, impl Table>) -> StructSize {
        match self {
            CopySize::Canonical => value.canonical_size(),
            CopySize::FromValue => value.size(),
            CopySize::Minimum(size) => value.size().max(*size),
        }
    }
}

impl CopySize<ElementSize> {
    /// Gets the required builder element size to copy the list.
    pub fn builder_size(
        &self,
        value: &ListReader<'_, impl Table>,
    ) -> Result<ElementSize, IncompatibleUpgrade> {
        match self {
            CopySize::Canonical => Ok(value.canonical_element_size()),
            CopySize::FromValue => Ok(value.element_size()),
            CopySize::Minimum(size) => {
                let src_size = value.element_size();
                src_size
                    .upgrade_to(*size)
                    .ok_or_else(|| IncompatibleUpgrade {
                        from: src_size.as_ptr_size(),
                        to: size.as_ptr_size(),
                    })
            }
        }
    }
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
    pub(crate) fn root(segment: &'a ArenaSegment, arena: &'a dyn BuildArena) -> Self {
        let ptr = unsafe { SegmentRef::new_unchecked(segment.segment().data) };
        Self {
            builder: ObjectBuilder { segment, arena },
            ptr,
            table: Empty,
        }
    }
}

impl<'a, T: Table> PtrBuilder<'a, T> {
    #[inline]
    pub fn is_null(&self) -> bool {
        *self.ptr.as_wire_ptr() == WirePtr::NULL
    }

    #[inline]
    pub fn as_reader(&self) -> PtrReader<'_, T> {
        PtrReader {
            ptr: self.ptr,
            reader: self.builder.as_reader(),
            table: self.table.as_reader(),
            nesting_limit: u32::MAX,
        }
    }

    #[inline]
    pub fn address(&self) -> Address {
        let b = &self.builder;
        let segment = b.segment.id();
        let offset = b.offset_from_start(self.ptr.into());
        Address { segment, offset }
    }

    /// Borrows the pointer, rather than consuming it.
    #[inline]
    pub fn by_ref(&mut self) -> PtrBuilder<'_, T> {
        unsafe { self.clone() }
    }

    /// Clone the pointer builder. This returns a new builder to the same pointer with the
    /// same lifetime.
    ///
    /// This can be used to get around some limitations of the borrowing system put in place by the
    /// library, but can also be used to break some of the invariants the library expects you to
    /// uphold. Use this wisely. While you *mostly* can't do anything that's technically unsafe
    /// with two copies of the same pointer builder, you can commit spooky action at a distance.
    /// For example
    ///
    ///  * You can produce a builder for some content derived from this pointer, build it,
    ///    and while you're still holding the builder, clear the whole thing on accident
    ///    by attempting to build something else with the cloned builder.
    ///  * You can write to two different union variants at the same time which the library
    ///    assumes you won't do.
    ///
    /// You can however commit a mut borrow violation by reading the same blob builder twice.
    /// Both blob builders have safe accessors that interpret the blob as a mut slice of `u8`.
    /// Those accessors are safe under the rules of the API lifetimes, but can violate the
    /// borrow rules through this function.
    ///
    /// It's not all unsafe though. For example you can use a clone to split a borrow and mutate
    /// two distinct fields in a struct or list at the same time as long as you make sure to never
    /// mutate the same field twice.
    #[inline]
    pub unsafe fn clone(&self) -> PtrBuilder<'a, T> {
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

        let (Content { ptr, .. }, _) = self.builder.clone().locate(self.ptr);
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

    #[inline]
    pub fn target_size(&self) -> MessageSize {
        self.as_reader().target_size().unwrap()
    }

    #[inline]
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

        match self.to_struct_mut_inner(expected_size) {
            Ok(s) => Ok(s),
            Err(err) => Err((Some(err), self)),
        }
    }

    fn to_struct_mut_inner(
        &self,
        expected_size: Option<StructSize>,
    ) -> Result<StructBuilder<'a, T>> {
        let (existing_struct, existing_builder) = self.builder.build_struct(self.ptr)?;
        let existing_size = existing_struct.size;

        // Take the expected size, filter out any smaller sizes that already fit in the existing
        // struct, and create a new size that we're going to promote to.
        let needed_size = expected_size
            .filter(|e| !e.fits_inside(existing_size))
            .map(|e| e.max(existing_size));

        let Some(promotion_size) = needed_size else {
            return Ok(StructBuilder::<T> {
                builder: existing_builder,
                table: self.table.clone(),
                data_start: existing_struct.ptr,
                ptrs_start: existing_struct.ptrs_start(),
                data_len: u32::from(existing_size.data) * Word::BYTES as u32,
                ptrs_len: existing_size.ptrs,
            });
        };

        // The space allocated for this struct is too small.  Unlike with readers, we can't just
        // run with it and do bounds checks at access time, because how would we handle writes?
        // Instead, we have to copy the struct to a new space now.

        self.builder.clear_ptrs(self.ptr);
        let (new_start, new_builder) = self.builder.alloc_struct(self.ptr, promotion_size)?;

        let new_ptrs_start = unsafe {
            new_start
                .offset(promotion_size.data.into())
                .as_ref_unchecked()
        };

        promote_struct(
            existing_struct.ptr,
            existing_size,
            &existing_builder,
            new_start,
            new_ptrs_start,
            &new_builder,
        )?;

        // Promotion successful, now we can clear the old struct in one call
        existing_builder.clear_section(existing_struct.ptr, existing_size.len());

        Ok(StructBuilder::<T> {
            builder: new_builder,
            table: self.table.clone(),
            data_start: new_start,
            ptrs_start: new_ptrs_start,
            data_len: u32::from(promotion_size.data) * Word::BYTES as u32,
            ptrs_len: promotion_size.ptrs,
        })
    }

    /// Initializes the pointer as a struct with the specified size.
    #[inline]
    pub fn init_struct(self, size: StructSize) -> StructBuilder<'a, T> {
        self.try_init_struct(size)
            .map_err(|(err, _)| err)
            .expect("failed to alloc struct")
    }

    #[inline]
    pub fn try_init_struct(
        mut self,
        size: StructSize,
    ) -> BuildResult<StructBuilder<'a, T>, Self, Error> {
        self.clear();
        match self.try_init_struct_zeroed(size) {
            Ok(s) => Ok(s),
            Err(err) => Err((err, self)),
        }
    }

    #[inline]
    fn try_init_struct_zeroed(&mut self, size: StructSize) -> Result<StructBuilder<'a, T>> {
        let (struct_start, builder) = self.builder.alloc_struct(self.ptr, size)?;
        let ptrs_start = unsafe { struct_start.offset(size.data.into()).as_ref_unchecked() };

        Ok(StructBuilder::<T> {
            builder,
            data_start: struct_start,
            ptrs_start,
            table: self.table.clone(),
            data_len: u32::from(size.data) * Word::BYTES as u32,
            ptrs_len: size.ptrs,
        })
    }

    /// Gets a builder for a struct from this pointer of the specified size.
    #[inline]
    pub fn to_struct_mut_or_init(self, size: StructSize) -> StructBuilder<'a, T> {
        match self.to_struct_mut(Some(size)) {
            Ok(builder) => builder,
            Err((_, this)) => this.init_struct(size),
        }
    }

    /// Sets the pointer to a struct copied from the specified reader.
    ///
    /// When calling to initialize a builder with a set size, pass `CopySize::Minimum` with the
    /// size of the struct type the copy is being made for. This will handle properly promoting
    /// the struct's size to be at least as large as the expected builder.
    ///
    /// When calling to simply copy a value directly without canonicalization, use
    /// `CopySize::FromValue`.
    #[inline]
    pub fn try_set_struct<E, U>(
        &mut self,
        value: &StructReader<U>,
        size: CopySize<StructSize>,
        mut err_handler: E,
    ) -> Result<(), E::Error>
    where
        U: InsertableInto<T>,
        E: ErrorHandler,
    {
        let canonical = size.is_canonical();
        let size = size.builder_size(value);

        self.clear();
        match self.try_init_struct_zeroed(size) {
            Ok(mut b) => b.try_copy_with_caveats(value, canonical, err_handler),
            Err(err) => match err_handler.handle_err(err) {
                ControlFlow::Continue(WriteNull) => Ok(()),
                ControlFlow::Break(err) => Err(err),
            },
        }
    }

    #[inline]
    pub fn set_struct(
        &mut self,
        value: &StructReader<impl InsertableInto<T>>,
        size: CopySize<StructSize>,
    ) {
        self.try_set_struct(value, size, IgnoreErrors).unwrap()
    }

    #[inline]
    pub fn to_list(
        &self,
        expected_element_size: Option<PtrElementSize>,
    ) -> Result<Option<ListReader<T>>> {
        self.as_reader().to_list(expected_element_size)
    }

    /// Interprets the pointer as a list. If the list is expected to have some element size,
    /// the list may be promoted.
    ///
    /// If an error occurs while the list is being promoted, the pointer is cleared.
    #[inline]
    pub fn to_list_mut(
        self,
        expected_element_size: Option<ElementSize>,
    ) -> BuildResult<ListBuilder<'a, T>, Self> {
        if self.is_null() {
            return Err((None, self));
        }

        match self.to_list_mut_inner(expected_element_size) {
            Ok(b) => Ok(b),
            Err(err) => Err((Some(err), self)),
        }
    }

    fn to_list_mut_inner(
        &self,
        expected_element_size: Option<ElementSize>,
    ) -> Result<ListBuilder<'a, T>> {
        let (
            existing_list @ ListContent {
                ptr: existing_ptr,
                element_size: existing_size,
                element_count: len,
            },
            builder,
        ) = self.builder.build_list(self.ptr)?;

        let Some(upgrade_size) = (match expected_element_size {
            Some(expected) => match existing_size.upgrade_to(expected) {
                Some(upgrade) if upgrade == existing_size => None,
                Some(ElementSize::InlineComposite(size)) => Some(size),
                Some(_) => unreachable!("can't upgrade to anything except an inline composite"),
                None => {
                    let failed_upgrade = IncompatibleUpgrade {
                        from: existing_size.into(),
                        to: expected.into(),
                    };
                    return Err(Error::IncompatibleUpgrade(failed_upgrade).into());
                }
            },
            None => None,
        }) else {
            return Ok(ListBuilder::<T> {
                builder,
                ptr: existing_ptr,
                table: self.table.clone(),
                element_count: len,
                element_size: existing_size,
            });
        };

        // Something doesn't match, so we need to do an upgrade.

        let upgrade_element_size = ElementSize::InlineComposite(upgrade_size);

        self.builder.clear_ptrs(self.ptr);
        let (new_list_ptr, _, new_builder) =
            self.builder
                .alloc_list(self.ptr, upgrade_element_size, len)?;
        promote_list(
            existing_ptr,
            existing_size,
            &builder,
            new_list_ptr,
            upgrade_size,
            &new_builder,
            len,
        )?;

        builder.clear_list_section(existing_list);

        Ok(ListBuilder::<T> {
            builder: new_builder,
            ptr: new_list_ptr,
            table: self.table.clone(),
            element_count: len,
            element_size: upgrade_element_size,
        })
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
                expected_element_size.unwrap_or(ElementSize::Void),
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

        match self
            .builder
            .alloc_list(self.ptr, element_size, element_count)
        {
            Ok((start, _, builder)) => Ok(ListBuilder {
                builder,
                ptr: start,
                table: self.table.clone(),
                element_count,
                element_size,
            }),
            Err(err) => Err((err, self)),
        }
    }

    #[inline]
    pub fn init_list(
        self,
        element_size: ElementSize,
        element_count: ElementCount,
    ) -> ListBuilder<'a, T> {
        self.try_init_list(element_size, element_count)
            .map_err(|(e, _)| e)
            .expect("failed to alloc list")
    }

    /// Set this pointer to a copy of the given list, returning a builder to modify it.
    /// This will automatically promote the list if necessary. If the list can't be promoted
    /// to the specified size, this clears the pointer and passes the error to the error handler.
    pub fn try_set_list<E, U>(
        &mut self,
        value: &ListReader<U>,
        size: CopySize<ElementSize>,
        mut err_handler: E,
    ) -> Result<(), E::Error>
    where
        U: InsertableInto<T>,
        E: ErrorHandler,
    {
        self.clear();

        let canonical = matches!(size, CopySize::Canonical);
        let size = match size.builder_size(value) {
            Ok(size) => size,
            Err(err) => {
                return match err_handler.handle_err(Error::IncompatibleUpgrade(err).into()) {
                    ControlFlow::Continue(WriteNull) => Ok(()),
                    ControlFlow::Break(err) => Err(err),
                }
            }
        };

        let (start, total_words, builder) =
            match self.builder.alloc_list(self.ptr, size, value.len()) {
                Ok(results) => results,
                Err(err) => {
                    return match err_handler.handle_err(err) {
                        ControlFlow::Continue(WriteNull) => Ok(()),
                        ControlFlow::Break(err) => Err(err),
                    }
                }
            };

        if total_words == ObjectLen::ZERO {
            return Ok(());
        }

        CopyMachine::<E, T, U> {
            src_table: &value.table,
            dst_table: &self.table,
            err_handler,
            canonical,
        }
        .copy_to_upgraded_list(
            value.ptr,
            &value.reader,
            value.element_size(),
            start,
            &builder,
            size,
            value.len(),
        )
    }

    #[inline]
    pub fn to_blob(&self) -> Result<Option<BlobReader>> {
        self.as_reader().to_blob()
    }

    #[inline]
    pub fn to_blob_mut(self) -> BuildResult<BlobBuilder<'a>, Self> {
        if self.is_null() {
            return Err((None, self));
        }

        match self.to_blob_mut_inner() {
            Ok(b) => Ok(b),
            Err(err) => Err((Some(err), self)),
        }
    }

    fn to_blob_mut_inner(&self) -> Result<BlobBuilder<'a>> {
        let (
            ListContent {
                ptr,
                element_size,
                element_count,
            },
            _,
        ) = self.builder.build_list(self.ptr)?;

        if element_size != ElementSize::Byte {
            return Err(Error::IncompatibleUpgrade(IncompatibleUpgrade {
                from: element_size.into(),
                to: PtrElementSize::Byte,
            })
            .into());
        }

        let ptr = ptr.as_inner().cast();
        Ok(BlobBuilder::new(ptr, element_count))
    }

    #[inline]
    pub fn init_blob(self, element_count: ElementCount) -> BlobBuilder<'a> {
        self.try_init_blob(element_count)
            .map_err(|(err, _)| err)
            .expect("failed to alloc blob")
    }

    pub fn try_init_blob(
        mut self,
        element_count: ElementCount,
    ) -> BuildResult<BlobBuilder<'a>, Self, Error> {
        self.clear();

        match self
            .builder
            .alloc_list(self.ptr, ElementSize::Byte, element_count)
        {
            Ok((start, _, _)) => Ok(BlobBuilder::new(start.as_inner().cast(), element_count)),
            Err(err) => Err((err, self)),
        }
    }

    #[inline]
    pub fn set_blob(self, value: BlobReader) -> BlobBuilder<'a> {
        let len = value.len();
        let mut builder = self.init_blob(len);
        builder.copy_from(value);
        builder
    }

    #[inline]
    pub fn set_capability_ptr(&mut self, index: u32) {
        self.builder.set_ptr(self.ptr, CapabilityPtr::new(index))
    }

    /// Disown this pointer and create an orphan builder. An orphanage from this message is used
    /// to give the orphan a more flexible lifetime.
    #[inline]
    pub fn disown_into<'b>(&mut self, orphanage: &Orphanage<'b, T>) -> OrphanBuilder<'b, T> {
        let orphanage_builder = orphanage.builder();
        assert!(self.builder.is_same_message(orphanage_builder));

        let table = self.table.clone();

        let object_ptr = *self.ptr.as_wire_ptr();
        if object_ptr.is_null() {
            return OrphanBuilder {
                object: None,
                builder: orphanage_builder.clone(),
                table,
            };
        }

        self.builder.set_ptr(self.ptr, WirePtr::NULL);
        match object_ptr.kind() {
            WireKind::Struct => {
                let struct_ptr = object_ptr.struct_ptr().unwrap();
                let (location, builder) = orphanage_builder
                    .build_object_from_end_in(
                        self.builder.id(),
                        self.ptr.into(),
                        struct_ptr.offset(),
                    )
                    .unwrap();
                let object = OrphanObject::Struct {
                    location,
                    size: struct_ptr.size(),
                };
                OrphanBuilder {
                    object: Some(object),
                    builder,
                    table,
                }
            }
            WireKind::List => {
                let list_ptr = object_ptr.list_ptr().unwrap();
                let (location, builder) = orphanage_builder
                    .build_object_from_end_in(self.builder.id(), self.ptr.into(), list_ptr.offset())
                    .unwrap();
                let object = OrphanObject::List {
                    location,
                    element_size: list_ptr.element_size(),
                    element_count: list_ptr.element_count(),
                };

                OrphanBuilder {
                    object: Some(object),
                    builder,
                    table,
                }
            }
            WireKind::Far => {
                let far_ptr = *object_ptr.far_ptr().unwrap();
                let (location, builder) = orphanage_builder
                    .build_object_in(far_ptr.segment(), far_ptr.offset())
                    .expect("far pointer cannot refer to read-only segment");
                let object = OrphanObject::Far {
                    location,
                    double_far: far_ptr.double_far(),
                };
                OrphanBuilder {
                    object: Some(object),
                    builder,
                    table,
                }
            }
            WireKind::Other => {
                let idx = object_ptr.cap_ptr().unwrap().capability_index();
                OrphanBuilder {
                    object: Some(OrphanObject::Capability(idx)),
                    builder: orphanage_builder.clone(),
                    table,
                }
            }
        }
    }

    /// Try to adopt the orphan at this pointer. This can fail if the orphan needs to have a
    /// far pointer allocated to locate it but no space exists in the message and nothing can
    /// be allocated.
    #[inline]
    pub fn try_adopt<'b>(
        &mut self,
        orphan: OrphanBuilder<'b, T>,
    ) -> BuildResult<(), OrphanBuilder<'b, T>, Error> {
        if !orphan.builder.is_same_message(&self.builder) {
            return Err((Error::OrphanFromDifferentMessage.into(), orphan));
        }

        self.clear();

        let (location, base_ptr) = match orphan.object {
            None => return Ok(()),
            Some(OrphanObject::Far {
                location,
                double_far,
            }) => {
                let offset = orphan.builder.offset_from_start(location.into());
                self.builder.set_ptr(
                    self.ptr,
                    FarPtr::new(orphan.builder.id(), offset, double_far),
                );
                return Ok(());
            }
            Some(OrphanObject::Capability(index)) => {
                self.set_capability_ptr(index);
                return Ok(());
            }
            Some(OrphanObject::Struct { location, size }) => {
                (location, WirePtr::from(StructPtr::new(0u16.into(), size)))
            }
            Some(OrphanObject::List {
                location,
                element_size,
                element_count,
            }) => (
                location,
                WirePtr::from(ListPtr::new(0u16.into(), element_size, element_count)),
            ),
        };

        if orphan.builder.is_same_segment(&self.builder) {
            let offset = self
                .builder
                .offset_from_end_of(self.ptr.into(), location.into());
            let new_parts = base_ptr.parts().set_content_offset(offset);
            self.builder.set_ptr(self.ptr, WirePtr { parts: new_parts });
        } else if let Some(landing_pad) = orphan.builder.alloc_in_segment(AllocLen::ONE) {
            // Not the same segment, so we've allocated a landing pad in the orphan's
            // segment.
            let offset = orphan
                .builder
                .offset_from_end_of(landing_pad.into(), location.into());
            let new_parts = base_ptr.parts().set_content_offset(offset);
            orphan
                .builder
                .set_ptr(self.ptr, WirePtr { parts: new_parts });

            let far_offset = orphan.builder.offset_from_start(landing_pad.into());
            self.builder.set_ptr(
                self.ptr,
                FarPtr::new(orphan.builder.id(), far_offset, false),
            );
        } else if let Some((far, tag, pad_builder)) = self.builder.alloc_double_far_landing_pad() {
            // We couldn't allocate a landing pad in the orphan's segment, so we allocated
            // a double far somewhere else.

            let far_offset = self.builder.offset_from_start(location.into());
            pad_builder.set_ptr(far, FarPtr::new(self.builder.id(), far_offset, false));
            pad_builder.set_ptr(tag, base_ptr);

            let pad_offset = pad_builder.offset_from_start(far.into());
            self.builder
                .set_ptr(self.ptr, FarPtr::new(pad_builder.id(), pad_offset, true));
        } else {
            return Err((Error::AllocFailed(LANDING_PAD_LEN).into(), orphan));
        }

        Ok(())
    }

    #[inline]
    pub fn adopt(&mut self, orphan: OrphanBuilder<'_, T>) {
        assert!(orphan.builder.is_same_message(&self.builder));

        self.try_adopt(orphan)
            .map_err(|(err, _)| err)
            .expect("failed to adopt orphan")
    }

    /// Clears the pointer, passing any errors to a given error handler.
    ///
    /// The handler can choose to break with an error E, or continue and write null instead.
    ///
    /// You probably don't want this function, as writing null is a perfectly reasonable default
    /// to use. In which case, `clear` does this for you. You should only use this if you want
    /// strict validation or to customize error handling somehow.
    #[inline]
    pub fn try_clear<E>(&mut self, err_handler: E) -> Result<(), E::Error>
    where
        E: ErrorHandler,
    {
        if self.ptr.as_wire_ptr().is_null() {
            return Ok(());
        }

        Nullifier::<T, _> {
            table: &self.table,
            err_handler,
        }
        .clear_ptr(&self.builder, self.ptr)
    }

    /// Clears the pointer. If any errors occur while clearing any child objects, null is written.
    /// If you want a fallible clear or want to customize error handling behavior, use `try_clear`.
    #[inline]
    pub fn clear(&mut self) {
        self.try_clear(IgnoreErrors).unwrap()
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
    pub fn try_copy_from<E, U>(
        &mut self,
        other: &PtrReader<U>,
        canonical: bool,
        err_handler: E,
    ) -> Result<(), E::Error>
    where
        E: ErrorHandler,
        U: InsertableInto<T>,
    {
        self.clear();

        CopyMachine::<E, T, U> {
            src_table: &other.table,
            dst_table: &self.table,
            err_handler,
            canonical,
        }
        .copy_ptr(other.ptr, &other.reader, self.ptr, &self.builder)
    }

    /// Performs a deep copy of the given pointer, optionally canonicalizing it.
    ///
    /// If any errors occur while copying, this writes null. If you want a fallible copy or want
    /// to customize error handling behavior, use `try_copy_from`.
    #[inline]
    pub fn copy_from(&mut self, other: &PtrReader<impl InsertableInto<T>>, canonical: bool) {
        self.try_copy_from(other, canonical, IgnoreErrors).unwrap()
    }
}

impl<T: CapTable> PtrBuilder<'_, T> {
    pub fn set_cap(&mut self, cap: T::Cap) {
        if let Some(index) = self.table.inject_cap(cap) {
            self.set_capability_ptr(index);
        } else {
            self.clear();
        }
    }
}

impl<'a, T: Table> Capable for PtrBuilder<'a, T> {
    type Table = T;

    type Imbued = T::Builder;
    type ImbuedWith<T2: Table> = PtrBuilder<'a, T2>;

    #[inline]
    fn imbued(&self) -> &Self::Imbued {
        &self.table
    }

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

pub struct OrphanBuilder<'a, T: Table = Empty> {
    builder: ObjectBuilder<'a>,
    object: Option<OrphanObject<'a>>,
    table: T::Builder,
}

impl<'a, T: Table> Capable for OrphanBuilder<'a, T> {
    type Table = T;

    type Imbued = T::Builder;
    type ImbuedWith<T2: Table> = OrphanBuilder<'a, T2>;

    #[inline]
    fn imbued(&self) -> &Self::Imbued {
        &self.table
    }

    #[inline]
    fn imbue_release<T2: Table>(
        self,
        new_table: T2::Builder,
    ) -> (Self::ImbuedWith<T2>, <Self::Table as Table>::Builder) {
        let old_table = self.table;
        let ptr = OrphanBuilder {
            builder: self.builder,
            object: self.object,
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

impl<'a, T: Table> OrphanBuilder<'a, T> {
    pub(crate) fn new(
        builder: ObjectBuilder<'a>,
        object: OrphanObject<'a>,
        table: T::Builder,
    ) -> Self {
        Self {
            builder,
            object: Some(object),
            table,
        }
    }

    #[inline]
    pub fn orphanage(&self) -> Orphanage<'a, T> {
        Orphanage::new(self.builder.clone()).imbue(self.table.clone())
    }

    #[inline]
    pub fn is_null(&self) -> bool {
        self.object.is_none()
    }
}

pub struct StructBuilder<'a, T: Table = Empty> {
    builder: ObjectBuilder<'a>,
    data_start: SegmentRef<'a>,
    ptrs_start: SegmentRef<'a>,
    table: T::Builder,
    /// The length of the data section in bytes. This is always divisible by 8 since lists
    /// don't return a struct builder for anything other than inline composites.
    data_len: u32,
    ptrs_len: u16,
}

impl<'a, T: Table> Capable for StructBuilder<'a, T> {
    type Table = T;

    type Imbued = T::Builder;
    type ImbuedWith<T2: Table> = StructBuilder<'a, T2>;

    #[inline]
    fn imbued(&self) -> &Self::Imbued {
        &self.table
    }

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
    fn data(&self) -> NonNull<u8> {
        self.data_start.as_inner().cast()
    }

    #[inline]
    pub fn as_reader(&self) -> StructReader<'_, T> {
        StructReader {
            reader: self.builder.as_reader(),
            data_start: self.data(),
            ptrs_start: self.ptrs_start,
            table: self.table.as_reader(),
            data_len: self.data_len,
            ptrs_len: self.ptrs_len,
            nesting_limit: u32::MAX,
        }
    }

    #[inline]
    fn data_const(&self) -> *const u8 {
        self.data_mut().cast_const()
    }

    #[inline]
    fn data_mut(&self) -> *mut u8 {
        self.data().as_ptr()
    }

    #[inline]
    pub fn address(&self) -> Address {
        let b = &self.builder;
        let segment = b.segment.id();
        let offset = b.offset_from_start(self.data_start.into());
        Address { segment, offset }
    }

    #[inline]
    pub fn size(&self) -> StructSize {
        StructSize {
            data: Word::round_up_byte_count(self.data_len) as u16,
            ptrs: self.ptrs_len,
        }
    }

    #[inline]
    pub fn total_size(&self) -> MessageSize {
        self.as_reader().total_size().unwrap()
    }

    #[inline]
    pub fn data_section(&self) -> &[u8] {
        unsafe { core::slice::from_raw_parts(self.data_const(), self.data_len as usize) }
    }

    #[inline]
    pub fn data_section_mut(&mut self) -> &mut [u8] {
        unsafe { core::slice::from_raw_parts_mut(self.data_mut(), self.data_len as usize) }
    }

    #[inline]
    pub fn by_ref(&mut self) -> StructBuilder<'_, T> {
        unsafe { self.clone() }
    }

    #[inline]
    pub unsafe fn clone(&self) -> StructBuilder<'a, T> {
        StructBuilder {
            builder: self.builder.clone(),
            data_start: self.data_start,
            ptrs_start: self.ptrs_start,
            table: self.table.clone(),
            data_len: self.data_len,
            ptrs_len: self.ptrs_len,
        }
    }

    /// Reads a field in the data section from the specified slot
    #[inline]
    pub fn data_field<D: Data>(&self, slot: usize) -> D {
        self.data_field_with_default(slot, D::default())
    }

    /// Reads a field in the data section with the specified slot and default value
    #[inline]
    pub fn data_field_with_default<D: Data>(&self, slot: usize, default: D) -> D {
        unsafe { D::read(self.data_const(), self.data_len, slot, default) }
    }

    /// Reads a field in the data section from the specified slot. This assumes the slot is valid
    /// and does not perform any bounds checking.
    #[inline]
    pub unsafe fn data_field_unchecked<D: Data>(&self, slot: usize) -> D {
        self.data_field_with_default_unchecked(slot, D::default())
    }

    /// Reads a field in the data section with the specified slot and default value. This assumes
    /// the slot is valid and does not perform any bounds checking.
    #[inline]
    pub unsafe fn data_field_with_default_unchecked<D: Data>(&self, slot: usize, default: D) -> D {
        D::read_unchecked(self.data_const(), slot, default)
    }

    /// Sets a field in the data section in the slot to the specified value. If the slot is outside
    /// the data section of this struct, this does nothing.
    #[inline]
    pub fn set_field<D: Data>(&mut self, slot: usize, value: D) {
        unsafe { D::write(self.data_mut(), self.data_len, slot, value, D::default()) }
    }

    /// Sets a field in the data section in the slot to the specified value. This assumes the slot
    /// is valid and does not perform any bounds checking.
    #[inline]
    pub unsafe fn set_field_unchecked<D: Data>(&mut self, slot: usize, value: D) {
        self.set_field_with_default_unchecked(slot, value, D::default())
    }

    #[inline]
    pub fn set_field_with_default<D: Data>(&mut self, slot: usize, value: D, default: D) {
        unsafe { D::write(self.data_mut(), self.data_len, slot, value, default) }
    }

    #[inline]
    pub unsafe fn set_field_with_default_unchecked<D: Data>(
        &mut self,
        slot: usize,
        value: D,
        default: D,
    ) {
        D::write_unchecked(self.data_mut(), slot, value, default)
    }

    /// The number of pointers in this struct's pointer section
    #[inline]
    pub fn ptr_count(&self) -> u16 {
        self.ptrs_len
    }

    #[inline]
    pub fn ptr_section(&self) -> ListReader<'_, T> {
        ListReader {
            reader: self.builder.as_reader(),
            ptr: self.ptrs_start,
            table: self.table.as_reader(),
            element_count: self.ptrs_len.into(),
            element_size: ElementSize::Pointer,
            nesting_limit: u32::MAX,
        }
    }

    #[inline]
    pub fn ptr_section_mut(&mut self) -> ListBuilder<'_, T> {
        ListBuilder {
            builder: self.builder.clone(),
            ptr: self.ptrs_start,
            table: self.table.clone(),
            element_count: self.ptrs_len.into(),
            element_size: ElementSize::Pointer,
        }
    }

    #[inline]
    pub fn ptr_field(&self, slot: u16) -> Option<PtrReader<'_, T>> {
        (slot < self.ptrs_len).then(move || unsafe { self.ptr_field_unchecked(slot) })
    }

    #[inline]
    pub unsafe fn ptr_field_unchecked(&self, slot: u16) -> PtrReader<'_, T> {
        PtrReader {
            ptr: self.ptrs_start.offset(slot.into()).as_ref_unchecked(),
            reader: self.builder.as_reader(),
            table: self.table.as_reader(),
            nesting_limit: u32::MAX,
        }
    }

    #[inline]
    pub fn ptr_field_mut(&mut self, slot: u16) -> Option<PtrBuilder<'_, T>> {
        (slot < self.ptrs_len).then(move || unsafe { self.ptr_field_mut_unchecked(slot) })
    }

    #[inline]
    pub unsafe fn ptr_field_mut_unchecked(&mut self, slot: u16) -> PtrBuilder<'_, T> {
        PtrBuilder {
            ptr: self.ptrs_start.offset(slot.into()).as_ref_unchecked(),
            builder: self.builder.clone(),
            table: self.table.clone(),
        }
    }

    #[inline]
    pub fn into_ptr_field(self, slot: u16) -> Option<PtrBuilder<'a, T>> {
        (slot < self.ptrs_len).then(move || unsafe { self.into_ptr_field_unchecked(slot) })
    }

    #[inline]
    pub unsafe fn into_ptr_field_unchecked(self, slot: u16) -> PtrBuilder<'a, T> {
        PtrBuilder {
            ptr: self.ptrs_start.offset(slot.into()).as_ref_unchecked(),
            builder: self.builder,
            table: self.table,
        }
    }

    /// Mostly behaves like you'd expect a copy to behave, but with a caveat originating from
    /// the fact that the struct cannot be resized in place.
    ///
    /// If the source struct is larger than the target struct -- say, because the source was built
    /// using a newer version of the schema that has additional fields -- it will be truncated,
    /// losing data.
    ///
    /// The canonical bool is used to control whether the pointers of the struct are copied in
    /// their canonical form.
    ///
    /// This is intended to support try_set_with_caveats in list builders.
    #[inline]
    pub fn try_copy_with_caveats<E, T2>(
        &mut self,
        other: &StructReader<'_, T2>,
        canonical: bool,
        err_handler: E,
    ) -> Result<(), E::Error>
    where
        E: ErrorHandler,
        T2: InsertableInto<T>,
    {
        let data_to_copy = cmp::min(self.data_len, other.data_len);
        unsafe {
            ptr::copy_nonoverlapping(other.data(), self.data_mut(), data_to_copy as usize);
        }

        let ptrs_to_copy = cmp::min(self.ptrs_len, other.ptrs_len);
        CopyMachine::<'_, E, T, T2> {
            src_table: &other.table,
            dst_table: &self.table,
            err_handler,
            canonical,
        }
        .copy_ptr_section(
            other.ptrs_start,
            &other.reader,
            self.ptrs_start,
            &self.builder,
            ptrs_to_copy.into(),
        )?;

        Ok(())
    }

    #[inline]
    pub fn copy_with_caveats(
        &mut self,
        other: &StructReader<'_, impl InsertableInto<T>>,
        canonical: bool,
    ) {
        self.try_copy_with_caveats(other, canonical, IgnoreErrors)
            .unwrap()
    }
}

pub struct ListBuilder<'a, T: Table = Empty> {
    builder: ObjectBuilder<'a>,
    ptr: SegmentRef<'a>,
    table: T::Builder,
    element_count: ElementCount,
    element_size: ElementSize,
}

impl<'a, T: Table> ListBuilder<'a, T> {
    #[inline]
    fn empty(
        builder: ObjectBuilder<'a>,
        ptr: SegmentRef<'a>,
        table: T::Builder,
        element_size: ElementSize,
    ) -> Self {
        Self {
            builder,
            ptr,
            table,
            element_count: 0.into(),
            element_size,
        }
    }

    #[inline]
    pub fn address(&self) -> Address {
        let b = &self.builder;
        let segment = b.segment.id();
        let offset = b.offset_from_start(self.ptr.into());
        Address { segment, offset }
    }

    #[inline]
    pub fn as_reader(&self) -> ListReader<T> {
        ListReader {
            ptr: self.ptr,
            reader: self.builder.as_reader(),
            table: self.table.as_reader(),
            element_count: self.element_count,
            nesting_limit: u32::MAX,
            element_size: self.element_size,
        }
    }

    #[inline]
    pub fn by_ref(&mut self) -> ListBuilder<'_, T> {
        unsafe { self.clone() }
    }

    #[inline]
    pub unsafe fn clone(&self) -> ListBuilder<'a, T> {
        ListBuilder {
            ptr: self.ptr,
            builder: self.builder.clone(),
            table: self.table.clone(),
            element_count: self.element_count,
            element_size: self.element_size,
        }
    }

    #[inline]
    pub fn len(&self) -> ElementCount {
        self.element_count
    }

    #[inline]
    pub fn element_size(&self) -> ElementSize {
        self.element_size
    }

    #[inline]
    pub fn total_size(&self) -> MessageSize {
        self.as_reader().total_size().unwrap()
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
        let step = u64::from(self.element_size.bits());
        let index = u64::from(index);
        let byte_offset = (step * index) / 8;
        byte_offset as usize
    }

    /// Copy from the source list into this builder.
    ///
    /// This supports upgrading, but can't resize the list any further. Thus, upgrading from the
    /// source element size into the builder's element size must yield the builder's element size.
    /// The builder list size must be equal to the reader list size.
    #[inline]
    pub fn try_copy_from<E, T2>(
        &mut self,
        other: &ListReader<'_, T2>,
        err_handler: E,
    ) -> Result<(), E::Error>
    where
        E: ErrorHandler,
        T2: InsertableInto<T>,
    {
        assert_eq!(
            self.element_size.upgrade_to(other.element_size),
            Some(self.element_size)
        );
        assert!(self.element_count >= other.element_count);

        CopyMachine::<E, T, T2> {
            err_handler,
            src_table: &other.table,
            dst_table: &self.table,
            canonical: false,
        }
        .copy_to_upgraded_list(
            other.ptr,
            &other.reader,
            other.element_size,
            self.ptr,
            &self.builder,
            self.element_size,
            self.element_count,
        )
    }

    /// Copy from the source list into this builder at specific ranges.
    ///
    /// This allows a subset of the values from the source list to be copied into a subset of this
    /// builder. This is primarily useful for copying multiple lists into one large list, but isn't
    /// as optimized because it must handle specific edge cases.
    ///
    /// # Panics
    ///
    /// This supports copying from un-upgraded lists into a larger upgraded list. Upgrading from
    /// the source element size into the builder element size should yield the builder element
    /// size. If this check fails the call panics.
    ///
    /// Both ranges should be in bounds. `self_start + len` should not exceed the bounds of this
    /// list and `other_start + len` should not exceed the bounds of the `other` list. If either
    /// of these ranges are out of bounds the call will panic.
    #[inline]
    pub fn try_copy_ranges<E, T2>(
        &mut self,
        self_start: ElementCount,
        other: &ListReader<'_, T2>,
        other_start: ElementCount,
        len: ElementCount,
        err_handler: E,
    ) -> Result<(), E::Error>
    where
        E: ErrorHandler,
        T2: InsertableInto<T>,
    {
        assert_eq!(
            self.element_size.upgrade_to(other.element_size),
            Some(self.element_size)
        );

        if len == ElementCount::ZERO {
            return Ok(());
        }

        // Make sure we don't accidentally go out of bounds on either list.
        let dst_end = self_start.get() + len.get();
        let dst_len = self.element_count.get();
        assert!(
            dst_end <= dst_len,
            "range exceeds bounds of destination list"
        );

        let src_end = other_start.get() + len.get();
        let src_len = other.element_count.get();
        assert!(
            src_end <= src_len,
            "range exceeds bounds of destination list"
        );

        CopyMachine::<E, T, T2> {
            err_handler,
            src_table: &other.table,
            dst_table: &self.table,
            canonical: false,
        }
        .copy_to_upgraded_list_at(
            other.ptr,
            &other.reader,
            other.element_size,
            other_start,
            self.ptr,
            &self.builder,
            self.element_size,
            self_start,
            len,
        )
    }

    /// Copy from the source list with the caveat that elements from the source list will be
    /// truncated if the destination isn't large enough.
    ///
    /// This is intended to support copying from a list into it's canonical size.
    ///
    /// Both lists must have the same element type, but inline composites can be different sizes.
    #[inline]
    pub fn try_copy_with_caveats<E, T2>(
        &mut self,
        other: &ListReader<'_, T2>,
        canonical: bool,
        err_handler: E,
    ) -> Result<(), E::Error>
    where
        E: ErrorHandler,
        T2: InsertableInto<T>,
    {
        assert_eq!(self.element_count, other.element_count);

        let len = self.element_count;
        let mut copier = CopyMachine::<E, T, T2> {
            err_handler,
            src_table: &other.table,
            dst_table: &self.table,
            canonical,
        };

        use ElementSize::InlineComposite;
        match (other.element_size, self.element_size) {
            (src, dst) if src == dst => {
                copier.copy_list(other.ptr, &other.reader, self.ptr, &self.builder, dst, len)
            }
            (InlineComposite(src), InlineComposite(dst)) => copier.copy_inline_composites(
                other.ptr,
                src,
                &other.reader,
                self.ptr,
                dst,
                &self.builder,
                len,
            ),
            _ => panic!("lists must have the same size or both must be inline composites"),
        }
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
    pub unsafe fn data_unchecked<D: Data>(&self, index: u32) -> D {
        use core::any::{type_name, TypeId};

        debug_assert!(index < self.element_count.get(), "index out of bounds");
        debug_assert!({
            let is_inline_composite_with_data = matches!(
                self.element_size,
                ElementSize::InlineComposite(size) if size.data != 0
            );
            self.element_size == D::ELEMENT_SIZE || is_inline_composite_with_data
        }, "attempted to access invalid data for this list; list element size: {:?}, data type: {}",
        self.element_size, type_name::<D>());

        if TypeId::of::<D>() == TypeId::of::<bool>() {
            D::read_unchecked(self.ptr(), index as usize, D::default())
        } else {
            let ptr = self.ptr().add(self.index_to_offset(index));
            D::read_unchecked(ptr, 0, D::default())
        }
    }
    #[inline]
    pub unsafe fn set_data_unchecked<D: Data>(&mut self, index: u32, value: D) {
        use core::any::{type_name, TypeId};

        debug_assert!(index < self.element_count.get(), "index out of bounds");
        debug_assert!({
            let is_inline_composite_with_data = matches!(
                self.element_size,
                ElementSize::InlineComposite(size) if size.data != 0
            );
            self.element_size == D::ELEMENT_SIZE || is_inline_composite_with_data
        }, "attempted to access invalid data for this list; list element size: {:?}, data type: {}",
        self.element_size, type_name::<D>());

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
    /// * This must be a pointer list or an inline composite with atleast one pointer element
    #[inline]
    pub unsafe fn ptr_unchecked(&self, index: u32) -> PtrReader<'_, T> {
        debug_assert!(index < self.element_count.get(), "index out of bounds");
        debug_assert!(
            {
                let is_inline_composite_with_ptr = matches!(
                    self.element_size, ElementSize::InlineComposite(size) if size.ptrs != 0
                );
                self.element_size == ElementSize::Pointer || is_inline_composite_with_ptr
            },
            "attempted to read pointer from a list of something else"
        );

        let base_offset = self.index_to_offset(index);
        let data_offset = if let ElementSize::InlineComposite(size) = self.element_size {
            size.data as usize * Word::BYTES
        } else {
            0
        };

        let offset = base_offset + data_offset;
        let ptr = self.ptr().add(offset).cast_mut().cast();

        PtrReader {
            ptr: SegmentRef::new_unchecked(NonNull::new_unchecked(ptr)),
            reader: self.builder.as_reader(),
            table: self.table.as_reader(),
            nesting_limit: u32::MAX,
        }
    }
    #[inline]
    pub unsafe fn ptr_mut_unchecked(&mut self, index: u32) -> PtrBuilder<'_, T> {
        debug_assert!(index < self.element_count.get(), "index out of bounds");
        debug_assert!(
            {
                let is_inline_composite_with_ptr = matches!(
                    self.element_size, ElementSize::InlineComposite(size) if size.ptrs != 0
                );
                self.element_size == ElementSize::Pointer || is_inline_composite_with_ptr
            },
            "attempted to read pointer from a list of something else"
        );

        let base_offset = self.index_to_offset(index);
        let data_offset = if let ElementSize::InlineComposite(size) = self.element_size {
            size.data as usize * Word::BYTES
        } else {
            0
        };

        let offset = base_offset + data_offset;
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
    pub unsafe fn struct_unchecked(&self, index: u32) -> StructReader<'_, T> {
        debug_assert!(index < self.element_count.get(), "index out of bounds");
        debug_assert!(
            self.element_size != ElementSize::Bit,
            "attempted to read struct from bit list"
        );

        let (data_len, ptrs_len) = self.element_size.bytes_and_ptrs();
        let offset = self.index_to_offset(index);
        let struct_start = self.ptr().add(offset);
        let struct_data = struct_start;
        let struct_ptrs = struct_start.add(data_len as usize).cast::<Word>();

        StructReader {
            data_start: NonNull::new_unchecked(struct_data.cast_mut()),
            ptrs_start: SegmentRef::new_unchecked(NonNull::new_unchecked(struct_ptrs.cast_mut())),
            reader: self.builder.as_reader().clone(),
            table: self.table.as_reader(),
            data_len,
            ptrs_len,
            nesting_limit: u32::MAX,
        }
    }
    #[inline]
    pub unsafe fn struct_mut_unchecked(&mut self, index: u32) -> StructBuilder<'_, T> {
        debug_assert!(index < self.element_count.get(), "index out of bounds");
        debug_assert!(
            self.element_size.is_inline_composite(),
            "attempted to build a struct in a non-struct list"
        );

        let (data_len, ptrs_len) = self.element_size.bytes_and_ptrs();
        let offset = self.index_to_offset(index);
        let struct_start = self.ptr().add(offset);
        let struct_data = struct_start.cast::<Word>();
        let struct_ptrs = struct_start.add(data_len as usize).cast::<Word>();

        StructBuilder {
            data_start: SegmentRef::new_unchecked(NonNull::new_unchecked(struct_data.cast_mut())),
            ptrs_start: SegmentRef::new_unchecked(NonNull::new_unchecked(struct_ptrs.cast_mut())),
            builder: self.builder.clone(),
            table: self.table.clone(),
            data_len,
            ptrs_len,
        }
    }
}

impl<'a, T: Table> Capable for ListBuilder<'a, T> {
    type Table = T;

    type Imbued = T::Builder;
    type ImbuedWith<T2: Table> = ListBuilder<'a, T2>;

    #[inline]
    fn imbued(&self) -> &Self::Imbued {
        &self.table
    }

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

pub struct BlobBuilder<'a> {
    a: PhantomData<&'a mut [u8]>,
    ptr: NonNull<u8>,
    len: ElementCount,
}

impl BlobBuilder<'_> {
    #[inline]
    pub(crate) fn new(ptr: NonNull<u8>, len: ElementCount) -> Self {
        Self {
            a: PhantomData,
            ptr,
            len,
        }
    }

    #[inline]
    pub fn empty() -> Self {
        Self {
            a: PhantomData,
            ptr: NonNull::dangling(),
            len: ElementCount::ZERO,
        }
    }

    #[inline]
    pub const fn data(&self) -> NonNull<u8> {
        self.ptr
    }

    #[inline]
    pub const fn len(&self) -> ElementCount {
        self.len
    }

    #[inline]
    pub const fn as_reader(&self) -> BlobReader {
        unsafe { BlobReader::new_unchecked(self.ptr, self.len) }
    }

    #[inline]
    pub(crate) fn copy_from(&mut self, other: BlobReader) {
        assert_eq!(self.len, other.len());

        let dst = self.ptr.as_ptr();
        let src = other.as_slice();
        unsafe { ptr::copy_nonoverlapping(src.as_ptr(), dst, src.len()) }
    }

    #[inline]
    pub(crate) const fn as_slice(&self) -> &[u8] {
        unsafe {
            core::slice::from_raw_parts(self.ptr.as_ptr().cast_const(), self.len.get() as usize)
        }
    }
}

struct Nullifier<'a, T: Table, E: ErrorHandler> {
    table: &'a T::Builder,
    err_handler: E,
}

impl<'a, T: Table, E: ErrorHandler> Nullifier<'a, T, E> {
    /// Clear the given pointer, assuming it's not null.
    fn clear_ptr(
        &mut self,
        ptr_builder: &ObjectBuilder<'a>,
        place: SegmentRef<'a>,
    ) -> Result<(), E::Error> {
        let (Content { location, ptr }, builder) = ptr_builder.clone().locate(place);
        ptr_builder.clear_ptrs(place);

        match ptr.kind() {
            WireKind::Struct => {
                let struct_ptr = ptr.struct_ptr().unwrap();
                let size = struct_ptr.size();
                if size != StructSize::EMPTY {
                    let (start, builder) = match location {
                        Location::Near { origin } | Location::Far { origin } => {
                            builder.build_object_from_end_of(origin, struct_ptr.offset())
                        }
                        Location::DoubleFar { segment, offset } => builder
                            .build_object_in(segment, offset)
                            .expect("struct pointers cannot refer to read-only segments"),
                    };

                    self.clear_struct(&builder, StructContent { ptr: start, size })?;
                }
            }
            WireKind::List => {
                use PtrElementSize::*;

                let list_ptr = ptr.list_ptr().unwrap();
                let (start, builder) = match location {
                    Location::Near { origin } | Location::Far { origin } => {
                        builder.build_object_from_end_of(origin, list_ptr.offset())
                    }
                    Location::DoubleFar { segment, offset } => match builder
                        .build_object_in(segment, offset)
                    {
                        Some(place) => place,
                        None if list_ptr.element_size() == Byte => {
                            builder.arena.remove_external_segment(segment);
                            return Ok(());
                        }
                        None => {
                            unreachable!("lists not of bytes cannot refer to read-only segments")
                        }
                    },
                };

                let count = list_ptr.element_count();
                let element_size = list_ptr.element_size();
                let content = if element_size == PtrElementSize::InlineComposite {
                    let tag = start
                        .as_wire_ptr()
                        .struct_ptr()
                        .expect("inline composite tag must be struct");
                    let content_start = unsafe { start.offset(1u16.into()).as_ref_unchecked() };
                    let size = tag.size();
                    ListContent {
                        ptr: content_start,
                        element_size: ElementSize::InlineComposite(size),
                        element_count: count,
                    }
                } else {
                    ListContent {
                        ptr: start,
                        element_size: element_size.to_element_size(),
                        element_count: count,
                    }
                };
                self.clear_list(&builder, content)?;
            }
            WireKind::Other => {
                let cap_idx = ptr
                    .cap_ptr()
                    .expect("unknown other pointer")
                    .capability_index();
                if let Err(err) = self.table.clear_cap(cap_idx) {
                    if let ControlFlow::Break(e) = self.err_handler.handle_err(err) {
                        return Err(e);
                    }
                }
            }
            WireKind::Far => unreachable!("invalid far pointer in builder"),
        }

        Ok(())
    }

    fn clear_ptr_section(
        &mut self,
        builder: &ObjectBuilder<'a>,
        ptr: SegmentRef<'a>,
        len: SegmentOffset,
    ) -> Result<(), E::Error> {
        let ptrs = unsafe { iter_unchecked(ptr, len) };
        for ptr in ptrs.filter(|r| !r.as_ref().is_null()) {
            self.clear_ptr(builder, ptr)?;
        }

        Ok(())
    }

    fn clear_struct(
        &mut self,
        builder: &ObjectBuilder<'a>,
        content: StructContent<'a>,
    ) -> Result<(), E::Error> {
        self.clear_ptr_section(builder, content.ptrs_start(), content.size.ptrs.into())?;
        builder.clear_struct_section(content);
        Ok(())
    }

    fn clear_list(
        &mut self,
        builder: &ObjectBuilder<'a>,
        content: ListContent<'a>,
    ) -> Result<(), E::Error> {
        match content.element_size {
            // Return the result of this clear since we will clear each pointer as we go through the list.
            ElementSize::Pointer => {
                return self.clear_ptr_section(builder, content.ptr, content.element_count)
            }
            ElementSize::InlineComposite(size) => {
                for (_, ptr) in iter_inline_composites(content.ptr, size, content.element_count) {
                    self.clear_ptr_section(builder, ptr, size.ptrs.into())?;
                }
            }
            _ => {}
        }
        builder.clear_list_section(content);
        Ok(())
    }
}

#[derive(PartialEq, Eq, Debug, Clone, Copy)]
pub enum PtrEquality {
    NotEqual,
    Equal,
    ContainsCaps,
}

fn cmp_ptr(
    our_ptr: SegmentRef,
    our_reader: &ObjectReader,
    their_ptr: SegmentRef,
    their_reader: &ObjectReader,
) -> Result<PtrEquality> {
    if our_ptr.as_ref().is_null() && their_ptr.as_ref().is_null() {
        return Ok(PtrEquality::Equal);
    }

    let mut our_reader = our_reader.clone();
    let mut their_reader = their_reader.clone();

    let our_content = our_reader.try_read_typed(our_ptr)?;
    let their_content = their_reader.try_read_typed(their_ptr)?;
    match (our_content, their_content) {
        (TypedContent::Struct(our_struct), TypedContent::Struct(their_struct)) => {
            cmp_struct(our_struct, &our_reader, their_struct, &their_reader)
        }
        (TypedContent::List(our_list), TypedContent::List(their_list)) => {
            cmp_list(&our_list, &our_reader, &their_list, &their_reader)
        }
        (TypedContent::Capability(_), TypedContent::Capability(_)) => Ok(PtrEquality::ContainsCaps),
        _ => Ok(PtrEquality::NotEqual),
    }
}

fn cmp_ptr_sections(
    our_ptrs: SegmentRef,
    our_len: SegmentOffset,
    our_reader: &ObjectReader,
    their_ptrs: SegmentRef,
    their_len: SegmentOffset,
    their_reader: &ObjectReader,
) -> Result<PtrEquality> {
    let ptrs = unsafe {
        let our_len = our_reader.trim_end_null_section(our_ptrs, our_len);
        let their_len = their_reader.trim_end_null_section(their_ptrs, their_len);

        if our_len != their_len {
            return Ok(PtrEquality::NotEqual);
        }

        let our_ptrs_iter = iter_unchecked(our_ptrs, our_len);
        let their_ptrs_iter = iter_unchecked(their_ptrs, our_len);
        our_ptrs_iter.zip(their_ptrs_iter)
    };

    let mut equality = PtrEquality::Equal;
    for (our_ptr, their_ptr) in ptrs {
        match cmp_ptr(our_ptr, our_reader, their_ptr, their_reader)? {
            PtrEquality::Equal => continue,
            PtrEquality::ContainsCaps => equality = PtrEquality::ContainsCaps,
            PtrEquality::NotEqual => return Ok(PtrEquality::NotEqual),
        }
    }

    Ok(equality)
}

fn cmp_struct(
    our_struct: StructContent,
    our_reader: &ObjectReader,
    their_struct: StructContent,
    their_reader: &ObjectReader,
) -> Result<PtrEquality> {
    unsafe {
        let our_data = our_reader.section_slice(our_struct.ptr, our_struct.size.data.into());
        let their_data =
            their_reader.section_slice(their_struct.ptr, their_struct.size.data.into());

        if our_data != their_data {
            return Ok(PtrEquality::NotEqual);
        }

        let our_ptrs = our_struct.ptrs_start();
        let our_ptrs_iter = iter_unchecked(our_ptrs, our_struct.size.ptrs.into());
        let their_ptrs = their_struct.ptrs_start();
        let their_ptrs_iter = iter_unchecked(their_ptrs, their_struct.size.ptrs.into());

        let ptrs = our_ptrs_iter.zip(their_ptrs_iter);

        let mut equality = PtrEquality::Equal;
        for (our_ptr, their_ptr) in ptrs {
            match cmp_ptr(our_ptr, our_reader, their_ptr, their_reader)? {
                PtrEquality::Equal => continue,
                PtrEquality::ContainsCaps => equality = PtrEquality::ContainsCaps,
                PtrEquality::NotEqual => return Ok(PtrEquality::NotEqual),
            }
        }

        Ok(equality)
    }
}

fn cmp_list(
    our_list: &ListContent,
    our_reader: &ObjectReader,
    their_list: &ListContent,
    their_reader: &ObjectReader,
) -> Result<PtrEquality> {
    if our_list.element_count != their_list.element_count {
        return Ok(PtrEquality::NotEqual);
    }

    if our_list.element_size.as_ptr_size() != their_list.element_size.as_ptr_size() {
        return Ok(PtrEquality::NotEqual);
    }

    if our_list.element_count == ElementCount::ZERO {
        return Ok(PtrEquality::Equal);
    }

    let len = our_list.element_count;

    use ElementSize::*;
    match our_list.element_size {
        Void => Ok(PtrEquality::Equal),
        Bit => {
            let total_bits = len.get();
            let total_bytes = total_bits.div_ceil(8) as usize;
            let total_words = Word::round_up_bit_count(u64::from(total_bits));
            let word_len = SegmentOffset::new(total_words).unwrap();
            let (our_data, their_data) = unsafe {
                (
                    Word::slice_to_bytes(our_reader.section_slice(our_list.ptr, word_len)),
                    Word::slice_to_bytes(their_reader.section_slice(their_list.ptr, word_len)),
                )
            };

            let our_data = &our_data[..total_bytes];
            let their_data = &their_data[..total_bytes];

            let partial_bits = total_bits % 8;
            let equals = if partial_bits != 0 {
                let (our_partial, our_full_data) = our_data.split_last().unwrap();
                let (their_partial, their_full_data) = their_data.split_last().unwrap();
                let mask = !(255u8 >> partial_bits);

                let full_data_equals = our_full_data == their_full_data;
                let partial_data_equals = (our_partial & mask) == (their_partial & mask);
                full_data_equals && partial_data_equals
            } else {
                our_data == their_data
            };

            if !equals {
                return Ok(PtrEquality::NotEqual);
            }

            Ok(PtrEquality::Equal)
        }
        size @ (Byte | TwoBytes | FourBytes | EightBytes) => {
            let total_bytes = size.total_bytes(len) as usize;
            let total_words = SegmentOffset::new(size.total_words(len)).unwrap();
            let (our_data, their_data) = unsafe {
                (
                    Word::slice_to_bytes(our_reader.section_slice(our_list.ptr, total_words)),
                    Word::slice_to_bytes(their_reader.section_slice(their_list.ptr, total_words)),
                )
            };

            // Remove any padding data that might be garbage.
            let our_data_set = &our_data[..total_bytes];
            let their_data_set = &their_data[..total_bytes];

            if our_data_set != their_data_set {
                return Ok(PtrEquality::NotEqual);
            }

            Ok(PtrEquality::Equal)
        }
        Pointer => unsafe {
            let our_ptrs_iter = iter_unchecked(our_list.ptr, len);
            let their_ptrs_iter = iter_unchecked(their_list.ptr, len);
            let ptrs = our_ptrs_iter.zip(their_ptrs_iter);

            let mut equality = PtrEquality::Equal;
            for (our_ptr, their_ptr) in ptrs {
                match cmp_ptr(our_ptr, our_reader, their_ptr, their_reader)? {
                    PtrEquality::Equal => continue,
                    PtrEquality::ContainsCaps => equality = PtrEquality::ContainsCaps,
                    PtrEquality::NotEqual => return Ok(PtrEquality::NotEqual),
                }
            }

            Ok(equality)
        },
        InlineComposite(our_size) => {
            let InlineComposite(their_size) = their_list.element_size else {
                unreachable!()
            };

            let our_structs = unsafe { step_by_unchecked(our_list.ptr, our_size.len(), len) };
            let their_structs = unsafe { step_by_unchecked(their_list.ptr, their_size.len(), len) };

            let mut equality = PtrEquality::Equal;
            for (our_struct, their_struct) in our_structs.zip(their_structs) {
                let our_struct = StructContent {
                    ptr: our_struct,
                    size: our_size,
                };
                let their_struct = StructContent {
                    ptr: their_struct,
                    size: their_size,
                };
                match cmp_struct(our_struct, our_reader, their_struct, their_reader)? {
                    PtrEquality::Equal => continue,
                    PtrEquality::ContainsCaps => equality = PtrEquality::ContainsCaps,
                    PtrEquality::NotEqual => return Ok(PtrEquality::NotEqual),
                }
            }
            Ok(equality)
        }
    }
}

fn transfer_ptr(
    src: SegmentRef,
    src_builder: &ObjectBuilder,
    dst: SegmentRef,
    dst_builder: &ObjectBuilder,
) -> Result<()> {
    debug_assert!(dst.as_ref().is_null());

    let ptr = src.as_wire_ptr();
    if ptr.is_null() {
        return Ok(());
    }

    let new_ptr = match ptr.kind() {
        WireKind::Struct | WireKind::List => {
            let parts = ptr.parts();
            let offset = parts.content_offset();
            let src_content = src.signed_offset_from_end(offset);

            let src_segment = src_builder.segment.id();
            let dst_segment = dst_builder.segment.id();
            if src_segment == dst_segment {
                // Both pointers are in the same segment. That means we can just
                // calculate the difference between the two pointers.

                let new_offset = dst_builder.offset_from_end_of(dst.into(), src_content);
                WirePtr {
                    parts: parts.set_content_offset(new_offset),
                }

            // The content and pointer are in different segments. This means we need to
            // allocate a far (and possibly double far) pointer to the content.
            // Attempt to allocate a content pointer in the source content's segment.
            } else if let Some(new_content_ptr) = src_builder.alloc_in_segment(AllocLen::ONE) {
                // Now we need to configure the content pointer to point to the content
                // and the original dst_ptr to be a far pointer that points to the
                // content pointer.

                let new_offset_for_content_ptr =
                    src_builder.offset_from_end_of(new_content_ptr.into(), src_content);
                src_builder.set_ptr(
                    new_content_ptr,
                    WirePtr {
                        parts: parts.set_content_offset(new_offset_for_content_ptr),
                    },
                );

                let far_offset = src_builder.offset_from_start(new_content_ptr.into());
                WirePtr {
                    far_ptr: FarPtr::new(src_segment, far_offset, false),
                }

            // We couldn't allocate there, which means we need to allocate a double
            // far anywhere. We use the dst_builder's segment as a first attempt
            // before falling back to allocating a new segment.
            } else if let Some((new_double_far, tag, double_far_builder)) =
                dst_builder.alloc_double_far_landing_pad()
            {
                let double_far_segment = double_far_builder.id();
                let double_far_offset = double_far_builder.offset_from_start(new_double_far.into());

                let new_dst_ptr = WirePtr {
                    far_ptr: FarPtr::new(double_far_segment, double_far_offset, true),
                };

                // Now configure the landing pad far pointer. It tells us where the start of the content is.
                let content_offset = src_builder.offset_from_start(src_content);
                let landing_pad = WirePtr {
                    far_ptr: FarPtr::new(src_segment, content_offset, false),
                };
                double_far_builder.set_ptr(new_double_far, landing_pad);

                let content_tag = WirePtr {
                    parts: parts.set_content_offset(0u16.into()),
                };
                double_far_builder.set_ptr(tag, content_tag);

                new_dst_ptr
            } else {
                return Err(Error::AllocFailed(LANDING_PAD_LEN).into());
            }
        }
        WireKind::Far | WireKind::Other => *ptr,
    };
    dst_builder.set_ptr(dst, new_ptr);

    Ok(())
}

fn transfer_ptr_section(
    src: SegmentRef,
    src_builder: &ObjectBuilder,
    dst: SegmentRef,
    dst_builder: &ObjectBuilder,
    len: SegmentOffset,
) -> Result<()> {
    let src_ptrs = unsafe { iter_unchecked(src, len) };
    let dst_ptrs = unsafe { iter_unchecked(dst, len) };

    for (src, dst) in src_ptrs.zip(dst_ptrs) {
        transfer_ptr(src, src_builder, dst, dst_builder)?;
    }

    Ok(())
}

/// Promote a struct to a new larger size. The destination struct must be at least as large as the
/// source struct.
fn promote_struct(
    src: SegmentRef,
    src_size: StructSize,
    src_builder: &ObjectBuilder,
    dst_data: SegmentRef,
    dst_ptrs: SegmentRef,
    dst_builder: &ObjectBuilder,
) -> Result<()> {
    let src_data = unsafe { src_builder.section_slice_mut(src, src_size.data.into()) };
    let dst_data = unsafe { dst_builder.section_slice_mut(dst_data, src_size.data.into()) };
    dst_data[..src_data.len()].copy_from_slice(&*src_data);

    let src_ptrs = unsafe { src.offset(src_size.data.into()).as_ref_unchecked() };
    transfer_ptr_section(
        src_ptrs,
        src_builder,
        dst_ptrs,
        dst_builder,
        src_size.ptrs.into(),
    )
}

fn promote_list(
    src: SegmentRef,
    src_size: ElementSize,
    src_builder: &ObjectBuilder,
    dst: SegmentRef,
    dst_size: StructSize,
    dst_builder: &ObjectBuilder,
    len: ElementCount,
) -> Result<()> {
    let dst_structs = unsafe { step_by_unchecked(dst, dst_size.len(), len) };

    macro_rules! copy_data {
        ($ty:ty) => {{
            let total_size = SegmentOffset::new(src_size.total_words(len)).unwrap();
            let src_words = unsafe { src_builder.section_slice_mut(src, total_size) };
            let src_bytes = &WireValue::<$ty>::from_word_slice(src_words)[..(len.get() as usize)];
            let dst_bytes =
                dst_structs.map(|r| unsafe { &mut *r.as_ptr_mut().cast::<WireValue<$ty>>() });
            src_bytes
                .iter()
                .zip(dst_bytes)
                .for_each(|(src, dst)| *dst = *src);
        }};
    }

    use ElementSize::*;
    match src_size {
        Void | Bit => {}
        Byte => copy_data!(u8),
        TwoBytes => copy_data!(u16),
        FourBytes => copy_data!(u32),
        EightBytes => copy_data!(u64),
        Pointer => transfer_ptr_section(src, src_builder, dst, dst_builder, len)?,
        InlineComposite(src_size) => {
            let src_structs = unsafe { step_by_unchecked(src, src_size.len(), len) };
            for (src_struct, dst_struct) in src_structs.zip(dst_structs) {
                let dst_ptrs =
                    unsafe { dst_struct.offset(dst_size.data.into()).as_ref_unchecked() };
                promote_struct(
                    src_struct,
                    src_size,
                    src_builder,
                    dst_struct,
                    dst_ptrs,
                    dst_builder,
                )?;
            }
        }
    }

    Ok(())
}

/// A helper type to make copying values in messages require fewer parameters
struct CopyMachine<'a, E, T: Table, U: InsertableInto<T>> {
    src_table: &'a U::Reader,
    dst_table: &'a T::Builder,
    err_handler: E,
    canonical: bool,
}

impl<'a, E, T: Table, U: InsertableInto<T>> CopyMachine<'a, E, T, U>
where
    E: ErrorHandler,
{
    fn copy_ptr(
        &mut self,
        src: SegmentRef,
        reader: &ObjectReader,
        dst: SegmentRef,
        builder: &ObjectBuilder,
    ) -> Result<(), E::Error> {
        let mut reader = reader.clone();
        let content = match reader.try_read_typed(src) {
            Ok(c) => c,
            Err(err) => return self.handle_err(err),
        };

        match content {
            TypedContent::Struct(StructContent {
                ptr: src,
                size: src_size,
            }) => {
                let mut dst_size = src_size;
                if self.canonical {
                    dst_size = calculate_canonical_struct_size(&reader, src, src_size);
                }

                match builder.alloc_struct(dst, dst_size) {
                    Ok((dst, builder)) => {
                        self.copy_struct(src, src_size, &reader, dst, dst_size, &builder)
                    }
                    Err(err) => self.handle_err(err),
                }
            }
            TypedContent::List(ListContent {
                ptr: src_ptr,
                element_size: ElementSize::InlineComposite(src_size),
                element_count,
            }) if self.canonical => {
                // Canonicalization only matters for inline composite elements, so we cordon off a
                // separate match arm to handle it.

                let dst_size = calculate_canonical_inline_composite_size(
                    &reader,
                    src,
                    src_size,
                    element_count,
                );
                match builder.alloc_list(dst, ElementSize::InlineComposite(dst_size), element_count)
                {
                    Ok((dst, _, builder)) => {
                        if dst_size == StructSize::EMPTY {
                            return Ok(());
                        }

                        self.copy_inline_composites(
                            src_ptr,
                            src_size,
                            &reader,
                            dst,
                            dst_size,
                            &builder,
                            element_count,
                        )
                    }
                    Err(err) => self.handle_err(err),
                }
            }
            TypedContent::List(ListContent {
                ptr: src_ptr,
                element_size,
                element_count,
            }) => match builder.alloc_list(dst, element_size, element_count) {
                Ok((dst, total_size, builder)) => {
                    if total_size.get() == 0 {
                        return Ok(());
                    }

                    self.copy_list(src_ptr, &reader, dst, &builder, element_size, element_count)
                }
                Err(err) => self.handle_err(err),
            },
            TypedContent::Capability(_) if self.canonical => {
                self.handle_err(Error::from(Error::CapabilityNotAllowed))
            }
            TypedContent::Capability(cap) => match U::copy(self.src_table, cap, self.dst_table) {
                Ok(Some(new_idx)) => {
                    builder.set_ptr(dst, CapabilityPtr::new(new_idx));
                    Ok(())
                }
                Ok(None) => {
                    // Weird case, but technically possible. When we copied from the source table
                    // to the destination table, the destination table interpreted the capability
                    // as null. So we have the clear our pointer. Which in this case means do
                    // nothing because the pointer is already empty.

                    Ok(())
                }
                Err(err) => self.handle_err(err),
            },
        }
    }

    fn copy_list<'b>(
        &mut self,
        src: SegmentRef,
        reader: &ObjectReader,
        dst: SegmentRef<'b>,
        builder: &ObjectBuilder<'b>,
        size: ElementSize,
        len: ElementCount,
    ) -> Result<(), E::Error> {
        if size.is_ptrs() {
            // The list is made up entirely of pointers, so we can copy the whole thing as a
            // big pointer section.
            let len = SegmentOffset::new(size.total_words(len)).unwrap();
            return self.copy_ptr_section(src, reader, dst, builder, len);
        }

        if size.is_data() {
            // The list is made up entirely of data, so we can copy the whole thing with a
            // single memcpy
            let total_size = SegmentOffset::new(size.total_words(len)).unwrap();
            let src_list = unsafe { reader.section_slice(src, total_size) };
            let dst_list = unsafe { builder.section_slice_mut(dst, total_size) };
            dst_list.copy_from_slice(src_list);
            return Ok(());
        }

        // It isn't empty, only pointers, or only data, so it must be some combination of the
        // two, making it an inline composite.
        let ElementSize::InlineComposite(size) = size else {
            unreachable!()
        };
        self.copy_inline_composites(src, size, reader, dst, size, builder, len)
    }

    /// Copy from a source list to another, possibly upgraded, destination list. The source and
    /// destination are allowed to have different element sizes, but the destination MUST be a
    /// valid upgrade from the source size.
    fn copy_to_upgraded_list<'b>(
        &mut self,
        src: SegmentRef,
        reader: &ObjectReader,
        src_size: ElementSize,
        dst: SegmentRef<'b>,
        builder: &ObjectBuilder<'b>,
        dst_size: ElementSize,
        len: ElementCount,
    ) -> Result<(), E::Error> {
        use ElementSize::*;

        if src_size.is_empty() {
            // We don't need to do anything, there's no data to copy.
            return Ok(());
        }

        // Most of the time we don't actually upgrade
        if src_size == dst_size {
            return self.copy_list(src, reader, dst, builder, dst_size, len);
        }

        // The only valid upgrade is from data or pointer to struct, so at this point our
        // destination has to be an inline composite.
        let InlineComposite(dst_size) = dst_size else {
            unreachable!()
        };

        // Most of the time if we do upgrade it's to larger structs
        if let InlineComposite(src_size) = src_size {
            return self.copy_inline_composites(src, src_size, reader, dst, dst_size, builder, len);
        }

        // Or from a pointer to a struct with a pointer
        if let (Pointer, StructSize { data: 0, ptrs: 1 }) = (src_size, dst_size) {
            // The list is made up entirely of single pointer elements, so we can copy the whole
            // thing as a big pointer section.
            let len = SegmentOffset::new(src_size.total_words(len)).unwrap();
            return self.copy_ptr_section(src, reader, dst, builder, len);
        }

        // This is rarer but might be worth doing separately
        if let (EightBytes, StructSize { data: 1, ptrs: 0 }) = (src_size, dst_size) {
            // The list is made up entirely of single word elements, so we can copy the whole thing
            // with a single memcpy
            let total_size = SegmentOffset::new(src_size.total_words(len)).unwrap();
            let src_list = unsafe { reader.section_slice(src, total_size) };
            let dst_list = unsafe { builder.section_slice_mut(dst, total_size) };
            dst_list.copy_from_slice(src_list);
            return Ok(());
        }

        let dst_structs = unsafe { step_by_unchecked(dst, dst_size.len(), len) };

        macro_rules! copy_data {
            ($ty:ty) => {{
                let total_size = SegmentOffset::new(src_size.total_words(len)).unwrap();
                let src_words = unsafe { reader.section_slice(src, total_size) };
                let src_bytes =
                    &WireValue::<$ty>::from_word_slice(src_words)[..(len.get() as usize)];
                let dst_bytes =
                    dst_structs.map(|r| unsafe { &mut *r.as_ptr_mut().cast::<WireValue<$ty>>() });
                src_bytes
                    .iter()
                    .zip(dst_bytes)
                    .for_each(|(src, dst)| *dst = *src);
            }};
        }

        match src_size {
            Byte => copy_data!(u8),
            TwoBytes => copy_data!(u16),
            FourBytes => copy_data!(u32),
            EightBytes => copy_data!(u64),
            Pointer => {
                let src_ptrs = unsafe { iter_unchecked(src, len) };
                let dst_ptrs = dst_structs
                    .map(|dst| unsafe { dst.offset(dst_size.data.into()).as_ref_unchecked() });
                src_ptrs
                    .zip(dst_ptrs)
                    .filter(|(src, _)| !src.as_ref().is_null())
                    .try_for_each(|(src, dst)| self.copy_ptr(src, reader, dst, builder))?
            }
            _ => unreachable!(),
        }

        Ok(())
    }

    /// Like copy_to_upgraded_list, but allows copying into a specific point in the destination.
    /// This means most of the optimizations of copy_to_upgraded_list don't work anymore, since
    /// we might have to handle weird edge cases. Instead we attempt to do the copy as simply as
    /// possible except in the case of bits which are pain.
    fn copy_to_upgraded_list_at<'b>(
        &mut self,
        src: SegmentRef,
        reader: &ObjectReader,
        src_size: ElementSize,
        src_start: ElementCount,
        dst: SegmentRef<'b>,
        builder: &ObjectBuilder<'b>,
        dst_size: ElementSize,
        dst_start: ElementCount,
        len: ElementCount,
    ) -> Result<(), E::Error> {
        // First set up some common variables. `src_words` and `dst_words` are used in the data
        // copy macros lower down so we need to write them up here so we can use them in the macro
        let src_start_usize = src_start.get() as usize;
        let src_len = SegmentOffset::new(src_size.total_words(len)).unwrap();
        let src_words = unsafe { reader.section_slice(src, src_len) };

        let dst_start_usize = dst_start.get() as usize;
        let dst_len = SegmentOffset::new(src_size.total_words(len)).unwrap();
        let dst_words = unsafe { builder.section_slice_mut(dst, dst_len) };

        let len_usize = len.get() as usize;

        // A simple macro to do a memcpy by interpreting the word slices with all data as wire
        // values of the type $ty, then subslicing them based on the start of each section.
        macro_rules! copy_by_slice {
            ($ty:ty) => {{
                let src_data =
                    &WireValue::<$ty>::from_word_slice(src_words)[src_start_usize..len_usize];
                let dst_data = &mut WireValue::<$ty>::from_word_slice_mut(dst_words)
                    [dst_start_usize..len_usize];
                dst_data.copy_from_slice(src_data);
            }};
        }

        // Now we match. Since this library does not support arbitrary upgrades between different
        // data lists, we only need to check for the cases where two lists are the same type or
        // where a list is upgraded to an inline composite.
        use ElementSize::*;
        match (src_size, dst_size) {
            // Void values don't need to be copied so we do nothing
            (Void, _) => {}
            // Bit lists are special...
            (Bit, Bit) => {
                // Everyone is lazy and just do it the manual way.
                let src_bytes = WireValue::<u8>::from_word_slice(src_words);
                let dst_bytes = WireValue::<u8>::from_word_slice_mut(dst_words);

                let indexes = (0..len_usize).map(|i| (src_start_usize + i, dst_start_usize + i));
                for (src_idx, dst_idx) in indexes {
                    let (src_offset, src_bit) = (src_idx / 8, src_idx % 8);
                    let byte = src_bytes[src_offset].get();
                    let value = ((byte) & (1 << (src_bit))) != 0;

                    let (dst_offset, dst_bit) = (dst_idx / 8, dst_idx % 8);
                    let byte = &mut dst_bytes[dst_offset];
                    let new_byte = (byte.get() & !(1 << dst_bit)) | (u8::from(value) << dst_bit);
                    byte.set(new_byte);
                }
            }
            // In the case of simple data copies, we only need to use the copy_by_slice macro
            // which will use memcpy
            (Byte, Byte) => copy_by_slice!(u8),
            (TwoBytes, TwoBytes) => copy_by_slice!(u16),
            (FourBytes, FourBytes) => copy_by_slice!(u32),
            (EightBytes, EightBytes) => copy_by_slice!(u64),
            // Pointer sections can be shelled out to copy_ptr_section after updating the
            // src and dst pointers to start at the proper positions.
            (Pointer, Pointer) => {
                let src = unsafe { src.offset(src_start).as_ref_unchecked() };
                let dst = unsafe { dst.offset(dst_start).as_ref_unchecked() };
                self.copy_ptr_section(src, reader, dst, builder, len)?;
            }
            // And inline composites can also shell out to copy_inline_composites similarly
            // after updating the src and dst pointers.
            (InlineComposite(src_size), InlineComposite(dst_size)) => {
                // Shell out to copy_inline_composites by adjusting the src and dst ptrs
                let src_offset = SegmentOffset::new(src_size.total() * src_start.get()).unwrap();
                let src = unsafe { src.offset(src_offset).as_ref_unchecked() };

                let dst_offset = SegmentOffset::new(dst_size.total() * dst_start.get()).unwrap();
                let dst = unsafe { dst.offset(dst_offset).as_ref_unchecked() };

                self.copy_inline_composites(src, src_size, reader, dst, dst_size, builder, len)?;
            }
            (_, InlineComposite(dst_size)) => {
                // Now we know we're copying into an inline composite but aren't copying from one.
                // So we need to set up our iterator that will give us the start of the data and
                // pointer sections of each struct.
                let dst_offset = SegmentOffset::new(dst_size.total() * dst_start.get()).unwrap();
                let dst = unsafe { dst.offset(dst_offset).as_ref_unchecked() };
                let dst_structs = iter_inline_composites(dst, dst_size, len);

                // When we copy data we can simply cast the data section pointer to a pointer
                // for the wire value type itself.
                macro_rules! upgrade_data {
                    ($ty:ty) => {{
                        let src_data = &WireValue::<$ty>::from_word_slice(src_words)
                            [src_start_usize..len_usize];
                        let dst_data = dst_structs
                            .map(|(r, _)| unsafe { &mut *r.as_ptr_mut().cast::<WireValue<$ty>>() });
                        src_data
                            .iter()
                            .zip(dst_data)
                            .for_each(|(src, dst)| *dst = *src);
                    }};
                }

                match src_size {
                    Byte => upgrade_data!(u8),
                    TwoBytes => upgrade_data!(u16),
                    FourBytes => upgrade_data!(u32),
                    EightBytes => upgrade_data!(u64),
                    Pointer => {
                        let src: SegmentRef<'_> =
                            unsafe { src.offset(src_start).as_ref_unchecked() };
                        let src_ptrs = unsafe { iter_unchecked(src, len) };
                        for (src, dst) in src_ptrs.zip(dst_structs.map(|(_, p)| p)) {
                            self.copy_ptr(src, reader, dst, builder)?;
                        }
                    }
                    _ => unreachable!(),
                }
            }
            _ => unreachable!(),
        }

        Ok(())
    }

    /// Copy from one list of inline composites to another. The source and destination sizes are
    /// allowed to be different.
    fn copy_inline_composites<'b>(
        &mut self,
        src: SegmentRef,
        src_size: StructSize,
        reader: &ObjectReader,
        dst: SegmentRef<'b>,
        dst_size: StructSize,
        builder: &ObjectBuilder<'b>,
        len: ElementCount,
    ) -> Result<(), E::Error> {
        let src_structs = unsafe { step_by_unchecked(src, src_size.len(), len) };
        let dst_structs = unsafe { step_by_unchecked(dst, dst_size.len(), len) };

        for (src_data, dst_data) in src_structs.zip(dst_structs) {
            self.copy_struct(src_data, src_size, reader, dst_data, dst_size, builder)?;
        }
        Ok(())
    }

    /// Copy from one struct to another. The source and destination are allowed to have different
    /// sizes.
    fn copy_struct<'b>(
        &mut self,
        src: SegmentRef,
        src_size: StructSize,
        reader: &ObjectReader,
        dst: SegmentRef<'b>,
        dst_size: StructSize,
        builder: &ObjectBuilder<'b>,
    ) -> Result<(), E::Error> {
        let data_to_copy = cmp::min(src_size.data, dst_size.data);
        let src_data_section = unsafe { reader.section_slice(src, data_to_copy.into()) };
        let dst_data_section = unsafe { builder.section_slice_mut(dst, data_to_copy.into()) };
        dst_data_section.copy_from_slice(src_data_section);

        let ptrs_to_copy = cmp::min(src_size.ptrs, dst_size.ptrs);
        let src_ptrs = unsafe { src.offset(src_size.data.into()).as_ref_unchecked() };
        let dst_ptrs = unsafe { dst.offset(dst_size.data.into()).as_ref_unchecked() };
        self.copy_ptr_section(src_ptrs, reader, dst_ptrs, builder, ptrs_to_copy.into())
    }

    fn copy_ptr_section<'b>(
        &mut self,
        src: SegmentRef,
        reader: &ObjectReader,
        dst: SegmentRef<'b>,
        builder: &ObjectBuilder<'b>,
        len: SegmentOffset,
    ) -> Result<(), E::Error> {
        let src_ptrs = unsafe { iter_unchecked(src, len) };
        let dst_ptrs = unsafe { iter_unchecked(dst, len) };
        src_ptrs
            .zip(dst_ptrs)
            .filter(|(src, _)| !src.as_ref().is_null())
            .try_for_each(|(src, dst)| self.copy_ptr(src, reader, dst, builder))
    }

    #[inline]
    fn handle_err(&mut self, err: Error) -> Result<(), E::Error> {
        map_control_flow(self.err_handler.handle_err(err))
    }
}

fn calculate_canonical_struct_size(
    reader: &ObjectReader,
    src: SegmentRef,
    size: StructSize,
) -> StructSize {
    let mut dst_size = size;

    let src_data_section = unsafe { reader.section_slice(src, size.data.into()) };
    dst_size.data = trim_end_null_words(src_data_section).len() as u16;

    let src_ptrs = unsafe { src.offset(size.data.into()).as_ref_unchecked() };
    let src_ptr_section = unsafe { reader.section_slice(src_ptrs, size.ptrs.into()) };
    dst_size.ptrs = trim_end_null_words(src_ptr_section).len() as u16;

    dst_size
}

/// Returns an iterator over a set of inline composites, yielding the start
/// of the data and ptr sections.
fn iter_inline_composites(
    src: SegmentRef,
    size: StructSize,
    count: ElementCount,
) -> impl Iterator<Item = (SegmentRef, SegmentRef)> {
    unsafe {
        step_by_unchecked(src, size.len(), count).map(move |src_data| {
            let src_ptrs = src_data.offset(size.data.into()).as_ref_unchecked();
            (src_data, src_ptrs)
        })
    }
}

fn calculate_canonical_inline_composite_size(
    reader: &ObjectReader,
    src: SegmentRef,
    size: StructSize,
    count: ElementCount,
) -> StructSize {
    unsafe { step_by_unchecked(src, size.len(), count) }
        .map(|src_data| calculate_canonical_struct_size(reader, src_data, size))
        .reduce(StructSize::max)
        .unwrap_or(StructSize::EMPTY)
}

#[cfg(test)]
#[cfg(feature = "alloc")]
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

    use rustalloc::vec::Vec;

    /// Test all normal upgrades, specificly upgrades into structs
    #[test]
    fn valid_upgrades() {
        use ElementSize::*;

        let size5 = InlineComposite(StructSize { data: 3, ptrs: 2 });
        for size in [Void, Bit, Byte, TwoBytes, FourBytes, EightBytes, size5] {
            assert_eq!(size.upgrade_to(size), Some(size));
        }

        let empty = InlineComposite(StructSize::EMPTY);
        assert_eq!(Void.upgrade_to(empty), Some(empty));
        assert_eq!(Void.upgrade_to(size5), Some(size5));

        let one_data = InlineComposite(StructSize { data: 1, ptrs: 0 });
        for size in [Byte, TwoBytes, FourBytes, EightBytes] {
            assert_eq!(size.upgrade_to(one_data), Some(one_data));
            assert_eq!(size.upgrade_to(size5), Some(size5));
        }

        let one_ptr = InlineComposite(StructSize { data: 0, ptrs: 1 });
        assert_eq!(Pointer.upgrade_to(one_ptr), Some(one_ptr));

        assert_eq!(one_data.upgrade_to(size5), Some(size5));
        assert_eq!(one_ptr.upgrade_to(size5), Some(size5));
    }

    #[test]
    fn simple_struct() {
        let data = [
            Word([0x00, 0x00, 0x00, 0x00, 0x01, 0x00, 0x00, 0x00]),
            Word([0x01, 0x23, 0x45, 0x67, 0x89, 0xab, 0xcd, 0xef]),
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
        assert_eq!(34, segments.first().len());
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
