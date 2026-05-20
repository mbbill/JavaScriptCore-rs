//! Weak block, weak set, and finalization descriptors.
//!
//! Weak clearing, weak owner callbacks, and finalizers are collector decisions.
//! This module names the states and records that future GC code will consume.

use crate::gc::{CellId, FinalizerId, GcRef, HeapId, JsCell, WeakId, WeakProcessingPhase};

/// Opaque weak-set identity.
///
/// Weak sets are registry containers owned by weak processing. They do not
/// identify target cells.
#[derive(Clone, Copy, Debug, Default, Eq, Hash, PartialEq)]
#[repr(transparent)]
pub struct WeakSetId(pub u64);

/// Opaque weak-block identity.
///
/// Weak blocks contain weak slots; their IDs name storage for weak metadata,
/// not heap cells.
#[derive(Clone, Copy, Debug, Default, Eq, Hash, PartialEq)]
#[repr(transparent)]
pub struct WeakBlockId(pub u64);

/// Opaque weak owner identity. The owner supplies clearing/finalize policy.
///
/// The owner ID names a callback policy provider. It is separate from both
/// target cell identity and weak-slot identity.
#[derive(Clone, Copy, Debug, Default, Eq, Hash, PartialEq)]
#[repr(transparent)]
pub struct WeakHandleOwnerId(pub u64);

/// Untyped weak-owner context supplied by VM/host code.
///
/// This tag is caller-owned context. The GC may pass it back to the weak owner
/// but must not interpret it as a pointer or cell identity.
#[derive(Clone, Copy, Debug, Default, Eq, Hash, PartialEq)]
#[repr(transparent)]
pub struct WeakContextTag(pub usize);

/// Callback authority exposed by `WeakHandleOwner`.
#[derive(Clone, Copy, Debug, Default, Eq, PartialEq)]
pub enum WeakOwnerAuthority {
    /// Validate reachability through opaque roots during weak visiting.
    #[default]
    ReachabilityFromOpaqueRoots,
    /// Run owner-specific finalization after weak liveness is resolved.
    Finalize,
}

/// Contract for a weak owner callback table.
#[derive(Clone, Copy, Debug, Default, Eq, PartialEq)]
pub struct WeakHandleOwnerContract {
    pub id: WeakHandleOwnerId,
    pub can_validate_opaque_roots: bool,
    pub can_finalize: bool,
    pub authority: WeakOwnerAuthority,
}

impl WeakHandleOwnerContract {
    pub const fn can_validate_ephemeron_roots(&self) -> bool {
        if !self.can_validate_opaque_roots {
            return false;
        }
        matches!(
            self.authority,
            WeakOwnerAuthority::ReachabilityFromOpaqueRoots
        )
    }
}

/// Weak slot state during weak processing.
#[derive(Clone, Copy, Debug, Default, Eq, PartialEq)]
pub enum WeakSlotState {
    #[default]
    Live,
    Dead,
    Finalized,
    Deallocated,
    ClearPending,
}

#[derive(Clone, Copy, Debug, Eq, PartialEq)]
pub struct WeakSlotTransitionRequest {
    pub state: WeakSlotState,
    pub phase: WeakProcessingPhase,
    pub kind: WeakEdgeKind,
    pub target_is_live: bool,
    pub owner: Option<WeakHandleOwnerContract>,
    pub ephemeron_value_is_retained: Option<bool>,
}

impl WeakSlotTransitionRequest {
    pub const fn new(
        state: WeakSlotState,
        phase: WeakProcessingPhase,
        kind: WeakEdgeKind,
        target_is_live: bool,
    ) -> Self {
        Self {
            state,
            phase,
            kind,
            target_is_live,
            owner: None,
            ephemeron_value_is_retained: None,
        }
    }

    pub const fn owner(mut self, owner: WeakHandleOwnerContract) -> Self {
        self.owner = Some(owner);
        self
    }

    pub const fn ephemeron_value_is_retained(mut self, retained: bool) -> Self {
        self.ephemeron_value_is_retained = Some(retained);
        self
    }
}

#[derive(Clone, Copy, Debug, Eq, PartialEq)]
pub struct WeakSlotTransitionOutcome {
    pub from: WeakSlotState,
    pub to: WeakSlotState,
    pub clears_target: bool,
    pub requires_owner_callback: bool,
}

#[derive(Clone, Copy, Debug, Eq, PartialEq)]
pub enum WeakStateTransitionError {
    DeallocatedSlotIsTerminal,
    ClearPendingRequiresClearPhase,
    DeadSlotCannotBecomeLive,
    MissingEphemeronValuePolicy,
    MissingOpaqueRootValidation(WeakHandleOwnerId),
    MissingFinalizationAuthority(WeakHandleOwnerId),
}

