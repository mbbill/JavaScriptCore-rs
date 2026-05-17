//! On-stack replacement entry and exit metadata.
//!
//! OSR entries and exits are represented as descriptors only. The VM and
//! machine-code layers will later own stack inspection, jump targets, patching,
//! and value materialization.

use crate::bytecode::VirtualRegister;
use crate::dfg::{BasicBlockId, DfgNodeId, SpeculationCheckId, SpeculationSite};
use crate::jit::{CallBoundaryId, JitCodeId, PatchpointDescriptor};
use crate::runtime::CodeBlockId;

/// Availability state for a bytecode index that may OSR into optimized code.
#[derive(Clone, Copy, Debug, Eq, PartialEq)]
pub enum OsrEntryAvailability {
    Unknown,
    Unavailable,
    Candidate,
    Prepared,
    Installed,
    Invalidated,
}

/// OSR entry family.
#[derive(Clone, Copy, Debug, Eq, PartialEq)]
pub enum OsrEntryKind {
    Loop,
    Catch,
    FunctionEntry,
    TierReplacement,
}

/// Value format expected at an OSR boundary.
#[derive(Clone, Copy, Debug, Eq, PartialEq)]
pub enum FlushFormat {
    Dead,
    FlushedJSValue,
    FlushedCell,
    FlushedBoolean,
    FlushedInt32,
    FlushedInt52,
    FlushedDouble,
    InRegister,
}

/// OSR entry reshuffling from a baseline frame location to optimized layout.
#[derive(Clone, Copy, Debug, Eq, PartialEq)]
pub struct RecoverySource {
    pub source: VirtualRegister,
    pub format: FlushFormat,
    pub stack_offset: Option<i32>,
}

/// Descriptor for an OSR entry target.
#[derive(Clone, Debug, Eq, PartialEq)]
pub struct DfgOsrEntryDescriptor {
    pub owner: CodeBlockId,
    pub kind: OsrEntryKind,
    pub bytecode_index: u32,
    pub target_block: Option<BasicBlockId>,
    pub optimized_code: Option<JitCodeId>,
    pub boundary: Option<CallBoundaryId>,
    pub availability: OsrEntryAvailability,
    pub expected_values: Vec<RecoverySource>,
    pub patchpoint: Option<PatchpointDescriptor>,
}

/// Stable identity for an OSR exit site.
#[derive(Clone, Copy, Debug, Eq, Hash, Ord, PartialEq, PartialOrd)]
pub struct DfgOsrExitId(pub u32);

/// Why optimized execution may leave the current tier.
#[derive(Clone, Copy, Debug, Eq, PartialEq)]
pub enum OsrExitKind {
    BadType,
    BadCell,
    BadStructure,
    Overflow,
    NegativeZero,
    BoundsCheck,
    Watchpoint,
    Uncountable,
    Exception,
    Unreachable,
}

/// Materialization operation needed by exit recovery.
#[derive(Clone, Copy, Debug, Eq, PartialEq)]
pub enum MaterializationKind {
    None,
    ArgumentsObject,
    DirectArguments,
    ClonedArguments,
    ObjectAllocationSinking,
    ActivationRecord,
}

/// Value recovery entry for one virtual register at exit time.
#[derive(Clone, Copy, Debug, Eq, PartialEq)]
pub struct OsrExitRecovery {
    pub virtual_register: VirtualRegister,
    pub source: RecoverySource,
    pub materialization: MaterializationKind,
}

/// Exit profile feedback that may later feed tiering policy.
#[derive(Clone, Copy, Debug, Eq, PartialEq)]
pub struct ExitProfileUpdate {
    pub site: SpeculationSite,
    pub exit_kind: OsrExitKind,
    pub counter_increment: u32,
    pub should_mark_frequent: bool,
}

/// Complete OSR exit descriptor.
#[derive(Clone, Debug, Eq, PartialEq)]
pub struct DfgOsrExitDescriptor {
    pub id: DfgOsrExitId,
    pub owner: CodeBlockId,
    pub node: Option<DfgNodeId>,
    pub check: Option<SpeculationCheckId>,
    pub kind: OsrExitKind,
    pub bytecode_index: u32,
    pub target_bytecode_index: u32,
    pub recoveries: Vec<OsrExitRecovery>,
    pub patchpoint: Option<PatchpointDescriptor>,
    pub profile_update: Option<ExitProfileUpdate>,
}
