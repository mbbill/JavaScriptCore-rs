use crate::syntax::arena::{ParserIdentifier, WellKnownIdentifier};
use crate::syntax::ast::{AstRoot, ScopeNode, ScopeNodeKind};
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
    pub needs_full_activation: bool,
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
    pub uses_await: bool,
    pub uses_import_meta: bool,
    pub shadows_arguments: bool,
    pub has_non_simple_parameters: bool,
    pub constant_count: u32,
    pub features: CodeFeatures,
    pub declared_variables: VariableEnvironment,
    pub lexical_variables: VariableEnvironment,
    pub captured_variables: Vec<ParserIdentifier>,
    pub private_name_references: Vec<PrivateNameReference>,
    pub class_elements: Vec<ClassElementSemanticRecord>,
    pub errors: Vec<EarlyError>,
}

#[derive(Clone, Copy, Debug, Eq, PartialEq)]
pub struct PrivateNameReference {
    pub name: ParserIdentifier,
    pub span: SourceSpan,
    pub kind: PrivateNameReferenceKind,
}

#[derive(Clone, Copy, Debug, Eq, PartialEq)]
pub enum PrivateNameReferenceKind {
    Get,
    Set,
    Call,
    In,
    BrandCheck,
}

#[derive(Clone, Copy, Debug, Eq, PartialEq)]
pub struct ClassElementSemanticRecord {
    pub name: ClassElementSemanticName,
    pub kind: ClassElementSemanticKind,
    pub is_static: bool,
    pub span: SourceSpan,
}

#[derive(Clone, Copy, Debug, Eq, PartialEq)]
pub enum ClassElementSemanticName {
    Public(ParserIdentifier),
    Private(ParserIdentifier),
    Computed,
}

#[derive(Clone, Copy, Debug, Eq, PartialEq)]
pub enum ClassElementSemanticKind {
    Field,
    Method,
    Getter,
    Setter,
    StaticBlock,
}

#[derive(Clone, Debug, Default, Eq, PartialEq)]
pub struct ModuleAnalysis {
    pub requested_modules: Vec<ModuleRequest>,
    pub imports: Vec<ModuleImport>,
    pub exports: Vec<ModuleExport>,
    pub scope_data: ModuleScopeData,
}

/// Module-scope export aliases produced by module analysis.
///
/// JSC stores this in `ModuleScopeData` so the module record can expose export
/// names without giving later stages authority to mutate parser environments.
#[derive(Clone, Debug, Default, Eq, PartialEq)]
pub struct ModuleScopeData {
    pub exported_names: Vec<ParserIdentifier>,
    pub exported_bindings: Vec<ModuleExportBinding>,
}

#[derive(Clone, Debug, Eq, PartialEq)]
pub struct ModuleExportBinding {
    pub local_name: ParserIdentifier,
    pub exported_names: Vec<ParserIdentifier>,
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
    pub await_expression: bool,
    pub non_simple_parameters: bool,
    pub arrow_function: bool,
    pub with_scope_taint: bool,
    pub tail_call_candidate: bool,
}

#[derive(Clone, Copy, Debug, Default, Eq, PartialEq, Hash)]
pub struct SemanticScopeId(pub u32);

/// Syntax/semantic-analysis scope record.
///
/// `SemanticModel` owns these records after parser finalization. Bytecompiler
/// code may borrow the IDs for planning, but runtime scope cells use
/// `runtime::ScopeId` and are materialized under VM/GC authority.
#[derive(Clone, Debug, Eq, PartialEq)]
pub struct Scope {
    pub id: SemanticScopeId,
    pub parent: Option<SemanticScopeId>,
    pub kind: ScopeKind,
    pub span: SourceSpan,
    pub labels: Vec<ScopeLabel>,
    pub declared: VariableEnvironment,
    pub lexical: VariableEnvironment,
    pub private_names: VariableEnvironment,
    pub flags: ScopeFlags,
    pub environment: EnvironmentSemanticRecord,
    pub parse: ParseSemanticMetadata,
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
    pub function_boundary: bool,
    pub generator_boundary: bool,
    pub async_boundary: bool,
    pub arrow_function: bool,
    pub arrow_boundary: bool,
    pub static_block: bool,
    pub static_block_boundary: bool,
    pub class_scope: bool,
    pub private_name_scope: bool,
    pub simple_catch_parameter_scope: bool,
    pub catch_block_scope: bool,
    pub implementation_private: bool,
    pub in_loop: bool,
    pub in_switch: bool,
    pub allows_var_declarations: bool,
    pub allows_lexical_declarations: bool,
}

#[derive(Clone, Copy, Debug, Default, Eq, PartialEq)]
pub struct EnvironmentSemanticRecord {
    pub kind: EnvironmentSemanticKind,
    pub declaration_count: u32,
    pub lexical_count: u32,
    pub private_name_count: u32,
    pub captured_count: u32,
    pub flags: EnvironmentSemanticFlags,
}

#[derive(Clone, Copy, Debug, Default, Eq, PartialEq)]
pub enum EnvironmentSemanticKind {
    #[default]
    Declarative,
    Global,
    Module,
    Function,
    Eval,
    Class,
    ClassStaticBlock,
    WithObject,
}

