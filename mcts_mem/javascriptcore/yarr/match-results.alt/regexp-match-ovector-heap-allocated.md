- RegExp::match allocates an output vector on the heap for each match.
- Ownership is carried through OwnArrayPtr or caller-managed raw pointers.
- RegExpConstructorPrivate stores the last output vector as heap-owned state.

## Moves

- 2009-07-04 (390e99e8) replaced by [[match-results]]: Replacing OwnArrayPtr<int> (heap allocation per match) with Vector<int,32> (32-int inline buffer, reused across calls) eliminated the per-match heap allocation, yielding ~5% speedup on SunSpider string-unpack-code and 0.3% overall. (sourced)
