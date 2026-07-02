//! DFG graph, node, and edge descriptors.
//!
//! These structures mirror the ownership boundaries of JSC's DFG graph
//! (`dfg/DFGGraph.h`): the graph owns its blocks, nodes, edges, and the
//! `VariableAccessData` arena; nodes carry a faithful `NodeType` opcode plus
//! `NodeFlags` (result bits included) as `DFG::Node` does. The bytecode parser
//! (a later unit) owns a mutable working graph and appends through the `&mut`
//! APIs below, exactly as `ByteCodeParser` mutates `Graph&`.

use std::collections::HashMap;

use crate::bytecode::{ConstantValue, Operands, VirtualRegister};
use crate::dfg::frozen_value::{FrozenValue, FrozenValueId, ValueStrength};
use crate::dfg::node_flags::{
    NodeFlags, NODE_HAS_VAR_ARGS, NODE_MUST_GENERATE, NODE_RESULT_BOOLEAN, NODE_RESULT_DOUBLE,
    NODE_RESULT_INT32, NODE_RESULT_INT52, NODE_RESULT_JS, NODE_RESULT_MASK, NODE_RESULT_NUMBER,
    NODE_RESULT_STORAGE,
};
use crate::dfg::node_type::{default_flags, NodeType};
use crate::dfg::variable_access_data::VariableAccessData;
use crate::gc::StructureId;
use crate::jit::{CodeOrigin, EffectSummary, JitCodeId, JitType, WatchpointDependency};
use crate::object::PropertyOffset;
use crate::runtime::{CodeBlockId, ExecutableId, ObjectId};
use crate::strings::AtomId;
use crate::value::JsValue;

/// Stable identity for a graph produced for one compilation plan.
#[derive(Clone, Copy, Debug, Eq, Hash, Ord, PartialEq, PartialOrd)]
pub struct DfgGraphId(pub u64);

/// Stable identity for a node in a graph: its index in the graph's node list.
///
/// JSC hands out `Node*` and gives each node an index (`Node::index()`); safe
/// Rust uses the index alone. The node list is append-only during parsing, so
/// ids stay stable while the parser builds the graph.
#[derive(Clone, Copy, Debug, Eq, Hash, Ord, PartialEq, PartialOrd)]
pub struct DfgNodeId(pub u32);

/// Stable identity for a basic block in a DFG graph: its index in the graph's
/// block list, as assigned by `Graph::appendBlock` (dfg/DFGGraph.h:633-637).
#[derive(Clone, Copy, Debug, Eq, Hash, Ord, PartialEq, PartialOrd)]
pub struct DfgBasicBlockId(pub u32);

/// Stable identity for a child edge. Varargs children can use IDs beyond the
/// fixed child slots.
#[derive(Clone, Copy, Debug, Eq, Hash, Ord, PartialEq, PartialOrd)]
pub struct DfgEdgeId(pub u32);

/// Index into the graph-owned `VariableAccessData` arena. The safe-Rust
/// stand-in for the stable `VariableAccessData*` JSC hands out from
/// `Graph::m_variableAccessData` (dfg/DFGGraph.h:1413).
#[derive(Clone, Copy, Debug, Default, Eq, Hash, Ord, PartialEq, PartialOrd)]
pub struct DfgVariableAccessDataId(pub u32);

#[derive(Clone, Copy, Debug, Default, Eq, Hash, Ord, PartialEq, PartialOrd)]
pub struct DfgInlineCallFrameId(pub u32);

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

#[derive(Clone, Copy, Debug, Eq, PartialEq)]
pub enum DfgGraphMutationAuthority {
    BytecodeParser,
    GraphPhase,
    SsaPhase,
    DfgJitCompiler,
    FtlLowering,
    DiagnosticsOnly,
}

/// Representation form of the graph.
///
/// Faithful to C++ `enum GraphForm` (dfg/DFGCommon.h:160-205): LoadStore has
/// implicit data flow through GetLocal/SetLocal/Flush and is suitable for CFG
/// transformations; ThreadedCPS threads explicit variablesAtHead/variablesAtTail
/// liveness for data-flow analysis and codegen; SSA is the FTL input form.
#[derive(Clone, Copy, Debug, Eq, PartialEq)]
pub enum GraphForm {
    LoadStore,
    ThreadedCps,
    Ssa,
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
///
/// Mirrors C++ `DFG::NodeOrigin` (dfg/DFGNodeOrigin.h:38). C++ carries TWO
/// `CodeOrigin`s (`semantic` and `forExit`) because code motion can move a
/// check away from its bytecode resume point; this descriptor keeps ONE
/// `code_origin` (the SEMANTIC origin) until a code-motion phase needs the
/// split — the bytecode parser never separates them except for delayed
/// SetLocals, whose recorded semantic origin is what this field stores.
#[derive(Clone, Copy, Debug, Eq, PartialEq)]
pub struct NodeOrigin {
    pub kind: NodeOriginKind,
    pub code_origin: CodeOrigin,
    pub owner: Option<CodeBlockId>,
    pub executable: Option<ExecutableId>,
    pub bytecode_index: Option<u32>,
    pub inline_depth: u16,
    /// `bool exitOK` (dfg/DFGNodeOrigin.h:124): whether OSR exit state was
    /// intact when the node was emitted.
    pub exit_ok: bool,
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

impl NodeEffects {
    pub const fn effect_summary(self) -> EffectSummary {
        EffectSummary {
            reads_heap: self.reads_heap,
            writes_heap: self.writes_heap,
            allocates: self.allocates,
            may_call_js: self.may_call,
            may_throw: self.may_throw,
            may_exit: self.may_exit,
            terminates: self.terminates_block,
            reads_local_state: false,
            writes_local_state: false,
            reads_pinned: false,
            writes_pinned: false,
            fence: false,
        }
    }

    pub const fn is_observably_pure(self) -> bool {
        !self.effect_summary().observes_world()
            && !self.effect_summary().mutates_world()
            && !self.terminates_block
    }
}

#[derive(Clone, Debug, Eq, PartialEq)]
pub struct InlineVariableData {
    pub inline_call_frame: DfgInlineCallFrameId,
    pub argument_position_start: u32,
    pub callee_variable: Option<DfgVariableAccessDataId>,
}

#[derive(Clone, Debug, Default, Eq, PartialEq)]
pub struct DfgCommonDataDescriptor {
    pub inline_call_frames: Vec<DfgInlineCallFrameId>,
    pub inline_variables: Vec<InlineVariableData>,
    pub watchpoint_count: u32,
    pub weak_reference_count: u32,
    pub transition_count: u32,
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
    Block(DfgBasicBlockId),
    BytecodeIndex(u32),
    Fallthrough,
    Unresolved,
}

/// Data-only DFG node descriptor.
///
/// Mirrors C++ `DFG::Node` (dfg/DFGNode.h): `op` is the faithful `NodeType`
/// opcode and `flags` the `NodeFlags` bitset whose low bits encode the result
/// representation. Per-node operand/payload data stays here (JSC packs it into
/// `m_opInfo`; the Rust descriptor uses typed optional fields).
#[derive(Clone, Debug, Eq, PartialEq)]
pub struct DfgNode {
    pub id: DfgNodeId,
    /// `m_op` (dfg/DFGNode.h).
    pub op: NodeType,
    /// `m_flags`, initialized from `defaultFlags(op)` exactly as
    /// `setOpAndDefaultFlags` does (dfg/DFGNode.h:493-497).
    pub flags: NodeFlags,
    pub origin: NodeOrigin,
    pub children: Vec<DfgEdgeId>,
    pub effects: NodeEffects,
    /// `variableAccessData()` payload for local-access nodes (GetLocal,
    /// SetLocal, Flush, Phantom over locals, SetArgumentDefinitely); JSC stores
    /// the pointer in `m_opInfo`.
    pub variable_access_data: Option<DfgVariableAccessDataId>,
    pub virtual_register: Option<VirtualRegister>,
    /// `constant()` payload for JSConstant/DoubleConstant: C++ stores a
    /// `FrozenValue*` in `m_opInfo` (DFGNode.h `hasConstant()`/`constant()`);
    /// the safe-Rust descriptor carries the constant-pool value directly
    /// (`ConstantValue` is the linked pool's value representation).
    ///
    /// The frozen-value arena + `freeze`/`freeze_strong` GC machinery this
    /// comment used to defer now exists (`FrozenValue`, `frozen_values`,
    /// `frozen_value_map` below — dfg/frozen_value.rs), but this field is
    /// NOT wired to it yet: the bytecode parser's `get()` builds `constant`
    /// directly from the linked pool's `ConstantValue`
    /// (dfg/parser.rs:391-455) without freezing, because no linked constant
    /// pool carries a cell-valued constant today (f213265's finding — string
    /// literals materialize via LoadString, not a pool cell) so there is
    /// nothing unsafe to freeze yet. Once link-time constant interning can
    /// produce a cell constant, `get()`'s `JSConstant`/`DoubleConstant`
    /// construction should route through `DfgGraph::freeze_strong`
    /// (mirroring `ByteCodeParser::get()`, DFGByteCodeParser.cpp:401-410) and
    /// this field likely becomes a `FrozenValueId` — a cross-cutting change
    /// deferred to that unit, not made here.
    pub constant: Option<ConstantValue>,
    pub structure: Option<StructureId>,
    pub property: Option<AtomId>,
    pub property_offset: Option<PropertyOffset>,
    pub object: Option<ObjectId>,
    pub watchpoints: Vec<WatchpointDependency>,
}

impl DfgNode {
    /// Fresh node with the opcode's default flags, mirroring the C++ Node
    /// constructors, which run `setOpAndDefaultFlags(op)`
    /// (dfg/DFGNode.h:366-378). The id is assigned by `DfgGraph::add_node`.
    pub fn new(op: NodeType, origin: NodeOrigin) -> Self {
        Self {
            id: DfgNodeId(0),
            op,
            flags: default_flags(op),
            origin,
            children: Vec::new(),
            effects: NodeEffects::default(),
            variable_access_data: None,
            virtual_register: None,
            constant: None,
            structure: None,
            property: None,
            property_offset: None,
            object: None,
            watchpoints: Vec::new(),
        }
    }

