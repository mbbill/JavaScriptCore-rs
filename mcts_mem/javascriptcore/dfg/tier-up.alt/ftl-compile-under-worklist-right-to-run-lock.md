- Worklist threads held the right-to-run mutex across Plan::compileInThread and FTL compilation.
- Active in-progress Graph state was not separately registered as scannable GC state.

## Moves

- 2014-02-10 (11ca79ff) replaced by [[tier-up]]: FTL compilation could unlock the worklist rightToRun mutex during long LLVM initialization/optimization/backend work if the in-progress DFG graph registered itself as scannable GC state. (code)
