//! Internal tools and diagnostic contracts.
//!
//! JavaScriptCore's `tools/` directory contains verifier, profiling, testing,
//! and VM-inspection utilities. These are not runtime semantics, but they are
//! important for a staged rewrite because they define how correctness,
//! debugging, and instrumentation will observe the engine.

use crate::gc::{HeapId, HeapSnapshotId};
use crate::jit::CompilationPlanId;
use crate::profiler::ProfilerRunId;
use crate::runtime::{CellId, CodeBlockId, ObjectId};
use crate::strings::Identifier;

#[derive(Clone, Copy, Debug, Default, Eq, Hash, PartialEq)]
pub struct ToolInvocationId(pub u64);

#[derive(Clone, Copy, Debug, Eq, PartialEq)]
pub enum ToolKind {
    IntegrityVerifier,
    HeapVerifier,
    SourceProfiler,
    VmInspector,
    DollarVmTestingHook,
    FunctionAllowlist,
    FunctionOverrides,
    CompilerTiming,
    LlvmProfiling,
}

#[derive(Clone, Debug, Eq, PartialEq)]
pub struct ToolInvocation {
    pub id: ToolInvocationId,
    pub kind: ToolKind,
    pub requires_mutable_vm: bool,
    pub observes_gc: bool,
    pub may_force_compilation: bool,
}

#[derive(Clone, Debug, Default, Eq, PartialEq)]
pub struct IntegrityCheckPlan {
    pub heap: Option<HeapId>,
    pub object: Option<ObjectId>,
    pub cell: Option<CellId>,
    pub verify_structures: bool,
    pub verify_watchpoints: bool,
    pub verify_write_barriers: bool,
}

#[derive(Clone, Debug, Default, Eq, PartialEq)]
pub struct HeapVerifierPlan {
    pub heap: Option<HeapId>,
    pub snapshot: Option<HeapSnapshotId>,
    pub include_weak_sets: bool,
    pub include_finalizers: bool,
    pub include_mark_bits: bool,
}

#[derive(Clone, Debug, Default, Eq, PartialEq)]
pub struct SourceProfilerPlan {
    pub run: Option<ProfilerRunId>,
    pub code_block: Option<CodeBlockId>,
    pub include_bytecode_counters: bool,
    pub include_jit_tiers: bool,
}

#[derive(Clone, Debug, Default, Eq, PartialEq)]
pub struct FunctionOverridePlan {
    pub name: Option<Identifier>,
    pub allowlist_enabled: bool,
    pub override_enabled: bool,
    pub affected_compilation_plan: Option<CompilationPlanId>,
}
