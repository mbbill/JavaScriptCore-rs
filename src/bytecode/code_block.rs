use std::{
    cell::{Cell, RefCell},
    collections::{BTreeMap, HashMap},
    sync::Arc,
};

use crate::gc::StructureId;
use crate::jit::plan::BaselineBytecodeSnapshotFingerprint;
use crate::runtime::{CodeBlockId, ExecutableId, GlobalObjectId, ScopeId};
use crate::strings::{AtomId, Identifier, PropertyKey};
use crate::value::{JsValue, NumberValue};

use crate::bytecode::debug::{BytecodeHookTable, ExceptionHandlerTable};
use crate::bytecode::gc::BytecodeRootMap;
use crate::bytecode::ic::{
    BaselineJitData, CallLinkInfo, CallLinkInlineCacheAttachedMetadata,
    CallLinkInlineCacheAttachedMetadataError, CallLinkInlineCacheAttachedMetadataMismatchField,
    CallLinkInlineCacheAttachedMetadataRequest, CallLinkInlineCacheAttachedMetadataResult,
    CallLinkInlineCacheAttachmentError, CallLinkInlineCacheAttachmentOutcome,
    CallLinkInlineCacheAttachmentRequest, CallLinkInlineCacheAttachmentResult,
    CallLinkInlineCacheClearError, CallLinkInlineCacheClearMetadataMismatchField,
    CallLinkInlineCacheClearOutcome, CallLinkInlineCacheClearRequest,
    CallLinkInlineCacheClearResult, CallLinkMode, CallTarget, CallType, GetByIdMode,
    GetByIdModeMetadata, HandlerPropertyInlineCacheRecord, InlineCacheMutationAuthority,
    InlineCacheState, InlineCacheTable, PropertyCacheKey, PropertyInlineCache,
    PropertyInlineCacheAttachedMetadata, PropertyInlineCacheAttachedMetadataError,
    PropertyInlineCacheAttachedMetadataMismatchField, PropertyInlineCacheAttachedMetadataRequest,
    PropertyInlineCacheAttachedMetadataResult, PropertyInlineCacheAttachmentError,
    PropertyInlineCacheAttachmentKind, PropertyInlineCacheAttachmentOutcome,
    PropertyInlineCacheAttachmentRequest, PropertyInlineCacheAttachmentResult,
    PropertyInlineCacheClearError, PropertyInlineCacheClearMetadataMismatchField,
    PropertyInlineCacheClearOutcome, PropertyInlineCacheClearRequest,
    PropertyInlineCacheClearResult, PropertyInlineCacheDispatch, PropertyInlineCacheStubMode,
    PutByIdMode, PutByIdModeMetadata, StructureStubAccessCaseLinkError,
    StructureStubAccessCaseLinkOutcome, StructureStubAccessCaseLinkRequest,
    StructureStubAccessCaseLinkResult, StructureStubInfo, StructureStubKind,
    StructureStubMetadataMismatchField,
};
use crate::bytecode::instruction::{
    DecodedInstruction, InstructionDecodeError, Operand, PackedInstructionStream,
};
use crate::bytecode::metadata::{InstructionMetadataPlan, MetadataLayout, MetadataLinkingData};
use crate::bytecode::opcode::{CoreOpcode, MetadataFieldSpec, Opcode, OpcodeSchemaVersion};
use crate::bytecode::origin::{CodeOrigin, CodeOriginTable, SourceNoteLookup};
use crate::bytecode::profiling::{
    ArrayProfile, BinaryArithProfile, ProfileUpdatePolicy, ProfilingCounterSet, UnaryArithProfile,
    UnlinkedValueProfile, ValueProfile, ValueProfileBucket, ValueProfileBucketKind,
    ValueProfileBucketSample, ValueProfileEmissionCapability, ValueProfileEmissionPolicy,
    ValueProfileSampleError, ValueProfileTable,
};
use crate::bytecode::register::{RegisterFrameShape, SpecialRegisters, VirtualRegister};
use crate::bytecode::speculated_type::SPEC_NONE;

pub const BYTECODE_INDEX_CHECKPOINTS: u8 = 4;
const BYTECODE_INDEX_CHECKPOINT_MASK: u32 = BYTECODE_INDEX_CHECKPOINTS as u32 - 1;
const BYTECODE_INDEX_CHECKPOINT_SHIFT: u32 = 2;
const INVALID_BYTECODE_INDEX_BITS: u32 = u32::MAX;

/// Byte offset plus checkpoint ordinal into a packed instruction stream.
///
/// This mirrors JSC's `BytecodeIndex`: the offset identifies an instruction
/// byte position and the low bits select an instruction checkpoint. Keeping the
/// checkpoint inside the index lets profiling and exception tables refer to
/// sub-instruction events without introducing a separate key type.
#[derive(Clone, Copy, Debug, Eq, PartialEq, Hash, Ord, PartialOrd)]
pub struct BytecodeIndex {
    packed_bits: u32,
}

impl BytecodeIndex {
    pub const INVALID: Self = Self {
        packed_bits: INVALID_BYTECODE_INDEX_BITS,
    };

    pub const fn from_offset(offset: u32) -> Self {
        Self::new(offset, Checkpoint::NONE)
    }

    pub const fn new(offset: u32, checkpoint: Checkpoint) -> Self {
        Self {
            packed_bits: (offset << BYTECODE_INDEX_CHECKPOINT_SHIFT)
                | (checkpoint.0 as u32 & BYTECODE_INDEX_CHECKPOINT_MASK),
        }
    }

    pub const fn from_bits(bits: u32) -> Self {
        Self { packed_bits: bits }
    }

    pub const fn as_bits(self) -> u32 {
        self.packed_bits
    }

    pub const fn offset(self) -> u32 {
        self.packed_bits >> BYTECODE_INDEX_CHECKPOINT_SHIFT
    }

    pub const fn checkpoint(self) -> Checkpoint {
        Checkpoint((self.packed_bits & BYTECODE_INDEX_CHECKPOINT_MASK) as u8)
    }

    pub const fn with_checkpoint(self, checkpoint: Checkpoint) -> Self {
        Self::new(self.offset(), checkpoint)
    }

    pub const fn is_valid(self) -> bool {
        self.packed_bits != INVALID_BYTECODE_INDEX_BITS
    }
}

impl Default for BytecodeIndex {
    fn default() -> Self {
        Self::INVALID
    }
}

#[derive(Clone, Copy, Debug, Default, Eq, PartialEq, Hash, Ord, PartialOrd)]
#[repr(transparent)]
pub struct Checkpoint(pub u8);

impl Checkpoint {
    pub const NONE: Self = Self(0);
}

/// Runtime call-site identifier used by metadata, exception, and profiling
/// tables.
#[derive(Clone, Copy, Debug, Default, Eq, PartialEq, Hash, Ord, PartialOrd)]
#[repr(transparent)]
pub struct CallSiteIndex(pub u32);

#[derive(Clone, Copy, Debug, Eq, PartialEq, Hash)]
pub enum CodeKind {
    Program,
    Eval,
    Function,
    Module,
}

#[derive(Clone, Copy, Debug, Default, Eq, PartialEq, Hash)]
pub enum ParseMode {
    #[default]
    Program,
    Module,
    Eval,
    NormalFunction,
    ArrowFunction,
    Method,
    Getter,
    Setter,
    ClassFieldInitializer,
    ClassStaticBlock,
    GeneratorBody,
    AsyncFunctionBody,
    AsyncGeneratorBody,
}

#[derive(Clone, Copy, Debug, Default, Eq, PartialEq, Hash)]
pub enum ScriptMode {
    #[default]
    Classic,
    Module,
}

#[derive(Clone, Copy, Debug, Default, Eq, PartialEq, Hash)]
pub enum EvalContext {
    #[default]
    None,
    Direct,
    Indirect,
}

/// Stable source identity and offsets for an executable.
///
/// The source text itself remains owned by the parser/source-provider layer.
/// Bytecode keeps handles and offsets so stack traces, debugging, code cache
/// keys, and source notes can resolve provenance without copying the program.
#[derive(Clone, Debug, Default, Eq, PartialEq)]
pub struct SourceProvenance {
    pub provider_id: Option<SourceProviderId>,
    pub origin: Option<SourceOriginId>,
    pub start_offset: u32,
    pub source_length: u32,
    pub first_line: u32,
    pub start_column: u32,
    pub source_url: Option<String>,
    pub pre_redirect_url: Option<String>,
}

/// Canonical source-provider identity for bytecode, runtime, and cache keys.
///
/// The provider registry owns allocation and lifetime. Syntax `SourceProvider`
/// values are parse-time storage; all persistent cross-component references use
/// this ID instead of raw pointers or local integer aliases.
#[derive(Clone, Copy, Debug, Eq, PartialEq, Hash)]
#[repr(transparent)]
pub struct SourceProviderId(pub u64);

/// Source-origin metadata record identity.
///
/// This qualifies URL/directive metadata associated with a provider but does
/// not replace `SourceProviderId` as provider identity.
#[derive(Clone, Copy, Debug, Eq, PartialEq, Hash)]
#[repr(transparent)]
pub struct SourceOriginId(pub u64);

#[derive(Clone, Debug, Default, Eq, PartialEq)]
pub struct SourceDirectiveTable {
    pub source_url_directive: Option<String>,
    pub source_mapping_url_directive: Option<String>,
}

#[derive(Clone, Copy, Debug, Default, Eq, PartialEq, Hash)]
pub struct SourcePosition {
    pub offset: u32,
    pub line: u32,
    pub column: u32,
}

#[derive(Clone, Copy, Debug, Default, Eq, PartialEq, Hash)]
pub struct SourceRange {
    pub start: SourcePosition,
    pub end: SourcePosition,
}

#[derive(Clone, Copy, Debug, Default, Eq, PartialEq, Hash)]
pub struct CodeFeatures {
    pub uses_arguments: bool,
    pub uses_eval: bool,
    pub uses_import_meta: bool,
    pub has_captured_variables: bool,
    pub has_tail_calls: bool,
    pub has_checkpoints: bool,
    pub no_eval_cache: bool,
    pub has_non_simple_parameters: bool,
}

#[derive(Clone, Copy, Debug, Default, Eq, PartialEq, Hash)]
pub struct CodeGenerationModeSet {
    pub debugger: bool,
    pub type_profiler: bool,
    pub control_flow_profiler: bool,
    pub collect_liveness: bool,
}

#[derive(Clone, Copy, Debug, Default, Eq, PartialEq, Hash)]
pub enum ConstructorKind {
    #[default]
    None,
    Base,
    Derived,
}

#[derive(Clone, Copy, Debug, Default, Eq, PartialEq, Hash)]
pub enum SuperBinding {
    #[default]
    NotNeeded,
    Needed,
}

#[derive(Clone, Copy, Debug, Default, Eq, PartialEq, Hash)]
pub enum PrivateBrandRequirement {
    #[default]
    None,
    Needed,
}

#[derive(Clone, Copy, Debug, Default, Eq, PartialEq, Hash)]
pub enum NeedsClassFieldInitializer {
    #[default]
    No,
    Yes,
}

#[derive(Clone, Copy, Debug, Default, Eq, PartialEq, Hash)]
pub enum DerivedContextType {
    #[default]
    None,
    DerivedConstructor,
    DerivedMethod,
}

#[derive(Clone, Copy, Debug, Default, Eq, PartialEq, Hash)]
pub enum EvalContextType {
    #[default]
    None,
    FunctionEval,
    InstanceFieldEval,
}

#[derive(Clone, Debug, Default, Eq, PartialEq)]
pub struct ExecutableInfo {
    pub parse_mode: Option<ParseMode>,
    pub script_mode: Option<ScriptMode>,
    pub eval_context: EvalContext,
    pub constructor_kind: ConstructorKind,
    pub super_binding: SuperBinding,
    pub private_brand_requirement: PrivateBrandRequirement,
    pub needs_class_field_initializer: NeedsClassFieldInitializer,
    pub derived_context: DerivedContextType,
    pub eval_context_type: EvalContextType,
    pub is_builtin_function: bool,
    pub is_builtin_default_class_constructor: bool,
    pub is_class_context: bool,
    pub is_arrow_function_context: bool,
    pub is_constructor: bool,
    pub is_strict_mode: bool,
}

/// Lifecycle of a source-derived unlinked code block.
///
/// Only the bytecompiler and unlinked-code-block generator may move a block
/// through generation phases. Once finalized, the block is immutable and can be
/// shared by code cache entries and linked code blocks.
#[derive(Clone, Copy, Debug, Default, Eq, PartialEq, Hash)]
pub enum UnlinkedCodeBlockPhase {
    #[default]
    Allocated,
    RecordingParse,
    EmittingInstructions,
    FinalizingSideTables,
    Finalized,
    Cached,
    DetachedForVmTeardown,
}

/// Source-derived reusable code artifact.
///
/// `UnlinkedCodeBlock` owns frozen instructions, generated metadata layout,
/// constants, identifiers, nested unlinked functions, source notes, handlers,
/// and jump tables. It deliberately does not own VM, global-object, scope,
/// inline-cache, executable edge, or JIT state.
#[derive(Clone, Debug)]
pub struct UnlinkedCodeBlock {
    kind: CodeKind,
    executable_info: ExecutableInfo,
    source: SourceProvenance,
    directives: SourceDirectiveTable,
    instructions: PackedInstructionStream,
    metadata: UnlinkedMetadataTable,
    constants: UnlinkedConstantPool,
    side_tables: UnlinkedSideTables,
    frame: RegisterFrameShape,
    features: CodeFeatures,
    generation_modes: CodeGenerationModeSet,
    tiering_hints: UnlinkedTieringHints,
    phase: UnlinkedCodeBlockPhase,
}

impl UnlinkedCodeBlock {
    pub fn new(kind: CodeKind, instructions: PackedInstructionStream) -> Self {
        Self {
            kind,
            executable_info: ExecutableInfo::default(),
            source: SourceProvenance::default(),
            directives: SourceDirectiveTable::default(),
            instructions,
            metadata: UnlinkedMetadataTable::default(),
            constants: UnlinkedConstantPool::default(),
            side_tables: UnlinkedSideTables::default(),
            frame: RegisterFrameShape::default(),
            features: CodeFeatures::default(),
            generation_modes: CodeGenerationModeSet::default(),
            tiering_hints: UnlinkedTieringHints::default(),
            phase: UnlinkedCodeBlockPhase::Allocated,
        }
    }

    pub fn with_source(mut self, source: SourceProvenance) -> Self {
        self.source = source;
        self
    }

    pub fn with_executable_info(mut self, info: ExecutableInfo) -> Self {
        self.executable_info = info;
        self
    }

    pub fn with_features(mut self, features: CodeFeatures) -> Self {
        self.features = features;
        self
    }

    pub fn with_generation_modes(mut self, modes: CodeGenerationModeSet) -> Self {
        self.generation_modes = modes;
        self
    }

    pub fn with_frame(mut self, frame: RegisterFrameShape) -> Self {
        self.frame = frame;
        self
    }

    pub fn with_metadata(mut self, metadata: UnlinkedMetadataTable) -> Self {
        self.metadata = metadata;
        self
    }

    pub fn with_constants(mut self, constants: UnlinkedConstantPool) -> Self {
        self.constants = constants;
        self
    }

    pub fn with_string_literals(
        mut self,
        entries: impl IntoIterator<Item = (u32, String)>,
    ) -> Self {
        self.constants.install_string_literals(entries);
        self
    }

    pub fn with_side_tables(mut self, side_tables: UnlinkedSideTables) -> Self {
        self.side_tables = side_tables;
        self
    }

    pub fn with_phase(mut self, phase: UnlinkedCodeBlockPhase) -> Self {
        self.phase = phase;
        self
    }

    pub fn kind(&self) -> CodeKind {
        self.kind
    }

    pub fn executable_info(&self) -> &ExecutableInfo {
        &self.executable_info
    }

    pub fn source(&self) -> &SourceProvenance {
        &self.source
    }

    pub fn directives(&self) -> &SourceDirectiveTable {
        &self.directives
    }

    pub fn instructions(&self) -> &PackedInstructionStream {
        &self.instructions
    }

    pub fn metadata(&self) -> &UnlinkedMetadataTable {
        &self.metadata
    }

    pub fn constants(&self) -> &UnlinkedConstantPool {
        &self.constants
    }

    pub fn string_literals(&self) -> &StringLiteralTable {
        self.constants.string_literals()
    }

    pub fn string_literal(&self, identifier_index: u32) -> Option<&str> {
        self.constants.string_literal(identifier_index)
    }

    pub fn side_tables(&self) -> &UnlinkedSideTables {
        &self.side_tables
    }

    pub fn frame(&self) -> RegisterFrameShape {
        self.frame
    }

    pub fn features(&self) -> CodeFeatures {
        self.features
    }

    pub fn generation_modes(&self) -> CodeGenerationModeSet {
        self.generation_modes
    }

    pub fn tiering_hints(&self) -> UnlinkedTieringHints {
        self.tiering_hints
    }

    pub fn phase(&self) -> UnlinkedCodeBlockPhase {
        self.phase
    }

    pub fn decoded_instruction_at(
        &self,
        bytecode_index: BytecodeIndex,
    ) -> Result<DecodedInstruction<'_>, InstructionDecodeError> {
        self.instructions.decoded_at(bytecode_index)
    }
}

/// Runtime mutation authority for linked code-block state.
///
/// This is distinct from Rust borrow permissions: linked metadata, ICs,
/// counters, and entrypoints may require VM locks, GC barriers, or executable
/// memory coordination even when a Rust reference is available.
#[derive(Clone, Copy, Debug, Default, Eq, PartialEq, Hash)]
pub enum CodeBlockMutationAuthority {
    #[default]
    VmMainThread,
    ConcurrentJsLocker,
    GcVisitor,
    JitCodeOwner,
    ReadOnlyObserver,
}

#[derive(Clone, Debug, Eq, PartialEq)]
pub enum CodeBlockMutationError {
    InvalidMutationAuthority {
        expected: CodeBlockMutationAuthority,
        actual: CodeBlockMutationAuthority,
    },
    InvalidLifecycle {
        expected: CodeBlockLifecycleState,
        actual: CodeBlockLifecycleState,
    },
    ValueProfileSample(ValueProfileSampleError),
}

#[derive(Clone, Copy, Debug, Default, Eq, PartialEq, Hash)]
pub enum CodeBlockLifecycleState {
    #[default]
    Allocated,
    Linking,
    LinkedInterpreter,
    BaselineInstalled,
    OptimizingInstalled,
    Jettisoned,
    Finalizing,
    Destructed,
}

/// Runtime-linked bytecode and mutable execution metadata.
///
/// A linked `CodeBlock` has VM/global/scope ownership, copied or barriered
/// constants, mutable metadata, interpreter/JIT entry surfaces, and tiering
/// counters. The unlinked block remains immutable and can be shared by cache
/// hits or multiple specializations.
#[derive(Clone, Debug)]
pub struct CodeBlock {
    unlinked: Arc<UnlinkedCodeBlock>,
    link_context: LinkContext,
    metadata: MetadataTable,
    constants: LinkedConstantPool,
    side_tables: LinkedSideTables,
    tier_state: CodeBlockTierState,
    entrypoints: CodeBlockEntrypoints,
    // C++ JSC divergence (install-time interior mutability): C++ `installCode`
    // (ScriptExecutable.cpp:121-186) mutates `m_jitCode`/`m_jitType` and the
    // executable's tier/lifecycle IN PLACE through the shared `CodeBlock*`/
    // `ScriptExecutable*` under the VM lock; there is no per-call copy. Rust
    // shares one `Rc<CodeBlock>` (see `CodeBlockRecord`), so the three
    // install-time fields baseline-JIT-installed mutates — `lifecycle`,
    // `tier_state.current_tier`, and `entrypoints.baseline_jit` — are wrapped in
    // `Cell` so `install_baseline_jit_slot` can mutate through `&self`. Only
    // these three Copy fields are interior-mutable; the rest of `tier_state`/
    // `entrypoints` stay plain data, so only the readers of these three fields
    // use `.get()`, not every accessor of the parent structs.
    lifecycle: Cell<CodeBlockLifecycleState>,
    mutation_authority: CodeBlockMutationAuthority,
    // Memoized baseline-bytecode snapshot fingerprint.
    //
    // C++ JSC divergence: C++ JSC never re-hashes the bytecode stream to decide
    // an artifact is valid. It establishes identity once at install/link and
    // validates on the hot path by cheap pointer/enum/generation reads:
    // CodeBlock::m_jitCode + const JITCode::m_jitType (CodeBlock.h:361,
    // JITCode.h:256,308), PropertyInlineCache::m_cacheType + watchpoint/GC
    // invalidation (PropertyInlineCache.h:207,391), and
    // CallLinkInfo::isLinked()'s stored Mode (CallLinkInfo.h:124). Rust instead
    // guards/keys baseline artifacts with
    // `baseline_bytecode_snapshot_fingerprint_from_code_block`, which the
    // profiler showed was a per-call O(N-instructions) re-hash on the hottest
    // dispatch paths. Everything that fingerprint hashes is immutable after this
    // CodeBlock is registered (the unlinked instruction stream + string literals
    // live behind `Arc<UnlinkedCodeBlock>` with no `&mut` accessor; exception
    // handler counts are link-fixed; root_map owner is stamped once at register;
    // and the property-IC term hashes only a structural projection that the IC
    // attach/clear mutators never flip). So Rust caches the compute-once value
    // here, matching the C++ "establish identity once at link, never recompute"
    // model. `Cell` gives interior-mutability memoization under the shared
    // `&CodeBlock` borrow the hot fingerprint callers hold (the VM is
    // single-threaded, `CodeBlockMutationAuthority::VmMainThread`). The builders
    // (`&mut self`) and the per-call interior-mutable feedback mutators (`&self`,
    // writing the `RefCell` IC table) that touch a hashed field call
    // `invalidate_snapshot_fingerprint()` so the memo tracks the actual mutation
    // surface rather than relying on the current structural-invariance fact.
    snapshot_fingerprint: Cell<Option<BaselineBytecodeSnapshotFingerprint>>,
    // C++ JSC map (install-time interior mutability): the per-CodeBlock baseline
    // data-IC record store, mirroring C++ `CodeBlock::m_jitData`
    // (CodeBlock.h:1002), a `BaselineJITData*` (BaselineJITCode.h:118)
    // allocated once in `setupWithUnlinkedBaselineCode` and freed only when
    // baseline code is discarded. Like the three Copy install-time fields above
    // and the `inline_caches` `RefCell`, the shared `Rc<CodeBlock>` cannot take
    // `&mut self` at baseline install, so the slot is interior-mutable. `None`
    // before baseline install; `install_baseline_jit_data` allocates the `Box`
    // exactly once and it is never reallocated, so its base address is stable
    // for the lifetime of the baseline code (later IC misses mutate records in
    // place). `RefCell` rather than `Cell` because callers need shared borrows
    // of the records, and the `Box` itself is `Clone` for the derived
    // `CodeBlock: Clone` (a clone copies the records into a fresh allocation,
    // matching how the cloned `inline_caches` `RefCell` already behaves).
    baseline_jit_data: RefCell<Option<Box<BaselineJitData>>>,
}

impl CodeBlock {
    pub fn from_unlinked(unlinked: UnlinkedCodeBlock, context: LinkContext) -> Self {
        Self::from_shared_unlinked(Arc::new(unlinked), context)
    }

    pub fn from_shared_unlinked(unlinked: Arc<UnlinkedCodeBlock>, context: LinkContext) -> Self {
        let constants = linked_constants_from_unlinked(&unlinked);
        let side_tables = linked_side_tables_from_unlinked(&unlinked);
        Self {
            unlinked,
            link_context: context,
            metadata: MetadataTable::default(),
            constants,
            side_tables,
            tier_state: CodeBlockTierState::default(),
            entrypoints: CodeBlockEntrypoints::default(),
            lifecycle: Cell::new(CodeBlockLifecycleState::Linking),
            mutation_authority: CodeBlockMutationAuthority::VmMainThread,
            snapshot_fingerprint: Cell::new(None),
            baseline_jit_data: RefCell::new(None),
        }
    }

    pub fn unlinked(&self) -> &UnlinkedCodeBlock {
        &self.unlinked
    }

    pub fn shared_unlinked(&self) -> &Arc<UnlinkedCodeBlock> {
        &self.unlinked
    }

    pub fn link_context(&self) -> &LinkContext {
        &self.link_context
    }

    pub fn metadata(&self) -> &MetadataTable {
        &self.metadata
    }

    pub fn constants(&self) -> &LinkedConstantPool {
        &self.constants
    }

    pub fn string_literals(&self) -> &StringLiteralTable {
        self.unlinked.string_literals()
    }

    pub fn string_literal(&self, identifier_index: u32) -> Option<&str> {
        self.unlinked.string_literal(identifier_index)
    }

    pub fn side_tables(&self) -> &LinkedSideTables {
        &self.side_tables
    }

    /// Return the memoized baseline-bytecode snapshot fingerprint, computing it
    /// once via `compute` on first use and caching the result.
    ///
    /// See the `snapshot_fingerprint` field comment for the C++ JSC divergence:
    /// this replaces a per-call O(N) re-hash with compute-once identity, mirroring
    /// C++ JSC's "establish identity at link, validate by pointer/enum/watchpoint"
    /// model. Only the success value is cached; a decode failure is deterministic
    /// on the immutable instruction stream, is not on the hot path, and simply
    /// recomputes (and fails again) the next time.
    pub(crate) fn cached_baseline_snapshot_fingerprint<E>(
        &self,
        compute: impl FnOnce() -> Result<BaselineBytecodeSnapshotFingerprint, E>,
    ) -> Result<BaselineBytecodeSnapshotFingerprint, E> {
        if let Some(cached) = self.snapshot_fingerprint.get() {
            return Ok(cached);
        }
        let fingerprint = compute()?;
        self.snapshot_fingerprint.set(Some(fingerprint));
        Ok(fingerprint)
    }

    /// Drop the memoized snapshot fingerprint so the next read recomputes it.
    ///
    /// Called from every `&mut` path that touches a hashed field so the memo
    /// tracks the actual mutation surface rather than relying on the current
    /// invariant that the hashed projection happens to stay constant.
    fn invalidate_snapshot_fingerprint(&self) {
        self.snapshot_fingerprint.set(None);
    }

    pub fn array_profile_for_bytecode_index(
        &self,
        bytecode_index: BytecodeIndex,
    ) -> Option<ArrayProfile> {
        self.side_tables
            .array_profiles
            .borrow()
            .iter()
            .find(|profile| profile.bytecode_index == bytecode_index)
            .copied()
    }

    /// The analog of `CodeBlock::binaryArithProfileForBytecodeIndex`
    /// (CodeBlock.cpp:3500-3503): C++ resolves the instruction's
    /// `m_profileIndex` argument into `UnlinkedCodeBlock::m_binaryArithProfiles`
    /// (CodeBlock.cpp:3510-3527); the Rust slots key by `BytecodeIndex`
    /// directly (see `BinaryArithProfileSlot`).
    pub fn binary_arith_profile_for_bytecode_index(
        &self,
        bytecode_index: BytecodeIndex,
    ) -> Option<BinaryArithProfile> {
        self.side_tables
            .binary_arith_profiles
            .borrow()
            .iter()
            .find(|slot| slot.bytecode_index == bytecode_index)
            .map(|slot| slot.profile)
    }

    /// The analog of `CodeBlock::unaryArithProfileForBytecodeIndex`
    /// (CodeBlock.cpp:3505-3508, resolving through
    /// `unaryArithProfileForPC`, CodeBlock.cpp:3529-3545).
    pub fn unary_arith_profile_for_bytecode_index(
        &self,
        bytecode_index: BytecodeIndex,
    ) -> Option<UnaryArithProfile> {
        self.side_tables
            .unary_arith_profiles
            .borrow()
            .iter()
            .find(|slot| slot.bytecode_index == bytecode_index)
            .map(|slot| slot.profile)
    }

    /// Read the monomorphic GET inline cache for a GetByName site, if filled.
    ///
    /// Mirrors loading `OpGetById::Metadata::m_modeMetadata` before
    /// `performGetByIDHelper` runs (LowLevelInterpreter64.asm:1676). Returns the
    /// `Copy` record so the caller does not hold the `RefCell` borrow across the
    /// fast path (and across the slow path's `&mut self` reentry).
    pub fn llint_get_by_id_cache(&self, bytecode_index: BytecodeIndex) -> LLIntGetByIdCache {
        self.side_tables
            .llint_get_by_id_caches
            .borrow()
            .get(&bytecode_index)
            .copied()
            .unwrap_or_default()
    }

    /// Install/overwrite the monomorphic GET cache for a GetByName site,
    /// mirroring the `metadata.defaultMode.*` writes in `performLLIntGetByID`
    /// (LLIntSlowPaths.cpp:945). Interior-mutable through `&self`.
    pub fn set_llint_get_by_id_cache(
        &self,
        bytecode_index: BytecodeIndex,
        metadata: LLIntGetByIdCache,
    ) {
        self.side_tables
            .llint_get_by_id_caches
            .borrow_mut()
            .insert(bytecode_index, metadata);
    }

    /// Read the monomorphic PUT (replace-existing) inline cache for a PutByName
    /// site, if filled. Mirrors loading `OpPutById::Metadata` before the LLInt
    /// put fast path (LowLevelInterpreter64.asm op_put_by_id). Returns a clone:
    /// the record carries a non-`Copy` cached key (O(1) for the common
    /// `Identifier` key).
    pub fn llint_put_by_id_cache(&self, bytecode_index: BytecodeIndex) -> LLIntPutByIdCache {
        self.side_tables
            .llint_put_by_id_caches
            .borrow()
            .get(&bytecode_index)
            .cloned()
            .unwrap_or_default()
    }

    /// Install/overwrite the monomorphic PUT cache for a PutByName site,
    /// mirroring the `metadata.m_oldStructureID`/`m_offset` writes in
    /// `slow_path_put_by_id` (LLIntSlowPaths.cpp:1436). Interior-mutable through
    /// `&self`.
    pub fn set_llint_put_by_id_cache(
        &self,
        bytecode_index: BytecodeIndex,
        metadata: LLIntPutByIdCache,
    ) {
        self.side_tables
            .llint_put_by_id_caches
            .borrow_mut()
            .insert(bytecode_index, metadata);
    }

    pub fn tier_state(&self) -> &CodeBlockTierState {
        &self.tier_state
    }

    pub fn value_profile_emission_policy(&self) -> ValueProfileEmissionPolicy {
        ValueProfileEmissionPolicy::from_capability(
            self.tier_state.value_profile_emission_capability,
        )
    }

    pub fn entrypoints(&self) -> &CodeBlockEntrypoints {
        &self.entrypoints
    }

    pub fn lifecycle(&self) -> CodeBlockLifecycleState {
        self.lifecycle.get()
    }

    pub fn mutation_authority(&self) -> CodeBlockMutationAuthority {
        self.mutation_authority
    }

    pub fn with_metadata(mut self, metadata: MetadataTable) -> Self {
        self.metadata = metadata;
        self
    }

    pub fn with_entrypoints(mut self, entrypoints: CodeBlockEntrypoints) -> Self {
        self.entrypoints = entrypoints;
        self
    }

    pub fn with_side_tables(mut self, side_tables: LinkedSideTables) -> Self {
        self.side_tables = side_tables;
        // Replaces the side tables (root_maps + property-IC table feed the hashed
        // side-table term); drop any memo cached against the old tables.
        self.invalidate_snapshot_fingerprint();
        self
    }

    pub(crate) fn with_root_map_owner(mut self, owner: CodeBlockId) -> Self {
        self.stamp_root_map_owner(owner);
        self
    }

    pub(crate) fn stamp_root_map_owner(&mut self, owner: CodeBlockId) {
        for root_map in &mut self.side_tables.root_maps {
            root_map.owner = Some(owner);
        }
        // root_map.owner is a hashed field; clone+restamp (e.g. the stale-block
        // re-registration path) would otherwise carry a stale memo from the clone.
        self.invalidate_snapshot_fingerprint();
    }

    pub fn with_tier_state(mut self, tier_state: CodeBlockTierState) -> Self {
        self.tier_state = tier_state;
        let emission_policy = self.value_profile_emission_policy();
        {
            let mut value_profiles = self.side_tables.value_profiles.borrow_mut();
            if value_profiles.emission_policy != emission_policy {
                value_profiles.emission_policy = emission_policy;
                value_profiles.materialize_jit_storage_from_profiles();
            }
        }
        // Value profiles are not hashed today, but this builder mutates side_tables
        // and runs during construction; invalidate defensively so no half-built memo
        // survives.
        self.invalidate_snapshot_fingerprint();
        self
    }

    pub fn with_lifecycle(self, lifecycle: CodeBlockLifecycleState) -> Self {
        self.lifecycle.set(lifecycle);
        self
    }

    // C++ JSC divergence (install through shared instance): C++ `installCode`
    // (ScriptExecutable.cpp:121-186) installs the baseline JIT artifact by
    // mutating `m_jitCode`/`m_jitType` and the executable's tier/lifecycle IN
    // PLACE through the shared `CodeBlock*`, never by copying. Rust shares one
    // `Rc<CodeBlock>`, so this mutates the three Copy install-time fields
    // (`entrypoints.baseline_jit`, `tier_state.current_tier`, `lifecycle`)
    // through their `Cell`s under `&self`, exactly mirroring the in-place install.
    pub fn install_baseline_jit_slot(
        &self,
        authority: CodeBlockMutationAuthority,
        slot: JitCodeSlot,
    ) -> Result<(), CodeBlockMutationError> {
        let expected_authority = CodeBlockMutationAuthority::VmMainThread;
        if authority != expected_authority {
            return Err(CodeBlockMutationError::InvalidMutationAuthority {
                expected: expected_authority,
                actual: authority,
            });
        }
        if self.mutation_authority != expected_authority {
            return Err(CodeBlockMutationError::InvalidMutationAuthority {
                expected: expected_authority,
                actual: self.mutation_authority,
            });
        }

        let expected_lifecycle = CodeBlockLifecycleState::LinkedInterpreter;
        if self.lifecycle.get() != expected_lifecycle {
            return Err(CodeBlockMutationError::InvalidLifecycle {
                expected: expected_lifecycle,
                actual: self.lifecycle.get(),
            });
        }

        self.entrypoints.baseline_jit.set(Some(slot));
        self.tier_state.current_tier.set(ExecutionTier::BaselineJit);
        self.lifecycle
            .set(CodeBlockLifecycleState::BaselineInstalled);
        Ok(())
    }

    // C++ JSC map: mirrors `CodeBlock::setupWithUnlinkedBaselineCode`
    // (CodeBlock.cpp:800-825), which allocates `BaselineJITData` once with
    // `propertyCacheSize = jitCode->m_unlinkedPropertyInlineCaches.size()` and
    // stores it in `m_jitData` (an allocate-once slot asserted empty:
    // `ASSERT(!m_jitData)` at CodeBlock.cpp:801). This takes `&self` because the
    // shared `Rc<CodeBlock>` install path has no `&mut self` (same in-place
    // install model as `install_baseline_jit_slot`), writing through the
    // interior-mutable `baseline_jit_data` `RefCell`. Allocates the `Box`
    // exactly once with `count` zero-initialized (sentinel) records; the `Box`
    // is never reallocated afterward, so its record-store base address is stable
    // and later IC misses mutate records in place. Returns the stable
    // record-store base address (what generated baseline code seeds into r13 =
    // `GPRInfo::jitDataRegister`), or `None` when there are zero IC sites
    // (`count == 0`), in which case generated code never dereferences the base.
    pub fn install_baseline_jit_data(
        &self,
        count: usize,
    ) -> Option<*const HandlerPropertyInlineCacheRecord> {
        let data = Box::new(BaselineJitData::from_property_cache_count(count));
        let base = if count == 0 {
            None
        } else {
            Some(data.record_store_base())
        };
        // Allocate-once contract: assert the slot was empty, mirroring C++
        // `ASSERT(!m_jitData)` (CodeBlock.cpp:801).
        let mut slot = self.baseline_jit_data.borrow_mut();
        debug_assert!(slot.is_none(), "baseline_jit_data installed more than once");
        *slot = Some(data);
        base
    }

