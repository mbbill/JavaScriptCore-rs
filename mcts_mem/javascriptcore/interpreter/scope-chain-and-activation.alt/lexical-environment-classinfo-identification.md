- Lexical, global lexical, and module environments are identified with class-info dynamic casts plus symbol-table scope type checks.
- Slow paths classify environment kinds through C++ class templates rather than JS cell type.

## Moves

- 2016-03-15 (7e032b27) replaced by [[scope-chain-and-activation]]: Distinct JSTypes for lexical, global lexical, and module environments replaced class-info dynamic casts so scope slow paths and cache setup can identify these scope kinds with cell type checks. (code)
