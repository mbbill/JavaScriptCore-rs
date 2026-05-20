//! Internal tools and diagnostic contracts.
//!
//! JavaScriptCore's `tools/` directory contains verifier, profiling, testing,
//! and VM-inspection utilities. These are not runtime semantics, but they are
//! important for a staged rewrite because they define how correctness,
//! debugging, and instrumentation will observe the engine.

use crate::api::{ApiGcDiagnosticSummary, ApiTierDiagnosticSummary};
use crate::bytecode::BytecodeIndex;
use crate::gc::{CellId, HeapId, HeapSnapshotId};
use crate::jit::CompilationPlanId;
use crate::profiler::ProfilerRunId;
use crate::runtime::{CodeBlockId, ObjectId, StackFrameId};
use crate::strings::Identifier;

#[derive(Clone, Copy, Debug, Default, Eq, Hash, PartialEq)]
pub struct ToolInvocationId(pub u64);

#[derive(Clone, Copy, Debug, Eq, PartialEq)]
pub enum ToolKind {
    IntegrityVerifier,
    HeapVerifier,
    SourceProfiler,
    VmInspector,
    DollarVmTestingHook,
    FunctionAllowlist,
    FunctionOverrides,
    CompilerTiming,
    LlvmProfiling,
}

/// Owner of immutable tool invocation schemas.
///
/// Shell and diagnostic entry points decide whether a tool may run. This owner
/// only records static metadata about a tool surface.
#[derive(Clone, Copy, Debug, Default, Eq, PartialEq, Hash)]
pub enum ToolSchemaOwner {
    #[default]
    DiagnosticTools,
    ShellHost,
    GeneratedToolMetadata,
    TestFixture,
}

/// Authority allowed to replace tool schema registries.
#[derive(Clone, Copy, Debug, Default, Eq, PartialEq, Hash)]
pub enum ToolRegistryMutationAuthority {
    #[default]
    CrateInitialization,
    GeneratedDataRefresh,
    ShellBootstrap,
}

/// Provenance for tool descriptor metadata.
#[derive(Clone, Copy, Debug, Eq, PartialEq, Hash)]
pub struct ToolSchemaProvenance {
    pub generator: &'static str,
    pub source: &'static str,
    pub revision: u64,
}

impl ToolSchemaProvenance {
    pub const fn new(generator: &'static str, source: &'static str, revision: u64) -> Self {
        Self {
            generator,
            source,
            revision,
        }
    }
}

/// Static argument family for a tool invocation.
#[derive(Clone, Copy, Debug, Eq, Hash, PartialEq)]
pub enum ToolArgumentKind {
    Boolean,
    Integer,
    Identifier,
    FilePath,
    Heap,
    Object,
    Cell,
    CodeBlock,
    ProfilerRun,
}

/// Immutable metadata for one tool argument.
#[derive(Clone, Copy, Debug, Eq, PartialEq)]
pub struct ToolArgumentSchema {
    pub name: &'static str,
    pub kind: ToolArgumentKind,
    pub required: bool,
}

impl ToolArgumentSchema {
    pub fn validate(self) -> Result<(), ToolValidationError> {
        if self.name.is_empty() {
            Err(ToolValidationError::EmptyArgumentName)
        } else {
            Ok(())
        }
    }
}

/// Immutable descriptor for one diagnostic tool invocation surface.
#[derive(Clone, Copy, Debug, Eq, PartialEq)]
pub struct ToolInvocationSchema {
    pub kind: ToolKind,
    pub command_name: &'static str,
    pub arguments: &'static [ToolArgumentSchema],
    pub requires_mutable_vm: bool,
    pub observes_gc: bool,
    pub may_force_compilation: bool,
    pub mutation_authority: ToolMutationAuthority,
    pub owner: ToolSchemaOwner,
    pub registry_authority: ToolRegistryMutationAuthority,
    pub provenance: ToolSchemaProvenance,
}

