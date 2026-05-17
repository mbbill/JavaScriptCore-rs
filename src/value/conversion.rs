//! Value conversion and coercion contracts.
//!
//! These types name the VM operations that can allocate, call user code, or
//! throw. `JsValue` remains a transport type; coercion behavior belongs behind
//! these VM-supplied hooks.

use crate::gc::StructureId;
use crate::strings::{Identifier, PropertyKey};
use crate::value::{JsValue, NumberValue};

/// Preferred primitive hint for `ToPrimitive`.
#[derive(Clone, Copy, Debug, Eq, PartialEq)]
pub enum PreferredPrimitiveType {
    NoPreference,
    PreferString,
    PreferNumber,
}

/// Numeric conversion flavor.
#[derive(Clone, Copy, Debug, Eq, PartialEq)]
pub enum NumericConversionHint {
    Number,
    BigInt,
    Numeric,
    IntegerOrInfinity,
    Length,
    Index,
}

/// Target operation for a coercion request.
#[derive(Clone, Copy, Debug, Eq, PartialEq)]
pub enum ValueCoercionTarget {
    Primitive(PreferredPrimitiveType),
    Boolean,
    Number(NumericConversionHint),
    String,
    Object,
    PropertyKey,
    PropertyName,
}

/// Policy describing whether a conversion may do observable work.
#[derive(Clone, Copy, Debug, Eq, PartialEq)]
pub enum ValueCoercionPolicy {
    PureClassification,
    AllowAllocation,
    AllowUserCode,
    VmInquiryNoSideEffects,
}

/// One conversion request.
#[derive(Clone, Copy, Debug, Eq, PartialEq)]
pub struct ValueCoercionRequest {
    pub value: JsValue,
    pub target: ValueCoercionTarget,
    pub policy: ValueCoercionPolicy,
}

/// Result of `ToPrimitive`.
#[derive(Clone, Copy, Debug, Eq, PartialEq)]
pub enum PrimitiveConversionOutcome {
    AlreadyPrimitive(JsValue),
    Converted(JsValue),
    NeedsOrdinaryToPrimitive,
}

/// Result of converting to a property key.
#[derive(Clone, Copy, Debug, Eq, PartialEq)]
pub enum PropertyKeyConversionOutcome {
    Key(PropertyKey),
    PrivateNameRejected,
    RequiresStringAllocation,
}

/// Result of `ToObject`.
#[derive(Clone, Copy, Debug, Eq, PartialEq)]
pub enum ToObjectOutcome {
    Object(JsValue),
    BoxedPrimitive {
        value: JsValue,
        structure: StructureId,
    },
}

/// Typeof classification.
#[derive(Clone, Copy, Debug, Eq, PartialEq)]
pub enum RuntimeTypeofResult {
    Undefined,
    Object,
    Boolean,
    Number,
    BigInt,
    String,
    Symbol,
    Function,
}

/// Object conversion failure class.
#[derive(Clone, Copy, Debug, Eq, PartialEq)]
pub enum ObjectCoercionFailure {
    NullOrUndefined,
    NonObject,
}

/// Integrity operation requested by `seal`, `freeze`, or object tests.
#[derive(Clone, Copy, Debug, Eq, PartialEq)]
pub enum IntegrityLevel {
    Sealed,
    Frozen,
}

/// Coercion failure category. Exception allocation is a VM concern.
#[derive(Clone, Copy, Debug, Eq, PartialEq)]
pub enum ValueCoercionError {
    TypeError,
    RangeError,
    SideEffectsDisallowed,
    AllocationRequired,
    SymbolToString,
    BigIntToNumber,
    ObjectCoercion(ObjectCoercionFailure),
}

/// Coercion result envelope.
#[derive(Clone, Copy, Debug, Eq, PartialEq)]
pub enum ValueCoercionResult {
    Primitive(PrimitiveConversionOutcome),
    Boolean(bool),
    Number(NumberValue),
    String(Identifier),
    Object(ToObjectOutcome),
    PropertyKey(PropertyKeyConversionOutcome),
    Typeof(RuntimeTypeofResult),
}

/// VM-supplied conversion hooks.
pub trait ValueConversionHooks {
    fn coerce(
        &mut self,
        request: ValueCoercionRequest,
    ) -> Result<ValueCoercionResult, ValueCoercionError>;

    fn ordinary_to_primitive(
        &mut self,
        value: JsValue,
        hint: PreferredPrimitiveType,
        policy: ValueCoercionPolicy,
    ) -> Result<PrimitiveConversionOutcome, ValueCoercionError>;
}
