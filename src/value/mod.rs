//! JavaScript value transport type.
//!
//! `JsValue` owns bits only. If those bits name a cell, the `Heap` owns that
//! cell and rooting/barrier APIs determine liveness.
//! Runtime-facing APIs may re-export this as `RuntimeValue`; the representation
//! authority remains here, while heap-cell identity remains in `gc::CellId`.

#![deny(unsafe_op_in_unsafe_fn)]

mod conversion;
mod repr;

pub use self::conversion::{
    static_conversion_descriptor_registry, static_conversion_descriptors, ConversionAlgorithmClass,
    ConversionDescriptor, ConversionDescriptorBuilder, ConversionDescriptorRegistry,
    ConversionDescriptorValidationError, ConversionPlan, ConversionRegistryAuthority,
    ConversionSchemaOwner, IntegrityLevel, NumericConversionHint, ObjectCoercionFailure,
    PreferredPrimitiveType, PrimitiveConversionOutcome, PropertyKeyConversionOutcome,
    RuntimeTypeofResult, ToObjectOutcome, ValueCoercionError, ValueCoercionPolicy,
    ValueCoercionRequest, ValueCoercionResult, ValueCoercionTarget, ValueConversionHooks,
    STATIC_CONVERSION_DESCRIPTORS, STATIC_CONVERSION_DESCRIPTOR_REGISTRY,
};
pub use self::repr::{
    decode_value_stack, encode_value_stack, plan_value_stack_roots,
    static_value_representation_layout, CellValue, EncodedDoubleBits, EncodedJsValue,
    ImmediateKind, ImmediateTagDescriptor, JsValue, NumberValue, ValueClassification,
    ValueEncodingError, ValueKind, ValueRepresentationAuthority, ValueRepresentationLayout,
    ValueRepresentationLayoutBuilder, ValueRepresentationOwner, ValueRepresentationValidationError,
    ValueStackConversionError, ValueStackRootDescriptor, ValueStackRootError, ValueStackRootPlan,
    ValueStackSlot, ValueStackSnapshot, NOT_CELL_MASK, NUMBER_TAG,
    STATIC_IMMEDIATE_TAG_DESCRIPTORS, STATIC_VALUE_REPRESENTATION_LAYOUT,
};
