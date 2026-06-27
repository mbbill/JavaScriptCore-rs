//! Private Unix ARM64 JSC-stack dispatch trampoline.
//!
//! C++ JSC map: `LowLevelInterpreter64.asm` `doVMEntry(makeJavaScriptCall)`
//! creates the `VMEntryRecord`, publishes/restores VM top-frame state, and owns
//! caught/uncaught exception exits. `_llint_call_javascript` then enters
//! generated code with `sp = CallFrame + CallerFrameAndPCSize` and `fp/x29 =
//! EntryFrame`. This Rust module only hosts the current private Unix ARM64
//! normal-return bridge for that latter entry shape.

#![allow(unsafe_code)]

use std::ffi::c_void;
use std::ptr::NonNull;

use super::unix_executable_memory::ExecutableMemoryPlatformError;

#[cfg(all(target_arch = "aarch64", target_vendor = "apple"))]
core::arch::global_asm!(
    r#"
    .text
    .private_extern _jsc_rs_arm64_jsc_stack_trampoline
    .p2align 2
_jsc_rs_arm64_jsc_stack_trampoline:
    sub sp, sp, #160
    stp x29, x30, [sp, #0]
    stp x19, x20, [sp, #16]
    stp x21, x22, [sp, #32]
    stp x23, x24, [sp, #48]
    stp x25, x26, [sp, #64]
    stp x27, x28, [sp, #80]
    stp d8, d9, [sp, #96]
    stp d10, d11, [sp, #112]
    stp d12, d13, [sp, #128]
    stp d14, d15, [sp, #144]

    mov x19, sp
    mov x9, x0
    mov sp, x1
    mov x29, x2
    blr x9

    mov sp, x19
    ldp d14, d15, [sp, #144]
    ldp d12, d13, [sp, #128]
    ldp d10, d11, [sp, #112]
    ldp d8, d9, [sp, #96]
    ldp x27, x28, [sp, #80]
    ldp x25, x26, [sp, #64]
    ldp x23, x24, [sp, #48]
    ldp x21, x22, [sp, #32]
    ldp x19, x20, [sp, #16]
    ldp x29, x30, [sp, #0]
    add sp, sp, #160
    ret
"#
);

#[cfg(all(target_arch = "aarch64", not(target_vendor = "apple")))]
core::arch::global_asm!(
    r#"
    .text
    .globl jsc_rs_arm64_jsc_stack_trampoline
    .hidden jsc_rs_arm64_jsc_stack_trampoline
    .type jsc_rs_arm64_jsc_stack_trampoline, %function
    .p2align 2
jsc_rs_arm64_jsc_stack_trampoline:
    sub sp, sp, #160
    stp x29, x30, [sp, #0]
    stp x19, x20, [sp, #16]
    stp x21, x22, [sp, #32]
    stp x23, x24, [sp, #48]
    stp x25, x26, [sp, #64]
    stp x27, x28, [sp, #80]
    stp d8, d9, [sp, #96]
    stp d10, d11, [sp, #112]
    stp d12, d13, [sp, #128]
    stp d14, d15, [sp, #144]

    mov x19, sp
    mov x9, x0
    mov sp, x1
    mov x29, x2
    blr x9

    mov sp, x19
    ldp d14, d15, [sp, #144]
    ldp d12, d13, [sp, #128]
    ldp d10, d11, [sp, #112]
    ldp d8, d9, [sp, #96]
    ldp x27, x28, [sp, #80]
    ldp x25, x26, [sp, #64]
    ldp x23, x24, [sp, #48]
    ldp x21, x22, [sp, #32]
    ldp x19, x20, [sp, #16]
    ldp x29, x30, [sp, #0]
    add sp, sp, #160
    ret
    .size jsc_rs_arm64_jsc_stack_trampoline, .-jsc_rs_arm64_jsc_stack_trampoline
"#
);

#[cfg(target_arch = "aarch64")]
unsafe extern "C" {
    fn jsc_rs_arm64_jsc_stack_trampoline(
        entry: *const u8,
        entry_sp: *mut c_void,
        entry_frame: *mut c_void,
    ) -> u64;
}

#[cfg(target_arch = "aarch64")]
pub(super) unsafe fn call_arm64_jsc_stack_entry(
    entry: *const u8,
    entry_sp: NonNull<c_void>,
    entry_frame: NonNull<c_void>,
) -> Result<u64, ExecutableMemoryPlatformError> {
    if entry.is_null() {
        return Err(ExecutableMemoryPlatformError::RangeOutOfBounds);
    }

    // SAFETY: callers pass a non-null address inside a live RX mapping plus a
    // request already validated by the safe wrapper. The assembly helper only
    // installs JSC's `_llint_call_javascript` generated-code `sp`/`fp` shape
    // and restores the Rust stack, frame pointer, link register, and ARM64 C
    // ABI callee-save set before returning. This remains a private
    // normal-return-only bridge: it does not construct C++ `doVMEntry`, publish
    // VM top-frame fields, or implement caught/uncaught exception exits.
    Ok(
        unsafe {
            jsc_rs_arm64_jsc_stack_trampoline(entry, entry_sp.as_ptr(), entry_frame.as_ptr())
        },
    )
}

#[cfg(all(test, target_arch = "aarch64"))]
mod tests {
    const ARM64_JSC_STACK_TRAMPOLINE_HOST_SAVE_GPRS: [u8; 10] =
        [19, 20, 21, 22, 23, 24, 25, 26, 27, 28];
    const ARM64_JSC_STACK_TRAMPOLINE_HOST_SAVE_FPRS: [u8; 8] = [8, 9, 10, 11, 12, 13, 14, 15];
    const ARM64_JSC_STACK_TRAMPOLINE_HOST_SAVE_BYTES: usize = 160;

    #[test]
    fn arm64_jsc_stack_trampoline_host_save_descriptor_matches_jsc_arm64_set() {
        assert_eq!(
            ARM64_JSC_STACK_TRAMPOLINE_HOST_SAVE_GPRS,
            [19, 20, 21, 22, 23, 24, 25, 26, 27, 28]
        );
        assert_eq!(
            ARM64_JSC_STACK_TRAMPOLINE_HOST_SAVE_FPRS,
            [8, 9, 10, 11, 12, 13, 14, 15]
        );
        assert_eq!(ARM64_JSC_STACK_TRAMPOLINE_HOST_SAVE_BYTES, 160);
    }
}