#[derive(Clone, Copy, Debug, Default, Eq, PartialEq)]
pub struct EnvironmentSemanticFlags {
    pub strict: bool,
    pub creates_mutable_bindings: bool,
    pub creates_immutable_bindings: bool,
    pub captures_dynamic_scope: bool,
    pub requires_tdz_checks: bool,
    pub contains_private_names: bool,
    pub has_using_cleanup: bool,
    pub has_await_using_cleanup: bool,
    pub needs_full_activation: bool,
    pub implementation_private: bool,
}

#[derive(Clone, Copy, Debug, Default, Eq, PartialEq)]
pub struct ParseSemanticMetadata {
    pub goal: ParseSemanticGoal,
    pub strictness: SemanticStrictness,
    pub eval_context: EvalSemanticContext,
    pub function: Option<FunctionParseSemanticMetadata>,
    pub eval: Option<EvalParseSemanticMetadata>,
    pub module: Option<ModuleParseSemanticMetadata>,
    pub features: CodeFeatures,
}

#[derive(Clone, Copy, Debug, Default, Eq, PartialEq)]
pub enum ParseSemanticGoal {
    #[default]
    Script,
    Module,
    Eval,
    Function,
    ClassStaticBlock,
}

#[derive(Clone, Copy, Debug, Default, Eq, PartialEq)]
pub enum SemanticStrictness {
    #[default]
    Sloppy,
    Strict,
}

#[derive(Clone, Copy, Debug, Default, Eq, PartialEq)]
pub enum EvalSemanticContext {
    #[default]
    None,
    Direct,
    FunctionEval,
    InstanceFieldEval,
}

#[derive(Clone, Copy, Debug, Default, Eq, PartialEq)]
pub struct FunctionParseSemanticMetadata {
    pub has_non_simple_parameters: bool,
    pub uses_arguments: bool,
    pub captures_this: bool,
    pub contains_direct_super: bool,
    pub private_brand_required: bool,
    pub needs_class_field_initializer: bool,
    pub is_arrow_context: bool,
}

#[derive(Clone, Copy, Debug, Default, Eq, PartialEq)]
pub struct EvalParseSemanticMetadata {
    pub direct: bool,
    pub inherits_private_names: bool,
    pub disables_eval_cache: bool,
}