impl ToolInvocationSchema {
    pub const fn arguments(self) -> &'static [ToolArgumentSchema] {
        self.arguments
    }

    pub fn argument_named(self, name: &str) -> Option<&'static ToolArgumentSchema> {
        self.arguments
            .iter()
            .find(|descriptor| descriptor.name == name)
    }

    pub fn validate(self) -> Result<(), ToolValidationError> {
        if self.command_name.is_empty() {
            return Err(ToolValidationError::EmptyCommandName);
        }
        validate_unique_arguments(self.arguments)?;
        for argument in self.arguments {
            argument.validate()?;
        }
        if self.requires_mutable_vm && self.mutation_authority == ToolMutationAuthority::ObserveOnly
        {
            return Err(ToolValidationError::MutableToolLacksAuthority);
        }
        if self.provenance.generator.is_empty() || self.provenance.source.is_empty() {
            return Err(ToolValidationError::EmptyProvenanceField);
        }
        Ok(())
    }
}

/// Structural tool invocation validation failure.
#[derive(Clone, Debug, Eq, PartialEq)]
pub enum ToolValidationError {
    EmptyCommandName,
    EmptyArgumentName,
    EmptyProvenanceField,
    DuplicateToolKind(ToolKind),
    DuplicateCommandName(&'static str),
    DuplicateArgumentName(&'static str),
    MutableToolLacksAuthority,
    InvocationSchemaMismatch,
    IntegrityPlanMissingTarget,
    IntegrityPlanConflictingTargets,
    HeapVerifierMissingHeapOrSnapshot,
    SourceProfilerMissingSubject,
    FunctionOverrideMissingName,
    FunctionOverrideConflictingModes,
    DollarVmHookDisabledWithExposedFunctions,
    VmInspectorMissingSubject,
    ToolCommandNotFound(String),
    MissingRequiredArgument(&'static str),
    ArgumentKindMismatch {
        name: String,
        expected: ToolArgumentKind,
        actual: ToolArgumentKind,
    },
    UnknownArgument(String),
    MutableInvocationNotAllowed(ToolKind),
    CompilationInvocationNotAllowed(ToolKind),
    SemanticOutcomeRequiresPlannedInvocation,
    ExecutionObservationSchemaMismatch,
    ExecutionObservationMissingSubject,
}

/// Integrity audit level mirrored from `Integrity::AuditLevel`.
#[derive(Clone, Copy, Debug, Eq, PartialEq)]
pub enum IntegrityAuditLevel {
    None,
    Minimal,
    Full,
    Random,
}

/// Entity selected for an integrity audit.
///
/// Raw cell identity is always `gc::CellId`; object and code-block entries are
/// typed runtime wrappers that borrow the same heap-cell lifetime.
#[derive(Clone, Copy, Debug, Eq, PartialEq)]
pub enum IntegrityAuditTarget {
    Vm,
    Cell(CellId),
    Object(ObjectId),
    ApiValue,
    ApiObject,
    Context,
}

/// Heap verifier phase.
#[derive(Clone, Copy, Debug, Eq, PartialEq)]
pub enum HeapVerifierPhase {
    BeforeGc,
    BeforeMarking,
    AfterMarking,
    AfterGc,
}

/// VMInspector query family.
#[derive(Clone, Copy, Debug, Eq, PartialEq)]
pub enum VmInspectorQueryKind {
    VmForCallFrame,
    ExecutableMemoryValidity,
    CodeBlockForMachinePc,
    CurrentThreadOwnsJsLock,
    HeapMembership,
    CellValidity,
    CodeBlockValidity,
    FrameCodeBlock,
    DumpCallFrame,
    DumpRegisters,
    DumpStack,
    DumpValue,
    DumpCellMemory,
    DumpSubspaceHashes,
}

/// Diagnostic mutation authority.
///
/// Most tools are observers. VMInspector GC hooks, function overrides, and
/// `$vm` testing hooks are explicit mutation points and must remain gated by
/// shell options or diagnostic-only entry paths.
#[derive(Clone, Copy, Debug, Eq, PartialEq)]
pub enum ToolMutationAuthority {
    ObserveOnly,
    MayForceGc,
    MayPatchFunctionSource,
    MayInstallTestingHook,
    MayForceCompilation,
}

/// Registry of immutable diagnostic tool schemas.
#[derive(Clone, Copy, Debug, Default, Eq, PartialEq)]
pub struct ToolSchemaRegistry {
    pub tools: &'static [ToolInvocationSchema],
}

impl ToolSchemaRegistry {
    pub const fn new(tools: &'static [ToolInvocationSchema]) -> Self {
        Self { tools }
    }

    pub const fn tools(self) -> &'static [ToolInvocationSchema] {
        self.tools
    }

    pub fn tool(self, kind: ToolKind) -> Option<&'static ToolInvocationSchema> {
        self.tools.iter().find(|descriptor| descriptor.kind == kind)
    }

    pub fn command(self, command_name: &str) -> Option<&'static ToolInvocationSchema> {
        self.tools
            .iter()
            .find(|descriptor| descriptor.command_name == command_name)
    }

