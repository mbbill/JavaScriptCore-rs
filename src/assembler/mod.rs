//! Assembler and MacroAssembler contracts.
//!
//! JSC treats assembler buffers, labels, jumps, calls, relocations, and link
//! buffers as a substrate shared by LLInt, baseline JIT, DFG, FTL, Yarr, and
//! Wasm. This module names those ownership boundaries without making code
//! executable.

use crate::jit::{CodePatchPlan, ExecutableAllocationId};

#[derive(Clone, Copy, Debug, Default, Eq, Hash, PartialEq)]
pub struct AssemblerBufferId(pub u64);

#[derive(Clone, Copy, Debug, Default, Eq, Hash, PartialEq)]
pub struct AssemblerByteImageId(pub u64);

#[derive(Clone, Copy, Debug, Default, Eq, Hash, PartialEq)]
pub struct AssemblerByteImageDigest(pub u64);

#[derive(Clone, Copy, Debug, Default, Eq, PartialEq)]
pub enum AssemblerDataKind {
    #[default]
    Code,
    Hashes,
}

#[derive(Clone, Copy, Debug, Default, Eq, PartialEq)]
pub enum AssemblerBufferLifecycle {
    #[default]
    ThreadLocalReusable,
    Building,
    FrozenForLink,
    CopiedToExecutableMemory,
    ReturnedToThreadCache,
    Released,
}

#[derive(Clone, Copy, Debug, Default, Eq, Hash, PartialEq)]
pub struct AssemblerLabel(pub u32);

#[derive(Clone, Copy, Debug, Default, Eq, Hash, PartialEq)]
pub struct AssemblerJumpId(pub u32);

#[derive(Clone, Copy, Debug, Eq, PartialEq)]
pub enum AssemblerArchitecture {
    X86,
    X86_64,
    Arm64,
    Riscv64,
    Unknown,
}

#[derive(Clone, Copy, Debug, Eq, PartialEq)]
pub enum AssemblerRelocationKind {
    CodeLabel,
    DataLabel,
    CompactDataLabel,
    PointerDataLabel,
    NearCall,
    FarCall,
    Jump,
    PatchableJump,
    ConvertibleLoad,
    AbsolutePointer,
    ExternalReference,
}

#[derive(Clone, Copy, Debug, Eq, PartialEq)]
pub struct AssemblerRelocation {
    pub kind: AssemblerRelocationKind,
    pub at_offset: u32,
    pub target: Option<AssemblerLabel>,
}

#[derive(Clone, Debug, Default, Eq, PartialEq)]
pub struct AssemblerBufferDescriptor {
    pub id: AssemblerBufferId,
    pub data_kind: AssemblerDataKind,
    pub lifecycle: AssemblerBufferLifecycle,
    pub architecture: Option<AssemblerArchitecture>,
    pub byte_len: u32,
    pub capacity_bytes: u32,
    pub inline_capacity_bytes: u32,
    pub labels: Vec<AssemblerLabel>,
    pub jumps: Vec<AssemblerJumpId>,
    pub relocations: Vec<AssemblerRelocation>,
}

#[derive(Clone, Debug, Eq, PartialEq)]
pub struct AssemblerByteImageDescriptor {
    pub id: AssemblerByteImageId,
    pub source: AssemblerBufferId,
    pub source_lifecycle: AssemblerBufferLifecycle,
    pub data_kind: AssemblerDataKind,
    pub architecture: Option<AssemblerArchitecture>,
    pub byte_len: u32,
    pub digest: AssemblerByteImageDigest,
    pub label_count: usize,
    pub jump_count: usize,
    pub relocation_count: usize,
    proof: AssemblerByteImageProof,
}

#[derive(Clone, Debug, Eq, PartialEq)]
pub struct AssemblerByteImage {
    descriptor: AssemblerByteImageDescriptor,
    bytes: Vec<u8>,
}

#[derive(Clone, Copy, Debug, Eq, PartialEq)]
struct AssemblerByteImageProof {
    id: AssemblerByteImageId,
    source: AssemblerBufferId,
    source_lifecycle: AssemblerBufferLifecycle,
    data_kind: AssemblerDataKind,
    architecture: Option<AssemblerArchitecture>,
    byte_len: u32,
    digest: AssemblerByteImageDigest,
    label_count: usize,
    jump_count: usize,
    relocation_count: usize,
}

#[derive(Clone, Debug, Eq, PartialEq)]
pub enum AssemblerValidationError {
    EmptyName,
    EmptyProvenance(&'static str),
    DuplicateRelocationKind(AssemblerRelocationKind),
    DuplicateLinkBufferProfile(LinkBufferProfile),
    BufferLengthExceedsCapacity,
    BufferHasRelocationsWithoutArchitecture,
    RelocationRequiresMissingLabel(AssemblerRelocationKind),
    RelocationTargetMissing(AssemblerLabel),
    LinkBufferSourceMissing,
    LinkBufferSchemaMissing(LinkBufferProfile),
    LinkBufferTransitionMismatch(LinkBufferProfile),
    LinkBufferRelocationNotAllowed(LinkBufferProfile, AssemblerRelocationKind),
    PatchWithoutAllocation,
    AssemblerByteImageIdZero,
    AssemblerByteImageDigestMissing,
    AssemblerByteImageSourceNotFrozen {
        actual: AssemblerBufferLifecycle,
    },
    AssemblerByteImageSourceEmpty,
    AssemblerByteImageProofMismatch,
    AssemblerByteImageBytesLengthMismatch {
        expected: u32,
        actual: usize,
    },
    AssemblerByteImageDigestMismatch {
        expected: AssemblerByteImageDigest,
        actual: AssemblerByteImageDigest,
    },
    AssemblerByteImageSourceMismatch {
        expected: AssemblerBufferId,
        actual: AssemblerBufferId,
    },
    AssemblerByteImageSourceLifecycleMismatch {
        expected: AssemblerBufferLifecycle,
        actual: AssemblerBufferLifecycle,
    },
    AssemblerByteImageDataKindMismatch {
        expected: AssemblerDataKind,
        actual: AssemblerDataKind,
    },
    AssemblerByteImageArchitectureMismatch {
        expected: Option<AssemblerArchitecture>,
        actual: Option<AssemblerArchitecture>,
    },
    AssemblerByteImageByteLengthMismatch {
        expected: u32,
        actual: u32,
    },
    AssemblerByteImageLabelCountMismatch {
        expected: usize,
        actual: usize,
    },
    AssemblerByteImageJumpCountMismatch {
        expected: usize,
        actual: usize,
    },
    AssemblerByteImageRelocationCountMismatch {
        expected: usize,
        actual: usize,
    },
    LinkBufferProfileMissing,
    LinkBufferLayoutSourceMismatch {
        expected: AssemblerBufferId,
        actual: AssemblerBufferId,
    },
    LinkBufferLayoutSizeMismatch {
        expected: u32,
        actual: u32,
    },
    LinkBufferLayoutStateMismatch {
        expected: LinkBufferState,
        actual: Option<LinkBufferState>,
    },
    LinkBufferLayoutRelocationCountMismatch {
        expected: usize,
        actual: usize,
    },
    LinkBufferLayoutRelocationsOutOfOrder,
    LinkBufferRelocationApplicationUnsupported(AssemblerRelocationKind),
}

#[derive(Clone, Debug, Default, Eq, PartialEq)]
pub struct AssemblerBufferBuilder {
    descriptor: AssemblerBufferDescriptor,
}

impl AssemblerBufferBuilder {
    pub fn new(id: AssemblerBufferId) -> Self {
        Self {
            descriptor: AssemblerBufferDescriptor {
                id,
                lifecycle: AssemblerBufferLifecycle::Building,
                ..AssemblerBufferDescriptor::default()
            },
        }
    }

