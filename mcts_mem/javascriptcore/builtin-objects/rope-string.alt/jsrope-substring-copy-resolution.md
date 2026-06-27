- Resolving a substring rope copied the requested bytes into a new StringImpl.
- The resolved substring did not share the parent string's buffer.

## Moves

- 2016-02-18 (fcee787a) replaced by [[rope-string]]: Resolving substring ropes by sharing the parent StringImpl was chosen over copying bytes, trading possible parent-string lifetime extension for less GC and lower peak memory on large diff-viewer pages. (sourced)
