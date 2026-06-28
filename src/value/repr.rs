//! Tagged JavaScript value representation.
//!
//! The live `JsValue` encoding below is a faithful port of C++ JSC's JSVALUE64
//! NaN-boxing (runtime/JSCJSValue.h:451-491, 950-1066): a reversible double
//! encoding (bit_cast + 2^49 `DoubleEncodeOffset`), int32 in the low 32 bits
//! with `NumberTag` set, and immediate sentinels for bool/null/undefined/
//! empty/deleted. See mcts_mem value-representation/tagged-encoding.md.
//!
//! TRANSITIONAL coexistence (D1a): two things still use the OLD low-byte tag
//! scheme and are deliberately NOT migrated in this batch.
//!   1. Cells are encoded `(ptr << 8) | TAG_CELL`, not as raw pointers. The
//!      genuine `isCell = !(v & NotCellMask)` test (JSCJSValue.h:998-1001) is
//!      unsound for a shifted pointer, so cells are classified by the leftover
//!      low byte `0x20` after the NumberTag/immediate tests. This is correct
//!      only while `(ptr << 8)` does not reach NumberTag bit 49, i.e.
//!      `ptr < 2^41`. S4 removes the shift and restores the single-mask test.
//!   2. The `ValueRepresentationLayout` apparatus and its `TAG_*` constants
//!      below describe a SEPARATE transitional layout read only by the baseline
//!      JIT emitter (jit/emitter.rs, jit/arm64_baseline.rs) to stamp/compare
//!      low-byte tags in emitted machine code. It is no longer the live JsValue
//!      encoding; it disagrees with it (e.g. undefined is 0x01 there, 0xa here).
//!      That is acceptable only because there is no live execute-JIT-then-feed-
//!      JsValue path yet (audit D1a). Porting the baseline JIT to JSVALUE64 and
//!      collapsing this apparatus is deferred (returned as an architecture
//!      question by the D1a implementer).

use crate::gc::{GcRef, HeapId, RootId, RootKind, RootRecord, RootSet, RootSetSemanticError};

// === Live JsValue encoding: JSVALUE64 (faithful port, JSCJSValue.h:451-491) ===

/// `NumberTag`: all 15 high bits set => int32; some-but-not-all => double
/// (JSCJSValue.h:457).
const NUMBER_TAG: u64 = 0xfffe_0000_0000_0000;
/// `DoubleEncodeOffset` = 2^49; biases a raw double so its encoded form lands in
/// the 0x0002..0xFFFC high-bit window (JSCJSValue.h:450-452, PureNaN.h:79-80).
const DOUBLE_ENCODE_OFFSET: u64 = 1 << 49;
/// `OtherTag` (bit 1): set on every non-numeric immediate (JSCJSValue.h:464).
const OTHER_TAG: u64 = 0x2;
/// `BoolTag` (bit 2) (JSCJSValue.h:465).
const BOOL_TAG: u64 = 0x4;
/// `UndefinedTag` (bit 3) (JSCJSValue.h:466).
const UNDEFINED_TAG: u64 = 0x8;
/// `NotCellMask` = NumberTag | OtherTag; in genuine JSVALUE64 a value is a cell
/// iff none of these bits are set (JSCJSValue.h:479). Used by the faithful raw
/// arm (feature `s4_raw_cell`); dead in the default transitional build (cells
/// are the shifted encoding), hence `#[allow(dead_code)]`.
#[allow(dead_code)]
const NOT_CELL_MASK: u64 = NUMBER_TAG | OTHER_TAG;
/// Combined non-numeric immediate values (JSCJSValue.h:472-475).
const VALUE_FALSE: u64 = OTHER_TAG | BOOL_TAG; // 0x6
const VALUE_TRUE: u64 = OTHER_TAG | BOOL_TAG | 1; // 0x7
const VALUE_UNDEFINED: u64 = OTHER_TAG | UNDEFINED_TAG; // 0xa
const VALUE_NULL: u64 = OTHER_TAG; // 0x2
/// Never visible to JS: Empty (array holes / uninitialized) is 0x0 so a
/// zero-initialized slot decodes to it; Deleted (hash tables) is 0x4
/// (JSCJSValue.h:483-488).
const VALUE_EMPTY: u64 = 0x0;
#[allow(dead_code)]
const VALUE_DELETED: u64 = 0x4;
/// Pure NaN bit pattern (PureNaN.h:75); impure NaNs are purified before boxing.
const PNAN_BITS: u64 = 0x7ff8_0000_0000_0000;

// === TRANSITIONAL baseline-JIT low-byte tag layout (NOT the live encoding) ===
//
// Read only by the baseline JIT emitter and the `ValueRepresentationLayout`
// apparatus below. `TAG_CELL` is additionally the live transitional cell tag
// (see `from_cell`); the rest describe the JIT's emitted low-byte layout.
// The immediate descriptors now carry the JSVALUE64 immediate VALUES (the live
// `VALUE_*` constants), so these original low-byte immediate tags are retained
// only to document the superseded scheme and for layout-apparatus unit tests.
#[allow(dead_code)]
const TAG_UNDEFINED: u64 = 0x01;
#[allow(dead_code)]
const TAG_NULL: u64 = 0x02;
#[allow(dead_code)]
const TAG_FALSE: u64 = 0x03;
#[allow(dead_code)]
const TAG_TRUE: u64 = 0x04;
const TAG_I32: u64 = 0x10;
const TAG_CELL: u64 = 0x20;
const TAG_DOUBLE: u64 = 0x30;
const TAG_MASK: u64 = 0xff;

/// Static owner for value representation schema.
#[derive(Clone, Copy, Debug, Default, Eq, PartialEq)]
pub enum ValueRepresentationOwner {
    /// `value::repr` owns the canonical Rust value layout schema.
    #[default]
    ValueReprSchema,
    /// A future generated table owns rows copied from C++ JSC value encoding.
    GeneratedCppValueEncoding,
}

/// Registry mutation authority for value representation metadata.
#[derive(Clone, Copy, Debug, Default, Eq, PartialEq)]
pub enum ValueRepresentationAuthority {
    /// The representation schema is compiled static data.
    #[default]
    StaticReadOnly,
    /// A generated source refresh may replace the compiled schema.
    GeneratedSourceRefresh,
}

/// One immediate tag row in the value representation.
#[derive(Clone, Copy, Debug, Eq, PartialEq)]
pub struct ImmediateTagDescriptor {
    pub kind: ImmediateKind,
    pub tag: u64,
    pub canonical_name: &'static str,
}

