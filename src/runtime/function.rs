use crate::runtime::scope::ScopeId;
use crate::runtime::state::{
    CodeBlockId, ExecutableId, NativeCodeId, ObjectId, SourceProviderId, StringId, StructureId,
    WatchpointGeneration,
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
