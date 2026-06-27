- Each object stored properties as a singly linked list of name, value, attribute, and next pointer records.
- get, hasProperty, and deleteProperty searched the per-object list linearly.
- Property enumeration walked the same linked storage representation.

## Moves

- 2002-03-22 (9491afaa) replaced by [[property-storage]]: Property storage changed from a singly-linked list (O(n) lookup) to an AVL balanced binary search tree (O(log n) lookup/insert/delete) to improve performance for objects with many properties. (code)
