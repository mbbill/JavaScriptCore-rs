//! Faithful executable-memory allocator + LinkBuffer finalize (emit -> execute).
//!
//! C++ map:
//! - `JavaScriptCore/jit/ExecutableAllocator.h` (`class ExecutableAllocator`,
//!   `allocate(size, JITCompilationEffort) -> RefPtr<ExecutableMemoryHandle>`)
//!   -> the [`ExecutableAllocator`] trait + [`ExecutableMemoryHandle`].
//! - `JavaScriptCore/jit/ExecutableMemoryHandle.h` (`ExecutableMemoryHandle`,
//!   the handle to one allocated executable range) -> [`ExecutableMemoryHandle`].
//! - `JavaScriptCore/assembler/LinkBuffer.cpp`
//!   (`linkCode` / `copyCompactAndLinkCode` / `finalizeCodeWithoutDisassembly`):
//!   copy the assembler byte image into executable memory, relink branches in
//!   place, seal RX, flush the icache, hand back a callable `CodeRef`)
//!   -> [`finalize_arm64_link_buffer`].
//!
//! All unsafe (MAP_JIT, `pthread_jit_write_protect_np`, `sys_icache_invalidate`,
//! the fn-pointer transmute) is sealed inside `super::unsafe_platform_boundary`;
//! this module is safe (`jit/mod.rs` is `#![deny(unsafe_code)]`).
//!
//! Faithfulness boundaries / deferred serial couplings (no C++ divergence
//! invented here, just not-yet-ported scope):
//! - Each [`ExecutableMemoryHandle`] is its own MAP_JIT mapping. JSC instead
//!   carves sub-ranges from ONE process-wide `FixedVMPoolExecutableAllocator`
//!   reservation managed by libpas `jit_heap` with bitfit + jump islands
//!   (`executable-memory.md` move 2021-07-13). Porting that single pool is a
//!   later unit; the W^X + finalize contract proven here is identical.
//! - `arm64_baseline.rs` still emits hardcoded byte blobs; rewiring it to emit
//!   through `Arm64Encoder` and finalize through this path is a serial step.

use core::cell::Cell;

use crate::assembler::link_records::{Arm64LinkError, Arm64LinkRecord};
use crate::jit::unsafe_platform_boundary::{JitBoundaryError, JitRegion};

/// Failure of executable allocation / finalize. Maps to the null-`RefPtr`
/// (`ExecutableAllocator::allocate`) and `!allocationSuccessful()`
/// (`LinkBuffer.h:194`) failure surface — allocation failure is control flow,
/// never a crash (`executable-memory.md` fact 2 / move 2010-08-04).
#[derive(Clone, Copy, Debug, Eq, PartialEq)]
pub enum ExecutableAllocationError {
    /// JSC never allocates a zero-length code range.
    EmptyRequest,
    /// The fixed executable reserve cannot satisfy this request — the faithful
    /// analog of `FixedVMPoolExecutableAllocator` returning null on exhaustion
    /// (`executable-memory.md` fact 2015-07-21 / move 2010-08-04).
    OutOfExecutableMemory { requested: usize, available: usize },
    /// The finalize image length did not match the allocated handle size.
    SizeMismatch { expected: usize, actual: usize },
    /// A branch could not be relinked by the in-place ARM64 link pass.
    LinkFailed(Arm64LinkError),
    /// The platform W^X boundary failed (unsupported platform, mmap failure,
    /// alignment, ...). Carries the precise [`JitBoundaryError`].
    Platform(JitBoundaryError),
}

impl From<JitBoundaryError> for ExecutableAllocationError {
    fn from(error: JitBoundaryError) -> Self {
        ExecutableAllocationError::Platform(error)
    }
}

/// A handle to one allocated (and, after finalize, sealed-executable) range of
/// JIT memory. Owns the MAP_JIT mapping; dropping it frees the mapping.
///
/// C++ map: `ExecutableMemoryHandle` (`jit/ExecutableMemoryHandle.h`), the thing
/// `LinkBuffer::m_executableMemory` holds and `CodeRef` keeps alive.
pub struct ExecutableMemoryHandle {
    size_in_bytes: usize,
    region: JitRegion,
}

impl ExecutableMemoryHandle {
    /// `ExecutableMemoryHandle::sizeInBytes()`.
    pub fn size_in_bytes(&self) -> usize {
        self.size_in_bytes
    }

