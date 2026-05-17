use crate::bytecode::code_block::{
    BitVectorRef, BytecodeIndex, CallSiteIndex, DebugHookKind, HandlerKind, RuntimeSlot,
    SourceRange,
};
use crate::bytecode::origin::CodeOrigin;
use crate::bytecode::profiling::ValueProfileBucket;

/// Exception-table view shared by unlinked bytecode and linked native ranges.
#[derive(Clone, Debug, Default, Eq, PartialEq)]
pub struct ExceptionHandlerTable {
    pub handlers: Vec<ExceptionHandlerRecord>,
    pub catch_profiles: Vec<CatchProfileRecord>,
}

#[derive(Clone, Copy, Debug, Eq, PartialEq, Hash)]
pub struct ExceptionHandlerRecord {
    pub protected_range: ExceptionRange,
    pub target: ExceptionTarget,
    pub kind: HandlerKind,
    pub order: HandlerSearchOrder,
}

#[derive(Clone, Copy, Debug, Eq, PartialEq, Hash)]
pub enum ExceptionRange {
    Bytecode {
        start: BytecodeIndex,
        end: BytecodeIndex,
    },
    CallSite {
        start: CallSiteIndex,
        end: CallSiteIndex,
    },
    NativePc {
        start_offset: u32,
        end_offset: u32,
    },
}

#[derive(Clone, Copy, Debug, Eq, PartialEq, Hash)]
pub enum ExceptionTarget {
    Bytecode(BytecodeIndex),
    NativeHandler(RuntimeSlot),
    ThrowThunk,
    UncaughtException,
}

#[derive(Clone, Copy, Debug, Default, Eq, PartialEq, Hash)]
pub enum HandlerSearchOrder {
    #[default]
    InnermostFirst,
    GeneratedNativeOrder,
}

#[derive(Clone, Copy, Debug, Eq, PartialEq, Hash)]
pub struct CatchProfileRecord {
    pub catch_bytecode: BytecodeIndex,
    pub value_bucket: Option<ValueProfileBucket>,
    pub liveness: Option<BitVectorRef>,
}

/// Debugger and profiler hooks attached to bytecode positions.
#[derive(Clone, Debug, Default, Eq, PartialEq)]
pub struct BytecodeHookTable {
    pub debugger_hooks: Vec<DebuggerBytecodeHook>,
    pub profiler_hooks: Vec<ProfilerBytecodeHook>,
    pub shadow_chicken_hooks: Vec<ShadowChickenHook>,
}

#[derive(Clone, Copy, Debug, Eq, PartialEq, Hash)]
pub struct DebuggerBytecodeHook {
    pub origin: CodeOrigin,
    pub kind: DebugHookKind,
    pub source_range: SourceRange,
    pub pause_policy: DebuggerPausePolicy,
}

#[derive(Clone, Copy, Debug, Default, Eq, PartialEq, Hash)]
pub enum DebuggerPausePolicy {
    #[default]
    Normal,
    MustPause,
    Hidden,
}

#[derive(Clone, Copy, Debug, Eq, PartialEq, Hash)]
pub struct ProfilerBytecodeHook {
    pub origin: CodeOrigin,
    pub kind: ProfilerHookKind,
    pub counter_slot: Option<RuntimeSlot>,
}

#[derive(Clone, Copy, Debug, Eq, PartialEq, Hash)]
pub enum ProfilerHookKind {
    TypeProfile,
    ControlFlow,
    SuperSampler,
    TieringCheckpoint,
    OsrExit,
}

#[derive(Clone, Copy, Debug, Eq, PartialEq, Hash)]
pub struct ShadowChickenHook {
    pub origin: CodeOrigin,
    pub kind: ShadowChickenHookKind,
}

#[derive(Clone, Copy, Debug, Eq, PartialEq, Hash)]
pub enum ShadowChickenHookKind {
    Prologue,
    Tail,
    Return,
}
