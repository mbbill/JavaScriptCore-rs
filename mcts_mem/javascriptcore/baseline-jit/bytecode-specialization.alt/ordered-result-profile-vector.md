- Result profiles are appended in bytecode order during baseline compilation.
- Profile lookup searches an ordered vector by bytecode offset.

## Moves

- 2016-01-05 (6ebc10cf) replaced by [[bytecode-specialization]]: ResultProfiles needed to be creatable at any time, including from slow paths during runtime execution, instead of only in bytecode order at baseline compilation time. (code)
