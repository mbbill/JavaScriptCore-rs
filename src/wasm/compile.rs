//! Deferred Wasm validation and compilation plans.
//!
//! Future implementations may coordinate with JIT worklists and code liveness.
//! Decoding, code generation, and execution live elsewhere. This module only
//! describes request, state, validation, and tier-planning boundaries.

use crate::bytecode::SourceProviderId;
use crate::jit::{CompilationProduct, JitType};
use crate::wasm::{
    WasmCalleeGroupId, WasmFunctionCodeIndex, WasmFunctionIndex, WasmMemoryStyle, WasmModuleId,
};

/// Stable identity for a future Wasm compilation job.
#[derive(Clone, Copy, Debug, Eq, Hash, Ord, PartialEq, PartialOrd)]
pub struct WasmCompilationPlanId(pub u64);

/// Lifecycle state for deferred Wasm compilation.
#[derive(Clone, Copy, Debug, Eq, PartialEq)]
pub enum WasmCompilationState {
    NotStarted,
    Parsing,
    Validating,
    WaitingForWork,
    Preparing,
    Compiling,
    Finalizing,
    CompletionTasksRunning,
    ReadyToInstantiate,
    Installed,
    Cancelled,
    Failed,
}

/// Validation entry path before compilation or instantiation.
#[derive(Clone, Copy, Debug, Eq, PartialEq)]
pub enum WasmValidationMode {
    SynchronousApi,
    AsynchronousApi,
    Streaming,
    ModuleLoader,
}

/// Validation request without decoding or executing bytes.
///
/// Source provider identity is owned by bytecode/source storage. Wasm
/// validation borrows it only to describe provenance for the module.
#[derive(Clone, Debug, Eq, PartialEq)]
pub struct WasmValidationRequest {
    pub module: WasmModuleId,
    pub source: Option<SourceProviderId>,
    pub mode: WasmValidationMode,
    pub features_required: u32,
}

/// Validation result metadata handed to compile and JS wrapper code.
#[derive(Clone, Copy, Debug, Eq, PartialEq)]
pub struct WasmValidationProduct {
    pub module: WasmModuleId,
    pub state: crate::wasm::WasmValidationState,
    pub function_count: u32,
    pub import_count: u32,
    pub export_count: u32,
}

/// Wasm compiler path, matching the future interpreter/JIT tiers.
#[derive(Clone, Copy, Debug, Eq, PartialEq)]
pub enum WasmCompilerMode {
    ValidateOnly,
    IpInt,
    Bbq,
    Omg,
    OsrEntry,
    JsToWasmIc,
    BridgeStub,
    WasmBuiltin,
    RestoreFrame,
}

/// Static tier family published to Wasm scheduling code.
#[derive(Clone, Copy, Debug, Eq, PartialEq)]
pub enum WasmCompilationTier {
    Validation,
    Interpreter,
    BaselineJit,
    OptimizingJit,
    Osr,
    Bridge,
    Builtin,
}

/// Immutable descriptor for a Wasm compilation tier.
///
/// Tier descriptors are feature/configuration metadata only. Work queues and
/// compilers may read this table, but tier-up policy owns scheduling mutation.
#[derive(Clone, Copy, Debug, Eq, PartialEq)]
pub struct WasmCompilationTierDescriptor {
    pub mode: WasmCompilerMode,
    pub tier: WasmCompilationTier,
    pub name: &'static str,
    pub can_install_code: bool,
    pub can_tier_up: bool,
    pub authority: crate::wasm::WasmRegistryAuthority,
}

