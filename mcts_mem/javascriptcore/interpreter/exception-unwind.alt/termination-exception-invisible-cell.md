- TerminationException uses an internal value invisible to JavaScript and C++ string conversion clients.
- C++ clients catching termination at the outermost boundary cannot convert the exception value to text.

## Moves

- 2021-04-13 (57e02476) replaced by [[exception-unwind]]: A JSString termination exception value can be converted to a string by C++ clients that catch termination at the outermost point, while the value remains invisible to JavaScript because TerminationException cannot be caught. (sourced)
