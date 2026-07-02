use crate::bytecode::code_block::{
    BytecodeIndex, CallSiteIndex, CodeBlockLifecycleState, CodeBlockMutationAuthority,
    CodeSpecialization, RuntimeSlot,
};
use crate::bytecode::opcode::CoreOpcode;
use crate::bytecode::origin::CodeOrigin;
use crate::bytecode::register::VirtualRegister;
use crate::gc::StructureId;
// `CallLinkAttachmentPlan` lives in `crate::jit::ic` (jit/ic.rs:3779-3798) and
// is re-exported at `crate::jit` (jit/mod.rs:170-173). `bytecode/ic.rs`
// already depends on `crate::jit` for `CallBoundaryId` (below), so pulling in
// this second jit-owned IC-descriptor type via the same top-level path is the
// SAME dependency direction already established here, not a new inversion —
// see docs/design/ic-resident-provenance.md, "Open question 2".
use crate::jit::{CallBoundaryId, CallLinkAttachmentPlan};
use crate::runtime::{CodeBlockId, ExecutableId, ObjectId};
use crate::strings::PropertyKey;

/// Inline-cache state owned by a linked code block.
#[derive(Clone, Debug, Default, Eq, PartialEq)]
pub struct InlineCacheTable {
    pub property_accesses: Vec<PropertyInlineCache>,
    pub calls: Vec<CallLinkInfo>,
    pub structure_stubs: Vec<StructureStubInfo>,
    pub iteration_modes: Vec<IterationModeMetadata>,
}

impl InlineCacheTable {
    pub fn property_access_for_bytecode_index(
        &self,
        bytecode_index: BytecodeIndex,
    ) -> Option<&PropertyInlineCache> {
        self.property_accesses
            .iter()
            .find(|cache| cache.bytecode_index == bytecode_index)
    }

    pub fn property_access_slot_for_bytecode_index(
        &self,
        bytecode_index: BytecodeIndex,
    ) -> Option<(usize, &PropertyInlineCache)> {
        self.property_accesses
            .iter()
            .enumerate()
            .find(|(_, cache)| cache.bytecode_index == bytecode_index)
    }

    pub fn call_for_bytecode_index(&self, bytecode_index: BytecodeIndex) -> Option<&CallLinkInfo> {
        self.calls
            .iter()
            .find(|call| call.bytecode_index == bytecode_index)
    }

    pub fn call_slot_for_bytecode_index(
        &self,
        bytecode_index: BytecodeIndex,
    ) -> Option<(usize, &CallLinkInfo)> {
        self.calls
            .iter()
            .enumerate()
            .find(|(_, call)| call.bytecode_index == bytecode_index)
    }

    // Mutable counterparts of `call_for_bytecode_index` /
    // `call_slot_for_bytecode_index`. C++ reaches the one embedded
    // `CallLinkInfo` for a call bytecode and mutates it in place at the slow
    // path (LLIntSlowPaths.cpp:616 `linkFor` -> `setMonomorphicCallee`); these
    // give the same O(call-sites-in-this-block) reach to the site for in-place
    // `set_monomorphic_callee` / `bump_slow_path_count` / `reset_to_unlinked`.
    pub fn call_for_bytecode_index_mut(
        &mut self,
        bytecode_index: BytecodeIndex,
    ) -> Option<&mut CallLinkInfo> {
        self.calls
            .iter_mut()
            .find(|call| call.bytecode_index == bytecode_index)
    }

    pub fn call_slot_for_bytecode_index_mut(
        &mut self,
        bytecode_index: BytecodeIndex,
    ) -> Option<(usize, &mut CallLinkInfo)> {
        self.calls
            .iter_mut()
            .enumerate()
            .find(|(_, call)| call.bytecode_index == bytecode_index)
    }
}

#[derive(Clone, Debug, Eq, PartialEq)]
pub struct PropertyInlineCache {
    pub bytecode_index: BytecodeIndex,
    pub access: PropertyAccessType,
    pub kind: PropertyCacheKind,
    pub dispatch: PropertyInlineCacheDispatch,
    pub mutation_authority: InlineCacheMutationAuthority,
    pub state: InlineCacheState,
    pub base: Option<VirtualRegister>,
    pub property: PropertyCacheKey,
    pub get_by_id: Option<GetByIdModeMetadata>,
    pub put_by_id: Option<PutByIdModeMetadata>,
    pub watchpoint: Option<RuntimeSlot>,
}

impl PropertyInlineCache {
    pub fn get_by_name_load(
        bytecode_index: BytecodeIndex,
        base: VirtualRegister,
        property: PropertyKey,
    ) -> Self {
        Self {
            bytecode_index,
            access: PropertyAccessType::GetById,
            kind: PropertyCacheKind::GetById,
            dispatch: PropertyInlineCacheDispatch::Unlinked,
            mutation_authority: InlineCacheMutationAuthority::LinkedCodeBlock,
            state: InlineCacheState::Unset,
            base: Some(base),
            property: PropertyCacheKey::Key(property),
            get_by_id: Some(GetByIdModeMetadata::default()),
            put_by_id: None,
            watchpoint: None,
        }
    }

    pub fn put_by_name_store(
        bytecode_index: BytecodeIndex,
        base: VirtualRegister,
        property: PropertyKey,
    ) -> Self {
        Self {
            bytecode_index,
            access: PropertyAccessType::PutByIdSloppy,
            kind: PropertyCacheKind::PutById,
            dispatch: PropertyInlineCacheDispatch::Unlinked,
            mutation_authority: InlineCacheMutationAuthority::LinkedCodeBlock,
            state: InlineCacheState::Unset,
            base: Some(base),
            property: PropertyCacheKey::Key(property),
            get_by_id: None,
            put_by_id: Some(PutByIdModeMetadata::default()),
            watchpoint: None,
        }
    }

    pub fn get_global_object_property_load(
        bytecode_index: BytecodeIndex,
        property: PropertyKey,
    ) -> Self {
        Self {
            bytecode_index,
            access: PropertyAccessType::GetById,
            kind: PropertyCacheKind::GetById,
            dispatch: PropertyInlineCacheDispatch::Unlinked,
            mutation_authority: InlineCacheMutationAuthority::LinkedCodeBlock,
            state: InlineCacheState::Unset,
            base: None,
            property: PropertyCacheKey::Key(property),
            get_by_id: Some(GetByIdModeMetadata::default()),
            put_by_id: None,
            watchpoint: None,
        }
    }

    pub fn put_global_object_property_store(
        bytecode_index: BytecodeIndex,
        property: PropertyKey,
    ) -> Self {
        Self {
            bytecode_index,
            access: PropertyAccessType::PutByIdSloppy,
            kind: PropertyCacheKind::PutById,
            dispatch: PropertyInlineCacheDispatch::Unlinked,
            mutation_authority: InlineCacheMutationAuthority::LinkedCodeBlock,
            state: InlineCacheState::Unset,
            base: None,
            property: PropertyCacheKey::Key(property),
            get_by_id: None,
            put_by_id: Some(PutByIdModeMetadata::default()),
            watchpoint: None,
        }
    }

    pub fn get_by_value_load(
        bytecode_index: BytecodeIndex,
        base: VirtualRegister,
        property: VirtualRegister,
    ) -> Self {
        Self {
            bytecode_index,
            access: PropertyAccessType::GetByVal,
            kind: PropertyCacheKind::GetByVal,
            dispatch: PropertyInlineCacheDispatch::Unlinked,
            mutation_authority: InlineCacheMutationAuthority::LinkedCodeBlock,
            state: InlineCacheState::Unset,
            base: Some(base),
            property: PropertyCacheKey::RuntimeValue(property),
            get_by_id: None,
            put_by_id: None,
            watchpoint: None,
        }
    }

    pub fn put_by_value_store(
        bytecode_index: BytecodeIndex,
        base: VirtualRegister,
        property: VirtualRegister,
    ) -> Self {
        Self {
            bytecode_index,
            access: PropertyAccessType::PutByValSloppy,
            kind: PropertyCacheKind::PutByVal,
            dispatch: PropertyInlineCacheDispatch::Unlinked,
            mutation_authority: InlineCacheMutationAuthority::LinkedCodeBlock,
            state: InlineCacheState::Unset,
            base: Some(base),
            property: PropertyCacheKey::RuntimeValue(property),
            get_by_id: None,
            put_by_id: None,
            watchpoint: None,
        }
    }

    pub fn in_by_id_has(
        bytecode_index: BytecodeIndex,
        base: VirtualRegister,
        property: PropertyKey,
    ) -> Self {
        Self {
            bytecode_index,
            access: PropertyAccessType::InById,
            kind: PropertyCacheKind::InById,
            dispatch: PropertyInlineCacheDispatch::Unlinked,
            mutation_authority: InlineCacheMutationAuthority::LinkedCodeBlock,
            state: InlineCacheState::Unset,
            base: Some(base),
            property: PropertyCacheKey::Key(property),
            get_by_id: None,
            put_by_id: None,
            watchpoint: None,
        }
    }

    pub fn in_by_value_has(
        bytecode_index: BytecodeIndex,
        base: VirtualRegister,
        property: VirtualRegister,
    ) -> Self {
        Self {
            bytecode_index,
            access: PropertyAccessType::InByVal,
            kind: PropertyCacheKind::InByVal,
            dispatch: PropertyInlineCacheDispatch::Unlinked,
            mutation_authority: InlineCacheMutationAuthority::LinkedCodeBlock,
            state: InlineCacheState::Unset,
            base: Some(base),
            property: PropertyCacheKey::RuntimeValue(property),
            get_by_id: None,
            put_by_id: None,
            watchpoint: None,
        }
    }
}

#[derive(Clone, Copy, Debug, Eq, PartialEq, Hash)]
pub enum PropertyInlineCacheAttachmentKind {
    GetByNameOwnDataLoad,
    GetByNamePrototypeDataLoad,
    GetByNameNegativeLookup,
    PutByNameStoreReplace,
    PutByNameStoreTransition,
}

impl PropertyInlineCacheAttachmentKind {
    pub const fn access_type(self) -> PropertyAccessType {
        match self {
            Self::GetByNameOwnDataLoad
            | Self::GetByNamePrototypeDataLoad
            | Self::GetByNameNegativeLookup => PropertyAccessType::GetById,
            Self::PutByNameStoreReplace | Self::PutByNameStoreTransition => {
                PropertyAccessType::PutByIdSloppy
            }
        }
    }

    pub const fn cache_kind(self) -> PropertyCacheKind {
        match self {
            Self::GetByNameOwnDataLoad
            | Self::GetByNamePrototypeDataLoad
            | Self::GetByNameNegativeLookup => PropertyCacheKind::GetById,
            Self::PutByNameStoreReplace | Self::PutByNameStoreTransition => {
                PropertyCacheKind::PutById
            }
        }
    }

    pub const fn put_by_id_mode(self) -> Option<PutByIdMode> {
        match self {
            Self::GetByNameOwnDataLoad
            | Self::GetByNamePrototypeDataLoad
            | Self::GetByNameNegativeLookup => None,
            Self::PutByNameStoreReplace => Some(PutByIdMode::Replace),
            Self::PutByNameStoreTransition => Some(PutByIdMode::Transition),
        }
    }

    pub const fn is_store(self) -> bool {
        matches!(
            self,
            Self::PutByNameStoreReplace | Self::PutByNameStoreTransition
        )
    }

    pub const fn is_guarded_get_by_name(self) -> bool {
        matches!(
            self,
            Self::GetByNamePrototypeDataLoad | Self::GetByNameNegativeLookup
        )
    }
}

#[derive(Clone, Copy, Debug, Default, Eq, PartialEq, Hash)]
pub enum PropertyInlineCacheStubMode {
    #[default]
    MetadataOnly,
    StructureStub,
}

#[derive(Clone, Copy, Debug, Default, Eq, PartialEq, Hash)]
pub struct PropertyInlineCacheAttachmentRequirements {
    pub requires_barrier: bool,
    pub has_barrier_evidence: bool,
    pub requires_watchpoint: bool,
    pub may_call: bool,
    pub may_allocate: bool,
}

#[derive(Clone, Copy, Debug, Eq, PartialEq, Hash)]
pub struct PropertyInlineCacheAttachmentRequest {
    pub slot: usize,
    pub bytecode_index: BytecodeIndex,
    pub key: PropertyKey,
    pub attachment_kind: PropertyInlineCacheAttachmentKind,
    pub base_structure: StructureId,
    pub holder_structure: Option<StructureId>,
    pub new_structure: Option<StructureId>,
    pub offset: Option<PropertyOffset>,
    pub dispatch: PropertyInlineCacheDispatch,
    pub stub_mode: PropertyInlineCacheStubMode,
    pub requirements: PropertyInlineCacheAttachmentRequirements,
}

