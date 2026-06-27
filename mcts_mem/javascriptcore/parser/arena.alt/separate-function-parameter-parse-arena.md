- Function parameter nodes and destructuring patterns were retained outside the function-body arena.
- A function source range began at the body rather than covering the parameter list needed for reparsing.

## Moves

- 2015-07-17 (7a073296) replaced by [[arena]]: A function's parameters are now parsed in the same arena as the function itself so destructuring AST nodes and FunctionParameters can be arena allocated and ES6 default parameter values can be implemented sanely. (sourced)
