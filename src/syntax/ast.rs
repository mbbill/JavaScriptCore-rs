use crate::syntax::arena::{AstRef, NodeId, ParserIdentifier};
use crate::syntax::semantic::{EarlySemanticInfo, ModuleAnalysis, ScopeId};
use crate::syntax::source::SourceSpan;

/// Root of an arena-owned syntax product.
#[derive(Clone, Copy, Debug, Eq, PartialEq)]
pub enum AstRoot {
    Script(AstRef<ScopeNode>),
    Module(AstRef<ScopeNode>),
    Function(AstRef<ScopeNode>),
}

/// Header shared by every future AST node.
///
/// JSC C++ nodes carry token location, end offset, and several virtual query
/// hooks. Rust nodes should carry a compact header plus enum-specific payloads;
/// bytecode and analysis phases can pattern-match on typed handles instead of
/// downcasting.
#[derive(Clone, Copy, Debug, Eq, PartialEq)]
pub struct NodeHeader {
    pub id: NodeId,
    pub kind: SyntaxKind,
    pub span: SourceSpan,
    pub debug_hook: DebugHook,
}

#[derive(Clone, Copy, Debug, Eq, PartialEq)]
pub enum DebugHook {
    None,
    NeedsStatementHook,
    NeedsExpressionHook,
}

#[derive(Clone, Copy, Debug, Eq, PartialEq)]
pub enum SyntaxKind {
    Program,
    Module,
    FunctionBody,
    Statement,
    Expression,
    Pattern,
    Declaration,
    ClassElement,
    ImportDeclaration,
    ExportDeclaration,
}

/// Expression node contract.
///
/// Variants are intentionally broad. Future generated or enum-based nodes must
/// remain arena-owned and immutable after parse finalization.
#[derive(Clone, Debug, Eq, PartialEq)]
#[non_exhaustive]
pub enum Expr {
    Literal(LiteralExpr),
    Name(NameExpr),
    Unary(UnaryExpr),
    Binary(BinaryExpr),
    Assignment(AssignmentExpr),
    Call(CallExpr),
    Member(MemberExpr),
    Function(AstRef<FunctionMetadata>),
    Class(ClassExpr),
    Template(TemplateExpr),
    ImportMeta(SourceSpan),
}

/// Statement node contract.
#[derive(Clone, Debug, Eq, PartialEq)]
#[non_exhaustive]
pub enum Stmt {
    Empty(SourceSpan),
    Expression(AstRef<Expr>),
    Block(ScopeBlock),
    Declaration(DeclarationStmt),
    Control(ControlStmt),
    Module(ModuleItem),
}

/// Scope-bearing syntax node with source and early semantic metadata.
#[derive(Clone, Debug, Eq, PartialEq)]
pub struct ScopeNode {
    pub id: ScopeId,
    pub kind: ScopeNodeKind,
    pub span: SourceSpan,
    pub statements: Vec<AstRef<Stmt>>,
    pub semantics: EarlySemanticInfo,
    pub module: Option<ModuleAnalysis>,
}

/// Function parse metadata preserved for bytecode generation and reparsing.
#[derive(Clone, Debug, Eq, PartialEq)]
pub struct FunctionMetadata {
    pub name_span: Option<SourceSpan>,
    pub name: Option<ParserIdentifier>,
    pub mode: FunctionSyntaxMode,
    pub body_span: SourceSpan,
    pub parameter_count: u32,
    pub strict: bool,
    pub contains_direct_eval: bool,
    pub super_binding: SuperBinding,
    pub private_brand: PrivateBrandRequirement,
}

#[derive(Clone, Copy, Debug, Eq, PartialEq)]
pub enum ScopeNodeKind {
    Script,
    Module,
    Function,
    Eval,
    ClassStaticBlock,
}

#[derive(Clone, Copy, Debug, Eq, PartialEq)]
pub enum FunctionSyntaxMode {
    Normal,
    Generator,
    Async,
    AsyncGenerator,
    Arrow,
    Method,
    Getter,
    Setter,
    ClassFieldInitializer,
    ClassStaticBlock,
}

