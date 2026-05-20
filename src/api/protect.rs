// This module contains staged API/GC ownership contracts and the narrow bridge
// from protection plans to live targeted-root heap mutations.
#![allow(dead_code)]

use std::collections::BTreeMap;

use crate::api::handles::{ApiContextGroup, ApiValueRef};
use crate::gc::{
    CellId, Heap, HeapId, HeapIntegrationError, RootId, RootKind, RootRecord, RootSetMutation,
    RootSetMutationAuthority, RootSetMutationKind, RootSetSemanticError, TargetedRootRecord,
    TargetedRootSet,
};

const API_PROTECTION_ROOT_ID_BASE: u64 = 6_000_000;

/// VM heap slot used to count API protections.
#[derive(Clone, Copy, Debug, Eq, Hash, Ord, PartialEq, PartialOrd)]
pub struct ApiProtectionSlot(u32);

impl ApiProtectionSlot {
    pub const fn from_heap_slot(slot: u32) -> Self {
        Self(slot)
    }

    pub const fn heap_slot(self) -> u32 {
        self.0
    }
}

pub const fn api_protection_root_id(slot: ApiProtectionSlot) -> RootId {
    RootId(API_PROTECTION_ROOT_ID_BASE + slot.heap_slot() as u64 + 1)
}

/// Owner of API protection/rooting state.
#[derive(Clone, Copy, Debug, Eq, Hash, PartialEq)]
pub struct ApiProtectionOwner {
    context_group: ApiContextGroup,
}

impl ApiProtectionOwner {
    pub const fn new(context_group: ApiContextGroup) -> Self {
        Self { context_group }
    }
}

/// Registered heap-finalizer hook for a context group.
///
/// The VM owns registration order and invocation timing. The embedder owns
/// `user_data`, and the callback receives the context group rather than a
/// normal execution context.
#[derive(Clone, Copy, Debug, Eq, PartialEq)]
pub struct ApiHeapFinalizerRegistration {
    owner: ApiProtectionOwner,
    user_data_id: usize,
    is_registered: bool,
}

impl ApiHeapFinalizerRegistration {
    pub const fn new(owner: ApiProtectionOwner, user_data_id: usize) -> Self {
        Self {
            owner,
            user_data_id,
            is_registered: true,
        }
    }

    pub const fn owner(self) -> ApiProtectionOwner {
        self.owner
    }

    pub const fn user_data_id(self) -> usize {
        self.user_data_id
    }

    pub const fn is_registered(self) -> bool {
        self.is_registered
    }
}

/// Lifecycle action applied to a protected value.
#[derive(Clone, Copy, Debug, Eq, PartialEq)]
pub enum ApiProtectionAction {
    Protect,
    Unprotect,
    Retarget,
}

/// Counter discipline for `JSValueProtect` / `JSValueUnprotect`.
///
/// A value may be protected multiple times. Rust ownership cannot express this
/// by itself because the C API exposes balanced calls rather than RAII-only
/// lifetimes, so the count remains VM heap state.
#[derive(Clone, Copy, Debug, Eq, PartialEq)]
pub struct ApiProtectionCount {
    slot: ApiProtectionSlot,
    count: u32,
}

impl ApiProtectionCount {
    pub const fn new(slot: ApiProtectionSlot, count: u32) -> Self {
        Self { slot, count }
    }

    pub const fn slot(self) -> ApiProtectionSlot {
        self.slot
    }

    pub const fn count(self) -> u32 {
        self.count
    }
}

/// Protected/rooted API value.
///
/// A protected value keeps a GC value alive until the embedder balances the
/// protection. The concrete root slot belongs to GC/VM integration and must not
/// be represented by ordinary Rust ownership alone.
#[derive(Clone, Copy, Debug, Eq, PartialEq)]
pub struct ProtectedValue {
    owner: ApiProtectionOwner,
    value: ApiValueRef,
    slot: Option<ApiProtectionSlot>,
}

impl ProtectedValue {
    pub const fn from_protected_slot(owner: ApiProtectionOwner, value: ApiValueRef) -> Self {
        Self {
            owner,
            value,
            slot: None,
        }
    }

    pub const fn with_slot(
        owner: ApiProtectionOwner,
        value: ApiValueRef,
        slot: ApiProtectionSlot,
    ) -> Self {
        Self {
            owner,
            value,
            slot: Some(slot),
        }
    }

    pub const fn value(self) -> ApiValueRef {
        self.value
    }

    pub const fn owner(self) -> ApiProtectionOwner {
        self.owner
    }

    pub const fn slot(self) -> Option<ApiProtectionSlot> {
        self.slot
    }
}

/// Heap target represented by a protected API value.
///
/// Immediate/non-cell values remain countable API protections, but they do not
/// emit a precise targeted root until the API can identify a concrete cell.
#[derive(Clone, Copy, Debug, Eq, PartialEq)]
pub enum ApiProtectionTarget {
    Cell { heap: HeapId, cell: CellId },
    NoTarget,
}

impl ApiProtectionTarget {
    pub const fn cell(heap: HeapId, cell: CellId) -> Self {
        Self::Cell { heap, cell }
    }

    pub const fn no_target() -> Self {
        Self::NoTarget
    }
}

/// Targeted-root mutation requested by an API protection lifecycle step.
#[derive(Clone, Copy, Debug, Eq, PartialEq)]
pub struct ApiProtectionRootMutation {
    kind: RootSetMutationKind,
    authority: RootSetMutationAuthority,
    record: TargetedRootRecord,
}

