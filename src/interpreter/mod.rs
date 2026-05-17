//! Interpreter stack and tracing contracts.
//!
//! The high-level runtime entry API lives in `runtime::interpreter`. This
//! module mirrors JavaScriptCore's `interpreter/` source area: call-frame stack
//! layout, stack visitors, cached calls, microtask call records, and
//! ShadowChicken-style tracing metadata. It still does not dispatch bytecode.

use crate::bytecode::BytecodeIndex;
use crate::runtime::{
    CallFrameId, CodeBlockId, EntryFrameId, ObjectId, RuntimeValue, ScopeId, StackFrameId,
    VmEntryReason,
};

#[derive(Clone, Copy, Debug, Default, Eq, Hash, PartialEq)]
pub struct InterpreterStackId(pub u64);

#[derive(Clone, Copy, Debug, Eq, PartialEq)]
pub enum InterpreterFrameKind {
    Entry,
    JavaScript,
    Native,
    WasmBridge,
    HostCallback,
    Microtask,
}

#[derive(Clone, Debug, Default, Eq, PartialEq)]
pub struct InterpreterFrameRecord {
    pub frame: Option<CallFrameId>,
    pub entry_frame: Option<EntryFrameId>,
    pub kind: Option<InterpreterFrameKind>,
    pub code_block: Option<CodeBlockId>,
    pub bytecode_index: Option<BytecodeIndex>,
    pub callee: Option<ObjectId>,
    pub lexical_scope: Option<ScopeId>,
}

#[derive(Clone, Copy, Debug, Eq, PartialEq)]
pub enum StackVisitPurpose {
    ExceptionUnwind,
    DebuggerInspection,
    ProfilerSample,
    ShadowChickenTrace,
    GcConservativeScan,
}

#[derive(Clone, Debug, Default, Eq, PartialEq)]
pub struct StackVisitorPlan {
    pub stack: Option<InterpreterStackId>,
    pub purpose: Option<StackVisitPurpose>,
    pub include_native_frames: bool,
    pub include_inlined_frames: bool,
    pub materialize_arguments: bool,
}

#[derive(Clone, Debug, Default, Eq, PartialEq)]
pub struct CachedCallPlan {
    pub callee: Option<ObjectId>,
    pub code_block: Option<CodeBlockId>,
    pub argument_count: u32,
    pub reusable_frame: Option<CallFrameId>,
    pub must_restore_entry_state: bool,
}

#[derive(Clone, Debug, Default, Eq, PartialEq)]
pub struct MicrotaskCallRecord {
    pub entry_reason: VmEntryReason,
    pub callback: Option<ObjectId>,
    pub incumbent_global: Option<ObjectId>,
    pub captured_stack_frame: Option<StackFrameId>,
}

#[derive(Clone, Copy, Debug, Eq, PartialEq)]
pub enum ShadowChickenEventKind {
    FramePushed,
    FramePopped,
    TailDeleted,
    Throw,
    AsyncResume,
}

#[derive(Clone, Debug, Eq, PartialEq)]
pub struct ShadowChickenEvent {
    pub kind: ShadowChickenEventKind,
    pub frame: Option<CallFrameId>,
    pub bytecode_index: Option<BytecodeIndex>,
    pub value: RuntimeValue,
}
