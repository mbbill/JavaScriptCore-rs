//! DFG graph, node, and edge descriptors.
//!
//! These structures mirror the ownership boundaries of JSC's DFG graph. They
//! describe graph shape, origin tracking, effects, and typed child edges without
//! parsing bytecode, optimizing, lowering, or executing code.

use crate::bytecode::{Opcode, VirtualRegister};
use crate::gc::StructureId;
use crate::jit::{CodeOrigin, EffectSummary, JitCodeId, JitType, WatchpointDependency};
use crate::object::PropertyOffset;
use crate::runtime::{CodeBlockId, ExecutableId, ObjectId};
use crate::strings::AtomId;

/// Stable identity for a graph produced for one compilation plan.
#[derive(Clone, Copy, Debug, Eq, Hash, Ord, PartialEq, PartialOrd)]
pub struct DfgGraphId(pub u64);

/// Stable identity for a node in a graph.
#[derive(Clone, Copy, Debug, Eq, Hash, Ord, PartialEq, PartialOrd)]
pub struct DfgNodeId(pub u32);

/// Stable identity for a basic block in a DFG graph.
///
/// This is graph-local block identity, not bytecode source identity. It is
/// valid only for snapshots with the same `DfgGraphId` generation.
#[derive(Clone, Copy, Debug, Eq, Hash, Ord, PartialEq, PartialOrd)]
pub struct DfgBasicBlockId(pub u32);

/// Stable identity for a child edge. Varargs children can use IDs beyond the
/// fixed child slots.
#[derive(Clone, Copy, Debug, Eq, Hash, Ord, PartialEq, PartialOrd)]
pub struct DfgEdgeId(pub u32);

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
    pub variable_access_data: Vec<DfgVariableAccessDataId>,
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
    pub id: DfgBasicBlockId,
    pub nodes: Vec<DfgNodeId>,
    pub predecessors: Vec<DfgBasicBlockId>,
    pub successors: Vec<BranchTarget>,
    pub bytecode_begin: Option<u32>,
    pub bytecode_end: Option<u32>,
    pub execution_count: Option<u64>,
    pub is_osr_entry: bool,
    pub is_catch_entry: bool,
}

/// Complete graph snapshot reserved for a DFG or FTL compilation plan.
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
    EmptyAllowedNodeKinds(&'static str),
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
    NodeKindNotAllowed(DfgNodeId),
    GraphFormMismatch,
}

#[derive(Clone, Debug, Eq, PartialEq)]
pub struct DfgGraphBuilder {
    graph: DfgGraph,
}

impl DfgGraphBuilder {
    pub fn new(id: DfgGraphId, owner: CodeBlockId) -> Self {
        Self {
            graph: DfgGraph {
                id,
                owner,
                executable: None,
                phase: DfgPhase::BytecodeParsing,
                form: GraphForm::Cps,
                root_code: None,
                blocks: Vec::new(),
                nodes: Vec::new(),
                edges: Vec::new(),
                watchpoints: Vec::new(),
                common_data: DfgCommonDataDescriptor::default(),
                generation: 0,
                mutation_authority: DfgGraphMutationAuthority::BytecodeParser,
            },
        }
    }

    pub fn executable(mut self, executable: ExecutableId) -> Self {
        self.graph.executable = Some(executable);
        self
    }

    pub fn phase(mut self, phase: DfgPhase) -> Self {
        self.graph.phase = phase;
        self
    }

    pub fn form(mut self, form: GraphForm) -> Self {
        self.graph.form = form;
        self
    }

    pub fn authority(mut self, authority: DfgGraphMutationAuthority) -> Self {
        self.graph.mutation_authority = authority;
        self
    }

    pub fn block(mut self, block: DfgBasicBlock) -> Self {
        self.graph.blocks.push(block);
        self
    }

    pub fn node(mut self, node: DfgNode) -> Self {
        self.graph.nodes.push(node);
        self
    }

    pub fn edge(mut self, edge: DfgEdge) -> Self {
        self.graph.edges.push(edge);
        self
    }

