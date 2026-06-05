//! ARM64 generated native frame materialization descriptor producer.
//!
//! C++ JSC map: `JITCall.cpp` prepares the callee `CallFrame` header,
//! `AssemblyHelpers::emitFunctionPrologue` pushes `fp/lr` and moves `sp` to
//! `fp`, `JIT.cpp` allocates the baseline frame and materializes x25-x28, and
//! `VMEntryRecord` links the published top-frame pair. This module only
//! produces descriptor evidence for that shape; public ARM64 admission remains
//! rejected by `native_reentry`.

use super::frame_materialization::{
    Arm64BaselineCallFrameHeaderDescriptor, Arm64BaselineEntryLinkageDescriptor,
    Arm64BaselineFrameAddressSource, Arm64BaselineFrameMaterializationTerminalPolicy,
    Arm64BaselineFramePointerSource, Arm64BaselineGeneratedNativeFrameMaterializationDescriptor,
    Arm64BaselineJscPrologueDescriptor, Arm64BaselineLiveRootSlotDescriptor,
    Arm64BaselineMaterializedRegisterDescriptor, Arm64BaselineMaterializedRegisterSource,
    Arm64BaselinePostFrameAllocationDescriptor, JSC_ARM64_PROLOGUE_PUSH_PAIR_DELTA_BYTES,
    JSC_REGISTER_BYTES, JSC_STACK_ALIGNMENT_BYTES,
};
use super::register_contract::{self, Arm64BaselineMaterializedState};

#[allow(dead_code)]
#[derive(Clone, Debug, Eq, PartialEq)]
pub(crate) struct Arm64BaselineGeneratedNativeFrameMaterializationProductionRequest {
    pub(crate) call_frame: usize,
    pub(crate) frame_top_offset_bytes: isize,
    pub(crate) argument_count_excluding_this: u32,
    pub(crate) live_local_count: u32,
    pub(crate) live_root_slots: Vec<Arm64BaselineLiveRootSlotDescriptor>,
    pub(crate) vm_entry_previous_top_call_frame: Option<usize>,
    pub(crate) vm_entry_previous_top_entry_frame: Option<usize>,
    pub(crate) published_top_entry_frame: usize,
}

#[allow(dead_code)]
#[derive(Clone, Debug, Eq, PartialEq)]
pub(crate) enum Arm64BaselineGeneratedNativeFrameMaterializationProductionError {
    EntryStackPointerOverflow {
        call_frame: usize,
    },
    PostFrameAllocationStackPointerOverflow {
        call_frame: usize,
        frame_top_offset_bytes: isize,
    },
    PostFrameAllocationStackPointerUnaligned {
        post_allocation_sp: usize,
    },
    PostFrameAllocationDoesNotCoverLiveLocals {
        live_local_count: u32,
        required_bytes: usize,
        allocated_bytes: usize,
    },
}

#[allow(dead_code)]
pub(crate) fn produce_arm64_baseline_generated_native_frame_materialization_descriptor(
    request: Arm64BaselineGeneratedNativeFrameMaterializationProductionRequest,
) -> Result<
    Arm64BaselineGeneratedNativeFrameMaterializationDescriptor,
    Arm64BaselineGeneratedNativeFrameMaterializationProductionError,
