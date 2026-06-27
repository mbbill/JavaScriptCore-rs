- Stack frames are exposed through a classic begin/end iterator pair.
- Callers compare against a sentinel and advance the iterator manually.
- Iterator values can be stored outside the traversal loop.

## Moves

- 2013-09-04 (0441e5cb) replaced by [[interpreter]]: The classic begin/end/operator++ iterator interface exposed StackIterator as a value that callers could store and manipulate outside the iteration loop; replacing it with a typed-functor callback (operator() returning Status) confines iterator lifetime to the iterate() call and allows early termination via Done/Continue return values without exposing iterator state. (sourced)
