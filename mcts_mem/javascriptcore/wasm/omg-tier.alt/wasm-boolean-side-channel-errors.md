- Module parsing, validation, and B3 generation report success as booleans while storing parsed outputs and error strings in side-channel fields.
- Callers synchronize success state with separate error-message accessors.

## Moves

- 2016-12-15 (1ed42e5a) replaced by [[omg-tier]]: Returning Expected results makes parser, validator, and compiler helpers return either their success value or an error together instead of synchronizing boolean failure state with side-channel error strings. (code)
