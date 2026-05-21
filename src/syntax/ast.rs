use crate::syntax::arena::{AstRef, NodeId, ParserIdentifier};
use crate::syntax::semantic::{EarlySemanticInfo, ModuleAnalysis, SemanticScopeId};
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
    Conditional(ConditionalExpr),
    Call(CallExpr),
    New(NewExpr),
    Member(MemberExpr),
    Object(ObjectLiteralExpr),
    Array(ArrayLiteralExpr),
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
    FunctionDeclaration(FunctionDecl),
    If(IfStmt),
    DoWhile(DoWhileStmt),
    While(WhileStmt),
    Switch(SwitchStmt),
    For(ForStmt),
    ForOf(ForOfStmt),
    Try(TryStmt),
    Control(ControlStmt),
    Module(ModuleItem),
}

/// Scope-bearing syntax node with source and early semantic metadata.
#[derive(Clone, Debug, Eq, PartialEq)]
pub struct ScopeNode {
    pub id: SemanticScopeId,
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
    pub body: AstRef<ScopeNode>,
    pub parameters: Vec<FunctionParameter>,
    pub parameter_count: u32,
    pub strict: bool,
    pub contains_direct_eval: bool,
    pub super_binding: SuperBinding,
    pub private_brand: PrivateBrandRequirement,
}

#[derive(Clone, Copy, Debug, Eq, PartialEq)]
pub struct FunctionParameter {
    pub span: SourceSpan,
    pub pattern: AstRef<Pattern>,
    pub default_value: Option<AstRef<Expr>>,
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
    Number {
        value: NumberLiteralValue,
    },
    BigInt {
        text: ParserIdentifier,
    },
    String {
        text: ParserIdentifier,
    },
    RegExp {
        pattern: ParserIdentifier,
        flags: ParserIdentifier,
    },
}

#[derive(Clone, Copy, Debug, Eq, PartialEq)]
pub enum NumberLiteralValue {
    Int32(i32),
    DoubleBits(u64),
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
    Equal,
    NotEqual,
    StrictEqual,
    StrictNotEqual,
    LessThan,
    GreaterThan,
    LessEqual,
    GreaterEqual,
    Instanceof,
    In,
    LeftShift,
    RightShift,
    UnsignedRightShift,
    Add,
    Subtract,
    Multiply,
    Divide,
    Modulo,
    Pow,
}

#[derive(Clone, Debug, Eq, PartialEq)]
pub struct AssignmentExpr {
    pub span: SourceSpan,
    pub op: AssignmentOperator,
    pub target: AstRef<Pattern>,
    pub value: AstRef<Expr>,
    pub context: AssignmentContext,
}

