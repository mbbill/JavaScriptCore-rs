- GetterSetterAccessCase owns CallLinkInfo for emitted getter/setter stub code.

## Moves

- 2019-12-11 (b26b45cc) replaced by [[inline-caches]]: GetterSetterAccessCase owned CallLinkInfo via unique_ptr and could be destroyed (on StructureStubInfo reset) while emitted stub code was still live on the stack and still held a pointer to that CallLinkInfo; moving ownership to MarkingGCAwareJITStubRoutine (via Bag<CallLinkInfo>) ensures CallLinkInfo lives exactly as long as the generated code. (code)
