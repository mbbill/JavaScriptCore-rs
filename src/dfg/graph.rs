//! DFG graph, node, and edge descriptors.
//!
//! These structures mirror the ownership boundaries of JSC's DFG graph
//! (`dfg/DFGGraph.h`): the graph owns its blocks, nodes, edges, and the
//! `VariableAccessData` arena; nodes carry a faithful `NodeType` opcode plus
//! `NodeFlags` (result bits included) as `DFG::Node` does. The bytecode parser
//! (a later unit) owns a mutable working graph and appends through the `&mut`
//! APIs below, exactly as `ByteCodeParser` mutates `Graph&`.

use crate::bytecode::{Operands, VirtualRegister};
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
#[derive(Clone, Copy, Debug, Eq, PartialEq)]
pub struct NodeOrigin {
    pub kind: NodeOriginKind,
    pub code_origin: CodeOrigin,
    pub owner: Option<CodeBlockId>,
    pub executable: Option<ExecutableId>,
    pub bytecode_index: Option<u32>,
    pub inline_depth: u16,
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
        if self.successors.is_empty()
            && self
                .nodes
                .last()
                .and_then(|node_id| graph.nodes.iter().find(|node| node.id == *node_id))
                .is_some_and(|node| !node.effects.terminates_block)
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
    NodeType::GetLocal,
    NodeType::SetLocal,
    NodeType::MovHint,
    NodeType::ExitOK,
    NodeType::Phantom,
    NodeType::Check,
    NodeType::Upsilon,
    NodeType::Phi,
    NodeType::Flush,
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
}