    pub fn build(self) -> Result<DfgGraph, DfgValidationError> {
        self.graph.validate()?;
        Ok(self.graph)
    }
}

impl DfgGraph {
    pub fn builder(id: DfgGraphId, owner: CodeBlockId) -> DfgGraphBuilder {
        DfgGraphBuilder::new(id, owner)
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
            return Err(DfgValidationError::NodeKindNotAllowed(node_id));
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
    pub allowed_node_kinds: &'static [DfgNodeKind],
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
        if self.allowed_node_kinds.is_empty() {
            return Err(DfgValidationError::EmptyAllowedNodeKinds(self.name));
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
            if !self.allowed_node_kinds.contains(&node.kind) {
                return Err(DfgValidationError::NodeKindNotAllowed(node.id));
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
const DFG_CORE_NODE_KINDS: &[DfgNodeKind] = &[
    DfgNodeKind::Argument,
    DfgNodeKind::GetLocal,
    DfgNodeKind::SetLocal,
    DfgNodeKind::Phi,
    DfgNodeKind::Check,
    DfgNodeKind::Arith,
    DfgNodeKind::Compare,
    DfgNodeKind::Branch,
    DfgNodeKind::Call,
    DfgNodeKind::GetById,
    DfgNodeKind::PutById,
    DfgNodeKind::OsrEntry,
    DfgNodeKind::OsrExit,
    DfgNodeKind::Return,
    DfgNodeKind::Throw,
];

pub const STATIC_DFG_PLAN_DESCRIPTORS: &[StaticDfgPlanDescriptor] = &[
    StaticDfgPlanDescriptor {
        name: "dfg-optimization-plan",
        target_tier: JitType::Dfg,
        input_form: GraphForm::Cps,
        output_form: GraphForm::LoweredForDfgJit,
        phases: DFG_OPTIMIZATION_PHASES,
        allowed_node_kinds: DFG_CORE_NODE_KINDS,
        mutation_authority: DfgGraphMutationAuthority::GraphPhase,
        schema_owner: DfgPlanSchemaOwner::DfgGraphOwner,
        registry_authority: DfgPlanRegistryMutationAuthority::GeneratedStaticDataRefresh,
        provenance: "static Rust DFG plan schema",
    },
    StaticDfgPlanDescriptor {
        name: "dfg-to-ftl-handoff",
        target_tier: JitType::Ftl,
        input_form: GraphForm::Ssa,
        output_form: GraphForm::LoweredForFtl,
        phases: DFG_FTL_HANDOFF_PHASES,
        allowed_node_kinds: DFG_CORE_NODE_KINDS,
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

    #[test]
    fn static_dfg_plan_registry_validates() {
        assert_eq!(DFG_PLAN_DESCRIPTOR_REGISTRY.validate(), Ok(()));
    }

    #[test]
    fn graph_builder_rejects_edge_to_missing_node() {
        let node = DfgNode {
            id: DfgNodeId(0),
            kind: DfgNodeKind::Return,
            origin: origin(),
            result: DfgValueRep::Void,
            children: vec![DfgEdgeId(0)],
            effects: NodeEffects {
                terminates_block: true,
                ..NodeEffects::default()
            },
            virtual_register: None,
            structure: None,
            property: None,
            property_offset: None,
            object: None,
            watchpoints: Vec::new(),
        };
        let edge = DfgEdge {
            id: DfgEdgeId(0),
            from: DfgNodeId(0),
            to: DfgNodeId(9),
            child_index: 0,
            use_kind: EdgeUseKind::Untyped,
            proof: EdgeProofStatus::NeedsCheck,
            kill: EdgeKillStatus::DoesNotKill,
        };
        let block = DfgBasicBlock {
            id: DfgBasicBlockId(0),
            nodes: vec![DfgNodeId(0)],
            predecessors: Vec::new(),
            successors: Vec::new(),
            bytecode_begin: Some(0),
            bytecode_end: Some(0),
            execution_count: None,
            is_osr_entry: false,
            is_catch_entry: false,
        };

        let graph = DfgGraph::builder(DfgGraphId(1), CodeBlockId(CellId(1)))
            .block(block)
            .node(node)
            .edge(edge)
            .build();

        assert_eq!(
            graph,
            Err(DfgValidationError::EdgeEndpointMissing(DfgEdgeId(0)))
        );
    }

    #[test]
    fn graph_traversal_computes_reachability_and_dominance() {
        let entry_node = DfgNode {
            id: DfgNodeId(0),
            kind: DfgNodeKind::Branch,
            origin: origin(),
            result: DfgValueRep::Void,
            children: Vec::new(),
            effects: NodeEffects::default(),
            virtual_register: None,
            structure: None,
            property: None,
            property_offset: None,
            object: None,
            watchpoints: Vec::new(),
        };
        let return_node = DfgNode {
            id: DfgNodeId(1),
            kind: DfgNodeKind::Return,
            origin: origin(),
            result: DfgValueRep::Void,
            children: Vec::new(),
            effects: NodeEffects {
                terminates_block: true,
                ..NodeEffects::default()
            },
            virtual_register: None,
            structure: None,
            property: None,
            property_offset: None,
            object: None,
            watchpoints: Vec::new(),
        };
        let entry = DfgBasicBlock {
            id: DfgBasicBlockId(0),
            nodes: vec![DfgNodeId(0)],
            predecessors: Vec::new(),
            successors: vec![BranchTarget::Block(DfgBasicBlockId(1))],
            bytecode_begin: Some(0),
            bytecode_end: Some(0),
            execution_count: None,
            is_osr_entry: false,
            is_catch_entry: false,
        };
        let exit = DfgBasicBlock {
            id: DfgBasicBlockId(1),
            nodes: vec![DfgNodeId(1)],
            predecessors: vec![DfgBasicBlockId(0)],
            successors: Vec::new(),
            bytecode_begin: Some(1),
            bytecode_end: Some(1),
            execution_count: None,
            is_osr_entry: false,
            is_catch_entry: false,
        };
        let graph = DfgGraph::builder(DfgGraphId(2), CodeBlockId(CellId(2)))
            .block(entry)
            .block(exit)
            .node(entry_node)
            .node(return_node)
            .build()
            .unwrap();

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
        let child = DfgNode {
            id: DfgNodeId(0),
            kind: DfgNodeKind::Argument,
            origin: origin(),
            result: DfgValueRep::Untyped,
            children: Vec::new(),
            effects: NodeEffects::default(),
            virtual_register: None,
            structure: None,
            property: None,
            property_offset: None,
            object: None,
            watchpoints: Vec::new(),
        };
        let parent = DfgNode {
            id: DfgNodeId(1),
            kind: DfgNodeKind::Return,
            origin: origin(),
            result: DfgValueRep::Void,
            children: vec![DfgEdgeId(0)],
            effects: NodeEffects {
                terminates_block: true,
                ..NodeEffects::default()
            },
            virtual_register: None,
            structure: None,
            property: None,
            property_offset: None,
            object: None,
            watchpoints: Vec::new(),
        };
        let edge = DfgEdge {
            id: DfgEdgeId(0),
            from: DfgNodeId(1),
            to: DfgNodeId(0),
            child_index: 0,
            use_kind: EdgeUseKind::Untyped,
            proof: EdgeProofStatus::NeedsCheck,
            kill: EdgeKillStatus::DoesNotKill,
        };
        let block = DfgBasicBlock {
            id: DfgBasicBlockId(0),
            nodes: vec![DfgNodeId(0), DfgNodeId(1)],
            predecessors: Vec::new(),
            successors: Vec::new(),
            bytecode_begin: Some(0),
            bytecode_end: Some(0),
            execution_count: None,
            is_osr_entry: false,
            is_catch_entry: false,
        };
        let graph = DfgGraph::builder(DfgGraphId(3), CodeBlockId(CellId(3)))
            .block(block)
            .node(child)
            .node(parent)
            .edge(edge)
            .build()
            .unwrap();

        assert_eq!(
            graph.scheduled_nodes_in_block(DfgBasicBlockId(0)),
            Ok(vec![DfgNodeId(0), DfgNodeId(1)])
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
