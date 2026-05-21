//! Bytecompiler front-end contracts.
//!
//! JavaScriptCore's bytecompiler owns the semantic handoff from parsed syntax
//! into unlinked bytecode: register allocation, label scopes, static property
//! analysis, profiling flags, and generator state. The `bytecode` module owns
//! the representation of instructions and code blocks; this module owns the
//! source-to-bytecode planning boundary.

use std::collections::{HashMap, HashSet};

use crate::bytecode::{
    BytecodeGenerator, BytecodeIndex, BytecodeRange, CodeFeatures as BytecodeCodeFeatures,
    CodeKind, CoreOpcode, GenerationPlan, GenerationRoot, GenerationValidationFinding, HandlerKind,
    Label, LabelRef, Operand, OperandWidth, ParseMode as BytecodeParseMode, RegisterAllocator,
    RegisterFrameShape, SourceProvenance, SpecialRegisters, TemporaryLifetime, UnlinkedCodeBlock,
    UnlinkedHandlerInfo, VirtualRegister,
};
use crate::syntax::ast::{
    ArrayLiteralElement, AssignmentExpr, AssignmentOperator, AstPropertyKey, BinaryExpr,
    BinaryOperator as AstBinaryOperator, CallExpr, ClassElementKind, ClassElementName, ClassExpr,
    ConditionalExpr, ControlKind, DeclarationStmt, Expr, ForInit, ForOfBinding, FunctionMetadata,
    LiteralKind, MemberExpr, MemberKind, NameKind, NewExpr, NumberLiteralValue,
    ObjectLiteralPropertyKind, Pattern, Stmt, UnaryExpr, UnaryOperator as AstUnaryOperator,
};
use crate::syntax::{
    AstRef, AstRoot, CodeFeatures, EnvironmentSemanticRecord, ModuleAnalysis,
    ModuleParseSemanticMetadata, ParseMode as SyntaxParseMode, ParseSemanticGoal,
    ParseSemanticMetadata, ParsedAst, ParserArena, ParserIdentifier, SemanticModel,
    SemanticScopeId, SemanticStrictness, SourceCode,
};

/// Bytecompiler-owned planning-session identity.
///
/// This ID groups frontend staging records for one source-to-bytecode pass. It
/// has no runtime lifetime and must not be reused as code-block, executable, or
/// source-provider identity.
#[derive(Clone, Copy, Debug, Default, Eq, Hash, PartialEq)]
pub struct BytecompilerSessionId(pub u64);

#[derive(Clone, Copy, Debug, Eq, PartialEq)]
pub enum BytecompilerMode {
    Program,
    Function,
    Eval,
    Module,
    Builtin,
}

#[derive(Clone, Copy, Debug, Eq, PartialEq)]
pub enum BytecompilerPhase {
    ParseProductInspection,
    StaticPropertyAnalysis,
    ScopePlanning,
    TdzPlanning,
    ControlFlowPlanning,
    RegisterPlanning,
    LabelResolution,
    HandlerPlanning,
    BytecodeEmission,
    UnlinkedCodeBlockAssembly,
}

#[derive(Clone, Debug)]
pub struct BytecompilerInput {
    pub session: BytecompilerSessionId,
    /// Borrowed parse-time source view; persistent provider identity is carried
    /// later by `bytecode::SourceProviderId`.
    pub source: SourceCode,
    pub root: AstRoot,
    pub mode: BytecompilerMode,
    pub code_features: CodeFeatures,
    pub module_analysis: Option<ModuleAnalysis>,
    pub semantic_model: Option<SemanticModel>,
}

#[derive(Clone, Debug, Eq, PartialEq)]
pub enum BytecompilerParseHandoffError {
    MissingRootScope,
    RootModeMismatch {
        root: &'static str,
        mode: SyntaxParseMode,
    },
}

