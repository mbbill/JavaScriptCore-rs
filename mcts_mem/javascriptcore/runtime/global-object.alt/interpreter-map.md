- A static map from global objects to interpreter instances records all live interpreters.
- Callers recover an interpreter for a global object by querying that map.
- Interpreter construction and destruction maintain the map manually.

## Moves

- 2007-10-25 (7efe5eb6) replaced by [[global-object]]: A static HashMap<JSObject*, Interpreter*> was needed to look up the Interpreter from a global object; introducing JSGlobalObject with an embedded m_interpreter back-pointer eliminates the hash-map lookup and map maintenance at init/destroy time, giving a 0.5% SunSpider speedup and removing the static data structure. (sourced)