#[derive(Clone, Copy, Debug, Eq, PartialEq, Hash)]
pub struct PropertyInlineCacheAttachmentOutcome {
    pub slot: usize,
    pub bytecode_index: BytecodeIndex,
    pub key: PropertyKey,
    pub attachment_kind: PropertyInlineCacheAttachmentKind,
    pub state: InlineCacheState,
    pub dispatch: PropertyInlineCacheDispatch,
    pub base_structure: StructureId,
    pub holder_structure: Option<StructureId>,
    pub new_structure: Option<StructureId>,
    pub offset: Option<PropertyOffset>,
    pub stub_mode: PropertyInlineCacheStubMode,
    pub structure_stub_index: Option<usize>,
}

pub type PropertyInlineCacheAttachmentResult =
    Result<PropertyInlineCacheAttachmentOutcome, PropertyInlineCacheAttachmentError>;

#[derive(Clone, Copy, Debug, Eq, PartialEq, Hash)]
pub enum PropertyInlineCacheAttachmentError {
    InvalidMutationAuthority {
        expected: CodeBlockMutationAuthority,
        actual: CodeBlockMutationAuthority,
    },
    InvalidLifecycle {
        actual: CodeBlockLifecycleState,
    },
    InvalidSlot {
        slot: usize,
        len: usize,
    },
    BytecodeIndexMismatch {
        slot: usize,
        expected: BytecodeIndex,
        actual: BytecodeIndex,
    },
    PropertyKeyMismatch {
        slot: usize,
        expected: PropertyCacheKey,
        actual: PropertyKey,
    },
    AccessKindMismatch {
        slot: usize,
        expected_access: PropertyAccessType,
        actual_access: PropertyAccessType,
        expected_kind: PropertyCacheKind,
        actual_kind: PropertyCacheKind,
    },
    InvalidExistingState {
        slot: usize,
        expected: InlineCacheState,
        actual: InlineCacheState,
    },
    InvalidExistingDispatch {
        slot: usize,
        expected: PropertyInlineCacheDispatch,
        actual: PropertyInlineCacheDispatch,
    },
    InvalidExistingMutationAuthority {
        slot: usize,
        expected: InlineCacheMutationAuthority,
        actual: InlineCacheMutationAuthority,
    },
    MissingGetByIdMetadata {
        slot: usize,
    },
    MissingPutByIdMetadata {
        slot: usize,
    },
    InvalidRequestedDispatch {
        actual: PropertyInlineCacheDispatch,
    },
    UnsupportedStubMode {
        actual: PropertyInlineCacheStubMode,
    },
    IncompatibleNewStructure {
        attachment_kind: PropertyInlineCacheAttachmentKind,
        new_structure: Option<StructureId>,
    },
    MissingHolderStructure {
        attachment_kind: PropertyInlineCacheAttachmentKind,
    },
    UnexpectedHolderStructure {
        attachment_kind: PropertyInlineCacheAttachmentKind,
        holder_structure: StructureId,
    },
    MissingPropertyOffset {
        attachment_kind: PropertyInlineCacheAttachmentKind,
    },
    UnexpectedPropertyOffset {
        attachment_kind: PropertyInlineCacheAttachmentKind,
        offset: PropertyOffset,
    },
    InvalidPropertyOffset {
        offset: PropertyOffset,
    },
    UnexpectedBarrierRequirement {
        attachment_kind: PropertyInlineCacheAttachmentKind,
    },
    MissingStoreBarrierEvidence {
        attachment_kind: PropertyInlineCacheAttachmentKind,
    },
    WatchpointBridgeUnavailable {
        attachment_kind: PropertyInlineCacheAttachmentKind,
    },
}

#[derive(Clone, Copy, Debug, Eq, PartialEq, Hash)]
pub struct PropertyInlineCacheClearRequest {
    pub slot: usize,
    pub bytecode_index: BytecodeIndex,
    pub key: PropertyKey,
    pub attachment_kind: PropertyInlineCacheAttachmentKind,
    pub base_structure: StructureId,
    pub holder_structure: Option<StructureId>,
    pub new_structure: Option<StructureId>,
    pub offset: Option<PropertyOffset>,
    pub dispatch: PropertyInlineCacheDispatch,
    pub stub_mode: PropertyInlineCacheStubMode,
    pub structure_stub_index: Option<usize>,
}

#[derive(Clone, Copy, Debug, Eq, PartialEq, Hash)]
pub struct PropertyInlineCacheClearOutcome {
    pub slot: usize,
    pub bytecode_index: BytecodeIndex,
    pub key: PropertyKey,
    pub attachment_kind: PropertyInlineCacheAttachmentKind,
    pub state: InlineCacheState,
    pub dispatch: PropertyInlineCacheDispatch,
    pub base_structure: StructureId,
    pub holder_structure: Option<StructureId>,
    pub new_structure: Option<StructureId>,
    pub offset: Option<PropertyOffset>,
    pub stub_mode: PropertyInlineCacheStubMode,
    pub structure_stub_index: Option<usize>,
}

pub type PropertyInlineCacheClearResult =
    Result<PropertyInlineCacheClearOutcome, PropertyInlineCacheClearError>;

#[derive(Clone, Copy, Debug, Eq, PartialEq, Hash)]
pub struct StructureStubAccessCaseLinkRequest {
    pub structure_stub_index: usize,
    pub bytecode_index: BytecodeIndex,
    pub slot: usize,
    pub key: PropertyKey,
    pub attachment_kind: PropertyInlineCacheAttachmentKind,
    pub base_structure: StructureId,
    pub holder_structure: Option<StructureId>,
    pub new_structure: Option<StructureId>,
    pub offset: Option<PropertyOffset>,
    pub access_case_ref: AccessCaseRef,
}

#[derive(Clone, Copy, Debug, Eq, PartialEq, Hash)]
pub struct StructureStubAccessCaseLinkOutcome {
    pub structure_stub_index: usize,
    pub bytecode_index: BytecodeIndex,
    pub slot: usize,
    pub key: PropertyKey,
    pub attachment_kind: PropertyInlineCacheAttachmentKind,
    pub base_structure: StructureId,
    pub holder_structure: Option<StructureId>,
    pub new_structure: Option<StructureId>,
    pub offset: Option<PropertyOffset>,
    pub access_case_ref: AccessCaseRef,
    pub inserted: bool,
    pub access_case_count: usize,
}

pub type StructureStubAccessCaseLinkResult =
    Result<StructureStubAccessCaseLinkOutcome, StructureStubAccessCaseLinkError>;

#[derive(Clone, Copy, Debug, Eq, PartialEq, Hash)]
pub enum StructureStubAccessCaseLinkError {
    InvalidMutationAuthority {
        expected: CodeBlockMutationAuthority,
        actual: CodeBlockMutationAuthority,
    },
    InvalidLifecycle {
        actual: CodeBlockLifecycleState,
    },
    InvalidStructureStubIndex {
        index: usize,
        len: usize,
    },
    StructureStubMetadataMismatch {
        index: usize,
        field: StructureStubMetadataMismatchField,
    },
}

#[derive(Clone, Copy, Debug, Eq, PartialEq, Hash)]
pub struct PropertyInlineCacheAttachedMetadataRequest {
    pub slot: usize,
    pub bytecode_index: BytecodeIndex,
    pub key: PropertyKey,
    pub attachment_kind: PropertyInlineCacheAttachmentKind,
    pub base_structure: StructureId,
    pub holder_structure: Option<StructureId>,
    pub new_structure: Option<StructureId>,
    pub offset: Option<PropertyOffset>,
    pub dispatch: PropertyInlineCacheDispatch,
    pub stub_mode: PropertyInlineCacheStubMode,
}

#[derive(Clone, Copy, Debug, Eq, PartialEq, Hash)]
pub struct PropertyInlineCacheAttachedMetadata {
    pub slot: usize,
    pub bytecode_index: BytecodeIndex,
    pub key: PropertyKey,
    pub attachment_kind: PropertyInlineCacheAttachmentKind,
    pub base_structure: StructureId,
    pub holder_structure: Option<StructureId>,
    pub new_structure: Option<StructureId>,
    pub offset: Option<PropertyOffset>,
    pub dispatch: PropertyInlineCacheDispatch,
    pub stub_mode: PropertyInlineCacheStubMode,
}

#[derive(Clone, Copy, Debug, Eq, PartialEq, Hash)]
pub enum PropertyInlineCacheAttachedMetadataError {
    InvalidLifecycle {
        actual: CodeBlockLifecycleState,
    },
    InvalidSlot {
        slot: usize,
        len: usize,
    },
    BytecodeIndexMismatch {
        slot: usize,
        expected: BytecodeIndex,
        actual: BytecodeIndex,
    },
    PropertyKeyMismatch {
        slot: usize,
        expected: PropertyCacheKey,
        actual: PropertyKey,
    },
    AccessKindMismatch {
        slot: usize,
        expected_access: PropertyAccessType,
        actual_access: PropertyAccessType,
        expected_kind: PropertyCacheKind,
        actual_kind: PropertyCacheKind,
    },
    InvalidExistingState {
        slot: usize,
        expected: InlineCacheState,
        actual: InlineCacheState,
    },
    InvalidExistingDispatch {
        slot: usize,
        expected: PropertyInlineCacheDispatch,
        actual: PropertyInlineCacheDispatch,
    },
    InvalidExistingMutationAuthority {
        slot: usize,
        expected: InlineCacheMutationAuthority,
        actual: InlineCacheMutationAuthority,
    },
    MissingGetByIdMetadata {
        slot: usize,
    },
    MissingPutByIdMetadata {
        slot: usize,
    },
    InvalidRequestedDispatch {
        actual: PropertyInlineCacheDispatch,
    },
    UnsupportedStubMode {
        actual: PropertyInlineCacheStubMode,
    },
    IncompatibleNewStructure {
        attachment_kind: PropertyInlineCacheAttachmentKind,
        new_structure: Option<StructureId>,
    },
    MissingHolderStructure {
        attachment_kind: PropertyInlineCacheAttachmentKind,
    },
    UnexpectedHolderStructure {
        attachment_kind: PropertyInlineCacheAttachmentKind,
        holder_structure: StructureId,
    },
    MissingPropertyOffset {
        attachment_kind: PropertyInlineCacheAttachmentKind,
    },
    UnexpectedPropertyOffset {
        attachment_kind: PropertyInlineCacheAttachmentKind,
        offset: PropertyOffset,
    },
    InvalidPropertyOffset {
        offset: PropertyOffset,
    },
    AttachedMetadataMismatch {
        slot: usize,
        field: PropertyInlineCacheAttachedMetadataMismatchField,
    },
}

#[derive(Clone, Copy, Debug, Eq, PartialEq, Hash)]
pub enum PropertyInlineCacheAttachedMetadataMismatchField {
    GetByIdMode,
    PutByIdMode,
    BaseStructure,
    HolderStructure,
    NewStructure,
    Offset,
}

pub type PropertyInlineCacheAttachedMetadataResult =
    Result<PropertyInlineCacheAttachedMetadata, PropertyInlineCacheAttachedMetadataError>;

