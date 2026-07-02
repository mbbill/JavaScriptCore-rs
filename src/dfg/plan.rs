//! `DFG::Plan` — the parse-only synchronous slice.
//!
//! Faithful port of the graph-construction prefix of
//! `DFG::Plan::compileInThreadImpl` (dfg/DFGPlan.cpp:186-204): construct a
//! plan-owned `Graph` stamped with real graph/owner identities
//! (`Graph dfg(*m_vm, *this)`, DFGPlan.cpp:198 — `Graph::Graph(VM&, Plan&)`
//! reads `m_codeBlock` off the Plan, dfg/DFGGraph.cpp:78-90) and hand it to
//! the bytecode parser (`parse(dfg)`, DFGPlan.cpp:203). This closes the
//! follow-up parser.rs left at its `parse()` doc comment: "parse() creates
//! its own DfgGraph with null graph/owner ids; the plan unit should own graph
//! creation" (formerly DFGPlan.cpp:198's citation there).
//!
//! Everything after the parser call is out of this unit's slice —
//! `CPSRethreadingPhase` onward (DFGPlan.cpp:230+), the worklist/thread
//! machinery `JITPlan`/`Plan` otherwise carry, OSR entry, and FTL handoff.
//! `JITPlan` (jit/JITPlan.h:44-140) also carries `VM* m_vm`, `JITPlanStage`,
//! the compile-in-thread/finalize/cancel machinery, and signposts; `DFG::Plan`
//! adds `CodeBlock* m_profiledDFGCodeBlock`, `BytecodeIndex
//! m_osrEntryBytecodeIndex`, `Operands<optional<JSValue>> m_mustHandleValues`,
//! the finalizer/callback, and desired-{watchpoints, identifiers,
//! weakReferences, transitions} builders (DFGPlan.h:116-140). None of that is
//! reachable from a parse-only slice — no OSR entry, no profiled-alternative
//! lookup, no worklist thread — so it is deliberately deferred to the units
//! that need it rather than stubbed here.

use crate::bytecode::code_block::CodeBlock;
use crate::dfg::graph::{DfgGraph, DfgGraphId};
use crate::dfg::parser::{self, DeclineReason};
use crate::runtime::CodeBlockId;

/// `JITCompilationMode` (jit/JITCompilationMode.h:33-39: `InvalidCompilation,
/// Baseline, DFG, UnlinkedDFG, FTL, FTLForOSREntry`) narrowed to the variants
/// `DFG::Plan` itself is ever constructed with. `Plan::Plan` takes any
/// `JITCompilationMode` (DFGPlan.h:60-63), but in practice only
/// `DFG`/`UnlinkedDFG`/`FTL`/`FTLForOSREntry` name a `DFG::Plan` —
/// `InvalidCompilation` never compiles and `Baseline` routes through the
/// sibling `JIT::Plan`, never this one (see `profilerCompilationKindForMode`'s
/// `RELEASE_ASSERT_NOT_REACHED` on those two, DFGPlan.cpp:110-127).
///
/// This parse-only slice only ever constructs `Dfg`; the other three are
/// reserved for the OSR-entry and FTL-handoff units and are not yet read by
/// `compile_parse_only` (no phase branches on mode the way
/// `compileInThreadImpl` branches on `FTLForOSREntry`, DFGPlan.cpp:234).
#[derive(Clone, Copy, Debug, Eq, PartialEq)]
pub enum DfgCompilationMode {
    Dfg,
    UnlinkedDfg,
    Ftl,
    FtlForOsrEntry,
}

/// `DFG::Plan` (dfg/DFGPlan.h:56-141), narrowed to the parse-only prefix of
/// `compileInThreadImpl` (DFGPlan.cpp:186-204: `RecordedStatuses` init,
/// `Graph dfg(*m_vm, *this)`, `parse(dfg)`). Rust makes the `Graph` a field
/// instead of `compileInThreadImpl`'s stack local because there is no
/// worklist-thread call frame here to hold it — the plan itself is the
/// scope that owns it.
///
/// `owner` mirrors `JITPlan::m_codeBlock` (jit/JITPlan.h:140): a `CodeBlock*`
/// in C++, a `CodeBlockId` handle here, following the crate's non-arena
/// CodeBlock-reference idiom already used by
/// `jit::plan::CompilationRequest::owner: Option<CodeBlockId>`
/// (jit/plan.rs:4130-4132) — identity only, no live borrow. Live access goes
/// through the CodeBlock registry (`vm::code_blocks::CodeBlockRegistry`,
/// keyed by `CodeBlockId`) the same way `JITWorklistThread` revalidates
/// `m_codeBlock` rather than assuming the raw pointer stays valid across a
/// worklist boundary. `compile_parse_only` below takes the resolved
/// `&CodeBlock` as a parameter for exactly that reason: this plan-only slice
/// has no registry/VM access of its own to resolve `owner` itself.
///
/// `owner` is intentionally NOT duplicated as a separate field: `DfgGraph`
/// already carries `owner: CodeBlockId` (graph.rs:415), and the two must
/// never disagree, so `DfgPlan::owner()` simply reads it off the graph.
pub struct DfgPlan {
    graph: DfgGraph,
    mode: DfgCompilationMode,
}

