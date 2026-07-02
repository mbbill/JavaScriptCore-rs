//! Bytecode → DFG graph construction: the first `ByteCodeParser` slice.
//!
//! Faithful port of C++ `dfg/DFGByteCodeParser.cpp` scoped exactly the way
//! JSC's own first DFG was scoped: a SINGLE basic block, non-speculative,
//! graph construction only — the 2011 DFG "deliberately accepted only
//! single-basic-block functions and fell back to the existing JIT" for
//! everything else (mcts_mem/javascriptcore/dfg.md, 2011-03-14 08a80d90). A
//! body outside the ratified `op_enter {op_mov|op_add|op_sub|op_mul}* op_ret`
//! shape DECLINES with no partial graph, mirroring the DFG's admission gate
//! (dfg/DFGCapabilities.h). This boundary is load-bearing: every
//! profile-consuming opcode reaches `getPrediction()`, and with empty value
//! profiles `getPrediction() == SpecNone` plants ForceOSRExit
//! (DFGByteCodeParser.cpp:1050-1061) — out of slice by design.
//!
//! The produced graph is ONE LoadStore-form block (`m_form(LoadStore)`,
//! dfg/DFGGraph.cpp:87): implicit data flow through GetLocal/SetLocal with
//! block-local reuse via `variablesAtTail`, and NO Phi threading — Phis belong
//! to CPSRethreadingPhase, the first phase after parsing (dfg/DFGPlan.cpp:237;
//! mcts_mem/javascriptcore/dfg.alt/dfg-phi-threading-in-bytecode-parser.md).
//! Parsing stops where `ByteCodeParser::parse` hands off to the analysis
//! phases (DFGByteCodeParser.cpp:12649-12661): backwards propagation,
//! ForceOSRExit pruning, and the numLocals/parameterSlots handoff land with
//! the phase ports.

use crate::bytecode::code_block::{BytecodeIndex, CodeBlock, CodeKind, SourceCodeRepresentation};
use crate::bytecode::instruction_stream::{decode_raw_instruction, opcode_id};
use crate::bytecode::register::{CallFrameSlotLayout, ThisArgumentOffset, VirtualRegister};
use crate::bytecode::speculated_type::SPEC_NONE;
use crate::bytecode::{ConstantValue, Operands};
use crate::dfg::graph::{
    DfgBasicBlock, DfgBasicBlockId, DfgEdge, DfgEdgeId, DfgGraph, DfgGraphId, DfgNode, DfgNodeId,
    DfgVariableAccessDataId, EdgeKillStatus, EdgeProofStatus, EdgeUseKind, NodeOrigin,
    NodeOriginKind,
};
use crate::dfg::node_flags::{
    NODE_MAY_HAVE_BIG_INT32_RESULT, NODE_MAY_HAVE_DOUBLE_RESULT, NODE_MAY_HAVE_HEAP_BIG_INT_RESULT,
    NODE_MAY_HAVE_NON_NUMERIC_RESULT, NODE_MAY_NEG_ZERO_IN_BASELINE,
    NODE_MAY_OVERFLOW_INT32_IN_BASELINE, NODE_MAY_OVERFLOW_INT52,
};
use crate::dfg::node_type::NodeType;
use crate::gc::CellId;
use crate::jit::{CodeOrigin, CodeOriginKind};
use crate::runtime::CodeBlockId;
use crate::value::{JsValue, NumberValue};

/// `CallFrameSlot::callee` (interpreter/CallFrame.h:178: slot 3 = 2 header
/// registers of CallerFrameAndPC + 1). The same constant the baseline frame
/// port uses (jit/abi.rs "callee @ slot 3").
const CALL_FRAME_SLOT_CALLEE: i32 = 3;

/// Why the parser refused to build a graph for this CodeBlock.
///
/// C++ has no return channel like this — unsupported CodeBlocks never reach
/// the parser because `DFG::capabilityLevel` filters them (dfg/DFGCapabilities.h),
/// and the first DFG fell back to the baseline JIT the same way
/// (mcts_mem/javascriptcore/dfg.md, 2011-03-14 08a80d90). Declining returns
/// NO partial graph.
#[derive(Clone, Copy, Debug, Eq, PartialEq)]
pub enum DeclineReason {
    /// The linked CodeBlock has no raw packed byte stream (typed-placeholder
    /// or schema-staged representation). C++ cannot hit this — every CodeBlock
    /// owns a packed `InstructionStream`; the live packed cutover removes this
    /// case.
    NoRawPackedInstructionStream,
    /// The packed stream failed to decode at `offset` (also covers opcodes the
    /// declared Rust opcode table does not know yet, e.g. `op_get_by_id`).
    InstructionDecode { offset: usize },
    /// A decoded opcode outside the ratified first-slice set.
    OutOfSliceOpcode { opcode_id: u8, offset: usize },
    /// Function bytecode always begins with `op_enter`; anything else is out
    /// of slice shape.
    BodyDoesNotStartWithEnter,
    /// The body must terminate with `op_ret` (single block, no jumps).
    BodyDoesNotEndWithRet,
    /// Bytecode continues after `op_ret`: a second (unreachable or jump-target)
    /// block would be needed — multi-block is out of slice.
    UnreachableCodeAfterRet { offset: usize },
    /// EvalCode lowers `op_enter` through GetEvalScope
    /// (DFGByteCodeParser.cpp:7554-7557) — out of slice.
    EvalCodeOutOfSlice,
    /// C++ CodeBlocks always carry a valid `scopeRegister` stamped by the
    /// bytecode generator; the Rust generator does not stamp one yet, so a
    /// block without it declines rather than inventing a lowering. The live
    /// cutover removes this case.
    MissingScopeRegister,
    /// A constant operand pointed outside the linked constant pool. C++
    /// `getConstant` cannot miss; safe Rust declines instead of asserting.
    ConstantIndexOutOfRange { index: usize },
    /// An operand outside the local/argument/constant namespaces this slice
    /// supports (e.g. a call-frame header slot other than the callee). C++
    /// ASSERTs in `getDirect`/`setDirect`; safe Rust declines.
    UnsupportedOperand { raw: i32 },
}

/// `enum SetMode` (DFGByteCodeParser.cpp:434-452).
#[derive(Clone, Copy, Debug, Eq, PartialEq)]
enum SetMode {
    /// Two-phase commit spanning code origins: MovHint now, SetLocal at the
    /// start of the next code origin (DFGByteCodeParser.cpp:435-439).
    NormalSet,
    /// SetLocal happens immediately and we do not Flush it even if the local
    /// is marked as needing it — used when initializing locals at the top of a
    /// function (DFGByteCodeParser.cpp:448-451).
    ImmediateNakedSet,
    // C++ also has ImmediateSetWithFlush (:441-446); no opcode in this slice
    // uses it, so it lands with its first consumer (op_ret's inlined-return
    // arm, :9348).
}

/// `struct DelayedSetLocal` (DFGByteCodeParser.cpp:1376-1398).
#[derive(Clone, Copy, Debug)]
struct DelayedSetLocal {
    /// `m_origin` (:1394): the SEMANTIC origin captured when the set was
    /// queued, restored around the SetLocal via SetForScope (:528, :586).
    origin: BytecodeIndex,
    operand: VirtualRegister,
    value: DfgNodeId,
    set_mode: SetMode,
}

