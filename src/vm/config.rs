//! VM creation configuration.

use crate::jit::TieringPolicy;

#[derive(Clone, Debug, Default)]
pub struct VmConfig {
    pub execution_mode: VmExecutionMode,
    pub enable_conservative_roots: bool,
    pub enable_jit_compatibility_fields: bool,
    pub max_stack_bytes: Option<usize>,
    pub heap_policy: HeapPolicy,
    pub host_capabilities: HostCapabilities,
}

impl VmConfig {
    pub fn interpreter_only() -> Self {
        Self {
            execution_mode: VmExecutionMode::InterpreterOnly,
            ..Self::default()
        }
    }

    pub fn baseline_allowed() -> Self {
        Self {
            execution_mode: VmExecutionMode::BaselineAllowed,
            enable_jit_compatibility_fields: true,
            host_capabilities: HostCapabilities {
                // C++ JSC gates callable JIT code on the host backend selected for
                // the process. Rust currently has executable native-entry backends
                // for Unix x86_64 and the narrow Unix aarch64 return seed.
                can_use_jit: cfg!(all(
                    unix,
                    any(target_arch = "x86_64", target_arch = "aarch64")
                )),
                ..HostCapabilities::default()
            },
            ..Self::default()
        }
    }

    pub fn tiering_policy(&self) -> TieringPolicy {
        match self.execution_mode {
            VmExecutionMode::InterpreterOnly => TieringPolicy::InterpreterOnly,
            VmExecutionMode::BaselineAllowed => TieringPolicy::BaselineAllowed,
        }
    }
}

#[derive(Clone, Copy, Debug, Default, Eq, PartialEq)]
pub enum VmExecutionMode {
    #[default]
    InterpreterOnly,
    BaselineAllowed,
}

#[derive(Clone, Copy, Debug, Default, Eq, PartialEq)]
pub enum HeapPolicy {
    #[default]
    Default,
    DeterministicTesting,
    ConservativeOnly,
}

#[derive(Clone, Copy, Debug, Default, Eq, PartialEq)]
pub struct HostCapabilities {
    pub can_call_host_functions: bool,
    pub can_schedule_microtasks: bool,
    pub can_use_watchdog: bool,
    pub can_use_jit: bool,
}
