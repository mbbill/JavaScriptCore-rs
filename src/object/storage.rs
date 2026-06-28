//! Inline, out-of-line, and indexed storage capsules.
//!
//! Raw butterfly layout, negative property indexing, and indexing-header
//! placement are unsafe representation boundaries. This skeleton keeps storage
//! as ordinary vectors while preserving the public mutation surfaces.

use crate::gc::{BarrierKind, GcRef, JsCell, ValueBarrier};
use crate::value::JsValue;

use super::property::PropertyOffset;

/// Unsafe out-of-line storage capsule.
#[derive(Debug, Default)]
pub struct Butterfly {
    layout: ButterflyLayout,
    indexed_header: Option<IndexingHeader>,
    property_slots: Vec<ValueBarrier<JsValue>>,
    indexed_slots: IndexedStorage,
}

impl Butterfly {
    pub fn new() -> Self {
        Self::default()
    }

    pub fn indexed_header(&self) -> Option<IndexingHeader> {
        self.indexed_header
    }

    pub fn layout(&self) -> ButterflyLayout {
        self.layout
    }

    pub fn property_slots(&self) -> &[ValueBarrier<JsValue>] {
        &self.property_slots
    }

    pub fn indexed_storage(&self) -> &IndexedStorage {
        &self.indexed_slots
    }
}

// gc-r4 B1a: `ButterflyHandle` moved to `object/butterfly_handle.rs` (the home of
// the LIVE butterfly rep over `RuntimeValue`). It is a value-type-agnostic slab
// index, so it lives beside the live rep rather than these NON-LIVE contract
// types (which are over `JsValue` and retired in a later GAP-D cleanup).

/// Butterfly allocation layout. JSC's concrete representation uses property
/// storage at negative offsets and indexed payload at positive offsets; this
/// skeleton keeps only the capacities and ownership boundary.
#[derive(Clone, Copy, Debug, Default, Eq, PartialEq)]
pub struct ButterflyLayout {
    pub pre_capacity: u32,
    pub property_capacity: u32,
    pub has_indexing_header: bool,
    pub indexing_payload_bytes: usize,
}

#[derive(Clone, Copy, Debug, Eq, PartialEq)]
pub enum StorageValidationError {
    IndexedPayloadWithoutHeader,
    HeaderWithoutIndexedPayload,
    IndexingLengthMismatch,
    ArrayStorageVectorTooSmall,
    ArrayLengthOffsetInvalid,
    TypedArrayElementSizeMismatch,
    TypedArrayContentMismatch,
    TypedArrayModeBufferMismatch,
    TypedArrayAutoLengthMismatch,
}

#[derive(Clone, Copy, Debug, Default, Eq, PartialEq)]
pub struct ButterflyLayoutBuilder {
    layout: ButterflyLayout,
}

impl ButterflyLayoutBuilder {
    pub const fn new() -> Self {
        Self {
            layout: ButterflyLayout {
                pre_capacity: 0,
                property_capacity: 0,
                has_indexing_header: false,
                indexing_payload_bytes: 0,
            },
        }
    }

    pub const fn property_capacity(mut self, property_capacity: u32) -> Self {
        self.layout.property_capacity = property_capacity;
        self
    }

    pub const fn indexed_payload(mut self, bytes: usize) -> Self {
        self.layout.has_indexing_header = true;
        self.layout.indexing_payload_bytes = bytes;
        self
    }

    pub fn build(self) -> Result<ButterflyLayout, StorageValidationError> {
        validate_butterfly_layout(self.layout)?;
        Ok(self.layout)
    }
}

pub fn validate_butterfly_layout(layout: ButterflyLayout) -> Result<(), StorageValidationError> {
    if layout.indexing_payload_bytes > 0 && !layout.has_indexing_header {
        return Err(StorageValidationError::IndexedPayloadWithoutHeader);
    }
    if layout.has_indexing_header && layout.indexing_payload_bytes == 0 {
        return Err(StorageValidationError::HeaderWithoutIndexedPayload);
    }
    Ok(())
}