impl ApiProtectionRootMutation {
    const fn register(record: TargetedRootRecord) -> Self {
        Self {
            kind: RootSetMutationKind::Register,
            authority: RootSetMutationAuthority::HandleScope,
            record,
        }
    }

    const fn unregister(record: TargetedRootRecord) -> Self {
        Self {
            kind: RootSetMutationKind::Unregister,
            authority: RootSetMutationAuthority::HandleScope,
            record,
        }
    }

    const fn retarget(record: TargetedRootRecord) -> Self {
        Self {
            kind: RootSetMutationKind::Retarget,
            authority: RootSetMutationAuthority::HandleScope,
            record,
        }
    }

    pub const fn kind(self) -> RootSetMutationKind {
        self.kind
    }

    pub const fn authority(self) -> RootSetMutationAuthority {
        self.authority
    }

    pub const fn record(self) -> TargetedRootRecord {
        self.record
    }

    pub const fn root_set_mutation(self) -> RootSetMutation {
        match self.kind {
            RootSetMutationKind::Register => {
                RootSetMutation::register(self.record.root, self.authority)
            }
            RootSetMutationKind::Unregister => {
                RootSetMutation::unregister(self.record.root, self.authority)
            }
            RootSetMutationKind::Retarget => {
                RootSetMutation::retarget(self.record.root, self.authority)
            }
        }
    }

    pub fn apply_to_heap(
        self,
        heap: &mut Heap,
    ) -> Result<ApiProtectionRootMutationOutcome, HeapIntegrationError> {
        match self.kind {
            RootSetMutationKind::Register => heap
                .register_targeted_root(self.record, self.authority)
                .map(ApiProtectionRootMutationOutcome::Registered),
            RootSetMutationKind::Unregister => heap
                .unregister_targeted_root(self.record.root, self.authority)
                .map(ApiProtectionRootMutationOutcome::Unregistered),
            RootSetMutationKind::Retarget => heap
                .retarget_targeted_root(self.record, self.authority)
                .map(ApiProtectionRootMutationOutcome::Retargeted),
        }
    }
}

#[derive(Clone, Copy, Debug, Eq, PartialEq)]
pub enum ApiProtectionRootMutationOutcome {
    Registered(TargetedRootRecord),
    Unregistered(RootId),
    Retargeted(TargetedRootRecord),
}

#[derive(Clone, Copy, Debug, Eq, PartialEq)]
pub struct ApiProtectionOutcome {
    plan: ApiProtectionPlan,
    root_mutation: Option<ApiProtectionRootMutationOutcome>,
}

impl ApiProtectionOutcome {
    pub(crate) const fn new(
        plan: ApiProtectionPlan,
        root_mutation: Option<ApiProtectionRootMutationOutcome>,
    ) -> Self {
        Self {
            plan,
            root_mutation,
        }
    }

    pub const fn action(self) -> ApiProtectionAction {
        self.plan.action()
    }

    pub const fn slot(self) -> ApiProtectionSlot {
        self.plan.slot()
    }

    pub const fn value(self) -> ApiValueRef {
        self.plan.value()
    }

    pub const fn target(self) -> ApiProtectionTarget {
        self.plan.target()
    }

    pub const fn count(self) -> ApiProtectionCount {
        self.plan.count()
    }

    pub const fn root_mutation_outcome(self) -> Option<ApiProtectionRootMutationOutcome> {
        self.root_mutation
    }
}

/// Data-level plan produced by protect/unprotect/retarget calls.
#[derive(Clone, Copy, Debug, Eq, PartialEq)]
pub struct ApiProtectionPlan {
    action: ApiProtectionAction,
    slot: ApiProtectionSlot,
    value: ApiValueRef,
    target: ApiProtectionTarget,
    count: ApiProtectionCount,
    root_mutation: Option<ApiProtectionRootMutation>,
}

impl ApiProtectionPlan {
    pub const fn action(self) -> ApiProtectionAction {
        self.action
    }

    pub const fn slot(self) -> ApiProtectionSlot {
        self.slot
    }

    pub const fn value(self) -> ApiValueRef {
        self.value
    }

    pub const fn target(self) -> ApiProtectionTarget {
        self.target
    }

    pub const fn count(self) -> ApiProtectionCount {
        self.count
    }

    pub const fn root_mutation(self) -> Option<ApiProtectionRootMutation> {
        self.root_mutation
    }

    pub fn apply_root_mutation_to_heap(
        self,
        heap: &mut Heap,
    ) -> Result<Option<ApiProtectionRootMutationOutcome>, HeapIntegrationError> {
        self.root_mutation
            .map(|mutation| mutation.apply_to_heap(heap))
            .transpose()
    }
}

#[derive(Clone, Copy, Debug, Eq, PartialEq)]
pub enum ApiProtectionRegistryError {
    MissingProtectionSlot,
    UnknownProtectionSlot(ApiProtectionSlot),
    SlotOwnerMismatch {
        slot: ApiProtectionSlot,
        existing: ApiProtectionOwner,
        requested: ApiProtectionOwner,
    },
    SlotValueMismatch {
        slot: ApiProtectionSlot,
        existing: ApiValueRef,
        requested: ApiValueRef,
    },
    SlotTargetMismatch {
        slot: ApiProtectionSlot,
        existing: ApiProtectionTarget,
        requested: ApiProtectionTarget,
    },
    DefaultTargetCell {
        slot: ApiProtectionSlot,
    },
    HeapMismatch {
        slot: ApiProtectionSlot,
        expected: HeapId,
        actual: HeapId,
    },
    ProtectionCountOverflow(ApiProtectionSlot),
    RootSet(RootSetSemanticError),
    Heap(HeapIntegrationError),
}

