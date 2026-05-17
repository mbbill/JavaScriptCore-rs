//! Bytecompiler front-end contracts.
//!
//! JavaScriptCore's bytecompiler owns the semantic handoff from parsed syntax
//! into unlinked bytecode: register allocation, label scopes, static property
//! analysis, profiling flags, and generator state. The `bytecode` module owns
//! the representation of instructions and code blocks; this module owns the
//! source-to-bytecode planning boundary.

use crate::bytecode::{GenerationPlan, Label, RegisterAllocator, UnlinkedCodeBlock};
use crate::syntax::{AstRef, AstRoot, CodeFeatures, Expr, ModuleAnalysis, ScopeId, SourceCode};

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
    RegisterPlanning,
    LabelResolution,
    BytecodeEmission,
    UnlinkedCodeBlockAssembly,
}

#[derive(Clone, Debug)]
pub struct BytecompilerInput {
    pub session: BytecompilerSessionId,
    pub source: SourceCode,
    pub root: AstRoot,
    pub mode: BytecompilerMode,
    pub code_features: CodeFeatures,
    pub module_analysis: Option<ModuleAnalysis>,
}

#[derive(Clone, Debug, Default, Eq, PartialEq)]
pub struct LabelScopePlan {
    pub break_target: Option<Label>,
    pub continue_target: Option<Label>,
    pub lexical_scope: Option<ScopeId>,
    pub consumes_dynamic_scope: bool,
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
    ValueProfile,
    TypeProfile,
    ControlFlowProfile,
    SuperSampler,
}

#[derive(Clone, Debug)]
pub struct BytecompilerOutputPlan {
    pub phase: BytecompilerPhase,
    pub generation: GenerationPlan,
    pub registers: RegisterAllocator,
    pub labels: Vec<LabelScopePlan>,
    pub static_properties: StaticPropertyAnalysisPlan,
    pub profile_flags: Vec<BytecompilerProfileFlag>,
    pub unlinked_code: Option<UnlinkedCodeBlock>,
}
