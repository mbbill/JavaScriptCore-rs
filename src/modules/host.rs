use crate::modules::graph::{GraphLoadPayloadId, GraphLoadPayloadKind, ModuleLoadCompletion};
use crate::modules::key::{ImportMapResolution, ModuleKey, ResolvedSpecifier};
use crate::modules::request::{ModuleRequest, ModuleRequestPhase};
use crate::modules::ModuleRecordId;

/// Opaque host payload carried across asynchronous load operations.
#[derive(Clone, Copy, Debug, Eq, Hash, PartialEq)]
pub struct HostModulePayload(u64);

impl HostModulePayload {
    pub const fn from_host_token(token: u64) -> Self {
        Self(token)
    }
}

/// Host-visible module-loading failure category.
#[derive(Clone, Copy, Debug, Eq, PartialEq)]
pub enum HostModuleError {
    ResolutionFailed,
    FetchFailed,
    UnsupportedModuleType,
    UnsupportedImportAttributes,
    ImportMapResolutionFailed,
    HostException,
    EvaluationFailed,
    TopLevelAwaitRejected,
}

/// Host hook result that can complete immediately or suspend.
#[derive(Clone, Copy, Debug, Eq, PartialEq)]
pub enum HostHookResult<T> {
    Ready(T),
    Pending(HostModulePayload),
    Failed(HostModuleError),
}

/// Referrer supplied to host resolution/loading hooks.
#[derive(Clone, Copy, Debug, Eq, PartialEq)]
pub enum HostModuleReferrer {
    Script,
    Module(ModuleRecordId),
    Realm,
}

/// Host-created import.meta payload.
#[derive(Clone, Copy, Debug, Eq, Hash, PartialEq)]
pub struct ImportMetaObjectId(u32);

impl ImportMetaObjectId {
    pub const fn from_runtime_slot(slot: u32) -> Self {
        Self(slot)
    }

    pub const fn runtime_slot(self) -> u32 {
        self.0
    }
}

/// Evaluation hook request.
///
/// `sent_value` and `resume_mode` are intentionally opaque. They are generator
/// protocol values owned by runtime execution state, not module-loader data.
#[derive(Clone, Debug, Eq, PartialEq)]
pub struct HostEvaluateRequest {
    key: ModuleKey,
    module: ModuleRecordId,
    phase: ModuleRequestPhase,
    top_level_await: bool,
}

impl HostEvaluateRequest {
    pub const fn new(key: ModuleKey, module: ModuleRecordId) -> Self {
        Self {
            key,
            module,
            phase: ModuleRequestPhase::Evaluation,
            top_level_await: false,
        }
    }

    pub const fn with_phase(
        key: ModuleKey,
        module: ModuleRecordId,
        phase: ModuleRequestPhase,
        top_level_await: bool,
    ) -> Self {
        Self {
            key,
            module,
            phase,
            top_level_await,
        }
    }

    pub const fn key(&self) -> &ModuleKey {
        &self.key
    }

    pub const fn module(&self) -> ModuleRecordId {
        self.module
    }

    pub const fn phase(&self) -> ModuleRequestPhase {
        self.phase
    }

    pub const fn top_level_await(&self) -> bool {
        self.top_level_await
    }
}

/// Completion passed from the host back into module loading.
#[derive(Clone, Copy, Debug, Eq, PartialEq)]
pub struct HostLoadCompletion {
    payload: GraphLoadPayloadId,
    payload_kind: GraphLoadPayloadKind,
    completion: ModuleLoadCompletion,
}

impl HostLoadCompletion {
    pub const fn new(
        payload: GraphLoadPayloadId,
        payload_kind: GraphLoadPayloadKind,
        completion: ModuleLoadCompletion,
    ) -> Self {
        Self {
            payload,
            payload_kind,
            completion,
        }
    }
}

/// Host hook boundary for module loading.
///
/// Implementations may complete asynchronously and may reenter the VM through
/// API entry scopes. Any opaque host data crossing FFI belongs behind this
/// boundary.
pub trait HostModuleLoader {
    fn resolve(&mut self, request: &ModuleRequest) -> Result<ResolvedSpecifier, HostModuleError>;
    fn resolve_import_map(
        &mut self,
        request: &ModuleRequest,
    ) -> HostHookResult<ImportMapResolution>;
    fn fetch(&mut self, key: &ModuleKey) -> Result<HostModulePayload, HostModuleError>;

    fn create_import_meta(
        &mut self,
        key: &ModuleKey,
        module: ModuleRecordId,
    ) -> HostHookResult<ImportMetaObjectId>;

    fn evaluate(&mut self, request: HostEvaluateRequest) -> HostHookResult<()>;
}
