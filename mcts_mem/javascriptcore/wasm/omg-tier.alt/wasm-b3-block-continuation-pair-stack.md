- The B3 wasm parser control stack stores only a continuation block and optional stack variables for each control entry.
- Loop backedges and arbitrary branch-result unification are not represented in the control frame.

## Moves

- 2016-09-07 (92cad735) replaced by [[omg-tier]]: Loops and branches require control frames to distinguish a block continuation from a loop backedge target and to unify branch result values at arbitrary control levels. (code)
