use crate::modules::key::ModuleKey;
use crate::modules::request::ModuleRequestFailureKind;
use crate::modules::ModuleRecordId;
use std::collections::HashSet;

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

/// Host fetcher retained by module registry entries and graph payloads.
#[derive(Clone, Copy, Debug, Eq, Hash, PartialEq)]
pub struct ScriptFetcherSlot(u32);

impl ScriptFetcherSlot {
    pub const fn from_host_slot(slot: u32) -> Self {
        Self(slot)
    }

    pub const fn host_slot(self) -> u32 {
        self.0
    }
}

/// Error cached on a registry entry.
#[derive(Clone, Copy, Debug, Eq, PartialEq)]
pub struct ModuleRegistryError {
    kind: ModuleRequestFailureKind,
    slot: ModuleErrorSlot,
}

/// Distinct error slots on a registry entry.
///
/// Fetch, instantiation, and evaluation errors have different duplication and
/// reporting rules in the loader. Keeping them separate avoids collapsing JSC's
/// registry-entry contract into a single generic cached error.
#[derive(Clone, Copy, Debug, Default, Eq, PartialEq)]
pub struct ModuleRegistryErrorSlots {
    pub fetch: Option<ModuleErrorSlot>,
    pub instantiation: Option<ModuleErrorSlot>,
    pub evaluation: Option<ModuleErrorSlot>,
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
    error_slots: ModuleRegistryErrorSlots,
    script_fetcher: Option<ScriptFetcherSlot>,
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
            error_slots: ModuleRegistryErrorSlots {
                fetch: None,
                instantiation: None,
                evaluation: None,
            },
            script_fetcher: None,
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
            error_slots: ModuleRegistryErrorSlots {
                fetch: None,
                instantiation: None,
                evaluation: None,
            },
            script_fetcher: None,
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

    pub const fn error_slots(&self) -> ModuleRegistryErrorSlots {
        self.error_slots
    }

    pub const fn script_fetcher(&self) -> Option<ScriptFetcherSlot> {
        self.script_fetcher
    }
}

/// Owner of immutable module registry metadata.
#[derive(Clone, Copy, Debug, Eq, PartialEq)]
pub enum ModuleRegistryOwner {
    RealmModuleLoader,
    VmModuleLoader,
    HostLoader,
    GeneratedStaticData,
}

/// Provenance for registry descriptor data.
#[derive(Clone, Copy, Debug, Eq, PartialEq)]
pub enum ModuleRegistryProvenance {
    RuntimeSchema,
    HostEmbedding,
    GeneratedFromModuleAnalysis,
    GeneratedFromEngineMetadata,
}

/// Immutable registry table descriptor.
///
/// Registry mutation authority remains with the realm loader. This descriptor
/// only exposes preexisting static registry-entry and failure metadata.
#[derive(Clone, Copy, Debug, Eq, PartialEq)]
pub struct ModuleRegistryDescriptor {
    pub name: &'static str,
    pub owner: ModuleRegistryOwner,
    pub provenance: ModuleRegistryProvenance,
    entries: &'static [ModuleRegistryEntry],
    resolution_failures: &'static [ModuleResolutionFailure],
}

#[derive(Clone, Debug, Eq, PartialEq)]
pub enum ModuleRegistryValidationError {
    EmptyName,
    DuplicateKey(ModuleKey),
    DuplicateResolutionFailure(ModuleErrorSlot),
    NewEntryHasRuntimeState,
    FetchingEntryHasNoPendingWork,
    LoadedEntryMissingRecord(ModuleKey),
    FailedEntryMissingError(ModuleKey),
    CachedErrorSlotMismatch(ModuleKey),
    InvalidStatusTransition(ModuleStatusTransition),
}

#[derive(Clone, Copy, Debug, Eq, PartialEq)]
pub struct ModuleRegistryDescriptorBuilder {
    name: &'static str,
    owner: ModuleRegistryOwner,
    provenance: ModuleRegistryProvenance,
    entries: &'static [ModuleRegistryEntry],
    resolution_failures: &'static [ModuleResolutionFailure],
}

impl ModuleRegistryDescriptorBuilder {
    pub const fn new(
        name: &'static str,
        owner: ModuleRegistryOwner,
        provenance: ModuleRegistryProvenance,
    ) -> Self {
        Self {
            name,
            owner,
            provenance,
            entries: &[],
            resolution_failures: &[],
        }
    }

    pub const fn entries(mut self, entries: &'static [ModuleRegistryEntry]) -> Self {
        self.entries = entries;
        self
    }

    pub const fn resolution_failures(
        mut self,
        resolution_failures: &'static [ModuleResolutionFailure],
    ) -> Self {
        self.resolution_failures = resolution_failures;
        self
    }

    pub fn build(self) -> Result<ModuleRegistryDescriptor, ModuleRegistryValidationError> {
        let descriptor = ModuleRegistryDescriptor::new(
            self.name,
            self.owner,
            self.provenance,
            self.entries,
            self.resolution_failures,
        );
        descriptor.validate()?;
        Ok(descriptor)
    }
}

