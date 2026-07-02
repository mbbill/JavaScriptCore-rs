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
    use crate::bytecode::instruction::Operand;
    use crate::bytecode::instruction_stream::{
        decode_raw_instruction, opcode_id, InstructionStreamWriter, OperandValue,
    };
    use crate::bytecode::opcode::CoreOpcode;
    use crate::bytecode::register::{RegisterFrameShape, SpecialRegisters};
    use crate::bytecode::{PackedInstructionStream, VirtualRegister};
    use crate::bytecompiler::{
        bytecompiler_input_from_parsed_ast, emit_unlinked_code_from_parsed_ast,
        BytecompilerOutputPlan, BytecompilerSessionId,
    };
    use crate::dfg::graph::{DfgPhase, GraphForm};
    use crate::dfg::node_type::NodeType;
    use crate::gc::CellId;
    use crate::syntax::source::{
        SourceOrigin, SourcePosition, SourceProvider, SourceSpan, SourceText,
    };
    use crate::syntax::{AstBuilder, Parser, ParserArena, SourceCode};
    use std::sync::Arc;

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

    // === G4-Unit-1: real bytecompiler output -> raw packed bytes -> DFG =====
    //
    // The tests above all hand-build raw bytes via `InstructionStreamWriter`
    // directly (`code_block`/`identity_function_code_block`). These tests
    // instead drive the REAL front end
    // (`Parser` -> `bytecompiler_input_from_parsed_ast` ->
    // `emit_unlinked_code_from_parsed_ast`, the same pipeline
    // `bytecompiler::tests::try_emit_program_plan_with_global_bindings` uses)
    // so the `UnlinkedCodeBlock`s under test are the ones a real script would
    // produce, exercising `PackedInstructionStream::with_raw_encoded_from_declarations`
    // (`bytecode/generator.rs`'s `BytecodeGenerator::finish()` hook) instead
    // of a fixture.

    fn compile_program_source(text: &str, session: u64) -> BytecompilerOutputPlan {
        let provider = Arc::new(SourceProvider::new(
            SourceOrigin::default(),
            SourceText::Latin1(text.as_bytes().to_vec()),
        ));
        let source = SourceCode::new(
            provider,
            SourceSpan::new(SourcePosition(0), SourcePosition(text.len() as u32)),
        );
        let mut arena = ParserArena::new();
        let parsed = Parser::with_mode(
            &mut arena,
            AstBuilder::default(),
            &source,
            crate::syntax::ParseMode::Program,
        )
        .parse()
        .expect("source parses");
        let input = bytecompiler_input_from_parsed_ast(
            BytecompilerSessionId(session),
            source.clone(),
            &parsed,
            &arena,
        )
        .expect("bytecompiler handoff succeeds");
        emit_unlinked_code_from_parsed_ast(&input, &arena).expect("bytecode emission succeeds")
    }

    /// Compile `text` (a whole program consisting of one function
    /// declaration) and return that function's OWN `UnlinkedCodeBlock`
    /// (`plan.function_bodies`), not the enclosing program's.
    fn compile_single_function_body(text: &str, session: u64) -> UnlinkedCodeBlock {
        let mut plan = compile_program_source(text, session);
        assert_eq!(
            plan.function_bodies.len(),
            1,
            "fixture must declare exactly one function"
        );
        plan.function_bodies.remove(0)
    }

    /// `var f = function(a,b){var t=a+b; return t-b};` compiles entirely
    /// within the G4-Unit-1 supported family (`op_enter`/`op_mov`/`op_ret`/
    /// `op_add`/`op_sub`/`op_mul`), and needs two Rust-vs-JSC divergences the
    /// fixture must dodge rather than this unit fixing:
    ///   1. A NAMED function (declaration or named expression) unconditionally
    ///      binds its own name to `CoreOpcode::LoadCallee` at entry
    ///      (`emit_current_function_name_binding`, bytecompiler/mod.rs:3036-3038)
    ///      regardless of whether the body ever references that name — so
    ///      this uses an ANONYMOUS function expression.
    ///   2. Numeric/string/bool LITERALS lower to Rust-only immediate-carrying
    ///      pseudo-opcodes (`CoreOpcode::LoadInt32` etc., `emit_load_int32`,
    ///      bytecompiler/mod.rs:6693-6710) instead of JSC's real
    ///      `op_mov dst, constant(i)` (constant-pool load) — JSC has no
    ///      `op_load_int32` at all. This is a separate, already-known,
    ///      load-bearing divergence (constant-pool wiring) well beyond this
    ///      unit's scope, so this fixture uses ONLY parameter/local operands
    ///      (`t-b`, not a literal) to stay entirely in the currently-real,
    ///      currently-supported family. `derive_binary_arith_profiles`
    /// (`bytecode/code_block.rs`) sees `AddInt32` then `SubInt32`, so this
    /// also exercises the encoder's profileIndex derivation (0 then 1)
    /// without div/bitwise/shift interference.
    #[test]
    fn real_bytecompiler_output_encodes_raw_packed_bytes_matching_declarations() {
        let unlinked =
            compile_single_function_body("var f = function(a,b){var t=a+b; return t-b;};", 1);
        let declarations = unlinked.instructions().declarations();
        assert!(
            !declarations.is_empty(),
            "a real function body must declare instructions"
        );
        for declaration in declarations {
            let core = CoreOpcode::from_opcode(declaration.opcode).unwrap_or_else(|| {
                panic!(
                    "declared opcode {:?} has no CoreOpcode — fixture picked an unsupported opcode",
                    declaration.opcode
                )
            });
            assert!(
                matches!(
                    core,
                    CoreOpcode::Move
                        | CoreOpcode::Return
                        | CoreOpcode::AddInt32
                        | CoreOpcode::SubInt32
                        | CoreOpcode::MulInt32
                ),
                "fixture emitted {core:?}, outside the G4-Unit-1 supported family; \
                 pick a narrower fixture or extend the encoder first"
            );
        }

        let raw = unlinked
            .instructions()
            .raw_bytes()
            .expect("an all-in-family body must encode raw packed bytes");

        // op_enter is SYNTHESIZED (BytecodeGenerator.cpp:1439-1449; see the
        // encoder's doc) — it has no `declarations` counterpart, so decode it
        // separately before walking `declarations` 1:1 against the rest.
        let mut offset = 0usize;
        let enter = decode_raw_instruction(raw, offset).expect("op_enter decodes");
        assert_eq!(enter.opcode_id, opcode_id::ENTER);
        assert!(enter.operands.is_empty());
        offset += enter.size;

        for declaration in declarations {
            let decoded =
                decode_raw_instruction(raw, offset).expect("declared instruction decodes");
            let core = CoreOpcode::from_opcode(declaration.opcode).unwrap();
            let expected_id = match core {
                CoreOpcode::Move => opcode_id::MOV,
                CoreOpcode::Return => opcode_id::RET,
                CoreOpcode::AddInt32 => opcode_id::ADD,
                CoreOpcode::SubInt32 => opcode_id::SUB,
                CoreOpcode::MulInt32 => opcode_id::MUL,
                other => panic!("unexpected in-family opcode {other:?}"),
            };
            assert_eq!(decoded.opcode_id, expected_id);

            let expected_registers: Vec<i32> = declaration
                .operands
                .iter()
                .map(|operand| {
                    operand
                        .as_register()
                        .expect("supported family carries only register operands")
                        .raw()
                })
                .collect();
            let decoded_registers: Vec<i32> = decoded.operands[..expected_registers.len()]
                .iter()
                .map(|value| *value as i32)
                .collect();
            assert_eq!(
                decoded_registers, expected_registers,
                "raw operand order must match the declaration's operand order"
            );
            offset += decoded.size;
        }
        assert_eq!(
            offset,
            raw.len(),
            "no bytes past the last declared instruction"
        );

        // THE PAYOFF: the first DFG parse of real bytecompiler output.
        let owner = CodeBlockId(CellId(1));
        let mut dfg_plan = DfgPlan::new(DfgGraphId(1), owner, DfgCompilationMode::Dfg);
        let code_block = CodeBlock::from_unlinked(unlinked, LinkContext::default());
        dfg_plan
            .compile_parse_only(&code_block)
            .expect("real bytecompiler output must parse");

        let graph = dfg_plan.graph();
        assert_eq!(graph.form, GraphForm::LoadStore);
        assert_eq!(graph.validate(), Ok(()));
        let ops: Vec<NodeType> = graph.blocks[0]
            .nodes
            .iter()
            .map(|id| graph.nodes[id.0 as usize].op)
            .collect();
        // `this` plus two declared parameters (a, b): the argument prologue
        // covers `numArguments = numParametersIncludingThis`
        // (DFGByteCodeParser.cpp:7473-7494, ctor :140).
        assert_eq!(
            ops.iter()
                .filter(|op| **op == NodeType::SetArgumentDefinitely)
                .count(),
            3
        );
        assert_eq!(ops.iter().filter(|op| **op == NodeType::Return).count(), 1);
        // `a+b`/`t-b`: GetLocal/GetArgument operands are NodeResultJS, not
        // NodeResultNumber, so `hasNumberResult()` is false and the parser
        // emits the Value* form (parse_block's op_add/op_sub/op_mul doc).
        assert!(ops.contains(&NodeType::ValueAdd) || ops.contains(&NodeType::ArithAdd));
        assert!(ops.contains(&NodeType::ValueSub) || ops.contains(&NodeType::ArithSub));
    }

    /// A body outside the supported family (`op_get_by_id`, from `o.x`)
    /// leaves `raw` empty — unchanged decline behavior — even though it went
    /// through the SAME real pipeline as the test above.
    #[test]
    fn real_bytecompiler_output_outside_supported_family_declines_with_unchanged_reason() {
        let unlinked = compile_single_function_body("function g(o){ return o.x; }", 2);
        assert!(
            unlinked.instructions().raw_bytes().is_none(),
            "an out-of-family body must not encode raw bytes"
        );

        let owner = CodeBlockId(CellId(2));
        let mut dfg_plan = DfgPlan::new(DfgGraphId(1), owner, DfgCompilationMode::Dfg);
        let code_block = CodeBlock::from_unlinked(unlinked, LinkContext::default());
        let result = dfg_plan.compile_parse_only(&code_block);

        assert_eq!(result, Err(DeclineReason::NoRawPackedInstructionStream));
    }
}
