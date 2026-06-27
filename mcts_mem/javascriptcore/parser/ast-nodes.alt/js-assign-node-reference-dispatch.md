- Assignment used one generic assignment node that evaluated the left side into a materialized Reference object.
- Target-kind dispatch happened at runtime through the Reference rather than through node type.

## Moves

- 2005-08-11 (80c72167) replaced by [[ast-nodes]]: The single AssignNode called left->evaluateReference() which required every lvalue node to materialise a Reference object and then get/set through it, preventing static dispatch to the correct property-store path and blocking future writable-PropertySlot optimisation; typed nodes (AssignResolveNode, AssignDotNode, AssignBracketNode) know their target kind at compile time and call the appropriate put/set directly. (sourced)
