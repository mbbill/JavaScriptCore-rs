- IPInt alignment was emitted through hand-written architecture-specific balign snippets.
- Alignment padding and the label it was intended to align were separate emitted constructs.

## Moves

- 2024-04-05 (75713dba) replaced by [[builtins-codegen]]: Offlineasm labels gained an alignment operand and C++-referenced validation labels because hand-emitted .balign padding could not make the label global/referenced, and the commit message says LTO linkers removed unreferenced labels. (sourced)