/// Immutable value representation layout schema.
///
/// This describes the current Rust transport bit layout only. It does not
/// decode cells, prove liveness, allocate boxed values, or promise C++ ABI
/// compatibility.
#[derive(Clone, Copy, Debug, Eq, PartialEq)]
pub struct ValueRepresentationLayout {
    pub name: &'static str,
    pub owner: ValueRepresentationOwner,
    pub authority: ValueRepresentationAuthority,
    pub storage_bits: u8,
    pub tag_mask: u64,
    pub payload_shift: u8,
    pub cell_tag: u64,
    pub double_tag: u64,
    pub immediate_tags: &'static [ImmediateTagDescriptor],
}

impl ValueRepresentationLayout {
    pub const fn immediate_tags(&self) -> &'static [ImmediateTagDescriptor] {
        self.immediate_tags
    }

    pub fn tag_for_immediate_kind(&self, kind: ImmediateKind) -> Option<u64> {
        self.immediate_tags
            .iter()
            .find(|descriptor| descriptor.kind == kind)
            .map(|descriptor| descriptor.tag)
    }

    pub fn tag_for_immediate_name(&self, name: &str) -> Option<u64> {
        self.immediate_tags
            .iter()
            .find(|descriptor| descriptor.canonical_name == name)
            .map(|descriptor| descriptor.tag)
    }

    pub fn tag_bits(&self, encoded: EncodedJsValue) -> u64 {
        encoded.0 & self.tag_mask
    }

    pub fn payload_bits(&self, encoded: EncodedJsValue) -> u64 {
        encoded.0 >> self.payload_shift
    }

    pub fn classify_encoded(&self, encoded: EncodedJsValue) -> ValueClassification {
        let tag = self.tag_bits(encoded);
        if tag == self.cell_tag {
            return ValueClassification::Cell(CellValue { encoded });
        }

        if let Some(descriptor) = self
            .immediate_tags
            .iter()
            .find(|descriptor| descriptor.tag == tag)
        {
            return ValueClassification::Immediate(descriptor.kind);
        }

        ValueClassification::Unknown(encoded)
    }

    pub fn kind_for_encoded(&self, encoded: EncodedJsValue) -> ValueKind {
        match self.classify_encoded(encoded) {
            ValueClassification::Immediate(ImmediateKind::Undefined) => ValueKind::Undefined,
            ValueClassification::Immediate(ImmediateKind::Null) => ValueKind::Null,
            ValueClassification::Immediate(ImmediateKind::Boolean) => ValueKind::Boolean,
            ValueClassification::Immediate(ImmediateKind::Int32) => ValueKind::Int32,
            ValueClassification::Immediate(ImmediateKind::Double) => ValueKind::Double,
            ValueClassification::Cell(_) => ValueKind::Cell,
            ValueClassification::Unknown(_) => ValueKind::Unknown,
        }
    }

    pub fn encode_immediate_name(&self, name: &str) -> Result<EncodedJsValue, ValueEncodingError> {
        self.validate().map_err(ValueEncodingError::InvalidLayout)?;
        self.tag_for_immediate_name(name)
            .map(EncodedJsValue)
            .ok_or(ValueEncodingError::UnknownImmediateName)
    }

    pub fn encode_i32(&self, value: i32) -> Result<EncodedJsValue, ValueEncodingError> {
        self.validate().map_err(ValueEncodingError::InvalidLayout)?;
        let tag = self.tag_for_immediate_kind(ImmediateKind::Int32).ok_or(
            ValueEncodingError::MissingImmediateKind(ImmediateKind::Int32),
        )?;
        Ok(EncodedJsValue(
            ((value as u32 as u64) << self.payload_shift) | tag,
        ))
    }

    pub fn encode_double_bits(&self, bits: u64) -> Result<EncodedJsValue, ValueEncodingError> {
        self.validate().map_err(ValueEncodingError::InvalidLayout)?;
        Ok(EncodedJsValue((bits & !self.tag_mask) | self.double_tag))
    }

    pub fn encode_cell_payload(
        &self,
        payload_bits: usize,
    ) -> Result<EncodedJsValue, ValueEncodingError> {
        self.validate().map_err(ValueEncodingError::InvalidLayout)?;
        let shifted = (payload_bits as u64)
            .checked_shl(self.payload_shift.into())
            .ok_or(ValueEncodingError::PayloadOverflow)?;
        if shifted >> self.payload_shift != payload_bits as u64 {
            return Err(ValueEncodingError::PayloadOverflow);
        }
        Ok(EncodedJsValue(shifted | self.cell_tag))
    }

    pub fn validate(&self) -> Result<(), ValueRepresentationValidationError> {
        if self.name.is_empty() {
            return Err(ValueRepresentationValidationError::EmptyLayoutName);
        }
        if self.storage_bits == 0
            || self.storage_bits > 64
            || self.payload_shift >= self.storage_bits
            || self.tag_mask == 0
            || self.cell_tag & !self.tag_mask != 0
            || self.double_tag & !self.tag_mask != 0
            || self.cell_tag == self.double_tag
        {
            return Err(ValueRepresentationValidationError::InvalidTagGeometry);
        }

        let mut saw_double_tag = false;
        for (index, descriptor) in self.immediate_tags.iter().enumerate() {
            descriptor.validate(self.tag_mask)?;
            if descriptor.tag == self.cell_tag {
                return Err(ValueRepresentationValidationError::ImmediateUsesCellTag(
                    descriptor.canonical_name,
                ));
            }
            if descriptor.tag == self.double_tag {
                saw_double_tag = true;
            }
            if self.immediate_tags[..index]
                .iter()
                .any(|previous| previous.tag == descriptor.tag)
            {
                return Err(ValueRepresentationValidationError::DuplicateImmediateTag(
                    descriptor.tag,
                ));
            }
            if self.immediate_tags[..index]
                .iter()
                .any(|previous| previous.canonical_name == descriptor.canonical_name)
            {
                return Err(ValueRepresentationValidationError::DuplicateImmediateName(
                    descriptor.canonical_name,
                ));
            }
        }

        if !saw_double_tag {
            return Err(ValueRepresentationValidationError::MissingDoubleTag);
        }

        Ok(())
    }
}

