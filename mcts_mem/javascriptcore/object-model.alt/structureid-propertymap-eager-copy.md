- Each add-property transition copied the predecessor StructureID property table.
- Every StructureID in a transition chain owned a populated PropertyMap.
- StructureID lookup assumed the property table was already materialized.

## Moves

- 2008-11-14 (6d9f96e8) replaced by [[object-model]]: Every addPropertyTransition always copied the PropertyMap into the new StructureID; the new design steals the PropertyMap from the predecessor and reconstructs it on demand via materializePropertyMap(), saving ~15MB on a 30-page Membuster test. (sourced)
