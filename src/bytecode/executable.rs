use std::sync::Arc;

use crate::jit::{
    BaselineArityCheckNativeEntry, BaselineArityCheckUnavailableReason,
    BaselineNativeEntryDescriptor, BaselineNativeEntryToken, BaselineNativeEntryTokenKind,
    JitCodeId, MachineCodeHandle, MachineCodeRange,
};
use crate::runtime::{
    ArityCheckMode, CodeBlockId, CodeSpecializationKind, ExecutableId, NativeCodeId,
};
use crate::strings::Identifier;
use crate::vm::{
    BaselineNativeDispatchTokenSelection, VmEntryDispatchSelection, VmEntryLaunchDescriptor,
};

use crate::bytecode::code_block::{
    CodeBlock, CodeBlockLifecycleState, CodeFeatures, CodeGenerationModeSet, CodeKind,
    CodeSpecialization, DerivedContextType, EvalContextType, ExecutableInfo, JitCodeSlot,
    NeedsClassFieldInitializer, ParseMode, PrivateBrandRequirement, ScriptMode, SourcePosition,
    SourceProvenance, SourceProviderId, SourceRange, UnlinkedCodeBlock,
};

/// Common executable-cell contract.
///
/// Executables install, replace, or clear code blocks through VM/GC-aware APIs.
/// Raw entrypoint pointers and future JIT state are represented as opaque slots
/// because ownership, write barriers, and executable memory lifetimes belong to
/// VM/runtime modules. Executable identity is the runtime-owned
/// `ExecutableId`; this module borrows it to describe bytecode ownership edges.
#[derive(Clone, Debug, Default)]
pub struct ExecutableBase {
    pub identity: Option<ExecutableId>,
    pub entrypoints: ExecutableEntrypoints,
    pub policy: ExecutablePolicy,
    pub visibility: ImplementationVisibility,
}

#[derive(Clone, Debug, Default)]
pub struct ExecutableEntrypoints {
    pub call: EntrypointState,
    pub construct: EntrypointState,
}

#[derive(Clone, Debug, Default)]
pub struct EntrypointState {
    pub generated_jit: Option<JitEntrypointSlot>,
    pub generated_jit_with_arity_check: Option<JitEntrypointSlot>,
    pub interpreter: Option<InterpreterEntrypointSlot>,
    /// ExecutableBase-style cache for the arity-check baseline entry only.
    ///
    /// The normal no-arity entry is returned in publication results for
    /// diagnostics, but is intentionally not cached here. That mirrors JSC's
    /// split where ExecutableBase caches the virtual arity-check entrypoint
    /// while no-arity entrypoint users cache through their own call-site state.
    pub cached_baseline_arity_publication: Option<ExecutableEntryCacheRecord>,
}

#[derive(Clone, Copy, Debug, Eq, PartialEq, Hash)]
#[repr(transparent)]
pub struct JitEntrypointSlot(pub u32);

#[derive(Clone, Copy, Debug, Eq, PartialEq, Hash)]
#[repr(transparent)]
pub struct InterpreterEntrypointSlot(pub u32);

#[derive(Clone, Copy, Debug, Eq, PartialEq, Hash)]
pub struct ExecutableEntryCacheKey {
    pub specialization: CodeSpecializationKind,
    pub arity: ExecutableAritySelection,
}

impl ExecutableEntryCacheKey {
    pub fn new(specialization: CodeSpecializationKind, arity: ExecutableAritySelection) -> Self {
        Self {
            specialization,
            arity,
        }
    }

    fn code_specialization(self) -> CodeSpecialization {
        match self.specialization {
            CodeSpecializationKind::Call => CodeSpecialization::Call,
            CodeSpecializationKind::Construct => CodeSpecialization::Construct,
        }
    }
}

#[derive(Clone, Copy, Debug, Eq, PartialEq, Hash)]
pub enum ExecutableAritySelection {
    AlreadyChecked,
    MustCheckArity,
}

impl From<ArityCheckMode> for ExecutableAritySelection {
    fn from(mode: ArityCheckMode) -> Self {
        match mode {
            ArityCheckMode::AlreadyChecked => Self::AlreadyChecked,
            ArityCheckMode::MustCheckArity => Self::MustCheckArity,
        }
    }
}

#[derive(Clone, Copy, Debug, Eq, PartialEq)]
pub struct ExecutableEntryCacheRecord {
    /// Symbolic publication result for one executable entrypoint lookup.
    ///
    /// Records are returned for both normal and arity-check lookups. Only
    /// `MustCheckArity` records are retained in `EntrypointState`.
    pub key: ExecutableEntryCacheKey,
    pub executable: Option<ExecutableId>,
    pub owner: CodeBlockId,
    pub code_block: CodeBlockId,
    pub readiness_ordinal: u64,
    pub baseline_jit_slot: JitCodeSlot,
    pub baseline_native_entry: ExecutableBaselineNativeEntryRecord,
}

#[derive(Clone, Copy, Debug, Eq, PartialEq)]
pub struct ExecutableBaselineNativeEntryRecord {
    pub artifact_id: JitCodeId,
    pub native_symbol: NativeCodeId,
    pub machine_code: MachineCodeHandle,
    pub machine_range: MachineCodeRange,
    pub entrypoint: crate::jit::Entrypoint,
    pub selection: ExecutableBaselineNativeEntrySelection,
}

#[derive(Clone, Copy, Debug, Eq, PartialEq)]
pub enum ExecutableBaselineNativeEntrySelection {
    Normal(BaselineNativeEntryToken),
    ArityCheck(BaselineNativeEntryToken),
    ArityCheckUnavailable(BaselineArityCheckUnavailableReason),
}

#[derive(Clone, Copy, Debug, Eq, PartialEq)]
pub struct ExecutableEntryPublicationRequest {
    /// Metadata-only bridge from the VM's validated native-entry launch
    /// descriptor into the executable-owned publication slot.
    pub executable: Option<ExecutableId>,
    pub launch_descriptor: VmEntryLaunchDescriptor,
    pub baseline_jit_slot: Option<JitCodeSlot>,
}

impl ExecutableEntryPublicationRequest {
    pub fn from_launch_descriptor(launch_descriptor: VmEntryLaunchDescriptor) -> Self {
        Self {
            executable: None,
            launch_descriptor,
            baseline_jit_slot: None,
        }
    }

