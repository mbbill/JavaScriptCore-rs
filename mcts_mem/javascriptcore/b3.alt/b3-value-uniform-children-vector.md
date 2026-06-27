- B3::Value stored children in a fixed Vector<Value*, 3> member.
- Per-subclass clone implementations mostly reallocated the same subclass.
- All opcodes shared one uniform child accessor path.

## Moves

- 2019-04-15 (402606bd) replaced by [[b3]]: The fixed Vector<Value*, 3> m_children (40 bytes) allocated the same footprint regardless of how many children a Value actually used; replacing it with a kind-tagged inline storage (0/1/2/3 pointers allocated immediately after the object, or a VarArgs Vector) halves memory for zero-child Values such as Const64/Const32/Nop/Identity and reduces Add from 72 to 48 bytes. (sourced)
