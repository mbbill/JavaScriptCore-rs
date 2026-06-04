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

use crate::bytecode::{BytecodeIndex, CoreOpcode};
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

use super::side_exit::{
    P6CallableSideExitNativeReentryInvocation, P6X86_64CallableSideExitReturnSite,
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
pub(super) enum P6Arm64BranchAwareCallableFallbackRootingProof {
    MissingJscStyleFrameAndMachineStackRoots,
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
#[derive(Clone, Copy, Debug, Eq, PartialEq)]
pub(super) struct P6Arm64BranchAwareCallableSideExitProof<'a> {
    pub(super) site: &'a P6X86_64CallableSideExitReturnSite,
    pub(super) opcode: Option<CoreOpcode>,
    pub(super) target_bytecode_index: BytecodeIndex,
    pub(super) fallthrough_bytecode_index: BytecodeIndex,
}

#[allow(dead_code)]
#[derive(Clone, Copy, Debug, Eq, PartialEq)]
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
    MissingFallbackRootingProof {
        proof: P6Arm64BranchAwareCallableFallbackRootingProof,
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

    // C++ JSC publishes branch targets in finalized baseline code
    // (JITCodeMapBuilder) and keeps CallFrame/topCallFrame visible to
    // FrameTracers, StackVisitor, and conservative stack roots. Rust currently
    // diverges intentionally: the ARM64 branch-aware encoder and retained
    // reentry records are metadata-only, and no public machine-stack/rooting
    // proof exists. The `Infallible` success type keeps this boundary rejected
    // until that proof is designed explicitly.
    Err(
        P6Arm64BranchAwareCallableAdmissionRejection::MissingFallbackRootingProof {
            proof: request.fallback_rooting_proof,
        },
    )
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

    for resume_bytecode_index in [
        proof.target_bytecode_index,
        proof.fallthrough_bytecode_index,
    ] {
        let Some(target) = proof
            .site
            .native_reentry_targets
            .iter()
            .find(|target| target.resume_bytecode_index == resume_bytecode_index)
        else {
            return Err(
                P6Arm64BranchAwareCallableAdmissionRejection::MissingNativeReentryTarget {
                    side_exit_index: proof.site.side_exit_index,
                    resume_bytecode_index,
                },
            );
        };
        if !p6_arm64_image_entry_offset_points_inside_descriptor_range(
            target.resume_entry_offset,
            range,
        ) {
            return Err(
                P6Arm64BranchAwareCallableAdmissionRejection::NativeReentryTargetOutsideDescriptorRange {
                    side_exit_index: proof.site.side_exit_index,
                    resume_bytecode_index,
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
    use crate::jit::{ExecutableAllocationId, P6BaselineNativeReentryTargetRecord};

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
                P6Arm64BranchAwareCallableFallbackRootingProof::MissingJscStyleFrameAndMachineStackRoots,
        }
    }

    #[test]
    fn public_arm64_branch_aware_admission_rejects_missing_fallback_rooting_proof() {
        let site = jump_if_false_site();
        let side_exits = [P6Arm64BranchAwareCallableSideExitProof {
            site: &site,
            opcode: Some(CoreOpcode::JumpIfFalse),
            target_bytecode_index: bci(4),
            fallthrough_bytecode_index: bci(2),
        }];
        let request = valid_request(&side_exits);

        assert_eq!(
            p6_arm64_public_branch_aware_callable_admission_proof(&request),
            Err(
                P6Arm64BranchAwareCallableAdmissionRejection::MissingFallbackRootingProof {
                    proof: P6Arm64BranchAwareCallableFallbackRootingProof::MissingJscStyleFrameAndMachineStackRoots,
                }
            )
        );
    }

    #[test]
    fn public_arm64_branch_aware_admission_rejects_x86_callable_kind() {
        let site = jump_if_false_site();
        let side_exits = [P6Arm64BranchAwareCallableSideExitProof {
            site: &site,
            opcode: Some(CoreOpcode::JumpIfFalse),
            target_bytecode_index: bci(4),
            fallthrough_bytecode_index: bci(2),
        }];
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
        let site = jump_if_false_site();
        let side_exits = [P6Arm64BranchAwareCallableSideExitProof {
            site: &site,
            opcode: Some(CoreOpcode::JumpIfFalse),
            target_bytecode_index: bci(4),
            fallthrough_bytecode_index: bci(2),
        }];
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
        let mut site = jump_if_false_site();
        site.native_reentry_targets = vec![target(bci(4), 12), target(bci(2), 64)];
        let side_exits = [P6Arm64BranchAwareCallableSideExitProof {
            site: &site,
            opcode: Some(CoreOpcode::JumpIfFalse),
            target_bytecode_index: bci(4),
            fallthrough_bytecode_index: bci(2),
        }];
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
}
