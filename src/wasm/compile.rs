//! Deferred Wasm validation and compilation plans.
//!
//! Future implementations may coordinate with JIT worklists and code liveness.
//! No validation, decoding, code generation, or tier selection lives here. The
//! types only describe request, state, and finalization ownership boundaries.

use crate::jit::{CompilationProduct, JitType};
use crate::runtime::SourceProviderId;
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
    Preparing,
    Compiling,
    Finalizing,
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
    BridgeStub,
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
    pub priority: WasmCompilationPriority,
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

/// Host-facing compilation-plan contract.
pub trait WasmCompilationPlan {
    fn id(&self) -> WasmCompilationPlanId;
    fn request(&self) -> &WasmCompilationRequest;
    fn module(&self) -> WasmModuleId;
    fn state(&self) -> WasmCompilationState;
    fn jit_tier_hint(&self) -> Option<JitType>;
    fn product(&self) -> Option<&WasmCompilationProduct>;
    fn cancel(&mut self);
}
