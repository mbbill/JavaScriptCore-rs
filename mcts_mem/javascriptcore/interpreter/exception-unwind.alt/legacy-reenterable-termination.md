- VM termination leaves legacy clients able to re-enter after a TerminationException.
- Worker clients rely on every outer catch site to forbid further execution explicitly.

## Moves

- 2021-12-11 (f33b362a) replaced by [[exception-unwind]]: Clients missing TerminationException catch sites made worker termination error-prone, so workers opt into having VM::throwTerminationException set executionForbidden immediately while legacy clients can still re-enter after termination. (sourced)
