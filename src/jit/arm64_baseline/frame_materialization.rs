//! ARM64 generated native frame materialization descriptors.
//!
//! C++ JSC map: this boundary mirrors `AssemblyHelpers::emitFunctionPrologue`,
//! ARM64 `GPRInfo`, baseline `JIT.cpp` frame setup/materialized registers,
//! `CallFrame.h` header slots, `JITCall.cpp` callee-frame setup,
//! `VMEntryRecord.h`/`EntryFrame.h` linkage, and `FrameTracers.h`
//! top-call-frame publication. It validates descriptor evidence only; it does
//! not admit public ARM64 native execution.

use crate::gc::{ConservativeRootCell, ConservativeRootSpan};

use super::register_contract::{self, Arm64BaselineMaterializedState, Arm64Gpr};

pub(crate) const JSC_REGISTER_BYTES: usize = 8;
pub(crate) const JSC_CALLER_FRAME_AND_PC_BYTES: usize = 2 * JSC_REGISTER_BYTES;
pub(crate) const JSC_ARM64_PROLOGUE_PUSH_PAIR_DELTA_BYTES: usize = 2 * JSC_REGISTER_BYTES;
pub(crate) const JSC_STACK_ALIGNMENT_BYTES: usize = 16;

const CALLER_FRAME_OFFSET: isize = 0;
const RETURN_PC_OFFSET: isize = JSC_REGISTER_BYTES as isize;
const CODE_BLOCK_OFFSET: isize = JSC_CALLER_FRAME_AND_PC_BYTES as isize;
const CALLEE_OFFSET: isize = CODE_BLOCK_OFFSET + JSC_REGISTER_BYTES as isize;
const ARGUMENT_COUNT_AND_CALL_SITE_BITS_OFFSET: isize = CALLEE_OFFSET + JSC_REGISTER_BYTES as isize;
const THIS_ARGUMENT_OFFSET: isize =
    ARGUMENT_COUNT_AND_CALL_SITE_BITS_OFFSET + JSC_REGISTER_BYTES as isize;
const FIRST_ARGUMENT_OFFSET: isize = THIS_ARGUMENT_OFFSET + JSC_REGISTER_BYTES as isize;

#[allow(dead_code)]
#[derive(Clone, Debug, Eq, PartialEq)]
pub(crate) struct Arm64BaselineGeneratedNativeFrameMaterializationDescriptor {
    pub(crate) terminal_policy: Arm64BaselineFrameMaterializationTerminalPolicy,
    pub(crate) call_frame_address_source: Arm64BaselineFrameAddressSource,
    pub(crate) prologue: Arm64BaselineJscPrologueDescriptor,
    pub(crate) post_frame_allocation: Arm64BaselinePostFrameAllocationDescriptor,
    pub(crate) header: Arm64BaselineCallFrameHeaderDescriptor,
    pub(crate) materialized_registers: Vec<Arm64BaselineMaterializedRegisterDescriptor>,
    pub(crate) live_root_slots: Vec<Arm64BaselineLiveRootSlotDescriptor>,
    pub(crate) entry_linkage: Arm64BaselineEntryLinkageDescriptor,
}

#[allow(dead_code)]
#[derive(Clone, Debug, Eq, PartialEq)]
pub(crate) struct Arm64BaselineGeneratedNativeFrameMaterializationValidationContext {
    pub(crate) published_top_frame: usize,
    pub(crate) residency_top_frame: usize,
    pub(crate) expected_argument_slots_excluding_this: usize,
    pub(crate) expected_live_local_slots: usize,
    pub(crate) vm_entry_previous_top_call_frame: Option<usize>,
    pub(crate) vm_entry_previous_top_entry_frame: Option<usize>,
    pub(crate) current_top_entry_frame: usize,
    pub(crate) residency_live_root_slots: Vec<Arm64BaselineMachineStackRootSlotDescriptor>,
}

#[allow(dead_code)]
#[derive(Clone, Copy, Debug, Eq, PartialEq)]
pub(crate) enum Arm64BaselineFrameMaterializationTerminalPolicy {
    JscBaselineGeneratedFrame,
    RustCAbiReturnSeed,
}

#[allow(dead_code)]
#[derive(Clone, Copy, Debug, Eq, PartialEq)]
pub(crate) enum Arm64BaselineFrameAddressSource {
    NativeMachineStack,
    JscCallFrameStorageBox,
    RustCAbiFrameBaseCarrier,
    MetadataRecordOnly,
}

#[allow(dead_code)]
#[derive(Clone, Copy, Debug, Eq, PartialEq)]
pub(crate) enum Arm64BaselineFramePointerSource {
    AssemblyHelpersPrologueStackPointer,
    RustCAbiX1Carrier,
    JscCallFrameStorageBox,
    MetadataRecordOnly,
}

#[allow(dead_code)]
#[derive(Clone, Copy, Debug, Eq, PartialEq)]
pub(crate) struct Arm64BaselineJscPrologueDescriptor {
    pub(crate) call_frame: usize,
    pub(crate) entry_sp: usize,
    pub(crate) post_push_sp: usize,
    pub(crate) post_prologue_sp: usize,
    pub(crate) post_prologue_fp: usize,
    pub(crate) frame_pointer_source: Arm64BaselineFramePointerSource,
}