#[derive(Clone, Copy, Debug, Eq, PartialEq)]
pub enum SuperBinding {
    Needed,
    NotNeeded,
}

#[derive(Clone, Copy, Debug, Eq, PartialEq)]
pub enum PrivateBrandRequirement {
    None,
    Needed,
}

#[derive(Clone, Debug, Eq, PartialEq)]
pub struct LiteralExpr {
    pub span: SourceSpan,
    pub kind: LiteralKind,
}

#[derive(Clone, Copy, Debug, Eq, PartialEq)]
pub enum LiteralKind {
    Null,
    Boolean(bool),
    Number,
    BigInt,
    String,
    RegExp,
}

#[derive(Clone, Debug, Eq, PartialEq)]
pub struct NameExpr {
    pub span: SourceSpan,
    pub name: ParserIdentifier,
    pub kind: NameKind,
}

#[derive(Clone, Copy, Debug, Eq, PartialEq)]
pub enum NameKind {
    Resolve,
    Private,
    This,
    Super,
    NewTarget,
}

#[derive(Clone, Debug, Eq, PartialEq)]
pub struct UnaryExpr {
    pub span: SourceSpan,
    pub op: UnaryOperator,
    pub argument: AstRef<Expr>,
}

#[derive(Clone, Copy, Debug, Eq, PartialEq)]
pub enum UnaryOperator {
    Delete,
    Void,
    Typeof,
    Plus,
    Minus,
    BitNot,
    LogicalNot,
    PreIncrement,
    PreDecrement,
    PostIncrement,
    PostDecrement,
    Await,
    Yield,
}

#[derive(Clone, Debug, Eq, PartialEq)]
pub struct BinaryExpr {
    pub span: SourceSpan,
    pub op: BinaryOperator,
    pub left: AstRef<Expr>,
    pub right: AstRef<Expr>,
}

#[derive(Clone, Copy, Debug, Eq, PartialEq)]
pub enum BinaryOperator {
    LogicalOr,
    LogicalAnd,
    Coalesce,
    BitOr,
    BitXor,
    BitAnd,
    Equality,
    Relational,
    Shift,
    Additive,
    Multiplicative,
    Exponentiation,
}

#[derive(Clone, Debug, Eq, PartialEq)]
pub struct AssignmentExpr {
    pub span: SourceSpan,
    pub target: AstRef<Pattern>,
    pub value: AstRef<Expr>,
    pub context: AssignmentContext,
}

#[derive(Clone, Copy, Debug, Eq, PartialEq)]
pub enum AssignmentContext {
    Expression,
    Declaration,
    ConstDeclaration,
    UsingDeclaration,
    AwaitUsingDeclaration,
}

#[derive(Clone, Debug, Eq, PartialEq)]
pub struct CallExpr {
    pub span: SourceSpan,
    pub callee: AstRef<Expr>,
    pub arguments: Vec<AstRef<Expr>>,
    pub optional: bool,
    pub tail_position: bool,
}

#[derive(Clone, Debug, Eq, PartialEq)]
pub struct MemberExpr {
    pub span: SourceSpan,
    pub base: AstRef<Expr>,
    pub member: MemberKind,
    pub optional: bool,
}

#[derive(Clone, Copy, Debug, Eq, PartialEq)]
pub enum MemberKind {
    Dot(ParserIdentifier),
    PrivateDot(ParserIdentifier),
    Bracket(AstRef<Expr>),
}

#[derive(Clone, Debug, Eq, PartialEq)]
pub struct ClassExpr {
    pub span: SourceSpan,
    pub name: Option<ParserIdentifier>,
    pub heritage: Option<AstRef<Expr>>,
    pub elements: Vec<ClassElement>,
}

#[derive(Clone, Debug, Eq, PartialEq)]
pub struct ClassElement {
    pub span: SourceSpan,
    pub name: ClassElementName,
    pub kind: ClassElementKind,
    pub is_static: bool,
}

#[derive(Clone, Copy, Debug, Eq, PartialEq)]
pub enum ClassElementName {
    Public(ParserIdentifier),
    Private(ParserIdentifier),
    Computed(AstRef<Expr>),
}