/// `class ByteCodeParser` (DFGByteCodeParser.cpp:127).
///
/// The single-CodeBlock slice has no `InlineStackEntry` stack: the one
/// implicit entry is the machine code block itself (`InlineStackEntry` with a
/// null `InlineCallFrame`, DFGByteCodeParser.cpp:12640-12642), so
/// `remapOperand` is the identity and the inline-frame branches are statically
/// dead. The stack lands with the inlining port.
struct ByteCodeParser<'a, 'g> {
    /// `m_codeBlock` / `m_profiledBlock` (:131-132): one linked block serves
    /// both roles until DFG plans carry a baseline alternative.
    code_block: &'a CodeBlock,
    /// `Graph& m_graph` (DFGByteCodeParser.cpp:133): C++'s `ByteCodeParser`
    /// BORROWS the plan-owned `Graph`, it does not construct or own one.
    /// `dfg::plan::DfgPlan` is the owner now (`DfgPlan::compile_parse_only`);
    /// this mirrors that by borrowing instead of holding a `DfgGraph` value.
    graph: &'g mut DfgGraph,
    /// `m_currentBlock` (:1262).
    current_block: DfgBasicBlockId,
    /// `m_currentIndex` (:1264).
    current_index: BytecodeIndex,
    /// `m_currentSemanticOrigin` (:1266).
    current_semantic_origin: Option<BytecodeIndex>,
    /// `m_exitOK` (:1270).
    exit_ok: bool,
    /// `m_constants` (:1276): per-constant-index node dedup cache.
    constants: Vec<Option<DfgNodeId>>,
    /// `m_setLocalQueue` (:1400).
    set_local_queue: Vec<DelayedSetLocal>,
    /// `m_numArguments` = `codeBlock->numParameters()` (ctor :140).
    num_arguments: usize,
    /// `m_numLocals` = `codeBlock->numCalleeLocals()` (ctor :141). Consumed
    /// (beyond sizing the block's Operands) by the `m_graph.m_localVars`
    /// handoff at the end of parse (:12675-12676), which lands with the phase
    /// ports.
    #[allow(dead_code)]
    num_locals: usize,
    /// `m_numTmps` = `codeBlock->numTmps()` (ctor :142); the Rust CodeBlock
    /// has no checkpoint-tmp surface yet, so this is 0. Kept for the
    /// `m_graph.m_tmps` handoff (:12675), which lands with the phase ports.
    #[allow(dead_code)]
    num_tmps: usize,
    /// C++ `VirtualRegister::toArgument()` bakes `CallFrame::thisArgumentOffset()`
    /// in as a build constant; the Rust register namespace threads it
    /// explicitly (bytecode/register.rs). This is the JSC layout (this = slot 5).
    this_offset: ThisArgumentOffset,
}

/// `bool parse(Graph& graph)` (DFGByteCodeParser.cpp:12682-12685) +
/// `ByteCodeParser::parse` (:12633-12680), minus the post-parse analysis
/// phases (module doc). Parses directly INTO a plan-owned `Graph`, exactly
/// like C++: `DFG::Plan::compileInThreadImpl` constructs `Graph dfg(*m_vm,
/// *this)` (DFGPlan.cpp:198, reading `m_codeBlock`/graph id off the Plan,
/// dfg/DFGGraph.cpp:78-82) and then calls `parse(dfg)` (DFGPlan.cpp:203);
/// `dfg::plan::DfgPlan::compile_parse_only` is that call site here
/// (dfg/plan.rs). A decline can still leave the graph partially mutated
/// (e.g. a mid-block `ConstantIndexOutOfRange`) — that matches C++, whose
/// `CancelPath` discards the whole `Graph` rather than repairing it; callers
/// MUST treat any `Err` as "discard this graph/plan", never resume it.
pub fn parse_into(graph: &mut DfgGraph, code_block: &CodeBlock) -> Result<(), DeclineReason> {
    let instructions = code_block.unlinked().instructions();
    let Some(bytes) = instructions.raw_bytes() else {
        return Err(DeclineReason::NoRawPackedInstructionStream);
    };
    capability_level(bytes)?;

    if code_block.unlinked().kind() == CodeKind::Eval {
        return Err(DeclineReason::EvalCodeOutOfSlice);
    }

    let mut parser = ByteCodeParser::new(code_block, graph);
    parser.parse_block(bytes)
}

/// Rust-only convenience wrapper with no C++ counterpart: C++ never calls
/// `ByteCodeParser`/`parse(Graph&)` without a `Plan`-owned `Graph` already in
/// hand, so there is no standalone-graph entry point to port. This one exists
/// for callers with no `DfgPlan` yet (chiefly this module's own unit tests
/// below), and stamps the same null placeholder identities the parser used
/// before the plan unit landed (`DfgGraphId(0)`/`CodeBlockId(CellId(0))`);
/// real identities come only from `dfg::plan::DfgPlan::compile_parse_only`.
pub fn parse(code_block: &CodeBlock) -> Result<DfgGraph, DeclineReason> {
    let mut graph = DfgGraph::new(DfgGraphId(0), CodeBlockId(CellId(0)));
    parse_into(&mut graph, code_block)?;
    Ok(graph)
}

/// The per-opcode admission gate, mirroring how `DFG::capabilityLevel` keeps
/// unsupported CodeBlocks away from the parser (dfg/DFGCapabilities.h) and how
/// the first DFG accepted only single-basic-block bodies
/// (mcts_mem/javascriptcore/dfg.md, 2011-03-14 08a80d90). Walks the packed
/// stream by `size()` exactly like `NEXT_OPCODE` (DFGByteCodeParser.cpp:7437).
fn capability_level(bytes: &[u8]) -> Result<(), DeclineReason> {
    let mut offset = 0usize;
    let mut index = 0usize;
    let mut last_was_ret = false;
    while offset < bytes.len() {
        if last_was_ret {
            return Err(DeclineReason::UnreachableCodeAfterRet { offset });
        }
        let instruction = decode_raw_instruction(bytes, offset)
            .map_err(|_| DeclineReason::InstructionDecode { offset })?;
        match instruction.opcode_id {
            opcode_id::ENTER if index == 0 => {}
            _ if index == 0 => return Err(DeclineReason::BodyDoesNotStartWithEnter),
            opcode_id::MOV | opcode_id::ADD | opcode_id::SUB | opcode_id::MUL => {}
            opcode_id::RET => last_was_ret = true,
            other => {
                return Err(DeclineReason::OutOfSliceOpcode {
                    opcode_id: other,
                    offset,
                })
            }
        }
        index += 1;
        offset += instruction.size;
    }
    if !last_was_ret {
        return Err(DeclineReason::BodyDoesNotEndWithRet);
    }
    Ok(())
}

/// `clobbersExitState` (dfg/DFGClobbersExitState.cpp:36-130) narrowed to the
/// declared subset: MovHint clobbers explicitly (:41-46); SetLocal explicitly
/// does not (:48-53); everything else takes the clobberize default, and of
/// this subset only CheckTraps (writes InternalState, DFGClobberize.h:617-620)
/// and the world-clobbering ValueAdd/ValueSub/ValueMul (DFGClobberize.h:939+)
/// write a non-SideState heap.
fn clobbers_exit_state(op: NodeType) -> bool {
    matches!(
        op,
        NodeType::MovHint
            | NodeType::CheckTraps
            | NodeType::ValueAdd
            | NodeType::ValueSub
            | NodeType::ValueMul
    )
}

/// `jsDoubleNumber(value.asNumber())` (DFGByteCodeParser.cpp:403): a
/// Double-represented constant is re-encoded as a pure double.
fn js_double_number(value: ConstantValue) -> ConstantValue {
    match value {
        ConstantValue::Encoded(encoded) => match encoded.as_number() {
            Some(NumberValue::Int32(int)) => {
                ConstantValue::Encoded(JsValue::from_double(int as f64))
            }
            // Already double-encoded (or, unreachably for generator output,
            // not a number at all — C++'s asNumber() would assert first).
            _ => value,
        },
        _ => value,
    }
}

impl<'a, 'g> ByteCodeParser<'a, 'g> {
    /// Constructor state (DFGByteCodeParser.cpp:129-150) + the first-block
    /// allocation from `parseCodeBlock` (:10929-10937): the entry block is
    /// targetable at bytecode 0, sized by (numArguments, numLocals, numTmps)
    /// like the `BasicBlock` constructor (:1410), and is "definitely an OSR
    /// target" (:10932-10936). `graph` is the plan-owned `Graph&` C++'s ctor
    /// borrows (:133); the caller (`parse_into`) is the one who owns/creates
    /// it, matching `ByteCodeParser(Graph& graph)` taking an existing graph
    /// rather than building one.
    fn new(code_block: &'a CodeBlock, graph: &'g mut DfgGraph) -> Self {
        let frame = code_block.unlinked().frame();
        let num_arguments = frame.num_parameters_including_this as usize;
        let num_locals = frame.num_callee_locals as usize;
        let num_tmps = 0usize;
        let block = DfgBasicBlock {
            id: DfgBasicBlockId(0),
            nodes: Vec::new(),
            predecessors: Vec::new(),
            successors: Vec::new(),
            variables_at_head: Operands::new(num_arguments, num_locals, num_tmps),
            variables_at_tail: Operands::new(num_arguments, num_locals, num_tmps),
            bytecode_begin: Some(0),
            bytecode_end: None,
            execution_count: None,
            is_osr_entry: true,
            is_catch_entry: false,
        };
        let current_block = graph.add_block(block);
        Self {
            code_block,
            graph,
            current_block,
            current_index: BytecodeIndex::from_offset(0),
            current_semantic_origin: None,
            exit_ok: false,
            constants: Vec::new(),
            set_local_queue: Vec::new(),
            num_arguments,
            num_locals,
            num_tmps,
            this_offset: CallFrameSlotLayout::JSC_RUST.this_argument_offset,
        }
    }

