use crate::runtime::exception::JsResult;
use crate::runtime::property::{PropertyDescriptor, RuntimePropertyAccessKey};
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
        key: RuntimePropertyAccessKey,
        descriptor: PropertyDescriptor,
    ) -> JsResult<bool>;
    fn data_view_get(&self, access: DataViewAccess) -> JsResult<RuntimeValue>;
    fn data_view_set(&mut self, access: DataViewAccess, value: RuntimeValue) -> JsResult<bool>;
    fn validate_attached_buffer(&self, buffer: ArrayBufferId) -> JsResult<BufferState>;
}

#[derive(Clone, Copy, Debug, Eq, PartialEq, Hash)]
pub enum IntegerIndexedAccessOutcome {
    InBounds,
    DetachedBuffer,
    OutOfBounds,
    BigIntContentMismatch,
    NumberContentMismatch,
}

pub fn typed_array_element_size(kind: TypedArrayElementKind) -> u8 {
    match kind {
        TypedArrayElementKind::Int8
        | TypedArrayElementKind::Uint8
        | TypedArrayElementKind::Uint8Clamped => 1,
        TypedArrayElementKind::Int16
        | TypedArrayElementKind::Uint16
        | TypedArrayElementKind::Float16 => 2,
        TypedArrayElementKind::Int32
        | TypedArrayElementKind::Uint32
        | TypedArrayElementKind::Float32 => 4,
        TypedArrayElementKind::BigInt64
        | TypedArrayElementKind::BigUint64
        | TypedArrayElementKind::Float64 => 8,
    }
}

pub fn typed_array_element_is_bigint(kind: TypedArrayElementKind) -> bool {
    matches!(
        kind,
        TypedArrayElementKind::BigInt64 | TypedArrayElementKind::BigUint64
    )
}

/// ECMA-262 ToInt32 applied to a finite-or-not double, mirroring C++
/// `JSC::toInt32(double)` used by `IntegralTypedArrayAdaptor::toNativeFromDouble`
/// (TypedArrayAdaptors.h:69-83). NaN/Inf -> 0, otherwise truncate toward zero
/// then reduce modulo 2^32 into the signed range.
fn double_to_int32(value: f64) -> i32 {
    if !value.is_finite() || value == 0.0 {
        return 0;
    }
    const TWO_32: f64 = 4_294_967_296.0;
    const TWO_31: f64 = 2_147_483_648.0;
    let integer = value.signum() * value.abs().floor();
    let mut modulo = integer % TWO_32;
    if modulo < 0.0 {
        modulo += TWO_32;
    }
    if modulo >= TWO_31 {
        (modulo - TWO_32) as i32
    } else {
        modulo as i32
    }
}

/// Mirror of `Adaptor::toNativeFromDouble` then byte serialization, producing
/// the native little-endian element bytes stored in the backing buffer.
///
/// C++ JSC TypedArrayAdaptors.h: integral kinds run ToInt32 then `static_cast`
/// to the element width (modulo 2^width); Uint8Clamped clamps NaN/neg to 0,
/// >255 to 255, else `lrint` (round-half-to-even); float kinds `static_cast`
/// narrow the double (Float64 identity, Float32 rounds). The audited Octane
/// consumers store via the double path; BigInt content is unreachable here.
///
/// DIVERGENCE: bytes are serialized little-endian unconditionally. C++ stores
/// in target-native order via the Gigacage-backed pointer; little-endian is the
/// only target order JetStream 3 runs on and matches the existing DataView byte
/// access (interpreter/mod.rs read/write_data_view_byte).
pub fn typed_array_store_native_bytes(kind: TypedArrayElementKind, value: f64) -> Vec<u8> {
    match kind {
        TypedArrayElementKind::Int8 => (double_to_int32(value) as i8).to_le_bytes().to_vec(),
        TypedArrayElementKind::Uint8 => (double_to_int32(value) as u8).to_le_bytes().to_vec(),
        TypedArrayElementKind::Uint8Clamped => {
            let clamped = if value.is_nan() || value < 0.0 {
                0u8
            } else if value > 255.0 {
                255u8
            } else {
                // lrint: round to nearest, ties to even (default FP rounding).
                round_half_to_even(value) as u8
            };
            clamped.to_le_bytes().to_vec()
        }
        TypedArrayElementKind::Int16 => (double_to_int32(value) as i16).to_le_bytes().to_vec(),
        TypedArrayElementKind::Uint16 => (double_to_int32(value) as u16).to_le_bytes().to_vec(),
        TypedArrayElementKind::Int32 => double_to_int32(value).to_le_bytes().to_vec(),
        TypedArrayElementKind::Uint32 => (double_to_int32(value) as u32).to_le_bytes().to_vec(),
        TypedArrayElementKind::Float32 => (value as f32).to_le_bytes().to_vec(),
        TypedArrayElementKind::Float64 => value.to_le_bytes().to_vec(),
        // Float16: narrow then re-expand on read. f16 is not in the audited
        // Octane consumer set; store as Float32-rounded bytes would be wrong, so
        // it is left out of the wired constructor set (see interpreter wiring).
        TypedArrayElementKind::Float16 => (value as f32).to_le_bytes().to_vec(),
        // BigInt content is not Number-coercible; unreachable for the wired set.
        TypedArrayElementKind::BigInt64 | TypedArrayElementKind::BigUint64 => {
            (double_to_int32(value) as i64).to_le_bytes().to_vec()
        }
    }
}

