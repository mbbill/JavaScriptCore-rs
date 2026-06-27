- JSActivation stores registers in malloc-owned backing storage.
- SharedSymbolTable lifetime is handled by refcounting.
- Activation objects need destruction/finalization to release backing resources.

## Moves

- 2012-08-26 (36e0a1ea) replaced by [[scope-chain-and-activation]]: Switching JSActivation's register backing store from malloc (requiring a destructor) to GC CopiedSpace and its SharedSymbolTable from ref-counting to GC MarkedSpace eliminates the need for a destructor on activation objects, enabling GC to collect them without finalization overhead. (code)