impl WeakSlotState {
    pub fn transition(
        request: WeakSlotTransitionRequest,
    ) -> Result<WeakSlotTransitionOutcome, WeakStateTransitionError> {
        if request.state == WeakSlotState::Deallocated {
            return Err(WeakStateTransitionError::DeallocatedSlotIsTerminal);
        }

        if request.state == WeakSlotState::ClearPending
            && request.phase != WeakProcessingPhase::Clear
        {
            return Err(WeakStateTransitionError::ClearPendingRequiresClearPhase);
        }

        if matches!(
            request.kind,
            WeakEdgeKind::EphemeronKey | WeakEdgeKind::EphemeronValue
        ) && request.phase == WeakProcessingPhase::Validate
        {
            match request.owner {
                Some(owner) => {
                    if !owner.can_validate_ephemeron_roots() {
                        return Err(WeakStateTransitionError::MissingOpaqueRootValidation(
                            owner.id,
                        ));
                    }
                }
                None => {
                    return Err(WeakStateTransitionError::MissingOpaqueRootValidation(
                        WeakHandleOwnerId::default(),
                    ));
                }
            }
        }

        let target_is_live = if request.kind == WeakEdgeKind::EphemeronValue
            && request.phase == WeakProcessingPhase::Validate
        {
            request
                .ephemeron_value_is_retained
                .ok_or(WeakStateTransitionError::MissingEphemeronValuePolicy)?
        } else {
            request.target_is_live
        };

        if request.state == WeakSlotState::Dead && target_is_live {
            return Err(WeakStateTransitionError::DeadSlotCannotBecomeLive);
        }

        let to = match (request.state, request.phase, target_is_live) {
            (WeakSlotState::Live, WeakProcessingPhase::Discover, _) => WeakSlotState::Live,
            (WeakSlotState::Live, WeakProcessingPhase::Validate, true) => WeakSlotState::Live,
            (WeakSlotState::Live, WeakProcessingPhase::Validate, false) => {
                WeakSlotState::ClearPending
            }
            (WeakSlotState::Live, WeakProcessingPhase::Clear, true) => WeakSlotState::Live,
            (WeakSlotState::Live, WeakProcessingPhase::Clear, false) => WeakSlotState::ClearPending,
            (WeakSlotState::ClearPending, WeakProcessingPhase::Clear, _) => WeakSlotState::Dead,
            (WeakSlotState::Dead, WeakProcessingPhase::Clear, false) => {
                if request
                    .owner
                    .map(|owner| owner.can_finalize)
                    .unwrap_or(false)
                {
                    WeakSlotState::Finalized
                } else {
                    WeakSlotState::Dead
                }
            }
            (WeakSlotState::Finalized, WeakProcessingPhase::Clear, false) => {
                WeakSlotState::Deallocated
            }
            (state, _, _) => state,
        };

        if to == WeakSlotState::Finalized {
            if let Some(owner) = request.owner {
                if !owner.can_finalize {
                    return Err(WeakStateTransitionError::MissingFinalizationAuthority(
                        owner.id,
                    ));
                }
            } else {
                return Err(WeakStateTransitionError::MissingFinalizationAuthority(
                    WeakHandleOwnerId::default(),
                ));
            }
        }

        Ok(WeakSlotTransitionOutcome {
            from: request.state,
            to,
            clears_target: matches!(
                (request.state, to),
                (WeakSlotState::ClearPending, WeakSlotState::Dead)
            ),
            requires_owner_callback: to == WeakSlotState::Finalized,
        })
    }
}

/// Weak edge category used by validation.
#[derive(Clone, Copy, Debug, Default, Eq, PartialEq)]
pub enum WeakEdgeKind {
    #[default]
    Ordinary,
    EphemeronKey,
    EphemeronValue,
    FinalizerCell,
}

/// One weak slot descriptor.
///
/// The slot record owns weak metadata for `id`. The `target` is a borrowed
/// optional cell reference whose clearing authority belongs to weak processing.
#[derive(Clone, Copy, Debug)]
pub struct WeakSlotRecord {
    pub id: WeakId,
    pub set: WeakSetId,
    pub owner: Option<WeakHandleOwnerId>,
    pub context: Option<WeakContextTag>,
    pub target: Option<GcRef<JsCell>>,
    pub kind: WeakEdgeKind,
    pub state: WeakSlotState,
}

#[derive(Clone, Copy, Debug, Default, Eq, PartialEq)]
pub enum WeakRootPolicyAction {
    #[default]
    Clear,
    Retain,
}

#[derive(Clone, Copy, Debug, Default, Eq, PartialEq)]
pub enum WeakRootPolicyReason {
    #[default]
    TargetDead,
    TargetMarkedLive,
    EphemeronKeyDead,
    EphemeronKeyMarkedLive,
    EphemeronKeyValidatedByOwnerOpaqueRoot,
}

#[derive(Clone, Copy, Debug, Eq, PartialEq)]
pub struct WeakRootPolicyDescriptor {
    pub weak: WeakId,
    pub kind: WeakEdgeKind,
    pub owner: Option<WeakHandleOwnerContract>,
    pub target: Option<CellId>,
    pub target_is_live: bool,
    pub ephemeron_key: Option<WeakId>,
    pub owner_validates_opaque_root: bool,
}

impl WeakRootPolicyDescriptor {
    pub const fn ordinary(weak: WeakId, target: Option<CellId>, target_is_live: bool) -> Self {
        Self {
            weak,
            kind: WeakEdgeKind::Ordinary,
            owner: None,
            target,
            target_is_live,
            ephemeron_key: None,
            owner_validates_opaque_root: false,
        }
    }