#[derive(Clone, Copy, Debug, Eq, PartialEq, Hash)]
pub enum PropertyInlineCacheClearError {
    InvalidMutationAuthority {
        expected: CodeBlockMutationAuthority,
        actual: CodeBlockMutationAuthority,
    },
    InvalidLifecycle {
        actual: CodeBlockLifecycleState,
    },
    InvalidSlot {
        slot: usize,
        len: usize,
    },
    BytecodeIndexMismatch {
        slot: usize,
        expected: BytecodeIndex,
        actual: BytecodeIndex,
    },
    PropertyKeyMismatch {
        slot: usize,
        expected: PropertyCacheKey,
        actual: PropertyKey,
    },
    AccessKindMismatch {
        slot: usize,
        expected_access: PropertyAccessType,
        actual_access: PropertyAccessType,
        expected_kind: PropertyCacheKind,
        actual_kind: PropertyCacheKind,
    },
    InvalidExistingState {
        slot: usize,
        expected: InlineCacheState,
        actual: InlineCacheState,
    },
    InvalidExistingDispatch {
        slot: usize,
        expected: PropertyInlineCacheDispatch,
        actual: PropertyInlineCacheDispatch,
    },
    InvalidExistingMutationAuthority {
        slot: usize,
        expected: InlineCacheMutationAuthority,
        actual: InlineCacheMutationAuthority,
    },
    MissingGetByIdMetadata {
        slot: usize,
    },
    MissingPutByIdMetadata {
        slot: usize,
    },
    InvalidRequestedDispatch {
        actual: PropertyInlineCacheDispatch,
    },
    UnsupportedStubMode {
        actual: PropertyInlineCacheStubMode,
    },
    InvalidStructureStubIndex {
        index: Option<usize>,
        len: usize,
    },
    StructureStubMetadataMismatch {
        index: usize,
        field: StructureStubMetadataMismatchField,
    },
    IncompatibleNewStructure {
        attachment_kind: PropertyInlineCacheAttachmentKind,
        new_structure: Option<StructureId>,
    },
    MissingHolderStructure {
        attachment_kind: PropertyInlineCacheAttachmentKind,
    },
    UnexpectedHolderStructure {
        attachment_kind: PropertyInlineCacheAttachmentKind,
        holder_structure: StructureId,
    },
    MissingPropertyOffset {
        attachment_kind: PropertyInlineCacheAttachmentKind,
    },
    UnexpectedPropertyOffset {
        attachment_kind: PropertyInlineCacheAttachmentKind,
        offset: PropertyOffset,
    },
    InvalidPropertyOffset {
        offset: PropertyOffset,
    },
    AttachedMetadataMismatch {
        slot: usize,
        field: PropertyInlineCacheClearMetadataMismatchField,
    },
}

#[derive(Clone, Copy, Debug, Eq, PartialEq, Hash)]
pub enum PropertyInlineCacheClearMetadataMismatchField {
    GetByIdMode,
    PutByIdMode,
    BaseStructure,
    HolderStructure,
    NewStructure,
    Offset,
}

#[derive(Clone, Copy, Debug, Eq, PartialEq, Hash)]
pub enum StructureStubMetadataMismatchField {
    BytecodeIndex,
    InlineCacheSlot,
    AttachmentKind,
    Key,
    Kind,
    CacheState,
    BaseStructure,
    HolderStructure,
    NewStructure,
    Offset,
    Requirements,
}

/// Full property IC access taxonomy from JSC's `AccessType`.
///
/// This is separate from `PropertyCacheKind`: access type preserves strictness,
/// directness, and by-id/by-val spelling for codegen and IC specialization,
/// while cache kind groups sites that share storage shape.
#[derive(Clone, Copy, Debug, Eq, PartialEq, Hash)]
#[non_exhaustive]
pub enum PropertyAccessType {
    GetById,
    GetByIdWithThis,
    GetByIdDirect,
    TryGetById,
    GetByVal,
    GetByValWithThis,
    PutByIdStrict,
    PutByIdSloppy,
    PutByIdDirectStrict,
    PutByIdDirectSloppy,
    PutByValStrict,
    PutByValSloppy,
    PutByValDirectStrict,
    PutByValDirectSloppy,
    DefinePrivateNameByVal,
    DefinePrivateNameById,
    SetPrivateNameByVal,
    SetPrivateNameById,
    InById,
    InByVal,
    HasPrivateName,
    HasPrivateBrand,
    InstanceOf,
    DeleteByIdStrict,
    DeleteByIdSloppy,
    DeleteByValStrict,
    DeleteByValSloppy,
    GetPrivateName,
    GetPrivateNameById,
    CheckPrivateBrand,
    SetPrivateBrand,
}

#[derive(Clone, Copy, Debug, Default, Eq, PartialEq, Hash)]
pub enum PropertyInlineCacheDispatch {
    #[default]
    Unlinked,
    Handler,
    Repatching,
}

#[derive(Clone, Copy, Debug, Default, Eq, PartialEq, Hash)]
pub enum InlineCacheMutationAuthority {
    #[default]
    LinkedCodeBlock,
    BaselineJit,
    DfgJit,
    FtlRepatcher,
    GcWeakVisit,
}

#[derive(Clone, Copy, Debug, Eq, PartialEq, Hash)]
pub enum PropertyCacheKind {
    GetById,
    GetByIdWithThis,
    GetByIdDirect,
    TryGetById,
    GetByVal,
    GetByValWithThis,
    PutById,
    PutByIdDirect,
    PutByVal,
    InById,
    InByVal,
    DeleteById,
    DeleteByVal,
    InstanceOf,
    PrivateName,
    PrivateBrand,
}

#[derive(Clone, Copy, Debug, Default, Eq, PartialEq, Hash)]
pub enum InlineCacheState {
    #[default]
    Unset,
    Monomorphic,
    Polymorphic,
    Megamorphic,
    Disabled,
}

/// Property key captured by a property inline cache.
///
/// String, symbol, private-name, and index identity is owned by
/// `strings::PropertyKey`. The IC may borrow that canonical identity after
/// runtime conversion has completed, or defer to a register when the site is
/// still by-value and not cacheable.
#[derive(Clone, Copy, Debug, Default, Eq, PartialEq, Hash)]
pub enum PropertyCacheKey {
    #[default]
    None,
    Key(PropertyKey),
    RuntimeValue(VirtualRegister),
}

/// LLInt get-by-id metadata variants.
#[derive(Clone, Copy, Debug, Default, Eq, PartialEq, Hash)]
pub struct GetByIdModeMetadata {
    pub mode: GetByIdMode,
    pub structure: Option<StructureId>,
    pub holder_structure: Option<StructureId>,
    pub cached_offset: Option<PropertyOffset>,
    pub cached_slot: Option<RuntimeSlot>,
    pub hit_count_for_llint_caching: u8,
}

#[derive(Clone, Copy, Debug, Default, Eq, PartialEq, Hash)]
pub enum GetByIdMode {
    ProtoLoad,
    NegativeLookup,
    #[default]
    Default,
    Unset,
    ArrayLength,
}

#[derive(Clone, Copy, Debug, Default, Eq, PartialEq, Hash)]
pub struct PutByIdModeMetadata {
    pub mode: PutByIdMode,
    pub old_structure: Option<StructureId>,
    pub new_structure: Option<StructureId>,
    pub cached_offset: Option<PropertyOffset>,
}

#[derive(Clone, Copy, Debug, Default, Eq, PartialEq, Hash)]
pub enum PutByIdMode {
    #[default]
    Default,
    Replace,
    Transition,
    Setter,
    CustomAccessor,
}

#[derive(Clone, Copy, Debug, Default, Eq, PartialEq, Hash)]
#[repr(transparent)]
pub struct PropertyOffset(pub i32);

/// Patchable structure stub metadata used by property inline caches.
///
/// R0 of `docs/design/ic-resident-provenance.md` ("§B. Property-IC
/// attachment cluster") added the `countdown`/`repatch_count`/
/// `number_of_cool_downs`/`buffering_countdown`/`buffered_structures`/
/// `ever_considered`/`took_slow_path` fields below, field-for-field from C++
/// `PropertyInlineCache` (`bytecode/PropertyInlineCache.h:463-478`, read in
/// full). They are the resident "should we even try to repatch" gate that
/// C++'s `considerRepatchingCacheImpl` (`PropertyInlineCache.h:248-342`)
/// reads/mutates in place. R2 wires them: `consider_repatching`/
/// `clear_buffered_structures` below port that method's exact arithmetic,
/// and the property-IC attach pipeline (`vm/mod.rs`,
/// `link_reserved_structure_stub_access_case_for_candidate`) now consults
/// this gate instead of the log-based `structure_stub_access_case_link_
/// attempt_exists` rejection memo it replaces.
#[derive(Clone, Debug, Eq, PartialEq)]
pub struct StructureStubInfo {
    pub bytecode_index: BytecodeIndex,
    pub inline_cache_slot: usize,
    pub attachment_kind: PropertyInlineCacheAttachmentKind,
    pub key: PropertyKey,
    pub base_structure: StructureId,
    pub holder_structure: Option<StructureId>,
    pub new_structure: Option<StructureId>,
    pub offset: Option<PropertyOffset>,
    pub requirements: PropertyInlineCacheAttachmentRequirements,
    pub kind: StructureStubKind,
    pub cache_state: InlineCacheState,
    pub code_origin: CodeOrigin,
    pub access_cases: Vec<AccessCaseRef>,
    pub reset_by_gc: bool,

    /// C++ `PropertyInlineCache::countdown` (`PropertyInlineCache.h:468`,
    /// init `{ 1 }`): repatch is considered once this hits 0; C++ patches
    /// after the first execution. Wired by `consider_repatching` (Unit R2).
    pub countdown: u8,
    /// C++ `repatchCount` (`:469`, init `{ 0 }`): saturating count of
    /// consecutive countdown-expirations, compared against
    /// `Options::repatchCountForCoolDown()` to detect over-frequent
    /// repatching. Wired by `consider_repatching` (Unit R2).
    pub repatch_count: u8,
    /// C++ `numberOfCoolDowns` (`:470`, init `{ 0 }`): exponential-backoff
    /// generation counter — each time repatching is throttled, `countdown`
    /// is reseeded via `leftShiftWithSaturation(initialCoolDownCount,
    /// numberOfCoolDowns++)`. Wired by `consider_repatching` (Unit R2).
    pub number_of_cool_downs: u8,
    /// C++ `bufferingCountdown` (`:471`, init
    /// `Options::initialRepatchBufferingCountdown()` — default `6`,
    /// `runtime/OptionsList.h:109`). Decremented once per buffered (but not
    /// yet regenerated) access-case attempt; hitting 0 forces a repatch
    /// rather than buffering indefinitely. Wired by `consider_repatching`
    /// (Unit R2).
    pub buffering_countdown: u8,
    /// C++ `m_bufferedStructures` (`:438`, guarded by
    /// `m_bufferedStructuresLock`): a BOUNDED, per-regeneration-cycle dedup
    /// set, NOT a permanent rejection memo — cleared every successful stub
    /// regeneration (`clearBufferedStructures`, `:346-357`). See
    /// docs/design/ic-resident-provenance.md, "the crux finding": JSC does
    /// not memoize permanently-rejected access-case attempts. Wired by
    /// `consider_repatching`/`clear_buffered_structures` (Unit R2).
    pub buffered_structures: PropertyInlineCacheBufferedStructures,
    /// C++ `everConsidered : 1` (`:477`, init `{ false }`): set once
    /// `considerRepatchingCacheImpl` has been called at all for this site.
    /// Wired by `consider_repatching` (Unit R2).
    pub ever_considered: bool,
    /// C++ `tookSlowPath : 1` (`:478`, init `{ false }`). Unread until Unit
    /// R3 (property-IC slow-path bookkeeping) -- `consider_repatching`
    /// itself does not read or write it in C++ either.
    #[allow(dead_code)] // wired by Unit R3
    pub took_slow_path: bool,
}

/// C++ `PropertyInlineCache::countdown`'s initial value
/// (`PropertyInlineCache.h:468`, `uint8_t countdown { 1 }`).
pub const STRUCTURE_STUB_INITIAL_COUNTDOWN: u8 = 1;
/// C++ `PropertyInlineCache::repatchCount`'s initial value (`:469`,
/// `uint8_t repatchCount { 0 }`).
pub const STRUCTURE_STUB_INITIAL_REPATCH_COUNT: u8 = 0;
/// C++ `PropertyInlineCache::numberOfCoolDowns`'s initial value (`:470`,
/// `uint8_t numberOfCoolDowns { 0 }`).
pub const STRUCTURE_STUB_INITIAL_NUMBER_OF_COOL_DOWNS: u8 = 0;
/// C++ `Options::initialRepatchBufferingCountdown()`'s default
/// (`runtime/OptionsList.h:109`: `v(Unsigned, initialRepatchBufferingCountdown,
/// 6, Normal, nullptr)`), which seeds `PropertyInlineCache::bufferingCountdown`
/// in its constructor (`PropertyInlineCache.h:363`).
pub const STRUCTURE_STUB_INITIAL_BUFFERING_COUNTDOWN: u8 = 6;
/// C++ `Options::repatchCountForCoolDown()`'s default (`runtime/OptionsList.h:106`:
/// `v(Unsigned, repatchCountForCoolDown, 8, Normal, nullptr)`), compared
/// against `repatchCount` by `considerRepatchingCacheImpl` (`:267`). Wired by
/// `consider_repatching` (Unit R2).
pub const STRUCTURE_STUB_REPATCH_COUNT_FOR_COOL_DOWN: u8 = 8;
/// C++ `Options::initialCoolDownCount()`'s default (`runtime/OptionsList.h:107`:
/// `v(Unsigned, initialCoolDownCount, 20, Normal, nullptr)`), the base value
/// left-shifted by `numberOfCoolDowns` to reseed `countdown` (`:274-277`).
/// Wired by `consider_repatching` (Unit R2).
pub const STRUCTURE_STUB_INITIAL_COOL_DOWN_COUNT: u8 = 20;

