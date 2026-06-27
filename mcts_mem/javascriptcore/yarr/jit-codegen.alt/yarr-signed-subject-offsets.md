- ByteTerm input positions and YARR generator offsets are signed integers.
- Generated character reads encode negative displacement directly when possible.
- Large quantified patterns can exceed signed displacement ranges.

## Moves

- 2016-07-14 (418d7655) replaced by [[jit-codegen]]: YARR switched from signed relative offsets to Checked<unsigned> negative-character distances because large quantified regular expressions can exceed signed displacement ranges, and the JIT now biases the base register when BaseIndex cannot encode the negative offset. (code)
