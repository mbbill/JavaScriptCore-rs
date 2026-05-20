//! Value conversion and coercion contracts.
//!
//! These types name the VM operations that can allocate, call user code, or
//! throw. `JsValue` remains a transport type; coercion behavior belongs behind
//! these VM-supplied hooks. Conversion hooks borrow or return value bits; they
//! do not own heap cells named by those bits.

use crate::gc::StructureId;
use crate::strings::{Identifier, PropertyKey};
use crate::value::{JsValue, NumberValue, ValueKind};

/// Static owner for conversion descriptor rows.
#[derive(Clone, Copy, Debug, Default, Eq, PartialEq)]
pub enum ConversionSchemaOwner {
    /// `value::conversion` owns the Rust conversion operation catalog.
    #[default]
    ValueConversionSchema,
    /// A future generated table owns rows copied from ECMAScript operation metadata.
    GeneratedEcmaOperationTable,
}

/// Registry mutation authority for conversion descriptor data.
#[derive(Clone, Copy, Debug, Default, Eq, PartialEq)]
pub enum ConversionRegistryAuthority {
    /// Conversion descriptors are compiled static data.
    #[default]
    StaticReadOnly,
    /// A generated source refresh may replace the compiled registry.
    GeneratedSourceRefresh,
}

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

/// Static descriptor for a conversion operation.
///
/// The descriptor names policy and observable-effect boundaries only. It does
/// not perform coercion, allocate wrappers, call user code, or throw.
#[derive(Clone, Copy, Debug, Eq, PartialEq)]
pub struct ConversionDescriptor {
    pub name: &'static str,
    pub target: ValueCoercionTarget,
    pub minimum_policy: ValueCoercionPolicy,
    pub may_allocate: bool,
    pub may_call_user_code: bool,
    pub may_throw: bool,
    pub owner: ConversionSchemaOwner,
}

/// Immutable registry of conversion descriptors.
#[derive(Clone, Copy, Debug, Eq, PartialEq)]
pub struct ConversionDescriptorRegistry {
    pub name: &'static str,
    pub authority: ConversionRegistryAuthority,
    pub descriptors: &'static [ConversionDescriptor],
}

impl ConversionDescriptorRegistry {
    pub const fn descriptors(&self) -> &'static [ConversionDescriptor] {
        self.descriptors
    }

    pub fn descriptor(&self, name: &str) -> Option<&'static ConversionDescriptor> {
        self.descriptors
            .iter()
            .find(|descriptor| descriptor.name == name)
    }

    pub fn plan_request(
        &self,
        request: ValueCoercionRequest,
    ) -> Result<ConversionPlan<'static>, ValueCoercionError> {
        self.validate()
            .map_err(|_| ValueCoercionError::SideEffectsDisallowed)?;
        let descriptor = self
            .descriptors
            .iter()
            .find(|descriptor| descriptor.target == request.target)
            .ok_or(ValueCoercionError::SideEffectsDisallowed)?;
        descriptor.plan_request(request)
    }

    pub fn validate(&self) -> Result<(), ConversionDescriptorValidationError> {
        if self.name.is_empty() {
            return Err(ConversionDescriptorValidationError::EmptyRegistryName);
        }

        for (index, descriptor) in self.descriptors.iter().enumerate() {
            descriptor.validate()?;
            if self.descriptors[..index]
                .iter()
                .any(|previous| previous.name == descriptor.name)
            {
                return Err(
                    ConversionDescriptorValidationError::DuplicateDescriptorName(descriptor.name),
                );
            }
        }

        Ok(())
    }
}