    pub const fn ephemeron_key(
        weak: WeakId,
        owner: WeakHandleOwnerContract,
        target: Option<CellId>,
        target_is_live: bool,
        owner_validates_opaque_root: bool,
    ) -> Self {
        Self {
            weak,
            kind: WeakEdgeKind::EphemeronKey,
            owner: Some(owner),
            target,
            target_is_live,
            ephemeron_key: None,
            owner_validates_opaque_root,
        }
    }

    pub const fn ephemeron_value(
        weak: WeakId,
        owner: WeakHandleOwnerContract,
        target: Option<CellId>,
        ephemeron_key: WeakId,
    ) -> Self {
        Self {
            weak,
            kind: WeakEdgeKind::EphemeronValue,
            owner: Some(owner),
            target,
            target_is_live: false,
            ephemeron_key: Some(ephemeron_key),
            owner_validates_opaque_root: false,
        }
    }

    pub const fn owner_id(&self) -> Option<WeakHandleOwnerId> {
        match self.owner {
            Some(owner) => Some(owner.id),
            None => None,
        }
    }
}

#[derive(Clone, Debug, Default, Eq, PartialEq)]
pub struct WeakRootPolicyPlan {
    pub entries: Vec<WeakRootPolicyPlanEntry>,
}

impl WeakRootPolicyPlan {
    pub fn from_descriptors(
        descriptors: &[WeakRootPolicyDescriptor],
    ) -> Result<Self, WeakRootPolicyError> {
        for (index, descriptor) in descriptors.iter().enumerate() {
            if descriptors[..index]
                .iter()
                .any(|previous| previous.weak == descriptor.weak)
            {
                return Err(WeakRootPolicyError::DuplicateSlot(descriptor.weak));
            }
        }

        let mut entries = Vec::with_capacity(descriptors.len());
        for descriptor in descriptors {
            entries.push(policy_entry_for_descriptor(descriptor, descriptors)?);
        }

        Ok(Self { entries })
    }

    pub fn entry_for(&self, weak: WeakId) -> Option<&WeakRootPolicyPlanEntry> {
        self.entries.iter().find(|entry| entry.weak == weak)
    }
}

#[derive(Clone, Copy, Debug, Eq, PartialEq)]
pub struct WeakRootPolicyPlanEntry {
    pub weak: WeakId,
    pub kind: WeakEdgeKind,
    pub owner: Option<WeakHandleOwnerId>,
    pub target: Option<CellId>,
    pub ephemeron_key: Option<WeakId>,
    pub action: WeakRootPolicyAction,
    pub reason: WeakRootPolicyReason,
}

impl WeakRootPolicyPlanEntry {
    pub const fn retains_target(&self) -> bool {
        matches!(self.action, WeakRootPolicyAction::Retain)
    }

    pub const fn is_clearable(&self) -> bool {
        matches!(self.action, WeakRootPolicyAction::Clear)
    }
}

#[derive(Clone, Copy, Debug, Eq, PartialEq)]
pub enum WeakRootPolicyError {
    DuplicateSlot(WeakId),
    MissingEphemeronOwner(WeakId),
    MissingOpaqueRootValidation {
        weak: WeakId,
        owner: WeakHandleOwnerId,
    },
    MissingEphemeronKey(WeakId),
    UnknownEphemeronKey {
        value: WeakId,
        key: WeakId,
    },
    EphemeronKeyKindMismatch {
        value: WeakId,
        key: WeakId,
        actual: WeakEdgeKind,
    },
    EphemeronOwnerMismatch {
        value: WeakId,
        key: WeakId,
        expected: WeakHandleOwnerId,
        actual: WeakHandleOwnerId,
    },
}

fn policy_entry_for_descriptor(
    descriptor: &WeakRootPolicyDescriptor,
    descriptors: &[WeakRootPolicyDescriptor],
) -> Result<WeakRootPolicyPlanEntry, WeakRootPolicyError> {
    match descriptor.kind {
        WeakEdgeKind::Ordinary | WeakEdgeKind::FinalizerCell => {
            let (action, reason) = ordinary_policy_action(descriptor);
            Ok(WeakRootPolicyPlanEntry {
                weak: descriptor.weak,
                kind: descriptor.kind,
                owner: descriptor.owner_id(),
                target: descriptor.target,
                ephemeron_key: None,
                action,
                reason,
            })
        }
        WeakEdgeKind::EphemeronKey => {
            validate_ephemeron_policy_owner(descriptor)?;
            let (action, reason) = ephemeron_key_policy_action(descriptor);
            Ok(WeakRootPolicyPlanEntry {
                weak: descriptor.weak,
                kind: descriptor.kind,
                owner: descriptor.owner_id(),
                target: descriptor.target,
                ephemeron_key: None,
                action,
                reason,
            })
        }
        WeakEdgeKind::EphemeronValue => {
            let value_owner = validate_ephemeron_policy_owner(descriptor)?;
            let key_id = descriptor
                .ephemeron_key
                .ok_or(WeakRootPolicyError::MissingEphemeronKey(descriptor.weak))?;
            let key = descriptors
                .iter()
                .find(|candidate| candidate.weak == key_id)
                .ok_or(WeakRootPolicyError::UnknownEphemeronKey {
                    value: descriptor.weak,
                    key: key_id,
                })?;
            if key.kind != WeakEdgeKind::EphemeronKey {
                return Err(WeakRootPolicyError::EphemeronKeyKindMismatch {
                    value: descriptor.weak,
                    key: key_id,
                    actual: key.kind,
                });
            }

            let key_owner = validate_ephemeron_policy_owner(key)?;
            if key_owner != value_owner {
                return Err(WeakRootPolicyError::EphemeronOwnerMismatch {
                    value: descriptor.weak,
                    key: key_id,
                    expected: key_owner,
                    actual: value_owner,
                });
            }

            let (key_action, key_reason) = ephemeron_key_policy_action(key);
            let action =
                if descriptor.target.is_some() && key_action == WeakRootPolicyAction::Retain {
                    WeakRootPolicyAction::Retain
                } else {
                    WeakRootPolicyAction::Clear
                };
            let reason = if descriptor.target.is_some() {
                key_reason
            } else {
                WeakRootPolicyReason::TargetDead
            };

            Ok(WeakRootPolicyPlanEntry {
                weak: descriptor.weak,
                kind: descriptor.kind,
                owner: descriptor.owner_id(),
                target: descriptor.target,
                ephemeron_key: Some(key_id),
                action,
                reason,
            })
        }
    }
}