    pub fn with_executable(mut self, executable: Option<ExecutableId>) -> Self {
        self.executable = executable;
        self
    }

    pub fn with_baseline_jit_slot(mut self, baseline_jit_slot: JitCodeSlot) -> Self {
        self.baseline_jit_slot = Some(baseline_jit_slot);
        self
    }

    pub fn key(&self) -> ExecutableEntryCacheKey {
        ExecutableEntryCacheKey::new(
            self.launch_descriptor.call_frame.specialization,
            ExecutableAritySelection::from(self.launch_descriptor.call_frame.arity_mode),
        )
    }
}

#[derive(Clone, Copy, Debug, Eq, PartialEq)]
pub enum ExecutableEntryPublicationError {
    InstalledCodeBlockMismatch {
        expected: CodeBlockId,
        actual: CodeBlockId,
    },
    LaunchCodeBlockMismatch {
        expected: CodeBlockId,
        actual: CodeBlockId,
    },
    ScopeOwnerMismatch {
        expected: CodeBlockId,
        actual: CodeBlockId,
    },
    ScopeEntryCodeBlockMismatch {
        expected: CodeBlockId,
        actual: Option<CodeBlockId>,
    },
    CallFrameCodeBlockMismatch {
        expected: CodeBlockId,
        actual: Option<CodeBlockId>,
    },
    DescriptorOwnerMismatch {
        expected: CodeBlockId,
        actual: CodeBlockId,
    },
    TokenOwnerMismatch {
        expected: CodeBlockId,
        actual: CodeBlockId,
    },
    TokenArtifactMismatch {
        expected: JitCodeId,
        actual: JitCodeId,
    },
    TokenNativeSymbolMismatch {
        expected: NativeCodeId,
        actual: NativeCodeId,
    },
    TokenMachineCodeMismatch {
        expected: MachineCodeHandle,
        actual: MachineCodeHandle,
    },
    TokenEntrypointMismatch,
    TokenKindMismatch {
        expected: BaselineNativeEntryTokenKind,
        actual: BaselineNativeEntryTokenKind,
    },
    DispatchSelectionMismatch,
    CodeBlockSpecializationMismatch {
        expected: CodeSpecialization,
        actual: CodeSpecialization,
    },
    CodeBlockLifecycleMismatch {
        expected: CodeBlockLifecycleState,
        actual: CodeBlockLifecycleState,
    },
    BaselineJitSlotMissing,
    BaselineJitSlotMismatch {
        expected: JitCodeSlot,
        actual: JitCodeSlot,
    },
    ExecutableIdentityMismatch {
        expected: ExecutableId,
        actual: ExecutableId,
    },
    InstalledCodeBlockMissing {
        specialization: CodeSpecializationKind,
    },
}

impl EntrypointState {
    pub fn cached_baseline_arity_publication(&self) -> Option<&ExecutableEntryCacheRecord> {
        self.cached_baseline_arity_publication.as_ref()
    }
}

impl ExecutableEntrypoints {
    pub fn publish_baseline_native_entry(
        &mut self,
        request: ExecutableEntryPublicationRequest,
        installed_code_block: CodeBlockId,
        code_block: &CodeBlock,
    ) -> Result<ExecutableEntryCacheRecord, ExecutableEntryPublicationError> {
        let key = request.key();
        validate_publication_request(&request, installed_code_block, code_block, key)?;
        let baseline_jit_slot = code_block
            .entrypoints()
            .baseline_jit
            .ok_or(ExecutableEntryPublicationError::BaselineJitSlotMissing)?;
        if let Some(expected) = request.baseline_jit_slot {
            if expected != baseline_jit_slot {
                return Err(ExecutableEntryPublicationError::BaselineJitSlotMismatch {
                    expected,
                    actual: baseline_jit_slot,
                });
            }
        }

        let baseline_native_entry =
            baseline_native_entry_record(request.launch_descriptor.native_entry, key.arity)?;
        let record = ExecutableEntryCacheRecord {
            key,
            executable: request.executable,
            owner: request.launch_descriptor.owner,
            code_block: request.launch_descriptor.code_block,
            readiness_ordinal: request.launch_descriptor.readiness_ordinal,
            baseline_jit_slot,
            baseline_native_entry,
        };
        if key.arity == ExecutableAritySelection::MustCheckArity {
            self.state_mut(key.specialization)
                .cache_baseline_arity_publication(record);
        }
        Ok(record)
    }

    fn state_mut(&mut self, specialization: CodeSpecializationKind) -> &mut EntrypointState {
        match specialization {
            CodeSpecializationKind::Call => &mut self.call,
            CodeSpecializationKind::Construct => &mut self.construct,
        }
    }
}

impl EntrypointState {
    fn cache_baseline_arity_publication(&mut self, record: ExecutableEntryCacheRecord) {
        debug_assert_eq!(record.key.arity, ExecutableAritySelection::MustCheckArity);
        self.cached_baseline_arity_publication = Some(record);
    }
}