    pub fn plan_invocation(
        self,
        request: ToolInvocationRequest<'_>,
    ) -> Result<ToolInvocationPlan, ToolValidationError> {
        self.validate()?;
        let Some(schema) = self.command(request.command_name) else {
            return Err(ToolValidationError::ToolCommandNotFound(
                request.command_name.to_string(),
            ));
        };
        if schema.requires_mutable_vm && !request.allow_mutable_vm {
            return Err(ToolValidationError::MutableInvocationNotAllowed(
                schema.kind,
            ));
        }
        if schema.may_force_compilation && !request.allow_compilation {
            return Err(ToolValidationError::CompilationInvocationNotAllowed(
                schema.kind,
            ));
        }

        let mut provided_required_arguments = 0;
        let mut provided_optional_arguments = 0;
        for argument in schema.arguments {
            let provided = request
                .arguments
                .iter()
                .find(|provided| provided.name == argument.name);
            if argument.required && provided.is_none() {
                return Err(ToolValidationError::MissingRequiredArgument(argument.name));
            }
            if let Some(provided) = provided {
                if provided.kind != argument.kind {
                    return Err(ToolValidationError::ArgumentKindMismatch {
                        name: provided.name.to_string(),
                        expected: argument.kind,
                        actual: provided.kind,
                    });
                }
                if argument.required {
                    provided_required_arguments += 1;
                } else {
                    provided_optional_arguments += 1;
                }
            }
        }

        for provided in request.arguments {
            if schema.argument_named(provided.name).is_none() {
                return Err(ToolValidationError::UnknownArgument(
                    provided.name.to_string(),
                ));
            }
        }

        let invocation = ToolInvocation::from_schema(request.id, *schema);
        invocation.validate(self)?;
        Ok(ToolInvocationPlan {
            invocation,
            provided_required_arguments,
            provided_optional_arguments,
            observes_gc: schema.observes_gc,
            requires_mutable_vm: schema.requires_mutable_vm,
            may_force_compilation: schema.may_force_compilation,
        })
    }

    pub fn validate(self) -> Result<(), ToolValidationError> {
        for (index, tool) in self.tools.iter().enumerate() {
            tool.validate()?;
            for other in self.tools.iter().skip(index + 1) {
                if tool.kind == other.kind {
                    return Err(ToolValidationError::DuplicateToolKind(tool.kind));
                }
                if tool.command_name == other.command_name {
                    return Err(ToolValidationError::DuplicateCommandName(tool.command_name));
                }
            }
        }
        Ok(())
    }
}

/// Static argument value shape supplied to a diagnostic tool.
#[derive(Clone, Copy, Debug, Eq, PartialEq)]
pub struct ToolArgumentInput<'a> {
    pub name: &'a str,
    pub kind: ToolArgumentKind,
}

/// Tool invocation request before any diagnostic operation runs.
#[derive(Clone, Copy, Debug, Eq, PartialEq)]
pub struct ToolInvocationRequest<'a> {
    pub id: ToolInvocationId,
    pub command_name: &'a str,
    pub arguments: &'a [ToolArgumentInput<'a>],
    pub allow_mutable_vm: bool,
    pub allow_compilation: bool,
}

/// Pure diagnostic invocation plan.
#[derive(Clone, Debug, Eq, PartialEq)]
pub struct ToolInvocationPlan {
    pub invocation: ToolInvocation,
    pub provided_required_arguments: usize,
    pub provided_optional_arguments: usize,
    pub observes_gc: bool,
    pub requires_mutable_vm: bool,
    pub may_force_compilation: bool,
}