#[derive(Clone, Copy, Debug, Eq, PartialEq)]
pub enum ConversionDescriptorValidationError {
    EmptyRegistryName,
    EmptyDescriptorName,
    DuplicateDescriptorName(&'static str),
    PureConversionHasObservableEffects(&'static str),
    AllocationPolicyCallsUserCode(&'static str),
    UserCodeWithoutThrowBoundary(&'static str),
    TargetPolicyMismatch(&'static str),
}

impl ConversionDescriptor {
    pub const fn new(
        name: &'static str,
        target: ValueCoercionTarget,
        minimum_policy: ValueCoercionPolicy,
    ) -> Self {
        Self {
            name,
            target,
            minimum_policy,
            may_allocate: false,
            may_call_user_code: false,
            may_throw: false,
            owner: ConversionSchemaOwner::ValueConversionSchema,
        }
    }

    pub fn validate(&self) -> Result<(), ConversionDescriptorValidationError> {
        if self.name.is_empty() {
            return Err(ConversionDescriptorValidationError::EmptyDescriptorName);
        }
        match self.minimum_policy {
            ValueCoercionPolicy::PureClassification => {
                if self.may_allocate || self.may_call_user_code || self.may_throw {
                    return Err(
                        ConversionDescriptorValidationError::PureConversionHasObservableEffects(
                            self.name,
                        ),
                    );
                }
            }
            ValueCoercionPolicy::AllowAllocation => {
                if self.may_call_user_code {
                    return Err(
                        ConversionDescriptorValidationError::AllocationPolicyCallsUserCode(
                            self.name,
                        ),
                    );
                }
            }
            ValueCoercionPolicy::AllowUserCode => {
                if self.may_call_user_code && !self.may_throw {
                    return Err(
                        ConversionDescriptorValidationError::UserCodeWithoutThrowBoundary(
                            self.name,
                        ),
                    );
                }
            }
            ValueCoercionPolicy::VmInquiryNoSideEffects => {
                if self.may_allocate || self.may_call_user_code || self.may_throw {
                    return Err(
                        ConversionDescriptorValidationError::PureConversionHasObservableEffects(
                            self.name,
                        ),
                    );
                }
            }
        }

        let policy_matches_target = match self.target {
            ValueCoercionTarget::Boolean => {
                self.minimum_policy == ValueCoercionPolicy::PureClassification
            }
            ValueCoercionTarget::Object => {
                matches!(
                    self.minimum_policy,
                    ValueCoercionPolicy::AllowAllocation | ValueCoercionPolicy::AllowUserCode
                )
            }
            ValueCoercionTarget::Primitive(_)
            | ValueCoercionTarget::Number(_)
            | ValueCoercionTarget::String
            | ValueCoercionTarget::PropertyKey
            | ValueCoercionTarget::PropertyName => true,
        };
        if !policy_matches_target {
            return Err(ConversionDescriptorValidationError::TargetPolicyMismatch(
                self.name,
            ));
        }

        Ok(())
    }

    pub fn plan_request(
        &self,
        request: ValueCoercionRequest,
    ) -> Result<ConversionPlan<'_>, ValueCoercionError> {
        self.validate()
            .map_err(|_| ValueCoercionError::SideEffectsDisallowed)?;
        if self.target != request.target {
            return Err(ValueCoercionError::SideEffectsDisallowed);
        }

        let value_kind = request.value.kind();
        let classification = self.classify_value(value_kind)?;
        let required_policy = classification.required_policy(self.minimum_policy);
        if !policy_allows(request.policy, required_policy) {
            return match required_policy {
                ValueCoercionPolicy::AllowAllocation if !self.may_call_user_code => {
                    Err(ValueCoercionError::AllocationRequired)
                }
                _ => Err(ValueCoercionError::SideEffectsDisallowed),
            };
        }

        Ok(ConversionPlan {
            descriptor: self,
            value_kind,
            classification,
            required_policy,
            may_allocate: classification.may_allocate(self),
            may_call_user_code: classification.may_call_user_code(self),
            may_throw: classification.may_throw(self),
        })
    }

    pub fn classify_value(
        &self,
        value_kind: ValueKind,
    ) -> Result<ConversionAlgorithmClass, ValueCoercionError> {
        match self.target {
            ValueCoercionTarget::Boolean => Ok(ConversionAlgorithmClass::PureValueBits),
            ValueCoercionTarget::Primitive(_) if value_kind != ValueKind::Cell => {
                Ok(ConversionAlgorithmClass::PureValueBits)
            }
            ValueCoercionTarget::Object
                if matches!(value_kind, ValueKind::Undefined | ValueKind::Null) =>
            {
                Err(ValueCoercionError::ObjectCoercion(
                    ObjectCoercionFailure::NullOrUndefined,
                ))
            }
            ValueCoercionTarget::Object if value_kind == ValueKind::Cell => {
                Ok(ConversionAlgorithmClass::VmInquiry)
            }
            ValueCoercionTarget::Object => Ok(ConversionAlgorithmClass::RequiresAllocation),
            _ if self.may_call_user_code => Ok(ConversionAlgorithmClass::RequiresUserCode),
            _ if self.may_allocate => Ok(ConversionAlgorithmClass::RequiresAllocation),
            ValueCoercionTarget::Number(_)
                if matches!(value_kind, ValueKind::Int32 | ValueKind::Double) =>
            {
                Ok(ConversionAlgorithmClass::PureValueBits)
            }
            _ => Ok(ConversionAlgorithmClass::VmInquiry),
        }
    }
}

#[derive(Clone, Copy, Debug, Eq, PartialEq)]
pub enum ConversionAlgorithmClass {
    PureValueBits,
    VmInquiry,
    RequiresAllocation,
    RequiresUserCode,
}

impl ConversionAlgorithmClass {
    fn required_policy(self, descriptor_policy: ValueCoercionPolicy) -> ValueCoercionPolicy {
        match self {
            Self::PureValueBits => ValueCoercionPolicy::PureClassification,
            Self::VmInquiry => ValueCoercionPolicy::VmInquiryNoSideEffects,
            Self::RequiresAllocation => ValueCoercionPolicy::AllowAllocation,
            Self::RequiresUserCode => descriptor_policy,
        }
    }

    fn may_allocate(self, descriptor: &ConversionDescriptor) -> bool {
        matches!(self, Self::RequiresAllocation | Self::RequiresUserCode) && descriptor.may_allocate
    }

    fn may_call_user_code(self, descriptor: &ConversionDescriptor) -> bool {
        matches!(self, Self::RequiresUserCode) && descriptor.may_call_user_code
    }

    fn may_throw(self, descriptor: &ConversionDescriptor) -> bool {
        matches!(self, Self::RequiresAllocation | Self::RequiresUserCode) && descriptor.may_throw
    }
}

#[derive(Clone, Copy, Debug, Eq, PartialEq)]
pub struct ConversionPlan<'descriptor> {
    pub descriptor: &'descriptor ConversionDescriptor,
    pub value_kind: ValueKind,
    pub classification: ConversionAlgorithmClass,
    pub required_policy: ValueCoercionPolicy,
    pub may_allocate: bool,
    pub may_call_user_code: bool,
    pub may_throw: bool,
}

fn policy_allows(available: ValueCoercionPolicy, required: ValueCoercionPolicy) -> bool {
    policy_rank(available) >= policy_rank(required)
}

fn policy_rank(policy: ValueCoercionPolicy) -> u8 {
    match policy {
        ValueCoercionPolicy::PureClassification => 0,
        ValueCoercionPolicy::VmInquiryNoSideEffects => 1,
        ValueCoercionPolicy::AllowAllocation => 2,
        ValueCoercionPolicy::AllowUserCode => 3,
    }
}

#[derive(Clone, Copy, Debug, Eq, PartialEq)]
pub struct ConversionDescriptorBuilder {
    descriptor: ConversionDescriptor,
}

impl ConversionDescriptorBuilder {
    pub const fn new(
        name: &'static str,
        target: ValueCoercionTarget,
        minimum_policy: ValueCoercionPolicy,
    ) -> Self {
        Self {
            descriptor: ConversionDescriptor::new(name, target, minimum_policy),
        }
    }

    pub const fn may_allocate(mut self, may_allocate: bool) -> Self {
        self.descriptor.may_allocate = may_allocate;
        self
    }

    pub const fn may_call_user_code(mut self, may_call_user_code: bool) -> Self {
        self.descriptor.may_call_user_code = may_call_user_code;
        self
    }

    pub const fn may_throw(mut self, may_throw: bool) -> Self {
        self.descriptor.may_throw = may_throw;
        self
    }

    pub const fn owner(mut self, owner: ConversionSchemaOwner) -> Self {
        self.descriptor.owner = owner;
        self
    }

    pub fn build(self) -> Result<ConversionDescriptor, ConversionDescriptorValidationError> {
        self.descriptor.validate()?;
        Ok(self.descriptor)
    }
}

pub const STATIC_CONVERSION_DESCRIPTORS: &[ConversionDescriptor] = &[
    ConversionDescriptor {
        name: "ToPrimitive",
        target: ValueCoercionTarget::Primitive(PreferredPrimitiveType::NoPreference),
        minimum_policy: ValueCoercionPolicy::AllowUserCode,
        may_allocate: false,
        may_call_user_code: true,
        may_throw: true,
        owner: ConversionSchemaOwner::ValueConversionSchema,
    },
    ConversionDescriptor {
        name: "ToBoolean",
        target: ValueCoercionTarget::Boolean,
        minimum_policy: ValueCoercionPolicy::PureClassification,
        may_allocate: false,
        may_call_user_code: false,
        may_throw: false,
        owner: ConversionSchemaOwner::ValueConversionSchema,
    },
    ConversionDescriptor {
        name: "ToNumber",
        target: ValueCoercionTarget::Number(NumericConversionHint::Number),
        minimum_policy: ValueCoercionPolicy::AllowUserCode,
        may_allocate: false,
        may_call_user_code: true,
        may_throw: true,
        owner: ConversionSchemaOwner::ValueConversionSchema,
    },
    ConversionDescriptor {
        name: "ToString",
        target: ValueCoercionTarget::String,
        minimum_policy: ValueCoercionPolicy::AllowUserCode,
        may_allocate: true,
        may_call_user_code: true,
        may_throw: true,
        owner: ConversionSchemaOwner::ValueConversionSchema,
    },
    ConversionDescriptor {
        name: "ToObject",
        target: ValueCoercionTarget::Object,
        minimum_policy: ValueCoercionPolicy::AllowAllocation,
        may_allocate: true,
        may_call_user_code: false,
        may_throw: true,
        owner: ConversionSchemaOwner::ValueConversionSchema,
    },
    ConversionDescriptor {
        name: "ToPropertyKey",
        target: ValueCoercionTarget::PropertyKey,
        minimum_policy: ValueCoercionPolicy::AllowUserCode,
        may_allocate: true,
        may_call_user_code: true,
        may_throw: true,
        owner: ConversionSchemaOwner::ValueConversionSchema,
    },
];

pub const STATIC_CONVERSION_DESCRIPTOR_REGISTRY: ConversionDescriptorRegistry =
    ConversionDescriptorRegistry {
        name: "value.conversion.static-descriptors",
        authority: ConversionRegistryAuthority::StaticReadOnly,
        descriptors: STATIC_CONVERSION_DESCRIPTORS,
    };

pub const fn static_conversion_descriptor_registry() -> &'static ConversionDescriptorRegistry {
    &STATIC_CONVERSION_DESCRIPTOR_REGISTRY
}

pub const fn static_conversion_descriptors() -> &'static [ConversionDescriptor] {
    STATIC_CONVERSION_DESCRIPTORS
}

/// One conversion request.
///
/// The request owns a copy of the value representation. Any object payload is
/// still heap-owned and must be rooted or barriered by the caller/VM.
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
///
/// Hook implementations are the mutation boundary for allocation, user-code
/// calls, and exception creation. `JsValue` itself stays inert.
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

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn static_conversion_descriptors_are_structurally_valid() {
        assert_eq!(static_conversion_descriptor_registry().validate(), Ok(()));
    }

