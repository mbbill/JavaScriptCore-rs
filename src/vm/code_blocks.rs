use std::collections::HashMap;
use std::rc::Rc;

use crate::bytecode::code_block::CodeBlockMutationError;
use crate::bytecode::ic::{
    CallLinkInlineCacheAttachedMetadata, CallLinkInlineCacheAttachedMetadataRequest,
    CallLinkInlineCacheAttachmentError, CallLinkInlineCacheAttachmentOutcome,
    CallLinkInlineCacheAttachmentRequest, CallLinkInlineCacheClearError,
    CallLinkInlineCacheClearOutcome, CallLinkInlineCacheClearRequest, CallLinkMode, CallTarget,
    PropertyInlineCacheAttachedMetadataRequest, PropertyInlineCacheAttachmentError,
    PropertyInlineCacheAttachmentOutcome, PropertyInlineCacheAttachmentRequest,
    PropertyInlineCacheClearError, PropertyInlineCacheClearOutcome,
    PropertyInlineCacheClearRequest, StructureStubAccessCaseLinkError,
    StructureStubAccessCaseLinkOutcome, StructureStubAccessCaseLinkRequest,
};
use crate::bytecode::{
    BytecodeIndex, Checkpoint, CodeBlock, CodeBlockMutationAuthority, CoreOpcode,
    ExecutableEntryCacheRecord, ExecutableEntryPublicationError, ExecutableEntryPublicationRequest,
    ExecutableEntrypoints, JitCodeSlot, ValueProfileBucketKind, ValueProfileBucketSample,
    ValueProfileJitStoreTarget, ValueProfileSampleError, VirtualRegister,
};
use crate::gc::{CellDestructionState, CellType, Heap, HeapAllocationRecord};
use crate::jit::plan::BaselineBytecodeSnapshotFingerprint;
use crate::jit::{CacheKey, InlineCacheSlotId};
use crate::runtime::CodeBlockId;
use crate::value::JsValue;
use crate::vm::tiering::{
    VmAttachedPropertyInlineCacheCandidate, VmCallLinkInlineCacheAttachmentLifecycle,
    VmCallLinkInlineCacheAttachmentOutcome, VmCallLinkInlineCacheAttachmentRecord,
    VmPropertyInlineCacheAttachmentLifecycle, VmPropertyInlineCacheAttachmentRecord,
    VmStructureStubPropertyInlineCacheCandidate,
};

#[derive(Clone, Debug, Default)]
pub(crate) struct CodeBlockRegistry {
    records: HashMap<CodeBlockId, CodeBlockRecord>,
}

impl CodeBlockRegistry {
    // C++ JSC divergence (one shared instance per (executable, specialization)):
    // C++ keeps exactly one stable `CodeBlock` heap object per function and
    // specialization (FunctionExecutable::m_codeBlockForCall/m_codeBlockForConstruct
    // as `WriteBarrier<CodeBlock>`), referenced everywhere by raw `CodeBlock*` and
    // never copied per call. Rust shares one instance via `Rc<CodeBlock>` (Rc, not
    // Arc: the VM is single-threaded — `CodeBlockMutationAuthority::VmMainThread` —
    // and `CodeBlock` is `!Sync` via its `Cell`s; `Rc` stands in for
    // `WriteBarrier<CodeBlock>`). `register` accepts anything convertible into an
    // `Rc<CodeBlock>` so existing by-value `CodeBlock` callers keep working while
    // the install path can hand in a pre-built shared `Rc` (shared by `Rc::clone`
    // into both this registry and the interpreter host).
    pub(crate) fn register(&mut self, owner: CodeBlockId, code_block: impl Into<Rc<CodeBlock>>) {
        let mut code_block = code_block.into();
        // The root-map owner is a hashed field. When this `Rc` is uniquely owned
        // (the common by-value-`CodeBlock` callers), stamp it in place via
        // `Rc::get_mut`; the install path stamps before sharing, so an already
        // shared `Rc` re-stamps to the same owner and the no-op is safe.
        if let Some(code_block_mut) = Rc::get_mut(&mut code_block) {
            code_block_mut.stamp_root_map_owner(owner);
        }
        self.records.insert(owner, CodeBlockRecord::new(code_block));
    }

    #[allow(dead_code)]
    pub(crate) fn get(&self, owner: CodeBlockId) -> Option<&CodeBlockRecord> {
        self.records.get(&owner)
    }

    /// Shared handle to the one registered `CodeBlock` instance (refcount bump,
    /// not a deep copy), mirroring C++ handing out the stable `CodeBlock*`. The
    /// hot dispatch path uses this so the per-instance snapshot-fingerprint memo
    /// and interior-mutable feedback persist on the single instance.
    pub(crate) fn code_block_shared(&self, owner: CodeBlockId) -> Option<Rc<CodeBlock>> {
        self.records
            .get(&owner)
            .map(|record| record.code_block_shared())
    }

    // Test-only `&mut` access to the registered instance via `Rc::get_mut`, which
    // succeeds only while the registry is the sole owner (no live dispatch alias).
    // Tests mutate the block before sharing it, so this holds.
    #[cfg(test)]
    pub(crate) fn code_block_mut_for_test(&mut self, owner: CodeBlockId) -> Option<&mut CodeBlock> {
        self.records
            .get_mut(&owner)
            .and_then(|record| Rc::get_mut(&mut record.code_block))
    }

    #[allow(dead_code)]
    pub(crate) fn contains(&self, owner: CodeBlockId) -> bool {
        self.records.contains_key(&owner)
    }

    #[allow(dead_code)]
    pub(crate) fn owner_is_live(&self, heap: &Heap, owner: CodeBlockId) -> bool {
        self.contains(owner) && self.owner_cell_is_live(heap, owner)
    }

    #[allow(dead_code)]
    pub(crate) fn owner_cell_is_live(&self, heap: &Heap, owner: CodeBlockId) -> bool {
        heap.allocation_records()
            .iter()
            .any(|record| allocation_is_live_code_block_owner(record, owner))
    }

