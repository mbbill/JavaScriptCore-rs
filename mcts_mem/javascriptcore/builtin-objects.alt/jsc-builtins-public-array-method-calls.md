- Self-hosted builtins called public Array.prototype methods for internal list manipulation.
- User replacement of public push or shift could affect internal builtin algorithms.

## Moves

- 2015-12-10 (58844ff4) replaced by [[builtin-objects]]: Builtins use private @push/@shift so internal array operations cannot be disrupted by user scripts overriding public Array.prototype methods. (sourced)
