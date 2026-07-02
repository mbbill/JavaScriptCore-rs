//! DFG OSR-exit landing: reify a resumable interpreter frame at an exit site.
//!
//! Faithful port of the exit-site machinery in C++ `dfg/DFGOSRExit.cpp`
//! (`OSRExit::compileExit`, :270-889 — value recovery writes every recovered
//! operand in BOXED form into its fp-relative virtual-register slot,
//! :759-836) and `dfg/DFGOSRExitCompilerCommon.cpp`
//! (`reifyInlinedCallFrames`, :290-412 — call-frame header slot writes;
//! `adjustAndJumpToTarget`, :461-506 — the `exitToLLInt` arm that hands the
//! reified frame to the interpreter). Per the ratified U0 decision, the first
//! Rust OSR exit lands in the INTERPRETER tier — JSC's own
//! `exitToLLInt` path (`Options::forceOSRExitToLLInt() || codeBlockForExit->
//! jitType() == JITType::InterpreterThunk`, DFGOSRExitCompilerCommon.cpp:468).
//!
//! SINGLE-FRAME SUBSET: `reifyInlinedCallFrames` walks the inline stack; no
//! inlining exists in the Rust DFG yet, so only the top-level arm is ported
//! (the `codeOrigin` tail at :406-411 plus the header writes every frame
//! receives). Inline-frame reification lands with DFG inlining.
//!
//! C++ header-slot -> Rust frame-model mapping (the Rust interpreter frame is
//! `InstalledCallFrame` + a `RegisterFile` window + the JsStack arena shadow;
//! see interpreter/mod.rs `push_frame`/`dual_write_shadow_frame` and
//! docs/design/jsstack.md):
//!
//! | C++ write (file:line)                                              | Rust equivalent                                                        |
//! |--------------------------------------------------------------------|------------------------------------------------------------------------|
//! | codeBlock @ CallFrameSlot::codeBlock (CompilerCommon.cpp:296)       | `FramePushRequest::code_block` + arena slot 2 via `code_block_ptr`      |
//! | callee @ CallFrameSlot::callee (:403)                               | `FramePushRequest::callee`/`callee_value` + arena slot 3                |
//! | argumentCountIncludingThis payload (:396)                           | `FramePushRequest::argument_count_including_this` + arena slot 4        |
//! | CallSiteIndex tag bits = exit BytecodeIndex (:401/:410)             | `InstalledCallFrame::bytecode_index` (`start_bytecode_index`)           |
//! | returnPC (:338/:370)                                                | `InstalledCallFrame::return_address` — a caller `BytecodeIndex`, not a  |
//! |                                                                     | machine PC (divergence: the Vec-frame model has no native return PC     |
//! |                                                                     | until B7/JIT frames; arena slot 1 mirrors the index bits)               |
//! | callerFrame (:397)                                                  | `InstalledCallFrame::caller` — `push_frame` links the live top frame    |
//! | operand value-recovery stores (DFGOSRExit.cpp:773-836)              | `RegisterFile::write(window, vreg, value)` per recovered operand        |
//! | pcGPR = bytecodeIndex.offset() (CompilerCommon.cpp:484)             | `BaselineFallbackRequest::bytecode_index` (the resume pc)               |
//! | metadataTable -> LLInt::Registers::metadataTableGPR (:482)          | the linked `CodeBlock` handed to `execute_baseline_fallback` (metadata  |
//! |                                                                     | lives ON the Rust `CodeBlock`)                                          |
//! | pbGPR = instructionsRawPointer() (:483)                             | `code_block.unlinked().instructions()` — the dispatch cursor's stream   |
//!
//! This module depends on `crate::interpreter` exactly as C++
//! `DFGOSRExitCompilerCommon.cpp` includes `LLIntData.h`/`LLIntThunks.h`: the
//! DFG's off-ramp targets the interpreter tier.

use crate::bytecode::{BytecodeIndex, Checkpoint, CodeBlock, Operands};
use crate::interpreter::{
    code_block_contains_instruction_start, execute_baseline_fallback, BaselineFallbackRequest,
    DispatchConfig, DispatchHost, ExecutionCompletion, ExecutionContextStack, ExecutionError,
    FramePushRequest, InterpreterExecutionState, RegisterFile,
};
use crate::runtime::{CodeBlockId, ObjectId, RuntimeValue, ScopeId};

