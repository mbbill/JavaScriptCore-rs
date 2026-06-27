- PolymorphicStubInfo::base (Structure*)
- PolymorphicStubInfo::proto (Structure*)
- PolymorphicStubInfo::cachedOffset (int)
- PolymorphicStubInfo::stubRoutine (void*)
- PolymorphicAccessStructureList constructor takes (Structure* firstBase, Structure* firstProto, int cachedOffset, void* stubRoutine)
- cti_op_get_by_id_proto_list generates only proto stubs

## Moves

- 2008-11-25 (a74fd2aa) replaced by [[inline-cache]]: The old PolymorphicStubInfo held a single Structure* proto field and could only record direct-prototype accesses; the new version uses a union { Structure* proto; StructureChain* chain } plus an isChain bit, enabling a single polymorphic list to hold both proto and proto-chain stubs and yielding ~2% on v8 benchmarks. (sourced)