/// C++ `PropertyInlineCache::m_bufferedStructures`
/// (`PropertyInlineCache.h:438`): `Variant<monostate, Vector<StructureID>,
/// Vector<tuple<StructureID, CacheableIdentifier>>>`. Plain `StructureID`s
/// when the site HAS a fixed identifier (`if (m_identifier)`,
/// `PropertyInlineCache.h:312-314` — correcting this design doc's inverted
/// prose in `docs/design/ic-resident-provenance.md` §B, which said the
/// opposite; the C++ source is authoritative and was re-read to confirm),
/// since the identifier is then implied by the site itself and only the
/// structure varies; `(StructureId, PropertyKey)` pairs when the site is
/// keyless (no fixed identifier) and the identifier must be recorded
/// alongside each structure. A BOUNDED per-cycle dedup set — cleared on
/// every regeneration (`clearBufferedStructures`,
/// `:346-357`) — not a permanent rejection log; see
/// docs/design/ic-resident-provenance.md §B.
#[derive(Clone, Debug, Default, Eq, PartialEq)]
pub enum PropertyInlineCacheBufferedStructures {
    #[default]
    Unset,
    Structures(Vec<StructureId>),
    StructuresWithKey(Vec<(StructureId, PropertyKey)>),
}

/// C++ `WTF::leftShiftWithSaturation` (`wtf/MathExtras.h:577-586`), the
/// `uint8_t` instantiation `PropertyInlineCache::considerRepatchingCacheImpl`
/// uses to reseed `countdown` (`PropertyInlineCache.h:274-277`). C++ shifts
/// `value` after implicit promotion to `int` and treats an overflowing shift
/// as "didn't recover the original value" (also true when `shiftAmount` is
/// large enough to itself be UB in C++, which cannot be replicated safely in
/// Rust); this widens to `u32` first so every shift amount up to 255 is
/// well-defined and still saturates to `max` exactly when the C++ check
/// would have.
fn structure_stub_left_shift_with_saturation(value: u8, shift_amount: u8, max: u8) -> u8 {
    if shift_amount >= 32 {
        return max;
    }
    let widened = (value as u32) << (shift_amount as u32);
    if widened > max as u32 {
        max
    } else {
        widened as u8
    }
}

impl StructureStubInfo {
    /// Faithful port of C++
    /// `PropertyInlineCache::considerRepatchingCacheImpl`
    /// (`bytecode/PropertyInlineCache.h:248-342`, read in full; confirmed
    /// against `/Users/bytedance/Dev/WebKit/Source/JavaScriptCore/bytecode/
    /// PropertyInlineCache.h`). This is the resident "should we even attempt
    /// to repatch" gate: a per-site cooldown that escalates exponentially
    /// under repeated repatching, plus a bounded per-cycle dedup buffer so
    /// the same structure is not buffered twice in one cycle. It does NOT
    /// memoize permanently-failing candidates — see
    /// `docs/design/ic-resident-provenance.md`, "the crux finding": a
    /// candidate that keeps failing for a structural reason (not a cooldown
    /// reason) is simply reconsidered again once the cooldown/buffering
    /// windows allow it, exactly like C++.
    ///
    /// `structure` mirrors the C++ `Structure*` argument (the base object's
    /// structure being considered for a new `AccessCase`; `None` mirrors a
    /// null `Structure*`, e.g. `considerRepatchingCacheMegamorphic`'s call
    /// with `structure = nullptr`). `key` mirrors the C++ `CacheableIdentifier
    /// impl` argument, read only on the branch where the site itself has no
    /// fixed identifier (see `site_has_fixed_identifier`'s doc comment).
    ///
    /// Returns `true` when the caller should attempt to generate/attach a
    /// new `AccessCase` now (either because the cooldown expired and this is
    /// a genuinely new structure, or because repatching too often forced an
    /// immediate cool-down reseed); `false` when the site is still counting
    /// down, still buffering indefinitely, or this exact
    /// `(structure, key)` pair is already buffered this cycle.
    pub fn consider_repatching(
        &mut self,
        structure: Option<StructureId>,
        key: Option<PropertyKey>,
    ) -> bool {
        // C++ `everConsidered = true;` (`:257`), unconditional.
        self.ever_considered = true;

        // C++ `if (!countdown) { ... } countdown--; return false;` --
        // restructured here as an early return for the `countdown != 0`
        // ("else") arm so the `countdown == 0` arm's body (below) mirrors the
        // C++ `if` block directly, with every one of ITS paths returning
        // explicitly, exactly like the source.
        if self.countdown != 0 {
            self.countdown -= 1;
            return false;
        }

        // C++ `WTF::incrementWithSaturation(repatchCount);` (`:262`).
        if self.repatch_count != u8::MAX {
            self.repatch_count += 1;
        }
        if self.repatch_count > STRUCTURE_STUB_REPATCH_COUNT_FOR_COOL_DOWN {
            // C++ `:263-278`: repatching too frequently -- reset and cool
            // down for exponentially longer each time.
            self.repatch_count = 0;
            // C++ `leftShiftWithSaturation(initialCoolDownCount,
            // numberOfCoolDowns, uint8_t max - 1)` (`:274-277`): the shift
            // amount is the OLD `numberOfCoolDowns`, read before the
            // saturating increment below.
            self.countdown = structure_stub_left_shift_with_saturation(
                STRUCTURE_STUB_INITIAL_COOL_DOWN_COUNT,
                self.number_of_cool_downs,
                u8::MAX - 1,
            );
            // C++ `WTF::incrementWithSaturation(numberOfCoolDowns);` (`:278`).
            if self.number_of_cool_downs != u8::MAX {
                self.number_of_cool_downs += 1;
            }
            // C++ `bufferingCountdown = 0;` (`:281`): whatever was buffered,
            // trigger generation now.
            self.buffering_countdown = 0;
            return true;
        }

        // C++ `:285-290`: don't buffer forever.
        if self.buffering_countdown == 0 {
            return true;
        }
        // C++ `bufferingCountdown--;` (`:292`).
        self.buffering_countdown -= 1;

        // C++ `if (!structure) return true;` (`:294-295`).
        let Some(structure) = structure else {
            return true;
        };

        self.dedup_buffered_structure(structure, key)
    }

    /// C++ `if (m_identifier) ... else ...` (`PropertyInlineCache.h:312-315`):
    /// whether THIS site has a fixed identifier baked in (e.g. a `GetById`
    /// site always caches property `"foo"`), vs. a keyless site (e.g.
    /// `GetByVal`) whose identifier varies per occurrence and is supplied via
    /// the `impl`/`key` call argument instead. Every `StructureStubKind` this
    /// port currently models (`GetById`, `PutById`, `InById`, `InstanceOf`,
    /// `PrivateName`, `ModuleNamespace`, `Proxyable`) carries a mandatory
    /// `key: PropertyKey` field on `StructureStubInfo` itself (never
    /// `Option`), so this is always `true` today -- there is no keyless kind
    /// modeled yet. Kept as its own method (rather than inlined `true`) so a
    /// future keyless (`GetByVal`-shaped) kind has one call site to flip.
    fn site_has_fixed_identifier(&self) -> bool {
        true
    }

    /// C++ `:296-327` (the `Locker`/`WTF::switchOn` block): dedup
    /// `structure` (plus `key` when the site is keyless) into
    /// `buffered_structures`, constructing the correct `Variant` alternative
    /// on first use exactly like C++'s `if (std::holds_alternative<
    /// std::monostate>(m_bufferedStructures)) { ... }`. Returns whether the
    /// entry was newly added (C++ `isNewlyAdded`).
    fn dedup_buffered_structure(
        &mut self,
        structure: StructureId,
        key: Option<PropertyKey>,
    ) -> bool {
        if self.site_has_fixed_identifier() {
            if !matches!(
                self.buffered_structures,
                PropertyInlineCacheBufferedStructures::Structures(_)
            ) {
                self.buffered_structures =
                    PropertyInlineCacheBufferedStructures::Structures(Vec::new());
            }
            let PropertyInlineCacheBufferedStructures::Structures(structures) =
                &mut self.buffered_structures
            else {
                unreachable!("just constructed the Structures alternative above");
            };
            if structures.contains(&structure) {
                false
            } else {
                structures.push(structure);
                true
            }
        } else {
            // C++ `ASSERT(!m_identifier);` (`:322`) guards this arm; a keyless
            // site with no `key` supplied has nothing to dedup against, so
            // mirror the "nothing to protect" outcome of the `structure`-only
            // arm above: attempt generation.
            let Some(key) = key else {
                return true;
            };
            if !matches!(
                self.buffered_structures,
                PropertyInlineCacheBufferedStructures::StructuresWithKey(_)
            ) {
                self.buffered_structures =
                    PropertyInlineCacheBufferedStructures::StructuresWithKey(Vec::new());
            }
            let PropertyInlineCacheBufferedStructures::StructuresWithKey(structures) =
                &mut self.buffered_structures
            else {
                unreachable!("just constructed the StructuresWithKey alternative above");
            };
            if structures.iter().any(|(s, k)| *s == structure && *k == key) {
                false
            } else {
                structures.push((structure, key));
                true
            }
        }
    }

    /// C++ `PropertyInlineCache::clearBufferedStructures`
    /// (`PropertyInlineCache.h:346-357`): called after every successful stub
    /// regeneration. Per the C++ doc comment on `m_bufferedStructures`
    /// (`:434-437`), it is always safe to clear this early -- worst case is a
    /// redundant `AccessCase` that gets deduped away on the next regenerate.
    pub fn clear_buffered_structures(&mut self) {
        match &mut self.buffered_structures {
            PropertyInlineCacheBufferedStructures::Unset => {}
            PropertyInlineCacheBufferedStructures::Structures(structures) => structures.clear(),
            PropertyInlineCacheBufferedStructures::StructuresWithKey(structures) => {
                structures.clear()
            }
        }
    }
}

#[derive(Clone, Copy, Debug, Eq, PartialEq, Hash)]
pub enum StructureStubKind {
    GetById,
    PutById,
    InById,
    InstanceOf,
    PrivateName,
    ModuleNamespace,
    Proxyable,
}

#[derive(Clone, Copy, Debug, Eq, PartialEq, Hash)]
#[repr(transparent)]
pub struct AccessCaseRef(pub u64);

/// Call link metadata for data ICs, direct calls, and optimizing tiers.
///
/// R0 of `docs/design/ic-resident-provenance.md` ("§A. Call-link pipeline",
/// "Open question 2") adds `attachment_plan` and `polymorphic_variants`
/// below, additive and unwired (`#[allow(dead_code)]`) so Unit R1 can port
/// `linkFor`'s atomic attempt function against them. Zero behavior change in
/// this commit.
#[derive(Clone, Debug, Eq, PartialEq)]
pub struct CallLinkInfo {
    pub call_site: CallSiteIndex,
    pub bytecode_index: BytecodeIndex,
    pub opcode: CoreOpcode,
    pub call_type: CallType,
    pub mode: CallLinkMode,
    pub specialization: CodeSpecialization,
    pub origin: CodeOrigin,
    pub target: CallTarget,
    pub slow_path_count: u32,
    pub max_argument_count_including_this_for_varargs: u8,
    pub flags: CallLinkFlags,

