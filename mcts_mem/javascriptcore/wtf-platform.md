- WTF is JavaScriptCore's cross-platform substrate for allocation, strings, containers, ownership wrappers, random numbers, Unicode, thread/run-loop primitives, and low-level portability.
- Platform differences are hidden behind narrow WTF abstractions, keeping JSC code from encoding pthread, Windows, GLib, Qt, ICU, or Darwin details directly.
- Generic helpers avoid adding object size or global-initializer cost to client classes; fast allocation, noncopyability, and ref-count ownership are injected through zero-footprint templates or macros where inheritance would change layout.
- Standard-library facilities replace WTF wrappers only when the project can depend on the standard facility directly.

- [[allocation]]
- [[containers]]
- [[randomness]]
- [[ref-counted-ownership]]
- [[threading]]
- [[unicode]]

## Moves

- 2010-09-27 (3af4b634) replaced [[noncopyable-base-class-inheritance]]: The Itanium C++ ABI forbids two empty base classes of the same type at the same offset, so inheriting both Noncopyable and FastAllocBase could silently inflate object sizes (String grew by sizeof(void*)); a macro avoids any base-class footprint entirely. (sourced)
- 2010-10-18 (bb218350) replaced [[fast-alloc-base-class]]: Inheriting from FastAllocBase could increase object sizes due to C++ base-class layout rules, causing memory regressions (investigated in bug #33896); a macro that injects operator new/delete directly into the target class avoids any size increase while delivering the same fast-malloc routing. (sourced)
- 2011-09-02 (5a9627ad) replaced [[decimal-number-dtoa]]: Old in-tree DecimalNumber/numberToString replaced by google double-conversion library (code.google.com/p/double-conversion) because the new library is faster for number-to-string conversion. (sourced)
- 2021-10-14 (657f6fe5) replaced [[wtf-variant-dependency-in-jsc]]: JSC dropped the in-tree WTF Variant wrapper in favor of the standard library variant dependency. (code)