impl ToolInvocationPlan {
    pub fn semantic_outcome(self) -> Result<ToolSemanticOutcome, ToolValidationError> {
        if self.invocation.id.0 == 0 {
            return Err(ToolValidationError::SemanticOutcomeRequiresPlannedInvocation);
        }
        Ok(ToolSemanticOutcome {
            invocation: self.invocation,
            may_observe_heap: self.observes_gc,
            may_mutate_vm: self.requires_mutable_vm,
            may_request_compilation: self.may_force_compilation,
            required_argument_count: self.provided_required_arguments,
            optional_argument_count: self.provided_optional_arguments,
        })
    }
}

/// Semantic diagnostic outcome record. It does not run the tool.
#[derive(Clone, Copy, Debug, Eq, PartialEq)]
pub struct ToolSemanticOutcome {
    pub invocation: ToolInvocation,
    pub may_observe_heap: bool,
    pub may_mutate_vm: bool,
    pub may_request_compilation: bool,
    pub required_argument_count: usize,
    pub optional_argument_count: usize,
}

/// Diagnostic execution-observation result for a planned tool invocation.
#[derive(Clone, Copy, Debug, Eq, PartialEq)]
pub enum ToolExecutionObservationKind {
    Observed,
    RejectedByPolicy,
    FailedBeforeObservation,
}

/// Tool-side observation of an execution-related subject.
#[derive(Clone, Copy, Debug, Eq, PartialEq)]
pub struct ToolExecutionObservationRecord {
    pub invocation: ToolInvocation,
    pub kind: ToolExecutionObservationKind,
    pub frame: Option<StackFrameId>,
    pub code_block: Option<CodeBlockId>,
    pub bytecode_index: Option<BytecodeIndex>,
    pub observed_register_count: usize,
    pub observed_stack_depth: usize,
}

impl ToolExecutionObservationRecord {
    pub fn validate(self, registry: ToolSchemaRegistry) -> Result<(), ToolValidationError> {
        self.invocation.validate(registry)?;
        if registry.tool(self.invocation.kind).is_none() {
            return Err(ToolValidationError::ExecutionObservationSchemaMismatch);
        }
        if self.kind == ToolExecutionObservationKind::Observed
            && self.frame.is_none()
            && self.code_block.is_none()
            && self.bytecode_index.is_none()
        {
            return Err(ToolValidationError::ExecutionObservationMissingSubject);
        }
        Ok(())
    }
}

/// Aggregated diagnostics for tools that observe execution, GC, and tier state.
#[derive(Clone, Debug, Eq, PartialEq)]
pub struct ToolDiagnosticReport {
    pub observations: Vec<ToolExecutionObservationRecord>,
    pub gc_summaries: Vec<ApiGcDiagnosticSummary>,
    pub tier_summaries: Vec<ApiTierDiagnosticSummary>,
    pub observed_subject_count: usize,
    pub fallback_visible_count: usize,
}

impl ToolDiagnosticReport {
    pub fn from_records(
        observations: Vec<ToolExecutionObservationRecord>,
        gc_summaries: Vec<ApiGcDiagnosticSummary>,
        tier_summaries: Vec<ApiTierDiagnosticSummary>,
        registry: ToolSchemaRegistry,
    ) -> Result<Self, ToolValidationError> {
        for observation in &observations {
            observation.validate(registry)?;
        }
        let observed_subject_count = observations
            .iter()
            .filter(|observation| {
                observation.frame.is_some()
                    || observation.code_block.is_some()
                    || observation.bytecode_index.is_some()
            })
            .count();
        let fallback_visible_count = tier_summaries
            .iter()
            .filter(|summary| summary.fallback_resume.is_some())
            .count();

        Ok(Self {
            observations,
            gc_summaries,
            tier_summaries,
            observed_subject_count,
            fallback_visible_count,
        })
    }
}

#[derive(Clone, Copy, Debug, Eq, PartialEq)]
pub struct ToolInvocation {
    pub id: ToolInvocationId,
    pub kind: ToolKind,
    pub requires_mutable_vm: bool,
    pub observes_gc: bool,
    pub may_force_compilation: bool,
    pub mutation_authority: ToolMutationAuthority,
}

impl ToolInvocation {
    pub const fn from_schema(id: ToolInvocationId, schema: ToolInvocationSchema) -> Self {
        Self {
            id,
            kind: schema.kind,
            requires_mutable_vm: schema.requires_mutable_vm,
            observes_gc: schema.observes_gc,
            may_force_compilation: schema.may_force_compilation,
            mutation_authority: schema.mutation_authority,
        }
    }