    /// Ratified by the orchestrator (design doc "Open question 2", reading
    /// (a)): a plain `Option<CallLinkAttachmentPlan>` field on `CallLinkInfo`
    /// itself. C++ `CallLinkInfo` (`bytecode/CallLinkInfo.h:58+`) has no
    /// separate plan object at all — `linkFor` (`bytecode/RepatchInlines.h`)
    /// computes-and-commits in one atomic function — so this field's
    /// long-term shape is "the plan computed and consumed within the SAME
    /// attempt call," matching JSC; it is added now, additive-only, so Unit
    /// R1 can land the atomic fold against a stable field instead of
    /// changing `CallLinkInfo`'s shape mid-fold. Unread until Unit R1.
    #[allow(dead_code)] // wired by Unit R1 (attempt_call_link)
    pub attachment_plan: Option<CallLinkAttachmentPlan>,
    /// Bounded polymorphic-callee-variant list skeleton (design doc §A,
    /// "the generated-call-link sidecar's legitimate multi-candidate need",
    /// Open question 5, ratified recommendation): mirrors C++
    /// `PolymorphicCallStubRoutine::variants()` (a `CallVariantList` built by
    /// `linkPolymorphicCall`, `bytecode/Repatch.cpp`), which is capped —
    /// not logged — at `MAX_POLYMORPHIC_CALL_VARIANT_LIST_SIZE`. Held as a
    /// plain `Vec` (like JSC's own `Vector`, itself not statically bounded
    /// at the type level — `Repatch.cpp:181` enforces the cap at the call
    /// site, falling back to `setVirtualCall` when exceeded); Unit R1 is
    /// responsible for enforcing the cap when it wires
    /// `linkPolymorphicCall`'s equivalent. Unread until Unit R1.
    #[allow(dead_code)] // wired by Unit R1 (attempt_call_link)
    pub polymorphic_variants: Vec<ObjectId>,
}

/// C++ `Options::maxPolymorphicCallVariantListSize()`'s default
/// (`runtime/OptionsList.h:272`: `v(Unsigned, maxPolymorphicCallVariantListSize,
/// 8, Normal, nullptr)`), the general (non-top-tier, non-Wasm-to-JS) cap
/// `linkPolymorphicCall` (`bytecode/Repatch.cpp:172-182`) applies before
/// falling back to a virtual call. Distinct top-tier
/// (`maxPolymorphicCallVariantListSizeForTopTier`, default 5) and
/// Wasm-to-JS (`maxPolymorphicCallVariantListSizeForWasmToJS`, default 5)
/// caps exist in C++ but are out of this unit's additive scope. Unread until
/// Unit R1 (attempt_call_link).
#[allow(dead_code)] // wired by Unit R1 (attempt_call_link)
pub const MAX_POLYMORPHIC_CALL_VARIANT_LIST_SIZE: usize = 8;

impl CallLinkInfo {
    fn metadata_only_unlinked(
        call_site: CallSiteIndex,
        bytecode_index: BytecodeIndex,
        opcode: CoreOpcode,
        call_type: CallType,
        specialization: CodeSpecialization,
    ) -> Self {
        Self {
            call_site,
            bytecode_index,
            opcode,
            call_type,
            mode: CallLinkMode::Init,
            specialization,
            origin: CodeOrigin::new(bytecode_index),
            target: CallTarget::Unlinked,
            slow_path_count: 0,
            max_argument_count_including_this_for_varargs: 0,
            flags: CallLinkFlags::default(),
            attachment_plan: None,
            polymorphic_variants: Vec::new(),
        }
    }

    pub fn metadata_only_unlinked_call(
        call_site: CallSiteIndex,
        bytecode_index: BytecodeIndex,
        opcode: CoreOpcode,
    ) -> Self {
        Self::metadata_only_unlinked(
            call_site,
            bytecode_index,
            opcode,
            CallType::Call,
            CodeSpecialization::Call,
        )
    }

    pub fn metadata_only_unlinked_construct(
        call_site: CallSiteIndex,
        bytecode_index: BytecodeIndex,
        opcode: CoreOpcode,
    ) -> Self {
        Self::metadata_only_unlinked(
            call_site,
            bytecode_index,
            opcode,
            CallType::Construct,
            CodeSpecialization::Construct,
        )
    }

    // ---- C++-faithful in-place mutators/accessors for one call site ----
    //
    // These mirror the public method surface of C++ `CallLinkInfo`
    // (bytecode/CallLinkInfo.h/.cpp), which is exactly ONE per op_call/
    // op_construct/op_tail_call embedded in the owning CodeBlock's metadata and
    // mutated O(1) in place by the call slow path and by GC visitWeak. They make
    // THIS existing side-table descriptor executable in place so per-call
    // linking can collapse onto the site itself instead of the VM-global record
    // ladder (see `VmTieringIntegration`, src/vm/tiering.rs).

    /// C++ `CallLinkInfo::mode()` (bytecode/CallLinkInfo.h:278): this site's
    /// cached linking state.
    pub fn mode(&self) -> CallLinkMode {
        self.mode
    }

    /// C++ `CallLinkInfo::isLinked()` (bytecode/CallLinkInfo.h:124): a site is
    /// linked once it caches a callee (Monomorphic) or a polymorphic stub, but
    /// not while Init or Virtual.
    pub fn is_linked(&self) -> bool {
        self.mode != CallLinkMode::Init && self.mode != CallLinkMode::Virtual
    }

    /// C++ `CallLinkInfo::slowPathCount()` (bytecode/CallLinkInfo.h:254): this
    /// site's own tiering counter.
    pub fn slow_path_count(&self) -> u32 {
        self.slow_path_count
    }

    /// C++ slow-path counter bump (`addi 1, CallLinkInfo::m_slowPathCount[t2]`,
    /// llint/LowLevelInterpreter.asm:2878): each slow-path traversal of THIS
    /// call site bumps its own counter in place. Saturating stands in for the
    /// C++ `uint32_t` wrap; the counter is only ever read as a hotness
    /// threshold, so saturation is a faithful-enough safe analog.
    pub fn bump_slow_path_count(&mut self) {
        self.slow_path_count = self.slow_path_count.saturating_add(1);
    }

    /// C++ `CallLinkInfo::setMonomorphicCallee(...)`
    /// (bytecode/CallLinkInfo.cpp:134-141): cache one callee at this site and
    /// flip `mode` to Monomorphic in place. The Rust `CallTarget` bundles the
    /// callee, target CodeBlock, and entry destination that C++ holds as the
    /// separate `m_callee`/`m_codeBlock`/`m_monomorphicCallDestination` fields.
    pub fn set_monomorphic_callee(&mut self, target: CallTarget) {
        self.target = target;
        self.mode = CallLinkMode::Monomorphic;
    }

    /// C++ `CallLinkInfo::reset(VM&)` (bytecode/CallLinkInfo.cpp:258-268): clear
    /// the cached callee/stub and return the site to the unlinked Init state.
    /// `call_type`/`specialization` are re-supplied because clearing a linked
    /// site re-derives them from the owning opcode's call shape (the caller
    /// computes them via `call_link_descriptor_shape_for_opcode`).
    pub fn reset_to_unlinked(&mut self, call_type: CallType, specialization: CodeSpecialization) {
        self.call_type = call_type;
        self.mode = CallLinkMode::Init;
        self.specialization = specialization;
        self.target = CallTarget::Unlinked;
        self.slow_path_count = 0;
        self.max_argument_count_including_this_for_varargs = 0;
        self.flags = CallLinkFlags::default();
    }
}

#[derive(Clone, Debug, Eq, PartialEq)]
pub struct CallLinkInlineCacheAttachmentRequest {
    pub slot: usize,
    pub bytecode_index: BytecodeIndex,
    pub target: CallTarget,
}

#[derive(Clone, Debug, Eq, PartialEq)]
pub struct CallLinkInlineCacheAttachmentOutcome {
    pub slot: usize,
    pub bytecode_index: BytecodeIndex,
    pub call_site: CallSiteIndex,
    pub opcode: CoreOpcode,
    pub call_type: CallType,
    pub mode: CallLinkMode,
    pub specialization: CodeSpecialization,
    pub target: CallTarget,
    pub slow_path_count: u32,
    pub max_argument_count_including_this_for_varargs: u8,
}

pub type CallLinkInlineCacheAttachmentResult =
    Result<CallLinkInlineCacheAttachmentOutcome, CallLinkInlineCacheAttachmentError>;

#[derive(Clone, Debug, Eq, PartialEq)]
pub enum CallLinkInlineCacheAttachmentError {
    InvalidMutationAuthority {
        expected: CodeBlockMutationAuthority,
        actual: CodeBlockMutationAuthority,
    },
    InvalidLifecycle {
        actual: CodeBlockLifecycleState,
    },
    InvalidSlot {
        slot: usize,
        len: usize,
    },
    BytecodeIndexMismatch {
        slot: usize,
        expected: BytecodeIndex,
        actual: BytecodeIndex,
    },
    CallSiteMismatch {
        slot: usize,
        expected: CallSiteIndex,
        actual: CallSiteIndex,
    },
    InstructionDecodeFailed {
        slot: usize,
        bytecode_index: BytecodeIndex,
    },
    InstructionOpcodeUnavailable {
        slot: usize,
        bytecode_index: BytecodeIndex,
    },
    UnsupportedOpcode {
        slot: usize,
        opcode: CoreOpcode,
    },
    OpcodeMismatch {
        slot: usize,
        expected: CoreOpcode,
        actual: CoreOpcode,
    },
    InvalidRequestedCallType {
        actual: CallType,
    },
    InvalidRequestedSpecialization {
        actual: CodeSpecialization,
    },
    InvalidRequestedArgumentCount {
        actual: u8,
    },
    InvalidRequestedTarget {
        actual: CallTarget,
    },
    InvalidExistingCallType {
        slot: usize,
        expected: CallType,
        actual: CallType,
    },
    InvalidExistingSpecialization {
        slot: usize,
        expected: CodeSpecialization,
        actual: CodeSpecialization,
    },
    OriginMismatch {
        slot: usize,
        expected: CodeOrigin,
        actual: CodeOrigin,
    },
    InvalidExistingMode {
        slot: usize,
        expected: CallLinkMode,
        actual: CallLinkMode,
    },
    InvalidExistingTarget {
        slot: usize,
        actual: CallTarget,
    },
}

#[derive(Clone, Debug, Eq, PartialEq)]
pub struct CallLinkInlineCacheClearRequest {
    pub slot: usize,
    pub bytecode_index: BytecodeIndex,
    pub target: CallTarget,
}

#[derive(Clone, Debug, Eq, PartialEq)]
pub struct CallLinkInlineCacheClearOutcome {
    pub slot: usize,
    pub bytecode_index: BytecodeIndex,
    pub call_site: CallSiteIndex,
    pub opcode: CoreOpcode,
    pub call_type: CallType,
    pub mode: CallLinkMode,
    pub specialization: CodeSpecialization,
    pub target: CallTarget,
    pub slow_path_count: u32,
    pub max_argument_count_including_this_for_varargs: u8,
}

pub type CallLinkInlineCacheClearResult =
    Result<CallLinkInlineCacheClearOutcome, CallLinkInlineCacheClearError>;

#[derive(Clone, Debug, Eq, PartialEq)]
pub enum CallLinkInlineCacheClearError {
    InvalidMutationAuthority {
        expected: CodeBlockMutationAuthority,
        actual: CodeBlockMutationAuthority,
    },
    InvalidLifecycle {
        actual: CodeBlockLifecycleState,
    },
    InvalidSlot {
        slot: usize,
        len: usize,
    },
    BytecodeIndexMismatch {
        slot: usize,
        expected: BytecodeIndex,
        actual: BytecodeIndex,
    },
    CallSiteMismatch {
        slot: usize,
        expected: CallSiteIndex,
        actual: CallSiteIndex,
    },
    InstructionDecodeFailed {
        slot: usize,
        bytecode_index: BytecodeIndex,
    },
    InstructionOpcodeUnavailable {
        slot: usize,
        bytecode_index: BytecodeIndex,
    },
    UnsupportedOpcode {
        slot: usize,
        opcode: CoreOpcode,
    },
    OpcodeMismatch {
        slot: usize,
        expected: CoreOpcode,
        actual: CoreOpcode,
    },
    InvalidRequestedTarget {
        actual: CallTarget,
    },
    InvalidExistingCallType {
        slot: usize,
        expected: CallType,
        actual: CallType,
    },
    InvalidExistingSpecialization {
        slot: usize,
        expected: CodeSpecialization,
        actual: CodeSpecialization,
    },
    OriginMismatch {
        slot: usize,
        expected: CodeOrigin,
        actual: CodeOrigin,
    },
    InvalidExistingMode {
        slot: usize,
        expected: CallLinkMode,
        actual: CallLinkMode,
    },
    AttachedMetadataMismatch {
        slot: usize,
        field: CallLinkInlineCacheClearMetadataMismatchField,
    },
}

#[derive(Clone, Copy, Debug, Eq, PartialEq, Hash)]
pub enum CallLinkInlineCacheClearMetadataMismatchField {
    Callee,
    Executable,
    TargetCodeBlock,
    Boundary,
}