    pub(crate) fn attached_property_inline_cache_candidates_for_owner(
        &self,
        owner: CodeBlockId,
        bytecode_snapshot: BaselineBytecodeSnapshotFingerprint,
        attachments: &[VmPropertyInlineCacheAttachmentRecord],
    ) -> Vec<VmAttachedPropertyInlineCacheCandidate> {
        let Some(record) = self.records.get(&owner) else {
            return Vec::new();
        };

        attachments
            .iter()
            .filter(|attachment| {
                attachment.lifecycle == VmPropertyInlineCacheAttachmentLifecycle::Active
                    && attachment.owner == owner
                    && attachment.bytecode_snapshot == bytecode_snapshot
            })
            .filter_map(|attachment| {
                let CacheKey::Property(key) = attachment.key else {
                    return None;
                };
                let code_block_outcome = attachment.code_block_outcome?;
                if code_block_outcome.structure_stub_index.is_some()
                    || code_block_outcome.slot != attachment.slot.0 as usize
                    || code_block_outcome.bytecode_index != attachment.bytecode_index
                    || code_block_outcome.key != key
                    || code_block_outcome.attachment_kind
                        != attachment.kind.bytecode_attachment_kind()
                    || code_block_outcome.base_structure != attachment.base_structure
                    || code_block_outcome.holder_structure != attachment.holder_structure
                    || code_block_outcome.new_structure != attachment.new_structure
                    || code_block_outcome.offset
                        != attachment
                            .offset
                            .map(|offset| crate::bytecode::ic::PropertyOffset(offset.raw()))
                {
                    return None;
                }

                let metadata = record
                    .code_block
                    .attached_property_inline_cache_metadata(
                        PropertyInlineCacheAttachedMetadataRequest {
                            slot: attachment.slot.0 as usize,
                            bytecode_index: attachment.bytecode_index,
                            key,
                            attachment_kind: attachment.kind.bytecode_attachment_kind(),
                            base_structure: attachment.base_structure,
                            holder_structure: attachment.holder_structure,
                            new_structure: attachment.new_structure,
                            offset: attachment
                                .offset
                                .map(|offset| crate::bytecode::ic::PropertyOffset(offset.raw())),
                            dispatch: code_block_outcome.dispatch,
                            stub_mode: code_block_outcome.stub_mode,
                        },
                    )
                    .ok()?;

                VmAttachedPropertyInlineCacheCandidate::from_attachment_record_and_code_block_metadata(
                    attachment,
                    metadata,
                )
            })
            .collect()
    }

    pub(crate) fn call_link_inline_cache_slot_for_owner(
        &self,
        owner: CodeBlockId,
        bytecode_index: BytecodeIndex,
    ) -> Option<InlineCacheSlotId> {
        let record = self.records.get(&owner)?;
        let (slot, _) = record
            .code_block
            .side_tables()
            .inline_caches()
            .call_slot_for_bytecode_index(bytecode_index)?;
        Some(InlineCacheSlotId(slot as u32))
    }

    pub(crate) fn attached_call_link_inline_cache_candidates_for_owner(
        &self,
        owner: CodeBlockId,
        bytecode_snapshot: BaselineBytecodeSnapshotFingerprint,
        attachments: &[VmCallLinkInlineCacheAttachmentRecord],
    ) -> Vec<CodeBlockRegistryAttachedCallLinkInlineCacheCandidate> {
        let Some(record) = self.records.get(&owner) else {
            return Vec::new();
        };

        attachments
            .iter()
            .filter(|attachment| {
                attachment.lifecycle == VmCallLinkInlineCacheAttachmentLifecycle::Active
                    && attachment.owner == owner
                    && attachment.bytecode_snapshot == bytecode_snapshot
            })
            .filter_map(|attachment| {
                let VmCallLinkInlineCacheAttachmentOutcome::Accepted { outcome } =
                    &attachment.outcome
                else {
                    return None;
                };
                let code_block_outcome = attachment.code_block_outcome.as_ref()?;
                if code_block_outcome != outcome
                    || code_block_outcome.slot != attachment.slot.0 as usize
                    || code_block_outcome.bytecode_index != attachment.bytecode_index
                    || code_block_outcome.mode != CallLinkMode::Monomorphic
                    || !matches!(
                        &code_block_outcome.target,
                        CallTarget::MetadataOnlyMonomorphic { .. }
                    )
                {
                    return None;
                }

                let metadata = record
                    .code_block
                    .attached_call_link_inline_cache_metadata(
                        CallLinkInlineCacheAttachedMetadataRequest {
                            slot: attachment.slot.0 as usize,
                            bytecode_index: attachment.bytecode_index,
                            target: code_block_outcome.target.clone(),
                        },
                    )
                    .ok()?;

                CodeBlockRegistryAttachedCallLinkInlineCacheCandidate::from_attachment_record_and_code_block_metadata(
                    attachment,
                    metadata,
                )
            })
            .collect()
    }

    #[allow(dead_code)]
    pub(crate) fn structure_stub_property_inline_cache_candidates_for_owner(
        &self,
        owner: CodeBlockId,
        bytecode_snapshot: BaselineBytecodeSnapshotFingerprint,
        attachments: &[VmPropertyInlineCacheAttachmentRecord],
    ) -> Vec<VmStructureStubPropertyInlineCacheCandidate> {
        let Some(record) = self.records.get(&owner) else {
            return Vec::new();
        };

        attachments
            .iter()
            .filter(|attachment| {
                attachment.lifecycle == VmPropertyInlineCacheAttachmentLifecycle::Active
                    && attachment.owner == owner
                    && attachment.bytecode_snapshot == bytecode_snapshot
            })
            .filter_map(|attachment| {
                let code_block_outcome = attachment.code_block_outcome?;
                if code_block_outcome.stub_mode
                    != crate::bytecode::ic::PropertyInlineCacheStubMode::StructureStub
                {
                    return None;
                }
                let structure_stub_index = code_block_outcome.structure_stub_index?;
                let inline_caches = record.code_block.side_tables().inline_caches();
                let structure_stub_info = inline_caches.structure_stubs.get(structure_stub_index)?;
                VmStructureStubPropertyInlineCacheCandidate::from_attachment_record_and_structure_stub_info(
                    attachment,
                    structure_stub_index,
                    structure_stub_info,
                )
            })
            .collect()
    }

    #[allow(dead_code)]
    pub(crate) fn attach_property_inline_cache_case(
        &mut self,
        owner: CodeBlockId,
        request: PropertyInlineCacheAttachmentRequest,
    ) -> CodeBlockRegistryPropertyInlineCacheAttachment {
        let Some(record) = self.records.get_mut(&owner) else {
            return CodeBlockRegistryPropertyInlineCacheAttachment::NotRegistered;
        };

        match record
            .code_block
            .attach_property_inline_cache_case(CodeBlockMutationAuthority::VmMainThread, request)
        {
            Ok(outcome) => CodeBlockRegistryPropertyInlineCacheAttachment::Attached(outcome),
            Err(error) => CodeBlockRegistryPropertyInlineCacheAttachment::Rejected(error),
        }
    }

