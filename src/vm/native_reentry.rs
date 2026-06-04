//! VM native side-exit reentry bridge helpers.
//!
//! C++ JSC maps this responsibility across Baseline JIT operations/thunks
//! (`JITOpcodes.cpp` falsey thunk calls), resume-label metadata
//! (`JIT::fastPathResumePoint` plus `JITCodeMapBuilder`), and
//! `FrameTracers`/`AssemblyHelpers::prepareCallOperation` updating
//! `VM::topCallFrame` for JIT operation rooting. Rust keeps the helper here
//! because `vm::mod` is already oversized; this module only classifies native
//! return payloads and builds opaque executable-memory call requests.

use core::ffi::c_void;
use std::{convert::Infallible, ptr::NonNull};

use crate::bytecode::{BytecodeIndex, CodeBlock, CoreOpcode};
use crate::interpreter::{ExecutionCompletion, ExecutionError};
use crate::jit::emitter::{
    P10X86_64BaselinePropertyNativeExitReturnPayload,
    P9X86_64BaselineJsCallNativeExitReturnPayload,
    P10_X86_64_BASELINE_PROPERTY_NATIVE_EXIT_RETURN_PAYLOAD_LOW_TAG,
    P9_X86_64_BASELINE_JS_CALL_NATIVE_EXIT_RETURN_PAYLOAD_LOW_TAG,
};
use crate::jit::{
    BaselineNativeEntryCallableKind, MachineCodeRange, P14X86_64BaselineLoopBackedgeReturnPayload,
    P6X86_64BaselineSelectedSideExitReason, P6X86_64BaselineSideExitReturnPayload,
    P6X86_64BaselineTerminalPolicy, P14_X86_64_BASELINE_LOOP_BACKEDGE_RETURN_PAYLOAD_LOW_TAG,
    P6_X86_64_BASELINE_SIDE_EXIT_RETURN_PAYLOAD_LOW_TAG,
};
use crate::platform::executable_memory_compartment::ExecutableMemoryP6CallRequest;
use crate::runtime::RuntimeValue;
use crate::value::EncodedJsValue;

use super::entry::VmNativeCallFramePublicationRecord;
use super::side_exit::{
    p6_jump_if_false_truthiness_side_exit_resume_shape, P6CallableSideExitNativeReentryInvocation,
    P6X86_64CallableSideExitReturnSite,
};
use super::BaselineNativeEntryVmExecution;

#[cfg(test)]
pub(super) fn p6_x86_64_callable_side_exit_payload_has_reserved_tag(raw_bits: u64) -> bool {
    (raw_bits & 0xff) == u64::from(P6_X86_64_BASELINE_SIDE_EXIT_RETURN_PAYLOAD_LOW_TAG)
}

#[derive(Clone, Copy, Debug, Eq, PartialEq)]
pub(super) enum P6P9P10P14X86_64CallableNativeReturnPayload {
    P6(P6X86_64BaselineSideExitReturnPayload),
    P9(P9X86_64BaselineJsCallNativeExitReturnPayload),
    P10(P10X86_64BaselinePropertyNativeExitReturnPayload),
    P14(P14X86_64BaselineLoopBackedgeReturnPayload),
}

#[derive(Clone, Copy, Debug, Eq, PartialEq)]
pub(super) enum P6Arm64EmittedSemanticNativeRawReturn {
    RuntimeValue(RuntimeValue),
    RetainedP6SideExit(P6X86_64BaselineSideExitReturnPayload),
}

pub(super) fn p6_p9_p10_p14_x86_64_callable_native_return_payload(
    raw_bits: u64,
) -> Result<Option<P6P9P10P14X86_64CallableNativeReturnPayload>, ExecutionError> {
    match (raw_bits & 0xff) as u8 {
        P6_X86_64_BASELINE_SIDE_EXIT_RETURN_PAYLOAD_LOW_TAG => {
            let payload = P6X86_64BaselineSideExitReturnPayload::decode(raw_bits)
                .ok_or(ExecutionError::BaselineGeneratedExecutionRejected)?;
            Ok(Some(P6P9P10P14X86_64CallableNativeReturnPayload::P6(
                payload,
            )))
        }
        P9_X86_64_BASELINE_JS_CALL_NATIVE_EXIT_RETURN_PAYLOAD_LOW_TAG => {
            let payload = P9X86_64BaselineJsCallNativeExitReturnPayload::decode(raw_bits)
                .ok_or(ExecutionError::BaselineGeneratedExecutionRejected)?;
            Ok(Some(P6P9P10P14X86_64CallableNativeReturnPayload::P9(
                payload,
            )))
        }
        P10_X86_64_BASELINE_PROPERTY_NATIVE_EXIT_RETURN_PAYLOAD_LOW_TAG => {
            let payload = P10X86_64BaselinePropertyNativeExitReturnPayload::decode(raw_bits)
                .ok_or(ExecutionError::BaselineGeneratedExecutionRejected)?;
            Ok(Some(P6P9P10P14X86_64CallableNativeReturnPayload::P10(
                payload,
            )))
        }
        P14_X86_64_BASELINE_LOOP_BACKEDGE_RETURN_PAYLOAD_LOW_TAG => {
            let payload = P14X86_64BaselineLoopBackedgeReturnPayload::decode(raw_bits)
                .ok_or(ExecutionError::BaselineGeneratedExecutionRejected)?;
            Ok(Some(P6P9P10P14X86_64CallableNativeReturnPayload::P14(
                payload,
            )))
        }
        _ => Ok(None),
    }
}

