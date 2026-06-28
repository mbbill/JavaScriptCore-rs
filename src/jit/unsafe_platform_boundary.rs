//! The single unsafe boundary for the JIT: Apple-Silicon W^X executable memory.
//!
//! Every other file under `jit/` is `#![deny(unsafe_code)]` (see `jit/mod.rs`).
//! This module is the ONE exception (`#![allow(unsafe_code)]`); it holds the
//! platform W^X primitives and nothing else. The rest of `jit/` reaches
//! executable code only through the sealed-safe wrappers defined here.
//!
//! ## C++/platform ground truth
//!
//! On arm64 macOS the optimizing default is the "fast JIT permissions" path, NOT
//! `mprotect` RW<->RX (that is the `ENABLE(MPROTECT_RX_TO_RWX)` debug path, off
//! by default — see `mcts_mem/.../executable-memory.md` fact 2024-05-29). The
//! faithful default mechanism is:
//!
//! 1. The executable heap is mapped once with `MAP_JIT` and `PROT_READ |
//!    PROT_WRITE | PROT_EXEC` — `WTF/wtf/posix/OSAllocatorPOSIX.cpp:78-92,113`
//!    (`MAP_EXECUTABLE_FOR_JIT == MAP_JIT`, `protection |= PROT_EXEC`).
//! 2. Write vs. execute is toggled PER THREAD, not per page:
//!    `threadSelfRestrict<kRwxToRw>()` -> `pthread_jit_write_protect_np(false)`
//!    (writable) and `threadSelfRestrict<kRwxToRx>()` ->
//!    `pthread_jit_write_protect_np(true)` (executable) —
//!    `JavaScriptCore/assembler/FastJITPermissions.h:110-120`. JSC drops
//!    write-protect, `memcpy`s the code (`performJITMemcpy`,
//!    `JavaScriptCore/jit/ExecutableAllocator.h:289-298`), relinks branches, then
//!    re-enables write-protect (`LinkBuffer.cpp:433-434`).
//! 3. After writing, the instruction cache is invalidated before execution:
//!    `cacheFlush` -> `sys_icache_invalidate(code, size)` on Darwin —
//!    `JavaScriptCore/assembler/ARM64Assembler.h:4021-4024`. ARM64 write sites
//!    are also asserted 4-byte aligned (`jitMemcpyChecks`,
//!    `ExecutableAllocator.h:260-266`).
//!
//! Allocation failure is control flow, never a crash
//! (`executable-memory.md` fact 2; move 2010-08-04): `allocate` returns
//! `Result`, mirroring `ExecutableAllocator::allocate` returning a null
//! `RefPtr<ExecutableMemoryHandle>`.

#![allow(unsafe_code)]

/// Failure modes of the platform W^X boundary. Maps to the null-`RefPtr` /
/// `allocationSuccessful()` failure surface of `ExecutableAllocator` /
/// `LinkBuffer` (`ExecutableAllocator.cpp`, `LinkBuffer.h:194`).
#[derive(Clone, Copy, Debug, Eq, PartialEq)]
pub enum JitBoundaryError {
    /// The fast-JIT-permissions MAP_JIT path is only modeled on arm64 macOS.
    /// Constructed only on the off-platform stub below (hence dead on the
    /// Apple-Silicon build), where `allocate` reports it instead of crashing.
    #[allow(dead_code)]
    UnsupportedPlatform,
    /// A zero-byte code image was requested; JSC never allocates an empty range.
    EmptyRequest,
    /// ARM64 instruction writes must be 4-byte aligned (`jitMemcpyChecks`,
    /// `ExecutableAllocator.h:262-266`): the code length was not a multiple of 4.
    UnalignedCodeLength { len: usize },
    /// `getpagesize()` returned a non-positive / non-power-of-two value.
    InvalidPageSize { page_size: i64 },
    /// `mmap(MAP_JIT, ...)` failed (e.g. EPERM without the JIT entitlement under
    /// the hardened runtime, or ENOMEM). The OS errno is captured for triage.
    MmapFailed { errno: Option<i32> },
}

#[cfg(all(target_os = "macos", target_arch = "aarch64"))]
mod apple_silicon {
    use super::JitBoundaryError;
    use std::ffi::{c_int, c_void};
    use std::ptr::NonNull;

    // mmap protection/flag constants (Darwin <sys/mman.h>). Mirrors the values
    // the existing `platform/unix_executable_memory.rs` uses, plus `MAP_JIT`.
    const PROT_READ: c_int = 0x1;
    const PROT_WRITE: c_int = 0x2;
    const PROT_EXEC: c_int = 0x4;
    const MAP_PRIVATE: c_int = 0x0002;
    const MAP_ANON: c_int = 0x1000;
    /// `MAP_JIT` (Darwin). `OSAllocatorPOSIX.cpp:44` defines
    /// `MAP_EXECUTABLE_FOR_JIT == MAP_JIT`; required for an executable+writable
    /// JIT mapping on Apple Silicon.
    const MAP_JIT: c_int = 0x0800;

