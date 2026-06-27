- Call IC relinking updates generated call instructions directly.

## Moves

- 2021-05-18 (3de6f842) replaced by [[inline-caches]]: Data Call ICs load the callee and code pointer from CallLinkInfo so relinking can update fields in CallLinkInfo instead of repatching generated JIT instructions. (code)