    /// `setOpAndDefaultFlags` (dfg/DFGNode.h:493-497).
    pub fn set_op_and_default_flags(&mut self, op: NodeType) {
        self.op = op;
        self.flags = default_flags(op);
    }

    /// `result()` (dfg/DFGNode.h:488-491).
    pub const fn result(&self) -> NodeFlags {
        self.flags & NODE_RESULT_MASK
    }

    /// `setResult` (dfg/DFGNode.h:481-486).
    pub fn set_result(&mut self, result: NodeFlags) {
        debug_assert!(result & !NODE_RESULT_MASK == 0);
        self.flags = (self.flags & !NODE_RESULT_MASK) | result;
    }

    /// `hasResult()` (dfg/DFGNode.h:1725-1728).
    pub const fn has_result(&self) -> bool {
        self.result() != 0
    }

    /// `hasInt32Result()` (dfg/DFGNode.h:1730-1733).
    pub const fn has_int32_result(&self) -> bool {
        self.result() == NODE_RESULT_INT32
    }

    /// `hasInt52Result()` (dfg/DFGNode.h:1735-1738).
    pub const fn has_int52_result(&self) -> bool {
        self.result() == NODE_RESULT_INT52
    }

    /// `hasNumberResult()` (dfg/DFGNode.h:1740-1743).
    pub const fn has_number_result(&self) -> bool {
        self.result() == NODE_RESULT_NUMBER
    }

    /// `hasNumberOrAnyIntResult()` (dfg/DFGNode.h:1745-1748).
    pub const fn has_number_or_any_int_result(&self) -> bool {
        self.has_number_result() || self.has_int32_result() || self.has_int52_result()
    }

    /// `hasDoubleResult()` (dfg/DFGNode.h:1770-1773).
    pub const fn has_double_result(&self) -> bool {
        self.result() == NODE_RESULT_DOUBLE
    }

    /// `hasJSResult()` (dfg/DFGNode.h:1775-1778).
    pub const fn has_js_result(&self) -> bool {
        self.result() == NODE_RESULT_JS
    }

    /// `hasBooleanResult()` (dfg/DFGNode.h:1780-1783).
    pub const fn has_boolean_result(&self) -> bool {
        self.result() == NODE_RESULT_BOOLEAN
    }

    /// `hasStorageResult()` (dfg/DFGNode.h:1785-1788).
    pub const fn has_storage_result(&self) -> bool {
        self.result() == NODE_RESULT_STORAGE
    }

    /// `mergeFlags(NodeFlags)` (dfg/DFGNode.h:458-464): bitwise-or merge,
    /// returning whether the flags changed.
    pub fn merge_flags(&mut self, flags: NodeFlags) -> bool {
        let merged = self.flags | flags;
        let changed = merged != self.flags;
        self.flags = merged;
        changed
    }

    /// `isTerminal()` (dfg/DFGNode.h:1873-1892), restricted to the ported
    /// `NodeType` subset (Switch/EntrySwitch/TailCall* are not declared yet).
    pub const fn is_terminal(&self) -> bool {
        matches!(
            self.op,
            NodeType::Jump
                | NodeType::Branch
                | NodeType::Return
                | NodeType::Unreachable
                | NodeType::Throw
        )
    }

    /// `mustGenerate()` — `NodeMustGenerate` set (dfg/DFGNodeFlags.h:46).
    pub const fn must_generate(&self) -> bool {
        self.flags & NODE_MUST_GENERATE != 0
    }

