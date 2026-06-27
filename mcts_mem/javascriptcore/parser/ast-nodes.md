- The AST is a typed node hierarchy that carries source positions, static result-type metadata, and bytecode-facing shape information.
- Node classes specialize when target kind or operator identity changes later compilation behavior.
- Parser-produced lists and recursive trees preserve source order without unbounded native stack growth during traversal.
- Function bodies can appear first as metadata nodes and later expand to full statement trees.

## Facts


## Moves

- 2005-08-11 (80c72167) replaced [[js-assign-node-reference-dispatch]]: The single AssignNode called left->evaluateReference() which required every lvalue node to materialise a Reference object and then get/set through it, preventing static dispatch to the correct property-store path and blocking future writable-PropertySlot optimisation; typed nodes (AssignResolveNode, AssignDotNode, AssignBracketNode) know their target kind at compile time and call the appropriate put/set directly. (sourced)
- 2007-10-23 (a10d7639) replaced [[binary-op-node-unified]]: Unified nodes carrying a runtime oper char/enum field branch on the operator in every evaluate() call; splitting into dedicated classes eliminates those branches and allows each evaluate() to be a direct inlinable implementation, yielding a measured 0.8-1.0% SunSpider speedup even before further optimization. (sourced)
- 2012-09-05 (2dd3aaab) replaced [[separate-prefix-postfix-ast-nodes]]: Four separate node classes per prefix/postfix direction (resolve/bracket/dot/error) held redundant fields and prevented access to m_subscriptHasAssignments when emitting bracket access, which required the unified node with a dispatching emitBytecode. (sourced)
- 2015-08-10 (d44fa932) replaced [[function-body-node-as-statement-node]]: Function metadata can appear in expression context and has no next statement, so modeling it as a StatementNode was the wrong AST type. (sourced)
