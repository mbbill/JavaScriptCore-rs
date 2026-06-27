- Each add-property StructureID transition copied the full predecessor property table.
- All shapes in a transition chain held materialized property maps.
- There was no transition-to-existing-structure fast path that could reconstruct a map on demand.

## Moves

- 2008-11-14 (6d9f96e8) replaced by [[structure-shapes]]: Every addPropertyTransition always copied the PropertyMap into the new StructureID; the new design steals the PropertyMap from the predecessor and reconstructs it on demand via materializePropertyMap(), saving ~15MB on a 30-page Membuster test. (sourced)