const WASM_COMPILATION_TIERS: &[WasmCompilationTierDescriptor] = &[
    WasmCompilationTierDescriptor {
        mode: WasmCompilerMode::ValidateOnly,
        tier: WasmCompilationTier::Validation,
        name: "validate-only",
        can_install_code: false,
        can_tier_up: false,
        authority: crate::wasm::WasmRegistryAuthority::StaticSpecTable,
    },
    WasmCompilationTierDescriptor {
        mode: WasmCompilerMode::IpInt,
        tier: WasmCompilationTier::Interpreter,
        name: "ipint",
        can_install_code: false,
        can_tier_up: true,
        authority: crate::wasm::WasmRegistryAuthority::FeatureConfiguration,
    },
    WasmCompilationTierDescriptor {
        mode: WasmCompilerMode::Bbq,
        tier: WasmCompilationTier::BaselineJit,
        name: "bbq",
        can_install_code: true,
        can_tier_up: true,
        authority: crate::wasm::WasmRegistryAuthority::FeatureConfiguration,
    },
    WasmCompilationTierDescriptor {
        mode: WasmCompilerMode::Omg,
        tier: WasmCompilationTier::OptimizingJit,
        name: "omg",
        can_install_code: true,
        can_tier_up: false,
        authority: crate::wasm::WasmRegistryAuthority::FeatureConfiguration,
    },
    WasmCompilationTierDescriptor {
        mode: WasmCompilerMode::OsrEntry,
        tier: WasmCompilationTier::Osr,
        name: "osr-entry",
        can_install_code: true,
        can_tier_up: false,
        authority: crate::wasm::WasmRegistryAuthority::FeatureConfiguration,
    },
    WasmCompilationTierDescriptor {
        mode: WasmCompilerMode::JsToWasmIc,
        tier: WasmCompilationTier::Bridge,
        name: "js-to-wasm-ic",
        can_install_code: true,
        can_tier_up: false,
        authority: crate::wasm::WasmRegistryAuthority::FeatureConfiguration,
    },
    WasmCompilationTierDescriptor {
        mode: WasmCompilerMode::BridgeStub,
        tier: WasmCompilationTier::Bridge,
        name: "bridge-stub",
        can_install_code: true,
        can_tier_up: false,
        authority: crate::wasm::WasmRegistryAuthority::FeatureConfiguration,
    },
    WasmCompilationTierDescriptor {
        mode: WasmCompilerMode::WasmBuiltin,
        tier: WasmCompilationTier::Builtin,
        name: "wasm-builtin",
        can_install_code: true,
        can_tier_up: false,
        authority: crate::wasm::WasmRegistryAuthority::FeatureConfiguration,
    },
    WasmCompilationTierDescriptor {
        mode: WasmCompilerMode::RestoreFrame,
        tier: WasmCompilationTier::Bridge,
        name: "restore-frame",
        can_install_code: true,
        can_tier_up: false,
        authority: crate::wasm::WasmRegistryAuthority::FeatureConfiguration,
    },
];

pub const fn wasm_compilation_tier_descriptors() -> &'static [WasmCompilationTierDescriptor] {
    WASM_COMPILATION_TIERS
}

pub fn wasm_compilation_tier_descriptor(
    mode: WasmCompilerMode,
) -> Option<&'static WasmCompilationTierDescriptor> {
    WASM_COMPILATION_TIERS
        .iter()
        .find(|descriptor| descriptor.mode == mode)
}

/// Structural error reported by Wasm compilation-plan builders and validators.
#[derive(Clone, Debug, Eq, PartialEq)]
pub enum WasmCompilationValidationError {
    EmptyTierName(WasmCompilerMode),
    DuplicateTierMode(WasmCompilerMode),
    InstallingValidationTier(WasmCompilerMode),
    UnknownCompilerMode(WasmCompilerMode),
    FunctionRequestMissingFunction,
    FunctionRequestMissingCodeIndex,
    ModuleRequestHasFunction,
    FeatureMaskIsEmpty,
    NoAvailableTier(WasmCompilerMode),
}

#[derive(Clone, Copy, Debug, Eq, PartialEq)]
pub struct WasmTierPlanningConfig {
    pub enable_interpreter: bool,
    pub enable_baseline_jit: bool,
    pub enable_optimizing_jit: bool,
    pub enable_bridge_tiers: bool,
}