#[derive(Clone, Copy, Debug, Default, Eq, PartialEq)]
pub struct ModuleParseSemanticMetadata {
    pub requested_module_count: u32,
    pub import_count: u32,
    pub local_export_count: u32,
    pub re_export_count: u32,
    pub has_top_level_await: bool,
    pub has_import_meta: bool,
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

impl SemanticModel {
    pub fn validate(&self) -> SemanticValidationReport {
        let mut findings = Vec::new();

        for (index, scope) in self.scopes.iter().enumerate() {
            if !scope.span.is_ordered() {
                findings.push(SemanticValidationFinding::UnorderedScopeSpan {
                    scope: scope.id,
                    span: scope.span,
                });
            }
            if scope.parent == Some(scope.id) {
                findings.push(SemanticValidationFinding::ScopeIsOwnParent { scope: scope.id });
            }
            if let Some(parent) = scope.parent {
                if !self.scopes.iter().any(|candidate| candidate.id == parent) {
                    findings.push(SemanticValidationFinding::MissingParentScope {
                        scope: scope.id,
                        parent,
                    });
                }
            }
            if self.scopes[..index]
                .iter()
                .any(|candidate| candidate.id == scope.id)
            {
                findings.push(SemanticValidationFinding::DuplicateScopeId { scope: scope.id });
            }
            if scope.flags.function_boundary
                && !matches!(
                    scope.kind,
                    ScopeKind::Function | ScopeKind::ArrowFunction | ScopeKind::Eval
                )
            {
                findings.push(SemanticValidationFinding::BoundaryFlagKindMismatch {
                    scope: scope.id,
                    kind: scope.kind,
                    flag: ScopeBoundaryFlag::Function,
                });
            }
            if scope.flags.static_block_boundary && scope.kind != ScopeKind::ClassStaticBlock {
                findings.push(SemanticValidationFinding::BoundaryFlagKindMismatch {
                    scope: scope.id,
                    kind: scope.kind,
                    flag: ScopeBoundaryFlag::StaticBlock,
                });
            }
            if scope.environment.private_name_count > 0 && !scope.flags.private_name_scope {
                findings.push(SemanticValidationFinding::PrivateEnvironmentFlagMismatch {
                    scope: scope.id,
                });
            }
            if scope.parse.goal != parse_goal_for_scope_kind(scope.kind) {
                findings.push(SemanticValidationFinding::ParseGoalKindMismatch {
                    scope: scope.id,
                    kind: scope.kind,
                    goal: scope.parse.goal,
                });
            }
            if scope.parse.goal == ParseSemanticGoal::Module
                && scope.parse.strictness != SemanticStrictness::Strict
            {
                findings.push(SemanticValidationFinding::ModuleNotStrict { scope: scope.id });
            }
        }

        if let Some(module) = &self.module {
            validate_module_analysis(module, &mut findings);
        }

        SemanticValidationReport { findings }
    }
}

pub fn analyze_root(root: AstRoot, scope: &ScopeNode) -> SemanticModel {
    let kind = match root {
        AstRoot::Script(_) => ScopeKind::Global,
        AstRoot::Module(_) => ScopeKind::Module,
        AstRoot::Function(_) => ScopeKind::Function,
    };
    analyze_scope_node(scope, None, kind)
}

pub fn analyze_scope_node(
    scope: &ScopeNode,
    parent: Option<SemanticScopeId>,
    fallback_kind: ScopeKind,
) -> SemanticModel {
    let mut semantic = scope.semantics.clone();
    let kind = scope_kind(scope.kind).unwrap_or(fallback_kind);
    analyze_environment(
        &mut semantic.declared_variables,
        semantic.strict,
        &mut semantic.errors,
    );
    analyze_environment(
        &mut semantic.lexical_variables,
        semantic.strict,
        &mut semantic.errors,
    );
    collect_duplicate_declarations(
        &semantic.declared_variables,
        &semantic.lexical_variables,
        &mut semantic.errors,
    );
    analyze_parameters(
        &semantic.declared_variables,
        semantic.strict,
        semantic.has_non_simple_parameters,
        &mut semantic.errors,
    );
    analyze_private_names(&semantic.lexical_variables, &mut semantic.errors);
    analyze_private_name_references(
        &semantic.lexical_variables,
        &semantic.private_name_references,
        &mut semantic.errors,
    );
    analyze_class_elements(&semantic.class_elements, &mut semantic.errors);

    let flags = scope_flags(scope.kind, &semantic);
    let mut model = SemanticModel {
        scopes: vec![Scope {
            id: scope.id,
            parent,
            kind,
            span: scope.span,
            labels: Vec::new(),
            declared: semantic.declared_variables.clone(),
            lexical: semantic.lexical_variables.clone(),
            private_names: semantic.lexical_variables.clone(),
            flags,
            environment: environment_semantics(kind, &semantic),
            parse: parse_semantics(kind, &semantic, scope.module.as_ref()),
        }],
        module: scope.module.clone(),
        early_errors: semantic.errors,
    };

    if let Some(module) = &mut model.module {
        analyze_module_exports(module, &mut model.early_errors);
    }

    model
}

fn analyze_parameters(
    declared: &VariableEnvironment,
    strict: bool,
    has_non_simple_parameters: bool,
    errors: &mut Vec<EarlyError>,
) {
    for (index, declaration) in declared.declarations().iter().enumerate() {
        if declaration.kind != DeclarationKind::Parameter {
            continue;
        }
        if !(strict || has_non_simple_parameters) {
            continue;
        }
        if declared.declarations()[..index].iter().any(|candidate| {
            candidate.kind == DeclarationKind::Parameter && candidate.name == declaration.name
        }) {
            errors.push(EarlyError {
                span: declaration.span,
                kind: EarlyErrorKind::DuplicateDeclaration(declaration.name),
            });
        }
    }
}

fn analyze_environment(
    environment: &mut VariableEnvironment,
    strict: bool,
    errors: &mut Vec<EarlyError>,
) {
    for declaration in environment.declarations() {
        if strict && is_restricted_strict_binding(declaration.name) {
            errors.push(EarlyError {
                span: declaration.span,
                kind: EarlyErrorKind::InvalidStrictModeBinding(declaration.name),
            });
        }
    }
    for (index, declaration) in environment.declarations().iter().enumerate() {
        if environment.declarations()[..index].iter().any(|candidate| {
            candidate.name == declaration.name
                && declarations_conflict(candidate.kind, declaration.kind)
        }) {
            errors.push(EarlyError {
                span: declaration.span,
                kind: EarlyErrorKind::DuplicateDeclaration(declaration.name),
            });
        }
    }
    environment.flags.has_using_declaration = environment
        .declarations()
        .iter()
        .any(|declaration| declaration.kind == DeclarationKind::Using);
    environment.flags.has_await_using_declaration = environment
        .declarations()
        .iter()
        .any(|declaration| declaration.kind == DeclarationKind::AwaitUsing);
}

fn collect_duplicate_declarations(
    declared: &VariableEnvironment,
    lexical: &VariableEnvironment,
    errors: &mut Vec<EarlyError>,
) {
    for lexical_declaration in lexical.declarations() {
        if declared.declarations().iter().any(|declaration| {
            declaration.name == lexical_declaration.name
                && declarations_conflict(declaration.kind, lexical_declaration.kind)
        }) {
            errors.push(EarlyError {
                span: lexical_declaration.span,
                kind: EarlyErrorKind::DuplicateDeclaration(lexical_declaration.name),
            });
        }
    }
}

fn analyze_private_names(environment: &VariableEnvironment, errors: &mut Vec<EarlyError>) {
    for (index, declaration) in environment.private_names().iter().enumerate() {
        for previous in &environment.private_names()[..index] {
            if previous.name != declaration.name {
                continue;
            }
            if private_staticness(previous.kind) != private_staticness(declaration.kind) {
                errors.push(EarlyError {
                    span: declaration.span,
                    kind: EarlyErrorKind::InvalidPrivateName {
                        name: declaration.name,
                        reason: PrivateNameError::StaticNonStaticConflict,
                    },
                });
            } else if !private_accessor_pair(previous.kind, declaration.kind) {
                errors.push(EarlyError {
                    span: declaration.span,
                    kind: EarlyErrorKind::InvalidPrivateName {
                        name: declaration.name,
                        reason: PrivateNameError::Duplicate,
                    },
                });
            }
        }
    }
}

fn analyze_private_name_references(
    lexical_variables: &VariableEnvironment,
    references: &[PrivateNameReference],
    errors: &mut Vec<EarlyError>,
) {
    for reference in references {
        if !lexical_variables
            .private_names()
            .iter()
            .any(|declaration| declaration.name == reference.name)
        {
            errors.push(EarlyError {
                span: reference.span,
                kind: EarlyErrorKind::UnboundPrivateName(reference.name),
            });
        }
    }
}

fn analyze_class_elements(elements: &[ClassElementSemanticRecord], errors: &mut Vec<EarlyError>) {
    for element in elements {
        match element.name {
            ClassElementSemanticName::Private(name)
                if name.0 == WellKnownIdentifier::Constructor as u32 =>
            {
                errors.push(EarlyError {
                    span: element.span,
                    kind: EarlyErrorKind::InvalidPrivateName {
                        name,
                        reason: PrivateNameError::ConstructorName,
                    },
                });
            }
            ClassElementSemanticName::Public(name)
                if element.is_static && name.0 == WellKnownIdentifier::Prototype as u32 =>
            {
                errors.push(EarlyError {
                    span: element.span,
                    kind: EarlyErrorKind::InvalidClassElement {
                        name: Some(name),
                        reason: ClassElementError::StaticPrototypeProperty,
                    },
                });
            }
            ClassElementSemanticName::Public(name)
                if matches!(
                    element.kind,
                    ClassElementSemanticKind::Getter | ClassElementSemanticKind::Setter
                ) && name.0 == WellKnownIdentifier::Constructor as u32 =>
            {
                errors.push(EarlyError {
                    span: element.span,
                    kind: EarlyErrorKind::InvalidClassElement {
                        name: Some(name),
                        reason: ClassElementError::ConstructorAccessor,
                    },
                });
            }
            _ => {}
        }
    }
}

fn analyze_module_exports(module: &mut ModuleAnalysis, errors: &mut Vec<EarlyError>) {
    module.scope_data.exported_names.clear();
    module.scope_data.exported_bindings.clear();
    for export in &module.exports {
        if module
            .scope_data
            .exported_names
            .contains(&export.exported_name)
        {
            errors.push(EarlyError {
                span: export.span,
                kind: EarlyErrorKind::InvalidImportExport(
                    "duplicate exported binding name".to_string(),
                ),
            });
        } else {
            module.scope_data.exported_names.push(export.exported_name);
        }
        if let Some(local_name) = export.local_name {
            match module
                .scope_data
                .exported_bindings
                .iter_mut()
                .find(|binding| binding.local_name == local_name)
            {
                Some(binding) => binding.exported_names.push(export.exported_name),
                None => module
                    .scope_data
                    .exported_bindings
                    .push(ModuleExportBinding {
                        local_name,
                        exported_names: vec![export.exported_name],
                    }),
            }
        }
    }
}

fn declarations_conflict(left: DeclarationKind, right: DeclarationKind) -> bool {
    let left_var_like = matches!(
        left,
        DeclarationKind::Var
            | DeclarationKind::Function
            | DeclarationKind::SloppyHoistedFunction
            | DeclarationKind::Parameter
    );
    let right_var_like = matches!(
        right,
        DeclarationKind::Var
            | DeclarationKind::Function
            | DeclarationKind::SloppyHoistedFunction
            | DeclarationKind::Parameter
    );
    !(left_var_like && right_var_like)
}

fn is_restricted_strict_binding(identifier: ParserIdentifier) -> bool {
    matches!(
        identifier.0,
        id if id == WellKnownIdentifier::Arguments as u32 || id == WellKnownIdentifier::Eval as u32
    )
}

fn private_staticness(kind: PrivateNameKind) -> bool {
    match kind {
        PrivateNameKind::Field { is_static }
        | PrivateNameKind::Method { is_static }
        | PrivateNameKind::Getter { is_static }
        | PrivateNameKind::Setter { is_static } => is_static,
    }
}

fn private_accessor_pair(left: PrivateNameKind, right: PrivateNameKind) -> bool {
    matches!(
        (left, right),
        (
            PrivateNameKind::Getter { is_static: left_static },
            PrivateNameKind::Setter { is_static: right_static }
        ) | (
            PrivateNameKind::Setter { is_static: left_static },
            PrivateNameKind::Getter { is_static: right_static }
        ) if left_static == right_static
    )
}

fn scope_kind(kind: ScopeNodeKind) -> Option<ScopeKind> {
    Some(match kind {
        ScopeNodeKind::Script => ScopeKind::Global,
        ScopeNodeKind::Module => ScopeKind::Module,
        ScopeNodeKind::Function => ScopeKind::Function,
        ScopeNodeKind::Eval => ScopeKind::Eval,
        ScopeNodeKind::ClassStaticBlock => ScopeKind::ClassStaticBlock,
    })
}

fn environment_semantics(
    kind: ScopeKind,
    semantic: &EarlySemanticInfo,
) -> EnvironmentSemanticRecord {
    let env_kind = match kind {
        ScopeKind::Global => EnvironmentSemanticKind::Global,
        ScopeKind::Module => EnvironmentSemanticKind::Module,
        ScopeKind::Eval => EnvironmentSemanticKind::Eval,
        ScopeKind::Function | ScopeKind::ArrowFunction => EnvironmentSemanticKind::Function,
        ScopeKind::Class => EnvironmentSemanticKind::Class,
        ScopeKind::ClassStaticBlock => EnvironmentSemanticKind::ClassStaticBlock,
        ScopeKind::With => EnvironmentSemanticKind::WithObject,
        ScopeKind::Block | ScopeKind::Catch => EnvironmentSemanticKind::Declarative,
    };
    let has_mutable = semantic
        .declared_variables
        .declarations()
        .iter()
        .chain(semantic.lexical_variables.declarations())
        .any(|declaration| {
            matches!(
                declaration.kind,
                DeclarationKind::Var
                    | DeclarationKind::Let
                    | DeclarationKind::Function
                    | DeclarationKind::Parameter
                    | DeclarationKind::SloppyHoistedFunction
            )
        });
    let has_immutable = semantic
        .lexical_variables
        .declarations()
        .iter()
        .any(|declaration| {
            matches!(
                declaration.kind,
                DeclarationKind::Const
                    | DeclarationKind::Import
                    | DeclarationKind::Class
                    | DeclarationKind::Using
                    | DeclarationKind::AwaitUsing
            )
        });
    let private_name_count = semantic.lexical_variables.private_names().len() as u32;

    EnvironmentSemanticRecord {
        kind: env_kind,
        declaration_count: semantic.declared_variables.declarations().len() as u32,
        lexical_count: semantic.lexical_variables.declarations().len() as u32,
        private_name_count,
        captured_count: semantic.captured_variables.len() as u32,
        flags: EnvironmentSemanticFlags {
            strict: semantic.strict || kind == ScopeKind::Module,
            creates_mutable_bindings: has_mutable,
            creates_immutable_bindings: has_immutable,
            captures_dynamic_scope: semantic.uses_eval || semantic.features.with_scope_taint,
            requires_tdz_checks: has_immutable || private_name_count > 0,
            contains_private_names: private_name_count > 0,
            has_using_cleanup: semantic.declared_variables.flags().has_using_declaration
                || semantic.lexical_variables.flags().has_using_declaration,
            has_await_using_cleanup: semantic
                .declared_variables
                .flags()
                .has_await_using_declaration
                || semantic
                    .lexical_variables
                    .flags()
                    .has_await_using_declaration,
            needs_full_activation: semantic.declared_variables.flags().needs_full_activation
                || semantic.lexical_variables.flags().needs_full_activation
                || semantic.uses_eval,
            implementation_private: false,
        },
    }
}

fn parse_semantics(
    kind: ScopeKind,
    semantic: &EarlySemanticInfo,
    module: Option<&ModuleAnalysis>,
) -> ParseSemanticMetadata {
    let goal = parse_goal_for_scope_kind(kind);
    let strictness = if semantic.strict || kind == ScopeKind::Module {
        SemanticStrictness::Strict
    } else {
        SemanticStrictness::Sloppy
    };
    ParseSemanticMetadata {
        goal,
        strictness,
        eval_context: if semantic.uses_eval {
            EvalSemanticContext::Direct
        } else {
            EvalSemanticContext::None
        },
        function: matches!(
            goal,
            ParseSemanticGoal::Function | ParseSemanticGoal::ClassStaticBlock
        )
        .then_some(FunctionParseSemanticMetadata {
            has_non_simple_parameters: semantic.has_non_simple_parameters,
            uses_arguments: semantic.features.arguments,
            captures_this: semantic.captures_this,
            contains_direct_super: semantic.contains_direct_super,
            private_brand_required: !semantic.lexical_variables.private_names().is_empty(),
            needs_class_field_initializer: semantic
                .class_elements
                .iter()
                .any(|element| matches!(element.kind, ClassElementSemanticKind::Field)),
            is_arrow_context: semantic.features.arrow_function,
        }),
        eval: (goal == ParseSemanticGoal::Eval).then_some(EvalParseSemanticMetadata {
            direct: semantic.uses_eval,
            inherits_private_names: !semantic.private_name_references.is_empty(),
            disables_eval_cache: semantic.uses_eval,
        }),
        module: module.map(|module| ModuleParseSemanticMetadata {
            requested_module_count: module.requested_modules.len() as u32,
            import_count: module.imports.len() as u32,
            local_export_count: module
                .exports
                .iter()
                .filter(|export| export.module_request.is_none())
                .count() as u32,
            re_export_count: module
                .exports
                .iter()
                .filter(|export| export.module_request.is_some())
                .count() as u32,
            has_top_level_await: semantic.uses_await,
            has_import_meta: semantic.uses_import_meta,
        }),
        features: semantic.features,
    }
}

fn parse_goal_for_scope_kind(kind: ScopeKind) -> ParseSemanticGoal {
    match kind {
        ScopeKind::Global => ParseSemanticGoal::Script,
        ScopeKind::Module => ParseSemanticGoal::Module,
        ScopeKind::Eval => ParseSemanticGoal::Eval,
        ScopeKind::Function | ScopeKind::ArrowFunction => ParseSemanticGoal::Function,
        ScopeKind::ClassStaticBlock => ParseSemanticGoal::ClassStaticBlock,
        ScopeKind::Class | ScopeKind::Block | ScopeKind::Catch | ScopeKind::With => {
            ParseSemanticGoal::Script
        }
    }
}

fn scope_flags(kind: ScopeNodeKind, semantic: &EarlySemanticInfo) -> ScopeFlags {
    ScopeFlags {
        strict: semantic.strict,
        function_boundary: matches!(kind, ScopeNodeKind::Function | ScopeNodeKind::Eval),
        static_block: kind == ScopeNodeKind::ClassStaticBlock,
        static_block_boundary: kind == ScopeNodeKind::ClassStaticBlock,
        private_name_scope: !semantic.lexical_variables.private_names().is_empty(),
        class_scope: !semantic.class_elements.is_empty(),
        allows_var_declarations: !matches!(kind, ScopeNodeKind::ClassStaticBlock),
        allows_lexical_declarations: true,
        ..ScopeFlags::default()
    }
}

fn validate_module_analysis(
    module: &ModuleAnalysis,
    findings: &mut Vec<SemanticValidationFinding>,
) {
    for import in &module.imports {
        if !module
            .requested_modules
            .iter()
            .any(|request| request.specifier == import.module_request)
        {
            findings.push(SemanticValidationFinding::MissingModuleRequest {
                specifier: import.module_request,
            });
        }
        if !import.span.is_ordered() {
            findings.push(SemanticValidationFinding::UnorderedModuleSpan { span: import.span });
        }
    }

    for export in &module.exports {
        if let Some(specifier) = export.module_request {
            if !module
                .requested_modules
                .iter()
                .any(|request| request.specifier == specifier)
            {
                findings.push(SemanticValidationFinding::MissingModuleRequest { specifier });
            }
        }
        if !module
            .scope_data
            .exported_names
            .contains(&export.exported_name)
        {
            findings.push(SemanticValidationFinding::ExportMissingFromScopeData {
                exported_name: export.exported_name,
            });
        }
        if !export.span.is_ordered() {
            findings.push(SemanticValidationFinding::UnorderedModuleSpan { span: export.span });
        }
    }

    for request in &module.requested_modules {
        if !request.span.is_ordered() {
            findings.push(SemanticValidationFinding::UnorderedModuleSpan { span: request.span });
        }
        for attribute in &request.attributes {
            if !attribute.span.is_ordered() {
                findings.push(SemanticValidationFinding::UnorderedModuleSpan {
                    span: attribute.span,
                });
            }
        }
    }
}

#[derive(Clone, Debug, Default, Eq, PartialEq)]
pub struct SemanticValidationReport {
    pub findings: Vec<SemanticValidationFinding>,
}

impl SemanticValidationReport {
    pub fn is_valid(&self) -> bool {
        self.findings.is_empty()
    }
}

#[derive(Clone, Copy, Debug, Eq, PartialEq)]
pub enum SemanticValidationFinding {
    DuplicateScopeId {
        scope: SemanticScopeId,
    },
    MissingParentScope {
        scope: SemanticScopeId,
        parent: SemanticScopeId,
    },
    ScopeIsOwnParent {
        scope: SemanticScopeId,
    },
    UnorderedScopeSpan {
        scope: SemanticScopeId,
        span: SourceSpan,
    },
    BoundaryFlagKindMismatch {
        scope: SemanticScopeId,
        kind: ScopeKind,
        flag: ScopeBoundaryFlag,
    },
    PrivateEnvironmentFlagMismatch {
        scope: SemanticScopeId,
    },
    ParseGoalKindMismatch {
        scope: SemanticScopeId,
        kind: ScopeKind,
        goal: ParseSemanticGoal,
    },
    ModuleNotStrict {
        scope: SemanticScopeId,
    },
    MissingModuleRequest {
        specifier: ParserIdentifier,
    },
    ExportMissingFromScopeData {
        exported_name: ParserIdentifier,
    },
    UnorderedModuleSpan {
        span: SourceSpan,
    },
}

#[derive(Clone, Copy, Debug, Eq, PartialEq)]
pub enum ScopeBoundaryFlag {
    Function,
    StaticBlock,
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
    InvalidClassElement {
        name: Option<ParserIdentifier>,
        reason: ClassElementError,
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
    ConstructorName,
}

#[derive(Clone, Copy, Debug, Eq, PartialEq)]
pub enum ClassElementError {
    StaticPrototypeProperty,
    ConstructorAccessor,
}

#[derive(Clone, Copy, Debug, Eq, PartialEq)]
pub enum ControlFlowError {
    BreakOutsideLoopOrSwitch,
    ContinueOutsideLoop,
    ReturnOutsideFunction,
    NewTargetOutsideFunction,
}

#[cfg(test)]
mod tests {
    use super::*;