    pub fn architecture(mut self, architecture: AssemblerArchitecture) -> Self {
        self.descriptor.architecture = Some(architecture);
        self
    }

    pub fn lifecycle(mut self, lifecycle: AssemblerBufferLifecycle) -> Self {
        self.descriptor.lifecycle = lifecycle;
        self
    }

    pub fn capacity(mut self, byte_len: u32, capacity_bytes: u32) -> Self {
        self.descriptor.byte_len = byte_len;
        self.descriptor.capacity_bytes = capacity_bytes;
        self
    }

    pub fn inline_capacity(mut self, inline_capacity_bytes: u32) -> Self {
        self.descriptor.inline_capacity_bytes = inline_capacity_bytes;
        self
    }

    pub fn label(mut self, label: AssemblerLabel) -> Self {
        self.descriptor.labels.push(label);
        self
    }

    pub fn jump(mut self, jump: AssemblerJumpId) -> Self {
        self.descriptor.jumps.push(jump);
        self
    }

    pub fn relocation(mut self, relocation: AssemblerRelocation) -> Self {
        self.descriptor.relocations.push(relocation);
        self
    }

    pub fn build(self) -> Result<AssemblerBufferDescriptor, AssemblerValidationError> {
        self.descriptor.validate()?;
        Ok(self.descriptor)
    }
}

impl AssemblerBufferDescriptor {
    pub fn builder(id: AssemblerBufferId) -> AssemblerBufferBuilder {
        AssemblerBufferBuilder::new(id)
    }

    pub fn validate(&self) -> Result<(), AssemblerValidationError> {
        if self.byte_len > self.capacity_bytes {
            return Err(AssemblerValidationError::BufferLengthExceedsCapacity);
        }
        if !self.relocations.is_empty() && self.architecture.is_none() {
            return Err(AssemblerValidationError::BufferHasRelocationsWithoutArchitecture);
        }

        for relocation in &self.relocations {
            let schema = ASSEMBLER_SCHEMA_REGISTRY
                .relocation_for_kind(relocation.kind)
                .ok_or(AssemblerValidationError::DuplicateRelocationKind(
                    relocation.kind,
                ))?;
            if schema.requires_label && relocation.target.is_none() {
                return Err(AssemblerValidationError::RelocationRequiresMissingLabel(
                    relocation.kind,
                ));
            }
            if let Some(target) = relocation.target {
                if !self.labels.contains(&target) {
                    return Err(AssemblerValidationError::RelocationTargetMissing(target));
                }
            }
        }

        Ok(())
    }
}

impl AssemblerByteImageDescriptor {
    pub fn validate(&self) -> Result<(), AssemblerValidationError> {
        if self.id.0 == 0 {
            return Err(AssemblerValidationError::AssemblerByteImageIdZero);
        }
        if self.digest.0 == 0 {
            return Err(AssemblerValidationError::AssemblerByteImageDigestMissing);
        }
        if self.source_lifecycle != AssemblerBufferLifecycle::FrozenForLink {
            return Err(
                AssemblerValidationError::AssemblerByteImageSourceNotFrozen {
                    actual: self.source_lifecycle,
                },
            );
        }
        if self.byte_len == 0 {
            return Err(AssemblerValidationError::AssemblerByteImageSourceEmpty);
        }
        if self.proof != self.expected_proof() {
            return Err(AssemblerValidationError::AssemblerByteImageProofMismatch);
        }

        Ok(())
    }