#[derive(Clone, Copy, Debug, Eq, PartialEq)]
pub struct WasmTierPlan {
    pub requested: WasmCompilerMode,
    pub selected: WasmCompilerMode,
    pub tier: WasmCompilationTier,
    pub tier_up: Option<WasmCompilerMode>,
    pub can_install_code: bool,
}

#[derive(Clone, Copy, Debug, Eq, PartialEq)]
pub enum WasmCompilationFallbackTarget {
    SelectedTier,
    Interpreter,
    ValidationOnly,
    RejectCompilation,
}

#[derive(Clone, Copy, Debug, Eq, PartialEq)]
pub enum WasmCompilationDiagnosticKind {
    ValidationRejected,
    RequestedTierUnavailable,
    FellBackToLowerTier,
    BridgeTierDisabled,
}

#[derive(Clone, Debug, Eq, PartialEq)]
pub struct WasmCompilationDiagnosticRecord {
    pub plan: WasmCompilationPlanId,
    pub module: WasmModuleId,
    pub requested: WasmCompilerMode,
    pub selected: Option<WasmCompilerMode>,
    pub kind: WasmCompilationDiagnosticKind,
    pub validation_error: Option<WasmCompilationValidationError>,
}

#[derive(Clone, Debug, Eq, PartialEq)]
pub struct WasmCompilationFallbackRecord {
    pub plan: WasmCompilationPlanId,
    pub module: WasmModuleId,
    pub requested: WasmCompilerMode,
    pub selected: Option<WasmCompilerMode>,
    pub target: WasmCompilationFallbackTarget,
    pub diagnostics: Vec<WasmCompilationDiagnosticRecord>,
}

pub fn describe_wasm_compilation_fallback(
    request: &WasmCompilationRequest,
    config: WasmTierPlanningConfig,
) -> WasmCompilationFallbackRecord {
    match plan_wasm_compilation_tiers(request, config) {
        Ok(plan) => {
            let mut diagnostics = Vec::new();
            if plan.selected != request.mode {
                diagnostics.push(WasmCompilationDiagnosticRecord {
                    plan: request.id,
                    module: request.module,
                    requested: request.mode,
                    selected: Some(plan.selected),
                    kind: WasmCompilationDiagnosticKind::FellBackToLowerTier,
                    validation_error: None,
                });
            }
            WasmCompilationFallbackRecord {
                plan: request.id,
                module: request.module,
                requested: request.mode,
                selected: Some(plan.selected),
                target: if plan.selected == WasmCompilerMode::ValidateOnly {
                    WasmCompilationFallbackTarget::ValidationOnly
                } else if plan.selected == WasmCompilerMode::IpInt {
                    WasmCompilationFallbackTarget::Interpreter
                } else {
                    WasmCompilationFallbackTarget::SelectedTier
                },
                diagnostics,
            }
        }
        Err(error) => {
            let kind = match error {
                WasmCompilationValidationError::NoAvailableTier(
                    WasmCompilerMode::JsToWasmIc
                    | WasmCompilerMode::BridgeStub
                    | WasmCompilerMode::WasmBuiltin
                    | WasmCompilerMode::RestoreFrame,
                ) => WasmCompilationDiagnosticKind::BridgeTierDisabled,
                WasmCompilationValidationError::NoAvailableTier(_) => {
                    WasmCompilationDiagnosticKind::RequestedTierUnavailable
                }
                _ => WasmCompilationDiagnosticKind::ValidationRejected,
            };
            WasmCompilationFallbackRecord {
                plan: request.id,
                module: request.module,
                requested: request.mode,
                selected: None,
                target: WasmCompilationFallbackTarget::RejectCompilation,
                diagnostics: vec![WasmCompilationDiagnosticRecord {
                    plan: request.id,
                    module: request.module,
                    requested: request.mode,
                    selected: None,
                    kind,
                    validation_error: Some(error),
                }],
            }
        }
    }
}