#[allow(dead_code)]
#[derive(Clone, Copy, Debug, Eq, PartialEq)]
pub(crate) struct Arm64BaselinePostFrameAllocationDescriptor {
    pub(crate) frame_top_offset_bytes: isize,
    pub(crate) post_allocation_sp: usize,
}

#[allow(dead_code)]
#[derive(Clone, Debug, Eq, PartialEq)]
pub(crate) struct Arm64BaselineCallFrameHeaderDescriptor {
    pub(crate) caller_frame_offset: isize,
    pub(crate) return_pc_offset: isize,
    pub(crate) code_block_offset: isize,
    pub(crate) callee_offset: isize,
    pub(crate) argument_count_and_call_site_bits_offset: isize,
    pub(crate) this_argument_offset: isize,
    pub(crate) first_argument_offset: isize,
    pub(crate) arguments: Vec<Arm64BaselineArgumentSlotDescriptor>,
    pub(crate) live_locals: Vec<Arm64BaselineLiveLocalSlotDescriptor>,
}

impl Arm64BaselineCallFrameHeaderDescriptor {
    #[allow(dead_code)]
    pub(crate) fn jsc(function_argument_count: u32, live_local_count: u32) -> Self {
        Self {
            caller_frame_offset: CALLER_FRAME_OFFSET,
            return_pc_offset: RETURN_PC_OFFSET,
            code_block_offset: CODE_BLOCK_OFFSET,
            callee_offset: CALLEE_OFFSET,
            argument_count_and_call_site_bits_offset: ARGUMENT_COUNT_AND_CALL_SITE_BITS_OFFSET,
            this_argument_offset: THIS_ARGUMENT_OFFSET,
            first_argument_offset: FIRST_ARGUMENT_OFFSET,
            arguments: (0..function_argument_count)
                .map(|argument_index| Arm64BaselineArgumentSlotDescriptor {
                    argument_index,
                    offset_from_call_frame: FIRST_ARGUMENT_OFFSET
                        + isize::try_from(argument_index).unwrap_or(isize::MAX)
                            * JSC_REGISTER_BYTES as isize,
                })
                .collect(),
            live_locals: (0..live_local_count)
                .map(|local_index| Arm64BaselineLiveLocalSlotDescriptor {
                    local_index,
                    offset_from_call_frame: -((isize::try_from(local_index).unwrap_or(isize::MAX)
                        + 1)
                        * JSC_REGISTER_BYTES as isize),
                })
                .collect(),
        }
    }
}

#[allow(dead_code)]
#[derive(Clone, Copy, Debug, Eq, PartialEq)]
pub(crate) struct Arm64BaselineArgumentSlotDescriptor {
    pub(crate) argument_index: u32,
    pub(crate) offset_from_call_frame: isize,
}

#[allow(dead_code)]
#[derive(Clone, Copy, Debug, Eq, PartialEq)]
pub(crate) struct Arm64BaselineLiveLocalSlotDescriptor {
    pub(crate) local_index: u32,
    pub(crate) offset_from_call_frame: isize,
}

#[allow(dead_code)]
#[derive(Clone, Copy, Debug, Eq, PartialEq)]
pub(crate) enum Arm64BaselineMaterializedRegisterSource {
    AssemblyHelpersTagCheckRegisters,
    CodeBlockJitDataField,
    CodeBlockMetadataTableField,
    RustMetadataRecord,
    RustCAbiCarrier,
    Unmaterialized,
}

#[allow(dead_code)]
#[derive(Clone, Copy, Debug, Eq, PartialEq)]
pub(crate) struct Arm64BaselineMaterializedRegisterDescriptor {
    pub(crate) register: Arm64Gpr,
    pub(crate) state: Arm64BaselineMaterializedState,
    pub(crate) source: Arm64BaselineMaterializedRegisterSource,
}

#[allow(dead_code)]
#[derive(Clone, Copy, Debug, Eq, PartialEq)]
pub(crate) enum Arm64BaselineLiveRootSlotKind {
    Callee,
    ThisValue,
    Argument,
    Local,
    Scratch,
}

#[allow(dead_code)]
#[derive(Clone, Copy, Debug, Eq, PartialEq)]
pub(crate) enum Arm64BaselineMachineStackSpanKind {
    RegisterState,
    Stack,
}

#[allow(dead_code)]
#[derive(Clone, Copy, Debug, Eq, PartialEq)]
pub(crate) struct Arm64BaselineMachineStackRootSlotDescriptor {
    pub(crate) kind: Arm64BaselineLiveRootSlotKind,
    pub(crate) slot_address: usize,
    pub(crate) encoded_payload: usize,
    pub(crate) expected_root: ConservativeRootCell,
    pub(crate) containing_span: Arm64BaselineMachineStackSpanKind,
    pub(crate) span: ConservativeRootSpan,
}

#[allow(dead_code)]
pub(crate) type Arm64BaselineLiveRootSlotDescriptor = Arm64BaselineMachineStackRootSlotDescriptor;

#[allow(dead_code)]
#[derive(Clone, Copy, Debug, Eq, PartialEq)]
pub(crate) struct Arm64BaselineEntryLinkageDescriptor {
    pub(crate) vm_entry_record_previous_top_call_frame: Option<usize>,
    pub(crate) vm_entry_record_previous_top_entry_frame: Option<usize>,
    pub(crate) published_top_call_frame: usize,
    pub(crate) published_top_entry_frame: usize,
}

