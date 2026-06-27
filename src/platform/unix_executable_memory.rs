//! Private Unix backend for executable-memory mappings.
//!
//! This module is the only platform executable-memory code that uses raw
//! pointers or FFI. The public compartment module wraps these operations in a
//! safe W^X state machine and never exposes the mapped address.

#![allow(unsafe_code)]

use std::ffi::{c_int, c_void};
use std::fmt;
use std::ptr::NonNull;

#[cfg(target_arch = "aarch64")]
use super::executable_memory_compartment::ExecutableMemoryArm64JscStackCallRequest;
use super::executable_memory_compartment::ExecutableMemoryPlatformOperation;

const PROT_READ: c_int = 0x1;
const PROT_WRITE: c_int = 0x2;
const PROT_EXEC: c_int = 0x4;
const MAP_PRIVATE: c_int = 0x02;

#[cfg(any(target_os = "linux", target_os = "android"))]
const MAP_ANON: c_int = 0x20;

#[cfg(any(
    target_os = "dragonfly",
    target_os = "freebsd",
    target_os = "ios",
    target_os = "macos",
    target_os = "netbsd",
    target_os = "openbsd",
    target_os = "tvos",
    target_os = "watchos"
))]
const MAP_ANON: c_int = 0x1000;

type OffT = i64;

unsafe extern "C" {
    fn getpagesize() -> c_int;
    fn mmap(
        addr: *mut c_void,
        length: usize,
        prot: c_int,
        flags: c_int,
        fd: c_int,
        offset: OffT,
    ) -> *mut c_void;
    fn mprotect(addr: *mut c_void, len: usize, prot: c_int) -> c_int;
    fn munmap(addr: *mut c_void, len: usize) -> c_int;
}

#[derive(Clone, Copy, Debug, Eq, PartialEq)]
pub(super) enum ExecutableMemoryPlatformError {
    InvalidPageSize {
        page_size: i64,
    },
    SystemCall {
        operation: ExecutableMemoryPlatformOperation,
        errno: Option<i32>,
    },
    RangeOutOfBounds,
    AlreadyReleased,
}

pub(super) struct ExecutableMemoryMapping {
    ptr: Option<NonNull<u8>>,
    byte_len: usize,
}

impl fmt::Debug for ExecutableMemoryMapping {
    fn fmt(&self, formatter: &mut fmt::Formatter<'_>) -> fmt::Result {
        formatter
            .debug_struct("ExecutableMemoryMapping")
            .field("byte_len", &self.byte_len)
            .field("mapped", &self.ptr.is_some())
            .finish()
    }
}

impl ExecutableMemoryMapping {
    pub(super) fn allocate_writable(
        byte_len: usize,
    ) -> Result<Self, ExecutableMemoryPlatformError> {
        if byte_len == 0 {
            return Err(ExecutableMemoryPlatformError::RangeOutOfBounds);
        }

        // SAFETY: `mmap` is called with a null preferred address, a non-zero
        // page-rounded length supplied by the safe wrapper, RW permissions, an
        // anonymous private mapping, and the required fd/offset pair. The
        // returned address is checked against MAP_FAILED and null before it is
        // stored in the private mapping owner.
        let ptr = unsafe {
            mmap(
                std::ptr::null_mut(),
                byte_len,
                PROT_READ | PROT_WRITE,
                MAP_PRIVATE | MAP_ANON,
                -1,
                0,
            )
        };
        if ptr == map_failed() || ptr.is_null() {
            return Err(ExecutableMemoryPlatformError::SystemCall {
                operation: ExecutableMemoryPlatformOperation::AllocateWritable,
                errno: last_errno(),
            });
        }

        Ok(Self {
            ptr: NonNull::new(ptr.cast::<u8>()),
            byte_len,
        })
    }

    pub(super) fn copy_from_slice(
        &self,
        offset: usize,
        bytes: &[u8],
    ) -> Result<(), ExecutableMemoryPlatformError> {
        let ptr = self
            .ptr
            .ok_or(ExecutableMemoryPlatformError::AlreadyReleased)?;
        let end = offset
            .checked_add(bytes.len())
            .ok_or(ExecutableMemoryPlatformError::RangeOutOfBounds)?;
        if end > self.byte_len {
            return Err(ExecutableMemoryPlatformError::RangeOutOfBounds);
        }

        // SAFETY: the safe wrapper validates the destination range and the
        // defensive check above ensures `[offset, offset + bytes.len())` lies
        // inside this live mapping. Source and destination cannot overlap
        // because the destination is private mmap-owned executable storage.
        unsafe {
            std::ptr::copy_nonoverlapping(bytes.as_ptr(), ptr.as_ptr().add(offset), bytes.len());
        }
        Ok(())
    }

