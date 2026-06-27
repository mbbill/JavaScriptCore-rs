- Stack-slot assignment tried one candidate offset at a time.
- Every candidate rescanned all assigned interfering slots for overlap.
- Candidate order followed arbitrary interference-list order rather than closest-to-FP first fit.

## Moves

- 2026-03-20 (c03651a4) replaced by [[stack-slots]]: Air stack allocation changed from trying candidate offsets and rescanning all interfering slots to sorting assigned interferences by frame offset and doing one downward sweep past overlaps, reducing the assignment algorithm from O(n²) to O(n log n). (code)
