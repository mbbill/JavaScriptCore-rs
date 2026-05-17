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

#[derive(Clone, Copy, Debug, Eq, PartialEq)]
pub enum LolJitStatus {
    PreservedForCompatibility,
    DisabledByPolicy,
    RegisterAllocationPlanned,
    CodeGenerationPlanned,
    ReplacedByMainJit,
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
    pub physical_registers_reserved: u32,
    pub spills_allowed: bool,
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
    pub operations: Vec<LolOperationBoundary>,
    pub buffer: Option<AssemblerBufferId>,
    pub entry_label: Option<AssemblerLabel>,
    pub allocation: Option<ExecutableAllocationId>,
    pub code: Option<JitCodeId>,
}
