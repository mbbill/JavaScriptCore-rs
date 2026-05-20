use crate::runtime::scope::ScopeId;
use crate::runtime::state::{
    CodeBlockId, ExecutableId, NativeCodeId, ObjectId, RuntimeValue, SourceProviderId, StringId,
    StructureId, WatchpointGeneration,
};

/// Function object contract.
///
/// `JsFunction` is eventually a GC cell with executable and captured-scope
/// edges. Lazy function data and allocation profiles must mutate through
/// VM/write-barrier APIs.
#[derive(Clone, Debug, Default)]
pub struct JsFunction {
    pub identity: FunctionIdentity,
    pub executable: FunctionExecutableLink,
    pub scope: FunctionScopeLink,
    pub allocation: FunctionAllocationMetadata,
    pub lazy_properties: FunctionLazyProperties,
    pub rare_data: Option<FunctionRareData>,
}

impl JsFunction {
    pub fn call_data(&self) -> CallData {
        match &self.executable {
            FunctionExecutableLink::Native(executable) => CallData::Native(NativeCallTarget {
                native_id: NativeCallId(executable.0 .0 .0),
                executable: Some(*executable),
                is_bound_function: self.identity.kind == FunctionObjectKind::Bound,
                is_wasm_entry: false,
            }),
            FunctionExecutableLink::Script(executable) => {
                let Some(scope) = self.scope.captured_scope.or(self.scope.realm_scope) else {
                    return CallData::None;
                };
                CallData::JavaScript(JavaScriptCallTarget {
                    executable: *executable,
                    scope,
                    code_block: None,
                })
            }
            FunctionExecutableLink::RareData(_) => self
                .rare_data
                .as_ref()
                .and_then(|rare| rare.executable)
                .and_then(|executable| {
                    self.scope
                        .captured_scope
                        .or(self.scope.realm_scope)
                        .map(|scope| (executable, scope))
                })
                .map(|(executable, scope)| {
                    CallData::JavaScript(JavaScriptCallTarget {
                        executable,
                        scope,
                        code_block: None,
                    })
                })
                .unwrap_or(CallData::None),
            FunctionExecutableLink::Empty => CallData::None,
        }
    }

    pub fn construct_data(&self, metadata: FunctionExecutableMetadata) -> ConstructData {
        if metadata.construct_ability == ConstructAbility::NotConstructable
            || metadata.constructor_kind == ConstructorKind::None
        {
            return ConstructData::None;
        }

        match &self.executable {
            FunctionExecutableLink::Native(executable) => ConstructData::Native(NativeCallTarget {
                native_id: NativeCallId(executable.0 .0 .0),
                executable: Some(*executable),
                is_bound_function: self.identity.kind == FunctionObjectKind::Bound,
                is_wasm_entry: false,
            }),
            FunctionExecutableLink::Script(executable) => {
                let Some(scope) = self.scope.captured_scope.or(self.scope.realm_scope) else {
                    return ConstructData::None;
                };
                ConstructData::JavaScript(JavaScriptConstructTarget {
                    executable: *executable,
                    scope,
                    constructor_kind: metadata.constructor_kind,
                    code_block: None,
                })
            }
            FunctionExecutableLink::RareData(_) => self
                .rare_data
                .as_ref()
                .and_then(|rare| rare.executable)
                .and_then(|executable| {
                    self.scope
                        .captured_scope
                        .or(self.scope.realm_scope)
                        .map(|scope| (executable, scope))
                })
                .map(|(executable, scope)| {
                    ConstructData::JavaScript(JavaScriptConstructTarget {
                        executable,
                        scope,
                        constructor_kind: metadata.constructor_kind,
                        code_block: None,
                    })
                })
                .unwrap_or(ConstructData::None),
            FunctionExecutableLink::Empty => ConstructData::None,
        }
    }
}

#[derive(Clone, Copy, Debug, Default, Eq, PartialEq, Hash)]
pub struct FunctionId(pub ObjectId);