impl DfgPlan {
    /// `Plan::Plan(CodeBlock*, CodeBlock*, JITCompilationMode, BytecodeIndex,
    /// Operands<optional<JSValue>>&&)` (DFGPlan.cpp:133-143), narrowed to the
    /// fields this slice carries (`owner`, `mode`) plus the graph identity
    /// (`id`) a real plan needs to mint once and stamp. C++ has no separate
    /// graph-id concept — a `Graph*`/`Graph&` IS its own identity; Rust's
    /// `DfgGraphId` exists because graphs here are values, not pointers, so
    /// something must name "this compilation's graph" stably. Until a
    /// crate-wide plan-id minting authority exists (no such counter is wired
    /// yet anywhere in `jit::plan`'s `CompilationPlanId` either — every
    /// existing caller passes an explicit literal), the caller supplies both
    /// ids explicitly, exactly like every existing `CompilationPlanId`
    /// call site.
    pub fn new(id: DfgGraphId, owner: CodeBlockId, mode: DfgCompilationMode) -> Self {
        Self {
            graph: DfgGraph::new(id, owner),
            mode,
        }
    }

    /// `JITPlan::codeBlock()` (jit/JITPlan.h:51): the plan's owning
    /// `CodeBlock` identity. See the struct doc for why this reads through
    /// `graph.owner` instead of a duplicated field.
    pub fn owner(&self) -> CodeBlockId {
        self.graph.owner
    }

    /// `JITPlan::mode()` (jit/JITPlan.h:56).
    pub fn mode(&self) -> DfgCompilationMode {
        self.mode
    }

    /// The graph this plan owns. Meaningful once `compile_parse_only`
    /// succeeds; before that (or after a decline — see `compile_parse_only`)
    /// it is the empty, correctly-stamped graph `new` produced.
    pub fn graph(&self) -> &DfgGraph {
        &self.graph
    }