pub fn validate_wasm_compilation_tier_descriptors(
    descriptors: &[WasmCompilationTierDescriptor],
) -> Result<(), WasmCompilationValidationError> {
    for (index, descriptor) in descriptors.iter().enumerate() {
        if descriptor.name.is_empty() {
            return Err(WasmCompilationValidationError::EmptyTierName(
                descriptor.mode,
            ));
        }
        if descriptor.tier == WasmCompilationTier::Validation && descriptor.can_install_code {
            return Err(WasmCompilationValidationError::InstallingValidationTier(
                descriptor.mode,
            ));
        }
        for other in descriptors.iter().skip(index + 1) {
            if descriptor.mode == other.mode {
                return Err(WasmCompilationValidationError::DuplicateTierMode(
                    descriptor.mode,
                ));
            }
        }
    }
    Ok(())
}

pub fn validate_wasm_validation_request(
    request: &WasmValidationRequest,
) -> Result<(), WasmCompilationValidationError> {
    if request.features_required == 0 {
        return Err(WasmCompilationValidationError::FeatureMaskIsEmpty);
    }
    Ok(())
}

pub fn validate_wasm_compilation_request(
    request: &WasmCompilationRequest,
) -> Result<(), WasmCompilationValidationError> {
    if wasm_compilation_tier_descriptor(request.mode).is_none() {
        return Err(WasmCompilationValidationError::UnknownCompilerMode(
            request.mode,
        ));
    }

    match request.kind {
        WasmCompilationKind::Function | WasmCompilationKind::TierUpFunction => {
            if request.function.is_none() {
                return Err(WasmCompilationValidationError::FunctionRequestMissingFunction);
            }
            if request.code_index.is_none() {
                return Err(WasmCompilationValidationError::FunctionRequestMissingCodeIndex);
            }
        }
        WasmCompilationKind::ModuleEntry | WasmCompilationKind::StreamingModule => {
            if request.function.is_some() || request.code_index.is_some() {
                return Err(WasmCompilationValidationError::ModuleRequestHasFunction);
            }
        }
        WasmCompilationKind::JsToWasmBridge | WasmCompilationKind::WasmToJsBridge => {
            if request.function.is_none() {
                return Err(WasmCompilationValidationError::FunctionRequestMissingFunction);
            }
        }
    }

    Ok(())
}

pub fn plan_wasm_compilation_tiers(
    request: &WasmCompilationRequest,
    config: WasmTierPlanningConfig,
) -> Result<WasmTierPlan, WasmCompilationValidationError> {
    validate_wasm_compilation_request(request)?;

    let selected = match request.mode {
        WasmCompilerMode::ValidateOnly => WasmCompilerMode::ValidateOnly,
        WasmCompilerMode::IpInt if config.enable_interpreter => WasmCompilerMode::IpInt,
        WasmCompilerMode::IpInt if config.enable_baseline_jit => WasmCompilerMode::Bbq,
        WasmCompilerMode::Bbq if config.enable_baseline_jit => WasmCompilerMode::Bbq,
        WasmCompilerMode::Bbq if config.enable_interpreter => WasmCompilerMode::IpInt,
        WasmCompilerMode::Omg if config.enable_optimizing_jit => WasmCompilerMode::Omg,
        WasmCompilerMode::Omg if config.enable_baseline_jit => WasmCompilerMode::Bbq,
        WasmCompilerMode::Omg if config.enable_interpreter => WasmCompilerMode::IpInt,
        WasmCompilerMode::OsrEntry if config.enable_optimizing_jit => WasmCompilerMode::OsrEntry,
        WasmCompilerMode::JsToWasmIc
        | WasmCompilerMode::BridgeStub
        | WasmCompilerMode::WasmBuiltin
        | WasmCompilerMode::RestoreFrame
            if config.enable_bridge_tiers =>
        {
            request.mode
        }
        _ => {
            return Err(WasmCompilationValidationError::NoAvailableTier(
                request.mode,
            ));
        }
    };

    let descriptor = wasm_compilation_tier_descriptor(selected).ok_or(
        WasmCompilationValidationError::UnknownCompilerMode(selected),
    )?;
    Ok(WasmTierPlan {
        requested: request.mode,
        selected,
        tier: descriptor.tier,
        tier_up: planned_tier_up(selected, config),
        can_install_code: descriptor.can_install_code,
    })
}

