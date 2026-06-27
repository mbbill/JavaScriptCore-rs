- Vector search returns an iterator into the vector.
- Callers subtract `begin()` to convert the found iterator to an index.
- The not-found sentinel is `end()` rather than `WTF::notFound`.

## Moves

- 2008-07-29 (5059fa56) replaced by [[containers]]: Returning an iterator from Vector::find was unnatural because Vector is index-oriented; callers had to subtract begin() to get the index, and iterator invalidation rules are confusing; returning a size_t index with sentinel WTF::notFound matches how Vector is used throughout JSC. (sourced)
