- Bytecode parsing built Phi nodes and predecessor links while parsing each block.
- Later phases had to preserve or rebuild variables-at-head/tail Phi links after any CFG mutation.

## Moves

- 2013-02-09 (39a8f3eb) replaced by [[dfg]]: The old design built and maintained Phi data-flow links during bytecode parsing and required every subsequent phase to preserve them or redo the work itself, making it impossible to freely restructure CFG or data flow in phases; the new design introduces two explicit graph forms (LoadStore: implicit data flow, suitable for CFG transforms/CSE; ThreadedCPS: explicit linked Phi network, suitable for CFA/regalloc) and a dedicated CPSRethreadingPhase that any phase can invoke after dethreading, decoupling phase correctness from Phi maintenance. (sourced)