fn validate_publication_request(
    request: &ExecutableEntryPublicationRequest,
    installed_code_block: CodeBlockId,
    code_block: &CodeBlock,
    key: ExecutableEntryCacheKey,
) -> Result<(), ExecutableEntryPublicationError> {
    let descriptor = request.launch_descriptor.native_entry;
    let owner = request.launch_descriptor.owner;
    if owner != installed_code_block {
        return Err(
            ExecutableEntryPublicationError::InstalledCodeBlockMismatch {
                expected: owner,
                actual: installed_code_block,
            },
        );
    }
    if request.launch_descriptor.code_block != installed_code_block {
        return Err(ExecutableEntryPublicationError::LaunchCodeBlockMismatch {
            expected: installed_code_block,
            actual: request.launch_descriptor.code_block,
        });
    }
    if request.launch_descriptor.scope.owner != owner {
        return Err(ExecutableEntryPublicationError::ScopeOwnerMismatch {
            expected: owner,
            actual: request.launch_descriptor.scope.owner,
        });
    }
    if request.launch_descriptor.scope.entry_code_block != Some(installed_code_block) {
        return Err(
            ExecutableEntryPublicationError::ScopeEntryCodeBlockMismatch {
                expected: installed_code_block,
                actual: request.launch_descriptor.scope.entry_code_block,
            },
        );
    }
    if request.launch_descriptor.call_frame.code_block != Some(installed_code_block) {
        return Err(
            ExecutableEntryPublicationError::CallFrameCodeBlockMismatch {
                expected: installed_code_block,
                actual: request.launch_descriptor.call_frame.code_block,
            },
        );
    }
    if descriptor.owner != owner {
        return Err(ExecutableEntryPublicationError::DescriptorOwnerMismatch {
            expected: owner,
            actual: descriptor.owner,
        });
    }

    validate_descriptor_tokens(descriptor)?;
    validate_dispatch_selection(request.launch_descriptor.dispatch, descriptor, key.arity)?;

    let actual_specialization = code_block.link_context().specialization;
    if !code_specialization_matches_publication_key(actual_specialization, key) {
        return Err(
            ExecutableEntryPublicationError::CodeBlockSpecializationMismatch {
                expected: key.code_specialization(),
                actual: actual_specialization,
            },
        );
    }
    let expected_lifecycle = CodeBlockLifecycleState::BaselineInstalled;
    let actual_lifecycle = code_block.lifecycle();
    if actual_lifecycle != expected_lifecycle {
        return Err(
            ExecutableEntryPublicationError::CodeBlockLifecycleMismatch {
                expected: expected_lifecycle,
                actual: actual_lifecycle,
            },
        );
    }
    if code_block.entrypoints().baseline_jit.is_none() {
        return Err(ExecutableEntryPublicationError::BaselineJitSlotMissing);
    }
    if let (Some(expected), Some(actual)) = (
        request.executable,
        code_block.link_context().owner_executable,
    ) {
        if expected != actual {
            return Err(
                ExecutableEntryPublicationError::ExecutableIdentityMismatch { expected, actual },
            );
        }
    }
    Ok(())
}

fn code_specialization_matches_publication_key(
    actual: CodeSpecialization,
    key: ExecutableEntryCacheKey,
) -> bool {
    matches!(
        (actual, key.specialization),
        (CodeSpecialization::Call, CodeSpecializationKind::Call)
            | (
                CodeSpecialization::Construct,
                CodeSpecializationKind::Construct
            )
            | (CodeSpecialization::None, CodeSpecializationKind::Call)
    )
}

fn validate_descriptor_tokens(
    descriptor: BaselineNativeEntryDescriptor,
) -> Result<(), ExecutableEntryPublicationError> {
    validate_token(
        descriptor.normal_entry,
        descriptor,
        BaselineNativeEntryTokenKind::Normal,
    )?;
    if let BaselineArityCheckNativeEntry::Token(token) = descriptor.arity_check_entry {
        validate_token(token, descriptor, BaselineNativeEntryTokenKind::ArityCheck)?;
    }
    Ok(())
}

fn validate_dispatch_selection(
    dispatch: VmEntryDispatchSelection,
    descriptor: BaselineNativeEntryDescriptor,
    arity: ExecutableAritySelection,
) -> Result<(), ExecutableEntryPublicationError> {
    match (arity, dispatch) {
        (
            ExecutableAritySelection::AlreadyChecked,
            VmEntryDispatchSelection::BaselineNative(
                BaselineNativeDispatchTokenSelection::NormalEntry { token },
            ),
        ) if token == descriptor.normal_entry => Ok(()),
        (
            ExecutableAritySelection::MustCheckArity,
            VmEntryDispatchSelection::BaselineNative(
                BaselineNativeDispatchTokenSelection::ArityCheckEntry { token },
            ),
        ) if descriptor.arity_check_entry == BaselineArityCheckNativeEntry::Token(token) => Ok(()),
        (
            ExecutableAritySelection::MustCheckArity,
            VmEntryDispatchSelection::BaselineNative(
                BaselineNativeDispatchTokenSelection::ArityCheckUnavailable { reason },
            ),
        ) if descriptor.arity_check_entry == BaselineArityCheckNativeEntry::Unavailable(reason) => {
            Ok(())
        }
        _ => Err(ExecutableEntryPublicationError::DispatchSelectionMismatch),
    }
}

fn validate_token(
    token: BaselineNativeEntryToken,
    descriptor: BaselineNativeEntryDescriptor,
    expected_kind: BaselineNativeEntryTokenKind,
) -> Result<(), ExecutableEntryPublicationError> {
    if token.owner != descriptor.owner {
        return Err(ExecutableEntryPublicationError::TokenOwnerMismatch {
            expected: descriptor.owner,
            actual: token.owner,
        });
    }
    if token.artifact_id != descriptor.artifact_id {
        return Err(ExecutableEntryPublicationError::TokenArtifactMismatch {
            expected: descriptor.artifact_id,
            actual: token.artifact_id,
        });
    }
    if token.native_symbol != descriptor.native_symbol {
        return Err(ExecutableEntryPublicationError::TokenNativeSymbolMismatch {
            expected: descriptor.native_symbol,
            actual: token.native_symbol,
        });
    }
    if token.machine_code != descriptor.machine_code {
        return Err(ExecutableEntryPublicationError::TokenMachineCodeMismatch {
            expected: descriptor.machine_code,
            actual: token.machine_code,
        });
    }
    if token.entrypoint != descriptor.entrypoint {
        return Err(ExecutableEntryPublicationError::TokenEntrypointMismatch);
    }
    if token.kind != expected_kind {
        return Err(ExecutableEntryPublicationError::TokenKindMismatch {
            expected: expected_kind,
            actual: token.kind,
        });
    }
    Ok(())
}

fn baseline_native_entry_record(
    descriptor: BaselineNativeEntryDescriptor,
    arity: ExecutableAritySelection,
) -> Result<ExecutableBaselineNativeEntryRecord, ExecutableEntryPublicationError> {
    let selection = match arity {
        ExecutableAritySelection::AlreadyChecked => {
            ExecutableBaselineNativeEntrySelection::Normal(descriptor.normal_entry)
        }
        ExecutableAritySelection::MustCheckArity => match descriptor.arity_check_entry {
            BaselineArityCheckNativeEntry::Token(token) => {
                ExecutableBaselineNativeEntrySelection::ArityCheck(token)
            }
            BaselineArityCheckNativeEntry::Unavailable(reason) => {
                ExecutableBaselineNativeEntrySelection::ArityCheckUnavailable(reason)
            }
        },
    };
    Ok(ExecutableBaselineNativeEntryRecord {
        artifact_id: descriptor.artifact_id,
        native_symbol: descriptor.native_symbol,
        machine_code: descriptor.machine_code,
        machine_range: descriptor.machine_range,
        entrypoint: descriptor.entrypoint,
        selection,
    })
}