    /// Copy `code` into the region and relink branches in place, then seal.
    ///
    /// Faithful core of `LinkBuffer::copyCompactAndLinkCode`: with the region
    /// writable (`threadSelfRestrict<kRwxToRw>`), `performJITMemcpy` the image
    /// in, run the existing ARM64 link pass over each jump record, then seal RX
    /// (`threadSelfRestrict<kRwxToRx>`) and flush the icache — the last three of
    /// which the boundary's `with_writable` guard performs on scope exit.
    fn copy_and_link(
        &self,
        code: &[u8],
        link_records: &mut [Arm64LinkRecord],
    ) -> Result<(), ExecutableAllocationError> {
        if code.len() != self.size_in_bytes {
            return Err(ExecutableAllocationError::SizeMismatch {
                expected: self.size_in_bytes,
                actual: code.len(),
            });
        }
        self.region.with_writable(|dst| {
            // performJITMemcpy: copy the byte-oracle-proven encoder image in.
            dst.copy_from_slice(code);
            // Relink branches in place via the existing LinkBuffer relocation
            // pass (assembler/link_records.rs). For straight-line code (the
            // movz/ret and add/ret milestones) this is an empty, faithful no-op.
            for record in link_records.iter_mut() {
                record
                    .link(dst)
                    .map_err(ExecutableAllocationError::LinkFailed)?;
            }
            Ok(())
        })
    }

    /// Call the finalized entry as `extern "C" fn() -> u64`.
    /// JSC analog: cast the finalized `CodeRef` to a typed function pointer.
    pub fn call_finalized_nullary_u64(&self) -> u64 {
        self.region.call_finalized_nullary_u64()
    }

    /// Call the finalized entry as `extern "C" fn(u64, u64) -> u64`.
    pub fn call_finalized_binary_u64(&self, a: u64, b: u64) -> u64 {
        self.region.call_finalized_binary_u64(a, b)
    }

    /// Enter a finalized baseline-JIT image on the native JS stack (A1.1): switch
    /// `sp` to the seeded `entry_sp` (= `calleeFrame + sizeof(CallerFrameAndPC)`),
    /// pass the `*mut Vm` in x0, and return the `op_ret` boxed value in x0.
    pub fn call_baseline_jit_entry(&self, entry_sp: usize, vm: u64) -> u64 {
        self.region.call_baseline_jit_entry(entry_sp, vm)
    }

    /// The absolute entry address of this sealed image (offset 0): the `blr` target
    /// a native JIT->JIT call bakes (A1.3), the analog of resolving a linked
    /// `CallLinkInfo`'s `m_monomorphicCallDestination`.
    #[cfg(all(target_os = "macos", target_arch = "aarch64"))]
    pub fn entry_address(&self) -> usize {
        self.region.entry_address()
    }
}

/// Allocator of executable memory. Maps to `class ExecutableAllocator`
/// (`jit/ExecutableAllocator.h:321`): `allocate` hands back a handle to a fresh
/// executable range, or fails (control flow).
///
/// Per the macroassembler audit this is a trait with a production MAP_JIT impl
/// ([`MapJitExecutableAllocator`]) and a capacity-bounded testable impl
/// ([`FixedCapacityExecutableAllocator`]) so allocation-failure-as-control-flow
/// is exercised deterministically.
pub trait ExecutableAllocator {
    fn allocate(
        &self,
        size_in_bytes: usize,
    ) -> Result<ExecutableMemoryHandle, ExecutableAllocationError>;
}

/// Production allocator: every request is its own MAP_JIT mapping.
///
/// C++ map: the Apple-Silicon `ExecutableAllocator` whose underlying allocator
/// is the fixed executable pool. The single-pool/libpas/jump-island machinery is
/// the deferred serial coupling noted in the module docs.
#[derive(Clone, Copy, Debug, Default)]
pub struct MapJitExecutableAllocator;

impl ExecutableAllocator for MapJitExecutableAllocator {
    fn allocate(
        &self,
        size_in_bytes: usize,
    ) -> Result<ExecutableMemoryHandle, ExecutableAllocationError> {
        if size_in_bytes == 0 {
            return Err(ExecutableAllocationError::EmptyRequest);
        }
        let region = JitRegion::allocate(size_in_bytes)?;
        Ok(ExecutableMemoryHandle {
            size_in_bytes,
            region,
        })
    }
}