fn ordinary_policy_action(
    descriptor: &WeakRootPolicyDescriptor,
) -> (WeakRootPolicyAction, WeakRootPolicyReason) {
    if descriptor.target.is_some() && descriptor.target_is_live {
        (
            WeakRootPolicyAction::Retain,
            WeakRootPolicyReason::TargetMarkedLive,
        )
    } else {
        (
            WeakRootPolicyAction::Clear,
            WeakRootPolicyReason::TargetDead,
        )
    }
}

fn ephemeron_key_policy_action(
    descriptor: &WeakRootPolicyDescriptor,
) -> (WeakRootPolicyAction, WeakRootPolicyReason) {
    if descriptor.target.is_none() {
        return (
            WeakRootPolicyAction::Clear,
            WeakRootPolicyReason::EphemeronKeyDead,
        );
    }

    if descriptor.target_is_live {
        (
            WeakRootPolicyAction::Retain,
            WeakRootPolicyReason::EphemeronKeyMarkedLive,
        )
    } else if descriptor.owner_validates_opaque_root {
        (
            WeakRootPolicyAction::Retain,
            WeakRootPolicyReason::EphemeronKeyValidatedByOwnerOpaqueRoot,
        )
    } else {
        (
            WeakRootPolicyAction::Clear,
            WeakRootPolicyReason::EphemeronKeyDead,
        )
    }
}

fn validate_ephemeron_policy_owner(
    descriptor: &WeakRootPolicyDescriptor,
) -> Result<WeakHandleOwnerId, WeakRootPolicyError> {
    let owner = descriptor
        .owner
        .ok_or(WeakRootPolicyError::MissingEphemeronOwner(descriptor.weak))?;
    if !owner.can_validate_ephemeron_roots() {
        return Err(WeakRootPolicyError::MissingOpaqueRootValidation {
            weak: descriptor.weak,
            owner: owner.id,
        });
    }

    Ok(owner.id)
}

/// Result staged by sweeping a weak block.
#[derive(Clone, Copy, Debug, Default, Eq, PartialEq)]
pub struct WeakSweepResult {
    pub block_is_free: bool,
    pub block_is_logically_empty: bool,
    pub free_slot_count: usize,
}

/// Descriptor for a compact block of weak slots.
///
/// Blocks own weak-slot storage. Sweep results are staged collector output and
/// do not grant callers authority to free target cells.
#[derive(Clone, Debug, Default, Eq, PartialEq)]
pub struct WeakBlockDescriptor {
    pub id: WeakBlockId,
    pub set: WeakSetId,
    pub slot_capacity: usize,
    pub live_slot_count: usize,
    pub allocator_is_attached: bool,
    pub logically_empty_but_not_free: bool,
    pub sweep_result: Option<WeakSweepResult>,
}

/// Descriptor for a weak set owned by a block or precise allocation.
///
/// The set owns grouping metadata for weak slots attached to heap containers.
/// Its block links are topology metadata, separate from target cell identity.
#[derive(Clone, Debug, Default, Eq, PartialEq)]
pub struct WeakSetDescriptor {
    pub id: WeakSetId,
    pub heap: HeapId,
    pub blocks: Vec<WeakBlockId>,
    pub allocator_block: Option<WeakBlockId>,
    pub next_allocator_block: Option<WeakBlockId>,
    pub active_phase: Option<WeakProcessingPhase>,
}

/// Finalizer family.
#[derive(Clone, Copy, Debug, Default, Eq, PartialEq)]
pub enum FinalizerKind {
    #[default]
    CellDestructor,
    CCallback,
    HostCallback,
    Unconditional,
}

#[derive(Clone, Copy, Debug, Default, Eq, PartialEq)]
pub enum FinalizerState {
    #[default]
    Registered,
    Ready,
    Running,
    Finished,
    Cancelled,
}

