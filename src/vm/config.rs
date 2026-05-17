//! VM creation configuration.

#[derive(Clone, Debug, Default)]
pub struct VmConfig {
    pub enable_conservative_roots: bool,
    pub enable_jit_compatibility_fields: bool,
    pub max_stack_bytes: Option<usize>,
    pub heap_policy: HeapPolicy,
    pub host_capabilities: HostCapabilities,
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
