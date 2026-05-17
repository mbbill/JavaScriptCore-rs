//! DFG graph, node, and edge descriptors.
//!
//! These structures mirror the ownership boundaries of JSC's DFG graph. They
//! describe graph shape, origin tracking, effects, and typed child edges without
//! parsing bytecode, optimizing, lowering, or executing code.

use crate::bytecode::{Opcode, VirtualRegister};
use crate::jit::{CodeOrigin, JitCodeId, WatchpointDependency};
use crate::object::{AtomId, PropertyOffset, StructureId};
use crate::runtime::{CodeBlockId, ExecutableId, ObjectId};

/// Stable identity for a graph produced for one compilation plan.
#[derive(Clone, Copy, Debug, Eq, Hash, Ord, PartialEq, PartialOrd)]
pub struct DfgGraphId(pub u64);

/// Stable identity for a node in a graph.
#[derive(Clone, Copy, Debug, Eq, Hash, Ord, PartialEq, PartialOrd)]
pub struct DfgNodeId(pub u32);

/// Stable identity for a basic block in a graph.
#[derive(Clone, Copy, Debug, Eq, Hash, Ord, PartialEq, PartialOrd)]
pub struct BasicBlockId(pub u32);

/// Stable identity for a child edge. Varargs children can use IDs beyond the
/// fixed child slots.
#[derive(Clone, Copy, Debug, Eq, Hash, Ord, PartialEq, PartialOrd)]
pub struct DfgEdgeId(pub u32);

/// Phase or ownership stage that produced the graph snapshot.
#[derive(Clone, Copy, Debug, Eq, PartialEq)]
pub enum DfgPhase {
    BytecodeParsing,
    PredictionInjection,
    Fixup,
    Cfa,
    SsaConversion,
    Optimization,
    LoweringPreparation,
    JitCompilation,
    FtlHandoff,
    DiagnosticSnapshot,
}

/// Representation form of the graph.
#[derive(Clone, Copy, Debug, Eq, PartialEq)]
pub enum GraphForm {
    Cps,
    Ssa,
    LoadStore,
    LoweredForDfgJit,
    LoweredForFtl,
}

/// Origin category used for diagnostics, OSR exits, and source mapping.
#[derive(Clone, Copy, Debug, Eq, PartialEq)]
pub enum NodeOriginKind {
    Bytecode,
    InlineCall,
    Checkpoint,
    ExitProfile,
    Constant,
    Synthetic,
    FtlLowering,
}

/// Source and bytecode provenance for a DFG node.
#[derive(Clone, Copy, Debug, Eq, PartialEq)]
pub struct NodeOrigin {
    pub kind: NodeOriginKind,
    pub code_origin: CodeOrigin,
    pub owner: Option<CodeBlockId>,
    pub executable: Option<ExecutableId>,
    pub bytecode_index: Option<u32>,
    pub inline_depth: u16,
}

/// High-level node family. Concrete opcode-specific payloads stay in generated
/// DFG tables later.
#[derive(Clone, Copy, Debug, Eq, PartialEq)]
pub enum DfgNodeKind {
    Phantom,
    Constant,
    Argument,
    GetLocal,
    SetLocal,
    Phi,
    Upsilon,
    Check,
    CheckStructure,
    CheckCell,
    Arith,
    Compare,
    Branch,
    Switch,
    Call,
    Construct,
    GetById,
    PutById,
    GetByVal,
    PutByVal,
    NewObject,
    NewArray,
    StructureTransition,
    Watchpoint,
    OsrEntry,
    OsrExit,
    Return,
    Throw,
    Unreachable,
    Bytecode(Opcode),
}

/// Value representation selected or expected for a node.
#[derive(Clone, Copy, Debug, Eq, PartialEq)]
pub enum DfgValueRep {
    Untyped,
    Cell,
    Boolean,
    Int32,
    Int52,
    Double,
    String,
    Object,
    Storage,
    BigInt,
    Void,
}

/// Effect summary used by planning and lowering contracts.
#[derive(Clone, Copy, Debug, Default, Eq, PartialEq)]
pub struct NodeEffects {
    pub reads_heap: bool,
    pub writes_heap: bool,
    pub allocates: bool,
    pub may_call: bool,
    pub may_throw: bool,
    pub may_exit: bool,
    pub terminates_block: bool,
}

/// Use category attached to a child edge.
#[derive(Clone, Copy, Debug, Eq, PartialEq)]
pub enum EdgeUseKind {
    Untyped,
    Cell,
    KnownCell,
    Boolean,
    Int32,
    Int52,
    Double,
    String,
    Object,
    Storage,
    BigInt,
}

/// Whether an edge still requires a runtime proof/check.
#[derive(Clone, Copy, Debug, Eq, PartialEq)]
pub enum EdgeProofStatus {
    NeedsCheck,
    IsProved,
    CannotBeProved,
}

/// Whether a child value is consumed by the parent.
#[derive(Clone, Copy, Debug, Eq, PartialEq)]
pub enum EdgeKillStatus {
    DoesNotKill,
    DoesKill,
}

/// Child edge between nodes.
#[derive(Clone, Copy, Debug, Eq, PartialEq)]
pub struct DfgEdge {
    pub id: DfgEdgeId,
    pub from: DfgNodeId,
    pub to: DfgNodeId,
    pub child_index: u16,
    pub use_kind: EdgeUseKind,
    pub proof: EdgeProofStatus,
    pub kill: EdgeKillStatus,
}

/// Branch target metadata that can point to a block or unresolved bytecode
/// index during graph construction.
#[derive(Clone, Copy, Debug, Eq, PartialEq)]
pub enum BranchTarget {
    Block(BasicBlockId),
    BytecodeIndex(u32),
    Fallthrough,
    Unresolved,
}

/// Data-only DFG node descriptor.
#[derive(Clone, Debug, Eq, PartialEq)]
pub struct DfgNode {
    pub id: DfgNodeId,
    pub kind: DfgNodeKind,
    pub origin: NodeOrigin,
    pub result: DfgValueRep,
    pub children: Vec<DfgEdgeId>,
    pub effects: NodeEffects,
    pub virtual_register: Option<VirtualRegister>,
    pub structure: Option<StructureId>,
    pub property: Option<AtomId>,
    pub property_offset: Option<PropertyOffset>,
    pub object: Option<ObjectId>,
    pub watchpoints: Vec<WatchpointDependency>,
}

/// Basic block descriptor with control-flow edges.
#[derive(Clone, Debug, Eq, PartialEq)]
pub struct DfgBasicBlock {
    pub id: BasicBlockId,
    pub nodes: Vec<DfgNodeId>,
    pub predecessors: Vec<BasicBlockId>,
    pub successors: Vec<BranchTarget>,
    pub bytecode_begin: Option<u32>,
    pub bytecode_end: Option<u32>,
    pub execution_count: Option<u64>,
    pub is_osr_entry: bool,
    pub is_catch_entry: bool,
}

/// Complete graph snapshot reserved for a DFG or FTL compilation plan.
#[derive(Clone, Debug, Eq, PartialEq)]
pub struct DfgGraph {
    pub id: DfgGraphId,
    pub owner: CodeBlockId,
    pub executable: Option<ExecutableId>,
    pub phase: DfgPhase,
    pub form: GraphForm,
    pub root_code: Option<JitCodeId>,
    pub blocks: Vec<DfgBasicBlock>,
    pub nodes: Vec<DfgNode>,
    pub edges: Vec<DfgEdge>,
    pub watchpoints: Vec<WatchpointDependency>,
    pub generation: u64,
}
