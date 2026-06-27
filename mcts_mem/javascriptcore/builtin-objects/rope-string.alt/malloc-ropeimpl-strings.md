- Rope fibers lived in malloc-allocated RopeImpl objects outside the JS heap.
- JSString destructors manually dereferenced rope fibers and nested rope storage.

## Moves

- 2011-10-19 (c172563e) replaced by [[rope-string]]: The new GC-backed rope representation was chosen because it gave a ~1% SunSpider speedup and removed one cause for strings having C++ destructors. (sourced)