pub(super) fn p6_arm64_emitted_semantic_native_raw_return(
    raw_bits: u64,
) -> Result<P6Arm64EmittedSemanticNativeRawReturn, ExecutionError> {
    match p6_p9_p10_p14_x86_64_callable_native_return_payload(raw_bits)? {
        Some(P6P9P10P14X86_64CallableNativeReturnPayload::P6(payload)) => Ok(
            P6Arm64EmittedSemanticNativeRawReturn::RetainedP6SideExit(payload),
        ),
        Some(
            P6P9P10P14X86_64CallableNativeReturnPayload::P9(_)
            | P6P9P10P14X86_64CallableNativeReturnPayload::P10(_)
            | P6P9P10P14X86_64CallableNativeReturnPayload::P14(_),
        ) => Err(ExecutionError::BaselineGeneratedExecutionRejected),
        None => Ok(P6Arm64EmittedSemanticNativeRawReturn::RuntimeValue(
            RuntimeValue::from_encoded(EncodedJsValue(raw_bits)),
        )),
    }
}

pub(super) fn p6_arm64_reject_side_exit_reentry_execution(
    execution: BaselineNativeEntryVmExecution,
) -> BaselineNativeEntryVmExecution {
    match execution {
        BaselineNativeEntryVmExecution::P6SideExitReentry(_) => {
            BaselineNativeEntryVmExecution::Native(ExecutionCompletion::Failed(
                ExecutionError::BaselineGeneratedExecutionRejected,
            ))
        }
        execution => execution,
    }
}

#[allow(dead_code)]
#[derive(Clone, Copy, Debug, Eq, PartialEq)]
pub(super) struct P6Arm64BranchAwareCallableTopCallFramePublicationProof {
    // C++ JSC stores a raw CallFrame* in VM::topCallFrame; this Rust evidence
    // is tied to the symbolic publication record from `entry.rs`, so it proves
    // top-frame metadata exists but not that conservative machine-stack roots
    // can see generated ARM64 state.
    pub(super) publication: VmNativeCallFramePublicationRecord,
}

impl P6Arm64BranchAwareCallableTopCallFramePublicationProof {
    #[allow(dead_code)]
    pub(super) const fn from_publication_record(
        publication: VmNativeCallFramePublicationRecord,
    ) -> Self {
        Self { publication }
    }
}

#[allow(dead_code)]
#[derive(Clone, Copy, Debug, Eq, PartialEq)]
pub(super) enum P6Arm64BranchAwareCallableFallbackRootingProof {
    MissingTopCallFramePublication,
    TopCallFramePublicationWithoutConservativeRoots(
        P6Arm64BranchAwareCallableTopCallFramePublicationProof,
    ),
}

#[allow(dead_code)]
#[derive(Clone, Copy, Debug, Default, Eq, PartialEq)]
pub(super) struct P6Arm64BranchAwareCallableExitCounts {
    pub(super) runtime_helper_native_exits: usize,
    pub(super) js_call_native_exits: usize,
    pub(super) property_native_exits: usize,
    pub(super) loop_backedge_native_exits: usize,
}

#[allow(dead_code)]
#[derive(Clone, Copy, Debug, Eq, PartialEq)]
pub(super) struct P6Arm64BranchAwareCallableMetadataProof {
    pub(super) readiness_matches_descriptor: bool,
    pub(super) readiness_matches_bytecode_snapshot: bool,
    pub(super) materialization_matches_install: bool,
    pub(super) retained_table_matches_materialization: bool,
}

#[allow(dead_code)]
#[derive(Clone, Copy, Debug)]
pub(super) struct P6Arm64BranchAwareCallableSideExitProof<'a> {
    pub(super) site: &'a P6X86_64CallableSideExitReturnSite,
    pub(super) code_block: &'a CodeBlock,
    pub(super) opcode: Option<CoreOpcode>,
    pub(super) target_bytecode_index: BytecodeIndex,
    pub(super) fallthrough_bytecode_index: BytecodeIndex,
}

#[allow(dead_code)]
#[derive(Clone, Copy, Debug)]
pub(super) struct P6Arm64BranchAwareCallableAdmissionProofRequest<'a> {
    pub(super) callable_kind: BaselineNativeEntryCallableKind,
    pub(super) terminal_policy: Option<P6X86_64BaselineTerminalPolicy>,
    pub(super) descriptor_machine_range: Option<MachineCodeRange>,
    pub(super) side_exits: &'a [P6Arm64BranchAwareCallableSideExitProof<'a>],
    pub(super) exit_counts: P6Arm64BranchAwareCallableExitCounts,
    pub(super) metadata: P6Arm64BranchAwareCallableMetadataProof,
    pub(super) fallback_rooting_proof: P6Arm64BranchAwareCallableFallbackRootingProof,
}

