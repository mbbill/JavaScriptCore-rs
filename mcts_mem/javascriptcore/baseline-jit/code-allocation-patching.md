- Baseline generated code is copied into executable memory, protected, flushed, and patched under platform-specific executable-page constraints.
- The executable allocator treats JIT memory as a scarce reserved region with guard pages, pressure callbacks, entitlement gates, and port-specific page-return behavior.
- Code patching records the information needed to decide whether a write must be atomic, whether executable pages must be made writable, and which cache-flush path is valid.

## Facts

- 2008-11-06 (0c48f33b) pitfall: JIT mmap allocation must include PROT_EXEC on kernels that enforce non-executable memory, such as ExecShield on Fedora Linux. (sourced)
- 2009-04-27 (2e42b35f) rationale: the JIT code pool base is randomized with limited entropy to provide ASLR for the JIT heap while preserving plugin compatibility. (sourced)
- 2009-06-02 (9703565a) rationale: W^X-exclusive mode allocates JIT pages RX, flips them to RW only during code generation, and was initially disabled because it cost 5-10% performance. (sourced)
- 2011-01-31 (daf2fadd) rationale: the fixed VM pool switched away from best-fit because real allocation patterns caused external fragmentation and production crashes. (sourced)
- 2012-05-11 (84ccbaa4) pitfall: MADV_FREE failure handling retries only EAGAIN because failing to return pages to the OS is preferable to crashing when the allocator can still reuse them internally. (code)
- 2013-01-16 (00d50293) rationale: MADV_FREE for JIT memory is gated to older non-iOS macOS because newer Darwin JIT mappings need decommit instead. (sourced)
- 2016-03-09 (57357db3) pitfall: separated-W^X initialization must tolerate mach_vm_remap failure and fall back to the original JIT base address. (code)
- 2019-09-07 (c483abc7) pitfall: performJITMemcpy must reject source buffers inside Gigacage so executable code is never copied from caged memory. (code)
- 2019-09-13 (21943389) pitfall: the Gigacage source assertion must fire at the point copying finishes, not in an RAII destructor that exception unwinding could skip. (sourced)
- 2024-04-05 (ce585987) rationale: iOS JIT enablement recognizes both legacy dynamic-codesigning and allow-jit entitlements, with SDK and watchOS gates on emitted entitlements. (code)
- 2025-04-08 (6f298859) pitfall: variable-sized JIT allocation must round with pointer-width arithmetic because 32-bit arithmetic can wrap before the large-object cutoff check. (code)
- 2026-06-03 (de822708) rationale: InlineCacheHandler hot fields are cache-line-aligned so structure ID, next pointer, targets, offset, cache type, JS-call bit, and uid fit in the first cache line. (sourced)

## Moves

- 2012-05-10 (fef52580) replaced [[fixed-vm-pool-decommit-free-pages]]: Work around the problem by using a different madvise() flag, but only for the JIT memory allocator. (sourced)
- 2016-03-16 (4905f9cd) replaced [[always-available-separated-wx-heap-jit-writes]]: Separated W^X heap support was put back behind ENABLE_SEPARATED_WX_HEAP and disabled in feature defines because the ungated version caused crashes on ARM. (sourced)
- 2024-11-07 (169e231f) replaced [[jit-code-copy-memcpy]]: JIT code repatching sometimes needs atomic writes, so copying executable bytes now uses relaxed atomic stores for 1-, 2-, 4-, and 8-byte writes on architectures that do not need aligned access and falls back to memcpy otherwise. (sourced)