    /// Borrow the installed baseline data-IC store, if any. `None` before
    /// baseline install. Mirrors a read of `CodeBlock::m_jitData`
    /// (CodeBlock.h:1002).
    pub fn baseline_jit_data(&self) -> std::cell::Ref<'_, Option<Box<BaselineJitData>>> {
        self.baseline_jit_data.borrow()
    }

    /// Stable base address of the installed baseline data-IC record store (the
    /// value generated baseline code seeds into r13 = `GPRInfo::jitDataRegister`),
    /// or `None` when no store is installed or the store has zero records.
    /// Stable because the `Box` is never reallocated after install.
    pub fn baseline_jit_data_record_store_base(
        &self,
    ) -> Option<*const HandlerPropertyInlineCacheRecord> {
        self.baseline_jit_data
            .borrow()
            .as_ref()
            .filter(|data| data.property_cache_count() != 0)
            .map(|data| data.record_store_base())
    }

    // STEP C: mirror a freshly cached own-data-load structure/offset into the
    // resident data-IC record store IN PLACE, so the next entry to the generated
    // `get_by_id` self-load fast path hits instead of taking the slow-path exit.
    //
    // C++ JSC map: when a baseline `GetByIdSelf` IC misses and the slow path resolves
    // a monomorphic own-data load, the IC's inline access fields are (re)written so the
    // structure-guarded fast path now matches -- the inline structure id /
    // PropertyOffset of `InlineAccess::generateSelfPropertyAccess`
    // (bytecode/InlineAccess.cpp:188-204) updated through the `BaselineJITData`
    // `HandlerPropertyInlineCache` (PropertyInlineCache.h:421-422). Here that is a
    // single in-place write to `records[record_index]` (the `Box` base is never
    // reallocated, so r13 still points at it; CodeBlock::Clone deep-copies the `Box`,
    // but the resident path mutates the registry-held Rc-shared instance, never a
    // clone). `record_index` is the dense per-site `property_site_index` the emitter
    // assigned in bytecode order, which equals the position of this site in the
    // property-handoff plan's sorted sites.
    //
    // Returns `true` when a record was written (store installed and index in range).
    // Out-of-range or no-store cases are no-ops: the fast path then keeps SENTINEL and
    // safely slow-paths, never reading a wrong record.
    pub fn mirror_self_load_data_ic_record(
        &self,
        record_index: usize,
        structure_id: u32,
        offset: i32,
    ) -> bool {
        // structure_id == 0 is the never-matching SENTINEL; refuse to write it so a
        // miss can never poison the record into a structure that aliases "no cache".
        if structure_id == 0 {
            return false;
        }
        let mut slot = self.baseline_jit_data.borrow_mut();
        let Some(data) = slot.as_mut() else {
            return false;
        };
        let Some(record) = data.property_caches.get_mut(record_index) else {
            return false;
        };
        record.structure_id = structure_id;
        record.offset = offset;
        // A self-load attach owns the whole record: clear any previously-baked
        // prototype holder so a site that transitions from a prototype load to an
        // own load can never read a stale holder pointer.
        record.holder_ptr = 0;
        true
    }

    // Arm a resident prototype-chain (holder) data-IC record IN PLACE so the next
    // entry to the generated DataIC fast path loads the property from the cached
    // prototype HOLDER instead of slow-pathing.
    //
    // C++ JSC map: mirrors `CacheType::GetByIdPrototype` filling the inline access
    // fields a baseline `get_by_id` prototype DataIC reads
    // (jit/JITInlineCacheGenerator.cpp:154-161): the receiver structure id
    // (`offsetOfInlineAccessBaseStructureID`), the holder property offset
    // (`offsetOfByIdSelfOffset`), and the constant holder pointer
    // (`offsetOfInlineHolder`). The holder is pinned valid by the
    // `m_conditionSet`/StructureTransition watchpoints (commit 6c035d6), not a
    // per-call re-guard, exactly as C++.
    //
    // `holder_ptr` is the raw, pinned `CoreObjectCell*` of the holder object,
    // resolved at the residency safepoint where the live objects store is
    // reachable (the registry/CodeBlock has no objects store of its own). LOAD-
    // BEARING: this must be reset to SENTINEL on a prototype shape change (the
    // StructureTransition-watchpoint retire path) so generated code never reads a
    // stale holder; see `reset_prototype_load_data_ic_record`.
    //
    // Returns `true` when a record was written. `structure_id == 0` (the
    // never-matching SENTINEL) and `holder_ptr == 0` (no holder) are refused so a
    // miss can never poison the record into "no cache" while claiming a holder, or
    // arm a prototype record with a null holder that the fast path would
    // dereference.
    pub fn mirror_prototype_load_data_ic_record(
        &self,
        record_index: usize,
        structure_id: u32,
        offset: i32,
        holder_ptr: u64,
    ) -> bool {
        if structure_id == 0 || holder_ptr == 0 {
            return false;
        }
        let mut slot = self.baseline_jit_data.borrow_mut();
        let Some(data) = slot.as_mut() else {
            return false;
        };
        let Some(record) = data.property_caches.get_mut(record_index) else {
            return false;
        };
        record.structure_id = structure_id;
        record.offset = offset;
        record.holder_ptr = holder_ptr;
        true
    }

    // Reset a resident data-IC record back to the never-matching SENTINEL
    // (structure_id == 0, holder_ptr == 0), so the next entry takes the slow-path
    // exit. Used on the StructureTransition-watchpoint retire / IC-clear path to
    // drop a baked prototype holder before its cell can become stale (a prototype
    // shape change), and by sidecar tests that must exercise the slow/miss path.
    // Returns `true` when a record was reset.
    pub fn reset_prototype_load_data_ic_record(&self, record_index: usize) -> bool {
        let mut slot = self.baseline_jit_data.borrow_mut();
        let Some(data) = slot.as_mut() else {
            return false;
        };
        let Some(record) = data.property_caches.get_mut(record_index) else {
            return false;
        };
        *record = crate::bytecode::ic::HandlerPropertyInlineCacheRecord::SENTINEL;
        true
    }

    // Test-only alias retained for the P11 sidecar tests that reset the self-load
    // record to force the slow/miss path. Identical to
    // `reset_prototype_load_data_ic_record` (both reset to SENTINEL).
    #[cfg(test)]
    pub fn reset_self_load_data_ic_record_to_sentinel(&self, record_index: usize) -> bool {
        self.reset_prototype_load_data_ic_record(record_index)
    }

    // Read a resident data-IC record by index (a COPY of the 16-byte
    // `{structure_id, offset, holder_ptr}`), or `None` before baseline install /
    // out of range. The DataIC slow-path bridge reads this on the HIT far-call to
    // recover the cached `PropertyOffset` for the cheap own-data load
    // (`operation_get_by_id_with_cached_offset`); the generated structure guard
    // has already proven the receiver's structure matches `record.structure_id`.
    pub fn baseline_property_ic_record(
        &self,
        record_index: usize,
    ) -> Option<crate::bytecode::ic::HandlerPropertyInlineCacheRecord> {
        self.baseline_jit_data
            .borrow()
            .as_ref()
            .and_then(|data| data.property_caches.get(record_index).copied())
    }

    // SQ4 churn cap: record one slow-path miss on a property IC site and return
    // whether it is STILL eligible to cache (count below
    // `BASELINE_PROPERTY_IC_CHURN_CAP`). The DataIC slow-path bridge calls this on
    // every optimize miss; once it returns `false` the bridge stops re-filling the
    // record (leaving/forcing SENTINEL) so a polymorphic/uncacheable site routes
    // to the slow path every time instead of thrashing the cached structure. The
    // faithful `StructureStubInfo` countdown -> give-up analog (Repatch.cpp).
    pub fn note_baseline_property_ic_slow_path(&self, record_index: usize) -> bool {
        let mut slot = self.baseline_jit_data.borrow_mut();
        let Some(data) = slot.as_mut() else {
            return false;
        };
        data.note_slow_path_and_should_cache(record_index)
    }

    // C++ JSC divergence: value-profile sampling mutates `m_metadata` through the
    // shared `CodeBlock*` on the hot return path, with no per-call copy. Rust shares
    // one `Rc<CodeBlock>`, so this takes `&self` and writes through the
    // interior-mutable `value_profiles` `RefCell`. Value profiles are not hashed by
    // the snapshot fingerprint, so no memo invalidation is needed here.
    pub fn record_value_profile_sample(
        &self,
        authority: CodeBlockMutationAuthority,
        bytecode_index: BytecodeIndex,
        checkpoint: Checkpoint,
        kind: ValueProfileBucketKind,
        value: JsValue,
    ) -> Result<ValueProfileBucketSample, CodeBlockMutationError> {
        let expected_authority = CodeBlockMutationAuthority::VmMainThread;
        if authority != expected_authority {
            return Err(CodeBlockMutationError::InvalidMutationAuthority {
                expected: expected_authority,
                actual: authority,
            });
        }
        if self.mutation_authority != expected_authority {
            return Err(CodeBlockMutationError::InvalidMutationAuthority {
                expected: expected_authority,
                actual: self.mutation_authority,
            });
        }

        match self.lifecycle.get() {
            CodeBlockLifecycleState::LinkedInterpreter
            | CodeBlockLifecycleState::BaselineInstalled => {}
            actual => {
                return Err(CodeBlockMutationError::InvalidLifecycle {
                    expected: CodeBlockLifecycleState::LinkedInterpreter,
                    actual,
                });
            }
        }

        self.side_tables
            .value_profiles_mut()
            .record_sample(bytecode_index, checkpoint, kind, value)
            .map_err(CodeBlockMutationError::ValueProfileSample)
    }

    // C++ JSC: `op_in_by_val` passes `&metadata.m_arrayProfile` to
    // `opInByVal`, which calls `ArrayProfile::observeIndexedRead` in-place
    // through the shared `CodeBlock*` metadata (CommonSlowPaths.h:105-119;
    // CodeBlock::getArrayProfile returns that same per-bytecode metadata slot,
    // CodeBlock.cpp:2911-2932). Rust shares one `Rc<CodeBlock>`, so this mirrors
    // value-profile feedback and mutates the linked metadata table through `&self`.
    pub fn record_array_profile_indexed_read(
        &self,
        authority: CodeBlockMutationAuthority,
        bytecode_index: BytecodeIndex,
        structure: StructureId,
        out_of_bounds: bool,
    ) -> Result<Option<ArrayProfile>, CodeBlockMutationError> {
        let expected_authority = CodeBlockMutationAuthority::VmMainThread;
        if authority != expected_authority {
            return Err(CodeBlockMutationError::InvalidMutationAuthority {
                expected: expected_authority,
                actual: authority,
            });
        }
        if self.mutation_authority != expected_authority {
            return Err(CodeBlockMutationError::InvalidMutationAuthority {
                expected: expected_authority,
                actual: self.mutation_authority,
            });
        }

        match self.lifecycle.get() {
            CodeBlockLifecycleState::LinkedInterpreter
            | CodeBlockLifecycleState::BaselineInstalled => {}
            actual => {
                return Err(CodeBlockMutationError::InvalidLifecycle {
                    expected: CodeBlockLifecycleState::LinkedInterpreter,
                    actual,
                });
            }
        }

        let mut array_profiles = self.side_tables.array_profiles.borrow_mut();
        let Some(profile) = array_profiles
            .iter_mut()
            .find(|profile| profile.bytecode_index == bytecode_index)
        else {
            return Ok(None);
        };
        profile.observe_indexed_read(structure, out_of_bounds);
        Ok(Some(*profile))
    }

    // C++ JSC: the LLInt `arrayProfile` macro stores the base cell's structureID
    // into the op's `ArrayProfile::m_lastSeenStructureID` on EVERY execution whose
    // base is a cell (llint/LowLevelInterpreter.asm:1447-1450; get_by_val
    // LowLevelInterpreter64.asm:1857, get_length :1715, put_by_val :2047), and the
    // C++ slow paths do the same through `observeStructure`/`observeStructureID`
    // (bytecode/ArrayProfile.h:244-245; llint/LLIntSlowPaths.cpp:982-983), all
    // mutating the shared `CodeBlock*` metadata in place. Rust shares one
    // `Rc<CodeBlock>`, so this mirrors `record_array_profile_indexed_read` and
    // mutates the linked slot through `&self`.
    pub fn record_array_profile_structure_seen(
        &self,
        authority: CodeBlockMutationAuthority,
        bytecode_index: BytecodeIndex,
        structure: StructureId,
    ) -> Result<Option<ArrayProfile>, CodeBlockMutationError> {
        self.check_profile_mutation_authority(authority)?;
        let mut array_profiles = self.side_tables.array_profiles.borrow_mut();
        let Some(profile) = array_profiles
            .iter_mut()
            .find(|profile| profile.bytecode_index == bytecode_index)
        else {
            return Ok(None);
        };
        profile.observe_structure_id(structure);
        Ok(Some(*profile))
    }

    // C++ JSC: `ArrayProfile::setOutOfBounds` (bytecode/ArrayProfile.h:242) fires
    // through the shared `CodeBlock*` metadata on the out-of-bounds access paths
    // (get_by_val slow path, llint/LLIntSlowPaths.cpp:1241/1257/1265; put_by_val
    // `.opPutByValOutOfBounds`, llint/LowLevelInterpreter64.asm:2112-2114).
    pub fn record_array_profile_out_of_bounds(
        &self,
        authority: CodeBlockMutationAuthority,
        bytecode_index: BytecodeIndex,
    ) -> Result<Option<ArrayProfile>, CodeBlockMutationError> {
        self.check_profile_mutation_authority(authority)?;
        let mut array_profiles = self.side_tables.array_profiles.borrow_mut();
        let Some(profile) = array_profiles
            .iter_mut()
            .find(|profile| profile.bytecode_index == bytecode_index)
        else {
            return Ok(None);
        };
        profile.set_out_of_bounds();
        Ok(Some(*profile))
    }

    // C++ JSC: `ArrayProfileFlag::MayStoreHole` (bytecode/ArrayProfile.h:205) is
    // ORed in place through the shared metadata on the put_by_val hole paths (the
    // contiguous beyond-publicLength extend, llint/LowLevelInterpreter64.asm:
    // 2033-2038; the ArrayStorage hole fill, :2102-2104).
    pub fn record_array_profile_may_store_to_hole(
        &self,
        authority: CodeBlockMutationAuthority,
        bytecode_index: BytecodeIndex,
    ) -> Result<Option<ArrayProfile>, CodeBlockMutationError> {
        self.check_profile_mutation_authority(authority)?;
        let mut array_profiles = self.side_tables.array_profiles.borrow_mut();
        let Some(profile) = array_profiles
            .iter_mut()
            .find(|profile| profile.bytecode_index == bytecode_index)
        else {
            return Ok(None);
        };
        profile.set_may_store_to_hole();
        Ok(Some(*profile))
    }

    // Shared authority/lifecycle gate for the runtime feedback-profile writes
    // (the checks every `record_*` profile mutation performs; see
    // `record_array_profile_indexed_read` for the C++ mutation-through-
    // `CodeBlock*` mapping).
    fn check_profile_mutation_authority(
        &self,
        authority: CodeBlockMutationAuthority,
    ) -> Result<(), CodeBlockMutationError> {
        let expected_authority = CodeBlockMutationAuthority::VmMainThread;
        if authority != expected_authority {
            return Err(CodeBlockMutationError::InvalidMutationAuthority {
                expected: expected_authority,
                actual: authority,
            });
        }
        if self.mutation_authority != expected_authority {
            return Err(CodeBlockMutationError::InvalidMutationAuthority {
                expected: expected_authority,
                actual: self.mutation_authority,
            });
        }
        match self.lifecycle.get() {
            CodeBlockLifecycleState::LinkedInterpreter
            | CodeBlockLifecycleState::BaselineInstalled => Ok(()),
            actual => Err(CodeBlockMutationError::InvalidLifecycle {
                expected: CodeBlockLifecycleState::LinkedInterpreter,
                actual,
            }),
        }
    }

    // C++ JSC: the binary arith slow paths fetch the site's profile through the
    // instruction's `m_profileIndex` and mutate it in place through the shared
    // `CodeBlock*` (`updateArithProfileForBinaryArithOp` calls
    // `profile.observeLHSAndRHS(left, right)` before setting the result bits,
    // CommonSlowPaths.cpp:470-530; `BinaryArithProfile::observeLHSAndRHS`,
    // ArithProfile.h:366-380). Rust shares one `Rc<CodeBlock>`, so this mirrors
    // `record_array_profile_indexed_read` and mutates the linked slot through
    // `&self`. Storage/derivation unit only: no interpreter caller yet.
    pub fn record_binary_arith_profile_operands(
        &self,
        authority: CodeBlockMutationAuthority,
        bytecode_index: BytecodeIndex,
        lhs: JsValue,
        rhs: JsValue,
    ) -> Result<Option<BinaryArithProfile>, CodeBlockMutationError> {
        self.check_profile_mutation_authority(authority)?;
        let mut slots = self.side_tables.binary_arith_profiles.borrow_mut();
        let Some(slot) = slots
            .iter_mut()
            .find(|slot| slot.bytecode_index == bytecode_index)
        else {
            return Ok(None);
        };
        slot.profile.observe_lhs_and_rhs(lhs, rhs);
        Ok(Some(slot.profile))
    }

    // C++ JSC: the result half of binary arith profiling
    // (`ArithProfile::observeResult`, ArithProfile.h:128-145, reached through
    // the same shared-`CodeBlock*` profile as
    // `record_binary_arith_profile_operands`).
    pub fn record_binary_arith_profile_result(
        &self,
        authority: CodeBlockMutationAuthority,
        bytecode_index: BytecodeIndex,
        result: JsValue,
    ) -> Result<Option<BinaryArithProfile>, CodeBlockMutationError> {
        self.check_profile_mutation_authority(authority)?;
        let mut slots = self.side_tables.binary_arith_profiles.borrow_mut();
        let Some(slot) = slots
            .iter_mut()
            .find(|slot| slot.bytecode_index == bytecode_index)
        else {
            return Ok(None);
        };
        slot.profile.arith_mut().observe_result(result);
        Ok(Some(slot.profile))
    }

    // C++ JSC: `updateArithProfileForBinaryArithOp` (CommonSlowPaths.cpp:470-502)
    // — the RESULT half of the interpreter binary-arith slow paths. Unlike the
    // coarse `ArithProfile::observeResult` (ArithProfile.h:128-145, kept above as
    // `record_binary_arith_profile_result` — that is what the baseline-JIT
    // profiled operations call), the interpreter slow path distinguishes
    // Int32Overflow (only when BOTH operands were int32), NegZero vs NonNegZero
    // doubles, and the 2^51 Int52 overflow point.
    //
    // `result_is_heap_big_int`: C++ tests `result.isHeapBigInt()`
    // (CommonSlowPaths.cpp:494). The Rust value model does not yet classify
    // BigInt cells on `JsValue` (same transitional gap commented at
    // `ArithProfile::observe_result`, profiling.rs), so the interpreter host —
    // which owns the BigInt store — passes the classification in. The
    // `#if USE(BIGINT32)` arm (CommonSlowPaths.cpp:496-499) is omitted: the
    // local C++ baseline builds with USE(BIGINT32) off and the Rust engine has
    // no BigInt32 inline representation.
    pub fn record_binary_arith_profile_binary_op_result(
        &self,
        authority: CodeBlockMutationAuthority,
        bytecode_index: BytecodeIndex,
        result: JsValue,
        left: JsValue,
        right: JsValue,
        result_is_heap_big_int: bool,
    ) -> Result<Option<BinaryArithProfile>, CodeBlockMutationError> {
        self.check_profile_mutation_authority(authority)?;
        let mut slots = self.side_tables.binary_arith_profiles.borrow_mut();
        let Some(slot) = slots
            .iter_mut()
            .find(|slot| slot.bytecode_index == bytecode_index)
        else {
            return Ok(None);
        };
        let profile = slot.profile.arith_mut();
        match result.as_number() {
            Some(number) => {
                // CommonSlowPaths.cpp:474-475: an int32 result records nothing.
                if !result.is_int32() {
                    // CommonSlowPaths.cpp:476-477.
                    if left.is_int32() && right.is_int32() {
                        profile.set_observed_int32_overflow();
                    }
                    let double_val = match number {
                        NumberValue::DoubleBits(bits) => bits.to_f64(),
                        // Unreachable: the `!result.is_int32()` guard above
                        // filtered the Int32 arm.
                        NumberValue::Int32(value) => f64::from(value),
                    };
                    // CommonSlowPaths.cpp:480-481
                    // (`!doubleVal && std::signbit(doubleVal)`).
                    if double_val == 0.0 && double_val.is_sign_negative() {
                        profile.set_observed_neg_zero_double();
                    } else {
                        profile.set_observed_non_neg_zero_double();
                        // CommonSlowPaths.cpp:483-491. C++ `truncateDoubleToInt64`
                        // compiles to a saturating fcvtzs on the arm64 baseline
                        // (NaN -> 0); Rust's `as i64` has the same saturating
                        // semantics.
                        const INT52_OVERFLOW_POINT: i64 = 1i64 << 51;
                        let int64_val = double_val.abs() as i64;
                        if int64_val >= INT52_OVERFLOW_POINT {
                            profile.set_observed_int52_overflow();
                        }
                    }
                }
            }
            // CommonSlowPaths.cpp:493-494.
            None if result_is_heap_big_int => profile.set_observed_heap_big_int(),
            // CommonSlowPaths.cpp:500-501.
            None => profile.set_observed_non_numeric(),
        }
        Ok(Some(slot.profile))
    }

    // C++ JSC: `updateArithProfileForUnaryArithOp` starts with
    // `profile.observeArg(operand)` (CommonSlowPaths.cpp:396-428;
    // `UnaryArithProfile::observeArg`, ArithProfile.h:243-255), mutating the
    // shared profile in place through `CodeBlock*`.
    pub fn record_unary_arith_profile_arg(
        &self,
        authority: CodeBlockMutationAuthority,
        bytecode_index: BytecodeIndex,
        arg: JsValue,
    ) -> Result<Option<UnaryArithProfile>, CodeBlockMutationError> {
        self.check_profile_mutation_authority(authority)?;
        let mut slots = self.side_tables.unary_arith_profiles.borrow_mut();
        let Some(slot) = slots
            .iter_mut()
            .find(|slot| slot.bytecode_index == bytecode_index)
        else {
            return Ok(None);
        };
        slot.profile.observe_arg(arg);
        Ok(Some(slot.profile))
    }

    // C++ JSC: the result half of unary arith profiling
    // (`ArithProfile::observeResult`, ArithProfile.h:128-145, via the shared
    // profile reached in `record_unary_arith_profile_arg`).
    pub fn record_unary_arith_profile_result(
        &self,
        authority: CodeBlockMutationAuthority,
        bytecode_index: BytecodeIndex,
        result: JsValue,
    ) -> Result<Option<UnaryArithProfile>, CodeBlockMutationError> {
        self.check_profile_mutation_authority(authority)?;
        let mut slots = self.side_tables.unary_arith_profiles.borrow_mut();
        let Some(slot) = slots
            .iter_mut()
            .find(|slot| slot.bytecode_index == bytecode_index)
        else {
            return Ok(None);
        };
        slot.profile.arith_mut().observe_result(result);
        Ok(Some(slot.profile))
    }

    // C++ JSC: `updateArithProfileForUnaryArithOp(profile, result, operand)`
    // (CommonSlowPaths.cpp:396-429) as invoked by `slow_path_negate`
    // (CommonSlowPaths.cpp:434-467) on the shared profile reached through
    // `CodeBlock*`; the shape logic lives on
    // `UnaryArithProfile::update_for_unary_arith_op`. `result_is_heap_big_int`
    // supplies the C++ `result.isHeapBigInt()` classification the transitional
    // Rust value model cannot derive from `JsValue` bits (BigInt identity lives
    // in the interpreter's BigIntStore).
    pub fn record_unary_arith_profile_unary_arith_op(
        &self,
        authority: CodeBlockMutationAuthority,
        bytecode_index: BytecodeIndex,
        result: JsValue,
        operand: JsValue,
        result_is_heap_big_int: bool,
    ) -> Result<Option<UnaryArithProfile>, CodeBlockMutationError> {
        self.check_profile_mutation_authority(authority)?;
        let mut slots = self.side_tables.unary_arith_profiles.borrow_mut();
        let Some(slot) = slots
            .iter_mut()
            .find(|slot| slot.bytecode_index == bytecode_index)
        else {
            return Ok(None);
        };
        slot.profile
            .update_for_unary_arith_op(result, operand, result_is_heap_big_int);
        Ok(Some(slot.profile))
    }

    // C++ JSC divergence: IC attach mutates the shared `CodeBlock`'s metadata
    // through `CodeBlock*`. Rust shares one `Rc<CodeBlock>`, so this takes `&self`
    // and mutates through the interior-mutable `inline_caches` `RefCell`.
    pub fn attach_property_inline_cache_case(
        &self,
        authority: CodeBlockMutationAuthority,
        request: PropertyInlineCacheAttachmentRequest,
    ) -> PropertyInlineCacheAttachmentResult {
        let expected_authority = CodeBlockMutationAuthority::VmMainThread;
        if authority != expected_authority {
            return Err(
                PropertyInlineCacheAttachmentError::InvalidMutationAuthority {
                    expected: expected_authority,
                    actual: authority,
                },
            );
        }
        if self.mutation_authority != expected_authority {
            return Err(
                PropertyInlineCacheAttachmentError::InvalidMutationAuthority {
                    expected: expected_authority,
                    actual: self.mutation_authority,
                },
            );
        }

        match self.lifecycle.get() {
            CodeBlockLifecycleState::LinkedInterpreter
            | CodeBlockLifecycleState::BaselineInstalled => {}
            actual => {
                return Err(PropertyInlineCacheAttachmentError::InvalidLifecycle { actual });
            }
        }

        validate_property_inline_cache_attachment_request(
            &self.side_tables.inline_caches.borrow(),
            &request,
        )?;

        // The property-IC table feeds the hashed side-table term; drop the memo so
        // the next fingerprint read reflects this attach. The hashed projection is
        // invariant under attach today, but invalidating keeps the memo tracking the
        // real mutation surface (see the field comment).
        self.invalidate_snapshot_fingerprint();

        let mut inline_caches = self.side_tables.inline_caches.borrow_mut();
        let (state, dispatch) = {
            let cache = &mut inline_caches.property_accesses[request.slot];
            cache.state = InlineCacheState::Monomorphic;
            cache.dispatch = request.dispatch;
            match request.attachment_kind {
                PropertyInlineCacheAttachmentKind::GetByNameOwnDataLoad => {
                    let offset = request
                        .offset
                        .expect("own data load attachment validation requires an offset");
                    cache.get_by_id = Some(GetByIdModeMetadata {
                        mode: GetByIdMode::Default,
                        structure: Some(request.base_structure),
                        holder_structure: None,
                        cached_offset: Some(offset),
                        cached_slot: None,
                        hit_count_for_llint_caching: 0,
                    });
                }
                PropertyInlineCacheAttachmentKind::GetByNamePrototypeDataLoad => {
                    let offset = request
                        .offset
                        .expect("prototype data load attachment validation requires an offset");
                    cache.get_by_id = Some(GetByIdModeMetadata {
                        mode: GetByIdMode::ProtoLoad,
                        structure: Some(request.base_structure),
                        holder_structure: request.holder_structure,
                        cached_offset: Some(offset),
                        cached_slot: None,
                        hit_count_for_llint_caching: 0,
                    });
                }
                PropertyInlineCacheAttachmentKind::GetByNameNegativeLookup => {
                    cache.get_by_id = Some(GetByIdModeMetadata {
                        mode: GetByIdMode::NegativeLookup,
                        structure: Some(request.base_structure),
                        holder_structure: None,
                        cached_offset: None,
                        cached_slot: None,
                        hit_count_for_llint_caching: 0,
                    });
                }
                PropertyInlineCacheAttachmentKind::PutByNameStoreReplace
                | PropertyInlineCacheAttachmentKind::PutByNameStoreTransition => {
                    let offset = request
                        .offset
                        .expect("store attachment validation requires an offset");
                    cache.put_by_id = Some(PutByIdModeMetadata {
                        mode: request
                            .attachment_kind
                            .put_by_id_mode()
                            .expect("store attachment kind has a put-by-id mode"),
                        old_structure: Some(request.base_structure),
                        new_structure: request.new_structure,
                        cached_offset: Some(offset),
                    });
                }
            }
            (cache.state, cache.dispatch)
        };

        let structure_stub_index =
            if request.stub_mode == PropertyInlineCacheStubMode::StructureStub {
                let index = inline_caches.structure_stubs.len();
                inline_caches.structure_stubs.push(
                    structure_stub_info_for_property_inline_cache_request(&request),
                );
                Some(index)
            } else {
                None
            };

        Ok(PropertyInlineCacheAttachmentOutcome {
            slot: request.slot,
            bytecode_index: request.bytecode_index,
            key: request.key,
            attachment_kind: request.attachment_kind,
            state,
            dispatch,
            base_structure: request.base_structure,
            holder_structure: request.holder_structure,
            new_structure: request.new_structure,
            offset: request.offset,
            stub_mode: request.stub_mode,
            structure_stub_index,
        })
    }

    // C++ JSC divergence: IC clear mutates the shared `CodeBlock`'s metadata
    // through `CodeBlock*`; Rust mutates through the interior-mutable
    // `inline_caches` `RefCell` under `&self` since the instance is shared by `Rc`.
    pub fn clear_property_inline_cache_case(
        &self,
        authority: CodeBlockMutationAuthority,
        request: PropertyInlineCacheClearRequest,
    ) -> PropertyInlineCacheClearResult {
        let expected_authority = CodeBlockMutationAuthority::VmMainThread;
        if authority != expected_authority {
            return Err(PropertyInlineCacheClearError::InvalidMutationAuthority {
                expected: expected_authority,
                actual: authority,
            });
        }
        if self.mutation_authority != expected_authority {
            return Err(PropertyInlineCacheClearError::InvalidMutationAuthority {
                expected: expected_authority,
                actual: self.mutation_authority,
            });
        }

        match self.lifecycle.get() {
            CodeBlockLifecycleState::LinkedInterpreter
            | CodeBlockLifecycleState::BaselineInstalled => {}
            actual => {
                return Err(PropertyInlineCacheClearError::InvalidLifecycle { actual });
            }
        }

        validate_property_inline_cache_clear_request(
            &self.side_tables.inline_caches.borrow(),
            &request,
        )?;

        // The property-IC table feeds the hashed side-table term; drop the memo so
        // the next fingerprint read reflects this clear. The hashed projection is
        // invariant under clear today, but invalidating keeps the memo tracking the
        // real mutation surface (see the field comment).
        self.invalidate_snapshot_fingerprint();

        let mut inline_caches = self.side_tables.inline_caches.borrow_mut();
        let cache = &mut inline_caches.property_accesses[request.slot];
        // Metadata-only clears return the slot to the cold interpreter state. There is
        // no native patch to disable here, and `Unset` allows a later validated
        // observation to attach fresh metadata instead of preserving stale guards.
        cache.state = InlineCacheState::Unset;
        cache.dispatch = PropertyInlineCacheDispatch::Unlinked;
        match request.attachment_kind {
            PropertyInlineCacheAttachmentKind::GetByNameOwnDataLoad
            | PropertyInlineCacheAttachmentKind::GetByNamePrototypeDataLoad
            | PropertyInlineCacheAttachmentKind::GetByNameNegativeLookup => {
                cache.get_by_id = Some(GetByIdModeMetadata {
                    mode: GetByIdMode::Unset,
                    structure: None,
                    holder_structure: None,
                    cached_offset: None,
                    cached_slot: None,
                    hit_count_for_llint_caching: 0,
                });
            }
            PropertyInlineCacheAttachmentKind::PutByNameStoreReplace
            | PropertyInlineCacheAttachmentKind::PutByNameStoreTransition => {
                cache.put_by_id = Some(PutByIdModeMetadata {
                    mode: PutByIdMode::Default,
                    old_structure: None,
                    new_structure: None,
                    cached_offset: None,
                });
            }
        }
        let cache_state = cache.state;
        let cache_dispatch = cache.dispatch;
        if let Some(structure_stub_index) = request.structure_stub_index {
            let stub = &mut inline_caches.structure_stubs[structure_stub_index];
            stub.cache_state = InlineCacheState::Unset;
            stub.access_cases.clear();
        }

        Ok(PropertyInlineCacheClearOutcome {
            slot: request.slot,
            bytecode_index: request.bytecode_index,
            key: request.key,
            attachment_kind: request.attachment_kind,
            state: cache_state,
            dispatch: cache_dispatch,
            base_structure: request.base_structure,
            holder_structure: request.holder_structure,
            new_structure: request.new_structure,
            offset: request.offset,
            stub_mode: request.stub_mode,
            structure_stub_index: request.structure_stub_index,
        })
    }

    // C++ JSC divergence: structure-stub linking mutates the shared `CodeBlock`'s
    // metadata through `CodeBlock*`; Rust mutates through the interior-mutable
    // `inline_caches` `RefCell` under `&self` since the instance is shared by `Rc`.
    pub fn link_structure_stub_access_case(
        &self,
        authority: CodeBlockMutationAuthority,
        request: StructureStubAccessCaseLinkRequest,
    ) -> StructureStubAccessCaseLinkResult {
        let expected_authority = CodeBlockMutationAuthority::VmMainThread;
        if authority != expected_authority {
            return Err(StructureStubAccessCaseLinkError::InvalidMutationAuthority {
                expected: expected_authority,
                actual: authority,
            });
        }
        if self.mutation_authority != expected_authority {
            return Err(StructureStubAccessCaseLinkError::InvalidMutationAuthority {
                expected: expected_authority,
                actual: self.mutation_authority,
            });
        }

        match self.lifecycle.get() {
            CodeBlockLifecycleState::LinkedInterpreter
            | CodeBlockLifecycleState::BaselineInstalled => {}
            actual => {
                return Err(StructureStubAccessCaseLinkError::InvalidLifecycle { actual });
            }
        }

        validate_structure_stub_access_case_link_request(
            &self.side_tables.inline_caches.borrow(),
            &request,
        )?;

        let mut inline_caches = self.side_tables.inline_caches.borrow_mut();
        let stub = &mut inline_caches.structure_stubs[request.structure_stub_index];
        let inserted = if stub.access_cases.contains(&request.access_case_ref) {
            false
        } else {
            stub.access_cases.push(request.access_case_ref);
            true
        };
        let access_case_count = stub.access_cases.len();

        Ok(StructureStubAccessCaseLinkOutcome {
            structure_stub_index: request.structure_stub_index,
            bytecode_index: request.bytecode_index,
            slot: request.slot,
            key: request.key,
            attachment_kind: request.attachment_kind,
            base_structure: request.base_structure,
            holder_structure: request.holder_structure,
            new_structure: request.new_structure,
            offset: request.offset,
            access_case_ref: request.access_case_ref,
            inserted,
            access_case_count,
        })
    }

    pub fn attached_property_inline_cache_metadata(
        &self,
        request: PropertyInlineCacheAttachedMetadataRequest,
    ) -> PropertyInlineCacheAttachedMetadataResult {
        match self.lifecycle.get() {
            CodeBlockLifecycleState::LinkedInterpreter
            | CodeBlockLifecycleState::BaselineInstalled => {}
            actual => {
                return Err(PropertyInlineCacheAttachedMetadataError::InvalidLifecycle { actual });
            }
        }

        validate_property_inline_cache_attached_metadata_request(
            &self.side_tables.inline_caches.borrow(),
            &request,
        )?;

        Ok(PropertyInlineCacheAttachedMetadata {
            slot: request.slot,
            bytecode_index: request.bytecode_index,
            key: request.key,
            attachment_kind: request.attachment_kind,
            base_structure: request.base_structure,
            holder_structure: request.holder_structure,
            new_structure: request.new_structure,
            offset: request.offset,
            dispatch: request.dispatch,
            stub_mode: request.stub_mode,
        })
    }

    // C++ JSC divergence: call-link IC attach mutates the shared `CodeBlock`'s
    // metadata through `CodeBlock*`; Rust mutates through the interior-mutable
    // `inline_caches` `RefCell` under `&self` since the instance is shared by `Rc`.
    pub fn attach_call_link_inline_cache(
        &self,
        authority: CodeBlockMutationAuthority,
        request: CallLinkInlineCacheAttachmentRequest,
    ) -> CallLinkInlineCacheAttachmentResult {
        let expected_authority = CodeBlockMutationAuthority::VmMainThread;
        if authority != expected_authority {
            return Err(
                CallLinkInlineCacheAttachmentError::InvalidMutationAuthority {
                    expected: expected_authority,
                    actual: authority,
                },
            );
        }
        if self.mutation_authority != expected_authority {
            return Err(
                CallLinkInlineCacheAttachmentError::InvalidMutationAuthority {
                    expected: expected_authority,
                    actual: self.mutation_authority,
                },
            );
        }

        match self.lifecycle.get() {
            CodeBlockLifecycleState::LinkedInterpreter
            | CodeBlockLifecycleState::BaselineInstalled => {}
            actual => {
                return Err(CallLinkInlineCacheAttachmentError::InvalidLifecycle { actual });
            }
        }

        validate_call_link_inline_cache_attachment_request(
            &self.side_tables.inline_caches.borrow(),
            self.unlinked.as_ref(),
            &request,
        )?;

        let target = call_target_for_call_link_inline_cache_attachment_request(&request);
        let mut inline_caches = self.side_tables.inline_caches.borrow_mut();
        let call = &mut inline_caches.calls[request.slot];
        // C++ `CallLinkInfo::setMonomorphicCallee` (CallLinkInfo.cpp:134-141).
        call.set_monomorphic_callee(target);
        let mode = call.mode;
        let target = call.target.clone();
        let slow_path_count = call.slow_path_count;
        let max_argument_count_including_this_for_varargs =
            call.max_argument_count_including_this_for_varargs;
        let call_site = call.call_site;
        let opcode = call.opcode;
        let call_type = call.call_type;
        let specialization = call.specialization;

        Ok(CallLinkInlineCacheAttachmentOutcome {
            slot: request.slot,
            bytecode_index: request.bytecode_index,
            call_site,
            opcode,
            call_type,
            mode,
            specialization,
            target,
            slow_path_count,
            max_argument_count_including_this_for_varargs,
        })
    }

    // C++ JSC divergence: call-link IC clear (relink/jettison) mutates the shared
    // `CodeBlock`'s metadata through `CodeBlock*`; Rust mutates through the
    // interior-mutable `inline_caches` `RefCell` under `&self` (instance shared by `Rc`).
    pub fn clear_call_link_inline_cache(
        &self,
        authority: CodeBlockMutationAuthority,
        request: CallLinkInlineCacheClearRequest,
    ) -> CallLinkInlineCacheClearResult {
        let expected_authority = CodeBlockMutationAuthority::VmMainThread;
        if authority != expected_authority {
            return Err(CallLinkInlineCacheClearError::InvalidMutationAuthority {
                expected: expected_authority,
                actual: authority,
            });
        }
        if self.mutation_authority != expected_authority {
            return Err(CallLinkInlineCacheClearError::InvalidMutationAuthority {
                expected: expected_authority,
                actual: self.mutation_authority,
            });
        }

        match self.lifecycle.get() {
            CodeBlockLifecycleState::LinkedInterpreter
            | CodeBlockLifecycleState::BaselineInstalled => {}
            actual => {
                return Err(CallLinkInlineCacheClearError::InvalidLifecycle { actual });
            }
        }

        validate_call_link_inline_cache_clear_request(
            &self.side_tables.inline_caches.borrow(),
            self.unlinked.as_ref(),
            &request,
        )?;

        let mut inline_caches = self.side_tables.inline_caches.borrow_mut();
        let (
            call_site,
            opcode,
            call_type,
            mode,
            specialization,
            target,
            slow_path_count,
            max_argument_count_including_this_for_varargs,
        ) = {
            let call = &mut inline_caches.calls[request.slot];
            let (call_type, specialization) = call_link_descriptor_shape_for_opcode(call.opcode)
                .ok_or(CallLinkInlineCacheClearError::UnsupportedOpcode {
                    slot: request.slot,
                    opcode: call.opcode,
                })?;
            // C++ `CallLinkInfo::reset(VM&)` (CallLinkInfo.cpp:258-268): clear
            // the cached callee and return the site to unlinked Init.
            call.reset_to_unlinked(call_type, specialization);
            (
                call.call_site,
                call.opcode,
                call.call_type,
                call.mode,
                call.specialization,
                call.target.clone(),
                call.slow_path_count,
                call.max_argument_count_including_this_for_varargs,
            )
        };

        Ok(CallLinkInlineCacheClearOutcome {
            slot: request.slot,
            bytecode_index: request.bytecode_index,
            call_site,
            opcode,
            call_type,
            mode,
            specialization,
            target,
            slow_path_count,
            max_argument_count_including_this_for_varargs,
        })
    }

    pub fn attached_call_link_inline_cache_metadata(
        &self,
        request: CallLinkInlineCacheAttachedMetadataRequest,
    ) -> CallLinkInlineCacheAttachedMetadataResult {
        match self.lifecycle.get() {
            CodeBlockLifecycleState::LinkedInterpreter
            | CodeBlockLifecycleState::BaselineInstalled => {}
            actual => {
                return Err(CallLinkInlineCacheAttachedMetadataError::InvalidLifecycle { actual });
            }
        }

        validate_call_link_inline_cache_attached_metadata_request(
            &self.side_tables.inline_caches.borrow(),
            self.unlinked.as_ref(),
            &request,
        )?;

        let inline_caches = self.side_tables.inline_caches.borrow();
        let call = &inline_caches.calls[request.slot];
        Ok(CallLinkInlineCacheAttachedMetadata {
            slot: request.slot,
            bytecode_index: request.bytecode_index,
            call_site: call.call_site,
            opcode: call.opcode,
            call_type: call.call_type,
            mode: call.mode,
            specialization: call.specialization,
            target: call.target.clone(),
            slow_path_count: call.slow_path_count,
            max_argument_count_including_this_for_varargs: call
                .max_argument_count_including_this_for_varargs,
        })
    }

    pub fn decoded_instruction_at(
        &self,
        bytecode_index: BytecodeIndex,
    ) -> Result<DecodedInstruction<'_>, InstructionDecodeError> {
        self.unlinked.decoded_instruction_at(bytecode_index)
    }

    pub fn metadata_entry_for_bytecode_index(
        &self,
        bytecode_index: BytecodeIndex,
    ) -> Option<&MetadataEntry> {
        self.metadata.entry_for_bytecode_index(bytecode_index)
    }

    pub fn execution_surface(&self) -> CodeBlockExecutionSurface<'_> {
        CodeBlockExecutionSurface { code_block: self }
    }

    pub fn link_record(&self) -> UnlinkedToLinkedBytecodeRecord {
        UnlinkedToLinkedBytecodeRecord {
            context: self.link_context.clone(),
            unlinked_phase: self.unlinked.phase(),
            linked_lifecycle: self.lifecycle.get(),
            instruction_count: self.unlinked.instructions().instruction_count() as u32,
            unlinked_metadata_entries: self.unlinked.metadata().entries().len() as u32,
            linked_metadata_entries: self.metadata.entries().len() as u32,
            root_map_count: self.side_tables.root_maps.len() as u32,
            value_profile_root_count: self.side_tables.value_profiles.borrow().root_metadata.len()
                as u32,
            has_interpreter_entry: self.entrypoints.interpreter.is_some(),
        }
    }
}

