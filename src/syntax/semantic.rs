use crate::syntax::arena::ParserIdentifier;
use crate::syntax::source::SourceSpan;

/// Parser-owned declaration table.
///
/// Mutation is parser-only. After parse finalization this data is frozen for
/// bytecode generation, module analysis, and diagnostics.
#[derive(Clone, Debug, Default, Eq, PartialEq)]
pub struct VariableEnvironment {
    declarations: Vec<Declaration>,
    private_names: Vec<PrivateNameDeclaration>,
    flags: VariableEnvironmentFlags,
}

impl VariableEnvironment {
    pub fn declarations(&self) -> &[Declaration] {
        &self.declarations
    }

    pub fn private_names(&self) -> &[PrivateNameDeclaration] {
        &self.private_names
    }

    pub fn flags(&self) -> VariableEnvironmentFlags {
        self.flags
    }

    pub fn record_declaration(&mut self, declaration: Declaration) {
        self.declarations.push(declaration);
    }

    pub fn record_private_name(&mut self, declaration: PrivateNameDeclaration) {
        self.private_names.push(declaration);
    }
}

#[derive(Clone, Debug, Eq, PartialEq)]
pub struct Declaration {
    pub name: ParserIdentifier,
    pub kind: DeclarationKind,
    pub span: SourceSpan,
    pub import: DeclarationImportType,
    pub flags: DeclarationFlags,
}

#[derive(Clone, Copy, Debug, Eq, PartialEq)]
pub enum DeclarationKind {
    Var,
    Let,
    Const,
    Using,
    AwaitUsing,
    Parameter,
    Function,
    Class,
    Import,
    SloppyHoistedFunction,
}

#[derive(Clone, Copy, Debug, Eq, PartialEq)]
pub enum DeclarationImportType {
    Imported,
    ImportedNamespace,
    NotImported,
}

#[derive(Clone, Copy, Debug, Default, Eq, PartialEq)]
pub struct DeclarationFlags {
    pub captured: bool,
    pub exported: bool,
    pub function_declaration: bool,
    pub lexical: bool,
}

#[derive(Clone, Copy, Debug, Default, Eq, PartialEq)]
pub struct VariableEnvironmentFlags {
    pub everything_captured: bool,
    pub has_using_declaration: bool,
    pub has_await_using_declaration: bool,
}

#[derive(Clone, Debug, Eq, PartialEq)]
pub struct PrivateNameDeclaration {
    pub name: ParserIdentifier,
    pub kind: PrivateNameKind,
    pub span: SourceSpan,
}

#[derive(Clone, Copy, Debug, Eq, PartialEq)]
pub enum PrivateNameKind {
    Field { is_static: bool },
    Method { is_static: bool },
    Getter { is_static: bool },
    Setter { is_static: bool },
}

#[derive(Clone, Debug, Default, Eq, PartialEq)]
pub struct EarlySemanticInfo {
    pub strict: bool,
    pub uses_eval: bool,
    pub captures_this: bool,
    pub contains_direct_super: bool,
    pub constant_count: u32,
    pub features: CodeFeatures,
    pub declared_variables: VariableEnvironment,
    pub lexical_variables: VariableEnvironment,
    pub captured_variables: Vec<ParserIdentifier>,
    pub errors: Vec<EarlyError>,
}

#[derive(Clone, Debug, Default, Eq, PartialEq)]
pub struct ModuleAnalysis {
    pub requested_modules: Vec<ModuleRequest>,
    pub imports: Vec<ModuleImport>,
    pub exports: Vec<ModuleExport>,
}

#[derive(Clone, Debug, Eq, PartialEq)]
pub struct ModuleImport {
    pub local_name: ParserIdentifier,
    pub module_request: ParserIdentifier,
    pub span: SourceSpan,
    pub kind: ImportBindingKind,
}

#[derive(Clone, Debug, Eq, PartialEq)]
pub struct ModuleExport {
    pub exported_name: ParserIdentifier,
    pub local_name: Option<ParserIdentifier>,
    pub module_request: Option<ParserIdentifier>,
    pub span: SourceSpan,
    pub kind: ExportBindingKind,
}