#[derive(Clone, Debug, Default)]
pub struct ExecutablePolicy {
    pub intrinsic: IntrinsicKind,
    pub inline_attribute: InlineAttribute,
    pub never_inline: bool,
    pub never_optimize: bool,
    pub never_ftl_optimize: bool,
    pub can_use_osr_exit_fuzzing: bool,
}

#[derive(Clone, Copy, Debug, Default, Eq, PartialEq, Hash)]
pub enum IntrinsicKind {
    #[default]
    None,
    Host,
    Builtin(u32),
}

#[derive(Clone, Copy, Debug, Default, Eq, PartialEq, Hash)]
pub enum InlineAttribute {
    #[default]
    None,
    Always,
    Never,
}

#[derive(Clone, Copy, Debug, Default, Eq, PartialEq, Hash)]
pub enum ImplementationVisibility {
    #[default]
    Public,
    Private,
    Hidden,
}

#[derive(Clone, Debug)]
pub struct ScriptExecutable {
    pub base: ExecutableBase,
    pub kind: ScriptExecutableKind,
    pub source: SourceProvenance,
    pub parse_record: ParseRecord,
    pub unlinked: Option<Arc<UnlinkedCodeBlock>>,
    pub installed_code: Option<CodeBlock>,
}

#[derive(Clone, Copy, Debug, Eq, PartialEq, Hash)]
pub enum ScriptExecutableKind {
    Program,
    Eval,
    ModuleProgram,
    Function,
}

#[derive(Clone, Debug, Default, Eq, PartialEq)]
pub struct ParseRecord {
    pub features: CodeFeatures,
    pub generation_modes: CodeGenerationModeSet,
    pub last_line: Option<u32>,
    pub end_column: Option<u32>,
    pub has_captured_variables: bool,
    pub semantic: ExecutableParseSemanticMetadata,
}

#[derive(Clone, Copy, Debug, Default, Eq, PartialEq)]
pub struct ExecutableParseSemanticMetadata {
    pub goal: ExecutableParseGoal,
    pub strict: bool,
    pub function: Option<ExecutableFunctionParseMetadata>,
    pub eval: Option<ExecutableEvalParseMetadata>,
    pub module: Option<ExecutableModuleParseMetadata>,
}

#[derive(Clone, Copy, Debug, Default, Eq, PartialEq, Hash)]
pub enum ExecutableParseGoal {
    #[default]
    Program,
    Function,
    Eval,
    Module,
}

#[derive(Clone, Copy, Debug, Default, Eq, PartialEq)]
pub struct ExecutableFunctionParseMetadata {
    pub parameter_count_excluding_this: u32,
    pub has_non_simple_parameters: bool,
    pub needs_arguments_object: bool,
    pub private_brand_requirement: PrivateBrandRequirement,
    pub needs_class_field_initializer: NeedsClassFieldInitializer,
}

#[derive(Clone, Copy, Debug, Default, Eq, PartialEq)]
pub struct ExecutableEvalParseMetadata {
    pub direct: bool,
    pub inherits_private_name_environment: bool,
    pub disables_cache: bool,
}

#[derive(Clone, Copy, Debug, Default, Eq, PartialEq)]
pub struct ExecutableModuleParseMetadata {
    pub requested_module_count: u32,
    pub import_count: u32,
    pub local_export_count: u32,
    pub re_export_count: u32,
    pub has_top_level_await: bool,
}

#[derive(Clone, Debug)]
pub struct FunctionExecutable {
    pub script: ScriptExecutable,
    pub unlinked: Arc<UnlinkedFunctionExecutable>,
    pub call_code: Option<CodeBlock>,
    pub construct_code: Option<CodeBlock>,
    pub rare: FunctionExecutableRareData,
}

impl FunctionExecutable {
    pub fn code_block_for(&self, specialization: CodeSpecialization) -> Option<&CodeBlock> {
        match specialization {
            CodeSpecialization::Call => self.call_code.as_ref(),
            CodeSpecialization::Construct => self.construct_code.as_ref(),
            CodeSpecialization::None => self.call_code.as_ref().or(self.construct_code.as_ref()),
        }
    }

    pub fn entry_metadata_for(
        &self,
        specialization: CodeSpecialization,
    ) -> Option<ExecutableEntryMetadata> {
        let code_block = self.code_block_for(specialization)?;
        Some(ExecutableEntryMetadata::from_code_block(
            self.script.base.identity,
            self.script.kind,
            specialization,
            self.unlinked.parameter_count_excluding_this,
            self.unlinked.executable_info.clone(),
            code_block,
        ))
    }

    pub fn publish_baseline_native_entry_for(
        &mut self,
        request: ExecutableEntryPublicationRequest,
    ) -> Result<ExecutableEntryCacheRecord, ExecutableEntryPublicationError> {
        let key = request.key();
        let FunctionExecutable {
            script,
            call_code,
            construct_code,
            ..
        } = self;
        let code_block = match key.specialization {
            CodeSpecializationKind::Call => call_code.as_ref(),
            CodeSpecializationKind::Construct => construct_code.as_ref(),
        }
        .ok_or(ExecutableEntryPublicationError::InstalledCodeBlockMissing {
            specialization: key.specialization,
        })?;

        if let (Some(expected), Some(actual)) = (script.base.identity, request.executable) {
            if expected != actual {
                return Err(
                    ExecutableEntryPublicationError::ExecutableIdentityMismatch {
                        expected,
                        actual,
                    },
                );
            }
        }

        let installed_code_block = request.launch_descriptor.code_block;
        let executable = script.base.identity.or(request.executable);
        let request = request.with_executable(executable);
        script.base.entrypoints.publish_baseline_native_entry(
            request,
            installed_code_block,
            code_block,
        )
    }
}

#[derive(Clone, Debug, Eq, PartialEq)]
pub struct ExecutableEntryMetadata {
    pub executable: Option<ExecutableId>,
    pub script_kind: ScriptExecutableKind,
    pub code_kind: CodeKind,
    pub specialization: CodeSpecialization,
    pub entrypoint: Option<InterpreterEntrypointSlot>,
    pub parameter_count_excluding_this: u32,
    pub parse_mode: Option<ParseMode>,
    pub script_mode: Option<ScriptMode>,
    pub source: SourceProvenance,
    pub features: CodeFeatures,
    pub unlinked_phase: crate::bytecode::UnlinkedCodeBlockPhase,
    pub linked_lifecycle: crate::bytecode::CodeBlockLifecycleState,
}