    fn block(&self) -> &DfgBasicBlock {
        &self.graph.blocks[self.current_block.0 as usize]
    }

    fn block_mut(&mut self) -> &mut DfgBasicBlock {
        &mut self.graph.blocks[self.current_block.0 as usize]
    }

    /// `currentNodeOrigin()` (DFGByteCodeParser.cpp:832-838). The descriptor
    /// `NodeOrigin` keeps one CodeOrigin (see graph.rs), so the SEMANTIC
    /// origin — `m_currentSemanticOrigin` when set, else `m_currentIndex` — is
    /// what gets recorded, along with `m_exitOK`.
    fn current_node_origin(&self) -> NodeOrigin {
        let semantic = self.current_semantic_origin.unwrap_or(self.current_index);
        NodeOrigin {
            kind: NodeOriginKind::Bytecode,
            code_origin: CodeOrigin {
                kind: CodeOriginKind::BaselineCodeBlock,
                owner: None,
                executable: None,
                bytecode_index: Some(semantic.offset()),
            },
            owner: None,
            executable: None,
            bytecode_index: Some(semantic.offset()),
            inline_depth: 0,
            exit_ok: self.exit_ok,
        }
    }

    /// `addToGraph` (DFGByteCodeParser.cpp:850-885): append the node to the
    /// current block, wire the child edges (defaulting to UntypedUse like
    /// `Edge(child)`), and drop `m_exitOK` if the node clobbers exit state
    /// (:861-862).
    fn add_to_graph(&mut self, op: NodeType, children: &[DfgNodeId]) -> DfgNodeId {
        let node = DfgNode::new(op, self.current_node_origin());
        let id = self.graph.add_node(node);
        for (child_index, child) in children.iter().enumerate() {
            let edge = self.graph.add_edge(DfgEdge {
                id: DfgEdgeId(0),
                from: id,
                to: *child,
                child_index: child_index as u16,
                use_kind: EdgeUseKind::Untyped,
                proof: EdgeProofStatus::NeedsCheck,
                kill: EdgeKillStatus::DoesNotKill,
            });
            self.graph.nodes[id.0 as usize].children.push(edge);
        }
        self.block_mut().nodes.push(id);
        if clobbers_exit_state(op) {
            self.exit_ok = false;
        }
        id
    }

    /// `addToGraph(op, OpInfo(variable), ...)` for the local-access nodes,
    /// which carry their `VariableAccessData*` in `m_opInfo`.
    fn add_local_node(
        &mut self,
        op: NodeType,
        variable: DfgVariableAccessDataId,
        children: &[DfgNodeId],
    ) -> DfgNodeId {
        let id = self.add_to_graph(op, children);
        self.graph.nodes[id.0 as usize].variable_access_data = Some(variable);
        id
    }

    /// Constant node carrying its frozen value (`OpInfo(FrozenValue*)`,
    /// DFGByteCodeParser.cpp:403-405).
    fn add_constant_node(&mut self, op: NodeType, value: ConstantValue) -> DfgNodeId {
        let id = self.add_to_graph(op, &[]);
        self.graph.nodes[id.0 as usize].constant = Some(value);
        id
    }

    /// Resolve `node->child1().node()` through the edge table.
    fn child1(&self, node: DfgNodeId) -> DfgNodeId {
        let edge = self.graph.nodes[node.0 as usize].children[0];
        self.graph.edges[edge.0 as usize].to
    }

    fn argument_index(&self, operand: VirtualRegister) -> usize {
        debug_assert!(operand.raw() >= self.this_offset.0);
        (operand.raw() - self.this_offset.0) as usize
    }

    fn is_argument(&self, operand: VirtualRegister) -> bool {
        operand.is_argument_or_header() && operand.raw() >= self.this_offset.0
    }

    fn is_header_slot(&self, operand: VirtualRegister) -> bool {
        operand.is_argument_or_header() && operand.raw() < self.this_offset.0
    }

    /// `get(Operand)` (DFGByteCodeParser.cpp:386-432).
    fn get(&mut self, operand: VirtualRegister) -> Result<DfgNodeId, DeclineReason> {
        if operand.is_constant() {
            // (:388-410) — dedup through m_constants by constant index.
            let constant_index = operand
                .to_constant_index()
                .expect("constant register has a constant index")
                as usize;
            if constant_index >= self.constants.len() {
                self.constants.resize(constant_index + 1, None);
            }
            if self.constants[constant_index].is_none() {
                let linked = self
                    .code_block
                    .constants()
                    .constants
                    .get(constant_index)
                    .copied()
                    .ok_or(DeclineReason::ConstantIndexOutOfRange {
                        index: constant_index,
                    })?;
                // `constantSourceCodeRepresentation` (:394) lives on the
                // unlinked pool in the Rust split.
                let representation = self
                    .code_block
                    .unlinked()
                    .constants()
                    .constants
                    .iter()
                    .find(|constant| constant.register == operand)
                    .map(|constant| constant.source_representation)
                    .unwrap_or(SourceCodeRepresentation::Other);
                let node = if representation == SourceCodeRepresentation::DoubleLiteral {
                    // (:402-403) DoubleConstant freezes jsDoubleNumber(...).
                    self.add_constant_node(NodeType::DoubleConstant, js_double_number(linked.value))
                } else {
                    // (:405) everything else is a JSConstant.
                    self.add_constant_node(NodeType::JSConstant, linked.value)
                };
                self.constants[constant_index] = Some(node);
            }
            return Ok(self.constants[constant_index].expect("constant node was just created"));
        }

        // No InlineCallFrame in this slice, so only the machine-block callee
        // branch applies (:418-429). C++ first tries watchpoint-based
        // singleton folding on the owner FunctionExecutable (:422-427); the
        // Rust executable surface has no singleton inferred-value watchpoint
        // yet, so this always takes the GetCallee fallback (:428).
        if operand == VirtualRegister::from_raw(CALL_FRAME_SLOT_CALLEE) {
            return Ok(self.add_to_graph(NodeType::GetCallee, &[]));
        }
        if self.is_header_slot(operand) {
            // C++ getDirect ASSERTs away header slots; safe Rust declines.
            return Err(DeclineReason::UnsupportedOperand { raw: operand.raw() });
        }

        // getDirect(m_inlineStackTop->remapOperand(operand)) (:431) with the
        // identity remap; getDirect (:376-384) routes arguments vs locals.
        if self.is_argument(operand) {
            Ok(self.get_argument(operand))
        } else {
            Ok(self.get_local_or_tmp(operand))
        }
    }

    /// `getLocalOrTmp` (DFGByteCodeParser.cpp:495-524): link variable access
    /// data together and avoid redundant GetLocals through `variablesAtTail`.
    fn get_local_or_tmp(&mut self, operand: VirtualRegister) -> DfgNodeId {
        debug_assert!(operand.is_local());
        let tail = *self
            .block()
            .variables_at_tail
            .operand(operand, self.this_offset);

        let variable = if let Some(node_id) = tail {
            let node = &self.graph.nodes[node_id.0 as usize];
            match node.op {
                // (:511-518)
                NodeType::GetLocal => return node_id,
                NodeType::SetLocal => return self.child1(node_id),
                _ => node
                    .variable_access_data
                    .expect("local-access node carries a VariableAccessData"),
            }
        } else {
            self.graph.new_variable_access_data(operand)
        };

        let node = self.add_local_node(NodeType::GetLocal, variable, &[]);
        self.inject_lazy_operand_speculation(node);
        let this_offset = self.this_offset;
        *self
            .block_mut()
            .variables_at_tail
            .operand_mut(operand, this_offset) = Some(node);
        node
    }

    /// `getArgument` (DFGByteCodeParser.cpp:557-583) — same tail-reuse pattern
    /// over the argument row.
    fn get_argument(&mut self, operand: VirtualRegister) -> DfgNodeId {
        let argument = self.argument_index(operand);
        debug_assert!(argument < self.num_arguments);
        let tail = *self.block().variables_at_tail.argument(argument);

        let variable = if let Some(node_id) = tail {
            let node = &self.graph.nodes[node_id.0 as usize];
            match node.op {
                NodeType::GetLocal => return node_id,
                NodeType::SetLocal => return self.child1(node_id),
                _ => node
                    .variable_access_data
                    .expect("argument-access node carries a VariableAccessData"),
            }
        } else {
            self.graph.new_variable_access_data(operand)
        };

        let node = self.add_local_node(NodeType::GetLocal, variable, &[]);
        self.inject_lazy_operand_speculation(node);
        *self.block_mut().variables_at_tail.argument_mut(argument) = Some(node);
        node
    }

