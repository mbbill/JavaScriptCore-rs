//! JavaScriptCore shell embedding contracts.
//!
//! The shell is not the engine. This module only names command-line host
//! services, testing hooks, module resolution hooks, and harness integration.

use crate::api::{
    ApiExecutionDiagnosticSummary, ApiExecutionResultKind, ApiGcDiagnosticSummary,
    ApiTierDiagnosticSummary,
};
use crate::bytecode::SourceProviderId;
use crate::gc::{CollectionKind, GcPhase, HeapId, HeapSnapshotId};
use crate::modules::{HostModulePayload, ImportMapId, ModuleKey, ModuleLoaderPolicy};
use crate::runtime::{GlobalObjectId, HostHookId};
use crate::wasm::WasmDebugTransport;

/// Shell execution mode.
#[derive(Clone, Copy, Debug, Eq, PartialEq)]
pub enum ShellMode {
    ScriptFile,
    Interactive,
    Worker,
    TestHarness,
    Module,
}

/// Startup and teardown phase around `jscmain` / `runJSC`.
#[derive(Clone, Copy, Debug, Eq, PartialEq)]
pub enum ShellLifecyclePhase {
    ProcessOptions,
    InitializeWtf,
    EnableRestrictedOptions,
    InitializeEngine,
    CreateVm,
    InstallGlobalHooks,
    RunScripts,
    DrainJobs,
    SaveProfilerData,
    Shutdown,
}

/// Shell run request identity for diagnostic observation.
#[derive(Clone, Copy, Debug, Default, Eq, Hash, PartialEq)]
pub struct ShellRunRequestId(pub u64);

#[derive(Clone, Copy, Debug, Eq, PartialEq)]
pub enum ShellGcEventResultKind {
    NotRequested,
    RequestedAtEnd,
    CompletedAtEnd,
    RejectedByPolicy,
    HeapSnapshotWritten,
}

#[derive(Clone, Copy, Debug, Eq, PartialEq)]
pub struct ShellGcEventResultRecord {
    pub run: ShellRunRequestId,
    pub kind: ShellGcEventResultKind,
    pub heap: Option<HeapId>,
    pub collection: Option<CollectionKind>,
    pub phase: Option<GcPhase>,
    pub snapshot: Option<HeapSnapshotId>,
    pub gc_at_end_requested: bool,
    pub profiler_output_requested: bool,
}

/// Authority to mutate global JSC options from the shell.
#[derive(Clone, Copy, Debug, Eq, PartialEq)]
pub enum ShellOptionMutationAuthority {
    CommandLine,
    ConfigFile,
    TestHarnessReset,
    RestrictedOptionBootstrap,
}

/// Owner of immutable shell option and source schemas.
///
/// The shell owns parsed command-line state. Static schemas describe accepted
/// option/source surfaces before any script is loaded or run.
#[derive(Clone, Copy, Debug, Default, Eq, PartialEq, Hash)]
pub enum ShellSchemaOwner {
    #[default]
    ShellCommandLine,
    TestHarness,
    GeneratedOptionMetadata,
    TestFixture,
}

/// Authority allowed to replace shell schema registries.
#[derive(Clone, Copy, Debug, Default, Eq, PartialEq, Hash)]
pub enum ShellRegistryMutationAuthority {
    #[default]
    CrateInitialization,
    GeneratedDataRefresh,
    ShellBootstrap,
}

/// Provenance for shell option/source metadata.
#[derive(Clone, Copy, Debug, Eq, PartialEq, Hash)]
pub struct ShellSchemaProvenance {
    pub generator: &'static str,
    pub source: &'static str,
    pub revision: u64,
}

impl ShellSchemaProvenance {
    pub const fn new(generator: &'static str, source: &'static str, revision: u64) -> Self {
        Self {
            generator,
            source,
            revision,
        }
    }
}

/// Static shell option value family.
#[derive(Clone, Copy, Debug, Eq, Hash, PartialEq)]
pub enum ShellOptionValueKind {
    Boolean,
    String,
    Path,
    Integer,
    Mode,
    ModuleLoaderPolicy,
}

/// Immutable metadata for one shell option.
#[derive(Clone, Copy, Debug, Eq, PartialEq)]
pub struct ShellOptionDescriptor {
    pub name: &'static str,
    pub value_kind: ShellOptionValueKind,
    pub mutation_authority: ShellOptionMutationAuthority,
    pub affects_runtime_options: bool,
    pub diagnostic_only: bool,
}

impl ShellOptionDescriptor {
    pub fn validate(&self) -> Result<(), ShellValidationError> {
        if self.name.is_empty() {
            return Err(ShellValidationError::EmptyOptionName);
        }
        if self.affects_runtime_options
            && self.mutation_authority == ShellOptionMutationAuthority::TestHarnessReset
        {
            return Err(ShellValidationError::RuntimeOptionUsesResetAuthority);
        }
        Ok(())
    }
}