#[derive(Clone, Copy, Debug, Eq, PartialEq)]
pub enum ValueRepresentationValidationError {
    EmptyLayoutName,
    EmptyTagName,
    InvalidTagGeometry,
    DuplicateImmediateTag(u64),
    DuplicateImmediateName(&'static str),
    ImmediateTagOutsideMask(&'static str),
    ImmediateUsesCellTag(&'static str),
    MissingDoubleTag,
}

#[derive(Clone, Copy, Debug, Eq, PartialEq)]
pub enum ValueEncodingError {
    InvalidLayout(ValueRepresentationValidationError),
    UnknownImmediateName,
    MissingImmediateKind(ImmediateKind),
    PayloadOverflow,
}

impl ImmediateTagDescriptor {
    pub const fn new(kind: ImmediateKind, tag: u64, canonical_name: &'static str) -> Self {
        Self {
            kind,
            tag,
            canonical_name,
        }
    }

    pub fn validate(&self, tag_mask: u64) -> Result<(), ValueRepresentationValidationError> {
        if self.canonical_name.is_empty() {
            return Err(ValueRepresentationValidationError::EmptyTagName);
        }
        if self.tag & !tag_mask != 0 {
            return Err(ValueRepresentationValidationError::ImmediateTagOutsideMask(
                self.canonical_name,
            ));
        }
        Ok(())
    }
}

#[derive(Clone, Copy, Debug, Eq, PartialEq)]
pub struct ValueRepresentationLayoutBuilder {
    layout: ValueRepresentationLayout,
}

impl ValueRepresentationLayoutBuilder {
    pub const fn new(
        name: &'static str,
        immediate_tags: &'static [ImmediateTagDescriptor],
    ) -> Self {
        Self {
            layout: ValueRepresentationLayout {
                name,
                owner: ValueRepresentationOwner::ValueReprSchema,
                authority: ValueRepresentationAuthority::StaticReadOnly,
                storage_bits: 64,
                tag_mask: TAG_MASK,
                payload_shift: 8,
                cell_tag: TAG_CELL,
                double_tag: TAG_DOUBLE,
                immediate_tags,
            },
        }
    }

    pub const fn owner(mut self, owner: ValueRepresentationOwner) -> Self {
        self.layout.owner = owner;
        self
    }

    pub const fn authority(mut self, authority: ValueRepresentationAuthority) -> Self {
        self.layout.authority = authority;
        self
    }

    pub const fn storage_bits(mut self, storage_bits: u8) -> Self {
        self.layout.storage_bits = storage_bits;
        self
    }

    pub const fn tag_mask(mut self, tag_mask: u64) -> Self {
        self.layout.tag_mask = tag_mask;
        self
    }

    pub const fn payload_shift(mut self, payload_shift: u8) -> Self {
        self.layout.payload_shift = payload_shift;
        self
    }

    pub const fn cell_tag(mut self, cell_tag: u64) -> Self {
        self.layout.cell_tag = cell_tag;
        self
    }

    pub const fn double_tag(mut self, double_tag: u64) -> Self {
        self.layout.double_tag = double_tag;
        self
    }

    pub fn build(self) -> Result<ValueRepresentationLayout, ValueRepresentationValidationError> {
        self.layout.validate()?;
        Ok(self.layout)
    }
}

// The immediate descriptors now carry the JSVALUE64 immediate VALUES so the
// baseline JIT contract stamps/compares the same bits the live `JsValue`
// encoding uses (undefined 0xa / null 0x2 / false 0x6 / true 0x7,
// JSCJSValue.h:472-491). The `int32` and `double` rows stay on the transitional
// low-byte tags that the x86 byte-emitter still uses for int32/double machine
// code (the live JSVALUE64 int32 `NumberTag | u32` is carried by the contract's
// `number_tag`, JSCJSValue.h:1023-1026); `double` is a placeholder (doubles are
// recognized by isNumber/isInt32, not a low-byte tag).
pub const STATIC_IMMEDIATE_TAG_DESCRIPTORS: &[ImmediateTagDescriptor] = &[
    ImmediateTagDescriptor {
        kind: ImmediateKind::Undefined,
        tag: VALUE_UNDEFINED,
        canonical_name: "undefined",
    },
    ImmediateTagDescriptor {
        kind: ImmediateKind::Null,
        tag: VALUE_NULL,
        canonical_name: "null",
    },
    ImmediateTagDescriptor {
        kind: ImmediateKind::Boolean,
        tag: VALUE_FALSE,
        canonical_name: "false",
    },
    ImmediateTagDescriptor {
        kind: ImmediateKind::Boolean,
        tag: VALUE_TRUE,
        canonical_name: "true",
    },
    ImmediateTagDescriptor {
        kind: ImmediateKind::Int32,
        tag: TAG_I32,
        canonical_name: "int32",
    },
    ImmediateTagDescriptor {
        kind: ImmediateKind::Double,
        tag: TAG_DOUBLE,
        canonical_name: "double",
    },
];

/// TRANSITIONAL (D1a): this layout describes the baseline JIT's low-byte tag
/// scheme, which is read only by the JIT emitter to stamp/compare tags in
/// emitted machine code. It is NO LONGER the live `JsValue` encoding (that is
/// JSVALUE64; see the module header). Do not classify live `JsValue` bits
/// through this layout. Collapsing it is deferred to the baseline-JIT JSVALUE64
/// port.
pub const STATIC_VALUE_REPRESENTATION_LAYOUT: ValueRepresentationLayout =
    ValueRepresentationLayout {
        name: "value.repr.encoded-js-value",
        owner: ValueRepresentationOwner::ValueReprSchema,
        authority: ValueRepresentationAuthority::StaticReadOnly,
        storage_bits: 64,
        tag_mask: TAG_MASK,
        payload_shift: 8,
        cell_tag: TAG_CELL,
        double_tag: TAG_DOUBLE,
        immediate_tags: STATIC_IMMEDIATE_TAG_DESCRIPTORS,
    };

pub const fn static_value_representation_layout() -> &'static ValueRepresentationLayout {
    &STATIC_VALUE_REPRESENTATION_LAYOUT
}

/// Raw ABI/storage representation for a JavaScript value.
///
/// These bits are owned by the value layer. They are not a borrow proof, root,
/// or authority to interpret heap-cell identity.
#[derive(Clone, Copy, Debug, Default, Eq, PartialEq)]
#[repr(transparent)]
pub struct EncodedJsValue(pub u64);

#[derive(Clone, Debug, Eq, PartialEq)]
pub struct ValueStackSnapshot {
    pub layout_name: &'static str,
    pub slots: Vec<ValueStackSlot>,
}

impl ValueStackSnapshot {
    pub fn from_values(values: &[JsValue]) -> Result<Self, ValueStackConversionError> {
        encode_value_stack(values)
    }

    pub fn decode_values(&self) -> Result<Vec<JsValue>, ValueStackConversionError> {
        decode_value_stack(self)
    }

    pub fn len(&self) -> usize {
        self.slots.len()
    }

    pub fn is_empty(&self) -> bool {
        self.slots.is_empty()
    }
}

#[derive(Clone, Copy, Debug, Eq, PartialEq)]
pub struct ValueStackSlot {
    pub index: usize,
    pub encoded: EncodedJsValue,
    pub kind: ValueKind,
}

#[derive(Clone, Copy, Debug, Eq, PartialEq)]
pub enum ValueStackConversionError {
    InvalidLayout(ValueRepresentationValidationError),
    LayoutMismatch,
    NonContiguousSlot { expected: usize, actual: usize },
}

pub fn encode_value_stack(
    values: &[JsValue],
) -> Result<ValueStackSnapshot, ValueStackConversionError> {
    let layout = static_value_representation_layout();
    layout
        .validate()
        .map_err(ValueStackConversionError::InvalidLayout)?;
    let slots = values
        .iter()
        .enumerate()
        .map(|(index, value)| ValueStackSlot {
            index,
            encoded: value.encoded(),
            // Classify via the live JSVALUE64 encoding. The `layout` apparatus
            // describes the transitional baseline-JIT low-byte tags, which no
            // longer match the live JsValue bits, so it must not classify here.
            kind: value.kind(),
        })
        .collect();

    Ok(ValueStackSnapshot {
        layout_name: layout.name,
        slots,
    })
}

pub fn decode_value_stack(
    snapshot: &ValueStackSnapshot,
) -> Result<Vec<JsValue>, ValueStackConversionError> {
    let layout = static_value_representation_layout();
    layout
        .validate()
        .map_err(ValueStackConversionError::InvalidLayout)?;
    if snapshot.layout_name != layout.name {
        return Err(ValueStackConversionError::LayoutMismatch);
    }

    let mut values = Vec::with_capacity(snapshot.slots.len());
    for (expected, slot) in snapshot.slots.iter().enumerate() {
        if slot.index != expected {
            return Err(ValueStackConversionError::NonContiguousSlot {
                expected,
                actual: slot.index,
            });
        }
        values.push(JsValue::from_encoded(slot.encoded));
    }

    Ok(values)
}

/// Tagged runtime transport value.
///
/// `JsValue` is the canonical value representation. Runtime aliases such as
/// `RuntimeValue` should re-export this type rather than introducing a second
/// representation owner.
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
        // JSCJSValue.h:972-975 (ValueUndefined).
        Self(EncodedJsValue(VALUE_UNDEFINED))
    }