impl ExecutableEntryMetadata {
    pub fn from_code_block(
        executable: Option<ExecutableId>,
        script_kind: ScriptExecutableKind,
        specialization: CodeSpecialization,
        parameter_count_excluding_this: u32,
        executable_info: ExecutableInfo,
        code_block: &CodeBlock,
    ) -> Self {
        Self {
            executable,
            script_kind,
            code_kind: code_block.unlinked().kind(),
            specialization,
            entrypoint: code_block
                .entrypoints()
                .interpreter
                .map(|slot| InterpreterEntrypointSlot(slot.0)),
            parameter_count_excluding_this,
            parse_mode: executable_info.parse_mode,
            script_mode: executable_info.script_mode,
            source: code_block.unlinked().source().clone(),
            features: code_block.unlinked().features(),
            unlinked_phase: code_block.unlinked().phase(),
            linked_lifecycle: code_block.lifecycle(),
        }
    }

    pub fn is_interpreter_callable(&self) -> bool {
        self.entrypoint.is_some()
            && matches!(
                self.linked_lifecycle,
                crate::bytecode::CodeBlockLifecycleState::LinkedInterpreter
                    | crate::bytecode::CodeBlockLifecycleState::BaselineInstalled
                    | crate::bytecode::CodeBlockLifecycleState::OptimizingInstalled
            )
    }
}

#[derive(Clone, Debug, Default)]
pub struct FunctionExecutableRareData {
    pub override_line_number: Option<i32>,
    pub return_statement_type_set: Option<TypeSetRef>,
    pub class_source: Option<SourceRange>,
}

#[derive(Clone, Copy, Debug, Eq, PartialEq, Hash)]
#[repr(transparent)]
pub struct TypeSetRef(pub u32);

#[derive(Clone, Debug)]
pub struct UnlinkedFunctionExecutable {
    pub name_hint: Option<Identifier>,
    pub ecma_name: Option<Identifier>,
    pub executable_info: ExecutableInfo,
    pub source: SourceProvenance,
    pub function_source: SourceRange,
    pub body_source: SourceRange,
    pub parameters_start: Option<SourcePosition>,
    pub parameter_count_excluding_this: u32,
    pub line_count: u32,
    pub function_mode: FunctionMode,
    pub unlinked_kind: UnlinkedFunctionKind,
    pub construct_ability: ConstructAbility,
    pub call_code: Option<Arc<UnlinkedCodeBlock>>,
    pub construct_code: Option<Arc<UnlinkedCodeBlock>>,
    pub rare: UnlinkedFunctionExecutableRareData,
}

impl UnlinkedFunctionExecutable {
    pub fn code_for(&self, specialization: CodeSpecialization) -> Option<&Arc<UnlinkedCodeBlock>> {
        match specialization {
            CodeSpecialization::Call => self.call_code.as_ref(),
            CodeSpecialization::Construct => self.construct_code.as_ref(),
            CodeSpecialization::None => self.call_code.as_ref().or(self.construct_code.as_ref()),
        }
    }
}

#[derive(Clone, Copy, Debug, Default, Eq, PartialEq, Hash)]
pub enum FunctionMode {
    #[default]
    Normal,
    Generator,
    Async,
    AsyncGenerator,
    ClassConstructor,
    Method,
}

#[derive(Clone, Copy, Debug, Default, Eq, PartialEq, Hash)]
pub enum UnlinkedFunctionKind {
    #[default]
    Normal,
    Builtin,
}

#[derive(Clone, Copy, Debug, Default, Eq, PartialEq, Hash)]
pub enum ConstructAbility {
    #[default]
    CannotConstruct,
    CanConstruct,
}

/// Rare source and environment data for nested function executables.
///
/// The unlinked executable owns source offsets and parse metadata, but parent
/// TDZ/private-name environments remain abstract handles so bytecompiler and VM
/// code decide when those environments can be materialized or cached.
#[derive(Clone, Debug, Default, Eq, PartialEq)]
pub struct UnlinkedFunctionExecutableRareData {
    pub class_source: Option<SourceRange>,
    pub source_url_directive: Option<String>,
    pub source_mapping_url_directive: Option<String>,
    pub parent_tdz_environment: Option<ParentTdzEnvironmentRef>,
    pub generator_or_async_wrapper_parameter_names: Vec<Identifier>,
    pub parent_private_name_environment: Option<ParentPrivateNameEnvironmentRef>,
    pub class_element_definitions: Vec<ClassElementDefinition>,
    pub singleton_state: FunctionSingletonState,
}

#[derive(Clone, Copy, Debug, Eq, PartialEq, Hash)]
#[repr(transparent)]
pub struct ParentTdzEnvironmentRef(pub u32);

#[derive(Clone, Copy, Debug, Eq, PartialEq, Hash)]
#[repr(transparent)]
pub struct ParentPrivateNameEnvironmentRef(pub u32);

#[derive(Clone, Debug, Eq, PartialEq)]
pub struct ClassElementDefinition {
    pub name: Option<Identifier>,
    pub position: SourcePosition,
    pub initializer_position: Option<SourcePosition>,
    pub kind: ClassElementDefinitionKind,
}

#[derive(Clone, Copy, Debug, Eq, PartialEq, Hash)]
pub enum ClassElementDefinitionKind {
    FieldWithLiteralPropertyKey,
    FieldWithComputedPropertyKey,
    FieldWithPrivatePropertyKey,
    StaticInitializationBlock,
}

#[derive(Clone, Copy, Debug, Default, Eq, PartialEq, Hash)]
pub enum FunctionSingletonState {
    #[default]
    Unknown,
    Valid,
    Invalidated,
}

/// Fully resolved executable-info input before bit packing.
#[derive(Clone, Copy, Debug, Default, Eq, PartialEq, Hash)]
pub struct ExecutableInfoPlan {
    pub is_constructor: bool,
    pub private_brand_requirement: PrivateBrandRequirement,
    pub needs_class_field_initializer: NeedsClassFieldInitializer,
    pub constructor_kind: crate::bytecode::ConstructorKind,
    pub derived_context: DerivedContextType,
    pub eval_context_type: EvalContextType,
    pub script_mode: ScriptMode,
    pub parse_mode: ParseMode,
    pub function_mode: FunctionMode,
    pub construct_ability: ConstructAbility,
}

