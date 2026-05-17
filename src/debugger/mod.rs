//! Debugger-facing contracts.
//!
//! This module is intentionally a skeleton. It names the object graph for
//! breakpoints, stepping, scope inspection, async stack traces, and debugger
//! statements before any debugger behavior is implemented.

use crate::runtime::{
    CallFrameId, CodeBlockId, ObjectId, RuntimeValue, ScopeId, SourceProviderId, StackFrameId,
};

/// Debugger source identifier.
#[derive(Clone, Copy, Debug, Eq, Hash, Ord, PartialEq, PartialOrd)]
pub struct DebuggerSourceId(pub u64);

/// Debugger breakpoint identifier.
#[derive(Clone, Copy, Debug, Eq, Hash, Ord, PartialEq, PartialOrd)]
pub struct DebuggerBreakpointId(pub u64);

/// Position used by debugger and inspector breakpoints.
#[derive(Clone, Copy, Debug, Eq, Hash, PartialEq)]
pub struct DebuggerPosition {
    pub line: u32,
    pub column: u32,
}

/// Breakpoint action category.
#[derive(Clone, Copy, Debug, Eq, PartialEq)]
pub enum BreakpointActionKind {
    Log,
    Evaluate,
    Sound,
    Probe,
}

/// Breakpoint action metadata without expression execution.
#[derive(Clone, Copy, Debug, Eq, PartialEq)]
pub struct BreakpointAction {
    pub kind: BreakpointActionKind,
    pub action_id: u64,
    pub emulate_user_gesture: bool,
}

/// Breakpoint resolution state.
#[derive(Clone, Copy, Debug, Eq, PartialEq)]
pub enum BreakpointResolutionState {
    Unlinked,
    Linked,
    Resolved,
    Disabled,
    Removed,
}

/// Source breakpoint descriptor.
#[derive(Clone, Debug, Eq, PartialEq)]
pub struct BreakpointDescriptor {
    pub id: DebuggerBreakpointId,
    pub source: Option<DebuggerSourceId>,
    pub requested_position: DebuggerPosition,
    pub resolved_position: Option<DebuggerPosition>,
    pub condition_source: Option<SourceProviderId>,
    pub actions: Vec<BreakpointAction>,
    pub auto_continue: bool,
    pub ignore_count: usize,
    pub hit_count: usize,
    pub state: BreakpointResolutionState,
}

/// Pause reason visible to debugger clients.
#[derive(Clone, Copy, Debug, Eq, PartialEq)]
pub enum DebuggerPauseReason {
    Breakpoint(DebuggerBreakpointId),
    DebuggerStatement,
    Exception,
    Assertion,
    Microtask,
    ExplicitPause,
    Step,
    WasmTrap,
}

/// Stepping mode requested by a debugger client.
#[derive(Clone, Copy, Debug, Eq, PartialEq)]
pub enum StepMode {
    None,
    Next,
    Over,
    Into,
    Out,
    ContinueToLocation(DebuggerSourceId, DebuggerPosition),
    ContinueUntilNextRunLoop,
}

/// Debugger pause state for one VM.
#[derive(Clone, Copy, Debug, Eq, PartialEq)]
pub struct DebuggerPauseState {
    pub reason: Option<DebuggerPauseReason>,
    pub step_mode: StepMode,
    pub breakpoints_active: bool,
    pub suppress_all_pauses: bool,
    pub pause_on_all_exceptions: bool,
    pub pause_on_uncaught_exceptions: bool,
}

/// Debugger-visible call-frame kind.
#[derive(Clone, Copy, Debug, Eq, PartialEq)]
pub enum DebuggerCallFrameKind {
    Program,
    Function,
    Eval,
    Module,
    Native,
    Wasm,
}

/// Borrowed debugger call-frame descriptor.
#[derive(Clone, Copy, Debug, Eq, PartialEq)]
pub struct DebuggerCallFrameDescriptor {
    pub frame: Option<CallFrameId>,
    pub stack_frame: Option<StackFrameId>,
    pub caller: Option<StackFrameId>,
    pub kind: DebuggerCallFrameKind,
    pub source: Option<DebuggerSourceId>,
    pub position: DebuggerPosition,
    pub lexical_scope: Option<ScopeId>,
    pub this_object: Option<ObjectId>,
    pub is_tail_deleted: bool,
}

/// Scope family exposed during debugger inspection.
#[derive(Clone, Copy, Debug, Eq, PartialEq)]
pub enum DebuggerScopeKind {
    Global,
    Local,
    Closure,
    Catch,
    With,
    Module,
    PrivateName,
    WasmLocals,
}

/// Scope object snapshot visible to injected-script and inspector code.
#[derive(Clone, Copy, Debug, Eq, PartialEq)]
pub struct DebuggerScopeDescriptor {
    pub scope: Option<ScopeId>,
    pub object: Option<ObjectId>,
    pub kind: DebuggerScopeKind,
    pub depth: u32,
    pub can_evaluate: bool,
}

/// Evaluation request scoped to a paused call frame.
#[derive(Clone, Debug, Eq, PartialEq)]
pub struct DebuggerEvaluationRequest {
    pub frame: DebuggerCallFrameDescriptor,
    pub expression_source: SourceProviderId,
    pub scope_extension: Option<ObjectId>,
    pub emulate_user_gesture: bool,
}

/// Evaluation outcome placeholder.
#[derive(Clone, Copy, Debug, Eq, PartialEq)]
pub enum DebuggerEvaluationOutcome {
    Value(RuntimeValue),
    Threw(RuntimeValue),
    Terminated,
    NotEvaluated,
}

/// Debugger script metadata used by parse notifications.
#[derive(Clone, Copy, Debug, Eq, PartialEq)]
pub struct DebuggerScript {
    pub source: DebuggerSourceId,
    pub provider: SourceProviderId,
    pub start_position: DebuggerPosition,
    pub end_position: DebuggerPosition,
    pub is_module: bool,
    pub is_internal: bool,
}

/// Request to apply debugger state to compiled code.
#[derive(Clone, Copy, Debug, Eq, PartialEq)]
pub struct DebuggerCodeInstrumentation {
    pub code_block: CodeBlockId,
    pub source: Option<DebuggerSourceId>,
    pub breakpoints_active: bool,
    pub needs_debugger_statement_hooks: bool,
}