    /// `hasVarArgs()` — `NodeHasVarArgs` set (dfg/DFGNodeFlags.h:47).
    pub const fn has_var_args(&self) -> bool {
        self.flags & NODE_HAS_VAR_ARGS != 0
    }
}

/// Basic block descriptor with control-flow edges.
#[derive(Clone, Debug, Eq, PartialEq)]
pub struct DfgBasicBlock {
    pub id: DfgBasicBlockId,
    pub nodes: Vec<DfgNodeId>,
    pub predecessors: Vec<DfgBasicBlockId>,
    pub successors: Vec<BranchTarget>,
    /// Variable state at the block head, keyed by operand
    /// (`Operands<Node*> variablesAtHead`, dfg/DFGBasicBlock.h:216). Safe Rust
    /// holds node identity as `Option<DfgNodeId>` where JSC uses a nullable
    /// `Node*`.
    pub variables_at_head: Operands<Option<DfgNodeId>>,
    /// Variable state at the block tail
    /// (`Operands<Node*> variablesAtTail`, dfg/DFGBasicBlock.h:217). The
    /// bytecode parser reads and updates this as it emits GetLocal/SetLocal.
    pub variables_at_tail: Operands<Option<DfgNodeId>>,
    pub bytecode_begin: Option<u32>,
    pub bytecode_end: Option<u32>,
    pub execution_count: Option<u64>,
    pub is_osr_entry: bool,
    pub is_catch_entry: bool,
}

/// Complete graph owned by a DFG or FTL compilation plan.
///
/// The owner is a typed runtime wrapper around `gc::CellId`; graph consumers
/// borrow that owner identity and must revalidate liveness before installation.
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
    /// Graph-owned `VariableAccessData` arena
    /// (`SegmentedVector<VariableAccessData, 16> m_variableAccessData`,
    /// dfg/DFGGraph.h:1413). Append-only during parsing, so
    /// `DfgVariableAccessDataId` indices stay stable where JSC hands out
    /// stable pointers.
    pub variable_access_data: Vec<VariableAccessData>,
    pub watchpoints: Vec<WatchpointDependency>,
    /// `m_frozenValues` (`SegmentedVector<FrozenValue, 16>`,
    /// dfg/DFGGraph.h:1376): the append-only frozen-value arena `freeze`/
    /// `freeze_strong` allocate into. Index-is-identity, same idiom as
    /// `nodes`/`blocks`/`edges` above; C++ hands out a stable `FrozenValue*`,
    /// Rust hands out the stable `FrozenValueId` index instead.
    pub frozen_values: Vec<FrozenValue>,
    /// `m_frozenValueMap` (`UncheckedKeyHashMap<EncodedJSValue, FrozenValue*,
    /// EncodedJSValueHash, EncodedJSValueHashTraits>`, dfg/DFGGraph.h:1375):
    /// dedups `freeze`/`freeze_strong` by the value's raw encoded bits, keyed
    /// the same way C++ keys by `EncodedJSValue` (a plain `uint64_t`).
    pub frozen_value_map: HashMap<u64, FrozenValueId>,
    pub common_data: DfgCommonDataDescriptor,
    pub generation: u64,
    /// DFG graph mutation is phase-local. Consumers outside the active phase
    /// should treat this snapshot as immutable and request a new phase output.
    pub mutation_authority: DfgGraphMutationAuthority,
}

#[derive(Clone, Debug, Eq, PartialEq)]
pub struct DfgDominanceSet {
    pub block: DfgBasicBlockId,
    pub dominators: Vec<DfgBasicBlockId>,
}

#[derive(Clone, Debug, Eq, PartialEq)]
pub enum DfgValidationError {
    EmptyName,
    EmptyProvenance(&'static str),
    DuplicatePlanName(&'static str),
    EmptyPhases(&'static str),
    EmptyAllowedNodeTypes(&'static str),
    GraphHasNoBlocks,
    DuplicateBlockId(DfgBasicBlockId),
    DuplicateNodeId(DfgNodeId),
    DuplicateEdgeId(DfgEdgeId),
    MissingBlock(DfgBasicBlockId),
    MissingNode(DfgNodeId),
    MissingEdge(DfgEdgeId),
    EdgeEndpointMissing(DfgEdgeId),
    EdgeNotOwnedBySourceNode(DfgEdgeId),
    BlockSuccessorMissing(DfgBasicBlockId),
    BlockBytecodeRangeInvalid(DfgBasicBlockId),
    TerminatorMismatch(DfgBasicBlockId),
    NodeTypeNotAllowed(DfgNodeId),
    GraphFormMismatch,
}

impl DfgGraph {
    /// Fresh, empty graph for one compilation plan. Mirrors the C++ Graph
    /// constructor: bytecode parsing starts in LoadStore form
    /// (`m_form(LoadStore)`, dfg/DFGGraph.cpp:87), and the parser is the first
    /// mutation authority. The parser (a later unit) owns this graph mutably
    /// and appends via the `add_*` APIs; there is no separate builder in JSC
    /// and none here.
    pub fn new(id: DfgGraphId, owner: CodeBlockId) -> Self {
        Self {
            id,
            owner,
            executable: None,
            phase: DfgPhase::BytecodeParsing,
            form: GraphForm::LoadStore,
            root_code: None,
            blocks: Vec::new(),
            nodes: Vec::new(),
            edges: Vec::new(),
            variable_access_data: Vec::new(),
            watchpoints: Vec::new(),
            frozen_values: Vec::new(),
            frozen_value_map: HashMap::new(),
            common_data: DfgCommonDataDescriptor::default(),
            generation: 0,
            mutation_authority: DfgGraphMutationAuthority::BytecodeParser,
        }
    }

    /// Append a node; its identity is its index. Mirrors `Graph::addNode`
    /// (dfg/DFGGraph.h:254-258), which allocates and returns the stable
    /// `Node*`; safe Rust returns the index id instead.
    pub fn add_node(&mut self, mut node: DfgNode) -> DfgNodeId {
        let id = DfgNodeId(self.nodes.len() as u32);
        node.id = id;
        self.nodes.push(node);
        id
    }

    /// Append a block; its identity is its index. Mirrors `Graph::appendBlock`,
    /// which sets `basicBlock->index = m_blocks.size()` (dfg/DFGGraph.h:633-637).
    pub fn add_block(&mut self, mut block: DfgBasicBlock) -> DfgBasicBlockId {
        let id = DfgBasicBlockId(self.blocks.len() as u32);
        block.id = id;
        self.blocks.push(block);
        id
    }

    /// Append a child edge; its identity is its index. JSC embeds edges in the
    /// node's `AdjacencyList`; the Rust descriptor keeps an edge table, so the
    /// same append-assigns-identity rule applies.
    pub fn add_edge(&mut self, mut edge: DfgEdge) -> DfgEdgeId {
        let id = DfgEdgeId(self.edges.len() as u32);
        edge.id = id;
        self.edges.push(edge);
        id
    }

    /// Allocate a fresh `VariableAccessData` in the graph-owned arena. Mirrors
    /// `ByteCodeParser::newVariableAccessData`, which appends to
    /// `m_graph.m_variableAccessData` (dfg/DFGByteCodeParser.cpp:368-372),
    /// including its `ASSERT(!operand.isConstant())`.
    pub fn new_variable_access_data(
        &mut self,
        operand: VirtualRegister,
    ) -> DfgVariableAccessDataId {
        debug_assert!(!operand.is_constant());
        let id = DfgVariableAccessDataId(self.variable_access_data.len() as u32);
        self.variable_access_data
            .push(VariableAccessData::new(operand));
        id
    }

    /// `Graph::freeze(JSValue)` (dfg/DFGGraph.cpp:1633-1657): "We use weak
    /// freezing by default" (DFGGraph.h:276). Dedups by the value's raw
    /// encoded bits through `frozen_value_map` into the append-only
    /// `frozen_values` arena, mirroring `m_frozenValueMap`/`m_frozenValues`.
    ///
    /// This is the ONLY faithful path a heap constant may enter a DFG graph
    /// through (GC-audit hard blocker #1 / divergence #3): a raw, un-frozen
    /// cell reference must never appear in the graph.
    ///
    /// `structure` is the resolved `Structure*` C++ reads live off the cell
    /// (`value.asCell()->structure()`, DFGFrozenValue.h:119) — see
    /// `FrozenValue`'s struct doc for why `DfgGraph::freeze` takes it as an
    /// explicit caller-supplied parameter instead (no heap access here) and
    /// for the second, narrower divergence (leaf cells not yet carrying a
    /// `StructureId` in this crate).
    ///
    /// C++ additionally special-cases `!value` by returning
    /// `FrozenValue::emptySingleton()` without touching the map/arena
    /// (DFGGraph.cpp:1636-1637) and `RELEASE_ASSERT`s the value is never a
    /// `CodeBlock` (:1643 — "we don't want [an optimized CodeBlock] to be
    /// part of the weak pointer set"). Neither applies here: this crate has
    /// no `CodeBlock`-as-`JSValue` concept, and an empty value simply dedups
    /// through the same map/arena as any other value (repeated freezes of
    /// the empty value still collapse to one arena entry, matching
    /// `emptySingleton`'s single shared instance in effect, just not in
    /// storage).
    pub fn freeze(&mut self, value: JsValue, structure: Option<StructureId>) -> FrozenValueId {
        let encoded = value.encoded().0;
        if let Some(&id) = self.frozen_value_map.get(&encoded) {
            return id;
        }
        let frozen = FrozenValue::with_structure(value, structure, ValueStrength::WeakValue);
        let id = FrozenValueId(self.frozen_values.len() as u32);
        self.frozen_values.push(frozen);
        self.frozen_value_map.insert(encoded, id);
        id
    }

    /// `Graph::freezeStrong(JSValue)` (dfg/DFGGraph.cpp:1659-1664): "Shorthand
    /// for freeze(value)->strengthenTo(StrongValue)" (DFGGraph.h:277). This is
    /// the path `ByteCodeParser::jsConstant`/`get()` route every bytecode
    /// constant operand through (DFGByteCodeParser.cpp:401-410, :807-810):
    /// "Assumes that the constant should be strongly marked."
    pub fn freeze_strong(
        &mut self,
        value: JsValue,
        structure: Option<StructureId>,
    ) -> FrozenValueId {
        let id = self.freeze(value, structure);
        self.frozen_values[id.0 as usize].strengthen_to(ValueStrength::StrongValue);
        id
    }

    /// Read back an arena entry by its stable id.
    pub fn frozen_value(&self, id: FrozenValueId) -> &FrozenValue {
        &self.frozen_values[id.0 as usize]
    }

    /// KEEP-ALIVE root set for this graph's frozen arena — the
    /// `Graph::visitChildren` analog (dfg/DFGGraph.cpp:1621-1628):
    /// ```cpp
    /// for (FrozenValue& value : m_frozenValues) {
    ///     visitor.appendUnbarriered(value.value());
    ///     visitor.appendUnbarriered(value.structure());
    /// }
    /// ```
    /// C++ traces EVERY frozen entry — both `WeakValue` and `StrongValue` —
    /// for as long as the owning `Plan`/`Graph` is reachable by the marker
    /// (a DFG worklist thread's live plans are GC roots during compilation).
    /// This is stronger than, and happens BEFORE, the one-time end-of-compile
    /// `registerFrozenValues` split into CodeBlock-constant (`StrongValue`)
    /// vs. `m_plan.weakReferences()` (`WeakValue`) (DFGGraph.cpp:1595-1619,
    /// out of this unit's scope — no `DfgPlan` reaches that compile stage
    /// yet). `value.structure()` is deliberately NOT included: this crate's
    /// `StructureId` (the `gc::StructureId` newtype) is a `StructureIdTable`
    /// registry handle (`object/structure_cell.rs`), not an arena `CellId`
    /// this collector sweeps, so there is nothing to root for it (divergence:
    /// C++ `Structure` is itself an ordinary swept `JSCell`, visited
    /// alongside the value; this crate's structures are a separate
    /// long-lived registry table outside the object-cell collector's sweep
    /// set — already true of every other `StructureId` field in this module,
    /// e.g. `DfgNode::structure`, none of which are rooted anywhere either).
    ///
    /// WIRING STATUS (the noted follow-up, not papered over): no live-plan
    /// registry exists anywhere in this crate yet — `DfgPlan`
    /// (dfg/plan.rs) is constructed ad hoc by each caller with no worklist or
    /// registry field on `CoreOpcodeDispatchHost` or `Vm` (verified: nothing
    /// outside dfg/plan.rs and its own tests references `DfgPlan`). C++'s
    /// production analog is a worklist thread's live `Plan` set, walked by
    /// the concurrent collector's root-marking pass. Until a live-plan owner
    /// exists, this method cannot be folded into
    /// `CoreOpcodeDispatchHost::poll_gc_collection_safepoint`'s `host_roots`
    /// gather (the established root-provider pattern from the CodeBlock
    /// constant-pool fold, `gather_code_block_constant_roots`,
    /// interpreter/mod.rs) — there is no live set to iterate. A DFG
    /// compilation MUST therefore run to completion without crossing a GC
    /// safepoint until that wiring lands, or the caller crossing a safepoint
    /// mid-compile must fold THIS method's output into its own roots itself.
    /// Proven directly (this unit's tests): fold this method's output into
    /// `force_collect_values`'s `extra_roots` and a frozen cell survives;
    /// omitting it, the same cell is reclaimed.
    pub fn gather_frozen_roots(&self) -> Vec<JsValue> {
        self.frozen_values
            .iter()
            .filter(|frozen| frozen.points_to_heap())
            .map(|frozen| frozen.value())
            .collect()
    }

    pub fn validate(&self) -> Result<(), DfgValidationError> {
        if self.blocks.is_empty() {
            return Err(DfgValidationError::GraphHasNoBlocks);
        }

        for (index, block) in self.blocks.iter().enumerate() {
            if self.blocks[index + 1..]
                .iter()
                .any(|other| other.id == block.id)
            {
                return Err(DfgValidationError::DuplicateBlockId(block.id));
            }
            block.validate(self)?;
        }

        for (index, node) in self.nodes.iter().enumerate() {
            if self.nodes[index + 1..]
                .iter()
                .any(|other| other.id == node.id)
            {
                return Err(DfgValidationError::DuplicateNodeId(node.id));
            }
            node.validate(self)?;
        }

        for (index, edge) in self.edges.iter().enumerate() {
            if self.edges[index + 1..]
                .iter()
                .any(|other| other.id == edge.id)
            {
                return Err(DfgValidationError::DuplicateEdgeId(edge.id));
            }
            edge.validate(self)?;
        }

        Ok(())
    }

    fn has_block(&self, id: DfgBasicBlockId) -> bool {
        self.blocks.iter().any(|block| block.id == id)
    }

    fn has_node(&self, id: DfgNodeId) -> bool {
        self.nodes.iter().any(|node| node.id == id)
    }

    fn has_edge(&self, id: DfgEdgeId) -> bool {
        self.edges.iter().any(|edge| edge.id == id)
    }

    fn block_by_id(&self, id: DfgBasicBlockId) -> Option<&DfgBasicBlock> {
        self.blocks.iter().find(|block| block.id == id)
    }

    fn node_by_id(&self, id: DfgNodeId) -> Option<&DfgNode> {
        self.nodes.iter().find(|node| node.id == id)
    }

    fn edge_by_id(&self, id: DfgEdgeId) -> Option<&DfgEdge> {
        self.edges.iter().find(|edge| edge.id == id)
    }

    pub fn reachable_blocks(
        &self,
        entry: DfgBasicBlockId,
    ) -> Result<Vec<DfgBasicBlockId>, DfgValidationError> {
        self.validate()?;
        if !self.has_block(entry) {
            return Err(DfgValidationError::MissingBlock(entry));
        }

        let mut reachable = Vec::new();
        let mut worklist = vec![entry];
        while let Some(block_id) = worklist.pop() {
            if reachable.contains(&block_id) {
                continue;
            }
            reachable.push(block_id);
            let block = self
                .block_by_id(block_id)
                .ok_or(DfgValidationError::MissingBlock(block_id))?;
            for successor in &block.successors {
                if let BranchTarget::Block(successor_id) = successor {
                    if !reachable.contains(successor_id) {
                        worklist.push(*successor_id);
                    }
                }
            }
        }

        Ok(reachable)
    }

    pub fn reverse_post_order(
        &self,
        entry: DfgBasicBlockId,
    ) -> Result<Vec<DfgBasicBlockId>, DfgValidationError> {
        self.validate()?;
        if !self.has_block(entry) {
            return Err(DfgValidationError::MissingBlock(entry));
        }

        let mut visited = Vec::new();
        let mut postorder = Vec::new();
        self.visit_block_postorder(entry, &mut visited, &mut postorder)?;
        postorder.reverse();
        Ok(postorder)
    }

    fn visit_block_postorder(
        &self,
        block_id: DfgBasicBlockId,
        visited: &mut Vec<DfgBasicBlockId>,
        postorder: &mut Vec<DfgBasicBlockId>,
    ) -> Result<(), DfgValidationError> {
        if visited.contains(&block_id) {
            return Ok(());
        }
        visited.push(block_id);
        let block = self
            .block_by_id(block_id)
            .ok_or(DfgValidationError::MissingBlock(block_id))?;
        for successor in &block.successors {
            if let BranchTarget::Block(successor_id) = successor {
                self.visit_block_postorder(*successor_id, visited, postorder)?;
            }
        }
        postorder.push(block_id);
        Ok(())
    }

    pub fn dominance_sets(
        &self,
        entry: DfgBasicBlockId,
    ) -> Result<Vec<DfgDominanceSet>, DfgValidationError> {
        self.validate()?;
        if !self.has_block(entry) {
            return Err(DfgValidationError::MissingBlock(entry));
        }

        let all_blocks: Vec<DfgBasicBlockId> = self.blocks.iter().map(|block| block.id).collect();
        let mut dominators: Vec<DfgDominanceSet> = all_blocks
            .iter()
            .map(|block_id| DfgDominanceSet {
                block: *block_id,
                dominators: if *block_id == entry {
                    vec![entry]
                } else {
                    all_blocks.clone()
                },
            })
            .collect();

        let mut changed = true;
        while changed {
            changed = false;
            for block in &self.blocks {
                if block.id == entry {
                    continue;
                }

                let mut next = all_blocks.clone();
                if block.predecessors.is_empty() {
                    next.clear();
                }
                for predecessor in &block.predecessors {
                    let predecessor_dominators = dominators
                        .iter()
                        .find(|set| set.block == *predecessor)
                        .ok_or(DfgValidationError::MissingBlock(*predecessor))?;
                    next.retain(|candidate| predecessor_dominators.dominators.contains(candidate));
                }
                if !next.contains(&block.id) {
                    next.push(block.id);
                }
                next.sort();

                let current = dominators
                    .iter_mut()
                    .find(|set| set.block == block.id)
                    .ok_or(DfgValidationError::MissingBlock(block.id))?;
                if current.dominators != next {
                    current.dominators = next;
                    changed = true;
                }
            }
        }

        Ok(dominators)
    }

    pub fn scheduled_nodes_in_block(
        &self,
        block_id: DfgBasicBlockId,
    ) -> Result<Vec<DfgNodeId>, DfgValidationError> {
        self.validate()?;
        let block = self
            .block_by_id(block_id)
            .ok_or(DfgValidationError::MissingBlock(block_id))?;
        let mut schedule = Vec::new();
        let mut visiting = Vec::new();
        for node in &block.nodes {
            self.schedule_node_in_block(*node, block, &mut visiting, &mut schedule)?;
        }
        Ok(schedule)
    }

    fn schedule_node_in_block(
        &self,
        node_id: DfgNodeId,
        block: &DfgBasicBlock,
        visiting: &mut Vec<DfgNodeId>,
        schedule: &mut Vec<DfgNodeId>,
    ) -> Result<(), DfgValidationError> {
        if schedule.contains(&node_id) {
            return Ok(());
        }
        if visiting.contains(&node_id) {
            return Err(DfgValidationError::NodeTypeNotAllowed(node_id));
        }
        visiting.push(node_id);

        let node = self
            .node_by_id(node_id)
            .ok_or(DfgValidationError::MissingNode(node_id))?;
        for edge_id in &node.children {
            let edge = self
                .edge_by_id(*edge_id)
                .ok_or(DfgValidationError::MissingEdge(*edge_id))?;
            if block.nodes.contains(&edge.to) {
                self.schedule_node_in_block(edge.to, block, visiting, schedule)?;
            }
        }

        visiting.retain(|visiting_node| *visiting_node != node_id);
        schedule.push(node_id);
        Ok(())
    }
}

impl DfgBasicBlock {
    /// `BasicBlock::findTerminal()` (dfg/DFGBasicBlock.h:94-114): scan
    /// backwards for the terminal, skipping the liveness-marking no-ops that
    /// legally sit after it — "most notably return blocks will have liveness
    /// markers for all of the flushed variables right after the return"
    /// (dfg/DFGBasicBlock.h:87-90). C++ skips Check/CheckVarargs/Phantom/
    /// PhantomLocal/Flush; the ported subset has no CheckVarargs yet.
    pub fn find_terminal<'a>(&self, graph: &'a DfgGraph) -> Option<&'a DfgNode> {
        for node_id in self.nodes.iter().rev() {
            let node = graph.nodes.iter().find(|node| node.id == *node_id)?;
            if node.is_terminal() || node.effects.terminates_block {
                return Some(node);
            }
            match node.op {
                NodeType::Check | NodeType::Phantom | NodeType::PhantomLocal | NodeType::Flush => {}
                _ => return None,
            }
        }
        None
    }

    pub fn validate(&self, graph: &DfgGraph) -> Result<(), DfgValidationError> {
        for node in &self.nodes {
            if !graph.has_node(*node) {
                return Err(DfgValidationError::MissingNode(*node));
            }
        }
        for predecessor in &self.predecessors {
            if !graph.has_block(*predecessor) {
                return Err(DfgValidationError::MissingBlock(*predecessor));
            }
        }
        for successor in &self.successors {
            if let BranchTarget::Block(block) = successor {
                if !graph.has_block(*block) {
                    return Err(DfgValidationError::BlockSuccessorMissing(*block));
                }
            }
        }
        if let (Some(begin), Some(end)) = (self.bytecode_begin, self.bytecode_end) {
            if begin > end {
                return Err(DfgValidationError::BlockBytecodeRangeInvalid(self.id));
            }
        }
        // A block with no successors must end in a terminal, possibly followed
        // by the liveness no-ops findTerminal skips (dfg/DFGBasicBlock.h:94-114).
        if self.successors.is_empty()
            && !self.nodes.is_empty()
            && self.find_terminal(graph).is_none()
        {
            return Err(DfgValidationError::TerminatorMismatch(self.id));
        }

        Ok(())
    }
}

impl DfgNode {
    pub fn validate(&self, graph: &DfgGraph) -> Result<(), DfgValidationError> {
        for edge in &self.children {
            if !graph.has_edge(*edge) {
                return Err(DfgValidationError::MissingEdge(*edge));
            }
            if graph
                .edges
                .iter()
                .find(|graph_edge| graph_edge.id == *edge)
                .is_some_and(|graph_edge| graph_edge.from != self.id)
            {
                return Err(DfgValidationError::EdgeNotOwnedBySourceNode(*edge));
            }
        }

        Ok(())
    }
}

impl DfgEdge {
    pub fn validate(&self, graph: &DfgGraph) -> Result<(), DfgValidationError> {
        if !graph.has_node(self.from) || !graph.has_node(self.to) {
            return Err(DfgValidationError::EdgeEndpointMissing(self.id));
        }

        Ok(())
    }
}

#[derive(Clone, Copy, Debug, Default, Eq, PartialEq, Hash)]
pub enum DfgPlanSchemaOwner {
    #[default]
    DfgPlanRegistry,
    DfgGraphOwner,
    FtlHandoffOwner,
}

#[derive(Clone, Copy, Debug, Default, Eq, PartialEq, Hash)]
pub enum DfgPlanRegistryMutationAuthority {
    #[default]
    GeneratedStaticDataRefresh,
    CrateInitialization,
}

/// Immutable DFG plan shape consumed by optimizing-tier descriptors.
///
/// This is schema data only. Graph building, phase execution, lowering, and
/// validation are owned by later compiler phases.
#[derive(Clone, Copy, Debug, Eq, PartialEq)]
pub struct StaticDfgPlanDescriptor {
    pub name: &'static str,
    pub target_tier: JitType,
    pub input_form: GraphForm,
    pub output_form: GraphForm,
    pub phases: &'static [DfgPhase],
    pub allowed_node_types: &'static [NodeType],
    pub mutation_authority: DfgGraphMutationAuthority,
    pub schema_owner: DfgPlanSchemaOwner,
    pub registry_authority: DfgPlanRegistryMutationAuthority,
    pub provenance: &'static str,
}

#[derive(Clone, Copy, Debug, Default, Eq, PartialEq)]
pub struct DfgPlanDescriptorRegistry {
    pub descriptors: &'static [StaticDfgPlanDescriptor],
}

impl DfgPlanDescriptorRegistry {
    pub const fn new(descriptors: &'static [StaticDfgPlanDescriptor]) -> Self {
        Self { descriptors }
    }

    pub const fn descriptors(self) -> &'static [StaticDfgPlanDescriptor] {
        self.descriptors
    }

    pub fn descriptor_for_name(self, name: &str) -> Option<&'static StaticDfgPlanDescriptor> {
        self.descriptors
            .iter()
            .find(|descriptor| descriptor.name == name)
    }