fn planned_tier_up(
    selected: WasmCompilerMode,
    config: WasmTierPlanningConfig,
) -> Option<WasmCompilerMode> {
    match selected {
        WasmCompilerMode::IpInt if config.enable_baseline_jit => Some(WasmCompilerMode::Bbq),
        WasmCompilerMode::Bbq if config.enable_optimizing_jit => Some(WasmCompilerMode::Omg),
        _ => None,
    }
}

/// Work item kind carried by a compilation plan.
#[derive(Clone, Copy, Debug, Eq, PartialEq)]
pub enum WasmCompilationKind {
    ModuleEntry,
    StreamingModule,
    Function,
    TierUpFunction,
    JsToWasmBridge,
    WasmToJsBridge,
}

/// Why a Wasm compilation plan was cancelled.
#[derive(Clone, Copy, Debug, Eq, PartialEq)]
pub enum WasmCompilationCancellation {
    ModuleDropped,
    InstanceDropped,
    LastContextRemoved,
    FeatureDisabled,
    MemoryModeChanged,
    SupersededByHigherTier,
    RuntimeShuttingDown,
}

/// Request payload for deferred Wasm work.
#[derive(Clone, Debug, Eq, PartialEq)]
pub struct WasmCompilationRequest {
    pub id: WasmCompilationPlanId,
    pub module: WasmModuleId,
    pub source: Option<SourceProviderId>,
    pub kind: WasmCompilationKind,
    pub mode: WasmCompilerMode,
    pub function: Option<WasmFunctionIndex>,
    pub code_index: Option<WasmFunctionCodeIndex>,
    pub memory_style: WasmMemoryStyle,
    pub memory_mode: WasmRuntimeMemoryMode,
    pub priority: WasmCompilationPriority,
}

#[derive(Clone, Debug, Eq, PartialEq)]
pub struct WasmValidationRequestBuilder {
    request: WasmValidationRequest,
}

impl WasmValidationRequestBuilder {
    pub fn new(module: WasmModuleId, mode: WasmValidationMode) -> Self {
        Self {
            request: WasmValidationRequest {
                module,
                source: None,
                mode,
                features_required: 1,
            },
        }
    }

    pub fn source(mut self, source: SourceProviderId) -> Self {
        self.request.source = Some(source);
        self
    }

    pub fn features_required(mut self, features_required: u32) -> Self {
        self.request.features_required = features_required;
        self
    }

    pub fn build(self) -> Result<WasmValidationRequest, WasmCompilationValidationError> {
        validate_wasm_validation_request(&self.request)?;
        Ok(self.request)
    }
}

#[derive(Clone, Debug, Eq, PartialEq)]
pub struct WasmCompilationRequestBuilder {
    request: WasmCompilationRequest,
}

impl WasmCompilationRequestBuilder {
    pub fn new(
        id: WasmCompilationPlanId,
        module: WasmModuleId,
        kind: WasmCompilationKind,
        mode: WasmCompilerMode,
    ) -> Self {
        Self {
            request: WasmCompilationRequest {
                id,
                module,
                source: None,
                kind,
                mode,
                function: None,
                code_index: None,
                memory_style: WasmMemoryStyle::Deferred,
                memory_mode: WasmRuntimeMemoryMode::BoundsChecking,
                priority: WasmCompilationPriority::InitialInstantiation,
            },
        }
    }

    pub fn source(mut self, source: SourceProviderId) -> Self {
        self.request.source = Some(source);
        self
    }

    pub fn function(mut self, function: WasmFunctionIndex) -> Self {
        self.request.function = Some(function);
        self
    }