    pub(super) fn protect_executable(&self) -> Result<(), ExecutableMemoryPlatformError> {
        let ptr = self
            .ptr
            .ok_or(ExecutableMemoryPlatformError::AlreadyReleased)?;

        // SAFETY: `ptr` and `byte_len` describe the live mapping created by
        // `mmap` and still owned by this object. The safe wrapper prevents
        // writes after this RX transition.
        let result = unsafe {
            mprotect(
                ptr.as_ptr().cast::<c_void>(),
                self.byte_len,
                PROT_READ | PROT_EXEC,
            )
        };
        if result != 0 {
            return Err(ExecutableMemoryPlatformError::SystemCall {
                operation: ExecutableMemoryPlatformOperation::ProtectExecutable,
                errno: last_errno(),
            });
        }
        Ok(())
    }

    #[cfg(target_arch = "x86_64")]
    pub(super) fn call_p6_x86_64_entry(
        &self,
        entry_offset: usize,
        vm: NonNull<c_void>,
        frame_base: NonNull<c_void>,
        callee_value_bits: u64,
        ic_store_base: NonNull<c_void>,
    ) -> Result<u64, ExecutableMemoryPlatformError> {
        let ptr = self
            .ptr
            .ok_or(ExecutableMemoryPlatformError::AlreadyReleased)?;
        let entry_end = entry_offset
            .checked_add(1)
            .ok_or(ExecutableMemoryPlatformError::RangeOutOfBounds)?;
        if entry_end > self.byte_len {
            return Err(ExecutableMemoryPlatformError::RangeOutOfBounds);
        }

        // SAFETY: the safe compartment wrapper requires RX protection and a
        // linked executable lifecycle before reaching this backend. The checked
        // offset above ensures the private entry address lies inside the live
        // mapping. The P6 byte contract defines this entry as
        // `extern "C" fn(*mut c_void, *mut c_void, u64, *mut c_void) -> u64`,
        // where the 4th argument is the baseline data-IC record store base.
        unsafe {
            call_p6_x86_64_entry(
                ptr.as_ptr().add(entry_offset).cast_const(),
                vm,
                frame_base,
                callee_value_bits,
                ic_store_base,
            )
        }
    }

    #[cfg(target_arch = "aarch64")]
    pub(super) fn call_p6_arm64_entry(
        &self,
        entry_offset: usize,
        vm: NonNull<c_void>,
        frame_base: NonNull<c_void>,
        callee_value_bits: u64,
        ic_store_base: NonNull<c_void>,
    ) -> Result<u64, ExecutableMemoryPlatformError> {
        let ptr = self
            .ptr
            .ok_or(ExecutableMemoryPlatformError::AlreadyReleased)?;
        let entry_end = entry_offset
            .checked_add(1)
            .ok_or(ExecutableMemoryPlatformError::RangeOutOfBounds)?;
        if entry_end > self.byte_len {
            return Err(ExecutableMemoryPlatformError::RangeOutOfBounds);
        }

        // SAFETY: the safe compartment wrapper requires RX protection and a
        // linked executable lifecycle before reaching this backend. The checked
        // offset above ensures the private entry address lies inside the live
        // mapping. The ARM64 seed uses the same C shape as x86_64:
        // `extern "C" fn(*mut c_void, *mut c_void, u64, *mut c_void) -> u64`.
        unsafe {
            call_p6_arm64_entry(
                ptr.as_ptr().add(entry_offset).cast_const(),
                vm,
                frame_base,
                callee_value_bits,
                ic_store_base,
            )
        }
    }

    #[cfg(target_arch = "aarch64")]
    pub(super) fn call_arm64_jsc_stack_entry(
        &self,
        entry_offset: usize,
        request: ExecutableMemoryArm64JscStackCallRequest,
    ) -> Result<u64, ExecutableMemoryPlatformError> {
        let ptr = self
            .ptr
            .ok_or(ExecutableMemoryPlatformError::AlreadyReleased)?;
        let entry_end = entry_offset
            .checked_add(1)
            .ok_or(ExecutableMemoryPlatformError::RangeOutOfBounds)?;
        if entry_end > self.byte_len {
            return Err(ExecutableMemoryPlatformError::RangeOutOfBounds);
        }

        // SAFETY: the safe compartment wrapper requires RX protection, linked
        // lifecycle, checked entry range, and a validated JSC-stack request
        // before reaching this backend. The trampoline is normal-return-only:
        // it gives generated code JSC's ARM64 `_llint_call_javascript` entry
        // shape (`sp = CallFrame + CallerFrameAndPCSize`, `fp = EntryFrame`,
        // and `lr` as the trampoline return label), then restores the Rust C
        // ABI state before returning. C++ doVMEntry has no extra ARM64 host
        // save area (`pushCalleeSaves` count is zero); the VMEntryRecord
        // callee-save buffer remains JSC VM/JIT metadata for unwind/exception
        // paths, not a public admission or rooting guarantee here.
        unsafe {
            super::unix_arm64_jsc_stack_dispatch::call_arm64_jsc_stack_entry(
                ptr.as_ptr().add(entry_offset).cast_const(),
                request.entry_sp,
                request.entry_frame,
            )
        }
    }

