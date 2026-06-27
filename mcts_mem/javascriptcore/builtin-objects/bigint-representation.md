- BigInts use BigInt32 immediates when the value fits platform support and heap JSBigInt cells for larger magnitudes.
- Heap BigInt digits are native-word arrays stored contiguously with immutable length-known cell payloads.
- Sign is encoded outside the digit payload, and arithmetic normalizes results before allocating final cells when possible.
- BigInt algorithms share digit spans and temporary vectors rather than duplicating separate inline and heap code paths.

## Facts

- 2020-04-25 (430772d6) measurement: Canonicalizing heap-BigInt math results back to BigInt32 when possible made a SunSpider sha1 BigInt variant 86% faster than ToT and 36% faster than the first BigInt32 implementation. (sourced)
- 2026-02-23 (d2f75e39) rationale: JSBigInt stores sign in JSCell's per-cell bit so the data payload can begin after length/hash and the minimum JSBigInt size is 16 bytes. (sourced)
- 2026-06-02 (9923753c) measurement: Increment/decrement now allocate the result at target digit length and write directly into storage, improving heap BigInt inc/dec microbenchmarks by 1.3392x and 1.4338x. (sourced)

## Moves

- 2019-11-18 (3e0ce76e) replaced [[bigint-digits-inline-cell-storage]]: Storing BigInt digit data inline in the GC cell meant speculative type confusion could use a BigInt cell as an arbitrary pointer source; moving digits to Gigacage::Primitive-allocated memory limits attacker-controlled pointer values to within the gigacage range even if they can confuse the type system. (sourced)
- 2026-02-07 (dbc50284) replaced [[jsbigint-auxiliary-digit-storage]]: JSBigInt digits became immutable, length-known payloads, making trailing cell storage sufficient and cheaper for direct access than a separately allocated caged digit buffer. (sourced)
- 2026-02-08 (304cf071) replaced [[jsbigint-result-normalization-by-righttrim]]: BigInt arithmetic should normalize spans in temporary Vector storage and allocate the final JSBigInt once, rather than allocate a JSBigInt as a temporary buffer and then right-trim it. (code)