    // ARM64 instruction size, used for the alignment precondition mirrored from
    // `jitMemcpyChecks` (`ExecutableAllocator.h:263`).
    const INSTRUCTION_SIZE: usize = 4;

    unsafe extern "C" {
        fn getpagesize() -> c_int;
        fn mmap(
            addr: *mut c_void,
            length: usize,
            prot: c_int,
            flags: c_int,
            fd: c_int,
            offset: i64,
        ) -> *mut c_void;
        fn munmap(addr: *mut c_void, len: usize) -> c_int;
        // `pthread_jit_write_protect_np` (Apple libpthread): toggles the calling
        // thread between writable (enabled = false) and executable
        // (enabled = true) for MAP_JIT memory. FastJITPermissions.h:115-117.
        fn pthread_jit_write_protect_np(enabled: c_int);
        // `sys_icache_invalidate` (Darwin libSystem): make freshly written code
        // visible to the instruction fetcher. ARM64Assembler.h:4024.
        fn sys_icache_invalidate(start: *mut c_void, len: usize);
    }

    fn map_failed() -> *mut c_void {
        usize::MAX as *mut c_void
    }

    fn last_errno() -> Option<i32> {
        std::io::Error::last_os_error().raw_os_error()
    }

    fn page_size() -> Result<usize, JitBoundaryError> {
        // SAFETY: `getpagesize` takes no arguments and only reads a process
        // constant; it cannot violate memory safety.
        let raw = unsafe { getpagesize() };
        if raw <= 0 || !(raw as u32).is_power_of_two() {
            return Err(JitBoundaryError::InvalidPageSize {
                page_size: i64::from(raw),
            });
        }
        Ok(raw as usize)
    }

    /// One MAP_JIT executable mapping. RAII owner that `munmap`s on drop.
    ///
    /// C++ map: the per-allocation slice of the fixed executable pool that
    /// `ExecutableMemoryHandle` (`jit/ExecutableMemoryHandle.h`) refers to. Here
    /// each region is its own MAP_JIT mapping rather than a sub-range of one big
    /// `FixedVMPoolExecutableAllocator` reservation; that single-pool +
    /// libpas jump-island allocator is a deferred serial coupling, noted in
    /// `executable_allocator.rs`.
    pub(crate) struct JitRegion {
        ptr: NonNull<u8>,
        /// Length of the valid code image (<= `mapped_len`).
        code_len: usize,
        /// Page-rounded length actually passed to `mmap`/`munmap`.
        mapped_len: usize,
    }

    impl JitRegion {
        /// Allocate a MAP_JIT executable region big enough for `code_len` bytes.
        ///
        /// Faithful to `OSAllocatorPOSIX::tryReserveAndCommitImpl` for the
        /// `executable` case (RWX + `MAP_JIT`). Returns `Err` on failure instead
        /// of crashing (allocation failure is control flow).
        pub(crate) fn allocate(code_len: usize) -> Result<Self, JitBoundaryError> {
            if code_len == 0 {
                return Err(JitBoundaryError::EmptyRequest);
            }
            if code_len % INSTRUCTION_SIZE != 0 {
                return Err(JitBoundaryError::UnalignedCodeLength { len: code_len });
            }
            let page = page_size()?;
            let mapped_len = code_len
                .checked_add(page - 1)
                .map(|v| v & !(page - 1))
                .ok_or(JitBoundaryError::EmptyRequest)?;

            // SAFETY: a fresh anonymous MAP_JIT mapping with a null preferred
            // address, a non-zero page-rounded length, RWX protection, the
            // required `fd = -1` / `offset = 0`, and no input pointer to alias.
            // The result is checked against MAP_FAILED/null before use. This is
            // exactly OSAllocatorPOSIX's executable mmap (PROT_EXEC + MAP_JIT).
            let raw = unsafe {
                mmap(
                    std::ptr::null_mut(),
                    mapped_len,
                    PROT_READ | PROT_WRITE | PROT_EXEC,
                    MAP_PRIVATE | MAP_ANON | MAP_JIT,
                    -1,
                    0,
                )
            };
            if raw == map_failed() || raw.is_null() {
                return Err(JitBoundaryError::MmapFailed {
                    errno: last_errno(),
                });
            }
            let ptr = NonNull::new(raw.cast::<u8>()).ok_or(JitBoundaryError::MmapFailed {
                errno: last_errno(),
            })?;
            Ok(Self {
                ptr,
                code_len,
                mapped_len,
            })
        }

        /// Run `f` with the region writable, then seal it executable and flush
        /// the instruction cache. This is the faithful
        /// `threadSelfRestrict<kRwxToRw>` -> copy/relink -> `threadSelfRestrict
        /// <kRwxToRx>` -> `cacheFlush` sequence (FastJITPermissions.h:114-117,
        /// LinkBuffer.cpp:433-434, ARM64Assembler.h:4024).
        ///
        /// The re-seal + icache flush run in a drop guard so they happen even if
        /// `f` panics: leaving the thread write-protected (executable) is the
        /// safe resting state, and an un-flushed icache is never reached because
        /// the guard always flushes before this returns.
        pub(crate) fn with_writable<R>(&self, f: impl FnOnce(&mut [u8]) -> R) -> R {
            struct SealGuard<'a>(&'a JitRegion);
            impl Drop for SealGuard<'_> {
                fn drop(&mut self) {
                    // SAFETY: re-enable write-protect (execute mode) for this
                    // thread and invalidate the icache over the live mapping the
                    // region still owns. `code_len <= mapped_len`, both within the
                    // mapping. FastJITPermissions.h:117, ARM64Assembler.h:4024.
                    unsafe {
                        pthread_jit_write_protect_np(1);
                        sys_icache_invalidate(
                            self.0.ptr.as_ptr().cast::<c_void>(),
                            self.0.code_len,
                        );
                    }
                }
            }