#[derive(Clone, Copy, Debug, Default, Eq, PartialEq, Hash)]
pub struct FunctionExecutableId(pub ExecutableId);

#[derive(Clone, Debug, Default)]
pub struct FunctionIdentity {
    pub function_id: Option<FunctionId>,
    pub name: Option<StringId>,
    pub display_name: Option<StringId>,
    pub original_length: u32,
    pub kind: FunctionObjectKind,
}

/// Distinguishes the object shape from the executable payload.
///
/// JavaScriptCore stores strict, sloppy, method, arrow, bound, host, and
/// function-with-fields objects in separate structures. The Rust skeleton keeps
/// that classification on the function object, while bytecode/native entry
/// information lives in the executable link.
#[derive(Clone, Copy, Debug, Default, Eq, PartialEq, Hash)]
pub enum FunctionObjectKind {
    #[default]
    Ordinary,
    Strict,
    Method,
    Arrow,
    Bound,
    Host,
    Builtin,
    ClassConstructor,
    Remote,
    WithFields,
}

/// Edge from a function object to either a script executable or native body.
#[derive(Clone, Debug, Default, Eq, PartialEq)]
pub enum FunctionExecutableLink {
    #[default]
    Empty,
    Script(FunctionExecutableId),
    Native(NativeExecutableId),
    RareData(FunctionRareDataId),
}

#[derive(Clone, Copy, Debug, Default, Eq, PartialEq, Hash)]
pub struct NativeExecutableId(pub ExecutableId);

#[derive(Clone, Copy, Debug, Default, Eq, PartialEq, Hash)]
pub struct FunctionRareDataId(pub u32);

#[derive(Clone, Debug, Default, Eq, PartialEq)]
pub struct FunctionScopeLink {
    pub captured_scope: Option<ScopeId>,
    pub realm_scope: Option<ScopeId>,
}

#[derive(Clone, Debug, Default)]
pub struct FunctionLazyProperties {
    pub length: LazyFunctionProperty,
    pub name: LazyFunctionProperty,
    pub prototype: LazyFunctionProperty,
}

#[derive(Clone, Copy, Debug, Default, Eq, PartialEq, Hash)]
pub enum LazyFunctionProperty {
    #[default]
    Absent,
    Eager,
    Lazy,
    Reified,
}

#[derive(Clone, Debug, Default)]
pub struct FunctionRareData {
    pub executable: Option<FunctionExecutableId>,
    pub allocation_profile: Option<AllocationProfile>,
    pub bound_target: Option<FunctionId>,
    pub cached_string_representation: Option<StringId>,
}

#[derive(Clone, Debug, Eq, PartialEq)]
pub enum CallData {
    None,
    Native(NativeCallTarget),
    JavaScript(JavaScriptCallTarget),
}

#[derive(Clone, Debug, Eq, PartialEq)]
pub enum ConstructData {
    None,
    Native(NativeCallTarget),
    JavaScript(JavaScriptConstructTarget),
}

#[derive(Clone, Copy, Debug, Eq, PartialEq, Hash)]
pub struct NativeCallId(pub u32);

#[derive(Clone, Debug, Eq, PartialEq)]
pub struct NativeCallTarget {
    pub native_id: NativeCallId,
    pub executable: Option<NativeExecutableId>,
    pub is_bound_function: bool,
    pub is_wasm_entry: bool,
}

#[derive(Clone, Debug, Eq, PartialEq)]
pub struct JavaScriptCallTarget {
    /// Linked executable and captured scope are resolved before interpreter entry.
    ///
    /// The optional code block records a prepared consumer-facing target, not a
    /// request for this module to compile or select one.
    pub executable: FunctionExecutableId,
    pub scope: ScopeId,
    pub code_block: Option<CodeBlockId>,
}

#[derive(Clone, Debug, Eq, PartialEq)]
pub struct JavaScriptConstructTarget {
    /// Construct metadata mirrors call metadata but keeps constructor semantics
    /// explicit for derived constructors and `new.target` handling.
    pub executable: FunctionExecutableId,
    pub scope: ScopeId,
    pub constructor_kind: ConstructorKind,
    pub code_block: Option<CodeBlockId>,
}

