//! Inspector protocol contracts.
//!
//! The inspector is a host/debugging boundary. This module records agents,
//! sessions, frontend channels, runtime domains, and instrumentation hooks
//! without implementing protocol transport.

use crate::debugger::{DebuggerBreakpointId, DebuggerCallFrameDescriptor, DebuggerSourceId};
use crate::runtime::{CodeBlockId, HostHookId, ObjectId, RuntimeValue, SourceProviderId};
use crate::wasm::{WasmDebugServerState, WasmModuleId};

/// Inspector protocol domain.
#[derive(Clone, Copy, Debug, Eq, Hash, PartialEq)]
pub enum InspectorDomain {
    Runtime,
    Debugger,
    Console,
    Heap,
    ScriptProfiler,
    Target,
    Audit,
    WasmDebugger,
}

/// Agent lifecycle state.
#[derive(Clone, Copy, Debug, Eq, PartialEq)]
pub enum InspectorAgentState {
    Created,
    FrontendAttached,
    Enabled,
    Suspended,
    Disabled,
    Destroyed,
}

/// Common agent descriptor.
#[derive(Clone, Copy, Debug, Eq, PartialEq)]
pub struct InspectorAgentDescriptor {
    pub domain: InspectorDomain,
    pub state: InspectorAgentState,
    pub backend_hook: Option<HostHookId>,
}

/// Protocol message identity without JSON parsing.
#[derive(Clone, Copy, Debug, Eq, Hash, PartialEq)]
pub struct InspectorRequestId(pub i64);

/// Backend dispatcher route for one request.
#[derive(Clone, Debug, Eq, PartialEq)]
pub struct InspectorProtocolCommand {
    pub request_id: InspectorRequestId,
    pub domain: InspectorDomain,
    pub method_ordinal: u32,
    pub requires_enabled_agent: bool,
}

/// Frontend event descriptor.
#[derive(Clone, Copy, Debug, Eq, PartialEq)]
pub struct InspectorFrontendEvent {
    pub domain: InspectorDomain,
    pub event_ordinal: u32,
    pub session: InspectorSessionId,
}

/// Inspector session identity.
#[derive(Clone, Copy, Debug, Eq, Hash, PartialEq)]
pub struct InspectorSessionId(pub u64);

/// Frontend channel state without transport.
#[derive(Clone, Copy, Debug, Eq, PartialEq)]
pub enum InspectorFrontendChannelState {
    Detached,
    Attaching,
    Attached,
    Detaching,
}

/// Inspector session boundary.
#[derive(Clone, Copy, Debug, Eq, PartialEq)]
pub struct InspectorSession {
    pub id: InspectorSessionId,
    pub channel_state: InspectorFrontendChannelState,
    pub has_backend_dispatcher: bool,
    pub has_frontend_router: bool,
}

/// Instrumentation event observed by inspector agents.
#[derive(Clone, Copy, Debug, Eq, PartialEq)]
pub enum InspectorInstrumentationKind {
    DidParseScript(DebuggerSourceId),
    FailedToParseScript,
    DidCreateNativeExecutable,
    WillEnterCallFrame,
    DidPause,
    DidContinue,
    DidQueueMicrotask,
    WillRunMicrotask,
    DidRunMicrotask,
    ConsoleMessage,
    HeapSnapshotRequested,
    WasmModuleRegistered(WasmModuleId),
}

/// Instrumentation dispatch record.
#[derive(Clone, Copy, Debug, Eq, PartialEq)]
pub struct InspectorInstrumentationEvent {
    pub kind: InspectorInstrumentationKind,
    pub source: Option<SourceProviderId>,
    pub code_block: Option<CodeBlockId>,
}

/// Debugger-agent breakpoint mapping.
#[derive(Clone, Copy, Debug, Eq, PartialEq)]
pub struct InspectorBreakpointBinding {
    pub protocol_breakpoint_id: u64,
    pub debugger_breakpoint: DebuggerBreakpointId,
    pub source: Option<DebuggerSourceId>,
    pub resolved: bool,
}

/// Inspector-visible call frame.
#[derive(Clone, Copy, Debug, Eq, PartialEq)]
pub struct InspectorCallFrame {
    pub call_frame_id: u64,
    pub debugger_frame: DebuggerCallFrameDescriptor,
    pub scope_chain_length: u32,
}

/// Remote object descriptor returned by runtime/debugger domains.
#[derive(Clone, Copy, Debug, Eq, PartialEq)]
pub struct InspectorRemoteObject {
    pub object: Option<ObjectId>,
    pub value: Option<RuntimeValue>,
    pub object_group: u32,
    pub return_by_value: bool,
}

/// Sampling-profiler lifecycle.
#[derive(Clone, Copy, Debug, Eq, PartialEq)]
pub enum SamplingProfilerState {
    Unsupported,
    Available,
    Running,
    Paused,
    Stopped,
}

/// Sampling-profiler frame family.
#[derive(Clone, Copy, Debug, Eq, PartialEq)]
pub enum SamplingProfilerFrameKind {
    Executable,
    Wasm,
    Host,
    RegExp,
    Native,
    Unknown,
}

/// Sampling-profiler sample descriptor.
#[derive(Clone, Copy, Debug, Eq, PartialEq)]
pub struct SamplingProfilerSample {
    pub frame_kind: SamplingProfilerFrameKind,
    pub source: Option<SourceProviderId>,
    pub code_block: Option<CodeBlockId>,
    pub bytecode_offset: Option<u32>,
    pub timestamp_ticks: u64,
}

/// Type-profiler query key.
#[derive(Clone, Copy, Debug, Eq, Hash, PartialEq)]
pub struct TypeProfilerQuery {
    pub source: DebuggerSourceId,
    pub divot: u32,
    pub function_return: bool,
}

/// Type-profiler location metadata.
#[derive(Clone, Copy, Debug, Eq, PartialEq)]
pub struct TypeProfilerLocation {
    pub query: TypeProfilerQuery,
    pub observed_type_set: u32,
    pub variable_id: u64,
}

/// Control-flow basic-block profiling range.
#[derive(Clone, Copy, Debug, Eq, PartialEq)]
pub struct ControlFlowBlockRange {
    pub source: DebuggerSourceId,
    pub start_offset: i32,
    pub end_offset: i32,
    pub has_executed: bool,
    pub execution_count: usize,
}

/// Script profiler agent state shared by sampling, type, and control-flow profilers.
#[derive(Clone, Debug, Eq, PartialEq)]
pub struct InspectorScriptProfilerState {
    pub sampling_state: SamplingProfilerState,
    pub type_profiler_enabled: bool,
    pub control_flow_profiler_enabled: bool,
    pub sample_count: usize,
    pub type_location_count: usize,
    pub basic_block_count: usize,
}

/// Wasm debugger target exposed through inspector discovery.
#[derive(Clone, Copy, Debug, Eq, PartialEq)]
pub struct InspectorWasmDebuggerTarget {
    pub module: Option<WasmModuleId>,
    pub server_state: WasmDebugServerState,
    pub has_local_debugger: bool,
}
