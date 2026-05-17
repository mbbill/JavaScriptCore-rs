use std::sync::Arc;

use crate::strings::Identifier;
use crate::value::JsValue;

use crate::bytecode::debug::{BytecodeHookTable, ExceptionHandlerTable};
use crate::bytecode::ic::InlineCacheTable;
use crate::bytecode::instruction::PackedInstructionStream;
use crate::bytecode::metadata::{InstructionMetadataPlan, MetadataLayout, MetadataLinkingData};
use crate::bytecode::opcode::{MetadataFieldSpec, Opcode, OpcodeSchemaVersion};
use crate::bytecode::origin::CodeOriginTable;
use crate::bytecode::profiling::{ProfilingCounterSet, ValueProfileTable};
use crate::bytecode::register::{RegisterFrameShape, SpecialRegisters, VirtualRegister};

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

#[derive(Clone, Copy, Debug, Eq, PartialEq, Hash)]
pub enum ParseMode {
    Program,
    Module,
    Eval,
    NormalFunction,
    ArrowFunction,
    Method,
    Getter,
    Setter,
    ClassFieldInitializer,
    GeneratorBody,
    AsyncFunctionBody,
    AsyncGeneratorBody,
}

#[derive(Clone, Copy, Debug, Eq, PartialEq, Hash)]
pub enum ScriptMode {
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

#[derive(Clone, Copy, Debug, Eq, PartialEq, Hash)]
#[repr(transparent)]
pub struct SourceProviderId(pub u64);

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

#[derive(Clone, Debug, Default, Eq, PartialEq)]
pub struct ExecutableInfo {
    pub parse_mode: Option<ParseMode>,
    pub script_mode: Option<ScriptMode>,
    pub eval_context: EvalContext,
    pub constructor_kind: ConstructorKind,
    pub super_binding: SuperBinding,
    pub is_builtin_function: bool,
    pub is_class_context: bool,
    pub is_arrow_function_context: bool,
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
}

impl CodeBlock {
    pub fn from_unlinked(unlinked: UnlinkedCodeBlock, context: LinkContext) -> Self {
        Self::from_shared_unlinked(Arc::new(unlinked), context)
    }

    pub fn from_shared_unlinked(unlinked: Arc<UnlinkedCodeBlock>, context: LinkContext) -> Self {
        Self {
            unlinked,
            link_context: context,
            metadata: MetadataTable::default(),
            constants: LinkedConstantPool::default(),
            side_tables: LinkedSideTables::default(),
            tier_state: CodeBlockTierState::default(),
            entrypoints: CodeBlockEntrypoints::default(),
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

    pub fn side_tables(&self) -> &LinkedSideTables {
        &self.side_tables
    }

    pub fn tier_state(&self) -> &CodeBlockTierState {
        &self.tier_state
    }

    pub fn entrypoints(&self) -> &CodeBlockEntrypoints {
        &self.entrypoints
    }
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
    pub fn entries(&self) -> &[UnlinkedMetadataEntry] {
        &self.entries
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
    pub fn entries(&self) -> &[MetadataEntry] {
        &self.entries
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
}

#[derive(Clone, Debug, Default, Eq, PartialEq)]
pub struct LinkedSideTables {
    pub handlers: Vec<HandlerInfo>,
    pub call_sites: Vec<CallSiteRecord>,
    pub code_origins: CodeOriginTable,
    pub exception_handlers: ExceptionHandlerTable,
    pub bytecode_hooks: BytecodeHookTable,
    pub inline_caches: InlineCacheTable,
    pub value_profiles: ValueProfileTable,
    pub direct_eval_cache: Option<DirectEvalCacheRef>,
    pub catch_liveness: Vec<CatchLivenessRecord>,
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
/// barriered constant ownership. Those dependencies are named as handles so
/// sibling modules can later define the actual ownership and rooting API.
#[derive(Clone, Debug, Default, Eq, PartialEq)]
pub struct LinkContext {
    pub vm: Option<VmHandle>,
    pub owner_executable: Option<ExecutableHandle>,
    pub global_object: Option<GlobalObjectHandle>,
    pub scope: Option<ScopeHandle>,
    pub specialization: CodeSpecialization,
}

#[derive(Clone, Copy, Debug, Eq, PartialEq, Hash)]
#[repr(transparent)]
pub struct VmHandle(pub u64);

#[derive(Clone, Copy, Debug, Eq, PartialEq, Hash)]
#[repr(transparent)]
pub struct ExecutableHandle(pub u64);

#[derive(Clone, Copy, Debug, Eq, PartialEq, Hash)]
#[repr(transparent)]
pub struct GlobalObjectHandle(pub u64);

#[derive(Clone, Copy, Debug, Eq, PartialEq, Hash)]
#[repr(transparent)]
pub struct ScopeHandle(pub u64);

#[derive(Clone, Copy, Debug, Default, Eq, PartialEq, Hash)]
pub enum CodeSpecialization {
    #[default]
    None,
    Call,
    Construct,
}

#[derive(Clone, Copy, Debug, Default, Eq, PartialEq, Hash)]
pub struct CodeBlockEntrypoints {
    pub interpreter: Option<InterpreterEntrySlot>,
    pub baseline_jit: Option<JitCodeSlot>,
    pub optimizing_jit: Option<JitCodeSlot>,
}

#[derive(Clone, Copy, Debug, Eq, PartialEq, Hash)]
#[repr(transparent)]
pub struct InterpreterEntrySlot(pub u32);

#[derive(Clone, Copy, Debug, Eq, PartialEq, Hash)]
#[repr(transparent)]
pub struct JitCodeSlot(pub u32);

#[derive(Clone, Debug, Default, Eq, PartialEq)]
pub struct CodeBlockTierState {
    pub llint_counter: TierCounterState,
    pub baseline_counter: TierCounterState,
    pub optimizing_counter: TierCounterState,
    pub profiling_counters: ProfilingCounterSet,
    pub current_tier: ExecutionTier,
    pub replacement: Option<ReplacementCodeBlockRef>,
    pub osr_exit_count: u32,
    pub did_fail_jit: bool,
    pub did_fail_ftl: bool,
}

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

#[derive(Clone, Copy, Debug, Eq, PartialEq, Hash)]
#[repr(transparent)]
pub struct ReplacementCodeBlockRef(pub u32);

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