    pub fn validate_against_source(
        &self,
        source: &AssemblerBufferDescriptor,
    ) -> Result<(), AssemblerValidationError> {
        self.validate()?;
        source.validate()?;
        if source.lifecycle != AssemblerBufferLifecycle::FrozenForLink {
            return Err(
                AssemblerValidationError::AssemblerByteImageSourceNotFrozen {
                    actual: source.lifecycle,
                },
            );
        }
        if source.byte_len == 0 {
            return Err(AssemblerValidationError::AssemblerByteImageSourceEmpty);
        }
        if self.source != source.id {
            return Err(AssemblerValidationError::AssemblerByteImageSourceMismatch {
                expected: self.source,
                actual: source.id,
            });
        }
        if self.source_lifecycle != source.lifecycle {
            return Err(
                AssemblerValidationError::AssemblerByteImageSourceLifecycleMismatch {
                    expected: self.source_lifecycle,
                    actual: source.lifecycle,
                },
            );
        }
        if self.data_kind != source.data_kind {
            return Err(
                AssemblerValidationError::AssemblerByteImageDataKindMismatch {
                    expected: self.data_kind,
                    actual: source.data_kind,
                },
            );
        }
        if self.architecture != source.architecture {
            return Err(
                AssemblerValidationError::AssemblerByteImageArchitectureMismatch {
                    expected: self.architecture,
                    actual: source.architecture,
                },
            );
        }
        if self.byte_len != source.byte_len {
            return Err(
                AssemblerValidationError::AssemblerByteImageByteLengthMismatch {
                    expected: self.byte_len,
                    actual: source.byte_len,
                },
            );
        }
        if self.label_count != source.labels.len() {
            return Err(
                AssemblerValidationError::AssemblerByteImageLabelCountMismatch {
                    expected: self.label_count,
                    actual: source.labels.len(),
                },
            );
        }
        if self.jump_count != source.jumps.len() {
            return Err(
                AssemblerValidationError::AssemblerByteImageJumpCountMismatch {
                    expected: self.jump_count,
                    actual: source.jumps.len(),
                },
            );
        }
        if self.relocation_count != source.relocations.len() {
            return Err(
                AssemblerValidationError::AssemblerByteImageRelocationCountMismatch {
                    expected: self.relocation_count,
                    actual: source.relocations.len(),
                },
            );
        }

        Ok(())
    }

    fn expected_proof(&self) -> AssemblerByteImageProof {
        AssemblerByteImageProof {
            id: self.id,
            source: self.source,
            source_lifecycle: self.source_lifecycle,
            data_kind: self.data_kind,
            architecture: self.architecture,
            byte_len: self.byte_len,
            digest: self.digest,
            label_count: self.label_count,
            jump_count: self.jump_count,
            relocation_count: self.relocation_count,
        }
    }
}

const ASSEMBLER_BYTE_IMAGE_DIGEST_OFFSET: u64 = 0xcbf2_9ce4_8422_2325;
const ASSEMBLER_BYTE_IMAGE_DIGEST_PRIME: u64 = 0x0000_0100_0000_01b3;
const ASSEMBLER_BYTE_IMAGE_DIGEST_LENGTH_MIX: u64 = 0x9e37_79b9_7f4a_7c15;
const ASSEMBLER_BYTE_IMAGE_DIGEST_FINAL_MIX: u64 = 0xd6e8_feb8_6659_fd93;

pub fn compute_assembler_byte_image_digest(bytes: &[u8]) -> AssemblerByteImageDigest {
    let mut hash = ASSEMBLER_BYTE_IMAGE_DIGEST_OFFSET
        ^ (bytes.len() as u64).wrapping_mul(ASSEMBLER_BYTE_IMAGE_DIGEST_LENGTH_MIX);

    for (index, byte) in bytes.iter().enumerate() {
        hash ^= u64::from(*byte);
        hash = hash.wrapping_mul(ASSEMBLER_BYTE_IMAGE_DIGEST_PRIME);
        hash ^= (index as u64).rotate_left((index as u32) & 63);
    }

    hash ^= hash >> 32;
    hash = hash.wrapping_mul(ASSEMBLER_BYTE_IMAGE_DIGEST_FINAL_MIX);
    hash ^= hash >> 32;

    if hash == 0 {
        AssemblerByteImageDigest(ASSEMBLER_BYTE_IMAGE_DIGEST_OFFSET)
    } else {
        AssemblerByteImageDigest(hash)
    }
}

impl AssemblerByteImage {
    pub fn new(
        descriptor: AssemblerByteImageDescriptor,
        bytes: Vec<u8>,
    ) -> Result<Self, AssemblerValidationError> {
        let image = Self { descriptor, bytes };
        image.validate()?;
        Ok(image)
    }

    pub fn descriptor(&self) -> &AssemblerByteImageDescriptor {
        &self.descriptor
    }

    pub fn bytes(&self) -> &[u8] {
        &self.bytes
    }

    pub fn id(&self) -> AssemblerByteImageId {
        self.descriptor.id
    }

    pub fn digest(&self) -> AssemblerByteImageDigest {
        self.descriptor.digest
    }

    pub fn byte_len(&self) -> u32 {
        self.descriptor.byte_len
    }

    pub fn validate(&self) -> Result<(), AssemblerValidationError> {
        self.descriptor.validate()?;
        self.validate_bytes()
    }

    pub fn validate_against_source(
        &self,
        source: &AssemblerBufferDescriptor,
    ) -> Result<(), AssemblerValidationError> {
        self.descriptor.validate_against_source(source)?;
        self.validate_bytes()
    }

