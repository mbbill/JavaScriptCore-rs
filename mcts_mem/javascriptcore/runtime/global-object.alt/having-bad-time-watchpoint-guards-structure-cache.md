- Structure-cache users guard cached structures by watching the global object's having-a-bad-time state.
- Cache clearing is coupled directly to firing the having-a-bad-time watchpoint.
- A cache-owning global is assumed to cover all prototype-chain bad-time dependencies in cached structures.

## Moves

- 2022-05-22 (fd038f44) replaced by [[global-object]]: A StructureCache can contain Structures whose prototype chains involve globals going bad even when the cache-owning JSGlobalObject is not itself having a bad time, so DFG must guard cached object structures with a cache-clear watchpoint rather than only the global's having-a-bad-time watchpoint. (code)
