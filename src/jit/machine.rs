//! Machine-code ownership and patching descriptors.
//!
//! This module deliberately avoids executable allocation, pointer arithmetic,
//! cache flushing, and instruction encoding. It names the metadata needed by
//! future link buffers, patchpoints, and executable-memory handles.

use crate::jit::{CallBoundaryId, JitCodeId, PatchpointDescriptor};
use crate::runtime::{CodeBlockId, NativeCodeId};

/// Stable identity for an executable allocation.
#[derive(Clone, Copy, Debug, Eq, Hash, Ord, PartialEq, PartialOrd)]
pub struct ExecutableAllocationId(pub u64);

/// Ownership source for executable memory.
#[derive(Clone, Copy, Debug, Eq, PartialEq)]
pub enum MachineCodeOwnership {
    CodeBlock(CodeBlockId),
    SharedStub,
    Thunk,
    WasmCallee,
    Host,
}

/// Protection state of an executable allocation.
#[derive(Clone, Copy, Debug, Eq, PartialEq)]
pub enum ExecutableMemoryProtection {
    Unallocated,
    Writable,
    Executable,
    WritableForPatching,
    Decommitted,
}

/// Offset range inside an executable allocation.
#[derive(Clone, Copy, Debug, Eq, PartialEq)]
pub struct MachineCodeRange {
    pub allocation: ExecutableAllocationId,
    pub start_offset: u32,
    pub size_bytes: u32,
}

/// Opaque machine-code handle attached to a JIT artifact.
#[derive(Clone, Copy, Debug, Eq, PartialEq)]
pub struct MachineCodeHandle {
    pub allocation: ExecutableAllocationId,
    pub owner: MachineCodeOwnership,
    pub range: MachineCodeRange,
    pub symbol: Option<NativeCodeId>,
    pub protection: ExecutableMemoryProtection,
}

/// Relocation or patch family.
#[derive(Clone, Copy, Debug, Eq, PartialEq)]
pub enum RelocationKind {
    Entrypoint,
    NearCall,
    FarCall,
    Jump,
    DataPointer,
    InlineCacheData,
    WatchpointJump,
    OsrExitJump,
    ExceptionHandler,
}

/// Patch lifecycle state.
#[derive(Clone, Copy, Debug, Eq, PartialEq)]
pub enum CodePatchState {
    Reserved,
    Linked,
    Armed,
    Applied,
    RolledBack,
    Invalidated,
}

/// Barrier required before a patch can be applied.
#[derive(Clone, Copy, Debug, Eq, PartialEq)]
pub enum PatchWriteBarrier {
    MainThreadOnly,
    StopTheWorld,
    CodeBlockLock,
    ExecutableAllocatorLock,
    NoneRequired,
}

/// One patchable machine-code location.
#[derive(Clone, Debug, Eq, PartialEq)]
pub struct CodePatchRecord {
    pub code: JitCodeId,
    pub location: PatchpointDescriptor,
    pub relocation: RelocationKind,
    pub range: Option<MachineCodeRange>,
    pub boundary: Option<CallBoundaryId>,
    pub state: CodePatchState,
}

/// Data-only patch plan. Applying the plan belongs to the future assembler and
/// executable-memory layer.
#[derive(Clone, Debug, Eq, PartialEq)]
pub struct CodePatchPlan {
    pub owner: CodeBlockId,
    pub records: Vec<CodePatchRecord>,
    pub write_barrier: PatchWriteBarrier,
    pub required_protection: ExecutableMemoryProtection,
    pub generation: u64,
}
