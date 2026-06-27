- OP_CIRC handles both single-line and multiline ^ with runtime md.multiline check
- OP_DOLL handles both $ modes with runtime md.multiline check
- jsRegExpExecute retries even anchored patterns

## Moves

- 2008-03-31 (2fb55858) replaced by [[yarr]]: The unified CIRC/DOLL opcodes checked the multiline flag at match time on every execution, preventing the anchored-regex optimization in jsRegExpExecute (which requires knowing at compile time whether the pattern is anchored); splitting into separate compile-time opcodes (CIRC=single-mode, BOL=multiline) allows branchNeedsLineStart to detect anchoring statically and skip the retry loop for anchored patterns. (code)