    /// `Graph dfg(*m_vm, *this); ... parse(dfg)` (DFGPlan.cpp:198-203) — the
    /// graph is already built (in `new`); this runs only the parser call.
    /// Everything else in `compileInThreadImpl` between those two points
    /// (`RecordedStatuses` allocation, `cleanMustHandleValuesIfNecessary`,
    /// verbose-compilation logging) and everything after (`RUN_PHASE`
    /// onward, DFGPlan.cpp:206+) is out of this slice.
    ///
    /// `code_block` is the live borrow the caller resolved from `self.owner()`
    /// through the CodeBlock registry (see the struct doc); C++ reads the
    /// same bytecode through `m_codeBlock`, which `Graph::Graph` captured
    /// from the Plan at graph-construction time (DFGGraph.cpp:81).
    ///
    /// On `Err`, `self.graph` may hold a partially-parsed block (see
    /// `parser::parse_into`'s doc). C++'s `CancelPath` discards the whole
    /// `Plan`/`Graph` rather than repairing it (DFGPlan.cpp:204); callers
    /// here MUST do the same — drop this `DfgPlan` rather than reuse
    /// `self.graph()` after a decline.
    pub fn compile_parse_only(&mut self, code_block: &CodeBlock) -> Result<(), DeclineReason> {
        parser::parse_into(&mut self.graph, code_block)
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::bytecode::code_block::{
        CodeKind, LinkContext, UnlinkedCodeBlock, UnlinkedConstantPool,
    };
    use crate::bytecode::instruction_stream::{opcode_id, InstructionStreamWriter, OperandValue};
    use crate::bytecode::register::{RegisterFrameShape, SpecialRegisters};
    use crate::bytecode::{PackedInstructionStream, VirtualRegister};
    use crate::dfg::graph::{DfgPhase, GraphForm};
    use crate::dfg::node_type::NodeType;
    use crate::gc::CellId;

    const THIS_OFFSET: i32 = 5; // CallFrameSlot::thisArgument (CallFrame.h), same as parser.rs's tests.

    fn argument_register(argument: u32) -> i32 {
        THIS_OFFSET + argument as i32
    }

    /// Same shape as parser.rs's `code_block()` test helper (dfg/parser.rs),
    /// duplicated locally rather than exposed from `parser` — this is
    /// throwaway fixture plumbing with no JSC counterpart of its own, and
    /// growing `parser`'s test-only public surface just to share it would add
    /// more Rust-only surface than repeating four lines here.
    fn code_block(bytes: Vec<u8>, num_parameters_including_this: u32, num_vars: u32) -> CodeBlock {
        let unlinked = UnlinkedCodeBlock::new(
            CodeKind::Function,
            PackedInstructionStream::from_raw_packed_bytes(bytes),
        )
        .with_frame(RegisterFrameShape {
            num_parameters_including_this,
            num_vars,
            num_callee_locals: num_vars + 8,
            num_temporaries: 0,
            special: SpecialRegisters {
                scope_register: VirtualRegister::local(0),
                ..SpecialRegisters::default()
            },
        })
        .with_constants(UnlinkedConstantPool::default());
        CodeBlock::from_unlinked(unlinked, LinkContext::default())
    }

    /// `f(a) { return a; }` bytecode — the same identity-function fixture as
    /// parser.rs's `identity_function_returns_argument_through_prologue_node`.
    fn identity_function_code_block() -> CodeBlock {
        let mut writer = InstructionStreamWriter::new();
        writer.emit(opcode_id::ENTER, &[]);
        writer.emit(
            opcode_id::RET,
            &[OperandValue::VirtualRegister(argument_register(1))],
        );
        let bytes = writer.finalize().bytes().to_vec();
        code_block(bytes, 2, 1)
    }

    #[test]
    fn new_stamps_non_null_graph_and_owner_identities() {
        let owner = CodeBlockId(CellId(42));
        let plan = DfgPlan::new(DfgGraphId(7), owner, DfgCompilationMode::Dfg);

        assert_eq!(plan.graph().id, DfgGraphId(7));
        assert_eq!(plan.owner(), owner);
        assert_eq!(plan.graph().owner, owner);
        assert_ne!(plan.owner(), CodeBlockId(CellId(0)));
        assert_ne!(plan.graph().id, DfgGraphId(0));
        assert_eq!(plan.mode(), DfgCompilationMode::Dfg);
    }

    #[test]
    fn compile_parse_only_reproduces_the_parser_identity_function_graph_shape() {
        let owner = CodeBlockId(CellId(99));
        let mut plan = DfgPlan::new(DfgGraphId(1), owner, DfgCompilationMode::Dfg);
        let code_block = identity_function_code_block();

        plan.compile_parse_only(&code_block)
            .expect("in-slice body must parse");

        let graph = plan.graph();
        // Real, plan-stamped identities survive parsing untouched.
        assert_eq!(graph.id, DfgGraphId(1));
        assert_eq!(graph.owner, owner);

        // Same graph shape as parser.rs's identity-function test: one
        // LoadStore-form block with the two-argument prologue
        // (SetArgumentDefinitely x2) followed by a Return.
        assert_eq!(graph.form, GraphForm::LoadStore);
        assert_eq!(graph.blocks.len(), 1);
        assert_eq!(graph.validate(), Ok(()));
        let ops: Vec<NodeType> = graph.blocks[0]
            .nodes
            .iter()
            .map(|id| graph.nodes[id.0 as usize].op)
            .collect();
        assert_eq!(
            &ops[..2],
            &[
                NodeType::SetArgumentDefinitely,
                NodeType::SetArgumentDefinitely
            ]
        );
        assert_eq!(ops.iter().filter(|op| **op == NodeType::Return).count(), 1);
    }

    #[test]
    fn compile_parse_only_surfaces_decline_reason_through_the_plan() {
        let owner = CodeBlockId(CellId(5));
        let mut plan = DfgPlan::new(DfgGraphId(1), owner, DfgCompilationMode::Dfg);
        // A body that never reaches op_ret declines
        // (DeclineReason::BodyDoesNotEndWithRet) rather than panicking or
        // silently producing a partial graph the caller might mistake for
        // success.
        let mut writer = InstructionStreamWriter::new();
        writer.emit(opcode_id::ENTER, &[]);
        let bytes = writer.finalize().bytes().to_vec();
        let code_block = code_block(bytes, 1, 0);

        let result = plan.compile_parse_only(&code_block);
        assert_eq!(result, Err(DeclineReason::BodyDoesNotEndWithRet));
    }

    #[test]
    fn no_phase_runs_form_stays_load_store_after_parse_only() {
        let owner = CodeBlockId(CellId(11));
        let mut plan = DfgPlan::new(DfgGraphId(1), owner, DfgCompilationMode::Dfg);
        let code_block = identity_function_code_block();

        plan.compile_parse_only(&code_block)
            .expect("in-slice body must parse");

        // `m_form(LoadStore)` at graph construction (dfg/DFGGraph.cpp:87);
        // CPSRethreadingPhase (the first phase after parsing, DFGPlan.cpp:237)
        // is what would move it to ThreadedCPS, and it is out of this slice.
        assert_eq!(plan.graph().phase, DfgPhase::BytecodeParsing);
        assert_eq!(plan.graph().form, GraphForm::LoadStore);
    }
}
