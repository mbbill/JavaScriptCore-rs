- Interpreter input access is mediated by a runtime character-size discriminator.
- CharAccess stores either LChar or UChar pointers and branches per indexed read.
- The public interpreter entry point takes a string rather than raw width-specialized buffers.

## Moves

- 2012-03-29 (f3df9bfc) replaced by [[interpreter-dispatch]]: We should be able to call to the interpreter after having already checked the character type, without having to re-package the character pointer back up into a string! (sourced)