#[derive(Clone, Copy, Debug, Eq, PartialEq)]
pub struct FinalizerTransitionRequest {
    pub state: FinalizerState,
    pub target_is_live: bool,
    pub phase: crate::gc::GcPhase,
    pub callback_registered: bool,
    pub kind: FinalizerKind,
}

#[derive(Clone, Copy, Debug, Eq, PartialEq)]
pub struct FinalizerTransitionOutcome {
    pub from: FinalizerState,
    pub to: FinalizerState,
    pub invokes_callback: bool,
}

#[derive(Clone, Copy, Debug, Eq, PartialEq)]
pub enum FinalizerStateTransitionError {
    MissingCallback,
    WrongPhase(crate::gc::GcPhase),
    FinishedStateIsTerminal,
    CancelledStateIsTerminal,
}

impl FinalizerState {
    pub fn transition(
        request: FinalizerTransitionRequest,
    ) -> Result<FinalizerTransitionOutcome, FinalizerStateTransitionError> {
        if request.state == FinalizerState::Finished {
            return Err(FinalizerStateTransitionError::FinishedStateIsTerminal);
        }
        if request.state == FinalizerState::Cancelled {
            return Err(FinalizerStateTransitionError::CancelledStateIsTerminal);
        }
        if !request.callback_registered {
            return Err(FinalizerStateTransitionError::MissingCallback);
        }
        if request.phase != crate::gc::GcPhase::End {
            return Err(FinalizerStateTransitionError::WrongPhase(request.phase));
        }

        let to = match (request.state, request.target_is_live, request.kind) {
            (FinalizerState::Registered, true, FinalizerKind::Unconditional) => {
                FinalizerState::Ready
            }
            (FinalizerState::Registered, true, _) => FinalizerState::Registered,
            (FinalizerState::Registered, false, _) => FinalizerState::Ready,
            (FinalizerState::Ready, _, _) => FinalizerState::Running,
            (FinalizerState::Running, _, _) => FinalizerState::Finished,
            (state, _, _) => state,
        };

        Ok(FinalizerTransitionOutcome {
            from: request.state,
            to,
            invokes_callback: request.state == FinalizerState::Ready
                && to == FinalizerState::Running,
        })
    }
}

/// Opaque callback registered with the heap.
///
/// Callback IDs name finalizer functions. They are not finalizable object IDs.
#[derive(Clone, Copy, Debug, Default, Eq, Hash, PartialEq)]
#[repr(transparent)]
pub struct HeapFinalizerCallbackId(pub u64);

/// Cell-level finalizer record.
///
/// The record borrows a target cell selected by the collector. Running the
/// callback requires finalization authority from the heap's end phase.
#[derive(Clone, Copy, Debug)]
pub struct FinalizerRecord {
    pub callback: HeapFinalizerCallbackId,
    pub target: GcRef<JsCell>,
    pub owner: Option<WeakHandleOwnerId>,
    pub kind: FinalizerKind,
}

/// Heap shutdown and end-phase callback descriptor.
///
/// The descriptor owns callback metadata only. User data is opaque embedder
/// context and must not be interpreted by `gc`.
#[derive(Clone, Copy, Debug, Default, Eq, PartialEq)]
pub struct HeapFinalizerCallback {
    pub id: HeapFinalizerCallbackId,
    pub kind: FinalizerKind,
    pub user_data_tag: usize,
}

/// Descriptor-only action selected for one weak block.
#[derive(Clone, Copy, Debug, Eq, PartialEq)]
pub enum WeakBlockPlanAction {
    DiscoverSlots,
    ValidateSlots,
    ClearDeadSlots,
    RetireEmptyBlock,
    ReleaseFreeBlock,
}

/// Pure weak-processing plan over weak-set and block descriptors.
#[derive(Clone, Debug, Eq, PartialEq)]
pub struct WeakProcessingPlan {
    pub set: WeakSetId,
    pub phase: WeakProcessingPhase,
    pub block_actions: Vec<WeakBlockPlanEntry>,
}

impl WeakProcessingPlan {
    pub fn from_descriptors(
        set: &WeakSetDescriptor,
        blocks: &[WeakBlockDescriptor],
        phase: WeakProcessingPhase,
    ) -> Result<Self, WeakPlanningError> {
        let mut block_actions = Vec::with_capacity(set.blocks.len());
        for block_id in &set.blocks {
            let block = blocks
                .iter()
                .find(|block| block.id == *block_id)
                .ok_or(WeakPlanningError::UnknownBlock(*block_id))?;
            if block.set != set.id {
                return Err(WeakPlanningError::BlockSetMismatch {
                    block: block.id,
                    expected: set.id,
                    actual: block.set,
                });
            }

            let action = match phase {
                WeakProcessingPhase::Discover => WeakBlockPlanAction::DiscoverSlots,
                WeakProcessingPhase::Validate => WeakBlockPlanAction::ValidateSlots,
                WeakProcessingPhase::Clear => {
                    if block
                        .sweep_result
                        .map(|result| result.block_is_free)
                        .unwrap_or(false)
                    {
                        WeakBlockPlanAction::ReleaseFreeBlock
                    } else if block.live_slot_count == 0 || block.logically_empty_but_not_free {
                        WeakBlockPlanAction::RetireEmptyBlock
                    } else {
                        WeakBlockPlanAction::ClearDeadSlots
                    }
                }
            };

            block_actions.push(WeakBlockPlanEntry {
                block: *block_id,
                action,
                live_slot_count: block.live_slot_count,
            });
        }

        Ok(Self {
            set: set.id,
            phase,
            block_actions,
        })
    }
}