    pub fn validate(self) -> Result<(), DfgValidationError> {
        for (index, descriptor) in self.descriptors.iter().enumerate() {
            descriptor.validate()?;
            if self.descriptors[index + 1..]
                .iter()
                .any(|other| other.name == descriptor.name)
            {
                return Err(DfgValidationError::DuplicatePlanName(descriptor.name));
            }
        }

        Ok(())
    }
}

impl StaticDfgPlanDescriptor {
    pub fn validate(&self) -> Result<(), DfgValidationError> {
        if self.name.is_empty() {
            return Err(DfgValidationError::EmptyName);
        }
        if self.provenance.is_empty() {
            return Err(DfgValidationError::EmptyProvenance(self.name));
        }
        if self.phases.is_empty() {
            return Err(DfgValidationError::EmptyPhases(self.name));
        }
        if self.allowed_node_types.is_empty() {
            return Err(DfgValidationError::EmptyAllowedNodeTypes(self.name));
        }
        if self.input_form == self.output_form {
            return Err(DfgValidationError::GraphFormMismatch);
        }

        Ok(())
    }

    pub fn validate_graph(&self, graph: &DfgGraph) -> Result<(), DfgValidationError> {
        graph.validate()?;
        if graph.form != self.output_form {
            return Err(DfgValidationError::GraphFormMismatch);
        }
        for node in &graph.nodes {
            if !self.allowed_node_types.contains(&node.op) {
                return Err(DfgValidationError::NodeTypeNotAllowed(node.id));
            }
        }

        Ok(())
    }
}

const DFG_OPTIMIZATION_PHASES: &[DfgPhase] = &[
    DfgPhase::BytecodeParsing,
    DfgPhase::PredictionInjection,
    DfgPhase::Fixup,
    DfgPhase::Cfa,
    DfgPhase::SsaConversion,
    DfgPhase::Optimization,
    DfgPhase::LoweringPreparation,
];
const DFG_FTL_HANDOFF_PHASES: &[DfgPhase] = &[
    DfgPhase::Optimization,
    DfgPhase::LoweringPreparation,
    DfgPhase::FtlHandoff,
];
// The first-parser slice of faithful JSC node types (see node_type.rs for the
// per-op DFGNodeType.h citations).
const DFG_CORE_NODE_TYPES: &[NodeType] = &[
    NodeType::JSConstant,
    NodeType::DoubleConstant,
    NodeType::GetCallee,
    NodeType::GetLocal,
    NodeType::SetLocal,
    NodeType::MovHint,
    NodeType::ExitOK,
    NodeType::Phantom,
    NodeType::Check,
    NodeType::Upsilon,
    NodeType::Phi,
    NodeType::Flush,
    NodeType::PhantomLocal,
    NodeType::LoopHint,
    NodeType::SetArgumentDefinitely,
    NodeType::ArithAdd,
    NodeType::ArithSub,
    NodeType::ArithMul,
    NodeType::ValueAdd,
    NodeType::ValueSub,
    NodeType::ValueMul,
    NodeType::GetById,
    NodeType::PutById,
    NodeType::GetScope,
    NodeType::CompareLess,
    NodeType::Call,
    NodeType::Jump,
    NodeType::Branch,
    NodeType::Return,
    NodeType::Unreachable,
    NodeType::Throw,
    NodeType::ForceOSRExit,
    NodeType::CheckTraps,
];

pub const STATIC_DFG_PLAN_DESCRIPTORS: &[StaticDfgPlanDescriptor] = &[
    StaticDfgPlanDescriptor {
        name: "dfg-optimization-plan",
        target_tier: JitType::Dfg,
        // Parsing and CFG-rewriting phases run in LoadStore; the DFG backend
        // consumes ThreadedCPS (dfg/DFGCommon.h:160-202).
        input_form: GraphForm::LoadStore,
        output_form: GraphForm::ThreadedCps,
        phases: DFG_OPTIMIZATION_PHASES,
        allowed_node_types: DFG_CORE_NODE_TYPES,
        mutation_authority: DfgGraphMutationAuthority::GraphPhase,
        schema_owner: DfgPlanSchemaOwner::DfgGraphOwner,
        registry_authority: DfgPlanRegistryMutationAuthority::GeneratedStaticDataRefresh,
        provenance: "static Rust DFG plan schema",
    },
    StaticDfgPlanDescriptor {
        name: "dfg-to-ftl-handoff",
        target_tier: JitType::Ftl,
        // SSAConversionPhase converts ThreadedCPS to SSA, the FTL input form
        // (dfg/DFGCommon.h:203-205; DFGSSAConversionPhase.h).
        input_form: GraphForm::ThreadedCps,
        output_form: GraphForm::Ssa,
        phases: DFG_FTL_HANDOFF_PHASES,
        allowed_node_types: DFG_CORE_NODE_TYPES,
        mutation_authority: DfgGraphMutationAuthority::FtlLowering,
        schema_owner: DfgPlanSchemaOwner::FtlHandoffOwner,
        registry_authority: DfgPlanRegistryMutationAuthority::GeneratedStaticDataRefresh,
        provenance: "static Rust DFG plan schema",
    },
];

pub const DFG_PLAN_DESCRIPTOR_REGISTRY: DfgPlanDescriptorRegistry =
    DfgPlanDescriptorRegistry::new(STATIC_DFG_PLAN_DESCRIPTORS);

#[cfg(test)]
mod tests {
    use super::*;
    use crate::bytecode::speculated_type::{SPEC_INT32_ONLY, SPEC_NONE};
    use crate::gc::CellId;
    use crate::jit::{CodeOrigin, CodeOriginKind};

