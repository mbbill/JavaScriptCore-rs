- The function top level is represented by an empty parser control stack.
- End-of-function return handling is a special case separate from ordinary block branch targets.

## Moves

- 2016-12-22 (e8254436) replaced by [[omg-tier]]: An explicit TopLevel BlockType replaced the parser's empty-control-stack end-of-function special case because top-level return semantics then share the same branch target representation as ordinary blocks. (code)