#[derive(Clone, Copy, Debug, Eq, PartialEq)]
pub enum AssignmentOperator {
    Assign,
    Add,
    Subtract,
    Multiply,
    Divide,
    Modulo,
    BitOr,
    BitAnd,
    BitXor,
    LeftShift,
    RightShift,
    UnsignedRightShift,
    Pow,
    Coalesce,
    LogicalOr,
    LogicalAnd,
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
pub struct ConditionalExpr {
    pub span: SourceSpan,
    pub test: AstRef<Expr>,
    pub consequent: AstRef<Expr>,
    pub alternate: AstRef<Expr>,
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
pub struct NewExpr {
    pub span: SourceSpan,
    pub callee: AstRef<Expr>,
    pub arguments: Vec<AstRef<Expr>>,
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
pub struct ObjectLiteralExpr {
    pub span: SourceSpan,
    pub properties: Vec<ObjectLiteralProperty>,
}

#[derive(Clone, Copy, Debug, Eq, PartialEq)]
pub struct ObjectLiteralProperty {
    pub span: SourceSpan,
    pub key: AstPropertyKey,
    pub kind: ObjectLiteralPropertyKind,
    pub value: AstRef<Expr>,
}

#[derive(Clone, Copy, Debug, Eq, PartialEq)]
pub enum ObjectLiteralPropertyKind {
    Data,
    Getter,
    Setter,
    Spread,
}

#[derive(Clone, Debug, Eq, PartialEq)]
pub struct ArrayLiteralExpr {
    pub span: SourceSpan,
    pub elements: Vec<ArrayLiteralElement>,
}

/// Parser-owned array element syntax before runtime array allocation.
///
/// Holes and spread positions are preserved explicitly so later bytecode
/// generation can distinguish dense-element initialization from iterator
/// expansion and elision semantics without reparsing source text.
#[derive(Clone, Copy, Debug, Eq, PartialEq)]
pub enum ArrayLiteralElement {
    Expression(AstRef<Expr>),
    Elision(SourceSpan),
    Spread {
        span: SourceSpan,
        value: AstRef<Expr>,
    },
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
    pub is_synthesized_default_constructor: bool,
    pub initializer: Option<AstRef<Expr>>,
    pub metadata: Option<AstRef<FunctionMetadata>>,
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
    pub index: usize,
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

/// AST-only property-name spelling.
///
/// This preserves parser syntax and source spans before runtime conversion.
/// It must not stand in for `strings::PropertyKey`, which owns interned
/// string, symbol, private-name, and index identity after VM conversion.
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
    pub initializers: Vec<Option<AstRef<Expr>>>,
}

#[derive(Clone, Debug, Eq, PartialEq)]
pub struct FunctionDecl {
    pub span: SourceSpan,
    pub name: ParserIdentifier,
    pub metadata: AstRef<FunctionMetadata>,
}

#[derive(Clone, Debug, Eq, PartialEq)]
pub struct IfStmt {
    pub span: SourceSpan,
    pub condition: AstRef<Expr>,
    pub consequent: AstRef<Stmt>,
    pub alternate: Option<AstRef<Stmt>>,
}

#[derive(Clone, Debug, Eq, PartialEq)]
pub struct DoWhileStmt {
    pub span: SourceSpan,
    pub body: AstRef<Stmt>,
    pub condition: AstRef<Expr>,
}

#[derive(Clone, Debug, Eq, PartialEq)]
pub struct WhileStmt {
    pub span: SourceSpan,
    pub condition: AstRef<Expr>,
    pub body: AstRef<Stmt>,
}

#[derive(Clone, Debug, Eq, PartialEq)]
pub struct SwitchStmt {
    pub span: SourceSpan,
    pub discriminant: AstRef<Expr>,
    pub cases: Vec<SwitchCase>,
}

#[derive(Clone, Debug, Eq, PartialEq)]
pub struct SwitchCase {
    pub span: SourceSpan,
    pub test: Option<AstRef<Expr>>,
    pub statements: Vec<AstRef<Stmt>>,
}

#[derive(Clone, Debug, Eq, PartialEq)]
pub struct ForStmt {
    pub span: SourceSpan,
    pub init: Option<ForInit>,
    pub condition: Option<AstRef<Expr>>,
    pub update: Option<AstRef<Expr>>,
    pub body: AstRef<Stmt>,
}

#[derive(Clone, Debug, Eq, PartialEq)]
pub struct ForOfStmt {
    pub span: SourceSpan,
    pub binding: ForOfBinding,
    pub iterable: AstRef<Expr>,
    pub body: AstRef<Stmt>,
}

#[derive(Clone, Copy, Debug, Eq, PartialEq)]
pub enum ForOfBinding {
    Declaration {
        kind: DeclarationSyntaxKind,
        name: ParserIdentifier,
    },
    Assignment(ParserIdentifier),
}

#[derive(Clone, Debug, Eq, PartialEq)]
pub struct TryStmt {
    pub span: SourceSpan,
    pub body: AstRef<Stmt>,
    pub catch: Option<CatchClause>,
    pub finally: Option<AstRef<Stmt>>,
}

#[derive(Clone, Copy, Debug, Eq, PartialEq)]
pub struct CatchClause {
    pub span: SourceSpan,
    pub binding: Option<ParserIdentifier>,
    pub body: AstRef<Stmt>,
}

/// Parser-owned `for` initializer syntax before scope lowering.
///
/// Keeping declarations distinct from expression initializers lets the
/// bytecompiler predeclare locals before it emits loop bytecode, matching the
/// existing top-down ownership split between parser shape and register
/// planning.
#[derive(Clone, Debug, Eq, PartialEq)]
pub enum ForInit {
    Declaration(DeclarationStmt),
    Expression(AstRef<Expr>),
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
