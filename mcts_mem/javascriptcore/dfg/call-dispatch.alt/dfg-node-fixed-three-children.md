- Every DFG node had three fixed child fields.
- Node construction accepted up to three NodeIndex child operands, with no vararg child area.

## Moves

- 2011-07-06 (2ea36dbf) replaced by [[call-dispatch]]: Call nodes require a variable number of child operands (one per argument) which cannot be expressed in the fixed three-child (child1/child2/child3) node representation; the new design adds a NodeHasVarArgs flag and a union of fixed-children and variable-children (firstChild+numChildren index into a separate child array) to handle both cases in one Node type. (code)