    pub const fn null() -> Self {
        // JSCJSValue.h:967-970 (ValueNull).
        Self(EncodedJsValue(VALUE_NULL))
    }

    pub const fn from_bool(value: bool) -> Self {
        // JSCJSValue.h:977-985 (ValueTrue / ValueFalse).
        if value {
            Self(EncodedJsValue(VALUE_TRUE))
        } else {
            Self(EncodedJsValue(VALUE_FALSE))
        }
    }

    pub fn from_i32(value: i32) -> Self {
        // JSCJSValue.h:1023-1026: NumberTag | static_cast<uint32_t>(i).
        Self(EncodedJsValue(NUMBER_TAG | (value as u32 as u64)))
    }

    pub fn from_double(value: f64) -> Self {
        // Faithful port of JSValue(double) (JSCJSValueInlines.h:58-65), folding
        // in purifyNaN (PureNaN.h:98-103) so the port never produces an
        // impure-NaN encoding. C++ leaves purifyNaN to callers and only ASSERTs
        // purity in the EncodeAsDouble ctor; the port enforces it at the single
        // construction chokepoint, which is the safe equivalent.
        let value = if value.is_nan() {
            f64::from_bits(PNAN_BITS)
        } else {
            value
        };
        // tryConvertToStrictInt32 (MathExtras.h:1048-1071): canonicalize an
        // exactly representable integral double to int32, exactly as JSC.
        //
        // DIVERGENCE (transitional, benign): Rust `value as i32` saturates an
        // out-of-range input while JSC's truncateDoubleToInt32 wraps modulo
        // 2^32. The `as_i32 as f64 == value` guard rejects every out-of-range
        // value either way, so the accepted set is identical. -0.0 is excluded
        // by the sign guard, matching JSC (-0.0 is not a strict int32).
        if value.is_finite() {
            let as_i32 = value as i32;
            if as_i32 as f64 == value && !(as_i32 == 0 && value.is_sign_negative()) {
                return Self::from_i32(as_i32);
            }
        }
        // EncodeAsDouble (JSCJSValue.h:1017-1021): reversible bit_cast + offset.
        Self(EncodedJsValue(
            value.to_bits().wrapping_add(DOUBLE_ENCODE_OFFSET),
        ))
    }

    pub fn from_cell<T: ?Sized>(cell: GcRef<T>) -> Self {
        // S4 value-path cfg-fork (feature `s4_raw_cell`). JSC encodes a cell as
        // the raw pointer: `u.asInt64 = reinterpret_cast<uintptr_t>(ptr)`
        // (JSCJSValue.h:905-907); isCell is then `!(v & NotCellMask)`
        // (JSCJSValue.h:998-1001), sound only for a raw pointer whose top 16
        // bits and OtherTag bit 1 are clear. Both arms compile so the landed S4
        // cell-arena core can be wired behind the flag.
        let ptr_bits = cell.as_ptr() as *mut () as usize as u64;
        #[cfg(not(feature = "s4_raw_cell"))]
        {
            // TRANSITIONAL cell encoding (default, unchanged). The port shifts
            // the pointer left 8 and tags 0x20, so the genuine NotCellMask test
            // is UNSOUND here (a shifted pointer reaching NumberTag bit 49 would
            // alias a double). Cells are recognized by the leftover low byte
            // 0x20 after the NumberTag/immediate tests (see `kind`/`is_cell`),
            // correct only while `(ptr << 8)` does not reach bit 49, i.e.
            // `ptr < 2^41`.
            debug_assert!(
                (ptr_bits << 8) & NUMBER_TAG == 0,
                "cell pointer too high for transitional JSVALUE64 coexistence (ptr must be < 2^41)"
            );
            Self(EncodedJsValue((ptr_bits << 8) | TAG_CELL))
        }
        #[cfg(feature = "s4_raw_cell")]
        {
            // FAITHFUL raw JSVALUE64 (JSCJSValue.h:905-907): store the raw
            // pointer bits unshifted. JSC relies on the pointer carrying none of
            // the NotCellMask bits (top-16 number/double bits or OtherTag bit 1)
            // so `asCell`/`isCell` round-trip; assert that invariant here.
            debug_assert!(
                (ptr_bits & NOT_CELL_MASK) == 0,
                "raw cell pointer overlaps NotCellMask (top-16/OtherTag bits must be clear)"
            );
            Self(EncodedJsValue(ptr_bits))
        }
    }