/// Borrowed argument view. Values must be rooted or frame-visible.
#[derive(Clone, Debug, Default)]
pub struct ArgList {
    pub value_count: usize,
    pub has_this_value: bool,
}

/// Rooted argument storage for APIs that can allocate or call into user code.
#[derive(Clone, Debug, Default)]
pub struct MarkedArgList {
    pub value_count: usize,
    pub capacity: usize,
    pub owner_frame: Option<crate::runtime::interpreter::CallFrameId>,
}

/// Execution-time argument values prepared for a call boundary.
///
/// The values are copied transport bits. If a value names a cell, the caller or
/// VM frame must already make it visible to rooting and exception machinery.
#[derive(Clone, Debug, Default, Eq, PartialEq)]
pub struct ExecutionArgumentList {
    pub values: Vec<RuntimeValue>,
    pub source: ArgumentListSource,
    pub has_this_value: bool,
}

impl ExecutionArgumentList {
    pub fn new(values: Vec<RuntimeValue>, source: ArgumentListSource) -> Self {
        Self {
            values,
            source,
            has_this_value: false,
        }
    }

    pub fn with_this(mut self) -> Self {
        self.has_this_value = true;
        self
    }

    pub fn len(&self) -> usize {
        self.values.len()
    }

    pub fn is_empty(&self) -> bool {
        self.values.is_empty()
    }

    pub fn at_or_undefined(&self, index: usize) -> RuntimeValue {
        self.values
            .get(index)
            .copied()
            .unwrap_or_else(RuntimeValue::undefined)
    }
}

#[derive(Clone, Copy, Debug, Default, Eq, PartialEq, Hash)]
pub enum ArgumentListSource {
    #[default]
    Api,
    CallFrame,
    BoundFunction,
    Spread,
    Microtask,
    ModuleEvaluation,
}

#[derive(Clone, Debug, Eq, PartialEq)]
pub struct ArgumentBindingRecord {
    pub arguments: ExecutionArgumentList,
    pub expected_parameter_count: u32,
    pub missing_argument_count: u32,
    pub arity_check: ArityCheckMode,
}

pub fn bind_arguments_for_entry(
    arguments: ExecutionArgumentList,
    expected_parameter_count: u32,
    arity_check: ArityCheckMode,
) -> ArgumentBindingRecord {
    let actual = arguments.len().min(u32::MAX as usize) as u32;
    ArgumentBindingRecord {
        arguments,
        expected_parameter_count,
        missing_argument_count: expected_parameter_count.saturating_sub(actual),
        arity_check,
    }
}

#[derive(Clone, Debug, Eq, PartialEq)]
pub struct ThisBindingRecord {
    pub mode: ThisMode,
    pub raw_this: RuntimeValue,
    pub bound_this: Option<RuntimeValue>,
    pub source: ThisBindingSource,
    pub requires_global_substitution: bool,
}

#[derive(Clone, Copy, Debug, Default, Eq, PartialEq, Hash)]
pub enum ThisBindingSource {
    #[default]
    CallerProvided,
    BoundFunction,
    ConstructorAllocation,
    LexicalEnvironment,
    Module,
}

pub fn bind_this_for_entry(
    mode: ThisMode,
    raw_this: RuntimeValue,
    global_this: Option<RuntimeValue>,
    source: ThisBindingSource,
) -> ThisBindingRecord {
    let needs_global = matches!(mode, ThisMode::Global)
        && matches!(
            raw_this.kind(),
            crate::value::ValueKind::Undefined | crate::value::ValueKind::Null
        );
    let bound_this = match mode {
        ThisMode::Lexical => None,
        ThisMode::Strict => Some(raw_this),
        ThisMode::Global if needs_global => global_this,
        ThisMode::Global => Some(raw_this),
    };

    ThisBindingRecord {
        mode,
        raw_this,
        bound_this,
        source,
        requires_global_substitution: needs_global && global_this.is_none(),
    }
}

