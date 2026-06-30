use crate::bytecode::code_block::{
    BytecodeIndex, CallSiteIndex, CodeBlockLifecycleState, CodeBlockMutationAuthority,
    CodeSpecialization, RuntimeSlot,
};
use crate::bytecode::opcode::CoreOpcode;
use crate::bytecode::origin::CodeOrigin;
use crate::bytecode::register::VirtualRegister;
use crate::gc::StructureId;
use crate::jit::CallBoundaryId;
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
}

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