> {
    let entry_sp = request.call_frame.checked_add(
        JSC_ARM64_PROLOGUE_PUSH_PAIR_DELTA_BYTES,
    ).ok_or(
        Arm64BaselineGeneratedNativeFrameMaterializationProductionError::EntryStackPointerOverflow {
            call_frame: request.call_frame,
        },
    )?;
    let post_allocation_sp = request
        .call_frame
        .checked_add_signed(request.frame_top_offset_bytes)
        .ok_or(
            Arm64BaselineGeneratedNativeFrameMaterializationProductionError::PostFrameAllocationStackPointerOverflow {
                call_frame: request.call_frame,
                frame_top_offset_bytes: request.frame_top_offset_bytes,
            },
        )?;
    if post_allocation_sp % JSC_STACK_ALIGNMENT_BYTES != 0 {
        return Err(
            Arm64BaselineGeneratedNativeFrameMaterializationProductionError::PostFrameAllocationStackPointerUnaligned {
                post_allocation_sp,
            },
        );
    }
    let required_local_bytes = usize::try_from(request.live_local_count)
        .unwrap_or(usize::MAX)
        .saturating_mul(JSC_REGISTER_BYTES);
    let allocated_bytes = request.call_frame.checked_sub(post_allocation_sp).ok_or(
        Arm64BaselineGeneratedNativeFrameMaterializationProductionError::PostFrameAllocationStackPointerOverflow {
            call_frame: request.call_frame,
            frame_top_offset_bytes: request.frame_top_offset_bytes,
        },
    )?;
    if allocated_bytes < required_local_bytes {
        return Err(
            Arm64BaselineGeneratedNativeFrameMaterializationProductionError::PostFrameAllocationDoesNotCoverLiveLocals {
                live_local_count: request.live_local_count,
                required_bytes: required_local_bytes,
                allocated_bytes,
            },
        );
    }

    Ok(Arm64BaselineGeneratedNativeFrameMaterializationDescriptor {
        terminal_policy: Arm64BaselineFrameMaterializationTerminalPolicy::JscBaselineGeneratedFrame,
        call_frame_address_source: Arm64BaselineFrameAddressSource::NativeMachineStack,
        prologue: Arm64BaselineJscPrologueDescriptor {
            call_frame: request.call_frame,
            entry_sp,
            post_push_sp: request.call_frame,
            post_prologue_sp: request.call_frame,
            post_prologue_fp: request.call_frame,
            frame_pointer_source:
                Arm64BaselineFramePointerSource::AssemblyHelpersPrologueStackPointer,
        },
        post_frame_allocation: Arm64BaselinePostFrameAllocationDescriptor {
            frame_top_offset_bytes: request.frame_top_offset_bytes,
            post_allocation_sp,
        },
        header: Arm64BaselineCallFrameHeaderDescriptor::jsc(
            request.argument_count_excluding_this,
            request.live_local_count,
        ),
        materialized_registers: required_materialized_registers(),
        live_root_slots: request.live_root_slots,
        entry_linkage: Arm64BaselineEntryLinkageDescriptor {
            vm_entry_record_previous_top_call_frame: request.vm_entry_previous_top_call_frame,
            vm_entry_record_previous_top_entry_frame: request.vm_entry_previous_top_entry_frame,
            published_top_call_frame: request.call_frame,
            published_top_entry_frame: request.published_top_entry_frame,
        },
    })
}

fn required_materialized_registers() -> Vec<Arm64BaselineMaterializedRegisterDescriptor> {
    register_contract::REQUIRED_MATERIALIZED_REGISTER_STATES
        .iter()
        .map(|required| Arm64BaselineMaterializedRegisterDescriptor {
            register: required.register,
            state: required.required_state,
            source: materialized_register_source(required.required_state),
        })
        .collect()
}