#[derive(Clone, Copy, Debug, Eq, PartialEq)]
pub struct WeakBlockPlanEntry {
    pub block: WeakBlockId,
    pub action: WeakBlockPlanAction,
    pub live_slot_count: usize,
}

#[derive(Clone, Copy, Debug, Eq, PartialEq)]
pub enum WeakPlanningError {
    UnknownBlock(WeakBlockId),
    BlockSetMismatch {
        block: WeakBlockId,
        expected: WeakSetId,
        actual: WeakSetId,
    },
}

/// ID-only finalizer record for planning without borrowing heap cells.
#[derive(Clone, Copy, Debug, Eq, PartialEq)]
pub struct FinalizerPlanningRecord {
    pub id: FinalizerId,
    pub callback: HeapFinalizerCallbackId,
    pub target: CellId,
    pub owner: Option<WeakHandleOwnerId>,
    pub kind: FinalizerKind,
}

/// Pure finalizer plan selected after liveness has been decided elsewhere.
#[derive(Clone, Debug, Default, Eq, PartialEq)]
pub struct FinalizerPlan {
    pub entries: Vec<FinalizerPlanEntry>,
}

impl FinalizerPlan {
    pub fn from_records(
        callbacks: &[HeapFinalizerCallback],
        records: &[FinalizerPlanningRecord],
    ) -> Result<Self, FinalizerPlanningError> {
        let mut entries = Vec::with_capacity(records.len());
        for (index, record) in records.iter().enumerate() {
            if record.id == FinalizerId::default() {
                return Err(FinalizerPlanningError::InvalidFinalizerId(record.id));
            }
            if record.target == CellId::default() {
                return Err(FinalizerPlanningError::InvalidTarget(record.target));
            }
            if records[..index]
                .iter()
                .any(|previous| previous.id == record.id)
            {
                return Err(FinalizerPlanningError::DuplicateFinalizer(record.id));
            }
            let callback = callbacks
                .iter()
                .find(|callback| callback.id == record.callback)
                .ok_or(FinalizerPlanningError::MissingCallback(record.callback))?;
            entries.push(FinalizerPlanEntry {
                finalizer: record.id,
                callback: callback.id,
                target: record.target,
                kind: record.kind,
                owner: record.owner,
            });
        }
        Ok(Self { entries })
    }
}

#[derive(Clone, Copy, Debug, Eq, PartialEq)]
pub struct FinalizerPlanEntry {
    pub finalizer: FinalizerId,
    pub callback: HeapFinalizerCallbackId,
    pub target: CellId,
    pub owner: Option<WeakHandleOwnerId>,
    pub kind: FinalizerKind,
}

#[derive(Clone, Copy, Debug, Eq, PartialEq)]
pub enum FinalizerPlanningError {
    InvalidFinalizerId(FinalizerId),
    InvalidTarget(CellId),
    DuplicateFinalizer(FinalizerId),
    MissingCallback(HeapFinalizerCallbackId),
}

#[cfg(test)]
mod tests {
    use super::*;

    fn validating_owner(id: u64) -> WeakHandleOwnerContract {
        WeakHandleOwnerContract {
            id: WeakHandleOwnerId(id),
            can_validate_opaque_roots: true,
            can_finalize: false,
            authority: WeakOwnerAuthority::ReachabilityFromOpaqueRoots,
        }
    }

    #[test]
    fn weak_plan_selects_clear_actions_from_block_descriptors() {
        let set = WeakSetDescriptor {
            id: WeakSetId(1),
            heap: HeapId(7),
            blocks: vec![WeakBlockId(10), WeakBlockId(11)],
            allocator_block: None,
            next_allocator_block: None,
            active_phase: None,
        };
        let blocks = vec![
            WeakBlockDescriptor {
                id: WeakBlockId(10),
                set: WeakSetId(1),
                slot_capacity: 8,
                live_slot_count: 3,
                ..WeakBlockDescriptor::default()
            },
            WeakBlockDescriptor {
                id: WeakBlockId(11),
                set: WeakSetId(1),
                slot_capacity: 8,
                live_slot_count: 0,
                logically_empty_but_not_free: true,
                ..WeakBlockDescriptor::default()
            },
        ];

        let plan = WeakProcessingPlan::from_descriptors(&set, &blocks, WeakProcessingPhase::Clear);

        assert_eq!(
            plan.map(|plan| {
                plan.block_actions
                    .iter()
                    .map(|entry| entry.action)
                    .collect::<Vec<_>>()
            }),
            Ok(vec![
                WeakBlockPlanAction::ClearDeadSlots,
                WeakBlockPlanAction::RetireEmptyBlock
            ])
        );
    }

