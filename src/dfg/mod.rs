//! Data Flow Graph tier design contracts.
//!
//! The DFG is planned as an optional optimized tier. This module reserves the
//! ownership shape for IR graphs, speculation, OSR, and invalidation without
//! compiling or executing optimized code.

#![forbid(unsafe_code)]

pub(crate) mod frozen_value;
pub(crate) mod graph;
pub(crate) mod node_flags;
pub(crate) mod node_type;
pub(crate) mod osr;
pub(crate) mod parser;
pub(crate) mod plan;
pub(crate) mod speculation;
pub(crate) mod variable_access_data;
pub(crate) mod watchpoint;

pub use frozen_value::{merge_strength, FrozenValue, FrozenValueId, ValueStrength};
pub use graph::{
    BranchTarget, DfgBasicBlock, DfgBasicBlockId, DfgCommonDataDescriptor, DfgDominanceSet,
    DfgEdge, DfgEdgeId, DfgGraph, DfgGraphId, DfgGraphMutationAuthority, DfgInlineCallFrameId,
    DfgNode, DfgNodeId, DfgPhase, DfgPlanDescriptorRegistry, DfgPlanRegistryMutationAuthority,
    DfgPlanSchemaOwner, DfgValidationError, DfgVariableAccessDataId, EdgeKillStatus,
    EdgeProofStatus, EdgeUseKind, GraphForm, InlineVariableData, NodeEffects, NodeOrigin,
    NodeOriginKind, StaticDfgPlanDescriptor, DFG_PLAN_DESCRIPTOR_REGISTRY,
    STATIC_DFG_PLAN_DESCRIPTORS,
};
pub use node_flags::{
    bytecode_can_ignore_nan_and_infinity, bytecode_can_ignore_negative_zero,
    bytecode_can_truncate_integer, bytecode_uses_as_number, NodeFlags, NODE_ARITH_FLAGS_MASK,
    NODE_BEHAVIOR_MASK, NODE_BYTECODE_BACK_PROP_MASK, NODE_BYTECODE_NEEDS_NAN_OR_INFINITY,
    NODE_BYTECODE_NEEDS_NEG_ZERO, NODE_BYTECODE_PREFERS_ARRAY_INDEX,
    NODE_BYTECODE_USES_AS_ARRAY_INDEX, NODE_BYTECODE_USES_AS_INT, NODE_BYTECODE_USES_AS_NUMBER,
    NODE_BYTECODE_USES_AS_OTHER, NODE_BYTECODE_USES_AS_VALUE, NODE_BYTECODE_USE_BOTTOM,
    NODE_HAS_VAR_ARGS, NODE_IS_FLUSHED, NODE_MAY_HAVE_BIG_INT32_RESULT,
    NODE_MAY_HAVE_DOUBLE_RESULT, NODE_MAY_HAVE_HEAP_BIG_INT_RESULT, NODE_MAY_HAVE_NON_INT_RESULT,
    NODE_MAY_HAVE_NON_NUMERIC_RESULT, NODE_MAY_NEG_ZERO_IN_BASELINE, NODE_MAY_NEG_ZERO_IN_DFG,
    NODE_MAY_OVERFLOW_INT32_IN_BASELINE, NODE_MAY_OVERFLOW_INT32_IN_DFG, NODE_MAY_OVERFLOW_INT52,
    NODE_MISC_FLAG1, NODE_MISC_FLAG2, NODE_MUST_GENERATE, NODE_RESULT_BOOLEAN, NODE_RESULT_DOUBLE,
    NODE_RESULT_INT32, NODE_RESULT_INT52, NODE_RESULT_JS, NODE_RESULT_MASK, NODE_RESULT_NUMBER,
    NODE_RESULT_STORAGE,
};
pub use node_type::{default_flags, NodeType};
pub use osr::{
    DfgOsrEntryDescriptor, DfgOsrExitDescriptor, DfgOsrExitId, ExitProfileUpdate, FlushFormat,
    MaterializationKind, OsrEntryAvailability, OsrEntryKind, OsrExitKind, OsrExitOutcomeKind,
    OsrExitOutcomeRecord, OsrExitRecovery, RecoverySource,
};
pub use parser::{parse, parse_into, DeclineReason};
pub use plan::{DfgCompilationMode, DfgPlan};
pub use speculation::{
    AbstractValueSource, DfgValueRep, PredictionSource, SpeculatedType, SpeculationCheck,
    SpeculationCheckId, SpeculationCheckKind, SpeculationDirection, SpeculationFailureSemantics,
    SpeculationPreconditionKind, SpeculationRecovery, SpeculationRecoveryKind,
    SpeculationSemanticValidationError, SpeculationSite,
};
pub use variable_access_data::VariableAccessData;
pub use watchpoint::{
    DesiredWatchpointCounts, DfgDesiredWatchpoints, DfgInvalidationPoint, DfgInvalidationPointId,
    DfgWatchpointCollectionPhase, InvalidationPointKind, WatchpointCollectionMode,
    WatchpointRegistrationConcurrency,
};