    pub fn code_index(mut self, code_index: WasmFunctionCodeIndex) -> Self {
        self.request.code_index = Some(code_index);
        self
    }

    pub fn memory_style(mut self, style: WasmMemoryStyle) -> Self {
        self.request.memory_style = style;
        self
    }

    pub fn memory_mode(mut self, mode: WasmRuntimeMemoryMode) -> Self {
        self.request.memory_mode = mode;
        self
    }

    pub fn priority(mut self, priority: WasmCompilationPriority) -> Self {
        self.request.priority = priority;
        self
    }

    pub fn build(self) -> Result<WasmCompilationRequest, WasmCompilationValidationError> {
        validate_wasm_compilation_request(&self.request)?;
        Ok(self.request)
    }
}

/// Runtime memory mode selected for compiled code.
#[derive(Clone, Copy, Debug, Eq, PartialEq)]
pub enum WasmRuntimeMemoryMode {
    BoundsChecking,
    Signaling,
}

/// Scheduling priority without defining a concrete worklist.
#[derive(Clone, Copy, Debug, Eq, PartialEq)]
pub enum WasmCompilationPriority {
    StreamingValidation,
    InitialInstantiation,
    HotFunction,
    BackgroundTierUp,
    BridgeRequiredForImport,
}

/// Product handed back from a future Wasm plan.
#[derive(Clone, Debug, Eq, PartialEq)]
pub struct WasmCompilationProduct {
    pub plan: WasmCompilationPlanId,
    pub state: WasmCompilationState,
    pub callee_group: Option<WasmCalleeGroupId>,
    pub jit_product: Option<CompilationProduct>,
    pub compiled_function: Option<WasmFunctionIndex>,
    pub cancellation: Option<WasmCompilationCancellation>,
}

/// Completion callback registration owned by a Plan.
#[derive(Clone, Copy, Debug, Eq, PartialEq)]
pub struct WasmCompilationCompletionTask {
    pub plan: WasmCompilationPlanId,
    pub vm_context_live: bool,
    pub finalize_module: bool,
}

/// Cross-thread error state published by a Plan.
#[derive(Clone, Debug, Eq, PartialEq)]
pub struct WasmCompilationErrorState {
    pub plan: WasmCompilationPlanId,
    pub error: WasmCompilationError,
    pub message: Option<String>,
}

/// Compilation error category tracked by tier-up counters and plans.
#[derive(Clone, Copy, Debug, Eq, PartialEq)]
pub enum WasmCompilationError {
    Default,
    OutOfMemory,
    Parse,
    Validation,
    Link,
}

/// Host-facing compilation-plan contract.
/// The plan owns its completion tasks and error state. Workers may advance work
/// state, while finalization and callback execution are main-VM responsibilities.
pub trait WasmCompilationPlan {
    fn id(&self) -> WasmCompilationPlanId;
    fn request(&self) -> &WasmCompilationRequest;
    fn module(&self) -> WasmModuleId;
    fn state(&self) -> WasmCompilationState;
    fn jit_tier_hint(&self) -> Option<JitType>;
    fn product(&self) -> Option<&WasmCompilationProduct>;
    fn cancel(&mut self);
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn wasm_tier_table_has_one_descriptor_per_mode() {
        let table = wasm_compilation_tier_descriptors();

        for (index, descriptor) in table.iter().enumerate() {
            assert_eq!(
                wasm_compilation_tier_descriptor(descriptor.mode),
                Some(descriptor)
            );

            for other in table.iter().skip(index + 1) {
                assert_ne!(descriptor.mode, other.mode);
            }
        }
    }

    #[test]
    fn wasm_tier_table_is_structurally_valid() {
        assert!(
            validate_wasm_compilation_tier_descriptors(wasm_compilation_tier_descriptors()).is_ok()
        );
    }