/// Source/unlinked-code reuse boundary.
#[derive(Clone, Debug, Default)]
pub struct CodeCache {
    pub entries: Vec<CodeCacheEntry>,
    pub policy: CodeCachePolicy,
}

#[derive(Clone, Debug)]
pub struct CodeCacheEntry {
    pub key: SourceCodeKey,
    pub value: CachedCodeValue,
    pub age: u8,
}

#[derive(Clone, Debug, Eq, PartialEq, Hash)]
pub struct SourceCodeKey {
    pub provider_id: Option<SourceProviderId>,
    pub source_hash: u64,
    pub code_kind: CodeKind,
    pub parse_mode: ParseMode,
    pub script_mode: ScriptMode,
    pub generation_modes: CodeGenerationModeSet,
    pub schema_fingerprint: u64,
}

#[derive(Clone, Debug)]
pub enum CachedCodeValue {
    Program(Arc<UnlinkedCodeBlock>),
    Eval(Arc<UnlinkedCodeBlock>),
    Module(Arc<UnlinkedCodeBlock>),
    Function(Arc<UnlinkedFunctionExecutable>),
    Serialized(CachedBytecodeRef),
}

#[derive(Clone, Copy, Debug, Eq, PartialEq, Hash)]
#[repr(transparent)]
pub struct CachedBytecodeRef(pub u32);

#[derive(Clone, Debug, Eq, PartialEq)]
pub struct CodeCachePolicy {
    pub max_age: u8,
    pub allow_disk_cache: bool,
    pub allow_direct_eval_cache: bool,
}

impl Default for CodeCachePolicy {
    fn default() -> Self {
        Self {
            max_age: 7,
            allow_disk_cache: false,
            allow_direct_eval_cache: false,
        }
    }
}

/// Reserved attachment point for JIT/tiering metadata.
#[derive(Clone, Debug, Default)]
pub struct DeferredTieringSlots {
    pub baseline_jit_data: Option<TieringSlot>,
    pub dfg_jit_data: Option<TieringSlot>,
    pub ftl_jit_data: Option<TieringSlot>,
    pub incoming_calls: Vec<TieringSlot>,
}

#[derive(Clone, Copy, Debug, Eq, PartialEq, Hash)]
#[repr(transparent)]
pub struct TieringSlot(pub u32);

#[cfg(test)]
mod tests {
    use super::*;
    use crate::bytecode::instruction::InstructionBuilder;
    use crate::bytecode::{
        CodeBlockEntrypoints, CodeBlockLifecycleState, InterpreterEntrySlot, LinkContext, Opcode,
        OperandWidth,
    };
    use crate::gc::CellId;
    use crate::jit::{
        CodeFinalizationAuthority, CodeLiveness, CodeOrigin, CodeOriginKind, CodeOwnership,
        EntryAbi, Entrypoint, EntrypointKind, ExecutableAllocationId,
        ExecutableAllocationLifecycle, ExecutableMemoryProtection, ExecutableMutationAuthority,
        JitCodeArtifact, JitType, MachineCodeHandle, MachineCodeOwnership, MachineCodeRange,
        TierFallbackReason,
    };
    use crate::runtime::{CallFrameId, EntryFrameId, RuntimeValue};
    use crate::vm::{
        BaselineEntryGateOutcome, VmEntryCallFrameMetadata, VmEntryLaunchArgumentValue,
        VmEntryLaunchFallbackRoute, VmEntryLaunchScope, VmEntryThrowRoute,
    };

    #[test]
    fn executable_entry_metadata_reflects_interpreter_entry_surface() {
        let mut builder = InstructionBuilder::new();
        builder.declare_instruction(Opcode::Reserved, OperandWidth::Narrow, Vec::new());
        let unlinked = UnlinkedCodeBlock::new(CodeKind::Function, builder.finalize())
            .with_source(SourceProvenance {
                start_offset: 3,
                source_length: 10,
                ..SourceProvenance::default()
            })
            .with_features(CodeFeatures {
                uses_arguments: true,
                ..CodeFeatures::default()
            });
        let code_block = CodeBlock::from_unlinked(unlinked, LinkContext::default())
            .with_entrypoints(CodeBlockEntrypoints {
                interpreter: Some(InterpreterEntrySlot(5)),
                ..CodeBlockEntrypoints::default()
            })
            .with_lifecycle(CodeBlockLifecycleState::LinkedInterpreter);
        let info = ExecutableInfo {
            parse_mode: Some(ParseMode::NormalFunction),
            script_mode: Some(ScriptMode::Classic),
            ..ExecutableInfo::default()
        };

        let metadata = ExecutableEntryMetadata::from_code_block(
            Some(ExecutableId::default()),
            ScriptExecutableKind::Function,
            CodeSpecialization::Call,
            2,
            info,
            &code_block,
        );

        assert_eq!(metadata.entrypoint, Some(InterpreterEntrypointSlot(5)));
        assert_eq!(metadata.parameter_count_excluding_this, 2);
        assert!(metadata.features.uses_arguments);
        assert!(metadata.is_interpreter_callable());
    }

    #[test]
    fn baseline_native_publication_returns_normal_call_token_without_caching_already_checked() {
        let owner = CodeBlockId(CellId(41));
        let executable = Some(ExecutableId(CellId(42)));
        let slot = JitCodeSlot(7);
        let descriptor = baseline_native_descriptor(owner, 401);
        let launch = launch_descriptor(
            owner,
            descriptor,
            CodeSpecializationKind::Call,
            ArityCheckMode::AlreadyChecked,
            9,
        );
        let request = ExecutableEntryPublicationRequest::from_launch_descriptor(launch)
            .with_executable(executable)
            .with_baseline_jit_slot(slot);
        let code_block = installed_code_block(
            executable,
            CodeSpecialization::Call,
            slot,
            CodeBlockLifecycleState::BaselineInstalled,
        );
        let mut entrypoints = ExecutableEntrypoints::default();

        let record = entrypoints
            .publish_baseline_native_entry(request, owner, &code_block)
            .expect("normal call publication");

        assert_eq!(
            record.key,
            ExecutableEntryCacheKey::new(
                CodeSpecializationKind::Call,
                ExecutableAritySelection::AlreadyChecked
            )
        );
        assert_eq!(record.owner, owner);
        assert_eq!(record.code_block, owner);
        assert_eq!(record.executable, executable);
        assert_eq!(record.readiness_ordinal, 9);
        assert_eq!(record.baseline_jit_slot, slot);
        assert_eq!(
            record.baseline_native_entry.selection,
            ExecutableBaselineNativeEntrySelection::Normal(descriptor.normal_entry)
        );
        assert!(entrypoints
            .call
            .cached_baseline_arity_publication()
            .is_none());
        assert!(entrypoints
            .construct
            .cached_baseline_arity_publication()
            .is_none());
    }