/// Description of an intended butterfly resize/growth.
#[derive(Clone, Copy, Debug, Eq, PartialEq)]
pub struct ButterflyGrowth {
    pub old_layout: ButterflyLayout,
    pub new_layout: ButterflyLayout,
    pub reason: ButterflyGrowthReason,
}

#[derive(Clone, Copy, Debug, Eq, PartialEq)]
pub enum ButterflyGrowthReason {
    AddOutOfLineProperty,
    GrowIndexedStorage,
    ShiftIndexedStorage,
    ConvertIndexingMode,
}

/// Indexed storage metadata.
#[derive(Clone, Copy, Debug, Default, Eq, PartialEq)]
pub struct IndexingHeader {
    pub public_length: u32,
    pub vector_length: u32,
    pub mode: IndexedStorageKind,
    pub history: IndexingHistory,
}

pub fn validate_indexing_header(header: IndexingHeader) -> Result<(), StorageValidationError> {
    if header.public_length > header.vector_length {
        return Err(StorageValidationError::IndexingLengthMismatch);
    }
    if header.history.copy_on_write
        && !matches!(
            header.mode,
            IndexedStorageKind::Int32 | IndexedStorageKind::Double | IndexedStorageKind::Contiguous
        )
    {
        return Err(StorageValidationError::IndexingLengthMismatch);
    }
    Ok(())
}

/// Indexed payload family. It deliberately does not expose array storage layout.
#[derive(Clone, Copy, Debug, Default, Eq, PartialEq)]
pub enum IndexedStorageKind {
    #[default]
    None,
    Undecided,
    Int32,
    Double,
    Contiguous,
    ArrayStorage,
    SlowPutArrayStorage,
    TypedArray(TypedArrayElementType),
    DataView,
}

/// Indexing history bits that can affect cache/prototype-chain decisions.
#[derive(Clone, Copy, Debug, Default, Eq, PartialEq)]
pub struct IndexingHistory {
    pub is_array: bool,
    pub copy_on_write: bool,
    pub may_have_indexed_accessors: bool,
    pub converted_from_fast_storage: bool,
}

/// Array length invariants for ordinary arrays.
#[derive(Clone, Copy, Debug, Eq, PartialEq)]
pub struct ArrayLengthContract {
    pub public_length: u32,
    pub writable: bool,
    pub length_property_offset: PropertyOffset,
    pub storage_may_contain_holes: bool,
}

/// Sparse or dictionary indexed-property metadata.
#[derive(Clone, Copy, Debug, Default, Eq, PartialEq)]
pub struct SparseIndexMetadata {
    pub sparse_entry_count: u32,
    pub highest_observed_index: Option<u32>,
    pub has_indexed_accessors: bool,
}

/// ArrayStorage side metadata.
#[derive(Clone, Copy, Debug, Eq, PartialEq)]
pub struct ArrayStorageMetadata {
    pub length: ArrayLengthContract,
    pub vector_length: u32,
    pub sparse: SparseIndexMetadata,
}

pub fn validate_array_storage_metadata(
    metadata: ArrayStorageMetadata,
) -> Result<(), StorageValidationError> {
    if metadata.length.length_property_offset == PropertyOffset::INVALID {
        return Err(StorageValidationError::ArrayLengthOffsetInvalid);
    }
    if metadata.vector_length < metadata.length.public_length {
        return Err(StorageValidationError::ArrayStorageVectorTooSmall);
    }
    if let Some(highest) = metadata.sparse.highest_observed_index {
        if metadata.sparse.sparse_entry_count == 0 || highest < metadata.length.public_length {
            return Err(StorageValidationError::ArrayStorageVectorTooSmall);
        }
    }
    Ok(())
}

/// Typed-array element family.
#[derive(Clone, Copy, Debug, Eq, PartialEq)]
pub enum TypedArrayElementType {
    Int8,
    Int16,
    Int32,
    Uint8,
    Uint8Clamped,
    Uint16,
    Uint32,
    Float16,
    Float32,
    Float64,
    BigInt64,
    BigUint64,
}

