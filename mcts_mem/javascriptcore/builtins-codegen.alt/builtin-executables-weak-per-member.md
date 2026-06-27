- BuiltinExecutables stored one Weak<UnlinkedFunctionExecutable> member per builtin.
- Each builtin also carried a separate SourceCode member.

## Moves

- 2019-03-11 (12d53564) replaced by [[builtins-codegen]]: Each Weak<UnlinkedFunctionExecutable> requires a WeakBlock (256 bytes) for GC bookkeeping; with 203 builtins plus 203 SourceCode members the old design consumed ~4KB of WeakBlocks plus 24*203=4KB of SourceCode fields, whereas a raw pointer array plus finalizeUnconditionally() scan replicates JSWeakSet behavior with no WeakBlock overhead. (sourced)
