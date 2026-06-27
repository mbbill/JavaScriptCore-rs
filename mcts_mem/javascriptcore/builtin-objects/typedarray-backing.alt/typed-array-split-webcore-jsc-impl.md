- Typed array implementation was split between WebCore and jsc-shell variants.
- Construction used multiple JS objects, weak handles, and malloc allocations while native views tracked neutering.

## Moves

- 2013-08-15 (93a48aa9) replaced by [[typedarray-backing]]: Old design split typed array implementation between WebCore and jsc-shell (two incompatible versions), made arrays invisible to JIT, required 7 allocations per array (two JS objects, two GC weak handles, three malloc), and tracked native views rather than JS wrappers for neutering — making the common single-buffer/single-view case pay for a multi-view data structure. (sourced)
