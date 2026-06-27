- Greedy register allocation was disabled for code using SIMD.
- SIMD functions fell back to graph coloring or linear scan.
- The greedy allocator asserted that SIMD code was not using it.

## Moves

- 2025-04-21 (171b7a8e) replaced by [[register-allocation]]: SIMD code stopped falling back to graph coloring or linear scan because greedy allocation learned to distinguish high-64-only FP clobbers from full-width FP conflicts. (code)