#[allow(dead_code)]
#[derive(Clone, Debug, Eq, PartialEq)]
pub(crate) enum Arm64BaselineGeneratedNativeFrameMaterializationMismatch {
    TerminalPolicyMismatch {
        actual: Arm64BaselineFrameMaterializationTerminalPolicy,
    },
    CallFrameAddressSourceMismatch {
        actual: Arm64BaselineFrameAddressSource,
    },
    FramePointerSourceMismatch {
        actual: Arm64BaselineFramePointerSource,
    },
    PublishedTopFrameMismatch {
        frame_pointer: usize,
        published_top_frame: usize,
    },
    ResidencyTopFrameMismatch {
        frame_pointer: usize,
        residency_top_frame: usize,
    },
    EntryStackPointerMismatch {
        expected: usize,
        actual: usize,
    },
    ProloguePushPairDeltaMismatch {
        expected: usize,
        actual: usize,
    },
    PostPushStackPointerMismatch {
        expected: usize,
        actual: usize,
    },
    PostPrologueFramePointerMismatch {
        expected: usize,
        actual: usize,
    },
    PostFrameAllocationStackPointerMismatch {
        expected: usize,
        actual: usize,
    },
    PostFrameAllocationStackPointerUnaligned {
        actual: usize,
    },
    HeaderSlotOffsetMismatch {
        slot: Arm64BaselineCallFrameHeaderSlot,
        expected: isize,
        actual: isize,
    },
    ArgumentSlotCountMismatch {
        expected: usize,
        actual: usize,
    },
    ArgumentSlotIndexMismatch {
        order: usize,
        expected: u32,
        actual: u32,
    },
    ArgumentSlotOffsetMismatch {
        argument_index: u32,
        expected: isize,
        actual: isize,
    },
    LiveLocalSlotCountMismatch {
        expected: usize,
        actual: usize,
    },
    LiveLocalSlotIndexMismatch {
        order: usize,
        expected: u32,
        actual: u32,
    },
    LiveLocalSlotOffsetMismatch {
        local_index: u32,
        expected: isize,
        actual: isize,
    },
    MaterializedRegisterMissing {
        register: Arm64Gpr,
    },
    MaterializedRegisterDuplicate {
        register: Arm64Gpr,
    },
    MaterializedRegisterStateMismatch {
        register: Arm64Gpr,
        expected: Arm64BaselineMaterializedState,
        actual: Arm64BaselineMaterializedState,
    },
    MaterializedRegisterSourceMismatch {
        register: Arm64Gpr,
        expected: Arm64BaselineMaterializedRegisterSource,
        actual: Arm64BaselineMaterializedRegisterSource,
    },
    LiveRootSlotCountMismatch {
        expected: usize,
        actual: usize,
    },
    LiveRootSlotMismatch {
        order: usize,
        expected: Arm64BaselineMachineStackRootSlotDescriptor,
        actual: Arm64BaselineLiveRootSlotDescriptor,
    },
    EntryPreviousTopCallFrameMismatch {
        expected: Option<usize>,
        actual: Option<usize>,
    },
    EntryPreviousTopEntryFrameMismatch {
        expected: Option<usize>,
        actual: Option<usize>,
    },
    EntryPublishedTopCallFrameMismatch {
        expected: usize,
        actual: usize,
    },
    EntryPublishedTopEntryFrameMismatch {
        expected: usize,
        actual: usize,
    },
}

#[allow(dead_code)]
#[derive(Clone, Copy, Debug, Eq, PartialEq)]
pub(crate) enum Arm64BaselineCallFrameHeaderSlot {
    CallerFrame,
    ReturnPc,
    CodeBlock,
    Callee,
    ArgumentCountAndCallSiteBits,
    ThisArgument,
    FirstArgument,
}

#[allow(dead_code)]
pub(crate) fn validate_arm64_baseline_generated_native_frame_materialization(
    context: &Arm64BaselineGeneratedNativeFrameMaterializationValidationContext,
    descriptor: &Arm64BaselineGeneratedNativeFrameMaterializationDescriptor,
) -> Result<(), Arm64BaselineGeneratedNativeFrameMaterializationMismatch> {
    validate_terminal_and_frame_source(descriptor)?;
    validate_prologue(context, &descriptor.prologue)?;
    validate_post_frame_allocation(&descriptor.prologue, descriptor.post_frame_allocation)?;
    validate_header(
        &descriptor.header,
        context.expected_argument_slots_excluding_this,
        context.expected_live_local_slots,
    )?;
    validate_materialized_registers(&descriptor.materialized_registers)?;
    validate_live_root_slots(context, &descriptor.live_root_slots)?;
    validate_entry_linkage(context, descriptor.entry_linkage)?;
    Ok(())
}

fn validate_terminal_and_frame_source(
    descriptor: &Arm64BaselineGeneratedNativeFrameMaterializationDescriptor,
) -> Result<(), Arm64BaselineGeneratedNativeFrameMaterializationMismatch> {
    if descriptor.terminal_policy
        != Arm64BaselineFrameMaterializationTerminalPolicy::JscBaselineGeneratedFrame
    {
        return Err(
            Arm64BaselineGeneratedNativeFrameMaterializationMismatch::TerminalPolicyMismatch {
                actual: descriptor.terminal_policy,
            },
        );
    }
    if descriptor.call_frame_address_source != Arm64BaselineFrameAddressSource::NativeMachineStack {
        return Err(
            Arm64BaselineGeneratedNativeFrameMaterializationMismatch::CallFrameAddressSourceMismatch {
                actual: descriptor.call_frame_address_source,
            },
        );
    }
    Ok(())
}

