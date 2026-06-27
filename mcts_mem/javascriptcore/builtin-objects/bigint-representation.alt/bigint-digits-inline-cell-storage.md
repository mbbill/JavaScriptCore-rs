- JSBigInt stored its digit words inline inside a variable-sized GC cell.
- The digit payload occupied pointer-sized words reachable through the object cell layout.

## Moves

- 2019-11-18 (3e0ce76e) replaced by [[bigint-representation]]: Storing BigInt digit data inline in the GC cell meant speculative type confusion could use a BigInt cell as an arbitrary pointer source; moving digits to Gigacage::Primitive-allocated memory limits attacker-controlled pointer values to within the gigacage range even if they can confuse the type system. (sourced)