    #[test]
    fn wasm_compilation_request_builder_requires_function_for_function_work() {
        let error = WasmCompilationRequestBuilder::new(
            WasmCompilationPlanId(1),
            WasmModuleId(1),
            WasmCompilationKind::Function,
            WasmCompilerMode::Bbq,
        )
        .build()
        .unwrap_err();

        assert_eq!(
            error,
            WasmCompilationValidationError::FunctionRequestMissingFunction
        );
    }

    #[test]
    fn wasm_tier_planner_selects_baseline_and_tier_up() {
        let request = WasmCompilationRequestBuilder::new(
            WasmCompilationPlanId(10),
            WasmModuleId(1),
            WasmCompilationKind::Function,
            WasmCompilerMode::Bbq,
        )
        .function(WasmFunctionIndex(0))
        .code_index(WasmFunctionCodeIndex(0))
        .build()
        .unwrap();

        let plan = plan_wasm_compilation_tiers(
            &request,
            WasmTierPlanningConfig {
                enable_interpreter: true,
                enable_baseline_jit: true,
                enable_optimizing_jit: true,
                enable_bridge_tiers: true,
            },
        )
        .unwrap();

        assert_eq!(plan.selected, WasmCompilerMode::Bbq);
        assert_eq!(plan.tier, WasmCompilationTier::BaselineJit);
        assert_eq!(plan.tier_up, Some(WasmCompilerMode::Omg));
        assert!(plan.can_install_code);
    }

    #[test]
    fn wasm_tier_planner_rejects_unavailable_tier() {
        let request = WasmCompilationRequestBuilder::new(
            WasmCompilationPlanId(11),
            WasmModuleId(1),
            WasmCompilationKind::ModuleEntry,
            WasmCompilerMode::Omg,
        )
        .build()
        .unwrap();

        let error = plan_wasm_compilation_tiers(
            &request,
            WasmTierPlanningConfig {
                enable_interpreter: false,
                enable_baseline_jit: false,
                enable_optimizing_jit: false,
                enable_bridge_tiers: false,
            },
        )
        .unwrap_err();

        assert_eq!(
            error,
            WasmCompilationValidationError::NoAvailableTier(WasmCompilerMode::Omg)
        );
    }

    #[test]
    fn wasm_fallback_record_reports_lower_tier_selection() {
        let request = WasmCompilationRequestBuilder::new(
            WasmCompilationPlanId(12),
            WasmModuleId(1),
            WasmCompilationKind::Function,
            WasmCompilerMode::Omg,
        )
        .function(WasmFunctionIndex(0))
        .code_index(WasmFunctionCodeIndex(0))
        .build()
        .unwrap();

        let record = describe_wasm_compilation_fallback(
            &request,
            WasmTierPlanningConfig {
                enable_interpreter: true,
                enable_baseline_jit: true,
                enable_optimizing_jit: false,
                enable_bridge_tiers: true,
            },
        );

        assert_eq!(record.selected, Some(WasmCompilerMode::Bbq));
        assert_eq!(record.target, WasmCompilationFallbackTarget::SelectedTier);
        assert_eq!(
            record.diagnostics[0].kind,
            WasmCompilationDiagnosticKind::FellBackToLowerTier
        );
    }

    #[test]
    fn wasm_fallback_record_reports_bridge_tier_disabled() {
        let request = WasmCompilationRequestBuilder::new(
            WasmCompilationPlanId(13),
            WasmModuleId(1),
            WasmCompilationKind::JsToWasmBridge,
            WasmCompilerMode::JsToWasmIc,
        )
        .function(WasmFunctionIndex(0))
        .build()
        .unwrap();

        let record = describe_wasm_compilation_fallback(
            &request,
            WasmTierPlanningConfig {
                enable_interpreter: true,
                enable_baseline_jit: true,
                enable_optimizing_jit: true,
                enable_bridge_tiers: false,
            },
        );

        assert_eq!(record.selected, None);
        assert_eq!(
            record.target,
            WasmCompilationFallbackTarget::RejectCompilation
        );
        assert_eq!(
            record.diagnostics[0].kind,
            WasmCompilationDiagnosticKind::BridgeTierDisabled
        );
    }
}