    pub(crate) fn attach_call_link_inline_cache(
        &mut self,
        owner: CodeBlockId,
        request: CallLinkInlineCacheAttachmentRequest,
    ) -> CodeBlockRegistryCallLinkInlineCacheAttachment {
        let Some(record) = self.records.get_mut(&owner) else {
            return CodeBlockRegistryCallLinkInlineCacheAttachment::NotRegistered;
        };

        match record
            .code_block
            .attach_call_link_inline_cache(CodeBlockMutationAuthority::VmMainThread, request)
        {
            Ok(outcome) => CodeBlockRegistryCallLinkInlineCacheAttachment::Attached(outcome),
            Err(error) => CodeBlockRegistryCallLinkInlineCacheAttachment::Rejected(error),
        }
    }

    pub(crate) fn clear_call_link_inline_cache(
        &mut self,
        owner: CodeBlockId,
        request: CallLinkInlineCacheClearRequest,
    ) -> CodeBlockRegistryCallLinkInlineCacheClear {
        let Some(record) = self.records.get_mut(&owner) else {
            return CodeBlockRegistryCallLinkInlineCacheClear::NotRegistered;
        };

        match record
            .code_block
            .clear_call_link_inline_cache(CodeBlockMutationAuthority::VmMainThread, request)
        {
            Ok(outcome) => CodeBlockRegistryCallLinkInlineCacheClear::Cleared(outcome),
            Err(error) => CodeBlockRegistryCallLinkInlineCacheClear::Rejected(error),
        }
    }

    pub(crate) fn record_call_result_value_profile_sample(
        &mut self,
        owner: CodeBlockId,
        opcode: CoreOpcode,
        bytecode_index: BytecodeIndex,
        destination: VirtualRegister,
        value: JsValue,
    ) -> Result<Option<ValueProfileBucketSample>, CodeBlockMutationError> {
        let Some(record) = self.records.get_mut(&owner) else {
            return Ok(None);
        };
        let Ok(decoded) = record.code_block.decoded_instruction_at(bytecode_index) else {
            return Err(CodeBlockMutationError::ValueProfileSample(
                ValueProfileSampleError::MissingProfile {
                    bytecode_index,
                    checkpoint: Checkpoint::NONE,
                },
            ));
        };
        if CoreOpcode::from_opcode(decoded.opcode) != Some(opcode) {
            return Err(CodeBlockMutationError::ValueProfileSample(
                ValueProfileSampleError::MissingProfile {
                    bytecode_index,
                    checkpoint: Checkpoint::NONE,
                },
            ));
        }
        if !matches!(opcode, CoreOpcode::Call | CoreOpcode::CallWithThis) {
            return Ok(None);
        }
        if decoded.register_operand(0).ok() != Some(destination) {
            return Err(CodeBlockMutationError::ValueProfileSample(
                ValueProfileSampleError::MissingProfile {
                    bytecode_index,
                    checkpoint: Checkpoint::NONE,
                },
            ));
        }

        match record.code_block.record_value_profile_sample(
            CodeBlockMutationAuthority::VmMainThread,
            bytecode_index,
            Checkpoint::NONE,
            ValueProfileBucketKind::Sample,
            value,
        ) {
            Ok(sample) => Ok(Some(sample)),
            Err(CodeBlockMutationError::ValueProfileSample(
                ValueProfileSampleError::MissingProfile { .. }
                | ValueProfileSampleError::MissingBucket { .. },
            )) => Ok(None),
            Err(error) => Err(error),
        }
    }

    pub(crate) fn call_result_value_profile_store_target(
        &self,
        owner: CodeBlockId,
        opcode: CoreOpcode,
        bytecode_index: BytecodeIndex,
        destination: VirtualRegister,
    ) -> Result<Option<ValueProfileJitStoreTarget>, CodeBlockMutationError> {
        let Some(record) = self.records.get(&owner) else {
            return Ok(None);
        };
        let Ok(decoded) = record.code_block.decoded_instruction_at(bytecode_index) else {
            return Err(CodeBlockMutationError::ValueProfileSample(
                ValueProfileSampleError::MissingProfile {
                    bytecode_index,
                    checkpoint: Checkpoint::NONE,
                },
            ));
        };
        if CoreOpcode::from_opcode(decoded.opcode) != Some(opcode) {
            return Err(CodeBlockMutationError::ValueProfileSample(
                ValueProfileSampleError::MissingProfile {
                    bytecode_index,
                    checkpoint: Checkpoint::NONE,
                },
            ));
        }
        if !matches!(opcode, CoreOpcode::Call | CoreOpcode::CallWithThis) {
            return Ok(None);
        }
        if decoded.register_operand(0).ok() != Some(destination) {
            return Err(CodeBlockMutationError::ValueProfileSample(
                ValueProfileSampleError::MissingProfile {
                    bytecode_index,
                    checkpoint: Checkpoint::NONE,
                },
            ));
        }

        record
            .code_block
            .side_tables()
            .value_profiles()
            .jit_store_target(
                bytecode_index,
                Checkpoint::NONE,
                ValueProfileBucketKind::Sample,
            )
            .map(Some)
            .map_err(CodeBlockMutationError::ValueProfileSample)
    }

    #[allow(dead_code)]
    pub(crate) fn clear_property_inline_cache_case(
        &mut self,
        owner: CodeBlockId,
        request: PropertyInlineCacheClearRequest,
    ) -> CodeBlockRegistryPropertyInlineCacheClear {
        let Some(record) = self.records.get_mut(&owner) else {
            return CodeBlockRegistryPropertyInlineCacheClear::NotRegistered;
        };

        match record
            .code_block
            .clear_property_inline_cache_case(CodeBlockMutationAuthority::VmMainThread, request)
        {
            Ok(outcome) => CodeBlockRegistryPropertyInlineCacheClear::Cleared(outcome),
            Err(error) => CodeBlockRegistryPropertyInlineCacheClear::Rejected(error),
        }
    }

    #[allow(dead_code)]
    pub(crate) fn link_structure_stub_access_case(
        &mut self,
        owner: CodeBlockId,
        request: StructureStubAccessCaseLinkRequest,
    ) -> CodeBlockRegistryStructureStubAccessCaseLink {
        let Some(record) = self.records.get_mut(&owner) else {
            return CodeBlockRegistryStructureStubAccessCaseLink::NotRegistered;
        };

        match record
            .code_block
            .link_structure_stub_access_case(CodeBlockMutationAuthority::VmMainThread, request)
        {
            Ok(outcome) => CodeBlockRegistryStructureStubAccessCaseLink::Attached(outcome),
            Err(error) => CodeBlockRegistryStructureStubAccessCaseLink::Rejected(error),
        }
    }