    /// JSCJSValue.h:1034-1037 (`isNumber`).
    pub fn is_number(self) -> bool {
        self.0 .0 & NUMBER_TAG != 0
    }

    /// JSCJSValue.h:1003-1006 (`isInt32`).
    pub fn is_int32(self) -> bool {
        (self.0 .0 & NUMBER_TAG) == NUMBER_TAG
    }

    /// JSCJSValue.h:962-965 (`isDouble`).
    pub fn is_double(self) -> bool {
        self.is_number() && !self.is_int32()
    }

    /// JSC uses `!(v & NotCellMask)` (JSCJSValue.h:998-1001). S4 value-path
    /// cfg-fork: the default transitional build cannot use that mask (cells are
    /// shifted `(ptr << 8) | 0x20`), so it classifies by the leftover low byte
    /// after the NumberTag/immediate tests, sound only while cell pointers stay
    /// below 2^41; the `s4_raw_cell` arm restores the faithful single-mask test.
    pub fn is_cell(self) -> bool {
        #[cfg(not(feature = "s4_raw_cell"))]
        {
            !self.is_number() && (self.0 .0 & TAG_MASK) == TAG_CELL
        }
        #[cfg(feature = "s4_raw_cell")]
        {
            // FAITHFUL JSVALUE64 (JSCJSValue.h:998-1001): a value is a cell iff
            // none of the NotCellMask bits are set. NumberTag is part of the
            // mask, so numbers are excluded without a separate isNumber test.
            // (As in JSC, ValueEmpty 0x0 also satisfies this; it is the
            // zero-page Empty sentinel filtered by `isEmpty`, not by isCell.)
            (self.0 .0 & NOT_CELL_MASK) == 0
        }
    }

    /// JSCJSValue.h:987-991 (`isUndefinedOrNull`): undefined and null differ
    /// only in the UndefinedTag bit.
    pub fn is_undefined_or_null(self) -> bool {
        (self.0 .0 & !UNDEFINED_TAG) == VALUE_NULL
    }

