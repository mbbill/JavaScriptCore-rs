- Interpreter instances are reference-counted objects held by external owners.
- The interpreter marks its global object during GC.
- The collector gives live interpreters special treatment outside normal object graph ownership.

## Moves

- 2007-12-01 (8104e1bb) replaced by [[global-object]]: Circular mark-graph and ref-count complexity arose because Interpreter marked JSGlobalObject and was itself ref-counted by external holders; reversing ownership so JSGlobalObject (a GC object) directly owns Interpreter (via std::auto_ptr) lets GC protect both through a single gcProtect(globalObject) call, eliminating special GC treatment for Interpreters. (sourced)
