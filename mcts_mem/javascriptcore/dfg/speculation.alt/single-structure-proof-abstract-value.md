- AbstractValue carried separate current and unclobbered StructureSet fields without modelling future watchpoint-bounded possibilities.
- Clobbering could erase current structure proof while entry validation retained a separate set.

## Moves

- 2012-08-20 (7b747c92) replaced by [[speculation]]: The old representation conflated structures proven right now by executed checks with structures bounded for future side effects by transition watchpoints, so it could not express the watchpoint-dependent future proof needed for sound watchpoint use and CheckStructure strength reduction. (code)