/// Number produced by reading a native element back, mirroring C++
/// `Adaptor::toJSValue` (TypedArrayAdaptors.h): integral kinds yield the signed/
/// unsigned integer as a JS Number; Uint32 may exceed i32 so it is widened to a
/// double; float kinds yield the float widened to a double (purifyNaN is the
/// canonical-NaN step, approximated here by Rust's NaN handling). Returns the
/// raw f64 so the interpreter can canonicalize via `runtime_number_from_f64`.
///
/// `bytes` must be `typed_array_element_size(kind)` bytes, little-endian.
pub fn typed_array_load_value_f64(kind: TypedArrayElementKind, bytes: &[u8]) -> f64 {
    match kind {
        TypedArrayElementKind::Int8 => f64::from(i8::from_le_bytes([bytes[0]])),
        TypedArrayElementKind::Uint8 | TypedArrayElementKind::Uint8Clamped => {
            f64::from(u8::from_le_bytes([bytes[0]]))
        }
        TypedArrayElementKind::Int16 => f64::from(i16::from_le_bytes([bytes[0], bytes[1]])),
        TypedArrayElementKind::Uint16 | TypedArrayElementKind::Float16 => {
            // Float16 is unreachable for the wired set; treat as raw u16.
            f64::from(u16::from_le_bytes([bytes[0], bytes[1]]))
        }
        TypedArrayElementKind::Int32 => {
            f64::from(i32::from_le_bytes([bytes[0], bytes[1], bytes[2], bytes[3]]))
        }
        TypedArrayElementKind::Uint32 => {
            f64::from(u32::from_le_bytes([bytes[0], bytes[1], bytes[2], bytes[3]]))
        }
        TypedArrayElementKind::Float32 => {
            f64::from(f32::from_le_bytes([bytes[0], bytes[1], bytes[2], bytes[3]]))
        }
        TypedArrayElementKind::Float64 => f64::from_le_bytes([
            bytes[0], bytes[1], bytes[2], bytes[3], bytes[4], bytes[5], bytes[6], bytes[7],
        ]),
        // BigInt content is not Number-readable; unreachable for the wired set.
        TypedArrayElementKind::BigInt64 | TypedArrayElementKind::BigUint64 => 0.0,
    }
}

/// Round-half-to-even (banker's rounding), matching C++ `lrint` under the
/// default FP rounding mode used by Uint8ClampedAdaptor::toNativeFromDouble.
fn round_half_to_even(value: f64) -> f64 {
    let floor = value.floor();
    let diff = value - floor;
    if diff < 0.5 {
        floor
    } else if diff > 0.5 {
        floor + 1.0
    } else if (floor as i64) % 2 == 0 {
        floor
    } else {
        floor + 1.0
    }
}

pub fn plan_integer_indexed_access(access: &IntegerIndexedAccess) -> IntegerIndexedAccessOutcome {
    if access.buffer_state == BufferState::Detached {
        return IntegerIndexedAccessOutcome::DetachedBuffer;
    }
    if access.out_of_bounds {
        return IntegerIndexedAccessOutcome::OutOfBounds;
    }
    IntegerIndexedAccessOutcome::InBounds
}

