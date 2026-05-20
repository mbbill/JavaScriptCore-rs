//! Tagged JavaScript value skeleton.
//!
//! This file reserves the value representation boundary without implementing
//! JavaScript coercion, arithmetic, or object behavior.

use crate::gc::{GcRef, HeapId, RootId, RootKind, RootRecord, RootSet, RootSetSemanticError};

const TAG_UNDEFINED: u64 = 0x01;
const TAG_NULL: u64 = 0x02;
const TAG_FALSE: u64 = 0x03;
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

pub const STATIC_IMMEDIATE_TAG_DESCRIPTORS: &[ImmediateTagDescriptor] = &[
    ImmediateTagDescriptor {
        kind: ImmediateKind::Undefined,
        tag: TAG_UNDEFINED,
        canonical_name: "undefined",
    },
    ImmediateTagDescriptor {
        kind: ImmediateKind::Null,
        tag: TAG_NULL,
        canonical_name: "null",
    },
    ImmediateTagDescriptor {
        kind: ImmediateKind::Boolean,
        tag: TAG_FALSE,
        canonical_name: "false",
    },
    ImmediateTagDescriptor {
        kind: ImmediateKind::Boolean,
        tag: TAG_TRUE,
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
            kind: layout.kind_for_encoded(value.encoded()),
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
        // Value construction copies a borrowed cell address into value bits.
        // The heap still owns identity, liveness, and payload interpretation.
        let ptr_bits = cell.as_ptr() as *mut () as usize as u64;
        Self(EncodedJsValue((ptr_bits << 8) | TAG_CELL))
    }

    pub fn kind(self) -> ValueKind {
        static_value_representation_layout().kind_for_encoded(self.0)
    }

    pub fn classification(self) -> ValueClassification {
        static_value_representation_layout().classify_encoded(self.0)
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

    pub fn is_boolean(self) -> bool {
        self.kind() == ValueKind::Boolean
    }

    pub fn as_bool(self) -> Option<bool> {
        match self.0 .0 & TAG_MASK {
            TAG_FALSE => Some(false),
            TAG_TRUE => Some(true),
            _ => None,
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
        f64::from_bits(self.0 & !TAG_MASK)
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
        (self.encoded.0 >> 8) as usize
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
    RootSet(RootSetSemanticError),
    RootIdOverflow,
}

impl From<RootSetSemanticError> for ValueStackRootError {
    fn from(error: RootSetSemanticError) -> Self {
        Self::RootSet(error)
    }
}

pub fn plan_value_stack_roots(
    snapshot: &ValueStackSnapshot,
    heap: HeapId,
    first_root: RootId,
) -> Result<ValueStackRootPlan, ValueStackRootError> {
    let mut roots = Vec::new();
    for slot in &snapshot.slots {
        if let ValueClassification::Cell(cell) =
            static_value_representation_layout().classify_encoded(slot.encoded)
        {
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
}
