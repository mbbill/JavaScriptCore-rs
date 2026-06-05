use super::tests;
use super::*;
use crate::jit::arm64_baseline::{JSC_REGISTER_BYTES, JSC_STACK_ALIGNMENT_BYTES};

fn attach_materialization_descriptor_to_fixture(fixture: &mut tests::NativeFrameResidencyFixture) {
    let call_frame = fixture
        .top_call_frame_publication
        .publication
        .published_top_frame
        .0;
    let expected_live_local_slots = 1;
    let required_live_local_bytes = expected_live_local_slots * JSC_REGISTER_BYTES;
    let aligned_frame_bytes = required_live_local_bytes.next_multiple_of(JSC_STACK_ALIGNMENT_BYTES);
    let aligned_post_allocation_sp = call_frame - aligned_frame_bytes;
    let frame_top_offset_bytes =
        isize::try_from(aligned_post_allocation_sp).unwrap() - isize::try_from(call_frame).unwrap();
    fixture.native_frame_residency_proof = fixture
        .native_frame_residency_proof
        .clone()
        .with_generated_native_frame_materialization(
            &fixture.top_call_frame_publication,
            frame_top_offset_bytes,
            u32::try_from(expected_live_local_slots).unwrap(),
        )
        .expect("fixture should attach generated native frame materialization");
}

#[test]
fn public_arm64_branch_aware_admission_keeps_valid_materialized_frame_descriptor_blocked() {
    let code_block = tests::jump_if_false_code_block(4);
    let site = tests::jump_if_false_site();
    let side_exits = [tests::branch_aware_side_exit_proof(&code_block, &site)];
    let mut request = tests::valid_request(&side_exits);
    let mut fixture = tests::native_frame_residency_fixture();
    attach_materialization_descriptor_to_fixture(&mut fixture);
    request.fallback_rooting_proof = fixture.fallback();

    assert_eq!(
        p6_arm64_public_branch_aware_callable_admission_proof(&request),
        Err(
            P6Arm64BranchAwareCallableAdmissionRejection::Arm64GeneratedNativeFrameMaterializationProofAcceptedButPublicAdmissionBlocked
        )
    );
}
