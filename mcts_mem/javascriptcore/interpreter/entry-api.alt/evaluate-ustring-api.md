- Interpreter::evaluate receives source code as UString values.
- Callers construct a UString even when the parser could consume raw characters and length.

## Moves

- 2005-12-16 (84892347) replaced by [[entry-api]]: Interpreter::evaluate() parameter changed from UString to UChar*+int to avoid constructing a UString object solely for parsing, which caused unnecessary allocation and copying of the source text. (sourced)