/// Typed-array content family.
#[derive(Clone, Copy, Debug, Eq, PartialEq)]
pub enum TypedArrayContentType {
    Number,
    BigInt,
}

/// JSArrayBufferView storage mode.
#[derive(Clone, Copy, Debug, Eq, PartialEq)]
pub enum TypedArrayMode {
    FastTypedArray,
    OversizeTypedArray,
    WastefulTypedArray,
    GrowableSharedWastefulTypedArray,
    GrowableSharedAutoLengthWastefulTypedArray,
    ResizableNonSharedWastefulTypedArray,
    ResizableNonSharedAutoLengthWastefulTypedArray,
    DataView,
    GrowableSharedDataView,
    GrowableSharedAutoLengthDataView,
    ResizableNonSharedDataView,
    ResizableNonSharedAutoLengthDataView,
}

impl TypedArrayMode {
    pub const fn has_array_buffer(self) -> bool {
        !matches!(self, Self::FastTypedArray | Self::OversizeTypedArray)
    }

    pub const fn is_auto_length(self) -> bool {
        matches!(
            self,
            Self::GrowableSharedAutoLengthWastefulTypedArray
                | Self::ResizableNonSharedAutoLengthWastefulTypedArray
                | Self::GrowableSharedAutoLengthDataView
                | Self::ResizableNonSharedAutoLengthDataView
        )
    }

    pub const fn is_resizable_or_growable_shared(self) -> bool {
        matches!(
            self,
            Self::GrowableSharedWastefulTypedArray
                | Self::GrowableSharedAutoLengthWastefulTypedArray
                | Self::ResizableNonSharedWastefulTypedArray
                | Self::ResizableNonSharedAutoLengthWastefulTypedArray
                | Self::GrowableSharedDataView
                | Self::GrowableSharedAutoLengthDataView
                | Self::ResizableNonSharedDataView
                | Self::ResizableNonSharedAutoLengthDataView
        )
    }
}

/// Whether a typed-array view has fixed or buffer-derived length.
#[derive(Clone, Copy, Debug, Eq, PartialEq)]
pub enum TypedArrayViewLength {
    FixedElements(usize),
    AutoLength,
}

/// Edge from a view object to its backing ArrayBuffer.
#[derive(Clone, Copy, Debug, Eq, PartialEq)]
pub enum TypedArrayBufferEdge {
    InlineOwnedVector,
    ExternalVectorNoBuffer,
    ArrayBufferObject,
    SharedArrayBufferObject,
    DetachedBuffer,
}

/// Integer-indexed exotic storage contract.
#[derive(Clone, Copy, Debug, Eq, PartialEq)]
pub struct TypedArrayStorageContract {
    pub element_type: TypedArrayElementType,
    pub content_type: TypedArrayContentType,
    pub mode: TypedArrayMode,
    pub buffer_edge: TypedArrayBufferEdge,
    pub byte_offset: usize,
    pub length: TypedArrayViewLength,
    pub element_size: u8,
}

#[derive(Clone, Copy, Debug, Eq, PartialEq)]
pub struct TypedArrayStorageContractBuilder {
    contract: TypedArrayStorageContract,
}

impl TypedArrayStorageContractBuilder {
    pub const fn new(element_type: TypedArrayElementType, mode: TypedArrayMode) -> Self {
        Self {
            contract: TypedArrayStorageContract {
                element_type,
                content_type: typed_array_content_type(element_type),
                mode,
                buffer_edge: TypedArrayBufferEdge::InlineOwnedVector,
                byte_offset: 0,
                length: TypedArrayViewLength::FixedElements(0),
                element_size: typed_array_element_size(element_type),
            },
        }
    }

    pub const fn buffer_edge(mut self, buffer_edge: TypedArrayBufferEdge) -> Self {
        self.contract.buffer_edge = buffer_edge;
        self
    }

    pub const fn byte_offset(mut self, byte_offset: usize) -> Self {
        self.contract.byte_offset = byte_offset;
        self
    }

    pub const fn length(mut self, length: TypedArrayViewLength) -> Self {
        self.contract.length = length;
        self
    }

