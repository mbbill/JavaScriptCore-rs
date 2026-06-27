- RegExp::match returns the matched substring as a UString.
- performMatch returns a UString and separately writes end offsets and output-vector pointers.
- Callers allocate match arrays from already-materialized substring results.

## Moves

- 2007-10-31 (d6aa380c) replaced by [[match-results]]: Returning a UString from RegExp::match and performMatch forced needless string allocation for every match; returning position+length as ints avoids the allocation and eliminates an intermediate UString copy on each call path. (code)