            // SAFETY: drop write-protect so this thread may write the MAP_JIT
            // region. The SealGuard restores execute mode + flushes on any exit.
            unsafe { pthread_jit_write_protect_np(0) };
            let _guard = SealGuard(self);
            // SAFETY: `ptr` is a live mapping of at least `code_len` bytes owned
            // by this region; with write-protect off the thread may write it, and
            // `&mut [u8]` is exclusive because `with_writable` takes `&self` and
            // hands the only slice to `f` for the duration of the call.
            let slice = unsafe { std::slice::from_raw_parts_mut(self.ptr.as_ptr(), self.code_len) };
            f(slice)
        }

        /// Call a sealed region as `extern "C" fn() -> u64` and return the value.
        ///
        /// Sealed-safe wrapper: the JSC analog of casting a finalized
        /// `MacroAssemblerCodeRef` to a typed function pointer and calling it
        /// (`finalizeCodeWithoutDisassembly` -> `CodeRef`). Safe to expose because
        /// the only producer of a `JitRegion` is the faithful finalize path
        /// (`executable_allocator::finalize_arm64_link_buffer`), which copies a
        /// byte-oracle-proven encoder image, relocates it, seals it executable,
        /// and flushes the icache; the precondition is that the caller emitted a
        /// real `extern "C" fn() -> u64` at offset 0.
        pub(crate) fn call_finalized_nullary_u64(&self) -> u64 {
            type Entry = unsafe extern "C" fn() -> u64;
            // SAFETY: the region is sealed RX (write-protect on) and holds a
            // finalized nullary C-ABI function at its base, per the wrapper
            // contract above. The transmute reinterprets the executable base
            // address as that function pointer.
            let entry: Entry =
                unsafe { std::mem::transmute::<*const u8, Entry>(self.ptr.as_ptr()) };
            // SAFETY: calling generated machine code that honors the C ABI of
            // `Entry`. The encoder produced `... ; ret`, so control returns here.
            unsafe { entry() }
        }

        /// Call a sealed region as `extern "C" fn(u64, u64) -> u64`.
        ///
        /// Same sealed-safe contract as [`Self::call_finalized_nullary_u64`], for
        /// a two-argument entry (the milestone `add x0, x0, x1; ret`).
        pub(crate) fn call_finalized_binary_u64(&self, a: u64, b: u64) -> u64 {
            type Entry = unsafe extern "C" fn(u64, u64) -> u64;
            // SAFETY: as above; the region holds a finalized two-argument C-ABI
            // function at its base. Arguments are passed in x0/x1 per the C ABI.
            let entry: Entry =
                unsafe { std::mem::transmute::<*const u8, Entry>(self.ptr.as_ptr()) };
            // SAFETY: calling generated machine code honoring `Entry`'s C ABI.
            unsafe { entry(a, b) }
        }
    }

    impl Drop for JitRegion {
        fn drop(&mut self) {
            // SAFETY: `ptr`/`mapped_len` describe the live mapping this region
            // exclusively owns; this is its single `munmap`. Errors are
            // unreportable in Drop and harmless (process teardown reclaims).
            unsafe {
                munmap(self.ptr.as_ptr().cast::<c_void>(), self.mapped_len);
            }
        }
    }
}

#[cfg(all(target_os = "macos", target_arch = "aarch64"))]
pub(crate) use apple_silicon::JitRegion;

// Non-(arm64 macOS) stub: the fast-JIT-permissions mechanism is Apple-Silicon
// specific, so allocation simply reports `UnsupportedPlatform`. No handle is ever
// constructed off-platform, so the execution methods are never reached.
#[cfg(not(all(target_os = "macos", target_arch = "aarch64")))]
pub(crate) struct JitRegion {
    _never: std::convert::Infallible,
}

#[cfg(not(all(target_os = "macos", target_arch = "aarch64")))]
impl JitRegion {
    pub(crate) fn allocate(_code_len: usize) -> Result<Self, JitBoundaryError> {
        Err(JitBoundaryError::UnsupportedPlatform)
    }

    pub(crate) fn with_writable<R>(&self, _f: impl FnOnce(&mut [u8]) -> R) -> R {
        match self._never {}
    }

    pub(crate) fn call_finalized_nullary_u64(&self) -> u64 {
        match self._never {}
    }

    pub(crate) fn call_finalized_binary_u64(&self, _a: u64, _b: u64) -> u64 {
        match self._never {}
    }
}
