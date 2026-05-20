//! Fourth Tier LLVM/B3 pipeline contracts.
//!
//! This module names FTL-level compilation products, lowering boundaries, and
//! install/invalidation points. It deliberately contains no optimizer.

#![forbid(unsafe_code)]

pub(crate) mod lowering;

pub use crate::b3::{
    AirBlockId, AirCodeId, AirInstructionId, B3BlockId, B3ProcedureId, B3ValueId, B3ValueKind,
};
pub use lowering::{
    AirGenerationDescriptor, DfgToB3LoweringBoundary, FtlArtifactDescriptor,
    FtlCompilationDescriptor, FtlCompilationStage, FtlExceptionTarget, FtlLoweringPhase,
    FtlOsrEntryPlan, FtlPatchpointDescriptor, FtlPlanDescriptorRegistry,
    FtlPlanRegistryMutationAuthority, FtlPlanSchemaOwner, FtlSlowPathDescriptor, FtlStateAuthority,
    FtlStateDescriptor, FtlValidationError, LoweringFailureReason, StaticFtlPlanDescriptor,
    FTL_PLAN_DESCRIPTOR_REGISTRY, STATIC_FTL_PLAN_DESCRIPTORS,
};