impl From<RootSetSemanticError> for ApiProtectionRegistryError {
    fn from(error: RootSetSemanticError) -> Self {
        Self::RootSet(error)
    }
}

impl From<HeapIntegrationError> for ApiProtectionRegistryError {
    fn from(error: HeapIntegrationError) -> Self {
        Self::Heap(error)
    }
}

/// Counted API protection entry plus the optional precise root it represents.
#[derive(Clone, Copy, Debug, Eq, PartialEq)]
pub struct ApiProtectionRegistryEntry {
    protected: ProtectedValue,
    count: ApiProtectionCount,
    target: ApiProtectionTarget,
    root: Option<TargetedRootRecord>,
}

impl ApiProtectionRegistryEntry {
    pub const fn protected(self) -> ProtectedValue {
        self.protected
    }

    pub const fn count(self) -> ApiProtectionCount {
        self.count
    }

    pub const fn target(self) -> ApiProtectionTarget {
        self.target
    }

    pub const fn root(self) -> Option<TargetedRootRecord> {
        self.root
    }
}

/// Data-level registry for `JSValueProtect` / `JSValueUnprotect`.
///
/// This mirrors the future API-protected-value root lifecycle without mutating
/// a live VM heap. Cell targets produce `RootKind::Handle` targeted records
/// under `RootSetMutationAuthority::HandleScope`; non-cell values are counted
/// but have no targeted root record.
#[derive(Clone, Debug, Default)]
pub struct ApiProtectionRegistry {
    heap: HeapId,
    entries: BTreeMap<ApiProtectionSlot, ApiProtectionRegistryEntry>,
}

impl ApiProtectionRegistry {
    pub const fn new(heap: HeapId) -> Self {
        Self {
            heap,
            entries: BTreeMap::new(),
        }
    }

    pub const fn heap(&self) -> HeapId {
        self.heap
    }

    pub fn is_empty(&self) -> bool {
        self.entries.is_empty()
    }

    pub fn entry(&self, slot: ApiProtectionSlot) -> Option<ApiProtectionRegistryEntry> {
        self.entries.get(&slot).copied()
    }

    pub fn targeted_roots(&self) -> Vec<TargetedRootRecord> {
        self.entries
            .values()
            .filter_map(|entry| entry.root)
            .collect()
    }

    pub fn protect_value(
        &mut self,
        heap: &mut Heap,
        protected: ProtectedValue,
        target: ApiProtectionTarget,
    ) -> Result<ApiProtectionOutcome, ApiProtectionRegistryError> {
        let entries = self.entries.clone();
        let plan = self.protect(protected, target)?;
        self.apply_plan_to_heap(heap, entries, plan)
    }

    pub fn unprotect_value(
        &mut self,
        heap: &mut Heap,
        slot: ApiProtectionSlot,
    ) -> Result<ApiProtectionOutcome, ApiProtectionRegistryError> {
        let entries = self.entries.clone();
        let plan = self.unprotect(slot)?;
        self.apply_plan_to_heap(heap, entries, plan)
    }

    pub fn retarget_value(
        &mut self,
        heap: &mut Heap,
        protected: ProtectedValue,
        target: ApiProtectionTarget,
    ) -> Result<ApiProtectionOutcome, ApiProtectionRegistryError> {
        let entries = self.entries.clone();
        let plan = self.retarget_slot(protected, target)?;
        self.apply_plan_to_heap(heap, entries, plan)
    }

    pub fn protect(
        &mut self,
        protected: ProtectedValue,
        target: ApiProtectionTarget,
    ) -> Result<ApiProtectionPlan, ApiProtectionRegistryError> {
        let slot = protected
            .slot()
            .ok_or(ApiProtectionRegistryError::MissingProtectionSlot)?;
        let root = self.targeted_root_for(slot, target)?;

        if let Some(existing) = self.entries.get(&slot).copied() {
            validate_same_slot_protection(slot, existing, protected, target)?;
            let count = existing
                .count
                .count()
                .checked_add(1)
                .ok_or(ApiProtectionRegistryError::ProtectionCountOverflow(slot))?;
            let updated = ApiProtectionRegistryEntry {
                count: ApiProtectionCount::new(slot, count),
                ..existing
            };
            self.entries.insert(slot, updated);
            return Ok(ApiProtectionPlan {
                action: ApiProtectionAction::Protect,
                slot,
                value: protected.value(),
                target,
                count: updated.count,
                root_mutation: None,
            });
        }

        self.validate_targeted_roots_with(slot, root)?;
        let entry = ApiProtectionRegistryEntry {
            protected,
            count: ApiProtectionCount::new(slot, 1),
            target,
            root,
        };
        self.entries.insert(slot, entry);
        Ok(ApiProtectionPlan {
            action: ApiProtectionAction::Protect,
            slot,
            value: protected.value(),
            target,
            count: entry.count,
            root_mutation: root.map(ApiProtectionRootMutation::register),
        })
    }

    pub fn unprotect(
        &mut self,
        slot: ApiProtectionSlot,
    ) -> Result<ApiProtectionPlan, ApiProtectionRegistryError> {
        let existing = self
            .entries
            .get(&slot)
            .copied()
            .ok_or(ApiProtectionRegistryError::UnknownProtectionSlot(slot))?;
        let next_count = existing
            .count
            .count()
            .checked_sub(1)
            .ok_or(ApiProtectionRegistryError::UnknownProtectionSlot(slot))?;

        if next_count > 0 {
            let updated = ApiProtectionRegistryEntry {
                count: ApiProtectionCount::new(slot, next_count),
                ..existing
            };
            self.entries.insert(slot, updated);
            return Ok(ApiProtectionPlan {
                action: ApiProtectionAction::Unprotect,
                slot,
                value: existing.protected.value(),
                target: existing.target,
                count: updated.count,
                root_mutation: None,
            });
        }

        self.entries.remove(&slot);
        self.validate_targeted_roots()?;
        Ok(ApiProtectionPlan {
            action: ApiProtectionAction::Unprotect,
            slot,
            value: existing.protected.value(),
            target: existing.target,
            count: ApiProtectionCount::new(slot, 0),
            root_mutation: existing.root.map(ApiProtectionRootMutation::unregister),
        })
    }