    fn origin() -> NodeOrigin {
        NodeOrigin {
            kind: NodeOriginKind::Synthetic,
            code_origin: CodeOrigin {
                kind: CodeOriginKind::DfgReplacement,
                owner: None,
                executable: None,
                bytecode_index: None,
            },
            owner: None,
            executable: None,
            bytecode_index: None,
            inline_depth: 0,
            exit_ok: false,
        }
    }

    fn block() -> DfgBasicBlock {
        DfgBasicBlock {
            id: DfgBasicBlockId(0),
            nodes: Vec::new(),
            predecessors: Vec::new(),
            successors: Vec::new(),
            variables_at_head: Operands::default(),
            variables_at_tail: Operands::default(),
            bytecode_begin: Some(0),
            bytecode_end: Some(0),
            execution_count: None,
            is_osr_entry: false,
            is_catch_entry: false,
        }
    }

    #[test]
    fn static_dfg_plan_registry_validates() {
        assert_eq!(DFG_PLAN_DESCRIPTOR_REGISTRY.validate(), Ok(()));
    }

    #[test]
    fn graph_starts_in_load_store_form_for_bytecode_parsing() {
        // DFGGraph.cpp:87: the Graph constructor starts in LoadStore form.
        let graph = DfgGraph::new(DfgGraphId(1), CodeBlockId(CellId(1)));
        assert_eq!(graph.form, GraphForm::LoadStore);
        assert_eq!(graph.phase, DfgPhase::BytecodeParsing);
        assert_eq!(
            graph.mutation_authority,
            DfgGraphMutationAuthority::BytecodeParser
        );
    }

