//! Tagged JavaScript value skeleton.
//!
//! This file reserves the value representation boundary without implementing
//! JavaScript coercion, arithmetic, or object behavior.

use crate::gc::GcRef;

const TAG_UNDEFINED: u64 = 0x01;
const TAG_NULL: u64 = 0x02;
const TAG_FALSE: u64 = 0x03;
const TAG_TRUE: u64 = 0x04;
const TAG_I32: u64 = 0x10;
const TAG_CELL: u64 = 0x20;
const TAG_DOUBLE: u64 = 0x30;
const TAG_MASK: u64 = 0xff;

/// Raw ABI/storage representation for a JavaScript value.
#[derive(Clone, Copy, Debug, Default, Eq, PartialEq)]
#[repr(transparent)]
pub struct EncodedJsValue(pub u64);

/// Tagged runtime transport value.
#[derive(Clone, Copy, Debug, Default, Eq, PartialEq)]
#[repr(transparent)]
pub struct JsValue(EncodedJsValue);

impl JsValue {
    pub const fn from_encoded(encoded: EncodedJsValue) -> Self {
        Self(encoded)
    }

    pub const fn encoded(self) -> EncodedJsValue {
        self.0
    }

    pub const fn undefined() -> Self {
        Self(EncodedJsValue(TAG_UNDEFINED))
    }

    pub const fn null() -> Self {
        Self(EncodedJsValue(TAG_NULL))
    }

    pub const fn from_bool(value: bool) -> Self {
        if value {
            Self(EncodedJsValue(TAG_TRUE))
        } else {
            Self(EncodedJsValue(TAG_FALSE))
        }
    }

    pub fn from_i32(value: i32) -> Self {
        Self(EncodedJsValue(((value as u32 as u64) << 8) | TAG_I32))
    }

    pub fn from_double(value: f64) -> Self {
        // Placeholder representation that preserves a classification tag but
        // does not promise NaN-boxing compatibility or exact payload recovery.
        Self(EncodedJsValue((value.to_bits() & !TAG_MASK) | TAG_DOUBLE))
    }

    pub fn from_cell<T: ?Sized>(cell: GcRef<T>) -> Self {
        let ptr_bits = cell.as_ptr() as *mut () as usize as u64;
        Self(EncodedJsValue((ptr_bits << 8) | TAG_CELL))
    }

    pub fn kind(self) -> ValueKind {
        match self.0 .0 & TAG_MASK {
            TAG_UNDEFINED => ValueKind::Undefined,
            TAG_NULL => ValueKind::Null,
            TAG_FALSE | TAG_TRUE => ValueKind::Boolean,
            TAG_I32 => ValueKind::Int32,
            TAG_CELL => ValueKind::Cell,
            TAG_DOUBLE => ValueKind::Double,
            _ => ValueKind::Unknown,
        }
    }

    pub fn classification(self) -> ValueClassification {
        match self.kind() {
            ValueKind::Undefined => ValueClassification::Immediate(ImmediateKind::Undefined),
            ValueKind::Null => ValueClassification::Immediate(ImmediateKind::Null),
            ValueKind::Boolean => ValueClassification::Immediate(ImmediateKind::Boolean),
            ValueKind::Int32 => ValueClassification::Immediate(ImmediateKind::Int32),
            ValueKind::Double => ValueClassification::Immediate(ImmediateKind::Double),
            ValueKind::Cell => ValueClassification::Cell(CellValue { encoded: self.0 }),
            ValueKind::Unknown => ValueClassification::Unknown(self.0),
        }
    }

    pub fn is_empty_or_deleted_sentinel(self) -> bool {
        matches!(self.kind(), ValueKind::Unknown) && self.0 .0 == 0
    }

    pub fn as_number(self) -> Option<NumberValue> {
        match self.kind() {
            ValueKind::Int32 => Some(NumberValue::Int32((self.0 .0 >> 8) as u32 as i32)),
            ValueKind::Double => Some(NumberValue::DoubleBits(EncodedDoubleBits(self.0 .0))),
            _ => None,
        }
    }

    pub fn as_cell(self) -> Option<CellValue> {
        (self.kind() == ValueKind::Cell).then_some(CellValue { encoded: self.0 })
    }
}

/// Safe classification result for a value.
#[derive(Clone, Copy, Debug, Eq, PartialEq)]
pub enum ValueKind {
    Undefined,
    Null,
    Boolean,
    Int32,
    Double,
    Cell,
    Unknown,
}

/// Immediate value category. This is separate from bit-level encoding.
#[derive(Clone, Copy, Debug, Eq, PartialEq)]
pub enum ImmediateKind {
    Undefined,
    Null,
    Boolean,
    Int32,
    Double,
}

/// Safe classification result. It answers "what domain owns this payload?"
/// without promising a concrete NaN-boxing layout.
#[derive(Clone, Copy, Debug, Eq, PartialEq)]
pub enum ValueClassification {
    Immediate(ImmediateKind),
    Cell(CellValue),
    Unknown(EncodedJsValue),
}

/// Number-specific view.
#[derive(Clone, Copy, Debug, Eq, PartialEq)]
pub enum NumberValue {
    Int32(i32),
    DoubleBits(EncodedDoubleBits),
}

/// Opaque double payload bits. Consumers must not rely on this matching C++ JSC.
#[derive(Clone, Copy, Debug, Default, Eq, PartialEq)]
#[repr(transparent)]
pub struct EncodedDoubleBits(pub u64);

/// Cell-containing value view.
///
/// Extracting a typed cell reference from this view is an unsafe boundary that
/// must prove heap ownership, pinning, and rooting. The skeleton keeps only the
/// encoded payload.
#[derive(Clone, Copy, Debug, Eq, PartialEq)]
pub struct CellValue {
    encoded: EncodedJsValue,
}

impl CellValue {
    pub fn encoded(self) -> EncodedJsValue {
        self.encoded
    }

    pub fn pointer_payload_bits(self) -> usize {
        (self.encoded.0 >> 8) as usize
    }
}
