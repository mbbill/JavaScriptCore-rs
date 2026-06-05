//! ARM64 baseline entry prologue proof.
//!
//! C++ JSC map: `AssemblyHelpers::emitFunctionPrologue()` tags the return
//! address, pushes `fp/lr`, and moves `sp` into `fp`; `emitFunctionEpilogue()`
//! moves `fp` back to `sp` before popping `fp/lr`. `LowLevelInterpreter64.asm`
//! `makeJavaScriptCall` positions `sp` at `CallFrame + CallerFrameAndPCSize`
//! before calling generated JavaScript, so the prologue makes `fp/x29` equal
//! the callee `CallFrame*`.
//!
//! The live Rust ARM64 return seed is still a raw C ABI helper: Rust passes the
//! register-window frame carrier in `x1`, and the seed moves `x1` into `fp`.
//! Keep that divergence explicit and byte-for-byte stable until public ARM64
//! admission switches to a real JSC machine-stack CallFrame entry path.

use super::register_contract::{self, Arm64Gpr};

pub(crate) const P6_ARM64_RAW_C_ABI_CALLABLE_PROLOGUE_BYTES: &[u8] = &[
    0xfd, 0x7b, 0xbf, 0xa9, // stp fp, lr, [sp, #-16]!
    0xfd, 0x03, 0x01, 0xaa, // mov fp, x1
];

pub(crate) const P6_ARM64_RAW_C_ABI_CALLABLE_EPILOGUE_BYTES: &[u8] = &[
    0xfd, 0x7b, 0xc1, 0xa8, // ldp fp, lr, [sp], #16
    0xc0, 0x03, 0x5f, 0xd6, // ret
];

pub(crate) const ARM64_JSC_BASELINE_GENERATED_PROLOGUE_BYTES: &[u8] = &[
    0xfd, 0x7b, 0xbf, 0xa9, // stp fp, lr, [sp, #-16]!
    0xfd, 0x03, 0x00, 0x91, // mov fp, sp
];

pub(crate) const ARM64_JSC_BASELINE_GENERATED_EPILOGUE_BYTES: &[u8] = &[
    0xbf, 0x03, 0x00, 0x91, // mov sp, fp
    0xfd, 0x7b, 0xc1, 0xa8, // ldp fp, lr, [sp], #16
    0xc0, 0x03, 0x5f, 0xd6, // ret
];

#[allow(dead_code)]
#[derive(Clone, Copy, Debug, Eq, PartialEq)]
pub(crate) enum Arm64BaselineEntryFrameKind {
    RawRustCAbiReturnSeed,
    JscBaselineGeneratedFrame,
}

#[allow(dead_code)]
#[derive(Clone, Copy, Debug, Eq, PartialEq)]
pub(crate) enum Arm64BaselineFramePointerSource {
    RustCAbiArgumentX1,
    StackPointerAfterProloguePush,
}

#[allow(dead_code)]
#[derive(Clone, Copy, Debug, Eq, PartialEq)]
pub(crate) enum Arm64BaselineStackPointerEntryPolicy {
    RustCAbiCallOwnsStack,
    CallerPositionsSpAtCallFramePlusCallerFrameAndPc,
}

#[allow(dead_code)]
#[derive(Clone, Copy, Debug, Eq, PartialEq)]
pub(crate) struct Arm64BaselineEntryFrameContract {
    pub(crate) kind: Arm64BaselineEntryFrameKind,
    pub(crate) call_frame_register: Arm64Gpr,
    pub(crate) frame_pointer_source: Arm64BaselineFramePointerSource,
    pub(crate) stack_pointer_entry_policy: Arm64BaselineStackPointerEntryPolicy,
    pub(crate) prologue_bytes: &'static [u8],
    pub(crate) epilogue_bytes: &'static [u8],
}

