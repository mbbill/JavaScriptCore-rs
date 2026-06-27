- Random doubles are produced from one platform integer sample.
- The output space is limited to 31 or 32 bits of entropy.
- Scaling divides by the integer generator's maximum value rather than composing a full double mantissa.

## Moves

- 2009-01-02 (5e5f53d8) replaced by [[randomness]]: JavaScript doubles have 53 bits of mantissa; generating only 2^32 (or 2^31) distinct values from Math.random() wastes the representable precision of the return type and makes output distinguishable from a true uniform double distribution. (sourced)