fn validate_prologue(
    context: &Arm64BaselineGeneratedNativeFrameMaterializationValidationContext,
    prologue: &Arm64BaselineJscPrologueDescriptor,
) -> Result<(), Arm64BaselineGeneratedNativeFrameMaterializationMismatch> {
    if prologue.frame_pointer_source
        != Arm64BaselineFramePointerSource::AssemblyHelpersPrologueStackPointer
    {
        return Err(
            Arm64BaselineGeneratedNativeFrameMaterializationMismatch::FramePointerSourceMismatch {
                actual: prologue.frame_pointer_source,
            },
        );
    }
    if prologue.post_prologue_fp != context.published_top_frame {
        return Err(
            Arm64BaselineGeneratedNativeFrameMaterializationMismatch::PublishedTopFrameMismatch {
                frame_pointer: prologue.post_prologue_fp,
                published_top_frame: context.published_top_frame,
            },
        );
    }
    if prologue.post_prologue_fp != context.residency_top_frame {
        return Err(
            Arm64BaselineGeneratedNativeFrameMaterializationMismatch::ResidencyTopFrameMismatch {
                frame_pointer: prologue.post_prologue_fp,
                residency_top_frame: context.residency_top_frame,
            },
        );
    }
    let expected_entry_sp = prologue
        .call_frame
        .checked_add(JSC_CALLER_FRAME_AND_PC_BYTES)
        .ok_or(
            Arm64BaselineGeneratedNativeFrameMaterializationMismatch::EntryStackPointerMismatch {
                expected: usize::MAX,
                actual: prologue.entry_sp,
            },
        )?;
    if prologue.entry_sp != expected_entry_sp {
        return Err(
            Arm64BaselineGeneratedNativeFrameMaterializationMismatch::EntryStackPointerMismatch {
                expected: expected_entry_sp,
                actual: prologue.entry_sp,
            },
        );
    }
    let actual_delta = prologue
        .entry_sp
        .checked_sub(prologue.post_push_sp)
        .unwrap_or(usize::MAX);
    if actual_delta != JSC_ARM64_PROLOGUE_PUSH_PAIR_DELTA_BYTES {
        return Err(
            Arm64BaselineGeneratedNativeFrameMaterializationMismatch::ProloguePushPairDeltaMismatch {
                expected: JSC_ARM64_PROLOGUE_PUSH_PAIR_DELTA_BYTES,
                actual: actual_delta,
            },
        );
    }
    if prologue.post_push_sp != prologue.call_frame {
        return Err(
            Arm64BaselineGeneratedNativeFrameMaterializationMismatch::PostPushStackPointerMismatch {
                expected: prologue.call_frame,
                actual: prologue.post_push_sp,
            },
        );
    }
    if prologue.post_prologue_sp != prologue.call_frame
        || prologue.post_prologue_fp != prologue.post_prologue_sp
    {
        return Err(
            Arm64BaselineGeneratedNativeFrameMaterializationMismatch::PostPrologueFramePointerMismatch {
                expected: prologue.call_frame,
                actual: prologue.post_prologue_fp,
            },
        );
    }
    Ok(())
}

fn validate_post_frame_allocation(
    prologue: &Arm64BaselineJscPrologueDescriptor,
    allocation: Arm64BaselinePostFrameAllocationDescriptor,
) -> Result<(), Arm64BaselineGeneratedNativeFrameMaterializationMismatch> {
    let expected = prologue
        .call_frame
        .checked_add_signed(allocation.frame_top_offset_bytes)
        .ok_or(
            Arm64BaselineGeneratedNativeFrameMaterializationMismatch::PostFrameAllocationStackPointerMismatch {
                expected: usize::MAX,
                actual: allocation.post_allocation_sp,
            },
        )?;
    if allocation.post_allocation_sp != expected {
        return Err(
            Arm64BaselineGeneratedNativeFrameMaterializationMismatch::PostFrameAllocationStackPointerMismatch {
                expected,
                actual: allocation.post_allocation_sp,
            },
        );
    }
    if allocation.post_allocation_sp % JSC_STACK_ALIGNMENT_BYTES != 0 {
        return Err(
            Arm64BaselineGeneratedNativeFrameMaterializationMismatch::PostFrameAllocationStackPointerUnaligned {
                actual: allocation.post_allocation_sp,
            },
        );
    }
    Ok(())
}