/// Testable allocator with a bounded executable reserve. Returns
/// `OutOfExecutableMemory` once the running total would exceed `capacity`,
/// modeling the `FixedVMPoolExecutableAllocator` finite reserve and its
/// allocation-failure path (`executable-memory.md` fact 2015-07-21: the
/// artificial small-pool test mode that exercised >20 failures).
///
/// Within budget it delegates to the same MAP_JIT path, so code finalized
/// through it is genuinely executable.
pub struct FixedCapacityExecutableAllocator {
    capacity: usize,
    used: Cell<usize>,
}

impl FixedCapacityExecutableAllocator {
    pub fn new(capacity: usize) -> Self {
        Self {
            capacity,
            used: Cell::new(0),
        }
    }

    /// Bytes still available in the reserve.
    pub fn available(&self) -> usize {
        self.capacity.saturating_sub(self.used.get())
    }
}

impl ExecutableAllocator for FixedCapacityExecutableAllocator {
    fn allocate(
        &self,
        size_in_bytes: usize,
    ) -> Result<ExecutableMemoryHandle, ExecutableAllocationError> {
        if size_in_bytes == 0 {
            return Err(ExecutableAllocationError::EmptyRequest);
        }
        // Capacity check first: an over-budget request fails as control flow
        // without touching the platform (so the failure path is deterministic on
        // every host), exactly as the fixed pool refuses before reserving.
        let available = self.available();
        if size_in_bytes > available {
            return Err(ExecutableAllocationError::OutOfExecutableMemory {
                requested: size_in_bytes,
                available,
            });
        }
        let region = JitRegion::allocate(size_in_bytes)?;
        self.used.set(self.used.get() + size_in_bytes);
        Ok(ExecutableMemoryHandle {
            size_in_bytes,
            region,
        })
    }
}

/// Finalize an ARM64 assembler byte image into callable executable memory.
///
/// Faithful `LinkBuffer::finalizeCodeWithoutDisassembly` for the ARM64
/// fast-JIT-permissions path: allocate executable memory (failure = `Err`),
/// `performJITMemcpy` the image in while the thread is writable, relink branches
/// in place via the existing ARM64 link pass, seal RX, and flush the icache. The
/// returned [`ExecutableMemoryHandle`] is the callable `CodeRef`.
pub fn finalize_arm64_link_buffer<A: ExecutableAllocator + ?Sized>(
    allocator: &A,
    code: &[u8],
    link_records: &mut [Arm64LinkRecord],
) -> Result<ExecutableMemoryHandle, ExecutableAllocationError> {
    if code.is_empty() {
        return Err(ExecutableAllocationError::EmptyRequest);
    }
    let handle = allocator.allocate(code.len())?;
    handle.copy_and_link(code, link_records)?;
    Ok(handle)
}

#[cfg(test)]
mod tests {
    use super::*;

    // The allocation-failure-as-control-flow tests are platform independent: the
    // capacity refusal happens before any platform call.
    #[test]
    fn allocation_failure_is_a_result_not_a_panic() {
        let allocator = FixedCapacityExecutableAllocator::new(8);
        let outcome = allocator.allocate(64);
        assert_eq!(
            outcome.err(),
            Some(ExecutableAllocationError::OutOfExecutableMemory {
                requested: 64,
                available: 8,
            })
        );
    }

    #[test]
    fn empty_request_is_a_result_not_a_panic() {
        let allocator = MapJitExecutableAllocator;
        assert_eq!(
            allocator.allocate(0).err(),
            Some(ExecutableAllocationError::EmptyRequest)
        );
        let mut empty: Vec<Arm64LinkRecord> = Vec::new();
        assert_eq!(
            finalize_arm64_link_buffer(&allocator, &[], &mut empty).err(),
            Some(ExecutableAllocationError::EmptyRequest)
        );
    }

    // ------------------------------------------------------------------------
    // THE MILESTONE: emit real ARM64 via the byte-oracle-proven encoder,
    // finalize through the W^X allocator, cast to a fn pointer, CALL it, and
    // assert it returns 42 / a+b. These execute machine code the encoder
    // produced; a wrong write-protect/icache order would fault or return stale
    // bytes, so a correct return value is also the ordering proof.
    // ------------------------------------------------------------------------
    #[cfg(all(target_os = "macos", target_arch = "aarch64"))]
    mod milestone {
        use super::*;
        use crate::assembler::arm64_encoder::{Arm64Encoder, Condition};
        use crate::assembler::link_records::{Arm64LinkRecord, JumpType};
        use crate::assembler::registers::RegisterID;

        fn emit(build: impl FnOnce(&mut Arm64Encoder)) -> Vec<u8> {
            let mut code = Vec::new();
            let mut enc = Arm64Encoder::new(&mut code);
            build(&mut enc);
            code
        }

