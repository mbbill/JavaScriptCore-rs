- Module environments use the global object itself as their upper scope.
- Module bytecode bypasses script global-property watch and frequent-exit paths.
- Module scope access does not share the same global lexical environment parent as script code.

## Moves

- 2020-04-09 (0ec50c9b) replaced by [[global-object]]: Making JSGlobalLexicalEnvironment the module environment's upper scope lets module bytecode use the same global-property watching and exit-site tallying as scripts. (code)