/// Parsed shell option surface that affects host integration.
#[derive(Clone, Debug, Eq, PartialEq)]
pub struct ShellOptions {
    pub mode: ShellMode,
    pub dump_sampling_profiler_data: bool,
    pub enable_type_profiler: bool,
    pub enable_control_flow_profiler: bool,
    pub enable_wasm_debugger: bool,
    pub expose_dollar_vm: bool,
    pub gc_at_end: bool,
    pub option_mutation_authority: Option<ShellOptionMutationAuthority>,
    pub module_loader_policy: Option<ModuleLoaderPolicy>,
    pub import_map: Option<ImportMapId>,
}

impl ShellOptions {
    pub const fn default_for_mode(mode: ShellMode) -> Self {
        Self {
            mode,
            dump_sampling_profiler_data: false,
            enable_type_profiler: false,
            enable_control_flow_profiler: false,
            enable_wasm_debugger: false,
            expose_dollar_vm: false,
            gc_at_end: false,
            option_mutation_authority: None,
            module_loader_policy: None,
            import_map: None,
        }
    }

    pub fn validate(&self) -> Result<(), ShellValidationError> {
        if self.expose_dollar_vm
            && self.option_mutation_authority
                != Some(ShellOptionMutationAuthority::RestrictedOptionBootstrap)
        {
            return Err(ShellValidationError::DollarVmRequiresRestrictedAuthority);
        }
        if self.import_map.is_some() && self.module_loader_policy.is_none() {
            return Err(ShellValidationError::ImportMapWithoutModulePolicy);
        }
        if self.mode == ShellMode::Interactive && self.module_loader_policy.is_some() {
            return Err(ShellValidationError::InteractiveModeUsesModulePolicy);
        }
        Ok(())
    }
}

/// Builder for parsed shell option state.
#[derive(Clone, Debug)]
pub struct ShellOptionsBuilder {
    options: ShellOptions,
}

impl ShellOptionsBuilder {
    pub const fn new(mode: ShellMode) -> Self {
        Self {
            options: ShellOptions::default_for_mode(mode),
        }
    }

    pub const fn mutation_authority(mut self, authority: ShellOptionMutationAuthority) -> Self {
        self.options.option_mutation_authority = Some(authority);
        self
    }

    pub const fn dump_sampling_profiler_data(mut self, enabled: bool) -> Self {
        self.options.dump_sampling_profiler_data = enabled;
        self
    }

    pub const fn type_profiler(mut self, enabled: bool) -> Self {
        self.options.enable_type_profiler = enabled;
        self
    }

    pub const fn control_flow_profiler(mut self, enabled: bool) -> Self {
        self.options.enable_control_flow_profiler = enabled;
        self
    }

    pub const fn wasm_debugger(mut self, enabled: bool) -> Self {
        self.options.enable_wasm_debugger = enabled;
        self
    }

    pub const fn expose_dollar_vm(mut self, enabled: bool) -> Self {
        self.options.expose_dollar_vm = enabled;
        self
    }

    pub const fn gc_at_end(mut self, enabled: bool) -> Self {
        self.options.gc_at_end = enabled;
        self
    }

    pub const fn module_loader_policy(mut self, policy: ModuleLoaderPolicy) -> Self {
        self.options.module_loader_policy = Some(policy);
        self
    }

    pub const fn import_map(mut self, import_map: ImportMapId) -> Self {
        self.options.import_map = Some(import_map);
        self
    }

    pub fn build(self) -> Result<ShellOptions, ShellValidationError> {
        self.options.validate()?;
        Ok(self.options)
    }
}

/// Source origin supplied by command-line scripts, stdin, or REPL input.
#[derive(Clone, Copy, Debug, Eq, PartialEq)]
pub enum ShellSourceKind {
    File,
    Stdin,
    EvalString,
    InteractivePrompt,
    ModuleFile,
    WorkerScript,
}

/// Immutable metadata for one shell source origin.
#[derive(Clone, Copy, Debug, Eq, PartialEq)]
pub struct ShellSourceDescriptor {
    pub kind: ShellSourceKind,
    pub name: &'static str,
    pub accepts_modules: bool,
    pub accepts_source_url: bool,
    pub requires_filesystem: bool,
}

/// Registry of immutable shell option and source schemas.
#[derive(Clone, Copy, Debug, Default, Eq, PartialEq)]
pub struct ShellSchemaRegistry {
    pub options: &'static [ShellOptionDescriptor],
    pub sources: &'static [ShellSourceDescriptor],
    pub owner: ShellSchemaOwner,
    pub mutation_authority: ShellRegistryMutationAuthority,
    pub provenance: Option<ShellSchemaProvenance>,
}

impl ShellSchemaRegistry {
    pub const fn new(
        options: &'static [ShellOptionDescriptor],
        sources: &'static [ShellSourceDescriptor],
        owner: ShellSchemaOwner,
        mutation_authority: ShellRegistryMutationAuthority,
        provenance: Option<ShellSchemaProvenance>,
    ) -> Self {
        Self {
            options,
            sources,
            owner,
            mutation_authority,
            provenance,
        }
    }