    pub fn kind(self) -> ValueKind {
        // Ordered hybrid classifier over the live JSVALUE64 encoding: NumberTag
        // first (a double carries no low-byte tag), then exact-compare
        // immediates, then the TRANSITIONAL low-byte cell tag, with the
        // Empty (0x0) and Deleted (0x4) sentinels falling through to Unknown.
        let bits = self.0 .0;
        if bits & NUMBER_TAG != 0 {
            if (bits & NUMBER_TAG) == NUMBER_TAG {
                ValueKind::Int32
            } else {
                ValueKind::Double
            }
        } else if bits == VALUE_UNDEFINED {
            ValueKind::Undefined
        } else if bits == VALUE_NULL {
            ValueKind::Null
        } else if (bits & !1) == VALUE_FALSE {
            ValueKind::Boolean
        } else if (bits & TAG_MASK) == TAG_CELL {
            ValueKind::Cell
        } else {
            ValueKind::Unknown
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
        matches!(self.kind(), ValueKind::Unknown) && self.0 .0 == VALUE_EMPTY
    }

    pub fn as_number(self) -> Option<NumberValue> {
        if self.is_int32() {
            // JSCJSValue.h:956-960 (`asInt32`): low 32 bits as int32.
            Some(NumberValue::Int32(self.0 .0 as u32 as i32))
        } else if self.is_double() {
            Some(NumberValue::DoubleBits(EncodedDoubleBits(self.0 .0)))
        } else {
            None
        }
    }

    pub fn as_cell(self) -> Option<CellValue> {
        self.is_cell().then_some(CellValue { encoded: self.0 })
    }

    pub fn is_boolean(self) -> bool {
        // JSCJSValue.h:993-996 (`isBoolean`).
        (self.0 .0 & !1) == VALUE_FALSE
    }

    pub fn as_bool(self) -> Option<bool> {
        // JSCJSValue.h:950-954 (`asBoolean`): == ValueTrue, gated by isBoolean.
        if self.is_boolean() {
            Some(self.0 .0 == VALUE_TRUE)
        } else {
            None
        }
    }

    pub fn is_primitive(self) -> bool {
        !matches!(self.kind(), ValueKind::Cell | ValueKind::Unknown)
    }

    pub fn strict_equals(self, other: Self) -> bool {
        match (self.kind(), other.kind()) {
            (ValueKind::Undefined, ValueKind::Undefined)
            | (ValueKind::Null, ValueKind::Null)
            | (ValueKind::Boolean, ValueKind::Boolean)
            | (ValueKind::Cell, ValueKind::Cell) => self.0 == other.0,
            (ValueKind::Int32 | ValueKind::Double, ValueKind::Int32 | ValueKind::Double) => {
                numeric_equal(self, other, NumberEqualityMode::Strict)
            }
            _ => false,
        }
    }

    pub fn same_value(self, other: Self) -> bool {
        match (self.kind(), other.kind()) {
            (ValueKind::Int32 | ValueKind::Double, ValueKind::Int32 | ValueKind::Double) => {
                numeric_equal(self, other, NumberEqualityMode::SameValue)
            }
            _ => self.strict_equals(other),
        }
    }

    pub fn same_value_zero(self, other: Self) -> bool {
        match (self.kind(), other.kind()) {
            (ValueKind::Int32 | ValueKind::Double, ValueKind::Int32 | ValueKind::Double) => {
                numeric_equal(self, other, NumberEqualityMode::SameValueZero)
            }
            _ => self.strict_equals(other),
        }
    }

    pub fn pure_to_boolean(self) -> bool {
        match self.kind() {
            ValueKind::Undefined | ValueKind::Null | ValueKind::Unknown => false,
            ValueKind::Boolean => self.as_bool().unwrap_or(false),
            ValueKind::Int32 => !matches!(self.as_number(), Some(NumberValue::Int32(0))),
            ValueKind::Double => match self.as_number() {
                Some(NumberValue::DoubleBits(bits)) => {
                    let value = bits.to_f64();
                    value != 0.0 && !value.is_nan()
                }
                _ => false,
            },
            ValueKind::Cell => true,
        }
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
///
/// `Cell` means the value carries a heap-owned reference-shaped payload. It
/// does not transfer cell ownership to the value layer.
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

impl EncodedDoubleBits {
    pub fn to_f64(self) -> f64 {
        // JSCJSValue.h:1028-1032 (`asDouble`): reverse the DoubleEncodeOffset
        // bias. Fully reversible -- no precision loss (was a lossy low-byte
        // mask before D1a).
        f64::from_bits(self.0.wrapping_sub(DOUBLE_ENCODE_OFFSET))
    }
}

#[derive(Clone, Copy, Debug, Eq, PartialEq)]
enum NumberEqualityMode {
    Strict,
    SameValue,
    SameValueZero,
}

fn numeric_equal(left: JsValue, right: JsValue, mode: NumberEqualityMode) -> bool {
    let Some(left) = numeric_as_f64(left) else {
        return false;
    };
    let Some(right) = numeric_as_f64(right) else {
        return false;
    };

    if left.is_nan() || right.is_nan() {
        return mode == NumberEqualityMode::SameValue && left.is_nan() && right.is_nan();
    }

    if left == 0.0 && right == 0.0 {
        return mode != NumberEqualityMode::SameValue
            || left.is_sign_positive() == right.is_sign_positive();
    }

    left == right
}

fn numeric_as_f64(value: JsValue) -> Option<f64> {
    match value.as_number()? {
        NumberValue::Int32(value) => Some(value as f64),
        NumberValue::DoubleBits(bits) => Some(bits.to_f64()),
    }
}

/// Cell-containing value view.
///
/// Extracting a typed cell reference from this view is an unsafe boundary that
/// must prove heap ownership, pinning, and rooting. The skeleton keeps only the
/// encoded payload. This is deliberately not a `CellId`; only `gc` may define
/// how raw heap-cell identity maps to storage.
#[derive(Clone, Copy, Debug, Eq, PartialEq)]
pub struct CellValue {
    encoded: EncodedJsValue,
}

impl CellValue {
    pub fn encoded(self) -> EncodedJsValue {
        self.encoded
    }

    pub fn pointer_payload_bits(self) -> usize {
        // S4 value-path cfg-fork: the default transitional encoding shifts the
        // pointer left 8 (`(ptr << 8) | 0x20`), so recover it with `>> 8`; the
        // faithful `s4_raw_cell` arm stores the raw pointer, so the bits ARE the
        // pointer (JSCJSValue.h:1039-1043, `asCell` returns `u.ptr`).
        #[cfg(not(feature = "s4_raw_cell"))]
        {
            (self.encoded.0 >> 8) as usize
        }
        #[cfg(feature = "s4_raw_cell")]
        {
            self.encoded.0 as usize
        }
    }
}

/// Root descriptor for a cell-containing value stack slot.
#[derive(Clone, Copy, Debug, Eq, PartialEq)]
pub struct ValueStackRootDescriptor {
    pub root: RootRecord,
    pub slot: ValueStackSlot,
    pub cell: CellValue,
}

/// Precise root plan for a value stack snapshot.
#[derive(Clone, Debug, Eq, PartialEq)]
pub struct ValueStackRootPlan {
    pub heap: HeapId,
    pub roots: Vec<ValueStackRootDescriptor>,
}

#[derive(Clone, Copy, Debug, Eq, PartialEq)]
pub enum ValueStackRootError {
    Conversion(ValueStackConversionError),
    RootSet(RootSetSemanticError),
    RootIdOverflow,
}

impl From<RootSetSemanticError> for ValueStackRootError {
    fn from(error: RootSetSemanticError) -> Self {
        Self::RootSet(error)
    }
}

impl From<ValueStackConversionError> for ValueStackRootError {
    fn from(error: ValueStackConversionError) -> Self {
        Self::Conversion(error)
    }
}

pub fn plan_value_stack_roots(
    snapshot: &ValueStackSnapshot,
    heap: HeapId,
    first_root: RootId,
) -> Result<ValueStackRootPlan, ValueStackRootError> {
    let mut roots = Vec::new();
    for slot in &snapshot.slots {
        // Detect cells via the live JSVALUE64 classifier, not the transitional
        // baseline-JIT layout apparatus: a live int32 whose low byte happened
        // to be 0x20 would be mis-flagged as a cell by `classify_encoded`.
        if let Some(cell) = JsValue::from_encoded(slot.encoded).as_cell() {
            let root_number = first_root
                .0
                .checked_add(slot.index as u64)
                .ok_or(ValueStackRootError::RootIdOverflow)?;
            roots.push(ValueStackRootDescriptor {
                root: RootRecord {
                    id: RootId(root_number),
                    kind: RootKind::VMRegister,
                    heap,
                },
                slot: *slot,
                cell,
            });
        }
    }
    RootSet::from_records(roots.iter().map(|descriptor| descriptor.root).collect())?;
    Ok(ValueStackRootPlan { heap, roots })
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn static_value_layout_matches_encoded_width() {
        assert_eq!(static_value_representation_layout().storage_bits, 64);
        assert_eq!(static_value_representation_layout().tag_mask, TAG_MASK);
        assert_eq!(static_value_representation_layout().validate(), Ok(()));
    }

    const DUPLICATE_IMMEDIATE_TAGS: &[ImmediateTagDescriptor] = &[
        ImmediateTagDescriptor::new(ImmediateKind::Undefined, TAG_UNDEFINED, "undefined"),
        ImmediateTagDescriptor::new(ImmediateKind::Null, TAG_UNDEFINED, "null"),
        ImmediateTagDescriptor::new(ImmediateKind::Double, TAG_DOUBLE, "double"),
    ];

    #[test]
    fn value_layout_builder_constructs_static_shape() {
        let layout =
            ValueRepresentationLayoutBuilder::new("custom", STATIC_IMMEDIATE_TAG_DESCRIPTORS)
                .build();

        assert_eq!(layout.map(|layout| layout.storage_bits), Ok(64));
    }

    #[test]
    fn value_layout_validator_rejects_duplicate_tags() {
        let layout = ValueRepresentationLayoutBuilder::new("bad", DUPLICATE_IMMEDIATE_TAGS).build();

        assert_eq!(
            layout,
            Err(ValueRepresentationValidationError::DuplicateImmediateTag(
                TAG_UNDEFINED
            ))
        );
    }

    #[test]
    fn layout_classifies_descriptor_tags_without_hardcoded_jsvalue_path() {
        let layout = static_value_representation_layout();

        assert_eq!(
            layout.classify_encoded(EncodedJsValue(TAG_NULL)),
            ValueClassification::Immediate(ImmediateKind::Null)
        );
        assert_eq!(
            layout.kind_for_encoded(EncodedJsValue((0x1234 << 8) | TAG_CELL)),
            ValueKind::Cell
        );
        assert_eq!(
            layout.classify_encoded(EncodedJsValue(0xff)),
            ValueClassification::Unknown(EncodedJsValue(0xff))
        );
    }

    #[test]
    fn layout_encodes_payloads_using_descriptor_geometry() {
        let layout = static_value_representation_layout();

        assert_eq!(
            layout.encode_i32(-7),
            Ok(EncodedJsValue(((-7_i32 as u32 as u64) << 8) | TAG_I32))
        );
        assert_eq!(
            layout.encode_cell_payload(0x1234),
            Ok(EncodedJsValue((0x1234 << 8) | TAG_CELL))
        );
        assert_eq!(
            layout.encode_immediate_name("missing"),
            Err(ValueEncodingError::UnknownImmediateName)
        );
    }

    #[test]
    fn value_semantics_classify_without_runtime_hooks() {
        assert!(JsValue::undefined().strict_equals(JsValue::undefined()));
        assert!(!JsValue::undefined().strict_equals(JsValue::null()));
        assert!(JsValue::from_i32(3).same_value(JsValue::from_double(3.0)));
        assert!(!JsValue::from_i32(0).pure_to_boolean());
        assert!(JsValue::from_bool(true).pure_to_boolean());
    }

    #[test]
    fn value_stack_snapshot_round_trips_transport_values() {
        let values = vec![
            JsValue::undefined(),
            JsValue::null(),
            JsValue::from_bool(true),
            JsValue::from_i32(42),
        ];

        let snapshot = encode_value_stack(&values).unwrap();
        let decoded = decode_value_stack(&snapshot).unwrap();

        assert_eq!(snapshot.len(), 4);
        assert_eq!(decoded, values);
        assert_eq!(snapshot.slots[3].kind, ValueKind::Int32);
    }

    #[test]
    fn value_stack_decode_rejects_non_contiguous_slots() {
        let snapshot = ValueStackSnapshot {
            layout_name: static_value_representation_layout().name,
            slots: vec![ValueStackSlot {
                index: 1,
                encoded: JsValue::undefined().encoded(),
                kind: ValueKind::Undefined,
            }],
        };

        assert_eq!(
            decode_value_stack(&snapshot),
            Err(ValueStackConversionError::NonContiguousSlot {
                expected: 0,
                actual: 1
            })
        );
    }

    #[test]
    fn value_stack_root_plan_describes_cell_slots_as_vm_roots() {
        let cell = JsValue::from_encoded(
            static_value_representation_layout()
                .encode_cell_payload(0x1234)
                .unwrap(),
        );
        let values = vec![JsValue::undefined(), cell, JsValue::from_i32(4)];
        let snapshot = encode_value_stack(&values).unwrap();

        let plan = plan_value_stack_roots(&snapshot, HeapId(7), RootId(20)).unwrap();

        assert_eq!(plan.roots.len(), 1);
        assert_eq!(
            plan.roots[0].root,
            RootRecord {
                id: RootId(21),
                kind: RootKind::VMRegister,
                heap: HeapId(7)
            }
        );
        assert_eq!(plan.roots[0].cell.pointer_payload_bits(), 0x1234);
    }

    #[test]
    fn jsvalue64_double_round_trips_losslessly() {
        // Headline correctness fix: the old low-byte mask destroyed mantissa
        // bits; JSVALUE64 box/unbox is bit-exact.
        let cases = [
            -0.0_f64,
            f64::INFINITY,
            f64::NEG_INFINITY,
            f64::MAX,
            f64::MIN,
            f64::MIN_POSITIVE,
            f64::from_bits(1),                     // smallest subnormal
            f64::from_bits(0x000F_FFFF_FFFF_FFFF), // largest subnormal
            std::f64::consts::PI,
            2.5_f64,
            f64::from_bits(0x3ff0_0000_0000_00FF), // nonzero low mantissa byte
        ];
        for &d in &cases {
            let v = JsValue::from_double(d);
            assert!(v.is_double(), "expected double for {:#018x}", d.to_bits());
            match v.as_number() {
                Some(NumberValue::DoubleBits(bits)) => assert_eq!(
                    bits.to_f64().to_bits(),
                    d.to_bits(),
                    "lossy round-trip for {:#018x}",
                    d.to_bits()
                ),
                other => panic!("expected DoubleBits, got {other:?}"),
            }
        }
    }

    #[test]
    fn jsvalue64_double_round_trips_random_bit_patterns() {
        // Property loop: any non-NaN double either canonicalizes to an exactly
        // representable int32 or round-trips bit-exactly as a boxed double.
        let mut state = 0x2545_f491_4f6c_dd1d_u64;
        for _ in 0..4096 {
            state ^= state << 13;
            state ^= state >> 7;
            state ^= state << 17;
            let d = f64::from_bits(state);
            if d.is_nan() {
                continue;
            }
            match JsValue::from_double(d).as_number() {
                Some(NumberValue::Int32(i)) => assert_eq!(i as f64, d),
                Some(NumberValue::DoubleBits(bits)) => {
                    assert!(JsValue::from_double(d).is_double());
                    assert_eq!(bits.to_f64().to_bits(), d.to_bits());
                }
                None => panic!("number expected for {:#018x}", d.to_bits()),
            }
        }
    }

    #[test]
    fn jsvalue64_nonzero_low_byte_double_no_longer_collapses() {
        // Regression: a double whose low 8 bits are nonzero must survive (the
        // exact pattern the old `& !TAG_MASK` mask destroyed).
        let d = f64::from_bits(0x3ff0_0000_0000_00FF);
        let v = JsValue::from_double(d);
        assert_ne!(v.as_number().unwrap(), NumberValue::Int32(0));
        match v.as_number() {
            Some(NumberValue::DoubleBits(bits)) => {
                assert_eq!(bits.to_f64().to_bits(), d.to_bits())
            }
            other => panic!("expected double, got {other:?}"),
        }
    }

    #[test]
    fn jsvalue64_purifies_nan_before_boxing() {
        for d in [f64::NAN, f64::from_bits(0xffff_0000_0000_0000)] {
            let v = JsValue::from_double(d);
            assert!(v.is_double());
            let unboxed = match v.as_number() {
                Some(NumberValue::DoubleBits(bits)) => bits.to_f64(),
                other => panic!("expected double, got {other:?}"),
            };
            assert!(unboxed.is_nan());
            // Unboxed NaN is the canonical pure NaN; encoded == PNaN + offset.
            assert_eq!(unboxed.to_bits(), PNAN_BITS);
            assert_eq!(v.encoded().0, PNAN_BITS + DOUBLE_ENCODE_OFFSET);
        }
    }

    #[test]
    fn jsvalue64_int32_canonicalization_matches_jsc() {
        assert_eq!(JsValue::from_double(3.0).kind(), ValueKind::Int32);
        assert_eq!(
            JsValue::from_double(3.0).as_number(),
            Some(NumberValue::Int32(3))
        );
        // -0.0 is NOT a strict int32; it stays a double with sign preserved.
        let neg_zero = JsValue::from_double(-0.0);
        assert!(neg_zero.is_double());
        match neg_zero.as_number() {
            Some(NumberValue::DoubleBits(bits)) => {
                assert!(bits.to_f64().is_sign_negative());
                assert_eq!(bits.to_f64(), 0.0);
            }
            other => panic!("expected double, got {other:?}"),
        }
        // i32::MAX + 1 is out of range -> double; non-integer -> double.
        assert!(JsValue::from_double(2_147_483_648.0).is_double());
        assert!(JsValue::from_double(2.5).is_double());
        // from_i32 encodes NumberTag | (i as u32).
        let v = JsValue::from_i32(-7);
        assert_eq!(v.encoded().0, NUMBER_TAG | 0xffff_fff9);
        assert_eq!(v.as_number(), Some(NumberValue::Int32(-7)));
    }

    #[test]
    fn jsvalue64_immediate_constants_match_jsc() {
        assert_eq!(JsValue::undefined().encoded().0, 0xa);
        assert_eq!(JsValue::null().encoded().0, 0x2);
        assert_eq!(JsValue::from_bool(true).encoded().0, 0x7);
        assert_eq!(JsValue::from_bool(false).encoded().0, 0x6);
        assert_eq!(JsValue::default().encoded().0, 0x0); // ValueEmpty

        assert!(JsValue::undefined().is_undefined_or_null());
        assert!(JsValue::null().is_undefined_or_null());
        assert!(!JsValue::from_bool(false).is_undefined_or_null());
        assert!(JsValue::from_bool(true).is_boolean());
        assert!(JsValue::from_bool(false).is_boolean());
        assert_eq!(JsValue::from_bool(true).as_bool(), Some(true));
        assert_eq!(JsValue::from_bool(false).as_bool(), Some(false));

        for v in [
            JsValue::undefined(),
            JsValue::null(),
            JsValue::from_bool(true),
            JsValue::from_bool(false),
            JsValue::default(),
        ] {
            assert!(!v.is_number());
            assert!(!v.is_cell());
        }
        // Empty sentinel still recognized after the encoding change.
        assert!(JsValue::default().is_empty_or_deleted_sentinel());
    }

    #[test]
    fn jsvalue64_cell_classification_is_transitional_low_byte() {
        // Cells stay (ptr << 8) | TAG_CELL during coexistence; classified by the
        // leftover 0x20 low byte after the NumberTag/immediate tests.
        let cell = JsValue::from_encoded(EncodedJsValue((0x1234_u64 << 8) | TAG_CELL));
        assert_eq!(cell.kind(), ValueKind::Cell);
        assert!(cell.is_cell());
        assert!(!cell.is_number());
        assert!(!cell.is_double());
        assert_eq!(cell.as_cell().unwrap().pointer_payload_bits(), 0x1234);

        // A payload just below 2^41 still classifies as Cell (documents the
        // transitional invariant ptr < 2^41).
        let high = (1_u64 << 41) - 1;
        let high_cell = JsValue::from_encoded(EncodedJsValue((high << 8) | TAG_CELL));
        assert_eq!(high_cell.kind(), ValueKind::Cell);

        assert!(cell.strict_equals(cell));
    }

    #[test]
    fn jsvalue64_numbertag_invariants_hold() {
        for i in [-2_000_000_000_i32, -1, 0, 1, 32, 0x10, 2_000_000_000] {
            let v = JsValue::from_i32(i);
            assert!(v.is_int32());
            assert!(v.is_number());
            assert!(!v.is_double());
            assert!(!v.is_cell());
        }
        // Offset-encoded doubles are never int32 (top 16 bits <= 0xFFFC).
        for d in [2.5_f64, -1e300, f64::INFINITY, f64::MAX, -0.0] {
            let v = JsValue::from_double(d);
            assert!(v.is_number());
            assert!(v.is_double());
            assert!(!v.is_int32());
        }
    }

    #[cfg(feature = "s4_raw_cell")]
    #[test]
    fn s4_raw_cell_round_trips_pointer_and_rejects_immediates() {
        use core::ptr::NonNull;

        // FAITHFUL raw JSVALUE64 (JSCJSValue.h:905-907, 998-1001, 1039-1043):
        // a real 8-aligned heap pointer is stored unshifted, classifies as a
        // cell by `!(v & NotCellMask)`, and `asCell` returns the same bits. A
        // fabricated, never-dereferenced 8-aligned address with the top 16 bits
        // and OtherTag bit 1 clear stands in for a heap-cell pointer; from_cell
        // only reads the bits, so no live or pinned cell is required.
        let addr: u64 = 0x1_0000_0000;
        // SAFETY: the address is only encoded and round-tripped as bits; it is
        // never dereferenced, so the live/pinned-cell precondition is vacuous.
        let cell = unsafe { GcRef::from_non_null(NonNull::new(addr as *mut u8).unwrap()) };
        let v = JsValue::from_cell(cell);
        assert!(v.is_cell());
        assert_eq!(v.encoded().0, addr); // raw pointer stored unshifted
        assert_eq!(v.as_cell().unwrap().pointer_payload_bits(), addr as usize);

        // JS-visible immediates carry OtherTag (bit 1), so `!(v & NotCellMask)`
        // rejects them: null 0x2, false 0x6, undefined 0xa (JSCJSValue.h:472-475).
        for bits in [0x2_u64, 0x6, 0xa] {
            assert!(!JsValue::from_encoded(EncodedJsValue(bits)).is_cell());
        }

        // DIVERGENCE from the unit's literal "0x0 stays non-cell": in FAITHFUL
        // JSVALUE64, ValueEmpty (0x0) has a 00 (pointer) tag with a zero-page
        // payload, so `isCell()` is TRUE for it (JSCJSValue.h:893, 998-1001).
        // JSC separates Empty via `isEmpty()` (== ValueEmpty), not via isCell;
        // asserting is_cell == false here would be inventing non-JSC behavior,
        // so the faithful assertion is is_cell == true, filtered by the Empty
        // sentinel instead.
        let empty = JsValue::from_encoded(EncodedJsValue(VALUE_EMPTY));
        assert!(empty.is_cell());
        assert!(empty.is_empty_or_deleted_sentinel());
    }
}
