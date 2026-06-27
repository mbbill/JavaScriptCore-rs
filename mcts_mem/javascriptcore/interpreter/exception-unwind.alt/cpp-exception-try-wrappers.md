- Host function calls are wrapped by tryCall, tryGet, and tryPut layers that catch C++ exceptions.
- DOMFunction and ClassFunc expose exception-catching virtual entrypoints for KJS host bindings.

## Moves

- 2005-07-19 (af8294dc) removed: C++ exception support was removed from JSC; the tryCall/tryGet/tryPut wrapper layer that caught C++ exceptions from KJS host functions became dead code once JSC switched to Completion-based error propagation exclusively. (sourced)