fn validate_header(
    header: &Arm64BaselineCallFrameHeaderDescriptor,
    expected_argument_slots_excluding_this: usize,
    expected_live_local_slots: usize,
) -> Result<(), Arm64BaselineGeneratedNativeFrameMaterializationMismatch> {
    for (slot, expected, actual) in [
        (
            Arm64BaselineCallFrameHeaderSlot::CallerFrame,
            CALLER_FRAME_OFFSET,
            header.caller_frame_offset,
        ),
        (
            Arm64BaselineCallFrameHeaderSlot::ReturnPc,
            RETURN_PC_OFFSET,
            header.return_pc_offset,
        ),
        (
            Arm64BaselineCallFrameHeaderSlot::CodeBlock,
            CODE_BLOCK_OFFSET,
            header.code_block_offset,
        ),
        (
            Arm64BaselineCallFrameHeaderSlot::Callee,
            CALLEE_OFFSET,
            header.callee_offset,
        ),
        (
            Arm64BaselineCallFrameHeaderSlot::ArgumentCountAndCallSiteBits,
            ARGUMENT_COUNT_AND_CALL_SITE_BITS_OFFSET,
            header.argument_count_and_call_site_bits_offset,
        ),
        (
            Arm64BaselineCallFrameHeaderSlot::ThisArgument,
            THIS_ARGUMENT_OFFSET,
            header.this_argument_offset,
        ),
        (
            Arm64BaselineCallFrameHeaderSlot::FirstArgument,
            FIRST_ARGUMENT_OFFSET,
            header.first_argument_offset,
        ),
    ] {
        if actual != expected {
            return Err(
                Arm64BaselineGeneratedNativeFrameMaterializationMismatch::HeaderSlotOffsetMismatch {
                    slot,
                    expected,
                    actual,
                },
            );
        }
    }
    if header.arguments.len() != expected_argument_slots_excluding_this {
        return Err(
            Arm64BaselineGeneratedNativeFrameMaterializationMismatch::ArgumentSlotCountMismatch {
                expected: expected_argument_slots_excluding_this,
                actual: header.arguments.len(),
            },
        );
    }
    for (order, argument) in header.arguments.iter().enumerate() {
        let expected_index = u32::try_from(order).unwrap_or(u32::MAX);
        if argument.argument_index != expected_index {
            return Err(
                Arm64BaselineGeneratedNativeFrameMaterializationMismatch::ArgumentSlotIndexMismatch {
                    order,
                    expected: expected_index,
                    actual: argument.argument_index,
                },
            );
        }
        let expected = FIRST_ARGUMENT_OFFSET
            + isize::try_from(argument.argument_index).unwrap_or(isize::MAX)
                * JSC_REGISTER_BYTES as isize;
        if argument.offset_from_call_frame != expected {
            return Err(
                Arm64BaselineGeneratedNativeFrameMaterializationMismatch::ArgumentSlotOffsetMismatch {
                    argument_index: argument.argument_index,
                    expected,
                    actual: argument.offset_from_call_frame,
                },
            );
        }
    }
    if header.live_locals.len() != expected_live_local_slots {
        return Err(
            Arm64BaselineGeneratedNativeFrameMaterializationMismatch::LiveLocalSlotCountMismatch {
                expected: expected_live_local_slots,
                actual: header.live_locals.len(),
            },
        );
    }
    for (order, local) in header.live_locals.iter().enumerate() {
        let expected_index = u32::try_from(order).unwrap_or(u32::MAX);
        if local.local_index != expected_index {
            return Err(
                Arm64BaselineGeneratedNativeFrameMaterializationMismatch::LiveLocalSlotIndexMismatch {
                    order,
                    expected: expected_index,
                    actual: local.local_index,
                },
            );
        }
        let expected = -((isize::try_from(local.local_index).unwrap_or(isize::MAX) + 1)
            * JSC_REGISTER_BYTES as isize);
        if local.offset_from_call_frame != expected {
            return Err(
                Arm64BaselineGeneratedNativeFrameMaterializationMismatch::LiveLocalSlotOffsetMismatch {
                    local_index: local.local_index,
                    expected,
                    actual: local.offset_from_call_frame,
                },
            );
        }
    }
    Ok(())
}

fn validate_materialized_registers(
    registers: &[Arm64BaselineMaterializedRegisterDescriptor],
) -> Result<(), Arm64BaselineGeneratedNativeFrameMaterializationMismatch> {
    for required in register_contract::REQUIRED_MATERIALIZED_REGISTER_STATES {
        let mut matches = registers
            .iter()
            .filter(|record| record.register == required.register);
        let Some(actual) = matches.next() else {
            return Err(
                Arm64BaselineGeneratedNativeFrameMaterializationMismatch::MaterializedRegisterMissing {
                    register: required.register,
                },
            );
        };
        if matches.next().is_some() {
            return Err(
                Arm64BaselineGeneratedNativeFrameMaterializationMismatch::MaterializedRegisterDuplicate {
                    register: required.register,
                },
            );
        }
        if actual.state != required.required_state {
            return Err(
                Arm64BaselineGeneratedNativeFrameMaterializationMismatch::MaterializedRegisterStateMismatch {
                    register: required.register,
                    expected: required.required_state,
                    actual: actual.state,
                },
            );
        }
        let expected_source = match required.required_state {
            Arm64BaselineMaterializedState::TagConstant(_) => {
                Arm64BaselineMaterializedRegisterSource::AssemblyHelpersTagCheckRegisters
            }
            Arm64BaselineMaterializedState::JitDataFromCodeBlockJitData => {
                Arm64BaselineMaterializedRegisterSource::CodeBlockJitDataField
            }
            Arm64BaselineMaterializedState::MetadataTableFromCodeBlockMetadataTable => {
                Arm64BaselineMaterializedRegisterSource::CodeBlockMetadataTableField
            }
        };
        if actual.source != expected_source {
            return Err(
                Arm64BaselineGeneratedNativeFrameMaterializationMismatch::MaterializedRegisterSourceMismatch {
                    register: required.register,
                    expected: expected_source,
                    actual: actual.source,
                },
            );
        }
    }
    Ok(())
}