    /// `injectLazyOperandSpeculation` (DFGByteCodeParser.cpp:483-492). The
    /// Rust CodeBlock has no LazyOperandValueProfile surface yet; an absent
    /// profile predicts SpecNone (`LazyOperandValueProfileParser::prediction`
    /// for a missing key), and `predict(SpecNone)` is a no-op merge.
    fn inject_lazy_operand_speculation(&mut self, node: DfgNodeId) {
        let variable = self.graph.nodes[node.0 as usize]
            .variable_access_data
            .expect("GetLocal carries a VariableAccessData");
        self.graph.variable_access_data[variable.0 as usize].predict(SPEC_NONE);
    }

    /// `set` (DFGByteCodeParser.cpp:478-481): remapOperand is the identity for
    /// the machine code block.
    fn set(
        &mut self,
        operand: VirtualRegister,
        value: DfgNodeId,
        set_mode: SetMode,
    ) -> Result<(), DeclineReason> {
        self.set_direct(operand, value, set_mode)
    }

    /// `setDirect` (DFGByteCodeParser.cpp:454-469): MovHint now, then either
    /// queue the SetLocal (NormalSet — the two-phase commit) or execute it
    /// immediately.
    fn set_direct(
        &mut self,
        operand: VirtualRegister,
        value: DfgNodeId,
        set_mode: SetMode,
    ) -> Result<(), DeclineReason> {
        if operand.is_constant() || self.is_header_slot(operand) {
            // C++ trusts the generator here (Operand validity is ASSERTed);
            // safe Rust declines on a malformed hand-built stream.
            return Err(DeclineReason::UnsupportedOperand { raw: operand.raw() });
        }
        // MovHint carries the operand in m_opInfo (:456; the MovHint addToGraph
        // overload, :898-902).
        let mov_hint = self.add_to_graph(NodeType::MovHint, &[value]);
        self.graph.nodes[mov_hint.0 as usize].virtual_register = Some(operand);

        // We can't exit anymore because our OSR exit state has changed (:458-459).
        self.exit_ok = false;

        let delayed = DelayedSetLocal {
            origin: self.current_index,
            operand,
            value,
            set_mode,
        };
        if set_mode == SetMode::NormalSet {
            // (:463-466)
            self.set_local_queue.push(delayed);
            return Ok(());
        }
        self.execute_delayed(delayed);
        Ok(())
    }

    /// `processSetLocalQueue` (DFGByteCodeParser.cpp:471-476).
    fn process_set_local_queue(&mut self) {
        let mut index = 0;
        while index < self.set_local_queue.len() {
            let delayed = self.set_local_queue[index];
            self.execute_delayed(delayed);
            index += 1;
        }
        self.set_local_queue.clear();
    }

    /// `DelayedSetLocal::execute` (DFGByteCodeParser.cpp:1387-1392).
    fn execute_delayed(&mut self, delayed: DelayedSetLocal) -> DfgNodeId {
        if self.is_argument(delayed.operand) {
            self.set_argument(
                delayed.origin,
                delayed.operand,
                delayed.value,
                delayed.set_mode,
            )
        } else {
            self.set_local_or_tmp(
                delayed.origin,
                delayed.operand,
                delayed.value,
                delayed.set_mode,
            )
        }
    }

    /// `setLocalOrTmp` (DFGByteCodeParser.cpp:525-554).
    fn set_local_or_tmp(
        &mut self,
        semantic_origin: BytecodeIndex,
        operand: VirtualRegister,
        value: DfgNodeId,
        set_mode: SetMode,
    ) -> DfgNodeId {
        debug_assert!(operand.is_local());
        // SetForScope(m_currentSemanticOrigin, semanticOrigin) (:528).
        let saved_semantic_origin = self.current_semantic_origin;
        self.current_semantic_origin = Some(semantic_origin);

        if set_mode != SetMode::ImmediateNakedSet {
            // findArgumentPositionForLocal (:539) walks InlineCallFrames only
            // (:624-638); with none it returns null, so no argument flush.
            // needsScopeRegister() is m_hasDebuggerEnabled (DFGGraph.h:1298);
            // this slice compiles without a debugger, so no scope-register
            // Flush either (:542-543).
        }

        // A SetLocal always allocates a fresh VariableAccessData (:546); the
        // mergeStructureCheckHoistingFailed/mergeCheckArrayHoistingFailed
        // calls (:547-550) read the QueryableExitProfile, which has no Rust
        // surface yet — hasExitSite is vacuously false, making them no-ops.
        let variable = self.graph.new_variable_access_data(operand);
        let node = self.add_local_node(NodeType::SetLocal, variable, &[value]);
        let this_offset = self.this_offset;
        *self
            .block_mut()
            .variables_at_tail
            .operand_mut(operand, this_offset) = Some(node);

        self.current_semantic_origin = saved_semantic_origin;
        node
    }

    /// `setArgument` (DFGByteCodeParser.cpp:584-614).
    fn set_argument(
        &mut self,
        semantic_origin: BytecodeIndex,
        operand: VirtualRegister,
        value: DfgNodeId,
        set_mode: SetMode,
    ) -> DfgNodeId {
        let saved_semantic_origin = self.current_semantic_origin;
        self.current_semantic_origin = Some(semantic_origin);

        let argument = self.argument_index(operand);
        debug_assert!(argument < self.num_arguments);
        let variable = self.graph.new_variable_access_data(operand);

        // Always flush arguments, except for 'this' (:594-602).
        if argument != 0 {
            if set_mode != SetMode::ImmediateNakedSet {
                self.flush_direct(operand);
            }
        } else if set_mode != SetMode::ImmediateNakedSet {
            self.phantom_local_direct(operand);
        }

        // The CodeForConstruct shouldNeverUnbox merge (:604-605) and the
        // exit-profile hoisting-failure merges (:607-610) touch
        // VariableAccessData state deferred with prediction propagation
        // (variable_access_data.rs module doc); this slice parses CodeForCall
        // with no exit profile, so both are no-ops.
        let node = self.add_local_node(NodeType::SetLocal, variable, &[value]);
        *self.block_mut().variables_at_tail.argument_mut(argument) = Some(node);

        self.current_semantic_origin = saved_semantic_origin;
        node
    }

    /// `flushDirect` → `addFlushOrPhantomLocal<Flush>` (DFGByteCodeParser.cpp:
    /// 704-731). The `ArgumentPosition::addVariable` call (:729-730) is the
    /// argument-prediction unification machinery, deferred with prediction
    /// propagation.
    fn flush_direct(&mut self, operand: VirtualRegister) {
        self.add_flush_or_phantom_local(NodeType::Flush, operand);
    }

    /// `phantomLocalDirect` → `addFlushOrPhantomLocal<PhantomLocal>` (:733-736).
    fn phantom_local_direct(&mut self, operand: VirtualRegister) {
        self.add_flush_or_phantom_local(NodeType::PhantomLocal, operand);
    }

    fn add_flush_or_phantom_local(&mut self, node_type: NodeType, operand: VirtualRegister) {
        debug_assert!(!operand.is_constant()); // (:717)
        let tail = if self.is_argument(operand) {
            let argument = self.argument_index(operand);
            *self.block().variables_at_tail.argument(argument)
        } else {
            *self
                .block()
                .variables_at_tail
                .operand(operand, self.this_offset)
        };
        let variable = tail
            .map(|node_id| {
                self.graph.nodes[node_id.0 as usize]
                    .variable_access_data
                    .expect("local-access node carries a VariableAccessData")
            })
            .unwrap_or_else(|| self.graph.new_variable_access_data(operand));
        let node = self.add_local_node(node_type, variable, &[]);
        if self.is_argument(operand) {
            let argument = self.argument_index(operand);
            *self.block_mut().variables_at_tail.argument_mut(argument) = Some(node);
        } else {
            let this_offset = self.this_offset;
            *self
                .block_mut()
                .variables_at_tail
                .operand_mut(operand, this_offset) = Some(node);
        }
    }

    /// `flushForReturn` (DFGByteCodeParser.cpp:751-754) → `flush(InlineStackEntry*)`
    /// (:738-742) → `flushImpl` (:649-668) with a null InlineCallFrame:
    /// `numArguments = numParameters()` and the arguments are flushed in
    /// DESCENDING order (`for (unsigned argument = numArguments; argument--;)`,
    /// :663-664). needsScopeRegister() is false (DFGGraph.h:1298), so no scope
    /// flush (:666-667).
    fn flush_for_return(&mut self) {
        for argument in (0..self.num_arguments).rev() {
            self.flush_direct(VirtualRegister::argument_including_this(
                argument as u32,
                self.this_offset,
            ));
        }
    }