    #[test]
    fn conversion_builder_constructs_allocating_descriptor() {
        let descriptor = ConversionDescriptorBuilder::new(
            "ToObjectLike",
            ValueCoercionTarget::Object,
            ValueCoercionPolicy::AllowAllocation,
        )
        .may_allocate(true)
        .may_throw(true)
        .build();

        assert_eq!(
            descriptor.map(|descriptor| descriptor.target),
            Ok(ValueCoercionTarget::Object)
        );
    }

    #[test]
    fn conversion_validator_rejects_effectful_pure_descriptor() {
        let descriptor = ConversionDescriptorBuilder::new(
            "bad",
            ValueCoercionTarget::Boolean,
            ValueCoercionPolicy::PureClassification,
        )
        .may_throw(true)
        .build();

        assert_eq!(
            descriptor,
            Err(ConversionDescriptorValidationError::PureConversionHasObservableEffects("bad"))
        );
    }

    #[test]
    fn conversion_planner_keeps_toboolean_pure_for_cells() {
        let plan = static_conversion_descriptor_registry().plan_request(ValueCoercionRequest {
            value: JsValue::from_encoded(crate::value::EncodedJsValue(0x20)),
            target: ValueCoercionTarget::Boolean,
            policy: ValueCoercionPolicy::PureClassification,
        });

        assert_eq!(
            plan.map(|plan| plan.classification),
            Ok(ConversionAlgorithmClass::PureValueBits)
        );
    }

    #[test]
    fn conversion_planner_rejects_allocation_when_policy_disallows_it() {
        let plan = static_conversion_descriptor_registry().plan_request(ValueCoercionRequest {
            value: JsValue::from_i32(1),
            target: ValueCoercionTarget::Object,
            policy: ValueCoercionPolicy::PureClassification,
        });

        assert_eq!(plan, Err(ValueCoercionError::AllocationRequired));
    }

    #[test]
    fn conversion_planner_classifies_user_code_boundary() {
        let plan = static_conversion_descriptor_registry().plan_request(ValueCoercionRequest {
            value: JsValue::from_encoded(crate::value::EncodedJsValue(0x20)),
            target: ValueCoercionTarget::Primitive(PreferredPrimitiveType::NoPreference),
            policy: ValueCoercionPolicy::AllowUserCode,
        });

        assert_eq!(
            plan.map(|plan| (plan.classification, plan.may_call_user_code)),
            Ok((ConversionAlgorithmClass::RequiresUserCode, true))
        );
    }
}
