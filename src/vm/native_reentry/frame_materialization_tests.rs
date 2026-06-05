use super::tests;
use super::*;
use crate::jit::arm64_baseline::register_contract::{
    self, Arm64BaselineMaterializedState as State,
};
use crate::jit::arm64_baseline::{
    Arm64BaselineCallFrameHeaderDescriptor, Arm64BaselineEntryLinkageDescriptor,
    Arm64BaselineFrameAddressSource, Arm64BaselineFrameMaterializationTerminalPolicy,
    Arm64BaselineFramePointerSource, Arm64BaselineGeneratedNativeFrameMaterializationDescriptor,
    Arm64BaselineJscPrologueDescriptor, Arm64BaselineMachineStackRootSlotDescriptor,
    Arm64BaselineMaterializedRegisterDescriptor, Arm64BaselineMaterializedRegisterSource,
    Arm64BaselinePostFrameAllocationDescriptor,
};

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

fn materialization_descriptor_from_fixture(
    fixture: &tests::NativeFrameResidencyFixture,
) -> Arm64BaselineGeneratedNativeFrameMaterializationDescriptor {
    let call_frame = fixture
        .top_call_frame_publication
        .publication
        .published_top_frame
        .0;
    let aligned_post_allocation_sp = call_frame & !0xf;
    let frame_top_offset_bytes =
        isize::try_from(aligned_post_allocation_sp).unwrap() - isize::try_from(call_frame).unwrap();
    let root_slot = fixture.native_frame_residency_proof.slot_records[0];
    let root_span = fixture
        .native_frame_residency_proof
        .machine_stack_spans
        .iter()
        .find(|record| {
            let end = root_slot
                .slot_address
                .saturating_add(core::mem::size_of::<usize>());
            record.kind == root_slot.containing_span
                && root_slot.slot_address >= record.span.begin
                && end <= record.span.end
        })
        .expect("root slot span")
        .span;

    Arm64BaselineGeneratedNativeFrameMaterializationDescriptor {
        terminal_policy: Arm64BaselineFrameMaterializationTerminalPolicy::JscBaselineGeneratedFrame,
        call_frame_address_source: Arm64BaselineFrameAddressSource::NativeMachineStack,
        prologue: Arm64BaselineJscPrologueDescriptor {
            call_frame,
            entry_sp: call_frame + 16,
            post_push_sp: call_frame,
            post_prologue_sp: call_frame,
            post_prologue_fp: call_frame,
            frame_pointer_source:
                Arm64BaselineFramePointerSource::AssemblyHelpersPrologueStackPointer,
        },
        post_frame_allocation: Arm64BaselinePostFrameAllocationDescriptor {
            frame_top_offset_bytes,
            post_allocation_sp: aligned_post_allocation_sp,
        },
        header: Arm64BaselineCallFrameHeaderDescriptor::jsc(0, 1),
        materialized_registers: materialized_registers(),
        live_root_slots: vec![Arm64BaselineMachineStackRootSlotDescriptor {
            kind: root_slot.kind.into(),
            slot_address: root_slot.slot_address,
            encoded_payload: root_slot.encoded_payload,
            expected_root: root_slot.expected_root,
            containing_span: root_slot.containing_span.into(),
            span: root_span,
        }],
        entry_linkage: Arm64BaselineEntryLinkageDescriptor {
            vm_entry_record_previous_top_call_frame: fixture
                .top_call_frame_publication
                .publication
                .vm_entry_previous_top_call_frame
                .map(|frame| frame.0),
            vm_entry_record_previous_top_entry_frame: fixture
                .top_call_frame_publication
                .publication
                .vm_entry_previous_top_entry_frame
                .map(|frame| frame.0),
            published_top_call_frame: call_frame,
            published_top_entry_frame: fixture
                .top_call_frame_publication
                .publication
                .current_entry_frame
                .0,
        },
    }
}

#[test]
fn public_arm64_branch_aware_admission_keeps_valid_materialized_frame_descriptor_blocked() {
    let code_block = tests::jump_if_false_code_block(4);
    let site = tests::jump_if_false_site();
    let side_exits = [tests::branch_aware_side_exit_proof(&code_block, &site)];
    let mut request = tests::valid_request(&side_exits);
    let mut fixture = tests::native_frame_residency_fixture();
    fixture
        .native_frame_residency_proof
        .generated_native_frame_materialization =
        Some(materialization_descriptor_from_fixture(&fixture));
    request.fallback_rooting_proof = fixture.fallback();

    assert_eq!(
        p6_arm64_public_branch_aware_callable_admission_proof(&request),
        Err(
            P6Arm64BranchAwareCallableAdmissionRejection::Arm64GeneratedNativeFrameMaterializationProofAcceptedButPublicAdmissionBlocked
        )
    );
}
