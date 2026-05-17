//! JavaScript value transport type.
//!
//! `JsValue` owns bits only. If those bits name a cell, the `Heap` owns that
//! cell and rooting/barrier APIs determine liveness.

#![deny(unsafe_op_in_unsafe_fn)]

mod conversion;
mod repr;

pub use self::conversion::{
    IntegrityLevel, NumericConversionHint, ObjectCoercionFailure, PreferredPrimitiveType,
    PrimitiveConversionOutcome, PropertyKeyConversionOutcome, RuntimeTypeofResult, ToObjectOutcome,
    ValueCoercionError, ValueCoercionPolicy, ValueCoercionRequest, ValueCoercionResult,
    ValueCoercionTarget, ValueConversionHooks,
};
pub use self::repr::{
    CellValue, EncodedDoubleBits, EncodedJsValue, ImmediateKind, JsValue, NumberValue,
    ValueClassification, ValueKind,
};