    #[allow(dead_code)]
    pub(crate) fn install_baseline_jit_slot(
        &mut self,
        owner: CodeBlockId,
        authority: CodeBlockMutationAuthority,
        slot: JitCodeSlot,
    ) -> Option<Result<(), CodeBlockMutationError>> {
        self.records
            .get_mut(&owner)
            .map(|record| record.code_block.install_baseline_jit_slot(authority, slot))
    }

    pub(crate) fn publish_executable_entry(
        &mut self,
        owner: CodeBlockId,
        request: ExecutableEntryPublicationRequest,
    ) -> CodeBlockRegistryExecutableEntryPublication {
        let Some(record) = self.records.get_mut(&owner) else {
            return CodeBlockRegistryExecutableEntryPublication::NotRegistered;
        };

        match record.executable_entrypoints.publish_baseline_native_entry(
            request,
            owner,
            &record.code_block,
        ) {
            Ok(publication) => {
                record.executable_entry_publications.push(publication);
                CodeBlockRegistryExecutableEntryPublication::Published(publication)
            }
            Err(error) => CodeBlockRegistryExecutableEntryPublication::Rejected(error),
        }
    }
}

#[derive(Clone, Copy, Debug, Eq, PartialEq)]
pub(crate) enum CodeBlockRegistryPropertyInlineCacheAttachment {
    NotRegistered,
    Attached(PropertyInlineCacheAttachmentOutcome),
    Rejected(PropertyInlineCacheAttachmentError),
}

#[derive(Clone, Debug, Eq, PartialEq)]
pub(crate) enum CodeBlockRegistryCallLinkInlineCacheAttachment {
    NotRegistered,
    Attached(CallLinkInlineCacheAttachmentOutcome),
    Rejected(CallLinkInlineCacheAttachmentError),
}

#[derive(Clone, Debug, Eq, PartialEq)]
pub(crate) enum CodeBlockRegistryCallLinkInlineCacheClear {
    NotRegistered,
    Cleared(CallLinkInlineCacheClearOutcome),
    Rejected(CallLinkInlineCacheClearError),
}

#[derive(Clone, Copy, Debug, Eq, PartialEq)]
pub(crate) enum CodeBlockRegistryPropertyInlineCacheClear {
    NotRegistered,
    Cleared(PropertyInlineCacheClearOutcome),
    Rejected(PropertyInlineCacheClearError),
}

#[derive(Clone, Debug, Eq, PartialEq)]
pub(crate) struct CodeBlockRegistryAttachedCallLinkInlineCacheCandidate {
    pub owner: CodeBlockId,
    pub bytecode_index: BytecodeIndex,
    pub bytecode_snapshot: BaselineBytecodeSnapshotFingerprint,
    pub slot: InlineCacheSlotId,
    pub attachment_ordinal: u64,
    pub attachment_plan_ordinal: u64,
    pub install_recheck_ordinal: u64,
    pub boundary_validation_ordinal: Option<u64>,
    pub descriptor_ordinal: Option<u64>,
    pub observation_ordinal: Option<u64>,
    pub readiness_ordinal: Option<u64>,
    pub target: CallTarget,
}

impl CodeBlockRegistryAttachedCallLinkInlineCacheCandidate {
    fn from_attachment_record_and_code_block_metadata(
        attachment: &VmCallLinkInlineCacheAttachmentRecord,
        metadata: CallLinkInlineCacheAttachedMetadata,
    ) -> Option<Self> {
        if attachment.lifecycle != VmCallLinkInlineCacheAttachmentLifecycle::Active {
            return None;
        }

        let code_block_outcome = attachment.code_block_outcome.as_ref()?;
        if code_block_outcome.slot != metadata.slot
            || code_block_outcome.bytecode_index != metadata.bytecode_index
            || code_block_outcome.call_site != metadata.call_site
            || code_block_outcome.opcode != metadata.opcode
            || code_block_outcome.call_type != metadata.call_type
            || code_block_outcome.mode != metadata.mode
            || code_block_outcome.specialization != metadata.specialization
            || code_block_outcome.target != metadata.target
            || code_block_outcome.slow_path_count != metadata.slow_path_count
            || code_block_outcome.max_argument_count_including_this_for_varargs
                != metadata.max_argument_count_including_this_for_varargs
            || attachment.slot.0 as usize != metadata.slot
            || attachment.bytecode_index != metadata.bytecode_index
            || metadata.mode != CallLinkMode::Monomorphic
            || !matches!(&metadata.target, CallTarget::MetadataOnlyMonomorphic { .. })
        {
            return None;
        }

        Some(Self {
            owner: attachment.owner,
            bytecode_index: attachment.bytecode_index,
            bytecode_snapshot: attachment.bytecode_snapshot,
            slot: attachment.slot,
            attachment_ordinal: attachment.ordinal,
            attachment_plan_ordinal: attachment.attachment_plan_ordinal,
            install_recheck_ordinal: attachment.install_recheck_ordinal,
            boundary_validation_ordinal: attachment.boundary_validation_ordinal,
            descriptor_ordinal: attachment.descriptor_ordinal,
            observation_ordinal: attachment.observation_ordinal,
            readiness_ordinal: attachment.readiness_ordinal,
            target: metadata.target,
        })
    }
}

#[derive(Clone, Copy, Debug, Eq, PartialEq)]
pub(crate) enum CodeBlockRegistryStructureStubAccessCaseLink {
    NotRegistered,
    Attached(StructureStubAccessCaseLinkOutcome),
    Rejected(StructureStubAccessCaseLinkError),
}

#[derive(Clone, Copy, Debug, Eq, PartialEq)]
pub(crate) enum CodeBlockRegistryExecutableEntryPublication {
    NotRegistered,
    Published(ExecutableEntryCacheRecord),
    Rejected(ExecutableEntryPublicationError),
}

#[derive(Clone, Debug)]
pub(crate) struct CodeBlockRecord {
    // C++ JSC divergence: the canonical `CodeBlock` is the single shared instance
    // (see `CodeBlockRegistry::register`). `Rc` stands in for C++
    // `WriteBarrier<CodeBlock>`; runtime feedback mutates it in place through the
    // interior-mutable fields (`&self`), so no `&mut` through the `Rc` is needed.
    code_block: Rc<CodeBlock>,
    executable_entrypoints: ExecutableEntrypoints,
    executable_entry_publications: Vec<ExecutableEntryCacheRecord>,
}

impl CodeBlockRecord {
    fn new(code_block: Rc<CodeBlock>) -> Self {
        Self {
            code_block,
            executable_entrypoints: ExecutableEntrypoints::default(),
            executable_entry_publications: Vec::new(),
        }
    }