fn validate_property_inline_cache_attachment_request(
    inline_caches: &InlineCacheTable,
    request: &PropertyInlineCacheAttachmentRequest,
) -> Result<(), PropertyInlineCacheAttachmentError> {
    let Some(cache) = inline_caches.property_accesses.get(request.slot) else {
        return Err(PropertyInlineCacheAttachmentError::InvalidSlot {
            slot: request.slot,
            len: inline_caches.property_accesses.len(),
        });
    };

    if cache.bytecode_index != request.bytecode_index {
        return Err(PropertyInlineCacheAttachmentError::BytecodeIndexMismatch {
            slot: request.slot,
            expected: cache.bytecode_index,
            actual: request.bytecode_index,
        });
    }

    let expected_property = PropertyCacheKey::Key(request.key);
    if cache.property != expected_property {
        return Err(PropertyInlineCacheAttachmentError::PropertyKeyMismatch {
            slot: request.slot,
            expected: cache.property,
            actual: request.key,
        });
    }

    let expected_access = request.attachment_kind.access_type();
    let expected_kind = request.attachment_kind.cache_kind();
    if cache.access != expected_access || cache.kind != expected_kind {
        return Err(PropertyInlineCacheAttachmentError::AccessKindMismatch {
            slot: request.slot,
            expected_access,
            actual_access: cache.access,
            expected_kind,
            actual_kind: cache.kind,
        });
    }

    let expected_authority = InlineCacheMutationAuthority::LinkedCodeBlock;
    if cache.mutation_authority != expected_authority {
        return Err(
            PropertyInlineCacheAttachmentError::InvalidExistingMutationAuthority {
                slot: request.slot,
                expected: expected_authority,
                actual: cache.mutation_authority,
            },
        );
    }

    let expected_state = InlineCacheState::Unset;
    if cache.state != expected_state {
        return Err(PropertyInlineCacheAttachmentError::InvalidExistingState {
            slot: request.slot,
            expected: expected_state,
            actual: cache.state,
        });
    }

    let expected_dispatch = PropertyInlineCacheDispatch::Unlinked;
    if cache.dispatch != expected_dispatch {
        return Err(
            PropertyInlineCacheAttachmentError::InvalidExistingDispatch {
                slot: request.slot,
                expected: expected_dispatch,
                actual: cache.dispatch,
            },
        );
    }

    match request.attachment_kind {
        PropertyInlineCacheAttachmentKind::GetByNameOwnDataLoad
        | PropertyInlineCacheAttachmentKind::GetByNamePrototypeDataLoad
        | PropertyInlineCacheAttachmentKind::GetByNameNegativeLookup => {
            if cache.get_by_id.is_none() {
                return Err(PropertyInlineCacheAttachmentError::MissingGetByIdMetadata {
                    slot: request.slot,
                });
            }
        }
        PropertyInlineCacheAttachmentKind::PutByNameStoreReplace
        | PropertyInlineCacheAttachmentKind::PutByNameStoreTransition => {
            if cache.put_by_id.is_none() {
                return Err(PropertyInlineCacheAttachmentError::MissingPutByIdMetadata {
                    slot: request.slot,
                });
            }
        }
    }

    if request.dispatch == PropertyInlineCacheDispatch::Unlinked {
        return Err(
            PropertyInlineCacheAttachmentError::InvalidRequestedDispatch {
                actual: request.dispatch,
            },
        );
    }

    validate_property_inline_cache_attachment_stub_mode(request)?;

    match request.attachment_kind {
        PropertyInlineCacheAttachmentKind::GetByNameOwnDataLoad
        | PropertyInlineCacheAttachmentKind::GetByNamePrototypeDataLoad
        | PropertyInlineCacheAttachmentKind::GetByNameNegativeLookup
        | PropertyInlineCacheAttachmentKind::PutByNameStoreReplace
            if request.new_structure.is_some() =>
        {
            return Err(
                PropertyInlineCacheAttachmentError::IncompatibleNewStructure {
                    attachment_kind: request.attachment_kind,
                    new_structure: request.new_structure,
                },
            );
        }
        PropertyInlineCacheAttachmentKind::PutByNameStoreTransition
            if request.new_structure.is_none() =>
        {
            return Err(
                PropertyInlineCacheAttachmentError::IncompatibleNewStructure {
                    attachment_kind: request.attachment_kind,
                    new_structure: request.new_structure,
                },
            );
        }
        _ => {}
    }

    match request.attachment_kind {
        PropertyInlineCacheAttachmentKind::GetByNamePrototypeDataLoad => {
            if request.holder_structure.is_none() {
                return Err(PropertyInlineCacheAttachmentError::MissingHolderStructure {
                    attachment_kind: request.attachment_kind,
                });
            }
        }
        _ => {
            if let Some(holder_structure) = request.holder_structure {
                return Err(
                    PropertyInlineCacheAttachmentError::UnexpectedHolderStructure {
                        attachment_kind: request.attachment_kind,
                        holder_structure,
                    },
                );
            }
        }
    }

    match request.attachment_kind {
        PropertyInlineCacheAttachmentKind::GetByNameNegativeLookup => {
            if let Some(offset) = request.offset {
                return Err(
                    PropertyInlineCacheAttachmentError::UnexpectedPropertyOffset {
                        attachment_kind: request.attachment_kind,
                        offset,
                    },
                );
            }
        }
        _ => {
            let Some(offset) = request.offset else {
                return Err(PropertyInlineCacheAttachmentError::MissingPropertyOffset {
                    attachment_kind: request.attachment_kind,
                });
            };
            if offset.0 < 0 {
                return Err(PropertyInlineCacheAttachmentError::InvalidPropertyOffset { offset });
            }
        }
    }

    let request_requires_guarded_watchpoint = request.attachment_kind.is_guarded_get_by_name();
    if request.requirements.requires_watchpoint != request_requires_guarded_watchpoint {
        return Err(
            PropertyInlineCacheAttachmentError::WatchpointBridgeUnavailable {
                attachment_kind: request.attachment_kind,
            },
        );
    }

    if request.attachment_kind.is_store() {
        if !request.requirements.requires_barrier || !request.requirements.has_barrier_evidence {
            return Err(
                PropertyInlineCacheAttachmentError::MissingStoreBarrierEvidence {
                    attachment_kind: request.attachment_kind,
                },
            );
        }
    } else if request.requirements.requires_barrier {
        return Err(
            PropertyInlineCacheAttachmentError::UnexpectedBarrierRequirement {
                attachment_kind: request.attachment_kind,
            },
        );
    }

    Ok(())
}

fn validate_call_link_inline_cache_attachment_request(
    inline_caches: &InlineCacheTable,
    unlinked: &UnlinkedCodeBlock,
    request: &CallLinkInlineCacheAttachmentRequest,
) -> Result<(), CallLinkInlineCacheAttachmentError> {
    let Some(call) = inline_caches.calls.get(request.slot) else {
        return Err(CallLinkInlineCacheAttachmentError::InvalidSlot {
            slot: request.slot,
            len: inline_caches.calls.len(),
        });
    };

    if call.bytecode_index != request.bytecode_index {
        return Err(CallLinkInlineCacheAttachmentError::BytecodeIndexMismatch {
            slot: request.slot,
            expected: call.bytecode_index,
            actual: request.bytecode_index,
        });
    }
    validate_call_link_inline_cache_attachment_opcode(unlinked, request, call)?;
    validate_call_link_inline_cache_attachment_descriptor(request, call)?;
    validate_call_link_inline_cache_attachment_target(request)?;

    let expected_mode = CallLinkMode::Init;
    if call.mode != expected_mode {
        return Err(CallLinkInlineCacheAttachmentError::InvalidExistingMode {
            slot: request.slot,
            expected: expected_mode,
            actual: call.mode,
        });
    }
    if call.target != CallTarget::Unlinked {
        return Err(CallLinkInlineCacheAttachmentError::InvalidExistingTarget {
            slot: request.slot,
            actual: call.target.clone(),
        });
    }

    Ok(())
}

fn validate_call_link_inline_cache_attachment_opcode(
    unlinked: &UnlinkedCodeBlock,
    request: &CallLinkInlineCacheAttachmentRequest,
    call: &CallLinkInfo,
) -> Result<(), CallLinkInlineCacheAttachmentError> {
    let decoded =
        decoded_instruction_for_call_link_bytecode_index(unlinked, request.bytecode_index).ok_or(
            CallLinkInlineCacheAttachmentError::InstructionDecodeFailed {
                slot: request.slot,
                bytecode_index: request.bytecode_index,
            },
        )?;
    let Some(decoded_opcode) = CoreOpcode::from_opcode(decoded.opcode) else {
        return Err(
            CallLinkInlineCacheAttachmentError::InstructionOpcodeUnavailable {
                slot: request.slot,
                bytecode_index: request.bytecode_index,
            },
        );
    };
    if call_link_descriptor_shape_for_opcode(decoded_opcode).is_none() {
        return Err(CallLinkInlineCacheAttachmentError::UnsupportedOpcode {
            slot: request.slot,
            opcode: decoded_opcode,
        });
    }
    let decoded_call_site = call_site_for_decoded_call(&decoded);
    if call.call_site != decoded_call_site {
        return Err(CallLinkInlineCacheAttachmentError::CallSiteMismatch {
            slot: request.slot,
            expected: decoded_call_site,
            actual: call.call_site,
        });
    }
    if call.opcode != decoded_opcode {
        return Err(CallLinkInlineCacheAttachmentError::OpcodeMismatch {
            slot: request.slot,
            expected: decoded_opcode,
            actual: call.opcode,
        });
    }

    Ok(())
}

fn validate_call_link_inline_cache_attachment_descriptor(
    request: &CallLinkInlineCacheAttachmentRequest,
    call: &CallLinkInfo,
) -> Result<(), CallLinkInlineCacheAttachmentError> {
    let (expected_call_type, expected_specialization) = call_link_descriptor_shape_for_opcode(
        call.opcode,
    )
    .ok_or(CallLinkInlineCacheAttachmentError::UnsupportedOpcode {
        slot: request.slot,
        opcode: call.opcode,
    })?;
    if call.call_type != expected_call_type {
        return Err(
            CallLinkInlineCacheAttachmentError::InvalidExistingCallType {
                slot: request.slot,
                expected: expected_call_type,
                actual: call.call_type,
            },
        );
    }
    if call.specialization != expected_specialization {
        return Err(
            CallLinkInlineCacheAttachmentError::InvalidExistingSpecialization {
                slot: request.slot,
                expected: expected_specialization,
                actual: call.specialization,
            },
        );
    }
    let expected_origin = CodeOrigin::new(request.bytecode_index);
    if call.origin != expected_origin {
        return Err(CallLinkInlineCacheAttachmentError::OriginMismatch {
            slot: request.slot,
            expected: expected_origin,
            actual: call.origin,
        });
    }

    Ok(())
}

fn validate_call_link_inline_cache_attachment_target(
    request: &CallLinkInlineCacheAttachmentRequest,
) -> Result<(), CallLinkInlineCacheAttachmentError> {
    if !matches!(request.target, CallTarget::MetadataOnlyMonomorphic { .. }) {
        return Err(CallLinkInlineCacheAttachmentError::InvalidRequestedTarget {
            actual: request.target.clone(),
        });
    }

    Ok(())
}

fn validate_call_link_inline_cache_clear_request(
    inline_caches: &InlineCacheTable,
    unlinked: &UnlinkedCodeBlock,
    request: &CallLinkInlineCacheClearRequest,
) -> Result<(), CallLinkInlineCacheClearError> {
    let Some(call) = inline_caches.calls.get(request.slot) else {
        return Err(CallLinkInlineCacheClearError::InvalidSlot {
            slot: request.slot,
            len: inline_caches.calls.len(),
        });
    };

    if call.bytecode_index != request.bytecode_index {
        return Err(CallLinkInlineCacheClearError::BytecodeIndexMismatch {
            slot: request.slot,
            expected: call.bytecode_index,
            actual: request.bytecode_index,
        });
    }
    validate_call_link_inline_cache_clear_opcode(unlinked, request, call)?;
    validate_call_link_inline_cache_clear_descriptor(request, call)?;
    validate_call_link_inline_cache_clear_target(request)?;

    let expected_mode = CallLinkMode::Monomorphic;
    if call.mode != expected_mode {
        return Err(CallLinkInlineCacheClearError::InvalidExistingMode {
            slot: request.slot,
            expected: expected_mode,
            actual: call.mode,
        });
    }

    validate_call_link_inline_cache_clear_target_matches(request, call)?;

    Ok(())
}

fn validate_call_link_inline_cache_clear_opcode(
    unlinked: &UnlinkedCodeBlock,
    request: &CallLinkInlineCacheClearRequest,
    call: &CallLinkInfo,
) -> Result<(), CallLinkInlineCacheClearError> {
    let decoded =
        decoded_instruction_for_call_link_bytecode_index(unlinked, request.bytecode_index).ok_or(
            CallLinkInlineCacheClearError::InstructionDecodeFailed {
                slot: request.slot,
                bytecode_index: request.bytecode_index,
            },
        )?;
    let Some(decoded_opcode) = CoreOpcode::from_opcode(decoded.opcode) else {
        return Err(
            CallLinkInlineCacheClearError::InstructionOpcodeUnavailable {
                slot: request.slot,
                bytecode_index: request.bytecode_index,
            },
        );
    };
    if call_link_descriptor_shape_for_opcode(decoded_opcode).is_none() {
        return Err(CallLinkInlineCacheClearError::UnsupportedOpcode {
            slot: request.slot,
            opcode: decoded_opcode,
        });
    }
    let decoded_call_site = call_site_for_decoded_call(&decoded);
    if call.call_site != decoded_call_site {
        return Err(CallLinkInlineCacheClearError::CallSiteMismatch {
            slot: request.slot,
            expected: decoded_call_site,
            actual: call.call_site,
        });
    }
    if call.opcode != decoded_opcode {
        return Err(CallLinkInlineCacheClearError::OpcodeMismatch {
            slot: request.slot,
            expected: decoded_opcode,
            actual: call.opcode,
        });
    }

    Ok(())
}

fn validate_call_link_inline_cache_clear_descriptor(
    request: &CallLinkInlineCacheClearRequest,
    call: &CallLinkInfo,
) -> Result<(), CallLinkInlineCacheClearError> {
    let (expected_call_type, expected_specialization) = call_link_descriptor_shape_for_opcode(
        call.opcode,
    )
    .ok_or(CallLinkInlineCacheClearError::UnsupportedOpcode {
        slot: request.slot,
        opcode: call.opcode,
    })?;
    if call.call_type != expected_call_type {
        return Err(CallLinkInlineCacheClearError::InvalidExistingCallType {
            slot: request.slot,
            expected: expected_call_type,
            actual: call.call_type,
        });
    }
    if call.specialization != expected_specialization {
        return Err(
            CallLinkInlineCacheClearError::InvalidExistingSpecialization {
                slot: request.slot,
                expected: expected_specialization,
                actual: call.specialization,
            },
        );
    }
    let expected_origin = CodeOrigin::new(request.bytecode_index);
    if call.origin != expected_origin {
        return Err(CallLinkInlineCacheClearError::OriginMismatch {
            slot: request.slot,
            expected: expected_origin,
            actual: call.origin,
        });
    }

    Ok(())
}

fn validate_call_link_inline_cache_clear_target(
    request: &CallLinkInlineCacheClearRequest,
) -> Result<(), CallLinkInlineCacheClearError> {
    if !matches!(request.target, CallTarget::MetadataOnlyMonomorphic { .. }) {
        return Err(CallLinkInlineCacheClearError::InvalidRequestedTarget {
            actual: request.target.clone(),
        });
    }

    Ok(())
}

fn validate_call_link_inline_cache_clear_target_matches(
    request: &CallLinkInlineCacheClearRequest,
    call: &CallLinkInfo,
) -> Result<(), CallLinkInlineCacheClearError> {
    let (
        CallTarget::MetadataOnlyMonomorphic {
            callee,
            executable,
            code_block,
            boundary,
        },
        CallTarget::MetadataOnlyMonomorphic {
            callee: expected_callee,
            executable: expected_executable,
            code_block: expected_code_block,
            boundary: expected_boundary,
        },
    ) = (&call.target, &request.target)
    else {
        return Err(call_link_inline_cache_clear_mismatch(
            request.slot,
            CallLinkInlineCacheClearMetadataMismatchField::Callee,
        ));
    };
    if callee != expected_callee {
        return Err(call_link_inline_cache_clear_mismatch(
            request.slot,
            CallLinkInlineCacheClearMetadataMismatchField::Callee,
        ));
    }
    if executable != expected_executable {
        return Err(call_link_inline_cache_clear_mismatch(
            request.slot,
            CallLinkInlineCacheClearMetadataMismatchField::Executable,
        ));
    }
    if code_block != expected_code_block {
        return Err(call_link_inline_cache_clear_mismatch(
            request.slot,
            CallLinkInlineCacheClearMetadataMismatchField::TargetCodeBlock,
        ));
    }
    if boundary != expected_boundary {
        return Err(call_link_inline_cache_clear_mismatch(
            request.slot,
            CallLinkInlineCacheClearMetadataMismatchField::Boundary,
        ));
    }

    Ok(())
}

const fn call_link_inline_cache_clear_mismatch(
    slot: usize,
    field: CallLinkInlineCacheClearMetadataMismatchField,
) -> CallLinkInlineCacheClearError {
    CallLinkInlineCacheClearError::AttachedMetadataMismatch { slot, field }
}

fn validate_call_link_inline_cache_attached_metadata_request(
    inline_caches: &InlineCacheTable,
    unlinked: &UnlinkedCodeBlock,
    request: &CallLinkInlineCacheAttachedMetadataRequest,
) -> Result<(), CallLinkInlineCacheAttachedMetadataError> {
    let Some(call) = inline_caches.calls.get(request.slot) else {
        return Err(CallLinkInlineCacheAttachedMetadataError::InvalidSlot {
            slot: request.slot,
            len: inline_caches.calls.len(),
        });
    };

    if call.bytecode_index != request.bytecode_index {
        return Err(
            CallLinkInlineCacheAttachedMetadataError::BytecodeIndexMismatch {
                slot: request.slot,
                expected: call.bytecode_index,
                actual: request.bytecode_index,
            },
        );
    }
    validate_call_link_inline_cache_attached_metadata_opcode(unlinked, request, call)?;
    validate_call_link_inline_cache_attached_metadata_descriptor(request, call)?;
    validate_call_link_inline_cache_attached_metadata_target(request)?;

    let expected_mode = CallLinkMode::Monomorphic;
    if call.mode != expected_mode {
        return Err(
            CallLinkInlineCacheAttachedMetadataError::InvalidExistingMode {
                slot: request.slot,
                expected: expected_mode,
                actual: call.mode,
            },
        );
    }

    validate_call_link_inline_cache_attached_metadata_target_matches(request, call)?;

    Ok(())
}

fn validate_call_link_inline_cache_attached_metadata_opcode(
    unlinked: &UnlinkedCodeBlock,
    request: &CallLinkInlineCacheAttachedMetadataRequest,
    call: &CallLinkInfo,
) -> Result<(), CallLinkInlineCacheAttachedMetadataError> {
    let decoded =
        decoded_instruction_for_call_link_bytecode_index(unlinked, request.bytecode_index).ok_or(
            CallLinkInlineCacheAttachedMetadataError::InstructionDecodeFailed {
                slot: request.slot,
                bytecode_index: request.bytecode_index,
            },
        )?;
    let Some(decoded_opcode) = CoreOpcode::from_opcode(decoded.opcode) else {
        return Err(
            CallLinkInlineCacheAttachedMetadataError::InstructionOpcodeUnavailable {
                slot: request.slot,
                bytecode_index: request.bytecode_index,
            },
        );
    };
    if call_link_descriptor_shape_for_opcode(decoded_opcode).is_none() {
        return Err(
            CallLinkInlineCacheAttachedMetadataError::UnsupportedOpcode {
                slot: request.slot,
                opcode: decoded_opcode,
            },
        );
    }
    let decoded_call_site = call_site_for_decoded_call(&decoded);
    if call.call_site != decoded_call_site {
        return Err(CallLinkInlineCacheAttachedMetadataError::CallSiteMismatch {
            slot: request.slot,
            expected: decoded_call_site,
            actual: call.call_site,
        });
    }
    if call.opcode != decoded_opcode {
        return Err(CallLinkInlineCacheAttachedMetadataError::OpcodeMismatch {
            slot: request.slot,
            expected: decoded_opcode,
            actual: call.opcode,
        });
    }

    Ok(())
}

fn validate_call_link_inline_cache_attached_metadata_descriptor(
    request: &CallLinkInlineCacheAttachedMetadataRequest,
    call: &CallLinkInfo,
) -> Result<(), CallLinkInlineCacheAttachedMetadataError> {
    let (expected_call_type, expected_specialization) =
        call_link_descriptor_shape_for_opcode(call.opcode).ok_or(
            CallLinkInlineCacheAttachedMetadataError::UnsupportedOpcode {
                slot: request.slot,
                opcode: call.opcode,
            },
        )?;
    if call.call_type != expected_call_type {
        return Err(
            CallLinkInlineCacheAttachedMetadataError::InvalidExistingCallType {
                slot: request.slot,
                expected: expected_call_type,
                actual: call.call_type,
            },
        );
    }
    if call.specialization != expected_specialization {
        return Err(
            CallLinkInlineCacheAttachedMetadataError::InvalidExistingSpecialization {
                slot: request.slot,
                expected: expected_specialization,
                actual: call.specialization,
            },
        );
    }
    let expected_origin = CodeOrigin::new(request.bytecode_index);
    if call.origin != expected_origin {
        return Err(CallLinkInlineCacheAttachedMetadataError::OriginMismatch {
            slot: request.slot,
            expected: expected_origin,
            actual: call.origin,
        });
    }

    Ok(())
}

fn validate_call_link_inline_cache_attached_metadata_target(
    request: &CallLinkInlineCacheAttachedMetadataRequest,
) -> Result<(), CallLinkInlineCacheAttachedMetadataError> {
    if !matches!(request.target, CallTarget::MetadataOnlyMonomorphic { .. }) {
        return Err(
            CallLinkInlineCacheAttachedMetadataError::InvalidRequestedTarget {
                actual: request.target.clone(),
            },
        );
    }

    Ok(())
}

fn validate_call_link_inline_cache_attached_metadata_target_matches(
    request: &CallLinkInlineCacheAttachedMetadataRequest,
    call: &CallLinkInfo,
) -> Result<(), CallLinkInlineCacheAttachedMetadataError> {
    let (
        CallTarget::MetadataOnlyMonomorphic {
            callee,
            executable,
            code_block,
            boundary,
        },
        CallTarget::MetadataOnlyMonomorphic {
            callee: expected_callee,
            executable: expected_executable,
            code_block: expected_code_block,
            boundary: expected_boundary,
        },
    ) = (&call.target, &request.target)
    else {
        return Err(call_link_inline_cache_attached_metadata_mismatch(
            request.slot,
            CallLinkInlineCacheAttachedMetadataMismatchField::Callee,
        ));
    };
    if callee != expected_callee {
        return Err(call_link_inline_cache_attached_metadata_mismatch(
            request.slot,
            CallLinkInlineCacheAttachedMetadataMismatchField::Callee,
        ));
    }
    if executable != expected_executable {
        return Err(call_link_inline_cache_attached_metadata_mismatch(
            request.slot,
            CallLinkInlineCacheAttachedMetadataMismatchField::Executable,
        ));
    }
    if code_block != expected_code_block {
        return Err(call_link_inline_cache_attached_metadata_mismatch(
            request.slot,
            CallLinkInlineCacheAttachedMetadataMismatchField::TargetCodeBlock,
        ));
    }
    if boundary != expected_boundary {
        return Err(call_link_inline_cache_attached_metadata_mismatch(
            request.slot,
            CallLinkInlineCacheAttachedMetadataMismatchField::Boundary,
        ));
    }

    Ok(())
}

const fn call_link_inline_cache_attached_metadata_mismatch(
    slot: usize,
    field: CallLinkInlineCacheAttachedMetadataMismatchField,
) -> CallLinkInlineCacheAttachedMetadataError {
    CallLinkInlineCacheAttachedMetadataError::AttachedMetadataMismatch { slot, field }
}

fn call_target_for_call_link_inline_cache_attachment_request(
    request: &CallLinkInlineCacheAttachmentRequest,
) -> CallTarget {
    request.target.clone()
}

fn call_site_for_decoded_call(decoded: &DecodedInstruction<'_>) -> CallSiteIndex {
    decoded
        .operands
        .iter()
        .find_map(|operand| {
            if let Operand::CallSite(call_site) = operand {
                Some(*call_site)
            } else {
                None
            }
        })
        .unwrap_or(CallSiteIndex(decoded.bytecode_index.offset()))
}

fn decoded_instruction_for_call_link_bytecode_index(
    unlinked: &UnlinkedCodeBlock,
    bytecode_index: BytecodeIndex,
) -> Option<DecodedInstruction<'_>> {
    unlinked
        .instructions()
        .decoded_instructions()
        .flatten()
        .find(|decoded| decoded.bytecode_index == bytecode_index)
}

fn validate_property_inline_cache_clear_request(
    inline_caches: &InlineCacheTable,
    request: &PropertyInlineCacheClearRequest,
) -> Result<(), PropertyInlineCacheClearError> {
    let Some(cache) = inline_caches.property_accesses.get(request.slot) else {
        return Err(PropertyInlineCacheClearError::InvalidSlot {
            slot: request.slot,
            len: inline_caches.property_accesses.len(),
        });
    };

    if cache.bytecode_index != request.bytecode_index {
        return Err(PropertyInlineCacheClearError::BytecodeIndexMismatch {
            slot: request.slot,
            expected: cache.bytecode_index,
            actual: request.bytecode_index,
        });
    }

    let expected_property = PropertyCacheKey::Key(request.key);
    if cache.property != expected_property {
        return Err(PropertyInlineCacheClearError::PropertyKeyMismatch {
            slot: request.slot,
            expected: cache.property,
            actual: request.key,
        });
    }

    let expected_access = request.attachment_kind.access_type();
    let expected_kind = request.attachment_kind.cache_kind();
    if cache.access != expected_access || cache.kind != expected_kind {
        return Err(PropertyInlineCacheClearError::AccessKindMismatch {
            slot: request.slot,
            expected_access,
            actual_access: cache.access,
            expected_kind,
            actual_kind: cache.kind,
        });
    }

    let expected_authority = InlineCacheMutationAuthority::LinkedCodeBlock;
    if cache.mutation_authority != expected_authority {
        return Err(
            PropertyInlineCacheClearError::InvalidExistingMutationAuthority {
                slot: request.slot,
                expected: expected_authority,
                actual: cache.mutation_authority,
            },
        );
    }

    let expected_state = InlineCacheState::Monomorphic;
    if cache.state != expected_state {
        return Err(PropertyInlineCacheClearError::InvalidExistingState {
            slot: request.slot,
            expected: expected_state,
            actual: cache.state,
        });
    }

    if request.dispatch == PropertyInlineCacheDispatch::Unlinked {
        return Err(PropertyInlineCacheClearError::InvalidRequestedDispatch {
            actual: request.dispatch,
        });
    }
    if cache.dispatch != request.dispatch {
        return Err(PropertyInlineCacheClearError::InvalidExistingDispatch {
            slot: request.slot,
            expected: request.dispatch,
            actual: cache.dispatch,
        });
    }

    validate_property_inline_cache_clear_stub_mode(inline_caches, request)?;

    match request.attachment_kind {
        PropertyInlineCacheAttachmentKind::GetByNameOwnDataLoad
        | PropertyInlineCacheAttachmentKind::GetByNamePrototypeDataLoad
        | PropertyInlineCacheAttachmentKind::GetByNameNegativeLookup
        | PropertyInlineCacheAttachmentKind::PutByNameStoreReplace
            if request.new_structure.is_some() =>
        {
            return Err(PropertyInlineCacheClearError::IncompatibleNewStructure {
                attachment_kind: request.attachment_kind,
                new_structure: request.new_structure,
            });
        }
        PropertyInlineCacheAttachmentKind::PutByNameStoreTransition
            if request.new_structure.is_none() =>
        {
            return Err(PropertyInlineCacheClearError::IncompatibleNewStructure {
                attachment_kind: request.attachment_kind,
                new_structure: request.new_structure,
            });
        }
        _ => {}
    }

    match request.attachment_kind {
        PropertyInlineCacheAttachmentKind::GetByNamePrototypeDataLoad => {
            if request.holder_structure.is_none() {
                return Err(PropertyInlineCacheClearError::MissingHolderStructure {
                    attachment_kind: request.attachment_kind,
                });
            }
        }
        _ => {
            if let Some(holder_structure) = request.holder_structure {
                return Err(PropertyInlineCacheClearError::UnexpectedHolderStructure {
                    attachment_kind: request.attachment_kind,
                    holder_structure,
                });
            }
        }
    }

    match request.attachment_kind {
        PropertyInlineCacheAttachmentKind::GetByNameNegativeLookup => {
            if let Some(offset) = request.offset {
                return Err(PropertyInlineCacheClearError::UnexpectedPropertyOffset {
                    attachment_kind: request.attachment_kind,
                    offset,
                });
            }
        }
        _ => {
            let Some(offset) = request.offset else {
                return Err(PropertyInlineCacheClearError::MissingPropertyOffset {
                    attachment_kind: request.attachment_kind,
                });
            };
            if offset.0 < 0 {
                return Err(PropertyInlineCacheClearError::InvalidPropertyOffset { offset });
            }
        }
    }

    match request.attachment_kind {
        PropertyInlineCacheAttachmentKind::GetByNameOwnDataLoad
        | PropertyInlineCacheAttachmentKind::GetByNamePrototypeDataLoad
        | PropertyInlineCacheAttachmentKind::GetByNameNegativeLookup => {
            let Some(metadata) = cache.get_by_id else {
                return Err(PropertyInlineCacheClearError::MissingGetByIdMetadata {
                    slot: request.slot,
                });
            };
            let expected_mode = match request.attachment_kind {
                PropertyInlineCacheAttachmentKind::GetByNameOwnDataLoad => GetByIdMode::Default,
                PropertyInlineCacheAttachmentKind::GetByNamePrototypeDataLoad => {
                    GetByIdMode::ProtoLoad
                }
                PropertyInlineCacheAttachmentKind::GetByNameNegativeLookup => {
                    GetByIdMode::NegativeLookup
                }
                _ => unreachable!("checked get-by-id attachment kind"),
            };
            if metadata.mode != expected_mode {
                return Err(property_inline_cache_clear_metadata_mismatch(
                    request.slot,
                    PropertyInlineCacheClearMetadataMismatchField::GetByIdMode,
                ));
            }
            if metadata.structure != Some(request.base_structure) {
                return Err(property_inline_cache_clear_metadata_mismatch(
                    request.slot,
                    PropertyInlineCacheClearMetadataMismatchField::BaseStructure,
                ));
            }
            if metadata.holder_structure != request.holder_structure {
                return Err(property_inline_cache_clear_metadata_mismatch(
                    request.slot,
                    PropertyInlineCacheClearMetadataMismatchField::HolderStructure,
                ));
            }
            if metadata.cached_offset != request.offset {
                return Err(property_inline_cache_clear_metadata_mismatch(
                    request.slot,
                    PropertyInlineCacheClearMetadataMismatchField::Offset,
                ));
            }
        }
        PropertyInlineCacheAttachmentKind::PutByNameStoreReplace
        | PropertyInlineCacheAttachmentKind::PutByNameStoreTransition => {
            let Some(metadata) = cache.put_by_id else {
                return Err(PropertyInlineCacheClearError::MissingPutByIdMetadata {
                    slot: request.slot,
                });
            };
            let expected_mode = request
                .attachment_kind
                .put_by_id_mode()
                .expect("store attachment kind has a put-by-id mode");
            if metadata.mode != expected_mode {
                return Err(property_inline_cache_clear_metadata_mismatch(
                    request.slot,
                    PropertyInlineCacheClearMetadataMismatchField::PutByIdMode,
                ));
            }
            if metadata.old_structure != Some(request.base_structure) {
                return Err(property_inline_cache_clear_metadata_mismatch(
                    request.slot,
                    PropertyInlineCacheClearMetadataMismatchField::BaseStructure,
                ));
            }
            if metadata.new_structure != request.new_structure {
                return Err(property_inline_cache_clear_metadata_mismatch(
                    request.slot,
                    PropertyInlineCacheClearMetadataMismatchField::NewStructure,
                ));
            }
            if metadata.cached_offset != request.offset {
                return Err(property_inline_cache_clear_metadata_mismatch(
                    request.slot,
                    PropertyInlineCacheClearMetadataMismatchField::Offset,
                ));
            }
        }
    }

    Ok(())
}

