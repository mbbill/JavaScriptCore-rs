- ArrayProfile::computeUpdatedPrediction and LazyOperandValueProfile holder access required a ConcurrentJSLocker tied to CodeBlock::m_lock at update and add sites.
- LazyOperandValueProfile storage was a SegmentedVector behind a unique_ptr and additions occurred while holding the CodeBlock lock.
- DFG parsing initialized lazy operand profiles while holding CodeBlock::m_lock even when it only needed a stable profile list.

## Moves

- 2023-10-05 (d62f981d) replaced by [[metadata-table]]: Profile updates on 64-bit no longer take CodeBlock::m_lock because ArrayProfile updates are intentionally racy and LazyOperandValueProfile additions are published from the mutator to compiler threads with ConcurrentVector plus storeStoreFence. (sourced)