    pub fn validate(self, registry: ToolSchemaRegistry) -> Result<(), ToolValidationError> {
        let Some(schema) = registry.tool(self.kind) else {
            return Err(ToolValidationError::InvocationSchemaMismatch);
        };
        if self.requires_mutable_vm == schema.requires_mutable_vm
            && self.observes_gc == schema.observes_gc
            && self.may_force_compilation == schema.may_force_compilation
            && self.mutation_authority == schema.mutation_authority
        {
            Ok(())
        } else {
            Err(ToolValidationError::InvocationSchemaMismatch)
        }
    }
}

#[derive(Clone, Debug, Default, Eq, PartialEq)]
pub struct IntegrityCheckPlan {
    pub heap: Option<HeapId>,
    pub object: Option<ObjectId>,
    pub cell: Option<CellId>,
    pub target: Option<IntegrityAuditTarget>,
    pub audit_level: Option<IntegrityAuditLevel>,
    pub verify_structures: bool,
    pub verify_watchpoints: bool,
    pub verify_write_barriers: bool,
}

impl IntegrityCheckPlan {
    pub fn validate(&self) -> Result<(), ToolValidationError> {
        if self.target.is_none()
            && self.heap.is_none()
            && self.object.is_none()
            && self.cell.is_none()
        {
            return Err(ToolValidationError::IntegrityPlanMissingTarget);
        }
        if self.object.is_some() && self.cell.is_some() {
            return Err(ToolValidationError::IntegrityPlanConflictingTargets);
        }
        Ok(())
    }
}

#[derive(Clone, Debug, Default, Eq, PartialEq)]
pub struct HeapVerifierPlan {
    pub heap: Option<HeapId>,
    pub snapshot: Option<HeapSnapshotId>,
    pub phase: Option<HeapVerifierPhase>,
    pub include_weak_sets: bool,
    pub include_finalizers: bool,
    pub include_mark_bits: bool,
    pub recorded_cycle_count: usize,
}

impl HeapVerifierPlan {
    pub fn validate(&self) -> Result<(), ToolValidationError> {
        if self.heap.is_none() && self.snapshot.is_none() {
            Err(ToolValidationError::HeapVerifierMissingHeapOrSnapshot)
        } else {
            Ok(())
        }
    }
}

#[derive(Clone, Debug, Default, Eq, PartialEq)]
pub struct SourceProfilerPlan {
    pub run: Option<ProfilerRunId>,
    pub code_block: Option<CodeBlockId>,
    pub include_bytecode_counters: bool,
    pub include_jit_tiers: bool,
}

impl SourceProfilerPlan {
    pub fn validate(&self) -> Result<(), ToolValidationError> {
        if self.run.is_none() && self.code_block.is_none() {
            Err(ToolValidationError::SourceProfilerMissingSubject)
        } else {
            Ok(())
        }
    }
}

#[derive(Clone, Debug, Default, Eq, PartialEq)]
pub struct FunctionOverridePlan {
    pub name: Option<Identifier>,
    pub allowlist_enabled: bool,
    pub override_enabled: bool,
    pub affected_compilation_plan: Option<CompilationPlanId>,
    pub override_file_loaded: bool,
    pub reinstall_required: bool,
}

impl FunctionOverridePlan {
    pub fn validate(&self) -> Result<(), ToolValidationError> {
        if self.name.is_none() {
            return Err(ToolValidationError::FunctionOverrideMissingName);
        }
        if self.allowlist_enabled && self.override_enabled {
            return Err(ToolValidationError::FunctionOverrideConflictingModes);
        }
        Ok(())
    }
}

/// `$vm` exposure contract for shell-only testing hooks.
#[derive(Clone, Debug, Eq, PartialEq)]
pub struct DollarVmTestingHookPlan {
    pub use_dollar_vm_enabled: bool,
    pub exposed_function_count: usize,
    pub exposes_property_enumeration: bool,
    pub guarded_structure: Option<ObjectId>,
}

impl DollarVmTestingHookPlan {
    pub fn validate(&self) -> Result<(), ToolValidationError> {
        if !self.use_dollar_vm_enabled && self.exposed_function_count != 0 {
            Err(ToolValidationError::DollarVmHookDisabledWithExposedFunctions)
        } else {
            Ok(())
        }
    }
}