/// Identity of the frame being exited into — the header state
/// `reifyInlinedCallFrames` writes besides the code block.
///
/// In C++ the exiting DFG frame is physically the SAME machine frame the
/// baseline resumes in, so callee / dynamic argument count / caller linkage
/// already sit in it and `reifyInlinedCallFrames` only (re)writes what the
/// exit changes. The Rust interpreter frame is rebuilt from scratch here, so
/// the surviving identity must be carried explicitly.
#[derive(Clone, Debug)]
pub(crate) struct OsrExitTargetFrameIdentity {
    /// `CallFrameSlot::callee` (DFGOSRExitCompilerCommon.cpp:403).
    pub callee: Option<ObjectId>,
    pub callee_value: Option<RuntimeValue>,
    /// The frame's scope register identity (no reify write; survives the exit).
    pub lexical_scope: Option<ScopeId>,
    /// DYNAMIC `argumentCountIncludingThis` (DFGOSRExitCompilerCommon.cpp:396)
    /// — may differ from the code block's `numParameters`.
    pub argument_count_including_this: u32,
    /// Values of dynamic arguments BEYOND the recovered `Operands` width
    /// (`operands.numberOfArguments()` == the DFG code block's numParameters;
    /// over-supplied arguments are not operands). In C++ these persist
    /// physically in the machine frame across the exit; the from-scratch Rust
    /// rebuild must carry them explicitly.
    pub extra_arguments: Vec<RuntimeValue>,
    /// `InstalledCallFrame::return_address` — the caller-side resume
    /// `BytecodeIndex`, the Vec-frame model's returnPC analog (see module
    /// table; C++ stores a machine return PC, :338/:370).
    pub return_bytecode_index: Option<BytecodeIndex>,
}