    #[test]
    fn add_node_assigns_stable_index_identity() {
        let mut graph = DfgGraph::new(DfgGraphId(1), CodeBlockId(CellId(1)));
        let constant = graph.add_node(DfgNode::new(NodeType::JSConstant, origin()));
        let arith = graph.add_node(DfgNode::new(NodeType::ArithAdd, origin()));
        assert_eq!(constant, DfgNodeId(0));
        assert_eq!(arith, DfgNodeId(1));

        // Later appends do not disturb earlier identities.
        let ret = graph.add_node(DfgNode::new(NodeType::Return, origin()));
        assert_eq!(ret, DfgNodeId(2));
        assert_eq!(graph.nodes[constant.0 as usize].op, NodeType::JSConstant);
        assert_eq!(graph.nodes[arith.0 as usize].op, NodeType::ArithAdd);
        assert_eq!(graph.nodes[arith.0 as usize].id, arith);
    }

    #[test]
    fn node_result_bits_follow_default_flags() {
        // ArithAdd is NodeResultNumber (DFGNodeType.h:163) so
        // hasNumberResult() is true (DFGNode.h:1740-1743).
        let arith = DfgNode::new(NodeType::ArithAdd, origin());
        assert!(arith.has_number_result());
        assert!(!arith.has_js_result());
        assert!(arith.must_generate());

        // ValueAdd is NodeResultJS (DFGNodeType.h:192): a JS result, not a
        // number result.
        let value = DfgNode::new(NodeType::ValueAdd, origin());
        assert!(!value.has_number_result());
        assert!(value.has_js_result());

        // DoubleConstant is NodeResultDouble (DFGNodeType.h:40).
        let double_constant = DfgNode::new(NodeType::DoubleConstant, origin());
        assert!(double_constant.has_double_result());
        assert!(double_constant.has_result());
        assert!(!double_constant.has_number_or_any_int_result());

        // SetLocal has no result bits (DFGNodeType.h:75).
        let set_local = DfgNode::new(NodeType::SetLocal, origin());
        assert!(!set_local.has_result());
        assert!(!set_local.must_generate());

        // Call carries NodeHasVarArgs (DFGNodeType.h:400).
        assert!(DfgNode::new(NodeType::Call, origin()).has_var_args());
    }

