- Every async function is parsed as a wrapper plus a separate async body function.
- The wrapper allocates generator infrastructure even when the body contains no lexical await.
- Promise settlement for no-await async functions still goes through the general suspend/resume shape.

## Moves

- 2025-12-29 (f42c3630) replaced by [[promises-and-microtasks]]: Async function bodies with no lexical await no longer need a separate body function or generator because there is no suspend/resume point, so the wrapper can inline the body and directly settle the promise. (sourced)
