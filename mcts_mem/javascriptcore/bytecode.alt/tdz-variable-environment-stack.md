- m_TDZStack was Vector<std::pair<VariableEnvironment, TDZCheckOptimization>>.
- pushLexicalScopeInternal only pushed TDZ variables when tdzRequirement == TDZRequirement::UnderTDZ, catch scopes with TDZRequirement::NotUnderTDZ did not add a blocking stack entry.
- pushTDZVariables removed function declarations from the VariableEnvironment before appending it with the optimization flag.
- needsTDZCheck returned true on the first stacked environment containing the identifier, and liftTDZCheckIfPossible removed the identifier when that environment was optimizable.
- getVariablesUnderTDZ unioned every identifier in every stacked VariableEnvironment.

## Moves

- 2016-07-02 (59a7a2d5) replaced by [[bytecode]]: A stack that only stored variables currently under TDZ could not represent intervening lexical scopes whose bindings are known not to need TDZ checks, so TDZ lifting could pass through those scopes to an outer declaration; a per-name necessity map can represent NotNeeded as a blocker alongside Optimize and DoNotOptimize. (code)
