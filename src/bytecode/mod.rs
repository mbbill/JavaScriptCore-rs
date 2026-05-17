//! Bytecode, code-block, and executable contracts for the Rust JavaScriptCore
//! rewrite.
//!
//! This module intentionally does not emit, pack, decode, link, or execute real
//! JavaScript bytecode. It names ownership and mutation boundaries that later
//! generated schema and runtime work must respect.

pub(crate) mod code_block;
pub(crate) mod debug;
pub(crate) mod executable;
pub(crate) mod generator;
pub(crate) mod ic;
pub(crate) mod instruction;
pub(crate) mod metadata;
pub(crate) mod opcode;
pub(crate) mod origin;
pub(crate) mod profiling;
pub(crate) mod register;

pub use crate::gc::StructureId;
pub use code_block::{
    BitVectorRef, BytecodeIndex, BytecodeRange, CallSiteIndex, CallSiteRecord, Checkpoint,
    CodeBlock, CodeBlockEntrypoints, CodeBlockRegisterSummary, CodeBlockTierState, CodeFeatures,
    CodeGenerationModeSet, CodeKind, CodeSpecialization, ConstantOwner, ConstantValue,
    ConstructorKind, DebugHookKind, DebugHookRecord, EvalContext, ExecutableInfo, HandlerInfo,
    HandlerKind, HandlerRange, HandlerTarget, JumpTableSet, LinkContext, LinkTimeConstant,
    LinkedConstant, LinkedConstantPool, LinkedMetadataField, LinkedMetadataStorage,
    LinkedSideTables, MetadataEntry, MetadataTable, ParseMode, ScriptMode,
    SourceCodeRepresentation, SourceDirectiveTable, SourceNote, SourceNoteTable, SourceOriginId,
    SourcePosition, SourceProvenance, SourceProviderId, SourceRange, SuperBinding, TierHint,
    UnlinkedCodeBlock, UnlinkedConstant, UnlinkedConstantPool, UnlinkedHandlerInfo,
    UnlinkedMetadataEntry, UnlinkedMetadataTable, UnlinkedSideTables, UnlinkedTieringHints,
};
pub use debug::{
    BytecodeHookTable, CatchProfileRecord, DebuggerBytecodeHook, DebuggerPausePolicy,
    ExceptionHandlerRecord, ExceptionHandlerTable, ExceptionRange, ExceptionTarget,
    HandlerSearchOrder, ProfilerBytecodeHook, ProfilerHookKind, ShadowChickenHook,
    ShadowChickenHookKind,
};
pub use executable::{
    CachedCodeValue, CodeCache, CodeCacheEntry, CodeCachePolicy, ConstructAbility,
    DeferredTieringSlots, EntrypointState, ExecutableBase, ExecutableEntrypoints, ExecutablePolicy,
    FunctionExecutable, FunctionExecutableRareData, FunctionMode, ImplementationVisibility,
    InlineAttribute, IntrinsicKind, ParseRecord, ScriptExecutable, ScriptExecutableKind,
    SourceCodeKey, UnlinkedFunctionExecutable,
};
pub use generator::{
    BytecodeGenerator, EnvironmentSlot, EnvironmentSlotKind, GenerationDiagnostic,
    GenerationEnvironment, GenerationOutput, GenerationPhase, GenerationPlan, GenerationRoot,
    Label, LabelArena, RegisterAllocator,
};
pub use ic::{
    AccessCaseRef, ArityCheckMode, CallLinkFlags, CallLinkInfo, CallLinkMode, CallSlot, CallTarget,
    CallType, CodeBlockSlot, GetByIdMode, GetByIdModeMetadata, InlineCacheState, InlineCacheTable,
    IterationModeMetadata, IterationModes, PropertyCacheKey, PropertyCacheKind,
    PropertyInlineCache, PropertyOffset, PutByIdMode, PutByIdModeMetadata, StructureStubInfo,
    StructureStubKind,
};
pub use instruction::{
    BytecodeByteOrder, BytecodeVerifier, CheckpointSpec, InstructionBuilder,
    InstructionDeclaration, InstructionDeclarationRef, InstructionSchemaRef,
    InstructionStreamLayout, LabelBinding, LabelDeclaration, LabelRef, OpcodeIdWidth, Operand,
    PackedByteStorage, PackedInstructionStream, RuntimeTypeRef, TypedInstruction,
    VerificationFinding, VerificationReport, VerificationResult, WidthPrefixPolicy,
};
pub use metadata::{
    InstructionMetadataFieldPlan, InstructionMetadataPlan, MetadataAlignment, MetadataBinding,
    MetadataLayout, MetadataLinkingData, MetadataOffsetEncoding, MetadataOffsetEntry,
    MetadataOffsetTable, MetadataTableMemoryLayout, MetadataTablePhase, MetadataTriState,
    MetadataValueProfileRegion, OpcodeMetadataLayout, UnlinkedMetadataTableRef,
    ValueProfileStorageOrder,
};
pub use opcode::{
    CheckpointDescriptor, CheckpointShape, MetadataFieldKind, MetadataFieldSpec,
    MetadataMutability, MetadataShape, Opcode, OpcodeCategory, OpcodeDescriptor, OpcodeEffects,
    OpcodeId, OpcodeSchema, OpcodeSchemaVersion, OperandKind, OperandPresence, OperandRole,
    OperandSpec, OperandWidth, TemporaryKind, TemporarySpec,
};
pub use origin::{
    BytecodeSourceMapping, CodeBlockRef, CodeOrigin, CodeOriginTable, FullCodeOrigin,
    InlineCallFrameRecord, InlineCallFrameRef, ProgramCounterMappingWidth, ProgramCounterOrigin,
    SourceNoteLookup, SourcePositionKind,
};
pub use profiling::{
    ArithProfile, ArrayModes, ArrayProfile, ArrayProfileFlags, BytecodeExecutionCounter,
    ControlFlowProfileRecord, CountingVariant, ExecutionCounterState, LoopOsrCounter,
    ObservedResults, ObservedType, ProfileUpdatePolicy, ProfilingCounterSet, SpeculatedTypeSet,
    TypeProfilerRecord, UnlinkedValueProfile, ValueProfile, ValueProfileBucket,
    ValueProfileBucketKind, ValueProfileTable,
};
pub use register::{
    RegisterClass, RegisterFrameShape, RegisterOperandEncoding, RegisterOperandWidth,
    SpecialRegisters, TemporaryLifetime, TemporaryRegister, ThisArgumentOffset, VirtualRegister,
    FIRST_CONSTANT_REGISTER_INDEX, INVALID_VIRTUAL_REGISTER,
};
