- FTL lowered DFG SSA graphs into LLVM IR.
- LLVM optimization, instruction selection, register allocation, and stackmap section parsing were part of the FTL backend path.

## Moves

- 2016-01-25 (8ecc9ff6) replaced by [[ftl]]: The X86_64/Mac FTL backend switched from LLVM to B3 because B3 was performance-neutral on major tests while cutting FTL compile time by about 5x-10x and avoiding stackmap section parsing. (sourced)
