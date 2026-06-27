- This-TDZ behavior was represented as a parser mode flag separate from the full source-code context.
- Cache keys could not distinguish every derived, eval, constructor, and arrow context that affects reuse.

## Moves

- 2016-05-24 (4127a66a) replaced by [[lazy-parse-cache]]: ConstructorKind, DerivedContextType, EvalContextType, and arrow-context flags carry the necessary this-TDZ and cache-identity information, while the old ThisTDZMode/cache flags could not distinguish all contexts needed for safe reuse. (code)
