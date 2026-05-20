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
    pub fn metadata_only_unlinked_call(
        call_site: CallSiteIndex,
        bytecode_index: BytecodeIndex,
        opcode: CoreOpcode,
    ) -> Self {
        Self {
            call_site,
            bytecode_index,
            opcode,
            call_type: CallType::Call,
            mode: CallLinkMode::Init,
            specialization: CodeSpecialization::Call,
            origin: CodeOrigin::new(bytecode_index),
            target: CallTarget::Unlinked,
            slow_path_count: 0,
            max_argument_count_including_this_for_varargs: 0,
            flags: CallLinkFlags::default(),
        }
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
