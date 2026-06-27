- Each wasm constant expression appends a new B3 constant at its use site.
- Only type zero values are shared; nonzero integer and floating constants are reconstructed per use.

## Moves

- 2017-03-30 (019f5194) replaced by [[omg-tier]]: Wasm B3 constants were pooled per function and inserted into the root basic block so repeated constants share one B3 value instead of being emitted at each use site. (code)