/// VMInspector request descriptor.
#[derive(Clone, Debug, Eq, PartialEq)]
pub struct VmInspectorRequest {
    pub kind: VmInspectorQueryKind,
    pub heap: Option<HeapId>,
    pub object: Option<ObjectId>,
    pub cell: Option<CellId>,
    pub code_block: Option<CodeBlockId>,
    pub can_time_out: bool,
}

impl VmInspectorRequest {
    pub fn validate(&self) -> Result<(), ToolValidationError> {
        if self.heap.is_none()
            && self.object.is_none()
            && self.cell.is_none()
            && self.code_block.is_none()
        {
            Err(ToolValidationError::VmInspectorMissingSubject)
        } else {
            Ok(())
        }
    }
}

const TOOL_SCHEMA_PROVENANCE: ToolSchemaProvenance = ToolSchemaProvenance {
    generator: "hand-authored",
    source: "Source/JavaScriptCore/rust/src/tools/mod.rs",
    revision: 1,
};

const TOOL_HEAP_ARGUMENTS: &[ToolArgumentSchema] = &[ToolArgumentSchema {
    name: "heap",
    kind: ToolArgumentKind::Heap,
    required: false,
}];

const TOOL_CODE_BLOCK_ARGUMENTS: &[ToolArgumentSchema] = &[ToolArgumentSchema {
    name: "code-block",
    kind: ToolArgumentKind::CodeBlock,
    required: false,
}];

pub const TOOL_INVOCATION_SCHEMAS: &[ToolInvocationSchema] = &[
    ToolInvocationSchema {
        kind: ToolKind::IntegrityVerifier,
        command_name: "integrity-verifier",
        arguments: TOOL_HEAP_ARGUMENTS,
        requires_mutable_vm: false,
        observes_gc: true,
        may_force_compilation: false,
        mutation_authority: ToolMutationAuthority::ObserveOnly,
        owner: ToolSchemaOwner::DiagnosticTools,
        registry_authority: ToolRegistryMutationAuthority::CrateInitialization,
        provenance: TOOL_SCHEMA_PROVENANCE,
    },
    ToolInvocationSchema {
        kind: ToolKind::HeapVerifier,
        command_name: "heap-verifier",
        arguments: TOOL_HEAP_ARGUMENTS,
        requires_mutable_vm: false,
        observes_gc: true,
        may_force_compilation: false,
        mutation_authority: ToolMutationAuthority::ObserveOnly,
        owner: ToolSchemaOwner::DiagnosticTools,
        registry_authority: ToolRegistryMutationAuthority::CrateInitialization,
        provenance: TOOL_SCHEMA_PROVENANCE,
    },
    ToolInvocationSchema {
        kind: ToolKind::SourceProfiler,
        command_name: "source-profiler",
        arguments: TOOL_CODE_BLOCK_ARGUMENTS,
        requires_mutable_vm: false,
        observes_gc: false,
        may_force_compilation: false,
        mutation_authority: ToolMutationAuthority::ObserveOnly,
        owner: ToolSchemaOwner::DiagnosticTools,
        registry_authority: ToolRegistryMutationAuthority::CrateInitialization,
        provenance: TOOL_SCHEMA_PROVENANCE,
    },
    ToolInvocationSchema {
        kind: ToolKind::VmInspector,
        command_name: "vm-inspector",
        arguments: &[],
        requires_mutable_vm: false,
        observes_gc: true,
        may_force_compilation: false,
        mutation_authority: ToolMutationAuthority::ObserveOnly,
        owner: ToolSchemaOwner::DiagnosticTools,
        registry_authority: ToolRegistryMutationAuthority::CrateInitialization,
        provenance: TOOL_SCHEMA_PROVENANCE,
    },
    ToolInvocationSchema {
        kind: ToolKind::DollarVmTestingHook,
        command_name: "dollar-vm-testing-hook",
        arguments: &[],
        requires_mutable_vm: true,
        observes_gc: true,
        may_force_compilation: true,
        mutation_authority: ToolMutationAuthority::MayInstallTestingHook,
        owner: ToolSchemaOwner::ShellHost,
        registry_authority: ToolRegistryMutationAuthority::ShellBootstrap,
        provenance: TOOL_SCHEMA_PROVENANCE,
    },
];

