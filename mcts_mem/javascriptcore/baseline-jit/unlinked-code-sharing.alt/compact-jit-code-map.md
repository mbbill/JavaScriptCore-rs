- Baseline bytecode-to-machine-code maps store delta-compressed bytecode and machine-code offsets.

## Moves

- 2018-04-11 (a033ca46) replaced by [[unlinked-code-sharing]]: Baseline bytecode-to-machine-code maps stopped storing delta-compressed bytecode and machine-code offsets and instead stored CodeLocationLabel entries directly, so OSR exits could retrieve a code label without decoding an offset vector and reconstituting an executable address. (code)