pub fn fixed_view_element_length(view: &TypedArrayView) -> Option<u64> {
    match view.length {
        ViewLength::Fixed { element_length, .. } => Some(element_length),
        ViewLength::AutoLength => None,
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::gc::CellId;

    #[test]
    fn integer_indexed_access_rejects_detached_buffer() {
        let access = IntegerIndexedAccess {
            object: ObjectId(CellId(1)),
            buffer_state: BufferState::Detached,
            ..IntegerIndexedAccess::default()
        };

        assert_eq!(
            plan_integer_indexed_access(&access),
            IntegerIndexedAccessOutcome::DetachedBuffer
        );
    }

    #[test]
    fn typed_array_element_sizes_match_content_kind() {
        assert_eq!(typed_array_element_size(TypedArrayElementKind::Uint8), 1);
        assert_eq!(typed_array_element_size(TypedArrayElementKind::BigInt64), 8);
        assert!(typed_array_element_is_bigint(
            TypedArrayElementKind::BigUint64
        ));
    }

    fn store_then_load(kind: TypedArrayElementKind, value: f64) -> f64 {
        let bytes = typed_array_store_native_bytes(kind, value);
        assert_eq!(bytes.len(), usize::from(typed_array_element_size(kind)));
        typed_array_load_value_f64(kind, &bytes)
    }

    #[test]
    fn integral_store_truncates_and_signs_per_cpp_adaptor() {
        // C++ Int8Adaptor: -1 stored reads back -1 (signed).
        assert_eq!(store_then_load(TypedArrayElementKind::Int8, -1.0), -1.0);
        // C++ Uint8Adaptor (checkForOperaMathBug): -1 -> 0xFF (255).
        assert_eq!(store_then_load(TypedArrayElementKind::Uint8, -1.0), 255.0);
        // C++ Uint8Adaptor: 257.9 -> toInt32(257.9)=257 -> static_cast<uint8>=1.
        assert_eq!(store_then_load(TypedArrayElementKind::Uint8, 257.9), 1.0);
        // C++ Int16Adaptor: 0x12345 truncates to 0x2345 = 9029.
        assert_eq!(
            store_then_load(TypedArrayElementKind::Int16, 0x1_2345 as f64),
            0x2345 as f64
        );
        // C++ Int32Adaptor: 2^32 + 5 -> ToInt32 -> 5.
        assert_eq!(
            store_then_load(TypedArrayElementKind::Int32, 4_294_967_301.0),
            5.0
        );
        // C++ Uint32Adaptor: -1 -> ToInt32 -> 0xFFFFFFFF read back as 4294967295.
        assert_eq!(
            store_then_load(TypedArrayElementKind::Uint32, -1.0),
            4_294_967_295.0
        );
        // NaN/Inf -> 0 for integral kinds.
        assert_eq!(store_then_load(TypedArrayElementKind::Int32, f64::NAN), 0.0);
        assert_eq!(
            store_then_load(TypedArrayElementKind::Int32, f64::INFINITY),
            0.0
        );
    }

    #[test]
    fn uint8_clamped_store_clamps_and_rounds_half_to_even() {
        // C++ Uint8ClampedAdaptor: NaN/neg -> 0, >255 -> 255.
        assert_eq!(
            store_then_load(TypedArrayElementKind::Uint8Clamped, f64::NAN),
            0.0
        );
        assert_eq!(
            store_then_load(TypedArrayElementKind::Uint8Clamped, -5.0),
            0.0
        );
        assert_eq!(
            store_then_load(TypedArrayElementKind::Uint8Clamped, 257.9),
            255.0
        );
        // lrint round-half-to-even: 2.5 -> 2, 3.5 -> 4.
        assert_eq!(
            store_then_load(TypedArrayElementKind::Uint8Clamped, 2.5),
            2.0
        );
        assert_eq!(
            store_then_load(TypedArrayElementKind::Uint8Clamped, 3.5),
            4.0
        );
    }

    #[test]
    fn float_store_narrows_and_preserves_value() {
        // C++ Float64Adaptor: identity for a representable value.
        assert_eq!(store_then_load(TypedArrayElementKind::Float64, 1.5), 1.5);
        // C++ Float32Adaptor: double->float rounding; 0.5 is exact in f32.
        assert_eq!(store_then_load(TypedArrayElementKind::Float32, 0.5), 0.5);
        // Float32 narrows a non-representable double to the nearest f32.
        let narrowed = store_then_load(TypedArrayElementKind::Float32, 0.1);
        assert_eq!(narrowed, f64::from(0.1f32));
        // NaN survives as NaN for float kinds.
        assert!(store_then_load(TypedArrayElementKind::Float32, f64::NAN).is_nan());
    }
}