    fn span(start: u32, end: u32) -> SourceSpan {
        SourceSpan::new(
            crate::syntax::source::SourcePosition(start),
            crate::syntax::source::SourcePosition(end),
        )
    }

    fn scope(id: u32, parent: Option<u32>) -> Scope {
        Scope {
            id: SemanticScopeId(id),
            parent: parent.map(SemanticScopeId),
            kind: ScopeKind::Function,
            span: span(0, 1),
            labels: Vec::new(),
            declared: VariableEnvironment::default(),
            lexical: VariableEnvironment::default(),
            private_names: VariableEnvironment::default(),
            flags: ScopeFlags {
                function_boundary: true,
                ..ScopeFlags::default()
            },
            environment: EnvironmentSemanticRecord::default(),
            parse: ParseSemanticMetadata {
                goal: ParseSemanticGoal::Function,
                function: Some(FunctionParseSemanticMetadata::default()),
                ..ParseSemanticMetadata::default()
            },
        }
    }

    #[test]
    fn semantic_validation_accepts_parented_scope_graph() {
        let model = SemanticModel {
            scopes: vec![scope(0, None), scope(1, Some(0))],
            module: None,
            early_errors: Vec::new(),
        };

        assert!(model.validate().is_valid());
    }

    #[test]
    fn semantic_validation_reports_scope_and_module_mismatches() {
        let exported = ParserIdentifier(1);
        let requested = ParserIdentifier(2);
        let model = SemanticModel {
            scopes: vec![
                Scope {
                    id: SemanticScopeId(0),
                    parent: Some(SemanticScopeId(0)),
                    kind: ScopeKind::Block,
                    span: span(3, 2),
                    labels: Vec::new(),
                    declared: VariableEnvironment::default(),
                    lexical: VariableEnvironment::default(),
                    private_names: VariableEnvironment::default(),
                    flags: ScopeFlags {
                        function_boundary: true,
                        ..ScopeFlags::default()
                    },
                    environment: EnvironmentSemanticRecord::default(),
                    parse: ParseSemanticMetadata {
                        goal: ParseSemanticGoal::Script,
                        ..ParseSemanticMetadata::default()
                    },
                },
                scope(0, Some(9)),
            ],
            module: Some(ModuleAnalysis {
                exports: vec![ModuleExport {
                    exported_name: exported,
                    local_name: None,
                    module_request: Some(requested),
                    span: span(0, 1),
                    kind: ExportBindingKind::ReExport,
                }],
                ..ModuleAnalysis::default()
            }),
            early_errors: Vec::new(),
        };

        let report = model.validate();
        assert!(report
            .findings
            .contains(&SemanticValidationFinding::ScopeIsOwnParent {
                scope: SemanticScopeId(0),
            }));
        assert!(report
            .findings
            .contains(&SemanticValidationFinding::DuplicateScopeId {
                scope: SemanticScopeId(0),
            }));
        assert!(report
            .findings
            .contains(&SemanticValidationFinding::MissingModuleRequest {
                specifier: requested,
            }));
        assert!(report
            .findings
            .contains(&SemanticValidationFinding::ExportMissingFromScopeData {
                exported_name: exported,
            }));
    }

