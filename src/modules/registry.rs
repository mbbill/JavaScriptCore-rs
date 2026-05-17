use crate::modules::key::ModuleKey;
use crate::modules::record::ModuleRecordId;
use crate::modules::request::ModuleRequestFailureKind;

/// Registry status for one module key.
#[derive(Clone, Copy, Debug, Eq, PartialEq)]
pub enum ModuleStatus {
    New,
    Fetching,
    Fetched,
    FetchFailed,
    InstantiationFailed,
    EvaluationFailed,
    Linking,
    Linked,
    Evaluating,
    Evaluated,
}

/// Explicit status transition request.
///
/// Implementations must preserve monotonic spec transitions except for
/// well-defined error states and cached failures.
#[derive(Clone, Copy, Debug, Eq, PartialEq)]
pub struct ModuleStatusTransition {
    pub from: ModuleStatus,
    pub to: ModuleStatus,
}

/// Promise-like slot owned by runtime promise integration.
#[derive(Clone, Copy, Debug, Eq, Hash, PartialEq)]
pub struct ModulePromiseSlot(u32);

impl ModulePromiseSlot {
    pub const fn from_runtime_slot(slot: u32) -> Self {
        Self(slot)
    }

    pub const fn runtime_slot(self) -> u32 {
        self.0
    }
}

/// Cached error slot owned by runtime exception storage.
#[derive(Clone, Copy, Debug, Eq, Hash, PartialEq)]
pub struct ModuleErrorSlot(u32);

impl ModuleErrorSlot {
    pub const fn from_runtime_slot(slot: u32) -> Self {
        Self(slot)
    }

    pub const fn runtime_slot(self) -> u32 {
        self.0
    }
}

/// Error cached on a registry entry.
#[derive(Clone, Copy, Debug, Eq, PartialEq)]
pub struct ModuleRegistryError {
    kind: ModuleRequestFailureKind,
    slot: ModuleErrorSlot,
}

impl ModuleRegistryError {
    pub const fn new(kind: ModuleRequestFailureKind, slot: ModuleErrorSlot) -> Self {
        Self { kind, slot }
    }

    pub const fn kind(self) -> ModuleRequestFailureKind {
        self.kind
    }

    pub const fn slot(self) -> ModuleErrorSlot {
        self.slot
    }
}

/// Registry entry keyed by resolved module identity.
///
/// Entries keep records, promises, fetchers, and cached errors alive. The
/// concrete GC/promise representation is deferred to runtime modules.
#[derive(Clone, Debug, Eq, PartialEq)]
pub struct ModuleRegistryEntry {
    key: ModuleKey,
    status: ModuleStatus,
    record: Option<ModuleRecordId>,
    fetch_promise: Option<ModulePromiseSlot>,
    module_promise: Option<ModulePromiseSlot>,
    load_promise: Option<ModulePromiseSlot>,
    cached_error: Option<ModuleRegistryError>,
}

impl ModuleRegistryEntry {
    pub const fn new(key: ModuleKey) -> Self {
        Self {
            key,
            status: ModuleStatus::New,
            record: None,
            fetch_promise: None,
            module_promise: None,
            load_promise: None,
            cached_error: None,
        }
    }

    pub const fn with_state(
        key: ModuleKey,
        status: ModuleStatus,
        record: Option<ModuleRecordId>,
        fetch_promise: Option<ModulePromiseSlot>,
        module_promise: Option<ModulePromiseSlot>,
        load_promise: Option<ModulePromiseSlot>,
        cached_error: Option<ModuleRegistryError>,
    ) -> Self {
        Self {
            key,
            status,
            record,
            fetch_promise,
            module_promise,
            load_promise,
            cached_error,
        }
    }

    pub const fn status(&self) -> ModuleStatus {
        self.status
    }

    pub const fn key(&self) -> &ModuleKey {
        &self.key
    }

    pub const fn record(&self) -> Option<ModuleRecordId> {
        self.record
    }

    pub const fn cached_error(&self) -> Option<ModuleRegistryError> {
        self.cached_error
    }
}

/// Cached resolution failure keyed by referrer plus unresolved specifier.
///
/// JSC keeps this separate from the module registry so repeated resolution
/// failures can reuse the same error while fetch failures are duplicated for
/// dynamic-import promise semantics.
#[derive(Clone, Copy, Debug, Eq, PartialEq)]
pub struct ModuleResolutionFailure {
    error: ModuleErrorSlot,
}

impl ModuleResolutionFailure {
    pub const fn new(error: ModuleErrorSlot) -> Self {
        Self { error }
    }

    pub const fn error(self) -> ModuleErrorSlot {
        self.error
    }
}

/// Realm-owned map from `ModuleKey` to registry entries.
#[derive(Debug, Default)]
pub struct ModuleRegistry {
    _sealed: (),
}

impl ModuleRegistry {
    pub const fn new_uninitialized() -> Self {
        Self { _sealed: () }
    }
}
