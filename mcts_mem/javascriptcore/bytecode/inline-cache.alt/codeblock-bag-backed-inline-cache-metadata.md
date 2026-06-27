- CodeBlock::JITData stored Bag<StructureStubInfo> and Bag<CallLinkInfo> shared by baseline and optimizing JIT metadata.
- Baseline install allocated each CallLinkInfo and StructureStubInfo by calling Bag::add while walking the JIT constant pool.
- Optimizing JIT cache generators allocated metadata through CodeBlock::addOptimizingStubInfo/addCallLinkInfo.

## Moves

- 2021-10-30 (24dcd913) replaced by [[inline-cache]]: Baseline JIT knows the final counts of StructureStubInfo and CallLinkInfo after compilation, so it can install fixed vectors instead of growing Bags, while DFG/FTL still allocate these records dynamically in DFG::CommonData. (code)
