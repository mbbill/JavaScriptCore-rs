- PrototypeStructureList with ProtoStubInfo holding base+proto+cachedOffset+stubRoutine
- only used by op_get_by_id_proto_list
- derefStructures iterated inline in CodeBlock::derefStructures

## Moves

- 2008-11-22 (d9cbee6f) replaced by [[inline-cache]]: Extending inline-cache polymorphism to cover self-slot accesses (not only prototype chain) required the structure-list type to be shared between op_get_by_id_self_list and op_get_by_id_proto_list, so the prototype-only PrototypeStructureList was generalized to PolymorphicAccessStructureList that can record either kind. (sourced)