const fn materialized_register_source(
    state: Arm64BaselineMaterializedState,
) -> Arm64BaselineMaterializedRegisterSource {
    match state {
        Arm64BaselineMaterializedState::TagConstant(_) => {
            Arm64BaselineMaterializedRegisterSource::AssemblyHelpersTagCheckRegisters
        }
        Arm64BaselineMaterializedState::JitDataFromCodeBlockJitData => {
            Arm64BaselineMaterializedRegisterSource::CodeBlockJitDataField
        }
        Arm64BaselineMaterializedState::MetadataTableFromCodeBlockMetadataTable => {
            Arm64BaselineMaterializedRegisterSource::CodeBlockMetadataTableField
        }
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::gc::{CellId, ConservativeRootCell, ConservativeRootSpan};
    use crate::jit::arm64_baseline::{
        validate_arm64_baseline_generated_native_frame_materialization,
        Arm64BaselineGeneratedNativeFrameMaterializationValidationContext,
        Arm64BaselineLiveRootSlotKind, Arm64BaselineMachineStackRootSlotDescriptor,
        Arm64BaselineMachineStackSpanKind,
    };

    fn root_slot() -> Arm64BaselineMachineStackRootSlotDescriptor {
        Arm64BaselineMachineStackRootSlotDescriptor {
            kind: Arm64BaselineLiveRootSlotKind::ThisValue,
            slot_address: 0x1ff0,
            encoded_payload: 0x5000,
            expected_root: ConservativeRootCell {
                candidate_address: 0x5000,
                cell: CellId(7),
            },
            containing_span: Arm64BaselineMachineStackSpanKind::Stack,
            span: ConservativeRootSpan {
                begin: 0x1fe0,
                end: 0x2000,
            },
        }
    }

    #[test]
    fn arm64_baseline_frame_materialization_producer_builds_valid_descriptor() {
        let slot = root_slot();
        let descriptor = produce_arm64_baseline_generated_native_frame_materialization_descriptor(
            Arm64BaselineGeneratedNativeFrameMaterializationProductionRequest {
                call_frame: 0x2000,
                frame_top_offset_bytes: -0x20,
                argument_count_excluding_this: 2,
                live_local_count: 1,
                live_root_slots: vec![slot],
                vm_entry_previous_top_call_frame: Some(0x1000),
                vm_entry_previous_top_entry_frame: Some(0x1100),
                published_top_entry_frame: 0x3000,
            },
        )
        .expect("valid JSC-shaped ARM64 frame materialization descriptor");

        validate_arm64_baseline_generated_native_frame_materialization(
            &Arm64BaselineGeneratedNativeFrameMaterializationValidationContext {
                published_top_frame: 0x2000,
                residency_top_frame: 0x2000,
                expected_argument_slots_excluding_this: 2,
                expected_live_local_slots: 1,
                vm_entry_previous_top_call_frame: Some(0x1000),
                vm_entry_previous_top_entry_frame: Some(0x1100),
                current_top_entry_frame: 0x3000,
                residency_live_root_slots: vec![slot],
            },
            &descriptor,
        )
        .expect("producer descriptor should satisfy the existing validator");
    }

    #[test]
    fn arm64_baseline_frame_materialization_producer_rejects_unaligned_frame_top() {
        assert_eq!(
            produce_arm64_baseline_generated_native_frame_materialization_descriptor(
                Arm64BaselineGeneratedNativeFrameMaterializationProductionRequest {
                    call_frame: 0x2000,
                    frame_top_offset_bytes: -8,
                    argument_count_excluding_this: 0,
                    live_local_count: 0,
                    live_root_slots: Vec::new(),
                    vm_entry_previous_top_call_frame: None,
                    vm_entry_previous_top_entry_frame: None,
                    published_top_entry_frame: 0x3000,
                },
            ),
            Err(
                Arm64BaselineGeneratedNativeFrameMaterializationProductionError::PostFrameAllocationStackPointerUnaligned {
                    post_allocation_sp: 0x1ff8,
                },
            )
        );
    }

    #[test]
    fn arm64_baseline_frame_materialization_producer_rejects_live_locals_beyond_allocation() {
        assert_eq!(
            produce_arm64_baseline_generated_native_frame_materialization_descriptor(
                Arm64BaselineGeneratedNativeFrameMaterializationProductionRequest {
                    call_frame: 0x2000,
                    frame_top_offset_bytes: 0,
                    argument_count_excluding_this: 0,
                    live_local_count: 1,
                    live_root_slots: Vec::new(),
                    vm_entry_previous_top_call_frame: None,
                    vm_entry_previous_top_entry_frame: None,
                    published_top_entry_frame: 0x3000,
                },
            ),
            Err(
                Arm64BaselineGeneratedNativeFrameMaterializationProductionError::PostFrameAllocationDoesNotCoverLiveLocals {
                    live_local_count: 1,
                    required_bytes: JSC_REGISTER_BYTES,
                    allocated_bytes: 0,
                },
            )
        );
    }
}