    /// `makeSafe` (DFGByteCodeParser.cpp:1096-1209), narrowed to the arith
    /// opcodes this slice emits. The `hasExitSite(Overflow/NegativeZero)`
    /// merges (:1098-1101) read the QueryableExitProfile, which has no Rust
    /// surface — vacuously false. With no arith profile row the switch arm
    /// `break`s without merging any flags (:1126-1127, :1160-1161): the
    /// SpecNone-safe absent-profile path.
    fn make_safe(&mut self, node: DfgNodeId) -> DfgNodeId {
        let op = self.graph.nodes[node.0 as usize].op;
        match op {
            // (:1106-1138) — the ArithAdd/ArithSub/ValueAdd (and bit-op) arm.
            // NOTE: ValueSub is deliberately NOT here. C++'s case labels
            // (:1107-1109) are ArithAdd, ArithSub, ValueAdd — ValueSub appears
            // nowhere in makeSafe and falls to `default: break` (:1204-1205),
            // merging no flags even with a profile present.
            NodeType::ArithAdd | NodeType::ArithSub | NodeType::ValueAdd => {
                let observed = if let Some(profile) = self
                    .code_block
                    .binary_arith_profile_for_bytecode_index(self.current_index)
                {
                    profile.arith().observed_results()
                } else if let Some(profile) = self
                    .code_block
                    .unary_arith_profile_for_bytecode_index(self.current_index)
                {
                    // Happens for OpInc/OpDec (:1123-1125).
                    profile.arith().observed_results()
                } else {
                    return node; // (:1126-1127)
                };
                let node_mut = &mut self.graph.nodes[node.0 as usize];
                if observed.did_observe_double() {
                    node_mut.merge_flags(NODE_MAY_HAVE_DOUBLE_RESULT);
                }
                if observed.did_observe_non_numeric() {
                    node_mut.merge_flags(NODE_MAY_HAVE_NON_NUMERIC_RESULT);
                }
                if observed.did_observe_big_int32() {
                    node_mut.merge_flags(NODE_MAY_HAVE_BIG_INT32_RESULT);
                }
                // The BigInt32Overflow exit-site alternative (:1135) is part
                // of the absent exit-profile surface.
                if observed.did_observe_heap_big_int() {
                    node_mut.merge_flags(NODE_MAY_HAVE_HEAP_BIG_INT_RESULT);
                }
            }
            // (:1157-1177) — the Mul arm reads richer overflow/negzero bits.
            NodeType::ValueMul | NodeType::ArithMul => {
                let Some(profile) = self
                    .code_block
                    .binary_arith_profile_for_bytecode_index(self.current_index)
                else {
                    return node; // (:1160-1161)
                };
                let arith = *profile.arith();
                let node_mut = &mut self.graph.nodes[node.0 as usize];
                if arith.did_observe_int52_overflow() {
                    node_mut.merge_flags(NODE_MAY_OVERFLOW_INT52);
                }
                if arith.did_observe_int32_overflow() {
                    node_mut.merge_flags(NODE_MAY_OVERFLOW_INT32_IN_BASELINE);
                }
                if arith.did_observe_neg_zero_double() {
                    node_mut.merge_flags(NODE_MAY_NEG_ZERO_IN_BASELINE);
                }
                if arith.did_observe_double() {
                    node_mut.merge_flags(NODE_MAY_HAVE_DOUBLE_RESULT);
                }
                if arith.did_observe_non_numeric() {
                    node_mut.merge_flags(NODE_MAY_HAVE_NON_NUMERIC_RESULT);
                }
                if arith.did_observe_big_int32() {
                    node_mut.merge_flags(NODE_MAY_HAVE_BIG_INT32_RESULT);
                }
                if arith.did_observe_heap_big_int() {
                    node_mut.merge_flags(NODE_MAY_HAVE_HEAP_BIG_INT_RESULT);
                }
            }
            _ => {}
        }
        node
    }

    /// `handleGetScope` (DFGByteCodeParser.cpp:7019-7028).
    fn handle_get_scope(&mut self, destination: VirtualRegister) -> Result<(), DeclineReason> {
        let callee = self.get(VirtualRegister::from_raw(CALL_FRAME_SLOT_CALLEE))?;
        // `callee->dynamicCastConstant<JSFunction*>()` (:7023-7024): callee is
        // a GetCallee node here (no executable-singleton constant surface, see
        // get()), so the constant-scope fold cannot fire; take the GetScope
        // arm (:7026).
        let result = self.add_to_graph(NodeType::GetScope, &[callee]);
        self.set(destination, result, SetMode::NormalSet)
    }

    /// `handleCheckTraps` (DFGByteCodeParser.cpp:7030-7033). C++ emits
    /// InvalidationPoint when signal-based traps are available on a linked
    /// plan, CheckTraps under `Options::usePollingTraps()` or an unlinked
    /// plan. The Rust engine has no signal/jump-replacement trap machinery —
    /// exactly JSC's polling-traps configuration — so this always emits
    /// CheckTraps.
    fn handle_check_traps(&mut self) {
        self.add_to_graph(NodeType::CheckTraps, &[]);
    }

    /// `op_enter` (DFGByteCodeParser.cpp:7542-7568).
    fn handle_op_enter(&mut self) -> Result<(), DeclineReason> {
        // (:7543) Node* undefined = addToGraph(JSConstant, OpInfo(m_constantUndefined)).
        let undefined = self.add_constant_node(
            NodeType::JSConstant,
            ConstantValue::Encoded(JsValue::undefined()),
        );
        // (:7544-7546) initialize all locals to undefined, naked-immediate.
        let num_vars = self.code_block.unlinked().frame().num_vars;
        for local in 0..num_vars {
            self.set(
                VirtualRegister::local(local),
                undefined,
                SetMode::ImmediateNakedSet,
            )?;
        }

        // hasTailCalls() (:7548-7552): the capability pre-scan admits no call
        // opcodes, so the recursive-tail-call entry block cannot be needed.
        // The EvalCode arm (:7554-7557) declined before parsing began.

        let scope_register = self.code_block.unlinked().frame().special.scope_register;
        if !scope_register.is_valid() {
            return Err(DeclineReason::MissingScopeRegister);
        }
        self.handle_get_scope(scope_register)?; // (:7559)

        // Normally we wouldn't be allowed to exit here, but in this case we'd
        // only be re-initializing the locals and resetting the scope register
        // (:7561-7564).
        self.exit_ok = true;
        self.add_to_graph(NodeType::ExitOK, &[]);

        self.handle_check_traps(); // (:7566)
        Ok(())
    }

