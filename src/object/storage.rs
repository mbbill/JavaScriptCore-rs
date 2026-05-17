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

/// Handle to auxiliary butterfly storage logically owned by one object.
#[derive(Clone, Copy, Debug, Eq, PartialEq)]
#[repr(transparent)]
pub struct ButterflyHandle(pub usize);

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