    #[cfg(target_arch = "x86_64")]
    pub(super) fn call_p9_x86_64_owner_post_call_reentry(
        &self,
        entry_offset: usize,
        vm: NonNull<c_void>,
        frame_base: NonNull<c_void>,
        result_bits: u64,
        metadata_table_base: NonNull<c_void>,
        callee_value_bits: u64,
    ) -> Result<u64, ExecutableMemoryPlatformError> {
        let ptr = self
            .ptr
            .ok_or(ExecutableMemoryPlatformError::AlreadyReleased)?;
        let entry_end = entry_offset
            .checked_add(1)
            .ok_or(ExecutableMemoryPlatformError::RangeOutOfBounds)?;
        if entry_end > self.byte_len {
            return Err(ExecutableMemoryPlatformError::RangeOutOfBounds);
        }

        // SAFETY: the safe compartment wrapper performs the same RX lifecycle
        // and range checks as the ordinary P6 callable entry. This reentry ABI
        // extends the C shape with raw JSValue bits plus a metadata-table base.
        unsafe {
            call_p9_x86_64_owner_post_call_reentry(
                ptr.as_ptr().add(entry_offset).cast_const(),
                vm,
                frame_base,
                result_bits,
                metadata_table_base,
                callee_value_bits,
            )
        }
    }

    pub(super) fn release(&mut self) -> Result<(), ExecutableMemoryPlatformError> {
        let ptr = self
            .ptr
            .ok_or(ExecutableMemoryPlatformError::AlreadyReleased)?;

        // SAFETY: `ptr` and `byte_len` still describe the live mapping owned by
        // this object. The pointer is cleared only after a successful unmap so
        // Drop cannot unmap it twice.
        let result = unsafe { munmap(ptr.as_ptr().cast::<c_void>(), self.byte_len) };
        if result != 0 {
            return Err(ExecutableMemoryPlatformError::SystemCall {
                operation: ExecutableMemoryPlatformOperation::Release,
                errno: last_errno(),
            });
        }
        self.ptr = None;
        Ok(())
    }

    #[cfg(test)]
    pub(super) fn bytes_for_testing(
        &self,
        offset: usize,
        byte_len: usize,
    ) -> Result<Vec<u8>, ExecutableMemoryPlatformError> {
        let ptr = self
            .ptr
            .ok_or(ExecutableMemoryPlatformError::AlreadyReleased)?;
        let end = offset
            .checked_add(byte_len)
            .ok_or(ExecutableMemoryPlatformError::RangeOutOfBounds)?;
        if end > self.byte_len {
            return Err(ExecutableMemoryPlatformError::RangeOutOfBounds);
        }

        // SAFETY: the range has been checked against the live mapping and this
        // method copies into an owned Vec without exposing the mapped address.
        let bytes = unsafe { std::slice::from_raw_parts(ptr.as_ptr().add(offset), byte_len) };
        Ok(bytes.to_vec())
    }
}

impl Drop for ExecutableMemoryMapping {
    fn drop(&mut self) {
        if let Some(ptr) = self.ptr {
            // SAFETY: Drop is the last-resort owner cleanup for a mapping that
            // has not been explicitly released. Errors cannot be reported here,
            // but the pointer is cleared to avoid any accidental reuse.
            let _ = unsafe { munmap(ptr.as_ptr().cast::<c_void>(), self.byte_len) };
            self.ptr = None;
        }
    }
}

pub(super) fn page_size() -> Result<u32, ExecutableMemoryPlatformError> {
    // SAFETY: `getpagesize` has no arguments and returns the process page size.
    let page_size = unsafe { getpagesize() };
    if page_size <= 0 {
        return Err(ExecutableMemoryPlatformError::InvalidPageSize {
            page_size: i64::from(page_size),
        });
    }
    let page_size = page_size as u32;
    if !page_size.is_power_of_two() {
        return Err(ExecutableMemoryPlatformError::InvalidPageSize {
            page_size: i64::from(page_size),
        });
    }
    Ok(page_size)
}