/// Materialize a live, resumable interpreter frame for an OSR exit.
///
/// The Rust analog of `OSRExit::compileExit`'s frame-state half for ONE frame:
/// value recovery (DFGOSRExit.cpp:759-836) + `reifyInlinedCallFrames`'s header
/// writes (DFGOSRExitCompilerCommon.cpp:290-412) + `adjustAndJumpToTarget`'s
/// exit-index resolution (:461-506). On success the frame is the live top
/// frame, positioned at `exit_index`, and the returned request is the
/// validated resume boundary `resume_interpreter_exit_frame` consumes.
///
/// `recovered` is Operands-shaped (arguments-including-this first, then
/// locals, then tmps — bytecode/Operands.h:138): every `Some` is an
/// already-BOXED `RuntimeValue` (the C++ scratch buffer holds boxed
/// `EncodedJSValue`s by the time of the stack stores, DFGOSRExit.cpp:759).
/// `None` is a dead operand: C++ writes nothing and the slot keeps stale frame
/// bytes; the fresh Rust window slot stays the `allocate_frame`-seeded
/// `undefined` (observably equivalent — the operand is dead). Tmp entries are
/// skipped exactly as C++ skips `operand.isTmp()` (DFGOSRExit.cpp:775-776);
/// checkpoint resume does not exist in the Rust interpreter.
///
/// `code_block_ptr` is the registry's stable `*const CodeBlock` for the arena
/// shadow's slot-2 seed (K1, see `push_frame`); `None` in bare-stack tests.
// Unwired until the speculative DFG emits live exits (U3+); exercised by the
// synthetic-exit tests below.
#[allow(dead_code)]
pub(crate) fn materialize_interpreter_exit_frame(
    stack: &mut ExecutionContextStack,
    registers: &mut RegisterFile,
    code_block_id: CodeBlockId,
    code_block: &CodeBlock,
    code_block_ptr: Option<*const CodeBlock>,
    exit_index: BytecodeIndex,
    recovered: &Operands<Option<RuntimeValue>>,
    identity: OsrExitTargetFrameIdentity,
) -> Result<BaselineFallbackRequest, ExecutionError> {
    // C++ JSC divergence (loud subset guard): `adjustAndJumpToTarget` routes a
    // checkpointed exit index to `checkpointOSRExitTrampolineThunk`
    // (DFGOSRExitCompilerCommon.cpp:470-473). The Rust interpreter has no
    // checkpoint-resume machinery yet, so a checkpointed exit is rejected
    // loudly instead of resuming at the wrong instruction boundary.
    if !exit_index.is_valid() || exit_index.checkpoint() != Checkpoint::NONE {
        return Err(ExecutionError::InvalidBytecodeIndex(exit_index));
    }

    // `adjustAndJumpToTarget` resolves the exit instruction and asserts it is
    // a real instruction start (`instructions().at(bytecodeIndex)`, :466-467;
    // `ASSERT(codeBlockForExit->bytecodeIndexForExit(exitIndex) == exitIndex)`,
    // :496). Same check as the dispatch loop's own resume validation, BEFORE
    // any frame mutation.
    match code_block_contains_instruction_start(code_block, exit_index) {
        Ok(true) => {}
        Ok(false) => return Err(ExecutionError::InvalidBytecodeIndex(exit_index)),
        Err(error) => return Err(ExecutionError::BytecodeDecode(error)),
    }

    // Operands shape: argument(0) is `this` (bytecode/Operands.h — arguments
    // occupy the front; DFG argument 0 is the this-argument), so a recovery
    // map with no argument entries cannot describe a frame.
    let recovered_argument_count = recovered.number_of_arguments();
    if recovered_argument_count == 0 || identity.argument_count_including_this == 0 {
        return Err(ExecutionError::InvalidArgumentCount);
    }
    // Verifier finding (U2 review): the Operands width is a trusted caller
    // invariant — a violating caller would build a malformed-but-plausible
    // frame. Catch it loudly in debug: the recovery map's argument width must
    // match the exited CodeBlock's parameter count (C++'s Operands is sized
    // from the CodeBlock itself, so the mismatch cannot arise there).
    debug_assert_eq!(
        recovered_argument_count,
        code_block.unlinked().frame().num_parameters_including_this as usize,
        "OSR-exit recovery map width must match the exited CodeBlock's parameters"
    );
    // Dynamic arguments beyond the Operands width must be carried explicitly
    // (see `OsrExitTargetFrameIdentity::extra_arguments`); under-supply needs
    // no extras (the pad below is undefined, the arity-fixup analog).
    let expected_extras =
        (identity.argument_count_including_this as usize).saturating_sub(recovered_argument_count);
    if identity.extra_arguments.len() != expected_extras {
        return Err(ExecutionError::InvalidArgumentCount);
    }

    // Argument slots: the compileExit argument-operand stores (DFGOSRExit.cpp
    // :773-836 for `operand.isArgument()`) land here as the window's seeded
    // argument values — `allocate_frame` writes exactly these slots.
    let mut argument_values: Vec<RuntimeValue> = (0..recovered_argument_count)
        .map(|index| {
            recovered
                .argument(index)
                .unwrap_or_else(RuntimeValue::undefined)
        })
        .collect();
    argument_values.extend_from_slice(&identity.extra_arguments);

    // C++ JSC divergence (inherited from the interpreter call path): the
    // stored `argumentCountIncludingThis` is the PADDED count
    // max(dynamic, numParameters) with undefined fill, mirroring what the Rust
    // interpreter's own call entry stores (interpreter/mod.rs:13109-13154),
    // so a materialized frame is indistinguishable from a normally-called one.
    // C++ keeps the DYNAMIC count in the header payload (reify :396) and pads
    // only the physical slots via arity fixup; correcting that count model is
    // an interpreter-wide divergence to fix at the call path, not to fork here.
    let formals = code_block.unlinked().frame().num_parameters_including_this;
    let padded_argument_count = (argument_values.len() as u32)
        .max(identity.argument_count_including_this)
        .max(formals);

    let frame = stack.push_frame(
        registers,
        FramePushRequest {
            code_block: Some(code_block_id),
            callee: identity.callee,
            callee_value: identity.callee_value,
            lexical_scope: identity.lexical_scope,
            shape: code_block.unlinked().frame(),
            argument_count_including_this: padded_argument_count,
            argument_values,
            // The CallSiteIndex tag-bits write (reify :401/:410): the frame's
            // current bytecode index IS the exit index.
            start_bytecode_index: Some(exit_index),
            return_bytecode_index: identity.return_bytecode_index,
        },
        code_block_ptr,
    )?;
    let window = match stack.top_frame() {
        Some(top) if top.id == frame => top.register_window,
        _ => return Err(ExecutionError::NoActiveFrame),
    };

    // Value recovery for LOCAL operands (DFGOSRExit.cpp:773-836: boxed store
    // to `operand.virtualRegister().offset() * sizeof(CPURegister)` off fp).
    // The RegisterFile window write is the fp-relative slot store analog (and
    // dual-writes the JsStack arena at fp + vreg.raw()*8, the literal C++
    // address). Tmps are skipped (:775-776). A failed write (recovery map
    // wider than the frame's local capacity) unwinds the half-built frame and
    // reports loudly — C++ cannot express this state (operands are sized from
    // the same code block).
    for index in 0..recovered.number_of_locals() {
        let Some(value) = *recovered.local(index) else {
            continue;
        };
        let register = crate::bytecode::VirtualRegister::local(index as u32);
        if let Err(error) = registers.write(window, register, value) {
            let _ = stack.pop_frame(registers, frame);
            return Err(error);
        }
    }

    // GC visibility: nothing extra to wire. The recovered values now live in
    // the RegisterFile's contiguous backing store (`allocate_frame` grew it;
    // the writes above stored into it), which the safepoint gather walks in
    // full (`gather_vm_register_roots`, the CLoopStack::gatherConservativeRoots
    // analog), and the header cells (code block / callee) are gathered off the
    // live frame chain (`gather_vm_frame_header_roots`). A materialized-but-
    // not-yet-resumed frame is therefore rooted exactly like any live frame —
    // proven by `recovered_string_cell_survives_collection_between_
    // materialize_and_resume` below.

    Ok(BaselineFallbackRequest::new(
        code_block_id,
        frame,
        exit_index,
    ))
}

