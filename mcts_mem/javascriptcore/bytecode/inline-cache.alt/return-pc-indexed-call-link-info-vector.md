- CodeBlock kept CallLinkInfo objects in Vector<CallLinkInfo> m_callLinkInfos.
- Slow paths recovered the call site by binary-searching m_callLinkInfos on the machine-code return PC via CodeBlock::getCallLinkInfo(ReturnAddressPtr).
- Bytecode/profiling lookup binary-searched the same vector by CodeOrigin bytecodeIndex via CodeBlock::getCallLinkInfo(unsigned).
- DFG::JITCode kept a slowPathCalls vector whose indices shadowed the CallLinkInfo vector.

## Moves

- 2014-03-23 (a67a45ec) replaced by [[inline-cache]]: Passing CallLinkInfo* directly to call-link slow paths lets call inline caches be planted inside other inline caches or stubs without requiring association with an op_call/op_construct return PC and a CodeBlock vector index. (sourced)