impl ModuleRegistryDescriptor {
    pub const fn new(
        name: &'static str,
        owner: ModuleRegistryOwner,
        provenance: ModuleRegistryProvenance,
        entries: &'static [ModuleRegistryEntry],
        resolution_failures: &'static [ModuleResolutionFailure],
    ) -> Self {
        Self {
            name,
            owner,
            provenance,
            entries,
            resolution_failures,
        }
    }

    /// Returns immutable module registry entries.
    pub const fn entries(&self) -> &'static [ModuleRegistryEntry] {
        self.entries
    }

    /// Returns immutable cached resolution failures.
    pub const fn resolution_failures(&self) -> &'static [ModuleResolutionFailure] {
        self.resolution_failures
    }

    /// Returns one existing registry entry by table index.
    pub const fn entry_at(&self, index: usize) -> Option<&'static ModuleRegistryEntry> {
        if index < self.entries.len() {
            Some(&self.entries[index])
        } else {
            None
        }
    }

    pub fn validate(&self) -> Result<(), ModuleRegistryValidationError> {
        validate_module_registry_descriptor(self)
    }
}

pub fn validate_module_registry_descriptor(
    descriptor: &ModuleRegistryDescriptor,
) -> Result<(), ModuleRegistryValidationError> {
    if descriptor.name.is_empty() {
        return Err(ModuleRegistryValidationError::EmptyName);
    }

    let mut seen_keys = HashSet::new();
    for entry in descriptor.entries {
        validate_module_registry_entry(entry)?;
        if !seen_keys.insert(entry.key.clone()) {
            return Err(ModuleRegistryValidationError::DuplicateKey(
                entry.key.clone(),
            ));
        }
    }

    let mut seen_errors = HashSet::new();
    for failure in descriptor.resolution_failures {
        if !seen_errors.insert(failure.error()) {
            return Err(ModuleRegistryValidationError::DuplicateResolutionFailure(
                failure.error(),
            ));
        }
    }

    Ok(())
}

pub fn validate_module_registry_entry(
    entry: &ModuleRegistryEntry,
) -> Result<(), ModuleRegistryValidationError> {
    match entry.status {
        ModuleStatus::New => {
            let has_runtime_state = entry.record.is_some()
                || entry.fetch_promise.is_some()
                || entry.module_promise.is_some()
                || entry.load_promise.is_some()
                || entry.cached_error.is_some()
                || entry.error_slots.fetch.is_some()
                || entry.error_slots.instantiation.is_some()
                || entry.error_slots.evaluation.is_some()
                || entry.script_fetcher.is_some();
            if has_runtime_state {
                return Err(ModuleRegistryValidationError::NewEntryHasRuntimeState);
            }
        }
        ModuleStatus::Fetching => {
            if entry.fetch_promise.is_none() && entry.script_fetcher.is_none() {
                return Err(ModuleRegistryValidationError::FetchingEntryHasNoPendingWork);
            }
        }
        ModuleStatus::Fetched
        | ModuleStatus::Linking
        | ModuleStatus::Linked
        | ModuleStatus::Evaluating
        | ModuleStatus::Evaluated => {
            if entry.record.is_none() {
                return Err(ModuleRegistryValidationError::LoadedEntryMissingRecord(
                    entry.key.clone(),
                ));
            }
        }
        ModuleStatus::FetchFailed
        | ModuleStatus::InstantiationFailed
        | ModuleStatus::EvaluationFailed => {
            if entry.cached_error.is_none()
                && entry.error_slots.fetch.is_none()
                && entry.error_slots.instantiation.is_none()
                && entry.error_slots.evaluation.is_none()
            {
                return Err(ModuleRegistryValidationError::FailedEntryMissingError(
                    entry.key.clone(),
                ));
            }
        }
    }

    if let Some(error) = entry.cached_error {
        let matching_slot = match error.kind() {
            ModuleRequestFailureKind::Resolution | ModuleRequestFailureKind::Fetch => {
                entry.error_slots.fetch
            }
            ModuleRequestFailureKind::Instantiation
            | ModuleRequestFailureKind::Linking
            | ModuleRequestFailureKind::UnsupportedAttributes
            | ModuleRequestFailureKind::ImportMap => entry.error_slots.instantiation,
            ModuleRequestFailureKind::Evaluation | ModuleRequestFailureKind::TopLevelAwait => {
                entry.error_slots.evaluation
            }
        };
        if matching_slot.is_some_and(|slot| slot != error.slot()) {
            return Err(ModuleRegistryValidationError::CachedErrorSlotMismatch(
                entry.key.clone(),
            ));
        }
    }

    Ok(())
}

