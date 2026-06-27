- RegExp literals are loaded as constant RegExpObject JSValues.
- CodeBlock does not keep a regexp allocation table for literal re-evaluation.

## Moves

- 2010-05-10 (af9962ba) replaced by [[bytecode-specialization]]: r57955 replaced op_new_regexp with emitLoad(RegExpObject as JSValue constant) to cache regexp instances, but the spec requires each regexp literal evaluation to produce a new object (ES3/ES5 differ but the cached approach caused test failures), so the caching was rolled back. (sourced)
