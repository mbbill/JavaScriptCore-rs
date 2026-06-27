- Each polymorphic DataIC call site generates its own BinarySwitch stub code.

## Moves

- 2024-05-04 (13455c7a) replaced by [[unlinked-code-sharing]]: A shared polymorphic thunk walking CallSlot trailing data replaced per-call-site BinarySwitch stub generation for DataIC so Baseline polymorphic calls no longer allocate new JIT code. (code)
