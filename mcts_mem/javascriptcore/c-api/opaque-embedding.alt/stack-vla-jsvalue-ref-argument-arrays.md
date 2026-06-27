- Objective-C call, construct, and invoke paths store converted `JSValueRef` arguments in stack variable-length arrays.
- Overflow argument storage is not represented as a GC-rooted API container.

## Moves

- 2020-03-15 (003d0374) replaced by [[opaque-embedding]]: Variable-length JSValueRef argument arrays let user-controlled argument counts consume C++ stack space and do not give the GC an explicit root list for spilled API values, so API argument storage moved to a stack-only object with inline capacity, caged heap spillover, and heap marking registration. (code)
