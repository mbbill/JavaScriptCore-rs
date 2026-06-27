- DFG call linking hard-coded CodeForCall.
- Virtual call operations and unlinking relinked only to call targets, not construct targets.

## Moves

- 2011-07-13 (36f0c9c8) replaced by [[call-dispatch]]: The original DFG call dispatch path hardcoded CodeForCall throughout (dfgLinkCall, operationVirtualCall, operationLinkCall) and had no path to select construct code blocks; op_construct requires CodeForConstruct selection which the call-only path could not express — an expressivity wall — so Call and Construct were unified under CodeSpecializationKind. (code)
