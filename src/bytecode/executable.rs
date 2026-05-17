use std::sync::Arc;

use crate::strings::Identifier;

use crate::bytecode::code_block::{
    CodeBlock, CodeFeatures, CodeGenerationModeSet, CodeKind, CodeSpecialization, ExecutableHandle,
    ExecutableInfo, ParseMode, ScriptMode, SourceProvenance, SourceRange, UnlinkedCodeBlock,
};

/// Common executable-cell contract.
///
/// Executables install, replace, or clear code blocks through VM/GC-aware APIs.
/// Raw entrypoint pointers and future JIT state are represented as opaque slots
/// because ownership, write barriers, and executable memory lifetimes belong to
/// VM/runtime modules.
#[derive(Clone, Debug, Default)]
pub struct ExecutableBase {
    pub identity: Option<ExecutableHandle>,
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
}

#[derive(Clone, Copy, Debug, Eq, PartialEq, Hash)]
#[repr(transparent)]
pub struct JitEntrypointSlot(pub u32);

#[derive(Clone, Copy, Debug, Eq, PartialEq, Hash)]
#[repr(transparent)]
pub struct InterpreterEntrypointSlot(pub u32);

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
    pub parameter_count_excluding_this: u32,
    pub function_mode: FunctionMode,
    pub construct_ability: ConstructAbility,
    pub call_code: Option<Arc<UnlinkedCodeBlock>>,
    pub construct_code: Option<Arc<UnlinkedCodeBlock>>,
    pub class_source: Option<SourceRange>,
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

#[derive(Clone, Copy, Debug, Eq, PartialEq, Hash)]
pub enum FunctionMode {
    Normal,
    Generator,
    Async,
    AsyncGenerator,
    ClassConstructor,
    Method,
}

#[derive(Clone, Copy, Debug, Eq, PartialEq, Hash)]
pub enum ConstructAbility {
    CannotConstruct,
    CanConstruct,
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
    pub provider_id: Option<u64>,
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