fn validate_property_inline_cache_attached_metadata_request(
    inline_caches: &InlineCacheTable,
    request: &PropertyInlineCacheAttachedMetadataRequest,
) -> Result<(), PropertyInlineCacheAttachedMetadataError> {
    let Some(cache) = inline_caches.property_accesses.get(request.slot) else {
        return Err(PropertyInlineCacheAttachedMetadataError::InvalidSlot {
            slot: request.slot,
            len: inline_caches.property_accesses.len(),
        });
    };

    if cache.bytecode_index != request.bytecode_index {
        return Err(
            PropertyInlineCacheAttachedMetadataError::BytecodeIndexMismatch {
                slot: request.slot,
                expected: cache.bytecode_index,
                actual: request.bytecode_index,
            },
        );
    }

    let expected_property = PropertyCacheKey::Key(request.key);
    if cache.property != expected_property {
        return Err(
            PropertyInlineCacheAttachedMetadataError::PropertyKeyMismatch {
                slot: request.slot,
                expected: cache.property,
                actual: request.key,
            },
        );
    }

    let expected_access = request.attachment_kind.access_type();
    let expected_kind = request.attachment_kind.cache_kind();
    if cache.access != expected_access || cache.kind != expected_kind {
        return Err(
            PropertyInlineCacheAttachedMetadataError::AccessKindMismatch {
                slot: request.slot,
                expected_access,
                actual_access: cache.access,
                expected_kind,
                actual_kind: cache.kind,
            },
        );
    }

    let expected_authority = InlineCacheMutationAuthority::LinkedCodeBlock;
    if cache.mutation_authority != expected_authority {
        return Err(
            PropertyInlineCacheAttachedMetadataError::InvalidExistingMutationAuthority {
                slot: request.slot,
                expected: expected_authority,
                actual: cache.mutation_authority,
            },
        );
    }

    let expected_state = InlineCacheState::Monomorphic;
    if cache.state != expected_state {
        return Err(
            PropertyInlineCacheAttachedMetadataError::InvalidExistingState {
                slot: request.slot,
                expected: expected_state,
                actual: cache.state,
            },
        );
    }

    if request.dispatch == PropertyInlineCacheDispatch::Unlinked {
        return Err(
            PropertyInlineCacheAttachedMetadataError::InvalidRequestedDispatch {
                actual: request.dispatch,
            },
        );
    }
    if cache.dispatch != request.dispatch {
        return Err(
            PropertyInlineCacheAttachedMetadataError::InvalidExistingDispatch {
                slot: request.slot,
                expected: request.dispatch,
                actual: cache.dispatch,
            },
        );
    }

    if request.stub_mode != PropertyInlineCacheStubMode::MetadataOnly {
        return Err(
            PropertyInlineCacheAttachedMetadataError::UnsupportedStubMode {
                actual: request.stub_mode,
            },
        );
    }

    match request.attachment_kind {
        PropertyInlineCacheAttachmentKind::GetByNameOwnDataLoad
        | PropertyInlineCacheAttachmentKind::GetByNamePrototypeDataLoad
        | PropertyInlineCacheAttachmentKind::GetByNameNegativeLookup
        | PropertyInlineCacheAttachmentKind::PutByNameStoreReplace
            if request.new_structure.is_some() =>
        {
            return Err(
                PropertyInlineCacheAttachedMetadataError::IncompatibleNewStructure {
                    attachment_kind: request.attachment_kind,
                    new_structure: request.new_structure,
                },
            );
        }
        PropertyInlineCacheAttachmentKind::PutByNameStoreTransition
            if request.new_structure.is_none() =>
        {
            return Err(
                PropertyInlineCacheAttachedMetadataError::IncompatibleNewStructure {
                    attachment_kind: request.attachment_kind,
                    new_structure: request.new_structure,
                },
            );
        }
        _ => {}
    }

    match request.attachment_kind {
        PropertyInlineCacheAttachmentKind::GetByNamePrototypeDataLoad => {
            if request.holder_structure.is_none() {
                return Err(
                    PropertyInlineCacheAttachedMetadataError::MissingHolderStructure {
                        attachment_kind: request.attachment_kind,
                    },
                );
            }
        }
        _ => {
            if let Some(holder_structure) = request.holder_structure {
                return Err(
                    PropertyInlineCacheAttachedMetadataError::UnexpectedHolderStructure {
                        attachment_kind: request.attachment_kind,
                        holder_structure,
                    },
                );
            }
        }
    }

    match request.attachment_kind {
        PropertyInlineCacheAttachmentKind::GetByNameNegativeLookup => {
            if let Some(offset) = request.offset {
                return Err(
                    PropertyInlineCacheAttachedMetadataError::UnexpectedPropertyOffset {
                        attachment_kind: request.attachment_kind,
                        offset,
                    },
                );
            }
        }
        _ => {
            let Some(offset) = request.offset else {
                return Err(
                    PropertyInlineCacheAttachedMetadataError::MissingPropertyOffset {
                        attachment_kind: request.attachment_kind,
                    },
                );
            };
            if offset.0 < 0 {
                return Err(
                    PropertyInlineCacheAttachedMetadataError::InvalidPropertyOffset { offset },
                );
            }
        }
    }

    match request.attachment_kind {
        PropertyInlineCacheAttachmentKind::GetByNameOwnDataLoad
        | PropertyInlineCacheAttachmentKind::GetByNamePrototypeDataLoad
        | PropertyInlineCacheAttachmentKind::GetByNameNegativeLookup => {
            let Some(metadata) = cache.get_by_id else {
                return Err(
                    PropertyInlineCacheAttachedMetadataError::MissingGetByIdMetadata {
                        slot: request.slot,
                    },
                );
            };
            let expected_mode = match request.attachment_kind {
                PropertyInlineCacheAttachmentKind::GetByNameOwnDataLoad => GetByIdMode::Default,
                PropertyInlineCacheAttachmentKind::GetByNamePrototypeDataLoad => {
                    GetByIdMode::ProtoLoad
                }
                PropertyInlineCacheAttachmentKind::GetByNameNegativeLookup => {
                    GetByIdMode::NegativeLookup
                }
                _ => unreachable!("checked get-by-id attachment kind"),
            };
            if metadata.mode != expected_mode {
                return Err(property_inline_cache_attached_metadata_mismatch(
                    request.slot,
                    PropertyInlineCacheAttachedMetadataMismatchField::GetByIdMode,
                ));
            }
            if metadata.structure != Some(request.base_structure) {
                return Err(property_inline_cache_attached_metadata_mismatch(
                    request.slot,
                    PropertyInlineCacheAttachedMetadataMismatchField::BaseStructure,
                ));
            }
            if metadata.holder_structure != request.holder_structure {
                return Err(property_inline_cache_attached_metadata_mismatch(
                    request.slot,
                    PropertyInlineCacheAttachedMetadataMismatchField::HolderStructure,
                ));
            }
            if metadata.cached_offset != request.offset {
                return Err(property_inline_cache_attached_metadata_mismatch(
                    request.slot,
                    PropertyInlineCacheAttachedMetadataMismatchField::Offset,
                ));
            }
        }
        PropertyInlineCacheAttachmentKind::PutByNameStoreReplace
        | PropertyInlineCacheAttachmentKind::PutByNameStoreTransition => {
            let Some(metadata) = cache.put_by_id else {
                return Err(
                    PropertyInlineCacheAttachedMetadataError::MissingPutByIdMetadata {
                        slot: request.slot,
                    },
                );
            };
            let expected_mode = request
                .attachment_kind
                .put_by_id_mode()
                .expect("store attachment kind has a put-by-id mode");
            if metadata.mode != expected_mode {
                return Err(property_inline_cache_attached_metadata_mismatch(
                    request.slot,
                    PropertyInlineCacheAttachedMetadataMismatchField::PutByIdMode,
                ));
            }
            if metadata.old_structure != Some(request.base_structure) {
                return Err(property_inline_cache_attached_metadata_mismatch(
                    request.slot,
                    PropertyInlineCacheAttachedMetadataMismatchField::BaseStructure,
                ));
            }
            if metadata.new_structure != request.new_structure {
                return Err(property_inline_cache_attached_metadata_mismatch(
                    request.slot,
                    PropertyInlineCacheAttachedMetadataMismatchField::NewStructure,
                ));
            }
            if metadata.cached_offset != request.offset {
                return Err(property_inline_cache_attached_metadata_mismatch(
                    request.slot,
                    PropertyInlineCacheAttachedMetadataMismatchField::Offset,
                ));
            }
        }
    }

    Ok(())
}

const fn property_inline_cache_attached_metadata_mismatch(
    slot: usize,
    field: PropertyInlineCacheAttachedMetadataMismatchField,
) -> PropertyInlineCacheAttachedMetadataError {
    PropertyInlineCacheAttachedMetadataError::AttachedMetadataMismatch { slot, field }
}

const fn property_inline_cache_clear_metadata_mismatch(
    slot: usize,
    field: PropertyInlineCacheClearMetadataMismatchField,
) -> PropertyInlineCacheClearError {
    PropertyInlineCacheClearError::AttachedMetadataMismatch { slot, field }
}

fn validate_property_inline_cache_attachment_stub_mode(
    request: &PropertyInlineCacheAttachmentRequest,
) -> Result<(), PropertyInlineCacheAttachmentError> {
    match request.stub_mode {
        PropertyInlineCacheStubMode::MetadataOnly => Ok(()),
        PropertyInlineCacheStubMode::StructureStub
            if request.attachment_kind
                == PropertyInlineCacheAttachmentKind::GetByNameOwnDataLoad
                && !request.requirements.requires_barrier
                && !request.requirements.has_barrier_evidence
                && !request.requirements.requires_watchpoint
                && !request.requirements.may_call
                && !request.requirements.may_allocate =>
        {
            Ok(())
        }
        PropertyInlineCacheStubMode::StructureStub => {
            Err(PropertyInlineCacheAttachmentError::UnsupportedStubMode {
                actual: request.stub_mode,
            })
        }
    }
}

fn validate_property_inline_cache_clear_stub_mode(
    inline_caches: &InlineCacheTable,
    request: &PropertyInlineCacheClearRequest,
) -> Result<(), PropertyInlineCacheClearError> {
    match request.stub_mode {
        PropertyInlineCacheStubMode::MetadataOnly => Ok(()),
        PropertyInlineCacheStubMode::StructureStub
            if request.attachment_kind
                == PropertyInlineCacheAttachmentKind::GetByNameOwnDataLoad =>
        {
            validate_property_inline_cache_clear_structure_stub(inline_caches, request)
        }
        PropertyInlineCacheStubMode::StructureStub => {
            Err(PropertyInlineCacheClearError::UnsupportedStubMode {
                actual: request.stub_mode,
            })
        }
    }
}

fn validate_property_inline_cache_clear_structure_stub(
    inline_caches: &InlineCacheTable,
    request: &PropertyInlineCacheClearRequest,
) -> Result<(), PropertyInlineCacheClearError> {
    let Some(structure_stub_index) = request.structure_stub_index else {
        return Err(PropertyInlineCacheClearError::InvalidStructureStubIndex {
            index: None,
            len: inline_caches.structure_stubs.len(),
        });
    };
    let Some(stub) = inline_caches.structure_stubs.get(structure_stub_index) else {
        return Err(PropertyInlineCacheClearError::InvalidStructureStubIndex {
            index: Some(structure_stub_index),
            len: inline_caches.structure_stubs.len(),
        });
    };

    let mismatch = |field| PropertyInlineCacheClearError::StructureStubMetadataMismatch {
        index: structure_stub_index,
        field,
    };
    if stub.bytecode_index != request.bytecode_index {
        return Err(mismatch(StructureStubMetadataMismatchField::BytecodeIndex));
    }
    if stub.inline_cache_slot != request.slot {
        return Err(mismatch(
            StructureStubMetadataMismatchField::InlineCacheSlot,
        ));
    }
    if stub.attachment_kind != request.attachment_kind {
        return Err(mismatch(StructureStubMetadataMismatchField::AttachmentKind));
    }
    if stub.key != request.key {
        return Err(mismatch(StructureStubMetadataMismatchField::Key));
    }
    if stub.kind != StructureStubKind::GetById {
        return Err(mismatch(StructureStubMetadataMismatchField::Kind));
    }
    if stub.cache_state != InlineCacheState::Monomorphic {
        return Err(mismatch(StructureStubMetadataMismatchField::CacheState));
    }
    if stub.base_structure != request.base_structure {
        return Err(mismatch(StructureStubMetadataMismatchField::BaseStructure));
    }
    if stub.holder_structure != request.holder_structure {
        return Err(mismatch(
            StructureStubMetadataMismatchField::HolderStructure,
        ));
    }
    if stub.new_structure != request.new_structure {
        return Err(mismatch(StructureStubMetadataMismatchField::NewStructure));
    }
    if stub.offset != request.offset {
        return Err(mismatch(StructureStubMetadataMismatchField::Offset));
    }
    if stub.requirements.requires_barrier
        || stub.requirements.has_barrier_evidence
        || stub.requirements.requires_watchpoint
        || stub.requirements.may_call
        || stub.requirements.may_allocate
    {
        return Err(mismatch(StructureStubMetadataMismatchField::Requirements));
    }

    Ok(())
}

fn validate_structure_stub_access_case_link_request(
    inline_caches: &InlineCacheTable,
    request: &StructureStubAccessCaseLinkRequest,
) -> Result<(), StructureStubAccessCaseLinkError> {
    let Some(stub) = inline_caches
        .structure_stubs
        .get(request.structure_stub_index)
    else {
        return Err(
            StructureStubAccessCaseLinkError::InvalidStructureStubIndex {
                index: request.structure_stub_index,
                len: inline_caches.structure_stubs.len(),
            },
        );
    };

    let mismatch = |field| StructureStubAccessCaseLinkError::StructureStubMetadataMismatch {
        index: request.structure_stub_index,
        field,
    };
    if stub.bytecode_index != request.bytecode_index {
        return Err(mismatch(StructureStubMetadataMismatchField::BytecodeIndex));
    }
    if stub.inline_cache_slot != request.slot {
        return Err(mismatch(
            StructureStubMetadataMismatchField::InlineCacheSlot,
        ));
    }
    if request.attachment_kind != PropertyInlineCacheAttachmentKind::GetByNameOwnDataLoad
        || stub.attachment_kind != request.attachment_kind
    {
        return Err(mismatch(StructureStubMetadataMismatchField::AttachmentKind));
    }
    if stub.key != request.key {
        return Err(mismatch(StructureStubMetadataMismatchField::Key));
    }
    if stub.kind != StructureStubKind::GetById {
        return Err(mismatch(StructureStubMetadataMismatchField::Kind));
    }
    if stub.cache_state != InlineCacheState::Monomorphic {
        return Err(mismatch(StructureStubMetadataMismatchField::CacheState));
    }
    if stub.base_structure != request.base_structure {
        return Err(mismatch(StructureStubMetadataMismatchField::BaseStructure));
    }
    if stub.holder_structure != request.holder_structure {
        return Err(mismatch(
            StructureStubMetadataMismatchField::HolderStructure,
        ));
    }
    if stub.new_structure != request.new_structure {
        return Err(mismatch(StructureStubMetadataMismatchField::NewStructure));
    }
    if stub.offset != request.offset {
        return Err(mismatch(StructureStubMetadataMismatchField::Offset));
    }
    if stub.requirements.requires_barrier
        || stub.requirements.has_barrier_evidence
        || stub.requirements.requires_watchpoint
        || stub.requirements.may_call
        || stub.requirements.may_allocate
    {
        return Err(mismatch(StructureStubMetadataMismatchField::Requirements));
    }

    Ok(())
}

fn structure_stub_info_for_property_inline_cache_request(
    request: &PropertyInlineCacheAttachmentRequest,
) -> StructureStubInfo {
    StructureStubInfo {
        bytecode_index: request.bytecode_index,
        inline_cache_slot: request.slot,
        attachment_kind: request.attachment_kind,
        key: request.key,
        base_structure: request.base_structure,
        holder_structure: request.holder_structure,
        new_structure: request.new_structure,
        offset: request.offset,
        requirements: request.requirements,
        kind: StructureStubKind::GetById,
        cache_state: InlineCacheState::Monomorphic,
        code_origin: CodeOrigin::new(request.bytecode_index),
        access_cases: Vec::new(),
        reset_by_gc: false,
    }
}

/// Link the unlinked constant pool into the `CodeBlock`, mirroring
/// `CodeBlock::setConstantRegisters` (CodeBlock.cpp:1044-1101): C++ writes
/// `constants[i]` into `m_constantRegisters[i]` because the unlinked vector
/// POSITION is the constant index — `UnlinkedCodeBlockGenerator::addConstant`
/// (UnlinkedCodeBlockGenerator.h:135-152) appends and hands out
/// `FirstConstantRegisterIndex + position` as the register. Readers then index
/// by `VirtualRegister::toConstantIndex()` (CodeBlock.h:203-206
/// `constantRegister`/`getConstant`). Rust's `UnlinkedConstant` carries its
/// `register` explicitly, so linking places each value at ITS constant index,
/// never at its incidental vector position.
fn linked_constants_from_unlinked(unlinked: &UnlinkedCodeBlock) -> LinkedConstantPool {
    let pool = unlinked.constants();
    let len = pool
        .constants
        .iter()
        .filter_map(|constant| constant.register.to_constant_index())
        .map(|index| index as usize + 1)
        .max()
        .unwrap_or(0);
    // Slots not claimed by any unlinked constant hold the EMPTY JSValue,
    // exactly what an unwritten `WriteBarrier<Unknown>` slot in
    // `m_constantRegisters` holds in C++ (a default WriteBarrier is JSValue(),
    // the empty encoding 0). JSC's dense generator layout never produces such
    // holes; they can only appear if a Rust unlinked pool is sparse.
    let mut constants: Vec<LinkedConstant> = (0..len)
        .map(|index| LinkedConstant {
            register: VirtualRegister::constant(index as u32),
            value: ConstantValue::Encoded(JsValue::default()),
            owner: ConstantOwner::SharedUnlinked,
        })
        .collect();
    for constant in &pool.constants {
        let Some(index) = constant.register.to_constant_index() else {
            // No C++ counterpart: an unlinked constant's register is always in
            // the constant namespace (VirtualRegister::toConstantIndex ASSERTs
            // isConstant, VirtualRegister.h). Mirror the ASSERT.
            debug_assert!(
                false,
                "unlinked constant register outside the constant namespace"
            );
            continue;
        };
        constants[index as usize] = LinkedConstant {
            register: constant.register,
            value: constant.value,
            owner: ConstantOwner::SharedUnlinked,
        };
    }
    LinkedConstantPool {
        constants,
        // C++ links function decls/exprs POSITIONALLY at CodeBlock creation:
        // `m_functionDecls[i]` links `unlinkedCodeBlock->functionDecl(i)` and
        // `m_functionExprs[i]` links `functionExpr(i)` (CodeBlock.cpp:425-440).
        // The Rust executable-link step is the index-preserving handle map.
        function_declarations: pool
            .function_declarations
            .iter()
            .map(|unlinked_ref| LinkedFunctionRef(unlinked_ref.0))
            .collect(),
        function_expressions: pool
            .function_expressions
            .iter()
            .map(|unlinked_ref| LinkedFunctionRef(unlinked_ref.0))
            .collect(),
    }
}

fn linked_side_tables_from_unlinked(unlinked: &UnlinkedCodeBlock) -> LinkedSideTables {
    let unlinked_side_tables = unlinked.side_tables();
    LinkedSideTables {
        handlers: unlinked_side_tables
            .handlers
            .iter()
            .map(|handler| HandlerInfo {
                range: HandlerRange::Bytecode(handler.range),
                target: HandlerTarget::Bytecode(handler.target),
                kind: handler.kind,
            })
            .collect(),
        code_origins: unlinked_side_tables.code_origins.clone(),
        exception_handlers: unlinked_side_tables.exception_handlers.clone(),
        root_maps: unlinked_side_tables.root_maps.clone(),
        inline_caches: RefCell::new(InlineCacheTable {
            property_accesses: derive_property_inline_caches(unlinked),
            calls: derive_call_link_inline_caches(unlinked),
            ..InlineCacheTable::default()
        }),
        array_profiles: RefCell::new(derive_array_profiles(unlinked)),
        value_profiles: RefCell::new(derive_value_profiles(
            unlinked,
            ValueProfileEmissionPolicy::default(),
        )),
        binary_arith_profiles: RefCell::new(derive_binary_arith_profiles(unlinked)),
        unary_arith_profiles: RefCell::new(derive_unary_arith_profiles(unlinked)),
        ..LinkedSideTables::default()
    }
}

fn derive_property_inline_caches(unlinked: &UnlinkedCodeBlock) -> Vec<PropertyInlineCache> {
    let mut caches = Vec::new();
    for decoded in unlinked.instructions().decoded_instructions().flatten() {
        match crate::bytecode::opcode::CoreOpcode::from_opcode(decoded.opcode) {
            Some(crate::bytecode::opcode::CoreOpcode::GetByName) => {
                let Ok(base) = decoded.register_operand(1) else {
                    continue;
                };
                let Ok(Operand::IdentifierIndex(identifier_index)) = decoded.operand(2) else {
                    continue;
                };
                let property = PropertyKey::from_identifier(Identifier::from_atom(
                    AtomId::from_table_slot(identifier_index),
                ));
                caches.push(PropertyInlineCache::get_by_name_load(
                    decoded.bytecode_index,
                    base,
                    property,
                ));
            }
            Some(crate::bytecode::opcode::CoreOpcode::GetLength) => {
                let Ok(base) = decoded.register_operand(1) else {
                    continue;
                };
                let Ok(Operand::IdentifierIndex(identifier_index)) = decoded.operand(2) else {
                    continue;
                };
                let property = PropertyKey::from_identifier(Identifier::from_atom(
                    AtomId::from_table_slot(identifier_index),
                ));
                caches.push(PropertyInlineCache::get_by_name_load(
                    decoded.bytecode_index,
                    base,
                    property,
                ));
            }
            Some(crate::bytecode::opcode::CoreOpcode::PutByName) => {
                let Ok(base) = decoded.register_operand(0) else {
                    continue;
                };
                let Ok(Operand::IdentifierIndex(identifier_index)) = decoded.operand(1) else {
                    continue;
                };
                let property = PropertyKey::from_identifier(Identifier::from_atom(
                    AtomId::from_table_slot(identifier_index),
                ));
                caches.push(PropertyInlineCache::put_by_name_store(
                    decoded.bytecode_index,
                    base,
                    property,
                ));
            }
            Some(crate::bytecode::opcode::CoreOpcode::GetGlobalObjectProperty) => {
                let Ok(Operand::IdentifierIndex(identifier_index)) = decoded.operand(1) else {
                    continue;
                };
                let property = PropertyKey::from_identifier(Identifier::from_atom(
                    AtomId::from_table_slot(identifier_index),
                ));
                caches.push(PropertyInlineCache::get_global_object_property_load(
                    decoded.bytecode_index,
                    property,
                ));
            }
            Some(crate::bytecode::opcode::CoreOpcode::PutGlobalObjectProperty) => {
                let Ok(Operand::IdentifierIndex(identifier_index)) = decoded.operand(0) else {
                    continue;
                };
                let property = PropertyKey::from_identifier(Identifier::from_atom(
                    AtomId::from_table_slot(identifier_index),
                ));
                caches.push(PropertyInlineCache::put_global_object_property_store(
                    decoded.bytecode_index,
                    property,
                ));
            }
            Some(crate::bytecode::opcode::CoreOpcode::GetByValue) => {
                let Ok(base) = decoded.register_operand(1) else {
                    continue;
                };
                let Ok(property) = decoded.register_operand(2) else {
                    continue;
                };
                caches.push(PropertyInlineCache::get_by_value_load(
                    decoded.bytecode_index,
                    base,
                    property,
                ));
            }
            Some(crate::bytecode::opcode::CoreOpcode::PutByValue) => {
                let Ok(base) = decoded.register_operand(0) else {
                    continue;
                };
                let Ok(property) = decoded.register_operand(1) else {
                    continue;
                };
                caches.push(PropertyInlineCache::put_by_value_store(
                    decoded.bytecode_index,
                    base,
                    property,
                ));
            }
            Some(crate::bytecode::opcode::CoreOpcode::InById) => {
                let Ok(base) = decoded.register_operand(1) else {
                    continue;
                };
                let Ok(Operand::IdentifierIndex(identifier_index)) = decoded.operand(2) else {
                    continue;
                };
                let property = PropertyKey::from_identifier(Identifier::from_atom(
                    AtomId::from_table_slot(identifier_index),
                ));
                caches.push(PropertyInlineCache::in_by_id_has(
                    decoded.bytecode_index,
                    base,
                    property,
                ));
            }
            Some(crate::bytecode::opcode::CoreOpcode::InByVal) => {
                let Ok(base) = decoded.register_operand(1) else {
                    continue;
                };
                let Ok(property) = decoded.register_operand(2) else {
                    continue;
                };
                caches.push(PropertyInlineCache::in_by_value_has(
                    decoded.bytecode_index,
                    base,
                    property,
                ));
            }
            _ => {}
        }
    }
    caches
}

/// One derived `ArrayProfile` per array-profiled bytecode site, in program
/// order, mirroring C++ JSC's per-opcode `ArrayProfile` metadata slots
/// (bytecode/BytecodeList.rb: `get_by_val`:617, `get_length`:406,
/// `put_by_val`:628 / `put_by_val_direct`:639, `in_by_val`:649, `call`:452 /
/// `call_ignore_result`:463). `GetByIndex`/`PutByIndex` are the Rust
/// constant-index lowerings of `op_get_by_val`/`op_put_by_val` and inherit
/// those slots. JSC's remaining array-profiled opcodes (`iterator_open`:221,
/// `tail_call`:319, `new_array_with_species`:438, the `enumerator_*`
/// family:664-733) have no Rust opcode yet and derive nothing.
fn derive_array_profiles(unlinked: &UnlinkedCodeBlock) -> Vec<ArrayProfile> {
    let mut profiles = Vec::new();
    for decoded in unlinked.instructions().decoded_instructions().flatten() {
        let Some(opcode) = CoreOpcode::from_opcode(decoded.opcode) else {
            continue;
        };
        if matches!(
            opcode,
            CoreOpcode::GetByValue
                | CoreOpcode::GetByIndex
                | CoreOpcode::GetLength
                | CoreOpcode::PutByValue
                | CoreOpcode::PutByIndex
                | CoreOpcode::InByVal
                | CoreOpcode::Call
                | CoreOpcode::CallWithThis
        ) {
            profiles.push(ArrayProfile::for_bytecode_index(decoded.bytecode_index));
        }
    }
    profiles
}

fn derive_call_link_inline_caches(unlinked: &UnlinkedCodeBlock) -> Vec<CallLinkInfo> {
    let mut calls = Vec::new();
    for decoded in unlinked.instructions().decoded_instructions().flatten() {
        let Some(opcode) = CoreOpcode::from_opcode(decoded.opcode) else {
            continue;
        };
        if call_link_descriptor_shape_for_opcode(opcode).is_none() {
            continue;
        }
        let call_site = call_site_for_decoded_call(&decoded);
        let call = match opcode {
            CoreOpcode::Call | CoreOpcode::CallWithThis => {
                CallLinkInfo::metadata_only_unlinked_call(call_site, decoded.bytecode_index, opcode)
            }
            CoreOpcode::Construct => CallLinkInfo::metadata_only_unlinked_construct(
                call_site,
                decoded.bytecode_index,
                opcode,
            ),
            _ => unreachable!(),
        };
        calls.push(call);
    }
    calls
}

// C++ JSC `op_instanceof` checkpoint ordinals (bytecode/BytecodeList.rb:230-249
// declares `checkpoints: getHasInstance, getPrototype, instanceof`; the
// generated `OpInstanceof::Checkpoints` enum numbers them 0, 1, 2). The two
// profiled intermediate loads key the derived slots below.
const INSTANCEOF_CHECKPOINT_GET_HAS_INSTANCE: Checkpoint = Checkpoint(0);
const INSTANCEOF_CHECKPOINT_GET_PROTOTYPE: Checkpoint = Checkpoint(1);

/// One derived `ValueProfile` slot per value-profiled bytecode site, in
/// program order.
///
/// C++ JSC hands each value-profiled op the next profile index at emission
/// time (the `valueProfile: unsigned` argument in bytecode/BytecodeList.rb)
/// and stores the profiles in a single program-order vector
/// (`UnlinkedCodeBlock::m_valueProfiles`, UnlinkedCodeBlock.h:380-382). This
/// walk mirrors that numbering, so `value_profile_offset` and the
/// metadata-table displacement math match the C++ layout.
///
/// Rust opcode coverage of JSC's value-profiled set:
/// - `GetByName` / `GetSuperByName` <-> `op_get_by_id`:387 /
///   `op_get_by_id_with_this`:803 (super property loads).
/// - `GetLength` <-> `op_get_length`:398.
/// - `GetByValue` / `GetByIndex` <-> `op_get_by_val`:609 (GetByIndex is the
///   Rust constant-index lowering).
/// - `GetClosureCell` / `GetGlobalLexical` / `GetGlobalObjectProperty` <->
///   the three Rust lowerings of `op_get_from_scope`:494.
/// - `EnsureThis` <-> `op_to_this`:711.
/// - `InstanceOf` <-> `op_instanceof`:230 (two slots, see below).
/// - `Call` / `CallWithThis` <-> `op_call`:442.
///
/// `Construct`/`ConstructSuper` intentionally stay unprofiled although
/// `op_construct`:285/`op_super_construct`:297 carry a valueProfile in JSC:
/// deriving a slot before `normalize_constructor_return` feeds a profiling
/// hook would record UN-normalized construct results (the consumer-side
/// Call|CallWithThis filter in `baseline_generated_owner_call_result_profile_
/// site` and the `construct.result_profile == None` assertion in jit/plan.rs
/// pin the exclusion). `CallDirect` (Rust-only call-by-constant-index
/// lowering, not `op_call_direct_eval`) is likewise excluded until its
/// profiling story settles; the bytecompiler does not emit it today. JSC's
/// remaining value-profiled opcodes (`try_get_by_id`:749, `get_by_id_direct`:737,
/// `get_private_name`:583, `get_argument`:773, `get_from_arguments`:780,
/// `get_prototype_of`:788, `get_internal_field`:795, `to_object`:812,
/// `new_array_with_species`:429, the varargs call family:110-205,
/// `call_direct_eval`:322, `iterator_open`:207 / `iterator_next`:132,
/// `enumerator_get_by_val`:722) have no Rust opcode yet and derive nothing.
fn derive_value_profiles(
    unlinked: &UnlinkedCodeBlock,
    emission_policy: ValueProfileEmissionPolicy,
) -> ValueProfileTable {
    let mut table = ValueProfileTable::default();
    table.emission_policy = emission_policy;
    fn push_profile(
        table: &mut ValueProfileTable,
        bytecode_index: BytecodeIndex,
        checkpoint: Checkpoint,
        operand: Option<VirtualRegister>,
    ) {
        let slot = RuntimeSlot(table.profiles.len().saturating_add(1) as u32);
        table.profiles.push(ValueProfile {
            bytecode_index,
            checkpoint,
            operand,
            buckets: vec![ValueProfileBucket {
                slot,
                kind: ValueProfileBucketKind::Sample,
            }],
            prediction: SPEC_NONE,
            update_policy: ProfileUpdatePolicy::ConcurrentBuckets,
        });
        table
            .unlinked_predictions
            .push(UnlinkedValueProfile::default());
    }
    for decoded in unlinked.instructions().decoded_instructions().flatten() {
        let Some(opcode) = CoreOpcode::from_opcode(decoded.opcode) else {
            continue;
        };
        match opcode {
            // Destination-profiled sites: the profile samples the value written
            // to the instruction's dst register (operand 0 in every Rust
            // lowering, matching JSC's profiled `dst`/`srcDst`).
            CoreOpcode::GetByName
            | CoreOpcode::GetSuperByName
            | CoreOpcode::GetLength
            | CoreOpcode::GetByValue
            | CoreOpcode::GetByIndex
            | CoreOpcode::GetClosureCell
            | CoreOpcode::GetGlobalLexical
            | CoreOpcode::GetGlobalObjectProperty
            | CoreOpcode::EnsureThis
            | CoreOpcode::Call
            | CoreOpcode::CallWithThis => {
                let Ok(destination) = decoded.register_operand(0) else {
                    continue;
                };
                push_profile(
                    &mut table,
                    decoded.bytecode_index,
                    Checkpoint::NONE,
                    Some(destination),
                );
            }
            // C++ JSC `op_instanceof` profiles its two intermediate property
            // loads, not the boolean result: `hasInstanceValueProfile` then
            // `prototypeValueProfile` (BytecodeList.rb:230-249), written at the
            // `getHasInstance`/`getPrototype` checkpoints into the shared
            // `m_hasInstanceOrPrototype` register. The Rust `InstanceOf`
            // lowering is fused (dst, value, constructor) and materializes no
            // such register, so the slots carry `operand: None`; they keep
            // JSC's checkpoint keying and per-site slot count.
            CoreOpcode::InstanceOf => {
                push_profile(
                    &mut table,
                    decoded.bytecode_index,
                    INSTANCEOF_CHECKPOINT_GET_HAS_INSTANCE,
                    None,
                );
                push_profile(
                    &mut table,
                    decoded.bytecode_index,
                    INSTANCEOF_CHECKPOINT_GET_PROTOTYPE,
                    None,
                );
            }
            _ => {}
        }
    }
    table.materialize_jit_storage_from_profiles();
    table
}