    pub const fn options(self) -> &'static [ShellOptionDescriptor] {
        self.options
    }

    pub const fn sources(self) -> &'static [ShellSourceDescriptor] {
        self.sources
    }

    pub fn option_named(self, name: &str) -> Option<&'static ShellOptionDescriptor> {
        self.options
            .iter()
            .find(|descriptor| descriptor.name == name)
    }

    pub fn source(self, kind: ShellSourceKind) -> Option<&'static ShellSourceDescriptor> {
        self.sources
            .iter()
            .find(|descriptor| descriptor.kind == kind)
    }

    pub fn classify_option(
        self,
        input: ShellOptionInput<'_>,
    ) -> Result<ShellOptionClassification, ShellValidationError> {
        self.validate()?;
        let Some(descriptor) = self.option_named(input.name) else {
            return Err(ShellValidationError::OptionNameMissingFromRegistry(
                input.name.to_string(),
            ));
        };
        if descriptor.value_kind != input.value_kind {
            return Err(ShellValidationError::OptionValueKindMismatch {
                name: input.name.to_string(),
                expected: descriptor.value_kind,
                actual: input.value_kind,
            });
        }

        Ok(ShellOptionClassification {
            name: descriptor.name,
            value_kind: descriptor.value_kind,
            mutation_authority: descriptor.mutation_authority,
            affects_runtime_options: descriptor.affects_runtime_options,
            diagnostic_only: descriptor.diagnostic_only,
        })
    }

    pub fn classify_source(
        self,
        input: ShellSourceInput,
    ) -> Result<ShellSourceClassification, ShellValidationError> {
        self.validate()?;
        let Some(descriptor) = self.source(input.kind) else {
            return Err(ShellValidationError::SourceKindMissingFromRegistry);
        };
        if input.is_module && !descriptor.accepts_modules {
            return Err(ShellValidationError::ModuleSourceRejected(input.kind));
        }
        if input.has_source_url && !descriptor.accepts_source_url {
            return Err(ShellValidationError::SourceUrlRejected(input.kind));
        }

        Ok(ShellSourceClassification {
            kind: descriptor.kind,
            source_name: descriptor.name,
            mode: input.mode,
            is_module: input.is_module,
            requires_filesystem: descriptor.requires_filesystem,
            accepts_source_url: descriptor.accepts_source_url,
        })
    }

    pub fn validate(self) -> Result<(), ShellValidationError> {
        validate_unique_options(self.options)?;
        validate_unique_sources(self.sources)?;
        for option in self.options {
            option.validate()?;
        }
        for source in self.sources {
            source.validate()?;
        }
        if let Some(provenance) = self.provenance {
            if provenance.generator.is_empty() || provenance.source.is_empty() {
                return Err(ShellValidationError::EmptyProvenanceField);
            }
        }
        Ok(())
    }
}

/// Parsed shell option token before mutating shell option state.
#[derive(Clone, Copy, Debug, Eq, PartialEq)]
pub struct ShellOptionInput<'a> {
    pub name: &'a str,
    pub value_kind: ShellOptionValueKind,
}

/// Shell option descriptor classification.
#[derive(Clone, Copy, Debug, Eq, PartialEq)]
pub struct ShellOptionClassification {
    pub name: &'static str,
    pub value_kind: ShellOptionValueKind,
    pub mutation_authority: ShellOptionMutationAuthority,
    pub affects_runtime_options: bool,
    pub diagnostic_only: bool,
}

/// Source launch token before creating source providers.
#[derive(Clone, Copy, Debug, Eq, PartialEq)]
pub struct ShellSourceInput {
    pub kind: ShellSourceKind,
    pub mode: ShellMode,
    pub is_module: bool,
    pub has_source_url: bool,
}

/// Shell source descriptor classification.
#[derive(Clone, Copy, Debug, Eq, PartialEq)]
pub struct ShellSourceClassification {
    pub kind: ShellSourceKind,
    pub source_name: &'static str,
    pub mode: ShellMode,
    pub is_module: bool,
    pub requires_filesystem: bool,
    pub accepts_source_url: bool,
}

/// Pure source/run policy decision before shell execution.
#[derive(Clone, Copy, Debug, Eq, PartialEq)]
pub struct ShellRunPolicyOutcome {
    pub mode: ShellMode,
    pub phase: ShellLifecyclePhase,
    pub source_kind: Option<ShellSourceKind>,
    pub can_load_source: bool,
    pub should_create_vm: bool,
    pub should_drain_jobs: bool,
    pub should_save_profiler_data: bool,
    pub source_requires_filesystem: bool,
    pub module_policy_required: bool,
}