#[derive(Clone, Debug, Eq, PartialEq)]
pub struct ModuleRequest {
    pub specifier: ParserIdentifier,
    pub phase: ModulePhase,
    pub attributes: Vec<ModuleImportAttribute>,
    pub span: SourceSpan,
}

#[derive(Clone, Copy, Debug, Eq, PartialEq)]
pub struct ModuleImportAttribute {
    pub key: ParserIdentifier,
    pub value: ParserIdentifier,
    pub span: SourceSpan,
}

#[derive(Clone, Copy, Debug, Eq, PartialEq)]
pub enum ModulePhase {
    Evaluation,
    Defer,
}

#[derive(Clone, Copy, Debug, Eq, PartialEq)]
pub enum ImportBindingKind {
    Default,
    Namespace,
    Named,
    SideEffectOnly,
}

#[derive(Clone, Copy, Debug, Eq, PartialEq)]
pub enum ExportBindingKind {
    Local,
    ReExport,
    Namespace,
    Star,
    Default,
}

#[derive(Clone, Copy, Debug, Default, Eq, PartialEq)]
pub struct CodeFeatures {
    pub eval: bool,
    pub arguments: bool,
    pub this: bool,
    pub new_target: bool,
    pub super_call: bool,
    pub super_property: bool,
    pub import_meta: bool,
    pub tail_call_candidate: bool,
}

#[derive(Clone, Copy, Debug, Default, Eq, PartialEq, Hash)]
pub struct ScopeId(pub u32);

#[derive(Clone, Debug, Eq, PartialEq)]
pub struct Scope {
    pub id: ScopeId,
    pub parent: Option<ScopeId>,
    pub kind: ScopeKind,
    pub span: SourceSpan,
    pub labels: Vec<ScopeLabel>,
    pub declared: VariableEnvironment,
    pub lexical: VariableEnvironment,
    pub private_names: VariableEnvironment,
    pub flags: ScopeFlags,
}

#[derive(Clone, Copy, Debug, Eq, PartialEq)]
pub enum ScopeKind {
    Global,
    Module,
    Eval,
    Function,
    ArrowFunction,
    Class,
    ClassStaticBlock,
    Block,
    Catch,
    With,
}

#[derive(Clone, Copy, Debug, Default, Eq, PartialEq)]
pub struct ScopeFlags {
    pub strict: bool,
    pub generator: bool,
    pub async_function: bool,
    pub static_block: bool,
    pub implementation_private: bool,
    pub in_loop: bool,
    pub in_switch: bool,
}

#[derive(Clone, Copy, Debug, Eq, PartialEq)]
pub struct ScopeLabel {
    pub name: ParserIdentifier,
    pub is_loop: bool,
    pub span: SourceSpan,
}

#[derive(Clone, Debug, Default, Eq, PartialEq)]
pub struct SemanticModel {
    pub scopes: Vec<Scope>,
    pub module: Option<ModuleAnalysis>,
    pub early_errors: Vec<EarlyError>,
}

#[derive(Clone, Debug, Eq, PartialEq)]
pub struct EarlyError {
    pub span: SourceSpan,
    pub kind: EarlyErrorKind,
}

#[derive(Clone, Debug, Eq, PartialEq)]
pub enum EarlyErrorKind {
    DuplicateDeclaration(ParserIdentifier),
    InvalidStrictModeBinding(ParserIdentifier),
    InvalidPrivateName {
        name: ParserIdentifier,
        reason: PrivateNameError,
    },
    UnboundPrivateName(ParserIdentifier),
    InvalidImportExport(String),
    InvalidControlFlow(ControlFlowError),
    InvalidAssignmentTarget,
    AwaitOrYieldInInvalidContext,
    SuperInInvalidContext,
}

#[derive(Clone, Copy, Debug, Eq, PartialEq)]
pub enum PrivateNameError {
    Duplicate,
    StaticNonStaticConflict,
    AccessorPairConflict,
}

#[derive(Clone, Copy, Debug, Eq, PartialEq)]
pub enum ControlFlowError {
    BreakOutsideLoopOrSwitch,
    ContinueOutsideLoop,
    ReturnOutsideFunction,
    NewTargetOutsideFunction,
}
