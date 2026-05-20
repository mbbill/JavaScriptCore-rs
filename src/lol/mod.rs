//! LOL JIT contracts.
//!
//! JavaScriptCore's `lol/` directory is a small JIT experiment with its own
//! register allocator and operation boundary. The Rust rewrite should not fold
//! those assumptions into the main JIT blindly; this module keeps the concept
//! visible until the project decides whether to preserve, replace, or delete it.

use crate::assembler::{AssemblerBufferId, AssemblerLabel};
use crate::jit::{CallBoundaryId, ExecutableAllocationId, JitCodeId};
use crate::runtime::{CodeBlockId, RuntimeValue};

#[derive(Clone, Copy, Debug, Default, Eq, Hash, PartialEq)]
pub struct LolJitPlanId(pub u64);

#[derive(Clone, Copy, Debug, Default, Eq, Hash, PartialEq)]
pub struct LolOperationId(pub u32);

#[derive(Clone, Copy, Debug, Default, Eq, Hash, PartialEq)]
pub struct LolVirtualRegisterId(pub u32);

#[derive(Clone, Copy, Debug, Eq, PartialEq)]
pub enum LolJitStatus {
    PreservedForCompatibility,
    DisabledByPolicy,
    RegisterAllocationPlanned,
    CodeGenerationPlanned,
    ReplacedByMainJit,
}

#[derive(Clone, Copy, Debug, Eq, PartialEq)]
pub enum LolOperationFamily {
    Arithmetic,
    Conversion,
    Scope,
    PropertyAccess,
    ControlFlow,
    Allocation,
    Throw,
    SlowCaseOnly,
}

#[derive(Clone, Debug, Eq, PartialEq)]
pub struct LolOperationDescriptor {
    pub id: LolOperationId,
    pub family: LolOperationFamily,
    pub has_slow_case: bool,
    pub operation_boundary: Option<CallBoundaryId>,
    /// LOL operations read directly from bytecode in C++; Rust contracts keep
    /// that bytecode-stream dependency explicit and isolated from the main JIT.
    pub bytecode_offset: Option<u32>,
}

#[derive(Clone, Copy, Debug, Eq, PartialEq)]
pub enum LolRegisterClass {
    GeneralPurpose,
    FloatingPoint,
    Temporary,
    PinnedVm,
}

#[derive(Clone, Debug, Eq, PartialEq)]
pub struct LolRegisterAllocationPlan {
    pub class: LolRegisterClass,
    pub virtual_registers: u32,
    pub tracked_virtual_registers: Vec<LolVirtualRegisterId>,
    pub physical_registers_reserved: u32,
    pub spills_allowed: bool,
    pub scratches_require_release: bool,
}

#[derive(Clone, Debug, Eq, PartialEq)]
pub struct LolOperationBoundary {
    pub boundary: CallBoundaryId,
    pub may_reenter_vm: bool,
    pub may_allocate: bool,
    pub result_placeholder: RuntimeValue,
}

#[derive(Clone, Debug, Eq, PartialEq)]
pub struct LolJitPlan {
    pub id: LolJitPlanId,
    pub owner: Option<CodeBlockId>,
    pub status: LolJitStatus,
    pub register_allocation: Vec<LolRegisterAllocationPlan>,
    pub implemented_operations: Vec<LolOperationDescriptor>,
    pub operations: Vec<LolOperationBoundary>,
    pub buffer: Option<AssemblerBufferId>,
    pub entry_label: Option<AssemblerLabel>,
    pub allocation: Option<ExecutableAllocationId>,
    pub code: Option<JitCodeId>,
}