    #[test]
    fn semantic_analysis_reports_duplicate_lexical_and_private_names() {
        let name = ParserIdentifier(11);
        let private = ParserIdentifier(12);
        let mut semantics = EarlySemanticInfo::default();
        semantics.lexical_variables.record_declaration(Declaration {
            name,
            kind: DeclarationKind::Let,
            span: span(0, 1),
            import: DeclarationImportType::NotImported,
            flags: DeclarationFlags::default(),
        });
        semantics.lexical_variables.record_declaration(Declaration {
            name,
            kind: DeclarationKind::Const,
            span: span(2, 3),
            import: DeclarationImportType::NotImported,
            flags: DeclarationFlags::default(),
        });
        semantics
            .lexical_variables
            .record_private_name(PrivateNameDeclaration {
                name: private,
                kind: PrivateNameKind::Field { is_static: true },
                span: span(4, 5),
            });
        semantics
            .lexical_variables
            .record_private_name(PrivateNameDeclaration {
                name: private,
                kind: PrivateNameKind::Method { is_static: true },
                span: span(6, 7),
            });
        let scope = ScopeNode {
            id: SemanticScopeId(0),
            kind: ScopeNodeKind::Script,
            span: span(0, 8),
            statements: Vec::new(),
            semantics,
            module: None,
        };

        let model = analyze_scope_node(&scope, None, ScopeKind::Global);

        assert!(model.early_errors.contains(&EarlyError {
            span: span(2, 3),
            kind: EarlyErrorKind::DuplicateDeclaration(name),
        }));
        assert!(model.early_errors.contains(&EarlyError {
            span: span(6, 7),
            kind: EarlyErrorKind::InvalidPrivateName {
                name: private,
                reason: PrivateNameError::Duplicate,
            },
        }));
    }