    #[test]
    fn baseline_native_publication_accepts_unspecialized_top_level_call_code_block() {
        let owner = CodeBlockId(CellId(45));
        let slot = JitCodeSlot(12);
        let descriptor = baseline_native_descriptor(owner, 451);
        let launch = launch_descriptor(
            owner,
            descriptor,
            CodeSpecializationKind::Call,
            ArityCheckMode::AlreadyChecked,
            14,
        );
        let request = ExecutableEntryPublicationRequest::from_launch_descriptor(launch)
            .with_baseline_jit_slot(slot);
        let code_block = installed_code_block(
            None,
            CodeSpecialization::None,
            slot,
            CodeBlockLifecycleState::BaselineInstalled,
        );
        let mut entrypoints = ExecutableEntrypoints::default();

        let record = entrypoints
            .publish_baseline_native_entry(request, owner, &code_block)
            .expect("top-level call publication");

        assert_eq!(record.key.specialization, CodeSpecializationKind::Call);
        assert_eq!(
            record.baseline_native_entry.selection,
            ExecutableBaselineNativeEntrySelection::Normal(descriptor.normal_entry)
        );
        assert!(entrypoints
            .call
            .cached_baseline_arity_publication()
            .is_none());
    }

    #[test]
    fn baseline_native_publication_records_arity_unavailable_without_failing() {
        let owner = CodeBlockId(CellId(51));
        let slot = JitCodeSlot(8);
        let descriptor = baseline_native_descriptor(owner, 501);
        let launch = launch_descriptor(
            owner,
            descriptor,
            CodeSpecializationKind::Call,
            ArityCheckMode::MustCheckArity,
            10,
        );
        let request = ExecutableEntryPublicationRequest::from_launch_descriptor(launch)
            .with_baseline_jit_slot(slot);
        let code_block = installed_code_block(
            None,
            CodeSpecialization::Call,
            slot,
            CodeBlockLifecycleState::BaselineInstalled,
        );
        let mut entrypoints = ExecutableEntrypoints::default();

        let record = entrypoints
            .publish_baseline_native_entry(request, owner, &code_block)
            .expect("arity-unavailable publication");

        assert_eq!(record.key.arity, ExecutableAritySelection::MustCheckArity);
        assert_eq!(
            record.baseline_native_entry.selection,
            ExecutableBaselineNativeEntrySelection::ArityCheckUnavailable(
                BaselineArityCheckUnavailableReason::NotEmitted
            )
        );
        assert_eq!(
            entrypoints.call.cached_baseline_arity_publication(),
            Some(&record)
        );
    }

    #[test]
    fn baseline_native_publication_rejects_owner_executable_and_lifecycle_mismatch() {
        let owner = CodeBlockId(CellId(61));
        let wrong_owner = CodeBlockId(CellId(62));
        let executable_id = ExecutableId(CellId(63));
        let executable = Some(executable_id);
        let wrong_executable_id = ExecutableId(CellId(64));
        let wrong_executable = Some(wrong_executable_id);
        let slot = JitCodeSlot(9);
        let descriptor = baseline_native_descriptor(owner, 601);
        let launch = launch_descriptor(
            owner,
            descriptor,
            CodeSpecializationKind::Call,
            ArityCheckMode::AlreadyChecked,
            11,
        );
        let request = ExecutableEntryPublicationRequest::from_launch_descriptor(launch)
            .with_executable(executable)
            .with_baseline_jit_slot(slot);
        let code_block = installed_code_block(
            executable,
            CodeSpecialization::Call,
            slot,
            CodeBlockLifecycleState::BaselineInstalled,
        );
        let mut entrypoints = ExecutableEntrypoints::default();

        assert_eq!(
            entrypoints.publish_baseline_native_entry(request, wrong_owner, &code_block),
            Err(
                ExecutableEntryPublicationError::InstalledCodeBlockMismatch {
                    expected: owner,
                    actual: wrong_owner,
                }
            )
        );

        let mismatched_executable_code_block = installed_code_block(
            wrong_executable,
            CodeSpecialization::Call,
            slot,
            CodeBlockLifecycleState::BaselineInstalled,
        );
        assert_eq!(
            entrypoints.publish_baseline_native_entry(
                request,
                owner,
                &mismatched_executable_code_block
            ),
            Err(
                ExecutableEntryPublicationError::ExecutableIdentityMismatch {
                    expected: executable_id,
                    actual: wrong_executable_id,
                }
            )
        );

        let linked_code_block = installed_code_block(
            executable,
            CodeSpecialization::Call,
            slot,
            CodeBlockLifecycleState::LinkedInterpreter,
        );
        assert_eq!(
            entrypoints.publish_baseline_native_entry(request, owner, &linked_code_block),
            Err(
                ExecutableEntryPublicationError::CodeBlockLifecycleMismatch {
                    expected: CodeBlockLifecycleState::BaselineInstalled,
                    actual: CodeBlockLifecycleState::LinkedInterpreter,
                }
            )
        );
    }

