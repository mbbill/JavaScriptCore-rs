- VM recursion checks use one soft stack limit for JS entry, host entry, parser, RegExp, and internal VM work.
- Callers optionally pass a needed-stack margin but share the same reserve policy.

## Moves

- 2016-07-13 (5a03dc32) replaced by [[entry-api]]: JSC split recursion checks into normal and soft stack limits because host/VM code with known stack usage can use the smaller guaranteed reserve while JS entry points and code that may call arbitrary JS need the more conservative soft reserve. (sourced)