#[derive(Clone, Copy, Debug, Eq, PartialEq, Hash)]
pub enum CallMode {
    Regular,
    Tail,
    Construct,
    Varargs,
}

#[derive(Clone, Copy, Debug, Default, Eq, PartialEq, Hash)]
pub enum ConstructorKind {
    #[default]
    Base,
    Derived,
    None,
}

#[derive(Clone, Copy, Debug, Default, Eq, PartialEq, Hash)]
pub enum ConstructAbility {
    #[default]
    Constructable,
    NotConstructable,
}

#[derive(Clone, Copy, Debug, Default, Eq, PartialEq, Hash)]
pub enum ThisMode {
    Lexical,
    Strict,
    #[default]
    Global,
}

#[derive(Clone, Copy, Debug, Eq, PartialEq, Hash)]
pub enum CodeSpecializationKind {
    Call,
    Construct,
}

#[derive(Clone, Copy, Debug, Eq, PartialEq, Hash)]
pub enum ArityCheckMode {
    MustCheckArity,
    AlreadyChecked,
}

#[derive(Clone, Debug, Default)]
pub struct ExecutableBase {
    /// Common executable state shared by native, function, eval, program, and
    /// module executables. It owns entrypoint metadata, while function objects
    /// own the object identity and captured-scope edge.
    pub executable_id: Option<ExecutableId>,
    pub kind: ExecutableKind,
    pub entrypoints: ExecutableEntrypoints,
    pub metadata: ExecutableMetadata,
}

#[derive(Clone, Copy, Debug, Default, Eq, PartialEq, Hash)]
pub enum ExecutableKind {
    #[default]
    Native,
    Function,
    Program,
    Eval,
    ModuleProgram,
}

#[derive(Clone, Debug, Default)]
pub struct ExecutableEntrypoints {
    pub call: EntrypointState,
    pub construct: EntrypointState,
}

#[derive(Clone, Debug, Default)]
pub struct EntrypointState {
    pub generated_code: Option<NativeCodeId>,
    pub arity_checked_code: Option<NativeCodeId>,
    pub code_block: Option<CodeBlockId>,
}

#[derive(Clone, Debug, Default)]
pub struct ExecutableMetadata {
    pub intrinsic: Option<IntrinsicId>,
    pub implementation_visibility: ImplementationVisibility,
    pub inline_attribute: InlineAttribute,
}

#[derive(Clone, Copy, Debug, Default, Eq, PartialEq, Hash)]
pub struct IntrinsicId(pub u16);

#[derive(Clone, Copy, Debug, Default, Eq, PartialEq, Hash)]
pub enum ImplementationVisibility {
    #[default]
    Public,
    Private,
    Hidden,
}

#[derive(Clone, Copy, Debug, Default, Eq, PartialEq, Hash)]
pub enum InlineAttribute {
    #[default]
    None,
    Always,
    Never,
}

#[derive(Clone, Debug, Default)]
pub struct ScriptExecutable {
    /// Script-level source and feature metadata used by parsers, compilers,
    /// debuggers, and error reporting. It does not imply executable code exists.
    pub base: ExecutableBase,
    pub source: SourceDescriptor,
    pub features: ScriptFeatures,
    pub optimization: ScriptOptimizationState,
}

#[derive(Clone, Debug, Default)]
pub struct SourceDescriptor {
    pub provider: Option<SourceProviderId>,
    pub range: SourceRange,
    pub first_line: u32,
    pub start_column: u32,
    pub source_url: Option<StringId>,
}

#[derive(Clone, Copy, Debug, Default, Eq, PartialEq, Hash)]
pub struct SourceRange {
    pub start: u32,
    pub end: u32,
}

#[derive(Clone, Debug, Default, Eq, PartialEq)]
pub struct ScriptFeatures {
    pub uses_arguments: bool,
    pub has_captured_variables: bool,
    pub is_strict_context: bool,
    pub uses_non_simple_parameters: bool,
    pub is_arrow_context: bool,
    pub is_inside_ordinary_function: bool,
    pub derived_context: DerivedContextType,
    pub eval_context: EvalContextType,
}