fn validate_live_root_slots(
    context: &Arm64BaselineGeneratedNativeFrameMaterializationValidationContext,
    live_root_slots: &[Arm64BaselineLiveRootSlotDescriptor],
) -> Result<(), Arm64BaselineGeneratedNativeFrameMaterializationMismatch> {
    if live_root_slots.len() != context.residency_live_root_slots.len() {
        return Err(
            Arm64BaselineGeneratedNativeFrameMaterializationMismatch::LiveRootSlotCountMismatch {
                expected: context.residency_live_root_slots.len(),
                actual: live_root_slots.len(),
            },
        );
    }
    for (order, (expected, actual)) in context
        .residency_live_root_slots
        .iter()
        .copied()
        .zip(live_root_slots.iter().copied())
        .enumerate()
    {
        if actual != expected {
            return Err(
                Arm64BaselineGeneratedNativeFrameMaterializationMismatch::LiveRootSlotMismatch {
                    order,
                    expected,
                    actual,
                },
            );
        }
    }
    Ok(())
}

fn validate_entry_linkage(
    context: &Arm64BaselineGeneratedNativeFrameMaterializationValidationContext,
    linkage: Arm64BaselineEntryLinkageDescriptor,
) -> Result<(), Arm64BaselineGeneratedNativeFrameMaterializationMismatch> {
    if linkage.vm_entry_record_previous_top_call_frame != context.vm_entry_previous_top_call_frame {
        return Err(
            Arm64BaselineGeneratedNativeFrameMaterializationMismatch::EntryPreviousTopCallFrameMismatch {
                expected: context.vm_entry_previous_top_call_frame,
                actual: linkage.vm_entry_record_previous_top_call_frame,
            },
        );
    }
    if linkage.vm_entry_record_previous_top_entry_frame != context.vm_entry_previous_top_entry_frame
    {
        return Err(
            Arm64BaselineGeneratedNativeFrameMaterializationMismatch::EntryPreviousTopEntryFrameMismatch {
                expected: context.vm_entry_previous_top_entry_frame,
                actual: linkage.vm_entry_record_previous_top_entry_frame,
            },
        );
    }
    if linkage.published_top_call_frame != context.published_top_frame {
        return Err(
            Arm64BaselineGeneratedNativeFrameMaterializationMismatch::EntryPublishedTopCallFrameMismatch {
                expected: context.published_top_frame,
                actual: linkage.published_top_call_frame,
            },
        );
    }
    if linkage.published_top_entry_frame != context.current_top_entry_frame {
        return Err(
            Arm64BaselineGeneratedNativeFrameMaterializationMismatch::EntryPublishedTopEntryFrameMismatch {
                expected: context.current_top_entry_frame,
                actual: linkage.published_top_entry_frame,
            },
        );
    }
    Ok(())
}

#[cfg(test)]
mod tests {
    use super::super::register_contract::{self, Arm64BaselineMaterializedState as State};
    use super::*;
    use crate::gc::{CellId, ConservativeRootCell, ConservativeRootSpan};

    fn root() -> ConservativeRootCell {
        ConservativeRootCell {
            candidate_address: 0x7000,
            cell: CellId(7),
        }
    }

    fn stack_span() -> ConservativeRootSpan {
        ConservativeRootSpan {
            begin: 0x0f00,
            end: 0x1200,
        }
    }

    fn root_slot() -> Arm64BaselineMachineStackRootSlotDescriptor {
        Arm64BaselineMachineStackRootSlotDescriptor {
            kind: Arm64BaselineLiveRootSlotKind::ThisValue,
            slot_address: 0x1030,
            encoded_payload: root().candidate_address,
            expected_root: root(),
            containing_span: Arm64BaselineMachineStackSpanKind::Stack,
            span: stack_span(),
        }
    }

    fn context() -> Arm64BaselineGeneratedNativeFrameMaterializationValidationContext {
        Arm64BaselineGeneratedNativeFrameMaterializationValidationContext {
            published_top_frame: 0x1000,
            residency_top_frame: 0x1000,
            expected_argument_slots_excluding_this: 2,
            expected_live_local_slots: 2,
            vm_entry_previous_top_call_frame: Some(0x9000),
            vm_entry_previous_top_entry_frame: Some(0xa000),
            current_top_entry_frame: 0x8000,
            residency_live_root_slots: vec![root_slot()],
        }
    }

    fn materialized_registers() -> Vec<Arm64BaselineMaterializedRegisterDescriptor> {
        vec![
            Arm64BaselineMaterializedRegisterDescriptor {
                register: register_contract::NUMBER_TAG_REGISTER,
                state: State::TagConstant(register_contract::Arm64BaselineTagConstant::NumberTag),
                source: Arm64BaselineMaterializedRegisterSource::AssemblyHelpersTagCheckRegisters,
            },
            Arm64BaselineMaterializedRegisterDescriptor {
                register: register_contract::NOT_CELL_MASK_REGISTER,
                state: State::TagConstant(register_contract::Arm64BaselineTagConstant::NotCellMask),
                source: Arm64BaselineMaterializedRegisterSource::AssemblyHelpersTagCheckRegisters,
            },
            Arm64BaselineMaterializedRegisterDescriptor {
                register: register_contract::JIT_DATA_REGISTER,
                state: State::JitDataFromCodeBlockJitData,
                source: Arm64BaselineMaterializedRegisterSource::CodeBlockJitDataField,
            },
            Arm64BaselineMaterializedRegisterDescriptor {
                register: register_contract::METADATA_TABLE_REGISTER,
                state: State::MetadataTableFromCodeBlockMetadataTable,
                source: Arm64BaselineMaterializedRegisterSource::CodeBlockMetadataTableField,
            },
        ]
    }

