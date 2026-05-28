//! Inline-cache attachment points.
//!
//! These types name where future property, call, construct, and global caches
//! attach. They preserve JSC's split between handler-list ICs, repatching ICs,
//! call link metadata, access cases, and GC-aware stubs without defining cache
//! probes, shape checks, patching, or stub generation.

use crate::bytecode::{CodeSpecialization, CoreOpcode};
use crate::gc::{
    BarrierFieldKind, BarrierKind, BarrierNotRequiredReason, BarrierRequirementOutcome,
    BarrierThreshold, CellId, StructureId,
};
use crate::jit::{
    AbiValue, CallBoundaryId, CallBoundaryMetadata, DependencyStrength, EffectSummary, EntryAbi,
    EntrypointKind, JitCodeId, JitType, WatchpointDependency, WatchpointDependencyId,
    WatchpointFirePolicy, WatchpointOwner, WatchpointSetDescriptor, WatchpointSetId,
    WatchpointSetState, WatchpointTarget,
};
use crate::object::{PropertyCacheability, PropertyOffset};
use crate::runtime::{CodeBlockId, ExecutableId, ObjectId, RuntimeValue};
use crate::strings::PropertyKey;
use crate::value::ValueKind;

/// Stable identity for an inline-cache slot within linked code state.
#[derive(Clone, Copy, Debug, Eq, Hash, Ord, PartialEq, PartialOrd)]
pub struct InlineCacheSlotId(pub u32);

/// Kind of runtime operation an inline-cache slot may later accelerate.
#[derive(Clone, Copy, Debug, Eq, PartialEq)]
pub enum InlineCacheKind {
    PropertyLoad,
    PropertyStore,
    ElementLoad,
    ElementStore,
    GlobalLoad,
    GlobalStore,
    Call,
    Construct,
    Delete,
    HasProperty,
    InstanceOf,
    PrivateBrand,
}

/// Reserved cache attachment point owned by code-block-equivalent state.
#[derive(Clone, Debug, Eq, PartialEq)]
pub struct InlineCacheSlot {
    pub id: InlineCacheSlotId,
    pub kind: InlineCacheKind,
    pub owner: Option<CodeBlockId>,
    pub bytecode_index: Option<u32>,
    pub state: InlineCacheState,
    pub dispatch: InlineCacheDispatch,
    pub cases: Vec<AccessCaseDescriptor>,
    pub stubs: Vec<InlineCacheStub>,
    pub watchpoints: Vec<WatchpointDependencyId>,
    pub barrier_metadata: Vec<InlineCacheBarrierMetadata>,
}

#[derive(Clone, Copy, Debug, Eq, PartialEq)]
pub enum InlineCacheCaseClassification {
    Empty,
    Monomorphic,
    Polymorphic,
    Megamorphic,
    CallLinking,
}

#[derive(Clone, Copy, Debug, Eq, PartialEq)]
pub enum InlineCacheSemanticClass {
    PropertyRead,
    PropertyWrite,
    ElementRead,
    ElementWrite,
    GlobalRead,
    GlobalWrite,
    Call,
    Construct,
    Delete,
    HasProperty,
    InstanceOf,
    PrivateBrand,
}

#[derive(Clone, Copy, Debug, Eq, PartialEq)]
pub enum InlineCacheFallbackSemantics {
    None,
    SlowPathLookup,
    SlowPathCall,
    MegamorphicGeneric,
    Disabled,
}

#[derive(Clone, Copy, Debug, Eq, PartialEq)]
pub struct InlineCacheSemanticSummary {
    pub class: InlineCacheSemanticClass,
    pub effects: EffectSummary,
    pub fallback: InlineCacheFallbackSemantics,
    pub requires_watchpoints: bool,
    pub may_transition_structure: bool,
    pub observes_prototype_chain: bool,
}

/// Bounded reasons a slow property-load observation cannot become an access
/// case yet. This is intentionally a fixed-size bitset so collecting local
/// lookup facts does not imply handler allocation or watchpoint ownership.
#[derive(Clone, Copy, Debug, Eq, PartialEq, Hash)]
pub enum PropertyLoadObservationBlocker {
    MissingBaseStructureGuard,
    MissingOffset,
    PrototypeChainGuardRequired,
    MayCallJsBoundary,
    UncacheableResult,
    OpaqueResult,
    NegativeLookupGuardRequired,
}

impl PropertyLoadObservationBlocker {
    const fn bit(self) -> u16 {
        match self {
            Self::MissingBaseStructureGuard => 1 << 0,
            Self::MissingOffset => 1 << 1,
            Self::PrototypeChainGuardRequired => 1 << 2,
            Self::MayCallJsBoundary => 1 << 3,
            Self::UncacheableResult => 1 << 4,
            Self::OpaqueResult => 1 << 5,
            Self::NegativeLookupGuardRequired => 1 << 6,
        }
    }
}

#[derive(Clone, Copy, Debug, Default, Eq, PartialEq, Hash)]
pub struct PropertyLoadObservationBlockers {
    bits: u16,
}

impl PropertyLoadObservationBlockers {
    pub const fn empty() -> Self {
        Self { bits: 0 }
    }

    pub const fn from_blocker(blocker: PropertyLoadObservationBlocker) -> Self {
        Self {
            bits: blocker.bit(),
        }
    }

    pub const fn bits(self) -> u16 {
        self.bits
    }

    pub const fn contains(self, blocker: PropertyLoadObservationBlocker) -> bool {
        self.bits & blocker.bit() != 0
    }

    pub const fn is_empty(self) -> bool {
        self.bits == 0
    }

    pub const fn union(self, other: Self) -> Self {
        Self {
            bits: self.bits | other.bits,
        }
    }

    pub fn insert(&mut self, blocker: PropertyLoadObservationBlocker) {
        self.bits |= blocker.bit();
    }
}

#[derive(Clone, Copy, Debug, Eq, PartialEq)]
pub enum PropertyLoadObservationReadiness {
    /// Metadata-only readiness; this does not attach code or claim generated
    /// property-load support exists.
    ReadyForAttachment,
    Blocked(PropertyLoadObservationBlockers),
}

impl PropertyLoadObservationReadiness {
    pub const fn is_ready(self) -> bool {
        match self {
            Self::ReadyForAttachment => true,
            Self::Blocked(_) => false,
        }
    }

    pub const fn blockers(self) -> PropertyLoadObservationBlockers {
        match self {
            Self::ReadyForAttachment => PropertyLoadObservationBlockers::empty(),
            Self::Blocked(blockers) => blockers,
        }
    }
}

/// Bounded reasons a generated call observation cannot become a call link yet.
/// These are metadata-only blockers; none of them allocate or attach call-link
/// state.
#[derive(Clone, Copy, Debug, Eq, PartialEq, Hash)]
pub enum CallLinkReadinessBlocker {
    DirectCallDisallowed,
    MissingCallSiteMetadata,
    MissingCallBoundary,
    MissingExecutableTarget,
    MissingTargetCodeBlock,
    RootSafetyBlocked,
    UnsupportedTargetKind,
    UnsupportedOutcome,
    MayCallJsBoundary,
    ArgumentCountTooLarge,
}

impl CallLinkReadinessBlocker {
    const fn bit(self) -> u16 {
        match self {
            Self::DirectCallDisallowed => 1 << 0,
            Self::MissingCallSiteMetadata => 1 << 1,
            Self::MissingCallBoundary => 1 << 2,
            Self::MissingExecutableTarget => 1 << 3,
            Self::MissingTargetCodeBlock => 1 << 4,
            Self::RootSafetyBlocked => 1 << 5,
            Self::UnsupportedTargetKind => 1 << 6,
            Self::UnsupportedOutcome => 1 << 7,
            Self::MayCallJsBoundary => 1 << 8,
            Self::ArgumentCountTooLarge => 1 << 9,
        }
    }
}

#[derive(Clone, Copy, Debug, Default, Eq, PartialEq, Hash)]
pub struct CallLinkReadinessBlockers {
    bits: u16,
}

impl CallLinkReadinessBlockers {
    pub const fn empty() -> Self {
        Self { bits: 0 }
    }

    pub const fn from_blocker(blocker: CallLinkReadinessBlocker) -> Self {
        Self {
            bits: blocker.bit(),
        }
    }

    pub const fn bits(self) -> u16 {
        self.bits
    }

    pub const fn contains(self, blocker: CallLinkReadinessBlocker) -> bool {
        self.bits & blocker.bit() != 0
    }

    pub const fn is_empty(self) -> bool {
        self.bits == 0
    }

    pub const fn union(self, other: Self) -> Self {
        Self {
            bits: self.bits | other.bits,
        }
    }

    pub fn insert(&mut self, blocker: CallLinkReadinessBlocker) {
        self.bits |= blocker.bit();
    }

    pub fn remove(&mut self, blocker: CallLinkReadinessBlocker) {
        self.bits &= !blocker.bit();
    }
}

#[derive(Clone, Debug, Eq, PartialEq)]
pub enum InlineCacheValidationError {
    EmptyName,
    EmptyProvenance(&'static str),
    DuplicateSchemaKind(InlineCacheKind),
    EmptyAllowedCases(&'static str),
    EmptyAllowedStubKinds(&'static str),
    DispatchMismatch,
    CaseNotAllowed(AccessCaseKind),
    StubKindNotAllowed(InlineCacheStubKind),
    StubOwnerMismatch(InlineCacheStubId),
    StubCaseNotInSlot(InlineCacheStubId),
    WatchpointOwnershipMismatch,
    BarrierMetadataMissing,
    BarrierKindMismatch,
    StateCaseMismatch,
    CallLinkMismatch,
    MissingHandoffOwner,
    MissingHandoffBytecodeIndex,
    MissingHandoffCallBoundary,
    PropertyObservationUnsupportedCacheKind {
        expected: InlineCacheKind,
        actual: InlineCacheKind,
    },
    PropertyObservationUnsupportedFallback {
        expected: InlineCacheFallbackSemantics,
        actual: InlineCacheFallbackSemantics,
    },
    PropertyObservationHandoffOwnerMismatch {
        observation: CodeBlockId,
        handoff: CodeBlockId,
    },
    PropertyObservationHandoffSlotMismatch {
        observation: InlineCacheSlotId,
        handoff: InlineCacheSlotId,
    },
    PropertyObservationHandoffBytecodeIndexMismatch {
        observation: u32,
        handoff: u32,
    },
    PropertyObservationHandoffCacheKindMismatch {
        observation: InlineCacheKind,
        handoff: InlineCacheKind,
    },
    PropertyObservationHandoffFallbackMismatch {
        observation: InlineCacheFallbackSemantics,
        handoff: InlineCacheFallbackSemantics,
    },
    PropertyObservationHandoffMissKindMismatch {
        expected: InlineCacheMissKind,
        actual: InlineCacheMissKind,
    },
    PropertyObservationBoundaryContamination,
    PropertyObservationCallLinkContamination,
    PropertyObservationHandoffClobbersOperandRegisters,
    PropertyObservationReadyButBlocked(PropertyLoadObservationBlockers),
    PropertyObservationReadinessMismatch {
        expected: PropertyLoadObservationReadiness,
        actual: PropertyLoadObservationReadiness,
    },
    PropertyHasObservationResultMismatch {
        result: bool,
        access_case_kind: AccessCaseKind,
    },
    PropertyHasObservationUnsupportedOpcode {
        opcode: CoreOpcode,
    },
    CallObservationUnsupportedOpcode {
        opcode: CoreOpcode,
    },
    CallObservationOwnerMismatch {
        observation: CodeBlockId,
        handoff: CodeBlockId,
    },
    CallObservationFrameMismatch {
        observation: Option<crate::runtime::CallFrameId>,
        handoff: crate::runtime::CallFrameId,
    },
    CallObservationBytecodeIndexMismatch {
        observation: u32,
        handoff: u32,
    },
    CallObservationOpcodeMismatch {
        observation: CoreOpcode,
        handoff: CoreOpcode,
    },
    CallObservationDirectCallReadinessClaimed,
    PropertyLoadPlanOwnerMismatch {
        expected: CodeBlockId,
        actual: CodeBlockId,
    },
    PropertyLoadPlanUnsupportedKind(PropertyLoadAccessCasePlanKind),
    PropertyLoadPlanUnsupportedStubKind(InlineCacheStubKind),
    PropertyLoadPlanUnsupportedEffectContract(PropertyLoadAccessCasePlanContract),
    PropertyLoadPlanUnsupportedAccessCase(AccessCaseKind),
    PropertyLoadPlanUnsupportedKey(CacheKey),
    PropertyLoadPlanAccessCaseKeyMismatch {
        plan: CacheKey,
        access_case: CacheKey,
    },
    PropertyLoadPlanMissingBaseStructure,
    PropertyLoadPlanInvalidBaseStructure(StructureId),
    PropertyLoadPlanMissingOffset,
    PropertyLoadPlanInvalidOffset(PropertyOffset),
    PropertyLoadPlanUnsupportedHolder(ObjectId),
    PropertyLoadPlanUnsupportedNewStructure(StructureId),
    PropertyLoadPlanUnsupportedDependencies,
    PropertyLoadPlanUnsupportedGlobalProxy,
    PropertyLoadPlanMayCallJs,
    PropertyLoadPlanDuplicate {
        bytecode_index: u32,
        base_structure: StructureId,
        offset: PropertyOffset,
        key: CacheKey,
    },
    PropertyStorePlanOwnerMismatch {
        expected: CodeBlockId,
        actual: CodeBlockId,
    },
    PropertyStorePlanUnsupportedKind(PropertyStoreAccessCasePlanKind),
    PropertyStorePlanUnsupportedStubKind(InlineCacheStubKind),
    PropertyStorePlanUnsupportedEffectContract(PropertyStoreAccessCasePlanContract),
    PropertyStorePlanMissingBarrierProof(PropertyStoreAccessCasePlanContract),
    PropertyStorePlanUnsupportedAccessCase(AccessCaseKind),
    PropertyStorePlanUnsupportedKey(CacheKey),
    PropertyStorePlanAccessCaseKeyMismatch {
        plan: CacheKey,
        access_case: CacheKey,
    },
    PropertyStorePlanMissingBaseStructure,
    PropertyStorePlanInvalidBaseStructure(StructureId),
    PropertyStorePlanMissingOffset,
    PropertyStorePlanInvalidOffset(PropertyOffset),
    PropertyStorePlanUnsupportedHolder(ObjectId),
    PropertyStorePlanUnsupportedNewStructure(StructureId),
    PropertyStorePlanMissingNewStructure,
    PropertyStorePlanInvalidNewStructure(StructureId),
    PropertyStorePlanRedundantTransitionStructure(StructureId),
    PropertyStorePlanUnsupportedDependencies,
    PropertyStorePlanUnsupportedGlobalProxy,
    PropertyStorePlanMayCallJs,
    PropertyStorePlanDuplicate {
        bytecode_index: u32,
        key: CacheKey,
        base_structure: StructureId,
        offset: PropertyOffset,
        new_structure: Option<StructureId>,
    },
    PropertyStoreMutationCandidateInvalidOrdinal {
        field: &'static str,
        ordinal: u64,
    },
    PropertyStoreMutationCandidateBarrierEvidenceMismatch {
        field: PropertyStoreMutationBarrierEvidenceMismatchField,
    },
    PropertyStoreMutationCandidateInvalidBarrierObservationCount(u32),
    PropertyStoreMutationCandidateDuplicateStorePlanOrdinal(u64),
    PropertyStoreMutationCandidateDuplicateReadinessOrdinal(u64),
    PropertyStoreMutationCandidateDuplicate {
        store_plan_ordinal: u64,
        bytecode_index: u32,
        key: CacheKey,
        base_structure: StructureId,
        offset: PropertyOffset,
        new_structure: Option<StructureId>,
    },
    PropertyHasPlanOwnerMismatch {
        expected: CodeBlockId,
        actual: CodeBlockId,
    },
    PropertyHasPlanUnsupportedKey(CacheKey),
    PropertyLoadGuardedCandidateOwnerMismatch {
        expected: CodeBlockId,
        actual: CodeBlockId,
    },
    PropertyLoadGuardedCandidateUnsupportedKey(CacheKey),
    PropertyLoadGuardedCandidateUnsupportedShape {
        candidate_kind: PropertyLoadGuardedCandidateKind,
        requirement: PropertyLoadGuardRequirement,
        outcome: PropertyLoadGuardChainOutcome,
    },
    PropertyLoadGuardedCandidateMalformedChain(&'static str),
    PropertyLoadGuardedCandidateInvalidOffset {
        descriptor_offset: Option<PropertyOffset>,
        outcome_offset: PropertyOffset,
    },
    PropertyLoadGuardedCandidateDependencyBindingCountMismatch {
        chain_length: usize,
        dependency_count: usize,
        binding_count: usize,
    },
    PropertyLoadGuardedCandidateInvalidOrdinal {
        field: &'static str,
        ordinal: u64,
    },
    PropertyLoadGuardedCandidateInvalidBindingSetId(WatchpointSetId),
    PropertyLoadGuardedCandidateDuplicateGuardPlanOrdinal(u64),
    PropertyLoadGuardedCandidateDuplicateDependencyOrdinal(u64),
    PropertyLoadGuardedCandidateDuplicateBindingSetId(WatchpointSetId),
    PropertyLoadGuardedCandidateDuplicate {
        bytecode_index: u32,
        base_structure: StructureId,
        key: CacheKey,
        requirement: PropertyLoadGuardRequirement,
        outcome: PropertyLoadGuardChainOutcome,
    },
    PropertyLoadGuardedCandidateMissingGuardPlan {
        guard_plan_ordinal: u64,
    },
}

#[derive(Clone, Debug, Eq, PartialEq)]
pub struct InlineCacheSlotBuilder {
    slot: InlineCacheSlot,
}

impl InlineCacheSlotBuilder {
    pub fn new(id: InlineCacheSlotId, kind: InlineCacheKind) -> Self {
        let dispatch = INLINE_CACHE_SCHEMA_REGISTRY
            .schema_for_kind(kind)
            .map(|schema| schema.dispatch)
            .unwrap_or(InlineCacheDispatch::SlowPathOnly);
        Self {
            slot: InlineCacheSlot {
                id,
                kind,
                owner: None,
                bytecode_index: None,
                state: InlineCacheState::Uninitialized,
                dispatch,
                cases: Vec::new(),
                stubs: Vec::new(),
                watchpoints: Vec::new(),
                barrier_metadata: Vec::new(),
            },
        }
    }

    pub fn owner(mut self, owner: CodeBlockId) -> Self {
        self.slot.owner = Some(owner);
        self
    }

    pub fn bytecode_index(mut self, bytecode_index: u32) -> Self {
        self.slot.bytecode_index = Some(bytecode_index);
        self
    }

    pub fn state(mut self, state: InlineCacheState) -> Self {
        self.slot.state = state;
        self
    }

    pub fn case(mut self, case: AccessCaseDescriptor) -> Self {
        self.slot.cases.push(case);
        self
    }

    pub fn stub(mut self, stub: InlineCacheStub) -> Self {
        self.slot.stubs.push(stub);
        self
    }

    pub fn watchpoint(mut self, watchpoint: WatchpointDependencyId) -> Self {
        self.slot.watchpoints.push(watchpoint);
        self
    }

    pub fn barrier_metadata(mut self, metadata: InlineCacheBarrierMetadata) -> Self {
        self.slot.barrier_metadata.push(metadata);
        self
    }

    pub fn build(self) -> Result<InlineCacheSlot, InlineCacheValidationError> {
        let schema = INLINE_CACHE_SCHEMA_REGISTRY
            .schema_for_kind(self.slot.kind)
            .ok_or(InlineCacheValidationError::DispatchMismatch)?;
        self.slot.validate_against(schema)?;
        Ok(self.slot)
    }
}

impl InlineCacheSlot {
    pub fn builder(id: InlineCacheSlotId, kind: InlineCacheKind) -> InlineCacheSlotBuilder {
        InlineCacheSlotBuilder::new(id, kind)
    }

    pub fn validate_against(
        &self,
        schema: &StaticInlineCacheSchema,
    ) -> Result<(), InlineCacheValidationError> {
        if self.kind != schema.kind || self.dispatch != schema.dispatch {
            return Err(InlineCacheValidationError::DispatchMismatch);
        }

        for case in &self.cases {
            if !schema.allowed_cases.contains(&case.kind) {
                return Err(InlineCacheValidationError::CaseNotAllowed(case.kind));
            }
            if case.may_call_js && !schema.may_call_js {
                return Err(InlineCacheValidationError::CallLinkMismatch);
            }
        }

        for stub in &self.stubs {
            if stub.owner_slot != self.id {
                return Err(InlineCacheValidationError::StubOwnerMismatch(stub.id));
            }
            if !schema.allowed_stub_kinds.contains(&stub.kind) {
                return Err(InlineCacheValidationError::StubKindNotAllowed(stub.kind));
            }
            for stub_case in &stub.cases {
                if !self.cases.iter().any(|slot_case| slot_case == stub_case) {
                    return Err(InlineCacheValidationError::StubCaseNotInSlot(stub.id));
                }
            }
            if !stub.call_links.is_empty() && !schema.may_call_js {
                return Err(InlineCacheValidationError::CallLinkMismatch);
            }
        }

        if !schema.owns_watchpoints
            && (!self.watchpoints.is_empty()
                || self.cases.iter().any(|case| !case.dependencies.is_empty()))
        {
            return Err(InlineCacheValidationError::WatchpointOwnershipMismatch);
        }

        if slot_requires_barrier_metadata(self) && self.barrier_metadata.is_empty() {
            return Err(InlineCacheValidationError::BarrierMetadataMissing);
        }
        for metadata in &self.barrier_metadata {
            metadata.validate_for_slot(self)?;
        }

        if matches!(
            self.state,
            InlineCacheState::Monomorphic
                | InlineCacheState::Polymorphic
                | InlineCacheState::Megamorphic
        ) && self.cases.is_empty()
        {
            return Err(InlineCacheValidationError::StateCaseMismatch);
        }

        Ok(())
    }

    pub fn classify_against(
        &self,
        schema: &StaticInlineCacheSchema,
    ) -> Result<InlineCacheCaseClassification, InlineCacheValidationError> {
        self.validate_against(schema)?;
        if self.kind == InlineCacheKind::Call
            || self.stubs.iter().any(|stub| !stub.call_links.is_empty())
        {
            return Ok(InlineCacheCaseClassification::CallLinking);
        }
        if self.state == InlineCacheState::Megamorphic
            || self
                .cases
                .iter()
                .any(|case| case.kind == AccessCaseKind::Megamorphic)
        {
            return Ok(InlineCacheCaseClassification::Megamorphic);
        }
        match self.cases.len() {
            0 => Ok(InlineCacheCaseClassification::Empty),
            1 => Ok(InlineCacheCaseClassification::Monomorphic),
            _ => Ok(InlineCacheCaseClassification::Polymorphic),
        }
    }
}

pub fn classify_inline_cache_slot(
    slot: &InlineCacheSlot,
) -> Result<InlineCacheCaseClassification, InlineCacheValidationError> {
    let schema = INLINE_CACHE_SCHEMA_REGISTRY
        .schema_for_kind(slot.kind)
        .ok_or(InlineCacheValidationError::DispatchMismatch)?;
    slot.classify_against(schema)
}

pub fn classify_inline_cache_semantics(
    slot: &InlineCacheSlot,
) -> Result<InlineCacheSemanticSummary, InlineCacheValidationError> {
    let schema = INLINE_CACHE_SCHEMA_REGISTRY
        .schema_for_kind(slot.kind)
        .ok_or(InlineCacheValidationError::DispatchMismatch)?;
    slot.validate_against(schema)?;

    let mut summary = InlineCacheSemanticSummary {
        class: semantic_class_for_kind(slot.kind),
        effects: effects_for_cache_kind(slot.kind),
        fallback: fallback_for_slot(slot),
        requires_watchpoints: !slot.watchpoints.is_empty()
            || slot.cases.iter().any(|case| !case.dependencies.is_empty()),
        may_transition_structure: false,
        observes_prototype_chain: false,
    };

    for case in &slot.cases {
        summary.effects = summary.effects.union(effects_for_access_case(case));
        summary.may_transition_structure |= case.kind == AccessCaseKind::Transition
            || (case.base_structure.is_some() && case.new_structure.is_some());
        summary.observes_prototype_chain |= matches!(
            case.kind,
            AccessCaseKind::Getter
                | AccessCaseKind::Setter
                | AccessCaseKind::CustomAccessor
                | AccessCaseKind::IntrinsicGetter
                | AccessCaseKind::ModuleNamespaceLoad
                | AccessCaseKind::ProxyObject
        ) || case.holder.is_some()
            || case.via_global_proxy;
    }

    Ok(summary)
}

/// Runtime state of an IC attachment.
#[derive(Clone, Copy, Debug, Eq, PartialEq)]
pub enum InlineCacheState {
    Uninitialized,
    ColdSlowPath,
    Monomorphic,
    Polymorphic,
    Megamorphic,
    Resetting,
    Disabled,
}

/// Dispatch strategy reserved for the future generated site.
#[derive(Clone, Copy, Debug, Eq, PartialEq)]
pub enum InlineCacheDispatch {
    DataOnlyHandlerChain,
    RepatchingSlab,
    SharedStatelessStub,
    SlowPathOnly,
}

/// Property lookup key used by IC metadata.
///
/// Concrete string, symbol, private-name, and index identity is owned by
/// `strings::PropertyKey`. `Dynamic` marks sites where the future runtime must
/// convert a value before lookup; it is not an alternate key identity.
#[derive(Clone, Copy, Debug, Eq, PartialEq)]
pub enum CacheKey {
    Property(PropertyKey),
    Dynamic,
}

/// Access case family mirrored from JSC's `AccessCase` contract.
#[derive(Clone, Copy, Debug, Eq, PartialEq)]
pub enum AccessCaseKind {
    Load,
    Transition,
    Replace,
    Delete,
    Miss,
    Getter,
    Setter,
    CustomAccessor,
    IntrinsicGetter,
    ArrayLength,
    StringLength,
    ModuleNamespaceLoad,
    ProxyObject,
    InstanceOf,
    InHit,
    InMiss,
    InMegamorphic,
    ProxyObjectIn,
    IndexedLoad,
    IndexedStore,
    IndexedIn,
    IndexedMegamorphicIn,
    IndexedInt32InHit,
    IndexedDoubleInHit,
    IndexedContiguousInHit,
    IndexedArrayStorageInHit,
    IndexedScopedArgumentsInHit,
    IndexedDirectArgumentsInHit,
    IndexedTypedArrayInt8In,
    IndexedTypedArrayUint8In,
    IndexedTypedArrayUint8ClampedIn,
    IndexedTypedArrayInt16In,
    IndexedTypedArrayUint16In,
    IndexedTypedArrayInt32In,
    IndexedTypedArrayUint32In,
    IndexedTypedArrayFloat16In,
    IndexedTypedArrayFloat32In,
    IndexedTypedArrayFloat64In,
    IndexedResizableTypedArrayInt8In,
    IndexedResizableTypedArrayUint8In,
    IndexedResizableTypedArrayUint8ClampedIn,
    IndexedResizableTypedArrayInt16In,
    IndexedResizableTypedArrayUint16In,
    IndexedResizableTypedArrayInt32In,
    IndexedResizableTypedArrayUint32In,
    IndexedResizableTypedArrayFloat16In,
    IndexedResizableTypedArrayFloat32In,
    IndexedResizableTypedArrayFloat64In,
    IndexedStringInHit,
    IndexedNoIndexingInMiss,
    IndexedProxyObjectIn,
    Megamorphic,
}

/// Structure and property condition for one polymorphic access case.
#[derive(Clone, Debug, Eq, PartialEq)]
pub struct AccessCaseDescriptor {
    pub kind: AccessCaseKind,
    pub key: CacheKey,
    pub base_structure: Option<StructureId>,
    pub new_structure: Option<StructureId>,
    pub holder: Option<ObjectId>,
    pub offset: Option<PropertyOffset>,
    pub via_global_proxy: bool,
    pub may_call_js: bool,
    pub dependencies: Vec<WatchpointDependency>,
}

#[derive(Clone, Copy, Debug, Eq, PartialEq)]
pub enum PropertyLoadAccessCasePlanKind {
    DataOnlyOwnLoad,
    DataOnlyIndexedLoad,
}

#[derive(Clone, Copy, Debug, Default, Eq, PartialEq)]
pub enum PropertyLoadBaseNormalization {
    #[default]
    None,
    StringPrototype,
}

#[derive(Clone, Copy, Debug, Eq, PartialEq)]
pub enum PropertyLoadHeapEffect {
    ReadsHeap,
}

#[derive(Clone, Copy, Debug, Eq, PartialEq)]
pub enum PropertyLoadResultEffect {
    WritesDestinationRegister,
}

#[derive(Clone, Copy, Debug, Eq, PartialEq)]
pub enum PropertyLoadExitEffect {
    MayExitToSlowPath,
}

#[derive(Clone, Copy, Debug, Eq, PartialEq)]
pub enum PropertyLoadHostBoundaryEffect {
    NoAllocationNoCallsNoHeapWrites,
}

#[derive(Clone, Copy, Debug, Eq, PartialEq)]
pub struct PropertyLoadDataOnlyOwnLoadEffects {
    pub heap: PropertyLoadHeapEffect,
    pub result: PropertyLoadResultEffect,
    pub exit: PropertyLoadExitEffect,
    pub host_boundary: PropertyLoadHostBoundaryEffect,
}

#[derive(Clone, Copy, Debug, Eq, PartialEq)]
pub enum PropertyLoadAccessCaseEffects {
    DataOnlyOwnLoad(PropertyLoadDataOnlyOwnLoadEffects),
    Unsupported,
}

#[derive(Clone, Copy, Debug, Eq, PartialEq)]
pub enum PropertyLoadReturnedCellRooting {
    TargetedDestinationRegisterBeforeGcBoundary,
}

#[derive(Clone, Copy, Debug, Eq, PartialEq)]
pub enum PropertyLoadAccessCaseRooting {
    ReturnedCell(PropertyLoadReturnedCellRooting),
    Unsupported,
}

#[derive(Clone, Copy, Debug, Eq, PartialEq)]
pub struct PropertyLoadAccessCasePlanContract {
    pub effects: PropertyLoadAccessCaseEffects,
    pub rooting: PropertyLoadAccessCaseRooting,
}

impl PropertyLoadAccessCasePlanContract {
    pub const DATA_ONLY_OWN_LOAD: Self = Self {
        effects: PropertyLoadAccessCaseEffects::DataOnlyOwnLoad(
            PropertyLoadDataOnlyOwnLoadEffects {
                heap: PropertyLoadHeapEffect::ReadsHeap,
                result: PropertyLoadResultEffect::WritesDestinationRegister,
                exit: PropertyLoadExitEffect::MayExitToSlowPath,
                host_boundary: PropertyLoadHostBoundaryEffect::NoAllocationNoCallsNoHeapWrites,
            },
        ),
        rooting: PropertyLoadAccessCaseRooting::ReturnedCell(
            PropertyLoadReturnedCellRooting::TargetedDestinationRegisterBeforeGcBoundary,
        ),
    };

    pub fn supports_generated_data_only_own_load(self) -> bool {
        self == Self::DATA_ONLY_OWN_LOAD
    }

    pub fn supports_generated_data_only_indexed_load(self) -> bool {
        self == Self::DATA_ONLY_OWN_LOAD
    }
}

/// Data-only plan for a future property-load access case attachment.
///
/// This is VM/JIT metadata only: it does not own generated code, mutate a real
/// inline-cache slot, install watchpoints, or attach an access case.
#[derive(Clone, Debug, Eq, PartialEq)]
pub struct PropertyLoadAccessCasePlan {
    pub plan_kind: PropertyLoadAccessCasePlanKind,
    pub owner: CodeBlockId,
    pub slot: InlineCacheSlotId,
    pub bytecode_index: u32,
    pub key: CacheKey,
    pub access_case: AccessCaseDescriptor,
    pub planned_stub_kind: InlineCacheStubKind,
    pub effect_contract: PropertyLoadAccessCasePlanContract,
}

#[derive(Clone, Copy, Debug, Eq, PartialEq)]
pub enum PropertyStoreAccessCasePlanKind {
    DataOnlyReplace,
    DataOnlyTransition,
    DataOnlyIndexedStore,
    Unsupported,
}

#[derive(Clone, Copy, Debug, Eq, PartialEq)]
pub enum PropertyStoreHeapEffect {
    WritesExistingOwnDataSlot,
    TransitionsStructureAndWritesOwnDataSlot,
    WritesExistingIndexedSlot,
}

#[derive(Clone, Copy, Debug, Eq, PartialEq)]
pub enum PropertyStoreStoredValueEffect {
    StoresProvidedValue,
}

#[derive(Clone, Copy, Debug, Eq, PartialEq)]
pub enum PropertyStoreExitEffect {
    MayExitToSlowPathBeforeHeapMutation,
}

#[derive(Clone, Copy, Debug, Eq, PartialEq)]
pub enum PropertyStoreHostBoundaryEffect {
    NoAllocationNoCallsNoGcBoundary,
}

/// Barrier requirement carried by store-plan metadata.
///
/// These variants only declare which runtime barrier proof a later VM/JIT
/// integration must provide before emitting executable stores. They are not
/// themselves proof that a write barrier has run or will run correctly.
#[derive(Clone, Copy, Debug, Eq, PartialEq)]
pub enum PropertyStoreBarrierEffect {
    RequiresRuntimeStoredValueBarrierProof,
    RequiresRuntimeStoredValueAndStructureTransitionBarrierProof,
    Unsupported,
}

#[derive(Clone, Copy, Debug, Eq, PartialEq)]
pub struct PropertyStoreDataOnlyReplaceEffects {
    pub heap: PropertyStoreHeapEffect,
    pub stored_value: PropertyStoreStoredValueEffect,
    pub exit: PropertyStoreExitEffect,
    pub host_boundary: PropertyStoreHostBoundaryEffect,
}

#[derive(Clone, Copy, Debug, Eq, PartialEq)]
pub struct PropertyStoreDataOnlyTransitionEffects {
    pub heap: PropertyStoreHeapEffect,
    pub stored_value: PropertyStoreStoredValueEffect,
    pub exit: PropertyStoreExitEffect,
    pub host_boundary: PropertyStoreHostBoundaryEffect,
}

#[derive(Clone, Copy, Debug, Eq, PartialEq)]
pub struct PropertyStoreDataOnlyIndexedStoreEffects {
    pub heap: PropertyStoreHeapEffect,
    pub stored_value: PropertyStoreStoredValueEffect,
    pub exit: PropertyStoreExitEffect,
    pub host_boundary: PropertyStoreHostBoundaryEffect,
}

#[derive(Clone, Copy, Debug, Eq, PartialEq)]
pub enum PropertyStoreAccessCaseEffects {
    DataOnlyReplace(PropertyStoreDataOnlyReplaceEffects),
    DataOnlyTransition(PropertyStoreDataOnlyTransitionEffects),
    DataOnlyIndexedStore(PropertyStoreDataOnlyIndexedStoreEffects),
    Unsupported,
}

/// Metadata-only store access-case contract.
///
/// The constants below describe the shape a future generated store may require.
/// They do not attach code, make PutByName observations cacheable, or prove
/// runtime barrier correctness by themselves.
#[derive(Clone, Copy, Debug, Eq, PartialEq)]
pub struct PropertyStoreAccessCasePlanContract {
    pub effects: PropertyStoreAccessCaseEffects,
    pub barrier: PropertyStoreBarrierEffect,
}

impl PropertyStoreAccessCasePlanContract {
    pub const DATA_ONLY_REPLACE: Self = Self {
        effects: PropertyStoreAccessCaseEffects::DataOnlyReplace(
            PropertyStoreDataOnlyReplaceEffects {
                heap: PropertyStoreHeapEffect::WritesExistingOwnDataSlot,
                stored_value: PropertyStoreStoredValueEffect::StoresProvidedValue,
                exit: PropertyStoreExitEffect::MayExitToSlowPathBeforeHeapMutation,
                host_boundary: PropertyStoreHostBoundaryEffect::NoAllocationNoCallsNoGcBoundary,
            },
        ),
        barrier: PropertyStoreBarrierEffect::RequiresRuntimeStoredValueBarrierProof,
    };

    pub const DATA_ONLY_TRANSITION: Self = Self {
        effects: PropertyStoreAccessCaseEffects::DataOnlyTransition(
            PropertyStoreDataOnlyTransitionEffects {
                heap: PropertyStoreHeapEffect::TransitionsStructureAndWritesOwnDataSlot,
                stored_value: PropertyStoreStoredValueEffect::StoresProvidedValue,
                exit: PropertyStoreExitEffect::MayExitToSlowPathBeforeHeapMutation,
                host_boundary: PropertyStoreHostBoundaryEffect::NoAllocationNoCallsNoGcBoundary,
            },
        ),
        barrier:
            PropertyStoreBarrierEffect::RequiresRuntimeStoredValueAndStructureTransitionBarrierProof,
    };

    pub const DATA_ONLY_INDEXED_STORE: Self = Self {
        effects: PropertyStoreAccessCaseEffects::DataOnlyIndexedStore(
            PropertyStoreDataOnlyIndexedStoreEffects {
                heap: PropertyStoreHeapEffect::WritesExistingIndexedSlot,
                stored_value: PropertyStoreStoredValueEffect::StoresProvidedValue,
                exit: PropertyStoreExitEffect::MayExitToSlowPathBeforeHeapMutation,
                host_boundary: PropertyStoreHostBoundaryEffect::NoAllocationNoCallsNoGcBoundary,
            },
        ),
        barrier: PropertyStoreBarrierEffect::RequiresRuntimeStoredValueBarrierProof,
    };

    pub const fn carries_runtime_barrier_requirement(self) -> bool {
        !matches!(self.barrier, PropertyStoreBarrierEffect::Unsupported)
    }

    pub fn supports_metadata_only_replace_plan(self) -> bool {
        self == Self::DATA_ONLY_REPLACE
    }

    pub fn supports_metadata_only_transition_plan(self) -> bool {
        self == Self::DATA_ONLY_TRANSITION
    }

    pub fn supports_metadata_only_indexed_store_plan(self) -> bool {
        self == Self::DATA_ONLY_INDEXED_STORE
    }
}

/// Data-only plan for a future property-store access case attachment.
///
/// This is JIT-facing metadata only: it does not own generated code, mutate a
/// real inline-cache slot, install watchpoints, perform barriers, transition
/// structures, or make any store observation cacheable or executable.
#[derive(Clone, Debug, Eq, PartialEq)]
#[allow(dead_code)]
pub struct PropertyStoreAccessCasePlan {
    pub plan_kind: PropertyStoreAccessCasePlanKind,
    pub owner: CodeBlockId,
    pub slot: InlineCacheSlotId,
    pub bytecode_index: u32,
    pub key: CacheKey,
    pub access_case: AccessCaseDescriptor,
    pub planned_stub_kind: InlineCacheStubKind,
    pub effect_contract: PropertyStoreAccessCasePlanContract,
}

/// Root-safe chain fact recorded by the VM for one slow property-load lookup.
///
/// This intentionally carries object identity and structure metadata only. The
/// proof attached to a guard plan is derived later after validating the chain
/// against the observed lookup outcome.
#[derive(Clone, Copy, Debug, Eq, PartialEq)]
pub struct PropertyLoadObservationChainEntry {
    pub object: ObjectId,
    pub structure: StructureId,
    pub next_prototype: Option<ObjectId>,
}

#[derive(Clone, Copy, Debug, Eq, PartialEq)]
pub enum PropertyLoadGuardChainEntryProof {
    NoOwnProperty,
    DataProperty { offset: PropertyOffset },
}

#[derive(Clone, Copy, Debug, Eq, PartialEq)]
pub struct PropertyLoadGuardChainEntry {
    pub object: ObjectId,
    pub structure: StructureId,
    pub next_prototype: Option<ObjectId>,
    pub proof: PropertyLoadGuardChainEntryProof,
}

#[derive(Clone, Copy, Debug, Eq, PartialEq)]
pub enum PropertyLoadGuardChainOutcome {
    PrototypeData {
        holder_index: usize,
        offset: PropertyOffset,
    },
    Missing {
        terminal_null: bool,
    },
}

#[derive(Clone, Debug, Eq, PartialEq)]
pub struct PropertyLoadGuardChainCertificate {
    pub entries: Vec<PropertyLoadGuardChainEntry>,
    pub outcome: PropertyLoadGuardChainOutcome,
}

/// Data-only guard fact needed before a blocked property-load observation can
/// become an executable access case. These are not installed watchpoint
/// dependencies and do not represent sidecar probe candidates.
#[derive(Clone, Copy, Debug, Eq, PartialEq)]
pub enum PropertyLoadGuardRequirement {
    PrototypeChain,
    NegativeLookup,
}

#[derive(Clone, Debug, Eq, PartialEq)]
pub struct PropertyLoadGuardDescriptor {
    pub requirement: PropertyLoadGuardRequirement,
    pub key: CacheKey,
    pub base_object: ObjectId,
    pub holder_object: Option<ObjectId>,
    pub base_structure: StructureId,
    pub offset: Option<PropertyOffset>,
    pub prototype_depth: u16,
    pub chain: PropertyLoadGuardChainCertificate,
}

#[derive(Clone, Debug, Eq, PartialEq)]
pub struct PropertyLoadGuardPlan {
    pub owner: CodeBlockId,
    pub slot: InlineCacheSlotId,
    pub bytecode_index: u32,
    pub descriptor: PropertyLoadGuardDescriptor,
}

#[derive(Clone, Debug, Eq, PartialEq)]
pub struct PropertyLoadGuardDependencyDescriptor {
    pub chain_index: usize,
    pub set: WatchpointSetDescriptor,
    pub dependency: WatchpointDependency,
}

#[derive(Clone, Copy, Debug, Eq, PartialEq)]
pub enum PropertyLoadGuardedCandidateKind {
    PrototypeData,
    NegativeLookup,
}

/// Immutable JIT-facing sidecar candidate derived from a VM-owned guarded
/// property-load projection. This is data only: it does not install generated
/// execution or mutate an inline-cache slot.
#[derive(Clone, Debug, Eq, PartialEq)]
pub struct PropertyLoadGuardedCandidate {
    pub plan: PropertyLoadGuardPlan,
    pub guard_plan_ordinal: u64,
    pub materialization_ordinal: u64,
    pub dependency_ordinals: Vec<u64>,
    pub binding_set_ids: Vec<WatchpointSetId>,
    pub candidate_kind: PropertyLoadGuardedCandidateKind,
}

/// Immutable sidecar table visible to future JIT property-load probes.
#[derive(Clone, Debug, Eq, PartialEq)]
pub struct PropertyLoadGuardedCandidateTable {
    owner: CodeBlockId,
    candidates: Vec<PropertyLoadGuardedCandidate>,
}

impl PropertyLoadGuardedCandidateTable {
    pub fn new(
        owner: CodeBlockId,
        candidates: Vec<PropertyLoadGuardedCandidate>,
    ) -> Result<Self, InlineCacheValidationError> {
        for candidate in &candidates {
            validate_property_load_guarded_candidate_for_table(owner, candidate)?;
        }

        for (index, candidate) in candidates.iter().enumerate() {
            if candidates[..index]
                .iter()
                .any(|prior| prior.guard_plan_ordinal == candidate.guard_plan_ordinal)
            {
                return Err(
                    InlineCacheValidationError::PropertyLoadGuardedCandidateDuplicateGuardPlanOrdinal(
                        candidate.guard_plan_ordinal,
                    ),
                );
            }

            let semantic_key = property_load_guarded_candidate_semantic_key(candidate);
            if candidates[..index]
                .iter()
                .any(|prior| property_load_guarded_candidate_semantic_key(prior) == semantic_key)
            {
                let (bytecode_index, base_structure, key, requirement, outcome) = semantic_key;
                return Err(
                    InlineCacheValidationError::PropertyLoadGuardedCandidateDuplicate {
                        bytecode_index,
                        base_structure,
                        key,
                        requirement,
                        outcome,
                    },
                );
            }
        }

        Ok(Self { owner, candidates })
    }

    pub const fn owner(&self) -> CodeBlockId {
        self.owner
    }

    pub fn candidates(&self) -> &[PropertyLoadGuardedCandidate] {
        &self.candidates
    }

    pub fn len(&self) -> usize {
        self.candidates.len()
    }

    pub fn is_empty(&self) -> bool {
        self.candidates.is_empty()
    }

    pub fn candidates_for_bytecode_index(
        &self,
        bytecode_index: u32,
    ) -> impl Iterator<Item = &PropertyLoadGuardedCandidate> {
        self.candidates
            .iter()
            .filter(move |candidate| candidate.plan.bytecode_index == bytecode_index)
    }
}

/// Validated, VM-owned data-only table for future generated property-load
/// probes. Construction only validates and organizes plan metadata; it never
/// attaches cases, mutates code-block IC side tables, or owns generated code.
#[derive(Clone, Debug, Eq, PartialEq)]
pub struct PropertyLoadAccessCasePlanTable {
    owner: CodeBlockId,
    plans: Vec<PropertyLoadAccessCasePlan>,
}

impl PropertyLoadAccessCasePlanTable {
    pub fn new(
        owner: CodeBlockId,
        plans: Vec<PropertyLoadAccessCasePlan>,
    ) -> Result<Self, InlineCacheValidationError> {
        for plan in &plans {
            validate_property_load_access_case_plan_for_table(owner, plan)?;
        }

        for (index, plan) in plans.iter().enumerate() {
            for prior in &plans[..index] {
                if property_load_access_case_plan_duplicate_key(prior)
                    == property_load_access_case_plan_duplicate_key(plan)
                {
                    let (bytecode_index, base_structure, offset, key) =
                        property_load_access_case_plan_duplicate_key(plan);
                    return Err(InlineCacheValidationError::PropertyLoadPlanDuplicate {
                        bytecode_index,
                        base_structure,
                        offset,
                        key,
                    });
                }
            }
        }

        Ok(Self { owner, plans })
    }

    pub const fn owner(&self) -> CodeBlockId {
        self.owner
    }

    pub fn plans(&self) -> &[PropertyLoadAccessCasePlan] {
        &self.plans
    }

    pub fn len(&self) -> usize {
        self.plans.len()
    }

    pub fn is_empty(&self) -> bool {
        self.plans.is_empty()
    }

    pub fn candidates_for_bytecode_index(
        &self,
        bytecode_index: u32,
    ) -> impl Iterator<Item = &PropertyLoadAccessCasePlan> {
        self.plans
            .iter()
            .filter(move |plan| plan.bytecode_index == bytecode_index)
    }

    pub fn candidates_for_bytecode_index_newest_first(
        &self,
        bytecode_index: u32,
    ) -> impl Iterator<Item = &PropertyLoadAccessCasePlan> {
        self.plans
            .iter()
            .rev()
            .filter(move |plan| plan.bytecode_index == bytecode_index)
    }
}

/// Validated, immutable table for future property-store access-case metadata.
///
/// Construction only validates the data contract. It does not publish plans to
/// VM tiering state, mutate CodeBlock IC vectors, install watchpoints, or emit
/// active store probes/stubs.
#[derive(Clone, Debug, Eq, PartialEq)]
#[allow(dead_code)]
pub struct PropertyStoreAccessCasePlanTable {
    owner: CodeBlockId,
    plans: Vec<PropertyStoreAccessCasePlan>,
}

#[allow(dead_code)]
impl PropertyStoreAccessCasePlanTable {
    pub fn new(
        owner: CodeBlockId,
        plans: Vec<PropertyStoreAccessCasePlan>,
    ) -> Result<Self, InlineCacheValidationError> {
        for plan in &plans {
            validate_property_store_access_case_plan_for_table(owner, plan)?;
        }

        for (index, plan) in plans.iter().enumerate() {
            for prior in &plans[..index] {
                if property_store_access_case_plan_duplicate_key(prior)
                    == property_store_access_case_plan_duplicate_key(plan)
                {
                    let (bytecode_index, key, base_structure, offset, new_structure) =
                        property_store_access_case_plan_duplicate_key(plan);
                    return Err(InlineCacheValidationError::PropertyStorePlanDuplicate {
                        bytecode_index,
                        key,
                        base_structure,
                        offset,
                        new_structure,
                    });
                }
            }
        }

        Ok(Self { owner, plans })
    }

    pub const fn owner(&self) -> CodeBlockId {
        self.owner
    }

    pub fn plans(&self) -> &[PropertyStoreAccessCasePlan] {
        &self.plans
    }

    pub fn len(&self) -> usize {
        self.plans.len()
    }

    pub fn is_empty(&self) -> bool {
        self.plans.is_empty()
    }

    pub fn candidates_for_bytecode_index(
        &self,
        bytecode_index: u32,
    ) -> impl Iterator<Item = &PropertyStoreAccessCasePlan> {
        self.plans
            .iter()
            .filter(move |plan| plan.bytecode_index == bytecode_index)
    }
}

/// JIT-local summary of VM-accepted property-store mutation barrier readiness.
///
/// This mirrors the metadata a future generated store sidecar may inspect, but
/// remains data only. It does not import VM records, run a barrier, or confer
/// authority to write heap state.
#[derive(Clone, Copy, Debug, Eq, PartialEq)]
pub struct PropertyStoreMutationBarrierEvidence {
    pub plan_kind: PropertyStoreAccessCasePlanKind,
    pub effect_contract: PropertyStoreAccessCasePlanContract,
    pub barrier_effect: PropertyStoreBarrierEffect,
    pub observed_write_barrier_count: u32,
    pub last_write_barrier: BarrierRequirementOutcome,
}

#[derive(Clone, Copy, Debug, Eq, PartialEq)]
pub enum PropertyStoreMutationBarrierEvidenceMismatchField {
    PlanKind,
    EffectContract,
    BarrierEffect,
}

/// Immutable JIT-facing property-store mutation candidate.
///
/// Candidates are derived from accepted VM readiness provenance before any
/// generated store execution exists. They keep the active store plan snapshot
/// separate from the readiness ordinals and accepted barrier evidence.
#[derive(Clone, Debug, Eq, PartialEq)]
pub struct PropertyStoreMutationCandidate {
    pub plan: PropertyStoreAccessCasePlan,
    pub store_plan_ordinal: u64,
    pub install_recheck_ordinal: u64,
    pub readiness_ordinal: u64,
    pub observation_ordinal: u64,
    pub barrier_evidence: PropertyStoreMutationBarrierEvidence,
    pub stored_value_kind: ValueKind,
}

/// Validated, immutable table for future property-store mutation sidecars.
///
/// Construction validates candidate provenance and barrier evidence only. It
/// never mutates CodeBlock IC state, invalidates artifacts, calls into the VM,
/// executes generated code, or runs write barriers.
#[derive(Clone, Debug, Eq, PartialEq)]
pub struct PropertyStoreMutationCandidateTable {
    owner: CodeBlockId,
    candidates: Vec<PropertyStoreMutationCandidate>,
}

impl PropertyStoreMutationCandidateTable {
    pub fn new(
        owner: CodeBlockId,
        candidates: Vec<PropertyStoreMutationCandidate>,
    ) -> Result<Self, InlineCacheValidationError> {
        for candidate in &candidates {
            validate_property_store_mutation_candidate_for_table(owner, candidate)?;
        }

        for (index, candidate) in candidates.iter().enumerate() {
            if candidates[..index]
                .iter()
                .any(|prior| prior.store_plan_ordinal == candidate.store_plan_ordinal)
            {
                return Err(
                    InlineCacheValidationError::PropertyStoreMutationCandidateDuplicateStorePlanOrdinal(
                        candidate.store_plan_ordinal,
                    ),
                );
            }

            if candidates[..index]
                .iter()
                .any(|prior| prior.readiness_ordinal == candidate.readiness_ordinal)
            {
                return Err(
                    InlineCacheValidationError::PropertyStoreMutationCandidateDuplicateReadinessOrdinal(
                        candidate.readiness_ordinal,
                    ),
                );
            }

            let semantic_key = property_store_mutation_candidate_semantic_key(candidate);
            if candidates[..index]
                .iter()
                .any(|prior| property_store_mutation_candidate_semantic_key(prior) == semantic_key)
            {
                let (bytecode_index, key, base_structure, offset, new_structure) = semantic_key;
                return Err(
                    InlineCacheValidationError::PropertyStoreMutationCandidateDuplicate {
                        store_plan_ordinal: candidate.store_plan_ordinal,
                        bytecode_index,
                        key,
                        base_structure,
                        offset,
                        new_structure,
                    },
                );
            }
        }

        Ok(Self { owner, candidates })
    }

    pub const fn owner(&self) -> CodeBlockId {
        self.owner
    }

    pub fn candidates(&self) -> &[PropertyStoreMutationCandidate] {
        &self.candidates
    }

    pub fn len(&self) -> usize {
        self.candidates.len()
    }

    pub fn is_empty(&self) -> bool {
        self.candidates.is_empty()
    }

    pub fn candidates_for_bytecode_index(
        &self,
        bytecode_index: u32,
    ) -> impl Iterator<Item = &PropertyStoreMutationCandidate> {
        self.candidates
            .iter()
            .filter(move |candidate| candidate.plan.bytecode_index == bytecode_index)
    }

    pub fn candidates_for_bytecode_index_newest_first(
        &self,
        bytecode_index: u32,
    ) -> impl Iterator<Item = &PropertyStoreMutationCandidate> {
        self.candidates
            .iter()
            .rev()
            .filter(move |candidate| candidate.plan.bytecode_index == bytecode_index)
    }
}

/// Megamorphic load site metadata projected for generated property sidecars.
///
/// This names the bytecode site that C++ JSC would patch to a
/// `LoadMegamorphic` access case. The mutable cache table itself remains
/// VM-owned; generated execution receives an immutable snapshot and performs a
/// direct primary/secondary lookup from the active base structure.
#[derive(Clone, Debug, Eq, PartialEq)]
pub struct GeneratedPropertyLoadMegamorphicSite {
    pub owner: CodeBlockId,
    pub slot: InlineCacheSlotId,
    pub bytecode_index: u32,
    pub key: CacheKey,
}

/// One load entry from the VM-wide megamorphic cache snapshot.
#[derive(Clone, Debug, Eq, PartialEq)]
pub struct GeneratedPropertyLoadMegamorphicCacheEntry {
    pub key: CacheKey,
    pub base_structure: StructureId,
    pub epoch: u16,
    pub kind: GeneratedPropertyLoadMegamorphicCacheEntryKind,
}

/// Safe-Rust spelling of C++ JSC's load megamorphic holder encoding.
#[derive(Clone, Copy, Debug, Eq, PartialEq)]
pub enum GeneratedPropertyLoadMegamorphicCacheEntryKind {
    /// C++ uses `JSCell::seenMultipleCalleeObjects()` as an own-property
    /// sentinel and stores a `uint16_t` offset.
    OwnData { offset: PropertyOffset },
    /// C++ stores the actual prototype/holder object and a `uint16_t` offset.
    PrototypeData {
        holder: ObjectId,
        offset: PropertyOffset,
    },
    /// C++ uses a null holder and returns `undefined`.
    Missing,
}

/// Result of a generated-side megamorphic lookup for one load site.
#[derive(Clone, Debug, Eq, PartialEq)]
pub enum GeneratedPropertyLoadMegamorphicLookup {
    NoSite,
    Miss,
    Hit(PropertyLoadAccessCasePlan),
    PrototypeData {
        key: CacheKey,
        base_structure: StructureId,
        holder: ObjectId,
        offset: PropertyOffset,
    },
    Missing,
}

/// Immutable generated-side view of the VM-owned load megamorphic cache.
///
/// Lookup intentionally mirrors C++ JSC's primary-then-secondary behavior. If
/// the primary entry matches `(StructureID, uid)` but has a stale epoch, lookup
/// stops and reports a miss; secondary is considered only when the primary
/// entry is for a different structure or uid.
#[derive(Clone, Debug, Eq, PartialEq)]
pub struct GeneratedPropertyLoadMegamorphicCandidateTable {
    owner: CodeBlockId,
    epoch: u16,
    sites: Vec<GeneratedPropertyLoadMegamorphicSite>,
    primary_entries: Vec<Option<GeneratedPropertyLoadMegamorphicCacheEntry>>,
    secondary_entries: Vec<Option<GeneratedPropertyLoadMegamorphicCacheEntry>>,
}

impl GeneratedPropertyLoadMegamorphicCandidateTable {
    pub fn new(
        owner: CodeBlockId,
        epoch: u16,
        sites: Vec<GeneratedPropertyLoadMegamorphicSite>,
        primary_entries: Vec<Option<GeneratedPropertyLoadMegamorphicCacheEntry>>,
        secondary_entries: Vec<Option<GeneratedPropertyLoadMegamorphicCacheEntry>>,
    ) -> Result<Self, InlineCacheValidationError> {
        for site in &sites {
            if site.owner != owner {
                return Err(InlineCacheValidationError::PropertyLoadPlanOwnerMismatch {
                    expected: owner,
                    actual: site.owner,
                });
            }
            if !generated_property_load_megamorphic_cache_key_supported(site.key) {
                return Err(InlineCacheValidationError::PropertyLoadPlanUnsupportedKey(
                    site.key,
                ));
            }
        }

        Ok(Self {
            owner,
            epoch,
            sites,
            primary_entries,
            secondary_entries,
        })
    }

    pub const fn owner(&self) -> CodeBlockId {
        self.owner
    }

    pub const fn epoch(&self) -> u16 {
        self.epoch
    }

    pub fn sites(&self) -> &[GeneratedPropertyLoadMegamorphicSite] {
        &self.sites
    }

    pub fn current_entry_count(&self) -> usize {
        self.primary_entries
            .iter()
            .chain(self.secondary_entries.iter())
            .filter_map(Option::as_ref)
            .filter(|entry| entry.epoch == self.epoch)
            .count()
    }

    pub fn is_empty(&self) -> bool {
        self.sites.is_empty() || self.current_entry_count() == 0
    }

    pub fn contains_site(
        &self,
        slot: InlineCacheSlotId,
        bytecode_index: u32,
        key: CacheKey,
    ) -> bool {
        self.sites.iter().any(|site| {
            site.slot == slot && site.bytecode_index == bytecode_index && site.key == key
        })
    }

    pub fn lookup(
        &self,
        slot: InlineCacheSlotId,
        bytecode_index: u32,
        key: CacheKey,
        base_structure: StructureId,
    ) -> GeneratedPropertyLoadMegamorphicLookup {
        let Some(site) = self.sites.iter().find(|site| {
            site.slot == slot && site.bytecode_index == bytecode_index && site.key == key
        }) else {
            return GeneratedPropertyLoadMegamorphicLookup::NoSite;
        };
        if base_structure == StructureId::INVALID
            || !generated_property_load_megamorphic_cache_key_supported(key)
        {
            return GeneratedPropertyLoadMegamorphicLookup::Miss;
        }

        let primary_index = generated_property_load_megamorphic_cache_primary_index(
            base_structure,
            key,
            self.primary_entries.len(),
        );
        let Some(primary_index) = primary_index else {
            return GeneratedPropertyLoadMegamorphicLookup::Miss;
        };
        if let Some(Some(entry)) = self.primary_entries.get(primary_index) {
            if entry.base_structure == base_structure && entry.key == key {
                return if self.entry_is_current_hit(entry) {
                    self.lookup_result_for_current_entry(site, entry)
                } else {
                    GeneratedPropertyLoadMegamorphicLookup::Miss
                };
            }
        }

        let secondary_index = generated_property_load_megamorphic_cache_secondary_index(
            base_structure,
            key,
            self.secondary_entries.len(),
        );
        let Some(secondary_index) = secondary_index else {
            return GeneratedPropertyLoadMegamorphicLookup::Miss;
        };
        let Some(Some(entry)) = self.secondary_entries.get(secondary_index) else {
            return GeneratedPropertyLoadMegamorphicLookup::Miss;
        };
        if entry.base_structure == base_structure
            && entry.key == key
            && self.entry_is_current_hit(entry)
        {
            self.lookup_result_for_current_entry(site, entry)
        } else {
            GeneratedPropertyLoadMegamorphicLookup::Miss
        }
    }

    #[cfg(test)]
    pub(crate) fn test_with_primary_entry(
        owner: CodeBlockId,
        epoch: u16,
        site: GeneratedPropertyLoadMegamorphicSite,
        entry: GeneratedPropertyLoadMegamorphicCacheEntry,
    ) -> Self {
        let mut primary_entries = vec![None; 2048];
        let secondary_entries = vec![None; 512];
        let primary_index = generated_property_load_megamorphic_cache_primary_index(
            entry.base_structure,
            entry.key,
            primary_entries.len(),
        )
        .expect("primary index");
        primary_entries[primary_index] = Some(entry);
        Self::new(owner, epoch, vec![site], primary_entries, secondary_entries)
            .expect("test megamorphic table")
    }

    fn entry_is_current_hit(&self, entry: &GeneratedPropertyLoadMegamorphicCacheEntry) -> bool {
        entry.epoch == self.epoch
            && entry.base_structure != StructureId::INVALID
            && match entry.kind {
                GeneratedPropertyLoadMegamorphicCacheEntryKind::OwnData { offset } => {
                    offset != PropertyOffset::INVALID
                        && offset.raw() >= 0
                        && offset.raw() <= u16::MAX as i32
                }
                GeneratedPropertyLoadMegamorphicCacheEntryKind::PrototypeData {
                    holder,
                    offset,
                } => {
                    holder.0 != CellId::default()
                        && offset != PropertyOffset::INVALID
                        && offset.raw() >= 0
                        && offset.raw() <= u16::MAX as i32
                }
                GeneratedPropertyLoadMegamorphicCacheEntryKind::Missing => true,
            }
    }

    fn lookup_result_for_current_entry(
        &self,
        site: &GeneratedPropertyLoadMegamorphicSite,
        entry: &GeneratedPropertyLoadMegamorphicCacheEntry,
    ) -> GeneratedPropertyLoadMegamorphicLookup {
        match entry.kind {
            GeneratedPropertyLoadMegamorphicCacheEntryKind::OwnData { .. } => {
                GeneratedPropertyLoadMegamorphicLookup::Hit(self.plan_for_entry(site, entry))
            }
            GeneratedPropertyLoadMegamorphicCacheEntryKind::PrototypeData { holder, offset } => {
                GeneratedPropertyLoadMegamorphicLookup::PrototypeData {
                    key: site.key,
                    base_structure: entry.base_structure,
                    holder,
                    offset,
                }
            }
            GeneratedPropertyLoadMegamorphicCacheEntryKind::Missing => {
                GeneratedPropertyLoadMegamorphicLookup::Missing
            }
        }
    }

    fn plan_for_entry(
        &self,
        site: &GeneratedPropertyLoadMegamorphicSite,
        entry: &GeneratedPropertyLoadMegamorphicCacheEntry,
    ) -> PropertyLoadAccessCasePlan {
        let GeneratedPropertyLoadMegamorphicCacheEntryKind::OwnData { offset } = entry.kind else {
            unreachable!("missing megamorphic load entries do not synthesize own-load plans")
        };
        let access_case = AccessCaseDescriptor {
            kind: AccessCaseKind::Load,
            key: site.key,
            base_structure: Some(entry.base_structure),
            new_structure: None,
            holder: None,
            offset: Some(offset),
            via_global_proxy: false,
            may_call_js: false,
            dependencies: Vec::new(),
        };

        PropertyLoadAccessCasePlan {
            plan_kind: PropertyLoadAccessCasePlanKind::DataOnlyOwnLoad,
            owner: site.owner,
            slot: site.slot,
            bytecode_index: site.bytecode_index,
            key: site.key,
            access_case,
            planned_stub_kind: InlineCacheStubKind::DataOnlyHandler,
            effect_contract: PropertyLoadAccessCasePlanContract::DATA_ONLY_OWN_LOAD,
        }
    }
}

fn generated_property_load_megamorphic_cache_key_supported(key: CacheKey) -> bool {
    matches!(key, CacheKey::Property(PropertyKey::String(_)))
}

fn generated_property_load_megamorphic_cache_key_hash(key: CacheKey) -> Option<u32> {
    let CacheKey::Property(PropertyKey::String(identifier)) = key else {
        return None;
    };
    // C++ hashes the UID and uses the UID pointer for secondary indexing. Rust
    // currently exposes atom identity to the generated side, so the hash is a
    // stable atom-slot surrogate and every consumed hit still requires exact
    // key equality.
    Some(
        identifier
            .atom()
            .table_slot()
            .wrapping_mul(0x9E37_79B1)
            .rotate_left(5),
    )
}

fn generated_property_load_megamorphic_cache_primary_index(
    structure: StructureId,
    key: CacheKey,
    table_len: usize,
) -> Option<usize> {
    if table_len == 0 || !table_len.is_power_of_two() {
        return None;
    }
    let sid = structure.0;
    let hash = ((sid >> 4) ^ (sid >> 15))
        .wrapping_add(generated_property_load_megamorphic_cache_key_hash(key)?);
    Some((hash as usize) & (table_len - 1))
}

fn generated_property_load_megamorphic_cache_secondary_index(
    structure: StructureId,
    key: CacheKey,
    table_len: usize,
) -> Option<usize> {
    if table_len == 0 || !table_len.is_power_of_two() {
        return None;
    }
    let key_hash = generated_property_load_megamorphic_cache_key_hash(key)?;
    let hash = structure.0.wrapping_add(key_hash);
    Some(((hash.wrapping_add(hash >> 13)) as usize) & (table_len - 1))
}

/// Megamorphic store site metadata projected for generated property sidecars.
///
/// This is the Rust spelling of C++ JSC's `StoreMegamorphic` access case for
/// named put-by-id sites. The VM owns the mutable cache table; generated
/// execution receives a snapshot and still routes the heap mutation through the
/// host-owned store commit boundary.
#[derive(Clone, Debug, Eq, PartialEq)]
pub struct GeneratedPropertyStoreMegamorphicSite {
    pub owner: CodeBlockId,
    pub slot: InlineCacheSlotId,
    pub bytecode_index: u32,
    pub key: CacheKey,
}

/// One store entry from the VM-wide megamorphic cache snapshot.
#[derive(Clone, Debug, Eq, PartialEq)]
pub struct GeneratedPropertyStoreMegamorphicCacheEntry {
    pub key: CacheKey,
    pub old_structure: StructureId,
    pub new_structure: StructureId,
    pub epoch: u16,
    pub offset: PropertyOffset,
    pub reallocating: bool,
}

/// Result of a generated-side megamorphic lookup for one store site.
#[derive(Clone, Debug, Eq, PartialEq)]
pub enum GeneratedPropertyStoreMegamorphicLookup {
    NoSite,
    Miss,
    Hit(PropertyStoreAccessCasePlan),
}

/// Immutable generated-side view of the VM-owned store megamorphic cache.
#[derive(Clone, Debug, Eq, PartialEq)]
pub struct GeneratedPropertyStoreMegamorphicCandidateTable {
    owner: CodeBlockId,
    epoch: u16,
    sites: Vec<GeneratedPropertyStoreMegamorphicSite>,
    primary_entries: Vec<Option<GeneratedPropertyStoreMegamorphicCacheEntry>>,
    secondary_entries: Vec<Option<GeneratedPropertyStoreMegamorphicCacheEntry>>,
}

impl GeneratedPropertyStoreMegamorphicCandidateTable {
    pub fn new(
        owner: CodeBlockId,
        epoch: u16,
        sites: Vec<GeneratedPropertyStoreMegamorphicSite>,
        primary_entries: Vec<Option<GeneratedPropertyStoreMegamorphicCacheEntry>>,
        secondary_entries: Vec<Option<GeneratedPropertyStoreMegamorphicCacheEntry>>,
    ) -> Result<Self, InlineCacheValidationError> {
        for site in &sites {
            if site.owner != owner {
                return Err(InlineCacheValidationError::PropertyStorePlanOwnerMismatch {
                    expected: owner,
                    actual: site.owner,
                });
            }
            if !generated_property_store_megamorphic_cache_key_supported(site.key) {
                return Err(InlineCacheValidationError::PropertyStorePlanUnsupportedKey(
                    site.key,
                ));
            }
        }

        Ok(Self {
            owner,
            epoch,
            sites,
            primary_entries,
            secondary_entries,
        })
    }

    pub const fn owner(&self) -> CodeBlockId {
        self.owner
    }

    pub const fn epoch(&self) -> u16 {
        self.epoch
    }

    pub fn sites(&self) -> &[GeneratedPropertyStoreMegamorphicSite] {
        &self.sites
    }

    pub fn current_entry_count(&self) -> usize {
        self.primary_entries
            .iter()
            .chain(self.secondary_entries.iter())
            .filter_map(Option::as_ref)
            .filter(|entry| entry.epoch == self.epoch)
            .count()
    }

    pub fn is_empty(&self) -> bool {
        self.sites.is_empty() || self.current_entry_count() == 0
    }

    pub fn lookup(
        &self,
        slot: InlineCacheSlotId,
        bytecode_index: u32,
        key: CacheKey,
        base_structure: StructureId,
    ) -> GeneratedPropertyStoreMegamorphicLookup {
        let Some(site) = self.sites.iter().find(|site| {
            site.slot == slot && site.bytecode_index == bytecode_index && site.key == key
        }) else {
            return GeneratedPropertyStoreMegamorphicLookup::NoSite;
        };
        if base_structure == StructureId::INVALID
            || !generated_property_store_megamorphic_cache_key_supported(key)
        {
            return GeneratedPropertyStoreMegamorphicLookup::Miss;
        }

        let primary_index = generated_property_store_megamorphic_cache_primary_index(
            base_structure,
            key,
            self.primary_entries.len(),
        );
        let Some(primary_index) = primary_index else {
            return GeneratedPropertyStoreMegamorphicLookup::Miss;
        };
        if let Some(Some(entry)) = self.primary_entries.get(primary_index) {
            if entry.old_structure == base_structure && entry.key == key {
                return if self.entry_is_current_hit(entry) {
                    GeneratedPropertyStoreMegamorphicLookup::Hit(self.plan_for_entry(site, entry))
                } else {
                    GeneratedPropertyStoreMegamorphicLookup::Miss
                };
            }
        }

        let secondary_index = generated_property_store_megamorphic_cache_secondary_index(
            base_structure,
            key,
            self.secondary_entries.len(),
        );
        let Some(secondary_index) = secondary_index else {
            return GeneratedPropertyStoreMegamorphicLookup::Miss;
        };
        let Some(Some(entry)) = self.secondary_entries.get(secondary_index) else {
            return GeneratedPropertyStoreMegamorphicLookup::Miss;
        };
        if entry.old_structure == base_structure
            && entry.key == key
            && self.entry_is_current_hit(entry)
        {
            GeneratedPropertyStoreMegamorphicLookup::Hit(self.plan_for_entry(site, entry))
        } else {
            GeneratedPropertyStoreMegamorphicLookup::Miss
        }
    }

    #[cfg(test)]
    pub(crate) fn test_with_primary_entry(
        owner: CodeBlockId,
        epoch: u16,
        site: GeneratedPropertyStoreMegamorphicSite,
        entry: GeneratedPropertyStoreMegamorphicCacheEntry,
    ) -> Self {
        let mut primary_entries = vec![None; 2048];
        let secondary_entries = vec![None; 512];
        let primary_index = generated_property_store_megamorphic_cache_primary_index(
            entry.old_structure,
            entry.key,
            primary_entries.len(),
        )
        .expect("primary index");
        primary_entries[primary_index] = Some(entry);
        Self::new(owner, epoch, vec![site], primary_entries, secondary_entries)
            .expect("test megamorphic store table")
    }

    fn entry_is_current_hit(&self, entry: &GeneratedPropertyStoreMegamorphicCacheEntry) -> bool {
        entry.epoch == self.epoch
            && entry.old_structure != StructureId::INVALID
            && entry.new_structure != StructureId::INVALID
            && entry.offset != PropertyOffset::INVALID
            && entry.offset.raw() >= 0
            && entry.offset.raw() <= u16::MAX as i32
            && generated_property_store_megamorphic_cache_key_supported(entry.key)
    }

    fn plan_for_entry(
        &self,
        site: &GeneratedPropertyStoreMegamorphicSite,
        entry: &GeneratedPropertyStoreMegamorphicCacheEntry,
    ) -> PropertyStoreAccessCasePlan {
        let is_replace = entry.old_structure == entry.new_structure;
        let (plan_kind, access_case_kind, new_structure, effect_contract) = if is_replace {
            (
                PropertyStoreAccessCasePlanKind::DataOnlyReplace,
                AccessCaseKind::Replace,
                None,
                PropertyStoreAccessCasePlanContract::DATA_ONLY_REPLACE,
            )
        } else {
            (
                PropertyStoreAccessCasePlanKind::DataOnlyTransition,
                AccessCaseKind::Transition,
                Some(entry.new_structure),
                PropertyStoreAccessCasePlanContract::DATA_ONLY_TRANSITION,
            )
        };
        let access_case = AccessCaseDescriptor {
            kind: access_case_kind,
            key: site.key,
            base_structure: Some(entry.old_structure),
            new_structure,
            holder: None,
            offset: Some(entry.offset),
            via_global_proxy: false,
            may_call_js: false,
            dependencies: Vec::new(),
        };

        PropertyStoreAccessCasePlan {
            plan_kind,
            owner: site.owner,
            slot: site.slot,
            bytecode_index: site.bytecode_index,
            key: site.key,
            access_case,
            planned_stub_kind: InlineCacheStubKind::RepatchingStub,
            effect_contract,
        }
    }
}

fn generated_property_store_megamorphic_cache_key_supported(key: CacheKey) -> bool {
    matches!(key, CacheKey::Property(property_key) if property_key.as_identifier().is_some())
}

fn generated_property_store_megamorphic_cache_key_hash(key: CacheKey) -> Option<u32> {
    let CacheKey::Property(PropertyKey::String(identifier)) = key else {
        return None;
    };
    Some(
        identifier
            .atom()
            .table_slot()
            .wrapping_mul(0x9E37_79B1)
            .rotate_left(5),
    )
}

fn generated_property_store_megamorphic_cache_primary_index(
    structure: StructureId,
    key: CacheKey,
    table_len: usize,
) -> Option<usize> {
    if table_len == 0 || !table_len.is_power_of_two() {
        return None;
    }
    let sid = structure.0;
    let hash = ((sid >> 4) ^ (sid >> 15))
        .wrapping_add(generated_property_store_megamorphic_cache_key_hash(key)?);
    Some((hash as usize) & (table_len - 1))
}

fn generated_property_store_megamorphic_cache_secondary_index(
    structure: StructureId,
    key: CacheKey,
    table_len: usize,
) -> Option<usize> {
    if table_len == 0 || !table_len.is_power_of_two() {
        return None;
    }
    let key_hash = generated_property_store_megamorphic_cache_key_hash(key)?;
    let hash = structure.0.wrapping_add(key_hash);
    Some(((hash.wrapping_add(hash >> 13)) as usize) & (table_len - 1))
}

/// Megamorphic has/in site metadata projected for generated property sidecars.
///
/// C++ JSC uses this cache for `InById` and eligible `InByVal` string/symbol
/// keys. The current Rust frontend parses `in` but has no executable core
/// opcode yet, so this table is a faithful substrate for that future boundary.
#[derive(Clone, Debug, Eq, PartialEq)]
pub struct GeneratedPropertyHasMegamorphicSite {
    pub owner: CodeBlockId,
    pub slot: InlineCacheSlotId,
    pub bytecode_index: u32,
    pub key: CacheKey,
}

/// One has/in entry from the VM-wide megamorphic cache snapshot.
#[derive(Clone, Debug, Eq, PartialEq)]
pub struct GeneratedPropertyHasMegamorphicCacheEntry {
    pub key: CacheKey,
    pub base_structure: StructureId,
    pub epoch: u16,
    pub result: bool,
}

/// Result of a generated-side megamorphic lookup for one has/in site.
#[derive(Clone, Copy, Debug, Eq, PartialEq)]
pub enum GeneratedPropertyHasMegamorphicLookup {
    NoSite,
    Miss,
    Hit(bool),
}

/// Immutable generated-side view of the VM-owned has/in megamorphic cache.
///
/// Lookup mirrors C++ JSC's `hasMegamorphicProperty`: primary lookup first,
/// stale primary `(StructureID, uid)` matches stop without consulting
/// secondary, and secondary is only checked after primary structure/key miss.
#[derive(Clone, Debug, Eq, PartialEq)]
pub struct GeneratedPropertyHasMegamorphicCandidateTable {
    owner: CodeBlockId,
    epoch: u16,
    sites: Vec<GeneratedPropertyHasMegamorphicSite>,
    primary_entries: Vec<Option<GeneratedPropertyHasMegamorphicCacheEntry>>,
    secondary_entries: Vec<Option<GeneratedPropertyHasMegamorphicCacheEntry>>,
}

impl GeneratedPropertyHasMegamorphicCandidateTable {
    pub fn new(
        owner: CodeBlockId,
        epoch: u16,
        sites: Vec<GeneratedPropertyHasMegamorphicSite>,
        primary_entries: Vec<Option<GeneratedPropertyHasMegamorphicCacheEntry>>,
        secondary_entries: Vec<Option<GeneratedPropertyHasMegamorphicCacheEntry>>,
    ) -> Result<Self, InlineCacheValidationError> {
        for site in &sites {
            if site.owner != owner {
                return Err(InlineCacheValidationError::PropertyHasPlanOwnerMismatch {
                    expected: owner,
                    actual: site.owner,
                });
            }
            if !generated_property_has_megamorphic_cache_key_supported(site.key) {
                return Err(InlineCacheValidationError::PropertyHasPlanUnsupportedKey(
                    site.key,
                ));
            }
        }

        Ok(Self {
            owner,
            epoch,
            sites,
            primary_entries,
            secondary_entries,
        })
    }

    pub const fn owner(&self) -> CodeBlockId {
        self.owner
    }

    pub const fn epoch(&self) -> u16 {
        self.epoch
    }

    pub fn sites(&self) -> &[GeneratedPropertyHasMegamorphicSite] {
        &self.sites
    }

    pub fn current_entry_count(&self) -> usize {
        self.primary_entries
            .iter()
            .chain(self.secondary_entries.iter())
            .filter_map(Option::as_ref)
            .filter(|entry| entry.epoch == self.epoch)
            .count()
    }

    pub fn is_empty(&self) -> bool {
        self.sites.is_empty() || self.current_entry_count() == 0
    }

    pub fn contains_site(
        &self,
        slot: InlineCacheSlotId,
        bytecode_index: u32,
        key: CacheKey,
    ) -> bool {
        self.sites.iter().any(|site| {
            site.slot == slot && site.bytecode_index == bytecode_index && site.key == key
        })
    }

    pub fn lookup(
        &self,
        slot: InlineCacheSlotId,
        bytecode_index: u32,
        key: CacheKey,
        base_structure: StructureId,
    ) -> GeneratedPropertyHasMegamorphicLookup {
        if !self.contains_site(slot, bytecode_index, key) {
            return GeneratedPropertyHasMegamorphicLookup::NoSite;
        }
        if base_structure == StructureId::INVALID
            || !generated_property_has_megamorphic_cache_key_supported(key)
        {
            return GeneratedPropertyHasMegamorphicLookup::Miss;
        }

        let primary_index = generated_property_has_megamorphic_cache_primary_index(
            base_structure,
            key,
            self.primary_entries.len(),
        );
        let Some(primary_index) = primary_index else {
            return GeneratedPropertyHasMegamorphicLookup::Miss;
        };
        if let Some(Some(entry)) = self.primary_entries.get(primary_index) {
            if entry.base_structure == base_structure && entry.key == key {
                return if self.entry_is_current_hit(entry) {
                    GeneratedPropertyHasMegamorphicLookup::Hit(entry.result)
                } else {
                    GeneratedPropertyHasMegamorphicLookup::Miss
                };
            }
        }

        let secondary_index = generated_property_has_megamorphic_cache_secondary_index(
            base_structure,
            key,
            self.secondary_entries.len(),
        );
        let Some(secondary_index) = secondary_index else {
            return GeneratedPropertyHasMegamorphicLookup::Miss;
        };
        let Some(Some(entry)) = self.secondary_entries.get(secondary_index) else {
            return GeneratedPropertyHasMegamorphicLookup::Miss;
        };
        if entry.base_structure == base_structure
            && entry.key == key
            && self.entry_is_current_hit(entry)
        {
            GeneratedPropertyHasMegamorphicLookup::Hit(entry.result)
        } else {
            GeneratedPropertyHasMegamorphicLookup::Miss
        }
    }

    #[cfg(test)]
    pub(crate) fn test_with_primary_entry(
        owner: CodeBlockId,
        epoch: u16,
        site: GeneratedPropertyHasMegamorphicSite,
        entry: GeneratedPropertyHasMegamorphicCacheEntry,
    ) -> Self {
        let mut primary_entries = vec![None; 512];
        let secondary_entries = vec![None; 128];
        let primary_index = generated_property_has_megamorphic_cache_primary_index(
            entry.base_structure,
            entry.key,
            primary_entries.len(),
        )
        .expect("primary index");
        primary_entries[primary_index] = Some(entry);
        Self::new(owner, epoch, vec![site], primary_entries, secondary_entries)
            .expect("test megamorphic has table")
    }

    fn entry_is_current_hit(&self, entry: &GeneratedPropertyHasMegamorphicCacheEntry) -> bool {
        entry.epoch == self.epoch
            && entry.base_structure != StructureId::INVALID
            && generated_property_has_megamorphic_cache_key_supported(entry.key)
    }
}

fn generated_property_has_megamorphic_cache_key_supported(key: CacheKey) -> bool {
    matches!(key, CacheKey::Property(PropertyKey::String(_)))
}

fn generated_property_has_megamorphic_cache_key_hash(key: CacheKey) -> Option<u32> {
    let CacheKey::Property(PropertyKey::String(identifier)) = key else {
        return None;
    };
    Some(
        identifier
            .atom()
            .table_slot()
            .wrapping_mul(0x9E37_79B1)
            .rotate_left(5),
    )
}

fn generated_property_has_megamorphic_cache_primary_index(
    structure: StructureId,
    key: CacheKey,
    table_len: usize,
) -> Option<usize> {
    if table_len == 0 || !table_len.is_power_of_two() {
        return None;
    }
    let sid = structure.0;
    let hash = ((sid >> 4) ^ (sid >> 13))
        .wrapping_add(generated_property_has_megamorphic_cache_key_hash(key)?);
    Some((hash as usize) & (table_len - 1))
}

fn generated_property_has_megamorphic_cache_secondary_index(
    structure: StructureId,
    key: CacheKey,
    table_len: usize,
) -> Option<usize> {
    if table_len == 0 || !table_len.is_power_of_two() {
        return None;
    }
    let key_hash = generated_property_has_megamorphic_cache_key_hash(key)?;
    let hash = structure.0.wrapping_add(key_hash);
    Some(((hash.wrapping_add(hash >> 11)) as usize) & (table_len - 1))
}

fn validate_property_load_access_case_plan_for_table(
    owner: CodeBlockId,
    plan: &PropertyLoadAccessCasePlan,
) -> Result<(), InlineCacheValidationError> {
    if plan.owner != owner {
        return Err(InlineCacheValidationError::PropertyLoadPlanOwnerMismatch {
            expected: owner,
            actual: plan.owner,
        });
    }
    let indexed_load = match plan.plan_kind {
        PropertyLoadAccessCasePlanKind::DataOnlyOwnLoad => false,
        PropertyLoadAccessCasePlanKind::DataOnlyIndexedLoad => true,
    };
    if plan.planned_stub_kind != InlineCacheStubKind::DataOnlyHandler {
        return Err(
            InlineCacheValidationError::PropertyLoadPlanUnsupportedStubKind(plan.planned_stub_kind),
        );
    }
    if (!indexed_load && !plan.effect_contract.supports_generated_data_only_own_load())
        || (indexed_load
            && !plan
                .effect_contract
                .supports_generated_data_only_indexed_load())
    {
        return Err(
            InlineCacheValidationError::PropertyLoadPlanUnsupportedEffectContract(
                plan.effect_contract,
            ),
        );
    }
    let expected_access_case = if indexed_load {
        AccessCaseKind::IndexedLoad
    } else {
        AccessCaseKind::Load
    };
    if plan.access_case.kind != expected_access_case {
        return Err(
            InlineCacheValidationError::PropertyLoadPlanUnsupportedAccessCase(
                plan.access_case.kind,
            ),
        );
    }
    let key_supported = if indexed_load {
        property_load_access_case_key_supports_generated_indexed_plan(plan.key)
    } else {
        property_load_access_case_key_supports_generated_own_data_plan(plan.key)
    };
    if !key_supported {
        return Err(InlineCacheValidationError::PropertyLoadPlanUnsupportedKey(
            plan.key,
        ));
    }
    if plan.access_case.key != plan.key {
        return Err(
            InlineCacheValidationError::PropertyLoadPlanAccessCaseKeyMismatch {
                plan: plan.key,
                access_case: plan.access_case.key,
            },
        );
    }
    let access_case_key_supported = if indexed_load {
        property_load_access_case_key_supports_generated_indexed_plan(plan.access_case.key)
    } else {
        property_load_access_case_key_supports_generated_own_data_plan(plan.access_case.key)
    };
    if !access_case_key_supported {
        return Err(InlineCacheValidationError::PropertyLoadPlanUnsupportedKey(
            plan.access_case.key,
        ));
    }
    let base_structure = plan
        .access_case
        .base_structure
        .ok_or(InlineCacheValidationError::PropertyLoadPlanMissingBaseStructure)?;
    if base_structure == StructureId::INVALID {
        return Err(
            InlineCacheValidationError::PropertyLoadPlanInvalidBaseStructure(base_structure),
        );
    }
    if indexed_load {
        if let Some(offset) = plan.access_case.offset {
            return Err(InlineCacheValidationError::PropertyLoadPlanInvalidOffset(
                offset,
            ));
        }
    } else {
        let offset = plan
            .access_case
            .offset
            .ok_or(InlineCacheValidationError::PropertyLoadPlanMissingOffset)?;
        if offset.raw() < 0 {
            return Err(InlineCacheValidationError::PropertyLoadPlanInvalidOffset(
                offset,
            ));
        }
    }
    if let Some(holder) = plan.access_case.holder {
        return Err(InlineCacheValidationError::PropertyLoadPlanUnsupportedHolder(holder));
    }
    if let Some(new_structure) = plan.access_case.new_structure {
        return Err(
            InlineCacheValidationError::PropertyLoadPlanUnsupportedNewStructure(new_structure),
        );
    }
    if !plan.access_case.dependencies.is_empty() {
        return Err(InlineCacheValidationError::PropertyLoadPlanUnsupportedDependencies);
    }
    if plan.access_case.via_global_proxy {
        return Err(InlineCacheValidationError::PropertyLoadPlanUnsupportedGlobalProxy);
    }
    if plan.access_case.may_call_js {
        return Err(InlineCacheValidationError::PropertyLoadPlanMayCallJs);
    }

    Ok(())
}

fn property_load_access_case_plan_duplicate_key(
    plan: &PropertyLoadAccessCasePlan,
) -> (u32, StructureId, PropertyOffset, CacheKey) {
    (
        plan.bytecode_index,
        plan.access_case
            .base_structure
            .expect("validated property-load plan base structure"),
        plan.access_case.offset.unwrap_or(PropertyOffset::INVALID),
        plan.key,
    )
}

#[allow(dead_code)]
fn validate_property_store_access_case_plan_for_table(
    owner: CodeBlockId,
    plan: &PropertyStoreAccessCasePlan,
) -> Result<(), InlineCacheValidationError> {
    if plan.owner != owner {
        return Err(InlineCacheValidationError::PropertyStorePlanOwnerMismatch {
            expected: owner,
            actual: plan.owner,
        });
    }

    let expected_access_case = match plan.plan_kind {
        PropertyStoreAccessCasePlanKind::DataOnlyReplace => AccessCaseKind::Replace,
        PropertyStoreAccessCasePlanKind::DataOnlyTransition => AccessCaseKind::Transition,
        PropertyStoreAccessCasePlanKind::DataOnlyIndexedStore => AccessCaseKind::IndexedStore,
        PropertyStoreAccessCasePlanKind::Unsupported => {
            return Err(
                InlineCacheValidationError::PropertyStorePlanUnsupportedKind(plan.plan_kind),
            );
        }
    };

    if plan.planned_stub_kind != InlineCacheStubKind::RepatchingStub {
        return Err(
            InlineCacheValidationError::PropertyStorePlanUnsupportedStubKind(
                plan.planned_stub_kind,
            ),
        );
    }
    if plan.access_case.kind != expected_access_case {
        return Err(
            InlineCacheValidationError::PropertyStorePlanUnsupportedAccessCase(
                plan.access_case.kind,
            ),
        );
    }
    if !property_store_access_case_key_supports_metadata_plan(plan.plan_kind, plan.key) {
        return Err(InlineCacheValidationError::PropertyStorePlanUnsupportedKey(
            plan.key,
        ));
    }
    if plan.access_case.key != plan.key {
        return Err(
            InlineCacheValidationError::PropertyStorePlanAccessCaseKeyMismatch {
                plan: plan.key,
                access_case: plan.access_case.key,
            },
        );
    }
    if !property_store_access_case_key_supports_metadata_plan(plan.plan_kind, plan.access_case.key)
    {
        return Err(InlineCacheValidationError::PropertyStorePlanUnsupportedKey(
            plan.access_case.key,
        ));
    }
    if !plan.effect_contract.carries_runtime_barrier_requirement() {
        return Err(
            InlineCacheValidationError::PropertyStorePlanMissingBarrierProof(plan.effect_contract),
        );
    }
    let expected_contract_supported = match plan.plan_kind {
        PropertyStoreAccessCasePlanKind::DataOnlyReplace => {
            plan.effect_contract.supports_metadata_only_replace_plan()
        }
        PropertyStoreAccessCasePlanKind::DataOnlyTransition => plan
            .effect_contract
            .supports_metadata_only_transition_plan(),
        PropertyStoreAccessCasePlanKind::DataOnlyIndexedStore => plan
            .effect_contract
            .supports_metadata_only_indexed_store_plan(),
        PropertyStoreAccessCasePlanKind::Unsupported => unreachable!("validated plan kind"),
    };
    if !expected_contract_supported {
        return Err(
            InlineCacheValidationError::PropertyStorePlanUnsupportedEffectContract(
                plan.effect_contract,
            ),
        );
    }

    let base_structure = plan
        .access_case
        .base_structure
        .ok_or(InlineCacheValidationError::PropertyStorePlanMissingBaseStructure)?;
    if base_structure == StructureId::INVALID {
        return Err(
            InlineCacheValidationError::PropertyStorePlanInvalidBaseStructure(base_structure),
        );
    }
    let offset = match plan.plan_kind {
        PropertyStoreAccessCasePlanKind::DataOnlyIndexedStore => {
            if let Some(offset) = plan.access_case.offset {
                return Err(InlineCacheValidationError::PropertyStorePlanInvalidOffset(
                    offset,
                ));
            }
            None
        }
        PropertyStoreAccessCasePlanKind::DataOnlyReplace
        | PropertyStoreAccessCasePlanKind::DataOnlyTransition => {
            let offset = plan
                .access_case
                .offset
                .ok_or(InlineCacheValidationError::PropertyStorePlanMissingOffset)?;
            if offset.raw() < 0 {
                return Err(InlineCacheValidationError::PropertyStorePlanInvalidOffset(
                    offset,
                ));
            }
            Some(offset)
        }
        PropertyStoreAccessCasePlanKind::Unsupported => unreachable!("validated plan kind"),
    };
    if let Some(holder) = plan.access_case.holder {
        return Err(InlineCacheValidationError::PropertyStorePlanUnsupportedHolder(holder));
    }
    if !plan.access_case.dependencies.is_empty() {
        return Err(InlineCacheValidationError::PropertyStorePlanUnsupportedDependencies);
    }
    if plan.access_case.via_global_proxy {
        return Err(InlineCacheValidationError::PropertyStorePlanUnsupportedGlobalProxy);
    }
    if plan.access_case.may_call_js {
        return Err(InlineCacheValidationError::PropertyStorePlanMayCallJs);
    }

    match plan.plan_kind {
        PropertyStoreAccessCasePlanKind::DataOnlyReplace => {
            if let Some(new_structure) = plan.access_case.new_structure {
                return Err(
                    InlineCacheValidationError::PropertyStorePlanUnsupportedNewStructure(
                        new_structure,
                    ),
                );
            }
        }
        PropertyStoreAccessCasePlanKind::DataOnlyTransition => {
            let new_structure = plan
                .access_case
                .new_structure
                .ok_or(InlineCacheValidationError::PropertyStorePlanMissingNewStructure)?;
            if new_structure == StructureId::INVALID {
                return Err(
                    InlineCacheValidationError::PropertyStorePlanInvalidNewStructure(new_structure),
                );
            }
            if new_structure == base_structure {
                return Err(
                    InlineCacheValidationError::PropertyStorePlanRedundantTransitionStructure(
                        new_structure,
                    ),
                );
            }
        }
        PropertyStoreAccessCasePlanKind::DataOnlyIndexedStore => {
            if let Some(new_structure) = plan.access_case.new_structure {
                return Err(
                    InlineCacheValidationError::PropertyStorePlanUnsupportedNewStructure(
                        new_structure,
                    ),
                );
            }
            debug_assert!(offset.is_none());
        }
        PropertyStoreAccessCasePlanKind::Unsupported => unreachable!("validated plan kind"),
    }

    Ok(())
}

#[allow(dead_code)]
fn property_store_access_case_plan_duplicate_key(
    plan: &PropertyStoreAccessCasePlan,
) -> (
    u32,
    CacheKey,
    StructureId,
    PropertyOffset,
    Option<StructureId>,
) {
    (
        plan.bytecode_index,
        plan.key,
        plan.access_case
            .base_structure
            .expect("validated property-store plan base structure"),
        plan.access_case.offset.unwrap_or(PropertyOffset::INVALID),
        plan.access_case.new_structure,
    )
}

fn validate_property_store_mutation_candidate_for_table(
    owner: CodeBlockId,
    candidate: &PropertyStoreMutationCandidate,
) -> Result<(), InlineCacheValidationError> {
    PropertyStoreAccessCasePlanTable::new(owner, vec![candidate.plan.clone()])?;
    validate_property_store_mutation_candidate_ordinals(candidate)?;
    validate_property_store_mutation_candidate_barrier_evidence(candidate)?;
    Ok(())
}

fn validate_property_store_mutation_candidate_ordinals(
    candidate: &PropertyStoreMutationCandidate,
) -> Result<(), InlineCacheValidationError> {
    for (field, ordinal) in [
        ("store_plan_ordinal", candidate.store_plan_ordinal),
        ("install_recheck_ordinal", candidate.install_recheck_ordinal),
        ("readiness_ordinal", candidate.readiness_ordinal),
        ("observation_ordinal", candidate.observation_ordinal),
    ] {
        if ordinal == 0 {
            return Err(
                InlineCacheValidationError::PropertyStoreMutationCandidateInvalidOrdinal {
                    field,
                    ordinal,
                },
            );
        }
    }
    Ok(())
}

fn validate_property_store_mutation_candidate_barrier_evidence(
    candidate: &PropertyStoreMutationCandidate,
) -> Result<(), InlineCacheValidationError> {
    let evidence = candidate.barrier_evidence;
    if evidence.plan_kind != candidate.plan.plan_kind {
        return Err(
            InlineCacheValidationError::PropertyStoreMutationCandidateBarrierEvidenceMismatch {
                field: PropertyStoreMutationBarrierEvidenceMismatchField::PlanKind,
            },
        );
    }
    if evidence.effect_contract != candidate.plan.effect_contract {
        return Err(
            InlineCacheValidationError::PropertyStoreMutationCandidateBarrierEvidenceMismatch {
                field: PropertyStoreMutationBarrierEvidenceMismatchField::EffectContract,
            },
        );
    }
    if evidence.barrier_effect != candidate.plan.effect_contract.barrier {
        return Err(
            InlineCacheValidationError::PropertyStoreMutationCandidateBarrierEvidenceMismatch {
                field: PropertyStoreMutationBarrierEvidenceMismatchField::BarrierEffect,
            },
        );
    }
    if evidence.observed_write_barrier_count == 0
        && !(matches!(
            candidate.stored_value_kind,
            ValueKind::Int32
                | ValueKind::Double
                | ValueKind::Boolean
                | ValueKind::Null
                | ValueKind::Undefined
        ) && evidence.last_write_barrier
            == BarrierRequirementOutcome::NotRequired(
                BarrierNotRequiredReason::NullOrNonCellTarget,
            ))
    {
        return Err(
            InlineCacheValidationError::PropertyStoreMutationCandidateInvalidBarrierObservationCount(
                evidence.observed_write_barrier_count,
            ),
        );
    }
    Ok(())
}

fn property_store_mutation_candidate_semantic_key(
    candidate: &PropertyStoreMutationCandidate,
) -> (
    u32,
    CacheKey,
    StructureId,
    PropertyOffset,
    Option<StructureId>,
) {
    property_store_access_case_plan_duplicate_key(&candidate.plan)
}

fn validate_property_load_guarded_candidate_for_table(
    owner: CodeBlockId,
    candidate: &PropertyLoadGuardedCandidate,
) -> Result<(), InlineCacheValidationError> {
    let plan = &candidate.plan;
    let descriptor = &plan.descriptor;
    let chain = descriptor.chain.entries.as_slice();

    if plan.owner != owner {
        return Err(
            InlineCacheValidationError::PropertyLoadGuardedCandidateOwnerMismatch {
                expected: owner,
                actual: plan.owner,
            },
        );
    }
    if !property_load_guard_key_supports_named_property_proof(descriptor.key) {
        return Err(
            InlineCacheValidationError::PropertyLoadGuardedCandidateUnsupportedKey(descriptor.key),
        );
    }

    validate_property_load_guarded_candidate_ordinals(candidate)?;
    validate_property_load_guarded_candidate_binding_counts(candidate, chain.len())?;
    validate_property_load_guarded_candidate_chain_shape(descriptor)?;

    match descriptor.chain.outcome {
        PropertyLoadGuardChainOutcome::PrototypeData {
            holder_index,
            offset,
        } if candidate.candidate_kind == PropertyLoadGuardedCandidateKind::PrototypeData
            && descriptor.requirement == PropertyLoadGuardRequirement::PrototypeChain =>
        {
            validate_property_load_guarded_prototype_data_candidate(
                descriptor,
                holder_index,
                offset,
            )
        }
        PropertyLoadGuardChainOutcome::Missing { terminal_null }
            if candidate.candidate_kind == PropertyLoadGuardedCandidateKind::NegativeLookup
                && descriptor.requirement == PropertyLoadGuardRequirement::NegativeLookup =>
        {
            validate_property_load_guarded_negative_lookup_candidate(descriptor, terminal_null)
        }
        outcome => Err(
            InlineCacheValidationError::PropertyLoadGuardedCandidateUnsupportedShape {
                candidate_kind: candidate.candidate_kind,
                requirement: descriptor.requirement,
                outcome,
            },
        ),
    }
}

fn validate_property_load_guarded_candidate_ordinals(
    candidate: &PropertyLoadGuardedCandidate,
) -> Result<(), InlineCacheValidationError> {
    if candidate.guard_plan_ordinal == 0 {
        return Err(
            InlineCacheValidationError::PropertyLoadGuardedCandidateInvalidOrdinal {
                field: "guard_plan_ordinal",
                ordinal: candidate.guard_plan_ordinal,
            },
        );
    }
    if candidate.materialization_ordinal == 0 {
        return Err(
            InlineCacheValidationError::PropertyLoadGuardedCandidateInvalidOrdinal {
                field: "materialization_ordinal",
                ordinal: candidate.materialization_ordinal,
            },
        );
    }

    for (index, ordinal) in candidate.dependency_ordinals.iter().copied().enumerate() {
        if ordinal == 0 {
            return Err(
                InlineCacheValidationError::PropertyLoadGuardedCandidateInvalidOrdinal {
                    field: "dependency_ordinal",
                    ordinal,
                },
            );
        }
        if candidate.dependency_ordinals[..index].contains(&ordinal) {
            return Err(
                InlineCacheValidationError::PropertyLoadGuardedCandidateDuplicateDependencyOrdinal(
                    ordinal,
                ),
            );
        }
    }

    for (index, set_id) in candidate.binding_set_ids.iter().copied().enumerate() {
        if set_id.0 == 0 {
            return Err(
                InlineCacheValidationError::PropertyLoadGuardedCandidateInvalidBindingSetId(set_id),
            );
        }
        if candidate.binding_set_ids[..index].contains(&set_id) {
            return Err(
                InlineCacheValidationError::PropertyLoadGuardedCandidateDuplicateBindingSetId(
                    set_id,
                ),
            );
        }
    }

    Ok(())
}

fn validate_property_load_guarded_candidate_binding_counts(
    candidate: &PropertyLoadGuardedCandidate,
    chain_length: usize,
) -> Result<(), InlineCacheValidationError> {
    let dependency_count = candidate.dependency_ordinals.len();
    let binding_count = candidate.binding_set_ids.len();
    if dependency_count != chain_length || binding_count != chain_length {
        return Err(
            InlineCacheValidationError::PropertyLoadGuardedCandidateDependencyBindingCountMismatch {
                chain_length,
                dependency_count,
                binding_count,
            },
        );
    }
    Ok(())
}

fn validate_property_load_guarded_candidate_chain_shape(
    descriptor: &PropertyLoadGuardDescriptor,
) -> Result<(), InlineCacheValidationError> {
    let chain = descriptor.chain.entries.as_slice();
    let Some(first) = chain.first() else {
        return Err(
            InlineCacheValidationError::PropertyLoadGuardedCandidateMalformedChain(
                "guard chain must not be empty",
            ),
        );
    };
    if first.object != descriptor.base_object || first.structure != descriptor.base_structure {
        return Err(
            InlineCacheValidationError::PropertyLoadGuardedCandidateMalformedChain(
                "guard chain must start at the base object and structure",
            ),
        );
    }
    for window in chain.windows(2) {
        if window[0].next_prototype != Some(window[1].object) {
            return Err(
                InlineCacheValidationError::PropertyLoadGuardedCandidateMalformedChain(
                    "guard chain prototype links must be contiguous",
                ),
            );
        }
    }
    Ok(())
}

fn validate_property_load_guarded_prototype_data_candidate(
    descriptor: &PropertyLoadGuardDescriptor,
    holder_index: usize,
    offset: PropertyOffset,
) -> Result<(), InlineCacheValidationError> {
    let chain = descriptor.chain.entries.as_slice();
    if holder_index == 0 || holder_index >= chain.len() {
        return Err(
            InlineCacheValidationError::PropertyLoadGuardedCandidateMalformedChain(
                "prototype-data holder index must be nonzero and in bounds",
            ),
        );
    }
    let Some(holder_object) = descriptor.holder_object else {
        return Err(
            InlineCacheValidationError::PropertyLoadGuardedCandidateMalformedChain(
                "prototype-data holder object must be present",
            ),
        );
    };
    let holder_entry = &chain[holder_index];
    if holder_entry.object != holder_object {
        return Err(
            InlineCacheValidationError::PropertyLoadGuardedCandidateMalformedChain(
                "prototype-data holder object must match the guard chain entry",
            ),
        );
    }
    if descriptor.offset != Some(offset) || offset.raw() < 0 {
        return Err(
            InlineCacheValidationError::PropertyLoadGuardedCandidateInvalidOffset {
                descriptor_offset: descriptor.offset,
                outcome_offset: offset,
            },
        );
    }

    for (index, entry) in chain.iter().enumerate() {
        let proof_valid = if index == holder_index {
            entry.proof == PropertyLoadGuardChainEntryProof::DataProperty { offset }
        } else {
            entry.proof == PropertyLoadGuardChainEntryProof::NoOwnProperty
        };
        if !proof_valid {
            return Err(
                InlineCacheValidationError::PropertyLoadGuardedCandidateMalformedChain(
                    "prototype-data guard chain proof does not match the outcome",
                ),
            );
        }
    }

    Ok(())
}

fn validate_property_load_guarded_negative_lookup_candidate(
    descriptor: &PropertyLoadGuardDescriptor,
    terminal_null: bool,
) -> Result<(), InlineCacheValidationError> {
    if !terminal_null {
        return Err(
            InlineCacheValidationError::PropertyLoadGuardedCandidateMalformedChain(
                "negative lookup must terminate at null",
            ),
        );
    }
    if descriptor.holder_object.is_some() || descriptor.offset.is_some() {
        return Err(
            InlineCacheValidationError::PropertyLoadGuardedCandidateMalformedChain(
                "negative lookup must not carry holder or offset metadata",
            ),
        );
    }
    if descriptor
        .chain
        .entries
        .last()
        .is_none_or(|entry| entry.next_prototype.is_some())
    {
        return Err(
            InlineCacheValidationError::PropertyLoadGuardedCandidateMalformedChain(
                "negative lookup chain must terminate at null",
            ),
        );
    }
    if descriptor
        .chain
        .entries
        .iter()
        .any(|entry| entry.proof != PropertyLoadGuardChainEntryProof::NoOwnProperty)
    {
        return Err(
            InlineCacheValidationError::PropertyLoadGuardedCandidateMalformedChain(
                "negative lookup chain must prove no own property at every entry",
            ),
        );
    }

    Ok(())
}

fn property_load_guarded_candidate_semantic_key(
    candidate: &PropertyLoadGuardedCandidate,
) -> (
    u32,
    StructureId,
    CacheKey,
    PropertyLoadGuardRequirement,
    PropertyLoadGuardChainOutcome,
) {
    (
        candidate.plan.bytecode_index,
        candidate.plan.descriptor.base_structure,
        candidate.plan.descriptor.key,
        candidate.plan.descriptor.requirement,
        candidate.plan.descriptor.chain.outcome,
    )
}

#[derive(Clone, Copy, Debug, Eq, PartialEq)]
pub struct GeneratedPropertyLoadProbeRequest<'a> {
    pub plan: &'a PropertyLoadAccessCasePlan,
    pub base: RuntimeValue,
}

#[derive(Clone, Copy, Debug, Eq, PartialEq)]
pub struct GeneratedPropertyLoadMegamorphicHolderProbeRequest {
    pub key: CacheKey,
    pub base_structure: StructureId,
    pub holder: ObjectId,
    pub offset: PropertyOffset,
}

#[derive(Clone, Copy, Debug, Eq, PartialEq)]
pub struct GeneratedPropertyStoreProbeRequest<'a> {
    pub plan: &'a PropertyStoreAccessCasePlan,
    pub base: RuntimeValue,
    pub stored_value: RuntimeValue,
}

impl<'a> GeneratedPropertyStoreProbeRequest<'a> {
    pub const fn new(
        plan: &'a PropertyStoreAccessCasePlan,
        base: RuntimeValue,
        stored_value: RuntimeValue,
    ) -> Self {
        Self {
            plan,
            base,
            stored_value,
        }
    }

    pub fn plan_kind(&self) -> PropertyStoreAccessCasePlanKind {
        self.plan.plan_kind
    }

    pub fn key(&self) -> CacheKey {
        self.plan.key
    }

    pub fn barrier_effect(&self) -> PropertyStoreBarrierEffect {
        self.plan.effect_contract.barrier
    }

    pub fn requires_structure_transition(&self) -> bool {
        matches!(
            self.plan.plan_kind,
            PropertyStoreAccessCasePlanKind::DataOnlyTransition
        )
    }
}

#[derive(Clone, Copy, Debug, Eq, PartialEq)]
#[allow(dead_code)]
pub struct GeneratedGuardedPropertyLoadProbeRequest<'a> {
    pub plan: &'a PropertyLoadGuardPlan,
    pub base: RuntimeValue,
}

#[allow(dead_code)]
impl<'a> GeneratedGuardedPropertyLoadProbeRequest<'a> {
    pub const fn new(plan: &'a PropertyLoadGuardPlan, base: RuntimeValue) -> Self {
        Self { plan, base }
    }

    pub fn requirement(&self) -> PropertyLoadGuardRequirement {
        self.plan.descriptor.requirement
    }

    pub fn key(&self) -> CacheKey {
        self.plan.descriptor.key
    }

    pub fn prototype_depth(&self) -> u16 {
        self.plan.descriptor.prototype_depth
    }

    pub fn outcome(&self) -> PropertyLoadGuardChainOutcome {
        self.plan.descriptor.chain.outcome
    }
}

#[derive(Clone, Copy, Debug, Eq, PartialEq)]
pub enum GeneratedPropertyLoadDestinationRootSync {
    NotRequiredForImmediate,
    TargetedRegisterRequiredForCell,
}

impl GeneratedPropertyLoadDestinationRootSync {
    pub fn for_value(value: RuntimeValue) -> Self {
        if value.as_cell().is_some() {
            Self::TargetedRegisterRequiredForCell
        } else {
            Self::NotRequiredForImmediate
        }
    }

    pub const fn requires_targeted_register_sync(self) -> bool {
        matches!(self, Self::TargetedRegisterRequiredForCell)
    }
}

#[derive(Clone, Copy, Debug, Eq, PartialEq)]
pub struct GeneratedPropertyLoadProbeHit {
    pub value: RuntimeValue,
    pub destination_root_sync: GeneratedPropertyLoadDestinationRootSync,
}

impl GeneratedPropertyLoadProbeHit {
    pub fn new(value: RuntimeValue) -> Self {
        Self {
            value,
            destination_root_sync: GeneratedPropertyLoadDestinationRootSync::for_value(value),
        }
    }
}

/// Metadata for a future generated `PutByName` sidecar hit.
///
/// The planned fields describe a mutation continuation a later executable
/// sidecar could take. They are not evidence that a heap write, structure
/// transition, or barrier execution has happened.
#[derive(Clone, Copy, Debug, Eq, PartialEq)]
pub struct GeneratedPropertyStoreProbeHit {
    pub continuation_plan_kind: PropertyStoreAccessCasePlanKind,
    pub key: CacheKey,
    pub stored_value: RuntimeValue,
    pub effect_contract: PropertyStoreAccessCasePlanContract,
    pub barrier_effect: PropertyStoreBarrierEffect,
    pub base_structure: StructureId,
    pub planned_new_structure: Option<StructureId>,
    pub planned_offset: PropertyOffset,
}

impl GeneratedPropertyStoreProbeHit {
    pub fn for_plan(
        plan: &PropertyStoreAccessCasePlan,
        stored_value: RuntimeValue,
    ) -> Result<Self, GeneratedPropertyStoreProbeMissReason> {
        let expected_access_case = match plan.plan_kind {
            PropertyStoreAccessCasePlanKind::DataOnlyReplace => AccessCaseKind::Replace,
            PropertyStoreAccessCasePlanKind::DataOnlyTransition => AccessCaseKind::Transition,
            PropertyStoreAccessCasePlanKind::DataOnlyIndexedStore => AccessCaseKind::IndexedStore,
            PropertyStoreAccessCasePlanKind::Unsupported => {
                return Err(GeneratedPropertyStoreProbeMissReason::UnsupportedPlan);
            }
        };

        if plan.planned_stub_kind != InlineCacheStubKind::RepatchingStub
            || plan.access_case.kind != expected_access_case
            || plan.access_case.holder.is_some()
            || !plan.access_case.dependencies.is_empty()
            || plan.access_case.via_global_proxy
            || plan.access_case.may_call_js
        {
            return Err(GeneratedPropertyStoreProbeMissReason::UnsupportedPlanMetadata);
        }

        if let Some(reason) =
            generated_property_store_probe_key_miss_reason(plan.plan_kind, plan.key)
        {
            return Err(reason);
        }
        if plan.access_case.key != plan.key {
            return Err(GeneratedPropertyStoreProbeMissReason::ExistingPropertyMismatch);
        }
        if let Some(reason) =
            generated_property_store_probe_key_miss_reason(plan.plan_kind, plan.access_case.key)
        {
            return Err(reason);
        }

        if plan.effect_contract.barrier == PropertyStoreBarrierEffect::Unsupported {
            return Err(GeneratedPropertyStoreProbeMissReason::UnsupportedBarrierEffect);
        }
        let contract_matches = match plan.plan_kind {
            PropertyStoreAccessCasePlanKind::DataOnlyReplace => {
                plan.effect_contract.supports_metadata_only_replace_plan()
            }
            PropertyStoreAccessCasePlanKind::DataOnlyTransition => plan
                .effect_contract
                .supports_metadata_only_transition_plan(),
            PropertyStoreAccessCasePlanKind::DataOnlyIndexedStore => plan
                .effect_contract
                .supports_metadata_only_indexed_store_plan(),
            PropertyStoreAccessCasePlanKind::Unsupported => unreachable!("checked above"),
        };
        if !contract_matches {
            return Err(GeneratedPropertyStoreProbeMissReason::BarrierContractMismatch);
        }

        let base_structure = plan
            .access_case
            .base_structure
            .filter(|structure| *structure != StructureId::INVALID)
            .ok_or(GeneratedPropertyStoreProbeMissReason::MissingBaseStructure)?;
        let planned_offset = match plan.plan_kind {
            PropertyStoreAccessCasePlanKind::DataOnlyIndexedStore => {
                if plan.access_case.offset.is_some() {
                    return Err(GeneratedPropertyStoreProbeMissReason::MissingOrInvalidOffset);
                }
                PropertyOffset::INVALID
            }
            PropertyStoreAccessCasePlanKind::DataOnlyReplace
            | PropertyStoreAccessCasePlanKind::DataOnlyTransition => plan
                .access_case
                .offset
                .filter(|offset| offset.raw() >= 0)
                .ok_or(GeneratedPropertyStoreProbeMissReason::MissingOrInvalidOffset)?,
            PropertyStoreAccessCasePlanKind::Unsupported => unreachable!("checked above"),
        };

        let planned_new_structure = match plan.plan_kind {
            PropertyStoreAccessCasePlanKind::DataOnlyReplace => {
                if plan.access_case.new_structure.is_some() {
                    return Err(GeneratedPropertyStoreProbeMissReason::TransitionStructureMismatch);
                }
                None
            }
            PropertyStoreAccessCasePlanKind::DataOnlyIndexedStore => {
                if plan.access_case.new_structure.is_some() {
                    return Err(GeneratedPropertyStoreProbeMissReason::TransitionStructureMismatch);
                }
                None
            }
            PropertyStoreAccessCasePlanKind::DataOnlyTransition => {
                let new_structure = plan
                    .access_case
                    .new_structure
                    .ok_or(GeneratedPropertyStoreProbeMissReason::MissingTransitionStructure)?;
                if new_structure == StructureId::INVALID || new_structure == base_structure {
                    return Err(GeneratedPropertyStoreProbeMissReason::TransitionStructureMismatch);
                }
                Some(new_structure)
            }
            PropertyStoreAccessCasePlanKind::Unsupported => unreachable!("checked above"),
        };

        Ok(Self {
            continuation_plan_kind: plan.plan_kind,
            key: plan.key,
            stored_value,
            effect_contract: plan.effect_contract,
            barrier_effect: plan.effect_contract.barrier,
            base_structure,
            planned_new_structure,
            planned_offset,
        })
    }
}

/// Request token for a host-owned generated `PutByName` mutation.
///
/// The generated sidecar may carry this after a successful probe, but authority
/// for the heap write, barrier proof, rooting, and no-GC boundary stays with the
/// host that accepts or rejects the request.
#[derive(Clone, Copy, Debug, Eq, PartialEq)]
pub struct GeneratedPropertyStoreMutationRequest {
    pub base: RuntimeValue,
    pub probe_hit: GeneratedPropertyStoreProbeHit,
}

impl GeneratedPropertyStoreMutationRequest {
    pub const fn new(base: RuntimeValue, probe_hit: GeneratedPropertyStoreProbeHit) -> Self {
        Self { base, probe_hit }
    }

    pub const fn plan_kind(&self) -> PropertyStoreAccessCasePlanKind {
        self.probe_hit.continuation_plan_kind
    }

    pub const fn key(&self) -> CacheKey {
        self.probe_hit.key
    }

    pub const fn barrier_effect(&self) -> PropertyStoreBarrierEffect {
        self.probe_hit.barrier_effect
    }

    pub fn stored_value_kind(&self) -> ValueKind {
        self.probe_hit.stored_value.kind()
    }
}

/// Host-confirmed metadata for a generated `PutByName` mutation.
///
/// This records evidence returned by a future host transaction. It is not a
/// sidecar capability to write the heap, run barriers, root values, mutate
/// CodeBlock IC state, or invalidate executable artifacts.
#[derive(Clone, Copy, Debug, Eq, PartialEq)]
pub struct GeneratedPropertyStoreMutationCommit {
    pub plan_kind: PropertyStoreAccessCasePlanKind,
    pub key: CacheKey,
    pub stored_value: RuntimeValue,
    pub stored_value_kind: ValueKind,
    pub base_structure_before: StructureId,
    pub base_structure_after: StructureId,
    pub planned_offset: PropertyOffset,
    pub planned_new_structure: Option<StructureId>,
    pub effect_contract: PropertyStoreAccessCasePlanContract,
    pub barrier_effect: PropertyStoreBarrierEffect,
}

impl GeneratedPropertyStoreMutationCommit {
    pub fn host_confirmed_for_request(request: &GeneratedPropertyStoreMutationRequest) -> Self {
        let hit = request.probe_hit;
        Self {
            plan_kind: hit.continuation_plan_kind,
            key: hit.key,
            stored_value: hit.stored_value,
            stored_value_kind: hit.stored_value.kind(),
            base_structure_before: hit.base_structure,
            base_structure_after: hit.planned_new_structure.unwrap_or(hit.base_structure),
            planned_offset: hit.planned_offset,
            planned_new_structure: hit.planned_new_structure,
            effect_contract: hit.effect_contract,
            barrier_effect: hit.barrier_effect,
        }
    }

    pub const fn requires_structure_transition(self) -> bool {
        matches!(
            self.plan_kind,
            PropertyStoreAccessCasePlanKind::DataOnlyTransition
        )
    }
}

#[allow(dead_code)]
pub type GeneratedPropertyStoreMutationHit = GeneratedPropertyStoreMutationCommit;

#[derive(Clone, Copy, Debug, Eq, PartialEq)]
#[allow(dead_code)]
pub struct GeneratedGuardedPropertyLoadProbeHit {
    pub value: RuntimeValue,
    pub destination_root_sync: GeneratedPropertyLoadDestinationRootSync,
}

#[allow(dead_code)]
impl GeneratedGuardedPropertyLoadProbeHit {
    pub fn new(value: RuntimeValue) -> Self {
        Self {
            value,
            destination_root_sync: GeneratedPropertyLoadDestinationRootSync::for_value(value),
        }
    }
}

#[derive(Clone, Copy, Debug, Eq, PartialEq)]
pub struct GeneratedPropertyLoadProbeMiss {
    pub reason: GeneratedPropertyLoadProbeMissReason,
}

#[derive(Clone, Copy, Debug, Eq, PartialEq)]
pub struct GeneratedPropertyStoreProbeMiss {
    pub reason: GeneratedPropertyStoreProbeMissReason,
}

#[derive(Clone, Copy, Debug, Eq, PartialEq)]
pub struct GeneratedPropertyStoreMutationRejection {
    pub reason: GeneratedPropertyStoreMutationMissReason,
}

#[allow(dead_code)]
pub type GeneratedPropertyStoreMutationMiss = GeneratedPropertyStoreMutationRejection;

#[derive(Clone, Copy, Debug, Eq, PartialEq)]
pub enum GeneratedPropertyLoadProbeMissReason {
    HostUnavailable,
    UnsupportedPlan,
    UnsupportedPlanMetadata,
    NonCellBase,
    UnknownObject,
    StructureMismatch,
    MissingBaseStructure,
    MissingOrInvalidOffset,
    KeyNotRepresentable,
    KeyOffsetMismatch,
    MissingProperty,
    NonDataProperty,
    IndexedProperty,
    OpaqueObject,
}

#[derive(Clone, Copy, Debug, Eq, PartialEq)]
pub enum GeneratedPropertyStoreProbeMissReason {
    HostUnavailable,
    UnsupportedPlan,
    UnsupportedPlanMetadata,
    NonCellBase,
    UnknownObject,
    StructureMismatch,
    MissingBaseStructure,
    MissingOrInvalidOffset,
    KeyNotRepresentable,
    ExistingPropertyMismatch,
    MissingTransitionStructure,
    TransitionStructureMismatch,
    IndexedProperty,
    OpaqueObject,
    MissingBarrierEvidence,
    UnsupportedBarrierEffect,
    BarrierContractMismatch,
}

#[derive(Clone, Copy, Debug, Eq, PartialEq)]
pub enum GeneratedPropertyStoreMutationMissReason {
    HostUnavailable,
    ProbeRejected(GeneratedPropertyStoreProbeMissReason),
    NonCellBase,
    UnknownObject,
    OpaqueObject,
    StructureMismatch,
    ExistingPropertyMismatch,
    MissingTransitionStructure,
    TransitionStructureMismatch,
    MissingOrInvalidOffset,
    KeyNotRepresentable,
    IndexedProperty,
    BarrierRejected,
}

#[derive(Clone, Copy, Debug, Eq, PartialEq)]
#[allow(dead_code)]
pub struct GeneratedGuardedPropertyLoadProbeMiss {
    pub reason: GeneratedGuardedPropertyLoadProbeMissReason,
    pub requirement: PropertyLoadGuardRequirement,
    pub key: CacheKey,
    pub prototype_depth: u16,
    pub chain_index: Option<usize>,
    pub outcome: PropertyLoadGuardChainOutcome,
}

#[allow(dead_code)]
impl GeneratedGuardedPropertyLoadProbeMiss {
    pub fn new(
        reason: GeneratedGuardedPropertyLoadProbeMissReason,
        plan: &PropertyLoadGuardPlan,
        chain_index: Option<usize>,
    ) -> Self {
        Self {
            reason,
            requirement: plan.descriptor.requirement,
            key: plan.descriptor.key,
            prototype_depth: plan.descriptor.prototype_depth,
            chain_index,
            outcome: plan.descriptor.chain.outcome,
        }
    }
}

#[derive(Clone, Copy, Debug, Eq, PartialEq)]
#[allow(dead_code)]
pub enum GeneratedGuardedPropertyLoadProbeMissReason {
    HostUnavailable,
    UnsupportedGuardRequirement,
    NonCellBase,
    UnknownGuardObject,
    GuardStructureMismatch,
    GuardPrototypeLinkMismatch,
    GuardNoOwnPropertyProofFailed,
    GuardDataPropertyProofFailed,
    GuardOutcomeMismatch,
    KeyNotRepresentable,
}

#[derive(Clone, Copy, Debug, Eq, PartialEq)]
pub enum GeneratedPropertyLoadProbeResult {
    Hit(GeneratedPropertyLoadProbeHit),
    Miss(GeneratedPropertyLoadProbeMiss),
}

#[derive(Clone, Copy, Debug, Eq, PartialEq)]
pub enum GeneratedPropertyStoreProbeResult {
    Hit(GeneratedPropertyStoreProbeHit),
    Miss(GeneratedPropertyStoreProbeMiss),
}

#[derive(Clone, Copy, Debug, Eq, PartialEq)]
pub enum GeneratedPropertyStoreMutationResult {
    Committed(GeneratedPropertyStoreMutationCommit),
    Rejected(GeneratedPropertyStoreMutationRejection),
}

impl GeneratedPropertyLoadProbeResult {
    pub fn hit(value: RuntimeValue) -> Self {
        Self::Hit(GeneratedPropertyLoadProbeHit::new(value))
    }

    pub const fn miss(reason: GeneratedPropertyLoadProbeMissReason) -> Self {
        Self::Miss(GeneratedPropertyLoadProbeMiss { reason })
    }
}

impl GeneratedPropertyStoreProbeResult {
    pub fn hit_for_plan(plan: &PropertyStoreAccessCasePlan, stored_value: RuntimeValue) -> Self {
        match GeneratedPropertyStoreProbeHit::for_plan(plan, stored_value) {
            Ok(hit) => Self::Hit(hit),
            Err(reason) => Self::miss(reason),
        }
    }

    pub const fn miss(reason: GeneratedPropertyStoreProbeMissReason) -> Self {
        Self::Miss(GeneratedPropertyStoreProbeMiss { reason })
    }
}

impl GeneratedPropertyStoreMutationResult {
    pub const fn committed(commit: GeneratedPropertyStoreMutationCommit) -> Self {
        Self::Committed(commit)
    }

    pub const fn rejected(reason: GeneratedPropertyStoreMutationMissReason) -> Self {
        Self::Rejected(GeneratedPropertyStoreMutationRejection { reason })
    }

    pub const fn probe_rejected(reason: GeneratedPropertyStoreProbeMissReason) -> Self {
        Self::rejected(GeneratedPropertyStoreMutationMissReason::ProbeRejected(
            reason,
        ))
    }

    pub const fn implies_host_mutation(self) -> bool {
        match self {
            Self::Committed(_) => true,
            Self::Rejected(_) => false,
        }
    }
}

fn generated_property_store_probe_key_miss_reason(
    plan_kind: PropertyStoreAccessCasePlanKind,
    key: CacheKey,
) -> Option<GeneratedPropertyStoreProbeMissReason> {
    match (plan_kind, key) {
        (
            PropertyStoreAccessCasePlanKind::DataOnlyReplace
            | PropertyStoreAccessCasePlanKind::DataOnlyTransition,
            CacheKey::Property(property_key),
        ) if property_key.as_identifier().is_some() => None,
        (
            PropertyStoreAccessCasePlanKind::DataOnlyIndexedStore,
            CacheKey::Property(property_key),
        ) if property_key.as_index().is_some() => None,
        (
            PropertyStoreAccessCasePlanKind::DataOnlyReplace
            | PropertyStoreAccessCasePlanKind::DataOnlyTransition,
            CacheKey::Property(property_key),
        ) if property_key.as_index().is_some() => {
            Some(GeneratedPropertyStoreProbeMissReason::IndexedProperty)
        }
        (_, CacheKey::Property(_)) | (_, CacheKey::Dynamic) => {
            Some(GeneratedPropertyStoreProbeMissReason::KeyNotRepresentable)
        }
    }
}

#[derive(Clone, Copy, Debug, Eq, PartialEq)]
#[allow(dead_code)]
pub enum GeneratedGuardedPropertyLoadProbeResult {
    Hit(GeneratedGuardedPropertyLoadProbeHit),
    Miss(GeneratedGuardedPropertyLoadProbeMiss),
}

#[allow(dead_code)]
impl GeneratedGuardedPropertyLoadProbeResult {
    pub fn hit(value: RuntimeValue) -> Self {
        Self::Hit(GeneratedGuardedPropertyLoadProbeHit::new(value))
    }

    pub fn miss_for_plan(
        reason: GeneratedGuardedPropertyLoadProbeMissReason,
        plan: &PropertyLoadGuardPlan,
        chain_index: Option<usize>,
    ) -> Self {
        Self::Miss(GeneratedGuardedPropertyLoadProbeMiss::new(
            reason,
            plan,
            chain_index,
        ))
    }
}

/// Stable identity for an IC stub or handler node.
#[derive(Clone, Copy, Debug, Eq, Hash, Ord, PartialEq, PartialOrd)]
pub struct InlineCacheStubId(pub u64);

/// Kind of stub storage an IC metadata node describes.
#[derive(Clone, Copy, Debug, Eq, PartialEq)]
pub enum InlineCacheStubKind {
    SlowPathHandler,
    DataOnlyHandler,
    HandlerWithCallLinkInfo,
    PolymorphicAccessStub,
    SharedStatelessStub,
    RepatchingStub,
}

/// Metadata for a generated or reserved IC stub.
#[derive(Clone, Debug, Eq, PartialEq)]
pub struct InlineCacheStub {
    pub id: InlineCacheStubId,
    pub kind: InlineCacheStubKind,
    pub owner_slot: InlineCacheSlotId,
    pub code: Option<JitCodeId>,
    pub tier: JitType,
    pub cases: Vec<AccessCaseDescriptor>,
    pub weak_structures: Vec<StructureId>,
    pub barrier_metadata: Vec<InlineCacheBarrierMetadata>,
    pub call_links: Vec<CallLinkInfoDescriptor>,
    pub invalidation_strength: DependencyStrength,
}

#[derive(Clone, Copy, Debug, Eq, PartialEq)]
pub enum InlineCacheBarrierTarget {
    BaseObject,
    HolderObject,
    NewStructure,
    StoredValue,
    CallTarget,
}

#[derive(Clone, Copy, Debug, Eq, PartialEq)]
pub struct InlineCacheBarrierMetadata {
    pub target: InlineCacheBarrierTarget,
    pub field_kind: BarrierFieldKind,
    pub barrier_kind: BarrierKind,
    pub threshold: BarrierThreshold,
    pub required_for_write: bool,
}

impl InlineCacheBarrierMetadata {
    pub const fn store_value(target: InlineCacheBarrierTarget) -> Self {
        Self {
            target,
            field_kind: BarrierFieldKind::Value,
            barrier_kind: BarrierKind::StoreCellValue,
            threshold: BarrierThreshold::PossiblyGrey,
            required_for_write: true,
        }
    }

    pub const fn structure_transition() -> Self {
        Self {
            target: InlineCacheBarrierTarget::NewStructure,
            field_kind: BarrierFieldKind::StructureId,
            barrier_kind: BarrierKind::StoreStructureId,
            threshold: BarrierThreshold::None,
            required_for_write: true,
        }
    }

    pub fn validate_for_slot(
        self,
        slot: &InlineCacheSlot,
    ) -> Result<(), InlineCacheValidationError> {
        if self.required_for_write && !effects_for_cache_kind(slot.kind).writes_heap {
            return Err(InlineCacheValidationError::BarrierKindMismatch);
        }
        if self.field_kind == BarrierFieldKind::StructureId
            && self.barrier_kind != BarrierKind::StoreStructureId
        {
            return Err(InlineCacheValidationError::BarrierKindMismatch);
        }
        if self.field_kind == BarrierFieldKind::Value
            && !matches!(
                self.barrier_kind,
                BarrierKind::Store | BarrierKind::StoreCellValue | BarrierKind::RememberedSet
            )
        {
            return Err(InlineCacheValidationError::BarrierKindMismatch);
        }
        Ok(())
    }
}

/// Call-link mode for call ICs and IC stubs that may call JS.
#[derive(Clone, Copy, Debug, Eq, PartialEq)]
pub enum CallLinkMode {
    Init,
    Monomorphic,
    Polymorphic,
    Virtual,
    Direct,
}

/// Call kind associated with a call-link descriptor.
#[derive(Clone, Copy, Debug, Eq, PartialEq)]
pub enum LinkedCallKind {
    Call,
    Construct,
    TailCall,
    VarargsCall,
    VarargsConstruct,
}

/// Data-only call link metadata. Callees and code blocks are weak IDs because
/// GC/liveness ownership belongs to runtime code and generated-code finalizers.
#[derive(Clone, Copy, Debug, Eq, PartialEq)]
pub struct CallLinkInfoDescriptor {
    pub mode: CallLinkMode,
    pub call_kind: LinkedCallKind,
    pub owner: Option<CodeBlockId>,
    pub executable: Option<ExecutableId>,
    pub callee: Option<ObjectId>,
    pub target_code_block: Option<CodeBlockId>,
    pub boundary: Option<CallBoundaryId>,
    pub slow_path_count: u32,
    pub max_argument_count_including_this: u8,
}

/// Metadata-only call-link attachment candidate.
///
/// This validates the descriptor and reserved IC stub shape for a future call
/// link without mutating code-block side tables, installing handlers, or
/// authorizing generated direct calls.
#[derive(Clone, Copy, Debug, Eq, PartialEq)]
pub struct CallLinkAttachmentTargetDescriptor {
    pub executable: ExecutableId,
    pub target_code_block: CodeBlockId,
    pub callee: ObjectId,
    pub specialization: CodeSpecialization,
}

#[derive(Clone, Debug, Eq, PartialEq)]
pub struct CallLinkAttachmentPlan {
    pub owner: CodeBlockId,
    pub opcode: CoreOpcode,
    pub slot: InlineCacheSlotId,
    pub bytecode_index: u32,
    pub descriptor: CallLinkInfoDescriptor,
    pub target: CallLinkAttachmentTargetDescriptor,
    pub boundary: CallBoundaryMetadata,
    pub planned_stub_kind: InlineCacheStubKind,
    pub remaining_blockers: CallLinkReadinessBlockers,
    pub stub: InlineCacheStub,
}

/// Validated, immutable table of metadata-only call-link attachment plans.
#[derive(Clone, Debug, Eq, PartialEq)]
pub struct CallLinkAttachmentPlanTable {
    owner: CodeBlockId,
    plans: Vec<CallLinkAttachmentPlan>,
}

impl CallLinkAttachmentPlanTable {
    pub fn new(
        owner: CodeBlockId,
        plans: Vec<CallLinkAttachmentPlan>,
    ) -> Result<Self, InlineCacheValidationError> {
        for plan in &plans {
            validate_call_link_attachment_plan_for_table(owner, plan)?;
        }

        for (index, plan) in plans.iter().enumerate() {
            for prior in &plans[..index] {
                if call_link_attachment_plan_duplicate_key(prior)
                    == call_link_attachment_plan_duplicate_key(plan)
                {
                    return Err(InlineCacheValidationError::CallLinkMismatch);
                }
            }
        }

        Ok(Self { owner, plans })
    }

    pub const fn owner(&self) -> CodeBlockId {
        self.owner
    }

    pub fn plans(&self) -> &[CallLinkAttachmentPlan] {
        &self.plans
    }

    pub fn len(&self) -> usize {
        self.plans.len()
    }

    pub fn is_empty(&self) -> bool {
        self.plans.is_empty()
    }

    pub fn candidates_for_bytecode_index(
        &self,
        bytecode_index: u32,
    ) -> impl Iterator<Item = &CallLinkAttachmentPlan> {
        self.plans
            .iter()
            .filter(move |plan| plan.bytecode_index == bytecode_index)
    }
}

fn validate_call_link_attachment_plan_for_table(
    owner: CodeBlockId,
    plan: &CallLinkAttachmentPlan,
) -> Result<(), InlineCacheValidationError> {
    let Some(expected_call_kind) = linked_call_kind_for_opcode(plan.opcode) else {
        return Err(InlineCacheValidationError::CallLinkMismatch);
    };
    let expected_specialization = call_link_specialization_for_kind(expected_call_kind);
    if plan.owner != owner
        || plan.descriptor.owner != Some(owner)
        || plan.descriptor.boundary.is_none()
        || plan.descriptor.executable.is_none()
        || plan.descriptor.callee.is_none()
        || plan.descriptor.target_code_block.is_none()
        || plan.descriptor.call_kind != expected_call_kind
        || !matches!(
            plan.descriptor.mode,
            CallLinkMode::Init | CallLinkMode::Monomorphic | CallLinkMode::Direct
        )
    {
        return Err(InlineCacheValidationError::CallLinkMismatch);
    }
    if plan.descriptor.executable != Some(plan.target.executable)
        || plan.descriptor.callee != Some(plan.target.callee)
        || plan.descriptor.target_code_block != Some(plan.target.target_code_block)
        || plan.target.specialization != expected_specialization
        || plan.descriptor.boundary != Some(plan.boundary.id)
        || plan.boundary.owner != Some(owner)
        || plan.boundary.abi != EntryAbi::LlIntCompatible
        || plan.boundary.entry_kind != EntrypointKind::InterpreterThunk
        || plan.boundary.native_symbol.is_some()
        || plan.boundary.arguments.len()
            != usize::from(plan.descriptor.max_argument_count_including_this)
        || plan
            .boundary
            .arguments
            .iter()
            .any(|argument| *argument != AbiValue::JsValue)
        || plan.boundary.returns.as_slice() != [AbiValue::JsValue]
        || !plan.boundary.requires_vm_entry_scope
        || !plan.boundary.may_call_js
        || !plan.boundary.may_throw
    {
        return Err(InlineCacheValidationError::CallLinkMismatch);
    }
    let mut unsupported_blockers = plan.remaining_blockers;
    unsupported_blockers.remove(CallLinkReadinessBlocker::DirectCallDisallowed);
    unsupported_blockers.remove(CallLinkReadinessBlocker::MayCallJsBoundary);
    if !unsupported_blockers.is_empty() {
        return Err(InlineCacheValidationError::CallLinkMismatch);
    }
    if plan.planned_stub_kind != InlineCacheStubKind::HandlerWithCallLinkInfo
        || plan.stub.kind != plan.planned_stub_kind
        || plan.stub.owner_slot != plan.slot
        || plan.stub.tier != JitType::Baseline
        || plan.stub.code.is_some()
        || !plan.stub.cases.is_empty()
        || !plan.stub.weak_structures.is_empty()
        || !plan.stub.barrier_metadata.is_empty()
        || plan.stub.call_links.as_slice() != [plan.descriptor]
        || plan.stub.invalidation_strength != DependencyStrength::WeakGc
    {
        return Err(InlineCacheValidationError::CallLinkMismatch);
    }

    let slot = InlineCacheSlot::builder(
        plan.slot,
        inline_cache_kind_for_call_link_opcode(plan.opcode),
    )
    .owner(owner)
    .bytecode_index(plan.bytecode_index)
    .stub(plan.stub.clone())
    .build()?;
    let classification = classify_inline_cache_slot(&slot)?;
    if classification != InlineCacheCaseClassification::CallLinking {
        return Err(InlineCacheValidationError::CallLinkMismatch);
    }

    Ok(())
}

fn call_link_attachment_plan_duplicate_key(
    plan: &CallLinkAttachmentPlan,
) -> (
    CoreOpcode,
    LinkedCallKind,
    u32,
    Option<ExecutableId>,
    Option<ObjectId>,
    Option<CodeBlockId>,
) {
    (
        plan.opcode,
        plan.descriptor.call_kind,
        plan.bytecode_index,
        plan.descriptor.executable,
        plan.descriptor.callee,
        plan.descriptor.target_code_block,
    )
}

const fn linked_call_kind_for_opcode(opcode: CoreOpcode) -> Option<LinkedCallKind> {
    match opcode {
        CoreOpcode::Call | CoreOpcode::CallWithThis => Some(LinkedCallKind::Call),
        CoreOpcode::Construct => Some(LinkedCallKind::Construct),
        _ => None,
    }
}

const fn call_link_specialization_for_kind(kind: LinkedCallKind) -> CodeSpecialization {
    match kind {
        LinkedCallKind::Construct | LinkedCallKind::VarargsConstruct => {
            CodeSpecialization::Construct
        }
        _ => CodeSpecialization::Call,
    }
}

const fn inline_cache_kind_for_call_link_opcode(opcode: CoreOpcode) -> InlineCacheKind {
    match opcode {
        CoreOpcode::Construct => InlineCacheKind::Construct,
        _ => InlineCacheKind::Call,
    }
}

/// Whether an active call-link projection may transfer control directly through
/// the VM-owned JS call boundary.
#[derive(Clone, Copy, Debug, Eq, PartialEq)]
pub enum GeneratedCallLinkDirectCallStatus {
    Disallowed,
    Authorized,
}

/// Immutable JIT-facing projection of an active attached call-link IC.
///
/// This records attached monomorphic metadata and provenance. An authorized
/// projection is still only a VM transaction request; generated code does not
/// push frames, mutate call-link metadata, or enter target code by itself.
#[derive(Clone, Debug, Eq, PartialEq)]
pub struct GeneratedCallLinkCandidate {
    pub owner: CodeBlockId,
    pub opcode: CoreOpcode,
    pub slot: InlineCacheSlotId,
    pub bytecode_index: u32,
    pub descriptor: CallLinkInfoDescriptor,
    pub target: CallLinkAttachmentTargetDescriptor,
    pub boundary: CallBoundaryMetadata,
    pub attachment_ordinal: u64,
    pub attachment_plan_ordinal: u64,
    pub install_recheck_ordinal: u64,
    pub boundary_validation_ordinal: Option<u64>,
    pub descriptor_ordinal: Option<u64>,
    pub observation_ordinal: Option<u64>,
    pub readiness_ordinal: Option<u64>,
    pub remaining_blockers: CallLinkReadinessBlockers,
    pub direct_call_status: GeneratedCallLinkDirectCallStatus,
}

/// Validated, immutable table for active generated call-link projections.
///
/// Construction validates current attached metadata only. The table never
/// mutates CodeBlock call IC state or emits call stubs.
#[derive(Clone, Debug, Eq, PartialEq)]
pub struct GeneratedCallLinkCandidateTable {
    owner: CodeBlockId,
    candidates: Vec<GeneratedCallLinkCandidate>,
}

impl GeneratedCallLinkCandidateTable {
    pub fn new(
        owner: CodeBlockId,
        candidates: Vec<GeneratedCallLinkCandidate>,
    ) -> Result<Self, InlineCacheValidationError> {
        for candidate in &candidates {
            validate_generated_call_link_candidate_for_table(owner, candidate)?;
        }

        for (index, candidate) in candidates.iter().enumerate() {
            if candidates[..index]
                .iter()
                .any(|prior| prior.attachment_ordinal == candidate.attachment_ordinal)
            {
                return Err(InlineCacheValidationError::CallLinkMismatch);
            }
            if candidates[..index]
                .iter()
                .any(|prior| prior.attachment_plan_ordinal == candidate.attachment_plan_ordinal)
            {
                return Err(InlineCacheValidationError::CallLinkMismatch);
            }

            let semantic_key = generated_call_link_candidate_semantic_key(candidate);
            if candidates[..index]
                .iter()
                .any(|prior| generated_call_link_candidate_semantic_key(prior) == semantic_key)
            {
                return Err(InlineCacheValidationError::CallLinkMismatch);
            }
        }

        Ok(Self { owner, candidates })
    }

    pub const fn owner(&self) -> CodeBlockId {
        self.owner
    }

    pub fn candidates(&self) -> &[GeneratedCallLinkCandidate] {
        &self.candidates
    }

    pub fn len(&self) -> usize {
        self.candidates.len()
    }

    pub fn is_empty(&self) -> bool {
        self.candidates.is_empty()
    }

    pub fn candidates_for_bytecode_index(
        &self,
        bytecode_index: u32,
    ) -> impl Iterator<Item = &GeneratedCallLinkCandidate> {
        self.candidates
            .iter()
            .filter(move |candidate| candidate.bytecode_index == bytecode_index)
    }
}

/// Facts decoded by a generated call-link sidecar.
///
/// The request borrows an already discovered candidate and records call-site
/// facts that can be observed without constructing a call frame. A matching
/// authorized candidate may request a VM-owned direct-call transaction.
#[derive(Clone, Copy, Debug, Eq, PartialEq)]
pub struct GeneratedCallLinkProbeRequest<'a> {
    pub candidate: &'a GeneratedCallLinkCandidate,
    pub owner: CodeBlockId,
    pub opcode: CoreOpcode,
    pub bytecode_index: u32,
    pub argument_count_including_this: u32,
    pub callee_value: RuntimeValue,
    pub callee_value_kind: ValueKind,
    pub callee_object: Option<ObjectId>,
    pub this_value: RuntimeValue,
    pub this_value_kind: ValueKind,
    pub this_object: Option<ObjectId>,
}

impl<'a> GeneratedCallLinkProbeRequest<'a> {
    #[allow(clippy::too_many_arguments)]
    pub const fn new(
        candidate: &'a GeneratedCallLinkCandidate,
        owner: CodeBlockId,
        opcode: CoreOpcode,
        bytecode_index: u32,
        argument_count_including_this: u32,
        callee_value: RuntimeValue,
        callee_value_kind: ValueKind,
        callee_object: Option<ObjectId>,
        this_value: RuntimeValue,
        this_value_kind: ValueKind,
        this_object: Option<ObjectId>,
    ) -> Self {
        Self {
            candidate,
            owner,
            opcode,
            bytecode_index,
            argument_count_including_this,
            callee_value,
            callee_value_kind,
            callee_object,
            this_value,
            this_value_kind,
            this_object,
        }
    }

    pub const fn expected_callee(&self) -> Option<ObjectId> {
        self.candidate.descriptor.callee
    }

    pub const fn expected_argument_count_including_this(&self) -> u8 {
        self.candidate.descriptor.max_argument_count_including_this
    }

    pub const fn expected_boundary(&self) -> Option<CallBoundaryId> {
        self.candidate.descriptor.boundary
    }

    pub fn callee_is_cell(&self) -> bool {
        self.callee_value_kind == ValueKind::Cell
    }

    pub fn this_is_cell(&self) -> bool {
        self.this_value_kind == ValueKind::Cell
    }
}

#[derive(Clone, Copy, Debug, Eq, PartialEq)]
pub struct GeneratedCallLinkProbeMiss {
    pub reason: GeneratedCallLinkProbeMissReason,
}

#[derive(Clone, Copy, Debug, Eq, PartialEq)]
pub struct GeneratedCallLinkProbeBlock {
    pub reason: GeneratedCallLinkProbeMissReason,
}

#[derive(Clone, Copy, Debug, Eq, PartialEq)]
pub struct GeneratedCallLinkDirectCall {
    pub slot: InlineCacheSlotId,
    pub attachment_ordinal: u64,
    pub attachment_plan_ordinal: u64,
    pub install_recheck_ordinal: u64,
    pub boundary_validation_ordinal: Option<u64>,
    pub descriptor_ordinal: Option<u64>,
    pub observation_ordinal: Option<u64>,
    pub readiness_ordinal: Option<u64>,
    pub target_executable: ExecutableId,
    pub target_callee: ObjectId,
    pub target_code_block: CodeBlockId,
    pub target_boundary: CallBoundaryId,
}

#[derive(Clone, Copy, Debug, Eq, PartialEq)]
pub enum GeneratedCallLinkProbeMissReason {
    HostUnavailable,
    OwnerMismatch,
    OpcodeMismatch,
    BytecodeIndexMismatch,
    ArgumentCountMismatch,
    MissingCalleeIdentity,
    NonCellCalleeIdentity,
    CalleeMismatch,
    DirectCallDisallowed,
    BoundaryRequiresInterpreter,
    CandidateNotFound,
    UnsupportedCandidateMetadata,
}

impl GeneratedCallLinkProbeMissReason {
    pub const fn is_metadata_blocker(self) -> bool {
        matches!(
            self,
            Self::DirectCallDisallowed | Self::BoundaryRequiresInterpreter
        )
    }
}

#[derive(Clone, Copy, Debug, Eq, PartialEq)]
pub enum GeneratedCallLinkProbeResult {
    DirectCall(GeneratedCallLinkDirectCall),
    Blocked(GeneratedCallLinkProbeBlock),
    Miss(GeneratedCallLinkProbeMiss),
}

impl GeneratedCallLinkProbeResult {
    pub fn for_request(request: &GeneratedCallLinkProbeRequest<'_>) -> Self {
        if let Some(reason) = generated_call_link_probe_miss_reason(request) {
            return Self::miss(reason);
        }

        if request.candidate.direct_call_status != GeneratedCallLinkDirectCallStatus::Authorized
            || request
                .candidate
                .remaining_blockers
                .contains(CallLinkReadinessBlocker::DirectCallDisallowed)
        {
            return Self::blocked(GeneratedCallLinkProbeMissReason::DirectCallDisallowed);
        }

        let mut unsupported_blockers = request.candidate.remaining_blockers;
        unsupported_blockers.remove(CallLinkReadinessBlocker::MayCallJsBoundary);
        if !unsupported_blockers.is_empty() {
            return Self::blocked(GeneratedCallLinkProbeMissReason::BoundaryRequiresInterpreter);
        }

        Self::DirectCall(GeneratedCallLinkDirectCall::from_candidate(
            request.candidate,
        ))
    }

    pub fn blocked(reason: GeneratedCallLinkProbeMissReason) -> Self {
        debug_assert!(reason.is_metadata_blocker());
        Self::Blocked(GeneratedCallLinkProbeBlock { reason })
    }

    pub const fn miss(reason: GeneratedCallLinkProbeMissReason) -> Self {
        Self::Miss(GeneratedCallLinkProbeMiss { reason })
    }

    pub const fn authorizes_direct_call(self) -> bool {
        matches!(self, Self::DirectCall(_))
    }
}

impl GeneratedCallLinkDirectCall {
    pub const fn from_candidate(candidate: &GeneratedCallLinkCandidate) -> Self {
        Self {
            slot: candidate.slot,
            attachment_ordinal: candidate.attachment_ordinal,
            attachment_plan_ordinal: candidate.attachment_plan_ordinal,
            install_recheck_ordinal: candidate.install_recheck_ordinal,
            boundary_validation_ordinal: candidate.boundary_validation_ordinal,
            descriptor_ordinal: candidate.descriptor_ordinal,
            observation_ordinal: candidate.observation_ordinal,
            readiness_ordinal: candidate.readiness_ordinal,
            target_executable: candidate.target.executable,
            target_callee: candidate.target.callee,
            target_code_block: candidate.target.target_code_block,
            target_boundary: candidate.boundary.id,
        }
    }
}

fn generated_call_link_probe_miss_reason(
    request: &GeneratedCallLinkProbeRequest<'_>,
) -> Option<GeneratedCallLinkProbeMissReason> {
    let candidate = request.candidate;
    if request.owner != candidate.owner {
        return Some(GeneratedCallLinkProbeMissReason::OwnerMismatch);
    }
    if request.opcode != candidate.opcode {
        return Some(GeneratedCallLinkProbeMissReason::OpcodeMismatch);
    }
    if request.bytecode_index != candidate.bytecode_index {
        return Some(GeneratedCallLinkProbeMissReason::BytecodeIndexMismatch);
    }
    if generated_call_link_probe_unsupported_candidate_metadata(candidate) {
        return Some(GeneratedCallLinkProbeMissReason::UnsupportedCandidateMetadata);
    }
    if request.argument_count_including_this
        != u32::from(candidate.descriptor.max_argument_count_including_this)
    {
        return Some(GeneratedCallLinkProbeMissReason::ArgumentCountMismatch);
    }
    if request.callee_value_kind != ValueKind::Cell {
        return Some(GeneratedCallLinkProbeMissReason::NonCellCalleeIdentity);
    }
    let Some(callee) = request.callee_object else {
        return Some(GeneratedCallLinkProbeMissReason::MissingCalleeIdentity);
    };
    if callee != candidate.target.callee {
        return Some(GeneratedCallLinkProbeMissReason::CalleeMismatch);
    }

    None
}

fn generated_call_link_probe_unsupported_candidate_metadata(
    candidate: &GeneratedCallLinkCandidate,
) -> bool {
    let Some(expected_call_kind) = linked_call_kind_for_opcode(candidate.opcode) else {
        return true;
    };
    let expected_specialization = call_link_specialization_for_kind(expected_call_kind);
    if candidate.descriptor.owner != Some(candidate.owner)
        || candidate.descriptor.call_kind != expected_call_kind
        || candidate.descriptor.mode != CallLinkMode::Monomorphic
        || candidate.target.specialization != expected_specialization
    {
        return true;
    }
    if candidate.descriptor.executable != Some(candidate.target.executable)
        || candidate.descriptor.callee != Some(candidate.target.callee)
        || candidate.descriptor.target_code_block != Some(candidate.target.target_code_block)
        || candidate.descriptor.boundary != Some(candidate.boundary.id)
        || candidate.boundary.owner != Some(candidate.owner)
        || candidate.boundary.abi != EntryAbi::LlIntCompatible
        || candidate.boundary.entry_kind != EntrypointKind::InterpreterThunk
        || candidate.boundary.native_symbol.is_some()
        || candidate.boundary.arguments.len()
            != usize::from(candidate.descriptor.max_argument_count_including_this)
        || candidate
            .boundary
            .arguments
            .iter()
            .any(|argument| *argument != AbiValue::JsValue)
        || candidate.boundary.returns.as_slice() != [AbiValue::JsValue]
        || !candidate.boundary.requires_vm_entry_scope
        || !candidate.boundary.may_call_js
        || !candidate.boundary.may_throw
    {
        return true;
    }

    validate_generated_call_link_direct_call_contract(candidate).is_err()
}

fn validate_generated_call_link_candidate_for_table(
    owner: CodeBlockId,
    candidate: &GeneratedCallLinkCandidate,
) -> Result<(), InlineCacheValidationError> {
    let Some(expected_call_kind) = linked_call_kind_for_opcode(candidate.opcode) else {
        return Err(InlineCacheValidationError::CallLinkMismatch);
    };
    let expected_specialization = call_link_specialization_for_kind(expected_call_kind);
    if candidate.owner != owner
        || candidate.descriptor.owner != Some(owner)
        || candidate.descriptor.call_kind != expected_call_kind
        || candidate.descriptor.mode != CallLinkMode::Monomorphic
        || candidate.target.specialization != expected_specialization
    {
        return Err(InlineCacheValidationError::CallLinkMismatch);
    }
    if candidate.descriptor.executable != Some(candidate.target.executable)
        || candidate.descriptor.callee != Some(candidate.target.callee)
        || candidate.descriptor.target_code_block != Some(candidate.target.target_code_block)
        || candidate.descriptor.boundary != Some(candidate.boundary.id)
        || candidate.boundary.owner != Some(owner)
        || candidate.boundary.abi != EntryAbi::LlIntCompatible
        || candidate.boundary.entry_kind != EntrypointKind::InterpreterThunk
        || candidate.boundary.native_symbol.is_some()
        || candidate.boundary.arguments.len()
            != usize::from(candidate.descriptor.max_argument_count_including_this)
        || candidate
            .boundary
            .arguments
            .iter()
            .any(|argument| *argument != AbiValue::JsValue)
        || candidate.boundary.returns.as_slice() != [AbiValue::JsValue]
        || !candidate.boundary.requires_vm_entry_scope
        || !candidate.boundary.may_call_js
        || !candidate.boundary.may_throw
    {
        return Err(InlineCacheValidationError::CallLinkMismatch);
    }
    validate_generated_call_link_direct_call_contract(candidate)?;

    validate_generated_call_link_candidate_ordinals(candidate)
}

fn validate_generated_call_link_direct_call_contract(
    candidate: &GeneratedCallLinkCandidate,
) -> Result<(), InlineCacheValidationError> {
    if !generated_call_link_direct_call_status_matches_blockers(
        candidate.direct_call_status,
        candidate.remaining_blockers,
    ) {
        return Err(InlineCacheValidationError::CallLinkMismatch);
    }

    if candidate.direct_call_status == GeneratedCallLinkDirectCallStatus::Authorized
        && !generated_call_link_authorized_ordinals_are_coherent(candidate)
    {
        return Err(InlineCacheValidationError::CallLinkMismatch);
    }

    Ok(())
}

fn generated_call_link_direct_call_status_matches_blockers(
    status: GeneratedCallLinkDirectCallStatus,
    blockers: CallLinkReadinessBlockers,
) -> bool {
    let mut unsupported_blockers = blockers;
    unsupported_blockers.remove(CallLinkReadinessBlocker::MayCallJsBoundary);
    match status {
        GeneratedCallLinkDirectCallStatus::Disallowed => {
            if !unsupported_blockers.contains(CallLinkReadinessBlocker::DirectCallDisallowed) {
                return false;
            }
            unsupported_blockers.remove(CallLinkReadinessBlocker::DirectCallDisallowed);
            unsupported_blockers.is_empty()
        }
        GeneratedCallLinkDirectCallStatus::Authorized => {
            !unsupported_blockers.contains(CallLinkReadinessBlocker::DirectCallDisallowed)
                && unsupported_blockers.is_empty()
        }
    }
}

fn generated_call_link_authorized_ordinals_are_coherent(
    candidate: &GeneratedCallLinkCandidate,
) -> bool {
    let (
        Some(boundary_validation_ordinal),
        Some(descriptor_ordinal),
        Some(observation_ordinal),
        Some(readiness_ordinal),
    ) = (
        candidate.boundary_validation_ordinal,
        candidate.descriptor_ordinal,
        candidate.observation_ordinal,
        candidate.readiness_ordinal,
    )
    else {
        return false;
    };

    observation_ordinal != 0
        && descriptor_ordinal != 0
        && readiness_ordinal != 0
        && boundary_validation_ordinal != 0
        && candidate.attachment_plan_ordinal != 0
        && candidate.install_recheck_ordinal != 0
        && candidate.attachment_ordinal != 0
        && observation_ordinal < descriptor_ordinal
        && descriptor_ordinal < readiness_ordinal
        && readiness_ordinal < boundary_validation_ordinal
        && boundary_validation_ordinal < candidate.attachment_plan_ordinal
        && candidate.attachment_plan_ordinal < candidate.install_recheck_ordinal
        && candidate.install_recheck_ordinal < candidate.attachment_ordinal
}

fn validate_generated_call_link_candidate_ordinals(
    candidate: &GeneratedCallLinkCandidate,
) -> Result<(), InlineCacheValidationError> {
    for ordinal in [
        candidate.attachment_ordinal,
        candidate.attachment_plan_ordinal,
        candidate.install_recheck_ordinal,
    ] {
        if ordinal == 0 {
            return Err(InlineCacheValidationError::CallLinkMismatch);
        }
    }

    for ordinal in [
        candidate.boundary_validation_ordinal,
        candidate.descriptor_ordinal,
        candidate.observation_ordinal,
        candidate.readiness_ordinal,
    ]
    .into_iter()
    .flatten()
    {
        if ordinal == 0 {
            return Err(InlineCacheValidationError::CallLinkMismatch);
        }
    }

    Ok(())
}

fn generated_call_link_candidate_semantic_key(
    candidate: &GeneratedCallLinkCandidate,
) -> (
    CoreOpcode,
    LinkedCallKind,
    u32,
    ExecutableId,
    ObjectId,
    CodeBlockId,
) {
    (
        candidate.opcode,
        candidate.descriptor.call_kind,
        candidate.bytecode_index,
        candidate
            .descriptor
            .executable
            .expect("validated generated call-link executable"),
        candidate
            .descriptor
            .callee
            .expect("validated generated call-link callee"),
        candidate
            .descriptor
            .target_code_block
            .expect("validated generated call-link target code block"),
    )
}

/// IC reset request. The VM will later translate this into handler unlinking,
/// slab restoration, and watchpoint cleanup.
#[derive(Clone, Debug, Eq, PartialEq)]
pub struct InlineCacheInvalidation {
    pub slot: InlineCacheSlotId,
    pub reason: InlineCacheInvalidationReason,
    pub affected_stubs: Vec<InlineCacheStubId>,
}

/// Reason an IC is no longer valid.
#[derive(Clone, Copy, Debug, Eq, PartialEq)]
pub enum InlineCacheInvalidationReason {
    WatchpointFired,
    OwnerCodeBlockDied,
    StubOwnerDied,
    WeakStructureDied,
    MegamorphicPolicy,
    ExplicitReset,
}

#[derive(Clone, Copy, Debug, Eq, PartialEq)]
pub enum InlineCacheMissKind {
    Cold,
    CaseMiss,
    WatchpointInvalidated,
    Megamorphic,
    Disabled,
}

#[derive(Clone, Copy, Debug, Eq, PartialEq)]
pub struct InlineCacheMissHandoffDescriptor {
    pub slot: InlineCacheSlotId,
    pub owner: CodeBlockId,
    pub bytecode_index: u32,
    pub cache_kind: InlineCacheKind,
    pub miss_kind: InlineCacheMissKind,
    pub fallback: InlineCacheFallbackSemantics,
    pub boundary: Option<CallBoundaryId>,
    pub call_link: Option<CallLinkInfoDescriptor>,
    pub preserves_operand_registers: bool,
}

impl InlineCacheMissHandoffDescriptor {
    pub fn from_slot(
        slot: &InlineCacheSlot,
        miss_kind: InlineCacheMissKind,
        boundary: Option<CallBoundaryId>,
    ) -> Result<Self, InlineCacheValidationError> {
        let owner = slot
            .owner
            .ok_or(InlineCacheValidationError::MissingHandoffOwner)?;
        let bytecode_index = slot
            .bytecode_index
            .ok_or(InlineCacheValidationError::MissingHandoffBytecodeIndex)?;
        let fallback = classify_inline_cache_semantics(slot)?.fallback;
        let call_link = slot
            .stubs
            .iter()
            .flat_map(|stub| stub.call_links.iter())
            .next()
            .copied();
        let descriptor = Self {
            slot: slot.id,
            owner,
            bytecode_index,
            cache_kind: slot.kind,
            miss_kind,
            fallback,
            boundary,
            call_link,
            preserves_operand_registers: !matches!(
                fallback,
                InlineCacheFallbackSemantics::SlowPathCall
            ),
        };
        descriptor.validate()?;
        Ok(descriptor)
    }

    pub fn validate(&self) -> Result<(), InlineCacheValidationError> {
        if matches!(self.fallback, InlineCacheFallbackSemantics::SlowPathCall)
            && self.boundary.is_none()
        {
            return Err(InlineCacheValidationError::MissingHandoffCallBoundary);
        }
        if !matches!(
            self.cache_kind,
            InlineCacheKind::Call | InlineCacheKind::Construct
        ) && self.call_link.is_some()
        {
            return Err(InlineCacheValidationError::CallLinkMismatch);
        }
        Ok(())
    }
}

/// Metadata-only record of one slow `GetByName` property-load observation.
///
/// The descriptor only carries lookup facts forward to later planning. It does
/// not mutate CodeBlock IC tables, allocate access cases, install watchpoints,
/// or claim generated property-load code is available.
#[derive(Clone, Debug, Eq, PartialEq)]
pub struct PropertyLoadObservationDescriptor {
    pub owner: CodeBlockId,
    pub slot: InlineCacheSlotId,
    pub bytecode_index: u32,
    pub cache_kind: InlineCacheKind,
    pub fallback: InlineCacheFallbackSemantics,
    pub key: CacheKey,
    pub base_object: Option<ObjectId>,
    pub holder_object: Option<ObjectId>,
    pub base_normalization: PropertyLoadBaseNormalization,
    pub base_structure: Option<StructureId>,
    pub offset: Option<PropertyOffset>,
    pub prototype_depth: u16,
    pub observed_access_case_kind: Option<AccessCaseKind>,
    pub may_call_js: bool,
    pub cacheability: PropertyCacheability,
    /// True only when the runtime proved this named property load satisfies
    /// JSC's GetById megamorphic key gate. Tiering must not infer this from an
    /// atom slot because the excluded names are text-sensitive.
    pub can_use_get_by_id_megamorphic: bool,
    pub readiness: PropertyLoadObservationReadiness,
    pub cold_miss_handoff: InlineCacheMissHandoffDescriptor,
    pub chain: Vec<PropertyLoadObservationChainEntry>,
}

/// Metadata-only record of one slow `InById`/`InByVal` has-property
/// observation.
///
/// This is deliberately separate from load observations: C++ JSC's
/// `HasProperty` slot records boolean presence and must not be consumed as a
/// value load or attach generated load code. VM tiering may later evolve
/// eligible named cases into has-megamorphic metadata using the same IC
/// thresholds as JSC.
#[derive(Clone, Debug, Eq, PartialEq)]
pub struct PropertyHasObservationDescriptor {
    pub owner: CodeBlockId,
    pub slot: InlineCacheSlotId,
    pub bytecode_index: u32,
    pub opcode: CoreOpcode,
    pub cache_kind: InlineCacheKind,
    pub fallback: InlineCacheFallbackSemantics,
    pub key: CacheKey,
    pub base_object: Option<ObjectId>,
    pub holder_object: Option<ObjectId>,
    pub base_structure: Option<StructureId>,
    pub offset: Option<PropertyOffset>,
    pub prototype_depth: u16,
    pub observed_access_case_kind: Option<AccessCaseKind>,
    pub result: bool,
    pub may_call_js: bool,
    pub cacheability: PropertyCacheability,
    pub can_use_in_by_id_megamorphic: bool,
    pub cold_miss_handoff: InlineCacheMissHandoffDescriptor,
    pub chain: Vec<PropertyLoadObservationChainEntry>,
}

impl PropertyHasObservationDescriptor {
    pub fn validate(&self) -> Result<(), InlineCacheValidationError> {
        if self.cold_miss_handoff.owner != self.owner {
            return Err(
                InlineCacheValidationError::PropertyObservationHandoffOwnerMismatch {
                    observation: self.owner,
                    handoff: self.cold_miss_handoff.owner,
                },
            );
        }
        if self.cold_miss_handoff.slot != self.slot {
            return Err(
                InlineCacheValidationError::PropertyObservationHandoffSlotMismatch {
                    observation: self.slot,
                    handoff: self.cold_miss_handoff.slot,
                },
            );
        }
        if self.cold_miss_handoff.bytecode_index != self.bytecode_index {
            return Err(
                InlineCacheValidationError::PropertyObservationHandoffBytecodeIndexMismatch {
                    observation: self.bytecode_index,
                    handoff: self.cold_miss_handoff.bytecode_index,
                },
            );
        }
        if self.cold_miss_handoff.cache_kind != self.cache_kind {
            return Err(
                InlineCacheValidationError::PropertyObservationHandoffCacheKindMismatch {
                    observation: self.cache_kind,
                    handoff: self.cold_miss_handoff.cache_kind,
                },
            );
        }
        if self.cold_miss_handoff.fallback != self.fallback {
            return Err(
                InlineCacheValidationError::PropertyObservationHandoffFallbackMismatch {
                    observation: self.fallback,
                    handoff: self.cold_miss_handoff.fallback,
                },
            );
        }
        if self.cache_kind != InlineCacheKind::HasProperty {
            return Err(
                InlineCacheValidationError::PropertyObservationUnsupportedCacheKind {
                    expected: InlineCacheKind::HasProperty,
                    actual: self.cache_kind,
                },
            );
        }
        if !matches!(self.opcode, CoreOpcode::InById | CoreOpcode::InByVal) {
            return Err(
                InlineCacheValidationError::PropertyHasObservationUnsupportedOpcode {
                    opcode: self.opcode,
                },
            );
        }
        if self.fallback != InlineCacheFallbackSemantics::SlowPathLookup {
            return Err(
                InlineCacheValidationError::PropertyObservationUnsupportedFallback {
                    expected: InlineCacheFallbackSemantics::SlowPathLookup,
                    actual: self.fallback,
                },
            );
        }
        if self.cold_miss_handoff.miss_kind != InlineCacheMissKind::Cold {
            return Err(
                InlineCacheValidationError::PropertyObservationHandoffMissKindMismatch {
                    expected: InlineCacheMissKind::Cold,
                    actual: self.cold_miss_handoff.miss_kind,
                },
            );
        }
        if self.cold_miss_handoff.boundary.is_some() {
            return Err(InlineCacheValidationError::PropertyObservationBoundaryContamination);
        }
        if self.cold_miss_handoff.call_link.is_some() {
            return Err(InlineCacheValidationError::PropertyObservationCallLinkContamination);
        }
        if !self.cold_miss_handoff.preserves_operand_registers {
            return Err(
                InlineCacheValidationError::PropertyObservationHandoffClobbersOperandRegisters,
            );
        }
        self.cold_miss_handoff.validate()?;

        let schema = INLINE_CACHE_SCHEMA_REGISTRY
            .schema_for_kind(self.cache_kind)
            .ok_or(InlineCacheValidationError::DispatchMismatch)?;
        if let Some(access_case_kind) = self.observed_access_case_kind {
            if !schema.allowed_cases.contains(&access_case_kind) {
                return Err(InlineCacheValidationError::CaseNotAllowed(access_case_kind));
            }
            if property_has_observation_true_case(access_case_kind) && !self.result {
                return Err(
                    InlineCacheValidationError::PropertyHasObservationResultMismatch {
                        result: self.result,
                        access_case_kind,
                    },
                );
            }
            if property_has_observation_false_case(access_case_kind) && self.result {
                return Err(
                    InlineCacheValidationError::PropertyHasObservationResultMismatch {
                        result: self.result,
                        access_case_kind,
                    },
                );
            }
            if property_has_observation_proxy_case(access_case_kind) && !self.may_call_js {
                return Err(InlineCacheValidationError::CallLinkMismatch);
            }
            if self.may_call_js && !schema.may_call_js {
                return Err(InlineCacheValidationError::CallLinkMismatch);
            }
        }

        if matches!(self.key, CacheKey::Dynamic) {
            if self.cacheability == PropertyCacheability::Allowed
                || self.can_use_in_by_id_megamorphic
            {
                return Err(InlineCacheValidationError::PropertyHasPlanUnsupportedKey(
                    self.key,
                ));
            }
        }
        if self.can_use_in_by_id_megamorphic
            && !matches!(self.key, CacheKey::Property(PropertyKey::String(_)))
        {
            return Err(InlineCacheValidationError::PropertyHasPlanUnsupportedKey(
                self.key,
            ));
        }
        if self.base_structure == Some(StructureId::INVALID) {
            return Err(
                InlineCacheValidationError::PropertyLoadPlanInvalidBaseStructure(
                    StructureId::INVALID,
                ),
            );
        }
        Ok(())
    }
}

const fn property_has_observation_true_case(kind: AccessCaseKind) -> bool {
    matches!(
        kind,
        AccessCaseKind::InHit
            | AccessCaseKind::IndexedInt32InHit
            | AccessCaseKind::IndexedDoubleInHit
            | AccessCaseKind::IndexedContiguousInHit
            | AccessCaseKind::IndexedArrayStorageInHit
            | AccessCaseKind::IndexedScopedArgumentsInHit
            | AccessCaseKind::IndexedDirectArgumentsInHit
            | AccessCaseKind::IndexedStringInHit
    )
}

const fn property_has_observation_false_case(kind: AccessCaseKind) -> bool {
    matches!(
        kind,
        AccessCaseKind::InMiss | AccessCaseKind::IndexedNoIndexingInMiss
    )
}

const fn property_has_observation_proxy_case(kind: AccessCaseKind) -> bool {
    matches!(
        kind,
        AccessCaseKind::ProxyObjectIn | AccessCaseKind::IndexedProxyObjectIn
    )
}

impl PropertyLoadObservationDescriptor {
    pub fn classify_readiness(&self) -> PropertyLoadObservationReadiness {
        let blockers = self.blockers();
        if blockers.is_empty() {
            PropertyLoadObservationReadiness::ReadyForAttachment
        } else {
            PropertyLoadObservationReadiness::Blocked(blockers)
        }
    }

    pub fn blockers(&self) -> PropertyLoadObservationBlockers {
        let mut blockers = PropertyLoadObservationBlockers::empty();

        if property_load_observation_requires_base_structure_guard(self.observed_access_case_kind)
            && !property_load_observation_has_valid_base_structure(self.base_structure)
        {
            blockers.insert(PropertyLoadObservationBlocker::MissingBaseStructureGuard);
        }

        if property_load_observation_requires_offset(self.observed_access_case_kind)
            && !property_load_observation_has_valid_offset(self.offset)
        {
            blockers.insert(PropertyLoadObservationBlocker::MissingOffset);
        }

        if self.prototype_depth > 0 {
            blockers.insert(PropertyLoadObservationBlocker::PrototypeChainGuardRequired);
        }

        if self.may_call_js {
            blockers.insert(PropertyLoadObservationBlocker::MayCallJsBoundary);
        }

        match self.cacheability {
            PropertyCacheability::Allowed => {
                if self.observed_access_case_kind.is_none() {
                    blockers.insert(PropertyLoadObservationBlocker::UncacheableResult);
                }
            }
            PropertyCacheability::Disallowed => {
                blockers.insert(PropertyLoadObservationBlocker::UncacheableResult);
            }
            PropertyCacheability::TaintedByOpaqueObject => {
                blockers.insert(PropertyLoadObservationBlocker::OpaqueResult);
            }
        }

        if self.observed_access_case_kind == Some(AccessCaseKind::Miss) {
            blockers.insert(PropertyLoadObservationBlocker::NegativeLookupGuardRequired);
        }

        blockers
    }

    pub fn validate(&self) -> Result<(), InlineCacheValidationError> {
        if self.cold_miss_handoff.owner != self.owner {
            return Err(
                InlineCacheValidationError::PropertyObservationHandoffOwnerMismatch {
                    observation: self.owner,
                    handoff: self.cold_miss_handoff.owner,
                },
            );
        }
        if self.cold_miss_handoff.slot != self.slot {
            return Err(
                InlineCacheValidationError::PropertyObservationHandoffSlotMismatch {
                    observation: self.slot,
                    handoff: self.cold_miss_handoff.slot,
                },
            );
        }
        if self.cold_miss_handoff.bytecode_index != self.bytecode_index {
            return Err(
                InlineCacheValidationError::PropertyObservationHandoffBytecodeIndexMismatch {
                    observation: self.bytecode_index,
                    handoff: self.cold_miss_handoff.bytecode_index,
                },
            );
        }
        if self.cold_miss_handoff.cache_kind != self.cache_kind {
            return Err(
                InlineCacheValidationError::PropertyObservationHandoffCacheKindMismatch {
                    observation: self.cache_kind,
                    handoff: self.cold_miss_handoff.cache_kind,
                },
            );
        }
        if self.cold_miss_handoff.fallback != self.fallback {
            return Err(
                InlineCacheValidationError::PropertyObservationHandoffFallbackMismatch {
                    observation: self.fallback,
                    handoff: self.cold_miss_handoff.fallback,
                },
            );
        }

        if !matches!(
            self.cache_kind,
            InlineCacheKind::PropertyLoad | InlineCacheKind::ElementLoad
        ) {
            return Err(
                InlineCacheValidationError::PropertyObservationUnsupportedCacheKind {
                    expected: InlineCacheKind::PropertyLoad,
                    actual: self.cache_kind,
                },
            );
        }
        if self.fallback != InlineCacheFallbackSemantics::SlowPathLookup {
            return Err(
                InlineCacheValidationError::PropertyObservationUnsupportedFallback {
                    expected: InlineCacheFallbackSemantics::SlowPathLookup,
                    actual: self.fallback,
                },
            );
        }
        if self.cold_miss_handoff.miss_kind != InlineCacheMissKind::Cold {
            return Err(
                InlineCacheValidationError::PropertyObservationHandoffMissKindMismatch {
                    expected: InlineCacheMissKind::Cold,
                    actual: self.cold_miss_handoff.miss_kind,
                },
            );
        }
        if self.cold_miss_handoff.boundary.is_some() {
            return Err(InlineCacheValidationError::PropertyObservationBoundaryContamination);
        }
        if self.cold_miss_handoff.call_link.is_some() {
            return Err(InlineCacheValidationError::PropertyObservationCallLinkContamination);
        }
        if !self.cold_miss_handoff.preserves_operand_registers {
            return Err(
                InlineCacheValidationError::PropertyObservationHandoffClobbersOperandRegisters,
            );
        }

        self.cold_miss_handoff.validate()?;

        let schema = INLINE_CACHE_SCHEMA_REGISTRY
            .schema_for_kind(self.cache_kind)
            .ok_or(InlineCacheValidationError::DispatchMismatch)?;
        if let Some(access_case_kind) = self.observed_access_case_kind {
            if !schema.allowed_cases.contains(&access_case_kind) {
                return Err(InlineCacheValidationError::CaseNotAllowed(access_case_kind));
            }
            if self.may_call_js && !schema.may_call_js {
                return Err(InlineCacheValidationError::CallLinkMismatch);
            }
        }

        let expected_readiness = self.classify_readiness();
        if self.readiness != expected_readiness {
            if self.readiness.is_ready() && !expected_readiness.blockers().is_empty() {
                return Err(
                    InlineCacheValidationError::PropertyObservationReadyButBlocked(
                        expected_readiness.blockers(),
                    ),
                );
            }
            return Err(
                InlineCacheValidationError::PropertyObservationReadinessMismatch {
                    expected: expected_readiness,
                    actual: self.readiness,
                },
            );
        }

        Ok(())
    }
}

pub fn plan_property_load_access_case_from_observation(
    observation: &PropertyLoadObservationDescriptor,
) -> Result<Option<PropertyLoadAccessCasePlan>, InlineCacheValidationError> {
    observation.validate()?;

    if observation.base_normalization != PropertyLoadBaseNormalization::None {
        return Ok(None);
    }

    let is_named_own_load = observation.cache_kind == InlineCacheKind::PropertyLoad
        && observation.observed_access_case_kind == Some(AccessCaseKind::Load)
        && property_load_access_case_key_supports_generated_own_data_plan(observation.key);
    let is_indexed_load = observation.cache_kind == InlineCacheKind::ElementLoad
        && observation.observed_access_case_kind == Some(AccessCaseKind::IndexedLoad)
        && property_load_access_case_key_supports_generated_indexed_plan(observation.key);

    if observation.readiness != PropertyLoadObservationReadiness::ReadyForAttachment
        || observation.fallback != InlineCacheFallbackSemantics::SlowPathLookup
        || observation.cacheability != PropertyCacheability::Allowed
        || observation.may_call_js
        || observation.prototype_depth != 0
        || (!is_named_own_load && !is_indexed_load)
        || observation.base_object.is_none()
        || observation.holder_object != observation.base_object
    {
        return Ok(None);
    }

    let Some(base_structure) = observation.base_structure else {
        return Ok(None);
    };
    if !property_load_observation_has_valid_base_structure(Some(base_structure)) {
        return Ok(None);
    }

    let (plan_kind, access_case_kind, offset, cache_kind) = if is_indexed_load {
        if observation.offset.is_some() {
            return Ok(None);
        }
        (
            PropertyLoadAccessCasePlanKind::DataOnlyIndexedLoad,
            AccessCaseKind::IndexedLoad,
            None,
            InlineCacheKind::ElementLoad,
        )
    } else {
        let Some(offset) = observation.offset else {
            return Ok(None);
        };
        if !property_load_observation_has_valid_offset(Some(offset)) {
            return Ok(None);
        }
        (
            PropertyLoadAccessCasePlanKind::DataOnlyOwnLoad,
            AccessCaseKind::Load,
            Some(offset),
            InlineCacheKind::PropertyLoad,
        )
    };

    let access_case = AccessCaseDescriptor {
        kind: access_case_kind,
        key: observation.key,
        base_structure: Some(base_structure),
        new_structure: None,
        holder: None,
        offset,
        via_global_proxy: false,
        may_call_js: false,
        dependencies: Vec::new(),
    };

    let temporary_slot = InlineCacheSlot::builder(observation.slot, cache_kind)
        .owner(observation.owner)
        .bytecode_index(observation.bytecode_index)
        .state(InlineCacheState::Monomorphic)
        .case(access_case.clone())
        .build()?;
    classify_inline_cache_slot(&temporary_slot)?;

    Ok(Some(PropertyLoadAccessCasePlan {
        plan_kind,
        owner: observation.owner,
        slot: observation.slot,
        bytecode_index: observation.bytecode_index,
        key: observation.key,
        access_case,
        planned_stub_kind: InlineCacheStubKind::DataOnlyHandler,
        effect_contract: PropertyLoadAccessCasePlanContract::DATA_ONLY_OWN_LOAD,
    }))
}

pub fn plan_property_load_guard_plan_from_observation(
    observation: &PropertyLoadObservationDescriptor,
) -> Result<Option<PropertyLoadGuardPlan>, InlineCacheValidationError> {
    observation.validate()?;

    if observation.cache_kind != InlineCacheKind::PropertyLoad
        || observation.fallback != InlineCacheFallbackSemantics::SlowPathLookup
        || observation.cacheability != PropertyCacheability::Allowed
        || observation.may_call_js
        || !property_load_guard_key_supports_named_property_proof(observation.key)
    {
        return Ok(None);
    }

    let Some(base_object) = observation.base_object else {
        return Ok(None);
    };
    let Some(base_structure) = observation.base_structure else {
        return Ok(None);
    };
    if !property_load_observation_has_valid_base_structure(Some(base_structure)) {
        return Ok(None);
    }
    let Some(chain) =
        property_load_guard_valid_observation_chain(observation, base_object, base_structure)
    else {
        return Ok(None);
    };

    let descriptor = match observation.observed_access_case_kind {
        Some(AccessCaseKind::Load) if observation.prototype_depth > 0 => {
            let Some(holder_object) = observation.holder_object else {
                return Ok(None);
            };
            if holder_object == base_object {
                return Ok(None);
            }
            let Some(offset) = observation.offset else {
                return Ok(None);
            };
            if !property_load_observation_has_valid_offset(Some(offset)) {
                return Ok(None);
            }
            let holder_index = usize::from(observation.prototype_depth);
            if chain.len() != holder_index.saturating_add(1) {
                return Ok(None);
            }
            if !chain.last().is_some_and(|entry| {
                entry.object == holder_object && entry.next_prototype.is_none()
            }) {
                return Ok(None);
            }
            let chain = PropertyLoadGuardChainCertificate {
                entries: property_load_guard_chain_entries_with_data_holder(
                    chain,
                    holder_index,
                    offset,
                ),
                outcome: PropertyLoadGuardChainOutcome::PrototypeData {
                    holder_index,
                    offset,
                },
            };

            PropertyLoadGuardDescriptor {
                requirement: PropertyLoadGuardRequirement::PrototypeChain,
                key: observation.key,
                base_object,
                holder_object: Some(holder_object),
                base_structure,
                offset: Some(offset),
                prototype_depth: observation.prototype_depth,
                chain,
            }
        }
        Some(AccessCaseKind::Miss)
            if observation.holder_object.is_none()
                && observation.offset.is_none()
                && chain.len() == usize::from(observation.prototype_depth).saturating_add(1) =>
        {
            if chain
                .last()
                .is_none_or(|entry| entry.next_prototype.is_some())
            {
                return Ok(None);
            }
            let chain = PropertyLoadGuardChainCertificate {
                entries: property_load_guard_chain_entries_with_no_own_property(chain),
                outcome: PropertyLoadGuardChainOutcome::Missing {
                    terminal_null: true,
                },
            };
            PropertyLoadGuardDescriptor {
                requirement: PropertyLoadGuardRequirement::NegativeLookup,
                key: observation.key,
                base_object,
                holder_object: None,
                base_structure,
                offset: None,
                prototype_depth: observation.prototype_depth,
                chain,
            }
        }
        _ => return Ok(None),
    };

    Ok(Some(PropertyLoadGuardPlan {
        owner: observation.owner,
        slot: observation.slot,
        bytecode_index: observation.bytecode_index,
        descriptor,
    }))
}

pub fn derive_property_load_guard_dependencies(
    plan: &PropertyLoadGuardPlan,
    mut allocate_ids: impl FnMut(usize) -> (WatchpointSetId, WatchpointDependencyId),
) -> Vec<PropertyLoadGuardDependencyDescriptor> {
    plan.descriptor
        .chain
        .entries
        .iter()
        .enumerate()
        .map(|(chain_index, entry)| {
            let (set_id, dependency_id) = allocate_ids(chain_index);
            let dependency = WatchpointDependency {
                id: dependency_id,
                strength: DependencyStrength::CompileTimeAssumption,
                target: WatchpointTarget::StructureTransition {
                    structure: entry.structure,
                },
                generation: None,
            };
            let set = WatchpointSetDescriptor {
                id: set_id,
                owner: WatchpointOwner::Structure(entry.structure),
                state: WatchpointSetState::Clear,
                fire_policy: WatchpointFirePolicy::RecheckBeforeInstall,
                dependencies: vec![dependency.id],
            };
            PropertyLoadGuardDependencyDescriptor {
                chain_index,
                set,
                dependency,
            }
        })
        .collect()
}

fn property_load_guard_key_supports_named_property_proof(key: CacheKey) -> bool {
    match key {
        CacheKey::Property(property_key) => property_key.as_identifier().is_some(),
        CacheKey::Dynamic => false,
    }
}

fn property_load_access_case_key_supports_generated_own_data_plan(key: CacheKey) -> bool {
    match key {
        CacheKey::Property(property_key) => property_key.as_identifier().is_some(),
        CacheKey::Dynamic => false,
    }
}

fn property_load_access_case_key_supports_generated_indexed_plan(key: CacheKey) -> bool {
    match key {
        CacheKey::Property(property_key) => property_key.as_index().is_some(),
        CacheKey::Dynamic => false,
    }
}

#[allow(dead_code)]
fn property_store_access_case_key_supports_metadata_plan(
    plan_kind: PropertyStoreAccessCasePlanKind,
    key: CacheKey,
) -> bool {
    match (plan_kind, key) {
        (
            PropertyStoreAccessCasePlanKind::DataOnlyReplace
            | PropertyStoreAccessCasePlanKind::DataOnlyTransition,
            CacheKey::Property(property_key),
        ) => property_key.as_identifier().is_some(),
        (
            PropertyStoreAccessCasePlanKind::DataOnlyIndexedStore,
            CacheKey::Property(property_key),
        ) => property_key.as_index().is_some(),
        (_, CacheKey::Dynamic) => false,
        (PropertyStoreAccessCasePlanKind::Unsupported, _) => false,
    }
}

fn property_load_guard_valid_observation_chain(
    observation: &PropertyLoadObservationDescriptor,
    base_object: ObjectId,
    base_structure: StructureId,
) -> Option<&[PropertyLoadObservationChainEntry]> {
    let chain = observation.chain.as_slice();
    let first = chain.first()?;
    if first.object != base_object || first.structure != base_structure {
        return None;
    }
    if chain
        .iter()
        .any(|entry| entry.structure == StructureId::INVALID)
    {
        return None;
    }
    for window in chain.windows(2) {
        if window[0].next_prototype != Some(window[1].object) {
            return None;
        }
    }
    Some(chain)
}

fn property_load_guard_chain_entries_with_data_holder(
    chain: &[PropertyLoadObservationChainEntry],
    holder_index: usize,
    offset: PropertyOffset,
) -> Vec<PropertyLoadGuardChainEntry> {
    let mut entries = Vec::with_capacity(chain.len());
    for (index, entry) in chain.iter().enumerate() {
        let proof = if index == holder_index {
            PropertyLoadGuardChainEntryProof::DataProperty { offset }
        } else {
            PropertyLoadGuardChainEntryProof::NoOwnProperty
        };
        entries.push(PropertyLoadGuardChainEntry {
            object: entry.object,
            structure: entry.structure,
            next_prototype: entry.next_prototype,
            proof,
        });
    }
    entries
}

fn property_load_guard_chain_entries_with_no_own_property(
    chain: &[PropertyLoadObservationChainEntry],
) -> Vec<PropertyLoadGuardChainEntry> {
    chain
        .iter()
        .map(|entry| PropertyLoadGuardChainEntry {
            object: entry.object,
            structure: entry.structure,
            next_prototype: entry.next_prototype,
            proof: PropertyLoadGuardChainEntryProof::NoOwnProperty,
        })
        .collect()
}

fn property_load_observation_requires_base_structure_guard(
    access_case_kind: Option<AccessCaseKind>,
) -> bool {
    matches!(
        access_case_kind,
        Some(
            AccessCaseKind::Load
                | AccessCaseKind::Getter
                | AccessCaseKind::IntrinsicGetter
                | AccessCaseKind::ArrayLength
                | AccessCaseKind::StringLength
                | AccessCaseKind::ModuleNamespaceLoad
                | AccessCaseKind::IndexedLoad
                | AccessCaseKind::Miss
        )
    )
}

fn property_load_observation_has_valid_base_structure(base_structure: Option<StructureId>) -> bool {
    match base_structure {
        Some(base_structure) => base_structure != StructureId::INVALID,
        None => false,
    }
}

fn property_load_observation_requires_offset(access_case_kind: Option<AccessCaseKind>) -> bool {
    matches!(access_case_kind, Some(AccessCaseKind::Load))
}

fn property_load_observation_has_valid_offset(offset: Option<PropertyOffset>) -> bool {
    match offset {
        Some(offset) => offset.raw() >= 0,
        None => false,
    }
}

const fn semantic_class_for_kind(kind: InlineCacheKind) -> InlineCacheSemanticClass {
    match kind {
        InlineCacheKind::PropertyLoad => InlineCacheSemanticClass::PropertyRead,
        InlineCacheKind::PropertyStore => InlineCacheSemanticClass::PropertyWrite,
        InlineCacheKind::ElementLoad => InlineCacheSemanticClass::ElementRead,
        InlineCacheKind::ElementStore => InlineCacheSemanticClass::ElementWrite,
        InlineCacheKind::GlobalLoad => InlineCacheSemanticClass::GlobalRead,
        InlineCacheKind::GlobalStore => InlineCacheSemanticClass::GlobalWrite,
        InlineCacheKind::Call => InlineCacheSemanticClass::Call,
        InlineCacheKind::Construct => InlineCacheSemanticClass::Construct,
        InlineCacheKind::Delete => InlineCacheSemanticClass::Delete,
        InlineCacheKind::HasProperty => InlineCacheSemanticClass::HasProperty,
        InlineCacheKind::InstanceOf => InlineCacheSemanticClass::InstanceOf,
        InlineCacheKind::PrivateBrand => InlineCacheSemanticClass::PrivateBrand,
    }
}

const fn effects_for_cache_kind(kind: InlineCacheKind) -> EffectSummary {
    match kind {
        InlineCacheKind::PropertyLoad
        | InlineCacheKind::ElementLoad
        | InlineCacheKind::GlobalLoad
        | InlineCacheKind::HasProperty
        | InlineCacheKind::InstanceOf => EffectSummary {
            reads_heap: true,
            ..EffectSummary::pure()
        },
        InlineCacheKind::PropertyStore
        | InlineCacheKind::ElementStore
        | InlineCacheKind::GlobalStore
        | InlineCacheKind::Delete
        | InlineCacheKind::PrivateBrand => EffectSummary {
            reads_heap: true,
            writes_heap: true,
            may_throw: true,
            ..EffectSummary::pure()
        },
        InlineCacheKind::Call => EffectSummary::for_call(),
        InlineCacheKind::Construct => EffectSummary {
            allocates: true,
            ..EffectSummary::for_call()
        },
    }
}

fn effects_for_access_case(case: &AccessCaseDescriptor) -> EffectSummary {
    let mut effects = match case.kind {
        AccessCaseKind::Load
        | AccessCaseKind::Miss
        | AccessCaseKind::ArrayLength
        | AccessCaseKind::StringLength
        | AccessCaseKind::ModuleNamespaceLoad
        | AccessCaseKind::IndexedLoad
        | AccessCaseKind::IndexedIn
        | AccessCaseKind::InHit
        | AccessCaseKind::InMiss
        | AccessCaseKind::InMegamorphic
        | AccessCaseKind::IndexedMegamorphicIn
        | AccessCaseKind::IndexedInt32InHit
        | AccessCaseKind::IndexedDoubleInHit
        | AccessCaseKind::IndexedContiguousInHit
        | AccessCaseKind::IndexedArrayStorageInHit
        | AccessCaseKind::IndexedScopedArgumentsInHit
        | AccessCaseKind::IndexedDirectArgumentsInHit
        | AccessCaseKind::IndexedTypedArrayInt8In
        | AccessCaseKind::IndexedTypedArrayUint8In
        | AccessCaseKind::IndexedTypedArrayUint8ClampedIn
        | AccessCaseKind::IndexedTypedArrayInt16In
        | AccessCaseKind::IndexedTypedArrayUint16In
        | AccessCaseKind::IndexedTypedArrayInt32In
        | AccessCaseKind::IndexedTypedArrayUint32In
        | AccessCaseKind::IndexedTypedArrayFloat16In
        | AccessCaseKind::IndexedTypedArrayFloat32In
        | AccessCaseKind::IndexedTypedArrayFloat64In
        | AccessCaseKind::IndexedResizableTypedArrayInt8In
        | AccessCaseKind::IndexedResizableTypedArrayUint8In
        | AccessCaseKind::IndexedResizableTypedArrayUint8ClampedIn
        | AccessCaseKind::IndexedResizableTypedArrayInt16In
        | AccessCaseKind::IndexedResizableTypedArrayUint16In
        | AccessCaseKind::IndexedResizableTypedArrayInt32In
        | AccessCaseKind::IndexedResizableTypedArrayUint32In
        | AccessCaseKind::IndexedResizableTypedArrayFloat16In
        | AccessCaseKind::IndexedResizableTypedArrayFloat32In
        | AccessCaseKind::IndexedResizableTypedArrayFloat64In
        | AccessCaseKind::IndexedStringInHit
        | AccessCaseKind::IndexedNoIndexingInMiss
        | AccessCaseKind::InstanceOf => EffectSummary {
            reads_heap: true,
            ..EffectSummary::pure()
        },
        AccessCaseKind::Replace
        | AccessCaseKind::Transition
        | AccessCaseKind::Delete
        | AccessCaseKind::IndexedStore => EffectSummary {
            reads_heap: true,
            writes_heap: true,
            may_throw: true,
            ..EffectSummary::pure()
        },
        AccessCaseKind::Getter
        | AccessCaseKind::Setter
        | AccessCaseKind::CustomAccessor
        | AccessCaseKind::ProxyObject
        | AccessCaseKind::ProxyObjectIn
        | AccessCaseKind::IndexedProxyObjectIn => EffectSummary::for_call(),
        AccessCaseKind::IntrinsicGetter => EffectSummary {
            reads_heap: true,
            may_throw: true,
            ..EffectSummary::pure()
        },
        AccessCaseKind::Megamorphic => EffectSummary::for_call(),
    };
    if case.may_call_js {
        effects = effects.union(EffectSummary::for_call());
    }
    effects
}

fn slot_requires_barrier_metadata(slot: &InlineCacheSlot) -> bool {
    cache_kind_requires_barrier_metadata(slot.kind)
        || slot.cases.iter().any(access_case_requires_barrier_metadata)
        || slot.stubs.iter().any(|stub| {
            !stub.weak_structures.is_empty()
                && stub.cases.iter().any(access_case_requires_barrier_metadata)
        })
}

const fn cache_kind_requires_barrier_metadata(kind: InlineCacheKind) -> bool {
    matches!(
        kind,
        InlineCacheKind::PropertyStore
            | InlineCacheKind::ElementStore
            | InlineCacheKind::GlobalStore
            | InlineCacheKind::PrivateBrand
    )
}

fn access_case_requires_barrier_metadata(case: &AccessCaseDescriptor) -> bool {
    case.new_structure.is_some()
        || matches!(
            case.kind,
            AccessCaseKind::Replace | AccessCaseKind::Transition | AccessCaseKind::IndexedStore
        )
}

fn fallback_for_slot(slot: &InlineCacheSlot) -> InlineCacheFallbackSemantics {
    match slot.state {
        InlineCacheState::Disabled => InlineCacheFallbackSemantics::Disabled,
        InlineCacheState::Megamorphic => InlineCacheFallbackSemantics::MegamorphicGeneric,
        _ if matches!(
            slot.kind,
            InlineCacheKind::Call | InlineCacheKind::Construct
        ) =>
        {
            InlineCacheFallbackSemantics::SlowPathCall
        }
        _ if slot.cases.is_empty() => InlineCacheFallbackSemantics::SlowPathLookup,
        _ => InlineCacheFallbackSemantics::None,
    }
}

#[derive(Clone, Copy, Debug, Default, Eq, PartialEq, Hash)]
pub enum InlineCacheSchemaOwner {
    #[default]
    InlineCacheRegistry,
    BaselineJit,
    DfgJit,
    FtlJit,
}

#[derive(Clone, Copy, Debug, Default, Eq, PartialEq, Hash)]
pub enum InlineCacheRegistryMutationAuthority {
    #[default]
    GeneratedStaticDataRefresh,
    CrateInitialization,
}

/// Immutable schema for one inline-cache slot family.
///
/// Access cases and stubs are generated or linked elsewhere. This schema only
/// states which static metadata classes may be attached to a slot.
#[derive(Clone, Copy, Debug, Eq, PartialEq)]
pub struct StaticInlineCacheSchema {
    pub kind: InlineCacheKind,
    pub name: &'static str,
    pub dispatch: InlineCacheDispatch,
    pub allowed_cases: &'static [AccessCaseKind],
    pub allowed_stub_kinds: &'static [InlineCacheStubKind],
    pub may_call_js: bool,
    pub owns_watchpoints: bool,
    pub owner: InlineCacheSchemaOwner,
    pub mutation_authority: InlineCacheRegistryMutationAuthority,
    pub provenance: &'static str,
}

#[derive(Clone, Copy, Debug, Default, Eq, PartialEq)]
pub struct InlineCacheSchemaRegistry {
    pub schemas: &'static [StaticInlineCacheSchema],
}

impl InlineCacheSchemaRegistry {
    pub const fn new(schemas: &'static [StaticInlineCacheSchema]) -> Self {
        Self { schemas }
    }

    pub const fn schemas(self) -> &'static [StaticInlineCacheSchema] {
        self.schemas
    }

    pub fn schema_for_kind(
        self,
        kind: InlineCacheKind,
    ) -> Option<&'static StaticInlineCacheSchema> {
        self.schemas.iter().find(|schema| schema.kind == kind)
    }

    pub fn validate(self) -> Result<(), InlineCacheValidationError> {
        for (index, schema) in self.schemas.iter().enumerate() {
            schema.validate()?;
            if self.schemas[index + 1..]
                .iter()
                .any(|other| other.kind == schema.kind)
            {
                return Err(InlineCacheValidationError::DuplicateSchemaKind(schema.kind));
            }
        }

        Ok(())
    }
}

impl StaticInlineCacheSchema {
    pub fn validate(&self) -> Result<(), InlineCacheValidationError> {
        if self.name.is_empty() {
            return Err(InlineCacheValidationError::EmptyName);
        }
        if self.provenance.is_empty() {
            return Err(InlineCacheValidationError::EmptyProvenance(self.name));
        }
        if self.allowed_cases.is_empty() {
            return Err(InlineCacheValidationError::EmptyAllowedCases(self.name));
        }
        if self.allowed_stub_kinds.is_empty() {
            return Err(InlineCacheValidationError::EmptyAllowedStubKinds(self.name));
        }
        if !self.may_call_js
            && self
                .allowed_stub_kinds
                .contains(&InlineCacheStubKind::HandlerWithCallLinkInfo)
        {
            return Err(InlineCacheValidationError::CallLinkMismatch);
        }

        Ok(())
    }
}

const PROPERTY_LOAD_CASES: &[AccessCaseKind] = &[
    AccessCaseKind::Load,
    AccessCaseKind::Getter,
    AccessCaseKind::IntrinsicGetter,
    AccessCaseKind::ArrayLength,
    AccessCaseKind::StringLength,
    AccessCaseKind::ModuleNamespaceLoad,
    AccessCaseKind::ProxyObject,
    AccessCaseKind::Miss,
    AccessCaseKind::Megamorphic,
];
const PROPERTY_STORE_CASES: &[AccessCaseKind] = &[
    AccessCaseKind::Replace,
    AccessCaseKind::Transition,
    AccessCaseKind::Setter,
    AccessCaseKind::ProxyObject,
    AccessCaseKind::Miss,
    AccessCaseKind::Megamorphic,
];
const ELEMENT_CASES: &[AccessCaseKind] = &[
    AccessCaseKind::IndexedLoad,
    AccessCaseKind::IndexedStore,
    AccessCaseKind::IndexedIn,
    AccessCaseKind::Miss,
    AccessCaseKind::Megamorphic,
];
const HAS_PROPERTY_CASES: &[AccessCaseKind] = &[
    AccessCaseKind::InHit,
    AccessCaseKind::InMiss,
    AccessCaseKind::InMegamorphic,
    AccessCaseKind::ProxyObjectIn,
    AccessCaseKind::IndexedMegamorphicIn,
    AccessCaseKind::IndexedInt32InHit,
    AccessCaseKind::IndexedDoubleInHit,
    AccessCaseKind::IndexedContiguousInHit,
    AccessCaseKind::IndexedArrayStorageInHit,
    AccessCaseKind::IndexedScopedArgumentsInHit,
    AccessCaseKind::IndexedDirectArgumentsInHit,
    AccessCaseKind::IndexedTypedArrayInt8In,
    AccessCaseKind::IndexedTypedArrayUint8In,
    AccessCaseKind::IndexedTypedArrayUint8ClampedIn,
    AccessCaseKind::IndexedTypedArrayInt16In,
    AccessCaseKind::IndexedTypedArrayUint16In,
    AccessCaseKind::IndexedTypedArrayInt32In,
    AccessCaseKind::IndexedTypedArrayUint32In,
    AccessCaseKind::IndexedTypedArrayFloat16In,
    AccessCaseKind::IndexedTypedArrayFloat32In,
    AccessCaseKind::IndexedTypedArrayFloat64In,
    AccessCaseKind::IndexedResizableTypedArrayInt8In,
    AccessCaseKind::IndexedResizableTypedArrayUint8In,
    AccessCaseKind::IndexedResizableTypedArrayUint8ClampedIn,
    AccessCaseKind::IndexedResizableTypedArrayInt16In,
    AccessCaseKind::IndexedResizableTypedArrayUint16In,
    AccessCaseKind::IndexedResizableTypedArrayInt32In,
    AccessCaseKind::IndexedResizableTypedArrayUint32In,
    AccessCaseKind::IndexedResizableTypedArrayFloat16In,
    AccessCaseKind::IndexedResizableTypedArrayFloat32In,
    AccessCaseKind::IndexedResizableTypedArrayFloat64In,
    AccessCaseKind::IndexedStringInHit,
    AccessCaseKind::IndexedNoIndexingInMiss,
    AccessCaseKind::IndexedProxyObjectIn,
];
const CALL_CASES: &[AccessCaseKind] = &[AccessCaseKind::Megamorphic];
const DEFAULT_STUB_KINDS: &[InlineCacheStubKind] = &[
    InlineCacheStubKind::SlowPathHandler,
    InlineCacheStubKind::DataOnlyHandler,
    InlineCacheStubKind::PolymorphicAccessStub,
    InlineCacheStubKind::RepatchingStub,
];
const CALL_STUB_KINDS: &[InlineCacheStubKind] = &[
    InlineCacheStubKind::SlowPathHandler,
    InlineCacheStubKind::HandlerWithCallLinkInfo,
    InlineCacheStubKind::SharedStatelessStub,
];

pub const STATIC_INLINE_CACHE_SCHEMAS: &[StaticInlineCacheSchema] = &[
    StaticInlineCacheSchema {
        kind: InlineCacheKind::PropertyLoad,
        name: "property-load",
        dispatch: InlineCacheDispatch::DataOnlyHandlerChain,
        allowed_cases: PROPERTY_LOAD_CASES,
        allowed_stub_kinds: DEFAULT_STUB_KINDS,
        may_call_js: true,
        owns_watchpoints: true,
        owner: InlineCacheSchemaOwner::BaselineJit,
        mutation_authority: InlineCacheRegistryMutationAuthority::GeneratedStaticDataRefresh,
        provenance: "static Rust IC schema table",
    },
    StaticInlineCacheSchema {
        kind: InlineCacheKind::PropertyStore,
        name: "property-store",
        dispatch: InlineCacheDispatch::RepatchingSlab,
        allowed_cases: PROPERTY_STORE_CASES,
        allowed_stub_kinds: DEFAULT_STUB_KINDS,
        may_call_js: true,
        owns_watchpoints: true,
        owner: InlineCacheSchemaOwner::BaselineJit,
        mutation_authority: InlineCacheRegistryMutationAuthority::GeneratedStaticDataRefresh,
        provenance: "static Rust IC schema table",
    },
    StaticInlineCacheSchema {
        kind: InlineCacheKind::ElementLoad,
        name: "element-load",
        dispatch: InlineCacheDispatch::DataOnlyHandlerChain,
        allowed_cases: ELEMENT_CASES,
        allowed_stub_kinds: DEFAULT_STUB_KINDS,
        may_call_js: false,
        owns_watchpoints: true,
        owner: InlineCacheSchemaOwner::BaselineJit,
        mutation_authority: InlineCacheRegistryMutationAuthority::GeneratedStaticDataRefresh,
        provenance: "static Rust IC schema table",
    },
    StaticInlineCacheSchema {
        kind: InlineCacheKind::ElementStore,
        name: "element-store",
        dispatch: InlineCacheDispatch::RepatchingSlab,
        allowed_cases: ELEMENT_CASES,
        allowed_stub_kinds: DEFAULT_STUB_KINDS,
        may_call_js: false,
        owns_watchpoints: true,
        owner: InlineCacheSchemaOwner::BaselineJit,
        mutation_authority: InlineCacheRegistryMutationAuthority::GeneratedStaticDataRefresh,
        provenance: "static Rust IC schema table",
    },
    StaticInlineCacheSchema {
        kind: InlineCacheKind::Call,
        name: "call-link",
        dispatch: InlineCacheDispatch::SharedStatelessStub,
        allowed_cases: CALL_CASES,
        allowed_stub_kinds: CALL_STUB_KINDS,
        may_call_js: true,
        owns_watchpoints: true,
        owner: InlineCacheSchemaOwner::InlineCacheRegistry,
        mutation_authority: InlineCacheRegistryMutationAuthority::GeneratedStaticDataRefresh,
        provenance: "static Rust IC schema table",
    },
    StaticInlineCacheSchema {
        kind: InlineCacheKind::Construct,
        name: "construct-link",
        dispatch: InlineCacheDispatch::SharedStatelessStub,
        allowed_cases: CALL_CASES,
        allowed_stub_kinds: CALL_STUB_KINDS,
        may_call_js: true,
        owns_watchpoints: true,
        owner: InlineCacheSchemaOwner::InlineCacheRegistry,
        mutation_authority: InlineCacheRegistryMutationAuthority::GeneratedStaticDataRefresh,
        provenance: "static Rust IC schema table",
    },
    StaticInlineCacheSchema {
        kind: InlineCacheKind::HasProperty,
        name: "has-property",
        dispatch: InlineCacheDispatch::RepatchingSlab,
        allowed_cases: HAS_PROPERTY_CASES,
        allowed_stub_kinds: DEFAULT_STUB_KINDS,
        may_call_js: true,
        owns_watchpoints: true,
        owner: InlineCacheSchemaOwner::BaselineJit,
        mutation_authority: InlineCacheRegistryMutationAuthority::GeneratedStaticDataRefresh,
        provenance: "static Rust IC schema table",
    },
];

pub const INLINE_CACHE_SCHEMA_REGISTRY: InlineCacheSchemaRegistry =
    InlineCacheSchemaRegistry::new(STATIC_INLINE_CACHE_SCHEMAS);

#[cfg(test)]
mod tests {
    use super::*;
    use crate::gc::{BarrierAction, CellId};
    use crate::jit::WatchpointTarget;
    use crate::strings::{AtomId, Identifier, PrivateName, PropertyIndex, PropertyKey, SymbolUid};
    use crate::value::EncodedJsValue;

    fn property_load_cold_miss_handoff(
        owner: CodeBlockId,
        slot: InlineCacheSlotId,
        bytecode_index: u32,
    ) -> InlineCacheMissHandoffDescriptor {
        InlineCacheMissHandoffDescriptor {
            slot,
            owner,
            bytecode_index,
            cache_kind: InlineCacheKind::PropertyLoad,
            miss_kind: InlineCacheMissKind::Cold,
            fallback: InlineCacheFallbackSemantics::SlowPathLookup,
            boundary: None,
            call_link: None,
            preserves_operand_registers: true,
        }
    }

    fn property_has_cold_miss_handoff(
        owner: CodeBlockId,
        slot: InlineCacheSlotId,
        bytecode_index: u32,
    ) -> InlineCacheMissHandoffDescriptor {
        InlineCacheMissHandoffDescriptor {
            slot,
            owner,
            bytecode_index,
            cache_kind: InlineCacheKind::HasProperty,
            miss_kind: InlineCacheMissKind::Cold,
            fallback: InlineCacheFallbackSemantics::SlowPathLookup,
            boundary: None,
            call_link: None,
            preserves_operand_registers: true,
        }
    }

    fn property_load_observation(
        observed_access_case_kind: Option<AccessCaseKind>,
        may_call_js: bool,
        cacheability: PropertyCacheability,
        base_structure: Option<StructureId>,
        offset: Option<PropertyOffset>,
        prototype_depth: u16,
    ) -> PropertyLoadObservationDescriptor {
        let owner = CodeBlockId(CellId(31));
        let slot = InlineCacheSlotId(41);
        let bytecode_index = 17;
        let base_object = ObjectId(CellId(51));
        let chain = base_structure
            .map(|structure| {
                vec![PropertyLoadObservationChainEntry {
                    object: base_object,
                    structure,
                    next_prototype: None,
                }]
            })
            .unwrap_or_default();
        let mut observation = PropertyLoadObservationDescriptor {
            owner,
            slot,
            bytecode_index,
            cache_kind: InlineCacheKind::PropertyLoad,
            fallback: InlineCacheFallbackSemantics::SlowPathLookup,
            key: CacheKey::Dynamic,
            base_object: Some(base_object),
            holder_object: None,
            base_normalization: PropertyLoadBaseNormalization::None,
            base_structure,
            offset,
            prototype_depth,
            observed_access_case_kind,
            may_call_js,
            cacheability,
            can_use_get_by_id_megamorphic: false,
            readiness: PropertyLoadObservationReadiness::ReadyForAttachment,
            cold_miss_handoff: property_load_cold_miss_handoff(owner, slot, bytecode_index),
            chain,
        };
        observation.readiness = observation.classify_readiness();
        observation
    }

    fn property_has_observation(
        observed_access_case_kind: Option<AccessCaseKind>,
        result: bool,
    ) -> PropertyHasObservationDescriptor {
        let owner = CodeBlockId(CellId(131));
        let slot = InlineCacheSlotId(141);
        let bytecode_index = 19;
        let base_object = ObjectId(CellId(151));
        let base_structure = StructureId(17);
        PropertyHasObservationDescriptor {
            owner,
            slot,
            bytecode_index,
            opcode: CoreOpcode::InById,
            cache_kind: InlineCacheKind::HasProperty,
            fallback: InlineCacheFallbackSemantics::SlowPathLookup,
            key: CacheKey::Property(PropertyKey::from_identifier(Identifier::from_atom(
                AtomId::from_table_slot(11),
            ))),
            base_object: Some(base_object),
            holder_object: Some(base_object),
            base_structure: Some(base_structure),
            offset: Some(PropertyOffset::new(3)),
            prototype_depth: 0,
            observed_access_case_kind,
            result,
            may_call_js: false,
            cacheability: PropertyCacheability::Allowed,
            can_use_in_by_id_megamorphic: true,
            cold_miss_handoff: property_has_cold_miss_handoff(owner, slot, bytecode_index),
            chain: vec![PropertyLoadObservationChainEntry {
                object: base_object,
                structure: base_structure,
                next_prototype: None,
            }],
        }
    }

    fn blockers(blockers: &[PropertyLoadObservationBlocker]) -> PropertyLoadObservationBlockers {
        let mut result = PropertyLoadObservationBlockers::empty();
        for blocker in blockers {
            result.insert(*blocker);
        }
        result
    }

    fn test_property_key() -> CacheKey {
        CacheKey::Property(PropertyKey::from_identifier(Identifier::from_atom(
            AtomId::from_table_slot(11),
        )))
    }

    fn ready_own_data_property_observation() -> PropertyLoadObservationDescriptor {
        let mut observation = property_load_observation(
            Some(AccessCaseKind::Load),
            false,
            PropertyCacheability::Allowed,
            Some(StructureId(7)),
            Some(PropertyOffset::new(3)),
            0,
        );
        observation.key = test_property_key();
        observation.holder_object = observation.base_object;
        observation.readiness = observation.classify_readiness();
        observation
    }

    fn ready_indexed_element_load_observation() -> PropertyLoadObservationDescriptor {
        let mut observation = property_load_observation(
            Some(AccessCaseKind::IndexedLoad),
            false,
            PropertyCacheability::Allowed,
            Some(StructureId(7)),
            None,
            0,
        );
        observation.cache_kind = InlineCacheKind::ElementLoad;
        observation.key = CacheKey::Property(PropertyKey::from_index(
            PropertyIndex::from_canonical_index(0),
        ));
        observation.holder_object = observation.base_object;
        observation.cold_miss_handoff.cache_kind = InlineCacheKind::ElementLoad;
        observation.readiness = observation.classify_readiness();
        observation
    }

    fn prototype_data_property_observation() -> PropertyLoadObservationDescriptor {
        let mut observation = property_load_observation(
            Some(AccessCaseKind::Load),
            false,
            PropertyCacheability::Allowed,
            Some(StructureId(7)),
            Some(PropertyOffset::new(3)),
            1,
        );
        observation.key = test_property_key();
        observation.holder_object = Some(ObjectId(CellId(52)));
        observation.chain = vec![
            PropertyLoadObservationChainEntry {
                object: observation.base_object.unwrap(),
                structure: observation.base_structure.unwrap(),
                next_prototype: observation.holder_object,
            },
            PropertyLoadObservationChainEntry {
                object: observation.holder_object.unwrap(),
                structure: StructureId(8),
                next_prototype: None,
            },
        ];
        observation.readiness = observation.classify_readiness();
        observation
    }

    fn own_missing_property_observation() -> PropertyLoadObservationDescriptor {
        let mut observation = property_load_observation(
            Some(AccessCaseKind::Miss),
            false,
            PropertyCacheability::Allowed,
            Some(StructureId(9)),
            None,
            0,
        );
        observation.key = test_property_key();
        observation.readiness = observation.classify_readiness();
        observation
    }

    fn prototype_negative_lookup_property_observation() -> PropertyLoadObservationDescriptor {
        let mut observation = own_missing_property_observation();
        let base = observation.base_object.unwrap();
        let first_prototype = ObjectId(CellId(52));
        let terminal = ObjectId(CellId(53));
        observation.prototype_depth = 2;
        observation.chain = vec![
            PropertyLoadObservationChainEntry {
                object: base,
                structure: observation.base_structure.unwrap(),
                next_prototype: Some(first_prototype),
            },
            PropertyLoadObservationChainEntry {
                object: first_prototype,
                structure: StructureId(10),
                next_prototype: Some(terminal),
            },
            PropertyLoadObservationChainEntry {
                object: terminal,
                structure: StructureId(11),
                next_prototype: None,
            },
        ];
        observation.readiness = observation.classify_readiness();
        observation
    }

    fn ready_own_data_property_plan() -> PropertyLoadAccessCasePlan {
        plan_property_load_access_case_from_observation(&ready_own_data_property_observation())
            .unwrap()
            .expect("ready own-data load plan")
    }

    fn ready_property_store_replace_plan() -> PropertyStoreAccessCasePlan {
        PropertyStoreAccessCasePlan {
            plan_kind: PropertyStoreAccessCasePlanKind::DataOnlyReplace,
            owner: CodeBlockId(CellId(131)),
            slot: InlineCacheSlotId(141),
            bytecode_index: 23,
            key: test_property_key(),
            access_case: AccessCaseDescriptor {
                kind: AccessCaseKind::Replace,
                key: test_property_key(),
                base_structure: Some(StructureId(151)),
                new_structure: None,
                holder: None,
                offset: Some(PropertyOffset::new(5)),
                via_global_proxy: false,
                may_call_js: false,
                dependencies: Vec::new(),
            },
            planned_stub_kind: InlineCacheStubKind::RepatchingStub,
            effect_contract: PropertyStoreAccessCasePlanContract::DATA_ONLY_REPLACE,
        }
    }

    fn ready_property_store_transition_plan() -> PropertyStoreAccessCasePlan {
        let mut plan = ready_property_store_replace_plan();
        plan.plan_kind = PropertyStoreAccessCasePlanKind::DataOnlyTransition;
        plan.bytecode_index = 24;
        plan.access_case.kind = AccessCaseKind::Transition;
        plan.access_case.new_structure = Some(StructureId(152));
        plan.effect_contract = PropertyStoreAccessCasePlanContract::DATA_ONLY_TRANSITION;
        plan
    }

    fn ready_property_store_indexed_plan() -> PropertyStoreAccessCasePlan {
        let mut plan = ready_property_store_replace_plan();
        plan.plan_kind = PropertyStoreAccessCasePlanKind::DataOnlyIndexedStore;
        plan.bytecode_index = 25;
        plan.key = CacheKey::Property(PropertyKey::from_index(
            PropertyIndex::from_canonical_index(0),
        ));
        plan.access_case.kind = AccessCaseKind::IndexedStore;
        plan.access_case.key = plan.key;
        plan.access_case.new_structure = None;
        plan.access_case.offset = None;
        plan.effect_contract = PropertyStoreAccessCasePlanContract::DATA_ONLY_INDEXED_STORE;
        plan
    }

    fn property_store_mutation_barrier_evidence(
        plan: &PropertyStoreAccessCasePlan,
    ) -> PropertyStoreMutationBarrierEvidence {
        PropertyStoreMutationBarrierEvidence {
            plan_kind: plan.plan_kind,
            effect_contract: plan.effect_contract,
            barrier_effect: plan.effect_contract.barrier,
            observed_write_barrier_count: 1,
            last_write_barrier: BarrierRequirementOutcome::Required(BarrierAction::MarkingBarrier),
        }
    }

    fn property_store_mutation_candidate(
        plan: PropertyStoreAccessCasePlan,
        store_plan_ordinal: u64,
    ) -> PropertyStoreMutationCandidate {
        let barrier_evidence = property_store_mutation_barrier_evidence(&plan);
        PropertyStoreMutationCandidate {
            plan,
            store_plan_ordinal,
            install_recheck_ordinal: store_plan_ordinal + 100,
            readiness_ordinal: store_plan_ordinal + 300,
            observation_ordinal: store_plan_ordinal + 200,
            barrier_evidence,
            stored_value_kind: ValueKind::Int32,
        }
    }

    fn prototype_data_guard_plan() -> PropertyLoadGuardPlan {
        plan_property_load_guard_plan_from_observation(&prototype_data_property_observation())
            .unwrap()
            .expect("prototype-data guard plan")
    }

    fn negative_lookup_guard_plan() -> PropertyLoadGuardPlan {
        plan_property_load_guard_plan_from_observation(
            &prototype_negative_lookup_property_observation(),
        )
        .unwrap()
        .expect("negative-lookup guard plan")
    }

    fn guarded_candidate(
        plan: PropertyLoadGuardPlan,
        candidate_kind: PropertyLoadGuardedCandidateKind,
        guard_plan_ordinal: u64,
    ) -> PropertyLoadGuardedCandidate {
        let chain_length = plan.descriptor.chain.entries.len();
        PropertyLoadGuardedCandidate {
            plan,
            guard_plan_ordinal,
            materialization_ordinal: guard_plan_ordinal + 100,
            dependency_ordinals: (0..chain_length)
                .map(|index| guard_plan_ordinal + 1_000 + index as u64)
                .collect(),
            binding_set_ids: (0..chain_length)
                .map(|index| WatchpointSetId(guard_plan_ordinal + 2_000 + index as u64))
                .collect(),
            candidate_kind,
        }
    }

    fn ready_call_link_attachment_plan(
        slot: InlineCacheSlotId,
        bytecode_index: u32,
        executable_cell: u32,
        callee_cell: u32,
        target_code_block_cell: u32,
    ) -> CallLinkAttachmentPlan {
        let owner = CodeBlockId(CellId(231));
        let boundary_id = CallBoundaryId(10_000 + u64::from(slot.0));
        let executable = ExecutableId(CellId(executable_cell));
        let callee = ObjectId(CellId(callee_cell));
        let target_code_block = CodeBlockId(CellId(target_code_block_cell));
        let max_argument_count_including_this = 3;
        let descriptor = CallLinkInfoDescriptor {
            mode: CallLinkMode::Init,
            call_kind: LinkedCallKind::Call,
            owner: Some(owner),
            executable: Some(executable),
            callee: Some(callee),
            target_code_block: Some(target_code_block),
            boundary: Some(boundary_id),
            slow_path_count: 0,
            max_argument_count_including_this,
        };

        CallLinkAttachmentPlan {
            owner,
            opcode: CoreOpcode::Call,
            slot,
            bytecode_index,
            descriptor,
            target: CallLinkAttachmentTargetDescriptor {
                executable,
                target_code_block,
                callee,
                specialization: CodeSpecialization::Call,
            },
            boundary: CallBoundaryMetadata {
                id: boundary_id,
                owner: Some(owner),
                abi: EntryAbi::LlIntCompatible,
                entry_kind: EntrypointKind::InterpreterThunk,
                native_symbol: None,
                arguments: vec![AbiValue::JsValue; usize::from(max_argument_count_including_this)],
                returns: vec![AbiValue::JsValue],
                registers: Vec::new(),
                frame_slots: Vec::new(),
                requires_vm_entry_scope: true,
                may_call_js: true,
                may_throw: true,
            },
            planned_stub_kind: InlineCacheStubKind::HandlerWithCallLinkInfo,
            remaining_blockers: CallLinkReadinessBlockers::from_blocker(
                CallLinkReadinessBlocker::DirectCallDisallowed,
            ),
            stub: InlineCacheStub {
                id: InlineCacheStubId(20_000 + u64::from(slot.0)),
                kind: InlineCacheStubKind::HandlerWithCallLinkInfo,
                owner_slot: slot,
                code: None,
                tier: JitType::Baseline,
                cases: Vec::new(),
                weak_structures: Vec::new(),
                barrier_metadata: Vec::new(),
                call_links: vec![descriptor],
                invalidation_strength: DependencyStrength::WeakGc,
            },
        }
    }

    fn generated_call_link_candidate_from_plan(
        mut plan: CallLinkAttachmentPlan,
        attachment_ordinal: u64,
    ) -> GeneratedCallLinkCandidate {
        plan.descriptor.mode = CallLinkMode::Monomorphic;
        GeneratedCallLinkCandidate {
            owner: plan.owner,
            opcode: plan.opcode,
            slot: plan.slot,
            bytecode_index: plan.bytecode_index,
            descriptor: plan.descriptor,
            target: plan.target,
            boundary: plan.boundary,
            attachment_ordinal,
            attachment_plan_ordinal: attachment_ordinal + 100,
            install_recheck_ordinal: attachment_ordinal + 200,
            boundary_validation_ordinal: Some(attachment_ordinal + 300),
            descriptor_ordinal: Some(attachment_ordinal + 400),
            observation_ordinal: Some(attachment_ordinal + 500),
            readiness_ordinal: Some(attachment_ordinal + 600),
            remaining_blockers: plan.remaining_blockers,
            direct_call_status: GeneratedCallLinkDirectCallStatus::Disallowed,
        }
    }

    fn ready_generated_call_link_candidate(
        slot: InlineCacheSlotId,
        bytecode_index: u32,
        executable_cell: u32,
        callee_cell: u32,
        target_code_block_cell: u32,
        attachment_ordinal: u64,
    ) -> GeneratedCallLinkCandidate {
        generated_call_link_candidate_from_plan(
            ready_call_link_attachment_plan(
                slot,
                bytecode_index,
                executable_cell,
                callee_cell,
                target_code_block_cell,
            ),
            attachment_ordinal,
        )
    }

    fn vm_authorized_generated_call_link_candidate(
        mut candidate: GeneratedCallLinkCandidate,
        ordinal_base: u64,
    ) -> GeneratedCallLinkCandidate {
        candidate.observation_ordinal = Some(ordinal_base + 1);
        candidate.descriptor_ordinal = Some(ordinal_base + 2);
        candidate.readiness_ordinal = Some(ordinal_base + 3);
        candidate.boundary_validation_ordinal = Some(ordinal_base + 4);
        candidate.attachment_plan_ordinal = ordinal_base + 5;
        candidate.install_recheck_ordinal = ordinal_base + 6;
        candidate.attachment_ordinal = ordinal_base + 7;
        candidate
            .remaining_blockers
            .remove(CallLinkReadinessBlocker::DirectCallDisallowed);
        candidate.direct_call_status = GeneratedCallLinkDirectCallStatus::Authorized;
        candidate
    }

    fn generated_call_link_cell_value(payload: u64) -> RuntimeValue {
        RuntimeValue::from_encoded(EncodedJsValue((payload << 8) | 0x20))
    }

    fn generated_call_link_probe_request(
        candidate: &GeneratedCallLinkCandidate,
    ) -> GeneratedCallLinkProbeRequest<'_> {
        GeneratedCallLinkProbeRequest::new(
            candidate,
            candidate.owner,
            candidate.opcode,
            candidate.bytecode_index,
            u32::from(candidate.descriptor.max_argument_count_including_this),
            generated_call_link_cell_value(0x44_000 + u64::from(candidate.slot.0)),
            ValueKind::Cell,
            Some(candidate.target.callee),
            RuntimeValue::undefined(),
            ValueKind::Undefined,
            None,
        )
    }

    fn assert_plan_table_error(
        plan: PropertyLoadAccessCasePlan,
        expected: InlineCacheValidationError,
    ) {
        assert_eq!(
            PropertyLoadAccessCasePlanTable::new(plan.owner, vec![plan]),
            Err(expected)
        );
    }

    fn assert_store_plan_table_error(
        plan: PropertyStoreAccessCasePlan,
        expected: InlineCacheValidationError,
    ) {
        assert_eq!(
            PropertyStoreAccessCasePlanTable::new(plan.owner, vec![plan]),
            Err(expected)
        );
    }

    fn assert_call_link_plan_table_error(plan: CallLinkAttachmentPlan) {
        assert_eq!(
            CallLinkAttachmentPlanTable::new(plan.owner, vec![plan]),
            Err(InlineCacheValidationError::CallLinkMismatch)
        );
    }

    fn assert_generated_call_link_candidate_table_error(candidate: GeneratedCallLinkCandidate) {
        assert_eq!(
            GeneratedCallLinkCandidateTable::new(candidate.owner, vec![candidate]),
            Err(InlineCacheValidationError::CallLinkMismatch)
        );
    }

    #[test]
    fn static_inline_cache_registry_validates() {
        assert_eq!(INLINE_CACHE_SCHEMA_REGISTRY.validate(), Ok(()));
    }

    #[test]
    fn has_property_observation_descriptor_validates_boolean_cases() {
        assert_eq!(
            property_has_observation(Some(AccessCaseKind::InHit), true).validate(),
            Ok(())
        );
        let mut missing = property_has_observation(Some(AccessCaseKind::InMiss), false);
        missing.offset = None;
        assert_eq!(missing.validate(), Ok(()));

        let mut indexed =
            property_has_observation(Some(AccessCaseKind::IndexedArrayStorageInHit), true);
        indexed.opcode = CoreOpcode::InByVal;
        indexed.key = CacheKey::Property(PropertyKey::from_index(
            PropertyIndex::from_canonical_index(0),
        ));
        indexed.offset = None;
        indexed.cacheability = PropertyCacheability::Disallowed;
        indexed.can_use_in_by_id_megamorphic = false;
        assert_eq!(indexed.validate(), Ok(()));

        let mut typed_array_false =
            property_has_observation(Some(AccessCaseKind::IndexedTypedArrayUint8In), false);
        typed_array_false.opcode = CoreOpcode::InByVal;
        typed_array_false.key = CacheKey::Property(PropertyKey::from_index(
            PropertyIndex::from_canonical_index(9),
        ));
        typed_array_false.offset = None;
        typed_array_false.cacheability = PropertyCacheability::Disallowed;
        typed_array_false.can_use_in_by_id_megamorphic = false;
        assert_eq!(typed_array_false.validate(), Ok(()));
    }

    #[test]
    fn has_property_observation_rejects_load_miss_and_result_mismatch() {
        assert_eq!(
            property_has_observation(Some(AccessCaseKind::Miss), false).validate(),
            Err(InlineCacheValidationError::CaseNotAllowed(
                AccessCaseKind::Miss
            ))
        );
        assert_eq!(
            property_has_observation(Some(AccessCaseKind::IndexedIn), true).validate(),
            Err(InlineCacheValidationError::CaseNotAllowed(
                AccessCaseKind::IndexedIn
            ))
        );
        assert_eq!(
            property_has_observation(Some(AccessCaseKind::ProxyObject), true).validate(),
            Err(InlineCacheValidationError::CaseNotAllowed(
                AccessCaseKind::ProxyObject
            ))
        );
        assert_eq!(
            property_has_observation(Some(AccessCaseKind::InHit), false).validate(),
            Err(
                InlineCacheValidationError::PropertyHasObservationResultMismatch {
                    result: false,
                    access_case_kind: AccessCaseKind::InHit
                }
            )
        );

        let mut dynamic = property_has_observation(None, true);
        dynamic.key = CacheKey::Dynamic;
        assert_eq!(
            dynamic.validate(),
            Err(InlineCacheValidationError::PropertyHasPlanUnsupportedKey(
                CacheKey::Dynamic
            ))
        );
        dynamic.cacheability = PropertyCacheability::Disallowed;
        dynamic.can_use_in_by_id_megamorphic = false;
        assert_eq!(dynamic.validate(), Ok(()));
    }

    #[test]
    fn slot_builder_rejects_active_slot_without_cases() {
        let slot = InlineCacheSlot::builder(InlineCacheSlotId(7), InlineCacheKind::PropertyLoad)
            .state(InlineCacheState::Monomorphic)
            .build();

        assert_eq!(slot, Err(InlineCacheValidationError::StateCaseMismatch));
    }

    #[test]
    fn inline_cache_classification_counts_polymorphic_cases() {
        let first = AccessCaseDescriptor {
            kind: AccessCaseKind::Load,
            key: CacheKey::Dynamic,
            base_structure: None,
            new_structure: None,
            holder: None,
            offset: None,
            via_global_proxy: false,
            may_call_js: false,
            dependencies: Vec::new(),
        };
        let second = AccessCaseDescriptor {
            kind: AccessCaseKind::ArrayLength,
            key: CacheKey::Dynamic,
            base_structure: None,
            new_structure: None,
            holder: None,
            offset: None,
            via_global_proxy: false,
            may_call_js: false,
            dependencies: Vec::new(),
        };
        let slot = InlineCacheSlot::builder(InlineCacheSlotId(9), InlineCacheKind::PropertyLoad)
            .state(InlineCacheState::Polymorphic)
            .case(first)
            .case(second)
            .build()
            .unwrap();

        assert_eq!(
            classify_inline_cache_slot(&slot),
            Ok(InlineCacheCaseClassification::Polymorphic)
        );
    }

    #[test]
    fn inline_cache_semantics_marks_getter_as_calling_read() {
        let getter = AccessCaseDescriptor {
            kind: AccessCaseKind::Getter,
            key: CacheKey::Dynamic,
            base_structure: None,
            new_structure: None,
            holder: None,
            offset: None,
            via_global_proxy: false,
            may_call_js: true,
            dependencies: Vec::new(),
        };
        let slot = InlineCacheSlot::builder(InlineCacheSlotId(11), InlineCacheKind::PropertyLoad)
            .state(InlineCacheState::Monomorphic)
            .case(getter)
            .build()
            .unwrap();

        let summary = classify_inline_cache_semantics(&slot).unwrap();

        assert_eq!(summary.class, InlineCacheSemanticClass::PropertyRead);
        assert!(summary.effects.reads_heap);
        assert!(summary.effects.may_call_js);
        assert!(summary.observes_prototype_chain);
    }

    #[test]
    fn inline_cache_miss_handoff_records_lookup_fallback() {
        let owner = CodeBlockId(CellId(21));
        let slot = InlineCacheSlot::builder(InlineCacheSlotId(12), InlineCacheKind::PropertyLoad)
            .owner(owner)
            .bytecode_index(24)
            .state(InlineCacheState::ColdSlowPath)
            .build()
            .unwrap();

        let handoff =
            InlineCacheMissHandoffDescriptor::from_slot(&slot, InlineCacheMissKind::Cold, None)
                .unwrap();

        assert_eq!(handoff.owner, owner);
        assert_eq!(
            handoff.fallback,
            InlineCacheFallbackSemantics::SlowPathLookup
        );
        assert!(handoff.preserves_operand_registers);
    }

    #[test]
    fn call_cache_miss_handoff_requires_call_boundary() {
        let owner = CodeBlockId(CellId(22));
        let slot = InlineCacheSlot::builder(InlineCacheSlotId(13), InlineCacheKind::Call)
            .owner(owner)
            .bytecode_index(28)
            .build()
            .unwrap();

        assert_eq!(
            InlineCacheMissHandoffDescriptor::from_slot(&slot, InlineCacheMissKind::CaseMiss, None),
            Err(InlineCacheValidationError::MissingHandoffCallBoundary)
        );
    }

    #[test]
    fn call_link_attachment_plan_table_accepts_metadata_only_plans_and_filters_candidates() {
        let first = ready_call_link_attachment_plan(InlineCacheSlotId(161), 45, 171, 181, 191);
        let same_bytecode =
            ready_call_link_attachment_plan(InlineCacheSlotId(162), 45, 172, 182, 192);
        let other_bytecode =
            ready_call_link_attachment_plan(InlineCacheSlotId(163), 46, 173, 183, 193);

        let table = CallLinkAttachmentPlanTable::new(
            first.owner,
            vec![first.clone(), same_bytecode.clone(), other_bytecode.clone()],
        )
        .expect("valid call-link attachment plan table");

        assert_eq!(table.owner(), first.owner);
        assert_eq!(table.len(), 3);
        assert!(!table.is_empty());
        assert_eq!(
            table.plans(),
            &[first.clone(), same_bytecode.clone(), other_bytecode.clone()]
        );
        assert_eq!(
            first.planned_stub_kind,
            InlineCacheStubKind::HandlerWithCallLinkInfo
        );
        assert!(first.stub.code.is_none());
        assert!(first.stub.cases.is_empty());
        assert!(first.stub.weak_structures.is_empty());
        assert!(first.stub.barrier_metadata.is_empty());
        assert!(first
            .remaining_blockers
            .contains(CallLinkReadinessBlocker::DirectCallDisallowed));
        assert_eq!(
            table.candidates_for_bytecode_index(45).collect::<Vec<_>>(),
            vec![&first, &same_bytecode]
        );
        assert_eq!(
            table.candidates_for_bytecode_index(46).collect::<Vec<_>>(),
            vec![&other_bytecode]
        );
        assert!(table.candidates_for_bytecode_index(1234).next().is_none());
    }

    #[test]
    fn call_link_attachment_plan_table_accepts_authorized_blocker_shape() {
        let mut plan = ready_call_link_attachment_plan(InlineCacheSlotId(164), 47, 174, 184, 194);
        plan.remaining_blockers
            .remove(CallLinkReadinessBlocker::DirectCallDisallowed);

        let table = CallLinkAttachmentPlanTable::new(plan.owner, vec![plan.clone()])
            .expect("authorized call-link attachment plan blocker shape");
        assert_eq!(table.plans(), &[plan]);
    }

    #[test]
    fn call_link_attachment_plan_table_rejects_code_and_materialized_stub_state() {
        let mut code_bearing =
            ready_call_link_attachment_plan(InlineCacheSlotId(165), 48, 175, 185, 195);
        code_bearing.stub.code = Some(JitCodeId(1));

        let mut with_case =
            ready_call_link_attachment_plan(InlineCacheSlotId(166), 49, 176, 186, 196);
        with_case.stub.cases.push(AccessCaseDescriptor {
            kind: AccessCaseKind::Megamorphic,
            key: CacheKey::Dynamic,
            base_structure: None,
            new_structure: None,
            holder: None,
            offset: None,
            via_global_proxy: false,
            may_call_js: false,
            dependencies: Vec::new(),
        });

        let mut with_barrier =
            ready_call_link_attachment_plan(InlineCacheSlotId(167), 50, 177, 187, 197);
        with_barrier
            .stub
            .barrier_metadata
            .push(InlineCacheBarrierMetadata::store_value(
                InlineCacheBarrierTarget::StoredValue,
            ));

        let mut with_weak_structure =
            ready_call_link_attachment_plan(InlineCacheSlotId(168), 51, 178, 188, 198);
        with_weak_structure
            .stub
            .weak_structures
            .push(StructureId(99));

        for plan in [code_bearing, with_case, with_barrier, with_weak_structure] {
            assert_call_link_plan_table_error(plan);
        }
    }

    #[test]
    fn call_link_attachment_plan_table_rejects_duplicate_keys() {
        let plan = ready_call_link_attachment_plan(InlineCacheSlotId(169), 52, 179, 189, 199);

        assert_eq!(
            CallLinkAttachmentPlanTable::new(plan.owner, vec![plan.clone(), plan.clone()]),
            Err(InlineCacheValidationError::CallLinkMismatch)
        );
    }

    #[test]
    fn generated_call_link_candidate_table_accepts_active_metadata_and_filters_lookup() {
        let first =
            ready_generated_call_link_candidate(InlineCacheSlotId(170), 53, 180, 190, 200, 1);
        let same_bytecode =
            ready_generated_call_link_candidate(InlineCacheSlotId(171), 53, 181, 191, 201, 2);
        let mut call_with_this =
            ready_generated_call_link_candidate(InlineCacheSlotId(172), 54, 182, 192, 202, 3);
        call_with_this.opcode = CoreOpcode::CallWithThis;

        let table = GeneratedCallLinkCandidateTable::new(
            first.owner,
            vec![first.clone(), same_bytecode.clone(), call_with_this.clone()],
        )
        .expect("valid generated call-link candidate table");

        assert_eq!(table.owner(), first.owner);
        assert_eq!(table.len(), 3);
        assert!(!table.is_empty());
        assert_eq!(
            table.candidates(),
            &[first.clone(), same_bytecode.clone(), call_with_this.clone()]
        );
        assert_eq!(first.descriptor.mode, CallLinkMode::Monomorphic);
        assert_eq!(first.descriptor.call_kind, LinkedCallKind::Call);
        assert_eq!(first.target.specialization, CodeSpecialization::Call);
        assert_eq!(
            first.direct_call_status,
            GeneratedCallLinkDirectCallStatus::Disallowed
        );
        assert!(first
            .remaining_blockers
            .contains(CallLinkReadinessBlocker::DirectCallDisallowed));
        assert_eq!(
            table.candidates_for_bytecode_index(53).collect::<Vec<_>>(),
            vec![&first, &same_bytecode]
        );
        assert_eq!(
            table.candidates_for_bytecode_index(54).collect::<Vec<_>>(),
            vec![&call_with_this]
        );
        assert!(table.candidates_for_bytecode_index(1234).next().is_none());
    }

    #[test]
    fn generated_call_link_candidate_table_accepts_vm_authorized_direct_call_candidate() {
        let candidate = vm_authorized_generated_call_link_candidate(
            ready_generated_call_link_candidate(InlineCacheSlotId(173), 55, 183, 193, 203, 1),
            10,
        );

        let table = GeneratedCallLinkCandidateTable::new(candidate.owner, vec![candidate.clone()])
            .expect("VM-authorized generated call-link candidate table");

        assert_eq!(table.candidates(), std::slice::from_ref(&candidate));
        assert_eq!(
            candidate.direct_call_status,
            GeneratedCallLinkDirectCallStatus::Authorized
        );
        assert!(!candidate
            .remaining_blockers
            .contains(CallLinkReadinessBlocker::DirectCallDisallowed));
    }

    #[test]
    fn generated_call_link_candidate_table_rejects_forged_authorized_candidate_ordinals() {
        let mut forged =
            ready_generated_call_link_candidate(InlineCacheSlotId(174), 56, 184, 194, 204, 1);
        forged
            .remaining_blockers
            .remove(CallLinkReadinessBlocker::DirectCallDisallowed);
        forged.direct_call_status = GeneratedCallLinkDirectCallStatus::Authorized;

        assert_generated_call_link_candidate_table_error(forged);

        let mut missing = vm_authorized_generated_call_link_candidate(
            ready_generated_call_link_candidate(InlineCacheSlotId(175), 57, 185, 195, 205, 1),
            20,
        );
        missing.readiness_ordinal = None;

        assert_generated_call_link_candidate_table_error(missing);
    }

    #[test]
    fn generated_call_link_candidate_table_rejects_duplicate_semantic_candidates() {
        let first =
            ready_generated_call_link_candidate(InlineCacheSlotId(176), 58, 186, 196, 206, 1);
        let second =
            ready_generated_call_link_candidate(InlineCacheSlotId(176), 58, 186, 196, 206, 2);

        assert_eq!(
            GeneratedCallLinkCandidateTable::new(first.owner, vec![first, second]),
            Err(InlineCacheValidationError::CallLinkMismatch)
        );
    }

    #[test]
    fn generated_call_link_candidate_table_rejects_owner_opcode_mode_and_boundary_mismatch() {
        let owner_mismatch =
            ready_generated_call_link_candidate(InlineCacheSlotId(177), 59, 187, 197, 207, 1);
        assert_eq!(
            GeneratedCallLinkCandidateTable::new(CodeBlockId(CellId(999)), vec![owner_mismatch]),
            Err(InlineCacheValidationError::CallLinkMismatch)
        );

        let mut wrong_opcode =
            ready_generated_call_link_candidate(InlineCacheSlotId(178), 60, 188, 198, 208, 2);
        wrong_opcode.opcode = CoreOpcode::GetByName;

        let mut wrong_mode =
            ready_generated_call_link_candidate(InlineCacheSlotId(179), 61, 189, 199, 209, 3);
        wrong_mode.descriptor.mode = CallLinkMode::Init;

        let mut missing_boundary =
            ready_generated_call_link_candidate(InlineCacheSlotId(180), 62, 190, 200, 210, 4);
        missing_boundary.descriptor.boundary = None;

        let mut non_llint_boundary =
            ready_generated_call_link_candidate(InlineCacheSlotId(181), 63, 191, 201, 211, 5);
        non_llint_boundary.boundary.abi = EntryAbi::Rust;

        for candidate in [
            wrong_opcode,
            wrong_mode,
            missing_boundary,
            non_llint_boundary,
        ] {
            assert_generated_call_link_candidate_table_error(candidate);
        }
    }

    #[test]
    fn generated_call_link_candidate_table_rejects_direct_dispatch_and_native_boundary_shapes() {
        let mut direct_status =
            ready_generated_call_link_candidate(InlineCacheSlotId(182), 64, 192, 202, 212, 1);
        direct_status.direct_call_status = GeneratedCallLinkDirectCallStatus::Authorized;

        let mut direct_mode =
            ready_generated_call_link_candidate(InlineCacheSlotId(183), 65, 193, 203, 213, 2);
        direct_mode.descriptor.mode = CallLinkMode::Direct;

        let mut generated_entry =
            ready_generated_call_link_candidate(InlineCacheSlotId(184), 66, 194, 204, 214, 3);
        generated_entry.boundary.entry_kind = EntrypointKind::GeneratedCode;

        let mut native_symbol =
            ready_generated_call_link_candidate(InlineCacheSlotId(185), 67, 195, 205, 215, 4);
        native_symbol.boundary.native_symbol = Some(crate::runtime::NativeCodeId(7));

        for candidate in [direct_status, direct_mode, generated_entry, native_symbol] {
            assert_generated_call_link_candidate_table_error(candidate);
        }
    }

    #[test]
    fn generated_call_link_probe_request_preserves_metadata_only_call_site_facts() {
        let candidate =
            ready_generated_call_link_candidate(InlineCacheSlotId(183), 65, 193, 203, 213, 1);
        let request = generated_call_link_probe_request(&candidate);

        assert_eq!(request.candidate, &candidate);
        assert_eq!(request.owner, candidate.owner);
        assert_eq!(request.opcode, CoreOpcode::Call);
        assert_eq!(request.bytecode_index, candidate.bytecode_index);
        assert_eq!(
            request.argument_count_including_this,
            u32::from(candidate.descriptor.max_argument_count_including_this)
        );
        assert_eq!(
            request.callee_value,
            generated_call_link_cell_value(0x44_000 + u64::from(candidate.slot.0))
        );
        assert_eq!(request.callee_value.kind(), ValueKind::Cell);
        assert_eq!(request.callee_value_kind, ValueKind::Cell);
        assert_eq!(request.callee_object, Some(candidate.target.callee));
        assert_eq!(request.this_value, RuntimeValue::undefined());
        assert_eq!(request.this_value.kind(), ValueKind::Undefined);
        assert_eq!(request.this_value_kind, ValueKind::Undefined);
        assert_eq!(request.this_object, None);
        assert_eq!(request.expected_callee(), Some(candidate.target.callee));
        assert_eq!(
            request.expected_argument_count_including_this(),
            candidate.descriptor.max_argument_count_including_this
        );
        assert_eq!(request.expected_boundary(), Some(candidate.boundary.id));
        assert!(request.callee_is_cell());
        assert!(!request.this_is_cell());

        let ObjectId(CellId(callee_identity_bits)) = candidate.target.callee;
        assert_ne!(
            request.callee_value.encoded(),
            EncodedJsValue((u64::from(callee_identity_bits) << 8) | 0x20)
        );
    }

    #[test]
    fn generated_call_link_probe_matching_candidate_blocks_direct_dispatch() {
        let candidate =
            ready_generated_call_link_candidate(InlineCacheSlotId(184), 66, 194, 204, 214, 1);
        let request = generated_call_link_probe_request(&candidate);

        let result = GeneratedCallLinkProbeResult::for_request(&request);

        assert_eq!(
            result,
            GeneratedCallLinkProbeResult::Blocked(GeneratedCallLinkProbeBlock {
                reason: GeneratedCallLinkProbeMissReason::DirectCallDisallowed
            })
        );
        assert!(!result.authorizes_direct_call());
        assert_eq!(
            GeneratedCallLinkProbeResult::blocked(
                GeneratedCallLinkProbeMissReason::BoundaryRequiresInterpreter
            ),
            GeneratedCallLinkProbeResult::Blocked(GeneratedCallLinkProbeBlock {
                reason: GeneratedCallLinkProbeMissReason::BoundaryRequiresInterpreter
            })
        );
    }

    #[test]
    fn generated_call_link_probe_matching_vm_authorized_candidate_returns_direct_call() {
        let candidate = vm_authorized_generated_call_link_candidate(
            ready_generated_call_link_candidate(InlineCacheSlotId(185), 67, 195, 205, 215, 1),
            20,
        );
        let request = generated_call_link_probe_request(&candidate);

        let result = GeneratedCallLinkProbeResult::for_request(&request);

        assert_eq!(
            result,
            GeneratedCallLinkProbeResult::DirectCall(GeneratedCallLinkDirectCall {
                slot: candidate.slot,
                attachment_ordinal: candidate.attachment_ordinal,
                attachment_plan_ordinal: candidate.attachment_plan_ordinal,
                install_recheck_ordinal: candidate.install_recheck_ordinal,
                boundary_validation_ordinal: candidate.boundary_validation_ordinal,
                descriptor_ordinal: candidate.descriptor_ordinal,
                observation_ordinal: candidate.observation_ordinal,
                readiness_ordinal: candidate.readiness_ordinal,
                target_executable: candidate.target.executable,
                target_callee: candidate.target.callee,
                target_code_block: candidate.target.target_code_block,
                target_boundary: candidate.boundary.id,
            })
        );
        assert!(result.authorizes_direct_call());
    }

    #[test]
    fn generated_call_link_probe_mismatches_report_bounded_miss_reasons() {
        let candidate =
            ready_generated_call_link_candidate(InlineCacheSlotId(186), 68, 196, 206, 216, 1);

        macro_rules! assert_probe_miss {
            ($mutate:expr, $reason:expr) => {{
                let mut request = generated_call_link_probe_request(&candidate);
                $mutate(&mut request);
                assert_eq!(
                    GeneratedCallLinkProbeResult::for_request(&request),
                    GeneratedCallLinkProbeResult::miss($reason)
                );
            }};
        }

        assert_probe_miss!(
            |request: &mut GeneratedCallLinkProbeRequest<'_>| {
                request.owner = CodeBlockId(CellId(999));
            },
            GeneratedCallLinkProbeMissReason::OwnerMismatch
        );
        assert_probe_miss!(
            |request: &mut GeneratedCallLinkProbeRequest<'_>| {
                request.opcode = CoreOpcode::CallWithThis;
            },
            GeneratedCallLinkProbeMissReason::OpcodeMismatch
        );
        assert_probe_miss!(
            |request: &mut GeneratedCallLinkProbeRequest<'_>| {
                request.bytecode_index += 1;
            },
            GeneratedCallLinkProbeMissReason::BytecodeIndexMismatch
        );
        assert_probe_miss!(
            |request: &mut GeneratedCallLinkProbeRequest<'_>| {
                request.argument_count_including_this += 1;
            },
            GeneratedCallLinkProbeMissReason::ArgumentCountMismatch
        );
        assert_probe_miss!(
            |request: &mut GeneratedCallLinkProbeRequest<'_>| {
                request.callee_value = RuntimeValue::from_i32(13);
                request.callee_value_kind = ValueKind::Int32;
                request.callee_object = None;
            },
            GeneratedCallLinkProbeMissReason::NonCellCalleeIdentity
        );
        assert_probe_miss!(
            |request: &mut GeneratedCallLinkProbeRequest<'_>| {
                request.callee_object = None;
            },
            GeneratedCallLinkProbeMissReason::MissingCalleeIdentity
        );
        assert_probe_miss!(
            |request: &mut GeneratedCallLinkProbeRequest<'_>| {
                request.callee_object = Some(ObjectId(CellId(999)));
            },
            GeneratedCallLinkProbeMissReason::CalleeMismatch
        );
    }

    #[test]
    fn generated_call_link_probe_miss_result_records_safe_blockers_without_host_or_vm() {
        for reason in [
            GeneratedCallLinkProbeMissReason::HostUnavailable,
            GeneratedCallLinkProbeMissReason::OwnerMismatch,
            GeneratedCallLinkProbeMissReason::OpcodeMismatch,
            GeneratedCallLinkProbeMissReason::BytecodeIndexMismatch,
            GeneratedCallLinkProbeMissReason::ArgumentCountMismatch,
            GeneratedCallLinkProbeMissReason::MissingCalleeIdentity,
            GeneratedCallLinkProbeMissReason::NonCellCalleeIdentity,
            GeneratedCallLinkProbeMissReason::CalleeMismatch,
            GeneratedCallLinkProbeMissReason::CandidateNotFound,
            GeneratedCallLinkProbeMissReason::UnsupportedCandidateMetadata,
        ] {
            assert_eq!(
                GeneratedCallLinkProbeResult::miss(reason),
                GeneratedCallLinkProbeResult::Miss(GeneratedCallLinkProbeMiss { reason })
            );
            assert!(!GeneratedCallLinkProbeResult::miss(reason).authorizes_direct_call());
            assert!(!reason.is_metadata_blocker());
        }

        for reason in [
            GeneratedCallLinkProbeMissReason::DirectCallDisallowed,
            GeneratedCallLinkProbeMissReason::BoundaryRequiresInterpreter,
        ] {
            assert_eq!(
                GeneratedCallLinkProbeResult::blocked(reason),
                GeneratedCallLinkProbeResult::Blocked(GeneratedCallLinkProbeBlock { reason })
            );
            assert!(reason.is_metadata_blocker());
            assert!(!GeneratedCallLinkProbeResult::blocked(reason).authorizes_direct_call());
        }
    }

    #[test]
    fn generated_call_link_probe_rejects_unsupported_candidate_metadata_without_authority() {
        let mut forged =
            ready_generated_call_link_candidate(InlineCacheSlotId(187), 69, 197, 207, 217, 1);
        forged
            .remaining_blockers
            .remove(CallLinkReadinessBlocker::DirectCallDisallowed);
        forged.direct_call_status = GeneratedCallLinkDirectCallStatus::Authorized;
        let request = generated_call_link_probe_request(&forged);
        let result = GeneratedCallLinkProbeResult::for_request(&request);

        assert_eq!(
            result,
            GeneratedCallLinkProbeResult::miss(
                GeneratedCallLinkProbeMissReason::UnsupportedCandidateMetadata
            )
        );
        assert!(!result.authorizes_direct_call());

        let source = include_str!("ic.rs");
        for forbidden in [
            concat!("GeneratedCallLink", "ProbeHit"),
            concat!("GeneratedCallLinkProbeResult::", "Hit"),
            concat!("GeneratedCallLinkProbeResult::", "Authorized"),
            concat!("GeneratedCallLinkProbeResult::", "DirectCallAuthorized"),
        ] {
            assert!(
                !source.contains(forbidden),
                "unexpected generated call-link probe authorization API found: {forbidden}"
            );
        }
    }

    #[test]
    fn property_store_cache_requires_barrier_metadata() {
        let store_case = AccessCaseDescriptor {
            kind: AccessCaseKind::Replace,
            key: CacheKey::Dynamic,
            base_structure: Some(StructureId(1)),
            new_structure: None,
            holder: None,
            offset: None,
            via_global_proxy: false,
            may_call_js: false,
            dependencies: Vec::new(),
        };

        let missing =
            InlineCacheSlot::builder(InlineCacheSlotId(14), InlineCacheKind::PropertyStore)
                .state(InlineCacheState::Monomorphic)
                .case(store_case.clone())
                .build();
        assert_eq!(
            missing,
            Err(InlineCacheValidationError::BarrierMetadataMissing)
        );

        let slot = InlineCacheSlot::builder(InlineCacheSlotId(15), InlineCacheKind::PropertyStore)
            .state(InlineCacheState::Monomorphic)
            .case(store_case)
            .barrier_metadata(InlineCacheBarrierMetadata::store_value(
                InlineCacheBarrierTarget::StoredValue,
            ))
            .build()
            .unwrap();

        assert_eq!(slot.barrier_metadata.len(), 1);
    }

    #[test]
    fn property_store_access_case_plan_table_accepts_replace_and_transition_metadata() {
        let replace = ready_property_store_replace_plan();
        let transition = ready_property_store_transition_plan();

        let table = PropertyStoreAccessCasePlanTable::new(
            replace.owner,
            vec![replace.clone(), transition.clone()],
        )
        .expect("valid property-store plan table");

        assert_eq!(table.owner(), replace.owner);
        assert_eq!(table.len(), 2);
        assert!(!table.is_empty());
        assert_eq!(table.plans(), &[replace.clone(), transition.clone()]);
        assert_eq!(
            table
                .candidates_for_bytecode_index(replace.bytecode_index)
                .collect::<Vec<_>>(),
            vec![&replace]
        );
        assert_eq!(
            table
                .candidates_for_bytecode_index(transition.bytecode_index)
                .collect::<Vec<_>>(),
            vec![&transition]
        );

        assert_eq!(
            replace.effect_contract,
            PropertyStoreAccessCasePlanContract::DATA_ONLY_REPLACE
        );
        assert!(replace
            .effect_contract
            .supports_metadata_only_replace_plan());
        assert_eq!(
            replace.effect_contract.barrier,
            PropertyStoreBarrierEffect::RequiresRuntimeStoredValueBarrierProof
        );
        assert_eq!(
            transition.effect_contract,
            PropertyStoreAccessCasePlanContract::DATA_ONLY_TRANSITION
        );
        assert!(transition
            .effect_contract
            .supports_metadata_only_transition_plan());
        assert_eq!(
            transition.effect_contract.barrier,
            PropertyStoreBarrierEffect::RequiresRuntimeStoredValueAndStructureTransitionBarrierProof
        );
        assert_eq!(
            replace.planned_stub_kind,
            InlineCacheStubKind::RepatchingStub
        );
        assert_eq!(
            transition.planned_stub_kind,
            InlineCacheStubKind::RepatchingStub
        );
    }

    #[test]
    fn property_store_access_case_plan_table_accepts_indexed_store_metadata() {
        let indexed = ready_property_store_indexed_plan();

        let table = PropertyStoreAccessCasePlanTable::new(indexed.owner, vec![indexed.clone()])
            .expect("valid indexed property-store plan table");

        assert_eq!(table.owner(), indexed.owner);
        assert_eq!(table.len(), 1);
        assert!(!table.is_empty());
        assert_eq!(table.plans(), &[indexed.clone()]);
        assert_eq!(
            table
                .candidates_for_bytecode_index(indexed.bytecode_index)
                .collect::<Vec<_>>(),
            vec![&indexed]
        );
        assert_eq!(
            indexed.effect_contract,
            PropertyStoreAccessCasePlanContract::DATA_ONLY_INDEXED_STORE
        );
        assert!(indexed
            .effect_contract
            .supports_metadata_only_indexed_store_plan());
        assert_eq!(
            indexed.effect_contract.barrier,
            PropertyStoreBarrierEffect::RequiresRuntimeStoredValueBarrierProof
        );
        assert_eq!(
            indexed.planned_stub_kind,
            InlineCacheStubKind::RepatchingStub
        );
        assert_eq!(indexed.access_case.kind, AccessCaseKind::IndexedStore);
        assert_eq!(indexed.access_case.offset, None);
        assert_eq!(indexed.access_case.new_structure, None);
    }

    #[test]
    fn property_store_mutation_candidate_table_accepts_replace_transition_and_indexed_readiness_metadata(
    ) {
        let replace = property_store_mutation_candidate(ready_property_store_replace_plan(), 1);
        let transition =
            property_store_mutation_candidate(ready_property_store_transition_plan(), 2);
        let indexed = property_store_mutation_candidate(ready_property_store_indexed_plan(), 3);

        let table = PropertyStoreMutationCandidateTable::new(
            replace.plan.owner,
            vec![replace.clone(), transition.clone(), indexed.clone()],
        )
        .expect("valid property-store mutation candidate table");

        assert_eq!(table.owner(), replace.plan.owner);
        assert_eq!(table.len(), 3);
        assert!(!table.is_empty());
        assert_eq!(
            table.candidates(),
            &[replace.clone(), transition.clone(), indexed.clone()]
        );
        assert_eq!(
            table
                .candidates_for_bytecode_index(replace.plan.bytecode_index)
                .collect::<Vec<_>>(),
            vec![&replace]
        );
        assert_eq!(
            table
                .candidates_for_bytecode_index(transition.plan.bytecode_index)
                .collect::<Vec<_>>(),
            vec![&transition]
        );
        assert_eq!(
            table
                .candidates_for_bytecode_index(indexed.plan.bytecode_index)
                .collect::<Vec<_>>(),
            vec![&indexed]
        );
        assert_eq!(replace.store_plan_ordinal, 1);
        assert_eq!(replace.install_recheck_ordinal, 101);
        assert_eq!(replace.readiness_ordinal, 301);
        assert_eq!(replace.observation_ordinal, 201);
        assert_eq!(replace.stored_value_kind, ValueKind::Int32);
        assert_eq!(
            replace.barrier_evidence.last_write_barrier,
            BarrierRequirementOutcome::Required(BarrierAction::MarkingBarrier)
        );
    }

    #[test]
    fn property_store_mutation_candidate_table_accepts_non_cell_no_barrier_readiness_metadata() {
        let mut replace = property_store_mutation_candidate(ready_property_store_replace_plan(), 1);
        replace.barrier_evidence.observed_write_barrier_count = 0;
        replace.barrier_evidence.last_write_barrier =
            BarrierRequirementOutcome::NotRequired(BarrierNotRequiredReason::NullOrNonCellTarget);
        replace.stored_value_kind = ValueKind::Int32;

        let table =
            PropertyStoreMutationCandidateTable::new(replace.plan.owner, vec![replace.clone()])
                .expect("valid non-cell no-barrier property-store candidate table");

        assert_eq!(table.candidates(), std::slice::from_ref(&replace));

        let mut cell = replace.clone();
        cell.stored_value_kind = ValueKind::Cell;
        assert_eq!(
            PropertyStoreMutationCandidateTable::new(cell.plan.owner, vec![cell]),
            Err(
                InlineCacheValidationError::PropertyStoreMutationCandidateInvalidBarrierObservationCount(
                    0,
                )
            )
        );
    }

    #[test]
    fn property_store_mutation_candidate_table_newest_first_filters_without_reordering_storage() {
        let replace = property_store_mutation_candidate(ready_property_store_replace_plan(), 1);
        let mut newer_replace_plan = replace.plan.clone();
        newer_replace_plan.access_case.base_structure = Some(StructureId(161));
        newer_replace_plan.access_case.offset = Some(PropertyOffset::new(6));
        let newer_replace = property_store_mutation_candidate(newer_replace_plan, 2);
        let indexed = property_store_mutation_candidate(ready_property_store_indexed_plan(), 3);

        let table = PropertyStoreMutationCandidateTable::new(
            replace.plan.owner,
            vec![replace.clone(), newer_replace.clone(), indexed.clone()],
        )
        .expect("valid property-store mutation candidate table");

        assert_eq!(
            table.candidates(),
            &[replace.clone(), newer_replace.clone(), indexed.clone()]
        );
        assert_eq!(
            table
                .candidates_for_bytecode_index(replace.plan.bytecode_index)
                .collect::<Vec<_>>(),
            vec![&replace, &newer_replace]
        );
        assert_eq!(
            table
                .candidates_for_bytecode_index_newest_first(replace.plan.bytecode_index)
                .collect::<Vec<_>>(),
            vec![&newer_replace, &replace]
        );
        assert_eq!(
            table
                .candidates_for_bytecode_index_newest_first(indexed.plan.bytecode_index)
                .collect::<Vec<_>>(),
            vec![&indexed]
        );
        assert!(table
            .candidates_for_bytecode_index_newest_first(1234)
            .next()
            .is_none());
    }

    #[test]
    fn property_store_mutation_candidate_table_rejects_barrier_evidence_metadata_mismatch() {
        let mut wrong_plan_kind =
            property_store_mutation_candidate(ready_property_store_replace_plan(), 1);
        wrong_plan_kind.barrier_evidence.plan_kind =
            PropertyStoreAccessCasePlanKind::DataOnlyTransition;
        assert_eq!(
            PropertyStoreMutationCandidateTable::new(
                wrong_plan_kind.plan.owner,
                vec![wrong_plan_kind]
            ),
            Err(
                InlineCacheValidationError::PropertyStoreMutationCandidateBarrierEvidenceMismatch {
                    field: PropertyStoreMutationBarrierEvidenceMismatchField::PlanKind,
                }
            )
        );

        let mut wrong_contract =
            property_store_mutation_candidate(ready_property_store_replace_plan(), 2);
        wrong_contract.barrier_evidence.effect_contract =
            PropertyStoreAccessCasePlanContract::DATA_ONLY_TRANSITION;
        assert_eq!(
            PropertyStoreMutationCandidateTable::new(
                wrong_contract.plan.owner,
                vec![wrong_contract]
            ),
            Err(
                InlineCacheValidationError::PropertyStoreMutationCandidateBarrierEvidenceMismatch {
                    field: PropertyStoreMutationBarrierEvidenceMismatchField::EffectContract,
                }
            )
        );

        let mut wrong_barrier =
            property_store_mutation_candidate(ready_property_store_replace_plan(), 3);
        wrong_barrier.barrier_evidence.barrier_effect =
            PropertyStoreBarrierEffect::RequiresRuntimeStoredValueAndStructureTransitionBarrierProof;
        assert_eq!(
            PropertyStoreMutationCandidateTable::new(wrong_barrier.plan.owner, vec![wrong_barrier]),
            Err(
                InlineCacheValidationError::PropertyStoreMutationCandidateBarrierEvidenceMismatch {
                    field: PropertyStoreMutationBarrierEvidenceMismatchField::BarrierEffect,
                }
            )
        );
    }

    #[test]
    fn property_store_mutation_candidate_table_rejects_malformed_plan_and_invalid_provenance() {
        let mut malformed_plan =
            property_store_mutation_candidate(ready_property_store_replace_plan(), 1);
        malformed_plan.plan.access_case.offset = None;
        assert_eq!(
            PropertyStoreMutationCandidateTable::new(
                malformed_plan.plan.owner,
                vec![malformed_plan]
            ),
            Err(InlineCacheValidationError::PropertyStorePlanMissingOffset)
        );

        let mut zero_store_plan =
            property_store_mutation_candidate(ready_property_store_replace_plan(), 2);
        zero_store_plan.store_plan_ordinal = 0;
        assert_eq!(
            PropertyStoreMutationCandidateTable::new(
                zero_store_plan.plan.owner,
                vec![zero_store_plan]
            ),
            Err(
                InlineCacheValidationError::PropertyStoreMutationCandidateInvalidOrdinal {
                    field: "store_plan_ordinal",
                    ordinal: 0,
                }
            )
        );

        let mut zero_observation =
            property_store_mutation_candidate(ready_property_store_replace_plan(), 3);
        zero_observation.observation_ordinal = 0;
        assert_eq!(
            PropertyStoreMutationCandidateTable::new(
                zero_observation.plan.owner,
                vec![zero_observation]
            ),
            Err(
                InlineCacheValidationError::PropertyStoreMutationCandidateInvalidOrdinal {
                    field: "observation_ordinal",
                    ordinal: 0,
                }
            )
        );

        let mut zero_readiness =
            property_store_mutation_candidate(ready_property_store_replace_plan(), 4);
        zero_readiness.readiness_ordinal = 0;
        assert_eq!(
            PropertyStoreMutationCandidateTable::new(
                zero_readiness.plan.owner,
                vec![zero_readiness]
            ),
            Err(
                InlineCacheValidationError::PropertyStoreMutationCandidateInvalidOrdinal {
                    field: "readiness_ordinal",
                    ordinal: 0,
                }
            )
        );

        let mut missing_barrier_observation =
            property_store_mutation_candidate(ready_property_store_replace_plan(), 5);
        missing_barrier_observation
            .barrier_evidence
            .observed_write_barrier_count = 0;
        assert_eq!(
            PropertyStoreMutationCandidateTable::new(
                missing_barrier_observation.plan.owner,
                vec![missing_barrier_observation]
            ),
            Err(
                InlineCacheValidationError::PropertyStoreMutationCandidateInvalidBarrierObservationCount(
                    0,
                )
            )
        );
    }

    #[test]
    fn property_store_mutation_candidate_table_rejects_duplicate_candidates() {
        let first = property_store_mutation_candidate(ready_property_store_replace_plan(), 1);
        let second_same_ordinal =
            property_store_mutation_candidate(ready_property_store_transition_plan(), 1);

        assert_eq!(
            PropertyStoreMutationCandidateTable::new(
                first.plan.owner,
                vec![first.clone(), second_same_ordinal]
            ),
            Err(
                InlineCacheValidationError::PropertyStoreMutationCandidateDuplicateStorePlanOrdinal(
                    1,
                )
            )
        );

        let mut second_same_readiness =
            property_store_mutation_candidate(ready_property_store_transition_plan(), 2);
        second_same_readiness.readiness_ordinal = first.readiness_ordinal;
        assert_eq!(
            PropertyStoreMutationCandidateTable::new(
                first.plan.owner,
                vec![first.clone(), second_same_readiness]
            ),
            Err(
                InlineCacheValidationError::PropertyStoreMutationCandidateDuplicateReadinessOrdinal(
                    first.readiness_ordinal,
                )
            )
        );

        let mut second_same_semantics = first.clone();
        second_same_semantics.store_plan_ordinal = 2;
        second_same_semantics.install_recheck_ordinal = 102;
        second_same_semantics.readiness_ordinal = 302;
        second_same_semantics.observation_ordinal = 202;
        assert_eq!(
            PropertyStoreMutationCandidateTable::new(
                first.plan.owner,
                vec![first.clone(), second_same_semantics]
            ),
            Err(
                InlineCacheValidationError::PropertyStoreMutationCandidateDuplicate {
                    store_plan_ordinal: 2,
                    bytecode_index: first.plan.bytecode_index,
                    key: first.plan.key,
                    base_structure: first.plan.access_case.base_structure.unwrap(),
                    offset: first.plan.access_case.offset.unwrap(),
                    new_structure: first.plan.access_case.new_structure,
                }
            )
        );
    }

    #[test]
    fn property_store_mutation_candidate_table_remains_separate_from_generated_execution() {
        let plan = ready_property_store_transition_plan();
        let plan_table = PropertyStoreAccessCasePlanTable::new(plan.owner, vec![plan.clone()])
            .expect("valid property-store plan table");
        let candidate = property_store_mutation_candidate(plan.clone(), 1);
        let candidate_table =
            PropertyStoreMutationCandidateTable::new(plan.owner, vec![candidate.clone()])
                .expect("valid property-store mutation candidate table");

        assert_eq!(plan_table.plans(), std::slice::from_ref(&plan));
        assert_eq!(
            candidate_table.candidates(),
            std::slice::from_ref(&candidate)
        );
        assert_eq!(candidate_table.candidates()[0].plan, plan);
        assert_eq!(
            candidate_table.candidates()[0]
                .barrier_evidence
                .barrier_effect,
            plan.effect_contract.barrier
        );

        let source = include_str!("ic.rs");
        for forbidden in [
            concat!("PropertyStoreMutationCandidate::", "execute"),
            concat!("PropertyStoreMutationCandidateTable::", "execute"),
            concat!("execute_", "property_store_mutation_candidate"),
            concat!("install_", "property_store_mutation_candidate"),
            concat!("record_", "property_store_mutation_candidate"),
            concat!("BaselineGenerated", "PropertyStoreExecutionSidecar"),
            concat!("GeneratedPropertyStoreMutationRequest::", "execute"),
            concat!("GeneratedPropertyStoreMutationCommit::", "apply"),
            concat!("BarrierMutationAuthority::", "MutatorFieldWrite"),
            concat!("CodeBlockMutationAuthority::", "VmMainThread"),
        ] {
            assert!(
                !source.contains(forbidden),
                "unexpected generated store mutation candidate wiring found: {forbidden}"
            );
        }
    }

    #[test]
    fn generated_property_store_probe_request_preserves_replace_plan_base_and_stored_value() {
        let plan = ready_property_store_replace_plan();
        let base = RuntimeValue::from_i32(17);
        let stored_value = RuntimeValue::from_i32(29);
        let request = GeneratedPropertyStoreProbeRequest::new(&plan, base, stored_value);

        assert_eq!(request.plan, &plan);
        assert_eq!(request.base, base);
        assert_eq!(request.stored_value, stored_value);
        assert_eq!(
            request.plan_kind(),
            PropertyStoreAccessCasePlanKind::DataOnlyReplace
        );
        assert_eq!(request.key(), plan.key);
        assert_eq!(
            request.barrier_effect(),
            PropertyStoreBarrierEffect::RequiresRuntimeStoredValueBarrierProof
        );
        assert!(!request.requires_structure_transition());
    }

    #[test]
    fn generated_property_store_probe_request_preserves_transition_plan_and_barrier_effect() {
        let plan = ready_property_store_transition_plan();
        let request = GeneratedPropertyStoreProbeRequest::new(
            &plan,
            RuntimeValue::from_i32(31),
            RuntimeValue::from_i32(37),
        );

        assert_eq!(
            request.plan_kind(),
            PropertyStoreAccessCasePlanKind::DataOnlyTransition
        );
        assert_eq!(request.key(), plan.key);
        assert_eq!(
            request.barrier_effect(),
            PropertyStoreBarrierEffect::RequiresRuntimeStoredValueAndStructureTransitionBarrierProof
        );
        assert!(request.requires_structure_transition());
    }

    #[test]
    fn generated_property_store_probe_hit_metadata_distinguishes_replace_and_transition() {
        let stored_value = RuntimeValue::from_i32(41);
        let replace = ready_property_store_replace_plan();
        let replace_hit =
            match GeneratedPropertyStoreProbeResult::hit_for_plan(&replace, stored_value) {
                GeneratedPropertyStoreProbeResult::Hit(hit) => hit,
                other => panic!("expected replace hit metadata, got {other:?}"),
            };

        assert_eq!(
            replace_hit.continuation_plan_kind,
            PropertyStoreAccessCasePlanKind::DataOnlyReplace
        );
        assert_eq!(replace_hit.key, replace.key);
        assert_eq!(replace_hit.stored_value, stored_value);
        assert_eq!(replace_hit.effect_contract, replace.effect_contract);
        assert_eq!(replace_hit.barrier_effect, replace.effect_contract.barrier);
        assert_eq!(
            replace_hit.base_structure,
            replace.access_case.base_structure.unwrap()
        );
        assert_eq!(replace_hit.planned_new_structure, None);
        assert_eq!(
            replace_hit.planned_offset,
            replace.access_case.offset.unwrap()
        );

        let transition = ready_property_store_transition_plan();
        let transition_hit =
            match GeneratedPropertyStoreProbeResult::hit_for_plan(&transition, stored_value) {
                GeneratedPropertyStoreProbeResult::Hit(hit) => hit,
                other => panic!("expected transition hit metadata, got {other:?}"),
            };

        assert_eq!(
            transition_hit.continuation_plan_kind,
            PropertyStoreAccessCasePlanKind::DataOnlyTransition
        );
        assert_eq!(transition_hit.key, transition.key);
        assert_eq!(transition_hit.stored_value, stored_value);
        assert_eq!(transition_hit.effect_contract, transition.effect_contract);
        assert_eq!(
            transition_hit.barrier_effect,
            transition.effect_contract.barrier
        );
        assert_eq!(
            transition_hit.base_structure,
            transition.access_case.base_structure.unwrap()
        );
        assert_eq!(
            transition_hit.planned_new_structure,
            transition.access_case.new_structure
        );
        assert_eq!(
            transition_hit.planned_offset,
            transition.access_case.offset.unwrap()
        );

        let indexed = ready_property_store_indexed_plan();
        let indexed_hit =
            match GeneratedPropertyStoreProbeResult::hit_for_plan(&indexed, stored_value) {
                GeneratedPropertyStoreProbeResult::Hit(hit) => hit,
                other => panic!("expected indexed store hit metadata, got {other:?}"),
            };

        assert_eq!(
            indexed_hit.continuation_plan_kind,
            PropertyStoreAccessCasePlanKind::DataOnlyIndexedStore
        );
        assert_eq!(indexed_hit.key, indexed.key);
        assert_eq!(indexed_hit.stored_value, stored_value);
        assert_eq!(indexed_hit.effect_contract, indexed.effect_contract);
        assert_eq!(indexed_hit.barrier_effect, indexed.effect_contract.barrier);
        assert_eq!(
            indexed_hit.base_structure,
            indexed.access_case.base_structure.unwrap()
        );
        assert_eq!(indexed_hit.planned_new_structure, None);
        assert_eq!(indexed_hit.planned_offset, PropertyOffset::INVALID);
    }

    #[test]
    fn generated_property_store_probe_miss_result_records_reasons_without_host_or_vm() {
        for reason in [
            GeneratedPropertyStoreProbeMissReason::HostUnavailable,
            GeneratedPropertyStoreProbeMissReason::UnsupportedPlan,
            GeneratedPropertyStoreProbeMissReason::UnsupportedPlanMetadata,
            GeneratedPropertyStoreProbeMissReason::NonCellBase,
            GeneratedPropertyStoreProbeMissReason::UnknownObject,
            GeneratedPropertyStoreProbeMissReason::StructureMismatch,
            GeneratedPropertyStoreProbeMissReason::MissingBaseStructure,
            GeneratedPropertyStoreProbeMissReason::MissingOrInvalidOffset,
            GeneratedPropertyStoreProbeMissReason::KeyNotRepresentable,
            GeneratedPropertyStoreProbeMissReason::ExistingPropertyMismatch,
            GeneratedPropertyStoreProbeMissReason::MissingTransitionStructure,
            GeneratedPropertyStoreProbeMissReason::TransitionStructureMismatch,
            GeneratedPropertyStoreProbeMissReason::IndexedProperty,
            GeneratedPropertyStoreProbeMissReason::OpaqueObject,
            GeneratedPropertyStoreProbeMissReason::MissingBarrierEvidence,
            GeneratedPropertyStoreProbeMissReason::UnsupportedBarrierEffect,
            GeneratedPropertyStoreProbeMissReason::BarrierContractMismatch,
        ] {
            assert_eq!(
                GeneratedPropertyStoreProbeResult::miss(reason),
                GeneratedPropertyStoreProbeResult::Miss(GeneratedPropertyStoreProbeMiss { reason })
            );
        }
    }

    #[test]
    fn generated_property_store_probe_malformed_plan_metadata_is_represented_as_misses() {
        let stored_value = RuntimeValue::from_i32(43);

        let mut unsupported_kind = ready_property_store_replace_plan();
        unsupported_kind.plan_kind = PropertyStoreAccessCasePlanKind::Unsupported;
        assert_eq!(
            GeneratedPropertyStoreProbeResult::hit_for_plan(&unsupported_kind, stored_value),
            GeneratedPropertyStoreProbeResult::miss(
                GeneratedPropertyStoreProbeMissReason::UnsupportedPlan
            )
        );

        let mut wrong_stub = ready_property_store_replace_plan();
        wrong_stub.planned_stub_kind = InlineCacheStubKind::DataOnlyHandler;
        assert_eq!(
            GeneratedPropertyStoreProbeResult::hit_for_plan(&wrong_stub, stored_value),
            GeneratedPropertyStoreProbeResult::miss(
                GeneratedPropertyStoreProbeMissReason::UnsupportedPlanMetadata
            )
        );

        let mut indexed_key = ready_property_store_replace_plan();
        indexed_key.key = CacheKey::Property(PropertyKey::from_index(
            PropertyIndex::from_canonical_index(0),
        ));
        indexed_key.access_case.key = indexed_key.key;
        assert_eq!(
            GeneratedPropertyStoreProbeResult::hit_for_plan(&indexed_key, stored_value),
            GeneratedPropertyStoreProbeResult::miss(
                GeneratedPropertyStoreProbeMissReason::IndexedProperty
            )
        );

        let mut dynamic_key = ready_property_store_replace_plan();
        dynamic_key.key = CacheKey::Dynamic;
        dynamic_key.access_case.key = CacheKey::Dynamic;
        assert_eq!(
            GeneratedPropertyStoreProbeResult::hit_for_plan(&dynamic_key, stored_value),
            GeneratedPropertyStoreProbeResult::miss(
                GeneratedPropertyStoreProbeMissReason::KeyNotRepresentable
            )
        );

        let mut key_mismatch = ready_property_store_replace_plan();
        key_mismatch.access_case.key = CacheKey::Dynamic;
        assert_eq!(
            GeneratedPropertyStoreProbeResult::hit_for_plan(&key_mismatch, stored_value),
            GeneratedPropertyStoreProbeResult::miss(
                GeneratedPropertyStoreProbeMissReason::ExistingPropertyMismatch
            )
        );

        let mut missing_structure = ready_property_store_replace_plan();
        missing_structure.access_case.base_structure = None;
        assert_eq!(
            GeneratedPropertyStoreProbeResult::hit_for_plan(&missing_structure, stored_value),
            GeneratedPropertyStoreProbeResult::miss(
                GeneratedPropertyStoreProbeMissReason::MissingBaseStructure
            )
        );

        let mut missing_offset = ready_property_store_replace_plan();
        missing_offset.access_case.offset = None;
        assert_eq!(
            GeneratedPropertyStoreProbeResult::hit_for_plan(&missing_offset, stored_value),
            GeneratedPropertyStoreProbeResult::miss(
                GeneratedPropertyStoreProbeMissReason::MissingOrInvalidOffset
            )
        );

        let mut replace_with_new_structure = ready_property_store_replace_plan();
        replace_with_new_structure.access_case.new_structure = Some(StructureId(153));
        assert_eq!(
            GeneratedPropertyStoreProbeResult::hit_for_plan(
                &replace_with_new_structure,
                stored_value
            ),
            GeneratedPropertyStoreProbeResult::miss(
                GeneratedPropertyStoreProbeMissReason::TransitionStructureMismatch
            )
        );

        let mut missing_new_structure = ready_property_store_transition_plan();
        missing_new_structure.access_case.new_structure = None;
        assert_eq!(
            GeneratedPropertyStoreProbeResult::hit_for_plan(&missing_new_structure, stored_value),
            GeneratedPropertyStoreProbeResult::miss(
                GeneratedPropertyStoreProbeMissReason::MissingTransitionStructure
            )
        );

        let mut wrong_barrier_contract = ready_property_store_replace_plan();
        wrong_barrier_contract.effect_contract =
            PropertyStoreAccessCasePlanContract::DATA_ONLY_TRANSITION;
        assert_eq!(
            GeneratedPropertyStoreProbeResult::hit_for_plan(&wrong_barrier_contract, stored_value),
            GeneratedPropertyStoreProbeResult::miss(
                GeneratedPropertyStoreProbeMissReason::BarrierContractMismatch
            )
        );

        let mut unsupported_barrier = ready_property_store_replace_plan();
        unsupported_barrier.effect_contract = PropertyStoreAccessCasePlanContract {
            effects: PropertyStoreAccessCaseEffects::DataOnlyReplace(
                PropertyStoreDataOnlyReplaceEffects {
                    heap: PropertyStoreHeapEffect::WritesExistingOwnDataSlot,
                    stored_value: PropertyStoreStoredValueEffect::StoresProvidedValue,
                    exit: PropertyStoreExitEffect::MayExitToSlowPathBeforeHeapMutation,
                    host_boundary: PropertyStoreHostBoundaryEffect::NoAllocationNoCallsNoGcBoundary,
                },
            ),
            barrier: PropertyStoreBarrierEffect::Unsupported,
        };
        assert_eq!(
            GeneratedPropertyStoreProbeResult::hit_for_plan(&unsupported_barrier, stored_value),
            GeneratedPropertyStoreProbeResult::miss(
                GeneratedPropertyStoreProbeMissReason::UnsupportedBarrierEffect
            )
        );
    }

    #[test]
    fn generated_property_store_mutation_request_preserves_probe_hit_exactly() {
        let stored_value = RuntimeValue::from_i32(47);
        let plan = ready_property_store_replace_plan();
        let probe_hit = match GeneratedPropertyStoreProbeResult::hit_for_plan(&plan, stored_value) {
            GeneratedPropertyStoreProbeResult::Hit(hit) => hit,
            other => panic!("expected store probe hit metadata, got {other:?}"),
        };
        let base = RuntimeValue::from_i32(53);

        let request = GeneratedPropertyStoreMutationRequest::new(base, probe_hit);

        assert_eq!(request.base, base);
        assert_eq!(request.probe_hit, probe_hit);
        assert_eq!(request.plan_kind(), probe_hit.continuation_plan_kind);
        assert_eq!(request.key(), probe_hit.key);
        assert_eq!(request.barrier_effect(), probe_hit.barrier_effect);
        assert_eq!(request.stored_value_kind(), stored_value.kind());
    }

    #[test]
    fn generated_property_store_mutation_replace_commit_preserves_host_confirmed_metadata() {
        let stored_value = RuntimeValue::from_i32(59);
        let plan = ready_property_store_replace_plan();
        let probe_hit = match GeneratedPropertyStoreProbeResult::hit_for_plan(&plan, stored_value) {
            GeneratedPropertyStoreProbeResult::Hit(hit) => hit,
            other => panic!("expected replace store probe hit metadata, got {other:?}"),
        };
        let request =
            GeneratedPropertyStoreMutationRequest::new(RuntimeValue::from_i32(61), probe_hit);

        let commit = GeneratedPropertyStoreMutationCommit::host_confirmed_for_request(&request);

        assert_eq!(
            commit.plan_kind,
            PropertyStoreAccessCasePlanKind::DataOnlyReplace
        );
        assert_eq!(commit.key, plan.key);
        assert_eq!(commit.stored_value, stored_value);
        assert_eq!(commit.stored_value_kind, ValueKind::Int32);
        assert_eq!(
            commit.base_structure_before,
            plan.access_case.base_structure.unwrap()
        );
        assert_eq!(
            commit.base_structure_after,
            plan.access_case.base_structure.unwrap()
        );
        assert_eq!(commit.planned_new_structure, None);
        assert_eq!(commit.planned_offset, plan.access_case.offset.unwrap());
        assert_eq!(commit.effect_contract, plan.effect_contract);
        assert_eq!(commit.barrier_effect, plan.effect_contract.barrier);
        assert!(!commit.requires_structure_transition());
    }

    #[test]
    fn generated_property_store_mutation_transition_commit_preserves_host_confirmed_metadata() {
        let stored_value = RuntimeValue::from_i32(67);
        let plan = ready_property_store_transition_plan();
        let probe_hit = match GeneratedPropertyStoreProbeResult::hit_for_plan(&plan, stored_value) {
            GeneratedPropertyStoreProbeResult::Hit(hit) => hit,
            other => panic!("expected transition store probe hit metadata, got {other:?}"),
        };
        let request =
            GeneratedPropertyStoreMutationRequest::new(RuntimeValue::from_i32(71), probe_hit);

        let commit = GeneratedPropertyStoreMutationCommit::host_confirmed_for_request(&request);

        assert_eq!(
            commit.plan_kind,
            PropertyStoreAccessCasePlanKind::DataOnlyTransition
        );
        assert_eq!(commit.key, plan.key);
        assert_eq!(commit.stored_value, stored_value);
        assert_eq!(commit.stored_value_kind, ValueKind::Int32);
        assert_eq!(
            commit.base_structure_before,
            plan.access_case.base_structure.unwrap()
        );
        assert_eq!(
            commit.base_structure_after,
            plan.access_case.new_structure.unwrap()
        );
        assert_eq!(commit.planned_new_structure, plan.access_case.new_structure);
        assert_eq!(commit.planned_offset, plan.access_case.offset.unwrap());
        assert_eq!(commit.effect_contract, plan.effect_contract);
        assert_eq!(commit.barrier_effect, plan.effect_contract.barrier);
        assert!(commit.requires_structure_transition());
    }

    #[test]
    fn generated_property_store_mutation_rejections_are_non_mutating_results() {
        for reason in [
            GeneratedPropertyStoreMutationMissReason::HostUnavailable,
            GeneratedPropertyStoreMutationMissReason::NonCellBase,
            GeneratedPropertyStoreMutationMissReason::UnknownObject,
            GeneratedPropertyStoreMutationMissReason::OpaqueObject,
            GeneratedPropertyStoreMutationMissReason::StructureMismatch,
            GeneratedPropertyStoreMutationMissReason::ExistingPropertyMismatch,
            GeneratedPropertyStoreMutationMissReason::MissingTransitionStructure,
            GeneratedPropertyStoreMutationMissReason::TransitionStructureMismatch,
            GeneratedPropertyStoreMutationMissReason::MissingOrInvalidOffset,
            GeneratedPropertyStoreMutationMissReason::KeyNotRepresentable,
            GeneratedPropertyStoreMutationMissReason::IndexedProperty,
            GeneratedPropertyStoreMutationMissReason::BarrierRejected,
        ] {
            let result = GeneratedPropertyStoreMutationResult::rejected(reason);

            assert_eq!(
                result,
                GeneratedPropertyStoreMutationResult::Rejected(
                    GeneratedPropertyStoreMutationRejection { reason }
                )
            );
            assert!(!result.implies_host_mutation());
        }

        let probe_reason = GeneratedPropertyStoreProbeMissReason::StructureMismatch;
        let probe_rejected = GeneratedPropertyStoreMutationResult::probe_rejected(probe_reason);
        assert_eq!(
            probe_rejected,
            GeneratedPropertyStoreMutationResult::Rejected(
                GeneratedPropertyStoreMutationRejection {
                    reason: GeneratedPropertyStoreMutationMissReason::ProbeRejected(probe_reason),
                }
            )
        );
        assert!(!probe_rejected.implies_host_mutation());
    }

    #[test]
    fn generated_property_store_mutation_contract_stays_metadata_only() {
        let stored_value = RuntimeValue::from_i32(73);
        let plan = ready_property_store_transition_plan();
        let table = PropertyStoreAccessCasePlanTable::new(plan.owner, vec![plan.clone()])
            .expect("valid property-store plan table");
        let original_table = table.clone();
        let probe_hit = match GeneratedPropertyStoreProbeResult::hit_for_plan(&plan, stored_value) {
            GeneratedPropertyStoreProbeResult::Hit(hit) => hit,
            other => panic!("expected transition store probe hit metadata, got {other:?}"),
        };
        let request =
            GeneratedPropertyStoreMutationRequest::new(RuntimeValue::from_i32(79), probe_hit);
        let commit = GeneratedPropertyStoreMutationCommit::host_confirmed_for_request(&request);

        assert_eq!(table, original_table);
        assert_eq!(
            GeneratedPropertyStoreMutationResult::committed(commit),
            GeneratedPropertyStoreMutationResult::Committed(commit)
        );
        assert!(GeneratedPropertyStoreMutationResult::committed(commit).implies_host_mutation());

        let source = include_str!("ic.rs");
        for forbidden in [
            concat!("BaselineGenerated", "PropertyStoreExecutionSidecar"),
            concat!(
                "execute_baseline_generated_code_with_",
                "property_store_sidecar"
            ),
            concat!("execute_", "property_store_sidecar_candidate"),
            concat!("apply_generated_", "property_store_mutation"),
            concat!("record_generated_", "property_store_mutation"),
            concat!("GeneratedPropertyStoreMutationRequest::", "execute"),
            concat!("GeneratedPropertyStoreMutationCommit::", "apply"),
            concat!("BarrierMutationAuthority::", "MutatorFieldWrite"),
            concat!("CodeBlockMutationAuthority::", "VmMainThread"),
        ] {
            assert!(
                !source.contains(forbidden),
                "unexpected generated store mutation wiring found: {forbidden}"
            );
        }
    }

    #[test]
    fn property_store_access_case_plan_table_rejects_owner_mismatch() {
        let plan = ready_property_store_replace_plan();
        let expected_owner = CodeBlockId(CellId(199));

        assert_eq!(
            PropertyStoreAccessCasePlanTable::new(expected_owner, vec![plan.clone()]),
            Err(InlineCacheValidationError::PropertyStorePlanOwnerMismatch {
                expected: expected_owner,
                actual: plan.owner,
            })
        );
    }

    #[test]
    fn property_store_access_case_plan_table_rejects_kind_stub_access_case_and_contract() {
        let mut unsupported_kind = ready_property_store_replace_plan();
        unsupported_kind.plan_kind = PropertyStoreAccessCasePlanKind::Unsupported;
        assert_store_plan_table_error(
            unsupported_kind,
            InlineCacheValidationError::PropertyStorePlanUnsupportedKind(
                PropertyStoreAccessCasePlanKind::Unsupported,
            ),
        );

        let mut wrong_stub = ready_property_store_replace_plan();
        wrong_stub.planned_stub_kind = InlineCacheStubKind::DataOnlyHandler;
        assert_store_plan_table_error(
            wrong_stub,
            InlineCacheValidationError::PropertyStorePlanUnsupportedStubKind(
                InlineCacheStubKind::DataOnlyHandler,
            ),
        );

        let mut setter = ready_property_store_replace_plan();
        setter.access_case.kind = AccessCaseKind::Setter;
        assert_store_plan_table_error(
            setter,
            InlineCacheValidationError::PropertyStorePlanUnsupportedAccessCase(
                AccessCaseKind::Setter,
            ),
        );

        let mut wrong_contract = ready_property_store_replace_plan();
        wrong_contract.effect_contract = PropertyStoreAccessCasePlanContract::DATA_ONLY_TRANSITION;
        assert_store_plan_table_error(
            wrong_contract.clone(),
            InlineCacheValidationError::PropertyStorePlanUnsupportedEffectContract(
                wrong_contract.effect_contract,
            ),
        );

        let mut unsupported_effect = ready_property_store_replace_plan();
        unsupported_effect.effect_contract = PropertyStoreAccessCasePlanContract {
            effects: PropertyStoreAccessCaseEffects::Unsupported,
            barrier: PropertyStoreBarrierEffect::RequiresRuntimeStoredValueBarrierProof,
        };
        assert_store_plan_table_error(
            unsupported_effect.clone(),
            InlineCacheValidationError::PropertyStorePlanUnsupportedEffectContract(
                unsupported_effect.effect_contract,
            ),
        );
    }

    #[test]
    fn property_store_access_case_plan_table_rejects_missing_barrier_proof() {
        let mut plan = ready_property_store_replace_plan();
        plan.effect_contract = PropertyStoreAccessCasePlanContract {
            effects: PropertyStoreAccessCaseEffects::DataOnlyReplace(
                PropertyStoreDataOnlyReplaceEffects {
                    heap: PropertyStoreHeapEffect::WritesExistingOwnDataSlot,
                    stored_value: PropertyStoreStoredValueEffect::StoresProvidedValue,
                    exit: PropertyStoreExitEffect::MayExitToSlowPathBeforeHeapMutation,
                    host_boundary: PropertyStoreHostBoundaryEffect::NoAllocationNoCallsNoGcBoundary,
                },
            ),
            barrier: PropertyStoreBarrierEffect::Unsupported,
        };

        assert_store_plan_table_error(
            plan.clone(),
            InlineCacheValidationError::PropertyStorePlanMissingBarrierProof(plan.effect_contract),
        );
    }

    #[test]
    fn property_store_access_case_plan_table_rejects_unsupported_keys() {
        let mut dynamic_key = ready_property_store_replace_plan();
        dynamic_key.key = CacheKey::Dynamic;
        dynamic_key.access_case.key = CacheKey::Dynamic;
        assert_store_plan_table_error(
            dynamic_key,
            InlineCacheValidationError::PropertyStorePlanUnsupportedKey(CacheKey::Dynamic),
        );

        let indexed_key = CacheKey::Property(PropertyKey::from_index(
            PropertyIndex::from_canonical_index(0),
        ));
        let mut index_key = ready_property_store_replace_plan();
        index_key.key = indexed_key;
        index_key.access_case.key = indexed_key;
        assert_store_plan_table_error(
            index_key,
            InlineCacheValidationError::PropertyStorePlanUnsupportedKey(indexed_key),
        );

        for key in [
            PropertyKey::from_symbol_uid(SymbolUid::from_table_slot(177)),
            PropertyKey::from_private_name(PrivateName::from_symbol_uid(
                SymbolUid::from_table_slot(178),
            )),
        ] {
            let cache_key = CacheKey::Property(key);
            let mut plan = ready_property_store_replace_plan();
            plan.key = cache_key;
            plan.access_case.key = cache_key;
            assert_store_plan_table_error(
                plan,
                InlineCacheValidationError::PropertyStorePlanUnsupportedKey(cache_key),
            );
        }

        let mut mismatched_access_key = ready_property_store_replace_plan();
        mismatched_access_key.access_case.key = CacheKey::Dynamic;
        assert_store_plan_table_error(
            mismatched_access_key.clone(),
            InlineCacheValidationError::PropertyStorePlanAccessCaseKeyMismatch {
                plan: mismatched_access_key.key,
                access_case: CacheKey::Dynamic,
            },
        );
    }

    #[test]
    fn property_store_access_case_plan_table_rejects_replace_shape_errors() {
        let mut missing_structure = ready_property_store_replace_plan();
        missing_structure.access_case.base_structure = None;
        assert_store_plan_table_error(
            missing_structure,
            InlineCacheValidationError::PropertyStorePlanMissingBaseStructure,
        );

        let mut invalid_structure = ready_property_store_replace_plan();
        invalid_structure.access_case.base_structure = Some(StructureId::INVALID);
        assert_store_plan_table_error(
            invalid_structure,
            InlineCacheValidationError::PropertyStorePlanInvalidBaseStructure(StructureId::INVALID),
        );

        let mut missing_offset = ready_property_store_replace_plan();
        missing_offset.access_case.offset = None;
        assert_store_plan_table_error(
            missing_offset,
            InlineCacheValidationError::PropertyStorePlanMissingOffset,
        );

        let mut invalid_offset = ready_property_store_replace_plan();
        invalid_offset.access_case.offset = Some(PropertyOffset::INVALID);
        assert_store_plan_table_error(
            invalid_offset,
            InlineCacheValidationError::PropertyStorePlanInvalidOffset(PropertyOffset::INVALID),
        );

        let new_structure = StructureId(153);
        let mut with_new_structure = ready_property_store_replace_plan();
        with_new_structure.access_case.new_structure = Some(new_structure);
        assert_store_plan_table_error(
            with_new_structure,
            InlineCacheValidationError::PropertyStorePlanUnsupportedNewStructure(new_structure),
        );
    }

    #[test]
    fn property_store_access_case_plan_table_rejects_transition_shape_errors() {
        let mut missing_structure = ready_property_store_transition_plan();
        missing_structure.access_case.base_structure = None;
        assert_store_plan_table_error(
            missing_structure,
            InlineCacheValidationError::PropertyStorePlanMissingBaseStructure,
        );

        let mut missing_offset = ready_property_store_transition_plan();
        missing_offset.access_case.offset = None;
        assert_store_plan_table_error(
            missing_offset,
            InlineCacheValidationError::PropertyStorePlanMissingOffset,
        );

        let mut missing_new_structure = ready_property_store_transition_plan();
        missing_new_structure.access_case.new_structure = None;
        assert_store_plan_table_error(
            missing_new_structure,
            InlineCacheValidationError::PropertyStorePlanMissingNewStructure,
        );

        let mut invalid_new_structure = ready_property_store_transition_plan();
        invalid_new_structure.access_case.new_structure = Some(StructureId::INVALID);
        assert_store_plan_table_error(
            invalid_new_structure,
            InlineCacheValidationError::PropertyStorePlanInvalidNewStructure(StructureId::INVALID),
        );

        let mut redundant_transition = ready_property_store_transition_plan();
        redundant_transition.access_case.new_structure =
            redundant_transition.access_case.base_structure;
        assert_store_plan_table_error(
            redundant_transition.clone(),
            InlineCacheValidationError::PropertyStorePlanRedundantTransitionStructure(
                redundant_transition.access_case.base_structure.unwrap(),
            ),
        );
    }

    #[test]
    fn property_store_access_case_plan_table_rejects_boundaries_and_dependencies() {
        let holder = ObjectId(CellId(181));
        let mut with_holder = ready_property_store_replace_plan();
        with_holder.access_case.holder = Some(holder);
        assert_store_plan_table_error(
            with_holder,
            InlineCacheValidationError::PropertyStorePlanUnsupportedHolder(holder),
        );

        let mut with_dependency = ready_property_store_replace_plan();
        with_dependency
            .access_case
            .dependencies
            .push(WatchpointDependency {
                id: WatchpointDependencyId(183),
                strength: DependencyStrength::CompileTimeAssumption,
                target: WatchpointTarget::StructureTransition {
                    structure: StructureId(184),
                },
                generation: None,
            });
        assert_store_plan_table_error(
            with_dependency,
            InlineCacheValidationError::PropertyStorePlanUnsupportedDependencies,
        );

        let mut via_global_proxy = ready_property_store_replace_plan();
        via_global_proxy.access_case.via_global_proxy = true;
        assert_store_plan_table_error(
            via_global_proxy,
            InlineCacheValidationError::PropertyStorePlanUnsupportedGlobalProxy,
        );

        let mut may_call_js = ready_property_store_replace_plan();
        may_call_js.access_case.may_call_js = true;
        assert_store_plan_table_error(
            may_call_js,
            InlineCacheValidationError::PropertyStorePlanMayCallJs,
        );
    }

    #[test]
    fn property_store_access_case_plan_table_rejects_duplicate_keys() {
        let replace = ready_property_store_replace_plan();
        assert_eq!(
            PropertyStoreAccessCasePlanTable::new(
                replace.owner,
                vec![replace.clone(), replace.clone()]
            ),
            Err(InlineCacheValidationError::PropertyStorePlanDuplicate {
                bytecode_index: replace.bytecode_index,
                key: replace.key,
                base_structure: replace.access_case.base_structure.unwrap(),
                offset: replace.access_case.offset.unwrap(),
                new_structure: None,
            })
        );

        let transition = ready_property_store_transition_plan();
        assert_eq!(
            PropertyStoreAccessCasePlanTable::new(
                transition.owner,
                vec![transition.clone(), transition.clone()]
            ),
            Err(InlineCacheValidationError::PropertyStorePlanDuplicate {
                bytecode_index: transition.bytecode_index,
                key: transition.key,
                base_structure: transition.access_case.base_structure.unwrap(),
                offset: transition.access_case.offset.unwrap(),
                new_structure: transition.access_case.new_structure,
            })
        );
    }

    #[test]
    fn property_store_access_case_plan_table_distinguishes_duplicate_key_parts() {
        let first = ready_property_store_replace_plan();
        let mut different_bytecode = first.clone();
        different_bytecode.bytecode_index += 1;
        let mut different_base = first.clone();
        different_base.access_case.base_structure = Some(StructureId(160));
        let mut different_offset = first.clone();
        different_offset.access_case.offset = Some(PropertyOffset::new(6));
        let mut transition = ready_property_store_transition_plan();
        transition.bytecode_index = first.bytecode_index;
        transition.access_case.base_structure = first.access_case.base_structure;
        transition.access_case.offset = first.access_case.offset;
        let mut different_new_structure = transition.clone();
        different_new_structure.access_case.new_structure = Some(StructureId(161));

        let table = PropertyStoreAccessCasePlanTable::new(
            first.owner,
            vec![
                first,
                different_bytecode,
                different_base,
                different_offset,
                transition,
                different_new_structure,
            ],
        )
        .expect("distinct property-store duplicate key parts");

        assert_eq!(table.len(), 6);
    }

    #[test]
    fn inline_cache_property_load_observation_blocks_own_data_without_guard_or_offset() {
        let observation = property_load_observation(
            Some(AccessCaseKind::Load),
            false,
            PropertyCacheability::Allowed,
            None,
            None,
            0,
        );

        assert_eq!(observation.validate(), Ok(()));
        assert_eq!(
            observation.readiness,
            PropertyLoadObservationReadiness::Blocked(blockers(&[
                PropertyLoadObservationBlocker::MissingBaseStructureGuard,
                PropertyLoadObservationBlocker::MissingOffset,
            ]))
        );
    }

    #[test]
    fn inline_cache_property_load_ready_own_data_observation_derives_data_only_plan() {
        let observation = ready_own_data_property_observation();

        let plan = plan_property_load_access_case_from_observation(&observation)
            .unwrap()
            .expect("ready own-data load plan");

        assert_eq!(
            plan.plan_kind,
            PropertyLoadAccessCasePlanKind::DataOnlyOwnLoad
        );
        assert_eq!(plan.owner, observation.owner);
        assert_eq!(plan.slot, observation.slot);
        assert_eq!(plan.bytecode_index, observation.bytecode_index);
        assert_eq!(plan.key, observation.key);
        assert_eq!(plan.planned_stub_kind, InlineCacheStubKind::DataOnlyHandler);
        assert_eq!(plan.access_case.kind, AccessCaseKind::Load);
        assert_eq!(plan.access_case.key, observation.key);
        assert_eq!(plan.access_case.base_structure, observation.base_structure);
        assert_eq!(plan.access_case.offset, observation.offset);
        assert_eq!(plan.access_case.new_structure, None);
        assert_eq!(plan.access_case.holder, None);
        assert!(!plan.access_case.via_global_proxy);
        assert!(!plan.access_case.may_call_js);
        assert!(plan.access_case.dependencies.is_empty());

        let slot = InlineCacheSlot::builder(plan.slot, InlineCacheKind::PropertyLoad)
            .owner(plan.owner)
            .bytecode_index(plan.bytecode_index)
            .state(InlineCacheState::Monomorphic)
            .case(plan.access_case.clone())
            .build()
            .unwrap();
        assert_eq!(
            classify_inline_cache_slot(&slot),
            Ok(InlineCacheCaseClassification::Monomorphic)
        );
    }

    #[test]
    fn inline_cache_property_load_normalized_string_base_skips_finite_own_plan() {
        let mut observation = ready_own_data_property_observation();
        observation.base_normalization = PropertyLoadBaseNormalization::StringPrototype;
        observation.readiness = observation.classify_readiness();

        assert_eq!(
            plan_property_load_access_case_from_observation(&observation),
            Ok(None)
        );
    }

    #[test]
    fn inline_cache_element_load_ready_indexed_observation_derives_indexed_plan() {
        let observation = ready_indexed_element_load_observation();

        let plan = plan_property_load_access_case_from_observation(&observation)
            .unwrap()
            .expect("ready indexed element-load plan");

        assert_eq!(
            plan.plan_kind,
            PropertyLoadAccessCasePlanKind::DataOnlyIndexedLoad
        );
        assert_eq!(plan.owner, observation.owner);
        assert_eq!(plan.slot, observation.slot);
        assert_eq!(plan.bytecode_index, observation.bytecode_index);
        assert_eq!(plan.key, observation.key);
        assert_eq!(plan.planned_stub_kind, InlineCacheStubKind::DataOnlyHandler);
        assert_eq!(plan.access_case.kind, AccessCaseKind::IndexedLoad);
        assert_eq!(plan.access_case.key, observation.key);
        assert_eq!(plan.access_case.base_structure, observation.base_structure);
        assert_eq!(plan.access_case.offset, None);
        assert_eq!(plan.access_case.new_structure, None);
        assert_eq!(plan.access_case.holder, None);
        assert!(!plan.access_case.via_global_proxy);
        assert!(!plan.access_case.may_call_js);
        assert!(plan.access_case.dependencies.is_empty());

        let slot = InlineCacheSlot::builder(plan.slot, InlineCacheKind::ElementLoad)
            .owner(plan.owner)
            .bytecode_index(plan.bytecode_index)
            .state(InlineCacheState::Monomorphic)
            .case(plan.access_case.clone())
            .build()
            .unwrap();
        assert_eq!(
            classify_inline_cache_slot(&slot),
            Ok(InlineCacheCaseClassification::Monomorphic)
        );
    }

    #[test]
    fn inline_cache_property_load_index_key_observation_does_not_derive_data_only_plan() {
        let mut observation = ready_own_data_property_observation();
        observation.key = CacheKey::Property(PropertyKey::from_index(
            PropertyIndex::from_canonical_index(0),
        ));
        observation.readiness = observation.classify_readiness();

        assert_eq!(
            plan_property_load_access_case_from_observation(&observation),
            Ok(None)
        );
    }

    #[test]
    fn inline_cache_property_load_symbol_and_private_observations_do_not_derive_data_only_plan() {
        for key in [
            PropertyKey::from_symbol_uid(SymbolUid::from_table_slot(73)),
            PropertyKey::from_private_name(PrivateName::from_symbol_uid(
                SymbolUid::from_table_slot(74),
            )),
        ] {
            let mut observation = ready_own_data_property_observation();
            observation.key = CacheKey::Property(key);
            observation.readiness = observation.classify_readiness();

            assert_eq!(
                plan_property_load_access_case_from_observation(&observation),
                Ok(None)
            );
        }
    }

    #[test]
    fn property_load_access_case_plan_carries_data_only_root_effect_contract() {
        let observation = ready_own_data_property_observation();

        let plan = plan_property_load_access_case_from_observation(&observation)
            .unwrap()
            .expect("ready own-data load plan");

        assert_eq!(
            plan.effect_contract,
            PropertyLoadAccessCasePlanContract::DATA_ONLY_OWN_LOAD
        );
        assert!(plan.effect_contract.supports_generated_data_only_own_load());
        assert_eq!(
            plan.effect_contract.effects,
            PropertyLoadAccessCaseEffects::DataOnlyOwnLoad(PropertyLoadDataOnlyOwnLoadEffects {
                heap: PropertyLoadHeapEffect::ReadsHeap,
                result: PropertyLoadResultEffect::WritesDestinationRegister,
                exit: PropertyLoadExitEffect::MayExitToSlowPath,
                host_boundary: PropertyLoadHostBoundaryEffect::NoAllocationNoCallsNoHeapWrites,
            })
        );
        assert_eq!(
            plan.effect_contract.rooting,
            PropertyLoadAccessCaseRooting::ReturnedCell(
                PropertyLoadReturnedCellRooting::TargetedDestinationRegisterBeforeGcBoundary
            )
        );
    }

    #[test]
    fn inline_cache_property_load_prototype_data_observation_derives_guard_plan_only() {
        let observation = prototype_data_property_observation();

        assert_eq!(
            plan_property_load_access_case_from_observation(&observation),
            Ok(None)
        );
        let plan = plan_property_load_guard_plan_from_observation(&observation)
            .unwrap()
            .expect("prototype-data guard plan");

        assert_eq!(plan.owner, observation.owner);
        assert_eq!(plan.slot, observation.slot);
        assert_eq!(plan.bytecode_index, observation.bytecode_index);
        assert_eq!(
            plan.descriptor.requirement,
            PropertyLoadGuardRequirement::PrototypeChain
        );
        assert_eq!(plan.descriptor.key, observation.key);
        assert_eq!(
            plan.descriptor.base_object,
            observation.base_object.unwrap()
        );
        assert_eq!(plan.descriptor.holder_object, observation.holder_object);
        assert_eq!(
            plan.descriptor.base_structure,
            observation.base_structure.unwrap()
        );
        assert_eq!(plan.descriptor.offset, observation.offset);
        assert_eq!(plan.descriptor.prototype_depth, observation.prototype_depth);
        assert_eq!(
            plan.descriptor.chain.outcome,
            PropertyLoadGuardChainOutcome::PrototypeData {
                holder_index: 1,
                offset: observation.offset.unwrap(),
            }
        );
        assert_eq!(
            plan.descriptor.chain.entries,
            vec![
                PropertyLoadGuardChainEntry {
                    object: observation.base_object.unwrap(),
                    structure: observation.base_structure.unwrap(),
                    next_prototype: observation.holder_object,
                    proof: PropertyLoadGuardChainEntryProof::NoOwnProperty,
                },
                PropertyLoadGuardChainEntry {
                    object: observation.holder_object.unwrap(),
                    structure: StructureId(8),
                    next_prototype: None,
                    proof: PropertyLoadGuardChainEntryProof::DataProperty {
                        offset: observation.offset.unwrap(),
                    },
                },
            ]
        );
    }

    #[test]
    fn inline_cache_property_load_prototype_data_guard_derives_structure_dependencies() {
        let observation = prototype_data_property_observation();
        let plan = plan_property_load_guard_plan_from_observation(&observation)
            .unwrap()
            .expect("prototype-data guard plan");

        let dependencies = derive_property_load_guard_dependencies(&plan, |chain_index| {
            (
                WatchpointSetId(100 + chain_index as u64),
                WatchpointDependencyId(200 + chain_index as u64),
            )
        });

        assert_eq!(dependencies.len(), plan.descriptor.chain.entries.len());
        for (dependency, entry) in dependencies
            .iter()
            .zip(plan.descriptor.chain.entries.iter())
        {
            assert_eq!(
                dependency.set.owner,
                WatchpointOwner::Structure(entry.structure)
            );
            assert_eq!(dependency.set.state, WatchpointSetState::Clear);
            assert_eq!(
                dependency.set.fire_policy,
                WatchpointFirePolicy::RecheckBeforeInstall
            );
            assert_eq!(dependency.set.dependencies, vec![dependency.dependency.id]);
            assert_eq!(
                dependency.dependency.strength,
                DependencyStrength::CompileTimeAssumption
            );
            assert_eq!(
                dependency.dependency.target,
                WatchpointTarget::StructureTransition {
                    structure: entry.structure,
                }
            );
            assert_eq!(dependency.dependency.generation, None);
        }
        assert_eq!(dependencies[0].chain_index, 0);
        assert_eq!(dependencies[0].set.id, WatchpointSetId(100));
        assert_eq!(dependencies[0].dependency.id, WatchpointDependencyId(200));
        assert_eq!(dependencies[1].chain_index, 1);
        assert_eq!(dependencies[1].set.id, WatchpointSetId(101));
        assert_eq!(dependencies[1].dependency.id, WatchpointDependencyId(201));
    }

    #[test]
    fn inline_cache_property_load_prototype_data_guard_rejects_nonterminal_holder_entry() {
        let mut observation = prototype_data_property_observation();
        observation.chain.last_mut().unwrap().next_prototype = Some(ObjectId(CellId(99)));
        observation.readiness = observation.classify_readiness();

        assert_eq!(
            plan_property_load_guard_plan_from_observation(&observation),
            Ok(None)
        );
    }

    #[test]
    fn inline_cache_property_load_own_missing_observation_derives_guard_plan_only() {
        let observation = own_missing_property_observation();

        assert_eq!(
            plan_property_load_access_case_from_observation(&observation),
            Ok(None)
        );
        let plan = plan_property_load_guard_plan_from_observation(&observation)
            .unwrap()
            .expect("own-missing guard plan");

        assert_eq!(plan.owner, observation.owner);
        assert_eq!(plan.slot, observation.slot);
        assert_eq!(plan.bytecode_index, observation.bytecode_index);
        assert_eq!(
            plan.descriptor.requirement,
            PropertyLoadGuardRequirement::NegativeLookup
        );
        assert_eq!(plan.descriptor.key, observation.key);
        assert_eq!(
            plan.descriptor.base_object,
            observation.base_object.unwrap()
        );
        assert_eq!(plan.descriptor.holder_object, None);
        assert_eq!(
            plan.descriptor.base_structure,
            observation.base_structure.unwrap()
        );
        assert_eq!(plan.descriptor.offset, None);
        assert_eq!(plan.descriptor.prototype_depth, 0);
        assert_eq!(
            plan.descriptor.chain.outcome,
            PropertyLoadGuardChainOutcome::Missing {
                terminal_null: true,
            }
        );
        assert_eq!(
            plan.descriptor.chain.entries,
            vec![PropertyLoadGuardChainEntry {
                object: observation.base_object.unwrap(),
                structure: observation.base_structure.unwrap(),
                next_prototype: None,
                proof: PropertyLoadGuardChainEntryProof::NoOwnProperty,
            }]
        );
    }

    #[test]
    fn inline_cache_property_load_index_missing_observation_does_not_derive_guard_plan() {
        let mut observation = own_missing_property_observation();
        observation.key = CacheKey::Property(PropertyKey::from_index(
            PropertyIndex::from_canonical_index(0),
        ));
        observation.readiness = observation.classify_readiness();

        assert_eq!(
            plan_property_load_guard_plan_from_observation(&observation),
            Ok(None)
        );
    }

    #[test]
    fn inline_cache_property_load_symbol_and_private_missing_observations_do_not_derive_guard_plan()
    {
        for key in [
            PropertyKey::from_symbol_uid(SymbolUid::from_table_slot(71)),
            PropertyKey::from_private_name(PrivateName::from_symbol_uid(
                SymbolUid::from_table_slot(72),
            )),
        ] {
            let mut observation = own_missing_property_observation();
            observation.key = CacheKey::Property(key);
            observation.readiness = observation.classify_readiness();

            assert_eq!(
                plan_property_load_guard_plan_from_observation(&observation),
                Ok(None)
            );
        }
    }

    #[test]
    fn inline_cache_property_load_missing_through_prototype_derives_negative_guard_plan_only() {
        let mut observation = own_missing_property_observation();
        let base = observation.base_object.unwrap();
        let first_prototype = ObjectId(CellId(52));
        let terminal = ObjectId(CellId(53));
        observation.prototype_depth = 2;
        observation.chain = vec![
            PropertyLoadObservationChainEntry {
                object: base,
                structure: observation.base_structure.unwrap(),
                next_prototype: Some(first_prototype),
            },
            PropertyLoadObservationChainEntry {
                object: first_prototype,
                structure: StructureId(10),
                next_prototype: Some(terminal),
            },
            PropertyLoadObservationChainEntry {
                object: terminal,
                structure: StructureId(11),
                next_prototype: None,
            },
        ];
        observation.readiness = observation.classify_readiness();

        assert_eq!(
            plan_property_load_access_case_from_observation(&observation),
            Ok(None)
        );
        let plan = plan_property_load_guard_plan_from_observation(&observation)
            .unwrap()
            .expect("missing-through-prototype guard plan");

        assert_eq!(
            plan.descriptor.requirement,
            PropertyLoadGuardRequirement::NegativeLookup
        );
        assert_eq!(plan.descriptor.prototype_depth, 2);
        assert_eq!(plan.descriptor.holder_object, None);
        assert_eq!(plan.descriptor.offset, None);
        assert_eq!(
            plan.descriptor.chain.outcome,
            PropertyLoadGuardChainOutcome::Missing {
                terminal_null: true,
            }
        );
        assert_eq!(
            plan.descriptor
                .chain
                .entries
                .iter()
                .map(|entry| entry.proof)
                .collect::<Vec<_>>(),
            vec![
                PropertyLoadGuardChainEntryProof::NoOwnProperty,
                PropertyLoadGuardChainEntryProof::NoOwnProperty,
                PropertyLoadGuardChainEntryProof::NoOwnProperty,
            ]
        );
        assert_eq!(
            plan.descriptor
                .chain
                .entries
                .iter()
                .map(|entry| entry.object)
                .collect::<Vec<_>>(),
            vec![base, first_prototype, terminal]
        );
    }

    #[test]
    fn generated_guarded_property_load_probe_request_preserves_prototype_data_guard_plan() {
        let observation = prototype_data_property_observation();
        let plan = plan_property_load_guard_plan_from_observation(&observation)
            .unwrap()
            .expect("prototype-data guard plan");
        let request =
            GeneratedGuardedPropertyLoadProbeRequest::new(&plan, RuntimeValue::from_i32(7));

        assert_eq!(request.base, RuntimeValue::from_i32(7));
        assert_eq!(
            request.requirement(),
            PropertyLoadGuardRequirement::PrototypeChain
        );
        assert_eq!(request.key(), observation.key);
        assert_eq!(
            request.plan.descriptor.holder_object,
            observation.holder_object
        );
        assert_eq!(request.plan.descriptor.offset, observation.offset);
        assert_eq!(request.prototype_depth(), observation.prototype_depth);
        assert_eq!(
            request.outcome(),
            PropertyLoadGuardChainOutcome::PrototypeData {
                holder_index: 1,
                offset: observation.offset.unwrap(),
            }
        );
    }

    #[test]
    fn generated_guarded_property_load_probe_request_preserves_negative_lookup_guard_plan() {
        let observation = prototype_negative_lookup_property_observation();
        let plan = plan_property_load_guard_plan_from_observation(&observation)
            .unwrap()
            .expect("negative-lookup guard plan");
        let request =
            GeneratedGuardedPropertyLoadProbeRequest::new(&plan, RuntimeValue::from_i32(9));

        assert_eq!(
            request.requirement(),
            PropertyLoadGuardRequirement::NegativeLookup
        );
        assert_eq!(request.key(), observation.key);
        assert_eq!(request.plan.descriptor.holder_object, None);
        assert_eq!(request.plan.descriptor.offset, None);
        assert_eq!(request.prototype_depth(), observation.prototype_depth);
        assert_eq!(
            request.outcome(),
            PropertyLoadGuardChainOutcome::Missing {
                terminal_null: true,
            }
        );
    }

    #[test]
    fn generated_guarded_property_load_probe_hit_reports_destination_root_sync() {
        let cell_value = RuntimeValue::from_encoded(EncodedJsValue((0x1234 << 8) | 0x20));
        let cell_hit = GeneratedGuardedPropertyLoadProbeHit::new(cell_value);

        assert_eq!(
            cell_hit.destination_root_sync,
            GeneratedPropertyLoadDestinationRootSync::TargetedRegisterRequiredForCell
        );
        assert!(cell_hit
            .destination_root_sync
            .requires_targeted_register_sync());

        let immediate = RuntimeValue::from_i32(42);
        let immediate_hit = GeneratedGuardedPropertyLoadProbeHit::new(immediate);

        assert_eq!(
            immediate_hit.destination_root_sync,
            GeneratedPropertyLoadDestinationRootSync::NotRequiredForImmediate
        );
        assert!(!immediate_hit
            .destination_root_sync
            .requires_targeted_register_sync());
        assert_eq!(
            GeneratedGuardedPropertyLoadProbeResult::hit(immediate),
            GeneratedGuardedPropertyLoadProbeResult::Hit(immediate_hit)
        );
    }

    #[test]
    fn generated_guarded_property_load_probe_miss_metadata_records_guard_context() {
        let observation = prototype_negative_lookup_property_observation();
        let plan = plan_property_load_guard_plan_from_observation(&observation)
            .unwrap()
            .expect("negative-lookup guard plan");
        let miss = GeneratedGuardedPropertyLoadProbeMiss::new(
            GeneratedGuardedPropertyLoadProbeMissReason::GuardStructureMismatch,
            &plan,
            Some(1),
        );

        assert_eq!(
            miss.reason,
            GeneratedGuardedPropertyLoadProbeMissReason::GuardStructureMismatch
        );
        assert_eq!(
            miss.requirement,
            PropertyLoadGuardRequirement::NegativeLookup
        );
        assert_eq!(miss.key, observation.key);
        assert_eq!(miss.prototype_depth, observation.prototype_depth);
        assert_eq!(miss.chain_index, Some(1));
        assert_eq!(
            miss.outcome,
            PropertyLoadGuardChainOutcome::Missing {
                terminal_null: true,
            }
        );
        assert_eq!(
            GeneratedGuardedPropertyLoadProbeResult::miss_for_plan(
                GeneratedGuardedPropertyLoadProbeMissReason::GuardStructureMismatch,
                &plan,
                Some(1),
            ),
            GeneratedGuardedPropertyLoadProbeResult::Miss(miss)
        );
    }

    #[test]
    fn inline_cache_property_load_negative_lookup_guard_derives_only_structure_transition_dependencies(
    ) {
        let observation = prototype_negative_lookup_property_observation();
        let plan = plan_property_load_guard_plan_from_observation(&observation)
            .unwrap()
            .expect("negative-lookup guard plan");

        let dependencies = derive_property_load_guard_dependencies(&plan, |chain_index| {
            (
                WatchpointSetId(300 + chain_index as u64),
                WatchpointDependencyId(400 + chain_index as u64),
            )
        });

        assert_eq!(dependencies.len(), 3);
        for dependency in dependencies {
            assert!(matches!(
                dependency.set.owner,
                WatchpointOwner::Structure(_)
            ));
            assert!(!matches!(dependency.set.owner, WatchpointOwner::Object(_)));
            assert!(matches!(
                dependency.dependency.target,
                WatchpointTarget::StructureTransition { .. }
            ));
            assert!(!matches!(
                dependency.dependency.target,
                WatchpointTarget::PropertyReplacement { .. }
                    | WatchpointTarget::PrototypeChain { .. }
            ));
        }
    }

    #[test]
    fn inline_cache_property_load_guard_plan_skips_unsupported_observations() {
        let mut missing_through_prototype = own_missing_property_observation();
        missing_through_prototype.prototype_depth = 1;
        missing_through_prototype.readiness = missing_through_prototype.classify_readiness();

        let mut getter = property_load_observation(
            Some(AccessCaseKind::Getter),
            true,
            PropertyCacheability::Disallowed,
            Some(StructureId(8)),
            None,
            0,
        );
        getter.key = test_property_key();
        getter.readiness = getter.classify_readiness();

        let mut proxy = property_load_observation(
            Some(AccessCaseKind::ProxyObject),
            true,
            PropertyCacheability::TaintedByOpaqueObject,
            None,
            None,
            0,
        );
        proxy.key = test_property_key();
        proxy.readiness = proxy.classify_readiness();

        let dynamic_key = property_load_observation(
            Some(AccessCaseKind::Load),
            false,
            PropertyCacheability::Allowed,
            Some(StructureId(10)),
            Some(PropertyOffset::new(4)),
            1,
        );

        let mut indexed_like =
            property_load_observation(None, false, PropertyCacheability::Disallowed, None, None, 0);
        indexed_like.key = test_property_key();
        indexed_like.readiness = indexed_like.classify_readiness();

        let mut invalid_prototype_holder = prototype_data_property_observation();
        invalid_prototype_holder.holder_object = None;
        invalid_prototype_holder.readiness = invalid_prototype_holder.classify_readiness();

        let mut mismatched_prototype_depth_holder = prototype_data_property_observation();
        mismatched_prototype_depth_holder.holder_object =
            mismatched_prototype_depth_holder.base_object;
        mismatched_prototype_depth_holder.readiness =
            mismatched_prototype_depth_holder.classify_readiness();

        let mut invalid_prototype_offset = prototype_data_property_observation();
        invalid_prototype_offset.offset = Some(PropertyOffset::INVALID);
        invalid_prototype_offset.readiness = invalid_prototype_offset.classify_readiness();

        let mut malformed_chain = prototype_data_property_observation();
        malformed_chain.chain[0].next_prototype = Some(ObjectId(CellId(99)));

        let mut missing_base_structure = own_missing_property_observation();
        missing_base_structure.base_structure = None;
        missing_base_structure.readiness = missing_base_structure.classify_readiness();

        for observation in [
            missing_through_prototype,
            getter,
            proxy,
            dynamic_key,
            indexed_like,
            invalid_prototype_holder,
            mismatched_prototype_depth_holder,
            invalid_prototype_offset,
            malformed_chain,
            missing_base_structure,
        ] {
            assert_eq!(
                plan_property_load_guard_plan_from_observation(&observation),
                Ok(None)
            );
        }
    }

    #[test]
    fn property_load_access_case_plan_table_accepts_polymorphic_same_bytecode_candidates_in_order()
    {
        let mut first = ready_own_data_property_plan();
        first.bytecode_index = 21;
        let mut second = first.clone();
        second.access_case.base_structure = Some(StructureId(8));

        let table =
            PropertyLoadAccessCasePlanTable::new(first.owner, vec![first.clone(), second.clone()])
                .expect("valid same-bytecode polymorphic table");

        assert_eq!(table.owner(), first.owner);
        assert_eq!(table.plans(), &[first.clone(), second.clone()]);
        let candidates = table.candidates_for_bytecode_index(21).collect::<Vec<_>>();
        assert_eq!(candidates, vec![&first, &second]);
    }

    #[test]
    fn property_load_access_case_plan_table_newest_first_filters_without_reordering_storage() {
        let mut first = ready_own_data_property_plan();
        first.bytecode_index = 21;
        let mut second = first.clone();
        second.access_case.base_structure = Some(StructureId(8));
        second.access_case.offset = Some(PropertyOffset::new(4));
        let mut other_bytecode = second.clone();
        other_bytecode.bytecode_index = 22;
        other_bytecode.access_case.base_structure = Some(StructureId(9));

        let table = PropertyLoadAccessCasePlanTable::new(
            first.owner,
            vec![first.clone(), second.clone(), other_bytecode.clone()],
        )
        .expect("valid same-bytecode polymorphic table");

        assert_eq!(
            table.plans(),
            &[first.clone(), second.clone(), other_bytecode.clone()]
        );
        assert_eq!(
            table.candidates_for_bytecode_index(21).collect::<Vec<_>>(),
            vec![&first, &second]
        );
        assert_eq!(
            table
                .candidates_for_bytecode_index_newest_first(21)
                .collect::<Vec<_>>(),
            vec![&second, &first]
        );
        assert_eq!(
            table
                .candidates_for_bytecode_index_newest_first(22)
                .collect::<Vec<_>>(),
            vec![&other_bytecode]
        );
        assert!(table
            .candidates_for_bytecode_index_newest_first(1234)
            .next()
            .is_none());
    }

    #[test]
    fn generated_property_load_megamorphic_lookup_keeps_same_primary_stale_terminal() {
        let owner = CodeBlockId(CellId(940));
        let slot = InlineCacheSlotId(3);
        let bytecode_index = 17;
        let key = test_property_key();
        let structure = StructureId(971);
        let epoch = 2;
        let mut primary_entries = vec![None; 2048];
        let mut secondary_entries = vec![None; 512];
        let primary_index = generated_property_load_megamorphic_cache_primary_index(
            structure,
            key,
            primary_entries.len(),
        )
        .expect("primary index");
        let secondary_index = generated_property_load_megamorphic_cache_secondary_index(
            structure,
            key,
            secondary_entries.len(),
        )
        .expect("secondary index");
        primary_entries[primary_index] = Some(GeneratedPropertyLoadMegamorphicCacheEntry {
            key,
            base_structure: structure,
            epoch: epoch - 1,
            kind: GeneratedPropertyLoadMegamorphicCacheEntryKind::OwnData {
                offset: PropertyOffset::new(3),
            },
        });
        secondary_entries[secondary_index] = Some(GeneratedPropertyLoadMegamorphicCacheEntry {
            key,
            base_structure: structure,
            epoch,
            kind: GeneratedPropertyLoadMegamorphicCacheEntryKind::OwnData {
                offset: PropertyOffset::new(4),
            },
        });
        let table = GeneratedPropertyLoadMegamorphicCandidateTable::new(
            owner,
            epoch,
            vec![GeneratedPropertyLoadMegamorphicSite {
                owner,
                slot,
                bytecode_index,
                key,
            }],
            primary_entries,
            secondary_entries,
        )
        .expect("megamorphic table");

        assert_eq!(
            table.lookup(slot, bytecode_index, key, structure),
            GeneratedPropertyLoadMegamorphicLookup::Miss
        );
    }

    #[test]
    fn generated_property_load_megamorphic_lookup_uses_secondary_after_primary_key_mismatch() {
        let owner = CodeBlockId(CellId(941));
        let slot = InlineCacheSlotId(3);
        let bytecode_index = 17;
        let key = test_property_key();
        let structure = StructureId(972);
        let epoch = 2;
        let mut primary_entries = vec![None; 2048];
        let mut secondary_entries = vec![None; 512];
        let primary_index = generated_property_load_megamorphic_cache_primary_index(
            structure,
            key,
            primary_entries.len(),
        )
        .expect("primary index");
        let secondary_index = generated_property_load_megamorphic_cache_secondary_index(
            structure,
            key,
            secondary_entries.len(),
        )
        .expect("secondary index");
        primary_entries[primary_index] = Some(GeneratedPropertyLoadMegamorphicCacheEntry {
            key,
            base_structure: StructureId(973),
            epoch,
            kind: GeneratedPropertyLoadMegamorphicCacheEntryKind::OwnData {
                offset: PropertyOffset::new(3),
            },
        });
        secondary_entries[secondary_index] = Some(GeneratedPropertyLoadMegamorphicCacheEntry {
            key,
            base_structure: structure,
            epoch,
            kind: GeneratedPropertyLoadMegamorphicCacheEntryKind::OwnData {
                offset: PropertyOffset::new(4),
            },
        });
        let table = GeneratedPropertyLoadMegamorphicCandidateTable::new(
            owner,
            epoch,
            vec![GeneratedPropertyLoadMegamorphicSite {
                owner,
                slot,
                bytecode_index,
                key,
            }],
            primary_entries,
            secondary_entries,
        )
        .expect("megamorphic table");

        let GeneratedPropertyLoadMegamorphicLookup::Hit(plan) =
            table.lookup(slot, bytecode_index, key, structure)
        else {
            panic!("expected secondary megamorphic hit");
        };
        assert_eq!(plan.access_case.base_structure, Some(structure));
        assert_eq!(plan.access_case.offset, Some(PropertyOffset::new(4)));
    }

    #[test]
    fn generated_property_store_megamorphic_lookup_uses_secondary_after_primary_key_mismatch() {
        let owner = CodeBlockId(CellId(944));
        let slot = InlineCacheSlotId(3);
        let bytecode_index = 17;
        let key = test_property_key();
        let structure = StructureId(982);
        let epoch = 2;
        let mut primary_entries = vec![None; 2048];
        let mut secondary_entries = vec![None; 512];
        let primary_index = generated_property_store_megamorphic_cache_primary_index(
            structure,
            key,
            primary_entries.len(),
        )
        .expect("primary index");
        let secondary_index = generated_property_store_megamorphic_cache_secondary_index(
            structure,
            key,
            secondary_entries.len(),
        )
        .expect("secondary index");
        primary_entries[primary_index] = Some(GeneratedPropertyStoreMegamorphicCacheEntry {
            key,
            old_structure: StructureId(983),
            new_structure: StructureId(983),
            epoch,
            offset: PropertyOffset::new(3),
            reallocating: false,
        });
        secondary_entries[secondary_index] = Some(GeneratedPropertyStoreMegamorphicCacheEntry {
            key,
            old_structure: structure,
            new_structure: structure,
            epoch,
            offset: PropertyOffset::new(4),
            reallocating: false,
        });
        let table = GeneratedPropertyStoreMegamorphicCandidateTable::new(
            owner,
            epoch,
            vec![GeneratedPropertyStoreMegamorphicSite {
                owner,
                slot,
                bytecode_index,
                key,
            }],
            primary_entries,
            secondary_entries,
        )
        .expect("store megamorphic table");

        let GeneratedPropertyStoreMegamorphicLookup::Hit(plan) =
            table.lookup(slot, bytecode_index, key, structure)
        else {
            panic!("expected secondary megamorphic store hit");
        };
        assert_eq!(
            plan.plan_kind,
            PropertyStoreAccessCasePlanKind::DataOnlyReplace
        );
        assert_eq!(plan.access_case.base_structure, Some(structure));
        assert_eq!(plan.access_case.offset, Some(PropertyOffset::new(4)));
    }

    #[test]
    fn generated_property_has_megamorphic_lookup_keeps_same_primary_stale_terminal() {
        let owner = CodeBlockId(CellId(945));
        let slot = InlineCacheSlotId(3);
        let bytecode_index = 17;
        let key = test_property_key();
        let structure = StructureId(984);
        let epoch = 2;
        let mut primary_entries = vec![None; 512];
        let mut secondary_entries = vec![None; 128];
        let primary_index = generated_property_has_megamorphic_cache_primary_index(
            structure,
            key,
            primary_entries.len(),
        )
        .expect("primary index");
        let secondary_index = generated_property_has_megamorphic_cache_secondary_index(
            structure,
            key,
            secondary_entries.len(),
        )
        .expect("secondary index");
        primary_entries[primary_index] = Some(GeneratedPropertyHasMegamorphicCacheEntry {
            key,
            base_structure: structure,
            epoch: epoch - 1,
            result: false,
        });
        secondary_entries[secondary_index] = Some(GeneratedPropertyHasMegamorphicCacheEntry {
            key,
            base_structure: structure,
            epoch,
            result: true,
        });
        let table = GeneratedPropertyHasMegamorphicCandidateTable::new(
            owner,
            epoch,
            vec![GeneratedPropertyHasMegamorphicSite {
                owner,
                slot,
                bytecode_index,
                key,
            }],
            primary_entries,
            secondary_entries,
        )
        .expect("has megamorphic table");

        assert_eq!(
            table.lookup(slot, bytecode_index, key, structure),
            GeneratedPropertyHasMegamorphicLookup::Miss
        );
    }

    #[test]
    fn generated_property_has_megamorphic_lookup_uses_secondary_after_primary_key_mismatch() {
        let owner = CodeBlockId(CellId(946));
        let slot = InlineCacheSlotId(3);
        let bytecode_index = 17;
        let key = test_property_key();
        let structure = StructureId(985);
        let epoch = 2;
        let mut primary_entries = vec![None; 512];
        let mut secondary_entries = vec![None; 128];
        let primary_index = generated_property_has_megamorphic_cache_primary_index(
            structure,
            key,
            primary_entries.len(),
        )
        .expect("primary index");
        let secondary_index = generated_property_has_megamorphic_cache_secondary_index(
            structure,
            key,
            secondary_entries.len(),
        )
        .expect("secondary index");
        primary_entries[primary_index] = Some(GeneratedPropertyHasMegamorphicCacheEntry {
            key,
            base_structure: StructureId(986),
            epoch,
            result: false,
        });
        secondary_entries[secondary_index] = Some(GeneratedPropertyHasMegamorphicCacheEntry {
            key,
            base_structure: structure,
            epoch,
            result: true,
        });
        let table = GeneratedPropertyHasMegamorphicCandidateTable::new(
            owner,
            epoch,
            vec![GeneratedPropertyHasMegamorphicSite {
                owner,
                slot,
                bytecode_index,
                key,
            }],
            primary_entries,
            secondary_entries,
        )
        .expect("has megamorphic table");

        assert_eq!(
            table.lookup(slot, bytecode_index, key, structure),
            GeneratedPropertyHasMegamorphicLookup::Hit(true)
        );
    }

    #[test]
    fn generated_property_has_megamorphic_lookup_returns_cached_false_result() {
        let owner = CodeBlockId(CellId(947));
        let slot = InlineCacheSlotId(3);
        let bytecode_index = 17;
        let key = test_property_key();
        let structure = StructureId(987);
        let epoch = 2;
        let table = GeneratedPropertyHasMegamorphicCandidateTable::test_with_primary_entry(
            owner,
            epoch,
            GeneratedPropertyHasMegamorphicSite {
                owner,
                slot,
                bytecode_index,
                key,
            },
            GeneratedPropertyHasMegamorphicCacheEntry {
                key,
                base_structure: structure,
                epoch,
                result: false,
            },
        );

        assert_eq!(
            table.lookup(slot, bytecode_index, key, structure),
            GeneratedPropertyHasMegamorphicLookup::Hit(false)
        );
    }

    #[test]
    fn generated_property_load_megamorphic_lookup_returns_missing_for_null_holder_entry() {
        let owner = CodeBlockId(CellId(942));
        let slot = InlineCacheSlotId(3);
        let bytecode_index = 17;
        let key = test_property_key();
        let structure = StructureId(974);
        let epoch = 2;
        let table = GeneratedPropertyLoadMegamorphicCandidateTable::test_with_primary_entry(
            owner,
            epoch,
            GeneratedPropertyLoadMegamorphicSite {
                owner,
                slot,
                bytecode_index,
                key,
            },
            GeneratedPropertyLoadMegamorphicCacheEntry {
                key,
                base_structure: structure,
                epoch,
                kind: GeneratedPropertyLoadMegamorphicCacheEntryKind::Missing,
            },
        );

        assert_eq!(
            table.lookup(slot, bytecode_index, key, structure),
            GeneratedPropertyLoadMegamorphicLookup::Missing
        );
    }

    #[test]
    fn generated_property_load_megamorphic_lookup_returns_prototype_holder_entry() {
        let owner = CodeBlockId(CellId(943));
        let slot = InlineCacheSlotId(3);
        let bytecode_index = 17;
        let key = test_property_key();
        let structure = StructureId(975);
        let holder = ObjectId(CellId(976));
        let offset = PropertyOffset::new(6);
        let epoch = 2;
        let table = GeneratedPropertyLoadMegamorphicCandidateTable::test_with_primary_entry(
            owner,
            epoch,
            GeneratedPropertyLoadMegamorphicSite {
                owner,
                slot,
                bytecode_index,
                key,
            },
            GeneratedPropertyLoadMegamorphicCacheEntry {
                key,
                base_structure: structure,
                epoch,
                kind: GeneratedPropertyLoadMegamorphicCacheEntryKind::PrototypeData {
                    holder,
                    offset,
                },
            },
        );

        assert_eq!(
            table.lookup(slot, bytecode_index, key, structure),
            GeneratedPropertyLoadMegamorphicLookup::PrototypeData {
                key,
                base_structure: structure,
                holder,
                offset,
            }
        );
    }

    #[test]
    fn property_load_access_case_plan_table_rejects_exact_duplicate_candidates() {
        let plan = ready_own_data_property_plan();

        assert_eq!(
            PropertyLoadAccessCasePlanTable::new(plan.owner, vec![plan.clone(), plan.clone()]),
            Err(InlineCacheValidationError::PropertyLoadPlanDuplicate {
                bytecode_index: plan.bytecode_index,
                base_structure: plan.access_case.base_structure.unwrap(),
                offset: plan.access_case.offset.unwrap(),
                key: plan.key,
            })
        );
    }

    #[test]
    fn property_load_access_case_plan_table_rejects_owner_mismatch() {
        let plan = ready_own_data_property_plan();
        let expected_owner = CodeBlockId(CellId(99));

        assert_eq!(
            PropertyLoadAccessCasePlanTable::new(expected_owner, vec![plan.clone()]),
            Err(InlineCacheValidationError::PropertyLoadPlanOwnerMismatch {
                expected: expected_owner,
                actual: plan.owner,
            })
        );
    }

    #[test]
    fn property_load_guarded_candidate_table_accepts_prototype_data_and_negative_lookup_candidates()
    {
        let prototype = guarded_candidate(
            prototype_data_guard_plan(),
            PropertyLoadGuardedCandidateKind::PrototypeData,
            1,
        );
        let mut negative = guarded_candidate(
            negative_lookup_guard_plan(),
            PropertyLoadGuardedCandidateKind::NegativeLookup,
            2,
        );
        negative.plan.bytecode_index = 99;

        let table = PropertyLoadGuardedCandidateTable::new(
            prototype.plan.owner,
            vec![prototype.clone(), negative.clone()],
        )
        .expect("valid guarded candidate table");

        assert_eq!(table.owner(), prototype.plan.owner);
        assert_eq!(table.len(), 2);
        assert_eq!(table.candidates(), &[prototype.clone(), negative.clone()]);
        assert_eq!(
            table
                .candidates_for_bytecode_index(prototype.plan.bytecode_index)
                .collect::<Vec<_>>(),
            vec![&prototype]
        );
        assert_eq!(
            table
                .candidates_for_bytecode_index(negative.plan.bytecode_index)
                .collect::<Vec<_>>(),
            vec![&negative]
        );
        assert!(table.candidates_for_bytecode_index(1234).next().is_none());
    }

    #[test]
    fn property_load_guarded_candidate_table_rejects_malformed_requirement_and_outcome() {
        let mut wrong_kind = guarded_candidate(
            prototype_data_guard_plan(),
            PropertyLoadGuardedCandidateKind::NegativeLookup,
            1,
        );
        assert_eq!(
            PropertyLoadGuardedCandidateTable::new(wrong_kind.plan.owner, vec![wrong_kind.clone()]),
            Err(
                InlineCacheValidationError::PropertyLoadGuardedCandidateUnsupportedShape {
                    candidate_kind: PropertyLoadGuardedCandidateKind::NegativeLookup,
                    requirement: PropertyLoadGuardRequirement::PrototypeChain,
                    outcome: wrong_kind.plan.descriptor.chain.outcome,
                }
            )
        );

        wrong_kind.candidate_kind = PropertyLoadGuardedCandidateKind::PrototypeData;
        wrong_kind.plan.descriptor.requirement = PropertyLoadGuardRequirement::NegativeLookup;
        assert_eq!(
            PropertyLoadGuardedCandidateTable::new(wrong_kind.plan.owner, vec![wrong_kind.clone()]),
            Err(
                InlineCacheValidationError::PropertyLoadGuardedCandidateUnsupportedShape {
                    candidate_kind: PropertyLoadGuardedCandidateKind::PrototypeData,
                    requirement: PropertyLoadGuardRequirement::NegativeLookup,
                    outcome: wrong_kind.plan.descriptor.chain.outcome,
                }
            )
        );
    }

    #[test]
    fn property_load_guarded_candidate_table_rejects_dependency_and_binding_length_mismatch() {
        let mut candidate = guarded_candidate(
            prototype_data_guard_plan(),
            PropertyLoadGuardedCandidateKind::PrototypeData,
            1,
        );
        candidate.dependency_ordinals.pop();

        assert_eq!(
            PropertyLoadGuardedCandidateTable::new(candidate.plan.owner, vec![candidate.clone()]),
            Err(
                InlineCacheValidationError::PropertyLoadGuardedCandidateDependencyBindingCountMismatch {
                    chain_length: candidate.plan.descriptor.chain.entries.len(),
                    dependency_count: candidate.dependency_ordinals.len(),
                    binding_count: candidate.binding_set_ids.len(),
                }
            )
        );
    }

    #[test]
    fn property_load_guarded_candidate_table_rejects_duplicate_guard_plan_ordinal() {
        let first = guarded_candidate(
            prototype_data_guard_plan(),
            PropertyLoadGuardedCandidateKind::PrototypeData,
            1,
        );
        let mut second = guarded_candidate(
            negative_lookup_guard_plan(),
            PropertyLoadGuardedCandidateKind::NegativeLookup,
            1,
        );
        second.materialization_ordinal = 500;

        assert_eq!(
            PropertyLoadGuardedCandidateTable::new(first.plan.owner, vec![first, second]),
            Err(
                InlineCacheValidationError::PropertyLoadGuardedCandidateDuplicateGuardPlanOrdinal(
                    1,
                )
            )
        );
    }

    #[test]
    fn property_load_guarded_candidate_table_rejects_duplicate_semantic_candidate() {
        let first = guarded_candidate(
            prototype_data_guard_plan(),
            PropertyLoadGuardedCandidateKind::PrototypeData,
            1,
        );
        let mut second = first.clone();
        second.guard_plan_ordinal = 2;
        second.materialization_ordinal = 202;
        second.dependency_ordinals = vec![3_001, 3_002];
        second.binding_set_ids = vec![WatchpointSetId(4_001), WatchpointSetId(4_002)];

        assert_eq!(
            PropertyLoadGuardedCandidateTable::new(first.plan.owner, vec![first.clone(), second]),
            Err(
                InlineCacheValidationError::PropertyLoadGuardedCandidateDuplicate {
                    bytecode_index: first.plan.bytecode_index,
                    base_structure: first.plan.descriptor.base_structure,
                    key: first.plan.descriptor.key,
                    requirement: first.plan.descriptor.requirement,
                    outcome: first.plan.descriptor.chain.outcome,
                }
            )
        );
    }

    #[test]
    fn property_load_access_case_plan_table_remains_separate_from_property_load_guarded_candidates()
    {
        let own_data = ready_own_data_property_plan();
        let guarded = guarded_candidate(
            prototype_data_guard_plan(),
            PropertyLoadGuardedCandidateKind::PrototypeData,
            1,
        );

        let own_data_table =
            PropertyLoadAccessCasePlanTable::new(own_data.owner, vec![own_data.clone()])
                .expect("own-data table");
        let guarded_table =
            PropertyLoadGuardedCandidateTable::new(guarded.plan.owner, vec![guarded.clone()])
                .expect("guarded table");

        assert_eq!(own_data_table.plans(), std::slice::from_ref(&own_data));
        assert!(own_data_table.plans()[0]
            .access_case
            .dependencies
            .is_empty());
        assert_eq!(guarded_table.candidates(), std::slice::from_ref(&guarded));
        assert_eq!(
            guarded_table.candidates()[0].plan.descriptor.requirement,
            PropertyLoadGuardRequirement::PrototypeChain
        );
    }

    #[test]
    fn property_load_access_case_plan_table_rejects_dynamic_keys_and_wrong_access_case() {
        let mut dynamic_key = ready_own_data_property_plan();
        dynamic_key.key = CacheKey::Dynamic;
        dynamic_key.access_case.key = CacheKey::Dynamic;
        assert_plan_table_error(
            dynamic_key,
            InlineCacheValidationError::PropertyLoadPlanUnsupportedKey(CacheKey::Dynamic),
        );

        let indexed_key = CacheKey::Property(PropertyKey::from_index(
            PropertyIndex::from_canonical_index(0),
        ));
        let mut index_key = ready_own_data_property_plan();
        index_key.key = indexed_key;
        index_key.access_case.key = indexed_key;
        assert_plan_table_error(
            index_key,
            InlineCacheValidationError::PropertyLoadPlanUnsupportedKey(indexed_key),
        );

        for key in [
            PropertyKey::from_symbol_uid(SymbolUid::from_table_slot(75)),
            PropertyKey::from_private_name(PrivateName::from_symbol_uid(
                SymbolUid::from_table_slot(76),
            )),
        ] {
            let cache_key = CacheKey::Property(key);
            let mut plan = ready_own_data_property_plan();
            plan.key = cache_key;
            plan.access_case.key = cache_key;
            assert_plan_table_error(
                plan,
                InlineCacheValidationError::PropertyLoadPlanUnsupportedKey(cache_key),
            );
        }

        let mut mismatched_access_key = ready_own_data_property_plan();
        mismatched_access_key.access_case.key = CacheKey::Dynamic;
        assert_plan_table_error(
            mismatched_access_key.clone(),
            InlineCacheValidationError::PropertyLoadPlanAccessCaseKeyMismatch {
                plan: mismatched_access_key.key,
                access_case: CacheKey::Dynamic,
            },
        );

        let mut getter = ready_own_data_property_plan();
        getter.access_case.kind = AccessCaseKind::Getter;
        assert_plan_table_error(
            getter,
            InlineCacheValidationError::PropertyLoadPlanUnsupportedAccessCase(
                AccessCaseKind::Getter,
            ),
        );
    }

    #[test]
    fn property_load_access_case_plan_table_rejects_invalid_structure_and_offset() {
        let mut missing_structure = ready_own_data_property_plan();
        missing_structure.access_case.base_structure = None;
        assert_plan_table_error(
            missing_structure,
            InlineCacheValidationError::PropertyLoadPlanMissingBaseStructure,
        );

        let mut invalid_structure = ready_own_data_property_plan();
        invalid_structure.access_case.base_structure = Some(StructureId::INVALID);
        assert_plan_table_error(
            invalid_structure,
            InlineCacheValidationError::PropertyLoadPlanInvalidBaseStructure(StructureId::INVALID),
        );

        let mut missing_offset = ready_own_data_property_plan();
        missing_offset.access_case.offset = None;
        assert_plan_table_error(
            missing_offset,
            InlineCacheValidationError::PropertyLoadPlanMissingOffset,
        );

        let mut invalid_offset = ready_own_data_property_plan();
        invalid_offset.access_case.offset = Some(PropertyOffset::INVALID);
        assert_plan_table_error(
            invalid_offset,
            InlineCacheValidationError::PropertyLoadPlanInvalidOffset(PropertyOffset::INVALID),
        );
    }

    #[test]
    fn property_load_access_case_plan_table_rejects_prototype_metadata_and_dependencies() {
        let holder = ObjectId(CellId(81));
        let mut with_holder = ready_own_data_property_plan();
        with_holder.access_case.holder = Some(holder);
        assert_plan_table_error(
            with_holder,
            InlineCacheValidationError::PropertyLoadPlanUnsupportedHolder(holder),
        );

        let new_structure = StructureId(82);
        let mut with_new_structure = ready_own_data_property_plan();
        with_new_structure.access_case.new_structure = Some(new_structure);
        assert_plan_table_error(
            with_new_structure,
            InlineCacheValidationError::PropertyLoadPlanUnsupportedNewStructure(new_structure),
        );

        let mut with_dependency = ready_own_data_property_plan();
        with_dependency
            .access_case
            .dependencies
            .push(WatchpointDependency {
                id: WatchpointDependencyId(83),
                strength: DependencyStrength::CompileTimeAssumption,
                target: WatchpointTarget::StructureTransition {
                    structure: StructureId(84),
                },
                generation: None,
            });
        assert_plan_table_error(
            with_dependency,
            InlineCacheValidationError::PropertyLoadPlanUnsupportedDependencies,
        );
    }

    #[test]
    fn property_load_access_case_plan_table_rejects_global_proxy_calls_stub_and_contract() {
        let mut via_global_proxy = ready_own_data_property_plan();
        via_global_proxy.access_case.via_global_proxy = true;
        assert_plan_table_error(
            via_global_proxy,
            InlineCacheValidationError::PropertyLoadPlanUnsupportedGlobalProxy,
        );

        let mut may_call_js = ready_own_data_property_plan();
        may_call_js.access_case.may_call_js = true;
        assert_plan_table_error(
            may_call_js,
            InlineCacheValidationError::PropertyLoadPlanMayCallJs,
        );

        let mut wrong_stub = ready_own_data_property_plan();
        wrong_stub.planned_stub_kind = InlineCacheStubKind::SlowPathHandler;
        assert_plan_table_error(
            wrong_stub,
            InlineCacheValidationError::PropertyLoadPlanUnsupportedStubKind(
                InlineCacheStubKind::SlowPathHandler,
            ),
        );

        let mut wrong_contract = ready_own_data_property_plan();
        wrong_contract.effect_contract = PropertyLoadAccessCasePlanContract {
            effects: PropertyLoadAccessCaseEffects::Unsupported,
            rooting: PropertyLoadAccessCaseRooting::Unsupported,
        };
        assert_plan_table_error(
            wrong_contract.clone(),
            InlineCacheValidationError::PropertyLoadPlanUnsupportedEffectContract(
                wrong_contract.effect_contract,
            ),
        );
    }

    #[test]
    fn inline_cache_property_load_plan_skips_unsupported_observations() {
        let mut prototype = ready_own_data_property_observation();
        prototype.prototype_depth = 1;
        prototype.holder_object = Some(ObjectId(CellId(52)));
        prototype.readiness = prototype.classify_readiness();

        let mut mismatched_holder = ready_own_data_property_observation();
        mismatched_holder.holder_object = Some(ObjectId(CellId(53)));

        let mut getter = property_load_observation(
            Some(AccessCaseKind::Getter),
            true,
            PropertyCacheability::Disallowed,
            Some(StructureId(8)),
            None,
            0,
        );
        getter.key = test_property_key();
        getter.readiness = getter.classify_readiness();

        let mut miss = property_load_observation(
            Some(AccessCaseKind::Miss),
            false,
            PropertyCacheability::Allowed,
            Some(StructureId(9)),
            None,
            0,
        );
        miss.key = test_property_key();
        miss.readiness = miss.classify_readiness();

        let mut proxy = property_load_observation(
            Some(AccessCaseKind::ProxyObject),
            true,
            PropertyCacheability::TaintedByOpaqueObject,
            None,
            None,
            0,
        );
        proxy.key = test_property_key();
        proxy.readiness = proxy.classify_readiness();

        let mut indexed_like =
            property_load_observation(None, false, PropertyCacheability::Disallowed, None, None, 0);
        indexed_like.key = test_property_key();
        indexed_like.readiness = indexed_like.classify_readiness();

        let mut unready = property_load_observation(
            Some(AccessCaseKind::Load),
            false,
            PropertyCacheability::Allowed,
            None,
            None,
            0,
        );
        unready.key = test_property_key();
        unready.readiness = unready.classify_readiness();

        let dynamic_key = property_load_observation(
            Some(AccessCaseKind::Load),
            false,
            PropertyCacheability::Allowed,
            Some(StructureId(10)),
            Some(PropertyOffset::new(4)),
            0,
        );

        for observation in [
            prototype,
            mismatched_holder,
            getter,
            miss,
            proxy,
            indexed_like,
            unready,
            dynamic_key,
        ] {
            assert_eq!(
                plan_property_load_access_case_from_observation(&observation),
                Ok(None)
            );
        }
    }

    #[test]
    fn inline_cache_property_load_plan_rejects_invalid_readiness() {
        let mut observation = property_load_observation(
            Some(AccessCaseKind::Load),
            false,
            PropertyCacheability::Allowed,
            None,
            None,
            0,
        );
        observation.key = test_property_key();
        observation.readiness = PropertyLoadObservationReadiness::ReadyForAttachment;

        assert_eq!(
            plan_property_load_access_case_from_observation(&observation),
            Err(
                InlineCacheValidationError::PropertyObservationReadyButBlocked(blockers(&[
                    PropertyLoadObservationBlocker::MissingBaseStructureGuard,
                    PropertyLoadObservationBlocker::MissingOffset,
                ]))
            )
        );
    }

    #[test]
    fn inline_cache_property_load_observation_blocks_prototype_data_on_chain_guard() {
        let mut observation = property_load_observation(
            Some(AccessCaseKind::Load),
            false,
            PropertyCacheability::Allowed,
            Some(StructureId(7)),
            Some(PropertyOffset::new(3)),
            1,
        );
        observation.holder_object = Some(ObjectId(CellId(52)));
        observation.readiness = observation.classify_readiness();

        assert_eq!(observation.validate(), Ok(()));
        assert_eq!(
            observation.readiness,
            PropertyLoadObservationReadiness::Blocked(blockers(&[
                PropertyLoadObservationBlocker::PrototypeChainGuardRequired,
            ]))
        );
    }

    #[test]
    fn inline_cache_property_load_observation_blocks_getter_on_call_boundary_and_cacheability() {
        let observation = property_load_observation(
            Some(AccessCaseKind::Getter),
            true,
            PropertyCacheability::Disallowed,
            Some(StructureId(8)),
            None,
            0,
        );

        assert_eq!(observation.validate(), Ok(()));
        assert_eq!(
            observation.readiness,
            PropertyLoadObservationReadiness::Blocked(blockers(&[
                PropertyLoadObservationBlocker::MayCallJsBoundary,
                PropertyLoadObservationBlocker::UncacheableResult,
            ]))
        );
    }

    #[test]
    fn inline_cache_property_load_observation_blocks_missing_on_negative_lookup_guard() {
        let observation = property_load_observation(
            Some(AccessCaseKind::Miss),
            false,
            PropertyCacheability::Allowed,
            Some(StructureId(9)),
            None,
            0,
        );

        assert_eq!(observation.validate(), Ok(()));
        assert_eq!(
            observation.readiness,
            PropertyLoadObservationReadiness::Blocked(blockers(&[
                PropertyLoadObservationBlocker::NegativeLookupGuardRequired,
            ]))
        );
    }

    #[test]
    fn inline_cache_property_load_observation_blocks_opaque_result() {
        let observation = property_load_observation(
            None,
            false,
            PropertyCacheability::TaintedByOpaqueObject,
            None,
            None,
            0,
        );

        assert_eq!(observation.validate(), Ok(()));
        assert_eq!(
            observation.readiness,
            PropertyLoadObservationReadiness::Blocked(blockers(&[
                PropertyLoadObservationBlocker::OpaqueResult,
            ]))
        );
    }

    #[test]
    fn inline_cache_property_load_observation_rejects_handoff_identity_mismatches() {
        let observation = property_load_observation(
            Some(AccessCaseKind::Load),
            false,
            PropertyCacheability::Allowed,
            None,
            None,
            0,
        );

        let mut owner_mismatch = observation.clone();
        owner_mismatch.cold_miss_handoff.owner = CodeBlockId(CellId(99));
        assert_eq!(
            owner_mismatch.validate(),
            Err(
                InlineCacheValidationError::PropertyObservationHandoffOwnerMismatch {
                    observation: observation.owner,
                    handoff: CodeBlockId(CellId(99)),
                }
            )
        );

        let mut slot_mismatch = observation.clone();
        slot_mismatch.cold_miss_handoff.slot = InlineCacheSlotId(99);
        assert_eq!(
            slot_mismatch.validate(),
            Err(
                InlineCacheValidationError::PropertyObservationHandoffSlotMismatch {
                    observation: observation.slot,
                    handoff: InlineCacheSlotId(99),
                }
            )
        );

        let mut bytecode_mismatch = observation.clone();
        bytecode_mismatch.cold_miss_handoff.bytecode_index = 99;
        assert_eq!(
            bytecode_mismatch.validate(),
            Err(
                InlineCacheValidationError::PropertyObservationHandoffBytecodeIndexMismatch {
                    observation: observation.bytecode_index,
                    handoff: 99,
                },
            )
        );

        let mut cache_kind_mismatch = observation.clone();
        cache_kind_mismatch.cold_miss_handoff.cache_kind = InlineCacheKind::ElementLoad;
        assert_eq!(
            cache_kind_mismatch.validate(),
            Err(
                InlineCacheValidationError::PropertyObservationHandoffCacheKindMismatch {
                    observation: InlineCacheKind::PropertyLoad,
                    handoff: InlineCacheKind::ElementLoad,
                }
            )
        );

        let mut fallback_mismatch = observation.clone();
        fallback_mismatch.cold_miss_handoff.fallback = InlineCacheFallbackSemantics::Disabled;
        assert_eq!(
            fallback_mismatch.validate(),
            Err(
                InlineCacheValidationError::PropertyObservationHandoffFallbackMismatch {
                    observation: InlineCacheFallbackSemantics::SlowPathLookup,
                    handoff: InlineCacheFallbackSemantics::Disabled,
                }
            )
        );
    }

    #[test]
    fn inline_cache_property_load_observation_rejects_call_boundary_and_call_link_contamination() {
        let observation = property_load_observation(
            Some(AccessCaseKind::Load),
            false,
            PropertyCacheability::Allowed,
            None,
            None,
            0,
        );

        let mut boundary_contaminated = observation.clone();
        boundary_contaminated.cold_miss_handoff.boundary = Some(CallBoundaryId(77));
        assert_eq!(
            boundary_contaminated.validate(),
            Err(InlineCacheValidationError::PropertyObservationBoundaryContamination)
        );

        let mut call_link_contaminated = observation.clone();
        call_link_contaminated.cold_miss_handoff.call_link = Some(CallLinkInfoDescriptor {
            mode: CallLinkMode::Init,
            call_kind: LinkedCallKind::Call,
            owner: None,
            executable: None,
            callee: None,
            target_code_block: None,
            boundary: None,
            slow_path_count: 0,
            max_argument_count_including_this: 0,
        });
        assert_eq!(
            call_link_contaminated.validate(),
            Err(InlineCacheValidationError::PropertyObservationCallLinkContamination)
        );
    }

    #[test]
    fn inline_cache_property_load_observation_requires_operand_register_preservation() {
        let mut observation = property_load_observation(
            Some(AccessCaseKind::Load),
            false,
            PropertyCacheability::Allowed,
            None,
            None,
            0,
        );
        observation.cold_miss_handoff.preserves_operand_registers = false;

        assert_eq!(
            observation.validate(),
            Err(InlineCacheValidationError::PropertyObservationHandoffClobbersOperandRegisters)
        );
    }

    #[test]
    fn inline_cache_property_load_observation_rejects_ready_claim_when_blockers_remain() {
        let mut observation = property_load_observation(
            Some(AccessCaseKind::Load),
            false,
            PropertyCacheability::Allowed,
            None,
            None,
            0,
        );
        observation.readiness = PropertyLoadObservationReadiness::ReadyForAttachment;

        assert_eq!(
            observation.validate(),
            Err(
                InlineCacheValidationError::PropertyObservationReadyButBlocked(blockers(&[
                    PropertyLoadObservationBlocker::MissingBaseStructureGuard,
                    PropertyLoadObservationBlocker::MissingOffset,
                ]))
            )
        );
    }
}