#[allow(dead_code)]
#[derive(Clone, Copy, Debug, Eq, PartialEq)]
pub(super) enum P6Arm64BranchAwareCallableAdmissionRejection {
    MissingBranchAwareSemanticEmission,
    CallableKindNotArm64 {
        actual: BaselineNativeEntryCallableKind,
    },
    MissingTerminalPolicy,
    NonBranchAwareTerminalPolicy {
        actual: P6X86_64BaselineTerminalPolicy,
    },
    MissingDescriptorRange,
    DescriptorRangeInvalid {
        range: MachineCodeRange,
    },
    MissingSideExitPayloadStub,
    UnexpectedSideExit {
        side_exit_index: u32,
        reason: P6X86_64BaselineSelectedSideExitReason,
        opcode: Option<CoreOpcode>,
    },
    MissingNativeReentryTarget {
        side_exit_index: u32,
        resume_bytecode_index: BytecodeIndex,
    },
    NativeReentryTargetOutsideDescriptorRange {
        side_exit_index: u32,
        resume_bytecode_index: BytecodeIndex,
        resume_entry_offset: u32,
        range: MachineCodeRange,
    },
    RuntimeHelperNativeExitPresent {
        count: usize,
    },
    JsCallNativeExitPresent {
        count: usize,
    },
    PropertyNativeExitPresent {
        count: usize,
    },
    LoopBackedgeNativeExitPresent {
        count: usize,
    },
    ReadinessDescriptorMismatch,
    ReadinessBytecodeSnapshotMismatch,
    MaterializationInstallMismatch,
    RetainedTableMaterializationMismatch,
    MissingTopCallFramePublicationProof,
    MissingMachineStackAndConservativeRootingProof {
        top_call_frame_publication: P6Arm64BranchAwareCallableTopCallFramePublicationProof,
    },
}

pub(super) const fn p6_arm64_public_branch_aware_callable_admission_rejection_for_unemitted_seed_candidate(
) -> P6Arm64BranchAwareCallableAdmissionRejection {
    P6Arm64BranchAwareCallableAdmissionRejection::MissingBranchAwareSemanticEmission
}

#[allow(dead_code)]
pub(super) fn p6_arm64_public_branch_aware_callable_admission_proof(
    request: &P6Arm64BranchAwareCallableAdmissionProofRequest<'_>,
) -> Result<Infallible, P6Arm64BranchAwareCallableAdmissionRejection> {
    if request.callable_kind != BaselineNativeEntryCallableKind::P6Arm64EmittedSemanticCAbiEntry {
        return Err(
            P6Arm64BranchAwareCallableAdmissionRejection::CallableKindNotArm64 {
                actual: request.callable_kind,
            },
        );
    }

    match request.terminal_policy {
        Some(
            P6X86_64BaselineTerminalPolicy::CallableCAbiPrologueBytecodeBranchesSharedNormalEpilogueThenInlinePayloadStubs,
        ) => {}
        Some(actual) => {
            return Err(
                P6Arm64BranchAwareCallableAdmissionRejection::NonBranchAwareTerminalPolicy {
                    actual,
                },
            );
        }
        None => return Err(P6Arm64BranchAwareCallableAdmissionRejection::MissingTerminalPolicy),
    }

    let Some(range) = request.descriptor_machine_range else {
        return Err(P6Arm64BranchAwareCallableAdmissionRejection::MissingDescriptorRange);
    };
    if range.size_bytes == 0 || range.end_offset().is_none() {
        return Err(P6Arm64BranchAwareCallableAdmissionRejection::DescriptorRangeInvalid { range });
    }

    if request.side_exits.is_empty() {
        return Err(P6Arm64BranchAwareCallableAdmissionRejection::MissingSideExitPayloadStub);
    }
    for proof in request.side_exits {
        validate_p6_arm64_branch_aware_callable_side_exit_proof(*proof, range)?;
    }

    let exit_counts = request.exit_counts;
    if exit_counts.runtime_helper_native_exits != 0 {
        return Err(
            P6Arm64BranchAwareCallableAdmissionRejection::RuntimeHelperNativeExitPresent {
                count: exit_counts.runtime_helper_native_exits,
            },
        );
    }
    if exit_counts.js_call_native_exits != 0 {
        return Err(
            P6Arm64BranchAwareCallableAdmissionRejection::JsCallNativeExitPresent {
                count: exit_counts.js_call_native_exits,
            },
        );
    }
    if exit_counts.property_native_exits != 0 {
        return Err(
            P6Arm64BranchAwareCallableAdmissionRejection::PropertyNativeExitPresent {
                count: exit_counts.property_native_exits,
            },
        );
    }
    if exit_counts.loop_backedge_native_exits != 0 {
        return Err(
            P6Arm64BranchAwareCallableAdmissionRejection::LoopBackedgeNativeExitPresent {
                count: exit_counts.loop_backedge_native_exits,
            },
        );
    }

    let metadata = request.metadata;
    if !metadata.readiness_matches_descriptor {
        return Err(P6Arm64BranchAwareCallableAdmissionRejection::ReadinessDescriptorMismatch);
    }
    if !metadata.readiness_matches_bytecode_snapshot {
        return Err(
            P6Arm64BranchAwareCallableAdmissionRejection::ReadinessBytecodeSnapshotMismatch,
        );
    }
    if !metadata.materialization_matches_install {
        return Err(P6Arm64BranchAwareCallableAdmissionRejection::MaterializationInstallMismatch);
    }
    if !metadata.retained_table_matches_materialization {
        return Err(
            P6Arm64BranchAwareCallableAdmissionRejection::RetainedTableMaterializationMismatch,
        );
    }

    // C++ JSC publishes an actual CallFrame* into VM::topCallFrame via
    // TopCallFrameSetter/NativeCallFrameTracer/prepareCallOperation, and
    // StackVisitor plus conservative roots consume that machine-stack fact.
    // Rust intentionally diverges here: VmNativeCallFramePublicationRecord is
    // symbolic VM-entry metadata, not a proof that generated ARM64 state is
    // visible to conservative stack/root scanning. The `Infallible` success
    // type keeps public ARM64 admission rejected after distinguishing these
    // two missing pieces.
    match request.fallback_rooting_proof {
        P6Arm64BranchAwareCallableFallbackRootingProof::MissingTopCallFramePublication => {
            Err(P6Arm64BranchAwareCallableAdmissionRejection::MissingTopCallFramePublicationProof)
        }
        P6Arm64BranchAwareCallableFallbackRootingProof::TopCallFramePublicationWithoutConservativeRoots(
            top_call_frame_publication,
        ) => Err(
            P6Arm64BranchAwareCallableAdmissionRejection::MissingMachineStackAndConservativeRootingProof {
                top_call_frame_publication,
            },
        ),
    }
}

