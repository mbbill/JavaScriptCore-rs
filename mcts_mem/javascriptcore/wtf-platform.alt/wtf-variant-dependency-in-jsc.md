- JSC clients include WTF's in-tree Variant wrapper for discriminated-union support.
- Variant support is treated as a WTF portability abstraction rather than a direct standard-library dependency.

## Moves

- 2021-10-14 (657f6fe5) replaced by [[wtf-platform]]: JSC dropped the in-tree WTF Variant wrapper in favor of the standard library variant dependency. (code)