#[derive(Clone, Copy, Debug, Default, Eq, PartialEq, Hash)]
pub enum DerivedContextType {
    #[default]
    None,
    DerivedConstructor,
    BaseConstructor,
}

#[derive(Clone, Copy, Debug, Default, Eq, PartialEq, Hash)]
pub enum EvalContextType {
    #[default]
    None,
    Direct,
    Indirect,
}

#[derive(Clone, Debug, Default, Eq, PartialEq)]
pub struct ScriptOptimizationState {
    pub never_inline: bool,
    pub never_optimize: bool,
    pub never_ftl_optimize: bool,
    pub did_try_to_enter_loop: bool,
}

#[derive(Clone, Debug, Default)]
pub struct FunctionExecutable {
    /// Linked function executable contract.
    ///
    /// `unlinked` names parse-time metadata, `code_blocks` names prepared
    /// runtime bodies, and rare data collects mutable caches that require
    /// owner-aware barriers or watchpoints when real GC cells exist.
    pub script: ScriptExecutable,
    pub unlinked: Option<UnlinkedFunctionExecutableId>,
    pub top_level_executable: Option<ExecutableId>,
    pub code_blocks: FunctionCodeBlocks,
    pub metadata: FunctionExecutableMetadata,
    pub rare_data: Option<FunctionExecutableRareData>,
    pub singleton_watchpoint: WatchpointGeneration,
}

#[derive(Clone, Copy, Debug, Default, Eq, PartialEq, Hash)]
pub struct UnlinkedFunctionExecutableId(pub u32);

#[derive(Clone, Debug, Default)]
pub struct FunctionCodeBlocks {
    pub call: Option<CodeBlockId>,
    pub construct: Option<CodeBlockId>,
    pub baseline_call: Option<CodeBlockId>,
    pub baseline_construct: Option<CodeBlockId>,
}

#[derive(Clone, Debug, Default, Eq, PartialEq)]
pub struct FunctionExecutableMetadata {
    pub parameter_count: u32,
    pub this_mode: ThisMode,
    pub constructor_kind: ConstructorKind,
    pub construct_ability: ConstructAbility,
    pub parse_mode: FunctionParseMode,
    pub is_builtin: bool,
    pub is_class: bool,
}

#[derive(Clone, Copy, Debug, Default, Eq, PartialEq, Hash)]
pub enum FunctionParseMode {
    #[default]
    Normal,
    Method,
    Getter,
    Setter,
    Arrow,
    Generator,
    Async,
    AsyncGenerator,
}

#[derive(Clone, Debug, Default)]
pub struct FunctionExecutableRareData {
    pub override_line_number: Option<u32>,
    pub line_count: u32,
    pub end_column: u32,
    pub function_start: u32,
    pub function_end: u32,
    pub parameters_start_offset: u32,
    pub cached_poly_proto_structure: Option<StructureId>,
    pub template_object_map_generation: u64,
}

#[derive(Clone, Debug, Default)]
pub struct CallLinkInfo {
    /// Runtime feedback and linked target state are owned by `CodeBlock`.
    pub generation: u64,
    pub owner_code_block: Option<CodeBlockId>,
    pub last_target: Option<FunctionId>,
    pub arity_check: Option<ArityCheckMode>,
}

#[derive(Clone, Debug, Default)]
pub struct AllocationProfile {
    /// Structure/prototype caches require object-model watchpoint contracts.
    pub inline_capacity: usize,
    pub structure: Option<StructureId>,
    pub prototype: Option<ObjectId>,
    pub watchpoint_generation: WatchpointGeneration,
}

#[derive(Clone, Debug, Default)]
pub struct FunctionAllocationMetadata {
    pub selected_structure: Option<StructureId>,
    pub allocation_profile: Option<AllocationProfile>,
    pub can_use_allocation_profiles: bool,
}