#[allow(dead_code)]
fn validate_p6_arm64_branch_aware_callable_side_exit_proof(
    proof: P6Arm64BranchAwareCallableSideExitProof<'_>,
    range: MachineCodeRange,
) -> Result<(), P6Arm64BranchAwareCallableAdmissionRejection> {
    if proof.site.reason != P6X86_64BaselineSelectedSideExitReason::UnsupportedTruthinessOperand
        || proof.opcode != Some(CoreOpcode::JumpIfFalse)
    {
        return Err(
            P6Arm64BranchAwareCallableAdmissionRejection::UnexpectedSideExit {
                side_exit_index: proof.site.side_exit_index,
                reason: proof.site.reason,
                opcode: proof.opcode,
            },
        );
    }

    let Some(shape) =
        p6_jump_if_false_truthiness_side_exit_resume_shape(proof.code_block, proof.site)
    else {
        return Err(
            P6Arm64BranchAwareCallableAdmissionRejection::UnexpectedSideExit {
                side_exit_index: proof.site.side_exit_index,
                reason: proof.site.reason,
                opcode: proof.opcode,
            },
        );
    };
    if proof.target_bytecode_index != shape.taken_target.resume_bytecode_index {
        return Err(
            P6Arm64BranchAwareCallableAdmissionRejection::MissingNativeReentryTarget {
                side_exit_index: proof.site.side_exit_index,
                resume_bytecode_index: proof.target_bytecode_index,
            },
        );
    }
    if proof.fallthrough_bytecode_index != shape.fallthrough_target.resume_bytecode_index {
        return Err(
            P6Arm64BranchAwareCallableAdmissionRejection::MissingNativeReentryTarget {
                side_exit_index: proof.site.side_exit_index,
                resume_bytecode_index: proof.fallthrough_bytecode_index,
            },
        );
    }

    for target in [shape.taken_target, shape.fallthrough_target] {
        if !p6_arm64_image_entry_offset_points_inside_descriptor_range(
            target.resume_entry_offset,
            range,
        ) {
            return Err(
                P6Arm64BranchAwareCallableAdmissionRejection::NativeReentryTargetOutsideDescriptorRange {
                    side_exit_index: proof.site.side_exit_index,
                    resume_bytecode_index: target.resume_bytecode_index,
                    resume_entry_offset: target.resume_entry_offset,
                    range,
                },
            );
        }
    }
    Ok(())
}

#[allow(dead_code)]
fn p6_arm64_image_entry_offset_points_inside_descriptor_range(
    image_entry_offset: u32,
    range: MachineCodeRange,
) -> bool {
    let Some(end_offset) = range.end_offset() else {
        return false;
    };
    let Some(allocation_relative_entry_offset) = range.start_offset.checked_add(image_entry_offset)
    else {
        return false;
    };
    image_entry_offset < range.size_bytes
        && allocation_relative_entry_offset >= range.start_offset
        && allocation_relative_entry_offset < end_offset
}

#[derive(Clone, Copy, Debug, Eq, PartialEq)]
pub(super) struct P6NativeSideExitReentryCallBridge {
    reentry: P6CallableSideExitNativeReentryInvocation,
}

impl P6NativeSideExitReentryCallBridge {
    pub(super) const fn new(reentry: P6CallableSideExitNativeReentryInvocation) -> Self {
        Self { reentry }
    }

    pub(super) const fn entry_offset(self) -> u32 {
        self.reentry.entry_offset
    }