pub fn validate_module_status_transition(
    transition: ModuleStatusTransition,
) -> Result<(), ModuleRegistryValidationError> {
    let allowed = matches!(
        (transition.from, transition.to),
        (ModuleStatus::New, ModuleStatus::Fetching)
            | (ModuleStatus::New, ModuleStatus::Fetched)
            | (ModuleStatus::Fetching, ModuleStatus::Fetched)
            | (ModuleStatus::Fetching, ModuleStatus::FetchFailed)
            | (ModuleStatus::Fetched, ModuleStatus::Linking)
            | (ModuleStatus::Fetched, ModuleStatus::InstantiationFailed)
            | (ModuleStatus::Linking, ModuleStatus::Linked)
            | (ModuleStatus::Linking, ModuleStatus::InstantiationFailed)
            | (ModuleStatus::Linked, ModuleStatus::Evaluating)
            | (ModuleStatus::Evaluating, ModuleStatus::Evaluated)
            | (ModuleStatus::Evaluating, ModuleStatus::EvaluationFailed)
            | (ModuleStatus::FetchFailed, ModuleStatus::FetchFailed)
            | (
                ModuleStatus::InstantiationFailed,
                ModuleStatus::InstantiationFailed
            )
            | (
                ModuleStatus::EvaluationFailed,
                ModuleStatus::EvaluationFailed
            )
    );

    if allowed {
        Ok(())
    } else {
        Err(ModuleRegistryValidationError::InvalidStatusTransition(
            transition,
        ))
    }
}

/// Runtime state a registry transition needs from its caller.
#[derive(Clone, Copy, Debug, Eq, PartialEq)]
pub enum ModuleRegistryTransitionRequirement {
    None,
    PendingFetch,
    ModuleRecord,
    CachedError,
}

/// Non-mutating transition plan for a registry entry.
#[derive(Clone, Debug, Eq, PartialEq)]
pub struct ModuleRegistryTransitionPlan {
    pub key: ModuleKey,
    pub transition: ModuleStatusTransition,
    pub requirement: ModuleRegistryTransitionRequirement,
}

pub fn plan_module_registry_transition(
    entry: &ModuleRegistryEntry,
    to: ModuleStatus,
) -> Result<ModuleRegistryTransitionPlan, ModuleRegistryValidationError> {
    validate_module_registry_entry(entry)?;

    let transition = ModuleStatusTransition {
        from: entry.status,
        to,
    };
    validate_module_status_transition(transition)?;

    Ok(ModuleRegistryTransitionPlan {
        key: entry.key.clone(),
        transition,
        requirement: module_registry_transition_requirement(to),
    })
}

pub const fn module_registry_transition_requirement(
    to: ModuleStatus,
) -> ModuleRegistryTransitionRequirement {
    match to {
        ModuleStatus::New => ModuleRegistryTransitionRequirement::None,
        ModuleStatus::Fetching => ModuleRegistryTransitionRequirement::PendingFetch,
        ModuleStatus::Fetched
        | ModuleStatus::Linking
        | ModuleStatus::Linked
        | ModuleStatus::Evaluating
        | ModuleStatus::Evaluated => ModuleRegistryTransitionRequirement::ModuleRecord,
        ModuleStatus::FetchFailed
        | ModuleStatus::InstantiationFailed
        | ModuleStatus::EvaluationFailed => ModuleRegistryTransitionRequirement::CachedError,
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

#[cfg(test)]
mod tests {
    use super::*;
    use crate::modules::{ImportAttributes, ModuleType, ResolvedSpecifier};
    use crate::strings::{AtomId, Identifier};

    fn module_key(slot: u32) -> ModuleKey {
        ModuleKey::new(
            ResolvedSpecifier::from_identifier(Identifier::from_atom(AtomId::from_table_slot(
                slot,
            ))),
            ModuleType::JavaScript,
            ImportAttributes::empty(),
        )
    }

    #[test]
    fn registry_transition_planner_reports_pending_fetch_requirement() {
        let entry = ModuleRegistryEntry::new(module_key(1));

        let plan = plan_module_registry_transition(&entry, ModuleStatus::Fetching).unwrap();

        assert_eq!(
            plan.transition,
            ModuleStatusTransition {
                from: ModuleStatus::New,
                to: ModuleStatus::Fetching
            }
        );
        assert_eq!(
            plan.requirement,
            ModuleRegistryTransitionRequirement::PendingFetch
        );
    }

    #[test]
    fn registry_transition_planner_rejects_non_monotonic_transition() {
        let entry = ModuleRegistryEntry::with_state(
            module_key(1),
            ModuleStatus::Linked,
            Some(ModuleRecordId::from_loader_slot(4)),
            None,
            None,
            None,
            None,
        );

        let error = plan_module_registry_transition(&entry, ModuleStatus::Fetched).unwrap_err();

        assert_eq!(
            error,
            ModuleRegistryValidationError::InvalidStatusTransition(ModuleStatusTransition {
                from: ModuleStatus::Linked,
                to: ModuleStatus::Fetched
            })
        );
    }
}