/// One derived `BinaryArithProfile` per binary-arith-profiled bytecode site,
/// in program order.
///
/// C++ JSC's profiled binary arith set is FOR_EACH_OPCODE_WITH_BINARY_ARITH_PROFILE
/// (bytecode/Opcode.h:158-167): `op_add`, `op_mul`, `op_div`, `op_sub`,
/// `op_bitand`, `op_bitor`, `op_bitxor` (BytecodeList.rb:1276-1292) plus
/// `op_lshift`, `op_rshift` (BytecodeList.rb:1294-1304). `op_mod`, `op_pow`,
/// and `op_urshift` sit in the unprofiled `BinaryOp` group
/// (BytecodeList.rb:1254-1274), so `ModNumber`/`PowNumber`/
/// `UnsignedRightShiftInt32` derive nothing.
fn derive_binary_arith_profiles(unlinked: &UnlinkedCodeBlock) -> Vec<BinaryArithProfileSlot> {
    let mut slots = Vec::new();
    for decoded in unlinked.instructions().decoded_instructions().flatten() {
        let Some(opcode) = CoreOpcode::from_opcode(decoded.opcode) else {
            continue;
        };
        if matches!(
            opcode,
            CoreOpcode::AddInt32
                | CoreOpcode::SubInt32
                | CoreOpcode::MulInt32
                | CoreOpcode::DivNumber
                | CoreOpcode::BitAndInt32
                | CoreOpcode::BitOrInt32
                | CoreOpcode::BitXorInt32
                | CoreOpcode::LeftShiftInt32
                | CoreOpcode::RightShiftInt32
        ) {
            slots.push(BinaryArithProfileSlot {
                bytecode_index: decoded.bytecode_index,
                profile: BinaryArithProfile::default(),
            });
        }
    }
    slots
}

/// One derived `UnaryArithProfile` per unary-arith-profiled bytecode site, in
/// program order.
///
/// C++ JSC's profiled unary arith set is FOR_EACH_OPCODE_WITH_UNARY_ARITH_PROFILE
/// (bytecode/Opcode.h:169-175): `op_bitnot`, `op_inc`, `op_dec`, `op_negate`,
/// `op_to_number`, `op_to_numeric` (BytecodeList.rb:1329-1345, 1381-1391).
/// Rust divergence: the bytecompiler pre-lowers `++`/`--` (JSC
/// `op_inc`/`op_dec`) into `ToNumber` + `AddInt32`/`SubInt32` with a constant
/// 1 (`emit_update`, bytecompiler/mod.rs), so an inc/dec site surfaces here as
/// a `ToNumber` unary profile plus an Add/Sub binary profile instead of JSC's
/// single `UnaryArithProfile`. `op_to_numeric` has no Rust opcode yet.
fn derive_unary_arith_profiles(unlinked: &UnlinkedCodeBlock) -> Vec<UnaryArithProfileSlot> {
    let mut slots = Vec::new();
    for decoded in unlinked.instructions().decoded_instructions().flatten() {
        let Some(opcode) = CoreOpcode::from_opcode(decoded.opcode) else {
            continue;
        };
        if matches!(
            opcode,
            CoreOpcode::ToNumber | CoreOpcode::NegateNumber | CoreOpcode::BitNotInt32
        ) {
            slots.push(UnaryArithProfileSlot {
                bytecode_index: decoded.bytecode_index,
                profile: UnaryArithProfile::default(),
            });
        }
    }
    slots
}

const fn call_link_descriptor_shape_for_opcode(
    opcode: CoreOpcode,
) -> Option<(CallType, CodeSpecialization)> {
    match opcode {
        CoreOpcode::Call | CoreOpcode::CallWithThis => {
            Some((CallType::Call, CodeSpecialization::Call))
        }
        CoreOpcode::Construct => Some((CallType::Construct, CodeSpecialization::Construct)),
        _ => None,
    }
}

#[derive(Clone, Copy, Debug)]
pub struct CodeBlockExecutionSurface<'a> {
    code_block: &'a CodeBlock,
}

impl<'a> CodeBlockExecutionSurface<'a> {
    pub fn code_block(&self) -> &'a CodeBlock {
        self.code_block
    }

    pub fn instruction_at(
        self,
        bytecode_index: BytecodeIndex,
    ) -> Result<DecodedInstruction<'a>, InstructionDecodeError> {
        self.code_block.decoded_instruction_at(bytecode_index)
    }

    pub fn metadata_entry(self, bytecode_index: BytecodeIndex) -> Option<&'a MetadataEntry> {
        self.code_block
            .metadata_entry_for_bytecode_index(bytecode_index)
    }

    pub fn source_note(self, bytecode_index: BytecodeIndex) -> Option<SourceNoteLookup> {
        self.code_block
            .unlinked()
            .side_tables()
            .source_notes
            .lookup(bytecode_index)
    }

    pub fn link_record(self) -> UnlinkedToLinkedBytecodeRecord {
        self.code_block.link_record()
    }
}

#[derive(Clone, Debug, Eq, PartialEq)]
pub struct UnlinkedToLinkedBytecodeRecord {
    pub context: LinkContext,
    pub unlinked_phase: UnlinkedCodeBlockPhase,
    pub linked_lifecycle: CodeBlockLifecycleState,
    pub instruction_count: u32,
    pub unlinked_metadata_entries: u32,
    pub linked_metadata_entries: u32,
    pub root_map_count: u32,
    pub value_profile_root_count: u32,
    pub has_interpreter_entry: bool,
}

#[derive(Clone, Debug, Default)]
pub struct UnlinkedMetadataTable {
    /// Generated per-opcode metadata layout. The table is frozen with the
    /// unlinked block and later materialized as mutable runtime feedback.
    entries: Vec<UnlinkedMetadataEntry>,
    pub layout: MetadataLayout,
    pub instruction_plans: Vec<InstructionMetadataPlan>,
    pub schema_version: OpcodeSchemaVersion,
    pub did_optimize_hint: TierHint,
}

impl UnlinkedMetadataTable {
    pub fn from_entries(
        entries: Vec<UnlinkedMetadataEntry>,
        layout: MetadataLayout,
        instruction_plans: Vec<InstructionMetadataPlan>,
        schema_version: OpcodeSchemaVersion,
    ) -> Self {
        Self {
            entries,
            layout,
            instruction_plans,
            schema_version,
            did_optimize_hint: TierHint::Unknown,
        }
    }

    pub fn entries(&self) -> &[UnlinkedMetadataEntry] {
        &self.entries
    }

    pub fn entry_for_bytecode_index(
        &self,
        bytecode_index: BytecodeIndex,
    ) -> Option<&UnlinkedMetadataEntry> {
        self.entries
            .iter()
            .find(|entry| entry.bytecode_index == bytecode_index)
    }

    pub fn plan_for_bytecode_index(
        &self,
        bytecode_index: BytecodeIndex,
    ) -> Option<&InstructionMetadataPlan> {
        self.instruction_plans
            .iter()
            .find(|plan| plan.bytecode_index == bytecode_index)
    }
}

#[derive(Clone, Debug, Default)]
pub struct MetadataTable {
    /// Runtime feedback, caches, profiles, and patchable data are linked-code
    /// state and must mutate through owner-aware VM/GC APIs.
    entries: Vec<MetadataEntry>,
    pub layout: MetadataLayout,
    pub linking_data: MetadataLinkingData,
    pub schema_version: OpcodeSchemaVersion,
}

impl MetadataTable {
    pub fn from_entries(
        entries: Vec<MetadataEntry>,
        layout: MetadataLayout,
        linking_data: MetadataLinkingData,
        schema_version: OpcodeSchemaVersion,
    ) -> Self {
        Self {
            entries,
            layout,
            linking_data,
            schema_version,
        }
    }

    pub fn entries(&self) -> &[MetadataEntry] {
        &self.entries
    }

    pub fn entry_for_bytecode_index(
        &self,
        bytecode_index: BytecodeIndex,
    ) -> Option<&MetadataEntry> {
        self.entries
            .iter()
            .find(|entry| entry.bytecode_index == bytecode_index)
    }
}

#[derive(Clone, Debug, Eq, PartialEq)]
pub struct UnlinkedMetadataEntry {
    pub bytecode_index: BytecodeIndex,
    pub opcode: Opcode,
    pub fields: Vec<MetadataFieldSpec>,
}

#[derive(Clone, Debug, Eq, PartialEq)]
pub struct MetadataEntry {
    pub bytecode_index: BytecodeIndex,
    pub opcode: Opcode,
    pub fields: Vec<LinkedMetadataField>,
}

#[derive(Clone, Debug, Eq, PartialEq)]
pub struct LinkedMetadataField {
    pub spec: MetadataFieldSpec,
    pub storage: LinkedMetadataStorage,
}

#[derive(Clone, Debug, Eq, PartialEq)]
pub enum LinkedMetadataStorage {
    Unallocated,
    InlineCacheSlot(RuntimeSlot),
    ProfileSlot(RuntimeSlot),
    BarrieredCell(RuntimeSlot),
    OpaqueGenerated(RuntimeSlot),
}

/// Slot in linked runtime metadata storage.
///
/// The owning `CodeBlock` controls allocation and mutation of these slots.
/// `RuntimeSlot` is not a `CellId`, `StructureId`, or `JsValue`; fields that
/// need heap identity must import the canonical runtime or GC IDs directly.
#[derive(Clone, Copy, Debug, Default, Eq, PartialEq, Hash)]
#[repr(transparent)]
pub struct RuntimeSlot(pub u32);

#[derive(Clone, Debug, Default, Eq, PartialEq)]
pub struct UnlinkedConstantPool {
    pub identifiers: Vec<Identifier>,
    pub constants: Vec<UnlinkedConstant>,
    pub function_declarations: Vec<UnlinkedFunctionRef>,
    pub function_expressions: Vec<UnlinkedFunctionRef>,
    pub identifier_sets: Vec<IdentifierSetIndex>,
    string_literals: StringLiteralTable,
}

impl UnlinkedConstantPool {
    pub fn with_string_literals(
        mut self,
        entries: impl IntoIterator<Item = (u32, String)>,
    ) -> Self {
        self.install_string_literals(entries);
        self
    }

    pub fn install_string_literals(&mut self, entries: impl IntoIterator<Item = (u32, String)>) {
        self.string_literals = StringLiteralTable::from_entries(entries);
    }

    pub fn string_literals(&self) -> &StringLiteralTable {
        &self.string_literals
    }

    pub fn string_literal(&self, identifier_index: u32) -> Option<&str> {
        self.string_literals.literal(identifier_index)
    }
}

#[derive(Clone, Debug, Default, Eq, PartialEq)]
pub struct StringLiteralTable {
    entries: Vec<StringLiteralEntry>,
}

impl StringLiteralTable {
    pub fn from_entries(entries: impl IntoIterator<Item = (u32, String)>) -> Self {
        let mut sorted = BTreeMap::new();
        for (identifier_index, text) in entries {
            sorted.entry(identifier_index).or_insert(text);
        }
        Self {
            entries: sorted
                .into_iter()
                .map(|(identifier_index, text)| StringLiteralEntry {
                    identifier_index,
                    text,
                })
                .collect(),
        }
    }

    pub fn entries(&self) -> &[StringLiteralEntry] {
        &self.entries
    }

    pub fn literal(&self, identifier_index: u32) -> Option<&str> {
        self.entries
            .binary_search_by_key(&identifier_index, |entry| entry.identifier_index)
            .ok()
            .map(|index| self.entries[index].text.as_str())
    }
}

#[derive(Clone, Debug, Eq, PartialEq)]
pub struct StringLiteralEntry {
    pub identifier_index: u32,
    pub text: String,
}

#[derive(Clone, Debug, Eq, PartialEq)]
pub struct UnlinkedConstant {
    pub register: VirtualRegister,
    pub value: ConstantValue,
    pub source_representation: SourceCodeRepresentation,
}

#[derive(Clone, Copy, Debug, Eq, PartialEq)]
pub enum ConstantValue {
    Encoded(JsValue),
    DeferredCell(ConstantCellIndex),
    LinkTimeConstant(LinkTimeConstant),
}

#[derive(Clone, Copy, Debug, Eq, PartialEq, Hash)]
#[repr(transparent)]
pub struct ConstantCellIndex(pub u32);

#[derive(Clone, Copy, Debug, Eq, PartialEq, Hash)]
#[repr(transparent)]
pub struct IdentifierSetIndex(pub u32);

#[derive(Clone, Copy, Debug, Eq, PartialEq, Hash)]
#[repr(transparent)]
pub struct UnlinkedFunctionRef(pub u32);

#[derive(Clone, Copy, Debug, Eq, PartialEq, Hash)]
#[non_exhaustive]
pub enum LinkTimeConstant {
    CopyDataProperties,
    IteratorSymbol,
    AsyncIteratorSymbol,
    PromiseConstructor,
    OpaqueGenerated(u32),
}

#[derive(Clone, Copy, Debug, Eq, PartialEq, Hash)]
pub enum SourceCodeRepresentation {
    Other,
    IntegerLiteral,
    DoubleLiteral,
    StringLiteral,
    BigIntLiteral,
    RegExpLiteral,
    TemplateObject,
    LinkTimeConstant,
}

#[derive(Clone, Debug, Default, Eq, PartialEq)]
pub struct LinkedConstantPool {
    pub constants: Vec<LinkedConstant>,
    pub function_declarations: Vec<LinkedFunctionRef>,
    pub function_expressions: Vec<LinkedFunctionRef>,
}

#[derive(Clone, Copy, Debug, Eq, PartialEq)]
pub struct LinkedConstant {
    pub register: VirtualRegister,
    pub value: ConstantValue,
    pub owner: ConstantOwner,
}

#[derive(Clone, Copy, Debug, Eq, PartialEq, Hash)]
pub enum ConstantOwner {
    SharedUnlinked,
    LinkedCodeBlock,
    OptimizingTier,
}

#[derive(Clone, Copy, Debug, Eq, PartialEq, Hash)]
#[repr(transparent)]
pub struct LinkedFunctionRef(pub u32);

#[derive(Clone, Debug, Default, Eq, PartialEq)]
pub struct UnlinkedSideTables {
    pub handlers: Vec<UnlinkedHandlerInfo>,
    pub source_notes: SourceNoteTable,
    pub code_origins: CodeOriginTable,
    pub exception_handlers: ExceptionHandlerTable,
    pub bytecode_hooks: BytecodeHookTable,
    pub debug_hooks: Vec<DebugHookRecord>,
    pub type_profiler_ranges: Vec<TypeProfilerRange>,
    pub control_flow_profile_offsets: Vec<BytecodeIndex>,
    pub jump_tables: JumpTableSet,
    pub bit_vectors: Vec<BitVectorRef>,
    pub root_maps: Vec<BytecodeRootMap>,
}

/// Per-bytecode-site monomorphic LLInt inline-cache mode, mirroring C++ JSC
/// `GetByIdMode` (bytecode/GetByIdMetadata.h:34).
///
/// C++ has four modes (ProtoLoad/Default/Unset/ArrayLength). This first
/// interpreter cut implements only `Default` (the structureID+cachedOffset
/// own-data hit that `performGetByIDHelper`'s `.opGetByIdDefault` arm serves,
/// LowLevelInterpreter64.asm:1639) plus a `Megamorphic` give-up state with no
/// C++ enum counterpart: C++ instead falls back to the inline-cache repatch /
/// megamorphic IC machinery, which the deferred JIT path owns. The
/// prototype-load (ProtoLoad), unset (Unset), and array-length (ArrayLength)
/// modes are deliberately deferred — they need the prototype-chain watchpoint
/// and indexing-header machinery — so an inherited/unset/length site simply
/// stays `Uninitialized`/`Megamorphic` and always takes the slow path.
#[derive(Clone, Copy, Debug, Default, Eq, PartialEq)]
pub enum GetByIdCacheMode {
    /// No cache filled yet; the next slow path may fill it. Mirrors a freshly
    /// constructed `LLIntGetByIdCache` (mode == Default, structureID == 0).
    #[default]
    Uninitialized,
    /// C++ `GetByIdMode::Default`: a monomorphic own-data hit. The stored
    /// structure id + offset are valid for the fast path.
    Monomorphic,
    /// Site gave up after exceeding the miss budget (saw a second structure on
    /// an already-monomorphic site). No C++ enum value; mirrors C++ abandoning
    /// LLInt caching for a polymorphic/megamorphic site. The fast path is never
    /// attempted again for this site.
    Megamorphic,
}

/// Per-bytecode-site monomorphic GET inline cache, the Rust mirror of C++ JSC
/// `LLIntGetByIdCache` in its `Default` mode (bytecode/GetByIdMetadata.h:41,
/// the `defaultMode.structureID`/`defaultMode.cachedOffset` pair read by
/// `performGetByIDHelper`, LowLevelInterpreter64.asm:1639). One record per
/// `op_get_by_id`-equivalent (GetByName) site, keyed by `BytecodeIndex` in the
/// `LinkedSideTables::llint_get_by_id_caches` map.
///
/// `cached_offset` is C++ `PropertyOffset` (an `int`); the interpreter resolves
/// it through `offset_storage_index`.
///
/// `warmup` mirrors C++ `hitCountForLLIntCaching` (GetByIdMetadata.h:71): the
/// number of slow-path passes still required before the cache arms. In C++ this
/// gates the PROTOTYPE cache (default 2); here it serves a Rust-specific
/// coordination DIVERGENCE: the interpreter slow path also feeds the Rust
/// access-case-plan observation pipeline (a baseline-JIT-tiering input C++ does
/// NOT have in the LLInt), which needs a couple of slow-path passes to form its
/// plan. Arming the own-data cache on the very first pass (as C++ Default mode
/// does) would starve that pipeline, so the own-data cache waits `warmup` slow
/// passes too. Once armed and then missed on a second structure, the site goes
/// `Megamorphic` and stops fast-pathing (the polymorphic give-up).
#[derive(Clone, Copy, Debug, Default, Eq, PartialEq)]
pub struct LLIntGetByIdCache {
    pub mode: GetByIdCacheMode,
    pub cached_structure_id: StructureId,
    pub cached_offset: i32,
    pub warmup: u8,
}

/// Per-bytecode-site monomorphic PUT (replace-existing) inline cache, the Rust
/// mirror of the C++ JSC `OpPutById::Metadata` LLInt cache in its
/// replace-existing shape (`m_oldStructureID`/`m_offset` with
/// `m_newStructureID == m_oldStructureID`, filled by `slow_path_put_by_id`,
/// LLIntSlowPaths.cpp:1436). C++ also caches the property-add TRANSITION case
/// (`m_newStructureID != m_oldStructureID` + `m_structureChain`); this cut
/// caches ONLY replace-existing and leaves adds uncached (see the dispatch-site
/// comment) because the add case touches the shared structure-transition graph.
///
/// DIVERGENCE: this carries the resolved property KEY (`cached_key`), which the
/// C++ metadata does not (C++'s store target is the Butterfly slot, the sole
/// source of truth). The Rust interpreter keeps a `properties` HashMap as the
/// value-authoritative store with `out_of_line_storage` as the lockstep mirror
/// (see CoreObjectCell), so the PUT fast path must update BOTH; the cached key
/// lets it do the `properties.get_mut` with NO per-iteration key rebuild/String
/// allocation (the key is built once at fill). This is exactly the identifier
/// the bytecode operand binds the site to, so caching it is faithful. The key
/// makes the record non-`Copy`; callers clone it (O(1) for the common
/// `Identifier` variant).
#[derive(Clone, Debug, Default, Eq, PartialEq)]
pub struct LLIntPutByIdCache {
    pub mode: GetByIdCacheMode,
    pub cached_structure_id: StructureId,
    pub cached_offset: i32,
    // Slow-path warmup gate; see LLIntGetByIdCache::warmup.
    pub warmup: u8,
    // `CorePropertyKey` is a `pub(crate)` interpreter type; this field matches its
    // visibility (the cache is a crate-internal interpreter concept, never public
    // API).
    pub(crate) cached_key: Option<crate::interpreter::CorePropertyKey>,
}

/// Per-bytecode-site `BinaryArithProfile` storage slot.
///
/// C++ JSC stores one `BinaryArithProfile` per profiled arith instruction in
/// `UnlinkedCodeBlock::m_binaryArithProfiles` (UnlinkedCodeBlock.h:514,528),
/// keyed by the instruction's `m_profileIndex` argument
/// (`CodeBlock::binaryArithProfileForPC`, CodeBlock.cpp:3510-3527). The Rust
/// instruction stream carries no `profileIndex` operand, so the derived slot
/// carries its owning `BytecodeIndex` instead — the same keying the derived
/// `array_profiles` (`ArrayProfile::bytecode_index`) and value-profile tables
/// use. The wrapper exists so the faithful 16-bit `BinaryArithProfile` port
/// (ArithProfile.h) does not grow a non-JSC field.
#[derive(Clone, Copy, Debug, Default, Eq, PartialEq)]
pub struct BinaryArithProfileSlot {
    pub bytecode_index: BytecodeIndex,
    pub profile: BinaryArithProfile,
}

/// Per-bytecode-site `UnaryArithProfile` storage slot; see
/// `BinaryArithProfileSlot` (C++ `UnlinkedCodeBlock::m_unaryArithProfiles`,
/// UnlinkedCodeBlock.h:515,529; `CodeBlock::unaryArithProfileForPC`,
/// CodeBlock.cpp:3529-3545).
#[derive(Clone, Copy, Debug, Default, Eq, PartialEq)]
pub struct UnaryArithProfileSlot {
    pub bytecode_index: BytecodeIndex,
    pub profile: UnaryArithProfile,
}

#[derive(Clone, Debug, Default)]
pub struct LinkedSideTables {
    pub handlers: Vec<HandlerInfo>,
    pub call_sites: Vec<CallSiteRecord>,
    pub code_origins: CodeOriginTable,
    pub exception_handlers: ExceptionHandlerTable,
    pub bytecode_hooks: BytecodeHookTable,
    // C++ JSC divergence: in C++ the inline-cache state and live feedback profiles
    // live in the metadata of the single stable CodeBlock heap object and are
    // mutated in place through a shared `CodeBlock*` on the runtime feedback path --
    // there is no per-call copy. Rust now shares one stable `Rc<CodeBlock>` per
    // (executable, specialization) (see the `CodeBlockRecord`/
    // `InterpreterFunctionCodeBlock` divergence comments), so the feedback path can
    // no longer take `&mut CodeBlock`. These tables are therefore interior-mutable so
    // the IC attach/clear, ArrayProfile, and ValueProfile writes go through
    // `&CodeBlock`, exactly as C++ mutates `m_metadata`/IC state through
    // `CodeBlock*`. All other side tables are link-fixed and stay plain.
    pub inline_caches: RefCell<InlineCacheTable>,
    pub array_profiles: RefCell<Vec<ArrayProfile>>,
    pub value_profiles: RefCell<ValueProfileTable>,
    // C++ JSC keeps the arith profiles in `UnlinkedCodeBlock`
    // (`m_binaryArithProfiles`/`m_unaryArithProfiles`, UnlinkedCodeBlock.h:528-529)
    // because they are shared across re-links of the same unlinked block. The
    // Rust port derives them into the linked side tables like the other
    // feedback profiles; interior-mutable for the same shared-`Rc<CodeBlock>`
    // reason as `array_profiles`/`value_profiles` above.
    pub binary_arith_profiles: RefCell<Vec<BinaryArithProfileSlot>>,
    pub unary_arith_profiles: RefCell<Vec<UnaryArithProfileSlot>>,
    pub root_maps: Vec<BytecodeRootMap>,
    pub direct_eval_cache: Option<DirectEvalCacheRef>,
    pub catch_liveness: Vec<CatchLivenessRecord>,
    // C++ JSC: each `op_get_by_id`/`op_put_by_id` carries its monomorphic LLInt
    // cache (`LLIntGetByIdCache` / `OpPutById::Metadata`) inline in the
    // CodeBlock metadata, mutated in place through `CodeBlock*` on the slow path.
    // The Rust interpreter has no per-site metadata slab for GetByName/PutByName
    // (the existing `inline_caches` table feeds the DEFERRED JIT codegen, not
    // interpreter dispatch), so these two `BytecodeIndex`-keyed maps are the
    // minimal per-site `LLIntGetByIdCache` store. Interior-mutable for the same
    // reason as `inline_caches`/`value_profiles`: the runtime feedback path mutates
    // through the shared `Rc<CodeBlock>` (`&self`), never `&mut`. Entries are
    // created lazily on first slow-path fill (no link-time pre-seed), so unrelated
    // code blocks pay nothing.
    pub llint_get_by_id_caches: RefCell<HashMap<BytecodeIndex, LLIntGetByIdCache>>,
    pub llint_put_by_id_caches: RefCell<HashMap<BytecodeIndex, LLIntPutByIdCache>>,
}

// `RefCell` is neither `Eq` nor `PartialEq`; compare through a short borrow so the
// interior-mutable tables still participate in structural equality (used by tests
// and the request-struct fallback comparison).
impl PartialEq for LinkedSideTables {
    fn eq(&self, other: &Self) -> bool {
        self.handlers == other.handlers
            && self.call_sites == other.call_sites
            && self.code_origins == other.code_origins
            && self.exception_handlers == other.exception_handlers
            && self.bytecode_hooks == other.bytecode_hooks
            && *self.inline_caches.borrow() == *other.inline_caches.borrow()
            && *self.array_profiles.borrow() == *other.array_profiles.borrow()
            && *self.value_profiles.borrow() == *other.value_profiles.borrow()
            && *self.binary_arith_profiles.borrow() == *other.binary_arith_profiles.borrow()
            && *self.unary_arith_profiles.borrow() == *other.unary_arith_profiles.borrow()
            && self.root_maps == other.root_maps
            && self.direct_eval_cache == other.direct_eval_cache
            && self.catch_liveness == other.catch_liveness
            && *self.llint_get_by_id_caches.borrow() == *other.llint_get_by_id_caches.borrow()
            && *self.llint_put_by_id_caches.borrow() == *other.llint_put_by_id_caches.borrow()
    }
}

impl Eq for LinkedSideTables {}

impl LinkedSideTables {
    /// Shared read borrow of the interior-mutable inline-cache table.
    pub fn inline_caches(&self) -> std::cell::Ref<'_, InlineCacheTable> {
        self.inline_caches.borrow()
    }

    /// Exclusive borrow of the interior-mutable inline-cache table.
    pub fn inline_caches_mut(&self) -> std::cell::RefMut<'_, InlineCacheTable> {
        self.inline_caches.borrow_mut()
    }

    /// Shared read borrow of the interior-mutable array-profile table.
    pub fn array_profiles(&self) -> std::cell::Ref<'_, Vec<ArrayProfile>> {
        self.array_profiles.borrow()
    }

    /// Exclusive borrow of the interior-mutable array-profile table.
    pub fn array_profiles_mut(&self) -> std::cell::RefMut<'_, Vec<ArrayProfile>> {
        self.array_profiles.borrow_mut()
    }

    /// Shared read borrow of the interior-mutable value-profile table.
    pub fn value_profiles(&self) -> std::cell::Ref<'_, ValueProfileTable> {
        self.value_profiles.borrow()
    }

    /// Exclusive borrow of the interior-mutable value-profile table.
    pub fn value_profiles_mut(&self) -> std::cell::RefMut<'_, ValueProfileTable> {
        self.value_profiles.borrow_mut()
    }

    /// Shared read borrow of the interior-mutable binary-arith-profile table.
    pub fn binary_arith_profiles(&self) -> std::cell::Ref<'_, Vec<BinaryArithProfileSlot>> {
        self.binary_arith_profiles.borrow()
    }

    /// Exclusive borrow of the interior-mutable binary-arith-profile table.
    pub fn binary_arith_profiles_mut(&self) -> std::cell::RefMut<'_, Vec<BinaryArithProfileSlot>> {
        self.binary_arith_profiles.borrow_mut()
    }

    /// Shared read borrow of the interior-mutable unary-arith-profile table.
    pub fn unary_arith_profiles(&self) -> std::cell::Ref<'_, Vec<UnaryArithProfileSlot>> {
        self.unary_arith_profiles.borrow()
    }

    /// Exclusive borrow of the interior-mutable unary-arith-profile table.
    pub fn unary_arith_profiles_mut(&self) -> std::cell::RefMut<'_, Vec<UnaryArithProfileSlot>> {
        self.unary_arith_profiles.borrow_mut()
    }
}

#[derive(Clone, Debug, Eq, PartialEq)]
pub struct UnlinkedHandlerInfo {
    pub range: BytecodeRange,
    pub target: BytecodeIndex,
    pub kind: HandlerKind,
}

#[derive(Clone, Debug, Eq, PartialEq)]
pub struct HandlerInfo {
    pub range: HandlerRange,
    pub target: HandlerTarget,
    pub kind: HandlerKind,
}

#[derive(Clone, Copy, Debug, Eq, PartialEq, Hash)]
pub enum HandlerKind {
    Catch,
    Finally,
    SynthesizedCatch,
    SynthesizedFinally,
}

#[derive(Clone, Copy, Debug, Eq, PartialEq, Hash)]
pub struct BytecodeRange {
    pub start: BytecodeIndex,
    pub end: BytecodeIndex,
}

#[derive(Clone, Copy, Debug, Eq, PartialEq, Hash)]
pub enum HandlerRange {
    Bytecode(BytecodeRange),
    CallSite {
        start: CallSiteIndex,
        end: CallSiteIndex,
    },
}

#[derive(Clone, Copy, Debug, Eq, PartialEq, Hash)]
pub enum HandlerTarget {
    Bytecode(BytecodeIndex),
    Native(RuntimeSlot),
}

#[derive(Clone, Debug, Default, Eq, PartialEq)]
pub struct SourceNoteTable {
    pub chapters: Vec<SourceNoteChapter>,
    pub notes: Vec<SourceNote>,
}

impl SourceNoteTable {
    pub fn lookup(&self, bytecode_index: BytecodeIndex) -> Option<SourceNoteLookup> {
        self.notes
            .iter()
            .filter(|note| note.bytecode_index <= bytecode_index)
            .max_by_key(|note| note.bytecode_index)
            .map(|note| SourceNoteLookup {
                bytecode_index: note.bytecode_index,
                position: SourcePosition {
                    offset: note.divot,
                    line: note.line,
                    column: note.column,
                },
                range: SourceRange {
                    start: SourcePosition {
                        offset: note.divot.saturating_sub(note.start_offset_from_divot),
                        line: note.line,
                        column: note.column.saturating_sub(note.start_offset_from_divot),
                    },
                    end: SourcePosition {
                        offset: note.divot.saturating_add(note.end_offset_from_divot),
                        line: note.line,
                        column: note.column.saturating_add(note.end_offset_from_divot),
                    },
                },
            })
    }
}

#[derive(Clone, Copy, Debug, Eq, PartialEq, Hash)]
pub struct SourceNoteChapter {
    pub start_bytecode: BytecodeIndex,
    pub first_note_index: u32,
}

#[derive(Clone, Copy, Debug, Eq, PartialEq, Hash)]
pub struct SourceNote {
    pub bytecode_index: BytecodeIndex,
    pub divot: u32,
    pub start_offset_from_divot: u32,
    pub end_offset_from_divot: u32,
    pub line: u32,
    pub column: u32,
}

#[derive(Clone, Copy, Debug, Eq, PartialEq, Hash)]
pub struct DebugHookRecord {
    pub bytecode_index: BytecodeIndex,
    pub hook: DebugHookKind,
    pub source_range: SourceRange,
}

#[derive(Clone, Copy, Debug, Eq, PartialEq, Hash)]
pub enum DebugHookKind {
    WillExecuteStatement,
    DidEnterCallFrame,
    DidReachDebuggerStatement,
    WillLeaveCallFrame,
}

#[derive(Clone, Copy, Debug, Eq, PartialEq, Hash)]
pub struct TypeProfilerRange {
    pub bytecode_index: BytecodeIndex,
    pub start_divot: u32,
    pub end_divot: u32,
}

#[derive(Clone, Debug, Default, Eq, PartialEq)]
pub struct JumpTableSet {
    pub simple: Vec<UnlinkedSimpleJumpTable>,
    pub string: Vec<UnlinkedStringJumpTable>,
    pub out_of_line_targets: Vec<OutOfLineJumpTarget>,
}

#[derive(Clone, Debug, Default, Eq, PartialEq)]
pub struct UnlinkedSimpleJumpTable {
    pub kind: JumpTableKind,
    pub min: i32,
    pub branch_offsets: Vec<i32>,
    pub default_target: Option<BytecodeIndex>,
}

#[derive(Clone, Debug, Default, Eq, PartialEq)]
pub struct UnlinkedStringJumpTable {
    pub entries: Vec<StringJumpTableEntry>,
    pub default_target: Option<BytecodeIndex>,
}

#[derive(Clone, Copy, Debug, Eq, PartialEq, Hash)]
pub struct StringJumpTableEntry {
    pub identifier_index: u32,
    pub target: BytecodeIndex,
}

#[derive(Clone, Copy, Debug, Default, Eq, PartialEq, Hash)]
pub enum JumpTableKind {
    #[default]
    Dense,
    Sparse,
}

#[derive(Clone, Copy, Debug, Eq, PartialEq, Hash)]
pub struct OutOfLineJumpTarget {
    pub owner: BytecodeIndex,
    pub offset: i32,
}

#[derive(Clone, Copy, Debug, Eq, PartialEq, Hash)]
#[repr(transparent)]
pub struct BitVectorRef(pub u32);

#[derive(Clone, Copy, Debug, Eq, PartialEq, Hash)]
pub struct CallSiteRecord {
    pub call_site: CallSiteIndex,
    pub bytecode_index: Option<BytecodeIndex>,
}

#[derive(Clone, Copy, Debug, Eq, PartialEq, Hash)]
#[repr(transparent)]
pub struct DirectEvalCacheRef(pub u32);

#[derive(Clone, Copy, Debug, Eq, PartialEq, Hash)]
pub struct CatchLivenessRecord {
    pub catch_bytecode: BytecodeIndex,
    pub bit_vector: BitVectorRef,
}

/// Runtime state required to link an `UnlinkedCodeBlock`.
///
/// Real linking depends on VM, heap, global object, scope, specialization, and
/// barriered constant ownership. Runtime cell identities are imported from the
/// runtime layer; the VM token remains an opaque borrower because there is no
/// canonical VM identity type in this skeleton.
#[derive(Clone, Debug, Default, Eq, PartialEq)]
pub struct LinkContext {
    pub vm: Option<VmHandle>,
    pub owner_executable: Option<ExecutableId>,
    pub global_object: Option<GlobalObjectId>,
    pub scope: Option<ScopeId>,
    pub specialization: CodeSpecialization,
}

/// Borrower token for the VM that performs linking.
///
/// This is not heap-cell identity and must not replace `CellId`-backed runtime
/// IDs. It only records that linked-code mutation requires VM authority.
#[derive(Clone, Copy, Debug, Eq, PartialEq, Hash)]
#[repr(transparent)]
pub struct VmHandle(pub u64);

#[derive(Clone, Copy, Debug, Default, Eq, PartialEq, Hash)]
pub enum CodeSpecialization {
    #[default]
    None,
    Call,
    Construct,
}

// C++ JSC divergence (install-time interior mutability): `baseline_jit` is one
// of the three install-time fields `installCode` mutates in place through the
// shared `CodeBlock*` (see the `CodeBlock::lifecycle` field comment). Rust shares
// one `Rc<CodeBlock>`, so this single field is a `Cell` to allow the in-place
// install under `&self`; the other two fields stay plain data. Wrapping one field
// in `Cell` removes the `Copy`/`Eq`/`PartialEq`/`Hash` derives, so `PartialEq`
// (used by the call-request structural comparison) is implemented by hand over
// `.get()`; `Hash` is unused on this type.
#[derive(Clone, Debug, Default)]
pub struct CodeBlockEntrypoints {
    pub interpreter: Option<InterpreterEntrySlot>,
    pub baseline_jit: Cell<Option<JitCodeSlot>>,
    pub optimizing_jit: Option<JitCodeSlot>,
}