    pub(super) const fn call_request(
        self,
        vm: NonNull<c_void>,
        frame_base: NonNull<c_void>,
        callee_value_bits: u64,
        ic_store_base: NonNull<c_void>,
    ) -> ExecutableMemoryP6CallRequest {
        // C++ JSC reenters by branching to a linked native label while
        // `prepareCallOperation`/FrameTracers keep `VM::topCallFrame` coherent
        // for stack walking and rooting. Rust diverges here intentionally:
        // the VM has already synchronized/cleaned the fallback roots, and this
        // bridge carries only opaque pointers plus an allocation-relative label.
        // It owns no roots and grants no public backend authority.
        ExecutableMemoryP6CallRequest::new(
            self.entry_offset(),
            vm,
            frame_base,
            callee_value_bits,
            ic_store_base,
        )
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use std::convert::Infallible;

    use crate::bytecode::{
        CodeBlockEntrypoints, CodeBlockLifecycleState, CodeKind, InterpreterEntrySlot, LinkContext,
        Operand, OperandWidth, PackedInstructionStream, RegisterFrameShape, TypedInstruction,
        UnlinkedCodeBlock, VirtualRegister,
    };
    use crate::jit::{ExecutableAllocationId, P6BaselineNativeReentryTargetRecord};
    use crate::runtime::{
        ArityCheckMode, CallFrameId, CellId, CodeBlockId, CodeSpecializationKind, EntryFrameId,
        RuntimeValue,
    };

    use super::super::entry::{
        FrameAddress, VmEntryCallFrameMetadata, VmEntryLaunchArgumentValue,
        VmNativeCallFramePublicationReason, VmNativeCallFramePublicationRecord,
    };

    fn bci(offset: u32) -> BytecodeIndex {
        BytecodeIndex::from_offset(offset)
    }

    fn range() -> MachineCodeRange {
        MachineCodeRange {
            allocation: ExecutableAllocationId(1),
            start_offset: 128,
            size_bytes: 64,
        }
    }

    fn target(
        resume_bytecode_index: BytecodeIndex,
        resume_entry_offset: u32,
    ) -> P6BaselineNativeReentryTargetRecord {
        P6BaselineNativeReentryTargetRecord {
            resume_bytecode_index,
            resume_entry_offset,
        }
    }

    fn local(index: u32) -> VirtualRegister {
        VirtualRegister::local(index)
    }

    fn typed_core_instruction_with_operands(
        offset: u32,
        opcode: CoreOpcode,
        operands: Vec<Operand>,
    ) -> TypedInstruction {
        TypedInstruction {
            opcode: opcode.opcode(),
            width: OperandWidth::Narrow,
            operands,
            schema: None,
            bytecode_index: Some(bci(offset)),
        }
    }

    fn code_block_from_instructions(instructions: Vec<TypedInstruction>) -> CodeBlock {
        CodeBlock::from_unlinked(
            UnlinkedCodeBlock::new(
                CodeKind::Program,
                PackedInstructionStream::from_typed_placeholder(instructions),
            )
            .with_frame(RegisterFrameShape {
                num_parameters_including_this: 1,
                num_vars: 1,
                num_callee_locals: 0,
                num_temporaries: 0,
                special: Default::default(),
            }),
            LinkContext::default(),
        )
        .with_entrypoints(CodeBlockEntrypoints {
            interpreter: Some(InterpreterEntrySlot(0)),
            ..CodeBlockEntrypoints::default()
        })
        .with_lifecycle(CodeBlockLifecycleState::LinkedInterpreter)
    }

    fn jump_if_false_code_block(taken_target: u32) -> CodeBlock {
        code_block_from_instructions(vec![
            typed_core_instruction_with_operands(
                0,
                CoreOpcode::LoadBool,
                vec![Operand::Register(local(0)), Operand::UnsignedImmediate(0)],
            ),
            typed_core_instruction_with_operands(
                1,
                CoreOpcode::JumpIfFalse,
                vec![
                    Operand::Register(local(0)),
                    Operand::BytecodeIndex(bci(taken_target)),
                ],
            ),
            typed_core_instruction_with_operands(
                2,
                CoreOpcode::Return,
                vec![Operand::Register(local(0))],
            ),
            typed_core_instruction_with_operands(
                3,
                CoreOpcode::LoadBool,
                vec![Operand::Register(local(0)), Operand::UnsignedImmediate(1)],
            ),
            typed_core_instruction_with_operands(
                4,
                CoreOpcode::Return,
                vec![Operand::Register(local(0))],
            ),
        ])
    }

    fn terminal_jump_if_false_code_block() -> CodeBlock {
        code_block_from_instructions(vec![
            typed_core_instruction_with_operands(
                0,
                CoreOpcode::LoadBool,
                vec![Operand::Register(local(0)), Operand::UnsignedImmediate(0)],
            ),
            typed_core_instruction_with_operands(
                1,
                CoreOpcode::JumpIfFalse,
                vec![Operand::Register(local(0)), Operand::BytecodeIndex(bci(0))],
            ),
        ])
    }

    fn jump_if_false_site() -> P6X86_64CallableSideExitReturnSite {
        P6X86_64CallableSideExitReturnSite {
            bytecode_index: bci(1),
            reason: P6X86_64BaselineSelectedSideExitReason::UnsupportedTruthinessOperand,
            side_exit_index: 0,
            resume_bytecode_index: None,
            resume_entry_offset: None,
            native_reentry_targets: vec![target(bci(4), 12), target(bci(2), 28)],
            encoded_payload: P6X86_64BaselineSideExitReturnPayload::encode(0),
        }
    }

    fn branch_aware_side_exit_proof<'a>(
        code_block: &'a CodeBlock,
        site: &'a P6X86_64CallableSideExitReturnSite,
    ) -> P6Arm64BranchAwareCallableSideExitProof<'a> {
        branch_aware_side_exit_proof_with_labels(
            code_block,
            site,
            Some(CoreOpcode::JumpIfFalse),
            bci(4),
            bci(2),
        )
    }

    fn branch_aware_side_exit_proof_with_labels<'a>(
        code_block: &'a CodeBlock,
        site: &'a P6X86_64CallableSideExitReturnSite,
        opcode: Option<CoreOpcode>,
        target_bytecode_index: BytecodeIndex,
        fallthrough_bytecode_index: BytecodeIndex,
    ) -> P6Arm64BranchAwareCallableSideExitProof<'a> {
        P6Arm64BranchAwareCallableSideExitProof {
            site,
            code_block,
            opcode,
            target_bytecode_index,
            fallthrough_bytecode_index,
        }
    }

    fn valid_metadata() -> P6Arm64BranchAwareCallableMetadataProof {
        P6Arm64BranchAwareCallableMetadataProof {
            readiness_matches_descriptor: true,
            readiness_matches_bytecode_snapshot: true,
            materialization_matches_install: true,
            retained_table_matches_materialization: true,
        }
    }

    fn valid_request<'a>(
        side_exits: &'a [P6Arm64BranchAwareCallableSideExitProof<'a>],
    ) -> P6Arm64BranchAwareCallableAdmissionProofRequest<'a> {
        P6Arm64BranchAwareCallableAdmissionProofRequest {
            callable_kind: BaselineNativeEntryCallableKind::P6Arm64EmittedSemanticCAbiEntry,
            terminal_policy: Some(
                P6X86_64BaselineTerminalPolicy::CallableCAbiPrologueBytecodeBranchesSharedNormalEpilogueThenInlinePayloadStubs,
            ),
            descriptor_machine_range: Some(range()),
            side_exits,
            exit_counts: P6Arm64BranchAwareCallableExitCounts::default(),
            metadata: valid_metadata(),
            fallback_rooting_proof:
                P6Arm64BranchAwareCallableFallbackRootingProof::MissingTopCallFramePublication,
        }
    }

    fn top_call_frame_publication_record() -> VmNativeCallFramePublicationRecord {
        let code_block = CodeBlockId(CellId(41));
        VmNativeCallFramePublicationRecord {
            ordinal: 1,
            entry_depth: 1,
            reason: VmNativeCallFramePublicationReason::BaselineNativeEntry,
            owner: code_block,
            code_block,
            current_entry_frame: FrameAddress(0x1000),
            previous_top_frame: Some(FrameAddress(0x1000)),
            published_top_frame: FrameAddress(0x2000),
            active_entry_frame: EntryFrameId(1),
            previous_entry_frame: None,
            saved_top_call_frame: None,
            active_top_call_frame: CallFrameId(2),
            call_frame: VmEntryCallFrameMetadata {
                frame: CallFrameId(2),
                entry_frame: Some(EntryFrameId(1)),
                caller_frame: None,
                code_block: Some(code_block),
                callee: None,
                callee_value: None,
                context: None,
                global_object: None,
                entry_value: VmEntryLaunchArgumentValue::This(RuntimeValue::undefined()),
                argument_count_including_this: 1,
                provided_argument_count: 0,
                padded_argument_count: 1,
                specialization: CodeSpecializationKind::Call,
                arity_mode: ArityCheckMode::AlreadyChecked,
            },
        }
    }

    fn admission_for_site(
        code_block: &CodeBlock,
        site: &P6X86_64CallableSideExitReturnSite,
    ) -> Result<Infallible, P6Arm64BranchAwareCallableAdmissionRejection> {
        admission_for_site_with_labels(
            code_block,
            site,
            Some(CoreOpcode::JumpIfFalse),
            bci(4),
            bci(2),
        )
    }

    fn admission_for_site_with_labels(
        code_block: &CodeBlock,
        site: &P6X86_64CallableSideExitReturnSite,
        opcode: Option<CoreOpcode>,
        target_bytecode_index: BytecodeIndex,
        fallthrough_bytecode_index: BytecodeIndex,
    ) -> Result<Infallible, P6Arm64BranchAwareCallableAdmissionRejection> {
        let side_exits = [branch_aware_side_exit_proof_with_labels(
            code_block,
            site,
            opcode,
            target_bytecode_index,
            fallthrough_bytecode_index,
        )];
        let request = valid_request(&side_exits);
        p6_arm64_public_branch_aware_callable_admission_proof(&request)
    }

    #[test]
    fn public_arm64_branch_aware_admission_rejects_missing_top_call_frame_publication_proof() {
        let code_block = jump_if_false_code_block(4);
        let site = jump_if_false_site();
        let side_exits = [branch_aware_side_exit_proof(&code_block, &site)];
        let request = valid_request(&side_exits);

        assert_eq!(
            p6_arm64_public_branch_aware_callable_admission_proof(&request),
            Err(P6Arm64BranchAwareCallableAdmissionRejection::MissingTopCallFramePublicationProof)
        );
    }

    #[test]
    fn public_arm64_branch_aware_admission_rejects_missing_machine_roots_after_publication() {
        let code_block = jump_if_false_code_block(4);
        let site = jump_if_false_site();
        let side_exits = [branch_aware_side_exit_proof(&code_block, &site)];
        let mut request = valid_request(&side_exits);
        let top_call_frame_publication =
            P6Arm64BranchAwareCallableTopCallFramePublicationProof::from_publication_record(
                top_call_frame_publication_record(),
            );
        request.fallback_rooting_proof =
            P6Arm64BranchAwareCallableFallbackRootingProof::TopCallFramePublicationWithoutConservativeRoots(
                top_call_frame_publication,
            );

        assert_eq!(
            p6_arm64_public_branch_aware_callable_admission_proof(&request),
            Err(
                P6Arm64BranchAwareCallableAdmissionRejection::MissingMachineStackAndConservativeRootingProof {
                    top_call_frame_publication,
                }
            )
        );
    }

    #[test]
    fn public_arm64_branch_aware_admission_rejects_x86_callable_kind() {
        let code_block = jump_if_false_code_block(4);
        let site = jump_if_false_site();
        let side_exits = [branch_aware_side_exit_proof(&code_block, &site)];
        let mut request = valid_request(&side_exits);
        request.callable_kind = BaselineNativeEntryCallableKind::P6X86_64EmittedSemanticCAbiEntry;

        assert_eq!(
            p6_arm64_public_branch_aware_callable_admission_proof(&request),
            Err(
                P6Arm64BranchAwareCallableAdmissionRejection::CallableKindNotArm64 {
                    actual: BaselineNativeEntryCallableKind::P6X86_64EmittedSemanticCAbiEntry,
                }
            )
        );
    }

    #[test]
    fn public_arm64_branch_aware_admission_rejects_non_branch_aware_terminal_policy() {
        let code_block = jump_if_false_code_block(4);
        let site = jump_if_false_site();
        let side_exits = [branch_aware_side_exit_proof(&code_block, &site)];
        let mut request = valid_request(&side_exits);
        request.terminal_policy = Some(
            P6X86_64BaselineTerminalPolicy::CallableCAbiPrologueSingleFinalEpilogueThenInlinePayloadSideExitStubs,
        );

        assert_eq!(
            p6_arm64_public_branch_aware_callable_admission_proof(&request),
            Err(
                P6Arm64BranchAwareCallableAdmissionRejection::NonBranchAwareTerminalPolicy {
                    actual: P6X86_64BaselineTerminalPolicy::CallableCAbiPrologueSingleFinalEpilogueThenInlinePayloadSideExitStubs,
                }
            )
        );
    }

    #[test]
    fn public_arm64_branch_aware_admission_requires_target_and_fallthrough_reentry_ranges() {
        let code_block = jump_if_false_code_block(4);
        let mut site = jump_if_false_site();
        site.native_reentry_targets = vec![target(bci(4), 12), target(bci(2), 64)];
        let side_exits = [branch_aware_side_exit_proof(&code_block, &site)];
        let request = valid_request(&side_exits);

        assert_eq!(
            p6_arm64_public_branch_aware_callable_admission_proof(&request),
            Err(
                P6Arm64BranchAwareCallableAdmissionRejection::NativeReentryTargetOutsideDescriptorRange {
                    side_exit_index: 0,
                    resume_bytecode_index: bci(2),
                    resume_entry_offset: 64,
                    range: range(),
                }
            )
        );
    }

    #[test]
    fn public_arm64_branch_aware_admission_rejects_legacy_single_target_shape() {
        let code_block = jump_if_false_code_block(4);
        let mut site = jump_if_false_site();
        site.resume_bytecode_index = Some(bci(2));
        site.resume_entry_offset = Some(28);

        assert_eq!(
            admission_for_site(&code_block, &site),
            Err(
                P6Arm64BranchAwareCallableAdmissionRejection::UnexpectedSideExit {
                    side_exit_index: 0,
                    reason: P6X86_64BaselineSelectedSideExitReason::UnsupportedTruthinessOperand,
                    opcode: Some(CoreOpcode::JumpIfFalse),
                }
            )
        );
    }

    #[test]
    fn public_arm64_branch_aware_admission_rejects_missing_extra_or_duplicate_reentry_labels() {
        let code_block = jump_if_false_code_block(4);

        for native_reentry_targets in [
            vec![target(bci(4), 12)],
            vec![target(bci(4), 12), target(bci(2), 28), target(bci(8), 36)],
            vec![target(bci(4), 12), target(bci(4), 36)],
            vec![target(bci(4), 12), target(bci(8), 36)],
        ] {
            let mut site = jump_if_false_site();
            site.native_reentry_targets = native_reentry_targets;

            assert_eq!(
                admission_for_site(&code_block, &site),
                Err(
                    P6Arm64BranchAwareCallableAdmissionRejection::UnexpectedSideExit {
                        side_exit_index: 0,
                        reason:
                            P6X86_64BaselineSelectedSideExitReason::UnsupportedTruthinessOperand,
                        opcode: Some(CoreOpcode::JumpIfFalse),
                    }
                )
            );
        }
    }

    #[test]
    fn public_arm64_branch_aware_admission_rejects_degenerate_or_invalid_decoded_labels() {
        let degenerate_code_block = jump_if_false_code_block(2);
        let mut degenerate_site = jump_if_false_site();
        degenerate_site.native_reentry_targets = vec![target(bci(2), 28), target(bci(2), 36)];

        assert_eq!(
            admission_for_site(&degenerate_code_block, &degenerate_site),
            Err(
                P6Arm64BranchAwareCallableAdmissionRejection::UnexpectedSideExit {
                    side_exit_index: 0,
                    reason: P6X86_64BaselineSelectedSideExitReason::UnsupportedTruthinessOperand,
                    opcode: Some(CoreOpcode::JumpIfFalse),
                }
            )
        );

        let invalid_target_code_block = jump_if_false_code_block(99);
        let mut invalid_target_site = jump_if_false_site();
        invalid_target_site.native_reentry_targets = vec![target(bci(99), 12), target(bci(2), 28)];

        assert_eq!(
            admission_for_site_with_labels(
                &invalid_target_code_block,
                &invalid_target_site,
                Some(CoreOpcode::JumpIfFalse),
                bci(99),
                bci(2),
            ),
            Err(
                P6Arm64BranchAwareCallableAdmissionRejection::UnexpectedSideExit {
                    side_exit_index: 0,
                    reason: P6X86_64BaselineSelectedSideExitReason::UnsupportedTruthinessOperand,
                    opcode: Some(CoreOpcode::JumpIfFalse),
                }
            )
        );

        let terminal_code_block = terminal_jump_if_false_code_block();
        let mut missing_fallthrough_site = jump_if_false_site();
        missing_fallthrough_site.native_reentry_targets =
            vec![target(bci(0), 12), target(bci(2), 28)];

        assert_eq!(
            admission_for_site_with_labels(
                &terminal_code_block,
                &missing_fallthrough_site,
                Some(CoreOpcode::JumpIfFalse),
                bci(0),
                bci(2),
            ),
            Err(
                P6Arm64BranchAwareCallableAdmissionRejection::UnexpectedSideExit {
                    side_exit_index: 0,
                    reason: P6X86_64BaselineSelectedSideExitReason::UnsupportedTruthinessOperand,
                    opcode: Some(CoreOpcode::JumpIfFalse),
                }
            )
        );
    }

    #[test]
    fn public_arm64_branch_aware_admission_rejects_wrong_reason_or_opcode() {
        let code_block = jump_if_false_code_block(4);
        let mut wrong_reason = jump_if_false_site();
        wrong_reason.reason = P6X86_64BaselineSelectedSideExitReason::NonInt32Operand;

        assert_eq!(
            admission_for_site(&code_block, &wrong_reason),
            Err(
                P6Arm64BranchAwareCallableAdmissionRejection::UnexpectedSideExit {
                    side_exit_index: 0,
                    reason: P6X86_64BaselineSelectedSideExitReason::NonInt32Operand,
                    opcode: Some(CoreOpcode::JumpIfFalse),
                }
            )
        );

        let site = jump_if_false_site();
        assert_eq!(
            admission_for_site_with_labels(
                &code_block,
                &site,
                Some(CoreOpcode::AddInt32),
                bci(4),
                bci(2),
            ),
            Err(
                P6Arm64BranchAwareCallableAdmissionRejection::UnexpectedSideExit {
                    side_exit_index: 0,
                    reason: P6X86_64BaselineSelectedSideExitReason::UnsupportedTruthinessOperand,
                    opcode: Some(CoreOpcode::AddInt32),
                }
            )
        );
    }

    #[test]
    fn public_arm64_branch_aware_admission_rejects_proof_label_mismatches() {
        let code_block = jump_if_false_code_block(4);
        let site = jump_if_false_site();

        assert_eq!(
            admission_for_site_with_labels(
                &code_block,
                &site,
                Some(CoreOpcode::JumpIfFalse),
                bci(8),
                bci(2),
            ),
            Err(
                P6Arm64BranchAwareCallableAdmissionRejection::MissingNativeReentryTarget {
                    side_exit_index: 0,
                    resume_bytecode_index: bci(8),
                }
            )
        );

        assert_eq!(
            admission_for_site_with_labels(
                &code_block,
                &site,
                Some(CoreOpcode::JumpIfFalse),
                bci(4),
                bci(8),
            ),
            Err(
                P6Arm64BranchAwareCallableAdmissionRejection::MissingNativeReentryTarget {
                    side_exit_index: 0,
                    resume_bytecode_index: bci(8),
                }
            )
        );
    }
}
