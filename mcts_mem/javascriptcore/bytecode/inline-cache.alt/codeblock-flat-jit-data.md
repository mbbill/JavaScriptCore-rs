- Bag<StructureStubInfo> m_stubInfos directly in CodeBlock
- Bag<JITAddIC/JITMulIC/JITSubIC/JITNegIC> directly in CodeBlock
- Bag<ByValInfo> m_byValInfos directly in CodeBlock
- Bag<CallLinkInfo> m_callLinkInfos directly in CodeBlock
- stubInfoBegin/stubInfoEnd iterators on CodeBlock
- callLinkInfosBegin/callLinkInfosEnd iterators on CodeBlock
- jitCodeMap() const accessor directly on CodeBlock
- numberOfRareCaseProfiles() directly on CodeBlock

## Moves

- 2019-02-02 (a91eff59) replaced by [[inline-cache]]: JIT-only data (stub infos, math ICs, call link infos, rare-case profiles, incoming call lists, PC-to-origin map, JIT code map) was always allocated inside every CodeBlock even though only a small fraction of CodeBlocks ever reach JIT compilation; moving them into a lazily-created CodeBlock::JITData reduces CodeBlock size from 512 to 352 bytes and yields 1.1% RAMification improvement. (sourced)
