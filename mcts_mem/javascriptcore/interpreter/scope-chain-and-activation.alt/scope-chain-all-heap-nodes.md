- Every ScopeChain push allocates a heap ScopeChainNode.
- Copy construction shares existing heap nodes through refcounts.
- Single-push function-call scopes pay allocation cost even when the chain never escapes.

## Moves

- 2008-01-01 (3480a600) replaced by [[scope-chain-and-activation]]: Every ScopeChain::push() allocated a heap ScopeChainNode even for transient pushes during function calls; embedding the top node directly in the ScopeChain object (m_initialTopNode) defers heap allocation to moveToHeap() called only on copy/assign, eliminating the allocation on the common single-push call path and yielding 1.019x SunSpider. (sourced)
