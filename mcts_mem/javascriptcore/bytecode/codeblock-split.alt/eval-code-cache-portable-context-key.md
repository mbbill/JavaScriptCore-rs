- CacheKey combined eval source with SourceCodeFlags derived from DerivedContextType, EvalContextType, and isArrowFunctionContext.
- isCacheable rejected strict mode, long sources, and scopes other than global lexical environment, function-name scope object, or var scope.

## Moves

- 2016-11-04 (431c607c) replaced by [[codeblock-split]]: Keying cached eval code by call-site location avoids relocating eval code across different surrounding scopes, so strict and lexical-scope evals can be cached without the old scope-shape exclusions. (code)