    #[test]
    fn semantic_analysis_populates_module_scope_exports() {
        let exported = ParserIdentifier(20);
        let local = ParserIdentifier(21);
        let scope = ScopeNode {
            id: SemanticScopeId(0),
            kind: ScopeNodeKind::Module,
            span: span(0, 4),
            statements: Vec::new(),
            semantics: EarlySemanticInfo::default(),
            module: Some(ModuleAnalysis {
                exports: vec![ModuleExport {
                    exported_name: exported,
                    local_name: Some(local),
                    module_request: None,
                    span: span(0, 1),
                    kind: ExportBindingKind::Local,
                }],
                ..ModuleAnalysis::default()
            }),
        };

        let model = analyze_scope_node(&scope, None, ScopeKind::Module);
        let module = model.module.as_ref().expect("module analysis");

        assert_eq!(module.scope_data.exported_names, vec![exported]);
        assert_eq!(
            module.scope_data.exported_bindings,
            vec![ModuleExportBinding {
                local_name: local,
                exported_names: vec![exported],
            }]
        );
    }

    #[test]
    fn semantic_analysis_reports_strict_parameter_private_reference_and_class_errors() {
        let parameter = ParserIdentifier(30);
        let missing_private = ParserIdentifier(31);
        let mut semantics = EarlySemanticInfo {
            strict: true,
            has_non_simple_parameters: true,
            ..EarlySemanticInfo::default()
        };
        semantics
            .declared_variables
            .record_declaration(Declaration {
                name: parameter,
                kind: DeclarationKind::Parameter,
                span: span(0, 1),
                import: DeclarationImportType::NotImported,
                flags: DeclarationFlags::default(),
            });
        semantics
            .declared_variables
            .record_declaration(Declaration {
                name: parameter,
                kind: DeclarationKind::Parameter,
                span: span(2, 3),
                import: DeclarationImportType::NotImported,
                flags: DeclarationFlags::default(),
            });
        semantics
            .private_name_references
            .push(PrivateNameReference {
                name: missing_private,
                span: span(4, 5),
                kind: PrivateNameReferenceKind::Get,
            });
        semantics.class_elements.push(ClassElementSemanticRecord {
            name: ClassElementSemanticName::Public(ParserIdentifier(
                WellKnownIdentifier::Prototype as u32,
            )),
            kind: ClassElementSemanticKind::Field,
            is_static: true,
            span: span(6, 7),
        });

        let scope = ScopeNode {
            id: SemanticScopeId(0),
            kind: ScopeNodeKind::Function,
            span: span(0, 8),
            statements: Vec::new(),
            semantics,
            module: None,
        };
        let model = analyze_scope_node(&scope, None, ScopeKind::Function);

        assert!(model.early_errors.contains(&EarlyError {
            span: span(2, 3),
            kind: EarlyErrorKind::DuplicateDeclaration(parameter),
        }));
        assert!(model.early_errors.contains(&EarlyError {
            span: span(4, 5),
            kind: EarlyErrorKind::UnboundPrivateName(missing_private),
        }));
        assert!(model.early_errors.contains(&EarlyError {
            span: span(6, 7),
            kind: EarlyErrorKind::InvalidClassElement {
                name: Some(ParserIdentifier(WellKnownIdentifier::Prototype as u32)),
                reason: ClassElementError::StaticPrototypeProperty,
            },
        }));
        assert_eq!(model.scopes[0].parse.strictness, SemanticStrictness::Strict);
        assert_eq!(
            model.scopes[0].parse.function,
            Some(FunctionParseSemanticMetadata {
                has_non_simple_parameters: true,
                needs_class_field_initializer: true,
                ..FunctionParseSemanticMetadata::default()
            })
        );
    }

    #[test]
    fn semantic_validation_checks_environment_and_parse_metadata_boundaries() {
        let mut broken = scope(3, None);
        broken.environment.private_name_count = 1;
        broken.parse.goal = ParseSemanticGoal::Module;

        let report = SemanticModel {
            scopes: vec![broken],
            module: None,
            early_errors: Vec::new(),
        }
        .validate();

        assert!(report.findings.contains(
            &SemanticValidationFinding::PrivateEnvironmentFlagMismatch {
                scope: SemanticScopeId(3),
            }
        ));
        assert!(report
            .findings
            .contains(&SemanticValidationFinding::ParseGoalKindMismatch {
                scope: SemanticScopeId(3),
                kind: ScopeKind::Function,
                goal: ParseSemanticGoal::Module,
            }));
    }
}
