- Function bodies may be syntax-checked first and fully parsed later from the same source range.
- Lazy cache items preserve scope facts needed for later compilation, including used variables, strictness, eval/import-meta use, super binding, parameters, and end-token position.
- Source-code cache keys include parse context flags that affect safe reuse.
- Reparse paths restore declaration and capture metadata instead of deriving all scope information after AST construction.

## Facts

- 2010-09-18 (b1e5b54b) rationale: Reparsing a function body sees the body without its enclosing declaration, so parameter names must be supplied to avoid misclassifying parameters as free variables. (sourced)
- 2012-05-07 (1d18036b) pitfall: Centralizing parser feature tracking in scope flags was intended as refactoring but changed behavior enough to break websites including qq.com. (sourced)
- 2017-01-20 (7d31ab6a) pitfall: Base-class constructors that use super property access still need home-object metadata even without an extends clause; lazy metadata must preserve actual super-property use. (sourced)
- 2018-11-09 (d280833e) pitfall: Function-constructor cache keys must include the parameter/body boundary, or different parameter sections with the same body can reuse a stale executable. (code)
- 2020-01-30 (da0d55fc) measurement: `SourceProviderCacheItem` used-variable storage was large enough in RAMification tests that its tail array switched from raw pointers to packed pointers. (sourced)

## Moves

- 2015-03-17 (225f3e93) replaced [[parser-combined-builtin-strictness-mode]]: Builtin-ness and strictness had to be represented independently because builtin functions use strict mode while still needing builtin lexing and cache separation. (sourced)
- 2016-05-24 (4127a66a) replaced [[this-tdz-mode-parse-flag]]: ConstructorKind, DerivedContextType, EvalContextType, and arrow-context flags carry the necessary this-TDZ and cache-identity information, while the old ThisTDZMode/cache flags could not distinguish all contexts needed for safe reuse. (code)
