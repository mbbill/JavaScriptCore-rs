- Empty buckets are initialized by copy-constructing the traits' empty value.
- Deleted-value checks create, destroy, and reconstruct value objects around the empty value.
- Noncopyable value types cannot be represented cleanly in buckets.

## Moves

- 2011-11-09 (b3673a3b) replaced by [[containers]]: HashTable needed an empty-bucket path that does not copy the empty value so noncopyable value types such as OwnPtr can be used as HashMap values. (sourced)
