- Generated code is copied into executable-memory allocations rather than making ordinary heap pages executable.
- JIT allocation failure is part of the control flow: allocation APIs can fail, LinkBuffer exposes the failure, and callers fall back or throw instead of crashing.
- Writable/executable transitions are mediated by allocator and permission APIs; fast-permission paths are selected from runtime platform support.
- Cache flushing, jump-island reachability, and code-signing/JIT entitlements are treated as executable-memory finalization policy, not as caller-local details.
- The executable heap is process-level infrastructure rather than VM-owned state, allowing generated code to be linked without a VM dependency.

## Facts

- 2008-12-07 (8a7bcdb1) measurement: replacing heap-wide PROT_EXEC with a dedicated ExecutableAllocator yielded 1-2% progression on SunSpider-v8 and 1% on SunSpider while reducing memory usage. (sourced)
- 2010-01-08 (96578057) pitfall: ARM_TRADITIONAL link-time branch patching flushed an instruction word redundantly because executableCopy already flushed instruction words and only the target address slot needed maintenance. (code)
- 2015-07-21 (b9ee9021) measurement: an artificial 50KB JIT pool test mode had more than 20 failures before adding a reserved allocation area and none after it. (sourced)
- 2015-07-21 (b9ee9021) rationale: the nonfailable allocation reserve is a fraction rather than a constant because each allocation that can fail may induce a variable number of allocations that cannot fail. (sourced)
- 2016-05-11 (aaa85863) rationale: reducing the X86_64 fixed executable memory pool from 1 GiB to 100 MiB was rolled back because it was bad news for asm.js. (sourced)
- 2018-09-27 (93daf9e2) pitfall: ARM64 JIT instruction writes must be 4-byte aligned; performJITMemcpy and ARM64 patching sites assert alignment and fixed-pool range membership before copying. (code)
- 2021-04-30 (ef0cc6ac) measurement: with ARM64 jump islands, reducing the fixed executable pool default from 1GB to 512MB was estimated to speed JetStream2 by 2% and Speedometer2 by 1.5%. (sourced)
- 2022-04-07 (e6c13308) rationale: ARMv7 near calls can use direct bl/b branches only if executable regions and jump islands are sized by nearJumpRange so out-of-range targets remain reachable. (code)
- 2022-10-19 (256a5b87) pitfall: AssemblyCommentRegistry ranges must be unregistered when non-libpas executable memory is released, or recycled executable memory can violate the registry's disjoint-range invariant. (code)
- 2023-12-21 (0d543178) rationale: libpas jump-island stress fragments executable memory with temporary same-size random allocations and then frees them so normal allocation semantics are preserved. (code)
- 2024-05-29 (3d6a7300) rationale: mprotect executable-memory mode is disabled by default because page-protection transitions are expected to be costly, but remains useful for debugging executable-memory corruption. (sourced)

## Moves

- 2009-02-19 (77607cda) replaced [[jit-pc-relative-call-to-stubs]]: On x86-64 the JSC text segment can lie >2GB from the JIT heap, making 32-bit pc-relative calls to Interpreter stub functions unreachable; x86-64 calls to out-of-range targets must go through an indirect mov-r11/call-r11 sequence instead. (sourced)
- 2010-08-04 (ae752df2) replaced [[jit-alloc-crash-on-oom]]: JIT code allocation exhaustion previously hit ASSERT/CRASH; changed so allocators return null, LinkBuffer exposes allocationSuccessful(), JIT throws a JS out-of-memory exception, and YARR falls back to PCRE, enabling recovery instead of process abort. (code)
- 2010-12-16 (abcf6673) replaced [[ios-arm-cache-flush-two-syscall]]: sys_dcache_flush + sys_icache_invalidate were replaced by sys_cache_control(kCacheFunctionPrepareForExecution,...) described as 'more correct and forward looking' by the commit author, unifying the two-step data+instruction cache invalidation into a single OS-provided call on iOS ARM Thumb2. (sourced)
- 2013-03-08 (9ddc1104) replaced [[armv7-cache-flush-single-syscall]]: The single ARM Linux syscall (r7=0xf0002) to flush an arbitrary range caused random crashes on ARMv7 Linux with V8 tests; the fix iterates page-by-page matching the approach that works for traditional ARM, similar to a prior bug fix for the same class of problem (bug 77712). (sourced)
- 2016-03-02 (f7dfea1b) replaced [[fixed-vm-executable-allocator-only]]: The on-demand executable allocator removal was rolled back because it caused crashes on Mac 32-bit and on ARM. (sourced)
- 2017-03-29 (4ed0e2b9) replaced [[vm-owned-executable-allocation]]: LinkBuffer and ExecutableAllocator were detached from VM ownership so generated code and executable memory allocation would not carry a VM dependency while moving WebAssembly toward position-independent code. (sourced)
- 2021-03-18 (756de704) replaced [[compile-time-fast-jit-permissions-selection]]: Fast JIT permission support is selected once during JIT page reservation using runtime API availability checks and stored in g_jscConfig, instead of treating every ARM64E build as supporting the fast permissions path. (code)
- 2021-07-13 (b6d532a7) replaced [[metaallocator-jit-executable-pool]]: JSC executable allocation switched from WTF::MetaAllocator handles to libpas jit_heap so the allocator could use bitfit/large-heap allocation, approximate first-fit over supplied ranges, no in-managed-memory metadata, bounded allocation/deallocation behavior, fine-grained locking, and libpas scavenging policy. (sourced)
