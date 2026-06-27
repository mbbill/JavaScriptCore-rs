- VM exception state stores only the thrown JSValue.
- Captured throw stack and debugger notification state are not part of a GC object tied to the exception.
- Rethrows can be observed as new throw events by debugger machinery.

## Moves

- 2015-06-05 (cc4c6bff) replaced by [[exception-unwind]]: Wrapping the thrown JSValue, captured stack, and debugger notification state in a GC object lets rethrows preserve the original throw stack and lets finally/synthesized-finally rethrows avoid being treated as new uncaught debugger events. (sourced)
