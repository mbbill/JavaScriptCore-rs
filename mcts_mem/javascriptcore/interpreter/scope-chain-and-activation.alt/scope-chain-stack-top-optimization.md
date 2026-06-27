- ScopeChain keeps its first pushed node inline on the stack/object and promotes it to heap on sharing.
- Copying or assigning another chain calls moveToHeap before sharing refcounted tails.
- Release walks heap nodes while specially skipping the inline initial node.

## Moves

- 2008-01-01 (dac291cc) replaced by [[scope-chain-and-activation]]: The stack-allocated first-node optimization (m_initialTopNode + moveToHeap()) was reverted because it was causing correctness failures in practice ("breaking the world"), making heap-only ref-counted nodes the surviving form. (sourced)