    fn validate_bytes(&self) -> Result<(), AssemblerValidationError> {
        if self.bytes.len() != self.descriptor.byte_len as usize {
            return Err(
                AssemblerValidationError::AssemblerByteImageBytesLengthMismatch {
                    expected: self.descriptor.byte_len,
                    actual: self.bytes.len(),
                },
            );
        }

        let actual = compute_assembler_byte_image_digest(&self.bytes);
        if actual != self.descriptor.digest {
            return Err(AssemblerValidationError::AssemblerByteImageDigestMismatch {
                expected: self.descriptor.digest,
                actual,
            });
        }

        Ok(())
    }
}

pub fn describe_assembler_byte_image(
    source: &AssemblerBufferDescriptor,
    id: AssemblerByteImageId,
    digest: AssemblerByteImageDigest,
) -> Result<AssemblerByteImageDescriptor, AssemblerValidationError> {
    source.validate()?;
    if source.lifecycle != AssemblerBufferLifecycle::FrozenForLink {
        return Err(
            AssemblerValidationError::AssemblerByteImageSourceNotFrozen {
                actual: source.lifecycle,
            },
        );
    }
    if source.byte_len == 0 {
        return Err(AssemblerValidationError::AssemblerByteImageSourceEmpty);
    }
    if id.0 == 0 {
        return Err(AssemblerValidationError::AssemblerByteImageIdZero);
    }
    if digest.0 == 0 {
        return Err(AssemblerValidationError::AssemblerByteImageDigestMissing);
    }

    let descriptor = AssemblerByteImageDescriptor {
        id,
        source: source.id,
        source_lifecycle: source.lifecycle,
        data_kind: source.data_kind,
        architecture: source.architecture,
        byte_len: source.byte_len,
        digest,
        label_count: source.labels.len(),
        jump_count: source.jumps.len(),
        relocation_count: source.relocations.len(),
        proof: AssemblerByteImageProof {
            id,
            source: source.id,
            source_lifecycle: source.lifecycle,
            data_kind: source.data_kind,
            architecture: source.architecture,
            byte_len: source.byte_len,
            digest,
            label_count: source.labels.len(),
            jump_count: source.jumps.len(),
            relocation_count: source.relocations.len(),
        },
    };
    descriptor.validate_against_source(source)?;
    Ok(descriptor)
}

pub fn freeze_assembler_byte_image(
    source: &AssemblerBufferDescriptor,
    id: AssemblerByteImageId,
    bytes: Vec<u8>,
) -> Result<AssemblerByteImage, AssemblerValidationError> {
    let digest = compute_assembler_byte_image_digest(&bytes);
    let descriptor = describe_assembler_byte_image(source, id, digest)?;
    AssemblerByteImage::new(descriptor, bytes)
}

#[derive(Clone, Copy, Debug, Eq, PartialEq)]
pub enum LinkBufferProfile {
    Baseline,
    Dfg,
    Ftl,
    DfgOsrEntry,
    DfgOsrExit,
    FtlOsrExit,
    InlineCache,
    JumpIsland,
    Thunk,
    LlIntThunk,
    DfgThunk,
    FtlThunk,
    WasmThunk,
    YarrJit,
    Uncategorized,
}

#[derive(Clone, Copy, Debug, Eq, PartialEq)]
pub enum LinkBufferState {
    Unlinked,
    Linking,
    Linked,
    AllocationFailed,
    RewritingExistingCode,
    Finalized,
}

#[derive(Clone, Copy, Debug, Eq, PartialEq)]
pub enum CodeRefOwnership {
    ExecutableMemoryHandle,
    SelfManagedImmortal,
    ExternalOwner,
}

#[derive(Clone, Copy, Debug, Eq, PartialEq)]
pub enum JitPermissionTransition {
    RwxToRw,
    RwxToRx,
    RwToRw,
    RwToRx,
    RwToRo,
}

#[derive(Clone, Debug, Eq, PartialEq)]
pub struct MacroAssemblerCodeRefDescriptor {
    pub allocation: Option<ExecutableAllocationId>,
    pub ownership: CodeRefOwnership,
    pub code_offset: u32,
    pub size_bytes: u32,
    pub may_disassemble: bool,
}

#[derive(Clone, Debug, Default, Eq, PartialEq)]
pub struct LinkBufferPlan {
    pub source: AssemblerBufferId,
    pub allocation: Option<ExecutableAllocationId>,
    pub profile: Option<LinkBufferProfile>,
    pub state: Option<LinkBufferState>,
    pub patches: Vec<CodePatchPlan>,
    /// LinkBuffer owns label resolution and patch recording. Executable memory
    /// permission changes remain under the future allocator/JIT-permissions
    /// layer, not general assembler clients.
    pub required_permission_transition: Option<JitPermissionTransition>,
}

#[derive(Clone, Debug, Eq, PartialEq)]
pub struct LinkBufferLayoutPlan {
    pub plan: LinkBufferPlan,
    pub ordered_relocations: Vec<AssemblerRelocation>,
    pub code_size_bytes: u32,
    pub inline_capacity_used: bool,
}

#[derive(Clone, Debug, Eq, PartialEq)]
pub struct LinkedAssemblerByteImage {
    pub source_image_id: AssemblerByteImageId,
    pub source_image_digest: AssemblerByteImageDigest,
    pub output_digest: AssemblerByteImageDigest,
    pub output_size_bytes: u32,
    pub relocation_count: usize,
    pub profile: LinkBufferProfile,
    pub state: LinkBufferState,
    output_bytes: Vec<u8>,
}

impl LinkedAssemblerByteImage {
    pub fn bytes(&self) -> &[u8] {
        &self.output_bytes
    }

    pub fn validate(&self) -> Result<(), AssemblerValidationError> {
        if self.source_image_id.0 == 0 {
            return Err(AssemblerValidationError::AssemblerByteImageIdZero);
        }
        if self.source_image_digest.0 == 0 || self.output_digest.0 == 0 {
            return Err(AssemblerValidationError::AssemblerByteImageDigestMissing);
        }
        if self.state != LinkBufferState::Linked {
            return Err(AssemblerValidationError::LinkBufferLayoutStateMismatch {
                expected: LinkBufferState::Linked,
                actual: Some(self.state),
            });
        }
        if self.output_bytes.len() != self.output_size_bytes as usize {
            return Err(
                AssemblerValidationError::AssemblerByteImageBytesLengthMismatch {
                    expected: self.output_size_bytes,
                    actual: self.output_bytes.len(),
                },
            );
        }

        let actual = compute_assembler_byte_image_digest(&self.output_bytes);
        if actual != self.output_digest {
            return Err(AssemblerValidationError::AssemblerByteImageDigestMismatch {
                expected: self.output_digest,
                actual,
            });
        }

        Ok(())
    }
}

#[derive(Clone, Copy, Debug, Default, Eq, PartialEq, Hash)]
pub enum AssemblerSchemaOwner {
    #[default]
    AssemblerRegistry,
    MacroAssembler,
    LinkBuffer,
}

#[derive(Clone, Copy, Debug, Default, Eq, PartialEq, Hash)]
pub enum AssemblerRegistryMutationAuthority {
    #[default]
    GeneratedStaticDataRefresh,
    CrateInitialization,
}

#[derive(Clone, Copy, Debug, Eq, PartialEq)]
pub struct StaticRelocationSchema {
    pub kind: AssemblerRelocationKind,
    pub name: &'static str,
    pub requires_label: bool,
    pub may_reference_external_symbol: bool,
    pub owner: AssemblerSchemaOwner,
    pub mutation_authority: AssemblerRegistryMutationAuthority,
    pub provenance: &'static str,
}

#[derive(Clone, Copy, Debug, Eq, PartialEq)]
pub struct StaticLinkBufferSchema {
    pub profile: LinkBufferProfile,
    pub name: &'static str,
    pub allowed_relocations: &'static [AssemblerRelocationKind],
    pub required_transition: Option<JitPermissionTransition>,
    pub owner: AssemblerSchemaOwner,
    pub mutation_authority: AssemblerRegistryMutationAuthority,
    pub provenance: &'static str,
}

#[derive(Clone, Copy, Debug, Default, Eq, PartialEq)]
pub struct AssemblerSchemaRegistry {
    pub relocations: &'static [StaticRelocationSchema],
    pub link_buffers: &'static [StaticLinkBufferSchema],
}

impl AssemblerSchemaRegistry {
    pub const fn new(
        relocations: &'static [StaticRelocationSchema],
        link_buffers: &'static [StaticLinkBufferSchema],
    ) -> Self {
        Self {
            relocations,
            link_buffers,
        }
    }

