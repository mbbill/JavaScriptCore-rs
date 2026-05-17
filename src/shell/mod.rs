//! JavaScriptCore shell embedding contracts.
//!
//! The shell is not the engine. This module only names command-line host
//! services, testing hooks, module resolution hooks, and harness integration.

use crate::modules::{HostModulePayload, ImportMapId, ModuleKey, ModuleLoaderPolicy};
use crate::runtime::{GlobalObjectId, HostHookId, SourceProviderId};
use crate::wasm::WasmDebugTransport;

/// Shell execution mode.
#[derive(Clone, Copy, Debug, Eq, PartialEq)]
pub enum ShellMode {
    ScriptFile,
    Interactive,
    Worker,
    TestHarness,
    Module,
}

/// Parsed shell option surface that affects host integration.
#[derive(Clone, Debug, Eq, PartialEq)]
pub struct ShellOptions {
    pub mode: ShellMode,
    pub dump_sampling_profiler_data: bool,
    pub enable_type_profiler: bool,
    pub enable_control_flow_profiler: bool,
    pub enable_wasm_debugger: bool,
    pub module_loader_policy: Option<ModuleLoaderPolicy>,
    pub import_map: Option<ImportMapId>,
}

/// Source fed to the shell by files, stdin, or harness callbacks.
#[derive(Clone, Copy, Debug, Eq, PartialEq)]
pub struct ShellSource {
    pub provider: SourceProviderId,
    pub is_module: bool,
    pub is_strict_mode: bool,
}

/// Host hook installed by the shell global object.
#[derive(Clone, Copy, Debug, Eq, PartialEq)]
pub enum ShellHostHookKind {
    Print,
    ReadFile,
    LoadScript,
    Quit,
    SetTimeout,
    DrainMicrotasks,
    StartSamplingProfiler,
    SamplingProfilerStackTraces,
    ModuleResolve,
    ModuleFetch,
    WasmDebugServer,
}

/// Shell host hook descriptor.
#[derive(Clone, Copy, Debug, Eq, PartialEq)]
pub struct ShellHostHook {
    pub kind: ShellHostHookKind,
    pub hook: HostHookId,
    pub can_reenter_vm: bool,
}

/// Test harness lifecycle hooks.
#[derive(Clone, Copy, Debug, Eq, PartialEq)]
pub enum ShellHarnessHook {
    BeforeRun,
    RunTest,
    PostTest,
    ShutdownTestRun,
    TimeoutCheck,
}

/// Shell module host operation.
#[derive(Clone, Debug, Eq, PartialEq)]
pub struct ShellModuleHostOperation {
    pub key: ModuleKey,
    pub payload: Option<HostModulePayload>,
    pub policy: ModuleLoaderPolicy,
}

/// Wasm debugger configuration owned by the shell host.
#[derive(Clone, Copy, Debug, Eq, PartialEq)]
pub struct ShellWasmDebugConfig {
    pub enabled: bool,
    pub transport: WasmDebugTransport,
}

/// A shell run context around `runJSC`.
#[derive(Clone, Debug, Eq, PartialEq)]
pub struct ShellRunContext {
    pub global_object: Option<GlobalObjectId>,
    pub options: ShellOptions,
    pub source: Option<ShellSource>,
    pub hooks_installed: usize,
    pub harness_hook: Option<ShellHarnessHook>,
    pub wasm_debug: ShellWasmDebugConfig,
}
