- PropertyMapHashTable stored a monotonically increasing insertion index in each entry.
- The table tracked lastIndexUsed separately from the entries.
- Enumeration allocated a pointer array and sorted entries by stored index before returning property names.

## Moves

- 2011-03-01 (e635e5f9) replaced by [[property-storage]]: Old PropertyMapHashTable stored an ever-increasing 'index' integer in each PropertyMapEntry and a 'lastIndexUsed' counter in the table; getEnumerablePropertyNames sorted a pointer array by index before returning, incurring an allocation and O(n log n) sort per enumeration; the new PropertyTable class maintains entries in insertion order in the value array itself so ordered_iterator can walk them sequentially without sorting, measured at 0.5-1% sunspider improvement. (sourced)
