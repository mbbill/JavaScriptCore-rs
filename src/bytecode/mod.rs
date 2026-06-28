//! Bytecode, code-block, and executable contracts for the Rust JavaScriptCore
//! rewrite.
//!
//! This module owns bytecode records, code blocks, executable metadata, and
//! safe decoded views. Generated JSC bytecode tables are still pending, but the
//! Rust interpreter can execute a small core opcode set emitted through the
//! bytecompiler while those generated tables are filled in.

pub(crate) mod code_block;
pub(crate) mod debug;
pub(crate) mod executable;
pub(crate) mod gc;
pub(crate) mod generator;
pub(crate) mod ic;
pub(crate) mod instruction;
pub(crate) mod instruction_stream;
pub(crate) mod integration;
pub(crate) mod metadata;
pub(crate) mod opcode;
pub(crate) mod origin;
pub(crate) mod profiling;
pub(crate) mod register;
pub(crate) mod speculated_type;

pub use crate::gc::StructureId;
pub use code_block::{
    BitVectorRef, BytecodeIndex, BytecodeRange, CallSiteIndex, CallSiteRecord, Checkpoint,
    CodeBlock, CodeBlockEntrypoints, CodeBlockExecutionSurface, CodeBlockLifecycleState,
    CodeBlockMutationAuthority, CodeBlockRegisterSummary, CodeBlockTierState, CodeFeatures,
    CodeGenerationModeSet, CodeKind, CodeSpecialization, ConstantOwner, ConstantValue,
    ConstructorKind, DebugHookKind, DebugHookRecord, DerivedContextType, EvalContext,
    EvalContextType, ExecutableInfo, ExecutionTier, HandlerInfo, HandlerKind, HandlerRange,
    HandlerTarget, InterpreterEntrySlot, JitCodeSlot, JumpTableSet, LinkContext, LinkTimeConstant,
    LinkedConstant, LinkedConstantPool, LinkedMetadataField, LinkedMetadataStorage,
    LinkedSideTables, MetadataEntry, MetadataTable, NeedsClassFieldInitializer, ParseMode,
    PrivateBrandRequirement, RuntimeSlot, ScriptMode, SourceCodeRepresentation,
    SourceDirectiveTable, SourceNote, SourceNoteTable, SourceOriginId, SourcePosition,
    SourceProvenance, SourceProviderId, SourceRange, SuperBinding, TierCounterState, TierHint,
    UnlinkedCodeBlock, UnlinkedCodeBlockPhase, UnlinkedConstant, UnlinkedConstantPool,
    UnlinkedHandlerInfo, UnlinkedMetadataEntry, UnlinkedMetadataTable, UnlinkedSideTables,
    UnlinkedTieringHints, UnlinkedToLinkedBytecodeRecord,
};
pub use debug::{
    BytecodeHookTable, CatchProfileRecord, DebuggerBytecodeHook, DebuggerPausePolicy,
    ExceptionHandlerRecord, ExceptionHandlerTable, ExceptionRange, ExceptionTarget,
    HandlerSearchOrder, ProfilerBytecodeHook, ProfilerHookKind, ShadowChickenHook,
    ShadowChickenHookKind,
};
pub use executable::{
    CachedCodeValue, ClassElementDefinition, ClassElementDefinitionKind, CodeCache, CodeCacheEntry,
    CodeCachePolicy, ConstructAbility, DeferredTieringSlots, EntrypointState,
    ExecutableAritySelection, ExecutableBase, ExecutableBaselineNativeEntryRecord,
    ExecutableBaselineNativeEntrySelection, ExecutableEntryCacheKey, ExecutableEntryCacheRecord,
    ExecutableEntryMetadata, ExecutableEntryPublicationError, ExecutableEntryPublicationRequest,
    ExecutableEntrypoints, ExecutableEvalParseMetadata, ExecutableFunctionParseMetadata,
    ExecutableInfoPlan, ExecutableModuleParseMetadata, ExecutableParseGoal,
    ExecutableParseSemanticMetadata, ExecutablePolicy, FunctionExecutable,
    FunctionExecutableRareData, FunctionMode, FunctionSingletonState, ImplementationVisibility,
    InlineAttribute, InterpreterEntrypointSlot, IntrinsicKind, JitEntrypointSlot,
    ParentPrivateNameEnvironmentRef, ParentTdzEnvironmentRef, ParseRecord, ScriptExecutable,
    ScriptExecutableKind, SourceCodeKey, UnlinkedFunctionExecutable,
    UnlinkedFunctionExecutableRareData, UnlinkedFunctionKind,
};
pub use gc::{
    BytecodeRootMap, BytecodeRootMapId, BytecodeRootMapValidationError, BytecodeRootSlotDescriptor,
    BytecodeRootSlotKind, BytecodeRootSlotStorage,
};
pub use generator::{
    BytecodeGenerator, EnvironmentSlot, EnvironmentSlotKind, EnvironmentSlotList,
    GenerationDiagnostic, GenerationEnvironment, GenerationMutationAuthority, GenerationOutput,
    GenerationPhase, GenerationPlan, GenerationRoot, GenerationValidationFinding,
    GenerationValidationReport, GeneratorStatePlan, Label, LabelArena, ParentEnvironmentRef,
    RegisterAllocator, SpecialRegisterKind, YieldPointKind, YieldPointPlan,
};
pub use ic::{
    AccessCaseRef, ArityCheckMode, BaselineJitData, CallLinkFlags, CallLinkInfo, CallLinkMode,
    CallSlot, CallTarget, CallType, GetByIdMode, GetByIdModeMetadata,
    HandlerPropertyInlineCacheRecord, InlineCacheMutationAuthority, InlineCacheState,
    InlineCacheTable, IterationModeMetadata, IterationModes, PropertyAccessType, PropertyCacheKey,
    PropertyCacheKind, PropertyInlineCache, PropertyInlineCacheDispatch, PropertyOffset,
    PutByIdMode, PutByIdModeMetadata, StructureStubAccessCaseLinkError,
    StructureStubAccessCaseLinkOutcome, StructureStubAccessCaseLinkRequest,
    StructureStubAccessCaseLinkResult, StructureStubInfo, StructureStubKind,
};
pub use instruction::{
    BytecodeByteOrder, BytecodeDeclarationOwner, BytecodeDeclarationTable, BytecodeVerifier,
    CheckpointSpec, DecodedInstruction, DecodedInstructionSource, InstructionBuilder,
    InstructionBuilderState, InstructionDeclaration, InstructionDeclarationRef,
    InstructionDecodeError, InstructionDecodeIter, InstructionLinkFinding, InstructionLinkOutput,
    InstructionLinker, InstructionPatchAuthority, InstructionSchemaRef, InstructionStreamLayout,
    LabelBinding, LabelDeclaration, LabelRef, OpcodeIdWidth, Operand, OperandAccessError,
    PackedByteStorage, PackedInstructionLifecycle, PackedInstructionStream, RuntimeTypeRef,
    StaticInstructionDeclaration, StaticLabelDeclaration, TypedInstruction, VerificationFinding,
    VerificationReport, VerificationResult, WidthPrefixPolicy,
};
pub use integration::{
    summarize_bytecode_integration, BytecodeIntegrationDiagnostic,
    BytecodeIntegrationDiagnosticKind, BytecodeIntegrationSummary, BytecodeRootIntegrationSummary,
    BytecodeTierExecutionSummary, BytecodeToolingSourceMapSummary,
};
pub use metadata::{
    BytecodeMetadataSemanticContract, ExecutionMetadataLookup, InstructionMetadataFieldPlan,
    InstructionMetadataPlan, MetadataAlignment, MetadataBinding, MetadataExceptionContract,
    MetadataLayout, MetadataLayoutOwner, MetadataLayoutProvenance, MetadataLayoutRegistry,
    MetadataLinkingData, MetadataObservableOrder, MetadataOffsetEncoding, MetadataOffsetEntry,
    MetadataOffsetTable, MetadataSideEffectSet, MetadataTableMemoryLayout, MetadataTablePhase,
    MetadataTriState, MetadataValidationFinding, MetadataValidationReport,
    MetadataValueProfileRegion, OpcodeMetadataLayout, StaticMetadataLayout,
    StaticMetadataOffsetTable, StaticOpcodeMetadataLayout, UnlinkedMetadataTableRef,
    ValueProfileStorageOrder,
};
pub use opcode::{
    CheckpointDescriptor, CheckpointShape, CoreOpcode, MetadataFieldKind, MetadataFieldSpec,
    MetadataMutability, MetadataShape, Opcode, OpcodeCategory, OpcodeDescriptor, OpcodeEffects,
    OpcodeId, OpcodeRegistryMutationAuthority, OpcodeSchema, OpcodeSchemaOwner,
    OpcodeSchemaProvenance, OpcodeSchemaRegistry, OpcodeSchemaVersion, OpcodeValidationFinding,
    OpcodeValidationReport, OperandKind, OperandPresence, OperandRole, OperandSpec, OperandWidth,
    StaticCheckpointShape, StaticMetadataShape, StaticOpcodeDescriptor, StaticOpcodeSchema,
    TemporaryKind, TemporarySpec,
};
pub use origin::{
    BytecodeSourceMapping, CodeOrigin, CodeOriginTable, ExecutionDiagnosticMapping, FullCodeOrigin,
    InlineCallFrameRecord, InlineCallFrameRef, ProgramCounterMappingWidth, ProgramCounterOrigin,
    SourceNoteLookup, SourceOriginSemanticEntry, SourceOriginSemanticKind, SourceOriginSemanticMap,
    SourceOriginSemanticValidationFinding, SourceOriginSemanticValidationReport,
    SourcePositionKind,
};
pub use profiling::{
    ArithProfile, ArrayModes, ArrayProfile, ArrayProfileFlags, BytecodeExecutionCounter,
    ControlFlowProfileRecord, CountingVariant, ExecutionCounterState, LoopOsrCounter,
    ObservedResults, ObservedType, ProfileUpdatePolicy, ProfilingCounterSet, SpeculatedTypeSet,
    TypeProfilerRecord, UnlinkedValueProfile, ValueProfile, ValueProfileBucket,
    ValueProfileBucketKind, ValueProfileBucketSample, ValueProfileEmissionCapability,
    ValueProfileEmissionPolicy, ValueProfileJitBucketBinding, ValueProfileJitStorageGeneration,
    ValueProfileJitStoreTarget, ValueProfileRootMetadata, ValueProfileRootValidationError,
    ValueProfileSampleError, ValueProfileTable,
};
pub use register::{
    RegisterClass, RegisterFrameShape, RegisterOperandEncoding, RegisterOperandWidth,
    SpecialRegisters, TemporaryLifetime, TemporaryRegister, ThisArgumentOffset, VirtualRegister,
    FIRST_CONSTANT_REGISTER_INDEX, INVALID_VIRTUAL_REGISTER,
};
