- iOS ARM cache maintenance used separate data-cache flush and instruction-cache invalidate syscalls.

## Moves

- 2010-12-16 (abcf6673) replaced by [[executable-memory]]: sys_dcache_flush + sys_icache_invalidate were replaced by sys_cache_control(kCacheFunctionPrepareForExecution,...) described as 'more correct and forward looking' by the commit author, unifying the two-step data+instruction cache invalidation into a single OS-provided call on iOS ARM Thumb2. (sourced)