#[allow(dead_code)]
#[derive(Clone, Copy, Debug, Eq, PartialEq)]
pub(crate) enum Arm64BaselineEntryFrameContractMismatch {
    KindMismatch {
        actual: Arm64BaselineEntryFrameKind,
    },
    CallFrameRegisterMismatch {
        actual: Arm64Gpr,
    },
    FramePointerSourceMismatch {
        actual: Arm64BaselineFramePointerSource,
    },
    StackPointerEntryPolicyMismatch {
        actual: Arm64BaselineStackPointerEntryPolicy,
    },
    PrologueBytesMismatch,
    EpilogueBytesMismatch,
}

#[allow(dead_code)]
pub(crate) const fn raw_rust_c_abi_return_seed_entry_contract() -> Arm64BaselineEntryFrameContract {
    Arm64BaselineEntryFrameContract {
        kind: Arm64BaselineEntryFrameKind::RawRustCAbiReturnSeed,
        call_frame_register: register_contract::CALL_FRAME_REGISTER,
        frame_pointer_source: Arm64BaselineFramePointerSource::RustCAbiArgumentX1,
        stack_pointer_entry_policy: Arm64BaselineStackPointerEntryPolicy::RustCAbiCallOwnsStack,
        prologue_bytes: P6_ARM64_RAW_C_ABI_CALLABLE_PROLOGUE_BYTES,
        epilogue_bytes: P6_ARM64_RAW_C_ABI_CALLABLE_EPILOGUE_BYTES,
    }
}

#[allow(dead_code)]
pub(crate) const fn jsc_baseline_generated_entry_contract() -> Arm64BaselineEntryFrameContract {
    Arm64BaselineEntryFrameContract {
        kind: Arm64BaselineEntryFrameKind::JscBaselineGeneratedFrame,
        call_frame_register: register_contract::CALL_FRAME_REGISTER,
        frame_pointer_source: Arm64BaselineFramePointerSource::StackPointerAfterProloguePush,
        stack_pointer_entry_policy:
            Arm64BaselineStackPointerEntryPolicy::CallerPositionsSpAtCallFramePlusCallerFrameAndPc,
        prologue_bytes: ARM64_JSC_BASELINE_GENERATED_PROLOGUE_BYTES,
        epilogue_bytes: ARM64_JSC_BASELINE_GENERATED_EPILOGUE_BYTES,
    }
}

#[allow(dead_code)]
pub(crate) fn validate_jsc_baseline_generated_entry_contract(
    contract: Arm64BaselineEntryFrameContract,
) -> Result<(), Arm64BaselineEntryFrameContractMismatch> {
    if contract.kind != Arm64BaselineEntryFrameKind::JscBaselineGeneratedFrame {
        return Err(Arm64BaselineEntryFrameContractMismatch::KindMismatch {
            actual: contract.kind,
        });
    }
    if contract.call_frame_register != register_contract::CALL_FRAME_REGISTER {
        return Err(
            Arm64BaselineEntryFrameContractMismatch::CallFrameRegisterMismatch {
                actual: contract.call_frame_register,
            },
        );
    }
    if contract.frame_pointer_source
        != Arm64BaselineFramePointerSource::StackPointerAfterProloguePush
    {
        return Err(
            Arm64BaselineEntryFrameContractMismatch::FramePointerSourceMismatch {
                actual: contract.frame_pointer_source,
            },
        );
    }
    if contract.stack_pointer_entry_policy
        != Arm64BaselineStackPointerEntryPolicy::CallerPositionsSpAtCallFramePlusCallerFrameAndPc
    {
        return Err(
            Arm64BaselineEntryFrameContractMismatch::StackPointerEntryPolicyMismatch {
                actual: contract.stack_pointer_entry_policy,
            },
        );
    }
    if contract.prologue_bytes != ARM64_JSC_BASELINE_GENERATED_PROLOGUE_BYTES {
        return Err(Arm64BaselineEntryFrameContractMismatch::PrologueBytesMismatch);
    }
    if contract.epilogue_bytes != ARM64_JSC_BASELINE_GENERATED_EPILOGUE_BYTES {
        return Err(Arm64BaselineEntryFrameContractMismatch::EpilogueBytesMismatch);
    }
    Ok(())
}