impl CodeBlockEntrypoints {
    /// Currently-installed baseline JIT entry slot (interior-mutable; see the
    /// type comment for the C++ in-place-install divergence).
    pub fn baseline_jit(&self) -> Option<JitCodeSlot> {
        self.baseline_jit.get()
    }
}

impl PartialEq for CodeBlockEntrypoints {
    fn eq(&self, other: &Self) -> bool {
        self.interpreter == other.interpreter
            && self.baseline_jit.get() == other.baseline_jit.get()
            && self.optimizing_jit == other.optimizing_jit
    }
}

impl Eq for CodeBlockEntrypoints {}

#[derive(Clone, Copy, Debug, Eq, PartialEq, Hash)]
#[repr(transparent)]
pub struct InterpreterEntrySlot(pub u32);

#[derive(Clone, Copy, Debug, Eq, PartialEq, Hash)]
#[repr(transparent)]
pub struct JitCodeSlot(pub u32);

// C++ JSC divergence (install-time interior mutability): `current_tier` is one of
// the three install-time fields `installCode` mutates in place through the shared
// `CodeBlock*` (see the `CodeBlock::lifecycle` field comment). Rust shares one
// `Rc<CodeBlock>`, so this single field is a `Cell` to allow the in-place install
// under `&self`; the remaining counters/flags stay plain data. The `Cell` removes
// the `Eq`/`PartialEq` derives, so equality is implemented by hand over `.get()`.
#[derive(Clone, Debug, Default)]
pub struct CodeBlockTierState {
    pub llint_counter: TierCounterState,
    pub baseline_counter: TierCounterState,
    pub optimizing_counter: TierCounterState,
    pub profiling_counters: ProfilingCounterSet,
    pub value_profile_emission_capability: ValueProfileEmissionCapability,
    pub current_tier: Cell<ExecutionTier>,
    pub replacement: Option<CodeBlockId>,
    pub osr_exit_count: u32,
    pub did_fail_jit: bool,
    pub did_fail_ftl: bool,
}

impl CodeBlockTierState {
    /// Currently-active execution tier (interior-mutable; see the type comment
    /// for the C++ in-place-install divergence).
    pub fn current_tier(&self) -> ExecutionTier {
        self.current_tier.get()
    }
}

impl PartialEq for CodeBlockTierState {
    fn eq(&self, other: &Self) -> bool {
        self.llint_counter == other.llint_counter
            && self.baseline_counter == other.baseline_counter
            && self.optimizing_counter == other.optimizing_counter
            && self.profiling_counters == other.profiling_counters
            && self.value_profile_emission_capability == other.value_profile_emission_capability
            && self.current_tier.get() == other.current_tier.get()
            && self.replacement == other.replacement
            && self.osr_exit_count == other.osr_exit_count
            && self.did_fail_jit == other.did_fail_jit
            && self.did_fail_ftl == other.did_fail_ftl
    }
}

impl Eq for CodeBlockTierState {}

#[derive(Clone, Copy, Debug, Default, Eq, PartialEq, Hash)]
pub struct TierCounterState {
    pub threshold: Option<i32>,
    pub deferred: bool,
}

#[derive(Clone, Copy, Debug, Default, Eq, PartialEq, Hash)]
pub enum ExecutionTier {
    #[default]
    Interpreter,
    BaselineJit,
    DfgJit,
    FtlJit,
}

#[derive(Clone, Copy, Debug, Default, Eq, PartialEq, Hash)]
pub struct UnlinkedTieringHints {
    pub quick_dfg_tier_up: TierHint,
    pub quick_ftl_tier_up: TierHint,
    pub has_checkpoints: bool,
}

#[derive(Clone, Copy, Debug, Default, Eq, PartialEq, Hash)]
pub enum TierHint {
    #[default]
    Unknown,
    PreferSoon,
    Avoid,
}

#[derive(Clone, Copy, Debug, Default, Eq, PartialEq, Hash)]
pub struct CodeBlockRegisterSummary {
    pub frame: RegisterFrameShape,
    pub special: SpecialRegisters,
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::bytecode::gc::{
        BytecodeRootMapId, BytecodeRootSlotDescriptor, BytecodeRootSlotKind,
    };
    use crate::bytecode::ic::{
        AccessCaseRef, CallLinkInlineCacheAttachedMetadataRequest,
        CallLinkInlineCacheAttachmentError as CallLinkAttachmentError,
        CallLinkInlineCacheAttachmentRequest, CallLinkInlineCacheClearError as CallLinkClearError,
        CallLinkInlineCacheClearMetadataMismatchField as CallLinkClearMetadataMismatchField,
        CallLinkInlineCacheClearRequest, CallLinkMode, CallTarget, CallType,
        PropertyInlineCacheAttachmentError as AttachmentError,
        PropertyInlineCacheAttachmentKind as AttachmentKind,
        PropertyInlineCacheAttachmentRequest as AttachmentRequest,
        PropertyInlineCacheAttachmentRequirements as AttachmentRequirements,
        PropertyInlineCacheClearError as ClearError,
        PropertyInlineCacheClearMetadataMismatchField as ClearMetadataMismatchField,
        PropertyInlineCacheClearRequest as ClearRequest, PropertyInlineCacheStubMode,
        StructureStubAccessCaseLinkRequest, StructureStubKind,
    };
    use crate::bytecode::instruction::{InstructionBuilder, Operand, TypedInstruction};
    use crate::bytecode::opcode::{
        CoreOpcode, MetadataFieldKind, MetadataMutability, OpcodeSchemaVersion, OperandWidth,
    };
    use crate::bytecode::register::VirtualRegister;
    use crate::bytecode::{
        PropertyAccessType, PropertyCacheKey, PropertyCacheKind, PropertyOffset, PutByIdMode,
        StructureId,
    };
    use crate::gc::CellId;
    use crate::jit::CallBoundaryId;
    use crate::runtime::ObjectId;
    use crate::strings::{AtomId, Identifier, PropertyKey};

    fn linked_interpreter_code_block() -> CodeBlock {
        let mut builder = InstructionBuilder::new();
        builder.declare_instruction(Opcode::Reserved, OperandWidth::Narrow, Vec::new());
        let instructions = builder.finalize();
        let unlinked = UnlinkedCodeBlock::new(CodeKind::Program, instructions)
            .with_phase(UnlinkedCodeBlockPhase::Finalized);

        CodeBlock::from_unlinked(unlinked, LinkContext::default())
            .with_entrypoints(CodeBlockEntrypoints {
                interpreter: Some(InterpreterEntrySlot(7)),
                ..CodeBlockEntrypoints::default()
            })
            .with_lifecycle(CodeBlockLifecycleState::LinkedInterpreter)
    }

    #[test]
    fn install_baseline_jit_data_allocates_stable_box_of_requested_size() {
        let code_block = linked_interpreter_code_block();

        // No baseline data-IC store before install.
        assert!(code_block.baseline_jit_data().is_none());
        assert!(code_block.baseline_jit_data_record_store_base().is_none());

        // Install allocates the Box once with `count` sentinel records and
        // returns the stable record-store base.
        let count = 4usize;
        let installed_base = code_block.install_baseline_jit_data(count);
        assert!(installed_base.is_some());

        {
            let data = code_block.baseline_jit_data();
            let data = data.as_ref().expect("store installed");
            assert_eq!(data.property_cache_count(), count);
            // All records are the never-matching sentinel.
            assert!(data
                .property_caches
                .iter()
                .all(|record| *record == HandlerPropertyInlineCacheRecord::SENTINEL));
            assert_eq!(data.record_store_base(), installed_base.unwrap());
        }

        // The Box is never reallocated: the base address stays stable across
        // repeated reads.
        let base_again = code_block.baseline_jit_data_record_store_base();
        assert_eq!(base_again, installed_base);
    }

    #[test]
    fn install_baseline_jit_data_zero_sites_has_no_dereferenceable_base() {
        let code_block = linked_interpreter_code_block();

        // A zero-site store is still allocated (matching C++ allocate-once
        // BaselineJITData with propertyCacheSize == 0), but exposes no
        // dereferenceable record-store base: generated code seeds r13 from a
        // dangling pointer it never reads.
        let base = code_block.install_baseline_jit_data(0);
        assert!(base.is_none());

        {
            let data = code_block.baseline_jit_data();
            let data = data.as_ref().expect("zero-site store still installed");
            assert_eq!(data.property_cache_count(), 0);
        }

        assert!(code_block.baseline_jit_data_record_store_base().is_none());
    }

    fn identifier_property_key(identifier_index: u32) -> PropertyKey {
        PropertyKey::from_identifier(Identifier::from_atom(AtomId::from_table_slot(
            identifier_index,
        )))
    }

    fn get_by_name_instruction(
        offset: u32,
        result: VirtualRegister,
        base: VirtualRegister,
        identifier_index: u32,
    ) -> TypedInstruction {
        TypedInstruction {
            opcode: CoreOpcode::GetByName.opcode(),
            width: OperandWidth::Narrow,
            operands: vec![
                Operand::Register(result),
                Operand::Register(base),
                Operand::IdentifierIndex(identifier_index),
            ],
            schema: None,
            bytecode_index: Some(BytecodeIndex::from_offset(offset)),
        }
    }

    fn get_global_object_property_instruction(
        offset: u32,
        result: VirtualRegister,
        identifier_index: u32,
    ) -> TypedInstruction {
        TypedInstruction {
            opcode: CoreOpcode::GetGlobalObjectProperty.opcode(),
            width: OperandWidth::Narrow,
            operands: vec![
                Operand::Register(result),
                Operand::IdentifierIndex(identifier_index),
            ],
            schema: None,
            bytecode_index: Some(BytecodeIndex::from_offset(offset)),
        }
    }

    fn put_by_name_instruction(
        offset: u32,
        base: VirtualRegister,
        identifier_index: u32,
        value: VirtualRegister,
    ) -> TypedInstruction {
        TypedInstruction {
            opcode: CoreOpcode::PutByName.opcode(),
            width: OperandWidth::Narrow,
            operands: vec![
                Operand::Register(base),
                Operand::IdentifierIndex(identifier_index),
                Operand::Register(value),
            ],
            schema: None,
            bytecode_index: Some(BytecodeIndex::from_offset(offset)),
        }
    }

    fn put_global_object_property_instruction(
        offset: u32,
        identifier_index: u32,
        value: VirtualRegister,
    ) -> TypedInstruction {
        TypedInstruction {
            opcode: CoreOpcode::PutGlobalObjectProperty.opcode(),
            width: OperandWidth::Narrow,
            operands: vec![
                Operand::IdentifierIndex(identifier_index),
                Operand::Register(value),
            ],
            schema: None,
            bytecode_index: Some(BytecodeIndex::from_offset(offset)),
        }
    }

    fn get_by_value_instruction(
        offset: u32,
        result: VirtualRegister,
        base: VirtualRegister,
        property: VirtualRegister,
    ) -> TypedInstruction {
        TypedInstruction {
            opcode: CoreOpcode::GetByValue.opcode(),
            width: OperandWidth::Narrow,
            operands: vec![
                Operand::Register(result),
                Operand::Register(base),
                Operand::Register(property),
            ],
            schema: None,
            bytecode_index: Some(BytecodeIndex::from_offset(offset)),
        }
    }

    fn put_by_value_instruction(
        offset: u32,
        base: VirtualRegister,
        property: VirtualRegister,
        value: VirtualRegister,
    ) -> TypedInstruction {
        TypedInstruction {
            opcode: CoreOpcode::PutByValue.opcode(),
            width: OperandWidth::Narrow,
            operands: vec![
                Operand::Register(base),
                Operand::Register(property),
                Operand::Register(value),
            ],
            schema: None,
            bytecode_index: Some(BytecodeIndex::from_offset(offset)),
        }
    }

    fn in_by_id_instruction(
        offset: u32,
        result: VirtualRegister,
        base: VirtualRegister,
        identifier_index: u32,
    ) -> TypedInstruction {
        TypedInstruction {
            opcode: CoreOpcode::InById.opcode(),
            width: OperandWidth::Narrow,
            operands: vec![
                Operand::Register(result),
                Operand::Register(base),
                Operand::IdentifierIndex(identifier_index),
            ],
            schema: None,
            bytecode_index: Some(BytecodeIndex::from_offset(offset)),
        }
    }

    fn in_by_value_instruction(
        offset: u32,
        result: VirtualRegister,
        base: VirtualRegister,
        property: VirtualRegister,
    ) -> TypedInstruction {
        TypedInstruction {
            opcode: CoreOpcode::InByVal.opcode(),
            width: OperandWidth::Narrow,
            operands: vec![
                Operand::Register(result),
                Operand::Register(base),
                Operand::Register(property),
            ],
            schema: None,
            bytecode_index: Some(BytecodeIndex::from_offset(offset)),
        }
    }

    fn ensure_this_instruction(
        offset: u32,
        result: VirtualRegister,
        source: VirtualRegister,
    ) -> TypedInstruction {
        TypedInstruction {
            opcode: CoreOpcode::EnsureThis.opcode(),
            width: OperandWidth::Narrow,
            operands: vec![Operand::Register(result), Operand::Register(source)],
            schema: None,
            bytecode_index: Some(BytecodeIndex::from_offset(offset)),
        }
    }

    fn instance_of_instruction(
        offset: u32,
        result: VirtualRegister,
        value: VirtualRegister,
        constructor: VirtualRegister,
    ) -> TypedInstruction {
        TypedInstruction {
            opcode: CoreOpcode::InstanceOf.opcode(),
            width: OperandWidth::Narrow,
            operands: vec![
                Operand::Register(result),
                Operand::Register(value),
                Operand::Register(constructor),
            ],
            schema: None,
            bytecode_index: Some(BytecodeIndex::from_offset(offset)),
        }
    }

    fn binary_arith_instruction(
        opcode: CoreOpcode,
        offset: u32,
        result: VirtualRegister,
        lhs: VirtualRegister,
        rhs: VirtualRegister,
    ) -> TypedInstruction {
        TypedInstruction {
            opcode: opcode.opcode(),
            width: OperandWidth::Narrow,
            operands: vec![
                Operand::Register(result),
                Operand::Register(lhs),
                Operand::Register(rhs),
            ],
            schema: None,
            bytecode_index: Some(BytecodeIndex::from_offset(offset)),
        }
    }

    fn unary_arith_instruction(
        opcode: CoreOpcode,
        offset: u32,
        result: VirtualRegister,
        source: VirtualRegister,
    ) -> TypedInstruction {
        TypedInstruction {
            opcode: opcode.opcode(),
            width: OperandWidth::Narrow,
            operands: vec![Operand::Register(result), Operand::Register(source)],
            schema: None,
            bytecode_index: Some(BytecodeIndex::from_offset(offset)),
        }
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

    fn call_with_this_instruction(
        offset: u32,
        destination: VirtualRegister,
        callee: VirtualRegister,
        this_value: VirtualRegister,
        arguments: Vec<VirtualRegister>,
    ) -> TypedInstruction {
        let mut operands = vec![
            Operand::Register(destination),
            Operand::Register(callee),
            Operand::Register(this_value),
            Operand::UnsignedImmediate(arguments.len().try_into().unwrap_or(u32::MAX)),
        ];
        operands.extend(arguments.into_iter().map(Operand::Register));
        TypedInstruction {
            opcode: CoreOpcode::CallWithThis.opcode(),
            width: OperandWidth::Narrow,
            operands,
            schema: None,
            bytecode_index: Some(BytecodeIndex::from_offset(offset)),
        }
    }

    fn construct_instruction(
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
            opcode: CoreOpcode::Construct.opcode(),
            width: OperandWidth::Narrow,
            operands,
            schema: None,
            bytecode_index: Some(BytecodeIndex::from_offset(offset)),
        }
    }

    fn linked_property_ic_code_block(instructions: Vec<TypedInstruction>) -> CodeBlock {
        let instructions = PackedInstructionStream::from_typed_placeholder(instructions);
        let unlinked = UnlinkedCodeBlock::new(CodeKind::Program, instructions)
            .with_phase(UnlinkedCodeBlockPhase::Finalized);

        CodeBlock::from_unlinked(unlinked, LinkContext::default())
            .with_lifecycle(CodeBlockLifecycleState::LinkedInterpreter)
    }

    fn linked_call_link_code_block(instructions: Vec<TypedInstruction>) -> CodeBlock {
        let instructions = PackedInstructionStream::from_typed_placeholder(instructions);
        let unlinked = UnlinkedCodeBlock::new(CodeKind::Program, instructions)
            .with_phase(UnlinkedCodeBlockPhase::Finalized);

        CodeBlock::from_unlinked(unlinked, LinkContext::default())
            .with_lifecycle(CodeBlockLifecycleState::LinkedInterpreter)
    }

    fn property_ic_attachment_request(
        slot: usize,
        bytecode_index: BytecodeIndex,
        identifier_index: u32,
        attachment_kind: AttachmentKind,
    ) -> AttachmentRequest {
        let is_store = attachment_kind.is_store();
        AttachmentRequest {
            slot,
            bytecode_index,
            key: identifier_property_key(identifier_index),
            attachment_kind,
            base_structure: StructureId::new(11),
            holder_structure: if attachment_kind == AttachmentKind::GetByNamePrototypeDataLoad {
                Some(StructureId::new(21))
            } else {
                None
            },
            new_structure: if attachment_kind == AttachmentKind::PutByNameStoreTransition {
                Some(StructureId::new(12))
            } else {
                None
            },
            offset: if attachment_kind == AttachmentKind::GetByNameNegativeLookup {
                None
            } else {
                Some(PropertyOffset(3))
            },
            dispatch: PropertyInlineCacheDispatch::Handler,
            stub_mode: PropertyInlineCacheStubMode::MetadataOnly,
            requirements: AttachmentRequirements {
                requires_barrier: is_store,
                has_barrier_evidence: is_store,
                requires_watchpoint: attachment_kind.is_guarded_get_by_name(),
                may_call: false,
                may_allocate: false,
            },
        }
    }

    fn property_ic_clear_request(request: AttachmentRequest) -> ClearRequest {
        ClearRequest {
            slot: request.slot,
            bytecode_index: request.bytecode_index,
            key: request.key,
            attachment_kind: request.attachment_kind,
            base_structure: request.base_structure,
            holder_structure: request.holder_structure,
            new_structure: request.new_structure,
            offset: request.offset,
            dispatch: request.dispatch,
            stub_mode: request.stub_mode,
            structure_stub_index: None,
        }
    }

    fn call_link_attachment_request(
        slot: usize,
        bytecode_index: BytecodeIndex,
        _opcode: CoreOpcode,
    ) -> CallLinkInlineCacheAttachmentRequest {
        CallLinkInlineCacheAttachmentRequest {
            slot,
            bytecode_index,
            target: CallTarget::MetadataOnlyMonomorphic {
                callee: ObjectId(CellId(60)),
                executable: ExecutableId(CellId(70)),
                code_block: CodeBlockId(CellId(71)),
                boundary: CallBoundaryId(900),
            },
        }
    }

    fn attached_call_link_metadata_request(
        request: &CallLinkInlineCacheAttachmentRequest,
    ) -> CallLinkInlineCacheAttachedMetadataRequest {
        CallLinkInlineCacheAttachedMetadataRequest {
            slot: request.slot,
            bytecode_index: request.bytecode_index,
            target: request.target.clone(),
        }
    }

    fn call_link_clear_request(
        request: &CallLinkInlineCacheAttachmentRequest,
    ) -> CallLinkInlineCacheClearRequest {
        CallLinkInlineCacheClearRequest {
            slot: request.slot,
            bytecode_index: request.bytecode_index,
            target: request.target.clone(),
        }
    }

    #[test]
    fn linked_code_block_derives_get_by_name_property_inline_cache_site() {
        let instructions = PackedInstructionStream::from_typed_placeholder(vec![
            TypedInstruction {
                opcode: CoreOpcode::GetByName.opcode(),
                width: OperandWidth::Narrow,
                operands: vec![
                    Operand::Register(VirtualRegister::local(0)),
                    Operand::Register(VirtualRegister::local(1)),
                    Operand::IdentifierIndex(17),
                ],
                schema: None,
                bytecode_index: Some(BytecodeIndex::from_offset(0)),
            },
            TypedInstruction {
                opcode: CoreOpcode::Return.opcode(),
                width: OperandWidth::Narrow,
                operands: vec![Operand::Register(VirtualRegister::local(0))],
                schema: None,
                bytecode_index: Some(BytecodeIndex::from_offset(1)),
            },
        ]);
        let unlinked = UnlinkedCodeBlock::new(CodeKind::Program, instructions)
            .with_phase(UnlinkedCodeBlockPhase::Finalized);

        let code_block = CodeBlock::from_unlinked(unlinked, LinkContext::default());
        let cache = code_block
            .side_tables()
            .inline_caches()
            .property_access_for_bytecode_index(BytecodeIndex::from_offset(0))
            .cloned()
            .expect("GetByName property IC metadata");

        assert_eq!(cache.bytecode_index, BytecodeIndex::from_offset(0));
        assert_eq!(cache.access, PropertyAccessType::GetById);
        assert_eq!(cache.kind, PropertyCacheKind::GetById);
        assert_eq!(cache.base, Some(VirtualRegister::local(1)));
        assert_eq!(
            cache.property,
            PropertyCacheKey::Key(PropertyKey::from_identifier(Identifier::from_atom(
                AtomId::from_table_slot(17),
            )))
        );
        assert!(cache.get_by_id.is_some());
        assert!(cache.put_by_id.is_none());
    }

    #[test]
    fn linked_code_block_derives_get_global_object_property_inline_cache_site() {
        let code_block =
            linked_property_ic_code_block(vec![get_global_object_property_instruction(
                0,
                VirtualRegister::local(0),
                23,
            )]);
        let cache = code_block
            .side_tables()
            .inline_caches()
            .property_access_for_bytecode_index(BytecodeIndex::from_offset(0))
            .cloned()
            .expect("GetGlobalObjectProperty property IC metadata");

        assert_eq!(cache.bytecode_index, BytecodeIndex::from_offset(0));
        assert_eq!(cache.access, PropertyAccessType::GetById);
        assert_eq!(cache.kind, PropertyCacheKind::GetById);
        assert_eq!(cache.base, None);
        assert_eq!(
            cache.property,
            PropertyCacheKey::Key(PropertyKey::from_identifier(Identifier::from_atom(
                AtomId::from_table_slot(23),
            )))
        );
        assert!(cache.get_by_id.is_some());
        assert!(cache.put_by_id.is_none());
    }

    #[test]
    fn linked_code_block_derives_put_by_name_property_inline_cache_site() {
        let instructions = PackedInstructionStream::from_typed_placeholder(vec![
            TypedInstruction {
                opcode: CoreOpcode::PutByName.opcode(),
                width: OperandWidth::Narrow,
                operands: vec![
                    Operand::Register(VirtualRegister::local(1)),
                    Operand::IdentifierIndex(19),
                    Operand::Register(VirtualRegister::local(2)),
                ],
                schema: None,
                bytecode_index: Some(BytecodeIndex::from_offset(0)),
            },
            TypedInstruction {
                opcode: CoreOpcode::Return.opcode(),
                width: OperandWidth::Narrow,
                operands: vec![Operand::Register(VirtualRegister::local(2))],
                schema: None,
                bytecode_index: Some(BytecodeIndex::from_offset(1)),
            },
        ]);
        let unlinked = UnlinkedCodeBlock::new(CodeKind::Program, instructions)
            .with_phase(UnlinkedCodeBlockPhase::Finalized);

        let code_block = CodeBlock::from_unlinked(unlinked, LinkContext::default());
        let cache = code_block
            .side_tables()
            .inline_caches()
            .property_access_for_bytecode_index(BytecodeIndex::from_offset(0))
            .cloned()
            .expect("PutByName property IC metadata");

        assert_eq!(cache.bytecode_index, BytecodeIndex::from_offset(0));
        assert_eq!(cache.access, PropertyAccessType::PutByIdSloppy);
        assert_eq!(cache.kind, PropertyCacheKind::PutById);
        assert_eq!(cache.base, Some(VirtualRegister::local(1)));
        assert_eq!(
            cache.property,
            PropertyCacheKey::Key(PropertyKey::from_identifier(Identifier::from_atom(
                AtomId::from_table_slot(19),
            )))
        );
        assert!(cache.get_by_id.is_none());
        assert!(cache.put_by_id.is_some());
    }

    #[test]
    fn linked_code_block_derives_put_global_object_property_inline_cache_site() {
        let code_block =
            linked_property_ic_code_block(vec![put_global_object_property_instruction(
                0,
                23,
                VirtualRegister::local(2),
            )]);
        let cache = code_block
            .side_tables()
            .inline_caches()
            .property_access_for_bytecode_index(BytecodeIndex::from_offset(0))
            .cloned()
            .expect("PutGlobalObjectProperty property IC metadata");

        assert_eq!(cache.bytecode_index, BytecodeIndex::from_offset(0));
        assert_eq!(cache.access, PropertyAccessType::PutByIdSloppy);
        assert_eq!(cache.kind, PropertyCacheKind::PutById);
        assert_eq!(cache.base, None);
        assert_eq!(
            cache.property,
            PropertyCacheKey::Key(PropertyKey::from_identifier(Identifier::from_atom(
                AtomId::from_table_slot(23),
            )))
        );
        assert!(cache.get_by_id.is_none());
        assert!(cache.put_by_id.is_some());
    }

    #[test]
    fn linked_code_block_derives_get_by_value_property_inline_cache_site() {
        let code_block = linked_property_ic_code_block(vec![get_by_value_instruction(
            0,
            VirtualRegister::local(0),
            VirtualRegister::local(1),
            VirtualRegister::local(2),
        )]);
        let cache = code_block
            .side_tables()
            .inline_caches()
            .property_access_for_bytecode_index(BytecodeIndex::from_offset(0))
            .cloned()
            .expect("GetByValue property IC metadata");

        assert_eq!(cache.bytecode_index, BytecodeIndex::from_offset(0));
        assert_eq!(cache.access, PropertyAccessType::GetByVal);
        assert_eq!(cache.kind, PropertyCacheKind::GetByVal);
        assert_eq!(cache.base, Some(VirtualRegister::local(1)));
        assert_eq!(
            cache.property,
            PropertyCacheKey::RuntimeValue(VirtualRegister::local(2))
        );
        assert!(cache.get_by_id.is_none());
        assert!(cache.put_by_id.is_none());
    }

    #[test]
    fn linked_code_block_derives_put_by_value_property_inline_cache_site() {
        let code_block = linked_property_ic_code_block(vec![put_by_value_instruction(
            0,
            VirtualRegister::local(1),
            VirtualRegister::local(2),
            VirtualRegister::local(3),
        )]);
        let cache = code_block
            .side_tables()
            .inline_caches()
            .property_access_for_bytecode_index(BytecodeIndex::from_offset(0))
            .cloned()
            .expect("PutByValue property IC metadata");

        assert_eq!(cache.bytecode_index, BytecodeIndex::from_offset(0));
        assert_eq!(cache.access, PropertyAccessType::PutByValSloppy);
        assert_eq!(cache.kind, PropertyCacheKind::PutByVal);
        assert_eq!(cache.base, Some(VirtualRegister::local(1)));
        assert_eq!(
            cache.property,
            PropertyCacheKey::RuntimeValue(VirtualRegister::local(2))
        );
        assert!(cache.get_by_id.is_none());
        assert!(cache.put_by_id.is_none());
    }

    #[test]
    fn linked_code_block_derives_in_by_id_property_inline_cache_site() {
        let code_block = linked_property_ic_code_block(vec![in_by_id_instruction(
            0,
            VirtualRegister::local(0),
            VirtualRegister::local(1),
            29,
        )]);
        let cache = code_block
            .side_tables()
            .inline_caches()
            .property_access_for_bytecode_index(BytecodeIndex::from_offset(0))
            .cloned()
            .expect("InById property IC metadata");

        assert_eq!(cache.bytecode_index, BytecodeIndex::from_offset(0));
        assert_eq!(cache.access, PropertyAccessType::InById);
        assert_eq!(cache.kind, PropertyCacheKind::InById);
        assert_eq!(cache.base, Some(VirtualRegister::local(1)));
        assert_eq!(
            cache.property,
            PropertyCacheKey::Key(PropertyKey::from_identifier(Identifier::from_atom(
                AtomId::from_table_slot(29),
            )))
        );
        assert!(cache.get_by_id.is_none());
        assert!(cache.put_by_id.is_none());
    }

    #[test]
    fn linked_code_block_derives_in_by_value_property_inline_cache_site() {
        let code_block = linked_property_ic_code_block(vec![in_by_value_instruction(
            0,
            VirtualRegister::local(0),
            VirtualRegister::local(1),
            VirtualRegister::local(2),
        )]);
        let cache = code_block
            .side_tables()
            .inline_caches()
            .property_access_for_bytecode_index(BytecodeIndex::from_offset(0))
            .cloned()
            .expect("InByVal property IC metadata");

        assert_eq!(cache.bytecode_index, BytecodeIndex::from_offset(0));
        assert_eq!(cache.access, PropertyAccessType::InByVal);
        assert_eq!(cache.kind, PropertyCacheKind::InByVal);
        assert_eq!(cache.base, Some(VirtualRegister::local(1)));
        assert_eq!(
            cache.property,
            PropertyCacheKey::RuntimeValue(VirtualRegister::local(2))
        );
        assert!(cache.get_by_id.is_none());
        assert!(cache.put_by_id.is_none());

        let profile = code_block
            .array_profile_for_bytecode_index(BytecodeIndex::from_offset(0))
            .expect("InByVal array profile metadata");
        assert_eq!(profile.bytecode_index, BytecodeIndex::from_offset(0));
        assert!(profile.last_seen_structure.is_none());
        assert!(profile.observed_modes.is_clear());
    }

    #[test]
    fn code_block_records_in_by_value_indexed_array_profile_read() {
        let code_block = linked_property_ic_code_block(vec![in_by_value_instruction(
            0,
            VirtualRegister::local(0),
            VirtualRegister::local(1),
            VirtualRegister::local(2),
        )])
        .with_lifecycle(CodeBlockLifecycleState::LinkedInterpreter);
        let structure = StructureId::new(77);

        let profile = code_block
            .record_array_profile_indexed_read(
                CodeBlockMutationAuthority::VmMainThread,
                BytecodeIndex::from_offset(0),
                structure,
                true,
            )
            .expect("array profile mutation succeeds")
            .expect("InByVal array profile exists");

        assert_eq!(profile.last_seen_structure, Some(structure));
        assert!(profile.flags.out_of_bounds);
        assert_eq!(
            code_block
                .array_profile_for_bytecode_index(BytecodeIndex::from_offset(0))
                .expect("profile")
                .last_seen_structure,
            Some(structure)
        );
    }

    #[test]
    fn linked_code_block_keeps_by_name_and_by_value_property_metadata_separate() {
        let instructions = PackedInstructionStream::from_typed_placeholder(vec![
            get_by_name_instruction(0, VirtualRegister::local(0), VirtualRegister::local(1), 17),
            put_by_name_instruction(1, VirtualRegister::local(2), 19, VirtualRegister::local(3)),
            put_global_object_property_instruction(2, 23, VirtualRegister::local(4)),
            get_by_value_instruction(
                3,
                VirtualRegister::local(5),
                VirtualRegister::local(6),
                VirtualRegister::local(7),
            ),
            put_by_value_instruction(
                4,
                VirtualRegister::local(8),
                VirtualRegister::local(9),
                VirtualRegister::local(10),
            ),
        ]);
        let unlinked = UnlinkedCodeBlock::new(CodeKind::Program, instructions)
            .with_phase(UnlinkedCodeBlockPhase::Finalized);

        let code_block = CodeBlock::from_unlinked(unlinked, LinkContext::default());
        let caches = code_block
            .side_tables()
            .inline_caches()
            .property_accesses
            .clone();
        assert_eq!(caches.len(), 5);

        let load = code_block
            .side_tables()
            .inline_caches()
            .property_access_for_bytecode_index(BytecodeIndex::from_offset(0))
            .cloned()
            .expect("GetByName property IC metadata");
        assert_eq!(load.access, PropertyAccessType::GetById);
        assert_eq!(load.kind, PropertyCacheKind::GetById);
        assert_eq!(load.base, Some(VirtualRegister::local(1)));
        assert!(load.get_by_id.is_some());
        assert!(load.put_by_id.is_none());

        let store = code_block
            .side_tables()
            .inline_caches()
            .property_access_for_bytecode_index(BytecodeIndex::from_offset(1))
            .cloned()
            .expect("PutByName property IC metadata");
        assert_eq!(store.access, PropertyAccessType::PutByIdSloppy);
        assert_eq!(store.kind, PropertyCacheKind::PutById);
        assert_eq!(store.base, Some(VirtualRegister::local(2)));
        assert!(store.get_by_id.is_none());
        assert!(store.put_by_id.is_some());

        let global_store = code_block
            .side_tables()
            .inline_caches()
            .property_access_for_bytecode_index(BytecodeIndex::from_offset(2))
            .cloned()
            .expect("PutGlobalObjectProperty property IC metadata");
        assert_eq!(global_store.access, PropertyAccessType::PutByIdSloppy);
        assert_eq!(global_store.kind, PropertyCacheKind::PutById);
        assert_eq!(global_store.base, None);
        assert!(global_store.get_by_id.is_none());
        assert!(global_store.put_by_id.is_some());

        let value_load = code_block
            .side_tables()
            .inline_caches()
            .property_access_for_bytecode_index(BytecodeIndex::from_offset(3))
            .cloned()
            .expect("GetByValue property IC metadata");
        assert_eq!(value_load.access, PropertyAccessType::GetByVal);
        assert_eq!(value_load.kind, PropertyCacheKind::GetByVal);
        assert_eq!(value_load.base, Some(VirtualRegister::local(6)));
        assert_eq!(
            value_load.property,
            PropertyCacheKey::RuntimeValue(VirtualRegister::local(7))
        );

        let value_store = code_block
            .side_tables()
            .inline_caches()
            .property_access_for_bytecode_index(BytecodeIndex::from_offset(4))
            .cloned()
            .expect("PutByValue property IC metadata");
        assert_eq!(value_store.access, PropertyAccessType::PutByValSloppy);
        assert_eq!(value_store.kind, PropertyCacheKind::PutByVal);
        assert_eq!(value_store.base, Some(VirtualRegister::local(8)));
        assert_eq!(
            value_store.property,
            PropertyCacheKey::RuntimeValue(VirtualRegister::local(9))
        );
    }

    #[test]
    fn linked_code_block_derives_call_link_inline_cache_sites() {
        let code_block = linked_call_link_code_block(vec![
            call_instruction(
                10,
                VirtualRegister::local(0),
                VirtualRegister::local(1),
                vec![VirtualRegister::local(2)],
            ),
            call_with_this_instruction(
                20,
                VirtualRegister::local(3),
                VirtualRegister::local(4),
                VirtualRegister::local(5),
                Vec::new(),
            ),
            construct_instruction(
                30,
                VirtualRegister::local(6),
                VirtualRegister::local(7),
                vec![VirtualRegister::local(8)],
            ),
        ]);

        let caches = code_block.side_tables().inline_caches().calls.clone();
        assert_eq!(caches.len(), 3);

        let call = code_block
            .side_tables()
            .inline_caches()
            .call_for_bytecode_index(BytecodeIndex::from_offset(10))
            .cloned()
            .expect("Call link IC metadata");
        assert_eq!(call.call_site, CallSiteIndex(10));
        assert_eq!(call.bytecode_index, BytecodeIndex::from_offset(10));
        assert_eq!(call.opcode, CoreOpcode::Call);
        assert_eq!(call.call_type, CallType::Call);
        assert_eq!(call.mode, CallLinkMode::Init);
        assert_eq!(call.specialization, CodeSpecialization::Call);
        assert_eq!(call.origin, CodeOrigin::new(BytecodeIndex::from_offset(10)));
        assert_eq!(call.target, CallTarget::Unlinked);

        let (slot, call_with_this) = code_block
            .side_tables()
            .inline_caches()
            .call_slot_for_bytecode_index(BytecodeIndex::from_offset(20))
            .map(|(slot, call)| (slot, call.clone()))
            .expect("CallWithThis link IC metadata");
        assert_eq!(slot, 1);
        assert_eq!(call_with_this.call_site, CallSiteIndex(20));
        assert_eq!(call_with_this.opcode, CoreOpcode::CallWithThis);
        assert_eq!(call_with_this.call_type, CallType::Call);
        assert_eq!(call_with_this.mode, CallLinkMode::Init);
        assert_eq!(call_with_this.target, CallTarget::Unlinked);

        let (slot, construct) = code_block
            .side_tables()
            .inline_caches()
            .call_slot_for_bytecode_index(BytecodeIndex::from_offset(30))
            .map(|(slot, call)| (slot, call.clone()))
            .expect("Construct link IC metadata");
        assert_eq!(slot, 2);
        assert_eq!(construct.call_site, CallSiteIndex(30));
        assert_eq!(construct.opcode, CoreOpcode::Construct);
        assert_eq!(construct.call_type, CallType::Construct);
        assert_eq!(construct.mode, CallLinkMode::Init);
        assert_eq!(construct.specialization, CodeSpecialization::Construct);
        assert_eq!(construct.target, CallTarget::Unlinked);
    }

