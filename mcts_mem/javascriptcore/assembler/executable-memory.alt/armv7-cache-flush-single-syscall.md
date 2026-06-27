- ARMv7 Linux flushed arbitrary executable ranges with one ARM_NR_cacheflush syscall.
- The syscall was passed the whole code start/end range in a single inline-assembly block.

## Moves

- 2013-03-08 (9ddc1104) replaced by [[executable-memory]]: The single ARM Linux syscall (r7=0xf0002) to flush an arbitrary range caused random crashes on ARMv7 Linux with V8 tests; the fix iterates page-by-page matching the approach that works for traditional ARM, similar to a prior bug fix for the same class of problem (bug 77712). (sourced)
