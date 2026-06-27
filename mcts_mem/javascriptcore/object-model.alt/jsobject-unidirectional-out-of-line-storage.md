- JSObject stored named out-of-line properties behind m_outOfLineStorage.
- JSArray stored indexed ArrayStorage separately from named-property storage.
- Growing object storage only grew the named-property side.

## Moves

- 2012-09-13 (0400d283) replaced by [[object-model]]: A single m_butterfly pointer allows named out-of-line properties to be placed to the left and indexed properties to the right of the pointed-to location with no space overhead vs m_outOfLineStorage, enabling all JSObjects (not just JSArray) to have O(1) indexed property access and allowing indexed storage to morph over time. (sourced)