        #[test]
        fn movz_42_ret_finalizes_and_executes_returning_42() {
            // movz x0, #42 ; ret
            let code = emit(|e| {
                e.emit_movz(RegisterID::X0, 42, 0);
                e.emit_ret();
            });
            assert_eq!(code.len(), 8);

            let allocator = MapJitExecutableAllocator;
            let mut no_relocations: Vec<Arm64LinkRecord> = Vec::new();
            let handle = finalize_arm64_link_buffer(&allocator, &code, &mut no_relocations)
                .expect("finalize movz/ret");

            // Execute the machine code the encoder produced.
            assert_eq!(handle.call_finalized_nullary_u64(), 42);
        }

        #[test]
        fn add_two_args_ret_finalizes_and_executes_returning_sum() {
            // add x0, x0, x1 ; ret   (extern "C" fn(u64, u64) -> u64)
            let code = emit(|e| {
                e.emit_add_reg(RegisterID::X0, RegisterID::X0, RegisterID::X1);
                e.emit_ret();
            });
            assert_eq!(code.len(), 8);

            let allocator = MapJitExecutableAllocator;
            let mut no_relocations: Vec<Arm64LinkRecord> = Vec::new();
            let handle = finalize_arm64_link_buffer(&allocator, &code, &mut no_relocations)
                .expect("finalize add/ret");

            assert_eq!(handle.call_finalized_binary_u64(40, 2), 42);
            assert_eq!(handle.call_finalized_binary_u64(0, 0), 0);
            assert_eq!(handle.call_finalized_binary_u64(1000, 337), 1337);
            assert_eq!(handle.call_finalized_binary_u64(u64::MAX, 1), 0); // wrapping add
        }

        #[test]
        fn finalize_relocates_an_unconditional_branch_then_executes() {
            // Prove the LinkBuffer relocation pass runs end-to-end on real
            // executable memory:
            //   [0] movz x0, #7
            //   [1] b   <placeholder 0>   -> relinked to [3]
            //   [2] movz x0, #99          (skipped at runtime)
            //   [3] ret
            // If the branch is relocated correctly the #99 store is skipped and
            // the function returns 7, not 99.
            let code = emit(|e| {
                e.emit_movz(RegisterID::X0, 7, 0);
                e.emit_b(0); // unlinked placeholder, like JSC before relocation
                e.emit_movz(RegisterID::X0, 99, 0);
                e.emit_ret();
            });
            assert_eq!(code.len(), 16);

            // Branch word is at byte 4; target (ret) is at byte 12.
            let mut records = vec![Arm64LinkRecord::new_jump(
                4,
                12,
                JumpType::JumpNoCondition,
                Condition::Invalid,
            )];

            let allocator = MapJitExecutableAllocator;
            let handle = finalize_arm64_link_buffer(&allocator, &code, &mut records)
                .expect("finalize branch + relocate");

            assert_eq!(handle.call_finalized_nullary_u64(), 7);
        }

        #[test]
        fn fixed_capacity_allocator_finalizes_and_executes_within_budget() {
            // The testable allocator also produces genuinely executable memory.
            let allocator = FixedCapacityExecutableAllocator::new(4096);
            let code = emit(|e| {
                e.emit_movz(RegisterID::X0, 42, 0);
                e.emit_ret();
            });
            let mut no_relocations: Vec<Arm64LinkRecord> = Vec::new();
            let handle = finalize_arm64_link_buffer(&allocator, &code, &mut no_relocations)
                .expect("finalize within budget");
            assert_eq!(handle.call_finalized_nullary_u64(), 42);
        }

        #[test]
        fn many_finalizations_each_execute_independently() {
            // Each handle owns its own sealed mapping; finalize/execute several
            // and confirm no cross-talk (and that drop/munmap of earlier handles
            // does not disturb later ones).
            let allocator = MapJitExecutableAllocator;
            for value in [0u16, 1, 42, 255, 4096, 65535] {
                let code = emit(|e| {
                    e.emit_movz(RegisterID::X0, value, 0);
                    e.emit_ret();
                });
                let mut no_relocations: Vec<Arm64LinkRecord> = Vec::new();
                let handle = finalize_arm64_link_buffer(&allocator, &code, &mut no_relocations)
                    .expect("finalize");
                assert_eq!(handle.call_finalized_nullary_u64(), u64::from(value));
            }
        }
    }
}