/// Host callback ABI boundary.
///
/// Real callbacks must define rooting, exception, reentrancy, and FFI lifetime
/// rules before this becomes callable.
#[derive(Clone, Debug, Eq, PartialEq)]
pub struct HostFunction {
    pub native_id: NativeCallId,
    pub call_entry: NativeCodeId,
    pub construct_entry: Option<NativeCodeId>,
    pub can_reenter_vm: bool,
}

#[derive(Clone, Debug, Eq, PartialEq)]
pub struct CallableEntryRecord {
    pub function_object: ObjectId,
    pub mode: CallMode,
    pub target: CallableEntryTarget,
    pub this_binding: ThisBindingRecord,
    pub arguments: ArgumentBindingRecord,
    pub specialization: CodeSpecializationKind,
    pub new_target: Option<RuntimeValue>,
}

#[derive(Clone, Debug, Eq, PartialEq)]
pub enum CallableEntryTarget {
    NativeCall(NativeCallTarget),
    ScriptCall(JavaScriptCallTarget),
    NativeConstruct(NativeCallTarget),
    ScriptConstruct(JavaScriptConstructTarget),
}

#[derive(Clone, Copy, Debug, Eq, PartialEq, Hash)]
pub enum FunctionEntryError {
    NotCallable,
    NotConstructor,
}

#[derive(Clone, Debug)]
pub struct CallableEntryRequest<'function> {
    function_object: ObjectId,
    function: &'function JsFunction,
    mode: CallMode,
    metadata: FunctionExecutableMetadata,
    raw_this: RuntimeValue,
    global_this: Option<RuntimeValue>,
    arguments: ExecutionArgumentList,
    new_target: Option<RuntimeValue>,
}

impl<'function> CallableEntryRequest<'function> {
    pub fn new(
        function_object: ObjectId,
        function: &'function JsFunction,
        mode: CallMode,
        metadata: FunctionExecutableMetadata,
        arguments: ExecutionArgumentList,
    ) -> Self {
        Self {
            function_object,
            function,
            mode,
            metadata,
            raw_this: RuntimeValue::undefined(),
            global_this: None,
            arguments,
            new_target: None,
        }
    }

    pub fn raw_this(mut self, raw_this: RuntimeValue) -> Self {
        self.raw_this = raw_this;
        self
    }

    pub fn global_this(mut self, global_this: Option<RuntimeValue>) -> Self {
        self.global_this = global_this;
        self
    }

    pub fn new_target(mut self, new_target: Option<RuntimeValue>) -> Self {
        self.new_target = new_target;
        self
    }
}

pub fn prepare_callable_entry_record(
    request: CallableEntryRequest<'_>,
) -> Result<CallableEntryRecord, FunctionEntryError> {
    let this_mode = request.metadata.this_mode;
    let expected_parameter_count = request.metadata.parameter_count;
    let plan = plan_function_call(request.function, request.mode, request.metadata);
    let arity_check = if plan.performs_arity_check {
        ArityCheckMode::MustCheckArity
    } else {
        ArityCheckMode::AlreadyChecked
    };
    let this_binding = bind_this_for_entry(
        this_mode,
        request.raw_this,
        request.global_this,
        ThisBindingSource::CallerProvided,
    );
    let arguments =
        bind_arguments_for_entry(request.arguments, expected_parameter_count, arity_check);

    let target = match request.mode {
        CallMode::Construct => match plan.construct {
            ConstructData::Native(target) => CallableEntryTarget::NativeConstruct(target),
            ConstructData::JavaScript(target) => CallableEntryTarget::ScriptConstruct(target),
            ConstructData::None => return Err(FunctionEntryError::NotConstructor),
        },
        CallMode::Regular | CallMode::Tail | CallMode::Varargs => match plan.call {
            CallData::Native(target) => CallableEntryTarget::NativeCall(target),
            CallData::JavaScript(target) => CallableEntryTarget::ScriptCall(target),
            CallData::None => return Err(FunctionEntryError::NotCallable),
        },
    };

    let specialization = if matches!(request.mode, CallMode::Construct) {
        CodeSpecializationKind::Construct
    } else {
        CodeSpecializationKind::Call
    };

    Ok(CallableEntryRecord {
        function_object: request.function_object,
        mode: request.mode,
        target,
        this_binding,
        arguments,
        specialization,
        new_target: request.new_target,
    })
}

