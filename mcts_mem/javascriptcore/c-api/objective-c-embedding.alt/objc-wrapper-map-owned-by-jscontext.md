- The Objective-C wrapper map is owned by the `JSContext` wrapper object.
- Exported-class wrapper caches disappear when the Objective-C context wrapper is deallocated.

## Moves

- 2017-05-22 (7b06ba41) replaced by [[objective-c-embedding]]: Objective-C wrapper caches need to survive JSContext wrapper object deallocation for the same JSGlobalContextRef, so ownership moved from JSContext to JSGlobalObject and per-call JSContext is passed explicitly. (code)