#[derive(Clone, Copy, Debug, Eq, PartialEq)]
pub enum ClassElementKind {
    Field,
    Method,
    Getter,
    Setter,
    StaticBlock,
}

#[derive(Clone, Debug, Eq, PartialEq)]
pub struct TemplateExpr {
    pub span: SourceSpan,
    pub tag: Option<AstRef<Expr>>,
    pub quasis: Vec<TemplatePart>,
    pub expressions: Vec<AstRef<Expr>>,
}

#[derive(Clone, Copy, Debug, Eq, PartialEq)]
pub struct TemplatePart {
    pub cooked: Option<ParserIdentifier>,
    pub raw: ParserIdentifier,
    pub span: SourceSpan,
}

#[derive(Clone, Debug, Eq, PartialEq)]
pub enum Pattern {
    Binding(ParserIdentifier),
    AssignmentTarget(AstRef<Expr>),
    Array(Vec<PatternElement>),
    Object(Vec<PatternProperty>),
    Rest(Box<Pattern>),
}

#[derive(Clone, Debug, Eq, PartialEq)]
pub struct PatternElement {
    pub span: SourceSpan,
    pub pattern: Pattern,
    pub default_value: Option<AstRef<Expr>>,
}

#[derive(Clone, Debug, Eq, PartialEq)]
pub struct PatternProperty {
    pub span: SourceSpan,
    pub key: AstPropertyKey,
    pub pattern: Pattern,
    pub default_value: Option<AstRef<Expr>>,
}

#[derive(Clone, Copy, Debug, Eq, PartialEq)]
pub enum AstPropertyKey {
    Identifier(ParserIdentifier),
    String(ParserIdentifier),
    Number(ParserIdentifier),
    Private(ParserIdentifier),
    Computed(AstRef<Expr>),
}

#[derive(Clone, Debug, Eq, PartialEq)]
pub struct ScopeBlock {
    pub span: SourceSpan,
    pub scope: AstRef<ScopeNode>,
}

#[derive(Clone, Debug, Eq, PartialEq)]
pub struct DeclarationStmt {
    pub span: SourceSpan,
    pub kind: DeclarationSyntaxKind,
    pub bindings: Vec<AstRef<Pattern>>,
}

#[derive(Clone, Copy, Debug, Eq, PartialEq)]
pub enum DeclarationSyntaxKind {
    Var,
    Let,
    Const,
    Using,
    AwaitUsing,
    Function,
    Class,
}

#[derive(Clone, Debug, Eq, PartialEq)]
pub struct ControlStmt {
    pub span: SourceSpan,
    pub kind: ControlKind,
}

#[derive(Clone, Copy, Debug, Eq, PartialEq)]
pub enum ControlKind {
    Return(Option<AstRef<Expr>>),
    Throw(AstRef<Expr>),
    Break(Option<ParserIdentifier>),
    Continue(Option<ParserIdentifier>),
}

#[derive(Clone, Debug, Eq, PartialEq)]
pub enum ModuleItem {
    Import(ImportDecl),
    Export(ExportDecl),
}

#[derive(Clone, Debug, Eq, PartialEq)]
pub struct ImportDecl {
    pub span: SourceSpan,
    pub module_request: ParserIdentifier,
    pub attributes: Vec<ImportAttribute>,
}

#[derive(Clone, Copy, Debug, Eq, PartialEq)]
pub struct ImportAttribute {
    pub key: ParserIdentifier,
    pub value: ParserIdentifier,
    pub span: SourceSpan,
}

#[derive(Clone, Debug, Eq, PartialEq)]
pub struct ExportDecl {
    pub span: SourceSpan,
    pub source: Option<ParserIdentifier>,
    pub entries: Vec<ExportEntry>,
}

#[derive(Clone, Copy, Debug, Eq, PartialEq)]
pub struct ExportEntry {
    pub local: Option<ParserIdentifier>,
    pub exported: ParserIdentifier,
    pub span: SourceSpan,
}