#[derive(Clone, Debug, Eq, PartialEq)]
pub struct CallLinkInlineCacheAttachedMetadataRequest {
    pub slot: usize,
    pub bytecode_index: BytecodeIndex,
    pub target: CallTarget,
}

#[derive(Clone, Debug, Eq, PartialEq)]
pub struct CallLinkInlineCacheAttachedMetadata {
    pub slot: usize,
    pub bytecode_index: BytecodeIndex,
    pub call_site: CallSiteIndex,
    pub opcode: CoreOpcode,
    pub call_type: CallType,
    pub mode: CallLinkMode,
    pub specialization: CodeSpecialization,
    pub target: CallTarget,
    pub slow_path_count: u32,
    pub max_argument_count_including_this_for_varargs: u8,
}

pub type CallLinkInlineCacheAttachedMetadataResult =
    Result<CallLinkInlineCacheAttachedMetadata, CallLinkInlineCacheAttachedMetadataError>;

#[derive(Clone, Debug, Eq, PartialEq)]
pub enum CallLinkInlineCacheAttachedMetadataError {
    InvalidLifecycle {
        actual: CodeBlockLifecycleState,
    },
    InvalidSlot {
        slot: usize,
        len: usize,
    },
    BytecodeIndexMismatch {
        slot: usize,
        expected: BytecodeIndex,
        actual: BytecodeIndex,
    },
    CallSiteMismatch {
        slot: usize,
        expected: CallSiteIndex,
        actual: CallSiteIndex,
    },
    InstructionDecodeFailed {
        slot: usize,
        bytecode_index: BytecodeIndex,
    },
    InstructionOpcodeUnavailable {
        slot: usize,
        bytecode_index: BytecodeIndex,
    },
    UnsupportedOpcode {
        slot: usize,
        opcode: CoreOpcode,
    },
    OpcodeMismatch {
        slot: usize,
        expected: CoreOpcode,
        actual: CoreOpcode,
    },
    InvalidRequestedCallType {
        actual: CallType,
    },
    InvalidRequestedSpecialization {
        actual: CodeSpecialization,
    },
    InvalidRequestedArgumentCount {
        actual: u8,
    },
    InvalidRequestedTarget {
        actual: CallTarget,
    },
    InvalidExistingCallType {
        slot: usize,
        expected: CallType,
        actual: CallType,
    },
    InvalidExistingSpecialization {
        slot: usize,
        expected: CodeSpecialization,
        actual: CodeSpecialization,
    },
    OriginMismatch {
        slot: usize,
        expected: CodeOrigin,
        actual: CodeOrigin,
    },
    InvalidExistingMode {
        slot: usize,
        expected: CallLinkMode,
        actual: CallLinkMode,
    },
    AttachedMetadataMismatch {
        slot: usize,
        field: CallLinkInlineCacheAttachedMetadataMismatchField,
    },
}

#[derive(Clone, Copy, Debug, Eq, PartialEq, Hash)]
pub enum CallLinkInlineCacheAttachedMetadataMismatchField {
    Callee,
    Executable,
    TargetCodeBlock,
    Boundary,
    SlowPathCount,
    MaxArgumentCountIncludingThisForVarargs,
}

#[derive(Clone, Copy, Debug, Default, Eq, PartialEq, Hash)]
pub enum CallType {
    #[default]
    None,
    Call,
    CallVarargs,
    Construct,
    ConstructVarargs,
    TailCall,
    TailCallVarargs,
    DirectCall,
    DirectConstruct,
    DirectTailCall,
}

#[derive(Clone, Copy, Debug, Default, Eq, PartialEq, Hash)]
pub enum CallLinkMode {
    #[default]
    Init,
    Monomorphic,
    Polymorphic,
    Virtual,
}

#[derive(Clone, Debug, Default, Eq, PartialEq)]
pub enum CallTarget {
    #[default]
    Unlinked,
    LastSeenCallee(RuntimeSlot),
    Monomorphic {
        callee: RuntimeSlot,
        code_block: Option<CodeBlockId>,
        entrypoint: Option<RuntimeSlot>,
    },
    MetadataOnlyMonomorphic {
        callee: ObjectId,
        executable: ExecutableId,
        code_block: CodeBlockId,
        boundary: CallBoundaryId,
    },
    PolymorphicStub(RuntimeSlot),
    DirectExecutable(ExecutableId),
    Virtual,
}

#[derive(Clone, Copy, Debug, Default, Eq, PartialEq, Hash)]
pub struct CallLinkFlags {
    pub has_seen_should_repatch: bool,
    pub has_seen_closure: bool,
    pub cleared_by_gc: bool,
    pub cleared_by_virtual: bool,
    pub uses_data_ic: bool,
}

#[derive(Clone, Copy, Debug, Default, Eq, PartialEq, Hash)]
pub struct CallSlot {
    pub callee_or_executable: Option<RuntimeSlot>,
    pub count: u32,
    pub index: u8,
    pub arity_check: ArityCheckMode,
    pub target: Option<RuntimeSlot>,
    pub code_block: Option<CodeBlockId>,
}

#[derive(Clone, Copy, Debug, Default, Eq, PartialEq, Hash)]
pub enum ArityCheckMode {
    #[default]
    MustCheckArity,
    ArityCheckNotRequired,
}

#[derive(Clone, Copy, Debug, Default, Eq, PartialEq, Hash)]
pub struct IterationModeMetadata {
    pub bytecode_index: BytecodeIndex,
    pub seen_modes: IterationModes,
}

#[derive(Clone, Copy, Debug, Default, Eq, PartialEq, Hash)]
pub struct IterationModes {
    pub generic: bool,
    pub fast_array: bool,
    pub fast_map: bool,
    pub fast_set: bool,
}

/// Fast-path fields of one baseline data IC site, read directly by generated
/// baseline code from a stable record store.
///
/// C++-to-Rust map: mirrors the inline fast-path fields of C++
/// `HandlerPropertyInlineCache`/`PropertyInlineCache` that generated baseline
/// code loads at the call site (PropertyInlineCache.h:421-422):
///   - `structure_id` <- `m_inlineAccessBaseStructureID`
///     (offsetOfInlineAccessBaseStructureID(), a `StructureID`/`u32`),
///   - `offset` <- `byIdSelfOffset` (offsetOfByIdSelfOffset(), a
///     `PropertyOffset`/`i32`).
/// These are the only two fields a `GetByIdSelf`/`PutByIdReplace` fast path
/// needs: structure-check the base, then load/store at the inline offset. The
/// full C++ `PropertyInlineCache` carries handler chains, watchpoints, and GC
/// state; this batch ports only the two inline fast-path slots because that is
/// all the upcoming `get_by_id` self-access emission consumes.
///
/// `#[repr(C)]` with these fields in this order gives a fixed 16-byte layout
/// (`structure_id` at +0, `offset` at +4, `holder_ptr` at +8) so generated code
/// can read the slots by constant displacement. `structure_id == 0` is the
/// never-matching sentinel: it mirrors a freshly created C++ IC whose
/// `m_inlineAccessBaseStructureID` is null (no structure cached yet), so the
/// inline structure check always misses and falls through to the slow path
/// until a real miss fills the record in place.
///
/// `holder_ptr` is the `offsetOfInlineHolder()` analog from the C++ prototype
/// (`CacheType::GetByIdPrototype`) DataIC fast path
/// (JITInlineCacheGenerator.cpp:158): `0` means a SELF load (no holder; the
/// receiver IS the storage base, exactly the prior 8-byte layout's behavior),
/// while a nonzero value is a raw, pinned `CoreObjectCell*` for the prototype
/// HOLDER object, baked in by the prototype-load arm so generated code loads the
/// property from the holder's storage instead of the receiver's. The pointer is
/// raw-but-pinned: `CoreObjectCell`s are `Pin<Box<_>>` and never move
/// (interpreter/mod.rs), so the baked pointer stays valid while the holder cell
/// is live. The LOAD-BEARING invariant is that the prototype's
/// `StructureTransition` watchpoint (commit 6c035d6) resets this field back to
/// SENTINEL on any prototype shape change, so generated code can never
/// dereference a stale holder; the holder's liveness for the artifact's lifetime
/// is enforced by that watchpoint plus the owning-artifact invalidation. Unlike
/// the receiver (a boxed `RuntimeValue` that the fast path unboxes with `shr 8`),
/// `holder_ptr` is already the raw cell pointer, so the prototype tail reads
/// `[holder_ptr + STORAGE_PTR_DISP]` with no unbox.
#[derive(Clone, Copy, Debug, Eq, PartialEq, Hash)]
#[repr(C)]
pub struct HandlerPropertyInlineCacheRecord {
    /// Cached base `StructureID`; `0` = never-matching sentinel (no structure).
    pub structure_id: u32,
    /// Inline self-access (own) or holder-access (prototype) `PropertyOffset`
    /// for the cached structure.
    pub offset: i32,
    /// `offsetOfInlineHolder()` analog: `0` = self load (receiver is the storage
    /// base); nonzero = raw pinned `CoreObjectCell*` of the prototype holder. A
    /// prototype record is only "armed" when `structure_id != 0` AND
    /// `holder_ptr != 0`; the writeback refuses `holder_ptr == 0` (like the
    /// `structure_id == 0` refusal) so a missing holder can never be
    /// dereferenced.
    pub holder_ptr: u64,
}

impl HandlerPropertyInlineCacheRecord {
    /// Sentinel record: `structure_id == 0`, so an inline structure check can
    /// never match. Mirrors a freshly created C++ `PropertyInlineCache` with a
    /// null `m_inlineAccessBaseStructureID`. `holder_ptr == 0` (no holder).
    pub const SENTINEL: Self = Self {
        structure_id: 0,
        offset: 0,
        holder_ptr: 0,
    };
}

impl Default for HandlerPropertyInlineCacheRecord {
    fn default() -> Self {
        Self::SENTINEL
    }
}

/// Stable per-CodeBlock baseline data-IC record store, allocated once at
/// baseline install and never reallocated.
///
/// C++-to-Rust map: mirrors C++ `BaselineJITData`
/// (BaselineJITCode.h:118-159), which a baseline `CodeBlock` owns through
/// `m_jitData` (CodeBlock.h:1002) and which generated baseline code addresses
/// via `GPRInfo::jitDataRegister` (r13 on x86_64). C++ `BaselineJITData` is a
/// `ButterflyArray<BaselineJITData, HandlerPropertyInlineCache, void*>`: the
/// `HandlerPropertyInlineCache` records live in the *leading* span at a
/// *negative* displacement from the `BaselineJITData` object pointer
/// (ButterflyArray.h:119 `leadingData() = derived - m_leadingSize`), and
/// `propertyCache(index)` reads them back-to-front as
/// `span[span.size() - index - 1]` (BaselineJITCode.h:135-138). It is allocated
/// once in `CodeBlock::setupWithUnlinkedBaselineCode`
/// (CodeBlock.cpp:800-825) and freed only when baseline code is discarded.
///
/// Permanent Rust divergence (positive-disp32 vs C++ negative leading span):
/// Rust stores the records in a plain `Box<[HandlerPropertyInlineCacheRecord]>`,
/// a contiguous *positive*-displacement array indexed front-to-back, instead of
/// the C++ negative leading-span ButterflyArray placement. Rust has no
/// `void*` trailing constant-pool span to co-allocate behind the same object,
/// and a forward `Box<[T]>` gives the same stable base address + constant
/// per-record displacement that generated code needs, with simpler safe Rust
/// ownership. Generated baseline code must therefore index this store with
/// positive `record_base + index * 8`, not the C++ `derived - (index+1)*8`.
/// The `Box` is allocated exactly once (`from_property_cache_count`) and the
/// records are mutated in place on later misses; the `Box` is never reallocated
/// so its base address stays stable for the lifetime of the baseline code,
/// matching the C++ allocate-once `m_jitData` contract.
/// The monomorphic churn cap (SQ4): after this many slow-path misses on one
/// property IC site, the DataIC slow-path bridge stops re-filling the record and
/// resets it to SENTINEL permanently, so a polymorphic/uncacheable site routes to
/// the slow path every time instead of thrashing the record between competing
/// structures. The faithful analog of `StructureStubInfo` giving up
/// (`Repatch.cpp` countdown -> `GiveUpOnDirectAccessForOptimizedCode` /
/// the megamorphic transition); the value mirrors the LLInt monomorphic
/// give-up budget (a small, fixed number of polymorphic passes).
pub const BASELINE_PROPERTY_IC_CHURN_CAP: u32 = 100;