    pub const fn relocations(self) -> &'static [StaticRelocationSchema] {
        self.relocations
    }

    pub const fn link_buffers(self) -> &'static [StaticLinkBufferSchema] {
        self.link_buffers
    }

    pub fn relocation_for_kind(
        self,
        kind: AssemblerRelocationKind,
    ) -> Option<&'static StaticRelocationSchema> {
        self.relocations.iter().find(|schema| schema.kind == kind)
    }

    pub fn link_buffer_for_profile(
        self,
        profile: LinkBufferProfile,
    ) -> Option<&'static StaticLinkBufferSchema> {
        self.link_buffers
            .iter()
            .find(|schema| schema.profile == profile)
    }

    pub fn validate(self) -> Result<(), AssemblerValidationError> {
        for (index, relocation) in self.relocations.iter().enumerate() {
            relocation.validate()?;
            if self.relocations[index + 1..]
                .iter()
                .any(|other| other.kind == relocation.kind)
            {
                return Err(AssemblerValidationError::DuplicateRelocationKind(
                    relocation.kind,
                ));
            }
        }

        for (index, link_buffer) in self.link_buffers.iter().enumerate() {
            link_buffer.validate()?;
            if self.link_buffers[index + 1..]
                .iter()
                .any(|other| other.profile == link_buffer.profile)
            {
                return Err(AssemblerValidationError::DuplicateLinkBufferProfile(
                    link_buffer.profile,
                ));
            }
        }

        Ok(())
    }
}

impl StaticRelocationSchema {
    pub fn validate(&self) -> Result<(), AssemblerValidationError> {
        if self.name.is_empty() {
            return Err(AssemblerValidationError::EmptyName);
        }
        if self.provenance.is_empty() {
            return Err(AssemblerValidationError::EmptyProvenance(self.name));
        }
        if !self.requires_label && !self.may_reference_external_symbol {
            return Err(AssemblerValidationError::RelocationRequiresMissingLabel(
                self.kind,
            ));
        }

        Ok(())
    }
}

impl StaticLinkBufferSchema {
    pub fn validate(&self) -> Result<(), AssemblerValidationError> {
        if self.name.is_empty() {
            return Err(AssemblerValidationError::EmptyName);
        }
        if self.provenance.is_empty() {
            return Err(AssemblerValidationError::EmptyProvenance(self.name));
        }

        Ok(())
    }
}

impl LinkBufferPlan {
    pub fn validate_against(
        &self,
        schema: &StaticLinkBufferSchema,
    ) -> Result<(), AssemblerValidationError> {
        if self.source == AssemblerBufferId::default() {
            return Err(AssemblerValidationError::LinkBufferSourceMissing);
        }
        if self.profile != Some(schema.profile) {
            return Err(AssemblerValidationError::LinkBufferSchemaMissing(
                schema.profile,
            ));
        }
        if self.required_permission_transition != schema.required_transition {
            return Err(AssemblerValidationError::LinkBufferTransitionMismatch(
                schema.profile,
            ));
        }
        if !self.patches.is_empty() && self.allocation.is_none() {
            return Err(AssemblerValidationError::PatchWithoutAllocation);
        }

        Ok(())
    }
}

pub fn plan_link_buffer_layout(
    buffer: &AssemblerBufferDescriptor,
    profile: LinkBufferProfile,
    allocation: Option<ExecutableAllocationId>,
) -> Result<LinkBufferLayoutPlan, AssemblerValidationError> {
    buffer.validate()?;
    let schema = ASSEMBLER_SCHEMA_REGISTRY
        .link_buffer_for_profile(profile)
        .ok_or(AssemblerValidationError::LinkBufferSchemaMissing(profile))?;

    for relocation in &buffer.relocations {
        if !schema.allowed_relocations.contains(&relocation.kind) {
            return Err(AssemblerValidationError::LinkBufferRelocationNotAllowed(
                profile,
                relocation.kind,
            ));
        }
    }

    let plan = LinkBufferPlan {
        source: buffer.id,
        allocation,
        profile: Some(profile),
        state: Some(LinkBufferState::Linking),
        patches: Vec::new(),
        required_permission_transition: schema.required_transition,
    };
    plan.validate_against(schema)?;

    let mut ordered_relocations = buffer.relocations.clone();
    ordered_relocations.sort_by_key(|relocation| relocation.at_offset);

    Ok(LinkBufferLayoutPlan {
        plan,
        ordered_relocations,
        code_size_bytes: buffer.byte_len,
        inline_capacity_used: buffer.byte_len <= buffer.inline_capacity_bytes,
    })
}

