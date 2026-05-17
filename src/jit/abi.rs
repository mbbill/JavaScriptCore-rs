//! Reserved ABI boundary for future LLInt, generated-code, and host bridges.
//!
//! This module names the metadata that code blocks, ICs, and Wasm bridges will
//! need before any machine-code generator exists. It deliberately stores
//! symbolic locations and calling conventions instead of function pointers or
//! executable memory.

use crate::jit::JitCodeId;
use crate::runtime::{CodeBlockId, NativeCodeId};

/// Abstract execution entry kind attached to code-block-equivalent state.
#[derive(Clone, Copy, Debug, Eq, PartialEq)]
pub enum EntrypointKind {
    /// No executable entry has been attached.
    None,
    /// Interpreter or LLInt-compatible thunk reserved before JIT exists.
    InterpreterThunk,
    /// Future generated machine-code entry.
    GeneratedCode,
    /// Host/native callback bridge entry.
    HostBridge,
    /// Future Wasm bridge or thunk entry.
    WasmBridge,
}

/// ABI family for a reserved entrypoint.
#[derive(Clone, Copy, Debug, Eq, PartialEq)]
pub enum EntryAbi {
    /// Rust-owned call boundary with no layout compatibility promise.
    Rust,
    /// Reserved compatibility boundary for LLInt/JIT-visible frames.
    LlIntCompatible,
    /// Future generated-code ABI with register and stack conventions.
    GeneratedCode,
    /// ABI is intentionally not selected by the skeleton.
    Deferred,
    /// Reserved JS-to-Wasm or Wasm-to-JS ABI.
    Wasm,
}

/// Opaque entrypoint descriptor.
#[derive(Clone, Copy, Debug, Eq, PartialEq)]
pub struct Entrypoint {
    pub kind: EntrypointKind,
    pub abi: EntryAbi,
    pub code: Option<JitCodeId>,
    pub boundary: Option<CallBoundaryId>,
}

/// Stable identity for a call boundary metadata record.
#[derive(Clone, Copy, Debug, Eq, Hash, Ord, PartialEq, PartialOrd)]
pub struct CallBoundaryId(pub u64);

/// Symbolic register families used by ABI metadata.
#[derive(Clone, Copy, Debug, Eq, PartialEq)]
pub enum RegisterRole {
    Argument,
    Return,
    CalleeSave,
    CallerSave,
    Scratch,
    PinnedVm,
    PinnedCallFrame,
    PinnedWasmContext,
}

/// Register descriptor without naming a physical architecture register.
#[derive(Clone, Copy, Debug, Eq, PartialEq)]
pub struct RegisterBinding {
    pub role: RegisterRole,
    pub index: u8,
    pub value: AbiValue,
}

/// Value category carried across a reserved ABI edge.
#[derive(Clone, Copy, Debug, Eq, PartialEq)]
pub enum AbiValue {
    JsValue,
    Cell,
    Int32,
    Int64,
    Float32,
    Float64,
    Pointer,
    WasmExternRef,
    WasmFuncRef,
    Void,
}

/// Stack slot role in a future frame layout.
#[derive(Clone, Copy, Debug, Eq, PartialEq)]
pub enum FrameSlotRole {
    ReturnAddress,
    CallerFrame,
    CalleeSaves,
    Arguments,
    Locals,
    Spill,
    ExceptionHandler,
    WasmScratch,
}

/// Symbolic frame slot. Offsets remain optional until layout code exists.
#[derive(Clone, Copy, Debug, Eq, PartialEq)]
pub struct FrameSlot {
    pub role: FrameSlotRole,
    pub index: u32,
    pub byte_offset: Option<i32>,
}

/// Metadata for a single executable or bridge boundary.
#[derive(Clone, Debug, Eq, PartialEq)]
pub struct CallBoundaryMetadata {
    pub id: CallBoundaryId,
    pub owner: Option<CodeBlockId>,
    pub abi: EntryAbi,
    pub entry_kind: EntrypointKind,
    pub native_symbol: Option<NativeCodeId>,
    pub arguments: Vec<AbiValue>,
    pub returns: Vec<AbiValue>,
    pub registers: Vec<RegisterBinding>,
    pub frame_slots: Vec<FrameSlot>,
    pub requires_vm_entry_scope: bool,
    pub may_call_js: bool,
    pub may_throw: bool,
}

/// Patchable location category reserved for generated code and thunks.
#[derive(Clone, Copy, Debug, Eq, PartialEq)]
pub enum PatchpointKind {
    Entrypoint,
    SlowPathCall,
    InlineCacheData,
    DirectCallTarget,
    WasmEntrypointLoad,
    ExceptionHandler,
}

/// Symbolic patchpoint owned by generated code metadata.
#[derive(Clone, Copy, Debug, Eq, PartialEq)]
pub struct PatchpointDescriptor {
    pub kind: PatchpointKind,
    pub owner_code: Option<JitCodeId>,
    pub byte_offset: Option<u32>,
    pub boundary: Option<CallBoundaryId>,
}