    fn valid_descriptor() -> Arm64BaselineGeneratedNativeFrameMaterializationDescriptor {
        Arm64BaselineGeneratedNativeFrameMaterializationDescriptor {
            terminal_policy:
                Arm64BaselineFrameMaterializationTerminalPolicy::JscBaselineGeneratedFrame,
            call_frame_address_source: Arm64BaselineFrameAddressSource::NativeMachineStack,
            prologue: Arm64BaselineJscPrologueDescriptor {
                call_frame: 0x1000,
                entry_sp: 0x1010,
                post_push_sp: 0x1000,
                post_prologue_sp: 0x1000,
                post_prologue_fp: 0x1000,
                frame_pointer_source:
                    Arm64BaselineFramePointerSource::AssemblyHelpersPrologueStackPointer,
            },
            post_frame_allocation: Arm64BaselinePostFrameAllocationDescriptor {
                frame_top_offset_bytes: -0x20,
                post_allocation_sp: 0x0fe0,
            },
            header: Arm64BaselineCallFrameHeaderDescriptor::jsc(2, 2),
            materialized_registers: materialized_registers(),
            live_root_slots: vec![root_slot()],
            entry_linkage: Arm64BaselineEntryLinkageDescriptor {
                vm_entry_record_previous_top_call_frame: Some(0x9000),
                vm_entry_record_previous_top_entry_frame: Some(0xa000),
                published_top_call_frame: 0x1000,
                published_top_entry_frame: 0x8000,
            },
        }
    }

    fn assert_descriptor_mismatch(
        descriptor: Arm64BaselineGeneratedNativeFrameMaterializationDescriptor,
        expected: Arm64BaselineGeneratedNativeFrameMaterializationMismatch,
    ) {
        assert_eq!(
            validate_arm64_baseline_generated_native_frame_materialization(&context(), &descriptor),
            Err(expected)
        );
    }

    #[test]
    fn arm64_baseline_frame_materialization_accepts_jsc_descriptor_boundary() {
        assert_eq!(
            validate_arm64_baseline_generated_native_frame_materialization(
                &context(),
                &valid_descriptor()
            ),
            Ok(())
        );
    }

    #[test]
    fn arm64_baseline_frame_materialization_rejects_current_return_seed_x1_frame_pointer() {
        let mut descriptor = valid_descriptor();
        descriptor.terminal_policy =
            Arm64BaselineFrameMaterializationTerminalPolicy::RustCAbiReturnSeed;
        assert_descriptor_mismatch(
            descriptor,
            Arm64BaselineGeneratedNativeFrameMaterializationMismatch::TerminalPolicyMismatch {
                actual: Arm64BaselineFrameMaterializationTerminalPolicy::RustCAbiReturnSeed,
            },
        );

        let mut descriptor = valid_descriptor();
        descriptor.prologue.frame_pointer_source =
            Arm64BaselineFramePointerSource::RustCAbiX1Carrier;
        assert_descriptor_mismatch(
            descriptor,
            Arm64BaselineGeneratedNativeFrameMaterializationMismatch::FramePointerSourceMismatch {
                actual: Arm64BaselineFramePointerSource::RustCAbiX1Carrier,
            },
        );
    }

    #[test]
    fn arm64_baseline_frame_materialization_rejects_boxed_storage_address_source() {
        let mut descriptor = valid_descriptor();
        descriptor.call_frame_address_source =
            Arm64BaselineFrameAddressSource::JscCallFrameStorageBox;
        assert_descriptor_mismatch(
            descriptor,
            Arm64BaselineGeneratedNativeFrameMaterializationMismatch::CallFrameAddressSourceMismatch {
                actual: Arm64BaselineFrameAddressSource::JscCallFrameStorageBox,
            },
        );
    }

    #[test]
    fn arm64_baseline_frame_materialization_rejects_wrong_fp_or_sp_relations() {
        let mut wrong_fp = valid_descriptor();
        wrong_fp.prologue.post_prologue_fp = 0x1010;
        assert_descriptor_mismatch(
            wrong_fp,
            Arm64BaselineGeneratedNativeFrameMaterializationMismatch::PublishedTopFrameMismatch {
                frame_pointer: 0x1010,
                published_top_frame: 0x1000,
            },
        );

        let mut wrong_entry_sp = valid_descriptor();
        wrong_entry_sp.prologue.entry_sp = 0x1020;
        assert_descriptor_mismatch(
            wrong_entry_sp,
            Arm64BaselineGeneratedNativeFrameMaterializationMismatch::EntryStackPointerMismatch {
                expected: 0x1010,
                actual: 0x1020,
            },
        );

        let mut wrong_post_alloc = valid_descriptor();
        wrong_post_alloc.post_frame_allocation.post_allocation_sp = 0x0ff0;
        assert_descriptor_mismatch(
            wrong_post_alloc,
            Arm64BaselineGeneratedNativeFrameMaterializationMismatch::PostFrameAllocationStackPointerMismatch {
                expected: 0x0fe0,
                actual: 0x0ff0,
            },
        );

        let mut unaligned_post_alloc = valid_descriptor();
        unaligned_post_alloc
            .post_frame_allocation
            .frame_top_offset_bytes = -0x18;
        unaligned_post_alloc
            .post_frame_allocation
            .post_allocation_sp = 0x0fe8;
        assert_descriptor_mismatch(
            unaligned_post_alloc,
            Arm64BaselineGeneratedNativeFrameMaterializationMismatch::PostFrameAllocationStackPointerUnaligned {
                actual: 0x0fe8,
            },
        );
    }

