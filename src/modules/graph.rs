use crate::modules::key::ModuleKey;
use crate::modules::record::ModuleRecordId;

/// Iterative graph-loading phase.
#[derive(Clone, Copy, Debug, Eq, PartialEq)]
pub enum GraphLoadPhase {
    Resolve,
    Fetch,
    Instantiate,
    Link,
    Evaluate,
    Complete,
    Error,
}

/// Host-defined payload threaded through graph loading callbacks.
#[derive(Clone, Copy, Debug, Eq, Hash, PartialEq)]
pub struct GraphLoadPayloadId(u64);

impl GraphLoadPayloadId {
    pub const fn from_host_token(token: u64) -> Self {
        Self(token)
    }

    pub const fn host_token(self) -> u64 {
        self.0
    }
}

/// Kind of payload supplied to `FinishLoadingImportedModule`.
#[derive(Clone, Copy, Debug, Eq, PartialEq)]
pub enum GraphLoadPayloadKind {
    GraphLoadingState,
    DynamicImport,
}

/// Completion passed from host load back into the graph state machine.
#[derive(Clone, Copy, Debug, Eq, PartialEq)]
pub enum ModuleLoadCompletion {
    Normal(ModuleRecordId),
    Abrupt(GraphLoadErrorKind),
}

/// Graph-load failure class.
#[derive(Clone, Copy, Debug, Eq, PartialEq)]
pub enum GraphLoadErrorKind {
    Resolution,
    Fetch,
    Instantiation,
    Evaluation,
    Cancelled,
}

/// A visited module in graph loading.
#[derive(Clone, Copy, Debug, Eq, Hash, PartialEq)]
pub struct VisitedModule {
    record: ModuleRecordId,
}

impl VisitedModule {
    pub const fn new(record: ModuleRecordId) -> Self {
        Self { record }
    }

    pub const fn record(self) -> ModuleRecordId {
        self.record
    }
}

/// State for an iterative module graph load.
///
/// This avoids recursive graph traversal and gives host callbacks a place to
/// suspend and later resume without baking in an event loop.
#[derive(Clone, Debug, Eq, PartialEq)]
pub struct ModuleGraphLoad {
    root: ModuleKey,
    phase: GraphLoadPhase,
    payload: Option<GraphLoadPayloadId>,
    pending_modules: u32,
    is_loading: bool,
}

impl ModuleGraphLoad {
    pub const fn new(root: ModuleKey) -> Self {
        Self {
            root,
            phase: GraphLoadPhase::Resolve,
            payload: None,
            pending_modules: 1,
            is_loading: true,
        }
    }

    pub const fn with_payload(
        root: ModuleKey,
        payload: GraphLoadPayloadId,
        phase: GraphLoadPhase,
    ) -> Self {
        Self {
            root,
            phase,
            payload: Some(payload),
            pending_modules: 1,
            is_loading: true,
        }
    }

    pub const fn phase(&self) -> GraphLoadPhase {
        self.phase
    }

    pub const fn root(&self) -> &ModuleKey {
        &self.root
    }

    pub const fn payload(&self) -> Option<GraphLoadPayloadId> {
        self.payload
    }

    pub const fn pending_modules(&self) -> u32 {
        self.pending_modules
    }

    pub const fn is_loading(&self) -> bool {
        self.is_loading
    }
}