fn validate_link_buffer_layout_for_image(
    image: &AssemblerByteImage,
    layout: &LinkBufferLayoutPlan,
) -> Result<LinkBufferProfile, AssemblerValidationError> {
    let profile = layout
        .plan
        .profile
        .ok_or(AssemblerValidationError::LinkBufferProfileMissing)?;
    let schema = ASSEMBLER_SCHEMA_REGISTRY
        .link_buffer_for_profile(profile)
        .ok_or(AssemblerValidationError::LinkBufferSchemaMissing(profile))?;

    layout.plan.validate_against(schema)?;

    if layout.plan.state != Some(LinkBufferState::Linking) {
        return Err(AssemblerValidationError::LinkBufferLayoutStateMismatch {
            expected: LinkBufferState::Linking,
            actual: layout.plan.state,
        });
    }
    if layout.plan.source != image.descriptor.source {
        return Err(AssemblerValidationError::LinkBufferLayoutSourceMismatch {
            expected: image.descriptor.source,
            actual: layout.plan.source,
        });
    }
    if layout.code_size_bytes != image.descriptor.byte_len {
        return Err(AssemblerValidationError::LinkBufferLayoutSizeMismatch {
            expected: image.descriptor.byte_len,
            actual: layout.code_size_bytes,
        });
    }
    if layout.ordered_relocations.len() != image.descriptor.relocation_count {
        return Err(
            AssemblerValidationError::LinkBufferLayoutRelocationCountMismatch {
                expected: image.descriptor.relocation_count,
                actual: layout.ordered_relocations.len(),
            },
        );
    }
    if layout
        .ordered_relocations
        .windows(2)
        .any(|window| window[0].at_offset > window[1].at_offset)
    {
        return Err(AssemblerValidationError::LinkBufferLayoutRelocationsOutOfOrder);
    }
    for relocation in &layout.ordered_relocations {
        if !schema.allowed_relocations.contains(&relocation.kind) {
            return Err(AssemblerValidationError::LinkBufferRelocationNotAllowed(
                profile,
                relocation.kind,
            ));
        }
    }

    Ok(profile)
}

pub fn link_assembler_byte_image(
    image: &AssemblerByteImage,
    layout: &LinkBufferLayoutPlan,
) -> Result<LinkedAssemblerByteImage, AssemblerValidationError> {
    image.validate()?;
    let profile = validate_link_buffer_layout_for_image(image, layout)?;

    if let Some(relocation) = layout.ordered_relocations.first() {
        return Err(
            AssemblerValidationError::LinkBufferRelocationApplicationUnsupported(relocation.kind),
        );
    }

    let output_bytes = image.bytes.clone();
    let output_digest = compute_assembler_byte_image_digest(&output_bytes);
    let linked = LinkedAssemblerByteImage {
        source_image_id: image.id(),
        source_image_digest: image.digest(),
        output_digest,
        output_size_bytes: layout.code_size_bytes,
        relocation_count: layout.ordered_relocations.len(),
        profile,
        state: LinkBufferState::Linked,
        output_bytes,
    };
    linked.validate()?;
    Ok(linked)
}

const LINK_BUFFER_CODE_RELOCATIONS: &[AssemblerRelocationKind] = &[
    AssemblerRelocationKind::CodeLabel,
    AssemblerRelocationKind::NearCall,
    AssemblerRelocationKind::FarCall,
    AssemblerRelocationKind::Jump,
    AssemblerRelocationKind::PatchableJump,
    AssemblerRelocationKind::ExternalReference,
];
const LINK_BUFFER_DATA_RELOCATIONS: &[AssemblerRelocationKind] = &[
    AssemblerRelocationKind::DataLabel,
    AssemblerRelocationKind::CompactDataLabel,
    AssemblerRelocationKind::PointerDataLabel,
    AssemblerRelocationKind::AbsolutePointer,
];

pub const STATIC_RELOCATION_SCHEMAS: &[StaticRelocationSchema] = &[
    StaticRelocationSchema {
        kind: AssemblerRelocationKind::CodeLabel,
        name: "code-label",
        requires_label: true,
        may_reference_external_symbol: false,
        owner: AssemblerSchemaOwner::MacroAssembler,
        mutation_authority: AssemblerRegistryMutationAuthority::GeneratedStaticDataRefresh,
        provenance: "static Rust assembler relocation schema",
    },
    StaticRelocationSchema {
        kind: AssemblerRelocationKind::NearCall,
        name: "near-call",
        requires_label: true,
        may_reference_external_symbol: true,
        owner: AssemblerSchemaOwner::LinkBuffer,
        mutation_authority: AssemblerRegistryMutationAuthority::GeneratedStaticDataRefresh,
        provenance: "static Rust assembler relocation schema",
    },
    StaticRelocationSchema {
        kind: AssemblerRelocationKind::Jump,
        name: "jump",
        requires_label: true,
        may_reference_external_symbol: false,
        owner: AssemblerSchemaOwner::LinkBuffer,
        mutation_authority: AssemblerRegistryMutationAuthority::GeneratedStaticDataRefresh,
        provenance: "static Rust assembler relocation schema",
    },
    StaticRelocationSchema {
        kind: AssemblerRelocationKind::AbsolutePointer,
        name: "absolute-pointer",
        requires_label: false,
        may_reference_external_symbol: true,
        owner: AssemblerSchemaOwner::MacroAssembler,
        mutation_authority: AssemblerRegistryMutationAuthority::GeneratedStaticDataRefresh,
        provenance: "static Rust assembler relocation schema",
    },
];

pub const STATIC_LINK_BUFFER_SCHEMAS: &[StaticLinkBufferSchema] = &[
    StaticLinkBufferSchema {
        profile: LinkBufferProfile::Baseline,
        name: "baseline-link-buffer",
        allowed_relocations: LINK_BUFFER_CODE_RELOCATIONS,
        required_transition: Some(JitPermissionTransition::RwToRx),
        owner: AssemblerSchemaOwner::LinkBuffer,
        mutation_authority: AssemblerRegistryMutationAuthority::GeneratedStaticDataRefresh,
        provenance: "static Rust link-buffer schema",
    },
    StaticLinkBufferSchema {
        profile: LinkBufferProfile::Ftl,
        name: "ftl-link-buffer",
        allowed_relocations: LINK_BUFFER_CODE_RELOCATIONS,
        required_transition: Some(JitPermissionTransition::RwToRx),
        owner: AssemblerSchemaOwner::LinkBuffer,
        mutation_authority: AssemblerRegistryMutationAuthority::GeneratedStaticDataRefresh,
        provenance: "static Rust link-buffer schema",
    },
    StaticLinkBufferSchema {
        profile: LinkBufferProfile::InlineCache,
        name: "inline-cache-link-buffer",
        allowed_relocations: LINK_BUFFER_DATA_RELOCATIONS,
        required_transition: Some(JitPermissionTransition::RwxToRx),
        owner: AssemblerSchemaOwner::LinkBuffer,
        mutation_authority: AssemblerRegistryMutationAuthority::GeneratedStaticDataRefresh,
        provenance: "static Rust link-buffer schema",
    },
];