pub fn plan_function_call(
    function: &JsFunction,
    mode: CallMode,
    metadata: FunctionExecutableMetadata,
) -> FunctionCallPlan {
    let requires_this_binding = metadata.this_mode != ThisMode::Lexical;
    let performs_arity_check = metadata.parameter_count > 0;
    let call = function.call_data();
    let construct = if matches!(mode, CallMode::Construct) {
        function.construct_data(metadata)
    } else {
        ConstructData::None
    };
    FunctionCallPlan {
        mode,
        call,
        construct,
        requires_this_binding,
        performs_arity_check,
    }
}

#[derive(Clone, Debug, Eq, PartialEq)]
pub struct FunctionCallPlan {
    pub mode: CallMode,
    pub call: CallData,
    pub construct: ConstructData,
    pub requires_this_binding: bool,
    pub performs_arity_check: bool,
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::gc::CellId;

    #[test]
    fn script_function_call_plan_uses_captured_scope_without_execution() {
        let function = JsFunction {
            executable: FunctionExecutableLink::Script(FunctionExecutableId(ExecutableId(CellId(
                7,
            )))),
            scope: FunctionScopeLink {
                captured_scope: Some(ScopeId(3)),
                realm_scope: None,
            },
            ..JsFunction::default()
        };

        let plan = plan_function_call(
            &function,
            CallMode::Regular,
            FunctionExecutableMetadata {
                parameter_count: 2,
                ..FunctionExecutableMetadata::default()
            },
        );

        assert!(matches!(plan.call, CallData::JavaScript(_)));
        assert!(plan.performs_arity_check);
        assert_eq!(plan.construct, ConstructData::None);
    }

    #[test]
    fn not_constructable_function_has_no_construct_data() {
        let function = JsFunction::default();

        let construct = function.construct_data(FunctionExecutableMetadata {
            construct_ability: ConstructAbility::NotConstructable,
            ..FunctionExecutableMetadata::default()
        });

        assert_eq!(construct, ConstructData::None);
    }

    #[test]
    fn callable_entry_record_uses_function_call_plan_and_argument_binding() {
        let function = JsFunction {
            executable: FunctionExecutableLink::Script(FunctionExecutableId(ExecutableId(CellId(
                7,
            )))),
            scope: FunctionScopeLink {
                captured_scope: Some(ScopeId(3)),
                realm_scope: None,
            },
            ..JsFunction::default()
        };

        let entry = prepare_callable_entry_record(
            CallableEntryRequest::new(
                ObjectId(CellId(11)),
                &function,
                CallMode::Regular,
                FunctionExecutableMetadata {
                    parameter_count: 2,
                    this_mode: ThisMode::Strict,
                    ..FunctionExecutableMetadata::default()
                },
                ExecutionArgumentList::new(
                    vec![RuntimeValue::from_i32(1)],
                    ArgumentListSource::Api,
                ),
            )
            .raw_this(RuntimeValue::null()),
        )
        .unwrap();

        assert!(matches!(entry.target, CallableEntryTarget::ScriptCall(_)));
        assert_eq!(entry.arguments.missing_argument_count, 1);
        assert_eq!(entry.this_binding.bound_this, Some(RuntimeValue::null()));
    }

    #[test]
    fn global_this_binding_records_required_substitution_without_global_object() {
        let binding = bind_this_for_entry(
            ThisMode::Global,
            RuntimeValue::undefined(),
            None,
            ThisBindingSource::CallerProvided,
        );

        assert_eq!(binding.bound_this, None);
        assert!(binding.requires_global_substitution);
    }
}