    #[test]
    fn variable_access_data_arena_appends_and_merges() {
        // ByteCodeParser::newVariableAccessData appends to the graph-owned
        // arena (DFGByteCodeParser.cpp:368-372).
        let mut graph = DfgGraph::new(DfgGraphId(1), CodeBlockId(CellId(1)));
        let local = VirtualRegister::local(0);
        let argument = VirtualRegister::argument_or_header(5);
        let first = graph.new_variable_access_data(local);
        let second = graph.new_variable_access_data(argument);
        assert_eq!(first, DfgVariableAccessDataId(0));
        assert_eq!(second, DfgVariableAccessDataId(1));
        assert_eq!(
            graph.variable_access_data[first.0 as usize].operand(),
            local
        );
        assert_eq!(
            graph.variable_access_data[second.0 as usize].operand(),
            argument
        );

        // Fresh entries mirror the C++ constructor: SpecNone prediction and
        // zero flags (DFGVariableAccessData.cpp:47-59).
        assert_eq!(
            graph.variable_access_data[first.0 as usize].prediction(),
            SPEC_NONE
        );
        assert_eq!(graph.variable_access_data[first.0 as usize].flags(), 0);

        // The parser reuses one shared entry per variable by merging new
        // speculations into it.
        assert!(graph.variable_access_data[first.0 as usize].predict(SPEC_INT32_ONLY));
        assert!(!graph.variable_access_data[first.0 as usize].predict(SPEC_INT32_ONLY));
        assert_eq!(
            graph.variable_access_data[first.0 as usize].prediction(),
            SPEC_INT32_ONLY
        );
    }

    #[test]
    fn block_variables_at_tail_track_operand_to_node() {
        // The parser records the most recent local access per operand in
        // variablesAtTail (DFGBasicBlock.h:217).
        let mut graph = DfgGraph::new(DfgGraphId(1), CodeBlockId(CellId(1)));
        let mut set_local = DfgNode::new(NodeType::SetLocal, origin());
        set_local.variable_access_data =
            Some(graph.new_variable_access_data(VirtualRegister::local(0)));
        let set_local_id = graph.add_node(set_local);

        let mut entry = block();
        entry.variables_at_head = Operands::new(1, 1, 0);
        entry.variables_at_tail = Operands::new(1, 1, 0);
        *entry.variables_at_tail.local_mut(0) = Some(set_local_id);
        entry.nodes = vec![set_local_id];
        entry.successors = vec![BranchTarget::Fallthrough];
        let entry_id = graph.add_block(entry);

        let stored = *graph.blocks[entry_id.0 as usize].variables_at_tail.local(0);
        assert_eq!(stored, Some(set_local_id));
        assert_eq!(
            *graph.blocks[entry_id.0 as usize].variables_at_head.local(0),
            None
        );
    }

    #[test]
    fn graph_rejects_edge_to_missing_node() {
        let mut graph = DfgGraph::new(DfgGraphId(1), CodeBlockId(CellId(1)));
        let mut return_node = DfgNode::new(NodeType::Return, origin());
        return_node.effects.terminates_block = true;
        return_node.children = vec![DfgEdgeId(0)];
        let return_id = graph.add_node(return_node);
        graph.add_edge(DfgEdge {
            id: DfgEdgeId(0),
            from: return_id,
            to: DfgNodeId(9),
            child_index: 0,
            use_kind: EdgeUseKind::Untyped,
            proof: EdgeProofStatus::NeedsCheck,
            kill: EdgeKillStatus::DoesNotKill,
        });
        let mut entry = block();
        entry.nodes = vec![return_id];
        graph.add_block(entry);

        assert_eq!(
            graph.validate(),
            Err(DfgValidationError::EdgeEndpointMissing(DfgEdgeId(0)))
        );
    }

    #[test]
    fn graph_traversal_computes_reachability_and_dominance() {
        let mut graph = DfgGraph::new(DfgGraphId(2), CodeBlockId(CellId(2)));
        let branch = graph.add_node(DfgNode::new(NodeType::Branch, origin()));
        let mut return_node = DfgNode::new(NodeType::Return, origin());
        return_node.effects.terminates_block = true;
        let ret = graph.add_node(return_node);

        let mut entry = block();
        entry.nodes = vec![branch];
        entry.successors = vec![BranchTarget::Block(DfgBasicBlockId(1))];
        graph.add_block(entry);
        let mut exit = block();
        exit.nodes = vec![ret];
        exit.predecessors = vec![DfgBasicBlockId(0)];
        exit.bytecode_begin = Some(1);
        exit.bytecode_end = Some(1);
        graph.add_block(exit);

        assert_eq!(
            graph.reachable_blocks(DfgBasicBlockId(0)),
            Ok(vec![DfgBasicBlockId(0), DfgBasicBlockId(1)])
        );
        assert_eq!(
            graph.reverse_post_order(DfgBasicBlockId(0)),
            Ok(vec![DfgBasicBlockId(0), DfgBasicBlockId(1)])
        );
        assert_eq!(
            graph
                .dominance_sets(DfgBasicBlockId(0))
                .unwrap()
                .into_iter()
                .find(|set| set.block == DfgBasicBlockId(1))
                .map(|set| set.dominators),
            Some(vec![DfgBasicBlockId(0), DfgBasicBlockId(1)])
        );
    }

    #[test]
    fn node_scheduler_places_children_before_parent() {
        let mut graph = DfgGraph::new(DfgGraphId(3), CodeBlockId(CellId(3)));
        let child = graph.add_node(DfgNode::new(NodeType::SetArgumentDefinitely, origin()));
        let mut return_node = DfgNode::new(NodeType::Return, origin());
        return_node.effects.terminates_block = true;
        return_node.children = vec![DfgEdgeId(0)];
        let parent = graph.add_node(return_node);
        graph.add_edge(DfgEdge {
            id: DfgEdgeId(0),
            from: parent,
            to: child,
            child_index: 0,
            use_kind: EdgeUseKind::Untyped,
            proof: EdgeProofStatus::NeedsCheck,
            kill: EdgeKillStatus::DoesNotKill,
        });
        let mut entry = block();
        entry.nodes = vec![child, parent];
        graph.add_block(entry);

        assert_eq!(
            graph.scheduled_nodes_in_block(DfgBasicBlockId(0)),
            Ok(vec![child, parent])
        );
    }

    #[test]
    fn node_effect_summary_marks_exiting_checks_as_ordered() {
        let effects = NodeEffects {
            may_exit: true,
            ..NodeEffects::default()
        };

        assert!(effects.effect_summary().must_preserve_order());
        assert!(!effects.is_observably_pure());
    }

    // ---- FrozenValue keep-alive unit (dfg/frozen_value.rs; GC-audit hard
    // blocker #1 / divergence #3): `Graph::freeze`/`freezeStrong` dedup
    // (DFGGraph.cpp:1633-1664) and the `visitChildren`-analog keep-alive
    // (`gather_frozen_roots`, DFGGraph.cpp:1621-1628). ----

    use crate::interpreter::CoreOpcodeDispatchHost;

    #[test]
    fn freeze_dedups_by_encoded_value() {
        // DFGGraph.cpp:1645-1647: `m_frozenValueMap.add(...)`; a repeated
        // freeze of the identical encoded value returns the SAME
        // `FrozenValue*` (here: the same `FrozenValueId`) instead of growing
        // the arena.
        let mut graph = DfgGraph::new(DfgGraphId(20), CodeBlockId(CellId(20)));
        let a = graph.freeze(JsValue::from_i32(42), None);
        let b = graph.freeze(JsValue::from_i32(42), None);
        let c = graph.freeze(JsValue::from_i32(43), None);

        assert_eq!(
            a, b,
            "freezing the same encoded value twice dedups to one id"
        );
        assert_ne!(a, c, "a different value gets a different id");
        assert_eq!(
            graph.frozen_values.len(),
            2,
            "the arena holds exactly one entry per DISTINCT value"
        );
    }

