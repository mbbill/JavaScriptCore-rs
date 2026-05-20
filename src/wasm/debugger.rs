//! WebAssembly debugger contracts.
//!
//! This module mirrors the process-wide debug server, module manager,
//! breakpoints, virtual addresses, and debug-info lookup boundaries used by
//! JSC's Wasm debugger. It does not implement a GDB packet parser, socket
//! transport, memory reads, register reads, or execution control.

use crate::bytecode::SourceProviderId;
use crate::wasm::{WasmFunctionIndex, WasmInstanceId, WasmModuleId};

/// Virtual address in the Wasm debugger address space.
#[derive(Clone, Copy, Debug, Eq, Hash, Ord, PartialEq, PartialOrd)]
pub struct WasmVirtualAddress(pub u64);

/// Debug server lifecycle for direct socket or remote-inspector mode.
#[derive(Clone, Copy, Debug, Eq, PartialEq)]
pub enum WasmDebugServerState {
    NotStarted,
    Listening,
    ClientAttached,
    StartupExchangeComplete,
    Running,
    Stopped,
    Resetting,
}

/// Transport selected by the embedding shell or remote inspector.
#[derive(Clone, Copy, Debug, Eq, PartialEq)]
pub enum WasmDebugTransport {
    DirectTcp { port: u16 },
    RemoteInspector,
    HostProvided,
}

/// Process-wide Wasm debug target descriptor.
#[derive(Clone, Copy, Debug, Eq, PartialEq)]
pub struct WasmDebugServerDescriptor {
    pub state: WasmDebugServerState,
    pub transport: WasmDebugTransport,
    pub has_debugger: bool,
    pub has_continued: bool,
    pub is_debugger_ready: bool,
}

/// Breakpoint category in the Wasm debugger.
#[derive(Clone, Copy, Debug, Eq, PartialEq)]
pub enum WasmDebugBreakpointKind {
    Persistent,
    OneTime,
    Trap,
}

/// Wasm breakpoint keyed by virtual address.
#[derive(Clone, Copy, Debug, Eq, PartialEq)]
pub struct WasmDebugBreakpoint {
    pub address: WasmVirtualAddress,
    pub kind: WasmDebugBreakpointKind,
    pub enabled: bool,
}

/// Function-local debug-info lookup state.
#[derive(Clone, Copy, Debug, Eq, PartialEq)]
pub enum WasmDebugInfoState {
    NotCollected,
    Collecting,
    Available,
    Missing,
}

/// Mapping from Wasm bytecode offsets to debugger-visible locations.
#[derive(Clone, Copy, Debug, Eq, PartialEq)]
pub struct WasmDebugLocation {
    pub module: WasmModuleId,
    pub function: WasmFunctionIndex,
    pub bytecode_offset: u32,
    pub virtual_address: WasmVirtualAddress,
}

/// Module debug information owned outside JS execution.
#[derive(Clone, Debug, Eq, PartialEq)]
pub struct WasmModuleDebugInfo {
    pub module: WasmModuleId,
    pub source: Option<SourceProviderId>,
    pub state: WasmDebugInfoState,
    pub location_count: u32,
    pub local_count: u32,
}

/// Registration edge from a live JS instance to the process debugger.
#[derive(Clone, Copy, Debug, Eq, PartialEq)]
pub struct WasmDebuggerInstanceRegistration {
    pub module: WasmModuleId,
    pub instance: WasmInstanceId,
    pub anchor_live: bool,
}
