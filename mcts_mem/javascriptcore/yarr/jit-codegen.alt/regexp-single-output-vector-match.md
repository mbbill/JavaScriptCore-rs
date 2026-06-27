- RegExp matching always receives and populates an output vector.
- The JIT execute path returns only the match start and writes captures through the vector.
- Callers that do not need captures still allocate temporary output-vector storage.

## Moves

- 2012-03-28 (f795f157) replaced by [[jit-codegen]]: The old API always allocated and populated an output vector even when callers only needed the match span, while the new API can compile and cache a JIT variant that omits subpattern output writes. (sourced)