    #[test]
    fn weak_plan_rejects_unknown_block() {
        let set = WeakSetDescriptor {
            id: WeakSetId(1),
            heap: HeapId(7),
            blocks: vec![WeakBlockId(10)],
            allocator_block: None,
            next_allocator_block: None,
            active_phase: None,
        };

        assert_eq!(
            WeakProcessingPlan::from_descriptors(&set, &[], WeakProcessingPhase::Discover),
            Err(WeakPlanningError::UnknownBlock(WeakBlockId(10)))
        );
    }

    #[test]
    fn weak_root_policy_clears_dead_ordinary_slot() {
        let descriptors = [WeakRootPolicyDescriptor::ordinary(
            WeakId(1),
            Some(CellId(10)),
            false,
        )];

        let plan = WeakRootPolicyPlan::from_descriptors(&descriptors).expect("policy plan");
        let entry = plan.entry_for(WeakId(1)).expect("ordinary entry");

        assert_eq!(entry.action, WeakRootPolicyAction::Clear);
        assert_eq!(entry.reason, WeakRootPolicyReason::TargetDead);
        assert!(entry.is_clearable());
    }

    #[test]
    fn weak_root_policy_retains_ephemeron_key_from_owner_opaque_root() {
        let owner = validating_owner(2);
        let descriptors = [WeakRootPolicyDescriptor::ephemeron_key(
            WeakId(1),
            owner,
            Some(CellId(10)),
            false,
            true,
        )];

        let plan = WeakRootPolicyPlan::from_descriptors(&descriptors).expect("policy plan");
        let entry = plan.entry_for(WeakId(1)).expect("key entry");

        assert_eq!(entry.action, WeakRootPolicyAction::Retain);
        assert_eq!(
            entry.reason,
            WeakRootPolicyReason::EphemeronKeyValidatedByOwnerOpaqueRoot
        );
        assert_eq!(
            WeakSlotState::transition(
                WeakSlotTransitionRequest::new(
                    WeakSlotState::Live,
                    WeakProcessingPhase::Validate,
                    WeakEdgeKind::EphemeronKey,
                    entry.retains_target(),
                )
                .owner(owner)
            )
            .map(|outcome| outcome.to),
            Ok(WeakSlotState::Live)
        );
    }

    #[test]
    fn weak_root_policy_retains_ephemeron_value_only_when_key_policy_allows() {
        let owner = validating_owner(2);
        let retained_key = WeakRootPolicyDescriptor::ephemeron_key(
            WeakId(1),
            owner,
            Some(CellId(10)),
            false,
            true,
        );
        let retained_value = WeakRootPolicyDescriptor::ephemeron_value(
            WeakId(2),
            owner,
            Some(CellId(20)),
            WeakId(1),
        );
        let retained_plan = WeakRootPolicyPlan::from_descriptors(&[retained_key, retained_value])
            .expect("retained policy plan");
        let retained_entry = retained_plan.entry_for(WeakId(2)).expect("value entry");

        assert_eq!(retained_entry.action, WeakRootPolicyAction::Retain);
        assert_eq!(
            retained_entry.reason,
            WeakRootPolicyReason::EphemeronKeyValidatedByOwnerOpaqueRoot
        );
        assert_eq!(
            WeakSlotState::transition(
                WeakSlotTransitionRequest::new(
                    WeakSlotState::Live,
                    WeakProcessingPhase::Validate,
                    WeakEdgeKind::EphemeronValue,
                    false,
                )
                .owner(owner)
                .ephemeron_value_is_retained(retained_entry.retains_target())
            )
            .map(|outcome| outcome.to),
            Ok(WeakSlotState::Live)
        );

        let cleared_key = WeakRootPolicyDescriptor::ephemeron_key(
            WeakId(3),
            owner,
            Some(CellId(30)),
            false,
            false,
        );
        let cleared_value = WeakRootPolicyDescriptor::ephemeron_value(
            WeakId(4),
            owner,
            Some(CellId(40)),
            WeakId(3),
        );
        let cleared_plan = WeakRootPolicyPlan::from_descriptors(&[cleared_key, cleared_value])
            .expect("cleared policy plan");
        let cleared_entry = cleared_plan.entry_for(WeakId(4)).expect("value entry");

        assert_eq!(cleared_entry.action, WeakRootPolicyAction::Clear);
        assert_eq!(cleared_entry.reason, WeakRootPolicyReason::EphemeronKeyDead);
        assert_eq!(
            WeakSlotState::transition(
                WeakSlotTransitionRequest::new(
                    WeakSlotState::Live,
                    WeakProcessingPhase::Validate,
                    WeakEdgeKind::EphemeronValue,
                    true,
                )
                .owner(owner)
                .ephemeron_value_is_retained(cleared_entry.retains_target())
            )
            .map(|outcome| outcome.to),
            Ok(WeakSlotState::ClearPending)
        );
    }