/// Structural shell option/source validation failure.
#[derive(Clone, Debug, Eq, PartialEq)]
pub enum ShellValidationError {
    EmptyOptionName,
    EmptySourceName,
    EmptyProvenanceField,
    DuplicateOptionName(&'static str),
    DuplicateSourceKind(ShellSourceKind),
    DuplicateSourceName(&'static str),
    RuntimeOptionUsesResetAuthority,
    DollarVmRequiresRestrictedAuthority,
    ImportMapWithoutModulePolicy,
    InteractiveModeUsesModulePolicy,
    SourceKindMissingFromRegistry,
    ModuleSourceRejected(ShellSourceKind),
    SourceUrlRejected(ShellSourceKind),
    OptionNameMissingFromRegistry(String),
    OptionValueKindMismatch {
        name: String,
        expected: ShellOptionValueKind,
        actual: ShellOptionValueKind,
    },
    RunContextMissingSource,
    RunContextProfilerOptionMismatch,
    SourceModeMismatch {
        mode: ShellMode,
        source: ShellSourceKind,
    },
    ModulePolicyRequiredForModuleSource,
    RunRequestRequiresNonzeroId,
}

/// Source fed to the shell by files, stdin, or harness callbacks.
///
/// The provider identity is owned by bytecode/source storage. The shell records
/// launch provenance and borrows the provider for the duration of the run.
#[derive(Clone, Copy, Debug, Eq, PartialEq)]
pub struct ShellSource {
    pub provider: SourceProviderId,
    pub kind: ShellSourceKind,
    pub is_module: bool,
    pub is_strict_mode: bool,
    pub has_source_url: bool,
}

impl ShellSourceDescriptor {
    pub fn validate(self) -> Result<(), ShellValidationError> {
        if self.name.is_empty() {
            Err(ShellValidationError::EmptySourceName)
        } else {
            Ok(())
        }
    }
}

impl ShellSource {
    pub fn validate(self, registry: ShellSchemaRegistry) -> Result<(), ShellValidationError> {
        let Some(descriptor) = registry.source(self.kind) else {
            return Err(ShellValidationError::SourceKindMissingFromRegistry);
        };
        if self.is_module && !descriptor.accepts_modules {
            return Err(ShellValidationError::ModuleSourceRejected(self.kind));
        }
        if self.has_source_url && !descriptor.accepts_source_url {
            return Err(ShellValidationError::SourceUrlRejected(self.kind));
        }
        Ok(())
    }
}

/// Host hook installed by the shell global object.
#[derive(Clone, Copy, Debug, Eq, PartialEq)]
pub enum ShellHostHookKind {
    Print,
    ReadFile,
    LoadScript,
    Quit,
    SetTimeout,
    DrainMicrotasks,
    StartSamplingProfiler,
    SamplingProfilerStackTraces,
    ModuleResolve,
    ModuleFetch,
    WasmDebugServer,
    CheckModuleSyntax,
    ShellOptions,
    HeapSnapshot,
}

/// Shell host hook descriptor.
#[derive(Clone, Copy, Debug, Eq, PartialEq)]
pub struct ShellHostHook {
    pub kind: ShellHostHookKind,
    pub hook: HostHookId,
    pub can_reenter_vm: bool,
    pub diagnostic_only: bool,
}

/// Test harness lifecycle hooks.
#[derive(Clone, Copy, Debug, Eq, PartialEq)]
pub enum ShellHarnessHook {
    BeforeRun,
    RunTest,
    PostTest,
    ShutdownTestRun,
    TimeoutCheck,
}

/// Test-shell option snapshot boundary.
#[derive(Clone, Debug, Eq, PartialEq)]
pub struct ShellTestRunState {
    pub setup_complete: bool,
    pub options_snapshot_count: usize,
    pub per_test_options_restored: bool,
    pub timeout_check_installed: bool,
}

/// Shell module host operation.
#[derive(Clone, Debug, Eq, PartialEq)]
pub struct ShellModuleHostOperation {
    pub key: ModuleKey,
    pub payload: Option<HostModulePayload>,
    pub policy: ModuleLoaderPolicy,
    pub referrer: Option<ModuleKey>,
    pub import_meta_requested: bool,
}

/// Wasm debugger configuration owned by the shell host.
#[derive(Clone, Copy, Debug, Eq, PartialEq)]
pub struct ShellWasmDebugConfig {
    pub enabled: bool,
    pub transport: WasmDebugTransport,
}

/// A shell run context around `runJSC`.
#[derive(Clone, Debug, Eq, PartialEq)]
pub struct ShellRunContext {
    pub global_object: Option<GlobalObjectId>,
    pub phase: ShellLifecyclePhase,
    pub options: ShellOptions,
    pub source: Option<ShellSource>,
    pub hooks_installed: usize,
    pub harness_hook: Option<ShellHarnessHook>,
    pub wasm_debug: ShellWasmDebugConfig,
    pub profiler_output_requested: bool,
    pub success: bool,
}

impl ShellRunContext {
    pub fn validate(self, registry: ShellSchemaRegistry) -> Result<(), ShellValidationError> {
        self.options.validate()?;
        if matches!(
            self.phase,
            ShellLifecyclePhase::RunScripts | ShellLifecyclePhase::DrainJobs
        ) && self.source.is_none()
        {
            return Err(ShellValidationError::RunContextMissingSource);
        }
        if let Some(source) = self.source {
            source.validate(registry)?;
        }
        if self.profiler_output_requested && !self.options.dump_sampling_profiler_data {
            return Err(ShellValidationError::RunContextProfilerOptionMismatch);
        }
        Ok(())
    }

    pub fn semantic_policy(
        &self,
        registry: ShellSchemaRegistry,
    ) -> Result<ShellRunPolicyOutcome, ShellValidationError> {
        self.clone().validate(registry)?;
        let source_descriptor = match self.source {
            Some(source) => {
                if source.is_module && self.options.module_loader_policy.is_none() {
                    return Err(ShellValidationError::ModulePolicyRequiredForModuleSource);
                }
                if !source_kind_matches_mode(source.kind, self.options.mode) {
                    return Err(ShellValidationError::SourceModeMismatch {
                        mode: self.options.mode,
                        source: source.kind,
                    });
                }
                registry.source(source.kind)
            }
            None => None,
        };

        Ok(ShellRunPolicyOutcome {
            mode: self.options.mode,
            phase: self.phase,
            source_kind: self.source.map(|source| source.kind),
            can_load_source: self.source.is_some()
                && matches!(
                    self.phase,
                    ShellLifecyclePhase::RunScripts | ShellLifecyclePhase::DrainJobs
                ),
            should_create_vm: matches!(
                self.phase,
                ShellLifecyclePhase::CreateVm
                    | ShellLifecyclePhase::InstallGlobalHooks
                    | ShellLifecyclePhase::RunScripts
                    | ShellLifecyclePhase::DrainJobs
            ),
            should_drain_jobs: self.phase == ShellLifecyclePhase::DrainJobs,
            should_save_profiler_data: self.phase == ShellLifecyclePhase::SaveProfilerData
                && self.options.dump_sampling_profiler_data,
            source_requires_filesystem: source_descriptor
                .is_some_and(|descriptor| descriptor.requires_filesystem),
            module_policy_required: self.source.is_some_and(|source| source.is_module),
        })
    }
}

/// Non-executing shell request record around a prepared run context.
#[derive(Clone, Debug, Eq, PartialEq)]
pub struct ShellRunRequestRecord {
    pub id: ShellRunRequestId,
    pub context: ShellRunContext,
}

impl ShellRunRequestRecord {
    pub fn validate(&self, registry: ShellSchemaRegistry) -> Result<(), ShellValidationError> {
        if self.id.0 == 0 {
            return Err(ShellValidationError::RunRequestRequiresNonzeroId);
        }
        self.context.clone().validate(registry)
    }

    pub fn classify_result(
        &self,
        observed: ShellObservedExecutionResult,
        registry: ShellSchemaRegistry,
    ) -> Result<ShellRunResultRecord, ShellValidationError> {
        self.validate(registry)?;
        let policy = self.context.semantic_policy(registry)?;
        let kind = match observed.execution_result {
            Some(ApiExecutionResultKind::ThrewException) => ShellRunResultKind::ThrewException,
            Some(ApiExecutionResultKind::Terminated) => ShellRunResultKind::Terminated,
            Some(ApiExecutionResultKind::ReturnedValue | ApiExecutionResultKind::ReturnedVoid) => {
                if self.context.success {
                    ShellRunResultKind::Succeeded
                } else {
                    ShellRunResultKind::Failed
                }
            }
            None => ShellRunResultKind::NoExecution,
        };

        Ok(ShellRunResultRecord {
            request: self.id,
            kind,
            phase: self.context.phase,
            source_kind: self.context.source.map(|source| source.kind),
            should_drain_jobs: policy.should_drain_jobs,
            should_save_profiler_data: policy.should_save_profiler_data,
            execution_result: observed.execution_result,
            exit_code: observed.exit_code,
        })
    }
}

/// Shell-observed execution completion before process exit handling.
#[derive(Clone, Copy, Debug, Eq, PartialEq)]
pub struct ShellObservedExecutionResult {
    pub execution_result: Option<ApiExecutionResultKind>,
    pub exit_code: Option<i32>,
}

/// Shell run result class derived from an observed execution boundary.
#[derive(Clone, Copy, Debug, Eq, PartialEq)]
pub enum ShellRunResultKind {
    Succeeded,
    Failed,
    ThrewException,
    Terminated,
    NoExecution,
}

/// Shell run result record. It does not run scripts or own process shutdown.
#[derive(Clone, Copy, Debug, Eq, PartialEq)]
pub struct ShellRunResultRecord {
    pub request: ShellRunRequestId,
    pub kind: ShellRunResultKind,
    pub phase: ShellLifecyclePhase,
    pub source_kind: Option<ShellSourceKind>,
    pub should_drain_jobs: bool,
    pub should_save_profiler_data: bool,
    pub execution_result: Option<ApiExecutionResultKind>,
    pub exit_code: Option<i32>,
}

/// Shell execution report assembled from run, API, GC, and tier diagnostics.
#[derive(Clone, Debug, Eq, PartialEq)]
pub struct ShellExecutionReport {
    pub run_result: ShellRunResultRecord,
    pub api_execution: Option<ApiExecutionDiagnosticSummary>,
    pub gc_events: Vec<ShellGcEventResultRecord>,
    pub api_gc_summaries: Vec<ApiGcDiagnosticSummary>,
    pub tier_summaries: Vec<ApiTierDiagnosticSummary>,
    pub profiler_output_visible: bool,
    pub fallback_visible_count: usize,
}

impl ShellExecutionReport {
    pub fn from_run_result(
        run_result: ShellRunResultRecord,
        api_execution: Option<ApiExecutionDiagnosticSummary>,
        gc_events: Vec<ShellGcEventResultRecord>,
        api_gc_summaries: Vec<ApiGcDiagnosticSummary>,
        tier_summaries: Vec<ApiTierDiagnosticSummary>,
    ) -> Self {
        let profiler_output_visible = run_result.should_save_profiler_data;
        let fallback_visible_count = tier_summaries
            .iter()
            .filter(|summary| summary.fallback_resume.is_some())
            .count();
        Self {
            run_result,
            api_execution,
            gc_events,
            api_gc_summaries,
            tier_summaries,
            profiler_output_visible,
            fallback_visible_count,
        }
    }
}

fn source_kind_matches_mode(kind: ShellSourceKind, mode: ShellMode) -> bool {
    matches!(
        (kind, mode),
        (
            ShellSourceKind::File,
            ShellMode::ScriptFile | ShellMode::TestHarness
        ) | (
            ShellSourceKind::Stdin,
            ShellMode::ScriptFile | ShellMode::TestHarness
        ) | (
            ShellSourceKind::EvalString,
            ShellMode::ScriptFile | ShellMode::TestHarness
        ) | (ShellSourceKind::InteractivePrompt, ShellMode::Interactive)
            | (ShellSourceKind::ModuleFile, ShellMode::Module)
            | (ShellSourceKind::WorkerScript, ShellMode::Worker)
    )
}

const SHELL_SCHEMA_PROVENANCE: ShellSchemaProvenance = ShellSchemaProvenance {
    generator: "hand-authored",
    source: "Source/JavaScriptCore/rust/src/shell/mod.rs",
    revision: 1,
};

pub const SHELL_OPTION_DESCRIPTORS: &[ShellOptionDescriptor] = &[
    ShellOptionDescriptor {
        name: "dumpSamplingProfilerData",
        value_kind: ShellOptionValueKind::Boolean,
        mutation_authority: ShellOptionMutationAuthority::CommandLine,
        affects_runtime_options: false,
        diagnostic_only: true,
    },
    ShellOptionDescriptor {
        name: "enableTypeProfiler",
        value_kind: ShellOptionValueKind::Boolean,
        mutation_authority: ShellOptionMutationAuthority::CommandLine,
        affects_runtime_options: true,
        diagnostic_only: true,
    },
    ShellOptionDescriptor {
        name: "enableControlFlowProfiler",
        value_kind: ShellOptionValueKind::Boolean,
        mutation_authority: ShellOptionMutationAuthority::CommandLine,
        affects_runtime_options: true,
        diagnostic_only: true,
    },
    ShellOptionDescriptor {
        name: "enableWasmDebugger",
        value_kind: ShellOptionValueKind::Boolean,
        mutation_authority: ShellOptionMutationAuthority::CommandLine,
        affects_runtime_options: true,
        diagnostic_only: true,
    },
    ShellOptionDescriptor {
        name: "useDollarVM",
        value_kind: ShellOptionValueKind::Boolean,
        mutation_authority: ShellOptionMutationAuthority::RestrictedOptionBootstrap,
        affects_runtime_options: true,
        diagnostic_only: true,
    },
];

pub const SHELL_SOURCE_DESCRIPTORS: &[ShellSourceDescriptor] = &[
    ShellSourceDescriptor {
        kind: ShellSourceKind::File,
        name: "file",
        accepts_modules: false,
        accepts_source_url: true,
        requires_filesystem: true,
    },
    ShellSourceDescriptor {
        kind: ShellSourceKind::Stdin,
        name: "stdin",
        accepts_modules: false,
        accepts_source_url: true,
        requires_filesystem: false,
    },
    ShellSourceDescriptor {
        kind: ShellSourceKind::EvalString,
        name: "eval-string",
        accepts_modules: false,
        accepts_source_url: true,
        requires_filesystem: false,
    },
    ShellSourceDescriptor {
        kind: ShellSourceKind::InteractivePrompt,
        name: "interactive-prompt",
        accepts_modules: false,
        accepts_source_url: false,
        requires_filesystem: false,
    },
    ShellSourceDescriptor {
        kind: ShellSourceKind::ModuleFile,
        name: "module-file",
        accepts_modules: true,
        accepts_source_url: true,
        requires_filesystem: true,
    },
    ShellSourceDescriptor {
        kind: ShellSourceKind::WorkerScript,
        name: "worker-script",
        accepts_modules: true,
        accepts_source_url: true,
        requires_filesystem: true,
    },
];

pub const SHELL_SCHEMA_REGISTRY: ShellSchemaRegistry = ShellSchemaRegistry {
    options: SHELL_OPTION_DESCRIPTORS,
    sources: SHELL_SOURCE_DESCRIPTORS,
    owner: ShellSchemaOwner::ShellCommandLine,
    mutation_authority: ShellRegistryMutationAuthority::CrateInitialization,
    provenance: Some(SHELL_SCHEMA_PROVENANCE),
};

fn validate_unique_options(options: &[ShellOptionDescriptor]) -> Result<(), ShellValidationError> {
    for (index, option) in options.iter().enumerate() {
        for other in options.iter().skip(index + 1) {
            if option.name == other.name {
                return Err(ShellValidationError::DuplicateOptionName(option.name));
            }
        }
    }
    Ok(())
}

fn validate_unique_sources(sources: &[ShellSourceDescriptor]) -> Result<(), ShellValidationError> {
    for (index, source) in sources.iter().enumerate() {
        for other in sources.iter().skip(index + 1) {
            if source.kind == other.kind {
                return Err(ShellValidationError::DuplicateSourceKind(source.kind));
            }
            if source.name == other.name {
                return Err(ShellValidationError::DuplicateSourceName(source.name));
            }
        }
    }
    Ok(())
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn validates_builtin_shell_registry() {
        assert_eq!(SHELL_SCHEMA_REGISTRY.validate(), Ok(()));
    }

    #[test]
    fn rejects_dollar_vm_without_restricted_authority() {
        let result = ShellOptionsBuilder::new(ShellMode::TestHarness)
            .expose_dollar_vm(true)
            .build();

        assert_eq!(
            result,
            Err(ShellValidationError::DollarVmRequiresRestrictedAuthority)
        );
    }

    #[test]
    fn builds_restricted_dollar_vm_options() {
        let result = ShellOptionsBuilder::new(ShellMode::TestHarness)
            .mutation_authority(ShellOptionMutationAuthority::RestrictedOptionBootstrap)
            .expose_dollar_vm(true)
            .build();

        assert!(result.is_ok());
    }

    #[test]
    fn classifies_shell_option_by_descriptor() {
        let classification = SHELL_SCHEMA_REGISTRY
            .classify_option(ShellOptionInput {
                name: "enableTypeProfiler",
                value_kind: ShellOptionValueKind::Boolean,
            })
            .expect("classification");

        assert_eq!(
            classification.mutation_authority,
            ShellOptionMutationAuthority::CommandLine
        );
        assert!(classification.affects_runtime_options);
        assert!(classification.diagnostic_only);
    }

    #[test]
    fn rejects_option_value_kind_mismatch() {
        assert_eq!(
            SHELL_SCHEMA_REGISTRY.classify_option(ShellOptionInput {
                name: "enableTypeProfiler",
                value_kind: ShellOptionValueKind::String,
            }),
            Err(ShellValidationError::OptionValueKindMismatch {
                name: "enableTypeProfiler".to_string(),
                expected: ShellOptionValueKind::Boolean,
                actual: ShellOptionValueKind::String,
            })
        );
    }

    #[test]
    fn classifies_module_file_source() {
        let classification = SHELL_SCHEMA_REGISTRY
            .classify_source(ShellSourceInput {
                kind: ShellSourceKind::ModuleFile,
                mode: ShellMode::Module,
                is_module: true,
                has_source_url: true,
            })
            .expect("source classification");

        assert_eq!(classification.source_name, "module-file");
        assert!(classification.requires_filesystem);
        assert!(classification.is_module);
    }

    #[test]
    fn rejects_module_for_non_module_source_kind() {
        assert_eq!(
            SHELL_SCHEMA_REGISTRY.classify_source(ShellSourceInput {
                kind: ShellSourceKind::EvalString,
                mode: ShellMode::ScriptFile,
                is_module: true,
                has_source_url: false,
            }),
            Err(ShellValidationError::ModuleSourceRejected(
                ShellSourceKind::EvalString
            ))
        );
    }

    #[test]
    fn run_policy_accepts_module_file_with_loader_policy() {
        let options = ShellOptionsBuilder::new(ShellMode::Module)
            .module_loader_policy(ModuleLoaderPolicy::new(
                crate::modules::ModuleLoaderOperation::LoadModule,
                true,
                false,
                false,
            ))
            .build()
            .expect("options");
        let context = ShellRunContext {
            global_object: None,
            phase: ShellLifecyclePhase::RunScripts,
            options,
            source: Some(ShellSource {
                provider: SourceProviderId(1),
                kind: ShellSourceKind::ModuleFile,
                is_module: true,
                is_strict_mode: true,
                has_source_url: true,
            }),
            hooks_installed: 0,
            harness_hook: None,
            wasm_debug: ShellWasmDebugConfig {
                enabled: false,
                transport: WasmDebugTransport::HostProvided,
            },
            profiler_output_requested: false,
            success: true,
        };

        let policy = context
            .semantic_policy(SHELL_SCHEMA_REGISTRY)
            .expect("policy");

        assert!(policy.can_load_source);
        assert!(policy.module_policy_required);
        assert!(policy.source_requires_filesystem);
    }

    #[test]
    fn run_policy_rejects_interactive_source_in_script_mode() {
        let options = ShellOptionsBuilder::new(ShellMode::ScriptFile)
            .build()
            .expect("options");
        let context = ShellRunContext {
            global_object: None,
            phase: ShellLifecyclePhase::RunScripts,
            options,
            source: Some(ShellSource {
                provider: SourceProviderId(2),
                kind: ShellSourceKind::InteractivePrompt,
                is_module: false,
                is_strict_mode: false,
                has_source_url: false,
            }),
            hooks_installed: 0,
            harness_hook: None,
            wasm_debug: ShellWasmDebugConfig {
                enabled: false,
                transport: WasmDebugTransport::HostProvided,
            },
            profiler_output_requested: false,
            success: true,
        };

        assert_eq!(
            context.semantic_policy(SHELL_SCHEMA_REGISTRY),
            Err(ShellValidationError::SourceModeMismatch {
                mode: ShellMode::ScriptFile,
                source: ShellSourceKind::InteractivePrompt,
            })
        );
    }

    #[test]
    fn classifies_shell_run_request_result() {
        let options = ShellOptionsBuilder::new(ShellMode::ScriptFile)
            .build()
            .expect("options");
        let request = ShellRunRequestRecord {
            id: ShellRunRequestId(1),
            context: ShellRunContext {
                global_object: None,
                phase: ShellLifecyclePhase::RunScripts,
                options,
                source: Some(ShellSource {
                    provider: SourceProviderId(5),
                    kind: ShellSourceKind::EvalString,
                    is_module: false,
                    is_strict_mode: false,
                    has_source_url: true,
                }),
                hooks_installed: 0,
                harness_hook: None,
                wasm_debug: ShellWasmDebugConfig {
                    enabled: false,
                    transport: WasmDebugTransport::HostProvided,
                },
                profiler_output_requested: false,
                success: true,
            },
        };

        let result = request
            .classify_result(
                ShellObservedExecutionResult {
                    execution_result: Some(ApiExecutionResultKind::ReturnedValue),
                    exit_code: Some(0),
                },
                SHELL_SCHEMA_REGISTRY,
            )
            .expect("run result");

        assert_eq!(result.kind, ShellRunResultKind::Succeeded);
        assert_eq!(result.source_kind, Some(ShellSourceKind::EvalString));
    }

    #[test]
    fn classifies_shell_run_exception_result() {
        let options = ShellOptionsBuilder::new(ShellMode::ScriptFile)
            .build()
            .expect("options");
        let request = ShellRunRequestRecord {
            id: ShellRunRequestId(2),
            context: ShellRunContext {
                global_object: None,
                phase: ShellLifecyclePhase::RunScripts,
                options,
                source: Some(ShellSource {
                    provider: SourceProviderId(6),
                    kind: ShellSourceKind::EvalString,
                    is_module: false,
                    is_strict_mode: false,
                    has_source_url: false,
                }),
                hooks_installed: 0,
                harness_hook: None,
                wasm_debug: ShellWasmDebugConfig {
                    enabled: false,
                    transport: WasmDebugTransport::HostProvided,
                },
                profiler_output_requested: false,
                success: false,
            },
        };

        let result = request
            .classify_result(
                ShellObservedExecutionResult {
                    execution_result: Some(ApiExecutionResultKind::ThrewException),
                    exit_code: Some(3),
                },
                SHELL_SCHEMA_REGISTRY,
            )
            .expect("run result");

        assert_eq!(result.kind, ShellRunResultKind::ThrewException);
        assert_eq!(result.exit_code, Some(3));
    }

    #[test]
    fn shell_execution_report_exposes_gc_and_tier_diagnostics() {
        let run_result = ShellRunResultRecord {
            request: ShellRunRequestId(3),
            kind: ShellRunResultKind::Succeeded,
            phase: ShellLifecyclePhase::SaveProfilerData,
            source_kind: Some(ShellSourceKind::EvalString),
            should_drain_jobs: false,
            should_save_profiler_data: true,
            execution_result: Some(ApiExecutionResultKind::ReturnedVoid),
            exit_code: Some(0),
        };
        let gc_event = ShellGcEventResultRecord {
            run: ShellRunRequestId(3),
            kind: ShellGcEventResultKind::CompletedAtEnd,
            heap: Some(HeapId(1)),
            collection: Some(CollectionKind::Full),
            phase: Some(GcPhase::NotRunning),
            snapshot: None,
            gc_at_end_requested: true,
            profiler_output_requested: true,
        };
        let tier = ApiTierDiagnosticSummary::from_fallback(crate::jit::TierFallbackResultRecord {
            owner: crate::runtime::CodeBlockId(crate::gc::CellId(7)),
            from_tier: crate::jit::JitType::Baseline,
            attempted_tier: crate::jit::JitType::Dfg,
            reason: crate::jit::TierFallbackReason::UnsupportedTier,
            target: crate::jit::TierFallbackTarget::ReturnToInterpreter,
            bytecode_index: Some(crate::bytecode::BytecodeIndex::from_offset(1)),
            resume: crate::jit::TierFallbackResumeKind::ContinueInInterpreter,
            preserves_profile: true,
            should_count_invalidation: true,
            clears_active_request: true,
        });

        let report = ShellExecutionReport::from_run_result(
            run_result,
            None,
            vec![gc_event],
            vec![],
            vec![tier],
        );

        assert!(report.profiler_output_visible);
        assert_eq!(report.gc_events.len(), 1);
        assert_eq!(report.fallback_visible_count, 1);
    }
}
