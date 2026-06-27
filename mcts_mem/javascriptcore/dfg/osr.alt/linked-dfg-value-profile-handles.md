- OSR exit value reporting stored concrete profile pointers or CodeBlock-backed lazy profile handles.
- Exit code wrote directly through the stored profile pointer.

## Moves

- 2022-04-29 (6eaf4a53) replaced by [[osr]]: Unlinked DFG cannot store CodeBlock* and concrete profile pointers in OSR-exit metadata, so value-profile reporting now stores a CodeOrigin, kind, and operand and resolves the concrete profile when exit code is generated. (sourced)