    #[test]
    fn arm64_baseline_frame_materialization_rejects_wrong_header_offsets() {
        let mut descriptor = valid_descriptor();
        descriptor.header.arguments.pop();
        assert_descriptor_mismatch(
            descriptor,
            Arm64BaselineGeneratedNativeFrameMaterializationMismatch::ArgumentSlotCountMismatch {
                expected: 2,
                actual: 1,
            },
        );

        let mut descriptor = valid_descriptor();
        descriptor.header.return_pc_offset = 0x10;
        assert_descriptor_mismatch(
            descriptor,
            Arm64BaselineGeneratedNativeFrameMaterializationMismatch::HeaderSlotOffsetMismatch {
                slot: Arm64BaselineCallFrameHeaderSlot::ReturnPc,
                expected: 0x08,
                actual: 0x10,
            },
        );

        let mut descriptor = valid_descriptor();
        descriptor.header.arguments[1].argument_index = 0;
        descriptor.header.arguments[1].offset_from_call_frame = 0x30;
        assert_descriptor_mismatch(
            descriptor,
            Arm64BaselineGeneratedNativeFrameMaterializationMismatch::ArgumentSlotIndexMismatch {
                order: 1,
                expected: 1,
                actual: 0,
            },
        );

        let mut descriptor = valid_descriptor();
        descriptor.header.arguments[1].offset_from_call_frame = 0x60;
        assert_descriptor_mismatch(
            descriptor,
            Arm64BaselineGeneratedNativeFrameMaterializationMismatch::ArgumentSlotOffsetMismatch {
                argument_index: 1,
                expected: 0x38,
                actual: 0x60,
            },
        );

        let mut descriptor = valid_descriptor();
        descriptor.header.live_locals.pop();
        assert_descriptor_mismatch(
            descriptor,
            Arm64BaselineGeneratedNativeFrameMaterializationMismatch::LiveLocalSlotCountMismatch {
                expected: 2,
                actual: 1,
            },
        );

        let mut descriptor = valid_descriptor();
        descriptor.header.live_locals[1].local_index = 0;
        descriptor.header.live_locals[1].offset_from_call_frame = -0x08;
        assert_descriptor_mismatch(
            descriptor,
            Arm64BaselineGeneratedNativeFrameMaterializationMismatch::LiveLocalSlotIndexMismatch {
                order: 1,
                expected: 1,
                actual: 0,
            },
        );

        let mut descriptor = valid_descriptor();
        descriptor.header.live_locals[0].offset_from_call_frame = -0x20;
        assert_descriptor_mismatch(
            descriptor,
            Arm64BaselineGeneratedNativeFrameMaterializationMismatch::LiveLocalSlotOffsetMismatch {
                local_index: 0,
                expected: -0x08,
                actual: -0x20,
            },
        );
    }

    #[test]
    fn arm64_baseline_frame_materialization_rejects_missing_or_metadata_only_x25_x26_x27_x28() {
        let mut missing = valid_descriptor();
        missing
            .materialized_registers
            .retain(|record| record.register != register_contract::METADATA_TABLE_REGISTER);
        assert_descriptor_mismatch(
            missing,
            Arm64BaselineGeneratedNativeFrameMaterializationMismatch::MaterializedRegisterMissing {
                register: register_contract::METADATA_TABLE_REGISTER,
            },
        );

        let mut wrong_source = valid_descriptor();
        wrong_source.materialized_registers[0].source =
            Arm64BaselineMaterializedRegisterSource::RustMetadataRecord;
        assert_descriptor_mismatch(
            wrong_source,
            Arm64BaselineGeneratedNativeFrameMaterializationMismatch::MaterializedRegisterSourceMismatch {
                register: register_contract::NUMBER_TAG_REGISTER,
                expected: Arm64BaselineMaterializedRegisterSource::AssemblyHelpersTagCheckRegisters,
                actual: Arm64BaselineMaterializedRegisterSource::RustMetadataRecord,
            },
        );
    }

    #[test]
    fn arm64_baseline_frame_materialization_rejects_root_slot_mismatch() {
        let mut descriptor = valid_descriptor();
        descriptor.live_root_slots[0].encoded_payload = 0xdead;
        assert_descriptor_mismatch(
            descriptor,
            Arm64BaselineGeneratedNativeFrameMaterializationMismatch::LiveRootSlotMismatch {
                order: 0,
                expected: root_slot(),
                actual: Arm64BaselineMachineStackRootSlotDescriptor {
                    encoded_payload: 0xdead,
                    ..root_slot()
                },
            },
        );
    }

    #[test]
    fn arm64_baseline_frame_materialization_rejects_entry_linkage_mismatch() {
        let mut descriptor = valid_descriptor();
        descriptor
            .entry_linkage
            .vm_entry_record_previous_top_entry_frame = Some(0xbeef);
        assert_descriptor_mismatch(
            descriptor,
            Arm64BaselineGeneratedNativeFrameMaterializationMismatch::EntryPreviousTopEntryFrameMismatch {
                expected: Some(0xa000),
                actual: Some(0xbeef),
            },
        );
    }
}