#[derive(Clone, Debug, Eq, PartialEq)]
pub struct BaselineJitData {
    /// Property data-IC records, one per baseline `get_by_id`/`put_by_id`
    /// self-access site.
    pub property_caches: Box<[HandlerPropertyInlineCacheRecord]>,
    /// Per-site slow-path miss counter (the SQ4 churn cap; the
    /// `StructureStubInfo::countdown` analog). Parallel to `property_caches`,
    /// NOT read by generated code (the fast path only reads the 16-byte record);
    /// only the slow-path bridge increments it and consults it to decide whether
    /// to keep caching. Kept as a SEPARATE array rather than a record field so
    /// the `#[repr(C)]` 16-byte record layout the generated structure guard reads
    /// (`[record + 0]`/`[record + 4]`) is unchanged.
    pub slow_path_counts: Box<[u32]>,
}

impl BaselineJitData {
    /// Allocate the record store once with `count` zero-initialized
    /// (sentinel) records. Mirrors `BaselineJITData::create(propertyCacheSize,
    /// ...)` where `propertyCacheSize` is the count of baseline property IC
    /// sites (CodeBlock.cpp:802).
    pub fn from_property_cache_count(count: usize) -> Self {
        Self {
            property_caches: vec![HandlerPropertyInlineCacheRecord::SENTINEL; count]
                .into_boxed_slice(),
            slow_path_counts: vec![0u32; count].into_boxed_slice(),
        }
    }

    /// Number of property IC records in the store (stable after allocation).
    pub fn property_cache_count(&self) -> usize {
        self.property_caches.len()
    }

    /// Increment the slow-path miss counter for `record_index` and return whether
    /// the site is STILL eligible to cache (count below the churn cap). Once the
    /// count reaches [`BASELINE_PROPERTY_IC_CHURN_CAP`] the site is "disabled":
    /// the bridge stops re-filling and leaves the record SENTINEL so the structure
    /// guard always misses (permanent slow path). Out-of-range indices return
    /// `false` (never cache).
    pub fn note_slow_path_and_should_cache(&mut self, record_index: usize) -> bool {
        let Some(count) = self.slow_path_counts.get_mut(record_index) else {
            return false;
        };
        *count = count.saturating_add(1);
        *count < BASELINE_PROPERTY_IC_CHURN_CAP
    }

    /// Base address of the record array, the value generated baseline code
    /// seeds into `GPRInfo::jitDataRegister` (r13). Stable for the store's
    /// lifetime because the `Box` is never reallocated. For an empty store this
    /// is a non-dereferenceable dangling slice base, which generated code never
    /// reads when there are zero sites.
    pub fn record_store_base(&self) -> *const HandlerPropertyInlineCacheRecord {
        self.property_caches.as_ptr()
    }
}

#[cfg(test)]
mod baseline_jit_data_tests {
    use super::*;

    // SQ4 churn cap: each slow-path miss increments the per-site counter and the
    // site stays cacheable until the counter reaches BASELINE_PROPERTY_IC_CHURN_CAP,
    // after which it is "disabled" (the bridge stops re-filling -> permanent slow
    // path). An out-of-range index never caches.
    #[test]
    fn churn_cap_disables_a_site_after_the_cap_is_reached() {
        let mut data = BaselineJitData::from_property_cache_count(1);
        // The first (CAP - 1) misses keep the site eligible to cache...
        for miss in 1..BASELINE_PROPERTY_IC_CHURN_CAP {
            assert!(
                data.note_slow_path_and_should_cache(0),
                "miss {miss} is below the churn cap, still cacheable",
            );
        }
        // ...and the CAP-th miss disables it (and every miss after).
        assert!(
            !data.note_slow_path_and_should_cache(0),
            "the cap-th miss disables the site (count == cap)",
        );
        assert!(
            !data.note_slow_path_and_should_cache(0),
            "a disabled site stays disabled",
        );
        // An out-of-range record index never caches.
        assert!(!data.note_slow_path_and_should_cache(7));
    }
}

#[cfg(test)]
mod call_link_info_tests {
    use super::*;
    use crate::bytecode::code_block::CodeSpecialization;
    use crate::gc::CellId;
    use crate::jit::CallBoundaryId;
    use crate::runtime::{CodeBlockId, ExecutableId, ObjectId};

    fn unlinked_call(call_site: u32, offset: u32) -> CallLinkInfo {
        CallLinkInfo::metadata_only_unlinked_call(
            CallSiteIndex(call_site),
            BytecodeIndex::from_offset(offset),
            CoreOpcode::Call,
        )
    }

    fn monomorphic_target() -> CallTarget {
        // Mirrors the metadata-only monomorphic target the attach path caches.
        CallTarget::MetadataOnlyMonomorphic {
            callee: ObjectId(CellId(7)),
            executable: ExecutableId(CellId(8)),
            code_block: CodeBlockId(CellId(9)),
            boundary: CallBoundaryId(11),
        }
    }

    // C++ `CallLinkInfo` is constructed in the Init state with no cached callee
    // (CallLinkInfo.h:306 `m_mode { Mode::Init }`, .cpp setMonomorphicCallee not
    // yet run) and `isLinked()` false (CallLinkInfo.h:124).
    #[test]
    fn fresh_call_link_info_is_unlinked() {
        let call = unlinked_call(10, 10);
        assert_eq!(call.mode(), CallLinkMode::Init);
        assert!(!call.is_linked());
        assert_eq!(call.slow_path_count(), 0);
        assert_eq!(call.target, CallTarget::Unlinked);
    }

    // R0 of docs/design/ic-resident-provenance.md ("§A. Call-link pipeline",
    // "Open question 2"/"5"): the new `attachment_plan`/`polymorphic_variants`
    // fields are additive-only in this unit -- C++ `CallLinkInfo` has no plan
    // object and no cached-variant list until a call has actually been seen
    // (`CallLinkInfo.h:58+`), so a freshly constructed, never-linked site must
    // start with both empty, matching every other freshly-constructed field
    // above.
    #[test]
    fn fresh_call_link_info_has_no_attachment_plan_or_polymorphic_variants() {
        let call = unlinked_call(10, 10);
        assert_eq!(call.attachment_plan, None);
        assert!(call.polymorphic_variants.is_empty());
    }

    // C++ `Options::maxPolymorphicCallVariantListSize()`'s default
    // (`runtime/OptionsList.h:272`), the general (non-top-tier,
    // non-Wasm-to-JS) polymorphic-call-variant cap `linkPolymorphicCall`
    // (`bytecode/Repatch.cpp:172-182`) enforces.
    #[test]
    fn max_polymorphic_call_variant_list_size_matches_cxx_default() {
        assert_eq!(MAX_POLYMORPHIC_CALL_VARIANT_LIST_SIZE, 8);
    }

    // C++ `setMonomorphicCallee` (CallLinkInfo.cpp:134-141): caches the callee
    // and flips `m_mode` to Monomorphic in place; `isLinked()` then true.
    #[test]
    fn set_monomorphic_callee_links_site_in_place() {
        let mut call = unlinked_call(10, 10);
        let target = monomorphic_target();
        call.set_monomorphic_callee(target.clone());
        assert_eq!(call.mode(), CallLinkMode::Monomorphic);
        assert!(call.is_linked());
        assert_eq!(call.target, target);
        // Linking does not by itself touch the slow-path counter.
        assert_eq!(call.slow_path_count(), 0);
    }

    // C++ slow-path counter bump (LowLevelInterpreter.asm:2878): each slow-path
    // traversal increments this site's own `m_slowPathCount`; saturating stands
    // in for the C++ `uint32_t` wrap.
    #[test]
    fn bump_slow_path_count_increments_and_saturates() {
        let mut call = unlinked_call(10, 10);
        call.bump_slow_path_count();
        call.bump_slow_path_count();
        assert_eq!(call.slow_path_count(), 2);

        call.slow_path_count = u32::MAX;
        call.bump_slow_path_count();
        assert_eq!(call.slow_path_count(), u32::MAX);
    }

    // C++ `reset(VM&)` (CallLinkInfo.cpp:258-268): clears the cached callee and
    // returns the site to unlinked Init; the Rust clear path additionally
    // re-derives call_type/specialization from the owning opcode shape and
    // zeroes the per-site counters/flags.
    #[test]
    fn reset_to_unlinked_clears_link() {
        let mut call = unlinked_call(10, 10);
        call.set_monomorphic_callee(monomorphic_target());
        call.bump_slow_path_count();
        call.max_argument_count_including_this_for_varargs = 3;
        call.flags.has_seen_closure = true;

        call.reset_to_unlinked(CallType::Call, CodeSpecialization::Call);

        assert_eq!(call.mode(), CallLinkMode::Init);
        assert!(!call.is_linked());
        assert_eq!(call.call_type, CallType::Call);
        assert_eq!(call.specialization, CodeSpecialization::Call);
        assert_eq!(call.target, CallTarget::Unlinked);
        assert_eq!(call.slow_path_count(), 0);
        assert_eq!(call.max_argument_count_including_this_for_varargs, 0);
        assert_eq!(call.flags, CallLinkFlags::default());
    }

    // C++ reaches the one embedded `CallLinkInfo` for a call bytecode and
    // mutates it in place (LLIntSlowPaths.cpp:616 `linkFor`). The mutable
    // accessors give the same per-site reach by bytecode index, and the slot
    // index agrees with the immutable lookup.
    #[test]
    fn call_get_mut_reaches_and_mutates_the_addressed_site() {
        let mut table = InlineCacheTable {
            calls: vec![
                unlinked_call(10, 10),
                unlinked_call(20, 20),
                unlinked_call(30, 30),
            ],
            ..Default::default()
        };

        let (slot, _) = table
            .call_slot_for_bytecode_index(BytecodeIndex::from_offset(20))
            .expect("immutable slot lookup");
        assert_eq!(slot, 1);

        let (slot_mut, call) = table
            .call_slot_for_bytecode_index_mut(BytecodeIndex::from_offset(20))
            .expect("mutable slot lookup");
        assert_eq!(slot_mut, slot);
        call.set_monomorphic_callee(monomorphic_target());

        // The mutation persisted on exactly the addressed site, and only it.
        assert_eq!(
            table
                .call_for_bytecode_index(BytecodeIndex::from_offset(20))
                .unwrap()
                .mode(),
            CallLinkMode::Monomorphic
        );
        assert_eq!(
            table
                .call_for_bytecode_index(BytecodeIndex::from_offset(10))
                .unwrap()
                .mode(),
            CallLinkMode::Init
        );
        assert_eq!(
            table
                .call_for_bytecode_index(BytecodeIndex::from_offset(30))
                .unwrap()
                .mode(),
            CallLinkMode::Init
        );

        // The mutable single-site accessor reaches the same object.
        let direct = table
            .call_for_bytecode_index_mut(BytecodeIndex::from_offset(20))
            .expect("mutable single-site lookup");
        assert!(direct.is_linked());
    }
}

// R0 of docs/design/ic-resident-provenance.md ("§B. Property-IC attachment
// cluster"): construction/default-value pinning tests for the new
// countdown/buffering/considered fields, proving they mirror C++
// `PropertyInlineCache`'s field initializers (`PropertyInlineCache.h:363,
// 468-478`) and `Options` defaults (`runtime/OptionsList.h:106-109`), not an
// accidental Rust value. These fields are unwired in this unit (Unit R2
// wires `consider_repatching`), so the tests only pin construction/defaults,
// not the countdown arithmetic itself.
#[cfg(test)]
mod structure_stub_info_tests {
    use super::*;
    use crate::strings::PropertyIndex;

    fn fresh_structure_stub() -> StructureStubInfo {
        StructureStubInfo {
            bytecode_index: BytecodeIndex::from_offset(10),
            inline_cache_slot: 0,
            attachment_kind: PropertyInlineCacheAttachmentKind::GetByNameOwnDataLoad,
            key: PropertyKey::Index(PropertyIndex::from_canonical_index(0)),
            base_structure: StructureId::new(1),
            holder_structure: None,
            new_structure: None,
            offset: None,
            requirements: PropertyInlineCacheAttachmentRequirements::default(),
            kind: StructureStubKind::GetById,
            cache_state: InlineCacheState::Unset,
            code_origin: CodeOrigin::new(BytecodeIndex::from_offset(10)),
            access_cases: Vec::new(),
            reset_by_gc: false,
            countdown: STRUCTURE_STUB_INITIAL_COUNTDOWN,
            repatch_count: STRUCTURE_STUB_INITIAL_REPATCH_COUNT,
            number_of_cool_downs: STRUCTURE_STUB_INITIAL_NUMBER_OF_COOL_DOWNS,
            buffering_countdown: STRUCTURE_STUB_INITIAL_BUFFERING_COUNTDOWN,
            buffered_structures: PropertyInlineCacheBufferedStructures::Unset,
            ever_considered: false,
            took_slow_path: false,
        }
    }