    #[test]
    fn freeze_strong_upgrades_the_deduped_entrys_strength_in_place() {
        // DFGGraph.cpp:1659-1664: `freezeStrong` is `freeze(value)->strengthenTo(StrongValue)`
        // — it does not allocate a second arena entry. Uses a CELL value: per
        // `strengthenTo`'s `isCell()` guard (DFGFrozenValue.h:88-92), a
        // non-cell value's strength never actually upgrades (see
        // `strengthen_to_is_a_no_op_for_non_cell_values` in frozen_value.rs),
        // so this test must exercise a cell to prove the upgrade happens.
        let mut graph = DfgGraph::new(DfgGraphId(21), CodeBlockId(CellId(21)));
        let cell_like = JsValue::from_encoded(crate::value::EncodedJsValue(0x3_0000_0020));
        assert!(
            cell_like.is_cell(),
            "fixture must actually decode as a cell"
        );

        let weak_id = graph.freeze(cell_like, Some(StructureId::new(1)));
        assert_eq!(
            graph.frozen_value(weak_id).strength(),
            ValueStrength::WeakValue
        );

        let strong_id = graph.freeze_strong(cell_like, Some(StructureId::new(1)));
        assert_eq!(
            strong_id, weak_id,
            "freeze_strong dedups onto the same arena entry"
        );
        assert_eq!(
            graph.frozen_value(weak_id).strength(),
            ValueStrength::StrongValue,
            "the existing entry's strength is upgraded in place"
        );
        assert_eq!(graph.frozen_values.len(), 1);
    }

    #[test]
    fn gather_frozen_roots_includes_only_cell_valued_entries() {
        // The root set only ever needs to protect HEAP values;
        // DFGFrozenValue.h:94's `pointsToHeap()` gate is exactly what filters
        // non-cell immediates out of `visitChildren`'s (and this method's)
        // output.
        let mut graph = DfgGraph::new(DfgGraphId(22), CodeBlockId(CellId(22)));
        let immediate = JsValue::from_i32(9);
        let cell_like = JsValue::from_encoded(crate::value::EncodedJsValue(0x2_0000_0020));
        assert!(
            cell_like.is_cell(),
            "fixture must actually decode as a cell"
        );

        graph.freeze(immediate, None);
        graph.freeze_strong(cell_like, Some(StructureId::new(1)));

        assert_eq!(graph.gather_frozen_roots(), vec![cell_like]);
    }

    #[test]
    fn frozen_string_cell_survives_collection_when_graph_roots_are_folded_in() {
        // LOAD-BEARING: this is the keep-alive claim itself. A string cell
        // referenced ONLY by a live DfgGraph's frozen arena (no register/
        // stack/lexical binding) must survive a real collection when the
        // graph's `gather_frozen_roots()` output is folded into the
        // collection's root set — the production `host_roots` fold this unit
        // could not wire (see `gather_frozen_roots`'s doc: no live-plan
        // registry exists yet). The companion test right below proves the
        // negative: the identical setup, minus the fold, reclaims the cell —
        // so survival here comes from the fold, not from some other
        // accidental root.
        let mut host = CoreOpcodeDispatchHost::default();
        let folded = host.allocate_untracked_string_for_test("dfg-frozen-constant");

        let mut graph = DfgGraph::new(DfgGraphId(23), CodeBlockId(CellId(23)));
        // Leaf string cells carry no `StructureId` in this crate yet (see
        // `FrozenValue`'s struct doc) — `None` is the faithful adaptation,
        // not a shortcut.
        graph.freeze_strong(folded, None);

        let roots = graph.gather_frozen_roots();
        assert_eq!(roots, vec![folded]);
        host.force_collect_values_for_test(&roots);

        assert!(
            host.is_value_marked_for_test(folded),
            "the frozen constant is reachable from the folded root"
        );
        assert_eq!(
            host.string_text_for_test(folded),
            Some("dfg-frozen-constant"),
            "the frozen constant reads back intact (the UAF read)"
        );
    }

    #[test]
    fn unfrozen_string_cell_is_reclaimed_without_the_root_fold() {
        // Negative half of the load-bearing proof above: the SAME setup, but
        // the collection runs with the frozen graph's roots deliberately NOT
        // folded in (as if `gather_frozen_roots` were never wired). The cell
        // — referenced by nothing else — is swept.
        let mut host = CoreOpcodeDispatchHost::default();
        let unrooted = host.allocate_untracked_string_for_test("dfg-unrooted-constant");

        let mut graph = DfgGraph::new(DfgGraphId(24), CodeBlockId(CellId(24)));
        graph.freeze_strong(unrooted, None); // frozen in the graph, but not folded into roots below

        host.force_collect_values_for_test(&[]);

        assert!(
            !host.is_value_marked_for_test(unrooted),
            "with the frozen-root fold omitted, nothing keeps this cell alive"
        );
        assert_eq!(
            host.string_text_for_test(unrooted),
            None,
            "the unrooted cell is swept + its slot reconciled"
        );
    }

    #[test]
    fn frozen_cell_constant_coexists_with_a_parser_produced_graph() {
        // The bytecode parser cannot yet PRODUCE a cell-valued constant node
        // itself: no linked constant pool carries a cell today (the
        // CodeBlock constant-pool root-fold commit f213265 found "NO
        // cell-valued constant reaches any real pool today" — string
        // literals materialize into registers via LoadString, not a pool
        // cell). This freezes one directly into a REAL parser-produced graph
        // instead, proving the frozen arena composes cleanly with parsing
        // output rather than only a freshly-`DfgGraph::new()`'d empty graph.
        use crate::bytecode::code_block::{
            CodeBlock, CodeKind, LinkContext, UnlinkedCodeBlock, UnlinkedConstantPool,
        };
        use crate::bytecode::instruction_stream::{
            opcode_id, InstructionStreamWriter, OperandValue,
        };
        use crate::bytecode::register::{RegisterFrameShape, SpecialRegisters};
        use crate::bytecode::PackedInstructionStream;

        const THIS_OFFSET: i32 = 5; // CallFrameSlot::thisArgument (CallFrame.h).
        let argument_register = |argument: u32| THIS_OFFSET + argument as i32;

        // `f(a) { return a; }` — the same identity-function fixture used by
        // parser.rs's and plan.rs's own tests.
        let mut writer = InstructionStreamWriter::new();
        writer.emit(opcode_id::ENTER, &[]);
        writer.emit(
            opcode_id::RET,
            &[OperandValue::VirtualRegister(argument_register(1))],
        );
        let bytes = writer.finalize().bytes().to_vec();

        let unlinked = UnlinkedCodeBlock::new(
            CodeKind::Function,
            PackedInstructionStream::from_raw_packed_bytes(bytes),
        )
        .with_frame(RegisterFrameShape {
            num_parameters_including_this: 2,
            num_vars: 1,
            num_callee_locals: 1 + 8,
            num_temporaries: 0,
            special: SpecialRegisters {
                scope_register: VirtualRegister::local(0),
                ..SpecialRegisters::default()
            },
        })
        .with_constants(UnlinkedConstantPool::default());
        let code_block = CodeBlock::from_unlinked(unlinked, LinkContext::default());

        let mut graph = crate::dfg::parser::parse(&code_block).expect("identity function parses");
        assert_eq!(graph.form, GraphForm::LoadStore);

        let mut host = CoreOpcodeDispatchHost::default();
        let cell = host.allocate_untracked_string_for_test("parser-graph-frozen-constant");
        let id = graph.freeze_strong(cell, None);

        assert_eq!(graph.frozen_value(id).value(), cell);
        assert_eq!(
            graph.frozen_value(id).strength(),
            ValueStrength::StrongValue
        );
        assert_eq!(graph.gather_frozen_roots(), vec![cell]);
    }
}