pub const ASSEMBLER_SCHEMA_REGISTRY: AssemblerSchemaRegistry =
    AssemblerSchemaRegistry::new(STATIC_RELOCATION_SCHEMAS, STATIC_LINK_BUFFER_SCHEMAS);

#[cfg(test)]
mod tests {
    use super::*;

    fn frozen_code_buffer(byte_len: u32) -> AssemblerBufferDescriptor {
        AssemblerBufferDescriptor::builder(AssemblerBufferId(11))
            .architecture(AssemblerArchitecture::X86_64)
            .lifecycle(AssemblerBufferLifecycle::FrozenForLink)
            .capacity(byte_len, byte_len)
            .label(AssemblerLabel(1))
            .jump(AssemblerJumpId(1))
            .build()
            .unwrap()
    }

    #[test]
    fn static_assembler_registry_validates() {
        assert_eq!(ASSEMBLER_SCHEMA_REGISTRY.validate(), Ok(()));
    }

    #[test]
    fn assembler_byte_image_accepts_frozen_code_buffer_without_bytes() {
        let buffer = frozen_code_buffer(32);
        let image = describe_assembler_byte_image(
            &buffer,
            AssemblerByteImageId(1),
            AssemblerByteImageDigest(0xfeed),
        )
        .unwrap();

        assert_eq!(image.source, buffer.id);
        assert_eq!(image.byte_len, 32);
        assert_eq!(image.label_count, 1);
        assert_eq!(image.jump_count, 1);
        assert_eq!(image.relocation_count, 0);
        assert_eq!(image.validate(), Ok(()));
        assert_eq!(image.validate_against_source(&buffer), Ok(()));
    }

    #[test]
    fn assembler_byte_image_computes_digest_from_bytes() {
        let bytes = vec![0x55, 0x48, 0x89, 0xe5];
        let buffer = frozen_code_buffer(bytes.len() as u32);
        let image =
            freeze_assembler_byte_image(&buffer, AssemblerByteImageId(5), bytes.clone()).unwrap();
        let digest = compute_assembler_byte_image_digest(&bytes);

        assert_eq!(image.bytes(), bytes.as_slice());
        assert_eq!(image.digest(), digest);
        assert_eq!(image.descriptor().digest, digest);
        assert_eq!(image.validate_against_source(&buffer), Ok(()));
    }

    #[test]
    fn assembler_byte_image_rejects_tampered_bytes_or_descriptor() {
        let bytes = vec![0x90, 0x90, 0xcc, 0xc3];
        let buffer = frozen_code_buffer(bytes.len() as u32);
        let image =
            freeze_assembler_byte_image(&buffer, AssemblerByteImageId(6), bytes.clone()).unwrap();
        let digest = image.digest();

        let mut tampered_bytes = bytes.clone();
        tampered_bytes[1] = 0xcc;
        assert_eq!(
            AssemblerByteImage::new(image.descriptor().clone(), tampered_bytes.clone()),
            Err(AssemblerValidationError::AssemblerByteImageDigestMismatch {
                expected: digest,
                actual: compute_assembler_byte_image_digest(&tampered_bytes),
            })
        );

        let caller_digest_descriptor = describe_assembler_byte_image(
            &buffer,
            AssemblerByteImageId(7),
            AssemblerByteImageDigest(0xfeed),
        )
        .unwrap();
        assert_eq!(
            AssemblerByteImage::new(caller_digest_descriptor, bytes.clone()),
            Err(AssemblerValidationError::AssemblerByteImageDigestMismatch {
                expected: AssemblerByteImageDigest(0xfeed),
                actual: digest,
            })
        );

        let mut tampered_descriptor = image.descriptor().clone();
        tampered_descriptor.byte_len = 3;
        tampered_descriptor.proof = tampered_descriptor.expected_proof();
        assert_eq!(
            AssemblerByteImage::new(tampered_descriptor, bytes.clone()),
            Err(
                AssemblerValidationError::AssemblerByteImageBytesLengthMismatch {
                    expected: 3,
                    actual: bytes.len(),
                }
            )
        );
    }

    #[test]
    fn assembler_byte_image_rejects_unfrozen_source() {
        let buffer = AssemblerBufferDescriptor::builder(AssemblerBufferId(12))
            .architecture(AssemblerArchitecture::X86_64)
            .capacity(16, 16)
            .build()
            .unwrap();

        assert_eq!(
            describe_assembler_byte_image(
                &buffer,
                AssemblerByteImageId(2),
                AssemblerByteImageDigest(0xbeef)
            ),
            Err(
                AssemblerValidationError::AssemblerByteImageSourceNotFrozen {
                    actual: AssemblerBufferLifecycle::Building,
                }
            )
        );
    }

    #[test]
    fn assembler_byte_image_rejects_zero_digest() {
        let buffer = frozen_code_buffer(16);

        assert_eq!(
            describe_assembler_byte_image(
                &buffer,
                AssemblerByteImageId(3),
                AssemblerByteImageDigest(0)
            ),
            Err(AssemblerValidationError::AssemblerByteImageDigestMissing)
        );
    }

    #[test]
    fn assembler_byte_image_rejects_tampered_source_or_length() {
        let buffer = frozen_code_buffer(24);
        let image = describe_assembler_byte_image(
            &buffer,
            AssemblerByteImageId(4),
            AssemblerByteImageDigest(0x1234),
        )
        .unwrap();

        let mut tampered_source = buffer.clone();
        tampered_source.id = AssemblerBufferId(99);
        assert_eq!(
            image.validate_against_source(&tampered_source),
            Err(AssemblerValidationError::AssemblerByteImageSourceMismatch {
                expected: AssemblerBufferId(11),
                actual: AssemblerBufferId(99),
            })
        );

        let mut tampered_length = image;
        tampered_length.byte_len = 12;
        assert_eq!(
            tampered_length.validate(),
            Err(AssemblerValidationError::AssemblerByteImageProofMismatch)
        );
    }