    #[test]
    fn baseline_native_publication_keeps_call_and_construct_records_separate() {
        let executable = Some(ExecutableId(CellId(70)));
        let call_owner = CodeBlockId(CellId(71));
        let construct_owner = CodeBlockId(CellId(72));
        let call_slot = JitCodeSlot(10);
        let construct_slot = JitCodeSlot(11);
        let call_descriptor = baseline_native_descriptor(call_owner, 701);
        let construct_descriptor = baseline_native_descriptor(construct_owner, 702);
        let call_code_block = installed_code_block(
            executable,
            CodeSpecialization::Call,
            call_slot,
            CodeBlockLifecycleState::BaselineInstalled,
        );
        let construct_code_block = installed_code_block(
            executable,
            CodeSpecialization::Construct,
            construct_slot,
            CodeBlockLifecycleState::BaselineInstalled,
        );
        let mut entrypoints = ExecutableEntrypoints::default();

        let call_record = entrypoints
            .publish_baseline_native_entry(
                ExecutableEntryPublicationRequest::from_launch_descriptor(launch_descriptor(
                    call_owner,
                    call_descriptor,
                    CodeSpecializationKind::Call,
                    ArityCheckMode::MustCheckArity,
                    12,
                ))
                .with_executable(executable)
                .with_baseline_jit_slot(call_slot),
                call_owner,
                &call_code_block,
            )
            .expect("call publication");
        let construct_record = entrypoints
            .publish_baseline_native_entry(
                ExecutableEntryPublicationRequest::from_launch_descriptor(launch_descriptor(
                    construct_owner,
                    construct_descriptor,
                    CodeSpecializationKind::Construct,
                    ArityCheckMode::MustCheckArity,
                    13,
                ))
                .with_executable(executable)
                .with_baseline_jit_slot(construct_slot),
                construct_owner,
                &construct_code_block,
            )
            .expect("construct publication");

        assert_eq!(
            entrypoints.call.cached_baseline_arity_publication(),
            Some(&call_record)
        );
        assert_eq!(
            entrypoints.construct.cached_baseline_arity_publication(),
            Some(&construct_record)
        );
        assert_eq!(
            call_record.baseline_native_entry.selection,
            ExecutableBaselineNativeEntrySelection::ArityCheckUnavailable(
                BaselineArityCheckUnavailableReason::NotEmitted
            )
        );
        assert_eq!(
            construct_record.baseline_native_entry.selection,
            ExecutableBaselineNativeEntrySelection::ArityCheckUnavailable(
                BaselineArityCheckUnavailableReason::NotEmitted
            )
        );
    }

    fn installed_code_block(
        executable: Option<ExecutableId>,
        specialization: CodeSpecialization,
        slot: JitCodeSlot,
        lifecycle: CodeBlockLifecycleState,
    ) -> CodeBlock {
        let mut builder = InstructionBuilder::new();
        builder.declare_instruction(Opcode::Reserved, OperandWidth::Narrow, Vec::new());
        let unlinked = UnlinkedCodeBlock::new(CodeKind::Function, builder.finalize());
        CodeBlock::from_unlinked(
            unlinked,
            LinkContext {
                owner_executable: executable,
                specialization,
                ..LinkContext::default()
            },
        )
        .with_entrypoints(CodeBlockEntrypoints {
            baseline_jit: Some(slot),
            ..CodeBlockEntrypoints::default()
        })
        .with_lifecycle(lifecycle)
    }

    fn launch_descriptor(
        owner: CodeBlockId,
        descriptor: BaselineNativeEntryDescriptor,
        specialization: CodeSpecializationKind,
        arity_mode: ArityCheckMode,
        readiness_ordinal: u64,
    ) -> VmEntryLaunchDescriptor {
        VmEntryLaunchDescriptor {
            owner,
            code_block: owner,
            scope: VmEntryLaunchScope {
                owner,
                entry_code_block: Some(owner),
                active_entry_frame: Some(EntryFrameId(1)),
                previous_entry_frame: None,
                saved_top_call_frame: None,
                active_top_call_frame: Some(CallFrameId(2)),
            },
            call_frame: VmEntryCallFrameMetadata {
                frame: CallFrameId(2),
                entry_frame: Some(EntryFrameId(1)),
                caller_frame: None,
                code_block: Some(owner),
                callee: None,
                callee_value: None,
                context: None,
                global_object: None,
                entry_value: VmEntryLaunchArgumentValue::This(RuntimeValue::undefined()),
                argument_count_including_this: 1,
                provided_argument_count: 0,
                padded_argument_count: 1,
                specialization,
                arity_mode,
            },
            baseline_entry_gate: crate::vm::BaselineEntryGateRecord {
                owner,
                requested_tier: JitType::Baseline,
                native_artifact: None,
                native_entry_readiness_ordinal: Some(readiness_ordinal),
                generated_artifact: None,
                outcome: BaselineEntryGateOutcome::NativeEntryReadyButExecutionDisabled,
            },
            readiness_ordinal,
            readiness_bytecode_snapshot_present: false,
            native_entry: descriptor,
            dispatch: VmEntryDispatchSelection::BaselineNative(
                BaselineNativeDispatchTokenSelection::select(descriptor, arity_mode),
            ),
            fallback_route: VmEntryLaunchFallbackRoute {
                reason: TierFallbackReason::NativeEntryDisabled,
                throw_route: VmEntryThrowRoute::InterpreterExceptionCheck,
            },
        }
    }

    fn baseline_native_descriptor(owner: CodeBlockId, id: u64) -> BaselineNativeEntryDescriptor {
        baseline_artifact(owner, id)
            .validate_baseline_entry_artifact(owner)
            .expect("baseline entry artifact")
            .validate_native_entry_descriptor()
            .expect("native entry descriptor")
    }

    fn baseline_artifact(owner: CodeBlockId, id: u64) -> JitCodeArtifact {
        let code = JitCodeId(id);
        let native_code = NativeCodeId(id as u32 + 100);
        let allocation = ExecutableAllocationId(id + 200);
        JitCodeArtifact {
            id: code,
            tier: JitType::Baseline,
            origin: CodeOrigin {
                kind: CodeOriginKind::BaselineCodeBlock,
                owner: Some(owner),
                executable: None,
                bytecode_index: Some(0),
            },
            ownership: CodeOwnership::CodeBlockOwned,
            native_code: Some(native_code),
            machine_code: Some(MachineCodeHandle {
                allocation,
                owner: MachineCodeOwnership::CodeBlock(owner),
                range: MachineCodeRange {
                    allocation,
                    start_offset: 0,
                    size_bytes: 64,
                },
                symbol: Some(native_code),
                protection: ExecutableMemoryProtection::Executable,
                lifecycle: ExecutableAllocationLifecycle::LinkedExecutable,
                mutation_authority: ExecutableMutationAuthority::LinkBuffer,
            }),
            entrypoint: Entrypoint {
                kind: EntrypointKind::GeneratedCode,
                abi: EntryAbi::GeneratedCode,
                code: Some(code),
                boundary: None,
            },
            patchpoints: Vec::new(),
            dependencies: Vec::new(),
            byproducts: Vec::new(),
            disassembly: None,
            liveness: CodeLiveness::Live,
            finalization_authority: CodeFinalizationAuthority::MainThread,
        }
    }
}