#[derive(Clone, Copy, Debug, Eq, PartialEq)]
pub enum BytecompilerEmissionError {
    MissingRootScope,
    MissingStatement,
    MissingExpression,
    MissingPattern,
    UnsupportedStatement(&'static str),
    UnsupportedExpression(&'static str),
    UnsupportedLiteral(&'static str),
    UnsupportedAssignmentTarget,
    UnboundIdentifier(ParserIdentifier),
    MissingInt32Literal,
}

pub fn bytecompiler_input_from_parsed_ast(
    session: BytecompilerSessionId,
    source: SourceCode,
    parsed: &ParsedAst,
    arena: &ParserArena,
) -> Result<BytecompilerInput, BytecompilerParseHandoffError> {
    let (root_scope, mode) = match (parsed.root, parsed.mode) {
        (AstRoot::Script(root), SyntaxParseMode::Program) => (root, BytecompilerMode::Program),
        (AstRoot::Script(root), SyntaxParseMode::Eval) => (root, BytecompilerMode::Eval),
        (AstRoot::Function(root), SyntaxParseMode::FunctionBody) => {
            (root, BytecompilerMode::Function)
        }
        (AstRoot::Module(root), SyntaxParseMode::Module) => (root, BytecompilerMode::Module),
        (AstRoot::Script(_), mode) => {
            return Err(BytecompilerParseHandoffError::RootModeMismatch {
                root: "script",
                mode,
            });
        }
        (AstRoot::Function(_), mode) => {
            return Err(BytecompilerParseHandoffError::RootModeMismatch {
                root: "function",
                mode,
            });
        }
        (AstRoot::Module(_), mode) => {
            return Err(BytecompilerParseHandoffError::RootModeMismatch {
                root: "module",
                mode,
            });
        }
    };
    let scope = arena
        .scope_node(root_scope)
        .ok_or(BytecompilerParseHandoffError::MissingRootScope)?;
    Ok(BytecompilerInput {
        session,
        source,
        root: parsed.root,
        mode,
        code_features: scope.semantics.features,
        module_analysis: scope.module.clone(),
        semantic_model: None,
    })
}

#[derive(Clone, Debug, Default, Eq, PartialEq)]
pub struct LabelScopePlan {
    pub kind: LabelScopeKind,
    pub name: Option<ParserIdentifier>,
    pub scope_depth: u32,
    pub break_target: Option<Label>,
    pub continue_target: Option<Label>,
    pub lexical_scope: Option<SemanticScopeId>,
    pub consumes_dynamic_scope: bool,
}

#[derive(Clone, Copy, Debug, Default, Eq, PartialEq)]
pub enum LabelScopeKind {
    #[default]
    Loop,
    Switch,
    NamedLabel,
}

/// Abstract completion record used for finally-block planning.
///
/// JSC encodes break and continue completions as bytecode-offset-derived jump
/// IDs. Rust keeps the source distinction explicit so label/finally planning
/// remains separate from instruction encoding.
#[derive(Clone, Copy, Debug, Eq, PartialEq)]
pub enum CompletionRecordKind {
    Normal,
    Throw,
    Return,
    Break { statement: BytecodeIndex },
    Continue { statement: BytecodeIndex },
}

#[derive(Clone, Debug, Default, Eq, PartialEq)]
pub struct FinallyContextPlan {
    pub finally_label: Option<Label>,
    pub outer: Option<u32>,
    pub completion_type_register: Option<VirtualRegister>,
    pub completion_value_register: Option<VirtualRegister>,
    pub handles_returns: bool,
    pub registered_jumps: Vec<FinallyJumpPlan>,
}

#[derive(Clone, Copy, Debug, Eq, PartialEq)]
pub struct FinallyJumpPlan {
    pub completion: CompletionRecordKind,
    pub target_lexical_scope_index: i32,
    pub target_label: Label,
}

#[derive(Clone, Debug, Eq, PartialEq)]
pub struct ControlFlowScopePlan {
    pub kind: ControlFlowScopeKind,
    pub lexical_scope_index: i32,
    pub finally_context: Option<u32>,
}

#[derive(Clone, Copy, Debug, Eq, PartialEq)]
pub enum ControlFlowScopeKind {
    Label,
    Finally,
}

/// Bytecompiler variable-resolution outcome.
///
/// A resolved variable may point to stack, scope, or special storage. An
/// unresolved variable remains a dynamic lookup and must not be treated as a
/// local register by later bytecode contracts.
#[derive(Clone, Debug, Eq, PartialEq)]
pub struct BytecompilerVariable {
    pub name: ParserIdentifier,
    pub resolution: VariableResolution,
    pub attributes: VariableAttributes,
    pub kind: VariableKind,
    pub symbol_table_constant_index: Option<u32>,
    pub lexically_scoped: bool,
}

#[derive(Clone, Copy, Debug, Eq, PartialEq)]
pub enum VariableResolution {
    Unresolved,
    Stack(VirtualRegister),
    Scope { scope: VirtualRegister, offset: u32 },
    DirectArgumentsObject { offset: u32 },
    Module { import_slot: u32 },
}

#[derive(Clone, Copy, Debug, Default, Eq, PartialEq)]
pub struct VariableAttributes {
    pub read_only: bool,
    pub captured: bool,
    pub imported: bool,
    pub exported: bool,
}

#[derive(Clone, Copy, Debug, Eq, PartialEq)]
pub enum VariableKind {
    Normal,
    Special,
}

/// TDZ state carried by bytecode generation.
///
/// TDZ mutation authority belongs to the bytecompiler while scopes are being
/// entered and exited. The produced records are frozen into generation plans or
/// executable rare data for nested functions.
#[derive(Clone, Debug, Default, Eq, PartialEq)]
pub struct TdzPlan {
    pub parent_link: Option<TdzEnvironmentLink>,
    pub entries: Vec<TdzEntry>,
    pub preserved_stack_depth: u32,
}

#[derive(Clone, Copy, Debug, Eq, PartialEq, Hash)]
pub struct TdzEnvironmentLink(pub u32);

#[derive(Clone, Copy, Debug, Eq, PartialEq)]
pub struct TdzEntry {
    pub name: ParserIdentifier,
    pub state: TdzRequirement,
    pub scope: Option<SemanticScopeId>,
}

#[derive(Clone, Copy, Debug, Eq, PartialEq)]
pub enum TdzRequirement {
    UnderTdz,
    NotUnderTdz,
}

#[derive(Clone, Copy, Debug, Eq, PartialEq)]
pub enum TdzCheckOptimization {
    Optimize,
    DoNotOptimize,
}

#[derive(Clone, Debug, Default, Eq, PartialEq)]
pub struct LexicalScopeStackPlan {
    pub entries: Vec<LexicalScopeStackEntry>,
    pub var_scope_index: Option<u32>,
    pub local_scope_count: u32,
}

#[derive(Clone, Copy, Debug, Eq, PartialEq)]
pub struct LexicalScopeStackEntry {
    pub scope: SemanticScopeId,
    pub scope_register: Option<VirtualRegister>,
    pub symbol_table_constant_index: Option<u32>,
    pub kind: LexicalScopeKind,
}

#[derive(Clone, Copy, Debug, Eq, PartialEq)]
pub enum LexicalScopeKind {
    Catch,
    CatchWithSimpleParameter,
    LetConst,
    FunctionName,
    Class,
    With,
}

#[derive(Clone, Debug, Default, Eq, PartialEq)]
pub struct ForInContextPlan {
    pub local: Option<VirtualRegister>,
    pub property_name: Option<VirtualRegister>,
    pub property_offset: Option<VirtualRegister>,
    pub enumerator: Option<VirtualRegister>,
    pub mode: Option<VirtualRegister>,
    pub base_variable: Option<BytecompilerVariable>,
    pub body_start: Option<BytecodeIndex>,
    pub state: ForInContextState,
}

#[derive(Clone, Copy, Debug, Default, Eq, PartialEq)]
pub enum ForInContextState {
    #[default]
    Open,
    Invalidated,
    Finalized,
}

#[derive(Clone, Debug, Default, Eq, PartialEq)]
pub struct TryContextPlan {
    pub active: Vec<TryContextEntry>,
    pub ranges: Vec<TryRangePlan>,
}

#[derive(Clone, Copy, Debug, Eq, PartialEq)]
pub struct TryContextEntry {
    pub start: Label,
    pub handler: TryHandlerPlan,
}

#[derive(Clone, Copy, Debug, Eq, PartialEq)]
pub struct TryRangePlan {
    pub start: Label,
    pub end: Label,
    pub handler: TryHandlerPlan,
}

#[derive(Clone, Copy, Debug, Eq, PartialEq)]
pub struct TryHandlerPlan {
    pub target: Label,
    pub kind: TryHandlerKind,
}

#[derive(Clone, Copy, Debug, Eq, PartialEq)]
pub enum TryHandlerKind {
    Catch,
    Finally,
    SynthesizedCatch,
    SynthesizedFinally,
}

#[derive(Clone, Debug, Default, Eq, PartialEq)]
pub struct UsingScopePlan {
    pub slots: Vec<UsingSlotPlan>,
    pub next_slot: u32,
    pub has_await_using: bool,
}

#[derive(Clone, Copy, Debug, Eq, PartialEq)]
pub struct UsingSlotPlan {
    pub value: VirtualRegister,
    pub method: VirtualRegister,
    pub reached: VirtualRegister,
    pub is_async: bool,
}

#[derive(Clone, Debug, Default, Eq, PartialEq)]
pub struct OptionalChainPlan {
    pub targets: Vec<LabelRef>,
}

#[derive(Clone, Copy, Debug, Eq, PartialEq)]
pub enum StaticPropertyAccessKind {
    DirectName,
    PrivateName,
    NumericIndex,
    Computed,
    Spread,
    Super,
}

#[derive(Clone, Debug, Eq, PartialEq)]
pub struct StaticPropertyAccess {
    pub expression: AstRef<Expr>,
    pub kind: StaticPropertyAccessKind,
    pub cacheable_without_side_effect: bool,
    pub requires_private_brand_check: bool,
}

#[derive(Clone, Debug, Default, Eq, PartialEq)]
pub struct StaticPropertyAnalysisPlan {
    pub accesses: Vec<StaticPropertyAccess>,
    pub creates_structure_literals: bool,
    pub needs_computed_name_temporaries: bool,
}

#[derive(Clone, Copy, Debug, Eq, PartialEq)]
pub enum BytecompilerProfileFlag {
    None,
    ClosureVariableTypeProfile,
    LocallyResolvedTypeProfile,
    GlobalIdlessTypeProfile,
    FunctionArgumentTypeProfile,
    FunctionReturnTypeProfile,
    SuperSampler,
}

#[derive(Clone, Debug)]
pub struct BytecompilerOutputPlan {
    pub phase: BytecompilerPhase,
    pub generation: GenerationPlan,
    pub registers: RegisterAllocator,
    pub labels: Vec<LabelScopePlan>,
    pub control_flow: Vec<ControlFlowScopePlan>,
    pub finally_contexts: Vec<FinallyContextPlan>,
    pub lexical_scopes: LexicalScopeStackPlan,
    pub tdz: TdzPlan,
    pub for_in_contexts: Vec<ForInContextPlan>,
    pub try_contexts: TryContextPlan,
    pub using_scopes: Vec<UsingScopePlan>,
    pub optional_chains: OptionalChainPlan,
    pub static_properties: StaticPropertyAnalysisPlan,
    pub semantic: BytecompilerSemanticPlan,
    pub profile_flags: Vec<BytecompilerProfileFlag>,
    pub unlinked_code: Option<UnlinkedCodeBlock>,
    pub function_bodies: Vec<UnlinkedCodeBlock>,
    pub literal_strings: HashMap<u32, String>,
}

impl BytecompilerOutputPlan {
    pub fn new(phase: BytecompilerPhase, generation: GenerationPlan) -> Self {
        Self {
            phase,
            registers: RegisterAllocator::new(generation.registers),
            generation,
            labels: Vec::new(),
            control_flow: Vec::new(),
            finally_contexts: Vec::new(),
            lexical_scopes: LexicalScopeStackPlan::default(),
            tdz: TdzPlan::default(),
            for_in_contexts: Vec::new(),
            try_contexts: TryContextPlan::default(),
            using_scopes: Vec::new(),
            optional_chains: OptionalChainPlan::default(),
            static_properties: StaticPropertyAnalysisPlan::default(),
            semantic: BytecompilerSemanticPlan::default(),
            profile_flags: Vec::new(),
            unlinked_code: None,
            function_bodies: Vec::new(),
            literal_strings: HashMap::new(),
        }
    }

    pub fn validate(&self) -> BytecompilerValidationReport {
        let mut findings = Vec::new();
        for finding in self.generation.validate().findings {
            findings.push(BytecompilerValidationFinding::GenerationPlan { finding });
        }
        validate_unlinked_code_presence(self.phase, self.unlinked_code.as_ref(), &mut findings);
        validate_finally_contexts(&self.finally_contexts, &mut findings);
        validate_lexical_scopes(&self.lexical_scopes, &mut findings);
        validate_for_in_contexts(&self.for_in_contexts, &mut findings);
        validate_using_scopes(&self.using_scopes, &mut findings);
        validate_try_contexts(&self.try_contexts, &mut findings);
        validate_semantic_plan(&self.semantic, &self.generation, &mut findings);
        BytecompilerValidationReport { findings }
    }
}

#[derive(Clone, Debug, Default, Eq, PartialEq)]
pub struct BytecompilerSemanticPlan {
    pub parse: ParseSemanticMetadata,
    pub environments: Vec<EnvironmentSemanticRecord>,
    pub module: Option<ModuleParseSemanticMetadata>,
}

pub fn plan_bytecompiler_input(
    input: &BytecompilerInput,
    registers: RegisterFrameShape,
) -> BytecompilerOutputPlan {
    let (kind, parse_mode, root) = match input.mode {
        BytecompilerMode::Program => (
            CodeKind::Program,
            BytecodeParseMode::Program,
            GenerationRoot::ProgramNode,
        ),
        BytecompilerMode::Function | BytecompilerMode::Builtin => (
            CodeKind::Function,
            BytecodeParseMode::NormalFunction,
            GenerationRoot::FunctionNode,
        ),
        BytecompilerMode::Eval => (
            CodeKind::Eval,
            BytecodeParseMode::Eval,
            GenerationRoot::EvalNode,
        ),
        BytecompilerMode::Module => (
            CodeKind::Module,
            BytecodeParseMode::Module,
            GenerationRoot::ModuleProgramNode,
        ),
    };
    let mut generation = GenerationPlan::new(kind, parse_mode);
    generation.registers = registers;
    generation.source = SourceProvenance {
        start_offset: input.source.range().start.0,
        source_length: input.source.range().unit_len(),
        first_line: input.source.first_line(),
        start_column: input.source.start_column(),
        source_url: input.source.provider().origin().source_url.clone(),
        pre_redirect_url: input.source.provider().origin().pre_redirect_url.clone(),
        ..SourceProvenance::default()
    };
    generation.features = map_code_features(input.code_features);
    generation.root = root;

    let mut plan =
        BytecompilerOutputPlan::new(BytecompilerPhase::ParseProductInspection, generation);
    plan.semantic = if let Some(model) = &input.semantic_model {
        let parse = model
            .scopes
            .first()
            .map(|scope| scope.parse)
            .unwrap_or_else(|| synthesize_parse_semantics(input));
        BytecompilerSemanticPlan {
            parse,
            environments: model.scopes.iter().map(|scope| scope.environment).collect(),
            module: parse
                .module
                .or_else(|| input.module_analysis.as_ref().map(module_parse_metadata)),
        }
    } else {
        let parse = synthesize_parse_semantics(input);
        BytecompilerSemanticPlan {
            parse,
            environments: Vec::new(),
            module: parse
                .module
                .or_else(|| input.module_analysis.as_ref().map(module_parse_metadata)),
        }
    };
    plan.generation.environment.strict_mode =
        plan.semantic.parse.strictness == SemanticStrictness::Strict;
    plan.generation.environment.allows_direct_eval_cache =
        !plan.semantic.parse.features.eval && !plan.generation.features.no_eval_cache;
    if let Some(module) = &input.module_analysis {
        plan.lexical_scopes.local_scope_count = module.scope_data.exported_bindings.len() as u32;
    }
    plan
}

pub fn emit_unlinked_code_from_parsed_ast(
    input: &BytecompilerInput,
    arena: &ParserArena,
) -> Result<BytecompilerOutputPlan, BytecompilerEmissionError> {
    let mut plan = plan_bytecompiler_input(input, core_execution_frame_shape());
    let root = root_scope_ref(input.root);
    let scope = arena
        .scope_node(root)
        .cloned()
        .ok_or(BytecompilerEmissionError::MissingRootScope)?;
    collect_scope_string_literals(arena, &scope, &mut plan.literal_strings)?;
    let function_plan = collect_function_compilation_plan(arena, &scope)?;
    let mut generator = BytecodeGenerator::new(plan.generation.clone());
    let mut emitter = AstBytecodeEmitter {
        arena,
        generator: &mut generator,
        locals: HashMap::new(),
        captured_bindings: function_plan
            .captures
            .values()
            .flat_map(|captures| captures.iter().copied())
            .collect(),
        initialized_cells: HashSet::new(),
        function_metadata_indices: function_plan.metadata_indices.clone(),
        function_captures: function_plan.captures.clone(),
        loop_stack: Vec::new(),
        finally_stack: Vec::new(),
    };
    let mut function_bodies = Vec::with_capacity(function_plan.metadata_order.len());
    for metadata in &function_plan.metadata_order {
        function_bodies.push(emitter.emit_function_body(*metadata)?);
    }
    emitter.predeclare_scope_locals(&scope)?;
    emitter.emit_intrinsic_bindings()?;
    emitter.initialize_captured_local_cells()?;
    emitter.emit_scope_function_bindings(&scope)?;
    let mut last_value = None;
    let mut terminated = false;
    for statement in &scope.statements {
        if terminated {
            break;
        }
        let emission = emitter.emit_statement(*statement)?;
        last_value = emission.value;
        terminated = emission.terminated;
    }
    if !terminated {
        let value = match last_value {
            Some(value) => value,
            None => emitter.emit_load_undefined()?,
        };
        emitter.emit_return(value);
    }
    let output = generator.finish();
    let code_block = install_code_block_literal_text_table(arena, output.code_block)?;
    plan.phase = BytecompilerPhase::UnlinkedCodeBlockAssembly;
    plan.generation.registers = code_block.frame();
    plan.unlinked_code = Some(code_block);
    plan.function_bodies = function_bodies;
    Ok(plan)
}

fn core_execution_frame_shape() -> RegisterFrameShape {
    function_execution_frame_shape(0)
}

fn function_execution_frame_shape(parameter_count: u32) -> RegisterFrameShape {
    RegisterFrameShape {
        num_parameters_including_this: parameter_count.saturating_add(1),
        num_vars: 1,
        num_callee_locals: 1,
        special: SpecialRegisters {
            this_register: VirtualRegister::argument_or_header(5),
            scope_register: VirtualRegister::local(0),
            ..SpecialRegisters::default()
        },
        ..RegisterFrameShape::default()
    }
}

fn root_scope_ref(root: AstRoot) -> AstRef<crate::syntax::ScopeNode> {
    match root {
        AstRoot::Script(root) | AstRoot::Module(root) | AstRoot::Function(root) => root,
    }
}

#[derive(Clone, Debug, Default)]
struct FunctionCompilationPlan {
    metadata_order: Vec<AstRef<FunctionMetadata>>,
    metadata_indices: HashMap<u32, u32>,
    captures: HashMap<u32, Vec<ParserIdentifier>>,
}

fn collect_function_compilation_plan(
    arena: &ParserArena,
    scope: &crate::syntax::ScopeNode,
) -> Result<FunctionCompilationPlan, BytecompilerEmissionError> {
    let mut plan = FunctionCompilationPlan::default();
    collect_scope_function_metadata(arena, scope, &mut plan)?;
    let mut capture_memo = HashMap::new();
    for metadata in plan.metadata_order.clone() {
        let captures = collect_function_captures(arena, metadata, &mut capture_memo)?;
        plan.captures.insert(metadata.raw_index(), captures);
    }
    Ok(plan)
}

fn collect_scope_function_metadata(
    arena: &ParserArena,
    scope: &crate::syntax::ScopeNode,
    plan: &mut FunctionCompilationPlan,
) -> Result<(), BytecompilerEmissionError> {
    for statement in &scope.statements {
        collect_statement_function_metadata(arena, *statement, plan)?;
    }
    Ok(())
}

fn collect_statement_function_metadata(
    arena: &ParserArena,
    statement: AstRef<Stmt>,
    plan: &mut FunctionCompilationPlan,
) -> Result<(), BytecompilerEmissionError> {
    let statement = arena
        .statement(statement)
        .ok_or(BytecompilerEmissionError::MissingStatement)?;
    match statement {
        Stmt::Empty(_) => Ok(()),
        Stmt::Expression(expression) => {
            collect_expression_function_metadata(arena, *expression, plan)
        }
        Stmt::Block(block) => {
            let scope = arena
                .scope_node(block.scope)
                .ok_or(BytecompilerEmissionError::MissingRootScope)?;
            collect_scope_function_metadata(arena, scope, plan)
        }
        Stmt::Declaration(declaration) => {
            for initializer in declaration.initializers.iter().flatten() {
                collect_expression_function_metadata(arena, *initializer, plan)?;
            }
            Ok(())
        }
        Stmt::FunctionDeclaration(declaration) => {
            record_function_metadata(arena, declaration.metadata, plan)
        }
        Stmt::If(statement) => {
            collect_expression_function_metadata(arena, statement.condition, plan)?;
            collect_statement_function_metadata(arena, statement.consequent, plan)?;
            if let Some(alternate) = statement.alternate {
                collect_statement_function_metadata(arena, alternate, plan)?;
            }
            Ok(())
        }
        Stmt::While(statement) => {
            collect_expression_function_metadata(arena, statement.condition, plan)?;
            collect_statement_function_metadata(arena, statement.body, plan)
        }
        Stmt::For(statement) => {
            if let Some(init) = &statement.init {
                match init {
                    ForInit::Declaration(declaration) => {
                        for initializer in declaration.initializers.iter().flatten() {
                            collect_expression_function_metadata(arena, *initializer, plan)?;
                        }
                    }
                    ForInit::Expression(expression) => {
                        collect_expression_function_metadata(arena, *expression, plan)?;
                    }
                }
            }
            if let Some(condition) = statement.condition {
                collect_expression_function_metadata(arena, condition, plan)?;
            }
            if let Some(update) = statement.update {
                collect_expression_function_metadata(arena, update, plan)?;
            }
            collect_statement_function_metadata(arena, statement.body, plan)
        }
        Stmt::ForOf(statement) => {
            collect_expression_function_metadata(arena, statement.iterable, plan)?;
            collect_statement_function_metadata(arena, statement.body, plan)
        }
        Stmt::Try(statement) => {
            collect_statement_function_metadata(arena, statement.body, plan)?;
            if let Some(catch) = statement.catch {
                collect_statement_function_metadata(arena, catch.body, plan)?;
            }
            if let Some(finally) = statement.finally {
                collect_statement_function_metadata(arena, finally, plan)?;
            }
            Ok(())
        }
        Stmt::Control(control) => match control.kind {
            ControlKind::Return(Some(value)) => {
                collect_expression_function_metadata(arena, value, plan)
            }
            ControlKind::Throw(value) => collect_expression_function_metadata(arena, value, plan),
            ControlKind::Return(None) | ControlKind::Break(_) | ControlKind::Continue(_) => Ok(()),
        },
        Stmt::Module(_) => Ok(()),
    }
}

fn collect_expression_function_metadata(
    arena: &ParserArena,
    expression: AstRef<Expr>,
    plan: &mut FunctionCompilationPlan,
) -> Result<(), BytecompilerEmissionError> {
    let expression = arena
        .expression(expression)
        .ok_or(BytecompilerEmissionError::MissingExpression)?;
    match expression {
        Expr::Literal(_) | Expr::Name(_) | Expr::ImportMeta(_) => Ok(()),
        Expr::Unary(unary) => collect_expression_function_metadata(arena, unary.argument, plan),
        Expr::Binary(binary) => {
            collect_expression_function_metadata(arena, binary.left, plan)?;
            collect_expression_function_metadata(arena, binary.right, plan)
        }
        Expr::Assignment(assignment) => {
            collect_pattern_function_metadata(arena, assignment.target, plan)?;
            collect_expression_function_metadata(arena, assignment.value, plan)
        }
        Expr::Conditional(conditional) => {
            collect_expression_function_metadata(arena, conditional.test, plan)?;
            collect_expression_function_metadata(arena, conditional.consequent, plan)?;
            collect_expression_function_metadata(arena, conditional.alternate, plan)
        }
        Expr::Call(call) => {
            collect_expression_function_metadata(arena, call.callee, plan)?;
            for argument in &call.arguments {
                collect_expression_function_metadata(arena, *argument, plan)?;
            }
            Ok(())
        }
        Expr::New(new) => {
            collect_expression_function_metadata(arena, new.callee, plan)?;
            for argument in &new.arguments {
                collect_expression_function_metadata(arena, *argument, plan)?;
            }
            Ok(())
        }
        Expr::Member(member) => {
            collect_expression_function_metadata(arena, member.base, plan)?;
            if let MemberKind::Bracket(expression) = member.member {
                collect_expression_function_metadata(arena, expression, plan)?;
            }
            Ok(())
        }
        Expr::Object(object) => {
            for property in &object.properties {
                if let AstPropertyKey::Computed(expression) = property.key {
                    collect_expression_function_metadata(arena, expression, plan)?;
                }
                collect_expression_function_metadata(arena, property.value, plan)?;
            }
            Ok(())
        }
        Expr::Array(array) => {
            for element in &array.elements {
                match element {
                    ArrayLiteralElement::Expression(expression) => {
                        collect_expression_function_metadata(arena, *expression, plan)?;
                    }
                    ArrayLiteralElement::Spread { value, .. } => {
                        collect_expression_function_metadata(arena, *value, plan)?;
                    }
                    ArrayLiteralElement::Elision(_) => {}
                }
            }
            Ok(())
        }
        Expr::Function(metadata) => record_function_metadata(arena, *metadata, plan),
        Expr::Class(class) => {
            if let Some(heritage) = class.heritage {
                collect_expression_function_metadata(arena, heritage, plan)?;
            }
            for element in &class.elements {
                if let crate::syntax::ast::ClassElementName::Computed(expression) = element.name {
                    collect_expression_function_metadata(arena, expression, plan)?;
                }
                if element.is_static || element.metadata.is_none() {
                    if let Some(initializer) = element.initializer {
                        collect_expression_function_metadata(arena, initializer, plan)?;
                    }
                }
                if let Some(metadata) = element.metadata {
                    record_function_metadata(arena, metadata, plan)?;
                }
            }
            Ok(())
        }
        Expr::Template(template) => {
            if let Some(tag) = template.tag {
                collect_expression_function_metadata(arena, tag, plan)?;
            }
            for expression in &template.expressions {
                collect_expression_function_metadata(arena, *expression, plan)?;
            }
            Ok(())
        }
    }
}

fn collect_pattern_function_metadata(
    arena: &ParserArena,
    pattern: AstRef<Pattern>,
    plan: &mut FunctionCompilationPlan,
) -> Result<(), BytecompilerEmissionError> {
    let pattern = arena
        .pattern(pattern)
        .ok_or(BytecompilerEmissionError::MissingPattern)?;
    collect_owned_pattern_function_metadata(arena, pattern, plan)
}

fn collect_owned_pattern_function_metadata(
    arena: &ParserArena,
    pattern: &Pattern,
    plan: &mut FunctionCompilationPlan,
) -> Result<(), BytecompilerEmissionError> {
    match pattern {
        Pattern::AssignmentTarget(expression) => {
            collect_expression_function_metadata(arena, *expression, plan)
        }
        Pattern::Array(elements) => {
            for element in elements {
                collect_owned_pattern_function_metadata(arena, &element.pattern, plan)?;
                if let Some(default_value) = element.default_value {
                    collect_expression_function_metadata(arena, default_value, plan)?;
                }
            }
            Ok(())
        }
        Pattern::Object(properties) => {
            for property in properties {
                if let crate::syntax::ast::AstPropertyKey::Computed(expression) = property.key {
                    collect_expression_function_metadata(arena, expression, plan)?;
                }
                collect_owned_pattern_function_metadata(arena, &property.pattern, plan)?;
                if let Some(default_value) = property.default_value {
                    collect_expression_function_metadata(arena, default_value, plan)?;
                }
            }
            Ok(())
        }
        Pattern::Rest(pattern) => collect_owned_pattern_function_metadata(arena, pattern, plan),
        Pattern::Binding(_) => Ok(()),
    }
}

fn record_function_metadata(
    arena: &ParserArena,
    metadata: AstRef<FunctionMetadata>,
    plan: &mut FunctionCompilationPlan,
) -> Result<(), BytecompilerEmissionError> {
    if plan.metadata_indices.contains_key(&metadata.raw_index()) {
        return Ok(());
    }
    let index = plan.metadata_order.len().try_into().unwrap_or(u32::MAX);
    plan.metadata_indices.insert(metadata.raw_index(), index);
    plan.metadata_order.push(metadata);
    let metadata = arena.function_metadata(metadata).ok_or(
        BytecompilerEmissionError::UnsupportedStatement("function declaration is missing metadata"),
    )?;
    for parameter in &metadata.parameters {
        collect_pattern_function_metadata(arena, parameter.pattern, plan)?;
        if let Some(default_value) = parameter.default_value {
            collect_expression_function_metadata(arena, default_value, plan)?;
        }
    }
    let scope = arena
        .scope_node(metadata.body)
        .ok_or(BytecompilerEmissionError::MissingRootScope)?;
    collect_scope_function_metadata(arena, scope, plan)
}

fn collect_scope_immediate_function_metadata(
    arena: &ParserArena,
    scope: &crate::syntax::ScopeNode,
    functions: &mut Vec<AstRef<FunctionMetadata>>,
) -> Result<(), BytecompilerEmissionError> {
    for statement in &scope.statements {
        collect_statement_immediate_function_metadata(arena, *statement, functions)?;
    }
    Ok(())
}

fn collect_statement_immediate_function_metadata(
    arena: &ParserArena,
    statement: AstRef<Stmt>,
    functions: &mut Vec<AstRef<FunctionMetadata>>,
) -> Result<(), BytecompilerEmissionError> {
    let statement = arena
        .statement(statement)
        .ok_or(BytecompilerEmissionError::MissingStatement)?;
    match statement {
        Stmt::Empty(_) | Stmt::Module(_) => Ok(()),
        Stmt::Expression(expression) => {
            collect_expression_immediate_function_metadata(arena, *expression, functions)
        }
        Stmt::Block(block) => {
            let scope = arena
                .scope_node(block.scope)
                .ok_or(BytecompilerEmissionError::MissingRootScope)?;
            collect_scope_immediate_function_metadata(arena, scope, functions)
        }
        Stmt::Declaration(declaration) => {
            for initializer in declaration.initializers.iter().flatten() {
                collect_expression_immediate_function_metadata(arena, *initializer, functions)?;
            }
            Ok(())
        }
        Stmt::FunctionDeclaration(declaration) => {
            functions.push(declaration.metadata);
            Ok(())
        }
        Stmt::If(statement) => {
            collect_expression_immediate_function_metadata(arena, statement.condition, functions)?;
            collect_statement_immediate_function_metadata(arena, statement.consequent, functions)?;
            if let Some(alternate) = statement.alternate {
                collect_statement_immediate_function_metadata(arena, alternate, functions)?;
            }
            Ok(())
        }
        Stmt::While(statement) => {
            collect_expression_immediate_function_metadata(arena, statement.condition, functions)?;
            collect_statement_immediate_function_metadata(arena, statement.body, functions)
        }
        Stmt::For(statement) => {
            if let Some(init) = &statement.init {
                match init {
                    ForInit::Declaration(declaration) => {
                        for initializer in declaration.initializers.iter().flatten() {
                            collect_expression_immediate_function_metadata(
                                arena,
                                *initializer,
                                functions,
                            )?;
                        }
                    }
                    ForInit::Expression(expression) => {
                        collect_expression_immediate_function_metadata(
                            arena,
                            *expression,
                            functions,
                        )?;
                    }
                }
            }
            if let Some(condition) = statement.condition {
                collect_expression_immediate_function_metadata(arena, condition, functions)?;
            }
            if let Some(update) = statement.update {
                collect_expression_immediate_function_metadata(arena, update, functions)?;
            }
            collect_statement_immediate_function_metadata(arena, statement.body, functions)
        }
        Stmt::ForOf(statement) => {
            collect_expression_immediate_function_metadata(arena, statement.iterable, functions)?;
            collect_statement_immediate_function_metadata(arena, statement.body, functions)
        }
        Stmt::Try(statement) => {
            collect_statement_immediate_function_metadata(arena, statement.body, functions)?;
            if let Some(catch) = statement.catch {
                collect_statement_immediate_function_metadata(arena, catch.body, functions)?;
            }
            if let Some(finally) = statement.finally {
                collect_statement_immediate_function_metadata(arena, finally, functions)?;
            }
            Ok(())
        }
        Stmt::Control(control) => match control.kind {
            ControlKind::Return(Some(value)) | ControlKind::Throw(value) => {
                collect_expression_immediate_function_metadata(arena, value, functions)
            }
            ControlKind::Return(None) | ControlKind::Break(_) | ControlKind::Continue(_) => Ok(()),
        },
    }
}

fn collect_expression_immediate_function_metadata(
    arena: &ParserArena,
    expression: AstRef<Expr>,
    functions: &mut Vec<AstRef<FunctionMetadata>>,
) -> Result<(), BytecompilerEmissionError> {
    let expression = arena
        .expression(expression)
        .ok_or(BytecompilerEmissionError::MissingExpression)?;
    match expression {
        Expr::Literal(_) | Expr::Name(_) | Expr::ImportMeta(_) => Ok(()),
        Expr::Unary(unary) => {
            collect_expression_immediate_function_metadata(arena, unary.argument, functions)
        }
        Expr::Binary(binary) => {
            collect_expression_immediate_function_metadata(arena, binary.left, functions)?;
            collect_expression_immediate_function_metadata(arena, binary.right, functions)
        }
        Expr::Assignment(assignment) => {
            collect_pattern_immediate_function_metadata(arena, assignment.target, functions)?;
            collect_expression_immediate_function_metadata(arena, assignment.value, functions)
        }
        Expr::Conditional(conditional) => {
            collect_expression_immediate_function_metadata(arena, conditional.test, functions)?;
            collect_expression_immediate_function_metadata(
                arena,
                conditional.consequent,
                functions,
            )?;
            collect_expression_immediate_function_metadata(arena, conditional.alternate, functions)
        }
        Expr::Call(call) => {
            collect_expression_immediate_function_metadata(arena, call.callee, functions)?;
            for argument in &call.arguments {
                collect_expression_immediate_function_metadata(arena, *argument, functions)?;
            }
            Ok(())
        }
        Expr::New(new) => {
            collect_expression_immediate_function_metadata(arena, new.callee, functions)?;
            for argument in &new.arguments {
                collect_expression_immediate_function_metadata(arena, *argument, functions)?;
            }
            Ok(())
        }
        Expr::Member(member) => {
            collect_expression_immediate_function_metadata(arena, member.base, functions)?;
            if let MemberKind::Bracket(expression) = member.member {
                collect_expression_immediate_function_metadata(arena, expression, functions)?;
            }
            Ok(())
        }
        Expr::Object(object) => {
            for property in &object.properties {
                if let AstPropertyKey::Computed(expression) = property.key {
                    collect_expression_immediate_function_metadata(arena, expression, functions)?;
                }
                collect_expression_immediate_function_metadata(arena, property.value, functions)?;
            }
            Ok(())
        }
        Expr::Array(array) => {
            for element in &array.elements {
                match element {
                    ArrayLiteralElement::Expression(expression) => {
                        collect_expression_immediate_function_metadata(
                            arena,
                            *expression,
                            functions,
                        )?;
                    }
                    ArrayLiteralElement::Spread { value, .. } => {
                        collect_expression_immediate_function_metadata(arena, *value, functions)?;
                    }
                    ArrayLiteralElement::Elision(_) => {}
                }
            }
            Ok(())
        }
        Expr::Function(metadata) => {
            functions.push(*metadata);
            Ok(())
        }
        Expr::Class(class) => {
            if let Some(heritage) = class.heritage {
                collect_expression_immediate_function_metadata(arena, heritage, functions)?;
            }
            for element in &class.elements {
                if let crate::syntax::ast::ClassElementName::Computed(expression) = element.name {
                    collect_expression_immediate_function_metadata(arena, expression, functions)?;
                }
                if element.is_static || element.metadata.is_none() {
                    if let Some(initializer) = element.initializer {
                        collect_expression_immediate_function_metadata(
                            arena,
                            initializer,
                            functions,
                        )?;
                    }
                }
                if let Some(metadata) = element.metadata {
                    functions.push(metadata);
                }
            }
            Ok(())
        }
        Expr::Template(template) => {
            if let Some(tag) = template.tag {
                collect_expression_immediate_function_metadata(arena, tag, functions)?;
            }
            for expression in &template.expressions {
                collect_expression_immediate_function_metadata(arena, *expression, functions)?;
            }
            Ok(())
        }
    }
}

fn collect_pattern_immediate_function_metadata(
    arena: &ParserArena,
    pattern: AstRef<Pattern>,
    functions: &mut Vec<AstRef<FunctionMetadata>>,
) -> Result<(), BytecompilerEmissionError> {
    let pattern = arena
        .pattern(pattern)
        .ok_or(BytecompilerEmissionError::MissingPattern)?;
    collect_owned_pattern_immediate_function_metadata(arena, pattern, functions)
}

fn collect_owned_pattern_immediate_function_metadata(
    arena: &ParserArena,
    pattern: &Pattern,
    functions: &mut Vec<AstRef<FunctionMetadata>>,
) -> Result<(), BytecompilerEmissionError> {
    match pattern {
        Pattern::AssignmentTarget(expression) => {
            collect_expression_immediate_function_metadata(arena, *expression, functions)
        }
        Pattern::Array(elements) => {
            for element in elements {
                collect_owned_pattern_immediate_function_metadata(
                    arena,
                    &element.pattern,
                    functions,
                )?;
                if let Some(default_value) = element.default_value {
                    collect_expression_immediate_function_metadata(
                        arena,
                        default_value,
                        functions,
                    )?;
                }
            }
            Ok(())
        }
        Pattern::Object(properties) => {
            for property in properties {
                if let crate::syntax::ast::AstPropertyKey::Computed(expression) = property.key {
                    collect_expression_immediate_function_metadata(arena, expression, functions)?;
                }
                collect_owned_pattern_immediate_function_metadata(
                    arena,
                    &property.pattern,
                    functions,
                )?;
                if let Some(default_value) = property.default_value {
                    collect_expression_immediate_function_metadata(
                        arena,
                        default_value,
                        functions,
                    )?;
                }
            }
            Ok(())
        }
        Pattern::Rest(pattern) => {
            collect_owned_pattern_immediate_function_metadata(arena, pattern, functions)
        }
        Pattern::Binding(_) => Ok(()),
    }
}

fn collect_function_captures(
    arena: &ParserArena,
    metadata: AstRef<FunctionMetadata>,
    memo: &mut HashMap<u32, Vec<ParserIdentifier>>,
) -> Result<Vec<ParserIdentifier>, BytecompilerEmissionError> {
    let metadata_index = metadata.raw_index();
    if let Some(captures) = memo.get(&metadata_index) {
        return Ok(captures.clone());
    }
    let metadata = arena.function_metadata(metadata).ok_or(
        BytecompilerEmissionError::UnsupportedStatement("function declaration is missing metadata"),
    )?;
    let scope = arena
        .scope_node(metadata.body)
        .ok_or(BytecompilerEmissionError::MissingRootScope)?;
    let mut locals = HashSet::new();
    for parameter in &metadata.parameters {
        collect_pattern_binding_names(arena, parameter.pattern, &mut locals)?;
    }
    if let Some(name) = metadata.name {
        locals.insert(name);
    }
    if let Some(arguments) = arena.identifiers().identifier_for_text("arguments") {
        locals.insert(arguments);
    }
    collect_scope_local_names(arena, scope, &mut locals)?;

    let mut references = Vec::new();
    for parameter in &metadata.parameters {
        if let Some(default_value) = parameter.default_value {
            collect_expression_referenced_names(arena, default_value, &mut references)?;
        }
    }
    collect_scope_referenced_names(arena, scope, &mut references)?;
    let mut nested_functions = Vec::new();
    collect_scope_immediate_function_metadata(arena, scope, &mut nested_functions)?;
    for nested in nested_functions {
        references.extend(collect_function_captures(arena, nested, memo)?);
    }
    let mut seen = HashSet::new();
    let captures = references
        .into_iter()
        .filter(|name| !locals.contains(name))
        .filter(|name| seen.insert(*name))
        .collect::<Vec<_>>();
    memo.insert(metadata_index, captures.clone());
    Ok(captures)
}

fn collect_scope_local_names(
    arena: &ParserArena,
    scope: &crate::syntax::ScopeNode,
    locals: &mut HashSet<ParserIdentifier>,
) -> Result<(), BytecompilerEmissionError> {
    for statement in &scope.statements {
        collect_statement_local_names(arena, *statement, locals)?;
    }
    Ok(())
}

fn collect_statement_local_names(
    arena: &ParserArena,
    statement: AstRef<Stmt>,
    locals: &mut HashSet<ParserIdentifier>,
) -> Result<(), BytecompilerEmissionError> {
    let statement = arena
        .statement(statement)
        .ok_or(BytecompilerEmissionError::MissingStatement)?;
    match statement {
        Stmt::Declaration(declaration) => {
            collect_declaration_local_names(arena, declaration, locals)
        }
        Stmt::Block(block) => {
            let scope = arena
                .scope_node(block.scope)
                .ok_or(BytecompilerEmissionError::MissingRootScope)?;
            collect_scope_local_names(arena, scope, locals)
        }
        Stmt::FunctionDeclaration(declaration) => {
            locals.insert(declaration.name);
            Ok(())
        }
        Stmt::If(statement) => {
            collect_statement_local_names(arena, statement.consequent, locals)?;
            if let Some(alternate) = statement.alternate {
                collect_statement_local_names(arena, alternate, locals)?;
            }
            Ok(())
        }
        Stmt::While(statement) => collect_statement_local_names(arena, statement.body, locals),
        Stmt::For(statement) => {
            if let Some(ForInit::Declaration(declaration)) = &statement.init {
                collect_declaration_local_names(arena, declaration, locals)?;
            }
            collect_statement_local_names(arena, statement.body, locals)
        }
        Stmt::ForOf(statement) => {
            if let ForOfBinding::Declaration { name, .. } = statement.binding {
                locals.insert(name);
            }
            collect_statement_local_names(arena, statement.body, locals)
        }
        Stmt::Try(statement) => {
            collect_statement_local_names(arena, statement.body, locals)?;
            if let Some(catch) = statement.catch {
                if let Some(binding) = catch.binding {
                    locals.insert(binding);
                }
                collect_statement_local_names(arena, catch.body, locals)?;
            }
            if let Some(finally) = statement.finally {
                collect_statement_local_names(arena, finally, locals)?;
            }
            Ok(())
        }
        Stmt::Empty(_) | Stmt::Expression(_) | Stmt::Control(_) | Stmt::Module(_) => Ok(()),
    }
}

fn collect_declaration_local_names(
    arena: &ParserArena,
    declaration: &DeclarationStmt,
    locals: &mut HashSet<ParserIdentifier>,
) -> Result<(), BytecompilerEmissionError> {
    for binding in &declaration.bindings {
        collect_pattern_binding_names(arena, *binding, locals)?;
    }
    Ok(())
}

fn collect_pattern_binding_names(
    arena: &ParserArena,
    pattern: AstRef<Pattern>,
    locals: &mut HashSet<ParserIdentifier>,
) -> Result<(), BytecompilerEmissionError> {
    let pattern = arena
        .pattern(pattern)
        .ok_or(BytecompilerEmissionError::MissingPattern)?;
    collect_owned_pattern_binding_names(pattern, locals);
    Ok(())
}

fn collect_owned_pattern_binding_names(pattern: &Pattern, locals: &mut HashSet<ParserIdentifier>) {
    match pattern {
        Pattern::Binding(name) => {
            locals.insert(*name);
        }
        Pattern::Array(elements) => {
            for element in elements {
                collect_owned_pattern_binding_names(&element.pattern, locals);
            }
        }
        Pattern::Object(properties) => {
            for property in properties {
                collect_owned_pattern_binding_names(&property.pattern, locals);
            }
        }
        Pattern::Rest(pattern) => collect_owned_pattern_binding_names(pattern, locals),
        Pattern::AssignmentTarget(_) => {}
    }
}

fn collect_scope_referenced_names(
    arena: &ParserArena,
    scope: &crate::syntax::ScopeNode,
    references: &mut Vec<ParserIdentifier>,
) -> Result<(), BytecompilerEmissionError> {
    for statement in &scope.statements {
        collect_statement_referenced_names(arena, *statement, references)?;
    }
    Ok(())
}

fn collect_statement_referenced_names(
    arena: &ParserArena,
    statement: AstRef<Stmt>,
    references: &mut Vec<ParserIdentifier>,
) -> Result<(), BytecompilerEmissionError> {
    let statement = arena
        .statement(statement)
        .ok_or(BytecompilerEmissionError::MissingStatement)?;
    match statement {
        Stmt::Empty(_) => Ok(()),
        Stmt::Expression(expression) => {
            collect_expression_referenced_names(arena, *expression, references)
        }
        Stmt::Block(block) => {
            let scope = arena
                .scope_node(block.scope)
                .ok_or(BytecompilerEmissionError::MissingRootScope)?;
            collect_scope_referenced_names(arena, scope, references)
        }
        Stmt::Declaration(declaration) => {
            for initializer in declaration.initializers.iter().flatten() {
                collect_expression_referenced_names(arena, *initializer, references)?;
            }
            Ok(())
        }
        Stmt::FunctionDeclaration(_) => Ok(()),
        Stmt::If(statement) => {
            collect_expression_referenced_names(arena, statement.condition, references)?;
            collect_statement_referenced_names(arena, statement.consequent, references)?;
            if let Some(alternate) = statement.alternate {
                collect_statement_referenced_names(arena, alternate, references)?;
            }
            Ok(())
        }
        Stmt::While(statement) => {
            collect_expression_referenced_names(arena, statement.condition, references)?;
            collect_statement_referenced_names(arena, statement.body, references)
        }
        Stmt::For(statement) => {
            if let Some(init) = &statement.init {
                match init {
                    ForInit::Declaration(declaration) => {
                        for initializer in declaration.initializers.iter().flatten() {
                            collect_expression_referenced_names(arena, *initializer, references)?;
                        }
                    }
                    ForInit::Expression(expression) => {
                        collect_expression_referenced_names(arena, *expression, references)?;
                    }
                }
            }
            if let Some(condition) = statement.condition {
                collect_expression_referenced_names(arena, condition, references)?;
            }
            if let Some(update) = statement.update {
                collect_expression_referenced_names(arena, update, references)?;
            }
            collect_statement_referenced_names(arena, statement.body, references)
        }
        Stmt::ForOf(statement) => {
            if let ForOfBinding::Assignment(name) = statement.binding {
                references.push(name);
            }
            collect_expression_referenced_names(arena, statement.iterable, references)?;
            collect_statement_referenced_names(arena, statement.body, references)
        }
        Stmt::Try(statement) => {
            collect_statement_referenced_names(arena, statement.body, references)?;
            if let Some(catch) = statement.catch {
                collect_statement_referenced_names(arena, catch.body, references)?;
            }
            if let Some(finally) = statement.finally {
                collect_statement_referenced_names(arena, finally, references)?;
            }
            Ok(())
        }
        Stmt::Control(control) => match control.kind {
            ControlKind::Return(Some(value)) => {
                collect_expression_referenced_names(arena, value, references)
            }
            ControlKind::Throw(value) => {
                collect_expression_referenced_names(arena, value, references)
            }
            ControlKind::Return(None) | ControlKind::Break(_) | ControlKind::Continue(_) => Ok(()),
        },
        Stmt::Module(_) => Ok(()),
    }
}

fn collect_expression_referenced_names(
    arena: &ParserArena,
    expression: AstRef<Expr>,
    references: &mut Vec<ParserIdentifier>,
) -> Result<(), BytecompilerEmissionError> {
    let expression = arena
        .expression(expression)
        .ok_or(BytecompilerEmissionError::MissingExpression)?;
    match expression {
        Expr::Name(name) if name.kind == NameKind::Resolve => {
            references.push(name.name);
            Ok(())
        }
        Expr::Literal(_) | Expr::Name(_) | Expr::Function(_) | Expr::ImportMeta(_) => Ok(()),
        Expr::Unary(unary) => {
            collect_expression_referenced_names(arena, unary.argument, references)
        }
        Expr::Binary(binary) => {
            collect_expression_referenced_names(arena, binary.left, references)?;
            collect_expression_referenced_names(arena, binary.right, references)
        }
        Expr::Assignment(assignment) => {
            collect_pattern_referenced_names(arena, assignment.target, references)?;
            collect_expression_referenced_names(arena, assignment.value, references)
        }
        Expr::Conditional(conditional) => {
            collect_expression_referenced_names(arena, conditional.test, references)?;
            collect_expression_referenced_names(arena, conditional.consequent, references)?;
            collect_expression_referenced_names(arena, conditional.alternate, references)
        }
        Expr::Call(call) => {
            collect_expression_referenced_names(arena, call.callee, references)?;
            for argument in &call.arguments {
                collect_expression_referenced_names(arena, *argument, references)?;
            }
            Ok(())
        }
        Expr::New(new) => {
            collect_expression_referenced_names(arena, new.callee, references)?;
            for argument in &new.arguments {
                collect_expression_referenced_names(arena, *argument, references)?;
            }
            Ok(())
        }
        Expr::Member(member) => {
            collect_expression_referenced_names(arena, member.base, references)?;
            if let MemberKind::Bracket(expression) = member.member {
                collect_expression_referenced_names(arena, expression, references)?;
            }
            Ok(())
        }
        Expr::Object(object) => {
            for property in &object.properties {
                if let AstPropertyKey::Computed(expression) = property.key {
                    collect_expression_referenced_names(arena, expression, references)?;
                }
                collect_expression_referenced_names(arena, property.value, references)?;
            }
            Ok(())
        }
        Expr::Array(array) => {
            for element in &array.elements {
                match element {
                    ArrayLiteralElement::Expression(expression) => {
                        collect_expression_referenced_names(arena, *expression, references)?;
                    }
                    ArrayLiteralElement::Spread { value, .. } => {
                        collect_expression_referenced_names(arena, *value, references)?;
                    }
                    ArrayLiteralElement::Elision(_) => {}
                }
            }
            Ok(())
        }
        Expr::Class(class) => {
            if let Some(heritage) = class.heritage {
                collect_expression_referenced_names(arena, heritage, references)?;
            }
            for element in &class.elements {
                if let crate::syntax::ast::ClassElementName::Computed(expression) = element.name {
                    collect_expression_referenced_names(arena, expression, references)?;
                }
                if element.is_static || element.metadata.is_none() {
                    if let Some(initializer) = element.initializer {
                        collect_expression_referenced_names(arena, initializer, references)?;
                    }
                }
            }
            Ok(())
        }
        Expr::Template(template) => {
            if let Some(tag) = template.tag {
                collect_expression_referenced_names(arena, tag, references)?;
            }
            for expression in &template.expressions {
                collect_expression_referenced_names(arena, *expression, references)?;
            }
            Ok(())
        }
    }
}

fn collect_pattern_referenced_names(
    arena: &ParserArena,
    pattern: AstRef<Pattern>,
    references: &mut Vec<ParserIdentifier>,
) -> Result<(), BytecompilerEmissionError> {
    let pattern = arena
        .pattern(pattern)
        .ok_or(BytecompilerEmissionError::MissingPattern)?;
    collect_owned_pattern_referenced_names(arena, pattern, references)
}

fn collect_owned_pattern_referenced_names(
    arena: &ParserArena,
    pattern: &Pattern,
    references: &mut Vec<ParserIdentifier>,
) -> Result<(), BytecompilerEmissionError> {
    match pattern {
        Pattern::AssignmentTarget(expression) => {
            collect_expression_referenced_names(arena, *expression, references)
        }
        Pattern::Array(elements) => {
            for element in elements {
                collect_owned_pattern_referenced_names(arena, &element.pattern, references)?;
                if let Some(default_value) = element.default_value {
                    collect_expression_referenced_names(arena, default_value, references)?;
                }
            }
            Ok(())
        }
        Pattern::Object(properties) => {
            for property in properties {
                if let crate::syntax::ast::AstPropertyKey::Computed(expression) = property.key {
                    collect_expression_referenced_names(arena, expression, references)?;
                }
                collect_owned_pattern_referenced_names(arena, &property.pattern, references)?;
                if let Some(default_value) = property.default_value {
                    collect_expression_referenced_names(arena, default_value, references)?;
                }
            }
            Ok(())
        }
        Pattern::Rest(pattern) => {
            collect_owned_pattern_referenced_names(arena, pattern, references)
        }
        Pattern::Binding(_) => Ok(()),
    }
}

fn collect_scope_string_literals(
    arena: &ParserArena,
    scope: &crate::syntax::ScopeNode,
    table: &mut HashMap<u32, String>,
) -> Result<(), BytecompilerEmissionError> {
    for statement in &scope.statements {
        collect_statement_string_literals(arena, *statement, table)?;
    }
    Ok(())
}

fn collect_statement_string_literals(
    arena: &ParserArena,
    statement: AstRef<Stmt>,
    table: &mut HashMap<u32, String>,
) -> Result<(), BytecompilerEmissionError> {
    let statement = arena
        .statement(statement)
        .ok_or(BytecompilerEmissionError::MissingStatement)?;
    match statement {
        Stmt::Empty(_) => Ok(()),
        Stmt::Expression(expression) => {
            collect_expression_string_literals(arena, *expression, table)
        }
        Stmt::Block(block) => {
            let scope = arena
                .scope_node(block.scope)
                .ok_or(BytecompilerEmissionError::MissingRootScope)?;
            collect_scope_string_literals(arena, scope, table)
        }
        Stmt::Declaration(declaration) => {
            for binding in &declaration.bindings {
                collect_pattern_string_literals(arena, *binding, table)?;
            }
            for initializer in declaration.initializers.iter().flatten() {
                collect_expression_string_literals(arena, *initializer, table)?;
            }
            Ok(())
        }
        Stmt::FunctionDeclaration(declaration) => {
            collect_function_string_literals(arena, declaration.metadata, table)
        }
        Stmt::If(statement) => {
            collect_expression_string_literals(arena, statement.condition, table)?;
            collect_statement_string_literals(arena, statement.consequent, table)?;
            if let Some(alternate) = statement.alternate {
                collect_statement_string_literals(arena, alternate, table)?;
            }
            Ok(())
        }
        Stmt::While(statement) => {
            collect_expression_string_literals(arena, statement.condition, table)?;
            collect_statement_string_literals(arena, statement.body, table)
        }
        Stmt::For(statement) => {
            if let Some(init) = &statement.init {
                match init {
                    ForInit::Declaration(declaration) => {
                        for initializer in declaration.initializers.iter().flatten() {
                            collect_expression_string_literals(arena, *initializer, table)?;
                        }
                    }
                    ForInit::Expression(expression) => {
                        collect_expression_string_literals(arena, *expression, table)?;
                    }
                }
            }
            if let Some(condition) = statement.condition {
                collect_expression_string_literals(arena, condition, table)?;
            }
            if let Some(update) = statement.update {
                collect_expression_string_literals(arena, update, table)?;
            }
            collect_statement_string_literals(arena, statement.body, table)
        }
        Stmt::ForOf(statement) => {
            collect_expression_string_literals(arena, statement.iterable, table)?;
            collect_statement_string_literals(arena, statement.body, table)
        }
        Stmt::Try(statement) => {
            collect_statement_string_literals(arena, statement.body, table)?;
            if let Some(catch) = statement.catch {
                collect_statement_string_literals(arena, catch.body, table)?;
            }
            if let Some(finally) = statement.finally {
                collect_statement_string_literals(arena, finally, table)?;
            }
            Ok(())
        }
        Stmt::Control(control) => match control.kind {
            ControlKind::Return(Some(value)) => {
                collect_expression_string_literals(arena, value, table)
            }
            ControlKind::Throw(value) => collect_expression_string_literals(arena, value, table),
            ControlKind::Return(None) | ControlKind::Break(_) | ControlKind::Continue(_) => Ok(()),
        },
        Stmt::Module(_) => Ok(()),
    }
}

fn collect_function_string_literals(
    arena: &ParserArena,
    metadata: AstRef<FunctionMetadata>,
    table: &mut HashMap<u32, String>,
) -> Result<(), BytecompilerEmissionError> {
    let metadata = arena.function_metadata(metadata).ok_or(
        BytecompilerEmissionError::UnsupportedStatement("function declaration is missing metadata"),
    )?;
    let scope = arena
        .scope_node(metadata.body)
        .ok_or(BytecompilerEmissionError::MissingRootScope)?;
    for parameter in &metadata.parameters {
        collect_pattern_string_literals(arena, parameter.pattern, table)?;
        if let Some(default_value) = parameter.default_value {
            collect_expression_string_literals(arena, default_value, table)?;
        }
    }
    collect_scope_string_literals(arena, scope, table)
}

fn collect_expression_string_literals(
    arena: &ParserArena,
    expression: AstRef<Expr>,
    table: &mut HashMap<u32, String>,
) -> Result<(), BytecompilerEmissionError> {
    let expression = arena
        .expression(expression)
        .ok_or(BytecompilerEmissionError::MissingExpression)?;
    match expression {
        Expr::Literal(literal) => {
            match literal.kind {
                LiteralKind::String { text } => record_string_literal(arena, text, table)?,
                LiteralKind::RegExp { pattern, flags } => {
                    record_string_literal(arena, pattern, table)?;
                    record_string_literal(arena, flags, table)?;
                }
                LiteralKind::BigInt { text } => record_string_literal(arena, text, table)?,
                LiteralKind::Null | LiteralKind::Boolean(_) | LiteralKind::Number { .. } => {}
            }
            Ok(())
        }
        Expr::Name(_) | Expr::ImportMeta(_) => Ok(()),
        Expr::Unary(unary) => collect_expression_string_literals(arena, unary.argument, table),
        Expr::Binary(binary) => {
            collect_expression_string_literals(arena, binary.left, table)?;
            collect_expression_string_literals(arena, binary.right, table)
        }
        Expr::Assignment(assignment) => {
            collect_pattern_string_literals(arena, assignment.target, table)?;
            collect_expression_string_literals(arena, assignment.value, table)
        }
        Expr::Conditional(conditional) => {
            collect_expression_string_literals(arena, conditional.test, table)?;
            collect_expression_string_literals(arena, conditional.consequent, table)?;
            collect_expression_string_literals(arena, conditional.alternate, table)
        }
        Expr::Call(call) => {
            collect_expression_string_literals(arena, call.callee, table)?;
            for argument in &call.arguments {
                collect_expression_string_literals(arena, *argument, table)?;
            }
            Ok(())
        }
        Expr::New(new) => {
            collect_expression_string_literals(arena, new.callee, table)?;
            for argument in &new.arguments {
                collect_expression_string_literals(arena, *argument, table)?;
            }
            Ok(())
        }
        Expr::Member(member) => {
            collect_expression_string_literals(arena, member.base, table)?;
            if let MemberKind::Bracket(expression) = member.member {
                collect_expression_string_literals(arena, expression, table)?;
            }
            Ok(())
        }
        Expr::Object(object) => {
            for property in &object.properties {
                if property.kind == ObjectLiteralPropertyKind::Spread {
                    collect_expression_string_literals(arena, property.value, table)?;
                    continue;
                }
                match property.key {
                    AstPropertyKey::Computed(expression) => {
                        collect_expression_string_literals(arena, expression, table)?;
                    }
                    AstPropertyKey::Identifier(name)
                    | AstPropertyKey::String(name)
                    | AstPropertyKey::Number(name) => {
                        record_string_literal(arena, name, table)?;
                    }
                    AstPropertyKey::Private(_) => {}
                }
                collect_expression_string_literals(arena, property.value, table)?;
            }
            Ok(())
        }
        Expr::Array(array) => {
            for element in &array.elements {
                match element {
                    ArrayLiteralElement::Expression(expression) => {
                        collect_expression_string_literals(arena, *expression, table)?;
                    }
                    ArrayLiteralElement::Spread { value, .. } => {
                        collect_expression_string_literals(arena, *value, table)?;
                    }
                    ArrayLiteralElement::Elision(_) => {}
                }
            }
            Ok(())
        }
        Expr::Function(metadata) => collect_function_string_literals(arena, *metadata, table),
        Expr::Class(class) => {
            if let Some(heritage) = class.heritage {
                collect_expression_string_literals(arena, heritage, table)?;
            }
            for element in &class.elements {
                if let crate::syntax::ast::ClassElementName::Computed(expression) = element.name {
                    collect_expression_string_literals(arena, expression, table)?;
                }
                if element.is_static || element.metadata.is_none() {
                    if let Some(initializer) = element.initializer {
                        collect_expression_string_literals(arena, initializer, table)?;
                    }
                }
                if let Some(metadata) = element.metadata {
                    collect_function_string_literals(arena, metadata, table)?;
                }
            }
            Ok(())
        }
        Expr::Template(template) => {
            if let Some(tag) = template.tag {
                collect_expression_string_literals(arena, tag, table)?;
            }
            for expression in &template.expressions {
                collect_expression_string_literals(arena, *expression, table)?;
            }
            Ok(())
        }
    }
}

fn collect_pattern_string_literals(
    arena: &ParserArena,
    pattern: AstRef<Pattern>,
    table: &mut HashMap<u32, String>,
) -> Result<(), BytecompilerEmissionError> {
    let pattern = arena
        .pattern(pattern)
        .ok_or(BytecompilerEmissionError::MissingPattern)?;
    collect_owned_pattern_string_literals(arena, pattern, table)
}

fn collect_owned_pattern_string_literals(
    arena: &ParserArena,
    pattern: &Pattern,
    table: &mut HashMap<u32, String>,
) -> Result<(), BytecompilerEmissionError> {
    match pattern {
        Pattern::AssignmentTarget(expression) => {
            collect_expression_string_literals(arena, *expression, table)
        }
        Pattern::Array(elements) => {
            for element in elements {
                collect_owned_pattern_string_literals(arena, &element.pattern, table)?;
                if let Some(default_value) = element.default_value {
                    collect_expression_string_literals(arena, default_value, table)?;
                }
            }
            Ok(())
        }
        Pattern::Object(properties) => {
            for property in properties {
                match property.key {
                    crate::syntax::ast::AstPropertyKey::Computed(expression) => {
                        collect_expression_string_literals(arena, expression, table)?;
                    }
                    crate::syntax::ast::AstPropertyKey::Identifier(name)
                    | crate::syntax::ast::AstPropertyKey::String(name)
                    | crate::syntax::ast::AstPropertyKey::Number(name) => {
                        record_string_literal(arena, name, table)?;
                    }
                    crate::syntax::ast::AstPropertyKey::Private(_) => {}
                }
                collect_owned_pattern_string_literals(arena, &property.pattern, table)?;
                if let Some(default_value) = property.default_value {
                    collect_expression_string_literals(arena, default_value, table)?;
                }
            }
            Ok(())
        }
        Pattern::Rest(pattern) => collect_owned_pattern_string_literals(arena, pattern, table),
        Pattern::Binding(_) => Ok(()),
    }
}

fn record_string_literal(
    arena: &ParserArena,
    text: ParserIdentifier,
    table: &mut HashMap<u32, String>,
) -> Result<(), BytecompilerEmissionError> {
    record_string_literal_by_key(arena, text.0, table)
}

fn record_string_literal_by_key(
    arena: &ParserArena,
    identifier_index: u32,
    table: &mut HashMap<u32, String>,
) -> Result<(), BytecompilerEmissionError> {
    let Some(value) = arena
        .identifiers()
        .identifier_text(ParserIdentifier(identifier_index))
    else {
        return Err(BytecompilerEmissionError::UnsupportedLiteral(
            "string literal is missing cooked text",
        ));
    };
    table
        .entry(identifier_index)
        .or_insert_with(|| value.to_owned());
    Ok(())
}

fn install_code_block_literal_text_table(
    arena: &ParserArena,
    code_block: UnlinkedCodeBlock,
) -> Result<UnlinkedCodeBlock, BytecompilerEmissionError> {
    let entries = collect_code_block_literal_text_entries(arena, &code_block)?;
    Ok(code_block.with_string_literals(entries))
}

fn collect_code_block_literal_text_entries(
    arena: &ParserArena,
    code_block: &UnlinkedCodeBlock,
) -> Result<Vec<(u32, String)>, BytecompilerEmissionError> {
    let mut table = HashMap::new();
    for declaration in code_block.instructions().declarations() {
        collect_code_block_literal_text_operand(
            arena,
            declaration.opcode,
            &declaration.operands,
            &mut table,
        )?;
    }
    for instruction in code_block.instructions().typed_placeholder() {
        collect_code_block_literal_text_operand(
            arena,
            instruction.opcode,
            &instruction.operands,
            &mut table,
        )?;
    }

    let mut entries = table.into_iter().collect::<Vec<_>>();
    entries.sort_by_key(|(identifier_index, _)| *identifier_index);
    Ok(entries)
}

fn collect_code_block_literal_text_operand(
    arena: &ParserArena,
    opcode: crate::bytecode::Opcode,
    operands: &[Operand],
    table: &mut HashMap<u32, String>,
) -> Result<(), BytecompilerEmissionError> {
    if !matches!(
        CoreOpcode::from_opcode(opcode),
        Some(CoreOpcode::LoadString | CoreOpcode::LoadBigInt)
    ) {
        return Ok(());
    }
    let Some(Operand::IdentifierIndex(identifier_index)) = operands.get(1).copied() else {
        return Err(BytecompilerEmissionError::UnsupportedLiteral(
            "literal text load is missing identifier operand",
        ));
    };
    record_string_literal_by_key(arena, identifier_index, table)
}

fn function_uses_arguments_identifier(
    arena: &ParserArena,
    metadata: &FunctionMetadata,
    scope: &crate::syntax::ScopeNode,
    arguments: ParserIdentifier,
) -> Result<bool, BytecompilerEmissionError> {
    let mut references = Vec::new();
    for parameter in &metadata.parameters {
        if let Some(default_value) = parameter.default_value {
            collect_expression_referenced_names(arena, default_value, &mut references)?;
        }
    }
    collect_scope_referenced_names(arena, scope, &mut references)?;
    Ok(references.into_iter().any(|name| name == arguments))
}

fn compound_assignment_opcode(op: AssignmentOperator) -> Option<CoreOpcode> {
    match op {
        AssignmentOperator::Assign => None,
        AssignmentOperator::Add => Some(CoreOpcode::AddInt32),
        AssignmentOperator::Subtract => Some(CoreOpcode::SubInt32),
        AssignmentOperator::Multiply => Some(CoreOpcode::MulInt32),
        AssignmentOperator::Divide => Some(CoreOpcode::DivNumber),
        AssignmentOperator::Modulo => Some(CoreOpcode::ModNumber),
        AssignmentOperator::BitOr => Some(CoreOpcode::BitOrInt32),
        AssignmentOperator::BitAnd => Some(CoreOpcode::BitAndInt32),
        AssignmentOperator::BitXor => Some(CoreOpcode::BitXorInt32),
        AssignmentOperator::LeftShift => Some(CoreOpcode::LeftShiftInt32),
        AssignmentOperator::RightShift => Some(CoreOpcode::RightShiftInt32),
        AssignmentOperator::UnsignedRightShift => Some(CoreOpcode::UnsignedRightShiftInt32),
        AssignmentOperator::Pow => Some(CoreOpcode::PowNumber),
        AssignmentOperator::Coalesce
        | AssignmentOperator::LogicalOr
        | AssignmentOperator::LogicalAnd => None,
    }
}

struct AstBytecodeEmitter<'a, 'g> {
    arena: &'a ParserArena,
    generator: &'g mut BytecodeGenerator,
    locals: HashMap<ParserIdentifier, LocalBinding>,
    captured_bindings: HashSet<ParserIdentifier>,
    initialized_cells: HashSet<ParserIdentifier>,
    function_metadata_indices: HashMap<u32, u32>,
    function_captures: HashMap<u32, Vec<ParserIdentifier>>,
    loop_stack: Vec<LoopControlLabels>,
    finally_stack: Vec<ActiveFinallyContext>,
}

#[derive(Clone, Copy, Debug, Eq, PartialEq)]
struct LocalBinding {
    register: VirtualRegister,
    kind: LocalBindingKind,
}

#[derive(Clone, Copy, Debug, Eq, PartialEq)]
enum LocalBindingKind {
    Value,
    ClosureCell,
}

#[derive(Clone, Copy, Debug, Default, Eq, PartialEq)]
struct StatementEmission {
    value: Option<VirtualRegister>,
    terminated: bool,
}

impl StatementEmission {
    const fn terminated(value: Option<VirtualRegister>) -> Self {
        Self {
            value,
            terminated: true,
        }
    }
}

#[derive(Clone, Copy, Debug, Eq, PartialEq)]
struct LoopControlLabels {
    break_target: LabelRef,
    continue_target: LabelRef,
    finally_stack_depth: usize,
}

#[derive(Clone, Debug, Eq, PartialEq)]
struct ActiveFinallyContext {
    body: AstRef<Stmt>,
    applies_to_throw: bool,
    exits: Vec<FinallyExit>,
}

#[derive(Clone, Copy, Debug, Eq, PartialEq)]
struct FinallyExit {
    label: LabelRef,
    kind: FinallyExitKind,
}

#[derive(Clone, Copy, Debug, Eq, PartialEq)]
enum FinallyExitKind {
    Return(VirtualRegister),
    Throw(VirtualRegister),
    Break {
        target: LabelRef,
        target_depth: usize,
    },
    Continue {
        target: LabelRef,
        target_depth: usize,
    },
}

#[derive(Clone, Copy, Debug, Eq, PartialEq)]
enum EmittedPropertyKey {
    Name(ParserIdentifier),
    Value(VirtualRegister),
}

#[derive(Clone, Copy, Debug, Eq, PartialEq)]
struct EmittedAssignmentReference {
    target: EmittedAssignmentTarget,
    value: VirtualRegister,
}

#[derive(Clone, Copy, Debug, Eq, PartialEq)]
enum EmittedAssignmentTarget {
    Local(LocalBinding),
    Dot {
        object: VirtualRegister,
        name: ParserIdentifier,
    },
    Bracket {
        object: VirtualRegister,
        key: VirtualRegister,
    },
}

impl AstBytecodeEmitter<'_, '_> {
    fn is_captured_binding(&self, name: ParserIdentifier) -> bool {
        self.captured_bindings.contains(&name)
    }

    fn local_binding_kind(&self, name: ParserIdentifier) -> LocalBindingKind {
        if self.is_captured_binding(name) {
            LocalBindingKind::ClosureCell
        } else {
            LocalBindingKind::Value
        }
    }

    fn initialize_captured_local_cells(&mut self) -> Result<(), BytecompilerEmissionError> {
        let cells: Vec<_> = self
            .locals
            .iter()
            .filter_map(|(name, binding)| {
                if binding.kind == LocalBindingKind::ClosureCell
                    && binding.register.is_local()
                    && !self.initialized_cells.contains(name)
                {
                    Some((*name, binding.register))
                } else {
                    None
                }
            })
            .collect();
        for (name, register) in cells {
            let value = self.emit_load_undefined()?;
            let cell = self.emit_new_closure_cell(value)?;
            self.emit_move(register, cell);
            self.initialized_cells.insert(name);
        }
        Ok(())
    }

    fn emit_function_body(
        &self,
        metadata: AstRef<FunctionMetadata>,
    ) -> Result<UnlinkedCodeBlock, BytecompilerEmissionError> {
        let metadata_ref = metadata;
        let metadata = self.arena.function_metadata(metadata_ref).cloned().ok_or(
            BytecompilerEmissionError::UnsupportedStatement(
                "function declaration is missing metadata",
            ),
        )?;
        let scope = self
            .arena
            .scope_node(metadata.body)
            .cloned()
            .ok_or(BytecompilerEmissionError::MissingRootScope)?;
        let mut generation =
            GenerationPlan::new(CodeKind::Function, BytecodeParseMode::NormalFunction);
        generation.registers = function_execution_frame_shape(metadata.parameter_count);
        generation.source = self.generator.plan().source.clone();
        generation.root = GenerationRoot::FunctionNode;
        let mut generator = BytecodeGenerator::new(generation);
        let mut child = AstBytecodeEmitter {
            arena: self.arena,
            generator: &mut generator,
            locals: HashMap::new(),
            captured_bindings: self.captured_bindings.clone(),
            initialized_cells: HashSet::new(),
            function_metadata_indices: self.function_metadata_indices.clone(),
            function_captures: self.function_captures.clone(),
            loop_stack: Vec::new(),
            finally_stack: Vec::new(),
        };
        for parameter in &metadata.parameters {
            child.predeclare_pattern_binding(parameter.pattern)?;
        }
        let captures = self
            .function_captures
            .get(&metadata_ref.raw_index())
            .cloned()
            .unwrap_or_default();
        for (index, capture) in captures.iter().copied().enumerate() {
            let register = child.generator.registers_mut().reserve_local();
            child.locals.insert(
                capture,
                LocalBinding {
                    register,
                    kind: LocalBindingKind::ClosureCell,
                },
            );
            let value = child.emit_load_capture(index.try_into().unwrap_or(u32::MAX))?;
            child.emit_move(register, value);
            child.initialized_cells.insert(capture);
        }
        child.predeclare_scope_locals(&scope)?;
        if let Some(name) = metadata.name {
            child.predeclare_function_binding(name)?;
        }
        child.emit_intrinsic_bindings()?;
        child.emit_arguments_object_binding(&metadata, &scope)?;
        child.initialize_captured_local_cells()?;
        for (index, parameter) in metadata.parameters.iter().enumerate() {
            if let Some(rest_pattern) = child.parameter_rest_pattern(parameter.pattern)? {
                let rest = child.emit_create_rest_parameter(index)?;
                child.emit_owned_pattern_binding(&rest_pattern, rest)?;
            } else {
                let raw = 6_u32.saturating_add(index.try_into().unwrap_or(u32::MAX));
                let argument = VirtualRegister::argument_or_header(raw);
                let value = child.emit_default_if_undefined(argument, parameter.default_value)?;
                child.emit_pattern_binding(parameter.pattern, value)?;
            }
        }
        if let Some(name) = metadata.name {
            child.emit_function_binding(name, metadata_ref)?;
        }
        child.emit_scope_function_bindings(&scope)?;
        let mut last_value = None;
        let mut terminated = false;
        for statement in &scope.statements {
            if terminated {
                break;
            }
            let emission = child.emit_statement(*statement)?;
            last_value = emission.value;
            terminated = emission.terminated;
        }
        if !terminated {
            let value = match last_value {
                Some(value) => value,
                None => child.emit_load_undefined()?,
            };
            child.emit_return(value);
        }
        install_code_block_literal_text_table(self.arena, generator.finish().code_block)
    }

    fn predeclare_scope_locals(
        &mut self,
        scope: &crate::syntax::ScopeNode,
    ) -> Result<(), BytecompilerEmissionError> {
        for statement in &scope.statements {
            let Some(statement) = self.arena.statement(*statement).cloned() else {
                return Err(BytecompilerEmissionError::MissingStatement);
            };
            match statement {
                Stmt::Declaration(declaration) => self.predeclare_declaration(&declaration)?,
                Stmt::Block(block) => {
                    let Some(scope) = self.arena.scope_node(block.scope).cloned() else {
                        return Err(BytecompilerEmissionError::MissingRootScope);
                    };
                    self.predeclare_scope_locals(&scope)?;
                }
                Stmt::If(statement) => {
                    self.predeclare_statement_locals(statement.consequent)?;
                    if let Some(alternate) = statement.alternate {
                        self.predeclare_statement_locals(alternate)?;
                    }
                }
                Stmt::While(statement) => self.predeclare_statement_locals(statement.body)?,
                Stmt::For(statement) => self.predeclare_for_locals(&statement)?,
                Stmt::ForOf(statement) => self.predeclare_for_of_locals(&statement)?,
                Stmt::Try(statement) => self.predeclare_try_locals(&statement)?,
                Stmt::FunctionDeclaration(declaration) => {
                    self.predeclare_function_binding(declaration.name)?
                }
                _ => {}
            }
        }
        Ok(())
    }

    fn predeclare_statement_locals(
        &mut self,
        statement: AstRef<Stmt>,
    ) -> Result<(), BytecompilerEmissionError> {
        let Some(statement) = self.arena.statement(statement).cloned() else {
            return Err(BytecompilerEmissionError::MissingStatement);
        };
        match statement {
            Stmt::Declaration(declaration) => self.predeclare_declaration(&declaration),
            Stmt::Block(block) => {
                let Some(scope) = self.arena.scope_node(block.scope).cloned() else {
                    return Err(BytecompilerEmissionError::MissingRootScope);
                };
                self.predeclare_scope_locals(&scope)
            }
            Stmt::If(statement) => {
                self.predeclare_statement_locals(statement.consequent)?;
                if let Some(alternate) = statement.alternate {
                    self.predeclare_statement_locals(alternate)?;
                }
                Ok(())
            }
            Stmt::While(statement) => self.predeclare_statement_locals(statement.body),
            Stmt::For(statement) => self.predeclare_for_locals(&statement),
            Stmt::ForOf(statement) => self.predeclare_for_of_locals(&statement),
            Stmt::Try(statement) => self.predeclare_try_locals(&statement),
            Stmt::FunctionDeclaration(declaration) => {
                self.predeclare_function_binding(declaration.name)
            }
            _ => Ok(()),
        }
    }

    fn predeclare_for_locals(
        &mut self,
        statement: &crate::syntax::ast::ForStmt,
    ) -> Result<(), BytecompilerEmissionError> {
        if let Some(ForInit::Declaration(declaration)) = &statement.init {
            self.predeclare_declaration(declaration)?;
        }
        self.predeclare_statement_locals(statement.body)
    }

    fn predeclare_for_of_locals(
        &mut self,
        statement: &crate::syntax::ast::ForOfStmt,
    ) -> Result<(), BytecompilerEmissionError> {
        if let ForOfBinding::Declaration { name, .. } = statement.binding {
            self.predeclare_function_binding(name)?;
        }
        self.predeclare_statement_locals(statement.body)
    }

    fn predeclare_try_locals(
        &mut self,
        statement: &crate::syntax::ast::TryStmt,
    ) -> Result<(), BytecompilerEmissionError> {
        self.predeclare_statement_locals(statement.body)?;
        if let Some(catch) = statement.catch {
            if let Some(binding) = catch.binding {
                self.predeclare_function_binding(binding)?;
            }
            self.predeclare_statement_locals(catch.body)?;
        }
        if let Some(finally) = statement.finally {
            self.predeclare_statement_locals(finally)?;
        }
        Ok(())
    }

    fn predeclare_declaration(
        &mut self,
        declaration: &DeclarationStmt,
    ) -> Result<(), BytecompilerEmissionError> {
        for binding in &declaration.bindings {
            self.predeclare_pattern_binding(*binding)?;
        }
        Ok(())
    }

    fn predeclare_pattern_binding(
        &mut self,
        pattern: AstRef<Pattern>,
    ) -> Result<(), BytecompilerEmissionError> {
        let pattern = self
            .arena
            .pattern(pattern)
            .cloned()
            .ok_or(BytecompilerEmissionError::MissingPattern)?;
        self.predeclare_owned_pattern_binding(&pattern)
    }

    fn predeclare_owned_pattern_binding(
        &mut self,
        pattern: &Pattern,
    ) -> Result<(), BytecompilerEmissionError> {
        match pattern {
            Pattern::Binding(name) => self.predeclare_function_binding(*name),
            Pattern::Array(elements) => {
                for element in elements {
                    self.predeclare_owned_pattern_binding(&element.pattern)?;
                }
                Ok(())
            }
            Pattern::Object(properties) => {
                for property in properties {
                    self.predeclare_owned_pattern_binding(&property.pattern)?;
                }
                Ok(())
            }
            Pattern::Rest(pattern) => self.predeclare_owned_pattern_binding(pattern),
            Pattern::AssignmentTarget(_) => {
                Err(BytecompilerEmissionError::UnsupportedAssignmentTarget)
            }
        }
    }

    fn predeclare_function_binding(
        &mut self,
        name: ParserIdentifier,
    ) -> Result<(), BytecompilerEmissionError> {
        if !self.locals.contains_key(&name) {
            let register = self.generator.registers_mut().reserve_local();
            self.locals.insert(
                name,
                LocalBinding {
                    register,
                    kind: self.local_binding_kind(name),
                },
            );
        }
        Ok(())
    }

    fn emit_scope_function_bindings(
        &mut self,
        scope: &crate::syntax::ScopeNode,
    ) -> Result<(), BytecompilerEmissionError> {
        for statement in &scope.statements {
            let Some(Stmt::FunctionDeclaration(declaration)) = self.arena.statement(*statement)
            else {
                continue;
            };
            self.function_metadata_indices
                .get(&declaration.metadata.raw_index())
                .ok_or(BytecompilerEmissionError::UnsupportedStatement(
                    "function declaration has no compiled body",
                ))?;
            self.emit_function_binding(declaration.name, declaration.metadata)?;
        }
        Ok(())
    }

    fn emit_intrinsic_bindings(&mut self) -> Result<(), BytecompilerEmissionError> {
        self.emit_intrinsic_binding("undefined", Self::emit_load_undefined)?;
        self.emit_intrinsic_binding("Object", Self::emit_load_object_constructor)?;
        self.emit_intrinsic_binding("Array", Self::emit_load_array_constructor)?;
        self.emit_intrinsic_binding("Math", Self::emit_load_math_object)?;
        self.emit_intrinsic_binding("JSON", Self::emit_load_json_object)?;
        self.emit_intrinsic_binding("Reflect", Self::emit_load_reflect_object)?;
        self.emit_intrinsic_binding("String", Self::emit_load_string_constructor)?;
        self.emit_intrinsic_binding("Number", Self::emit_load_number_constructor)?;
        self.emit_intrinsic_binding("Boolean", Self::emit_load_boolean_constructor)?;
        self.emit_intrinsic_binding("Error", Self::emit_load_error_constructor)?;
        self.emit_intrinsic_binding("TypeError", Self::emit_load_type_error_constructor)?;
        self.emit_intrinsic_binding("Map", Self::emit_load_map_constructor)?;
        self.emit_intrinsic_binding("Set", Self::emit_load_set_constructor)?;
        self.emit_intrinsic_binding("WeakMap", Self::emit_load_weak_map_constructor)?;
        self.emit_intrinsic_binding("WeakSet", Self::emit_load_weak_set_constructor)?;
        self.emit_intrinsic_binding("RegExp", Self::emit_load_regexp_constructor)?;
        self.emit_intrinsic_binding("Promise", Self::emit_load_promise_constructor)?;
        self.emit_intrinsic_binding("Date", Self::emit_load_date_constructor)?;
        self.emit_intrinsic_binding("BigInt", Self::emit_load_bigint_constructor)?;
        self.emit_intrinsic_binding("ArrayBuffer", Self::emit_load_array_buffer_constructor)?;
        self.emit_intrinsic_binding("Uint8Array", Self::emit_load_uint8_array_constructor)?;
        self.emit_intrinsic_binding("DataView", Self::emit_load_data_view_constructor)?;
        self.emit_intrinsic_binding("Proxy", Self::emit_load_proxy_constructor)?;
        self.emit_intrinsic_binding("Symbol", Self::emit_load_symbol_constructor)
    }

    fn emit_intrinsic_binding(
        &mut self,
        text: &str,
        emit_load: fn(&mut Self) -> Result<VirtualRegister, BytecompilerEmissionError>,
    ) -> Result<(), BytecompilerEmissionError> {
        let Some(name) = self.arena.identifiers().identifier_for_text(text) else {
            return Ok(());
        };
        if self.locals.contains_key(&name) {
            return Ok(());
        }
        let register = self.generator.registers_mut().reserve_local();
        let kind = self.local_binding_kind(name);
        let binding = LocalBinding { register, kind };
        self.locals.insert(name, binding);
        let value = emit_load(self)?;
        match kind {
            LocalBindingKind::Value => self.emit_write_binding(binding, value),
            LocalBindingKind::ClosureCell => {
                let cell = self.emit_new_closure_cell(value)?;
                self.emit_move(register, cell);
                self.initialized_cells.insert(name);
            }
        }
        Ok(())
    }

    fn emit_arguments_object_binding(
        &mut self,
        metadata: &FunctionMetadata,
        scope: &crate::syntax::ScopeNode,
    ) -> Result<(), BytecompilerEmissionError> {
        let Some(arguments) = self.arena.identifiers().identifier_for_text("arguments") else {
            return Ok(());
        };
        if self.locals.contains_key(&arguments)
            || !function_uses_arguments_identifier(self.arena, metadata, scope, arguments)?
        {
            return Ok(());
        }
        self.predeclare_function_binding(arguments)?;
        let binding = *self
            .locals
            .get(&arguments)
            .ok_or(BytecompilerEmissionError::UnboundIdentifier(arguments))?;
        let value = self.emit_create_arguments_object();
        match binding.kind {
            LocalBindingKind::Value => self.emit_write_binding(binding, value),
            LocalBindingKind::ClosureCell => {
                let cell = self.emit_new_closure_cell(value)?;
                self.emit_move(binding.register, cell);
                self.initialized_cells.insert(arguments);
            }
        }
        Ok(())
    }

    fn emit_function_binding(
        &mut self,
        name: ParserIdentifier,
        metadata: AstRef<FunctionMetadata>,
    ) -> Result<(), BytecompilerEmissionError> {
        let target = *self
            .locals
            .get(&name)
            .ok_or(BytecompilerEmissionError::UnboundIdentifier(name))?;
        let index = *self
            .function_metadata_indices
            .get(&metadata.raw_index())
            .ok_or(BytecompilerEmissionError::UnsupportedStatement(
                "function declaration has no compiled body",
            ))?;
        let captures = self
            .function_captures
            .get(&metadata.raw_index())
            .cloned()
            .unwrap_or_default();
        let value = self.emit_load_function(index, &captures)?;
        self.emit_write_binding(target, value);
        Ok(())
    }

    fn emit_declaration(
        &mut self,
        declaration: &DeclarationStmt,
    ) -> Result<(), BytecompilerEmissionError> {
        for (index, binding) in declaration.bindings.iter().enumerate() {
            let value = match declaration.initializers.get(index).copied().flatten() {
                Some(initializer) => self.emit_expression(initializer)?,
                None => self.emit_load_undefined()?,
            };
            self.emit_pattern_binding(*binding, value)?;
        }
        Ok(())
    }

    fn emit_pattern_binding(
        &mut self,
        pattern: AstRef<Pattern>,
        value: VirtualRegister,
    ) -> Result<(), BytecompilerEmissionError> {
        let pattern = self
            .arena
            .pattern(pattern)
            .cloned()
            .ok_or(BytecompilerEmissionError::MissingPattern)?;
        self.emit_owned_pattern_binding(&pattern, value)
    }

    fn parameter_rest_pattern(
        &self,
        pattern: AstRef<Pattern>,
    ) -> Result<Option<Pattern>, BytecompilerEmissionError> {
        let pattern = self
            .arena
            .pattern(pattern)
            .cloned()
            .ok_or(BytecompilerEmissionError::MissingPattern)?;
        Ok(match pattern {
            Pattern::Rest(pattern) => Some(*pattern),
            _ => None,
        })
    }

    fn emit_owned_pattern_binding(
        &mut self,
        pattern: &Pattern,
        value: VirtualRegister,
    ) -> Result<(), BytecompilerEmissionError> {
        match pattern {
            Pattern::Binding(name) => {
                let target = *self
                    .locals
                    .get(name)
                    .ok_or(BytecompilerEmissionError::UnboundIdentifier(*name))?;
                self.emit_write_binding(target, value);
                Ok(())
            }
            Pattern::Array(elements) => {
                for element in elements {
                    if let Pattern::Rest(pattern) = &element.pattern {
                        let rest = self.emit_array_rest(value, element.index)?;
                        self.emit_owned_pattern_binding(pattern, rest)?;
                    } else {
                        let index =
                            self.emit_load_int32(element.index.try_into().unwrap_or(i32::MAX))?;
                        let element_value = self.emit_get_by_value(value, index)?;
                        let element_value =
                            self.emit_default_if_undefined(element_value, element.default_value)?;
                        self.emit_owned_pattern_binding(&element.pattern, element_value)?;
                    }
                }
                Ok(())
            }
            Pattern::Object(properties) => {
                let excluded_keys = self.emit_new_array();
                for property in properties {
                    if let Pattern::Rest(pattern) = &property.pattern {
                        let rest = self.emit_object_rest(value, excluded_keys);
                        self.emit_owned_pattern_binding(pattern, rest)?;
                    } else {
                        let key = self.emit_object_literal_property_key(property.key)?;
                        self.emit_array_append_property_key(excluded_keys, key)?;
                        let property_value = self.emit_get_by_property_key(value, key)?;
                        let property_value =
                            self.emit_default_if_undefined(property_value, property.default_value)?;
                        self.emit_owned_pattern_binding(&property.pattern, property_value)?;
                    }
                }
                Ok(())
            }
            Pattern::Rest(pattern) => self.emit_owned_pattern_binding(pattern, value),
            Pattern::AssignmentTarget(_) => {
                Err(BytecompilerEmissionError::UnsupportedAssignmentTarget)
            }
        }
    }

    fn emit_default_if_undefined(
        &mut self,
        value: VirtualRegister,
        default_value: Option<AstRef<Expr>>,
    ) -> Result<VirtualRegister, BytecompilerEmissionError> {
        let Some(default_value) = default_value else {
            return Ok(value);
        };
        let undefined = self.emit_load_undefined()?;
        let is_undefined = self.emit_binary_opcode(CoreOpcode::StrictEqual, value, undefined)?;
        let after_default = self.declare_label(Some("binding_default_end"));
        self.emit_jump_if_false(is_undefined, after_default);
        let default_value = self.emit_expression(default_value)?;
        self.emit_move(value, default_value);
        self.bind_label(after_default)?;
        Ok(value)
    }

    fn emit_statement(
        &mut self,
        statement: AstRef<Stmt>,
    ) -> Result<StatementEmission, BytecompilerEmissionError> {
        let statement = self
            .arena
            .statement(statement)
            .cloned()
            .ok_or(BytecompilerEmissionError::MissingStatement)?;
        match statement {
            Stmt::Empty(_) => Ok(StatementEmission::default()),
            Stmt::Expression(expr) => self.emit_expression(expr).map(|value| StatementEmission {
                value: Some(value),
                terminated: false,
            }),
            Stmt::Declaration(declaration) => {
                self.emit_declaration(&declaration)?;
                Ok(StatementEmission::default())
            }
            Stmt::Block(block) => {
                let scope = self
                    .arena
                    .scope_node(block.scope)
                    .cloned()
                    .ok_or(BytecompilerEmissionError::MissingRootScope)?;
                let mut emission = StatementEmission::default();
                for statement in &scope.statements {
                    if emission.terminated {
                        break;
                    }
                    emission = self.emit_statement(*statement)?;
                }
                Ok(emission)
            }
            Stmt::FunctionDeclaration(_) => Ok(StatementEmission::default()),
            Stmt::If(statement) => {
                let condition = self.emit_expression(statement.condition)?;
                let else_label = self.declare_label(Some("if_else"));
                let end_label = self.declare_label(Some("if_end"));
                self.emit_jump_if_false(condition, else_label);
                let consequent = self.emit_statement(statement.consequent)?;
                if !consequent.terminated {
                    self.emit_jump(end_label);
                }
                self.bind_label(else_label)?;
                let alternate = match statement.alternate {
                    Some(alternate) => self.emit_statement(alternate)?,
                    None => StatementEmission::default(),
                };
                self.bind_label(end_label)?;
                Ok(StatementEmission {
                    value: alternate.value.or(consequent.value),
                    terminated: consequent.terminated
                        && statement.alternate.is_some()
                        && alternate.terminated,
                })
            }
            Stmt::While(statement) => {
                let continue_target = self.declare_label(Some("while_continue"));
                let break_target = self.declare_label(Some("while_break"));
                self.bind_label(continue_target)?;
                let condition = self.emit_expression(statement.condition)?;
                self.emit_jump_if_false(condition, break_target);
                self.loop_stack.push(LoopControlLabels {
                    break_target,
                    continue_target,
                    finally_stack_depth: self.finally_stack.len(),
                });
                let body = self.emit_statement(statement.body)?;
                let Some(labels) = self.loop_stack.pop() else {
                    return Err(BytecompilerEmissionError::UnsupportedStatement(
                        "loop control stack underflow",
                    ));
                };
                if !body.terminated {
                    self.emit_jump(labels.continue_target);
                }
                self.bind_label(labels.break_target)?;
                Ok(StatementEmission::default())
            }
            Stmt::For(statement) => self.emit_for_statement(&statement),
            Stmt::ForOf(statement) => self.emit_for_of_statement(&statement),
            Stmt::Try(statement) => self.emit_try_statement(&statement),
            Stmt::Control(control) => match control.kind {
                ControlKind::Return(value) => {
                    let value = match value {
                        Some(value) => self.emit_expression(value)?,
                        None => self.emit_load_undefined()?,
                    };
                    self.emit_return_completion(value)?;
                    Ok(StatementEmission::terminated(Some(value)))
                }
                ControlKind::Throw(value) => {
                    let value = self.emit_expression(value)?;
                    self.emit_throw_completion(value)?;
                    Ok(StatementEmission::terminated(Some(value)))
                }
                ControlKind::Break(label) => {
                    if label.is_some() {
                        return Err(BytecompilerEmissionError::UnsupportedStatement(
                            "named break statements need label-scope resolution",
                        ));
                    }
                    let Some(labels) = self.loop_stack.last().copied() else {
                        return Err(BytecompilerEmissionError::UnsupportedStatement(
                            "break statement outside loop",
                        ));
                    };
                    self.emit_break_completion(labels)?;
                    Ok(StatementEmission::terminated(None))
                }
                ControlKind::Continue(label) => {
                    if label.is_some() {
                        return Err(BytecompilerEmissionError::UnsupportedStatement(
                            "named continue statements need label-scope resolution",
                        ));
                    }
                    let Some(labels) = self.loop_stack.last().copied() else {
                        return Err(BytecompilerEmissionError::UnsupportedStatement(
                            "continue statement outside loop",
                        ));
                    };
                    self.emit_continue_completion(labels)?;
                    Ok(StatementEmission::terminated(None))
                }
            },
            Stmt::Module(_) => Err(BytecompilerEmissionError::UnsupportedStatement(
                "module items need module-record emission",
            )),
        }
    }

    fn emit_for_statement(
        &mut self,
        statement: &crate::syntax::ast::ForStmt,
    ) -> Result<StatementEmission, BytecompilerEmissionError> {
        if let Some(init) = &statement.init {
            match init {
                ForInit::Declaration(declaration) => self.emit_declaration(declaration)?,
                ForInit::Expression(expression) => {
                    self.emit_expression(*expression)?;
                }
            }
        }
        let loop_start = self.declare_label(Some("for_start"));
        let continue_target = self.declare_label(Some("for_continue"));
        let break_target = self.declare_label(Some("for_break"));
        self.bind_label(loop_start)?;
        if let Some(condition) = statement.condition {
            let condition = self.emit_expression(condition)?;
            self.emit_jump_if_false(condition, break_target);
        }
        self.loop_stack.push(LoopControlLabels {
            break_target,
            continue_target,
            finally_stack_depth: self.finally_stack.len(),
        });
        let body = self.emit_statement(statement.body)?;
        let Some(labels) = self.loop_stack.pop() else {
            return Err(BytecompilerEmissionError::UnsupportedStatement(
                "loop control stack underflow",
            ));
        };
        self.bind_label(labels.continue_target)?;
        if let Some(update) = statement.update {
            self.emit_expression(update)?;
        }
        if !body.terminated {
            self.emit_jump(loop_start);
        }
        self.bind_label(labels.break_target)?;
        Ok(StatementEmission::default())
    }

    fn emit_for_of_statement(
        &mut self,
        statement: &crate::syntax::ast::ForOfStmt,
    ) -> Result<StatementEmission, BytecompilerEmissionError> {
        let iterable = self.emit_expression(statement.iterable)?;
        let values = self.emit_new_array();
        self.emit_array_append_spread(values, iterable);
        let index = self.emit_load_int32(0)?;
        let one = self.emit_load_int32(1)?;
        let loop_start = self.declare_label(Some("for_of_start"));
        let continue_target = self.declare_label(Some("for_of_continue"));
        let break_target = self.declare_label(Some("for_of_break"));
        self.bind_label(loop_start)?;
        let length = self.emit_array_length(values)?;
        let condition = self.emit_binary_opcode(CoreOpcode::LessThanInt32, index, length)?;
        self.emit_jump_if_false(condition, break_target);
        let value = self.emit_get_by_index(values, index)?;
        self.emit_write_for_of_binding(statement.binding, value)?;
        self.loop_stack.push(LoopControlLabels {
            break_target,
            continue_target,
            finally_stack_depth: self.finally_stack.len(),
        });
        let body = self.emit_statement(statement.body)?;
        let Some(labels) = self.loop_stack.pop() else {
            return Err(BytecompilerEmissionError::UnsupportedStatement(
                "loop control stack underflow",
            ));
        };
        self.bind_label(labels.continue_target)?;
        let next_index = self.emit_binary_opcode(CoreOpcode::AddInt32, index, one)?;
        self.emit_move(index, next_index);
        if !body.terminated {
            self.emit_jump(loop_start);
        }
        self.bind_label(labels.break_target)?;
        Ok(StatementEmission::default())
    }

    fn emit_write_for_of_binding(
        &mut self,
        binding: ForOfBinding,
        value: VirtualRegister,
    ) -> Result<(), BytecompilerEmissionError> {
        let name = match binding {
            ForOfBinding::Declaration { name, .. } | ForOfBinding::Assignment(name) => name,
        };
        let target = *self
            .locals
            .get(&name)
            .ok_or(BytecompilerEmissionError::UnboundIdentifier(name))?;
        self.emit_write_binding(target, value);
        Ok(())
    }

    fn emit_try_statement(
        &mut self,
        statement: &crate::syntax::ast::TryStmt,
    ) -> Result<StatementEmission, BytecompilerEmissionError> {
        match (statement.catch, statement.finally) {
            (Some(catch), Some(finally_body)) => {
                self.emit_try_catch_finally_statement(statement.body, catch, finally_body)
            }
            (Some(catch), None) => self.emit_try_catch_statement(statement.body, catch),
            (None, Some(finally_body)) => {
                self.emit_try_finally_statement(statement.body, finally_body)
            }
            (None, None) => Err(BytecompilerEmissionError::UnsupportedStatement(
                "try statements need catch or finally",
            )),
        }
    }

    fn emit_try_catch_statement(
        &mut self,
        body: AstRef<Stmt>,
        catch: crate::syntax::ast::CatchClause,
    ) -> Result<StatementEmission, BytecompilerEmissionError> {
        let protected_start = self.current_bytecode_index();
        let body_emission = self.emit_statement(body)?;
        let protected_end = self.current_bytecode_index();
        let after_catch = self.declare_label(Some("try_after_catch"));
        if !body_emission.terminated {
            self.emit_jump(after_catch);
        }
        let catch_target = self.declare_label(Some("try_catch"));
        self.bind_label(catch_target)?;
        let catch_index = self.current_bytecode_index();
        self.record_handler(
            protected_start,
            protected_end,
            catch_index,
            HandlerKind::Catch,
        );
        let exception = self.emit_take_exception()?;
        if let Some(binding) = catch.binding {
            let target = *self
                .locals
                .get(&binding)
                .ok_or(BytecompilerEmissionError::UnboundIdentifier(binding))?;
            self.emit_write_binding(target, exception);
        }
        let catch_emission = self.emit_statement(catch.body)?;
        self.bind_label(after_catch)?;
        Ok(StatementEmission {
            value: catch_emission.value.or(body_emission.value),
            terminated: false,
        })
    }

    fn emit_try_finally_statement(
        &mut self,
        body: AstRef<Stmt>,
        finally_body: AstRef<Stmt>,
    ) -> Result<StatementEmission, BytecompilerEmissionError> {
        let protected_start = self.current_bytecode_index();
        self.push_finally_context(finally_body, true);
        let body_emission = self.emit_statement(body)?;
        let context = self.pop_finally_context()?;
        let protected_end = self.current_bytecode_index();

        let after_try = self.declare_label(Some("try_after_finally"));
        let exceptional_target = self.declare_label(Some("try_finally_exception"));

        if !body_emission.terminated {
            let finalizer = self.emit_finally_body(finally_body)?;
            if !finalizer.terminated {
                self.emit_jump(after_try);
            }
        }

        self.emit_finally_exit_blocks(context.body, context.exits)?;

        self.bind_label(exceptional_target)?;
        let exceptional_index = self.current_bytecode_index();
        self.record_handler(
            protected_start,
            protected_end,
            exceptional_index,
            HandlerKind::Finally,
        );
        let exception = self.emit_take_exception()?;
        let finalizer = self.emit_finally_body(finally_body)?;
        if !finalizer.terminated {
            self.emit_throw_completion(exception)?;
        }

        self.bind_label(after_try)?;
        Ok(StatementEmission {
            value: body_emission.value,
            terminated: false,
        })
    }

    fn emit_try_catch_finally_statement(
        &mut self,
        body: AstRef<Stmt>,
        catch: crate::syntax::ast::CatchClause,
        finally_body: AstRef<Stmt>,
    ) -> Result<StatementEmission, BytecompilerEmissionError> {
        let body_start = self.current_bytecode_index();
        self.push_finally_context(finally_body, false);
        let body_emission = self.emit_statement(body)?;
        let body_end = self.current_bytecode_index();
        let body_context = self.pop_finally_context()?;

        let after_try = self.declare_label(Some("try_after_catch_finally"));
        let catch_target = self.declare_label(Some("try_catch"));
        let catch_exception_target = self.declare_label(Some("try_catch_finally_exception"));

        if !body_emission.terminated {
            let finalizer = self.emit_finally_body(finally_body)?;
            if !finalizer.terminated {
                self.emit_jump(after_try);
            }
        }

        self.emit_finally_exit_blocks(body_context.body, body_context.exits)?;

        self.bind_label(catch_target)?;
        let catch_index = self.current_bytecode_index();
        self.record_handler(body_start, body_end, catch_index, HandlerKind::Catch);
        let exception = self.emit_take_exception()?;
        if let Some(binding) = catch.binding {
            let target = *self
                .locals
                .get(&binding)
                .ok_or(BytecompilerEmissionError::UnboundIdentifier(binding))?;
            self.emit_write_binding(target, exception);
        }

        let catch_body_start = self.current_bytecode_index();
        self.push_finally_context(finally_body, true);
        let catch_emission = self.emit_statement(catch.body)?;
        let catch_context = self.pop_finally_context()?;
        let catch_body_end = self.current_bytecode_index();

        if !catch_emission.terminated {
            let finalizer = self.emit_finally_body(finally_body)?;
            if !finalizer.terminated {
                self.emit_jump(after_try);
            }
        }

        self.emit_finally_exit_blocks(catch_context.body, catch_context.exits)?;

        self.bind_label(catch_exception_target)?;
        let catch_exception_index = self.current_bytecode_index();
        self.record_handler(
            catch_body_start,
            catch_body_end,
            catch_exception_index,
            HandlerKind::Finally,
        );
        let exception = self.emit_take_exception()?;
        let finalizer = self.emit_finally_body(finally_body)?;
        if !finalizer.terminated {
            self.emit_throw_completion(exception)?;
        }

        self.bind_label(after_try)?;
        Ok(StatementEmission {
            value: catch_emission.value.or(body_emission.value),
            terminated: false,
        })
    }

    fn emit_expression(
        &mut self,
        expression: AstRef<Expr>,
    ) -> Result<VirtualRegister, BytecompilerEmissionError> {
        let expression = self
            .arena
            .expression(expression)
            .cloned()
            .ok_or(BytecompilerEmissionError::MissingExpression)?;
        match expression {
            Expr::Literal(literal) => match literal.kind {
                LiteralKind::Number {
                    value: NumberLiteralValue::Int32(value),
                } => self.emit_load_int32(value),
                LiteralKind::Number {
                    value: NumberLiteralValue::DoubleBits(bits),
                } => self.emit_load_double(bits),
                LiteralKind::Boolean(value) => self.emit_load_bool(value),
                LiteralKind::Null => self.emit_load_null(),
                LiteralKind::String { text } => self.emit_load_string(text),
                LiteralKind::BigInt { text } => self.emit_load_bigint(text),
                LiteralKind::RegExp { pattern, flags } => self.emit_new_regexp(pattern, flags),
            },
            Expr::Name(name) => match name.kind {
                NameKind::Resolve => {
                    let binding = *self
                        .locals
                        .get(&name.name)
                        .ok_or(BytecompilerEmissionError::UnboundIdentifier(name.name))?;
                    self.emit_read_binding(binding)
                }
                NameKind::This => Ok(VirtualRegister::argument_or_header(5)),
                _ => Err(BytecompilerEmissionError::UnsupportedExpression(
                    "special name expression is not lowered yet",
                )),
            },
            Expr::Assignment(assignment) => self.emit_assignment(&assignment),
            Expr::Conditional(conditional) => self.emit_conditional(&conditional),
            Expr::Binary(binary) => self.emit_binary(&binary),
            Expr::Call(call) => self.emit_call(&call),
            Expr::New(new) => self.emit_construct(&new),
            Expr::Member(member) => self.emit_member_get(&member),
            Expr::Object(object) => self.emit_object_literal(&object),
            Expr::Array(array) => self.emit_array_literal(&array),
            Expr::Function(metadata) => self.emit_function_expression(metadata),
            Expr::Class(class) => self.emit_class_expression(&class),
            Expr::Unary(unary) => self.emit_unary(&unary),
            Expr::Template(_) | Expr::ImportMeta(_) => {
                Err(BytecompilerEmissionError::UnsupportedExpression(
                    "expression kind is not lowered yet",
                ))
            }
        }
    }

    fn emit_unary(
        &mut self,
        unary: &UnaryExpr,
    ) -> Result<VirtualRegister, BytecompilerEmissionError> {
        if unary.op == AstUnaryOperator::Delete {
            return self.emit_delete(unary.argument);
        }
        if matches!(
            unary.op,
            AstUnaryOperator::PreIncrement
                | AstUnaryOperator::PreDecrement
                | AstUnaryOperator::PostIncrement
                | AstUnaryOperator::PostDecrement
        ) {
            return self.emit_update(unary);
        }
        let argument = self.emit_expression(unary.argument)?;
        let opcode = match unary.op {
            AstUnaryOperator::Plus => CoreOpcode::ToNumber,
            AstUnaryOperator::Minus => CoreOpcode::NegateNumber,
            AstUnaryOperator::LogicalNot => CoreOpcode::LogicalNot,
            AstUnaryOperator::Typeof => CoreOpcode::TypeOf,
            AstUnaryOperator::Void => CoreOpcode::Void,
            AstUnaryOperator::Delete => return self.emit_delete(unary.argument),
            AstUnaryOperator::BitNot => CoreOpcode::BitNotInt32,
            AstUnaryOperator::PreIncrement
            | AstUnaryOperator::PreDecrement
            | AstUnaryOperator::PostIncrement
            | AstUnaryOperator::PostDecrement => {
                unreachable!("update expressions are lowered before unary argument emission");
            }
            AstUnaryOperator::Await | AstUnaryOperator::Yield => {
                return Err(BytecompilerEmissionError::UnsupportedExpression(
                    "suspending unary expression needs async/generator lowering",
                ));
            }
        };
        let destination = self
            .generator
            .registers_mut()
            .reserve_temporary(TemporaryLifetime::Expression);
        self.generator.instructions_mut().declare_instruction(
            opcode.opcode(),
            OperandWidth::Narrow,
            vec![Operand::Register(destination), Operand::Register(argument)],
        );
        Ok(destination)
    }

    fn emit_update(
        &mut self,
        unary: &UnaryExpr,
    ) -> Result<VirtualRegister, BytecompilerEmissionError> {
        let reference = self.emit_assignment_reference(unary.argument)?;
        let numeric_old = self.emit_unary_opcode(CoreOpcode::ToNumber, reference.value)?;
        let result = if matches!(
            unary.op,
            AstUnaryOperator::PostIncrement | AstUnaryOperator::PostDecrement
        ) {
            self.emit_move_temporary(numeric_old)
        } else {
            numeric_old
        };
        let one = self.emit_load_int32(1)?;
        let opcode = match unary.op {
            AstUnaryOperator::PreIncrement | AstUnaryOperator::PostIncrement => {
                CoreOpcode::AddInt32
            }
            AstUnaryOperator::PreDecrement | AstUnaryOperator::PostDecrement => {
                CoreOpcode::SubInt32
            }
            _ => unreachable!("emit_update only receives update operators"),
        };
        let updated = self.emit_binary_opcode(opcode, numeric_old, one)?;
        self.emit_write_assignment_target(reference.target, updated);
        if matches!(
            unary.op,
            AstUnaryOperator::PostIncrement | AstUnaryOperator::PostDecrement
        ) {
            Ok(result)
        } else {
            Ok(updated)
        }
    }

    fn emit_unary_opcode(
        &mut self,
        opcode: CoreOpcode,
        source: VirtualRegister,
    ) -> Result<VirtualRegister, BytecompilerEmissionError> {
        let destination = self
            .generator
            .registers_mut()
            .reserve_temporary(TemporaryLifetime::Expression);
        self.generator.instructions_mut().declare_instruction(
            opcode.opcode(),
            OperandWidth::Narrow,
            vec![Operand::Register(destination), Operand::Register(source)],
        );
        Ok(destination)
    }

    fn emit_delete(
        &mut self,
        argument: AstRef<Expr>,
    ) -> Result<VirtualRegister, BytecompilerEmissionError> {
        let expression = self
            .arena
            .expression(argument)
            .cloned()
            .ok_or(BytecompilerEmissionError::MissingExpression)?;
        match expression {
            Expr::Member(member) => {
                if self.expression_is_super(member.base) {
                    return Err(BytecompilerEmissionError::UnsupportedExpression(
                        "delete of super property is invalid",
                    ));
                }
                let object = self.emit_expression(member.base)?;
                match member.member {
                    MemberKind::Dot(name) => self.emit_delete_by_name(object, name),
                    MemberKind::Bracket(index) => {
                        let key = self.emit_expression(index)?;
                        self.emit_delete_by_value(object, key)
                    }
                    MemberKind::PrivateDot(_) => {
                        Err(BytecompilerEmissionError::UnsupportedExpression(
                            "delete of private property needs private-name lowering",
                        ))
                    }
                }
            }
            Expr::Name(name) if name.kind == NameKind::Resolve => {
                self.emit_load_bool(!self.locals.contains_key(&name.name))
            }
            _ => {
                self.emit_expression(argument)?;
                self.emit_load_bool(true)
            }
        }
    }

    fn emit_assignment(
        &mut self,
        assignment: &AssignmentExpr,
    ) -> Result<VirtualRegister, BytecompilerEmissionError> {
        let target = self
            .arena
            .pattern(assignment.target)
            .cloned()
            .ok_or(BytecompilerEmissionError::MissingPattern)?;
        let Pattern::AssignmentTarget(expr) = target else {
            return Err(BytecompilerEmissionError::UnsupportedAssignmentTarget);
        };
        if assignment.op != AssignmentOperator::Assign {
            return self.emit_compound_assignment(expr, assignment.op, assignment.value);
        }
        match self.arena.expression(expr).cloned() {
            Some(Expr::Name(name)) if name.kind == NameKind::Resolve => {
                let target = *self
                    .locals
                    .get(&name.name)
                    .ok_or(BytecompilerEmissionError::UnboundIdentifier(name.name))?;
                let value = self.emit_expression(assignment.value)?;
                self.emit_write_binding(target, value);
                Ok(value)
            }
            Some(Expr::Member(member)) => {
                let object = self.emit_expression(member.base)?;
                match member.member {
                    MemberKind::Dot(name) => {
                        let value = self.emit_expression(assignment.value)?;
                        self.emit_put_by_name(object, name, value);
                        Ok(value)
                    }
                    MemberKind::Bracket(index) => {
                        let key = self.emit_expression(index)?;
                        let value = self.emit_expression(assignment.value)?;
                        self.emit_put_by_value(object, key, value);
                        Ok(value)
                    }
                    MemberKind::PrivateDot(_) => {
                        Err(BytecompilerEmissionError::UnsupportedAssignmentTarget)
                    }
                }
            }
            Some(Expr::Array(array)) => {
                let value = self.emit_expression(assignment.value)?;
                self.emit_array_destructuring_assignment(&array, value)?;
                Ok(value)
            }
            Some(Expr::Object(object)) => {
                let value = self.emit_expression(assignment.value)?;
                self.emit_object_destructuring_assignment(&object, value)?;
                Ok(value)
            }
            _ => Err(BytecompilerEmissionError::UnsupportedAssignmentTarget),
        }
    }

    fn emit_compound_assignment(
        &mut self,
        target: AstRef<Expr>,
        op: AssignmentOperator,
        value: AstRef<Expr>,
    ) -> Result<VirtualRegister, BytecompilerEmissionError> {
        let opcode = compound_assignment_opcode(op).ok_or(
            BytecompilerEmissionError::UnsupportedExpression(
                "logical compound assignment is not lowered yet",
            ),
        )?;
        let reference = self.emit_assignment_reference(target)?;
        let right = self.emit_expression(value)?;
        let result = self.emit_binary_opcode(opcode, reference.value, right)?;
        self.emit_write_assignment_target(reference.target, result);
        Ok(result)
    }

    fn emit_assignment_reference(
        &mut self,
        target: AstRef<Expr>,
    ) -> Result<EmittedAssignmentReference, BytecompilerEmissionError> {
        match self.arena.expression(target).cloned() {
            Some(Expr::Name(name)) if name.kind == NameKind::Resolve => {
                let binding = *self
                    .locals
                    .get(&name.name)
                    .ok_or(BytecompilerEmissionError::UnboundIdentifier(name.name))?;
                let value = self.emit_read_binding(binding)?;
                Ok(EmittedAssignmentReference {
                    target: EmittedAssignmentTarget::Local(binding),
                    value,
                })
            }
            Some(Expr::Member(member)) => {
                let object = self.emit_expression(member.base)?;
                match member.member {
                    MemberKind::Dot(name) => {
                        let value = self.emit_get_by_name(object, name)?;
                        Ok(EmittedAssignmentReference {
                            target: EmittedAssignmentTarget::Dot { object, name },
                            value,
                        })
                    }
                    MemberKind::Bracket(index) => {
                        let key = self.emit_expression(index)?;
                        let value = self.emit_get_by_value(object, key)?;
                        Ok(EmittedAssignmentReference {
                            target: EmittedAssignmentTarget::Bracket { object, key },
                            value,
                        })
                    }
                    MemberKind::PrivateDot(_) => {
                        Err(BytecompilerEmissionError::UnsupportedAssignmentTarget)
                    }
                }
            }
            _ => Err(BytecompilerEmissionError::UnsupportedAssignmentTarget),
        }
    }

    fn emit_write_assignment_target(
        &mut self,
        target: EmittedAssignmentTarget,
        value: VirtualRegister,
    ) {
        match target {
            EmittedAssignmentTarget::Local(binding) => self.emit_write_binding(binding, value),
            EmittedAssignmentTarget::Dot { object, name } => {
                self.emit_put_by_name(object, name, value)
            }
            EmittedAssignmentTarget::Bracket { object, key } => {
                self.emit_put_by_value(object, key, value)
            }
        }
    }

    fn emit_destructuring_assignment_target(
        &mut self,
        target: AstRef<Expr>,
        value: VirtualRegister,
    ) -> Result<(), BytecompilerEmissionError> {
        match self.arena.expression(target).cloned() {
            Some(Expr::Name(name)) if name.kind == NameKind::Resolve => {
                let target = *self
                    .locals
                    .get(&name.name)
                    .ok_or(BytecompilerEmissionError::UnboundIdentifier(name.name))?;
                self.emit_write_binding(target, value);
                Ok(())
            }
            Some(Expr::Member(member)) => {
                let object = self.emit_expression(member.base)?;
                match member.member {
                    MemberKind::Dot(name) => {
                        self.emit_put_by_name(object, name, value);
                        Ok(())
                    }
                    MemberKind::Bracket(index) => {
                        let key = self.emit_expression(index)?;
                        self.emit_put_by_value(object, key, value);
                        Ok(())
                    }
                    MemberKind::PrivateDot(_) => {
                        Err(BytecompilerEmissionError::UnsupportedAssignmentTarget)
                    }
                }
            }
            Some(Expr::Array(array)) => self.emit_array_destructuring_assignment(&array, value),
            Some(Expr::Object(object)) => self.emit_object_destructuring_assignment(&object, value),
            _ => Err(BytecompilerEmissionError::UnsupportedAssignmentTarget),
        }
    }

    fn emit_array_destructuring_assignment(
        &mut self,
        array: &crate::syntax::ast::ArrayLiteralExpr,
        value: VirtualRegister,
    ) -> Result<(), BytecompilerEmissionError> {
        for (index, element) in array.elements.iter().enumerate() {
            match element {
                ArrayLiteralElement::Expression(expression) => {
                    let index = self.emit_load_int32(index.try_into().unwrap_or(i32::MAX))?;
                    let element_value = self.emit_get_by_value(value, index)?;
                    let (target, default_value) =
                        self.destructuring_assignment_target_and_default(*expression)?;
                    let element_value =
                        self.emit_default_if_undefined(element_value, default_value)?;
                    self.emit_destructuring_assignment_target(target, element_value)?;
                }
                ArrayLiteralElement::Elision(_) => {}
                ArrayLiteralElement::Spread { value: target, .. } => {
                    if index + 1 != array.elements.len() {
                        return Err(BytecompilerEmissionError::UnsupportedAssignmentTarget);
                    }
                    let rest = self.emit_array_rest(value, index)?;
                    let (target, default_value) =
                        self.destructuring_assignment_target_and_default(*target)?;
                    if default_value.is_some() {
                        return Err(BytecompilerEmissionError::UnsupportedAssignmentTarget);
                    }
                    self.emit_destructuring_assignment_target(target, rest)?;
                }
            }
        }
        Ok(())
    }

    fn emit_object_destructuring_assignment(
        &mut self,
        object: &crate::syntax::ast::ObjectLiteralExpr,
        value: VirtualRegister,
    ) -> Result<(), BytecompilerEmissionError> {
        let excluded_keys = self.emit_new_array();
        for (index, property) in object.properties.iter().enumerate() {
            if property.kind == ObjectLiteralPropertyKind::Spread {
                if index + 1 != object.properties.len() {
                    return Err(BytecompilerEmissionError::UnsupportedAssignmentTarget);
                }
                let rest = self.emit_object_rest(value, excluded_keys);
                let (target, default_value) =
                    self.destructuring_assignment_target_and_default(property.value)?;
                if default_value.is_some() {
                    return Err(BytecompilerEmissionError::UnsupportedAssignmentTarget);
                }
                self.emit_destructuring_assignment_target(target, rest)?;
                continue;
            }
            if property.kind != ObjectLiteralPropertyKind::Data {
                return Err(BytecompilerEmissionError::UnsupportedAssignmentTarget);
            }
            let key = self.emit_object_literal_property_key(property.key)?;
            self.emit_array_append_property_key(excluded_keys, key)?;
            let property_value = self.emit_get_by_property_key(value, key)?;
            let (target, default_value) =
                self.destructuring_assignment_target_and_default(property.value)?;
            let property_value = self.emit_default_if_undefined(property_value, default_value)?;
            self.emit_destructuring_assignment_target(target, property_value)?;
        }
        Ok(())
    }

    fn destructuring_assignment_target_and_default(
        &self,
        expression: AstRef<Expr>,
    ) -> Result<(AstRef<Expr>, Option<AstRef<Expr>>), BytecompilerEmissionError> {
        if let Some(Expr::Assignment(assignment)) = self.arena.expression(expression) {
            let Pattern::AssignmentTarget(target) = self
                .arena
                .pattern(assignment.target)
                .ok_or(BytecompilerEmissionError::MissingPattern)?
            else {
                return Err(BytecompilerEmissionError::UnsupportedAssignmentTarget);
            };
            return Ok((*target, Some(assignment.value)));
        }
        Ok((expression, None))
    }

    fn emit_call(&mut self, call: &CallExpr) -> Result<VirtualRegister, BytecompilerEmissionError> {
        if self.expression_is_super(call.callee) {
            return self.emit_construct_super(&call.arguments);
        }
        if let Some(Expr::Member(member)) = self.arena.expression(call.callee).cloned() {
            return self.emit_member_call(&member, &call.arguments);
        }
        let callee = self.emit_expression(call.callee)?;
        let mut argument_registers = Vec::new();
        for argument in &call.arguments {
            argument_registers.push(self.emit_expression(*argument)?);
        }
        let destination = self
            .generator
            .registers_mut()
            .reserve_temporary(TemporaryLifetime::Expression);
        let mut operands = vec![
            Operand::Register(destination),
            Operand::Register(callee),
            Operand::UnsignedImmediate(argument_registers.len().try_into().unwrap_or(u32::MAX)),
        ];
        operands.extend(argument_registers.into_iter().map(Operand::Register));
        self.generator.instructions_mut().declare_instruction(
            CoreOpcode::Call.opcode(),
            OperandWidth::Narrow,
            operands,
        );
        Ok(destination)
    }

    fn emit_member_call(
        &mut self,
        member: &MemberExpr,
        arguments: &[AstRef<Expr>],
    ) -> Result<VirtualRegister, BytecompilerEmissionError> {
        if self.expression_is_super(member.base) {
            let callee = match member.member {
                MemberKind::Dot(name) => self.emit_get_super_by_name(name)?,
                MemberKind::Bracket(_) => {
                    return Err(BytecompilerEmissionError::UnsupportedExpression(
                        "super indexed method calls need reference-aware lowering",
                    ));
                }
                MemberKind::PrivateDot(_) => {
                    return Err(BytecompilerEmissionError::UnsupportedExpression(
                        "super private method calls need private-name lowering",
                    ));
                }
            };
            let this_value = VirtualRegister::argument_or_header(5);
            let mut argument_registers = Vec::new();
            for argument in arguments {
                argument_registers.push(self.emit_expression(*argument)?);
            }
            let destination = self
                .generator
                .registers_mut()
                .reserve_temporary(TemporaryLifetime::Expression);
            let mut operands = vec![
                Operand::Register(destination),
                Operand::Register(callee),
                Operand::Register(this_value),
                Operand::UnsignedImmediate(argument_registers.len().try_into().unwrap_or(u32::MAX)),
            ];
            operands.extend(argument_registers.into_iter().map(Operand::Register));
            self.generator.instructions_mut().declare_instruction(
                CoreOpcode::CallWithThis.opcode(),
                OperandWidth::Narrow,
                operands,
            );
            return Ok(destination);
        }
        let this_value = self.emit_expression(member.base)?;
        let callee = match member.member {
            MemberKind::Dot(name) if self.is_length_property(name) => {
                self.emit_get_length(this_value, name)?
            }
            MemberKind::Dot(name) => self.emit_get_by_name(this_value, name)?,
            MemberKind::Bracket(index) => {
                let key = self.emit_expression(index)?;
                self.emit_get_by_value(this_value, key)?
            }
            MemberKind::PrivateDot(_) => {
                return Err(BytecompilerEmissionError::UnsupportedExpression(
                    "private method call needs private-name lowering",
                ));
            }
        };
        let mut argument_registers = Vec::new();
        for argument in arguments {
            argument_registers.push(self.emit_expression(*argument)?);
        }
        let destination = self
            .generator
            .registers_mut()
            .reserve_temporary(TemporaryLifetime::Expression);
        let mut operands = vec![
            Operand::Register(destination),
            Operand::Register(callee),
            Operand::Register(this_value),
            Operand::UnsignedImmediate(argument_registers.len().try_into().unwrap_or(u32::MAX)),
        ];
        operands.extend(argument_registers.into_iter().map(Operand::Register));
        self.generator.instructions_mut().declare_instruction(
            CoreOpcode::CallWithThis.opcode(),
            OperandWidth::Narrow,
            operands,
        );
        Ok(destination)
    }

    fn emit_construct(
        &mut self,
        new: &NewExpr,
    ) -> Result<VirtualRegister, BytecompilerEmissionError> {
        let callee = self.emit_expression(new.callee)?;
        let mut argument_registers = Vec::new();
        for argument in &new.arguments {
            argument_registers.push(self.emit_expression(*argument)?);
        }
        let destination = self
            .generator
            .registers_mut()
            .reserve_temporary(TemporaryLifetime::Expression);
        let mut operands = vec![
            Operand::Register(destination),
            Operand::Register(callee),
            Operand::UnsignedImmediate(argument_registers.len().try_into().unwrap_or(u32::MAX)),
        ];
        operands.extend(argument_registers.into_iter().map(Operand::Register));
        self.generator.instructions_mut().declare_instruction(
            CoreOpcode::Construct.opcode(),
            OperandWidth::Narrow,
            operands,
        );
        Ok(destination)
    }

    fn emit_construct_super(
        &mut self,
        arguments: &[AstRef<Expr>],
    ) -> Result<VirtualRegister, BytecompilerEmissionError> {
        let this_value = VirtualRegister::argument_or_header(5);
        let mut argument_registers = Vec::new();
        for argument in arguments {
            argument_registers.push(self.emit_expression(*argument)?);
        }
        let destination = self
            .generator
            .registers_mut()
            .reserve_temporary(TemporaryLifetime::Expression);
        let mut operands = vec![
            Operand::Register(destination),
            Operand::Register(this_value),
            Operand::UnsignedImmediate(argument_registers.len().try_into().unwrap_or(u32::MAX)),
        ];
        operands.extend(argument_registers.into_iter().map(Operand::Register));
        self.generator.instructions_mut().declare_instruction(
            CoreOpcode::ConstructSuper.opcode(),
            OperandWidth::Narrow,
            operands,
        );
        Ok(destination)
    }

    fn emit_function_expression(
        &mut self,
        metadata: AstRef<FunctionMetadata>,
    ) -> Result<VirtualRegister, BytecompilerEmissionError> {
        let index = *self
            .function_metadata_indices
            .get(&metadata.raw_index())
            .ok_or(BytecompilerEmissionError::UnsupportedExpression(
                "function expression has no compiled body",
            ))?;
        let captures = self
            .function_captures
            .get(&metadata.raw_index())
            .cloned()
            .unwrap_or_default();
        self.emit_load_function(index, &captures)
    }

    fn emit_class_expression(
        &mut self,
        class: &ClassExpr,
    ) -> Result<VirtualRegister, BytecompilerEmissionError> {
        let superclass = match class.heritage {
            Some(heritage) => Some(self.emit_expression(heritage)?),
            None => None,
        };
        let constructor = class
            .elements
            .iter()
            .find(|element| {
                !element.is_static
                    && element.kind == ClassElementKind::Method
                    && matches!(
                        element.name,
                        ClassElementName::Public(name) if self.identifier_text_is(name, "constructor")
                    )
            })
            .and_then(|element| element.metadata)
            .ok_or(BytecompilerEmissionError::UnsupportedExpression(
                "class without constructor needs default constructor synthesis",
            ))?;

        let class_value = self.emit_function_expression(constructor)?;
        let prototype_name = self
            .arena
            .identifiers()
            .identifier_for_text("prototype")
            .ok_or(BytecompilerEmissionError::UnsupportedExpression(
                "class lowering needs prototype identifier",
            ))?;
        let prototype = self.emit_get_by_name(class_value, prototype_name)?;

        let superclass_prototype = if let Some(superclass) = superclass {
            let superclass_prototype = self.emit_get_by_name(superclass, prototype_name)?;
            self.emit_set_prototype(prototype, superclass_prototype);
            self.emit_set_prototype(class_value, superclass);
            self.emit_set_function_super(class_value, superclass_prototype, superclass);
            if class.elements.iter().any(|element| {
                element.is_synthesized_default_constructor
                    && !element.is_static
                    && element.kind == ClassElementKind::Method
            }) {
                self.emit_set_default_derived_constructor(class_value);
            }
            Some(superclass_prototype)
        } else {
            None
        };

        for element in &class.elements {
            if !element.is_static
                && element.kind == ClassElementKind::Method
                && matches!(
                    element.name,
                    ClassElementName::Public(name) if self.identifier_text_is(name, "constructor")
                )
            {
                continue;
            }
            let key = self.emit_class_element_key(element.name)?;
            if element.kind == ClassElementKind::Field {
                let value = if element.is_static {
                    match element.initializer {
                        Some(initializer) => self.emit_expression(initializer)?,
                        None => self.emit_load_undefined()?,
                    }
                } else {
                    match element.metadata {
                        Some(initializer) => self.emit_function_expression(initializer)?,
                        None => self.emit_load_undefined()?,
                    }
                };
                if element.is_static {
                    self.emit_put_by_property_key(class_value, key, value);
                } else {
                    self.emit_add_instance_field_by_property_key(class_value, key, value);
                }
                continue;
            }
            if matches!(
                element.kind,
                ClassElementKind::Getter | ClassElementKind::Setter
            ) {
                let metadata =
                    element
                        .metadata
                        .ok_or(BytecompilerEmissionError::UnsupportedExpression(
                            "class accessor is missing function metadata",
                        ))?;
                let accessor = self.emit_function_expression(metadata)?;
                if let Some(superclass) = superclass {
                    let super_base = if element.is_static {
                        superclass
                    } else {
                        superclass_prototype.ok_or(
                            BytecompilerEmissionError::UnsupportedExpression(
                                "instance super accessor needs superclass prototype",
                            ),
                        )?
                    };
                    self.emit_set_function_super(accessor, super_base, superclass);
                }
                let receiver = if element.is_static {
                    class_value
                } else {
                    prototype
                };
                match element.kind {
                    ClassElementKind::Getter => {
                        self.emit_define_getter_by_property_key(receiver, key, accessor)
                    }
                    ClassElementKind::Setter => {
                        self.emit_define_setter_by_property_key(receiver, key, accessor)
                    }
                    _ => {}
                }
                continue;
            }
            if element.kind != ClassElementKind::Method {
                return Err(BytecompilerEmissionError::UnsupportedExpression(
                    "class element kind needs dedicated lowering",
                ));
            }
            let metadata =
                element
                    .metadata
                    .ok_or(BytecompilerEmissionError::UnsupportedExpression(
                        "class method is missing function metadata",
                    ))?;
            let method = self.emit_function_expression(metadata)?;
            if let Some(superclass) = superclass {
                let super_base = if element.is_static {
                    superclass
                } else {
                    superclass_prototype.ok_or(BytecompilerEmissionError::UnsupportedExpression(
                        "instance super method needs superclass prototype",
                    ))?
                };
                self.emit_set_function_super(method, super_base, superclass);
            }
            if element.is_static {
                self.emit_put_by_property_key(class_value, key, method);
            } else {
                self.emit_put_by_property_key(prototype, key, method);
            }
        }

        Ok(class_value)
    }

    fn emit_class_element_key(
        &mut self,
        name: ClassElementName,
    ) -> Result<EmittedPropertyKey, BytecompilerEmissionError> {
        match name {
            ClassElementName::Public(name) => Ok(EmittedPropertyKey::Name(name)),
            ClassElementName::Computed(expression) => self
                .emit_expression(expression)
                .map(EmittedPropertyKey::Value),
            ClassElementName::Private(_) => Err(BytecompilerEmissionError::UnsupportedExpression(
                "private class elements need private-name lowering",
            )),
        }
    }

    fn emit_member_get(
        &mut self,
        member: &MemberExpr,
    ) -> Result<VirtualRegister, BytecompilerEmissionError> {
        if self.expression_is_super(member.base) {
            return match member.member {
                MemberKind::Dot(name) => self.emit_get_super_by_name(name),
                MemberKind::Bracket(_) => Err(BytecompilerEmissionError::UnsupportedExpression(
                    "super indexed property access needs reference-aware lowering",
                )),
                MemberKind::PrivateDot(_) => Err(BytecompilerEmissionError::UnsupportedExpression(
                    "super private property access needs private-name lowering",
                )),
            };
        }
        let object = self.emit_expression(member.base)?;
        match member.member {
            MemberKind::Dot(name) if self.is_length_property(name) => {
                return self.emit_get_length(object, name);
            }
            MemberKind::Dot(name) => return self.emit_get_by_name(object, name),
            MemberKind::Bracket(index) => {
                let key = self.emit_expression(index)?;
                return self.emit_get_by_value(object, key);
            }
            MemberKind::PrivateDot(_) => {}
        }
        Err(BytecompilerEmissionError::UnsupportedExpression(
            "private property access needs private-name lowering",
        ))
    }

    fn emit_get_by_name(
        &mut self,
        object: VirtualRegister,
        name: ParserIdentifier,
    ) -> Result<VirtualRegister, BytecompilerEmissionError> {
        let destination = self
            .generator
            .registers_mut()
            .reserve_temporary(TemporaryLifetime::Expression);
        self.generator.instructions_mut().declare_instruction(
            CoreOpcode::GetByName.opcode(),
            OperandWidth::Narrow,
            vec![
                Operand::Register(destination),
                Operand::Register(object),
                Operand::IdentifierIndex(name.0),
            ],
        );
        Ok(destination)
    }

    fn emit_get_by_value(
        &mut self,
        object: VirtualRegister,
        key: VirtualRegister,
    ) -> Result<VirtualRegister, BytecompilerEmissionError> {
        let destination = self
            .generator
            .registers_mut()
            .reserve_temporary(TemporaryLifetime::Expression);
        self.generator.instructions_mut().declare_instruction(
            CoreOpcode::GetByValue.opcode(),
            OperandWidth::Narrow,
            vec![
                Operand::Register(destination),
                Operand::Register(object),
                Operand::Register(key),
            ],
        );
        Ok(destination)
    }

    fn emit_get_by_property_key(
        &mut self,
        object: VirtualRegister,
        key: EmittedPropertyKey,
    ) -> Result<VirtualRegister, BytecompilerEmissionError> {
        match key {
            EmittedPropertyKey::Name(name) => self.emit_get_by_name(object, name),
            EmittedPropertyKey::Value(key) => self.emit_get_by_value(object, key),
        }
    }

    fn emit_get_by_index(
        &mut self,
        object: VirtualRegister,
        index: VirtualRegister,
    ) -> Result<VirtualRegister, BytecompilerEmissionError> {
        let destination = self
            .generator
            .registers_mut()
            .reserve_temporary(TemporaryLifetime::Expression);
        self.generator.instructions_mut().declare_instruction(
            CoreOpcode::GetByIndex.opcode(),
            OperandWidth::Narrow,
            vec![
                Operand::Register(destination),
                Operand::Register(object),
                Operand::Register(index),
            ],
        );
        Ok(destination)
    }

    fn emit_get_super_by_name(
        &mut self,
        name: ParserIdentifier,
    ) -> Result<VirtualRegister, BytecompilerEmissionError> {
        let destination = self
            .generator
            .registers_mut()
            .reserve_temporary(TemporaryLifetime::Expression);
        self.generator.instructions_mut().declare_instruction(
            CoreOpcode::GetSuperByName.opcode(),
            OperandWidth::Narrow,
            vec![
                Operand::Register(destination),
                Operand::IdentifierIndex(name.0),
            ],
        );
        Ok(destination)
    }

    fn emit_get_length(
        &mut self,
        object: VirtualRegister,
        name: ParserIdentifier,
    ) -> Result<VirtualRegister, BytecompilerEmissionError> {
        let destination = self
            .generator
            .registers_mut()
            .reserve_temporary(TemporaryLifetime::Expression);
        self.generator.instructions_mut().declare_instruction(
            CoreOpcode::GetLength.opcode(),
            OperandWidth::Narrow,
            vec![
                Operand::Register(destination),
                Operand::Register(object),
                Operand::IdentifierIndex(name.0),
            ],
        );
        Ok(destination)
    }

    fn emit_array_length(
        &mut self,
        array: VirtualRegister,
    ) -> Result<VirtualRegister, BytecompilerEmissionError> {
        let destination = self
            .generator
            .registers_mut()
            .reserve_temporary(TemporaryLifetime::Expression);
        self.generator.instructions_mut().declare_instruction(
            CoreOpcode::ArrayLength.opcode(),
            OperandWidth::Narrow,
            vec![Operand::Register(destination), Operand::Register(array)],
        );
        Ok(destination)
    }

    fn emit_array_rest(
        &mut self,
        iterable: VirtualRegister,
        start_index: usize,
    ) -> Result<VirtualRegister, BytecompilerEmissionError> {
        let destination = self.emit_new_array();
        let start = self.emit_load_int32(start_index.try_into().unwrap_or(i32::MAX))?;
        self.generator.instructions_mut().declare_instruction(
            CoreOpcode::ArrayAppendRest.opcode(),
            OperandWidth::Narrow,
            vec![
                Operand::Register(destination),
                Operand::Register(iterable),
                Operand::Register(start),
            ],
        );
        Ok(destination)
    }

    fn emit_create_rest_parameter(
        &mut self,
        start_index: usize,
    ) -> Result<VirtualRegister, BytecompilerEmissionError> {
        let destination = self.emit_new_array();
        self.generator.instructions_mut().declare_instruction(
            CoreOpcode::CreateRestParameter.opcode(),
            OperandWidth::Narrow,
            vec![
                Operand::Register(destination),
                Operand::UnsignedImmediate(start_index.try_into().unwrap_or(u32::MAX)),
            ],
        );
        Ok(destination)
    }

    fn emit_create_arguments_object(&mut self) -> VirtualRegister {
        let destination = self
            .generator
            .registers_mut()
            .reserve_temporary(TemporaryLifetime::Expression);
        self.generator.instructions_mut().declare_instruction(
            CoreOpcode::CreateArgumentsObject.opcode(),
            OperandWidth::Narrow,
            vec![Operand::Register(destination)],
        );
        destination
    }

    fn emit_object_rest(
        &mut self,
        source: VirtualRegister,
        excluded_keys: VirtualRegister,
    ) -> VirtualRegister {
        let destination = self.emit_new_object();
        self.emit_copy_object_rest(destination, source, excluded_keys);
        destination
    }

    fn emit_object_literal(
        &mut self,
        object: &crate::syntax::ast::ObjectLiteralExpr,
    ) -> Result<VirtualRegister, BytecompilerEmissionError> {
        let destination = self.emit_new_object();
        for property in &object.properties {
            match property.kind {
                ObjectLiteralPropertyKind::Data => {
                    let key = self.emit_object_literal_property_key(property.key)?;
                    let value = self.emit_expression(property.value)?;
                    self.emit_put_by_property_key(destination, key, value)
                }
                ObjectLiteralPropertyKind::Getter => {
                    let key = self.emit_object_literal_property_key(property.key)?;
                    let value = self.emit_expression(property.value)?;
                    self.emit_define_getter_by_property_key(destination, key, value)
                }
                ObjectLiteralPropertyKind::Setter => {
                    let key = self.emit_object_literal_property_key(property.key)?;
                    let value = self.emit_expression(property.value)?;
                    self.emit_define_setter_by_property_key(destination, key, value)
                }
                ObjectLiteralPropertyKind::Spread => {
                    let source = self.emit_expression(property.value)?;
                    let excluded_keys = self.emit_new_array();
                    self.emit_copy_object_rest(destination, source, excluded_keys);
                }
            }
        }
        Ok(destination)
    }

    fn emit_new_object(&mut self) -> VirtualRegister {
        let destination = self
            .generator
            .registers_mut()
            .reserve_temporary(TemporaryLifetime::Expression);
        self.generator.instructions_mut().declare_instruction(
            CoreOpcode::NewObject.opcode(),
            OperandWidth::Narrow,
            vec![Operand::Register(destination)],
        );
        destination
    }

    fn emit_object_literal_property_key(
        &mut self,
        key: AstPropertyKey,
    ) -> Result<EmittedPropertyKey, BytecompilerEmissionError> {
        match key {
            AstPropertyKey::Identifier(name)
            | AstPropertyKey::String(name)
            | AstPropertyKey::Number(name) => Ok(EmittedPropertyKey::Name(name)),
            AstPropertyKey::Computed(expression) => self
                .emit_expression(expression)
                .map(EmittedPropertyKey::Value),
            AstPropertyKey::Private(_) => Err(BytecompilerEmissionError::UnsupportedExpression(
                "private object literal properties need private-name lowering",
            )),
        }
    }

    fn emit_array_literal(
        &mut self,
        array: &crate::syntax::ast::ArrayLiteralExpr,
    ) -> Result<VirtualRegister, BytecompilerEmissionError> {
        let destination = self.emit_new_array();
        for element in &array.elements {
            match element {
                ArrayLiteralElement::Expression(expression) => {
                    let value = self.emit_expression(*expression)?;
                    self.emit_array_append(destination, value);
                }
                ArrayLiteralElement::Elision(_) => {
                    let value = self.emit_load_undefined()?;
                    self.emit_array_append(destination, value);
                }
                ArrayLiteralElement::Spread { value, .. } => {
                    let iterable = self.emit_expression(*value)?;
                    self.emit_array_append_spread(destination, iterable);
                }
            }
        }
        Ok(destination)
    }

    fn emit_new_array(&mut self) -> VirtualRegister {
        let destination = self
            .generator
            .registers_mut()
            .reserve_temporary(TemporaryLifetime::Expression);
        self.generator.instructions_mut().declare_instruction(
            CoreOpcode::NewArray.opcode(),
            OperandWidth::Narrow,
            vec![Operand::Register(destination)],
        );
        destination
    }

    fn emit_put_by_name(
        &mut self,
        object: VirtualRegister,
        key: ParserIdentifier,
        value: VirtualRegister,
    ) {
        self.generator.instructions_mut().declare_instruction(
            CoreOpcode::PutByName.opcode(),
            OperandWidth::Narrow,
            vec![
                Operand::Register(object),
                Operand::IdentifierIndex(key.0),
                Operand::Register(value),
            ],
        );
    }

    fn emit_put_by_value(
        &mut self,
        object: VirtualRegister,
        key: VirtualRegister,
        value: VirtualRegister,
    ) {
        self.generator.instructions_mut().declare_instruction(
            CoreOpcode::PutByValue.opcode(),
            OperandWidth::Narrow,
            vec![
                Operand::Register(object),
                Operand::Register(key),
                Operand::Register(value),
            ],
        );
    }

    fn emit_delete_by_name(
        &mut self,
        object: VirtualRegister,
        key: ParserIdentifier,
    ) -> Result<VirtualRegister, BytecompilerEmissionError> {
        let destination = self
            .generator
            .registers_mut()
            .reserve_temporary(TemporaryLifetime::Expression);
        self.generator.instructions_mut().declare_instruction(
            CoreOpcode::DeleteByName.opcode(),
            OperandWidth::Narrow,
            vec![
                Operand::Register(destination),
                Operand::Register(object),
                Operand::IdentifierIndex(key.0),
            ],
        );
        Ok(destination)
    }

    fn emit_delete_by_value(
        &mut self,
        object: VirtualRegister,
        key: VirtualRegister,
    ) -> Result<VirtualRegister, BytecompilerEmissionError> {
        let destination = self
            .generator
            .registers_mut()
            .reserve_temporary(TemporaryLifetime::Expression);
        self.generator.instructions_mut().declare_instruction(
            CoreOpcode::DeleteByValue.opcode(),
            OperandWidth::Narrow,
            vec![
                Operand::Register(destination),
                Operand::Register(object),
                Operand::Register(key),
            ],
        );
        Ok(destination)
    }

    fn emit_put_by_property_key(
        &mut self,
        object: VirtualRegister,
        key: EmittedPropertyKey,
        value: VirtualRegister,
    ) {
        match key {
            EmittedPropertyKey::Name(name) => self.emit_put_by_name(object, name, value),
            EmittedPropertyKey::Value(key) => self.emit_put_by_value(object, key, value),
        }
    }

    fn emit_array_append(&mut self, array: VirtualRegister, value: VirtualRegister) {
        self.generator.instructions_mut().declare_instruction(
            CoreOpcode::ArrayAppend.opcode(),
            OperandWidth::Narrow,
            vec![Operand::Register(array), Operand::Register(value)],
        );
    }

    fn emit_array_append_spread(&mut self, array: VirtualRegister, iterable: VirtualRegister) {
        self.generator.instructions_mut().declare_instruction(
            CoreOpcode::ArrayAppendSpread.opcode(),
            OperandWidth::Narrow,
            vec![Operand::Register(array), Operand::Register(iterable)],
        );
    }

    fn emit_array_append_property_key(
        &mut self,
        array: VirtualRegister,
        key: EmittedPropertyKey,
    ) -> Result<(), BytecompilerEmissionError> {
        let value = match key {
            EmittedPropertyKey::Name(name) => self.emit_load_string(name)?,
            EmittedPropertyKey::Value(value) => value,
        };
        self.emit_array_append(array, value);
        Ok(())
    }

    fn emit_copy_object_rest(
        &mut self,
        target: VirtualRegister,
        source: VirtualRegister,
        excluded_keys: VirtualRegister,
    ) {
        self.generator.instructions_mut().declare_instruction(
            CoreOpcode::CopyObjectRest.opcode(),
            OperandWidth::Narrow,
            vec![
                Operand::Register(target),
                Operand::Register(source),
                Operand::Register(excluded_keys),
            ],
        );
    }

    fn emit_set_prototype(&mut self, object: VirtualRegister, prototype: VirtualRegister) {
        self.generator.instructions_mut().declare_instruction(
            CoreOpcode::SetPrototype.opcode(),
            OperandWidth::Narrow,
            vec![Operand::Register(object), Operand::Register(prototype)],
        );
    }

    fn emit_set_function_super(
        &mut self,
        function: VirtualRegister,
        super_base: VirtualRegister,
        super_constructor: VirtualRegister,
    ) {
        self.generator.instructions_mut().declare_instruction(
            CoreOpcode::SetFunctionSuper.opcode(),
            OperandWidth::Narrow,
            vec![
                Operand::Register(function),
                Operand::Register(super_base),
                Operand::Register(super_constructor),
            ],
        );
    }

    fn emit_set_default_derived_constructor(&mut self, function: VirtualRegister) {
        self.generator.instructions_mut().declare_instruction(
            CoreOpcode::SetDefaultDerivedConstructor.opcode(),
            OperandWidth::Narrow,
            vec![Operand::Register(function)],
        );
    }

    fn emit_add_instance_field(
        &mut self,
        constructor: VirtualRegister,
        key: ParserIdentifier,
        initializer: VirtualRegister,
    ) {
        self.generator.instructions_mut().declare_instruction(
            CoreOpcode::AddInstanceField.opcode(),
            OperandWidth::Narrow,
            vec![
                Operand::Register(constructor),
                Operand::IdentifierIndex(key.0),
                Operand::Register(initializer),
            ],
        );
    }

    fn emit_add_instance_field_by_value(
        &mut self,
        constructor: VirtualRegister,
        key: VirtualRegister,
        initializer: VirtualRegister,
    ) {
        self.generator.instructions_mut().declare_instruction(
            CoreOpcode::AddInstanceFieldByValue.opcode(),
            OperandWidth::Narrow,
            vec![
                Operand::Register(constructor),
                Operand::Register(key),
                Operand::Register(initializer),
            ],
        );
    }

    fn emit_add_instance_field_by_property_key(
        &mut self,
        constructor: VirtualRegister,
        key: EmittedPropertyKey,
        initializer: VirtualRegister,
    ) {
        match key {
            EmittedPropertyKey::Name(name) => {
                self.emit_add_instance_field(constructor, name, initializer)
            }
            EmittedPropertyKey::Value(key) => {
                self.emit_add_instance_field_by_value(constructor, key, initializer)
            }
        }
    }

    fn emit_define_getter(
        &mut self,
        object: VirtualRegister,
        key: ParserIdentifier,
        getter: VirtualRegister,
    ) {
        self.generator.instructions_mut().declare_instruction(
            CoreOpcode::DefineGetter.opcode(),
            OperandWidth::Narrow,
            vec![
                Operand::Register(object),
                Operand::IdentifierIndex(key.0),
                Operand::Register(getter),
            ],
        );
    }

    fn emit_define_getter_by_value(
        &mut self,
        object: VirtualRegister,
        key: VirtualRegister,
        getter: VirtualRegister,
    ) {
        self.generator.instructions_mut().declare_instruction(
            CoreOpcode::DefineGetterByValue.opcode(),
            OperandWidth::Narrow,
            vec![
                Operand::Register(object),
                Operand::Register(key),
                Operand::Register(getter),
            ],
        );
    }

    fn emit_define_getter_by_property_key(
        &mut self,
        object: VirtualRegister,
        key: EmittedPropertyKey,
        getter: VirtualRegister,
    ) {
        match key {
            EmittedPropertyKey::Name(name) => self.emit_define_getter(object, name, getter),
            EmittedPropertyKey::Value(key) => self.emit_define_getter_by_value(object, key, getter),
        }
    }

    fn emit_define_setter(
        &mut self,
        object: VirtualRegister,
        key: ParserIdentifier,
        setter: VirtualRegister,
    ) {
        self.generator.instructions_mut().declare_instruction(
            CoreOpcode::DefineSetter.opcode(),
            OperandWidth::Narrow,
            vec![
                Operand::Register(object),
                Operand::IdentifierIndex(key.0),
                Operand::Register(setter),
            ],
        );
    }

    fn emit_define_setter_by_value(
        &mut self,
        object: VirtualRegister,
        key: VirtualRegister,
        setter: VirtualRegister,
    ) {
        self.generator.instructions_mut().declare_instruction(
            CoreOpcode::DefineSetterByValue.opcode(),
            OperandWidth::Narrow,
            vec![
                Operand::Register(object),
                Operand::Register(key),
                Operand::Register(setter),
            ],
        );
    }

    fn emit_define_setter_by_property_key(
        &mut self,
        object: VirtualRegister,
        key: EmittedPropertyKey,
        setter: VirtualRegister,
    ) {
        match key {
            EmittedPropertyKey::Name(name) => self.emit_define_setter(object, name, setter),
            EmittedPropertyKey::Value(key) => self.emit_define_setter_by_value(object, key, setter),
        }
    }

    fn is_length_property(&self, name: ParserIdentifier) -> bool {
        self.identifier_text_is(name, "length")
    }

    fn expression_is_super(&self, expression: AstRef<Expr>) -> bool {
        matches!(
            self.arena.expression(expression),
            Some(Expr::Name(name)) if name.kind == NameKind::Super
        )
    }

    fn identifier_text_is(&self, name: ParserIdentifier, text: &str) -> bool {
        self.arena.identifiers().identifier_text(name) == Some(text)
    }

    fn emit_binary(
        &mut self,
        binary: &BinaryExpr,
    ) -> Result<VirtualRegister, BytecompilerEmissionError> {
        match binary.op {
            AstBinaryOperator::LogicalAnd => return self.emit_logical_and(binary),
            AstBinaryOperator::LogicalOr => return self.emit_logical_or(binary),
            AstBinaryOperator::Coalesce => return self.emit_nullish_coalesce(binary),
            _ => {}
        }
        let left = self.emit_expression(binary.left)?;
        let right = self.emit_expression(binary.right)?;
        let opcode = match binary.op {
            AstBinaryOperator::Add => CoreOpcode::AddInt32,
            AstBinaryOperator::Subtract => CoreOpcode::SubInt32,
            AstBinaryOperator::Multiply => CoreOpcode::MulInt32,
            AstBinaryOperator::Divide => CoreOpcode::DivNumber,
            AstBinaryOperator::Modulo => CoreOpcode::ModNumber,
            AstBinaryOperator::Pow => CoreOpcode::PowNumber,
            AstBinaryOperator::BitOr => CoreOpcode::BitOrInt32,
            AstBinaryOperator::BitXor => CoreOpcode::BitXorInt32,
            AstBinaryOperator::BitAnd => CoreOpcode::BitAndInt32,
            AstBinaryOperator::LeftShift => CoreOpcode::LeftShiftInt32,
            AstBinaryOperator::RightShift => CoreOpcode::RightShiftInt32,
            AstBinaryOperator::UnsignedRightShift => CoreOpcode::UnsignedRightShiftInt32,
            AstBinaryOperator::LessThan => CoreOpcode::LessThanInt32,
            AstBinaryOperator::LessEqual => CoreOpcode::LessEqualInt32,
            AstBinaryOperator::GreaterThan => CoreOpcode::GreaterThanInt32,
            AstBinaryOperator::GreaterEqual => CoreOpcode::GreaterEqualInt32,
            AstBinaryOperator::Equal => CoreOpcode::Equal,
            AstBinaryOperator::NotEqual => CoreOpcode::NotEqual,
            AstBinaryOperator::StrictEqual => CoreOpcode::StrictEqual,
            AstBinaryOperator::StrictNotEqual => CoreOpcode::StrictNotEqual,
            AstBinaryOperator::Instanceof => CoreOpcode::InstanceOf,
            _ => {
                return Err(BytecompilerEmissionError::UnsupportedExpression(
                    "binary operator is not lowered yet",
                ));
            }
        };
        let destination = self
            .generator
            .registers_mut()
            .reserve_temporary(TemporaryLifetime::Expression);
        self.generator.instructions_mut().declare_instruction(
            opcode.opcode(),
            OperandWidth::Narrow,
            vec![
                Operand::Register(destination),
                Operand::Register(left),
                Operand::Register(right),
            ],
        );
        Ok(destination)
    }

    fn emit_binary_opcode(
        &mut self,
        opcode: CoreOpcode,
        left: VirtualRegister,
        right: VirtualRegister,
    ) -> Result<VirtualRegister, BytecompilerEmissionError> {
        let destination = self
            .generator
            .registers_mut()
            .reserve_temporary(TemporaryLifetime::Expression);
        self.generator.instructions_mut().declare_instruction(
            opcode.opcode(),
            OperandWidth::Narrow,
            vec![
                Operand::Register(destination),
                Operand::Register(left),
                Operand::Register(right),
            ],
        );
        Ok(destination)
    }

    fn emit_conditional(
        &mut self,
        conditional: &ConditionalExpr,
    ) -> Result<VirtualRegister, BytecompilerEmissionError> {
        let test = self.emit_expression(conditional.test)?;
        let destination = self
            .generator
            .registers_mut()
            .reserve_temporary(TemporaryLifetime::Expression);
        let alternate_label = self.declare_label(Some("conditional_alternate"));
        let end_label = self.declare_label(Some("conditional_end"));
        self.emit_jump_if_false(test, alternate_label);
        let consequent = self.emit_expression(conditional.consequent)?;
        self.emit_move(destination, consequent);
        self.emit_jump(end_label);
        self.bind_label(alternate_label)?;
        let alternate = self.emit_expression(conditional.alternate)?;
        self.emit_move(destination, alternate);
        self.bind_label(end_label)?;
        Ok(destination)
    }

    fn emit_logical_and(
        &mut self,
        binary: &BinaryExpr,
    ) -> Result<VirtualRegister, BytecompilerEmissionError> {
        let left = self.emit_expression(binary.left)?;
        let destination = self
            .generator
            .registers_mut()
            .reserve_temporary(TemporaryLifetime::Expression);
        self.emit_move(destination, left);
        let end = self.declare_label(Some("logical_and_end"));
        self.emit_jump_if_false(left, end);
        let right = self.emit_expression(binary.right)?;
        self.emit_move(destination, right);
        self.bind_label(end)?;
        Ok(destination)
    }

    fn emit_logical_or(
        &mut self,
        binary: &BinaryExpr,
    ) -> Result<VirtualRegister, BytecompilerEmissionError> {
        let left = self.emit_expression(binary.left)?;
        let destination = self
            .generator
            .registers_mut()
            .reserve_temporary(TemporaryLifetime::Expression);
        self.emit_move(destination, left);
        let right_label = self.declare_label(Some("logical_or_right"));
        let end = self.declare_label(Some("logical_or_end"));
        self.emit_jump_if_false(left, right_label);
        self.emit_jump(end);
        self.bind_label(right_label)?;
        let right = self.emit_expression(binary.right)?;
        self.emit_move(destination, right);
        self.bind_label(end)?;
        Ok(destination)
    }

    fn emit_nullish_coalesce(
        &mut self,
        binary: &BinaryExpr,
    ) -> Result<VirtualRegister, BytecompilerEmissionError> {
        let left = self.emit_expression(binary.left)?;
        let destination = self
            .generator
            .registers_mut()
            .reserve_temporary(TemporaryLifetime::Expression);
        self.emit_move(destination, left);
        let end = self.declare_label(Some("coalesce_end"));
        self.emit_jump_if_not_nullish(left, end);
        let right = self.emit_expression(binary.right)?;
        self.emit_move(destination, right);
        self.bind_label(end)?;
        Ok(destination)
    }

    fn emit_read_binding(
        &mut self,
        binding: LocalBinding,
    ) -> Result<VirtualRegister, BytecompilerEmissionError> {
        match binding.kind {
            LocalBindingKind::Value => Ok(binding.register),
            LocalBindingKind::ClosureCell => self.emit_get_closure_cell(binding.register),
        }
    }

    fn emit_write_binding(&mut self, binding: LocalBinding, value: VirtualRegister) {
        match binding.kind {
            LocalBindingKind::Value => self.emit_move(binding.register, value),
            LocalBindingKind::ClosureCell => self.emit_put_closure_cell(binding.register, value),
        }
    }

    fn emit_load_undefined(&mut self) -> Result<VirtualRegister, BytecompilerEmissionError> {
        let destination = self
            .generator
            .registers_mut()
            .reserve_temporary(TemporaryLifetime::Expression);
        self.generator.instructions_mut().declare_instruction(
            CoreOpcode::LoadUndefined.opcode(),
            OperandWidth::Narrow,
            vec![Operand::Register(destination)],
        );
        Ok(destination)
    }

    fn emit_new_closure_cell(
        &mut self,
        value: VirtualRegister,
    ) -> Result<VirtualRegister, BytecompilerEmissionError> {
        let destination = self
            .generator
            .registers_mut()
            .reserve_temporary(TemporaryLifetime::Expression);
        self.generator.instructions_mut().declare_instruction(
            CoreOpcode::NewClosureCell.opcode(),
            OperandWidth::Narrow,
            vec![Operand::Register(destination), Operand::Register(value)],
        );
        Ok(destination)
    }

    fn emit_get_closure_cell(
        &mut self,
        cell: VirtualRegister,
    ) -> Result<VirtualRegister, BytecompilerEmissionError> {
        let destination = self
            .generator
            .registers_mut()
            .reserve_temporary(TemporaryLifetime::Expression);
        self.generator.instructions_mut().declare_instruction(
            CoreOpcode::GetClosureCell.opcode(),
            OperandWidth::Narrow,
            vec![Operand::Register(destination), Operand::Register(cell)],
        );
        Ok(destination)
    }

    fn emit_put_closure_cell(&mut self, cell: VirtualRegister, value: VirtualRegister) {
        self.generator.instructions_mut().declare_instruction(
            CoreOpcode::PutClosureCell.opcode(),
            OperandWidth::Narrow,
            vec![Operand::Register(cell), Operand::Register(value)],
        );
    }

    fn emit_load_null(&mut self) -> Result<VirtualRegister, BytecompilerEmissionError> {
        let destination = self
            .generator
            .registers_mut()
            .reserve_temporary(TemporaryLifetime::Expression);
        self.generator.instructions_mut().declare_instruction(
            CoreOpcode::LoadNull.opcode(),
            OperandWidth::Narrow,
            vec![Operand::Register(destination)],
        );
        Ok(destination)
    }

    fn emit_load_bool(
        &mut self,
        value: bool,
    ) -> Result<VirtualRegister, BytecompilerEmissionError> {
        let destination = self
            .generator
            .registers_mut()
            .reserve_temporary(TemporaryLifetime::Expression);
        self.generator.instructions_mut().declare_instruction(
            CoreOpcode::LoadBool.opcode(),
            OperandWidth::Narrow,
            vec![
                Operand::Register(destination),
                Operand::UnsignedImmediate(u32::from(value)),
            ],
        );
        Ok(destination)
    }

    fn emit_load_int32(
        &mut self,
        value: i32,
    ) -> Result<VirtualRegister, BytecompilerEmissionError> {
        let destination = self
            .generator
            .registers_mut()
            .reserve_temporary(TemporaryLifetime::Expression);
        self.generator.instructions_mut().declare_instruction(
            CoreOpcode::LoadInt32.opcode(),
            OperandWidth::Narrow,
            vec![
                Operand::Register(destination),
                Operand::SignedImmediate(value),
            ],
        );
        Ok(destination)
    }

    fn emit_load_double(
        &mut self,
        bits: u64,
    ) -> Result<VirtualRegister, BytecompilerEmissionError> {
        let destination = self
            .generator
            .registers_mut()
            .reserve_temporary(TemporaryLifetime::Expression);
        self.generator.instructions_mut().declare_instruction(
            CoreOpcode::LoadDouble.opcode(),
            OperandWidth::Narrow,
            vec![
                Operand::Register(destination),
                Operand::UnsignedImmediate((bits & u64::from(u32::MAX)) as u32),
                Operand::UnsignedImmediate((bits >> 32) as u32),
            ],
        );
        Ok(destination)
    }

    fn emit_load_string(
        &mut self,
        text: ParserIdentifier,
    ) -> Result<VirtualRegister, BytecompilerEmissionError> {
        let destination = self
            .generator
            .registers_mut()
            .reserve_temporary(TemporaryLifetime::Expression);
        self.generator.instructions_mut().declare_instruction(
            CoreOpcode::LoadString.opcode(),
            OperandWidth::Narrow,
            vec![
                Operand::Register(destination),
                Operand::IdentifierIndex(text.0),
            ],
        );
        Ok(destination)
    }

    fn emit_load_bigint(
        &mut self,
        text: ParserIdentifier,
    ) -> Result<VirtualRegister, BytecompilerEmissionError> {
        let destination = self
            .generator
            .registers_mut()
            .reserve_temporary(TemporaryLifetime::Expression);
        self.generator.instructions_mut().declare_instruction(
            CoreOpcode::LoadBigInt.opcode(),
            OperandWidth::Narrow,
            vec![
                Operand::Register(destination),
                Operand::IdentifierIndex(text.0),
            ],
        );
        Ok(destination)
    }

    fn emit_new_regexp(
        &mut self,
        pattern: ParserIdentifier,
        flags: ParserIdentifier,
    ) -> Result<VirtualRegister, BytecompilerEmissionError> {
        let destination = self
            .generator
            .registers_mut()
            .reserve_temporary(TemporaryLifetime::Expression);
        self.generator.instructions_mut().declare_instruction(
            CoreOpcode::NewRegExp.opcode(),
            OperandWidth::Narrow,
            vec![
                Operand::Register(destination),
                Operand::IdentifierIndex(pattern.0),
                Operand::IdentifierIndex(flags.0),
            ],
        );
        Ok(destination)
    }

    fn emit_load_function(
        &mut self,
        function_index: u32,
        captures: &[ParserIdentifier],
    ) -> Result<VirtualRegister, BytecompilerEmissionError> {
        let destination = self
            .generator
            .registers_mut()
            .reserve_temporary(TemporaryLifetime::Expression);
        let mut operands = vec![
            Operand::Register(destination),
            Operand::UnsignedImmediate(function_index),
            Operand::UnsignedImmediate(captures.len().try_into().unwrap_or(u32::MAX)),
        ];
        for capture in captures {
            let binding = *self
                .locals
                .get(capture)
                .ok_or(BytecompilerEmissionError::UnboundIdentifier(*capture))?;
            if binding.kind != LocalBindingKind::ClosureCell {
                return Err(BytecompilerEmissionError::UnsupportedExpression(
                    "captured binding was not lowered to a closure cell",
                ));
            }
            operands.push(Operand::Register(binding.register));
        }
        self.generator.instructions_mut().declare_instruction(
            CoreOpcode::LoadFunction.opcode(),
            OperandWidth::Narrow,
            operands,
        );
        Ok(destination)
    }

    fn emit_load_capture(
        &mut self,
        capture_index: u32,
    ) -> Result<VirtualRegister, BytecompilerEmissionError> {
        let destination = self
            .generator
            .registers_mut()
            .reserve_temporary(TemporaryLifetime::Expression);
        self.generator.instructions_mut().declare_instruction(
            CoreOpcode::LoadCapture.opcode(),
            OperandWidth::Narrow,
            vec![
                Operand::Register(destination),
                Operand::UnsignedImmediate(capture_index),
            ],
        );
        Ok(destination)
    }

    fn emit_load_object_constructor(
        &mut self,
    ) -> Result<VirtualRegister, BytecompilerEmissionError> {
        let destination = self
            .generator
            .registers_mut()
            .reserve_temporary(TemporaryLifetime::Expression);
        self.generator.instructions_mut().declare_instruction(
            CoreOpcode::LoadObjectConstructor.opcode(),
            OperandWidth::Narrow,
            vec![Operand::Register(destination)],
        );
        Ok(destination)
    }

    fn emit_load_array_constructor(
        &mut self,
    ) -> Result<VirtualRegister, BytecompilerEmissionError> {
        let destination = self
            .generator
            .registers_mut()
            .reserve_temporary(TemporaryLifetime::Expression);
        self.generator.instructions_mut().declare_instruction(
            CoreOpcode::LoadArrayConstructor.opcode(),
            OperandWidth::Narrow,
            vec![Operand::Register(destination)],
        );
        Ok(destination)
    }

    fn emit_load_math_object(&mut self) -> Result<VirtualRegister, BytecompilerEmissionError> {
        let destination = self
            .generator
            .registers_mut()
            .reserve_temporary(TemporaryLifetime::Expression);
        self.generator.instructions_mut().declare_instruction(
            CoreOpcode::LoadMathObject.opcode(),
            OperandWidth::Narrow,
            vec![Operand::Register(destination)],
        );
        Ok(destination)
    }

    fn emit_load_json_object(&mut self) -> Result<VirtualRegister, BytecompilerEmissionError> {
        let destination = self
            .generator
            .registers_mut()
            .reserve_temporary(TemporaryLifetime::Expression);
        self.generator.instructions_mut().declare_instruction(
            CoreOpcode::LoadJsonObject.opcode(),
            OperandWidth::Narrow,
            vec![Operand::Register(destination)],
        );
        Ok(destination)
    }

    fn emit_load_reflect_object(&mut self) -> Result<VirtualRegister, BytecompilerEmissionError> {
        let destination = self
            .generator
            .registers_mut()
            .reserve_temporary(TemporaryLifetime::Expression);
        self.generator.instructions_mut().declare_instruction(
            CoreOpcode::LoadReflectObject.opcode(),
            OperandWidth::Narrow,
            vec![Operand::Register(destination)],
        );
        Ok(destination)
    }

    fn emit_load_string_constructor(
        &mut self,
    ) -> Result<VirtualRegister, BytecompilerEmissionError> {
        let destination = self
            .generator
            .registers_mut()
            .reserve_temporary(TemporaryLifetime::Expression);
        self.generator.instructions_mut().declare_instruction(
            CoreOpcode::LoadStringConstructor.opcode(),
            OperandWidth::Narrow,
            vec![Operand::Register(destination)],
        );
        Ok(destination)
    }

    fn emit_load_number_constructor(
        &mut self,
    ) -> Result<VirtualRegister, BytecompilerEmissionError> {
        let destination = self
            .generator
            .registers_mut()
            .reserve_temporary(TemporaryLifetime::Expression);
        self.generator.instructions_mut().declare_instruction(
            CoreOpcode::LoadNumberConstructor.opcode(),
            OperandWidth::Narrow,
            vec![Operand::Register(destination)],
        );
        Ok(destination)
    }

    fn emit_load_boolean_constructor(
        &mut self,
    ) -> Result<VirtualRegister, BytecompilerEmissionError> {
        let destination = self
            .generator
            .registers_mut()
            .reserve_temporary(TemporaryLifetime::Expression);
        self.generator.instructions_mut().declare_instruction(
            CoreOpcode::LoadBooleanConstructor.opcode(),
            OperandWidth::Narrow,
            vec![Operand::Register(destination)],
        );
        Ok(destination)
    }

    fn emit_load_error_constructor(
        &mut self,
    ) -> Result<VirtualRegister, BytecompilerEmissionError> {
        let destination = self
            .generator
            .registers_mut()
            .reserve_temporary(TemporaryLifetime::Expression);
        self.generator.instructions_mut().declare_instruction(
            CoreOpcode::LoadErrorConstructor.opcode(),
            OperandWidth::Narrow,
            vec![Operand::Register(destination)],
        );
        Ok(destination)
    }

    fn emit_load_type_error_constructor(
        &mut self,
    ) -> Result<VirtualRegister, BytecompilerEmissionError> {
        let destination = self
            .generator
            .registers_mut()
            .reserve_temporary(TemporaryLifetime::Expression);
        self.generator.instructions_mut().declare_instruction(
            CoreOpcode::LoadTypeErrorConstructor.opcode(),
            OperandWidth::Narrow,
            vec![Operand::Register(destination)],
        );
        Ok(destination)
    }

    fn emit_load_map_constructor(&mut self) -> Result<VirtualRegister, BytecompilerEmissionError> {
        let destination = self
            .generator
            .registers_mut()
            .reserve_temporary(TemporaryLifetime::Expression);
        self.generator.instructions_mut().declare_instruction(
            CoreOpcode::LoadMapConstructor.opcode(),
            OperandWidth::Narrow,
            vec![Operand::Register(destination)],
        );
        Ok(destination)
    }

    fn emit_load_set_constructor(&mut self) -> Result<VirtualRegister, BytecompilerEmissionError> {
        let destination = self
            .generator
            .registers_mut()
            .reserve_temporary(TemporaryLifetime::Expression);
        self.generator.instructions_mut().declare_instruction(
            CoreOpcode::LoadSetConstructor.opcode(),
            OperandWidth::Narrow,
            vec![Operand::Register(destination)],
        );
        Ok(destination)
    }

    fn emit_load_weak_map_constructor(
        &mut self,
    ) -> Result<VirtualRegister, BytecompilerEmissionError> {
        let destination = self
            .generator
            .registers_mut()
            .reserve_temporary(TemporaryLifetime::Expression);
        self.generator.instructions_mut().declare_instruction(
            CoreOpcode::LoadWeakMapConstructor.opcode(),
            OperandWidth::Narrow,
            vec![Operand::Register(destination)],
        );
        Ok(destination)
    }

    fn emit_load_weak_set_constructor(
        &mut self,
    ) -> Result<VirtualRegister, BytecompilerEmissionError> {
        let destination = self
            .generator
            .registers_mut()
            .reserve_temporary(TemporaryLifetime::Expression);
        self.generator.instructions_mut().declare_instruction(
            CoreOpcode::LoadWeakSetConstructor.opcode(),
            OperandWidth::Narrow,
            vec![Operand::Register(destination)],
        );
        Ok(destination)
    }

    fn emit_load_regexp_constructor(
        &mut self,
    ) -> Result<VirtualRegister, BytecompilerEmissionError> {
        let destination = self
            .generator
            .registers_mut()
            .reserve_temporary(TemporaryLifetime::Expression);
        self.generator.instructions_mut().declare_instruction(
            CoreOpcode::LoadRegExpConstructor.opcode(),
            OperandWidth::Narrow,
            vec![Operand::Register(destination)],
        );
        Ok(destination)
    }

    fn emit_load_promise_constructor(
        &mut self,
    ) -> Result<VirtualRegister, BytecompilerEmissionError> {
        let destination = self
            .generator
            .registers_mut()
            .reserve_temporary(TemporaryLifetime::Expression);
        self.generator.instructions_mut().declare_instruction(
            CoreOpcode::LoadPromiseConstructor.opcode(),
            OperandWidth::Narrow,
            vec![Operand::Register(destination)],
        );
        Ok(destination)
    }

    fn emit_load_date_constructor(&mut self) -> Result<VirtualRegister, BytecompilerEmissionError> {
        let destination = self
            .generator
            .registers_mut()
            .reserve_temporary(TemporaryLifetime::Expression);
        self.generator.instructions_mut().declare_instruction(
            CoreOpcode::LoadDateConstructor.opcode(),
            OperandWidth::Narrow,
            vec![Operand::Register(destination)],
        );
        Ok(destination)
    }

    fn emit_load_bigint_constructor(
        &mut self,
    ) -> Result<VirtualRegister, BytecompilerEmissionError> {
        let destination = self
            .generator
            .registers_mut()
            .reserve_temporary(TemporaryLifetime::Expression);
        self.generator.instructions_mut().declare_instruction(
            CoreOpcode::LoadBigIntConstructor.opcode(),
            OperandWidth::Narrow,
            vec![Operand::Register(destination)],
        );
        Ok(destination)
    }

    fn emit_load_array_buffer_constructor(
        &mut self,
    ) -> Result<VirtualRegister, BytecompilerEmissionError> {
        let destination = self
            .generator
            .registers_mut()
            .reserve_temporary(TemporaryLifetime::Expression);
        self.generator.instructions_mut().declare_instruction(
            CoreOpcode::LoadArrayBufferConstructor.opcode(),
            OperandWidth::Narrow,
            vec![Operand::Register(destination)],
        );
        Ok(destination)
    }

    fn emit_load_uint8_array_constructor(
        &mut self,
    ) -> Result<VirtualRegister, BytecompilerEmissionError> {
        let destination = self
            .generator
            .registers_mut()
            .reserve_temporary(TemporaryLifetime::Expression);
        self.generator.instructions_mut().declare_instruction(
            CoreOpcode::LoadUint8ArrayConstructor.opcode(),
            OperandWidth::Narrow,
            vec![Operand::Register(destination)],
        );
        Ok(destination)
    }

    fn emit_load_data_view_constructor(
        &mut self,
    ) -> Result<VirtualRegister, BytecompilerEmissionError> {
        let destination = self
            .generator
            .registers_mut()
            .reserve_temporary(TemporaryLifetime::Expression);
        self.generator.instructions_mut().declare_instruction(
            CoreOpcode::LoadDataViewConstructor.opcode(),
            OperandWidth::Narrow,
            vec![Operand::Register(destination)],
        );
        Ok(destination)
    }

    fn emit_load_proxy_constructor(
        &mut self,
    ) -> Result<VirtualRegister, BytecompilerEmissionError> {
        let destination = self
            .generator
            .registers_mut()
            .reserve_temporary(TemporaryLifetime::Expression);
        self.generator.instructions_mut().declare_instruction(
            CoreOpcode::LoadProxyConstructor.opcode(),
            OperandWidth::Narrow,
            vec![Operand::Register(destination)],
        );
        Ok(destination)
    }

    fn emit_load_symbol_constructor(
        &mut self,
    ) -> Result<VirtualRegister, BytecompilerEmissionError> {
        let destination = self
            .generator
            .registers_mut()
            .reserve_temporary(TemporaryLifetime::Expression);
        self.generator.instructions_mut().declare_instruction(
            CoreOpcode::LoadSymbolConstructor.opcode(),
            OperandWidth::Narrow,
            vec![Operand::Register(destination)],
        );
        Ok(destination)
    }

    fn emit_move(&mut self, destination: VirtualRegister, source: VirtualRegister) {
        self.generator.instructions_mut().declare_instruction(
            CoreOpcode::Move.opcode(),
            OperandWidth::Narrow,
            vec![Operand::Register(destination), Operand::Register(source)],
        );
    }

    fn emit_move_temporary(&mut self, source: VirtualRegister) -> VirtualRegister {
        let destination = self
            .generator
            .registers_mut()
            .reserve_temporary(TemporaryLifetime::Expression);
        self.emit_move(destination, source);
        destination
    }

    fn declare_label(&mut self, name: Option<&'static str>) -> LabelRef {
        self.generator.instructions_mut().declare_label(name)
    }

    fn current_bytecode_index(&mut self) -> BytecodeIndex {
        BytecodeIndex::from_offset(
            self.generator
                .instructions_mut()
                .declarations()
                .len()
                .try_into()
                .unwrap_or(u32::MAX),
        )
    }

    fn bind_label(&mut self, label: LabelRef) -> Result<(), BytecompilerEmissionError> {
        let bytecode_index = self.current_bytecode_index();
        if self
            .generator
            .instructions_mut()
            .bind_label(label, bytecode_index)
        {
            Ok(())
        } else {
            Err(BytecompilerEmissionError::UnsupportedStatement(
                "invalid bytecode label binding",
            ))
        }
    }

    fn push_finally_context(&mut self, body: AstRef<Stmt>, applies_to_throw: bool) {
        self.finally_stack.push(ActiveFinallyContext {
            body,
            applies_to_throw,
            exits: Vec::new(),
        });
    }

    fn pop_finally_context(&mut self) -> Result<ActiveFinallyContext, BytecompilerEmissionError> {
        self.finally_stack
            .pop()
            .ok_or(BytecompilerEmissionError::UnsupportedStatement(
                "finally control stack underflow",
            ))
    }

    fn emit_finally_body(
        &mut self,
        body: AstRef<Stmt>,
    ) -> Result<StatementEmission, BytecompilerEmissionError> {
        self.emit_statement(body)
    }

    fn emit_finally_exit_blocks(
        &mut self,
        body: AstRef<Stmt>,
        exits: Vec<FinallyExit>,
    ) -> Result<(), BytecompilerEmissionError> {
        for exit in exits {
            self.bind_label(exit.label)?;
            let finalizer = self.emit_finally_body(body)?;
            if !finalizer.terminated {
                self.emit_finally_exit_completion(exit.kind)?;
            }
        }
        Ok(())
    }

    fn emit_finally_exit_completion(
        &mut self,
        kind: FinallyExitKind,
    ) -> Result<(), BytecompilerEmissionError> {
        match kind {
            FinallyExitKind::Return(value) => self.emit_return_completion(value),
            FinallyExitKind::Throw(value) => self.emit_throw_completion(value),
            FinallyExitKind::Break {
                target,
                target_depth,
            } => self.emit_break_to(target, target_depth),
            FinallyExitKind::Continue {
                target,
                target_depth,
            } => self.emit_continue_to(target, target_depth),
        }
    }

    fn emit_return_completion(
        &mut self,
        value: VirtualRegister,
    ) -> Result<(), BytecompilerEmissionError> {
        if self.finally_stack.is_empty() {
            self.emit_return(value);
        } else {
            let context_index = self.finally_stack.len() - 1;
            self.emit_finally_exit(context_index, FinallyExitKind::Return(value));
        }
        Ok(())
    }

    fn emit_throw_completion(
        &mut self,
        value: VirtualRegister,
    ) -> Result<(), BytecompilerEmissionError> {
        if let Some(context) = self.finally_stack.last() {
            if context.applies_to_throw {
                let context_index = self.finally_stack.len() - 1;
                self.emit_finally_exit(context_index, FinallyExitKind::Throw(value));
                return Ok(());
            }
        }
        self.emit_throw(value);
        Ok(())
    }

    fn emit_break_completion(
        &mut self,
        labels: LoopControlLabels,
    ) -> Result<(), BytecompilerEmissionError> {
        self.emit_break_to(labels.break_target, labels.finally_stack_depth)
    }

    fn emit_continue_completion(
        &mut self,
        labels: LoopControlLabels,
    ) -> Result<(), BytecompilerEmissionError> {
        self.emit_continue_to(labels.continue_target, labels.finally_stack_depth)
    }

    fn emit_break_to(
        &mut self,
        target: LabelRef,
        target_depth: usize,
    ) -> Result<(), BytecompilerEmissionError> {
        if self.finally_stack.len() > target_depth {
            let context_index = self.finally_stack.len() - 1;
            self.emit_finally_exit(
                context_index,
                FinallyExitKind::Break {
                    target,
                    target_depth,
                },
            );
        } else {
            self.emit_jump(target);
        }
        Ok(())
    }

    fn emit_continue_to(
        &mut self,
        target: LabelRef,
        target_depth: usize,
    ) -> Result<(), BytecompilerEmissionError> {
        if self.finally_stack.len() > target_depth {
            let context_index = self.finally_stack.len() - 1;
            self.emit_finally_exit(
                context_index,
                FinallyExitKind::Continue {
                    target,
                    target_depth,
                },
            );
        } else {
            self.emit_jump(target);
        }
        Ok(())
    }

    fn emit_finally_exit(&mut self, context_index: usize, kind: FinallyExitKind) {
        let label = self.declare_label(Some("finally_exit"));
        if let Some(context) = self.finally_stack.get_mut(context_index) {
            context.exits.push(FinallyExit { label, kind });
        }
        self.emit_jump(label);
    }

    fn record_handler(
        &mut self,
        start: BytecodeIndex,
        end: BytecodeIndex,
        target: BytecodeIndex,
        kind: HandlerKind,
    ) {
        self.generator
            .side_tables_mut()
            .handlers
            .push(UnlinkedHandlerInfo {
                range: BytecodeRange { start, end },
                target,
                kind,
            });
    }

    fn emit_jump(&mut self, target: LabelRef) {
        self.generator.instructions_mut().declare_instruction(
            CoreOpcode::Jump.opcode(),
            OperandWidth::Narrow,
            vec![Operand::Label(target)],
        );
    }

    fn emit_jump_if_false(&mut self, condition: VirtualRegister, target: LabelRef) {
        self.generator.instructions_mut().declare_instruction(
            CoreOpcode::JumpIfFalse.opcode(),
            OperandWidth::Narrow,
            vec![Operand::Register(condition), Operand::Label(target)],
        );
    }

    fn emit_jump_if_not_nullish(&mut self, value: VirtualRegister, target: LabelRef) {
        self.generator.instructions_mut().declare_instruction(
            CoreOpcode::JumpIfNotNullish.opcode(),
            OperandWidth::Narrow,
            vec![Operand::Register(value), Operand::Label(target)],
        );
    }

    fn emit_return(&mut self, source: VirtualRegister) {
        self.generator.instructions_mut().declare_instruction(
            CoreOpcode::Return.opcode(),
            OperandWidth::Narrow,
            vec![Operand::Register(source)],
        );
    }

    fn emit_throw(&mut self, source: VirtualRegister) {
        self.generator.instructions_mut().declare_instruction(
            CoreOpcode::Throw.opcode(),
            OperandWidth::Narrow,
            vec![Operand::Register(source)],
        );
    }

    fn emit_take_exception(&mut self) -> Result<VirtualRegister, BytecompilerEmissionError> {
        let destination = self
            .generator
            .registers_mut()
            .reserve_temporary(TemporaryLifetime::Expression);
        self.generator.instructions_mut().declare_instruction(
            CoreOpcode::TakeException.opcode(),
            OperandWidth::Narrow,
            vec![Operand::Register(destination)],
        );
        Ok(destination)
    }
}

fn synthesize_parse_semantics(input: &BytecompilerInput) -> ParseSemanticMetadata {
    let goal = match input.mode {
        BytecompilerMode::Program | BytecompilerMode::Builtin => ParseSemanticGoal::Script,
        BytecompilerMode::Function => ParseSemanticGoal::Function,
        BytecompilerMode::Eval => ParseSemanticGoal::Eval,
        BytecompilerMode::Module => ParseSemanticGoal::Module,
    };
    ParseSemanticMetadata {
        goal,
        strictness: if matches!(input.mode, BytecompilerMode::Module) {
            SemanticStrictness::Strict
        } else {
            SemanticStrictness::Sloppy
        },
        module: input.module_analysis.as_ref().map(module_parse_metadata),
        features: input.code_features,
        ..ParseSemanticMetadata::default()
    }
}

fn module_parse_metadata(module: &ModuleAnalysis) -> ModuleParseSemanticMetadata {
    ModuleParseSemanticMetadata {
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
        ..ModuleParseSemanticMetadata::default()
    }
}

fn map_code_features(features: CodeFeatures) -> BytecodeCodeFeatures {
    BytecodeCodeFeatures {
        uses_arguments: features.arguments,
        uses_eval: features.eval,
        uses_import_meta: features.import_meta,
        has_captured_variables: false,
        has_tail_calls: features.tail_call_candidate,
        has_checkpoints: false,
        no_eval_cache: features.eval,
        has_non_simple_parameters: features.non_simple_parameters,
    }
}

#[derive(Clone, Debug, Default, Eq, PartialEq)]
pub struct StaticPropertyAnalysisBuilder {
    accesses: Vec<StaticPropertyAccess>,
    creates_structure_literals: bool,
    needs_computed_name_temporaries: bool,
}

impl StaticPropertyAnalysisBuilder {
    pub fn new() -> Self {
        Self::default()
    }

    pub fn record_member_expr(&mut self, expression: AstRef<Expr>, member: &MemberExpr) {
        let kind = match member.member {
            MemberKind::Dot(_) => StaticPropertyAccessKind::DirectName,
            MemberKind::PrivateDot(_) => StaticPropertyAccessKind::PrivateName,
            MemberKind::Bracket(_) => {
                self.needs_computed_name_temporaries = true;
                StaticPropertyAccessKind::Computed
            }
        };
        self.accesses.push(StaticPropertyAccess {
            expression,
            kind,
            cacheable_without_side_effect: matches!(
                kind,
                StaticPropertyAccessKind::DirectName | StaticPropertyAccessKind::PrivateName
            ),
            requires_private_brand_check: kind == StaticPropertyAccessKind::PrivateName,
        });
    }

    pub fn record_numeric_index(&mut self, expression: AstRef<Expr>) {
        self.accesses.push(StaticPropertyAccess {
            expression,
            kind: StaticPropertyAccessKind::NumericIndex,
            cacheable_without_side_effect: true,
            requires_private_brand_check: false,
        });
    }

    pub fn record_spread(&mut self, expression: AstRef<Expr>) {
        self.accesses.push(StaticPropertyAccess {
            expression,
            kind: StaticPropertyAccessKind::Spread,
            cacheable_without_side_effect: false,
            requires_private_brand_check: false,
        });
    }

    pub fn note_structure_literal(&mut self) {
        self.creates_structure_literals = true;
    }

    pub fn finish(self) -> StaticPropertyAnalysisPlan {
        StaticPropertyAnalysisPlan {
            accesses: self.accesses,
            creates_structure_literals: self.creates_structure_literals,
            needs_computed_name_temporaries: self.needs_computed_name_temporaries,
        }
    }
}

fn validate_unlinked_code_presence(
    phase: BytecompilerPhase,
    unlinked_code: Option<&UnlinkedCodeBlock>,
    findings: &mut Vec<BytecompilerValidationFinding>,
) {
    if phase == BytecompilerPhase::UnlinkedCodeBlockAssembly && unlinked_code.is_none() {
        findings.push(BytecompilerValidationFinding::MissingUnlinkedCodeBlock { phase });
    }
    if phase != BytecompilerPhase::UnlinkedCodeBlockAssembly && unlinked_code.is_some() {
        findings.push(BytecompilerValidationFinding::UnexpectedUnlinkedCodeBlock { phase });
    }
}

fn validate_finally_contexts(
    contexts: &[FinallyContextPlan],
    findings: &mut Vec<BytecompilerValidationFinding>,
) {
    for (index, context) in contexts.iter().enumerate() {
        if let Some(outer) = context.outer {
            if usize::try_from(outer)
                .ok()
                .is_none_or(|outer| outer >= index)
            {
                findings.push(BytecompilerValidationFinding::InvalidFinallyOuter {
                    context: index as u32,
                    outer,
                });
            }
        }
        if context.handles_returns
            && (context.completion_type_register.is_none()
                || context.completion_value_register.is_none())
        {
            findings.push(
                BytecompilerValidationFinding::FinallyReturnRegistersMissing {
                    context: index as u32,
                },
            );
        }
    }
}

fn validate_lexical_scopes(
    scopes: &LexicalScopeStackPlan,
    findings: &mut Vec<BytecompilerValidationFinding>,
) {
    if let Some(var_scope_index) = scopes.var_scope_index {
        if usize::try_from(var_scope_index)
            .ok()
            .is_none_or(|index| index >= scopes.entries.len())
        {
            findings.push(BytecompilerValidationFinding::InvalidVarScopeIndex { var_scope_index });
        }
    }
    if scopes.local_scope_count < scopes.entries.len() as u32 {
        findings.push(BytecompilerValidationFinding::LocalScopeCountTooSmall {
            declared: scopes.local_scope_count,
            actual: scopes.entries.len() as u32,
        });
    }
}

fn validate_for_in_contexts(
    contexts: &[ForInContextPlan],
    findings: &mut Vec<BytecompilerValidationFinding>,
) {
    for (index, context) in contexts.iter().enumerate() {
        if context.state == ForInContextState::Finalized
            && (context.local.is_none()
                || context.property_name.is_none()
                || context.property_offset.is_none()
                || context.enumerator.is_none()
                || context.mode.is_none()
                || context.body_start.is_none_or(|start| !start.is_valid()))
        {
            findings.push(
                BytecompilerValidationFinding::IncompleteFinalizedForInContext {
                    context: index as u32,
                },
            );
        }
    }
}

fn validate_using_scopes(
    scopes: &[UsingScopePlan],
    findings: &mut Vec<BytecompilerValidationFinding>,
) {
    for (index, scope) in scopes.iter().enumerate() {
        if scope.next_slot != scope.slots.len() as u32 {
            findings.push(BytecompilerValidationFinding::UsingScopeSlotCountMismatch {
                scope: index as u32,
                next_slot: scope.next_slot,
                actual: scope.slots.len() as u32,
            });
        }
    }
}

fn validate_try_contexts(
    contexts: &TryContextPlan,
    findings: &mut Vec<BytecompilerValidationFinding>,
) {
    for (index, range) in contexts.ranges.iter().enumerate() {
        if range.start == range.end {
            findings.push(BytecompilerValidationFinding::EmptyTryRange {
                range: index as u32,
            });
        }
    }
}

fn validate_semantic_plan(
    semantic: &BytecompilerSemanticPlan,
    generation: &GenerationPlan,
    findings: &mut Vec<BytecompilerValidationFinding>,
) {
    if semantic.parse.goal == ParseSemanticGoal::Module
        && semantic.parse.strictness != SemanticStrictness::Strict
    {
        findings.push(BytecompilerValidationFinding::ModuleSemanticPlanNotStrict);
    }
    if generation.environment.strict_mode
        != (semantic.parse.strictness == SemanticStrictness::Strict)
    {
        findings.push(BytecompilerValidationFinding::StrictModeHandoffMismatch {
            generation: generation.environment.strict_mode,
            semantic: semantic.parse.strictness,
        });
    }
    if semantic
        .environments
        .iter()
        .any(|environment| environment.flags.contains_private_names)
        && !semantic
            .environments
            .iter()
            .any(|environment| environment.private_name_count > 0)
    {
        findings.push(BytecompilerValidationFinding::PrivateEnvironmentCountMismatch);
    }
}

#[derive(Clone, Debug, Eq, PartialEq)]
pub struct BytecompilerIntegrationSummary {
    pub phase: BytecompilerPhase,
    pub code_kind: CodeKind,
    pub parse_mode: BytecodeParseMode,
    pub generation_root: GenerationRoot,
    pub source_length: u32,
    pub label_scope_count: u32,
    pub lexical_scope_count: u32,
    pub try_range_count: u32,
    pub profile_flag_count: u32,
    pub has_unlinked_code: bool,
    pub semantic_environment_count: u32,
    pub module_semantics_present: bool,
    pub validation: BytecompilerValidationReport,
}

impl BytecompilerIntegrationSummary {
    pub fn is_ready_for_bytecode_handoff(&self) -> bool {
        self.validation.is_valid()
            && matches!(
                self.phase,
                BytecompilerPhase::BytecodeEmission | BytecompilerPhase::UnlinkedCodeBlockAssembly
            )
            && self.has_unlinked_code
    }
}

pub fn summarize_bytecompiler_integration(
    plan: &BytecompilerOutputPlan,
) -> BytecompilerIntegrationSummary {
    BytecompilerIntegrationSummary {
        phase: plan.phase,
        code_kind: plan.generation.kind,
        parse_mode: plan.generation.parse_mode,
        generation_root: plan.generation.root,
        source_length: plan.generation.source.source_length,
        label_scope_count: plan.labels.len() as u32,
        lexical_scope_count: plan.lexical_scopes.entries.len() as u32,
        try_range_count: plan.try_contexts.ranges.len() as u32,
        profile_flag_count: plan.profile_flags.len() as u32,
        has_unlinked_code: plan.unlinked_code.is_some(),
        semantic_environment_count: plan.semantic.environments.len() as u32,
        module_semantics_present: plan.semantic.module.is_some(),
        validation: plan.validate(),
    }
}

#[derive(Clone, Debug, Default, Eq, PartialEq)]
pub struct BytecompilerValidationReport {
    pub findings: Vec<BytecompilerValidationFinding>,
}

impl BytecompilerValidationReport {
    pub fn is_valid(&self) -> bool {
        self.findings.is_empty()
    }
}

#[derive(Clone, Debug, Eq, PartialEq)]
pub enum BytecompilerValidationFinding {
    GenerationPlan {
        finding: GenerationValidationFinding,
    },
    MissingUnlinkedCodeBlock {
        phase: BytecompilerPhase,
    },
    UnexpectedUnlinkedCodeBlock {
        phase: BytecompilerPhase,
    },
    InvalidFinallyOuter {
        context: u32,
        outer: u32,
    },
    FinallyReturnRegistersMissing {
        context: u32,
    },
    InvalidVarScopeIndex {
        var_scope_index: u32,
    },
    LocalScopeCountTooSmall {
        declared: u32,
        actual: u32,
    },
    IncompleteFinalizedForInContext {
        context: u32,
    },
    UsingScopeSlotCountMismatch {
        scope: u32,
        next_slot: u32,
        actual: u32,
    },
    EmptyTryRange {
        range: u32,
    },
    ModuleSemanticPlanNotStrict,
    StrictModeHandoffMismatch {
        generation: bool,
        semantic: SemanticStrictness,
    },
    PrivateEnvironmentCountMismatch,
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::bytecode::{
        BytecodeRootMap, BytecodeRootSlotKind, BytecodeRootSlotStorage, CodeBlock, CodeKind,
        LinkContext, ParseMode, RegisterFrameShape, SpecialRegisters, VirtualRegister,
    };
    use crate::gc::{CellId, Heap};
    use crate::interpreter::{
        execute_code_block, CoreOpcodeDispatchHost, DispatchConfig, ExecutionCompletion,
        ExecutionContextStack, ExecutionEntryRecord, FramePushRequest, InterpreterExecutionState,
        ProgramExecutionEntry, RegisterFile,
    };
    use crate::runtime::{CodeBlockId, GlobalObjectId, ObjectId, RuntimeValue};
    use crate::syntax::ast::{MemberExpr, MemberKind};
    use crate::syntax::source::{
        SourceOrigin, SourcePosition, SourceProvider, SourceSpan, SourceText,
    };
    use crate::syntax::{AstBuilder, Parser, ParserArena};
    use std::sync::Arc;

    fn valid_generation_plan() -> GenerationPlan {
        let mut plan = GenerationPlan::new(CodeKind::Program, ParseMode::Program);
        plan.registers = RegisterFrameShape {
            special: SpecialRegisters {
                this_register: VirtualRegister::argument_or_header(0),
                scope_register: VirtualRegister::local(0),
                ..SpecialRegisters::default()
            },
            ..RegisterFrameShape::default()
        };
        plan
    }

    fn source(text: &str) -> SourceCode {
        let provider = Arc::new(SourceProvider::new(
            SourceOrigin::default(),
            SourceText::Latin1(text.as_bytes().to_vec()),
        ));
        SourceCode::new(
            provider,
            SourceSpan::new(SourcePosition(0), SourcePosition(text.len() as u32)),
        )
    }

    fn load_string_identifier_keys(code_block: &UnlinkedCodeBlock) -> Vec<u32> {
        literal_identifier_keys(code_block, CoreOpcode::LoadString)
    }

    fn load_bigint_identifier_keys(code_block: &UnlinkedCodeBlock) -> Vec<u32> {
        literal_identifier_keys(code_block, CoreOpcode::LoadBigInt)
    }

    fn literal_identifier_keys(code_block: &UnlinkedCodeBlock, opcode: CoreOpcode) -> Vec<u32> {
        code_block
            .instructions()
            .declarations()
            .iter()
            .filter(|instruction| CoreOpcode::from_opcode(instruction.opcode) == Some(opcode))
            .filter_map(|instruction| match instruction.operands.get(1) {
                Some(Operand::IdentifierIndex(key)) => Some(*key),
                _ => None,
            })
            .collect()
    }

    fn emit_program_source(text: &str, session: u64) -> UnlinkedCodeBlock {
        let source = source(text);
        let mut arena = ParserArena::new();
        let parsed = Parser::with_mode(
            &mut arena,
            AstBuilder::default(),
            &source,
            crate::syntax::ParseMode::Program,
        )
        .parse()
        .unwrap();
        let input = bytecompiler_input_from_parsed_ast(
            BytecompilerSessionId(session),
            source.clone(),
            &parsed,
            &arena,
        )
        .unwrap();

        emit_unlinked_code_from_parsed_ast(&input, &arena)
            .unwrap()
            .unlinked_code
            .unwrap()
    }

    fn execute_program_source(text: &str, session: u64) -> ExecutionCompletion {
        let unlinked = emit_program_source(text, session);
        let code_block_id = CodeBlockId(CellId(session.try_into().unwrap_or(u32::MAX)));
        let code_block = CodeBlock::from_unlinked(unlinked, LinkContext::default());
        let mut stack = ExecutionContextStack::default();
        let mut registers = RegisterFile::default();
        let mut exceptions = crate::vm::ExceptionState::default();
        let mut heap = Heap::new();
        stack.enter(ExecutionEntryRecord::Program(ProgramExecutionEntry {
            code_block: code_block_id,
            global_object: GlobalObjectId(ObjectId(CellId(1))),
            this_value: RuntimeValue::undefined(),
        }));
        stack
            .push_frame(
                &mut registers,
                FramePushRequest {
                    code_block: Some(code_block_id),
                    callee: None,
                    callee_value: None,
                    lexical_scope: None,
                    shape: code_block.unlinked().frame(),
                    argument_count_including_this: 1,
                    argument_values: Vec::new(),
                    start_bytecode_index: Some(BytecodeIndex::from_offset(0)),
                    return_bytecode_index: None,
                },
            )
            .unwrap();

        let mut host = CoreOpcodeDispatchHost::new();
        execute_code_block(
            InterpreterExecutionState {
                stack: &mut stack,
                registers: &mut registers,
                exceptions: &mut exceptions,
                heap: &mut heap,
            },
            code_block_id,
            &code_block,
            &mut host,
            DispatchConfig::default(),
        )
    }

    fn first_instruction_index(
        code_block: &UnlinkedCodeBlock,
        opcode: CoreOpcode,
    ) -> BytecodeIndex {
        code_block
            .instructions()
            .decoded_instructions()
            .map(|instruction| instruction.expect("decoded instruction"))
            .find(|instruction| CoreOpcode::from_opcode(instruction.opcode) == Some(opcode))
            .map(|instruction| instruction.bytecode_index)
            .expect("opcode in bytecode")
    }

    fn exact_root_map_for_opcode(
        code_block: &UnlinkedCodeBlock,
        opcode: CoreOpcode,
    ) -> &BytecodeRootMap {
        let bytecode_index = first_instruction_index(code_block, opcode);
        code_block
            .side_tables()
            .root_maps
            .iter()
            .find(|root_map| {
                root_map.bytecode_range_start == bytecode_index
                    && root_map.bytecode_range_end == bytecode_index
            })
            .expect("exact helper root map")
    }

    #[test]
    fn bytecompiler_plan_validation_accepts_empty_early_phase() {
        let plan = BytecompilerOutputPlan::new(
            BytecompilerPhase::ParseProductInspection,
            valid_generation_plan(),
        );

        assert!(plan.validate().is_valid());
    }

    #[test]
    fn bytecompiler_plan_validation_reports_structural_mismatches() {
        let mut plan = BytecompilerOutputPlan::new(
            BytecompilerPhase::UnlinkedCodeBlockAssembly,
            valid_generation_plan(),
        );
        plan.finally_contexts.push(FinallyContextPlan {
            outer: Some(0),
            handles_returns: true,
            ..FinallyContextPlan::default()
        });
        plan.lexical_scopes.var_scope_index = Some(1);
        plan.for_in_contexts.push(ForInContextPlan {
            state: ForInContextState::Finalized,
            ..ForInContextPlan::default()
        });
        plan.using_scopes.push(UsingScopePlan {
            next_slot: 1,
            ..UsingScopePlan::default()
        });

        let findings = plan.validate().findings;
        assert!(
            findings.contains(&BytecompilerValidationFinding::MissingUnlinkedCodeBlock {
                phase: BytecompilerPhase::UnlinkedCodeBlockAssembly,
            })
        );
        assert!(findings.contains(
            &BytecompilerValidationFinding::FinallyReturnRegistersMissing { context: 0 }
        ));
        assert!(findings.contains(
            &BytecompilerValidationFinding::IncompleteFinalizedForInContext { context: 0 }
        ));
    }

    #[test]
    fn bytecompiler_input_planner_maps_module_source_to_generation_plan() {
        let input = BytecompilerInput {
            session: BytecompilerSessionId(7),
            source: source("import x from 'm';"),
            root: AstRoot::Module(AstRef::from_raw_index(0)),
            mode: BytecompilerMode::Module,
            code_features: CodeFeatures {
                import_meta: true,
                ..CodeFeatures::default()
            },
            module_analysis: Some(ModuleAnalysis::default()),
            semantic_model: None,
        };

        let plan = plan_bytecompiler_input(&input, valid_generation_plan().registers);

        assert_eq!(plan.generation.kind, CodeKind::Module);
        assert_eq!(plan.generation.parse_mode, ParseMode::Module);
        assert!(plan.generation.features.uses_import_meta);
        assert_eq!(plan.generation.source.source_length, 18);
        assert_eq!(plan.semantic.parse.goal, ParseSemanticGoal::Module);
        assert_eq!(plan.semantic.parse.strictness, SemanticStrictness::Strict);
    }

    #[test]
    fn parsed_ast_handoff_creates_bytecompiler_input() {
        let source = source("let answer = 42; answer;");
        let mut arena = ParserArena::new();
        let parsed = Parser::with_mode(
            &mut arena,
            AstBuilder::default(),
            &source,
            crate::syntax::ParseMode::Program,
        )
        .parse()
        .unwrap();

        let input = bytecompiler_input_from_parsed_ast(
            BytecompilerSessionId(9),
            source.clone(),
            &parsed,
            &arena,
        )
        .unwrap();
        let plan = plan_bytecompiler_input(&input, valid_generation_plan().registers);

        assert_eq!(input.mode, BytecompilerMode::Program);
        assert_eq!(input.root, parsed.root);
        assert_eq!(plan.generation.kind, CodeKind::Program);
        assert_eq!(plan.generation.source.source_length, 24);
        assert_eq!(plan.semantic.parse.goal, ParseSemanticGoal::Script);
    }

    #[test]
    fn parsed_ast_emits_core_bytecode_that_interpreter_executes() {
        let source = source("let answer = 40 + 2; return answer;");
        let mut arena = ParserArena::new();
        let parsed = Parser::with_mode(
            &mut arena,
            AstBuilder::default(),
            &source,
            crate::syntax::ParseMode::Program,
        )
        .parse()
        .unwrap();
        let input = bytecompiler_input_from_parsed_ast(
            BytecompilerSessionId(10),
            source.clone(),
            &parsed,
            &arena,
        )
        .unwrap();

        let plan = emit_unlinked_code_from_parsed_ast(&input, &arena).unwrap();
        let summary = summarize_bytecompiler_integration(&plan);
        assert!(summary.is_ready_for_bytecode_handoff());
        let unlinked = plan.unlinked_code.unwrap();
        assert_eq!(unlinked.instructions().instruction_count(), 5);

        let code_block_id = CodeBlockId(CellId(99));
        let code_block = CodeBlock::from_unlinked(unlinked, LinkContext::default());
        let mut stack = ExecutionContextStack::default();
        let mut registers = RegisterFile::default();
        let mut exceptions = crate::vm::ExceptionState::default();
        let mut heap = Heap::new();
        stack.enter(ExecutionEntryRecord::Program(ProgramExecutionEntry {
            code_block: code_block_id,
            global_object: GlobalObjectId(ObjectId(CellId(1))),
            this_value: RuntimeValue::undefined(),
        }));
        stack
            .push_frame(
                &mut registers,
                FramePushRequest {
                    code_block: Some(code_block_id),
                    callee: None,
                    callee_value: None,
                    lexical_scope: None,
                    shape: code_block.unlinked().frame(),
                    argument_count_including_this: 1,
                    argument_values: Vec::new(),
                    start_bytecode_index: Some(BytecodeIndex::from_offset(0)),
                    return_bytecode_index: None,
                },
            )
            .unwrap();

        let mut host = CoreOpcodeDispatchHost::new();
        let completion = execute_code_block(
            InterpreterExecutionState {
                stack: &mut stack,
                registers: &mut registers,
                exceptions: &mut exceptions,
                heap: &mut heap,
            },
            code_block_id,
            &code_block,
            &mut host,
            DispatchConfig::default(),
        );

        assert_eq!(
            completion,
            ExecutionCompletion::Returned(RuntimeValue::from_i32(42))
        );
    }

    #[test]
    fn parsed_ast_lowers_delete_member_without_property_get() {
        let source = source("let object = { value: 1 }; return delete object.value;");
        let mut arena = ParserArena::new();
        let parsed = Parser::with_mode(
            &mut arena,
            AstBuilder::default(),
            &source,
            crate::syntax::ParseMode::Program,
        )
        .parse()
        .unwrap();
        let input = bytecompiler_input_from_parsed_ast(
            BytecompilerSessionId(11),
            source.clone(),
            &parsed,
            &arena,
        )
        .unwrap();

        let plan = emit_unlinked_code_from_parsed_ast(&input, &arena).unwrap();
        let unlinked = plan.unlinked_code.unwrap();
        let opcodes = unlinked
            .instructions()
            .declarations()
            .iter()
            .map(|instruction| instruction.opcode)
            .collect::<Vec<_>>();

        assert!(opcodes.contains(&CoreOpcode::DeleteByName.opcode()));
        assert!(!opcodes.contains(&CoreOpcode::GetByName.opcode()));
    }

    #[test]
    fn parsed_ast_generates_root_map_for_object_literal() {
        let unlinked = emit_program_source("return {};", 15);
        let root_map = exact_root_map_for_opcode(&unlinked, CoreOpcode::NewObject);

        assert_eq!(root_map.owner, None);
        assert!(root_map.complete);
        assert_eq!(root_map.slots.len(), 1);
        assert_eq!(
            root_map.slots[0].bytecode_index,
            root_map.bytecode_range_start
        );
        assert_eq!(
            root_map.slots[0].kind,
            BytecodeRootSlotKind::VirtualRegister
        );
        assert!(matches!(
            root_map.slots[0].storage,
            BytecodeRootSlotStorage::Register(register) if register.is_local()
        ));
    }

    #[test]
    fn parsed_ast_generates_root_map_for_array_literal() {
        let unlinked = emit_program_source("return [];", 16);
        let root_map = exact_root_map_for_opcode(&unlinked, CoreOpcode::NewArray);

        assert_eq!(root_map.owner, None);
        assert!(root_map.complete);
        assert_eq!(root_map.slots.len(), 1);
        assert_eq!(
            root_map.slots[0].bytecode_index,
            root_map.bytecode_range_start
        );
        assert_eq!(
            root_map.slots[0].kind,
            BytecodeRootSlotKind::VirtualRegister
        );
    }

    #[test]
    fn parsed_ast_generates_typeof_root_map_with_argument_source_kind() {
        let unlinked = emit_program_source("return typeof this;", 17);
        let root_map = exact_root_map_for_opcode(&unlinked, CoreOpcode::TypeOf);

        assert_eq!(root_map.owner, None);
        assert!(root_map.complete);
        assert_eq!(root_map.slots.len(), 2);
        assert_eq!(
            root_map.slots[0].kind,
            BytecodeRootSlotKind::VirtualRegister
        );
        assert_eq!(root_map.slots[1].kind, BytecodeRootSlotKind::Argument);
        assert!(matches!(
            root_map.slots[1].storage,
            BytecodeRootSlotStorage::Register(register) if register.is_argument_or_header()
        ));
    }

    #[test]
    fn parsed_ast_generates_root_map_for_string_literal_load() {
        let unlinked = emit_program_source("return \"owned\";", 18);
        let root_map = exact_root_map_for_opcode(&unlinked, CoreOpcode::LoadString);
        let keys = load_string_identifier_keys(&unlinked);

        assert_eq!(keys.len(), 1);
        assert_eq!(unlinked.string_literal(keys[0]), Some("owned"));
        assert_eq!(root_map.owner, None);
        assert!(root_map.complete);
        assert_eq!(root_map.slots.len(), 1);
        assert_eq!(
            root_map.slots[0].kind,
            BytecodeRootSlotKind::VirtualRegister
        );
    }

    #[test]
    fn parsed_ast_generates_root_map_for_bigint_literal_load() {
        let unlinked = emit_program_source("return 12345678901234567890n;", 19);
        let root_map = exact_root_map_for_opcode(&unlinked, CoreOpcode::LoadBigInt);
        let keys = load_bigint_identifier_keys(&unlinked);

        assert_eq!(keys.len(), 1);
        assert_eq!(
            unlinked.string_literal(keys[0]),
            Some("12345678901234567890n")
        );
        assert_eq!(root_map.owner, None);
        assert!(root_map.complete);
        assert_eq!(root_map.slots.len(), 1);
        assert_eq!(
            root_map.slots[0].kind,
            BytecodeRootSlotKind::VirtualRegister
        );
    }

    #[test]
    fn parsed_ast_installs_code_block_string_literals_for_load_string() {
        let source = source("return \"owned\";");
        let mut arena = ParserArena::new();
        let parsed = Parser::with_mode(
            &mut arena,
            AstBuilder::default(),
            &source,
            crate::syntax::ParseMode::Program,
        )
        .parse()
        .unwrap();
        let input = bytecompiler_input_from_parsed_ast(
            BytecompilerSessionId(12),
            source.clone(),
            &parsed,
            &arena,
        )
        .unwrap();

        let plan = emit_unlinked_code_from_parsed_ast(&input, &arena).unwrap();
        let unlinked = plan.unlinked_code.as_ref().unwrap();
        let keys = load_string_identifier_keys(unlinked);

        assert_eq!(keys.len(), 1);
        assert_eq!(unlinked.string_literal(keys[0]), Some("owned"));
    }

    #[test]
    fn parsed_ast_installs_code_block_literal_text_for_load_bigint() {
        let source = source("return 12345678901234567890n;");
        let mut arena = ParserArena::new();
        let parsed = Parser::with_mode(
            &mut arena,
            AstBuilder::default(),
            &source,
            crate::syntax::ParseMode::Program,
        )
        .parse()
        .unwrap();
        let input = bytecompiler_input_from_parsed_ast(
            BytecompilerSessionId(13),
            source.clone(),
            &parsed,
            &arena,
        )
        .unwrap();

        let plan = emit_unlinked_code_from_parsed_ast(&input, &arena).unwrap();
        let unlinked = plan.unlinked_code.as_ref().unwrap();
        let keys = load_bigint_identifier_keys(unlinked);

        assert_eq!(keys.len(), 1);
        assert_eq!(
            unlinked.string_literal(keys[0]),
            Some("12345678901234567890n")
        );
    }

    #[test]
    fn parsed_ast_installs_nested_function_string_literals_on_function_body() {
        let source = source("function read() { return \"nested\"; } return read;");
        let mut arena = ParserArena::new();
        let parsed = Parser::with_mode(
            &mut arena,
            AstBuilder::default(),
            &source,
            crate::syntax::ParseMode::Program,
        )
        .parse()
        .unwrap();
        let input = bytecompiler_input_from_parsed_ast(
            BytecompilerSessionId(13),
            source.clone(),
            &parsed,
            &arena,
        )
        .unwrap();

        let plan = emit_unlinked_code_from_parsed_ast(&input, &arena).unwrap();
        let program = plan.unlinked_code.as_ref().unwrap();
        let nested = plan
            .function_bodies
            .iter()
            .find(|body| {
                body.string_literals()
                    .entries()
                    .iter()
                    .any(|entry| entry.text == "nested")
            })
            .unwrap();
        let nested_keys = load_string_identifier_keys(nested);

        assert!(!program
            .string_literals()
            .entries()
            .iter()
            .any(|entry| entry.text == "nested"));
        assert!(nested_keys
            .iter()
            .any(|key| nested.string_literal(*key) == Some("nested")));
    }

    #[test]
    fn parsed_ast_installs_nested_function_bigint_literals_on_function_body() {
        let source = source("function read() { return 9007199254740993n; } return read;");
        let mut arena = ParserArena::new();
        let parsed = Parser::with_mode(
            &mut arena,
            AstBuilder::default(),
            &source,
            crate::syntax::ParseMode::Program,
        )
        .parse()
        .unwrap();
        let input = bytecompiler_input_from_parsed_ast(
            BytecompilerSessionId(14),
            source.clone(),
            &parsed,
            &arena,
        )
        .unwrap();

        let plan = emit_unlinked_code_from_parsed_ast(&input, &arena).unwrap();
        let program = plan.unlinked_code.as_ref().unwrap();
        let nested = plan
            .function_bodies
            .iter()
            .find(|body| {
                body.string_literals()
                    .entries()
                    .iter()
                    .any(|entry| entry.text == "9007199254740993n")
            })
            .unwrap();
        let nested_keys = load_bigint_identifier_keys(nested);

        assert!(!program
            .string_literals()
            .entries()
            .iter()
            .any(|entry| entry.text == "9007199254740993n"));
        assert!(nested_keys
            .iter()
            .any(|key| nested.string_literal(*key) == Some("9007199254740993n")));
    }

    #[test]
    fn parsed_ast_collects_string_literals_for_program_and_function_bodies() {
        let source = source(
            "function read() { return \"inner\"; } let object = { name: \"outer\" }; let value = 19n; return object.name === read();",
        );
        let mut arena = ParserArena::new();
        let parsed = Parser::with_mode(
            &mut arena,
            AstBuilder::default(),
            &source,
            crate::syntax::ParseMode::Program,
        )
        .parse()
        .unwrap();
        let input = bytecompiler_input_from_parsed_ast(
            BytecompilerSessionId(11),
            source.clone(),
            &parsed,
            &arena,
        )
        .unwrap();

        let plan = emit_unlinked_code_from_parsed_ast(&input, &arena).unwrap();

        assert!(plan
            .literal_strings
            .values()
            .any(|value| value.as_str() == "inner"));
        assert!(plan
            .literal_strings
            .values()
            .any(|value| value.as_str() == "outer"));
        assert!(plan
            .literal_strings
            .values()
            .any(|value| value.as_str() == "19n"));
    }

    #[test]
    fn parsed_ast_executes_local_update_prefix_and_postfix_values() {
        let completion = execute_program_source(
            "let x = 1; let a = ++x; let b = x++; let c = --x; let d = x--; return a * 1000 + b * 100 + c * 10 + d + x;",
            20,
        );

        assert_eq!(
            completion,
            ExecutionCompletion::Returned(RuntimeValue::from_i32(2223))
        );
    }

    #[test]
    fn parsed_ast_executes_property_and_index_updates_with_single_base_and_key() {
        let completion = execute_program_source(
            "let count = 0; let object = { value: 1 }; let property = ((count = count + 1) ? object : object).value++; let array = [10]; let indexed = array[(count = count + 1) ? 0 : 0]++; return property + object.value * 10 + indexed * 100 + array[0] * 1000 + count * 10000;",
            21,
        );

        assert_eq!(
            completion,
            ExecutionCompletion::Returned(RuntimeValue::from_i32(32021))
        );
    }

    #[test]
    fn parsed_ast_executes_compound_assignments_for_locals_properties_and_indexes() {
        let completion = execute_program_source(
            "let x = 8; x += 4; x -= 2; x *= 3; x /= 5; x %= 4; x |= 8; x &= 10; x ^= 3; x <<= 1; x >>= 1; x >>>= 1; let object = { value: 5 }; let array = [3]; object.value += 2; array[0] *= 4; return x * 100 + object.value * 10 + array[0];",
            22,
        );

        assert_eq!(
            completion,
            ExecutionCompletion::Returned(RuntimeValue::from_i32(482))
        );
    }

    #[test]
    fn parsed_ast_executes_conditional_expression_branch_results() {
        let completion = execute_program_source(
            "let x = 0; let y = true ? (x = 1) : (x = 2); let z = false ? (x = 3) : (x = 4); return y * 100 + z * 10 + x;",
            23,
        );

        assert_eq!(
            completion,
            ExecutionCompletion::Returned(RuntimeValue::from_i32(144))
        );
    }

    #[test]
    fn parsed_ast_executes_octane_core_loose_equality_primitives() {
        let completion = execute_program_source(
            "let a = null == undefined; let b = 1 == \"1\"; let c = true == 1; let d = 0 != false; return a && b && c && !d ? 42 : 0;",
            24,
        );

        assert_eq!(
            completion,
            ExecutionCompletion::Returned(RuntimeValue::from_i32(42))
        );
    }

    #[test]
    fn bytecompiler_validation_reports_semantic_handoff_mismatches() {
        let mut plan = BytecompilerOutputPlan::new(
            BytecompilerPhase::ParseProductInspection,
            valid_generation_plan(),
        );
        plan.semantic.parse.goal = ParseSemanticGoal::Module;
        plan.semantic.parse.strictness = SemanticStrictness::Sloppy;
        plan.generation.environment.strict_mode = true;

        let findings = plan.validate().findings;

        assert!(findings.contains(&BytecompilerValidationFinding::ModuleSemanticPlanNotStrict));
        assert!(
            findings.contains(&BytecompilerValidationFinding::StrictModeHandoffMismatch {
                generation: true,
                semantic: SemanticStrictness::Sloppy,
            })
        );
    }

    #[test]
    fn bytecompiler_integration_summary_reports_handoff_shape() {
        let mut plan = BytecompilerOutputPlan::new(
            BytecompilerPhase::UnlinkedCodeBlockAssembly,
            valid_generation_plan(),
        );
        plan.unlinked_code = Some(UnlinkedCodeBlock::new(
            CodeKind::Program,
            crate::bytecode::PackedInstructionStream::default(),
        ));
        plan.labels.push(LabelScopePlan::default());
        plan.profile_flags
            .push(BytecompilerProfileFlag::FunctionReturnTypeProfile);

        let summary = summarize_bytecompiler_integration(&plan);

        assert!(summary.is_ready_for_bytecode_handoff());
        assert_eq!(summary.code_kind, CodeKind::Program);
        assert_eq!(summary.label_scope_count, 1);
        assert_eq!(summary.profile_flag_count, 1);
    }

    #[test]
    fn static_property_builder_classifies_member_accesses() {
        let expression = AstRef::<Expr>::from_raw_index(3);
        let member = MemberExpr {
            span: SourceSpan::new(SourcePosition(0), SourcePosition(1)),
            base: AstRef::from_raw_index(1),
            member: MemberKind::PrivateDot(ParserIdentifier(4)),
            optional: false,
        };
        let mut builder = StaticPropertyAnalysisBuilder::new();

        builder.record_member_expr(expression, &member);
        builder.record_spread(AstRef::from_raw_index(5));
        let plan = builder.finish();

        assert_eq!(plan.accesses[0].kind, StaticPropertyAccessKind::PrivateName);
        assert!(plan.accesses[0].requires_private_brand_check);
        assert_eq!(plan.accesses[1].kind, StaticPropertyAccessKind::Spread);
    }
}
