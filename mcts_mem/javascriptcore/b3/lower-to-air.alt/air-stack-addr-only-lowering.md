- Stack argument lowering returned only encodable Air Addr forms.
- Unencodable stack addresses were fatal after logging offset, width, and code.
- The implementation assumed giant stack frames were rare and did not reserve a scratch temporary for oversized offsets.

## Moves

- 2017-05-06 (98655afe) replaced by [[lower-to-air]]: Stack arguments were replaced with an ExtendedOffsetAddr-capable lowering because WebAssembly can produce ARM64 stack frames whose FP/SP offsets do not fit normal Air Addr encodings while patchpoints and stackmaps still need logical FP/SP-relative offsets. (sourced)