    /// `parseBlock` (DFGByteCodeParser.cpp:7465-7538 + the per-opcode arms),
    /// walking the packed stream by `size()` (`NEXT_OPCODE`, :7437).
    fn parse_block(&mut self, bytes: &[u8]) -> Result<(), DeclineReason> {
        // First-basic-block argument prologue (:7473-7494). The
        // m_rootToArguments record (:7474-7477) feeds the arguments-elimination
        // phase and lands with it.
        self.exit_ok = true; // (:7479-7481)
        for argument in 0..self.num_arguments {
            let register =
                VirtualRegister::argument_including_this(argument as u32, self.this_offset);
            // Exit-profile hoisting-failure merges (:7485-7488): no
            // QueryableExitProfile surface — no-ops.
            let variable = self.graph.new_variable_access_data(register);
            let set_argument = self.add_local_node(NodeType::SetArgumentDefinitely, variable, &[]);
            // setArgumentFirstTime (:7492).
            *self.block_mut().variables_at_tail.argument_mut(argument) = Some(set_argument);
        }

        loop {
            // We're staring at a new bytecode instruction: we can exit again
            // (:7505-7507), and queued SetLocals commit here (:7509).
            self.exit_ok = true;
            self.process_set_local_queue();

            let offset = self.current_index.offset() as usize;
            // The capability pre-scan guarantees the body ends with op_ret,
            // which returns below via LAST_OPCODE, so the jump-planting limit
            // path (:7512-7524) is unreachable in this slice.
            debug_assert!(offset < bytes.len());

            let instruction = decode_raw_instruction(bytes, offset)
                .map_err(|_| DeclineReason::InstructionDecode { offset })?;
            let next_index = BytecodeIndex::from_offset((offset + instruction.size) as u32);
            let operand_register =
                |index: usize| VirtualRegister::from_raw(instruction.operands[index] as i32);

            match instruction.opcode_id {
                opcode_id::ENTER => {
                    self.handle_op_enter()?;
                }
                opcode_id::MOV => {
                    // op_mov (:8086-8091).
                    let dst = operand_register(0);
                    let src = operand_register(1);
                    let op = self.get(src)?;
                    self.set(dst, op, SetMode::NormalSet)?;
                }
                opcode_id::ADD | opcode_id::SUB | opcode_id::MUL => {
                    // op_add/op_sub/op_mul (:8000-8020, :8032-8042). The
                    // Arith* arm fires only when BOTH operands satisfy
                    // hasNumberResult(), which is EXACT equality
                    // `result() == NodeResultNumber` (DFGNode.h:1741-1744
                    // over result(), DFGNode.h:488-491). DoubleConstant
                    // defaults to NodeResultDouble (DFGNodeType.h:40) and
                    // GetLocal/JSConstant to NodeResultJS, so parser output
                    // here is always the Value* form; Value*→Arith*
                    // strengthening is FixupPhase's job, driven by
                    // predictions.
                    let dst = operand_register(0);
                    let op1 = self.get(operand_register(1))?;
                    let op2 = self.get(operand_register(2))?;
                    let both_number = self.graph.nodes[op1.0 as usize].has_number_result()
                        && self.graph.nodes[op2.0 as usize].has_number_result();
                    let (arith_op, value_op) = match instruction.opcode_id {
                        opcode_id::ADD => (NodeType::ArithAdd, NodeType::ValueAdd),
                        opcode_id::SUB => (NodeType::ArithSub, NodeType::ValueSub),
                        _ => (NodeType::ArithMul, NodeType::ValueMul),
                    };
                    let node = self
                        .add_to_graph(if both_number { arith_op } else { value_op }, &[op1, op2]);
                    let node = self.make_safe(node);
                    self.set(dst, node, SetMode::NormalSet)?;
                }
                opcode_id::RET => {
                    // op_ret (:9332-9344), non-inlined arm only.
                    debug_assert!(self.block().find_terminal(&self.graph).is_none()); // (:9334)
                    let return_value = self.get(operand_register(0))?;
                    self.add_to_graph(NodeType::Return, &[return_value]);
                    self.flush_for_return();
                    // LAST_OPCODE → LAST_OPCODE_LINKED (:7443-7447).
                    self.current_index = next_index;
                    self.exit_ok = false;
                    return Ok(());
                }
                other => {
                    // Unreachable behind capability_level; keep the decline
                    // for defense in depth.
                    return Err(DeclineReason::OutOfSliceOpcode {
                        opcode_id: other,
                        offset,
                    });
                }
            }

            // NEXT_OPCODE (:7437): advance by the instruction's SIZE.
            self.current_index = next_index;
        }
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::bytecode::code_block::{
        CodeKind, LinkContext, UnlinkedCodeBlock, UnlinkedConstant, UnlinkedConstantPool,
    };
    use crate::bytecode::instruction_stream::{InstructionStreamWriter, OperandValue};
    use crate::bytecode::register::{RegisterFrameShape, SpecialRegisters};
    use crate::bytecode::PackedInstructionStream;
    use crate::dfg::graph::GraphForm;
    use crate::dfg::node_type::default_flags;

    const THIS_OFFSET: i32 = 5; // CallFrameSlot::thisArgument (CallFrame.h).

    fn argument_register(argument: u32) -> i32 {
        THIS_OFFSET + argument as i32
    }

    fn local_register(local: u32) -> i32 {
        VirtualRegister::local(local).raw()
    }

    fn constant_register(index: u32) -> i32 {
        VirtualRegister::constant(index).raw()
    }

    fn stream(build: impl FnOnce(&mut InstructionStreamWriter)) -> Vec<u8> {
        let mut writer = InstructionStreamWriter::new();
        build(&mut writer);
        writer.finalize().bytes().to_vec()
    }

    fn emit_add(writer: &mut InstructionStreamWriter, dst: i32, lhs: i32, rhs: i32) {
        writer.emit(
            opcode_id::ADD,
            &[
                OperandValue::VirtualRegister(dst),
                OperandValue::VirtualRegister(lhs),
                OperandValue::VirtualRegister(rhs),
                OperandValue::UnsignedImmediate(1),
                OperandValue::OperandTypes(0),
            ],
        );
    }

    fn code_block(
        bytes: Vec<u8>,
        num_parameters_including_this: u32,
        num_vars: u32,
        constants: Vec<UnlinkedConstant>,
    ) -> CodeBlock {
        let mut pool = UnlinkedConstantPool::default();
        pool.constants = constants;
        let unlinked = UnlinkedCodeBlock::new(
            CodeKind::Function,
            PackedInstructionStream::from_raw_packed_bytes(bytes),
        )
        .with_frame(RegisterFrameShape {
            num_parameters_including_this,
            num_vars,
            // numCalleeLocals covers vars plus expression temporaries.
            num_callee_locals: num_vars + 8,
            num_temporaries: 0,
            special: SpecialRegisters {
                // C++ function CodeBlocks always carry a scope register; keep
                // it at loc0, inside numVars where the generator puts it.
                scope_register: VirtualRegister::local(0),
                ..SpecialRegisters::default()
            },
        })
        .with_constants(pool);
        CodeBlock::from_unlinked(unlinked, LinkContext::default())
    }

    fn double_constant(index: u32, value: f64) -> UnlinkedConstant {
        UnlinkedConstant {
            register: VirtualRegister::constant(index),
            value: ConstantValue::Encoded(JsValue::from_double(value)),
            source_representation: SourceCodeRepresentation::DoubleLiteral,
        }
    }

    fn block_ops(graph: &DfgGraph) -> Vec<NodeType> {
        graph.blocks[0]
            .nodes
            .iter()
            .map(|id| graph.nodes[id.0 as usize].op)
            .collect()
    }

    fn nth_of_kind(graph: &DfgGraph, kind: NodeType, n: usize) -> DfgNodeId {
        graph.blocks[0]
            .nodes
            .iter()
            .copied()
            .filter(|id| graph.nodes[id.0 as usize].op == kind)
            .nth(n)
            .unwrap_or_else(|| panic!("missing {kind:?} #{n}"))
    }

    fn only_of_kind(graph: &DfgGraph, kind: NodeType) -> DfgNodeId {
        assert_eq!(
            count_of_kind(graph, kind),
            1,
            "expected exactly one {kind:?}"
        );
        nth_of_kind(graph, kind, 0)
    }

    fn count_of_kind(graph: &DfgGraph, kind: NodeType) -> usize {
        graph.blocks[0]
            .nodes
            .iter()
            .filter(|id| graph.nodes[id.0 as usize].op == kind)
            .count()
    }

    fn child(graph: &DfgGraph, node: DfgNodeId, index: usize) -> DfgNodeId {
        let edge = graph.nodes[node.0 as usize].children[index];
        graph.edges[edge.0 as usize].to
    }

    // Test 1: identity `f(a){ return a; }` — SetArgumentDefinitely prologue;
    // Return's child resolves to a's node (a fresh GetLocal sharing the
    // prologue node's VariableAccessData, DFGByteCodeParser.cpp:557-583).
    #[test]
    fn identity_function_returns_argument_through_prologue_node() {
        let bytes = stream(|writer| {
            writer.emit(opcode_id::ENTER, &[]);
            writer.emit(
                opcode_id::RET,
                &[OperandValue::VirtualRegister(argument_register(1))],
            );
        });
        let code_block = code_block(bytes, 2, 1, Vec::new());
        let graph = parse(&code_block).expect("in-slice body must parse");

        assert_eq!(graph.form, GraphForm::LoadStore);
        assert_eq!(graph.blocks.len(), 1);
        assert_eq!(graph.validate(), Ok(()));

        // Prologue precedes everything (:7473-7494).
        let ops = block_ops(&graph);
        assert_eq!(
            &ops[..2],
            &[
                NodeType::SetArgumentDefinitely,
                NodeType::SetArgumentDefinitely
            ]
        );

        let return_node = only_of_kind(&graph, NodeType::Return);
        let returned = child(&graph, return_node, 0);
        assert_eq!(graph.nodes[returned.0 as usize].op, NodeType::GetLocal);
        // The GetLocal shares the prologue SetArgumentDefinitely's
        // VariableAccessData for argument a (block-local unification, :500-509).
        let prologue_a = nth_of_kind(&graph, NodeType::SetArgumentDefinitely, 1);
        assert_eq!(
            graph.nodes[returned.0 as usize].variable_access_data,
            graph.nodes[prologue_a.0 as usize].variable_access_data
        );
        // Return blocks end with the argument Flushes (reverse order,
        // :663-664), which findTerminal legally skips (DFGBasicBlock.h:87-90).
        assert_eq!(&ops[ops.len() - 2..], &[NodeType::Flush, NodeType::Flush]);
        assert!(graph.blocks[0].find_terminal(&graph).is_some());
    }

    // Test 2: `f(a,b){ return a+b; }` — GetLocal a,b feed the add; both are
    // NodeResultJS (DFGNodeType.h:74), so hasNumberResult() — EXACT equality
    // result()==NodeResultNumber (DFGNode.h:1741-1744) — is false for both and
    // the REAL predicate picks ValueAdd (DFGByteCodeParser.cpp:8004-8007).
    #[test]
    fn add_of_arguments_lowers_to_value_add_with_two_phase_set() {
        let dst = local_register(1);
        let bytes = stream(|writer| {
            writer.emit(opcode_id::ENTER, &[]);
            emit_add(writer, dst, argument_register(1), argument_register(2));
            writer.emit(opcode_id::RET, &[OperandValue::VirtualRegister(dst)]);
        });
        let code_block = code_block(bytes, 3, 1, Vec::new());
        let graph = parse(&code_block).expect("in-slice body must parse");
        assert_eq!(graph.validate(), Ok(()));

        assert_eq!(count_of_kind(&graph, NodeType::ArithAdd), 0);
        let add = only_of_kind(&graph, NodeType::ValueAdd);
        let lhs = child(&graph, add, 0);
        let rhs = child(&graph, add, 1);
        assert_eq!(graph.nodes[lhs.0 as usize].op, NodeType::GetLocal);
        assert_eq!(graph.nodes[rhs.0 as usize].op, NodeType::GetLocal);
        assert_ne!(lhs, rhs);

        // Two-phase commit (:454-469 + :7509): a MovHint for dst follows the
        // add immediately; the queued SetLocal commits at the NEXT bytecode.
        let block = &graph.blocks[0].nodes;
        let add_position = block.iter().position(|id| *id == add).unwrap();
        let mov_hint = block[add_position + 1];
        assert_eq!(graph.nodes[mov_hint.0 as usize].op, NodeType::MovHint);
        assert_eq!(
            graph.nodes[mov_hint.0 as usize].virtual_register,
            Some(VirtualRegister::from_raw(dst))
        );
        assert_eq!(child(&graph, mov_hint, 0), add);
        let dst_set_local = block[add_position + 2];
        assert_eq!(graph.nodes[dst_set_local.0 as usize].op, NodeType::SetLocal);
        assert_eq!(child(&graph, dst_set_local, 0), add);

        // The ret's get(dst) reuses the SetLocal's child — Return consumes the
        // ValueAdd directly (:514-515).
        let return_node = only_of_kind(&graph, NodeType::Return);
        assert_eq!(child(&graph, return_node, 0), add);
    }

    // Test 3: `1.5+2.5` — two DoubleConstants (SourceCodeRepresentation::Double
    // takes the DoubleConstant arm, DFGByteCodeParser.cpp:402-403), and the
    // resolved predicate makes this a ValueAdd: DoubleConstant defaults to
    // NodeResultDouble (DFGNodeType.h:40) and hasNumberResult() is exact
    // equality with NodeResultNumber (DFGNode.h:1741-1744), so the scoping
    // audit's ArithAdd claim was wrong.
    #[test]
    fn double_constant_add_lowers_to_value_add_over_double_constants() {
        let dst = local_register(1);
        let bytes = stream(|writer| {
            writer.emit(opcode_id::ENTER, &[]);
            emit_add(writer, dst, constant_register(0), constant_register(1));
            writer.emit(opcode_id::RET, &[OperandValue::VirtualRegister(dst)]);
        });
        let code_block = code_block(
            bytes,
            1,
            1,
            vec![double_constant(0, 1.5), double_constant(1, 2.5)],
        );
        let graph = parse(&code_block).expect("in-slice body must parse");
        assert_eq!(graph.validate(), Ok(()));

        assert_eq!(count_of_kind(&graph, NodeType::DoubleConstant), 2);
        assert_eq!(count_of_kind(&graph, NodeType::ArithAdd), 0);
        let add = only_of_kind(&graph, NodeType::ValueAdd);
        let lhs = child(&graph, add, 0);
        let rhs = child(&graph, add, 1);
        assert_eq!(graph.nodes[lhs.0 as usize].op, NodeType::DoubleConstant);
        assert_eq!(graph.nodes[rhs.0 as usize].op, NodeType::DoubleConstant);
        assert_ne!(lhs, rhs);
        assert_eq!(
            graph.nodes[lhs.0 as usize].constant,
            Some(ConstantValue::Encoded(JsValue::from_double(1.5)))
        );
        assert_eq!(
            graph.nodes[rhs.0 as usize].constant,
            Some(ConstantValue::Encoded(JsValue::from_double(2.5)))
        );
    }

    // Test 4: mov aliasing `x=a; return x` — the MovHint and delayed SetLocal
    // share a's GetLocal, and get(x) at the ret REUSES the SetLocal's child
    // via variablesAtTail (:514-515): exactly ONE GetLocal in the block.
    #[test]
    fn mov_aliasing_reuses_value_through_variables_at_tail() {
        let x = local_register(1);
        let bytes = stream(|writer| {
            writer.emit(opcode_id::ENTER, &[]);
            writer.emit(
                opcode_id::MOV,
                &[
                    OperandValue::VirtualRegister(x),
                    OperandValue::VirtualRegister(argument_register(1)),
                ],
            );
            writer.emit(opcode_id::RET, &[OperandValue::VirtualRegister(x)]);
        });
        let code_block = code_block(bytes, 2, 1, Vec::new());
        let graph = parse(&code_block).expect("in-slice body must parse");
        assert_eq!(graph.validate(), Ok(()));

        let get_a = only_of_kind(&graph, NodeType::GetLocal);
        // MovHint(x) and the delayed SetLocal(x) both consume a's node.
        let mov_hints: Vec<DfgNodeId> = graph.blocks[0]
            .nodes
            .iter()
            .copied()
            .filter(|id| {
                graph.nodes[id.0 as usize].op == NodeType::MovHint
                    && graph.nodes[id.0 as usize].virtual_register
                        == Some(VirtualRegister::from_raw(x))
            })
            .collect();
        assert_eq!(mov_hints.len(), 1);
        assert_eq!(child(&graph, mov_hints[0], 0), get_a);
        let set_locals_of_a: Vec<DfgNodeId> = graph.blocks[0]
            .nodes
            .iter()
            .copied()
            .filter(|id| {
                graph.nodes[id.0 as usize].op == NodeType::SetLocal
                    && child(&graph, *id, 0) == get_a
            })
            .collect();
        assert_eq!(set_locals_of_a.len(), 1);

        // No redundant GetLocal for x: Return reuses a's node.
        let return_node = only_of_kind(&graph, NodeType::Return);
        assert_eq!(child(&graph, return_node, 0), get_a);
    }

    // Test 5: declines. Any out-of-slice opcode returns a DeclineReason and NO
    // graph — the getPrediction invariant (empty profiles would force
    // ForceOSRExit, DFGByteCodeParser.cpp:1050-1061).
    #[test]
    fn out_of_slice_opcodes_decline_without_partial_graph() {
        // op_jmp: declared in the Rust opcode table, so it declines as an
        // out-of-slice opcode.
        let jmp_bytes = stream(|writer| {
            writer.emit(opcode_id::ENTER, &[]);
            writer.emit(opcode_id::JMP, &[OperandValue::BoundLabel(2)]);
            writer.emit(
                opcode_id::RET,
                &[OperandValue::VirtualRegister(local_register(1))],
            );
        });
        let jmp_offset = 1; // op_enter is 1 byte (no operands).
        assert_eq!(
            parse(&code_block(jmp_bytes, 1, 1, Vec::new())),
            Err(DeclineReason::OutOfSliceOpcode {
                opcode_id: opcode_id::JMP,
                offset: jmp_offset,
            })
        );

        // op_get_by_id: REAL generated id 20 (Bytecodes.h
        // `op_get_by_id_value_string "20"`), not yet in the declared Rust
        // opcode table — the packed decode itself refuses it.
        const OP_GET_BY_ID: u8 = 20;
        let mut get_by_id_bytes = stream(|writer| {
            writer.emit(opcode_id::ENTER, &[]);
        });
        get_by_id_bytes.extend_from_slice(&[OP_GET_BY_ID, 0, 0, 0]);
        assert_eq!(
            parse(&code_block(get_by_id_bytes, 1, 1, Vec::new())),
            Err(DeclineReason::InstructionDecode { offset: 1 })
        );

        // Shape declines: no terminal ret / trailing code after ret.
        let no_ret = stream(|writer| {
            writer.emit(opcode_id::ENTER, &[]);
        });
        assert_eq!(
            parse(&code_block(no_ret, 1, 1, Vec::new())),
            Err(DeclineReason::BodyDoesNotEndWithRet)
        );
        let tail_after_ret = stream(|writer| {
            writer.emit(opcode_id::ENTER, &[]);
            writer.emit(
                opcode_id::RET,
                &[OperandValue::VirtualRegister(local_register(1))],
            );
            writer.emit(opcode_id::ENTER, &[]);
        });
        assert_eq!(
            parse(&code_block(tail_after_ret, 1, 1, Vec::new())),
            Err(DeclineReason::UnreachableCodeAfterRet { offset: 3 })
        );
    }

    // Test 6: op_enter shape (:7542-7568) — JSConstant(undefined) + per-var
    // MovHint/SetLocal (ImmediateNakedSet), GetCallee+GetScope with the
    // MovHint for the scope register, ExitOK, CheckTraps; the queued scope
    // SetLocal commits at the next opcode; SetArgumentDefinitely precedes it
    // all.
    #[test]
    fn enter_emits_undefined_locals_scope_exit_ok_and_check_traps() {
        let bytes = stream(|writer| {
            writer.emit(opcode_id::ENTER, &[]);
            writer.emit(
                opcode_id::RET,
                &[OperandValue::VirtualRegister(local_register(1))],
            );
        });
        let code_block = code_block(bytes, 1, 2, Vec::new());
        let graph = parse(&code_block).expect("in-slice body must parse");
        assert_eq!(graph.validate(), Ok(()));
        assert_eq!(graph.form, GraphForm::LoadStore);

        let ops = block_ops(&graph);
        assert_eq!(
            ops,
            vec![
                // Prologue (:7473-7494).
                NodeType::SetArgumentDefinitely,
                // op_enter (:7542-7568).
                NodeType::JSConstant,
                NodeType::MovHint,  // loc0 = undefined (ImmediateNakedSet:
                NodeType::SetLocal, // MovHint then immediate SetLocal, :454-468)
                NodeType::MovHint,  // loc1 = undefined
                NodeType::SetLocal,
                NodeType::GetCallee, // handleGetScope (:7019-7028)
                NodeType::GetScope,
                NodeType::MovHint,    // scope register set is a NormalSet:
                NodeType::ExitOK,     // its SetLocal is QUEUED, so ExitOK and
                NodeType::CheckTraps, // CheckTraps come first,
                NodeType::SetLocal,   // and the SetLocal commits at op_ret's
                // origin (:7509).
                // op_ret (:9332-9344): get(loc1) reuses SetLocal@5's child —
                // the undefined JSConstant — so Return consumes it directly.
                NodeType::Return,
                NodeType::Flush, // flushForReturn over the 1 argument.
            ]
        );

        // The undefined constant feeds both local inits and the return.
        let undefined = nth_of_kind(&graph, NodeType::JSConstant, 0);
        assert_eq!(
            graph.nodes[undefined.0 as usize].constant,
            Some(ConstantValue::Encoded(JsValue::undefined()))
        );
        let return_node = only_of_kind(&graph, NodeType::Return);
        assert_eq!(child(&graph, return_node, 0), undefined);
        // GetScope consumes GetCallee.
        let get_scope = only_of_kind(&graph, NodeType::GetScope);
        assert_eq!(
            child(&graph, get_scope, 0),
            only_of_kind(&graph, NodeType::GetCallee)
        );
    }

    // Test 7: makeSafe absent-profile path (:1121-1127): with NO arith profile
    // rows, the add carries NO overflow/negzero/double arith flags — its flags
    // stay exactly the opcode defaults. With a profile row observing a double
    // result, NodeMayHaveDoubleResult is merged (:1129-1130).
    #[test]
    fn make_safe_merges_nothing_without_profiles_and_double_with_them() {
        let dst = local_register(1);
        let build = |writer: &mut InstructionStreamWriter| {
            writer.emit(opcode_id::ENTER, &[]);
            emit_add(writer, dst, argument_register(1), argument_register(2));
            writer.emit(opcode_id::RET, &[OperandValue::VirtualRegister(dst)]);
        };

        // Absent profile: default flags only.
        let code_block_without_profile = code_block(stream(build), 3, 1, Vec::new());
        let graph = parse(&code_block_without_profile).expect("in-slice body must parse");
        let add = only_of_kind(&graph, NodeType::ValueAdd);
        assert_eq!(
            graph.nodes[add.0 as usize].flags,
            default_flags(NodeType::ValueAdd)
        );

        // Present profile that observed a double result: the double flag is
        // merged (:1129-1130). The add starts at byte offset 1 (op_enter is
        // one byte).
        let code_block_with_profile = code_block(stream(build), 3, 1, Vec::new());
        {
            use crate::bytecode::{BinaryArithProfile, BinaryArithProfileSlot};
            let mut profile = BinaryArithProfile::default();
            profile.arith_mut().set_observed_non_neg_zero_double();
            code_block_with_profile
                .side_tables()
                .binary_arith_profiles
                .borrow_mut()
                .push(BinaryArithProfileSlot {
                    bytecode_index: BytecodeIndex::from_offset(1),
                    profile,
                });
        }
        let graph = parse(&code_block_with_profile).expect("in-slice body must parse");
        let add = only_of_kind(&graph, NodeType::ValueAdd);
        assert_eq!(
            graph.nodes[add.0 as usize].flags,
            default_flags(NodeType::ValueAdd) | NODE_MAY_HAVE_DOUBLE_RESULT
        );
    }

    // ValueSub is NOT a makeSafe case label: C++'s Add arm lists ArithAdd,
    // ArithSub, ValueAdd and the bit ops (DFGByteCodeParser.cpp:1107-1119) —
    // ValueSub appears only at node creation (:8018) and falls to
    // `default: break` (:1204-1205), so even a PRESENT profile merges no
    // flags onto it.
    #[test]
    fn value_sub_with_profile_merges_no_flags() {
        let dst = local_register(1);
        let bytes = stream(|writer| {
            writer.emit(opcode_id::ENTER, &[]);
            writer.emit(
                opcode_id::SUB,
                &[
                    OperandValue::VirtualRegister(dst),
                    OperandValue::VirtualRegister(argument_register(1)),
                    OperandValue::VirtualRegister(argument_register(2)),
                    OperandValue::UnsignedImmediate(1),
                    OperandValue::OperandTypes(0),
                ],
            );
            writer.emit(opcode_id::RET, &[OperandValue::VirtualRegister(dst)]);
        });
        let code_block = code_block(bytes, 3, 1, Vec::new());
        {
            use crate::bytecode::{BinaryArithProfile, BinaryArithProfileSlot};
            let mut profile = BinaryArithProfile::default();
            profile.arith_mut().set_observed_non_neg_zero_double();
            // The sub starts at byte offset 1 (op_enter is one byte).
            code_block
                .side_tables()
                .binary_arith_profiles
                .borrow_mut()
                .push(BinaryArithProfileSlot {
                    bytecode_index: BytecodeIndex::from_offset(1),
                    profile,
                });
        }
        let graph = parse(&code_block).expect("in-slice body must parse");
        let sub = only_of_kind(&graph, NodeType::ValueSub);
        assert_eq!(
            graph.nodes[sub.0 as usize].flags,
            default_flags(NodeType::ValueSub)
        );
    }

    // Declining a CodeBlock with no raw packed stream (typed placeholder).
    #[test]
    fn typed_placeholder_streams_decline() {
        let unlinked = UnlinkedCodeBlock::new(
            CodeKind::Function,
            PackedInstructionStream::from_typed_placeholder(Vec::new()),
        );
        let code_block = CodeBlock::from_unlinked(unlinked, LinkContext::default());
        assert_eq!(
            parse(&code_block),
            Err(DeclineReason::NoRawPackedInstructionStream)
        );
    }

    // Constant dedup: two uses of the SAME constant index share one node
    // (m_constants, :388-410).
    #[test]
    fn same_constant_index_dedups_to_one_node() {
        let dst = local_register(1);
        let bytes = stream(|writer| {
            writer.emit(opcode_id::ENTER, &[]);
            emit_add(writer, dst, constant_register(0), constant_register(0));
            writer.emit(opcode_id::RET, &[OperandValue::VirtualRegister(dst)]);
        });
        let code_block = code_block(bytes, 1, 1, vec![double_constant(0, 1.5)]);
        let graph = parse(&code_block).expect("in-slice body must parse");
        assert_eq!(count_of_kind(&graph, NodeType::DoubleConstant), 1);
        let add = only_of_kind(&graph, NodeType::ValueAdd);
        assert_eq!(child(&graph, add, 0), child(&graph, add, 1));
    }
}