#[cfg(test)]
mod tests {
    use super::*;

    fn word(bytes: &[u8], index: usize) -> u32 {
        let start = index * 4;
        u32::from_le_bytes([
            bytes[start],
            bytes[start + 1],
            bytes[start + 2],
            bytes[start + 3],
        ])
    }

    #[test]
    fn arm64_entry_prologue_keeps_raw_c_abi_seed_explicit() {
        let contract = raw_rust_c_abi_return_seed_entry_contract();

        assert_eq!(
            contract.kind,
            Arm64BaselineEntryFrameKind::RawRustCAbiReturnSeed
        );
        assert_eq!(
            contract.call_frame_register,
            register_contract::CALL_FRAME_REGISTER
        );
        assert_eq!(
            contract.frame_pointer_source,
            Arm64BaselineFramePointerSource::RustCAbiArgumentX1
        );
        assert_eq!(
            contract.stack_pointer_entry_policy,
            Arm64BaselineStackPointerEntryPolicy::RustCAbiCallOwnsStack
        );
        assert_eq!(contract.prologue_bytes.len(), 8);
        assert_eq!(word(contract.prologue_bytes, 0), 0xa9bf_7bfd);
        assert_eq!(word(contract.prologue_bytes, 1), 0xaa01_03fd);
        assert_eq!(word(contract.epilogue_bytes, 0), 0xa8c1_7bfd);
        assert_eq!(word(contract.epilogue_bytes, 1), 0xd65f_03c0);
    }

    #[test]
    fn arm64_entry_prologue_matches_jsc_generated_frame_shape() {
        let contract = jsc_baseline_generated_entry_contract();

        assert_eq!(
            contract.kind,
            Arm64BaselineEntryFrameKind::JscBaselineGeneratedFrame
        );
        assert_eq!(
            contract.call_frame_register,
            register_contract::CALL_FRAME_REGISTER
        );
        assert_eq!(
            contract.frame_pointer_source,
            Arm64BaselineFramePointerSource::StackPointerAfterProloguePush
        );
        assert_eq!(
            contract.stack_pointer_entry_policy,
            Arm64BaselineStackPointerEntryPolicy::CallerPositionsSpAtCallFramePlusCallerFrameAndPc
        );
        assert_eq!(contract.prologue_bytes.len(), 8);
        assert_eq!(word(contract.prologue_bytes, 0), 0xa9bf_7bfd);
        assert_eq!(word(contract.prologue_bytes, 1), 0x9100_03fd);
        assert_eq!(word(contract.epilogue_bytes, 0), 0x9100_03bf);
        assert_eq!(word(contract.epilogue_bytes, 1), 0xa8c1_7bfd);
        assert_eq!(word(contract.epilogue_bytes, 2), 0xd65f_03c0);
        assert_eq!(
            validate_jsc_baseline_generated_entry_contract(contract),
            Ok(())
        );
    }

    #[test]
    fn arm64_entry_prologue_rejects_raw_seed_for_jsc_public_admission() {
        let raw = raw_rust_c_abi_return_seed_entry_contract();

        assert_eq!(
            validate_jsc_baseline_generated_entry_contract(raw),
            Err(Arm64BaselineEntryFrameContractMismatch::KindMismatch {
                actual: Arm64BaselineEntryFrameKind::RawRustCAbiReturnSeed,
            })
        );

        let mut wrong_source = jsc_baseline_generated_entry_contract();
        wrong_source.frame_pointer_source = Arm64BaselineFramePointerSource::RustCAbiArgumentX1;
        assert_eq!(
            validate_jsc_baseline_generated_entry_contract(wrong_source),
            Err(
                Arm64BaselineEntryFrameContractMismatch::FramePointerSourceMismatch {
                    actual: Arm64BaselineFramePointerSource::RustCAbiArgumentX1,
                }
            )
        );
    }
}