#[cfg(target_arch = "x86_64")]
unsafe fn call_p6_x86_64_entry(
    entry: *const u8,
    vm: NonNull<c_void>,
    frame_base: NonNull<c_void>,
    callee_value_bits: u64,
    ic_store_base: NonNull<c_void>,
) -> Result<u64, ExecutableMemoryPlatformError> {
    // The 4th C-ABI argument (rcx) is the baseline data-IC record store base,
    // which the P6 prologue seeds into r13 (GPRInfo::jitDataRegister). Mirrors
    // how P9 reentry added a 4th metadata-table-base argument.
    type P6Entry = unsafe extern "C" fn(*mut c_void, *mut c_void, u64, *mut c_void) -> u64;

    if entry.is_null() {
        return Err(ExecutableMemoryPlatformError::RangeOutOfBounds);
    }

    // SAFETY: callers pass a non-null address inside a live RX mapping and the
    // P6 x86_64 contract says the bytes at this address implement the C ABI
    // entry shape represented by `P6Entry`.
    let entry: P6Entry = unsafe { std::mem::transmute(entry) };
    // SAFETY: the function pointer was just formed from the checked RX entry
    // address above, and the opaque arguments are non-null values supplied by
    // the VM-owned call boundary. `ic_store_base` is the IC record store base
    // (or a dangling pointer when there are zero IC sites, which the entry never
    // dereferences); generated code only seeds it into the callee-saved r13.
    Ok(unsafe {
        entry(
            vm.as_ptr(),
            frame_base.as_ptr(),
            callee_value_bits,
            ic_store_base.as_ptr(),
        )
    })
}

#[cfg(target_arch = "aarch64")]
unsafe fn call_p6_arm64_entry(
    entry: *const u8,
    vm: NonNull<c_void>,
    frame_base: NonNull<c_void>,
    callee_value_bits: u64,
    ic_store_base: NonNull<c_void>,
) -> Result<u64, ExecutableMemoryPlatformError> {
    type P6Entry = unsafe extern "C" fn(*mut c_void, *mut c_void, u64, *mut c_void) -> u64;

    if entry.is_null() {
        return Err(ExecutableMemoryPlatformError::RangeOutOfBounds);
    }

    // SAFETY: callers pass a non-null address inside a live RX mapping and the
    // P6 ARM64 seed contract says the bytes at this address implement the C ABI
    // entry shape represented by `P6Entry`.
    let entry: P6Entry = unsafe { std::mem::transmute(entry) };
    // SAFETY: the checked RX function pointer receives VM/frame opaque pointers
    // plus raw JSValue bits. The current ARM64 seed reads only the frame/callee
    // carrier for no-call/no-heap returns and never dereferences `ic_store_base`.
    Ok(unsafe {
        entry(
            vm.as_ptr(),
            frame_base.as_ptr(),
            callee_value_bits,
            ic_store_base.as_ptr(),
        )
    })
}

#[cfg(target_arch = "x86_64")]
unsafe fn call_p9_x86_64_owner_post_call_reentry(
    entry: *const u8,
    vm: NonNull<c_void>,
    frame_base: NonNull<c_void>,
    result_bits: u64,
    metadata_table_base: NonNull<c_void>,
    callee_value_bits: u64,
) -> Result<u64, ExecutableMemoryPlatformError> {
    type P9PostCallReentry =
        unsafe extern "C" fn(*mut c_void, *mut c_void, u64, *mut c_void, u64) -> u64;

    if entry.is_null() {
        return Err(ExecutableMemoryPlatformError::RangeOutOfBounds);
    }

    // SAFETY: callers pass a non-null address inside a live RX mapping and the
    // P9 owner reentry proof says the bytes at this address implement the
    // five-argument C ABI represented by `P9PostCallReentry`.
    let entry: P9PostCallReentry = unsafe { std::mem::transmute(entry) };
    // SAFETY: the checked RX function pointer receives VM/frame opaque pointers
    // plus raw JSValue bits for rax, a metadata-table base for r12, and the
    // active callee JSValue bits for the Rust callee carrier.
    Ok(unsafe {
        entry(
            vm.as_ptr(),
            frame_base.as_ptr(),
            result_bits,
            metadata_table_base.as_ptr(),
            callee_value_bits,
        )
    })
}

fn map_failed() -> *mut c_void {
    usize::MAX as *mut c_void
}

fn last_errno() -> Option<i32> {
    std::io::Error::last_os_error().raw_os_error()
}