    // C++ `PropertyInlineCache::countdown { 1 }` (`PropertyInlineCache.h:468`):
    // a fresh site patches after its first execution, not its zeroth.
    #[test]
    fn fresh_structure_stub_countdown_starts_at_one() {
        assert_eq!(fresh_structure_stub().countdown, 1);
    }

    // C++ `repatchCount { 0 }` / `numberOfCoolDowns { 0 }`
    // (`PropertyInlineCache.h:469-470`): no repatches and no cool-downs have
    // happened yet on a fresh site.
    #[test]
    fn fresh_structure_stub_repatch_and_cool_down_counters_start_at_zero() {
        let stub = fresh_structure_stub();
        assert_eq!(stub.repatch_count, 0);
        assert_eq!(stub.number_of_cool_downs, 0);
    }

    // C++ `PropertyInlineCache(...)` constructor
    // (`PropertyInlineCache.h:361-364`): `bufferingCountdown(Options::
    // initialRepatchBufferingCountdown())`, default 6
    // (`runtime/OptionsList.h:109`).
    #[test]
    fn fresh_structure_stub_buffering_countdown_matches_cxx_options_default() {
        assert_eq!(fresh_structure_stub().buffering_countdown, 6);
        assert_eq!(STRUCTURE_STUB_INITIAL_BUFFERING_COUNTDOWN, 6);
    }

    // C++ `Options::repatchCountForCoolDown()` (default 8,
    // `runtime/OptionsList.h:106`) and `Options::initialCoolDownCount()`
    // (default 20, `runtime/OptionsList.h:107`) -- the two thresholds Unit
    // R2's `consider_repatching` arithmetic will read.
    #[test]
    fn repatch_cool_down_thresholds_match_cxx_options_defaults() {
        assert_eq!(STRUCTURE_STUB_REPATCH_COUNT_FOR_COOL_DOWN, 8);
        assert_eq!(STRUCTURE_STUB_INITIAL_COOL_DOWN_COUNT, 20);
    }

    // C++ `everConsidered : 1 { false }` / `tookSlowPath : 1 { false }`
    // (`PropertyInlineCache.h:477-478`) and `m_bufferedStructures` defaulting
    // to `std::monostate` (the `Variant`'s default alternative,
    // `PropertyInlineCache.h:438`, mirrored by
    // `PropertyInlineCacheBufferedStructures::Unset`).
    #[test]
    fn fresh_structure_stub_considered_flags_and_buffered_structures_are_unset() {
        let stub = fresh_structure_stub();
        assert!(!stub.ever_considered);
        assert!(!stub.took_slow_path);
        assert_eq!(
            stub.buffered_structures,
            PropertyInlineCacheBufferedStructures::Unset
        );
        assert_eq!(
            PropertyInlineCacheBufferedStructures::default(),
            PropertyInlineCacheBufferedStructures::Unset
        );
    }

    // Unit R2: `consider_repatching`'s exact arithmetic, ported from
    // `PropertyInlineCache::considerRepatchingCacheImpl`
    // (`PropertyInlineCache.h:248-342`, read in full). A fresh stub's
    // `countdown` starts at 1 (C++: "Setting 1 for a totally clear stub,
    // we'll patch it after the first execution."), so the FIRST call is
    // always throttled; the SECOND call (countdown now 0) is where the
    // repatch/buffering/dedup decision actually runs. `repatch_count` climbs
    // by one on every `countdown == 0` call regardless of outcome; once it
    // exceeds `STRUCTURE_STUB_REPATCH_COUNT_FOR_COOL_DOWN` (8), the site
    // cools down: `repatch_count` resets to 0, `countdown` is reseeded to
    // `20 << numberOfCoolDowns` (saturating), and `numberOfCoolDowns`
    // increments. This test drives that exact 10-call sequence and pins
    // every intermediate value.
    #[test]
    fn consider_repatching_end_to_end_cool_down_escalation_matches_cxx_arithmetic() {
        let mut stub = fresh_structure_stub();
        let s1 = StructureId::new(1);

        // Call 1: countdown 1 -> 0, throttled (C++ `countdown--; return
        // false;`).
        assert!(!stub.consider_repatching(Some(s1), None));
        assert!(stub.ever_considered);
        assert_eq!(stub.countdown, 0);
        assert_eq!(stub.repatch_count, 0);

        // Call 2: countdown == 0. repatch_count 0->1 (not > 8). buffering
        // path: bufferingCountdown 6 != 0 -> 5; structure is genuinely new
        // -> buffered and `true` returned.
        assert!(stub.consider_repatching(Some(s1), None));
        assert_eq!(stub.repatch_count, 1);
        assert_eq!(stub.buffering_countdown, 5);
        assert_eq!(
            stub.buffered_structures,
            PropertyInlineCacheBufferedStructures::Structures(vec![s1])
        );

        // Calls 3-7: repatch_count climbs 2..=6 (still <= 8); bufferingCountdown
        // walks 4,3,2,1,0 while `s1` stays buffered (already-seen -> `false`
        // each time, matching C++'s `isNewlyAdded == false`).
        for expected_repatch_count in 2u8..=6 {
            assert!(!stub.consider_repatching(Some(s1), None));
            assert_eq!(stub.repatch_count, expected_repatch_count);
        }
        assert_eq!(stub.buffering_countdown, 0);

        // Call 8: repatch_count 6->7 (not > 8). bufferingCountdown is now 0,
        // so C++ "don't buffer forever" fires unconditionally: `true`, no
        // dedup performed, `s1` is untouched.
        assert!(stub.consider_repatching(Some(s1), None));
        assert_eq!(stub.repatch_count, 7);
        assert_eq!(
            stub.buffered_structures,
            PropertyInlineCacheBufferedStructures::Structures(vec![s1])
        );

        // Call 9: repatch_count 7->8; 8 is NOT > 8, so still the
        // "don't buffer forever" `true` arm, not yet the cool-down.
        assert!(stub.consider_repatching(Some(s1), None));
        assert_eq!(stub.repatch_count, 8);

        // Call 10: repatch_count 8->9; 9 > 8 triggers the cool-down:
        // repatch_count resets to 0, countdown reseeds to
        // `leftShiftWithSaturation(20, numberOfCoolDowns=0, 254) == 20`,
        // numberOfCoolDowns becomes 1, bufferingCountdown forced to 0 (was
        // already 0).
        assert!(stub.consider_repatching(Some(s1), None));
        assert_eq!(stub.repatch_count, 0);
        assert_eq!(stub.number_of_cool_downs, 1);
        assert_eq!(stub.countdown, 20);
        assert_eq!(stub.buffering_countdown, 0);
    }

    // C++ `leftShiftWithSaturation(initialCoolDownCount, numberOfCoolDowns,
    // max)` (`PropertyInlineCache.h:274-277`): the shift amount is the OLD
    // `numberOfCoolDowns`, confirmed directly (without replaying an entire
    // call sequence) by seeding a stub already mid-cool-down-cycle.
    #[test]
    fn consider_repatching_cool_down_shifts_by_prior_number_of_cool_downs() {
        let mut stub = fresh_structure_stub();
        stub.countdown = 0;
        stub.repatch_count = STRUCTURE_STUB_REPATCH_COUNT_FOR_COOL_DOWN; // 8, about to exceed
        stub.number_of_cool_downs = 3;

        assert!(stub.consider_repatching(None, None));
        assert_eq!(stub.repatch_count, 0);
        assert_eq!(stub.number_of_cool_downs, 4);
        assert_eq!(stub.countdown, 20u8 << 3); // 160
    }

    // C++ `leftShiftWithSaturation`'s saturating clamp
    // (`wtf/MathExtras.h:577-586`, max `uint8_t::max() - 1` == 254 per
    // `PropertyInlineCache.h:276`) and `WTF::incrementWithSaturation`
    // (`numberOfCoolDowns` never wraps past `u8::MAX`).
    #[test]
    fn consider_repatching_cool_down_saturates_at_the_cxx_bounds() {
        let mut stub = fresh_structure_stub();
        stub.countdown = 0;
        stub.repatch_count = STRUCTURE_STUB_REPATCH_COUNT_FOR_COOL_DOWN;
        stub.number_of_cool_downs = 10; // 20 << 10 vastly overflows u8

        assert!(stub.consider_repatching(None, None));
        assert_eq!(stub.countdown, u8::MAX - 1); // 254
        assert_eq!(stub.number_of_cool_downs, 11);

        // A `numberOfCoolDowns` already saturated at `u8::MAX` stays there
        // (`incrementWithSaturation` never wraps to 0).
        let mut saturated = fresh_structure_stub();
        saturated.countdown = 0;
        saturated.repatch_count = STRUCTURE_STUB_REPATCH_COUNT_FOR_COOL_DOWN;
        saturated.number_of_cool_downs = u8::MAX;
        assert!(saturated.consider_repatching(None, None));
        assert_eq!(saturated.number_of_cool_downs, u8::MAX);
        assert_eq!(saturated.countdown, u8::MAX - 1);
    }

    // Test category (b) from the R2 task brief: `buffered_structures` dedups
    // a structure within one buffering cycle and clears on regeneration
    // (C++ `clearBufferedStructures`, `PropertyInlineCache.h:346-357`).
    #[test]
    fn buffered_structures_dedups_within_a_cycle_and_clears_on_regenerate() {
        let mut stub = fresh_structure_stub();
        stub.countdown = 0; // skip the "first execution" throttle
        let s1 = StructureId::new(1);
        let s2 = StructureId::new(2);

        assert!(stub.consider_repatching(Some(s1), None));
        assert_eq!(
            stub.buffered_structures,
            PropertyInlineCacheBufferedStructures::Structures(vec![s1])
        );

        // Same structure again, same cycle: already buffered -> not newly
        // added.
        assert!(!stub.consider_repatching(Some(s1), None));
        assert_eq!(
            stub.buffered_structures,
            PropertyInlineCacheBufferedStructures::Structures(vec![s1])
        );

        // A different structure in the same cycle dedups independently.
        assert!(stub.consider_repatching(Some(s2), None));
        assert_eq!(
            stub.buffered_structures,
            PropertyInlineCacheBufferedStructures::Structures(vec![s1, s2])
        );

        // Regeneration clears the buffer -- a structure seen before the
        // clear is "newly added" again afterward, exactly like C++'s doc
        // comment: "if we clear it prematurely... we'll get rid of the
        // redundant ones once we regenerate."
        stub.clear_buffered_structures();
        assert_eq!(
            stub.buffered_structures,
            PropertyInlineCacheBufferedStructures::Structures(vec![])
        );
        assert!(stub.consider_repatching(Some(s1), None));
        assert_eq!(
            stub.buffered_structures,
            PropertyInlineCacheBufferedStructures::Structures(vec![s1])
        );
    }

    // Test category (c): a previously-failing candidate is throttled by the
    // decrementing cooldown counter, NOT by a permanent rejection memo --
    // the design doc's crux finding, answered the JSC way. Cooling down
    // returns `false` every time until `countdown` reaches 0, at which point
    // the SAME candidate is reconsidered again (never permanently blocked).
    #[test]
    fn consider_repatching_cool_down_throttles_then_resumes_not_permanently_blocked() {
        let mut stub = fresh_structure_stub();
        stub.countdown = 0;
        stub.repatch_count = STRUCTURE_STUB_REPATCH_COUNT_FOR_COOL_DOWN;
        assert!(stub.consider_repatching(None, None)); // triggers escalation
        assert_eq!(stub.countdown, 20);

        for _ in 0..19 {
            assert!(!stub.consider_repatching(None, None));
        }
        assert_eq!(stub.countdown, 1);
        assert!(!stub.consider_repatching(None, None));
        assert_eq!(stub.countdown, 0);

        // Cooldown expired: reconsidered again, not memoized as permanently
        // rejected.
        assert!(stub.consider_repatching(None, None));
    }
}