    /// Borrow the shared instance as `&CodeBlock` (auto-derefs through `Rc`) for
    /// read-only callers; unchanged from before the `Rc` migration.
    #[allow(dead_code)]
    pub(crate) fn code_block(&self) -> &CodeBlock {
        &self.code_block
    }

    /// Shared handle to the one instance (refcount bump). Used by the hot dispatch
    /// path so the memo + interior-mutable feedback persist on the single instance.
    pub(crate) fn code_block_shared(&self) -> Rc<CodeBlock> {
        Rc::clone(&self.code_block)
    }

    #[allow(dead_code)]
    pub(crate) fn executable_entrypoints(&self) -> &ExecutableEntrypoints {
        &self.executable_entrypoints
    }

    #[allow(dead_code)]
    pub(crate) fn executable_entry_publications(&self) -> &[ExecutableEntryCacheRecord] {
        &self.executable_entry_publications
    }
}

#[allow(dead_code)]
fn allocation_is_live_code_block_owner(record: &HeapAllocationRecord, owner: CodeBlockId) -> bool {
    record.response.cell == owner.0
        && record.published
        && record.response.metadata.type_info.cell_type == CellType::CodeBlock
        && record.request.metadata.type_info.cell_type == CellType::CodeBlock
        && record.lifecycle.destruction_state == CellDestructionState::NotPending
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::bytecode::ic::{
        CallType, PropertyInlineCacheAttachmentKind, PropertyInlineCacheAttachmentRequest,
        PropertyInlineCacheAttachmentRequirements, PropertyInlineCacheDispatch,
        PropertyInlineCacheStubMode,
    };
    use crate::bytecode::{
        BytecodeIndex, CallSiteIndex, CodeBlockLifecycleState, CodeKind, CodeSpecialization,
        CoreOpcode, LinkContext, Operand, OperandWidth, PackedInstructionStream, PropertyOffset,
        TypedInstruction, UnlinkedCodeBlock, UnlinkedCodeBlockPhase, VirtualRegister,
    };
    use crate::gc::{CellId, StructureId};
    use crate::jit::plan::BaselineGeneratedPropertyHandoffPlanMetadata;
    use crate::jit::CallBoundaryId;
    use crate::jit::{CacheKey, InlineCacheSlotId};
    use crate::runtime::{ExecutableId, ObjectId};
    use crate::strings::{AtomId, Identifier, PropertyKey};
    use crate::vm::tiering::{
        VmCallLinkInlineCacheAttachmentRejectionReason, VmPropertyInlineCacheAttachmentKind,
        VmPropertyInlineCacheAttachmentOutcome,
    };

    fn get_by_name_code_block(identifier_index: u32) -> CodeBlock {
        let instructions =
            PackedInstructionStream::from_typed_placeholder(vec![TypedInstruction {
                opcode: CoreOpcode::GetByName.opcode(),
                width: OperandWidth::Narrow,
                operands: vec![
                    Operand::Register(VirtualRegister::local(0)),
                    Operand::Register(VirtualRegister::local(1)),
                    Operand::IdentifierIndex(identifier_index),
                ],
                schema: None,
                bytecode_index: Some(BytecodeIndex::from_offset(0)),
            }]);
        let unlinked = UnlinkedCodeBlock::new(CodeKind::Program, instructions)
            .with_phase(UnlinkedCodeBlockPhase::Finalized);

        CodeBlock::from_unlinked(unlinked, LinkContext::default())
            .with_lifecycle(CodeBlockLifecycleState::LinkedInterpreter)
    }

    fn identifier_property_key(identifier_index: u32) -> PropertyKey {
        PropertyKey::from_identifier(Identifier::from_atom(AtomId::from_table_slot(
            identifier_index,
        )))
    }

    fn call_instruction(
        offset: u32,
        destination: VirtualRegister,
        callee: VirtualRegister,
        arguments: Vec<VirtualRegister>,
    ) -> TypedInstruction {
        let mut operands = vec![
            Operand::Register(destination),
            Operand::Register(callee),
            Operand::UnsignedImmediate(arguments.len().try_into().unwrap_or(u32::MAX)),
        ];
        operands.extend(arguments.into_iter().map(Operand::Register));
        TypedInstruction {
            opcode: CoreOpcode::Call.opcode(),
            width: OperandWidth::Narrow,
            operands,
            schema: None,
            bytecode_index: Some(BytecodeIndex::from_offset(offset)),
        }
    }

    fn call_link_code_block() -> CodeBlock {
        let instructions = PackedInstructionStream::from_typed_placeholder(vec![call_instruction(
            10,
            VirtualRegister::local(0),
            VirtualRegister::local(1),
            vec![VirtualRegister::local(2)],
        )]);
        let unlinked = UnlinkedCodeBlock::new(CodeKind::Program, instructions)
            .with_phase(UnlinkedCodeBlockPhase::Finalized);

        CodeBlock::from_unlinked(unlinked, LinkContext::default())
            .with_lifecycle(CodeBlockLifecycleState::LinkedInterpreter)
    }

    fn call_link_metadata_target(seed: u32) -> CallTarget {
        CallTarget::MetadataOnlyMonomorphic {
            callee: ObjectId(CellId(seed)),
            executable: ExecutableId(CellId(seed + 1)),
            code_block: CodeBlockId(CellId(seed + 2)),
            boundary: CallBoundaryId(u64::from(seed + 3)),
        }
    }

    fn call_link_attachment_request() -> CallLinkInlineCacheAttachmentRequest {
        CallLinkInlineCacheAttachmentRequest {
            slot: 0,
            bytecode_index: BytecodeIndex::from_offset(10),
            target: call_link_metadata_target(80),
        }
    }

    fn call_link_attachment_record(
        owner: CodeBlockId,
        bytecode_snapshot: BaselineBytecodeSnapshotFingerprint,
        request: &CallLinkInlineCacheAttachmentRequest,
        outcome: CallLinkInlineCacheAttachmentOutcome,
    ) -> VmCallLinkInlineCacheAttachmentRecord {
        VmCallLinkInlineCacheAttachmentRecord {
            ordinal: 101,
            owner,
            bytecode_index: request.bytecode_index,
            bytecode_snapshot,
            slot: InlineCacheSlotId(request.slot as u32),
            attachment_plan_ordinal: 102,
            install_recheck_ordinal: 103,
            boundary_validation_ordinal: Some(104),
            descriptor_ordinal: Some(105),
            observation_ordinal: Some(106),
            readiness_ordinal: Some(107),
            code_block_outcome: Some(outcome.clone()),
            code_block_error: None,
            lifecycle: VmCallLinkInlineCacheAttachmentLifecycle::Active,
            outcome: VmCallLinkInlineCacheAttachmentOutcome::Accepted { outcome },
        }
    }

    fn call_link_registry_fixture() -> (
        CodeBlockRegistry,
        CodeBlockId,
        BaselineBytecodeSnapshotFingerprint,
        CallLinkInlineCacheAttachmentRequest,
        VmCallLinkInlineCacheAttachmentRecord,
    ) {
        let owner = CodeBlockId(CellId(170));
        let code_block = call_link_code_block();
        let bytecode_snapshot =
            BaselineGeneratedPropertyHandoffPlanMetadata::from_code_block_snapshot(
                &code_block,
                Vec::new(),
            )
            .expect("bytecode snapshot")
            .bytecode_snapshot();
        let mut registry = CodeBlockRegistry::default();
        registry.register(owner, code_block);

        let request = call_link_attachment_request();
        let outcome = match registry.attach_call_link_inline_cache(owner, request.clone()) {
            CodeBlockRegistryCallLinkInlineCacheAttachment::Attached(outcome) => outcome,
            result => panic!("call-link attachment should succeed: {result:?}"),
        };
        let attachment = call_link_attachment_record(owner, bytecode_snapshot, &request, outcome);

        (registry, owner, bytecode_snapshot, request, attachment)
    }

    fn own_data_structure_stub_request(key: PropertyKey) -> PropertyInlineCacheAttachmentRequest {
        PropertyInlineCacheAttachmentRequest {
            slot: 0,
            bytecode_index: BytecodeIndex::from_offset(0),
            key,
            attachment_kind: PropertyInlineCacheAttachmentKind::GetByNameOwnDataLoad,
            base_structure: StructureId::new(11),
            holder_structure: None,
            new_structure: None,
            offset: Some(PropertyOffset(3)),
            dispatch: PropertyInlineCacheDispatch::Handler,
            stub_mode: PropertyInlineCacheStubMode::StructureStub,
            requirements: PropertyInlineCacheAttachmentRequirements {
                requires_barrier: false,
                has_barrier_evidence: false,
                requires_watchpoint: false,
                may_call: false,
                may_allocate: false,
            },
        }
    }

    fn own_data_attachment_record(
        owner: CodeBlockId,
        bytecode_snapshot: BaselineBytecodeSnapshotFingerprint,
        request: PropertyInlineCacheAttachmentRequest,
        outcome: PropertyInlineCacheAttachmentOutcome,
    ) -> VmPropertyInlineCacheAttachmentRecord {
        VmPropertyInlineCacheAttachmentRecord {
            ordinal: 1,
            owner,
            bytecode_index: request.bytecode_index,
            bytecode_snapshot,
            slot: InlineCacheSlotId(request.slot as u32),
            kind: VmPropertyInlineCacheAttachmentKind::OwnDataLoad,
            source_plan_ordinal: 2,
            store_install_recheck_ordinal: None,
            store_readiness_ordinal: None,
            guarded_materialization_ordinal: None,
            guarded_dependency_ordinals: Vec::new(),
            guarded_binding_set_ids: Vec::new(),
            requested_stub_mode: request.stub_mode,
            key: CacheKey::Property(request.key),
            base_structure: request.base_structure,
            holder_structure: request.holder_structure,
            new_structure: request.new_structure,
            offset: request
                .offset
                .map(|offset| crate::object::PropertyOffset::new(offset.0)),
            code_block_outcome: Some(outcome),
            code_block_error: None,
            lifecycle: VmPropertyInlineCacheAttachmentLifecycle::Active,
            outcome: VmPropertyInlineCacheAttachmentOutcome::Accepted { outcome },
        }
    }

    fn structure_stub_registry_fixture() -> (
        CodeBlockRegistry,
        CodeBlockId,
        BaselineBytecodeSnapshotFingerprint,
        PropertyInlineCacheAttachmentRequest,
        VmPropertyInlineCacheAttachmentRecord,
    ) {
        let owner = CodeBlockId(CellId(70));
        let key = identifier_property_key(17);
        let code_block = get_by_name_code_block(17);
        let bytecode_snapshot =
            BaselineGeneratedPropertyHandoffPlanMetadata::from_code_block_snapshot(
                &code_block,
                Vec::new(),
            )
            .expect("bytecode snapshot")
            .bytecode_snapshot();
        let mut registry = CodeBlockRegistry::default();
        registry.register(owner, code_block);

        let request = own_data_structure_stub_request(key);
        let outcome = match registry.attach_property_inline_cache_case(owner, request) {
            CodeBlockRegistryPropertyInlineCacheAttachment::Attached(outcome) => outcome,
            result => panic!("structure-stub attachment should succeed: {result:?}"),
        };
        assert_eq!(outcome.structure_stub_index, Some(0));
        let attachment = own_data_attachment_record(owner, bytecode_snapshot, request, outcome);

        (registry, owner, bytecode_snapshot, request, attachment)
    }

    #[test]
    fn registry_attached_call_link_inline_cache_candidates_project_active_metadata_only_monomorphic_attachment(
    ) {
        let (registry, owner, bytecode_snapshot, request, attachment) =
            call_link_registry_fixture();

        let candidates = registry.attached_call_link_inline_cache_candidates_for_owner(
            owner,
            bytecode_snapshot,
            std::slice::from_ref(&attachment),
        );

        assert_eq!(candidates.len(), 1);
        let candidate = &candidates[0];
        let outcome = attachment
            .code_block_outcome
            .as_ref()
            .expect("accepted attachment has code-block outcome");
        assert_eq!(outcome.call_site, CallSiteIndex(10));
        assert_eq!(outcome.opcode, CoreOpcode::Call);
        assert_eq!(outcome.call_type, CallType::Call);
        assert_eq!(outcome.mode, CallLinkMode::Monomorphic);
        assert_eq!(outcome.specialization, CodeSpecialization::Call);
        assert_eq!(candidate.owner, owner);
        assert_eq!(candidate.bytecode_index, request.bytecode_index);
        assert_eq!(candidate.bytecode_snapshot, bytecode_snapshot);
        assert_eq!(candidate.slot, InlineCacheSlotId(0));
        assert_eq!(candidate.attachment_ordinal, attachment.ordinal);
        assert_eq!(
            candidate.attachment_plan_ordinal,
            attachment.attachment_plan_ordinal
        );
        assert_eq!(
            candidate.install_recheck_ordinal,
            attachment.install_recheck_ordinal
        );
        assert_eq!(
            candidate.boundary_validation_ordinal,
            attachment.boundary_validation_ordinal
        );
        assert_eq!(candidate.descriptor_ordinal, attachment.descriptor_ordinal);
        assert_eq!(
            candidate.observation_ordinal,
            attachment.observation_ordinal
        );
        assert_eq!(candidate.readiness_ordinal, attachment.readiness_ordinal);
        assert_eq!(candidate.target, request.target);
        assert!(matches!(
            &candidate.target,
            CallTarget::MetadataOnlyMonomorphic { .. }
        ));
    }

    #[test]
    fn registry_attached_call_link_inline_cache_candidates_suppress_rejected_and_non_active_attachments(
    ) {
        let (registry, owner, bytecode_snapshot, _, attachment) = call_link_registry_fixture();
        let cleared = VmCallLinkInlineCacheAttachmentRecord {
            lifecycle: VmCallLinkInlineCacheAttachmentLifecycle::Cleared { clear_ordinal: 108 },
            ..attachment.clone()
        };
        let rejected = VmCallLinkInlineCacheAttachmentRecord {
            code_block_outcome: None,
            lifecycle: VmCallLinkInlineCacheAttachmentLifecycle::Rejected,
            outcome: VmCallLinkInlineCacheAttachmentOutcome::Rejected {
                reason: VmCallLinkInlineCacheAttachmentRejectionReason::CodeBlockNotRegistered,
            },
            ..attachment
        };

        assert!(registry
            .attached_call_link_inline_cache_candidates_for_owner(
                owner,
                bytecode_snapshot,
                &[cleared, rejected],
            )
            .is_empty());
    }

    #[test]
    fn registry_attached_call_link_inline_cache_candidates_suppress_owner_and_snapshot_mismatches()
    {
        let (registry, owner, bytecode_snapshot, _, attachment) = call_link_registry_fixture();
        let different_code_block = get_by_name_code_block(29);
        let different_bytecode_snapshot =
            BaselineGeneratedPropertyHandoffPlanMetadata::from_code_block_snapshot(
                &different_code_block,
                Vec::new(),
            )
            .expect("different bytecode snapshot")
            .bytecode_snapshot();
        assert_ne!(different_bytecode_snapshot, bytecode_snapshot);

        let owner_mismatch = VmCallLinkInlineCacheAttachmentRecord {
            owner: CodeBlockId(CellId(171)),
            ..attachment.clone()
        };
        let snapshot_mismatch = VmCallLinkInlineCacheAttachmentRecord {
            bytecode_snapshot: different_bytecode_snapshot,
            ..attachment
        };

        assert!(registry
            .attached_call_link_inline_cache_candidates_for_owner(
                owner,
                bytecode_snapshot,
                &[owner_mismatch, snapshot_mismatch],
            )
            .is_empty());
    }

    #[test]
    fn registry_attached_call_link_inline_cache_candidates_suppress_outcome_and_target_drift() {
        let (registry, owner, bytecode_snapshot, _, attachment) = call_link_registry_fixture();
        let mut drifted_outcome = attachment
            .code_block_outcome
            .as_ref()
            .expect("accepted attachment has code-block outcome")
            .clone();
        drifted_outcome.slow_path_count = 1;
        let outcome_drift = VmCallLinkInlineCacheAttachmentRecord {
            outcome: VmCallLinkInlineCacheAttachmentOutcome::Accepted {
                outcome: drifted_outcome,
            },
            ..attachment.clone()
        };

        assert!(registry
            .attached_call_link_inline_cache_candidates_for_owner(
                owner,
                bytecode_snapshot,
                std::slice::from_ref(&outcome_drift),
            )
            .is_empty());

        let (mut registry, owner, bytecode_snapshot, _, attachment) = call_link_registry_fixture();
        let record = registry.records.get_mut(&owner).expect("registered record");
        let side_tables = record.code_block.side_tables().clone();
        side_tables.inline_caches_mut().calls[0].target = call_link_metadata_target(90);
        record.code_block =
            std::rc::Rc::new((*record.code_block).clone().with_side_tables(side_tables));

        assert!(registry
            .attached_call_link_inline_cache_candidates_for_owner(
                owner,
                bytecode_snapshot,
                std::slice::from_ref(&attachment),
            )
            .is_empty());
    }

    #[test]
    fn registry_attached_call_link_inline_cache_candidates_suppress_non_monomorphic_and_non_metadata_targets(
    ) {
        let (registry, owner, bytecode_snapshot, _, attachment) = call_link_registry_fixture();
        let mut non_monomorphic_outcome = attachment
            .code_block_outcome
            .as_ref()
            .expect("accepted attachment has code-block outcome")
            .clone();
        non_monomorphic_outcome.mode = CallLinkMode::Polymorphic;
        let non_monomorphic = VmCallLinkInlineCacheAttachmentRecord {
            code_block_outcome: Some(non_monomorphic_outcome.clone()),
            outcome: VmCallLinkInlineCacheAttachmentOutcome::Accepted {
                outcome: non_monomorphic_outcome,
            },
            ..attachment.clone()
        };

        let mut non_metadata_outcome = attachment
            .code_block_outcome
            .as_ref()
            .expect("accepted attachment has code-block outcome")
            .clone();
        non_metadata_outcome.target = CallTarget::DirectExecutable(ExecutableId(CellId(99)));
        let non_metadata = VmCallLinkInlineCacheAttachmentRecord {
            code_block_outcome: Some(non_metadata_outcome.clone()),
            outcome: VmCallLinkInlineCacheAttachmentOutcome::Accepted {
                outcome: non_metadata_outcome,
            },
            ..attachment
        };

        assert!(registry
            .attached_call_link_inline_cache_candidates_for_owner(
                owner,
                bytecode_snapshot,
                &[non_monomorphic, non_metadata],
            )
            .is_empty());
    }

    #[test]
    fn registry_property_inline_cache_attachment_candidates_suppress_structure_stubs() {
        let owner = CodeBlockId(CellId(70));
        let key = identifier_property_key(17);
        let code_block = get_by_name_code_block(17);
        let bytecode_snapshot =
            BaselineGeneratedPropertyHandoffPlanMetadata::from_code_block_snapshot(
                &code_block,
                Vec::new(),
            )
            .expect("bytecode snapshot")
            .bytecode_snapshot();
        let mut registry = CodeBlockRegistry::default();
        registry.register(owner, code_block);

        let request = PropertyInlineCacheAttachmentRequest {
            slot: 0,
            bytecode_index: BytecodeIndex::from_offset(0),
            key,
            attachment_kind: PropertyInlineCacheAttachmentKind::GetByNameOwnDataLoad,
            base_structure: StructureId::new(11),
            holder_structure: None,
            new_structure: None,
            offset: Some(PropertyOffset(3)),
            dispatch: PropertyInlineCacheDispatch::Handler,
            stub_mode: PropertyInlineCacheStubMode::StructureStub,
            requirements: PropertyInlineCacheAttachmentRequirements {
                requires_barrier: false,
                has_barrier_evidence: false,
                requires_watchpoint: false,
                may_call: false,
                may_allocate: false,
            },
        };
        let outcome = match registry.attach_property_inline_cache_case(owner, request) {
            CodeBlockRegistryPropertyInlineCacheAttachment::Attached(outcome) => outcome,
            result => panic!("structure-stub attachment should succeed: {result:?}"),
        };
        assert_eq!(outcome.structure_stub_index, Some(0));

        let attachment = VmPropertyInlineCacheAttachmentRecord {
            ordinal: 1,
            owner,
            bytecode_index: request.bytecode_index,
            bytecode_snapshot,
            slot: InlineCacheSlotId(0),
            kind: VmPropertyInlineCacheAttachmentKind::OwnDataLoad,
            source_plan_ordinal: 2,
            store_install_recheck_ordinal: None,
            store_readiness_ordinal: None,
            guarded_materialization_ordinal: None,
            guarded_dependency_ordinals: Vec::new(),
            guarded_binding_set_ids: Vec::new(),
            requested_stub_mode: request.stub_mode,
            key: CacheKey::Property(key),
            base_structure: request.base_structure,
            holder_structure: None,
            new_structure: None,
            offset: request
                .offset
                .map(|offset| crate::object::PropertyOffset::new(offset.0)),
            code_block_outcome: Some(outcome),
            code_block_error: None,
            lifecycle: VmPropertyInlineCacheAttachmentLifecycle::Active,
            outcome: VmPropertyInlineCacheAttachmentOutcome::Accepted { outcome },
        };

        assert!(registry
            .attached_property_inline_cache_candidates_for_owner(
                owner,
                bytecode_snapshot,
                &[attachment],
            )
            .is_empty());
    }

    #[test]
    fn registry_structure_stub_projection_accepts_active_own_data_structure_stub() {
        let (registry, owner, bytecode_snapshot, _, attachment) = structure_stub_registry_fixture();

        let candidates = registry.structure_stub_property_inline_cache_candidates_for_owner(
            owner,
            bytecode_snapshot,
            std::slice::from_ref(&attachment),
        );

        assert_eq!(candidates.len(), 1);
        let candidate = &candidates[0];
        assert_eq!(candidate.owner, owner);
        assert_eq!(candidate.bytecode_snapshot, bytecode_snapshot);
        assert_eq!(candidate.attachment_ordinal, attachment.ordinal);
        assert_eq!(
            candidate.source_plan_ordinal,
            attachment.source_plan_ordinal
        );
        assert_eq!(candidate.structure_stub_index, 0);
        assert_eq!(
            candidate.kind,
            VmPropertyInlineCacheAttachmentKind::OwnDataLoad
        );
        assert_eq!(
            candidate.stub_mode,
            PropertyInlineCacheStubMode::StructureStub
        );
        assert_eq!(candidate.key, attachment.key);
        assert_eq!(candidate.base_structure, attachment.base_structure);
        assert_eq!(candidate.offset, attachment.offset);

        let registered_stub = registry
            .get(owner)
            .expect("registered code block")
            .code_block()
            .side_tables()
            .inline_caches()
            .structure_stubs[0]
            .clone();
        assert_eq!(candidate.structure_stub_info, registered_stub);
    }

    #[test]
    fn registry_structure_stub_projection_suppresses_metadata_only_and_inactive_attachments() {
        let (registry, owner, bytecode_snapshot, request, attachment) =
            structure_stub_registry_fixture();

        let cleared = VmPropertyInlineCacheAttachmentRecord {
            lifecycle: VmPropertyInlineCacheAttachmentLifecycle::Cleared {
                clear_ordinal: 9,
                invalidation_ordinal: None,
                event_dispatch_ordinal: None,
            },
            ..attachment.clone()
        };
        let rejected = VmPropertyInlineCacheAttachmentRecord {
            code_block_outcome: None,
            lifecycle: VmPropertyInlineCacheAttachmentLifecycle::Rejected,
            outcome: VmPropertyInlineCacheAttachmentOutcome::Rejected {
                reason: crate::vm::tiering::VmPropertyInlineCacheAttachmentRejectionReason::CodeBlockNotRegistered,
            },
            ..attachment.clone()
        };

        let mut metadata_only_outcome = attachment.code_block_outcome.expect("accepted outcome");
        metadata_only_outcome.stub_mode = PropertyInlineCacheStubMode::MetadataOnly;
        metadata_only_outcome.structure_stub_index = None;
        let mut metadata_only_request = request;
        metadata_only_request.stub_mode = PropertyInlineCacheStubMode::MetadataOnly;
        let metadata_only = own_data_attachment_record(
            owner,
            bytecode_snapshot,
            metadata_only_request,
            metadata_only_outcome,
        );

        assert!(registry
            .structure_stub_property_inline_cache_candidates_for_owner(
                owner,
                bytecode_snapshot,
                &[metadata_only, cleared, rejected],
            )
            .is_empty());
    }

    #[test]
    fn registry_structure_stub_projection_suppresses_mismatched_stub_metadata() {
        let (mut registry, owner, bytecode_snapshot, _, attachment) =
            structure_stub_registry_fixture();
        let record = registry.records.get_mut(&owner).expect("registered record");
        let side_tables = record.code_block.side_tables().clone();
        side_tables.inline_caches_mut().structure_stubs[0].offset = Some(PropertyOffset(99));
        record.code_block =
            std::rc::Rc::new((*record.code_block).clone().with_side_tables(side_tables));

        assert!(registry
            .structure_stub_property_inline_cache_candidates_for_owner(
                owner,
                bytecode_snapshot,
                std::slice::from_ref(&attachment),
            )
            .is_empty());
    }

    #[test]
    fn registry_structure_stub_projection_suppresses_guarded_and_store_attachment_records() {
        let (registry, owner, bytecode_snapshot, _, attachment) = structure_stub_registry_fixture();
        let guarded = VmPropertyInlineCacheAttachmentRecord {
            kind: VmPropertyInlineCacheAttachmentKind::GuardedPrototypeDataLoad,
            ..attachment.clone()
        };
        let store = VmPropertyInlineCacheAttachmentRecord {
            kind: VmPropertyInlineCacheAttachmentKind::StoreReplace,
            ..attachment
        };

        assert!(registry
            .structure_stub_property_inline_cache_candidates_for_owner(
                owner,
                bytecode_snapshot,
                &[guarded, store],
            )
            .is_empty());
    }
}