pub const TOOL_SCHEMA_REGISTRY: ToolSchemaRegistry = ToolSchemaRegistry {
    tools: TOOL_INVOCATION_SCHEMAS,
};

fn validate_unique_arguments(arguments: &[ToolArgumentSchema]) -> Result<(), ToolValidationError> {
    for (index, argument) in arguments.iter().enumerate() {
        for other in arguments.iter().skip(index + 1) {
            if argument.name == other.name {
                return Err(ToolValidationError::DuplicateArgumentName(argument.name));
            }
        }
    }
    Ok(())
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::gc::CellId;

    const REQUIRED_OBJECT_ARGUMENTS: &[ToolArgumentSchema] = &[ToolArgumentSchema {
        name: "object",
        kind: ToolArgumentKind::Object,
        required: true,
    }];
    const REQUIRED_TOOL: ToolInvocationSchema = ToolInvocationSchema {
        kind: ToolKind::VmInspector,
        command_name: "required-object-inspector",
        arguments: REQUIRED_OBJECT_ARGUMENTS,
        requires_mutable_vm: false,
        observes_gc: true,
        may_force_compilation: false,
        mutation_authority: ToolMutationAuthority::ObserveOnly,
        owner: ToolSchemaOwner::TestFixture,
        registry_authority: ToolRegistryMutationAuthority::CrateInitialization,
        provenance: ToolSchemaProvenance::new("test", "tools/mod.rs", 1),
    };
    const REQUIRED_TOOLS: &[ToolInvocationSchema] = &[REQUIRED_TOOL];
    const REQUIRED_REGISTRY: ToolSchemaRegistry = ToolSchemaRegistry::new(REQUIRED_TOOLS);

    #[test]
    fn validates_builtin_tool_registry() {
        assert_eq!(TOOL_SCHEMA_REGISTRY.validate(), Ok(()));
    }

    #[test]
    fn builds_invocation_from_schema() {
        let schema = TOOL_SCHEMA_REGISTRY
            .tool(ToolKind::HeapVerifier)
            .expect("heap verifier schema");
        let invocation = ToolInvocation::from_schema(ToolInvocationId(1), *schema);

        assert_eq!(invocation.validate(TOOL_SCHEMA_REGISTRY), Ok(()));
    }

    #[test]
    fn rejects_integrity_plan_without_target() {
        assert_eq!(
            IntegrityCheckPlan::default().validate(),
            Err(ToolValidationError::IntegrityPlanMissingTarget)
        );
    }

    #[test]
    fn plans_tool_invocation_from_command_schema() {
        let args = [ToolArgumentInput {
            name: "heap",
            kind: ToolArgumentKind::Heap,
        }];
        let plan = TOOL_SCHEMA_REGISTRY
            .plan_invocation(ToolInvocationRequest {
                id: ToolInvocationId(11),
                command_name: "heap-verifier",
                arguments: &args,
                allow_mutable_vm: false,
                allow_compilation: false,
            })
            .expect("plan");

        assert_eq!(plan.invocation.kind, ToolKind::HeapVerifier);
        assert_eq!(plan.provided_optional_arguments, 1);
        assert!(plan.observes_gc);
    }

    #[test]
    fn rejects_missing_required_tool_argument() {
        assert_eq!(
            REQUIRED_REGISTRY.plan_invocation(ToolInvocationRequest {
                id: ToolInvocationId(12),
                command_name: "required-object-inspector",
                arguments: &[],
                allow_mutable_vm: false,
                allow_compilation: false,
            }),
            Err(ToolValidationError::MissingRequiredArgument("object"))
        );
    }

    #[test]
    fn rejects_mutating_tool_without_permission() {
        assert_eq!(
            TOOL_SCHEMA_REGISTRY.plan_invocation(ToolInvocationRequest {
                id: ToolInvocationId(13),
                command_name: "dollar-vm-testing-hook",
                arguments: &[],
                allow_mutable_vm: false,
                allow_compilation: true,
            }),
            Err(ToolValidationError::MutableInvocationNotAllowed(
                ToolKind::DollarVmTestingHook
            ))
        );
    }

    #[test]
    fn tool_semantics_preserve_non_executing_capabilities() {
        let args = [ToolArgumentInput {
            name: "heap",
            kind: ToolArgumentKind::Heap,
        }];
        let outcome = TOOL_SCHEMA_REGISTRY
            .plan_invocation(ToolInvocationRequest {
                id: ToolInvocationId(14),
                command_name: "heap-verifier",
                arguments: &args,
                allow_mutable_vm: false,
                allow_compilation: false,
            })
            .expect("plan")
            .semantic_outcome()
            .expect("semantic outcome");

        assert_eq!(outcome.invocation.kind, ToolKind::HeapVerifier);
        assert!(outcome.may_observe_heap);
        assert!(!outcome.may_mutate_vm);
        assert_eq!(outcome.optional_argument_count, 1);
    }

    #[test]
    fn validates_tool_execution_observation_record() {
        let schema = TOOL_SCHEMA_REGISTRY
            .tool(ToolKind::SourceProfiler)
            .expect("source profiler schema");
        let invocation = ToolInvocation::from_schema(ToolInvocationId(15), *schema);
        let observation = ToolExecutionObservationRecord {
            invocation,
            kind: ToolExecutionObservationKind::Observed,
            frame: Some(StackFrameId(1)),
            code_block: Some(CodeBlockId(CellId(2))),
            bytecode_index: Some(BytecodeIndex::from_offset(4)),
            observed_register_count: 3,
            observed_stack_depth: 1,
        };

        assert_eq!(observation.validate(TOOL_SCHEMA_REGISTRY), Ok(()));
    }

    #[test]
    fn rejects_observed_tool_execution_without_subject() {
        let schema = TOOL_SCHEMA_REGISTRY
            .tool(ToolKind::VmInspector)
            .expect("vm inspector schema");
        let invocation = ToolInvocation::from_schema(ToolInvocationId(16), *schema);
        let observation = ToolExecutionObservationRecord {
            invocation,
            kind: ToolExecutionObservationKind::Observed,
            frame: None,
            code_block: None,
            bytecode_index: None,
            observed_register_count: 0,
            observed_stack_depth: 0,
        };

        assert_eq!(
            observation.validate(TOOL_SCHEMA_REGISTRY),
            Err(ToolValidationError::ExecutionObservationMissingSubject)
        );
    }

    #[test]
    fn diagnostic_report_exposes_tool_execution_gc_and_tier_visibility() {
        let schema = TOOL_SCHEMA_REGISTRY
            .tool(ToolKind::VmInspector)
            .expect("vm inspector schema");
        let invocation = ToolInvocation::from_schema(ToolInvocationId(17), *schema);
        let observation = ToolExecutionObservationRecord {
            invocation,
            kind: ToolExecutionObservationKind::Observed,
            frame: None,
            code_block: Some(CodeBlockId(CellId(4))),
            bytecode_index: Some(BytecodeIndex::from_offset(2)),
            observed_register_count: 0,
            observed_stack_depth: 1,
        };
        let gc = ApiGcDiagnosticSummary::from_record(crate::api::ApiGcEventResultRecord {
            kind: crate::api::ApiGcEventResultKind::Completed,
            heap: Some(HeapId(1)),
            collection: None,
            phase: None,
            snapshot: None,
            protected_value_count: 0,
            forced_by_api: false,
        });
        let tier = ApiTierDiagnosticSummary::from_fallback(crate::jit::TierFallbackResultRecord {
            owner: CodeBlockId(CellId(4)),
            from_tier: crate::jit::JitType::Baseline,
            attempted_tier: crate::jit::JitType::Dfg,
            reason: crate::jit::TierFallbackReason::UnsupportedTier,
            target: crate::jit::TierFallbackTarget::ReturnToInterpreter,
            bytecode_index: Some(BytecodeIndex::from_offset(2)),
            resume: crate::jit::TierFallbackResumeKind::ContinueInInterpreter,
            preserves_profile: true,
            should_count_invalidation: true,
            clears_active_request: true,
        });

        let report = ToolDiagnosticReport::from_records(
            vec![observation],
            vec![gc],
            vec![tier],
            TOOL_SCHEMA_REGISTRY,
        )
        .expect("tool diagnostics");

        assert_eq!(report.observed_subject_count, 1);
        assert_eq!(report.gc_summaries.len(), 1);
        assert_eq!(report.fallback_visible_count, 1);
    }
}