    #[test]
    fn linked_code_block_derives_call_result_value_profile_sites() {
        let code_block = linked_call_link_code_block(vec![
            call_instruction(
                10,
                VirtualRegister::local(0),
                VirtualRegister::local(1),
                vec![VirtualRegister::local(2)],
            ),
            call_with_this_instruction(
                20,
                VirtualRegister::local(3),
                VirtualRegister::local(4),
                VirtualRegister::local(5),
                Vec::new(),
            ),
            construct_instruction(
                30,
                VirtualRegister::local(6),
                VirtualRegister::local(7),
                vec![VirtualRegister::local(8)],
            ),
        ]);

        let profiles = code_block.side_tables().value_profiles().clone();
        assert_eq!(profiles.profiles.len(), 2);
        assert_eq!(profiles.unlinked_predictions.len(), 2);
        assert!(
            profiles.root_metadata.is_empty(),
            "call-result value-profile samples are raw buckets, not strong roots"
        );

        let call = &profiles.profiles[0];
        assert_eq!(call.bytecode_index, BytecodeIndex::from_offset(10));
        assert_eq!(call.checkpoint, Checkpoint::NONE);
        assert_eq!(call.operand, Some(VirtualRegister::local(0)));
        assert_eq!(
            call.buckets,
            vec![ValueProfileBucket {
                slot: RuntimeSlot(1),
                kind: ValueProfileBucketKind::Sample,
            }]
        );
        let call_target = profiles
            .jit_store_target(
                BytecodeIndex::from_offset(10),
                Checkpoint::NONE,
                ValueProfileBucketKind::Sample,
            )
            .expect("Call result JIT profile store target");
        assert_eq!(call_target.binding.profile_slot, RuntimeSlot(1));
        assert_eq!(call_target.binding.value_profile_offset, 1);
        assert_eq!(call_target.binding.metadata_table_displacement, -32);
        assert_eq!(
            call_target.binding.emission_policy,
            ValueProfileEmissionPolicy::from_capability(ValueProfileEmissionCapability::CanCompile)
        );
        assert_ne!(call_target.raw_bucket_address, 0);

        let call_with_this = &profiles.profiles[1];
        assert_eq!(
            call_with_this.bytecode_index,
            BytecodeIndex::from_offset(20)
        );
        assert_eq!(call_with_this.operand, Some(VirtualRegister::local(3)));
        assert_eq!(
            call_with_this.buckets,
            vec![ValueProfileBucket {
                slot: RuntimeSlot(2),
                kind: ValueProfileBucketKind::Sample,
            }]
        );
        let call_with_this_target = profiles
            .jit_store_target(
                BytecodeIndex::from_offset(20),
                Checkpoint::NONE,
                ValueProfileBucketKind::Sample,
            )
            .expect("CallWithThis result JIT profile store target");
        assert_eq!(call_with_this_target.binding.profile_slot, RuntimeSlot(2));
        assert_eq!(call_with_this_target.binding.value_profile_offset, 2);
        assert_eq!(
            call_with_this_target.binding.metadata_table_displacement,
            -48
        );
        assert_eq!(
            call_with_this_target.binding.emission_policy,
            ValueProfileEmissionPolicy::from_capability(ValueProfileEmissionCapability::CanCompile)
        );
        assert!(
            profiles
                .profiles
                .iter()
                .all(|profile| profile.bytecode_index != BytecodeIndex::from_offset(30)),
            "construct result profiling stays explicit pending until construct normalization is wired"
        );
    }

    #[test]
    fn linked_code_block_derives_value_profiles_in_program_order() {
        // Program-order allocation across profile-carrying opcode families,
        // mirroring the C++ `m_valueProfiles` index handed out at emission
        // time (UnlinkedCodeBlock.h:380-382).
        let code_block = linked_call_link_code_block(vec![
            get_by_name_instruction(0, VirtualRegister::local(0), VirtualRegister::local(1), 7),
            get_by_value_instruction(
                10,
                VirtualRegister::local(2),
                VirtualRegister::local(3),
                VirtualRegister::local(4),
            ),
            instance_of_instruction(
                20,
                VirtualRegister::local(5),
                VirtualRegister::local(6),
                VirtualRegister::local(7),
            ),
            call_instruction(
                30,
                VirtualRegister::local(8),
                VirtualRegister::local(9),
                vec![VirtualRegister::local(10)],
            ),
            ensure_this_instruction(40, VirtualRegister::local(11), VirtualRegister::local(12)),
        ]);

        let profiles = code_block.side_tables().value_profiles().clone();
        assert_eq!(profiles.profiles.len(), 6);
        assert_eq!(profiles.unlinked_predictions.len(), 6);

        // (bytecode offset, checkpoint, profiled destination operand), in
        // program order. InstanceOf derives two operand-less slots at JSC's
        // getHasInstance/getPrototype checkpoints (BytecodeList.rb:230-249).
        let expected = [
            (0, Checkpoint::NONE, Some(VirtualRegister::local(0))),
            (10, Checkpoint::NONE, Some(VirtualRegister::local(2))),
            (20, Checkpoint(0), None),
            (20, Checkpoint(1), None),
            (30, Checkpoint::NONE, Some(VirtualRegister::local(8))),
            (40, Checkpoint::NONE, Some(VirtualRegister::local(11))),
        ];
        for (index, (offset, checkpoint, operand)) in expected.iter().enumerate() {
            let profile = &profiles.profiles[index];
            assert_eq!(profile.bytecode_index, BytecodeIndex::from_offset(*offset));
            assert_eq!(profile.checkpoint, *checkpoint);
            assert_eq!(profile.operand, *operand);
            assert_eq!(
                profile.buckets,
                vec![ValueProfileBucket {
                    slot: RuntimeSlot(index as u32 + 1),
                    kind: ValueProfileBucketKind::Sample,
                }]
            );
        }

        // Per-kind (bytecode_index, checkpoint) lookup resolves the right slot
        // and the metadata displacement math stays consistent with the
        // program-order offset: -(offset + 1) * VALUE_PROFILE_RECORD_BYTES.
        for (index, (offset, checkpoint, _)) in expected.iter().enumerate() {
            let target = profiles
                .jit_store_target(
                    BytecodeIndex::from_offset(*offset),
                    *checkpoint,
                    ValueProfileBucketKind::Sample,
                )
                .expect("derived value profile store target");
            let value_profile_offset = index as u32 + 1;
            assert_eq!(
                target.binding.profile_slot,
                RuntimeSlot(value_profile_offset)
            );
            assert_eq!(target.binding.value_profile_offset, value_profile_offset);
            assert_eq!(
                target.binding.metadata_table_displacement,
                -((value_profile_offset as i32 + 1) * 16)
            );
            assert_ne!(target.raw_bucket_address, 0);
        }
    }

    #[test]
    fn linked_code_block_derives_array_profiles_for_get_put_call_sites() {
        let code_block = linked_property_ic_code_block(vec![
            get_by_value_instruction(
                0,
                VirtualRegister::local(0),
                VirtualRegister::local(1),
                VirtualRegister::local(2),
            ),
            put_by_value_instruction(
                10,
                VirtualRegister::local(3),
                VirtualRegister::local(4),
                VirtualRegister::local(5),
            ),
            in_by_value_instruction(
                20,
                VirtualRegister::local(6),
                VirtualRegister::local(7),
                VirtualRegister::local(8),
            ),
            call_instruction(
                30,
                VirtualRegister::local(9),
                VirtualRegister::local(10),
                Vec::new(),
            ),
            // op_get_by_id carries no ArrayProfile (BytecodeList.rb:387-397).
            get_by_name_instruction(
                40,
                VirtualRegister::local(11),
                VirtualRegister::local(12),
                7,
            ),
        ]);

        let profiles = code_block.side_tables().array_profiles().clone();
        assert_eq!(profiles.len(), 4);
        for (index, offset) in [0u32, 10, 20, 30].iter().enumerate() {
            assert_eq!(
                profiles[index].bytecode_index,
                BytecodeIndex::from_offset(*offset)
            );
        }
        assert!(code_block
            .array_profile_for_bytecode_index(BytecodeIndex::from_offset(40))
            .is_none());

        // The wired InByVal runtime write keys by bytecode_index and must keep
        // working over the generalized table; a get_by_val slot records too.
        let structure = StructureId::new(31);
        for offset in [0u32, 20] {
            let profile = code_block
                .record_array_profile_indexed_read(
                    CodeBlockMutationAuthority::VmMainThread,
                    BytecodeIndex::from_offset(offset),
                    structure,
                    false,
                )
                .expect("array profile mutation succeeds")
                .expect("derived array profile exists");
            assert_eq!(profile.bytecode_index, BytecodeIndex::from_offset(offset));
            assert_eq!(profile.last_seen_structure, Some(structure));
        }
    }

    #[test]
    fn linked_code_block_derives_arith_profiles_and_records_observations() {
        let code_block = linked_property_ic_code_block(vec![
            binary_arith_instruction(
                CoreOpcode::AddInt32,
                0,
                VirtualRegister::local(0),
                VirtualRegister::local(1),
                VirtualRegister::local(2),
            ),
            unary_arith_instruction(
                CoreOpcode::NegateNumber,
                10,
                VirtualRegister::local(3),
                VirtualRegister::local(4),
            ),
            // op_mod sits in the unprofiled BinaryOp group
            // (BytecodeList.rb:1254-1274): no arith profile slot.
            binary_arith_instruction(
                CoreOpcode::ModNumber,
                20,
                VirtualRegister::local(5),
                VirtualRegister::local(6),
                VirtualRegister::local(7),
            ),
        ]);

        let binary = code_block.side_tables().binary_arith_profiles().clone();
        assert_eq!(binary.len(), 1);
        assert_eq!(binary[0].bytecode_index, BytecodeIndex::from_offset(0));
        assert_eq!(binary[0].profile.bits(), 0);

        let unary = code_block.side_tables().unary_arith_profiles().clone();
        assert_eq!(unary.len(), 1);
        assert_eq!(unary[0].bytecode_index, BytecodeIndex::from_offset(10));
        assert_eq!(unary[0].profile.bits(), 0);

        assert!(code_block
            .binary_arith_profile_for_bytecode_index(BytecodeIndex::from_offset(20))
            .is_none());
        assert!(code_block
            .unary_arith_profile_for_bytecode_index(BytecodeIndex::from_offset(20))
            .is_none());

        // Binary record round-trip: observeLHSAndRHS then observeResult
        // (ArithProfile.h:366-380, :128-145).
        let observed = code_block
            .record_binary_arith_profile_operands(
                CodeBlockMutationAuthority::VmMainThread,
                BytecodeIndex::from_offset(0),
                JsValue::from_i32(1),
                JsValue::from_double(1.5),
            )
            .expect("binary arith mutation succeeds")
            .expect("binary arith profile exists");
        assert!(observed.lhs_observed_type().is_only_int32());
        assert!(observed.rhs_observed_type().is_only_number());

        let observed = code_block
            .record_binary_arith_profile_result(
                CodeBlockMutationAuthority::VmMainThread,
                BytecodeIndex::from_offset(0),
                JsValue::from_double(2.5),
            )
            .expect("binary arith mutation succeeds")
            .expect("binary arith profile exists");
        assert!(observed.arith().did_observe_double());
        assert_eq!(
            code_block.binary_arith_profile_for_bytecode_index(BytecodeIndex::from_offset(0)),
            Some(observed),
            "record API round-trips through the stored slot"
        );

        // Unary record round-trip: observeArg then observeResult
        // (ArithProfile.h:243-255, :128-145).
        let observed = code_block
            .record_unary_arith_profile_arg(
                CodeBlockMutationAuthority::VmMainThread,
                BytecodeIndex::from_offset(10),
                JsValue::from_i32(3),
            )
            .expect("unary arith mutation succeeds")
            .expect("unary arith profile exists");
        assert!(observed.arg_observed_type().is_only_int32());

        let observed = code_block
            .record_unary_arith_profile_result(
                CodeBlockMutationAuthority::VmMainThread,
                BytecodeIndex::from_offset(10),
                JsValue::from_double(0.5),
            )
            .expect("unary arith mutation succeeds")
            .expect("unary arith profile exists");
        assert!(observed.arith().did_observe_double());
        assert_eq!(
            code_block.unary_arith_profile_for_bytecode_index(BytecodeIndex::from_offset(10)),
            Some(observed)
        );

        // No slot at an unprofiled site: Ok(None), matching
        // record_array_profile_indexed_read's missing-profile contract.
        assert_eq!(
            code_block.record_binary_arith_profile_operands(
                CodeBlockMutationAuthority::VmMainThread,
                BytecodeIndex::from_offset(20),
                JsValue::from_i32(1),
                JsValue::from_i32(2),
            ),
            Ok(None)
        );

        // Wrong mutation authority is rejected before any lookup.
        assert!(matches!(
            code_block.record_unary_arith_profile_arg(
                CodeBlockMutationAuthority::GcVisitor,
                BytecodeIndex::from_offset(10),
                JsValue::from_i32(3),
            ),
            Err(CodeBlockMutationError::InvalidMutationAuthority { .. })
        ));
    }

    #[test]
    fn linked_code_block_rebinds_value_profile_policy_from_tier_capability() {
        let code_block = linked_call_link_code_block(vec![call_instruction(
            10,
            VirtualRegister::local(0),
            VirtualRegister::local(1),
            vec![VirtualRegister::local(2)],
        )])
        .with_tier_state(CodeBlockTierState {
            value_profile_emission_capability: ValueProfileEmissionCapability::CannotCompile,
            ..CodeBlockTierState::default()
        });
        let target = code_block
            .side_tables()
            .value_profiles()
            .jit_store_target(
                BytecodeIndex::from_offset(10),
                Checkpoint::NONE,
                ValueProfileBucketKind::Sample,
            )
            .expect("Call result JIT profile store target");

        assert_eq!(
            target.binding.emission_policy,
            ValueProfileEmissionPolicy::from_capability(
                ValueProfileEmissionCapability::CannotCompile
            )
        );
        assert!(!target.binding.emission_policy.should_emit);
    }

    #[test]
    fn code_block_attaches_metadata_only_call_link_inline_cache() {
        let code_block = linked_call_link_code_block(vec![call_instruction(
            10,
            VirtualRegister::local(0),
            VirtualRegister::local(1),
            vec![VirtualRegister::local(2)],
        )]);
        let request =
            call_link_attachment_request(0, BytecodeIndex::from_offset(10), CoreOpcode::Call);
        let expected_target = request.target.clone();

        let outcome = code_block
            .attach_call_link_inline_cache(
                CodeBlockMutationAuthority::VmMainThread,
                request.clone(),
            )
            .expect("call link metadata attachment succeeds");

        assert_eq!(outcome.slot, 0);
        assert_eq!(outcome.bytecode_index, BytecodeIndex::from_offset(10));
        assert_eq!(outcome.call_site, CallSiteIndex(10));
        assert_eq!(outcome.opcode, CoreOpcode::Call);
        assert_eq!(outcome.call_type, CallType::Call);
        assert_eq!(outcome.mode, CallLinkMode::Monomorphic);
        assert_eq!(outcome.specialization, CodeSpecialization::Call);
        assert_eq!(outcome.target, expected_target);
        assert_eq!(outcome.slow_path_count, 0);
        assert_eq!(outcome.max_argument_count_including_this_for_varargs, 0);

        let metadata = code_block
            .attached_call_link_inline_cache_metadata(attached_call_link_metadata_request(&request))
            .expect("attached call link metadata validates");
        assert_eq!(metadata.mode, CallLinkMode::Monomorphic);
        assert_eq!(metadata.target, expected_target);

        let call = code_block.side_tables().inline_caches().calls[0].clone();
        assert_eq!(call.mode, CallLinkMode::Monomorphic);
        assert_eq!(call.target, expected_target);
        assert_eq!(call.slow_path_count, 0);
        assert_eq!(call.max_argument_count_including_this_for_varargs, 0);
    }

    #[test]
    fn code_block_rejects_call_link_inline_cache_attachment_without_mutation() {
        let code_block = linked_call_link_code_block(vec![call_instruction(
            10,
            VirtualRegister::local(0),
            VirtualRegister::local(1),
            vec![VirtualRegister::local(2)],
        )]);
        let mut request =
            call_link_attachment_request(0, BytecodeIndex::from_offset(10), CoreOpcode::Call);
        request.target = CallTarget::DirectExecutable(ExecutableId(CellId(99)));
        let before = code_block.side_tables().inline_caches().clone();

        let error = code_block
            .attach_call_link_inline_cache(CodeBlockMutationAuthority::VmMainThread, request)
            .expect_err("direct executable target must fail");

        assert_eq!(
            error,
            CallLinkAttachmentError::InvalidRequestedTarget {
                actual: CallTarget::DirectExecutable(ExecutableId(CellId(99))),
            }
        );
        assert_eq!(&*code_block.side_tables().inline_caches(), &before);

        let request =
            call_link_attachment_request(0, BytecodeIndex::from_offset(10), CoreOpcode::Call);
        code_block
            .attach_call_link_inline_cache(
                CodeBlockMutationAuthority::VmMainThread,
                request.clone(),
            )
            .expect("initial call link metadata attachment succeeds");
        let before_duplicate = code_block.side_tables().inline_caches().clone();

        let error = code_block
            .attach_call_link_inline_cache(CodeBlockMutationAuthority::VmMainThread, request)
            .expect_err("duplicate attachment must fail");

        assert_eq!(
            error,
            CallLinkAttachmentError::InvalidExistingMode {
                slot: 0,
                expected: CallLinkMode::Init,
                actual: CallLinkMode::Monomorphic,
            }
        );
        assert_eq!(
            &*code_block.side_tables().inline_caches(),
            &before_duplicate
        );
    }

    #[test]
    fn code_block_clears_metadata_only_call_link_inline_cache() {
        let code_block = linked_call_link_code_block(vec![
            call_instruction(
                10,
                VirtualRegister::local(0),
                VirtualRegister::local(1),
                vec![VirtualRegister::local(2)],
            ),
            get_by_name_instruction(15, VirtualRegister::local(3), VirtualRegister::local(4), 17),
            call_with_this_instruction(
                20,
                VirtualRegister::local(5),
                VirtualRegister::local(6),
                VirtualRegister::local(7),
                Vec::new(),
            ),
        ]);
        let request =
            call_link_attachment_request(0, BytecodeIndex::from_offset(10), CoreOpcode::Call);
        let attached_target = request.target.clone();
        code_block
            .attach_call_link_inline_cache(
                CodeBlockMutationAuthority::VmMainThread,
                request.clone(),
            )
            .expect("initial call link metadata attachment succeeds");

        let before_clear = code_block.side_tables().inline_caches().clone();
        assert_eq!(before_clear.calls[0].target, attached_target);

        let outcome = code_block
            .clear_call_link_inline_cache(
                CodeBlockMutationAuthority::VmMainThread,
                call_link_clear_request(&request),
            )
            .expect("call link metadata clear succeeds");

        assert_eq!(outcome.slot, 0);
        assert_eq!(outcome.bytecode_index, BytecodeIndex::from_offset(10));
        assert_eq!(outcome.call_site, CallSiteIndex(10));
        assert_eq!(outcome.opcode, CoreOpcode::Call);
        assert_eq!(outcome.call_type, CallType::Call);
        assert_eq!(outcome.mode, CallLinkMode::Init);
        assert_eq!(outcome.specialization, CodeSpecialization::Call);
        assert_eq!(outcome.target, CallTarget::Unlinked);
        assert_eq!(outcome.slow_path_count, 0);
        assert_eq!(outcome.max_argument_count_including_this_for_varargs, 0);

        let after_clear = code_block.side_tables().inline_caches().clone();
        assert_eq!(
            after_clear.property_accesses,
            before_clear.property_accesses
        );
        assert_eq!(after_clear.structure_stubs, before_clear.structure_stubs);
        assert_eq!(after_clear.iteration_modes, before_clear.iteration_modes);
        assert_eq!(after_clear.calls[1], before_clear.calls[1]);

        let call = &after_clear.calls[0];
        assert_eq!(call.call_site, before_clear.calls[0].call_site);
        assert_eq!(call.bytecode_index, before_clear.calls[0].bytecode_index);
        assert_eq!(call.opcode, before_clear.calls[0].opcode);
        assert_eq!(call.origin, before_clear.calls[0].origin);
        assert_eq!(call.call_type, CallType::Call);
        assert_eq!(call.mode, CallLinkMode::Init);
        assert_eq!(call.specialization, CodeSpecialization::Call);
        assert_eq!(call.target, CallTarget::Unlinked);
        assert_eq!(call.slow_path_count, before_clear.calls[0].slow_path_count);
        assert_eq!(
            call.max_argument_count_including_this_for_varargs,
            before_clear.calls[0].max_argument_count_including_this_for_varargs
        );
        assert_eq!(call.flags, before_clear.calls[0].flags);
    }

    #[test]
    fn code_block_clears_metadata_only_construct_link_inline_cache_to_construct_shape() {
        let code_block = linked_call_link_code_block(vec![construct_instruction(
            30,
            VirtualRegister::local(0),
            VirtualRegister::local(1),
            vec![VirtualRegister::local(2)],
        )]);
        let request =
            call_link_attachment_request(0, BytecodeIndex::from_offset(30), CoreOpcode::Construct);
        code_block
            .attach_call_link_inline_cache(
                CodeBlockMutationAuthority::VmMainThread,
                request.clone(),
            )
            .expect("initial construct link metadata attachment succeeds");

        let outcome = code_block
            .clear_call_link_inline_cache(
                CodeBlockMutationAuthority::VmMainThread,
                call_link_clear_request(&request),
            )
            .expect("construct link metadata clear succeeds");

        assert_eq!(outcome.opcode, CoreOpcode::Construct);
        assert_eq!(outcome.call_type, CallType::Construct);
        assert_eq!(outcome.mode, CallLinkMode::Init);
        assert_eq!(outcome.specialization, CodeSpecialization::Construct);
        assert_eq!(outcome.target, CallTarget::Unlinked);

        let call = code_block.side_tables().inline_caches().calls[0].clone();
        assert_eq!(call.opcode, CoreOpcode::Construct);
        assert_eq!(call.call_type, CallType::Construct);
        assert_eq!(call.mode, CallLinkMode::Init);
        assert_eq!(call.specialization, CodeSpecialization::Construct);
        assert_eq!(call.target, CallTarget::Unlinked);
    }

    #[test]
    fn code_block_rejects_call_link_inline_cache_clear_with_wrong_authority_or_lifecycle() {
        let mut code_block = linked_call_link_code_block(vec![call_instruction(
            10,
            VirtualRegister::local(0),
            VirtualRegister::local(1),
            vec![VirtualRegister::local(2)],
        )]);
        let request =
            call_link_attachment_request(0, BytecodeIndex::from_offset(10), CoreOpcode::Call);
        code_block
            .attach_call_link_inline_cache(
                CodeBlockMutationAuthority::VmMainThread,
                request.clone(),
            )
            .expect("initial call link metadata attachment succeeds");
        let clear_request = call_link_clear_request(&request);
        let before = code_block.side_tables().inline_caches().clone();

        let error = code_block
            .clear_call_link_inline_cache(
                CodeBlockMutationAuthority::ReadOnlyObserver,
                clear_request.clone(),
            )
            .expect_err("caller authority must fail");

        assert_eq!(
            error,
            CallLinkClearError::InvalidMutationAuthority {
                expected: CodeBlockMutationAuthority::VmMainThread,
                actual: CodeBlockMutationAuthority::ReadOnlyObserver,
            }
        );
        assert_eq!(&*code_block.side_tables().inline_caches(), &before);

        code_block.mutation_authority = CodeBlockMutationAuthority::ReadOnlyObserver;
        let error = code_block
            .clear_call_link_inline_cache(CodeBlockMutationAuthority::VmMainThread, clear_request)
            .expect_err("current code block authority must fail");

        assert_eq!(
            error,
            CallLinkClearError::InvalidMutationAuthority {
                expected: CodeBlockMutationAuthority::VmMainThread,
                actual: CodeBlockMutationAuthority::ReadOnlyObserver,
            }
        );
        assert_eq!(&*code_block.side_tables().inline_caches(), &before);

        let optimizing_code_block = linked_call_link_code_block(vec![call_instruction(
            10,
            VirtualRegister::local(0),
            VirtualRegister::local(1),
            vec![VirtualRegister::local(2)],
        )]);
        optimizing_code_block
            .attach_call_link_inline_cache(
                CodeBlockMutationAuthority::VmMainThread,
                request.clone(),
            )
            .expect("initial call link metadata attachment succeeds");
        optimizing_code_block
            .lifecycle
            .set(CodeBlockLifecycleState::OptimizingInstalled);
        let before_lifecycle = optimizing_code_block.side_tables().inline_caches().clone();

        let error = optimizing_code_block
            .clear_call_link_inline_cache(
                CodeBlockMutationAuthority::VmMainThread,
                call_link_clear_request(&request),
            )
            .expect_err("disallowed lifecycle must fail");

        assert_eq!(
            error,
            CallLinkClearError::InvalidLifecycle {
                actual: CodeBlockLifecycleState::OptimizingInstalled,
            }
        );
        assert_eq!(
            &*optimizing_code_block.side_tables().inline_caches(),
            &before_lifecycle
        );
    }

    #[test]
    fn code_block_rejects_call_link_inline_cache_clear_target_mismatches_without_mutation() {
        let code_block = linked_call_link_code_block(vec![call_instruction(
            10,
            VirtualRegister::local(0),
            VirtualRegister::local(1),
            vec![VirtualRegister::local(2)],
        )]);
        let request =
            call_link_attachment_request(0, BytecodeIndex::from_offset(10), CoreOpcode::Call);
        let before_missing = code_block.side_tables().inline_caches().clone();

        let error = code_block
            .clear_call_link_inline_cache(
                CodeBlockMutationAuthority::VmMainThread,
                call_link_clear_request(&request),
            )
            .expect_err("missing attached target must fail");

        assert_eq!(
            error,
            CallLinkClearError::InvalidExistingMode {
                slot: 0,
                expected: CallLinkMode::Monomorphic,
                actual: CallLinkMode::Init,
            }
        );
        assert_eq!(&*code_block.side_tables().inline_caches(), &before_missing);

        code_block
            .attach_call_link_inline_cache(
                CodeBlockMutationAuthority::VmMainThread,
                request.clone(),
            )
            .expect("initial call link metadata attachment succeeds");
        let before_wrong_target = code_block.side_tables().inline_caches().clone();
        let mut wrong_target = call_link_clear_request(&request);
        wrong_target.target = CallTarget::MetadataOnlyMonomorphic {
            callee: ObjectId(CellId(61)),
            executable: ExecutableId(CellId(70)),
            code_block: CodeBlockId(CellId(71)),
            boundary: CallBoundaryId(900),
        };

        let error = code_block
            .clear_call_link_inline_cache(CodeBlockMutationAuthority::VmMainThread, wrong_target)
            .expect_err("wrong attached target must fail");

        assert_eq!(
            error,
            CallLinkClearError::AttachedMetadataMismatch {
                slot: 0,
                field: CallLinkClearMetadataMismatchField::Callee,
            }
        );
        assert_eq!(
            &*code_block.side_tables().inline_caches(),
            &before_wrong_target
        );

        code_block
            .clear_call_link_inline_cache(
                CodeBlockMutationAuthority::VmMainThread,
                call_link_clear_request(&request),
            )
            .expect("first call link metadata clear succeeds");
        let after_clear = code_block.side_tables().inline_caches().clone();

        let error = code_block
            .clear_call_link_inline_cache(
                CodeBlockMutationAuthority::VmMainThread,
                call_link_clear_request(&request),
            )
            .expect_err("duplicate clear must fail");

        assert_eq!(
            error,
            CallLinkClearError::InvalidExistingMode {
                slot: 0,
                expected: CallLinkMode::Monomorphic,
                actual: CallLinkMode::Init,
            }
        );
        assert_eq!(&*code_block.side_tables().inline_caches(), &after_clear);
    }

    #[test]
    fn code_block_attaches_get_by_name_own_data_load_property_ic_case() {
        let code_block = linked_property_ic_code_block(vec![get_by_name_instruction(
            0,
            VirtualRegister::local(0),
            VirtualRegister::local(1),
            17,
        )]);
        let request = property_ic_attachment_request(
            0,
            BytecodeIndex::from_offset(0),
            17,
            AttachmentKind::GetByNameOwnDataLoad,
        );

        let outcome = code_block
            .attach_property_inline_cache_case(CodeBlockMutationAuthority::VmMainThread, request)
            .expect("load attachment succeeds");

        assert_eq!(outcome.slot, 0);
        assert_eq!(
            outcome.attachment_kind,
            AttachmentKind::GetByNameOwnDataLoad
        );
        assert_eq!(outcome.state, InlineCacheState::Monomorphic);
        assert_eq!(outcome.dispatch, PropertyInlineCacheDispatch::Handler);
        assert_eq!(outcome.structure_stub_index, None);

        let cache = code_block.side_tables().inline_caches().property_accesses[0].clone();
        assert_eq!(cache.state, InlineCacheState::Monomorphic);
        assert_eq!(cache.dispatch, PropertyInlineCacheDispatch::Handler);
        let get_by_id = cache.get_by_id.expect("get metadata");
        assert_eq!(get_by_id.mode, GetByIdMode::Default);
        assert_eq!(get_by_id.structure, Some(request.base_structure));
        assert_eq!(get_by_id.holder_structure, None);
        assert_eq!(get_by_id.cached_offset, request.offset);
        assert_eq!(get_by_id.cached_slot, None);
        assert!(cache.put_by_id.is_none());
        assert!(code_block
            .side_tables()
            .inline_caches()
            .structure_stubs
            .is_empty());
    }

    #[test]
    fn code_block_property_inline_cache_attachment_attaches_get_by_name_own_data_load_structure_stub_case(
    ) {
        let code_block = linked_property_ic_code_block(vec![get_by_name_instruction(
            0,
            VirtualRegister::local(0),
            VirtualRegister::local(1),
            17,
        )]);
        let mut request = property_ic_attachment_request(
            0,
            BytecodeIndex::from_offset(0),
            17,
            AttachmentKind::GetByNameOwnDataLoad,
        );
        request.stub_mode = PropertyInlineCacheStubMode::StructureStub;

        let outcome = code_block
            .attach_property_inline_cache_case(CodeBlockMutationAuthority::VmMainThread, request)
            .expect("structure-stub own data load attachment succeeds");

        assert_eq!(
            outcome.attachment_kind,
            AttachmentKind::GetByNameOwnDataLoad
        );
        assert_eq!(
            outcome.stub_mode,
            PropertyInlineCacheStubMode::StructureStub
        );
        assert_eq!(outcome.structure_stub_index, Some(0));

        let cache = code_block.side_tables().inline_caches().property_accesses[0].clone();
        assert_eq!(cache.state, InlineCacheState::Monomorphic);
        assert_eq!(cache.dispatch, PropertyInlineCacheDispatch::Handler);
        let get_by_id = cache.get_by_id.expect("get metadata");
        assert_eq!(get_by_id.mode, GetByIdMode::Default);
        assert_eq!(get_by_id.structure, Some(request.base_structure));
        assert_eq!(get_by_id.holder_structure, None);
        assert_eq!(get_by_id.cached_offset, request.offset);

        let stubs = code_block
            .side_tables()
            .inline_caches()
            .structure_stubs
            .clone();
        assert_eq!(stubs.len(), 1);
        let stub = &stubs[0];
        assert_eq!(stub.bytecode_index, request.bytecode_index);
        assert_eq!(stub.inline_cache_slot, request.slot);
        assert_eq!(stub.attachment_kind, request.attachment_kind);
        assert_eq!(stub.key, request.key);
        assert_eq!(stub.base_structure, request.base_structure);
        assert_eq!(stub.holder_structure, None);
        assert_eq!(stub.new_structure, None);
        assert_eq!(stub.offset, request.offset);
        assert_eq!(stub.requirements, request.requirements);
        assert_eq!(stub.kind, StructureStubKind::GetById);
        assert_eq!(stub.cache_state, InlineCacheState::Monomorphic);
        assert_eq!(stub.code_origin, CodeOrigin::new(request.bytecode_index));
        assert!(stub.access_cases.is_empty());
        assert!(!stub.reset_by_gc);

        let access_case_ref = AccessCaseRef((u64::from(u32::MAX)) + 1);
        let link_request = StructureStubAccessCaseLinkRequest {
            structure_stub_index: outcome.structure_stub_index.unwrap(),
            bytecode_index: request.bytecode_index,
            slot: request.slot,
            key: request.key,
            attachment_kind: request.attachment_kind,
            base_structure: request.base_structure,
            holder_structure: request.holder_structure,
            new_structure: request.new_structure,
            offset: request.offset,
            access_case_ref,
        };
        let link_outcome = code_block
            .link_structure_stub_access_case(CodeBlockMutationAuthority::VmMainThread, link_request)
            .expect("structure-stub access case link succeeds");
        assert!(link_outcome.inserted);
        assert_eq!(link_outcome.access_case_count, 1);
        assert_eq!(
            code_block.side_tables().inline_caches().structure_stubs[0].access_cases,
            vec![access_case_ref]
        );

        let duplicate_link_outcome = code_block
            .link_structure_stub_access_case(CodeBlockMutationAuthority::VmMainThread, link_request)
            .expect("duplicate structure-stub access case link is idempotent");
        assert!(!duplicate_link_outcome.inserted);
        assert_eq!(duplicate_link_outcome.access_case_count, 1);
        assert_eq!(
            code_block.side_tables().inline_caches().structure_stubs[0].access_cases,
            vec![access_case_ref]
        );

        let mut clear_request = property_ic_clear_request(request);
        clear_request.structure_stub_index = outcome.structure_stub_index;
        let clear_outcome = code_block
            .clear_property_inline_cache_case(
                CodeBlockMutationAuthority::VmMainThread,
                clear_request,
            )
            .expect("structure-stub own data load clear succeeds");

        assert_eq!(clear_outcome.structure_stub_index, Some(0));
        assert_eq!(clear_outcome.state, InlineCacheState::Unset);
        let cache = code_block.side_tables().inline_caches().property_accesses[0].clone();
        assert_eq!(cache.state, InlineCacheState::Unset);
        assert_eq!(cache.dispatch, PropertyInlineCacheDispatch::Unlinked);
        assert_eq!(
            code_block.side_tables().inline_caches().structure_stubs[0].cache_state,
            InlineCacheState::Unset
        );
        assert!(code_block.side_tables().inline_caches().structure_stubs[0]
            .access_cases
            .is_empty());
    }

