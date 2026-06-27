- `JSStringRef` is a reinterpret-cast alias for an internal `UString::Rep`.
- Public string lifetime and identifier-table lifetime are coupled through the internal string representation.

## Moves

- 2008-08-15 (f7218e4f) replaced by [[c-api]]: JSStringRef was a raw reinterpret_cast alias for UString::Rep*, which caused the public API string to be silently linked into an internal identifier table, breaking the implicit API contract that strings are context-free. (sourced)