    pub fn retarget_slot(
        &mut self,
        protected: ProtectedValue,
        target: ApiProtectionTarget,
    ) -> Result<ApiProtectionPlan, ApiProtectionRegistryError> {
        let slot = protected
            .slot()
            .ok_or(ApiProtectionRegistryError::MissingProtectionSlot)?;
        let existing = self
            .entries
            .get(&slot)
            .copied()
            .ok_or(ApiProtectionRegistryError::UnknownProtectionSlot(slot))?;
        if existing.protected.owner() != protected.owner() {
            return Err(ApiProtectionRegistryError::SlotOwnerMismatch {
                slot,
                existing: existing.protected.owner(),
                requested: protected.owner(),
            });
        }

        let root = self.targeted_root_for(slot, target)?;
        self.validate_targeted_roots_with(slot, root)?;
        let root_mutation = match (existing.root, root) {
            (Some(_), Some(root)) if existing.target != target => {
                Some(ApiProtectionRootMutation::retarget(root))
            }
            (None, Some(root)) => Some(ApiProtectionRootMutation::register(root)),
            (Some(root), None) => Some(ApiProtectionRootMutation::unregister(root)),
            _ => None,
        };
        let updated = ApiProtectionRegistryEntry {
            protected,
            target,
            root,
            ..existing
        };
        self.entries.insert(slot, updated);
        Ok(ApiProtectionPlan {
            action: ApiProtectionAction::Retarget,
            slot,
            value: protected.value(),
            target,
            count: updated.count,
            root_mutation,
        })
    }

    fn targeted_root_for(
        &self,
        slot: ApiProtectionSlot,
        target: ApiProtectionTarget,
    ) -> Result<Option<TargetedRootRecord>, ApiProtectionRegistryError> {
        match target {
            ApiProtectionTarget::Cell { heap, cell } => {
                if heap != self.heap {
                    return Err(ApiProtectionRegistryError::HeapMismatch {
                        slot,
                        expected: self.heap,
                        actual: heap,
                    });
                }
                if cell == CellId::default() {
                    return Err(ApiProtectionRegistryError::DefaultTargetCell { slot });
                }
                Ok(Some(TargetedRootRecord {
                    root: RootRecord {
                        id: api_protection_root_id(slot),
                        kind: RootKind::Handle,
                        heap,
                    },
                    target: cell,
                }))
            }
            ApiProtectionTarget::NoTarget => Ok(None),
        }
    }

    fn validate_targeted_roots(&self) -> Result<(), ApiProtectionRegistryError> {
        TargetedRootSet::from_records(self.heap, self.targeted_roots())?;
        Ok(())
    }

    fn validate_targeted_roots_with(
        &self,
        slot: ApiProtectionSlot,
        replacement: Option<TargetedRootRecord>,
    ) -> Result<(), ApiProtectionRegistryError> {
        let roots = self
            .entries
            .iter()
            .filter_map(|(entry_slot, entry)| {
                if *entry_slot == slot {
                    replacement
                } else {
                    entry.root
                }
            })
            .chain(
                (!self.entries.contains_key(&slot))
                    .then_some(replacement)
                    .into_iter()
                    .flatten(),
            )
            .collect();
        TargetedRootSet::from_records(self.heap, roots)?;
        Ok(())
    }

    fn apply_plan_to_heap(
        &mut self,
        heap: &mut Heap,
        entries_before_plan: BTreeMap<ApiProtectionSlot, ApiProtectionRegistryEntry>,
        plan: ApiProtectionPlan,
    ) -> Result<ApiProtectionOutcome, ApiProtectionRegistryError> {
        match plan.apply_root_mutation_to_heap(heap) {
            Ok(root_mutation) => Ok(ApiProtectionOutcome::new(plan, root_mutation)),
            Err(error) => {
                self.entries = entries_before_plan;
                Err(error.into())
            }
        }
    }
}