    #[test]
    fn code_block_attaches_guarded_get_by_name_prototype_data_property_ic_case() {
        let code_block = linked_property_ic_code_block(vec![get_by_name_instruction(
            0,
            VirtualRegister::local(0),
            VirtualRegister::local(1),
            17,
        )]);
        let request = property_ic_attachment_request(
            0,
            BytecodeIndex::from_offset(0),
            17,
            AttachmentKind::GetByNamePrototypeDataLoad,
        );

        let outcome = code_block
            .attach_property_inline_cache_case(CodeBlockMutationAuthority::VmMainThread, request)
            .expect("prototype data guarded load attachment succeeds");

        assert_eq!(
            outcome.attachment_kind,
            AttachmentKind::GetByNamePrototypeDataLoad
        );
        assert_eq!(outcome.stub_mode, PropertyInlineCacheStubMode::MetadataOnly);
        assert_eq!(outcome.structure_stub_index, None);
        assert_eq!(outcome.offset, request.offset);
        assert_eq!(outcome.holder_structure, request.holder_structure);

        let cache = code_block.side_tables().inline_caches().property_accesses[0].clone();
        assert_eq!(cache.state, InlineCacheState::Monomorphic);
        assert_eq!(cache.dispatch, PropertyInlineCacheDispatch::Handler);
        let get_by_id = cache.get_by_id.expect("get metadata");
        assert_eq!(get_by_id.mode, GetByIdMode::ProtoLoad);
        assert_eq!(get_by_id.structure, Some(request.base_structure));
        assert_eq!(get_by_id.holder_structure, request.holder_structure);
        assert_eq!(get_by_id.cached_offset, request.offset);
        assert_eq!(get_by_id.cached_slot, None);
        assert!(cache.put_by_id.is_none());
        assert!(code_block
            .side_tables()
            .inline_caches()
            .structure_stubs
            .is_empty());
    }

    #[test]
    fn code_block_attaches_guarded_get_by_name_negative_lookup_property_ic_case_without_offset() {
        let code_block = linked_property_ic_code_block(vec![get_by_name_instruction(
            0,
            VirtualRegister::local(0),
            VirtualRegister::local(1),
            17,
        )]);
        let request = property_ic_attachment_request(
            0,
            BytecodeIndex::from_offset(0),
            17,
            AttachmentKind::GetByNameNegativeLookup,
        );

        let outcome = code_block
            .attach_property_inline_cache_case(CodeBlockMutationAuthority::VmMainThread, request)
            .expect("negative lookup guarded load attachment succeeds");

        assert_eq!(
            outcome.attachment_kind,
            AttachmentKind::GetByNameNegativeLookup
        );
        assert_eq!(outcome.stub_mode, PropertyInlineCacheStubMode::MetadataOnly);
        assert_eq!(outcome.structure_stub_index, None);
        assert_eq!(outcome.offset, None);

        let cache = code_block.side_tables().inline_caches().property_accesses[0].clone();
        assert_eq!(cache.state, InlineCacheState::Monomorphic);
        assert_eq!(cache.dispatch, PropertyInlineCacheDispatch::Handler);
        let get_by_id = cache.get_by_id.expect("get metadata");
        assert_eq!(get_by_id.mode, GetByIdMode::NegativeLookup);
        assert_eq!(get_by_id.structure, Some(request.base_structure));
        assert_eq!(get_by_id.holder_structure, None);
        assert_eq!(get_by_id.cached_offset, None);
        assert_eq!(get_by_id.cached_slot, None);
        assert!(cache.put_by_id.is_none());
        assert!(code_block
            .side_tables()
            .inline_caches()
            .structure_stubs
            .is_empty());
    }

    #[test]
    fn code_block_clears_attached_guarded_get_by_name_prototype_data_property_ic_case() {
        let code_block = linked_property_ic_code_block(vec![get_by_name_instruction(
            0,
            VirtualRegister::local(0),
            VirtualRegister::local(1),
            17,
        )]);
        let attachment = property_ic_attachment_request(
            0,
            BytecodeIndex::from_offset(0),
            17,
            AttachmentKind::GetByNamePrototypeDataLoad,
        );
        code_block
            .attach_property_inline_cache_case(CodeBlockMutationAuthority::VmMainThread, attachment)
            .expect("prototype data guarded load attachment succeeds");

        let outcome = code_block
            .clear_property_inline_cache_case(
                CodeBlockMutationAuthority::VmMainThread,
                property_ic_clear_request(attachment),
            )
            .expect("prototype data guarded load clear succeeds");

        assert_eq!(
            outcome.attachment_kind,
            AttachmentKind::GetByNamePrototypeDataLoad
        );
        assert_eq!(outcome.state, InlineCacheState::Unset);
        assert_eq!(outcome.dispatch, PropertyInlineCacheDispatch::Unlinked);
        let cache = code_block.side_tables().inline_caches().property_accesses[0].clone();
        assert_eq!(cache.state, InlineCacheState::Unset);
        assert_eq!(cache.dispatch, PropertyInlineCacheDispatch::Unlinked);
        let get_by_id = cache.get_by_id.expect("cleared get metadata");
        assert_eq!(get_by_id.mode, GetByIdMode::Unset);
        assert_eq!(get_by_id.structure, None);
        assert_eq!(get_by_id.holder_structure, None);
        assert_eq!(get_by_id.cached_offset, None);
        assert!(cache.put_by_id.is_none());
        assert!(code_block
            .side_tables()
            .inline_caches()
            .structure_stubs
            .is_empty());
    }

    #[test]
    fn code_block_clears_attached_guarded_get_by_name_negative_lookup_property_ic_case_without_offset(
    ) {
        let code_block = linked_property_ic_code_block(vec![get_by_name_instruction(
            0,
            VirtualRegister::local(0),
            VirtualRegister::local(1),
            17,
        )]);
        let attachment = property_ic_attachment_request(
            0,
            BytecodeIndex::from_offset(0),
            17,
            AttachmentKind::GetByNameNegativeLookup,
        );
        code_block
            .attach_property_inline_cache_case(CodeBlockMutationAuthority::VmMainThread, attachment)
            .expect("negative lookup guarded load attachment succeeds");

        let outcome = code_block
            .clear_property_inline_cache_case(
                CodeBlockMutationAuthority::VmMainThread,
                property_ic_clear_request(attachment),
            )
            .expect("negative lookup guarded load clear succeeds");

        assert_eq!(
            outcome.attachment_kind,
            AttachmentKind::GetByNameNegativeLookup
        );
        assert_eq!(outcome.offset, None);
        assert_eq!(outcome.state, InlineCacheState::Unset);
        assert_eq!(outcome.dispatch, PropertyInlineCacheDispatch::Unlinked);
        let cache = code_block.side_tables().inline_caches().property_accesses[0].clone();
        assert_eq!(cache.state, InlineCacheState::Unset);
        assert_eq!(cache.dispatch, PropertyInlineCacheDispatch::Unlinked);
        let get_by_id = cache.get_by_id.expect("cleared get metadata");
        assert_eq!(get_by_id.mode, GetByIdMode::Unset);
        assert_eq!(get_by_id.structure, None);
        assert_eq!(get_by_id.holder_structure, None);
        assert_eq!(get_by_id.cached_offset, None);
        assert!(cache.put_by_id.is_none());
    }

    #[test]
    fn code_block_rejects_property_ic_clear_mismatches_without_partial_mutation() {
        let code_block = linked_property_ic_code_block(vec![
            get_by_name_instruction(0, VirtualRegister::local(0), VirtualRegister::local(1), 17),
            put_by_name_instruction(1, VirtualRegister::local(2), 19, VirtualRegister::local(3)),
        ]);
        let attachment = property_ic_attachment_request(
            0,
            BytecodeIndex::from_offset(0),
            17,
            AttachmentKind::GetByNamePrototypeDataLoad,
        );
        code_block
            .attach_property_inline_cache_case(CodeBlockMutationAuthority::VmMainThread, attachment)
            .expect("prototype data guarded load attachment succeeds");
        let before = code_block.side_tables().inline_caches().clone();

        let mut wrong_slot = property_ic_clear_request(attachment);
        wrong_slot.slot = 1;
        assert!(matches!(
            code_block.clear_property_inline_cache_case(
                CodeBlockMutationAuthority::VmMainThread,
                wrong_slot,
            ),
            Err(ClearError::BytecodeIndexMismatch { .. })
        ));
        assert_eq!(&*code_block.side_tables().inline_caches(), &before);

        let mut wrong_index = property_ic_clear_request(attachment);
        wrong_index.bytecode_index = BytecodeIndex::from_offset(99);
        assert!(matches!(
            code_block.clear_property_inline_cache_case(
                CodeBlockMutationAuthority::VmMainThread,
                wrong_index,
            ),
            Err(ClearError::BytecodeIndexMismatch { .. })
        ));
        assert_eq!(&*code_block.side_tables().inline_caches(), &before);

        let mut wrong_key = property_ic_clear_request(attachment);
        wrong_key.key = identifier_property_key(18);
        assert!(matches!(
            code_block.clear_property_inline_cache_case(
                CodeBlockMutationAuthority::VmMainThread,
                wrong_key,
            ),
            Err(ClearError::PropertyKeyMismatch { .. })
        ));
        assert_eq!(&*code_block.side_tables().inline_caches(), &before);

        let mut wrong_kind = property_ic_clear_request(attachment);
        wrong_kind.attachment_kind = AttachmentKind::GetByNameOwnDataLoad;
        wrong_kind.holder_structure = None;
        assert_eq!(
            code_block.clear_property_inline_cache_case(
                CodeBlockMutationAuthority::VmMainThread,
                wrong_kind,
            ),
            Err(ClearError::AttachedMetadataMismatch {
                slot: 0,
                field: ClearMetadataMismatchField::GetByIdMode,
            })
        );
        assert_eq!(&*code_block.side_tables().inline_caches(), &before);

        code_block
            .clear_property_inline_cache_case(
                CodeBlockMutationAuthority::VmMainThread,
                property_ic_clear_request(attachment),
            )
            .expect("first clear succeeds");
        let after_clear = code_block.side_tables().inline_caches().clone();
        assert!(matches!(
            code_block.clear_property_inline_cache_case(
                CodeBlockMutationAuthority::VmMainThread,
                property_ic_clear_request(attachment),
            ),
            Err(ClearError::InvalidExistingState {
                expected: InlineCacheState::Monomorphic,
                actual: InlineCacheState::Unset,
                ..
            })
        ));
        assert_eq!(&*code_block.side_tables().inline_caches(), &after_clear);
    }

    #[test]
    fn code_block_attaches_put_by_name_store_replace_property_ic_case() {
        let code_block = linked_property_ic_code_block(vec![put_by_name_instruction(
            0,
            VirtualRegister::local(1),
            19,
            VirtualRegister::local(2),
        )]);
        let request = property_ic_attachment_request(
            0,
            BytecodeIndex::from_offset(0),
            19,
            AttachmentKind::PutByNameStoreReplace,
        );

        let outcome = code_block
            .attach_property_inline_cache_case(CodeBlockMutationAuthority::VmMainThread, request)
            .expect("store replace attachment succeeds");

        assert_eq!(
            outcome.attachment_kind,
            AttachmentKind::PutByNameStoreReplace
        );
        assert_eq!(outcome.structure_stub_index, None);

        let cache = code_block.side_tables().inline_caches().property_accesses[0].clone();
        assert_eq!(cache.state, InlineCacheState::Monomorphic);
        assert_eq!(cache.dispatch, PropertyInlineCacheDispatch::Handler);
        assert!(cache.get_by_id.is_none());
        let put_by_id = cache.put_by_id.expect("put metadata");
        assert_eq!(put_by_id.mode, PutByIdMode::Replace);
        assert_eq!(put_by_id.old_structure, Some(request.base_structure));
        assert_eq!(put_by_id.new_structure, None);
        assert_eq!(put_by_id.cached_offset, request.offset);
        assert!(code_block
            .side_tables()
            .inline_caches()
            .structure_stubs
            .is_empty());
    }

    #[test]
    fn code_block_attaches_put_by_name_store_transition_property_ic_case() {
        let code_block = linked_property_ic_code_block(vec![put_by_name_instruction(
            0,
            VirtualRegister::local(1),
            19,
            VirtualRegister::local(2),
        )])
        .with_lifecycle(CodeBlockLifecycleState::BaselineInstalled);
        let request = property_ic_attachment_request(
            0,
            BytecodeIndex::from_offset(0),
            19,
            AttachmentKind::PutByNameStoreTransition,
        );

        let outcome = code_block
            .attach_property_inline_cache_case(CodeBlockMutationAuthority::VmMainThread, request)
            .expect("store transition attachment succeeds");

        assert_eq!(
            outcome.attachment_kind,
            AttachmentKind::PutByNameStoreTransition
        );
        assert_eq!(outcome.new_structure, request.new_structure);
        assert_eq!(outcome.structure_stub_index, None);

        let cache = code_block.side_tables().inline_caches().property_accesses[0].clone();
        assert_eq!(cache.state, InlineCacheState::Monomorphic);
        assert_eq!(cache.dispatch, PropertyInlineCacheDispatch::Handler);
        let put_by_id = cache.put_by_id.expect("put metadata");
        assert_eq!(put_by_id.mode, PutByIdMode::Transition);
        assert_eq!(put_by_id.old_structure, Some(request.base_structure));
        assert_eq!(put_by_id.new_structure, request.new_structure);
        assert_eq!(put_by_id.cached_offset, request.offset);
        assert!(code_block
            .side_tables()
            .inline_caches()
            .structure_stubs
            .is_empty());
    }

    #[test]
    fn code_block_property_inline_cache_attachment_rejects_structure_stub_for_guarded_loads_and_stores(
    ) {
        let reject_structure_stub =
            |instruction: TypedInstruction, identifier_index: u32, kind: AttachmentKind| {
                let code_block = linked_property_ic_code_block(vec![instruction]);
                let mut request = property_ic_attachment_request(
                    0,
                    BytecodeIndex::from_offset(0),
                    identifier_index,
                    kind,
                );
                request.stub_mode = PropertyInlineCacheStubMode::StructureStub;
                let before = code_block.side_tables().inline_caches().clone();

                let error = code_block
                    .attach_property_inline_cache_case(
                        CodeBlockMutationAuthority::VmMainThread,
                        request,
                    )
                    .expect_err("unsupported structure-stub attachment kind must fail");

                assert_eq!(
                    error,
                    AttachmentError::UnsupportedStubMode {
                        actual: PropertyInlineCacheStubMode::StructureStub,
                    }
                );
                assert_eq!(&*code_block.side_tables().inline_caches(), &before);
                assert!(code_block
                    .side_tables()
                    .inline_caches()
                    .structure_stubs
                    .is_empty());
            };

        reject_structure_stub(
            get_by_name_instruction(0, VirtualRegister::local(0), VirtualRegister::local(1), 17),
            17,
            AttachmentKind::GetByNamePrototypeDataLoad,
        );
        reject_structure_stub(
            get_by_name_instruction(0, VirtualRegister::local(0), VirtualRegister::local(1), 17),
            17,
            AttachmentKind::GetByNameNegativeLookup,
        );
        reject_structure_stub(
            put_by_name_instruction(0, VirtualRegister::local(1), 19, VirtualRegister::local(2)),
            19,
            AttachmentKind::PutByNameStoreReplace,
        );
        reject_structure_stub(
            put_by_name_instruction(0, VirtualRegister::local(1), 19, VirtualRegister::local(2)),
            19,
            AttachmentKind::PutByNameStoreTransition,
        );
    }

    #[test]
    fn code_block_rejects_property_ic_attachment_with_wrong_authority() {
        let mut code_block = linked_property_ic_code_block(vec![get_by_name_instruction(
            0,
            VirtualRegister::local(0),
            VirtualRegister::local(1),
            17,
        )]);
        let request = property_ic_attachment_request(
            0,
            BytecodeIndex::from_offset(0),
            17,
            AttachmentKind::GetByNameOwnDataLoad,
        );
        let before = code_block.side_tables().inline_caches().clone();

        let error = code_block
            .attach_property_inline_cache_case(
                CodeBlockMutationAuthority::ReadOnlyObserver,
                request,
            )
            .expect_err("caller authority must fail");

        assert_eq!(
            error,
            AttachmentError::InvalidMutationAuthority {
                expected: CodeBlockMutationAuthority::VmMainThread,
                actual: CodeBlockMutationAuthority::ReadOnlyObserver,
            }
        );
        assert_eq!(&*code_block.side_tables().inline_caches(), &before);

        code_block.mutation_authority = CodeBlockMutationAuthority::ReadOnlyObserver;
        let error = code_block
            .attach_property_inline_cache_case(CodeBlockMutationAuthority::VmMainThread, request)
            .expect_err("current code block authority must fail");

        assert_eq!(
            error,
            AttachmentError::InvalidMutationAuthority {
                expected: CodeBlockMutationAuthority::VmMainThread,
                actual: CodeBlockMutationAuthority::ReadOnlyObserver,
            }
        );
        assert_eq!(&*code_block.side_tables().inline_caches(), &before);
    }

    #[test]
    fn code_block_rejects_property_ic_attachment_from_disallowed_lifecycle() {
        let code_block = linked_property_ic_code_block(vec![get_by_name_instruction(
            0,
            VirtualRegister::local(0),
            VirtualRegister::local(1),
            17,
        )])
        .with_lifecycle(CodeBlockLifecycleState::OptimizingInstalled);
        let request = property_ic_attachment_request(
            0,
            BytecodeIndex::from_offset(0),
            17,
            AttachmentKind::GetByNameOwnDataLoad,
        );
        let before = code_block.side_tables().inline_caches().clone();

        let error = code_block
            .attach_property_inline_cache_case(CodeBlockMutationAuthority::VmMainThread, request)
            .expect_err("disallowed lifecycle must fail");

        assert_eq!(
            error,
            AttachmentError::InvalidLifecycle {
                actual: CodeBlockLifecycleState::OptimizingInstalled,
            }
        );
        assert_eq!(&*code_block.side_tables().inline_caches(), &before);
    }

    #[test]
    fn code_block_rejects_property_ic_attachment_for_slot_mismatch() {
        let code_block = linked_property_ic_code_block(vec![
            get_by_name_instruction(0, VirtualRegister::local(0), VirtualRegister::local(1), 17),
            put_by_name_instruction(1, VirtualRegister::local(2), 19, VirtualRegister::local(3)),
        ]);
        let request = property_ic_attachment_request(
            1,
            BytecodeIndex::from_offset(0),
            17,
            AttachmentKind::GetByNameOwnDataLoad,
        );
        let before = code_block.side_tables().inline_caches().clone();

        let error = code_block
            .attach_property_inline_cache_case(CodeBlockMutationAuthority::VmMainThread, request)
            .expect_err("slot mismatch must fail");

        assert_eq!(
            error,
            AttachmentError::BytecodeIndexMismatch {
                slot: 1,
                expected: BytecodeIndex::from_offset(1),
                actual: BytecodeIndex::from_offset(0),
            }
        );
        assert_eq!(&*code_block.side_tables().inline_caches(), &before);
    }

    #[test]
    fn code_block_rejects_property_ic_attachment_for_key_or_bytecode_mismatch() {
        let code_block = linked_property_ic_code_block(vec![get_by_name_instruction(
            0,
            VirtualRegister::local(0),
            VirtualRegister::local(1),
            17,
        )]);
        let key_mismatch = property_ic_attachment_request(
            0,
            BytecodeIndex::from_offset(0),
            18,
            AttachmentKind::GetByNameOwnDataLoad,
        );
        let before = code_block.side_tables().inline_caches().clone();

        let error = code_block
            .attach_property_inline_cache_case(
                CodeBlockMutationAuthority::VmMainThread,
                key_mismatch,
            )
            .expect_err("key mismatch must fail");

        assert_eq!(
            error,
            AttachmentError::PropertyKeyMismatch {
                slot: 0,
                expected: PropertyCacheKey::Key(identifier_property_key(17)),
                actual: identifier_property_key(18),
            }
        );
        assert_eq!(&*code_block.side_tables().inline_caches(), &before);

        let bytecode_mismatch = property_ic_attachment_request(
            0,
            BytecodeIndex::from_offset(99),
            17,
            AttachmentKind::GetByNameOwnDataLoad,
        );
        let error = code_block
            .attach_property_inline_cache_case(
                CodeBlockMutationAuthority::VmMainThread,
                bytecode_mismatch,
            )
            .expect_err("bytecode mismatch must fail");

        assert_eq!(
            error,
            AttachmentError::BytecodeIndexMismatch {
                slot: 0,
                expected: BytecodeIndex::from_offset(0),
                actual: BytecodeIndex::from_offset(99),
            }
        );
        assert_eq!(&*code_block.side_tables().inline_caches(), &before);
    }

    #[test]
    fn code_block_rejects_property_ic_attachment_for_access_kind_mismatch() {
        let code_block = linked_property_ic_code_block(vec![put_by_name_instruction(
            0,
            VirtualRegister::local(1),
            17,
            VirtualRegister::local(2),
        )]);
        let request = property_ic_attachment_request(
            0,
            BytecodeIndex::from_offset(0),
            17,
            AttachmentKind::GetByNameOwnDataLoad,
        );
        let before = code_block.side_tables().inline_caches().clone();

        let error = code_block
            .attach_property_inline_cache_case(CodeBlockMutationAuthority::VmMainThread, request)
            .expect_err("access mismatch must fail");

        assert_eq!(
            error,
            AttachmentError::AccessKindMismatch {
                slot: 0,
                expected_access: PropertyAccessType::GetById,
                actual_access: PropertyAccessType::PutByIdSloppy,
                expected_kind: PropertyCacheKind::GetById,
                actual_kind: PropertyCacheKind::PutById,
            }
        );
        assert_eq!(&*code_block.side_tables().inline_caches(), &before);
    }

    #[test]
    fn code_block_rejects_property_ic_attachment_that_requires_watchpoints() {
        let code_block = linked_property_ic_code_block(vec![get_by_name_instruction(
            0,
            VirtualRegister::local(0),
            VirtualRegister::local(1),
            17,
        )]);
        let mut request = property_ic_attachment_request(
            0,
            BytecodeIndex::from_offset(0),
            17,
            AttachmentKind::GetByNameOwnDataLoad,
        );
        request.requirements.requires_watchpoint = true;
        let before = code_block.side_tables().inline_caches().clone();

        let error = code_block
            .attach_property_inline_cache_case(CodeBlockMutationAuthority::VmMainThread, request)
            .expect_err("watchpoint-required attachment must fail");

        assert_eq!(
            error,
            AttachmentError::WatchpointBridgeUnavailable {
                attachment_kind: AttachmentKind::GetByNameOwnDataLoad,
            }
        );
        assert_eq!(&*code_block.side_tables().inline_caches(), &before);
    }

    #[test]
    fn code_block_rejects_guarded_property_ic_attachment_without_watchpoint_evidence_or_coherent_metadata(
    ) {
        let code_block = linked_property_ic_code_block(vec![get_by_name_instruction(
            0,
            VirtualRegister::local(0),
            VirtualRegister::local(1),
            17,
        )]);
        let mut no_watchpoint = property_ic_attachment_request(
            0,
            BytecodeIndex::from_offset(0),
            17,
            AttachmentKind::GetByNamePrototypeDataLoad,
        );
        no_watchpoint.requirements.requires_watchpoint = false;
        let before = code_block.side_tables().inline_caches().clone();

        let error = code_block
            .attach_property_inline_cache_case(
                CodeBlockMutationAuthority::VmMainThread,
                no_watchpoint,
            )
            .expect_err("guarded load without watchpoint bridge evidence must fail");

        assert_eq!(
            error,
            AttachmentError::WatchpointBridgeUnavailable {
                attachment_kind: AttachmentKind::GetByNamePrototypeDataLoad,
            }
        );
        assert_eq!(&*code_block.side_tables().inline_caches(), &before);

        let mut missing_holder = property_ic_attachment_request(
            0,
            BytecodeIndex::from_offset(0),
            17,
            AttachmentKind::GetByNamePrototypeDataLoad,
        );
        missing_holder.holder_structure = None;
        let error = code_block
            .attach_property_inline_cache_case(
                CodeBlockMutationAuthority::VmMainThread,
                missing_holder,
            )
            .expect_err("prototype data guarded load without holder must fail");

        assert_eq!(
            error,
            AttachmentError::MissingHolderStructure {
                attachment_kind: AttachmentKind::GetByNamePrototypeDataLoad,
            }
        );
        assert_eq!(&*code_block.side_tables().inline_caches(), &before);

        let mut fake_offset = property_ic_attachment_request(
            0,
            BytecodeIndex::from_offset(0),
            17,
            AttachmentKind::GetByNameNegativeLookup,
        );
        fake_offset.offset = Some(PropertyOffset(7));
        let error = code_block
            .attach_property_inline_cache_case(
                CodeBlockMutationAuthority::VmMainThread,
                fake_offset,
            )
            .expect_err("negative lookup guarded load with fake offset must fail");

        assert_eq!(
            error,
            AttachmentError::UnexpectedPropertyOffset {
                attachment_kind: AttachmentKind::GetByNameNegativeLookup,
                offset: PropertyOffset(7),
            }
        );
        assert_eq!(&*code_block.side_tables().inline_caches(), &before);
    }

    #[test]
    fn code_block_rejects_store_property_ic_attachment_without_barrier_evidence() {
        let code_block = linked_property_ic_code_block(vec![put_by_name_instruction(
            0,
            VirtualRegister::local(1),
            19,
            VirtualRegister::local(2),
        )]);
        let mut request = property_ic_attachment_request(
            0,
            BytecodeIndex::from_offset(0),
            19,
            AttachmentKind::PutByNameStoreReplace,
        );
        request.requirements.has_barrier_evidence = false;
        let before = code_block.side_tables().inline_caches().clone();

        let error = code_block
            .attach_property_inline_cache_case(CodeBlockMutationAuthority::VmMainThread, request)
            .expect_err("store without barrier evidence must fail");

        assert_eq!(
            error,
            AttachmentError::MissingStoreBarrierEvidence {
                attachment_kind: AttachmentKind::PutByNameStoreReplace,
            }
        );
        assert_eq!(&*code_block.side_tables().inline_caches(), &before);
    }

    #[test]
    fn code_block_property_ic_attachment_rejection_does_not_partially_mutate() {
        let code_block = linked_property_ic_code_block(vec![get_by_name_instruction(
            0,
            VirtualRegister::local(0),
            VirtualRegister::local(1),
            17,
        )]);
        let mut request = property_ic_attachment_request(
            0,
            BytecodeIndex::from_offset(0),
            17,
            AttachmentKind::GetByNameOwnDataLoad,
        );
        request.new_structure = Some(StructureId::new(99));
        let before = code_block.side_tables().inline_caches().clone();

        let error = code_block
            .attach_property_inline_cache_case(CodeBlockMutationAuthority::VmMainThread, request)
            .expect_err("incompatible request must fail");

        assert_eq!(
            error,
            AttachmentError::IncompatibleNewStructure {
                attachment_kind: AttachmentKind::GetByNameOwnDataLoad,
                new_structure: Some(StructureId::new(99)),
            }
        );
        assert_eq!(&*code_block.side_tables().inline_caches(), &before);
        assert!(code_block
            .side_tables()
            .inline_caches()
            .structure_stubs
            .is_empty());
    }

    #[test]
    fn unlinked_code_block_owns_sorted_string_literal_table() {
        let mut builder = InstructionBuilder::new();
        builder.declare_instruction(Opcode::Reserved, OperandWidth::Narrow, Vec::new());
        let instructions = builder.finalize();
        let unlinked = UnlinkedCodeBlock::new(CodeKind::Program, instructions)
            .with_string_literals(vec![
                (9, "zeta".to_string()),
                (2, "alpha".to_string()),
                (5, "middle".to_string()),
            ])
            .with_phase(UnlinkedCodeBlockPhase::Finalized);

        assert_eq!(unlinked.string_literal(2), Some("alpha"));
        assert_eq!(unlinked.string_literal(9), Some("zeta"));
        assert_eq!(unlinked.string_literal(7), None);
        assert_eq!(
            unlinked
                .string_literals()
                .entries()
                .iter()
                .map(|entry| entry.identifier_index)
                .collect::<Vec<_>>(),
            vec![2, 5, 9]
        );
    }

    #[test]
    fn code_block_execution_surface_decodes_metadata_and_source_notes() {
        let mut builder = InstructionBuilder::new();
        builder.declare_instruction(
            Opcode::Reserved,
            OperandWidth::Narrow,
            vec![Operand::MetadataIndex(0)],
        );
        let instructions = builder.finalize();
        let bytecode_index = BytecodeIndex::from_offset(0);
        let field = MetadataFieldSpec {
            name: "profile",
            kind: MetadataFieldKind::ValueProfile,
            mutability: MetadataMutability::LinkedMutable,
        };
        let metadata = MetadataTable::from_entries(
            vec![MetadataEntry {
                bytecode_index,
                opcode: Opcode::Reserved,
                fields: vec![LinkedMetadataField {
                    spec: field,
                    storage: LinkedMetadataStorage::Unallocated,
                }],
            }],
            MetadataLayout::default(),
            MetadataLinkingData::default(),
            OpcodeSchemaVersion(1),
        );
        let side_tables = UnlinkedSideTables {
            source_notes: SourceNoteTable {
                notes: vec![SourceNote {
                    bytecode_index,
                    divot: 10,
                    start_offset_from_divot: 2,
                    end_offset_from_divot: 3,
                    line: 4,
                    column: 6,
                }],
                ..SourceNoteTable::default()
            },
            ..UnlinkedSideTables::default()
        };
        let unlinked = UnlinkedCodeBlock::new(CodeKind::Program, instructions)
            .with_side_tables(side_tables)
            .with_phase(UnlinkedCodeBlockPhase::Finalized);
        let code_block = CodeBlock::from_unlinked(unlinked, LinkContext::default())
            .with_metadata(metadata)
            .with_entrypoints(CodeBlockEntrypoints {
                interpreter: Some(InterpreterEntrySlot(7)),
                ..CodeBlockEntrypoints::default()
            })
            .with_lifecycle(CodeBlockLifecycleState::LinkedInterpreter);

        let surface = code_block.execution_surface();
        let decoded = surface.instruction_at(bytecode_index).expect("instruction");
        let metadata_entry = surface.metadata_entry(bytecode_index).expect("metadata");
        let source_note = surface.source_note(bytecode_index).expect("source note");
        let link_record = surface.link_record();

        assert_eq!(decoded.metadata_index_operand(0), Ok(0));
        assert_eq!(metadata_entry.fields[0].spec, field);
        assert_eq!(source_note.position.offset, 10);
        assert_eq!(link_record.instruction_count, 1);
        assert_eq!(link_record.root_map_count, 0);
        assert_eq!(link_record.value_profile_root_count, 0);
        assert!(link_record.has_interpreter_entry);
    }

    #[test]
    fn code_block_links_unlinked_root_maps_and_stamps_owner() {
        let mut builder = InstructionBuilder::new();
        builder.declare_instruction(Opcode::Reserved, OperandWidth::Narrow, Vec::new());
        let bytecode_index = BytecodeIndex::from_offset(0);
        let root_map = BytecodeRootMap {
            id: BytecodeRootMapId(7),
            owner: None,
            bytecode_range_start: bytecode_index,
            bytecode_range_end: bytecode_index,
            slots: vec![BytecodeRootSlotDescriptor::virtual_register(
                bytecode_index,
                VirtualRegister::local(0),
                BytecodeRootSlotKind::VirtualRegister,
            )],
            complete: true,
        };
        let unlinked = UnlinkedCodeBlock::new(CodeKind::Program, builder.finalize())
            .with_side_tables(UnlinkedSideTables {
                root_maps: vec![root_map],
                ..UnlinkedSideTables::default()
            })
            .with_phase(UnlinkedCodeBlockPhase::Finalized);
        let owner = CodeBlockId(CellId(12));
        let code_block =
            CodeBlock::from_unlinked(unlinked, LinkContext::default()).with_root_map_owner(owner);

        assert_eq!(code_block.side_tables().root_maps.len(), 1);
        assert_eq!(code_block.side_tables().root_maps[0].owner, Some(owner));
        assert_eq!(code_block.link_record().root_map_count, 1);
    }

    #[test]
    fn code_block_installs_baseline_slot_with_vm_authority() {
        let code_block = linked_interpreter_code_block();
        let slot = JitCodeSlot(11);

        let result =
            code_block.install_baseline_jit_slot(CodeBlockMutationAuthority::VmMainThread, slot);

        assert_eq!(result, Ok(()));
        assert_eq!(code_block.entrypoints().baseline_jit(), Some(slot));
        assert_eq!(
            code_block.tier_state().current_tier(),
            ExecutionTier::BaselineJit
        );
        assert_eq!(
            code_block.lifecycle(),
            CodeBlockLifecycleState::BaselineInstalled
        );
    }

    #[test]
    fn code_block_rejects_baseline_slot_update_with_wrong_authority() {
        let code_block = linked_interpreter_code_block();
        let entrypoints = code_block.entrypoints().clone();
        let tier_state = code_block.tier_state().clone();
        let lifecycle = code_block.lifecycle();

        let error = code_block
            .install_baseline_jit_slot(
                CodeBlockMutationAuthority::ReadOnlyObserver,
                JitCodeSlot(12),
            )
            .expect_err("wrong authority must fail");

        assert_eq!(
            error,
            CodeBlockMutationError::InvalidMutationAuthority {
                expected: CodeBlockMutationAuthority::VmMainThread,
                actual: CodeBlockMutationAuthority::ReadOnlyObserver,
            }
        );
        assert_eq!(code_block.entrypoints(), &entrypoints);
        assert_eq!(code_block.tier_state(), &tier_state);
        assert_eq!(code_block.lifecycle(), lifecycle);
    }

    #[test]
    fn code_block_rejects_baseline_slot_update_from_stale_lifecycle() {
        let code_block = linked_interpreter_code_block()
            .with_lifecycle(CodeBlockLifecycleState::BaselineInstalled);
        let entrypoints = code_block.entrypoints().clone();
        let tier_state = code_block.tier_state().clone();

        let error = code_block
            .install_baseline_jit_slot(CodeBlockMutationAuthority::VmMainThread, JitCodeSlot(13))
            .expect_err("stale lifecycle must fail");

        assert_eq!(
            error,
            CodeBlockMutationError::InvalidLifecycle {
                expected: CodeBlockLifecycleState::LinkedInterpreter,
                actual: CodeBlockLifecycleState::BaselineInstalled,
            }
        );
        assert_eq!(code_block.entrypoints(), &entrypoints);
        assert_eq!(code_block.tier_state(), &tier_state);
        assert_eq!(
            code_block.lifecycle(),
            CodeBlockLifecycleState::BaselineInstalled
        );
    }

    /// `CodeBlock::setConstantRegisters` (CodeBlock.cpp:1044-1101) places
    /// `constants[i]` at `m_constantRegisters[i]`, and readers index by
    /// `VirtualRegister::toConstantIndex()` (CodeBlock.h:203-206). Linking must
    /// therefore place each constant at ITS constant index, not at its
    /// incidental unlinked vector position; unclaimed slots hold the empty
    /// value (a default `WriteBarrier<Unknown>` is the empty JSValue), and
    /// function decls/exprs link positionally (CodeBlock.cpp:425-440).
    #[test]
    fn linked_constants_are_placed_at_their_constant_index() {
        let value = JsValue::from_encoded(crate::value::EncodedJsValue(0x1234));
        let mut pool = UnlinkedConstantPool::default();
        // The ONLY entry claims constant index 1: vector position (0) and
        // constant index (1) intentionally disagree.
        pool.constants.push(UnlinkedConstant {
            register: VirtualRegister::constant(1),
            value: ConstantValue::Encoded(value),
            source_representation: SourceCodeRepresentation::IntegerLiteral,
        });
        pool.function_declarations.push(UnlinkedFunctionRef(4));
        pool.function_expressions.push(UnlinkedFunctionRef(9));
        let unlinked =
            UnlinkedCodeBlock::new(CodeKind::Program, PackedInstructionStream::default())
                .with_constants(pool);

        let linked = linked_constants_from_unlinked(&unlinked);

        assert_eq!(linked.constants.len(), 2);
        // constant(1) is served at index 1 — where op_mov's decoded constant
        // index points.
        assert_eq!(linked.constants[1].register, VirtualRegister::constant(1));
        assert_eq!(linked.constants[1].value, ConstantValue::Encoded(value));
        // The unclaimed slot 0 holds the empty JSValue, like an unwritten
        // WriteBarrier<Unknown>.
        assert_eq!(linked.constants[0].register, VirtualRegister::constant(0));
        assert_eq!(
            linked.constants[0].value,
            ConstantValue::Encoded(JsValue::default())
        );
        // Function declarations/expressions are linked positionally, not
        // dropped.
        assert_eq!(linked.function_declarations, vec![LinkedFunctionRef(4)]);
        assert_eq!(linked.function_expressions, vec![LinkedFunctionRef(9)]);
    }
}
