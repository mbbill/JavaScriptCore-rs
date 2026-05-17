use crate::runtime::exception::JsResult;
use crate::runtime::property::{PropertyDescriptor, RuntimePropertyKey};
use crate::runtime::state::{ObjectId, RuntimeValue, StructureId};

#[derive(Clone, Copy, Debug, Default, Eq, PartialEq, Hash)]
pub struct ArrayBufferId(pub ObjectId);

#[derive(Clone, Debug, Default, Eq, PartialEq)]
pub struct TypedArrayView {
    /// Integer-indexed exotic object state.
    ///
    /// The raw data pointer, cage, and buffer lifetime are GC/object concerns.
    /// Runtime semantics depend on the element kind, offset, length mode, and
    /// whether buffer detachment or resize can affect every access.
    pub object: Option<ObjectId>,
    pub structure: Option<StructureId>,
    pub buffer: Option<ArrayBufferId>,
    pub element_kind: TypedArrayElementKind,
    pub mode: ArrayBufferViewMode,
    pub byte_offset: u64,
    pub length: ViewLength,
}

#[derive(Clone, Copy, Debug, Default, Eq, PartialEq, Hash)]
pub enum TypedArrayElementKind {
    #[default]
    Int8,
    Uint8,
    Uint8Clamped,
    Int16,
    Uint16,
    Int32,
    Uint32,
    BigInt64,
    BigUint64,
    Float16,
    Float32,
    Float64,
}

#[derive(Clone, Copy, Debug, Default, Eq, PartialEq, Hash)]
pub enum ArrayBufferViewMode {
    #[default]
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

#[derive(Clone, Copy, Debug, Eq, PartialEq, Hash)]
pub enum ViewLength {
    Fixed {
        element_length: u64,
        byte_length: u64,
    },
    AutoLength,
}

impl Default for ViewLength {
    fn default() -> Self {
        Self::Fixed {
            element_length: 0,
            byte_length: 0,
        }
    }
}

#[derive(Clone, Debug, Default, Eq, PartialEq)]
pub struct DataViewObject {
    pub object: Option<ObjectId>,
    pub buffer: Option<ArrayBufferId>,
    pub mode: ArrayBufferViewMode,
    pub byte_offset: u64,
    pub byte_length: ViewByteLength,
}

#[derive(Clone, Copy, Debug, Eq, PartialEq, Hash)]
pub enum ViewByteLength {
    Fixed(u64),
    AutoLength,
}

impl Default for ViewByteLength {
    fn default() -> Self {
        Self::Fixed(0)
    }
}

#[derive(Clone, Copy, Debug, Default, Eq, PartialEq, Hash)]
pub enum BufferState {
    #[default]
    Attached,
    Detached,
    Resizable,
    GrowableShared,
}

#[derive(Clone, Debug, Default, Eq, PartialEq)]
pub struct IntegerIndexedAccess {
    pub object: ObjectId,
    pub index: u64,
    pub element_kind: TypedArrayElementKind,
    pub buffer_state: BufferState,
    pub out_of_bounds: bool,
}

#[derive(Clone, Copy, Debug, Default, Eq, PartialEq, Hash)]
pub enum ByteOrder {
    #[default]
    BigEndian,
    LittleEndian,
}

#[derive(Clone, Copy, Debug, Default, Eq, PartialEq, Hash)]
pub enum DataViewElementKind {
    #[default]
    Int8,
    Uint8,
    Int16,
    Uint16,
    Int32,
    Uint32,
    BigInt64,
    BigUint64,
    Float16,
    Float32,
    Float64,
}

#[derive(Clone, Debug, Default, Eq, PartialEq)]
pub struct DataViewAccess {
    pub view: ObjectId,
    pub byte_offset: u64,
    pub element_kind: DataViewElementKind,
    pub byte_order: ByteOrder,
    pub bounds_check_required: bool,
}

/// Integer-indexed object and DataView operation boundary.
pub trait TypedArrayOperations {
    fn integer_indexed_get(&self, access: IntegerIndexedAccess) -> JsResult<Option<RuntimeValue>>;
    fn integer_indexed_set(
        &mut self,
        access: IntegerIndexedAccess,
        value: RuntimeValue,
    ) -> JsResult<bool>;
    fn integer_indexed_define_own_property(
        &mut self,
        object: ObjectId,
        key: RuntimePropertyKey,
        descriptor: PropertyDescriptor,
    ) -> JsResult<bool>;
    fn data_view_get(&self, access: DataViewAccess) -> JsResult<RuntimeValue>;
    fn data_view_set(&mut self, access: DataViewAccess, value: RuntimeValue) -> JsResult<bool>;
    fn validate_attached_buffer(&self, buffer: ArrayBufferId) -> JsResult<BufferState>;
}