    #[test]
    fn weak_root_policy_rejects_ephemeron_entry_without_owner_authority() {
        let descriptor = WeakRootPolicyDescriptor {
            weak: WeakId(1),
            kind: WeakEdgeKind::EphemeronKey,
            owner: None,
            target: Some(CellId(10)),
            target_is_live: true,
            ephemeron_key: None,
            owner_validates_opaque_root: false,
        };

        assert_eq!(
            WeakRootPolicyPlan::from_descriptors(&[descriptor]),
            Err(WeakRootPolicyError::MissingEphemeronOwner(WeakId(1)))
        );
        assert_eq!(
            WeakSlotState::transition(WeakSlotTransitionRequest::new(
                WeakSlotState::Live,
                WeakProcessingPhase::Validate,
                WeakEdgeKind::EphemeronKey,
                true,
            )),
            Err(WeakStateTransitionError::MissingOpaqueRootValidation(
                WeakHandleOwnerId::default()
            ))
        );
    }

    #[test]
    fn finalizer_plan_validates_callback_registration() {
        let callbacks = [HeapFinalizerCallback {
            id: HeapFinalizerCallbackId(3),
            kind: FinalizerKind::CellDestructor,
            user_data_tag: 0,
        }];
        let records = [FinalizerPlanningRecord {
            id: FinalizerId(9),
            callback: HeapFinalizerCallbackId(3),
            target: CellId(1),
            owner: None,
            kind: FinalizerKind::CellDestructor,
        }];

        assert_eq!(
            FinalizerPlan::from_records(&callbacks, &records).map(|plan| plan.entries.len()),
            Ok(1)
        );
    }

    #[test]
    fn finalizer_plan_rejects_missing_callback() {
        let records = [FinalizerPlanningRecord {
            id: FinalizerId(9),
            callback: HeapFinalizerCallbackId(3),
            target: CellId(1),
            owner: None,
            kind: FinalizerKind::CellDestructor,
        }];

        assert_eq!(
            FinalizerPlan::from_records(&[], &records),
            Err(FinalizerPlanningError::MissingCallback(
                HeapFinalizerCallbackId(3)
            ))
        );
    }

    #[test]
    fn weak_slot_validate_moves_dead_target_to_clear_pending() {
        let outcome = WeakSlotState::transition(WeakSlotTransitionRequest::new(
            WeakSlotState::Live,
            WeakProcessingPhase::Validate,
            WeakEdgeKind::Ordinary,
            false,
        ));

        assert_eq!(
            outcome,
            Ok(WeakSlotTransitionOutcome {
                from: WeakSlotState::Live,
                to: WeakSlotState::ClearPending,
                clears_target: false,
                requires_owner_callback: false
            })
        );
    }

    #[test]
    fn weak_slot_clear_pending_clears_target_only_in_clear_phase() {
        let outcome = WeakSlotState::transition(WeakSlotTransitionRequest::new(
            WeakSlotState::ClearPending,
            WeakProcessingPhase::Clear,
            WeakEdgeKind::Ordinary,
            false,
        ));

        assert_eq!(
            outcome.map(|outcome| (outcome.to, outcome.clears_target)),
            Ok((WeakSlotState::Dead, true))
        );
    }

    #[test]
    fn weak_slot_rejects_ephemeron_validation_without_owner_authority() {
        let outcome = WeakSlotState::transition(
            WeakSlotTransitionRequest::new(
                WeakSlotState::Live,
                WeakProcessingPhase::Validate,
                WeakEdgeKind::EphemeronKey,
                false,
            )
            .owner(WeakHandleOwnerContract {
                id: WeakHandleOwnerId(2),
                can_validate_opaque_roots: false,
                can_finalize: false,
                authority: WeakOwnerAuthority::ReachabilityFromOpaqueRoots,
            }),
        );

        assert_eq!(
            outcome,
            Err(WeakStateTransitionError::MissingOpaqueRootValidation(
                WeakHandleOwnerId(2)
            ))
        );
    }

    #[test]
    fn weak_slot_rejects_ephemeron_value_validation_without_policy() {
        let owner = validating_owner(2);
        let outcome = WeakSlotState::transition(
            WeakSlotTransitionRequest::new(
                WeakSlotState::Live,
                WeakProcessingPhase::Validate,
                WeakEdgeKind::EphemeronValue,
                true,
            )
            .owner(owner),
        );

        assert_eq!(
            outcome,
            Err(WeakStateTransitionError::MissingEphemeronValuePolicy)
        );
    }

    #[test]
    fn finalizer_transition_requires_end_phase() {
        let outcome = FinalizerState::transition(FinalizerTransitionRequest {
            state: FinalizerState::Registered,
            target_is_live: false,
            phase: crate::gc::GcPhase::Fixpoint,
            callback_registered: true,
            kind: FinalizerKind::CellDestructor,
        });

        assert_eq!(
            outcome,
            Err(FinalizerStateTransitionError::WrongPhase(
                crate::gc::GcPhase::Fixpoint
            ))
        );
    }

    #[test]
    fn finalizer_ready_state_invokes_callback_boundary() {
        let outcome = FinalizerState::transition(FinalizerTransitionRequest {
            state: FinalizerState::Ready,
            target_is_live: false,
            phase: crate::gc::GcPhase::End,
            callback_registered: true,
            kind: FinalizerKind::CellDestructor,
        });

        assert_eq!(
            outcome,
            Ok(FinalizerTransitionOutcome {
                from: FinalizerState::Ready,
                to: FinalizerState::Running,
                invokes_callback: true
            })
        );
    }
}