    pub fn build(self) -> Result<TypedArrayStorageContract, StorageValidationError> {
        validate_typed_array_storage_contract(self.contract)?;
        Ok(self.contract)
    }
}

pub const fn typed_array_element_size(element_type: TypedArrayElementType) -> u8 {
    match element_type {
        TypedArrayElementType::Int8
        | TypedArrayElementType::Uint8
        | TypedArrayElementType::Uint8Clamped => 1,
        TypedArrayElementType::Int16
        | TypedArrayElementType::Uint16
        | TypedArrayElementType::Float16 => 2,
        TypedArrayElementType::Int32
        | TypedArrayElementType::Uint32
        | TypedArrayElementType::Float32 => 4,
        TypedArrayElementType::Float64
        | TypedArrayElementType::BigInt64
        | TypedArrayElementType::BigUint64 => 8,
    }
}

pub const fn typed_array_content_type(
    element_type: TypedArrayElementType,
) -> TypedArrayContentType {
    match element_type {
        TypedArrayElementType::BigInt64 | TypedArrayElementType::BigUint64 => {
            TypedArrayContentType::BigInt
        }
        _ => TypedArrayContentType::Number,
    }
}

pub fn validate_typed_array_storage_contract(
    contract: TypedArrayStorageContract,
) -> Result<(), StorageValidationError> {
    if contract.element_size != typed_array_element_size(contract.element_type) {
        return Err(StorageValidationError::TypedArrayElementSizeMismatch);
    }
    if contract.content_type != typed_array_content_type(contract.element_type) {
        return Err(StorageValidationError::TypedArrayContentMismatch);
    }
    if contract.mode.has_array_buffer()
        != matches!(
            contract.buffer_edge,
            TypedArrayBufferEdge::ArrayBufferObject
                | TypedArrayBufferEdge::SharedArrayBufferObject
                | TypedArrayBufferEdge::DetachedBuffer
        )
    {
        return Err(StorageValidationError::TypedArrayModeBufferMismatch);
    }
    if contract.mode.is_auto_length() != matches!(contract.length, TypedArrayViewLength::AutoLength)
    {
        return Err(StorageValidationError::TypedArrayAutoLengthMismatch);
    }
    Ok(())
}

/// Opaque indexed storage payload.
#[derive(Debug, Default)]
pub struct IndexedStorage {
    kind: IndexedStorageKind,
    values: Vec<ValueBarrier<JsValue>>,
}

impl IndexedStorage {
    pub fn kind(&self) -> IndexedStorageKind {
        self.kind
    }

    pub fn values(&self) -> &[ValueBarrier<JsValue>] {
        &self.values
    }
}

/// Typed inline slot view.
#[derive(Debug, Default)]
pub struct InlineStorage {
    slots: Vec<ValueBarrier<JsValue>>,
}

impl InlineStorage {
    pub fn new(slot_count: usize, initial_value: JsValue) -> Self {
        Self {
            slots: vec![ValueBarrier::new_initial(initial_value); slot_count],
        }
    }

    pub fn len(&self) -> usize {
        self.slots.len()
    }

    pub fn is_empty(&self) -> bool {
        self.slots.is_empty()
    }

    pub fn initialize_slot(&mut self, offset: PropertyOffset, value: JsValue) {
        if let Some(slot) = self.slots.get_mut(offset.0 as usize) {
            slot.initialize_without_barrier(value);
        }
    }

    pub fn set_slot(
        &mut self,
        owner: GcRef<JsCell>,
        offset: PropertyOffset,
        value: JsValue,
    ) -> Option<BarrierKind> {
        if let Some(slot) = self.slots.get_mut(offset.0 as usize) {
            return Some(slot.set(owner, value));
        }
        None
    }
}

/// Typed out-of-line property view.
#[derive(Debug, Default)]
pub struct OutOfLineStorage {
    slots: Vec<ValueBarrier<JsValue>>,
    capacity: u32,
}

impl OutOfLineStorage {
    pub fn slots(&self) -> &[ValueBarrier<JsValue>] {
        &self.slots
    }

    pub fn capacity(&self) -> u32 {
        self.capacity
    }
}