#[derive(Clone, Debug, Eq, PartialEq)]
pub enum LolValidationError {
    EmptyName,
    EmptyProvenance(&'static str),
    DuplicateTierName(&'static str),
    EmptyOperationFamilies(&'static str),
    EmptyRegisterClasses(&'static str),
    OperationFamilyNotAllowed(LolOperationFamily),
    RegisterClassNotAllowed(LolRegisterClass),
    DuplicateOperationId(LolOperationId),
    BoundaryMissingOperation(CallBoundaryId),
    RegisterCountMismatch(LolRegisterClass),
    CodeWithoutAllocation,
    EntryLabelWithoutBuffer,
}

impl LolJitPlan {
    pub fn validate_against(&self, schema: &StaticLolTierSchema) -> Result<(), LolValidationError> {
        for (index, operation) in self.implemented_operations.iter().enumerate() {
            if !schema.operation_families.contains(&operation.family) {
                return Err(LolValidationError::OperationFamilyNotAllowed(
                    operation.family,
                ));
            }
            if self.implemented_operations[index + 1..]
                .iter()
                .any(|other| other.id == operation.id)
            {
                return Err(LolValidationError::DuplicateOperationId(operation.id));
            }
        }

        for allocation in &self.register_allocation {
            if !schema.register_classes.contains(&allocation.class) {
                return Err(LolValidationError::RegisterClassNotAllowed(
                    allocation.class,
                ));
            }
            if allocation.tracked_virtual_registers.len() as u32 > allocation.virtual_registers {
                return Err(LolValidationError::RegisterCountMismatch(allocation.class));
            }
        }

        for boundary in &self.operations {
            if !self
                .implemented_operations
                .iter()
                .any(|operation| operation.operation_boundary == Some(boundary.boundary))
            {
                return Err(LolValidationError::BoundaryMissingOperation(
                    boundary.boundary,
                ));
            }
        }

        if self.code.is_some() && self.allocation.is_none() {
            return Err(LolValidationError::CodeWithoutAllocation);
        }
        if self.entry_label.is_some() && self.buffer.is_none() {
            return Err(LolValidationError::EntryLabelWithoutBuffer);
        }

        Ok(())
    }
}

#[derive(Clone, Copy, Debug, Default, Eq, PartialEq, Hash)]
pub enum LolTierSchemaOwner {
    #[default]
    LolTierRegistry,
    LolCompatibilityLayer,
}

#[derive(Clone, Copy, Debug, Default, Eq, PartialEq, Hash)]
pub enum LolTierRegistryMutationAuthority {
    #[default]
    GeneratedStaticDataRefresh,
    CrateInitialization,
}

#[derive(Clone, Copy, Debug, Eq, PartialEq)]
pub struct StaticLolTierSchema {
    pub name: &'static str,
    pub status: LolJitStatus,
    pub operation_families: &'static [LolOperationFamily],
    pub register_classes: &'static [LolRegisterClass],
    pub preserves_main_jit_boundary: bool,
    pub owner: LolTierSchemaOwner,
    pub mutation_authority: LolTierRegistryMutationAuthority,
    pub provenance: &'static str,
}

#[derive(Clone, Copy, Debug, Default, Eq, PartialEq)]
pub struct LolTierSchemaRegistry {
    pub tiers: &'static [StaticLolTierSchema],
}

impl LolTierSchemaRegistry {
    pub const fn new(tiers: &'static [StaticLolTierSchema]) -> Self {
        Self { tiers }
    }

    pub const fn tiers(self) -> &'static [StaticLolTierSchema] {
        self.tiers
    }

    pub fn tier_for_name(self, name: &str) -> Option<&'static StaticLolTierSchema> {
        self.tiers.iter().find(|tier| tier.name == name)
    }

    pub fn validate(self) -> Result<(), LolValidationError> {
        for (index, tier) in self.tiers.iter().enumerate() {
            tier.validate()?;
            if self.tiers[index + 1..]
                .iter()
                .any(|other| other.name == tier.name)
            {
                return Err(LolValidationError::DuplicateTierName(tier.name));
            }
        }

        Ok(())
    }
}

impl StaticLolTierSchema {
    pub fn validate(&self) -> Result<(), LolValidationError> {
        if self.name.is_empty() {
            return Err(LolValidationError::EmptyName);
        }
        if self.provenance.is_empty() {
            return Err(LolValidationError::EmptyProvenance(self.name));
        }
        if self.operation_families.is_empty() {
            return Err(LolValidationError::EmptyOperationFamilies(self.name));
        }
        if self.register_classes.is_empty() {
            return Err(LolValidationError::EmptyRegisterClasses(self.name));
        }

        Ok(())
    }
}

const LOL_OPERATION_FAMILIES: &[LolOperationFamily] = &[
    LolOperationFamily::Arithmetic,
    LolOperationFamily::Conversion,
    LolOperationFamily::Scope,
    LolOperationFamily::PropertyAccess,
    LolOperationFamily::ControlFlow,
    LolOperationFamily::Allocation,
    LolOperationFamily::Throw,
    LolOperationFamily::SlowCaseOnly,
];
const LOL_REGISTER_CLASSES: &[LolRegisterClass] = &[
    LolRegisterClass::GeneralPurpose,
    LolRegisterClass::FloatingPoint,
    LolRegisterClass::Temporary,
    LolRegisterClass::PinnedVm,
];

pub const STATIC_LOL_TIER_SCHEMAS: &[StaticLolTierSchema] = &[StaticLolTierSchema {
    name: "lol-compatibility-tier",
    status: LolJitStatus::PreservedForCompatibility,
    operation_families: LOL_OPERATION_FAMILIES,
    register_classes: LOL_REGISTER_CLASSES,
    preserves_main_jit_boundary: true,
    owner: LolTierSchemaOwner::LolCompatibilityLayer,
    mutation_authority: LolTierRegistryMutationAuthority::GeneratedStaticDataRefresh,
    provenance: "static Rust LOL tier schema",
}];

pub const LOL_TIER_SCHEMA_REGISTRY: LolTierSchemaRegistry =
    LolTierSchemaRegistry::new(STATIC_LOL_TIER_SCHEMAS);

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn static_lol_registry_validates() {
        assert_eq!(LOL_TIER_SCHEMA_REGISTRY.validate(), Ok(()));
    }

    #[test]
    fn lol_plan_rejects_code_without_allocation() {
        let schema = LOL_TIER_SCHEMA_REGISTRY
            .tier_for_name("lol-compatibility-tier")
            .expect("lol compatibility schema");
        let plan = LolJitPlan {
            id: LolJitPlanId(1),
            owner: None,
            status: LolJitStatus::PreservedForCompatibility,
            register_allocation: Vec::new(),
            implemented_operations: Vec::new(),
            operations: Vec::new(),
            buffer: None,
            entry_label: None,
            allocation: None,
            code: Some(JitCodeId(1)),
        };

        assert_eq!(
            plan.validate_against(schema),
            Err(LolValidationError::CodeWithoutAllocation)
        );
    }
}