    #[test]
    fn buffer_builder_rejects_missing_relocation_target_label() {
        let buffer = AssemblerBufferDescriptor::builder(AssemblerBufferId(1))
            .architecture(AssemblerArchitecture::X86_64)
            .capacity(4, 4)
            .relocation(AssemblerRelocation {
                kind: AssemblerRelocationKind::Jump,
                at_offset: 0,
                target: Some(AssemblerLabel(3)),
            })
            .build();

        assert_eq!(
            buffer,
            Err(AssemblerValidationError::RelocationTargetMissing(
                AssemblerLabel(3)
            ))
        );
    }

    #[test]
    fn link_buffer_layout_orders_allowed_relocations() {
        let buffer = AssemblerBufferDescriptor::builder(AssemblerBufferId(5))
            .architecture(AssemblerArchitecture::X86_64)
            .capacity(16, 32)
            .inline_capacity(16)
            .relocation(AssemblerRelocation {
                kind: AssemblerRelocationKind::AbsolutePointer,
                at_offset: 12,
                target: None,
            })
            .relocation(AssemblerRelocation {
                kind: AssemblerRelocationKind::AbsolutePointer,
                at_offset: 4,
                target: None,
            })
            .build()
            .unwrap();

        let layout = plan_link_buffer_layout(
            &buffer,
            LinkBufferProfile::InlineCache,
            Some(ExecutableAllocationId(1)),
        )
        .unwrap();

        assert_eq!(
            layout
                .ordered_relocations
                .iter()
                .map(|relocation| relocation.at_offset)
                .collect::<Vec<_>>(),
            vec![4, 12]
        );
        assert!(layout.inline_capacity_used);
    }

    #[test]
    fn link_buffer_layout_rejects_profile_relocation_mismatch() {
        let buffer = AssemblerBufferDescriptor::builder(AssemblerBufferId(6))
            .architecture(AssemblerArchitecture::X86_64)
            .capacity(8, 8)
            .label(AssemblerLabel(1))
            .relocation(AssemblerRelocation {
                kind: AssemblerRelocationKind::Jump,
                at_offset: 0,
                target: Some(AssemblerLabel(1)),
            })
            .build()
            .unwrap();

        assert_eq!(
            plan_link_buffer_layout(&buffer, LinkBufferProfile::InlineCache, None),
            Err(AssemblerValidationError::LinkBufferRelocationNotAllowed(
                LinkBufferProfile::InlineCache,
                AssemblerRelocationKind::Jump
            ))
        );
    }

    #[test]
    fn linked_assembler_byte_image_preserves_no_relocation_bytes() {
        let bytes = vec![0x90, 0x90, 0xcc, 0xc3];
        let buffer = frozen_code_buffer(bytes.len() as u32);
        let image =
            freeze_assembler_byte_image(&buffer, AssemblerByteImageId(8), bytes.clone()).unwrap();
        let layout = plan_link_buffer_layout(&buffer, LinkBufferProfile::Baseline, None).unwrap();
        let linked = link_assembler_byte_image(&image, &layout).unwrap();

        assert_eq!(linked.bytes(), bytes.as_slice());
        assert_eq!(
            linked.output_digest,
            compute_assembler_byte_image_digest(&bytes)
        );
        assert_eq!(linked.output_size_bytes, bytes.len() as u32);
        assert_eq!(linked.source_image_id, image.id());
        assert_eq!(linked.relocation_count, 0);
        assert_eq!(linked.profile, LinkBufferProfile::Baseline);
        assert_eq!(linked.state, LinkBufferState::Linked);
        assert_eq!(linked.validate(), Ok(()));
    }

    #[test]
    fn linked_assembler_byte_image_rejects_relocation_application() {
        let bytes = vec![0x90; 8];
        let buffer = AssemblerBufferDescriptor::builder(AssemblerBufferId(21))
            .architecture(AssemblerArchitecture::X86_64)
            .lifecycle(AssemblerBufferLifecycle::FrozenForLink)
            .capacity(bytes.len() as u32, bytes.len() as u32)
            .label(AssemblerLabel(1))
            .relocation(AssemblerRelocation {
                kind: AssemblerRelocationKind::Jump,
                at_offset: 4,
                target: Some(AssemblerLabel(1)),
            })
            .build()
            .unwrap();
        let image =
            freeze_assembler_byte_image(&buffer, AssemblerByteImageId(9), bytes.clone()).unwrap();
        let layout = plan_link_buffer_layout(&buffer, LinkBufferProfile::Baseline, None).unwrap();

        assert_eq!(
            link_assembler_byte_image(&image, &layout),
            Err(
                AssemblerValidationError::LinkBufferRelocationApplicationUnsupported(
                    AssemblerRelocationKind::Jump
                )
            )
        );
    }

    #[test]
    fn linked_assembler_byte_image_rejects_layout_mismatches() {
        let bytes = vec![0x90, 0xc3];
        let buffer = frozen_code_buffer(bytes.len() as u32);
        let image =
            freeze_assembler_byte_image(&buffer, AssemblerByteImageId(10), bytes.clone()).unwrap();

        let mut profile_mismatch =
            plan_link_buffer_layout(&buffer, LinkBufferProfile::Baseline, None).unwrap();
        profile_mismatch.plan.profile = Some(LinkBufferProfile::InlineCache);
        assert_eq!(
            link_assembler_byte_image(&image, &profile_mismatch),
            Err(AssemblerValidationError::LinkBufferTransitionMismatch(
                LinkBufferProfile::InlineCache
            ))
        );

        let mut source_mismatch =
            plan_link_buffer_layout(&buffer, LinkBufferProfile::Baseline, None).unwrap();
        source_mismatch.plan.source = AssemblerBufferId(99);
        assert_eq!(
            link_assembler_byte_image(&image, &source_mismatch),
            Err(AssemblerValidationError::LinkBufferLayoutSourceMismatch {
                expected: buffer.id,
                actual: AssemblerBufferId(99),
            })
        );
    }
}
