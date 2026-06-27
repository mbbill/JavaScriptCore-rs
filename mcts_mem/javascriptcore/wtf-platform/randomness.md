- WTF exposes random-number APIs through a cryptographic-randomness abstraction where the OS provides one, with narrow fallback paths for ports without OS randomness.
- JavaScript-facing random doubles consume enough entropy to fill the 53-bit mantissa rather than using a single 31- or 32-bit integer sample.
- Per-realm `Math.random()` state is seeded from secure randomness and is not shared across global objects.

## Moves

- 2008-12-30 (874abb7f) replaced [[darwin-random-prng]]: random() output is predictable and led to user tracking via Math.random(); arc4random() is cryptographically strong and self-seeding, eliminating the need for srandomdev() initialization. (sourced)
- 2009-01-02 (5e5f53d8) replaced [[random-number-32bit-resolution]]: JavaScript doubles have 53 bits of mantissa; generating only 2^32 (or 2^31) distinct values from Math.random() wastes the representable precision of the return type and makes output distinguishable from a true uniform double distribution. (sourced)
- 2011-02-28 (3ffbcd40) replaced [[random-number-per-platform-dispatch]]: randomNumber() previously contained per-platform branches (MSVC rand_s, Darwin arc4random, Unix random(), Windows rand, Mersenne Twister) duplicating logic already in cryptographicallyRandomNumber(); refactoring delegates to that abstraction when USE(OS_RANDOMNESS) is available, leaving only non-OS_RANDOMNESS ports (Mersenne Twister, BREWMP) in the fallback path. (code)