fn validate_same_slot_protection(
    slot: ApiProtectionSlot,
    existing: ApiProtectionRegistryEntry,
    requested: ProtectedValue,
    requested_target: ApiProtectionTarget,
) -> Result<(), ApiProtectionRegistryError> {
    if existing.protected.owner() != requested.owner() {
        return Err(ApiProtectionRegistryError::SlotOwnerMismatch {
            slot,
            existing: existing.protected.owner(),
            requested: requested.owner(),
        });
    }
    if existing.protected.value() != requested.value() {
        return Err(ApiProtectionRegistryError::SlotValueMismatch {
            slot,
            existing: existing.protected.value(),
            requested: requested.value(),
        });
    }
    if existing.target != requested_target {
        return Err(ApiProtectionRegistryError::SlotTargetMismatch {
            slot,
            existing: existing.target,
            requested: requested_target,
        });
    }
    Ok(())
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::api::handles::ApiOpaqueHandle;
    use crate::gc::{
        static_cell_metadata_registry, CellType, JsCellHeader, Trace, TraceCell, Tracer,
    };

    use core::ffi::c_void;
    use core::ptr::NonNull;

    struct TestCell {
        header: JsCellHeader,
    }

    impl Trace for TestCell {
        fn trace(&self, _tracer: &mut dyn Tracer) {}
    }

    impl TraceCell for TestCell {
        fn cell_header(&self) -> &JsCellHeader {
            &self.header
        }
    }

    fn opaque_handle(address: usize) -> ApiOpaqueHandle {
        let raw = NonNull::new(address as *mut c_void).unwrap();
        unsafe { ApiOpaqueHandle::from_raw(raw) }
    }

    fn context_group(address: usize) -> ApiContextGroup {
        unsafe { ApiContextGroup::from_opaque(opaque_handle(address)) }
    }

    fn value_ref(address: usize) -> ApiValueRef {
        unsafe { ApiValueRef::from_opaque(opaque_handle(address)) }
    }

    fn owner() -> ApiProtectionOwner {
        ApiProtectionOwner::new(context_group(0x1000))
    }

    fn protected(slot: ApiProtectionSlot) -> ProtectedValue {
        ProtectedValue::with_slot(owner(), value_ref(0x2000), slot)
    }

    fn allocate_heap_cell(heap: &mut Heap) -> CellId {
        let metadata = static_cell_metadata_registry()
            .metadata_for_type(CellType::Object)
            .map(|descriptor| descriptor.metadata)
            .unwrap_or_default();
        let subspace = heap.subspace::<TestCell>("api-protection-test", metadata);
        heap.allocate(heap.allocation_plan(&subspace), 64)
            .map(|response| response.cell)
            .expect("test allocation")
    }

    #[test]
    fn root_plan_bridge_registers_first_protect_and_repeated_protect_is_noop() {
        let mut heap = Heap::new();
        let heap_id = heap.id();
        let target = allocate_heap_cell(&mut heap);
        let slot = ApiProtectionSlot::from_heap_slot(50);
        let target = ApiProtectionTarget::cell(heap_id, target);
        let mut registry = ApiProtectionRegistry::new(heap_id);

        let first = registry.protect(protected(slot), target).unwrap();
        let record = first.root_mutation().unwrap().record();
        assert_eq!(
            first.apply_root_mutation_to_heap(&mut heap),
            Ok(Some(ApiProtectionRootMutationOutcome::Registered(record)))
        );
        assert_eq!(heap.targeted_roots().records(), &[record]);

        let repeated = registry.protect(protected(slot), target).unwrap();
        assert_eq!(repeated.count(), ApiProtectionCount::new(slot, 2));
        assert_eq!(repeated.root_mutation(), None);
        assert_eq!(repeated.apply_root_mutation_to_heap(&mut heap), Ok(None));
        assert_eq!(heap.targeted_roots().records(), &[record]);
    }

    #[test]
    fn root_plan_bridge_unregisters_only_on_final_unprotect() {
        let mut heap = Heap::new();
        let heap_id = heap.id();
        let target = ApiProtectionTarget::cell(heap_id, allocate_heap_cell(&mut heap));
        let slot = ApiProtectionSlot::from_heap_slot(51);
        let mut registry = ApiProtectionRegistry::new(heap_id);
        let first = registry.protect(protected(slot), target).unwrap();
        let record = first.root_mutation().unwrap().record();
        first
            .apply_root_mutation_to_heap(&mut heap)
            .expect("register targeted root");
        registry.protect(protected(slot), target).unwrap();

        let decrement = registry.unprotect(slot).unwrap();
        assert_eq!(decrement.count(), ApiProtectionCount::new(slot, 1));
        assert_eq!(decrement.root_mutation(), None);
        assert_eq!(decrement.apply_root_mutation_to_heap(&mut heap), Ok(None));
        assert_eq!(heap.targeted_roots().records(), &[record]);

        let final_plan = registry.unprotect(slot).unwrap();
        assert_eq!(
            final_plan.apply_root_mutation_to_heap(&mut heap),
            Ok(Some(ApiProtectionRootMutationOutcome::Unregistered(
                record.root.id
            )))
        );
        assert!(heap.targeted_roots().records().is_empty());
    }

    #[test]
    fn root_plan_bridge_leaves_no_target_protection_off_heap() {
        let mut heap = Heap::new();
        let heap_id = heap.id();
        let slot = ApiProtectionSlot::from_heap_slot(52);
        let mut registry = ApiProtectionRegistry::new(heap_id);

        let first = registry
            .protect(protected(slot), ApiProtectionTarget::no_target())
            .unwrap();
        let repeated = registry
            .protect(protected(slot), ApiProtectionTarget::no_target())
            .unwrap();
        assert_eq!(first.count(), ApiProtectionCount::new(slot, 1));
        assert_eq!(repeated.count(), ApiProtectionCount::new(slot, 2));
        assert_eq!(first.apply_root_mutation_to_heap(&mut heap), Ok(None));
        assert_eq!(repeated.apply_root_mutation_to_heap(&mut heap), Ok(None));
        assert!(heap.targeted_roots().records().is_empty());

        registry.unprotect(slot).unwrap();
        let final_plan = registry.unprotect(slot).unwrap();
        assert_eq!(final_plan.count(), ApiProtectionCount::new(slot, 0));
        assert_eq!(final_plan.apply_root_mutation_to_heap(&mut heap), Ok(None));
        assert!(heap.targeted_roots().records().is_empty());
    }

    #[test]
    fn root_plan_bridge_retarget_updates_heap_target() {
        let mut heap = Heap::new();
        let heap_id = heap.id();
        let first_target = allocate_heap_cell(&mut heap);
        let second_target = allocate_heap_cell(&mut heap);
        let slot = ApiProtectionSlot::from_heap_slot(53);
        let mut registry = ApiProtectionRegistry::new(heap_id);
        let first = registry
            .protect(
                protected(slot),
                ApiProtectionTarget::cell(heap_id, first_target),
            )
            .unwrap();
        first
            .apply_root_mutation_to_heap(&mut heap)
            .expect("register targeted root");

        let retarget = registry
            .retarget_slot(
                protected(slot),
                ApiProtectionTarget::cell(heap_id, second_target),
            )
            .unwrap();
        let record = retarget.root_mutation().unwrap().record();
        assert_eq!(
            retarget.apply_root_mutation_to_heap(&mut heap),
            Ok(Some(ApiProtectionRootMutationOutcome::Retargeted(record)))
        );
        assert_eq!(record.target, second_target);
        assert_eq!(heap.targeted_roots().records(), &[record]);
    }

    #[test]
    fn root_plan_bridge_rejects_unknown_target_through_heap() {
        let mut heap = Heap::new();
        let heap_id = heap.id();
        let slot = ApiProtectionSlot::from_heap_slot(54);
        let unknown = CellId(99);
        let mut registry = ApiProtectionRegistry::new(heap_id);
        let plan = registry
            .protect(protected(slot), ApiProtectionTarget::cell(heap_id, unknown))
            .unwrap();

        assert_eq!(
            plan.apply_root_mutation_to_heap(&mut heap),
            Err(HeapIntegrationError::UnknownCell(unknown))
        );
        assert!(heap.targeted_roots().records().is_empty());
    }

    #[test]
    fn root_mutation_bridge_rejects_invalid_records_through_heap() {
        let mut heap = Heap::new();
        let target = allocate_heap_cell(&mut heap);
        let wrong_heap_record = TargetedRootRecord {
            root: RootRecord {
                id: RootId(55),
                kind: RootKind::Handle,
                heap: HeapId(99),
            },
            target,
        };
        let default_target_record = TargetedRootRecord {
            root: RootRecord {
                id: RootId(56),
                kind: RootKind::Handle,
                heap: heap.id(),
            },
            target: CellId::default(),
        };

        assert_eq!(
            ApiProtectionRootMutation::register(wrong_heap_record).apply_to_heap(&mut heap),
            Err(HeapIntegrationError::HeapMismatch {
                expected: heap.id(),
                actual: HeapId(99),
            })
        );
        assert_eq!(
            ApiProtectionRootMutation::register(default_target_record).apply_to_heap(&mut heap),
            Err(HeapIntegrationError::Root(
                RootSetSemanticError::InvalidRootTarget {
                    root: RootId(56),
                    target: CellId::default(),
                }
            ))
        );
        assert!(heap.targeted_roots().records().is_empty());
    }

    #[test]
    fn live_registry_boundary_updates_heap_for_protect_retarget_and_unprotect() {
        let mut heap = Heap::new();
        let heap_id = heap.id();
        let first_target = allocate_heap_cell(&mut heap);
        let second_target = allocate_heap_cell(&mut heap);
        let slot = ApiProtectionSlot::from_heap_slot(55);
        let mut registry = ApiProtectionRegistry::new(heap_id);

        let first = registry
            .protect_value(
                &mut heap,
                protected(slot),
                ApiProtectionTarget::cell(heap_id, first_target),
            )
            .unwrap();
        let first_record = if let Some(ApiProtectionRootMutationOutcome::Registered(record)) =
            first.root_mutation_outcome()
        {
            record
        } else {
            unreachable!("expected first protect to register a targeted root");
        };
        assert_eq!(first.count(), ApiProtectionCount::new(slot, 1));
        assert_eq!(first_record.target, first_target);
        assert_eq!(heap.targeted_roots().records(), &[first_record]);

        let repeated = registry
            .protect_value(
                &mut heap,
                protected(slot),
                ApiProtectionTarget::cell(heap_id, first_target),
            )
            .unwrap();
        assert_eq!(repeated.count(), ApiProtectionCount::new(slot, 2));
        assert_eq!(repeated.root_mutation_outcome(), None);
        assert_eq!(heap.targeted_roots().records(), &[first_record]);

        let retarget = registry
            .retarget_value(
                &mut heap,
                protected(slot),
                ApiProtectionTarget::cell(heap_id, second_target),
            )
            .unwrap();
        let second_record = if let Some(ApiProtectionRootMutationOutcome::Retargeted(record)) =
            retarget.root_mutation_outcome()
        {
            record
        } else {
            unreachable!("expected retarget to update the targeted root");
        };
        assert_eq!(retarget.count(), ApiProtectionCount::new(slot, 2));
        assert_eq!(second_record.root, first_record.root);
        assert_eq!(second_record.target, second_target);
        assert_eq!(heap.targeted_roots().records(), &[second_record]);
        assert_eq!(
            registry.entry(slot).unwrap().target(),
            ApiProtectionTarget::cell(heap_id, second_target)
        );

        let decrement = registry.unprotect_value(&mut heap, slot).unwrap();
        assert_eq!(decrement.count(), ApiProtectionCount::new(slot, 1));
        assert_eq!(decrement.root_mutation_outcome(), None);
        assert_eq!(heap.targeted_roots().records(), &[second_record]);

        let final_unprotect = registry.unprotect_value(&mut heap, slot).unwrap();
        assert_eq!(final_unprotect.count(), ApiProtectionCount::new(slot, 0));
        assert_eq!(
            final_unprotect.root_mutation_outcome(),
            Some(ApiProtectionRootMutationOutcome::Unregistered(
                second_record.root.id
            ))
        );
        assert!(registry.is_empty());
        assert!(heap.targeted_roots().records().is_empty());
    }

    #[test]
    fn live_registry_boundary_rolls_back_failed_unknown_target_mutations() {
        let mut heap = Heap::new();
        let heap_id = heap.id();
        let first_target = allocate_heap_cell(&mut heap);
        let unknown = CellId(99);
        let slot = ApiProtectionSlot::from_heap_slot(56);
        let mut registry = ApiProtectionRegistry::new(heap_id);

        assert_eq!(
            registry.protect_value(
                &mut heap,
                protected(slot),
                ApiProtectionTarget::cell(heap_id, unknown)
            ),
            Err(ApiProtectionRegistryError::Heap(
                HeapIntegrationError::UnknownCell(unknown)
            ))
        );
        assert!(registry.is_empty());
        assert!(heap.targeted_roots().records().is_empty());

        let first = registry
            .protect_value(
                &mut heap,
                protected(slot),
                ApiProtectionTarget::cell(heap_id, first_target),
            )
            .unwrap();
        let first_record = if let Some(ApiProtectionRootMutationOutcome::Registered(record)) =
            first.root_mutation_outcome()
        {
            record
        } else {
            unreachable!("expected first protect to register a targeted root");
        };

        assert_eq!(
            registry.retarget_value(
                &mut heap,
                protected(slot),
                ApiProtectionTarget::cell(heap_id, unknown)
            ),
            Err(ApiProtectionRegistryError::Heap(
                HeapIntegrationError::UnknownCell(unknown)
            ))
        );
        assert_eq!(heap.targeted_roots().records(), &[first_record]);
        assert_eq!(
            registry.entry(slot).unwrap().target(),
            ApiProtectionTarget::cell(heap_id, first_target)
        );
    }

    #[test]
    fn live_registry_boundary_keeps_rejected_and_no_target_values_rootless() {
        let mut heap = Heap::new();
        let heap_id = heap.id();
        let slot = ApiProtectionSlot::from_heap_slot(57);
        let mut registry = ApiProtectionRegistry::new(heap_id);

        assert_eq!(
            registry.protect_value(
                &mut heap,
                protected(slot),
                ApiProtectionTarget::cell(heap_id, CellId::default())
            ),
            Err(ApiProtectionRegistryError::DefaultTargetCell { slot })
        );
        assert!(registry.is_empty());
        assert!(heap.targeted_roots().records().is_empty());

        let first = registry
            .protect_value(&mut heap, protected(slot), ApiProtectionTarget::no_target())
            .unwrap();
        let repeated = registry
            .protect_value(&mut heap, protected(slot), ApiProtectionTarget::no_target())
            .unwrap();
        assert_eq!(first.count(), ApiProtectionCount::new(slot, 1));
        assert_eq!(repeated.count(), ApiProtectionCount::new(slot, 2));
        assert_eq!(first.root_mutation_outcome(), None);
        assert_eq!(repeated.root_mutation_outcome(), None);
        assert!(heap.targeted_roots().records().is_empty());

        registry.unprotect_value(&mut heap, slot).unwrap();
        registry.unprotect_value(&mut heap, slot).unwrap();
        assert_eq!(
            registry.unprotect_value(&mut heap, slot),
            Err(ApiProtectionRegistryError::UnknownProtectionSlot(slot))
        );
        assert!(heap.targeted_roots().records().is_empty());
    }

    #[test]
    fn live_registry_boundary_rejects_wrong_heap_through_heap_application() {
        let mut heap = Heap::new();
        let actual_heap = heap.id();
        let registry_heap = HeapId(7);
        let target = allocate_heap_cell(&mut heap);
        let slot = ApiProtectionSlot::from_heap_slot(58);
        let mut registry = ApiProtectionRegistry::new(registry_heap);

        assert_eq!(
            registry.protect_value(
                &mut heap,
                protected(slot),
                ApiProtectionTarget::cell(registry_heap, target)
            ),
            Err(ApiProtectionRegistryError::Heap(
                HeapIntegrationError::HeapMismatch {
                    expected: actual_heap,
                    actual: registry_heap,
                }
            ))
        );
        assert!(registry.is_empty());
        assert!(heap.targeted_roots().records().is_empty());
    }

    #[test]
    fn first_protect_creates_handle_targeted_root_plan() {
        let heap = HeapId(7);
        let slot = ApiProtectionSlot::from_heap_slot(41);
        let target = CellId(99);
        let mut registry = ApiProtectionRegistry::new(heap);

        let plan = registry
            .protect(protected(slot), ApiProtectionTarget::cell(heap, target))
            .unwrap();

        assert_eq!(plan.action(), ApiProtectionAction::Protect);
        assert_eq!(plan.count(), ApiProtectionCount::new(slot, 1));
        let mutation = plan.root_mutation().unwrap();
        assert_eq!(mutation.kind(), RootSetMutationKind::Register);
        assert_eq!(mutation.authority(), RootSetMutationAuthority::HandleScope);
        assert_eq!(
            mutation.root_set_mutation(),
            RootSetMutation::register(
                mutation.record().root,
                RootSetMutationAuthority::HandleScope
            )
        );
        assert_eq!(
            mutation.record(),
            TargetedRootRecord {
                root: RootRecord {
                    id: api_protection_root_id(slot),
                    kind: RootKind::Handle,
                    heap,
                },
                target,
            }
        );
        assert_eq!(registry.targeted_roots(), vec![mutation.record()]);
    }

    #[test]
    fn repeated_protect_increments_count_without_duplicate_root() {
        let heap = HeapId(7);
        let slot = ApiProtectionSlot::from_heap_slot(4);
        let target = ApiProtectionTarget::cell(heap, CellId(12));
        let mut registry = ApiProtectionRegistry::new(heap);

        let first = registry.protect(protected(slot), target).unwrap();
        let second = registry.protect(protected(slot), target).unwrap();

        assert!(first.root_mutation().is_some());
        assert_eq!(second.count(), ApiProtectionCount::new(slot, 2));
        assert_eq!(second.root_mutation(), None);
        assert_eq!(registry.targeted_roots().len(), 1);
        assert_eq!(registry.entry(slot).unwrap().count().count(), 2);
    }

    #[test]
    fn final_unprotect_unregisters_and_over_unprotect_rejects() {
        let heap = HeapId(7);
        let slot = ApiProtectionSlot::from_heap_slot(5);
        let target = ApiProtectionTarget::cell(heap, CellId(13));
        let mut registry = ApiProtectionRegistry::new(heap);
        let registered = registry
            .protect(protected(slot), target)
            .unwrap()
            .root_mutation()
            .unwrap()
            .record();
        registry.protect(protected(slot), target).unwrap();

        let decremented = registry.unprotect(slot).unwrap();
        assert_eq!(decremented.count(), ApiProtectionCount::new(slot, 1));
        assert_eq!(decremented.root_mutation(), None);
        assert_eq!(registry.targeted_roots(), vec![registered]);

        let final_plan = registry.unprotect(slot).unwrap();
        assert_eq!(final_plan.count(), ApiProtectionCount::new(slot, 0));
        let mutation = final_plan.root_mutation().unwrap();
        assert_eq!(mutation.kind(), RootSetMutationKind::Unregister);
        assert_eq!(mutation.record(), registered);
        assert!(registry.is_empty());
        assert_eq!(
            registry.unprotect(slot),
            Err(ApiProtectionRegistryError::UnknownProtectionSlot(slot))
        );
    }

    #[test]
    fn default_target_cell_is_rejected_for_targeted_protection() {
        let heap = HeapId(7);
        let slot = ApiProtectionSlot::from_heap_slot(6);
        let mut registry = ApiProtectionRegistry::new(heap);

        assert_eq!(
            registry.protect(
                protected(slot),
                ApiProtectionTarget::cell(heap, CellId::default())
            ),
            Err(ApiProtectionRegistryError::DefaultTargetCell { slot })
        );
        assert!(registry.is_empty());
    }

    #[test]
    fn wrong_heap_is_rejected_for_targeted_protection() {
        let heap = HeapId(7);
        let slot = ApiProtectionSlot::from_heap_slot(7);
        let mut registry = ApiProtectionRegistry::new(heap);

        assert_eq!(
            registry.protect(
                protected(slot),
                ApiProtectionTarget::cell(HeapId(8), CellId(14))
            ),
            Err(ApiProtectionRegistryError::HeapMismatch {
                slot,
                expected: heap,
                actual: HeapId(8),
            })
        );
        assert!(registry.is_empty());
    }

    #[test]
    fn no_target_protection_is_counted_without_targeted_root() {
        let heap = HeapId(7);
        let slot = ApiProtectionSlot::from_heap_slot(8);
        let mut registry = ApiProtectionRegistry::new(heap);

        let first = registry
            .protect(protected(slot), ApiProtectionTarget::no_target())
            .unwrap();
        let second = registry
            .protect(protected(slot), ApiProtectionTarget::no_target())
            .unwrap();
        assert_eq!(first.count(), ApiProtectionCount::new(slot, 1));
        assert_eq!(first.root_mutation(), None);
        assert_eq!(second.count(), ApiProtectionCount::new(slot, 2));
        assert_eq!(registry.targeted_roots(), Vec::new());

        registry.unprotect(slot).unwrap();
        let final_plan = registry.unprotect(slot).unwrap();
        assert_eq!(final_plan.count(), ApiProtectionCount::new(slot, 0));
        assert_eq!(final_plan.root_mutation(), None);
        assert!(registry.is_empty());
    }

    #[test]
    fn retargeting_same_slot_must_be_explicit_and_plans_retarget_root() {
        let heap = HeapId(7);
        let slot = ApiProtectionSlot::from_heap_slot(9);
        let mut registry = ApiProtectionRegistry::new(heap);
        registry
            .protect(protected(slot), ApiProtectionTarget::cell(heap, CellId(15)))
            .unwrap();

        assert_eq!(
            registry.protect(protected(slot), ApiProtectionTarget::cell(heap, CellId(16))),
            Err(ApiProtectionRegistryError::SlotTargetMismatch {
                slot,
                existing: ApiProtectionTarget::cell(heap, CellId(15)),
                requested: ApiProtectionTarget::cell(heap, CellId(16)),
            })
        );

        let plan = registry
            .retarget_slot(protected(slot), ApiProtectionTarget::cell(heap, CellId(16)))
            .unwrap();
        assert_eq!(plan.action(), ApiProtectionAction::Retarget);
        assert_eq!(plan.count(), ApiProtectionCount::new(slot, 1));
        let mutation = plan.root_mutation().unwrap();
        assert_eq!(mutation.kind(), RootSetMutationKind::Retarget);
        assert_eq!(
            mutation.record(),
            TargetedRootRecord {
                root: RootRecord {
                    id: api_protection_root_id(slot),
                    kind: RootKind::Handle,
                    heap,
                },
                target: CellId(16),
            }
        );
        assert_eq!(registry.targeted_roots(), vec![mutation.record()]);
    }
}