/// Resume the interpreter from a materialized exit frame.
///
/// The `adjustAndJumpToTarget` exitToLLInt hand-off analog
/// (DFGOSRExitCompilerCommon.cpp:479-485): pcGPR carries the exit bytecode
/// index (`request.bytecode_index`), metadataTable/pbGPR carry the target code
/// block's metadata and instruction stream (both live ON `code_block` in the
/// Rust port), and the far jump into `normalOSRExitTrampolineThunk` becomes
/// entering the dispatch loop mid-function through the SAME validated resume
/// boundary the baseline side exits use (`execute_baseline_fallback`, which
/// re-validates frame/code-block/index before dispatching).
// Unwired until the speculative DFG emits live exits (U3+); exercised by the
// synthetic-exit tests below.
#[allow(dead_code)]
pub(crate) fn resume_interpreter_exit_frame<H: DispatchHost>(
    execution: InterpreterExecutionState<'_>,
    request: BaselineFallbackRequest,
    code_block: &CodeBlock,
    host: &mut H,
    config: DispatchConfig,
) -> ExecutionCompletion {
    execute_baseline_fallback(execution, request, code_block, host, config)
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::bytecode::instruction_stream::opcode_id;
    use crate::bytecode::register::CallFrameSlotLayout;
    use crate::bytecode::{
        CodeKind, ConstantValue, CoreOpcode, LinkContext, Operand, OperandWidth,
        PackedInstructionStream, RegisterFrameShape, SourceCodeRepresentation, TypedInstruction,
        UnlinkedCodeBlock, UnlinkedConstant, UnlinkedConstantPool, VirtualRegister,
    };
    use crate::gc::{CellId, Heap};
    use crate::interpreter::{
        execute_code_block, CoreOpcodeDispatchHost, ExecutionEntryRecord, ProgramExecutionEntry,
    };
    use crate::runtime::{GlobalObjectId, ObjectId};
    use crate::vm::ExceptionState;

    fn bci(offset: u32) -> BytecodeIndex {
        BytecodeIndex::from_offset(offset)
    }

    fn local(index: u32) -> VirtualRegister {
        VirtualRegister::local(index)
    }

    fn argument_including_this(index: u32) -> VirtualRegister {
        VirtualRegister::argument_including_this(
            index,
            CallFrameSlotLayout::JSC_RUST.this_argument_offset,
        )
    }

    fn core_typed(offset: u32, opcode: CoreOpcode, operands: Vec<Operand>) -> TypedInstruction {
        TypedInstruction {
            opcode: opcode.opcode(),
            width: OperandWidth::Narrow,
            operands,
            schema: None,
            bytecode_index: Some(bci(offset)),
        }
    }

    fn function_code_block(
        instructions: Vec<TypedInstruction>,
        parameters_including_this: u32,
        num_vars: u32,
    ) -> CodeBlock {
        CodeBlock::from_unlinked(
            UnlinkedCodeBlock::new(
                CodeKind::Function,
                PackedInstructionStream::from_typed_placeholder(instructions),
            )
            .with_frame(RegisterFrameShape {
                num_parameters_including_this: parameters_including_this,
                num_vars,
                num_callee_locals: 0,
                num_temporaries: 0,
                special: Default::default(),
            }),
            LinkContext::default(),
        )
    }

    /// f(a, b) { var t = a + b; return t * 2; } — ordinal typed stream:
    ///   0: add   loc0, arg1, arg2
    ///   1: mov   loc1, #2 (LoadInt32)
    ///   2: mul   loc2, loc0, loc1
    ///   3: ret   loc2
    fn add_double_code_block() -> CodeBlock {
        function_code_block(
            vec![
                core_typed(
                    0,
                    CoreOpcode::AddInt32,
                    vec![
                        Operand::Register(local(0)),
                        Operand::Register(argument_including_this(1)),
                        Operand::Register(argument_including_this(2)),
                    ],
                ),
                core_typed(
                    1,
                    CoreOpcode::LoadInt32,
                    vec![Operand::Register(local(1)), Operand::SignedImmediate(2)],
                ),
                core_typed(
                    2,
                    CoreOpcode::MulInt32,
                    vec![
                        Operand::Register(local(2)),
                        Operand::Register(local(0)),
                        Operand::Register(local(1)),
                    ],
                ),
                core_typed(3, CoreOpcode::Return, vec![Operand::Register(local(2))]),
            ],
            3,
            3,
        )
    }

    fn enter_test_entry(stack: &mut ExecutionContextStack, code_block_id: CodeBlockId) {
        stack.enter(ExecutionEntryRecord::Program(ProgramExecutionEntry {
            code_block: code_block_id,
            global_object: GlobalObjectId(ObjectId(CellId(1))),
            this_value: RuntimeValue::undefined(),
        }));
    }

    struct TestVmState {
        stack: ExecutionContextStack,
        registers: RegisterFile,
        exceptions: ExceptionState,
        heap: Heap,
    }

    impl TestVmState {
        fn new(code_block_id: CodeBlockId) -> Self {
            let mut state = Self {
                stack: ExecutionContextStack::default(),
                registers: RegisterFile::default(),
                exceptions: ExceptionState::default(),
                heap: Heap::new(),
            };
            enter_test_entry(&mut state.stack, code_block_id);
            state
        }

        fn execution(&mut self) -> InterpreterExecutionState<'_> {
            InterpreterExecutionState {
                stack: &mut self.stack,
                registers: &mut self.registers,
                exceptions: &mut self.exceptions,
                heap: &mut self.heap,
            }
        }
    }

    /// The reference run: a plain interpreter call from bytecode index 0,
    /// pushing the frame the way the interpreter's own call path does —
    /// argument values undefined-padded to the formal parameter count
    /// (interpreter/mod.rs:13109-13154).
    fn run_reference_call(
        code_block_id: CodeBlockId,
        code_block: &CodeBlock,
        mut argument_values: Vec<RuntimeValue>,
        host: &mut CoreOpcodeDispatchHost,
    ) -> ExecutionCompletion {
        let mut state = TestVmState::new(code_block_id);
        let formals = code_block.unlinked().frame().num_parameters_including_this as usize;
        while argument_values.len() < formals {
            argument_values.push(RuntimeValue::undefined());
        }
        state
            .stack
            .push_frame(
                &mut state.registers,
                FramePushRequest {
                    code_block: Some(code_block_id),
                    callee: None,
                    callee_value: None,
                    lexical_scope: None,
                    shape: code_block.unlinked().frame(),
                    argument_count_including_this: argument_values.len() as u32,
                    argument_values,
                    start_bytecode_index: Some(bci(0)),
                    return_bytecode_index: None,
                },
                None,
            )
            .expect("reference frame push");
        execute_code_block(
            state.execution(),
            code_block_id,
            code_block,
            host,
            DispatchConfig::default(),
        )
    }

    fn recovered_operands(
        arguments: Vec<Option<RuntimeValue>>,
        locals: Vec<Option<RuntimeValue>>,
    ) -> Operands<Option<RuntimeValue>> {
        let mut operands = Operands::new(arguments.len(), locals.len(), 0);
        for (index, value) in arguments.into_iter().enumerate() {
            *operands.argument_mut(index) = value;
        }
        for (index, value) in locals.into_iter().enumerate() {
            *operands.local_mut(index) = value;
        }
        operands
    }

    fn identity_with_argument_count(
        argument_count_including_this: u32,
    ) -> OsrExitTargetFrameIdentity {
        OsrExitTargetFrameIdentity {
            callee: None,
            callee_value: None,
            lexical_scope: None,
            argument_count_including_this,
            extra_arguments: Vec::new(),
            return_bytecode_index: None,
        }
    }

    /// (a) Resume soundness at a mid-function exit: an exit AFTER the add with
    /// recovered {this, a, b, t} completes with the SAME value the normal run
    /// produces.
    #[test]
    fn mid_function_exit_resumes_to_the_reference_completion() {
        let block = add_double_code_block();
        let owner = CodeBlockId(CellId(9001));
        let args = vec![
            RuntimeValue::undefined(),
            RuntimeValue::from_i32(3),
            RuntimeValue::from_i32(4),
        ];

        let mut reference_host = CoreOpcodeDispatchHost::new();
        let reference = run_reference_call(owner, &block, args.clone(), &mut reference_host);
        assert_eq!(
            reference,
            ExecutionCompletion::Returned(RuntimeValue::from_i32(14)),
            "reference run of f(3, 4) returns (3+4)*2"
        );

        // Exit at bci 1 (the LoadInt32 AFTER the add): a, b recovered as
        // arguments, t = 7 recovered into local 0; locals 1/2 are dead.
        let mut state = TestVmState::new(owner);
        let recovered = recovered_operands(
            args.iter().copied().map(Some).collect(),
            vec![Some(RuntimeValue::from_i32(7)), None, None],
        );
        let request = materialize_interpreter_exit_frame(
            &mut state.stack,
            &mut state.registers,
            owner,
            &block,
            None,
            bci(1),
            &recovered,
            identity_with_argument_count(3),
        )
        .expect("materialization succeeds at an instruction start");
        assert_eq!(request.bytecode_index, bci(1));
        assert_eq!(
            state.stack.top_frame().map(|frame| frame.bytecode_index),
            Some(Some(bci(1))),
            "the frame is positioned at the exit index (the CallSiteIndex write)"
        );

        let mut host = CoreOpcodeDispatchHost::new();
        let completion = resume_interpreter_exit_frame(
            state.execution(),
            request,
            &block,
            &mut host,
            DispatchConfig::default(),
        );
        assert_eq!(
            completion, reference,
            "resuming from the materialized exit frame matches the normal run"
        );
    }

    /// (b) An exit at bytecode index 0 (function entry, no recovered locals)
    /// reproduces a plain call.
    #[test]
    fn function_entry_exit_reproduces_a_plain_call() {
        let block = add_double_code_block();
        let owner = CodeBlockId(CellId(9002));
        let args = vec![
            RuntimeValue::undefined(),
            RuntimeValue::from_i32(20),
            RuntimeValue::from_i32(1),
        ];

        let mut reference_host = CoreOpcodeDispatchHost::new();
        let reference = run_reference_call(owner, &block, args.clone(), &mut reference_host);
        assert_eq!(
            reference,
            ExecutionCompletion::Returned(RuntimeValue::from_i32(42))
        );

        let mut state = TestVmState::new(owner);
        let recovered = recovered_operands(
            args.iter().copied().map(Some).collect(),
            vec![None, None, None],
        );
        let request = materialize_interpreter_exit_frame(
            &mut state.stack,
            &mut state.registers,
            owner,
            &block,
            None,
            bci(0),
            &recovered,
            identity_with_argument_count(3),
        )
        .expect("entry-exit materialization succeeds");

        let mut host = CoreOpcodeDispatchHost::new();
        let completion = resume_interpreter_exit_frame(
            state.execution(),
            request,
            &block,
            &mut host,
            DispatchConfig::default(),
        );
        assert_eq!(completion, reference);
    }

    /// (c, value forms) Every boxed representation the recovery map supports
    /// round-trips bit-exactly through materialization + resume: int32,
    /// double (including -0.0, whose NaN-boxed bits differ from +0.0),
    /// boolean, undefined.
    #[test]
    fn recovered_value_forms_round_trip_bit_exactly() {
        // g() { return t; } with t recovered into local 0.
        let block = function_code_block(
            vec![core_typed(
                0,
                CoreOpcode::Return,
                vec![Operand::Register(local(0))],
            )],
            1,
            1,
        );
        let owner = CodeBlockId(CellId(9003));
        let forms = [
            RuntimeValue::from_i32(7),
            RuntimeValue::from_double(-0.0),
            RuntimeValue::from_double(2.5),
            RuntimeValue::from_bool(true),
            RuntimeValue::undefined(),
        ];
        // -0.0 must be a DIFFERENT boxed value than +0.0 for this to prove
        // sign preservation.
        assert_ne!(
            RuntimeValue::from_double(-0.0).encoded(),
            RuntimeValue::from_double(0.0).encoded()
        );

        for value in forms {
            let mut state = TestVmState::new(owner);
            let recovered =
                recovered_operands(vec![Some(RuntimeValue::undefined())], vec![Some(value)]);
            let request = materialize_interpreter_exit_frame(
                &mut state.stack,
                &mut state.registers,
                owner,
                &block,
                None,
                bci(0),
                &recovered,
                identity_with_argument_count(1),
            )
            .expect("materialization succeeds");
            let mut host = CoreOpcodeDispatchHost::new();
            let completion = resume_interpreter_exit_frame(
                state.execution(),
                request,
                &block,
                &mut host,
                DispatchConfig::default(),
            );
            let ExecutionCompletion::Returned(returned) = completion else {
                panic!("resume returned a non-Returned completion: {completion:?}");
            };
            assert_eq!(
                returned.encoded(),
                value.encoded(),
                "recovered value round-trips bit-exactly"
            );
        }
    }

    /// (c, GC visibility) A recovered STRING CELL in a materialized-but-not-
    /// yet-resumed exit frame survives a full collection cycle: the frame's
    /// register window is gathered as a live root by the same safepoint gather
    /// that covers every interpreter frame. An unrooted sibling string is
    /// swept by the SAME collection, proving the cycle genuinely ran.
    #[test]
    fn recovered_string_cell_survives_collection_between_materialize_and_resume() {
        let block = function_code_block(
            vec![core_typed(
                0,
                CoreOpcode::Return,
                vec![Operand::Register(local(0))],
            )],
            1,
            1,
        );
        let owner = CodeBlockId(CellId(9004));
        let mut host = CoreOpcodeDispatchHost::new();
        let recovered_string = host.allocate_untracked_string_for_test("osr-recovered");
        let dead_string = host.allocate_untracked_string_for_test("osr-unrooted-garbage");

        let mut state = TestVmState::new(owner);
        let recovered = recovered_operands(
            vec![Some(RuntimeValue::undefined())],
            vec![Some(recovered_string)],
        );
        let request = materialize_interpreter_exit_frame(
            &mut state.stack,
            &mut state.registers,
            owner,
            &block,
            None,
            bci(0),
            &recovered,
            identity_with_argument_count(1),
        )
        .expect("materialization succeeds");

        // Collect BETWEEN materialize and resume. The recovered cell is
        // reachable only through the materialized frame's register window.
        host.force_one_gc_collection_for_test(
            &state.registers,
            &state.stack,
            &state.exceptions,
            &mut state.heap,
        );
        assert_eq!(
            host.string_text_for_test(dead_string),
            None,
            "the unrooted sibling string was swept — the collection really ran"
        );
        assert_eq!(
            host.string_text_for_test(recovered_string),
            Some("osr-recovered"),
            "the materialized frame's recovered cell was gathered as a live root"
        );

        let completion = resume_interpreter_exit_frame(
            state.execution(),
            request,
            &block,
            &mut host,
            DispatchConfig::default(),
        );
        let ExecutionCompletion::Returned(returned) = completion else {
            panic!("resume returned a non-Returned completion: {completion:?}");
        };
        assert_eq!(
            returned.encoded(),
            recovered_string.encoded(),
            "the same live cell pointer comes back out of the resumed frame"
        );
        assert_eq!(host.string_text_for_test(returned), Some("osr-recovered"));
    }

    /// (d) Invalid exit indices are rejected loudly BEFORE any frame is
    /// pushed: mid-instruction and out-of-bounds byte offsets on a RAW packed
    /// stream (the landed is_instruction_start machinery), a checkpointed
    /// index, and an out-of-range ordinal on a typed stream. A valid raw
    /// instruction start still materializes and resumes.
    #[test]
    fn invalid_exit_indices_are_rejected_before_any_frame_mutation() {
        // Raw packed wedge bytes (same JSC-derived fixture as the dispatch
        // test): offset 0 `mov local0, constant0` (3 bytes), offset 3
        // `ret local0` (2 bytes). Offsets 1, 2, 4 are mid-instruction.
        let constant0 = VirtualRegister::constant(0);
        let mut constants = UnlinkedConstantPool::default();
        constants.constants.push(UnlinkedConstant {
            register: constant0,
            value: ConstantValue::Encoded(RuntimeValue::from_i32(42)),
            source_representation: SourceCodeRepresentation::IntegerLiteral,
        });
        let raw_block = CodeBlock::from_unlinked(
            UnlinkedCodeBlock::new(
                CodeKind::Function,
                PackedInstructionStream::from_raw_packed_bytes(vec![
                    opcode_id::MOV,
                    0xff,
                    0x10,
                    opcode_id::RET,
                    0xff,
                ]),
            )
            .with_frame(RegisterFrameShape {
                num_parameters_including_this: 1,
                num_vars: 1,
                num_callee_locals: 0,
                num_temporaries: 0,
                special: Default::default(),
            })
            .with_constants(constants),
            LinkContext::default(),
        );
        let owner = CodeBlockId(CellId(9005));
        let mut state = TestVmState::new(owner);
        let recovered = recovered_operands(vec![Some(RuntimeValue::undefined())], vec![None]);

        for invalid in [bci(1), bci(2), bci(4), bci(5), bci(999)] {
            let result = materialize_interpreter_exit_frame(
                &mut state.stack,
                &mut state.registers,
                owner,
                &raw_block,
                None,
                invalid,
                &recovered,
                identity_with_argument_count(1),
            );
            assert_eq!(
                result.err(),
                Some(ExecutionError::InvalidBytecodeIndex(invalid)),
                "non-instruction-start exit index {invalid:?} is rejected"
            );
            assert_eq!(
                state.stack.frame_depth(),
                0,
                "no frame was materialized for a rejected exit index"
            );
        }

        // Checkpointed exit index: the interpreter has no checkpoint resume.
        let checkpointed = BytecodeIndex::new(0, Checkpoint(1));
        assert_eq!(
            materialize_interpreter_exit_frame(
                &mut state.stack,
                &mut state.registers,
                owner,
                &raw_block,
                None,
                checkpointed,
                &recovered,
                identity_with_argument_count(1),
            )
            .err(),
            Some(ExecutionError::InvalidBytecodeIndex(checkpointed))
        );
        assert_eq!(state.stack.frame_depth(), 0);

        // Positive control on the SAME raw stream: offset 3 (the ret start)
        // is a valid exit target; the recovered local flows out through ret.
        let recovered_live = recovered_operands(
            vec![Some(RuntimeValue::undefined())],
            vec![Some(RuntimeValue::from_i32(7))],
        );
        let request = materialize_interpreter_exit_frame(
            &mut state.stack,
            &mut state.registers,
            owner,
            &raw_block,
            None,
            bci(3),
            &recovered_live,
            identity_with_argument_count(1),
        )
        .expect("a raw instruction start is a valid exit target");
        let mut host = CoreOpcodeDispatchHost::new();
        let completion = resume_interpreter_exit_frame(
            state.execution(),
            request,
            &raw_block,
            &mut host,
            DispatchConfig::default(),
        );
        assert_eq!(
            completion,
            ExecutionCompletion::Returned(RuntimeValue::from_i32(7))
        );
    }

    /// (e) Arity edges: exits into frames whose DYNAMIC argument count differs
    /// from numParameters behave exactly like the corresponding plain calls —
    /// over-supplied extras are preserved, under-supply pads with undefined
    /// (the arity-fixup analog the interpreter call path applies).
    #[test]
    fn arity_mismatched_exits_match_the_equivalent_plain_calls() {
        let block = add_double_code_block();

        // Over-supplied: f(3, 4, 99) — dynamic count 4 > numParameters 3. The
        // extra argument is not an Operands entry; it rides in the identity.
        let owner_over = CodeBlockId(CellId(9006));
        let mut reference_host = CoreOpcodeDispatchHost::new();
        let reference_over = run_reference_call(
            owner_over,
            &block,
            vec![
                RuntimeValue::undefined(),
                RuntimeValue::from_i32(3),
                RuntimeValue::from_i32(4),
                RuntimeValue::from_i32(99),
            ],
            &mut reference_host,
        );
        let mut state = TestVmState::new(owner_over);
        let recovered = recovered_operands(
            vec![
                Some(RuntimeValue::undefined()),
                Some(RuntimeValue::from_i32(3)),
                Some(RuntimeValue::from_i32(4)),
            ],
            vec![None, None, None],
        );
        let identity = OsrExitTargetFrameIdentity {
            extra_arguments: vec![RuntimeValue::from_i32(99)],
            ..identity_with_argument_count(4)
        };
        let request = materialize_interpreter_exit_frame(
            &mut state.stack,
            &mut state.registers,
            owner_over,
            &block,
            None,
            bci(0),
            &recovered,
            identity,
        )
        .expect("over-supplied materialization succeeds");
        let top = state.stack.top_frame().expect("materialized frame");
        assert_eq!(
            top.argument_count_including_this, 4,
            "the dynamic (over-supplied) argument count is preserved"
        );
        assert_eq!(
            state
                .registers
                .read(top.register_window, argument_including_this(3), None),
            Ok(RuntimeValue::from_i32(99)),
            "the extra dynamic argument value persists in the frame"
        );
        let mut host = CoreOpcodeDispatchHost::new();
        let completion = resume_interpreter_exit_frame(
            state.execution(),
            request,
            &block,
            &mut host,
            DispatchConfig::default(),
        );
        assert_eq!(completion, reference_over);

        // Under-supplied: f(3) — dynamic count 2 < numParameters 3. The dead
        // formal b is recovered as undefined; the window pads to formals just
        // as the interpreter's own call entry does, so the completion matches
        // the plain under-supplied call (NaN result bits included).
        let owner_under = CodeBlockId(CellId(9007));
        let mut reference_host = CoreOpcodeDispatchHost::new();
        let reference_under = run_reference_call(
            owner_under,
            &block,
            vec![RuntimeValue::undefined(), RuntimeValue::from_i32(3)],
            &mut reference_host,
        );
        let mut state = TestVmState::new(owner_under);
        let recovered = recovered_operands(
            vec![
                Some(RuntimeValue::undefined()),
                Some(RuntimeValue::from_i32(3)),
                Some(RuntimeValue::undefined()),
            ],
            vec![None, None, None],
        );
        let request = materialize_interpreter_exit_frame(
            &mut state.stack,
            &mut state.registers,
            owner_under,
            &block,
            None,
            bci(0),
            &recovered,
            identity_with_argument_count(2),
        )
        .expect("under-supplied materialization succeeds");
        let top = state.stack.top_frame().expect("materialized frame");
        assert_eq!(
            top.argument_count_including_this, 3,
            "the window pads to numParameters, mirroring the interpreter call path \
             (see the divergence comment at the padding site)"
        );
        let mut host = CoreOpcodeDispatchHost::new();
        let completion = resume_interpreter_exit_frame(
            state.execution(),
            request,
            &block,
            &mut host,
            DispatchConfig::default(),
        );
        assert_eq!(completion, reference_under);
    }

    /// Malformed recovery maps are rejected before (or unwound after) frame
    /// mutation: a missing this-argument entry, an extras/dynamic-count
    /// mismatch, and a recovery map wider than the frame's local capacity.
    #[test]
    fn malformed_recovery_maps_are_rejected_loudly() {
        let block = add_double_code_block();
        let owner = CodeBlockId(CellId(9008));
        let mut state = TestVmState::new(owner);

        // No argument entries at all (no `this`).
        let no_this = recovered_operands(Vec::new(), vec![None]);
        assert_eq!(
            materialize_interpreter_exit_frame(
                &mut state.stack,
                &mut state.registers,
                owner,
                &block,
                None,
                bci(0),
                &no_this,
                identity_with_argument_count(1),
            )
            .err(),
            Some(ExecutionError::InvalidArgumentCount)
        );

        // Dynamic count 5 with recovered width 3 requires exactly 2 extras.
        let recovered = recovered_operands(
            vec![Some(RuntimeValue::undefined()), None, None],
            vec![None, None, None],
        );
        assert_eq!(
            materialize_interpreter_exit_frame(
                &mut state.stack,
                &mut state.registers,
                owner,
                &block,
                None,
                bci(0),
                &recovered,
                identity_with_argument_count(5),
            )
            .err(),
            Some(ExecutionError::InvalidArgumentCount)
        );

        // Recovery map wider than the frame's local capacity (3): the write
        // fails, the half-built frame is unwound, and the error is loud.
        let too_wide = recovered_operands(
            vec![Some(RuntimeValue::undefined()), None, None],
            vec![None, None, None, Some(RuntimeValue::from_i32(1))],
        );
        assert_eq!(
            materialize_interpreter_exit_frame(
                &mut state.stack,
                &mut state.registers,
                owner,
                &block,
                None,
                bci(0),
                &too_wide,
                identity_with_argument_count(3),
            )
            .err(),
            Some(ExecutionError::RegisterOutOfBounds)
        );
        assert_eq!(
            state.stack.frame_depth(),
            0,
            "every rejected materialization left the stack unchanged"
        );
    }
}
