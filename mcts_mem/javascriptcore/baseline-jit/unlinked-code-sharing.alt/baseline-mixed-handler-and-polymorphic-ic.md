- Baseline ByVal ICs mix Handler IC and polymorphic compile paths depending on observed access shape.

## Moves

- 2024-06-17 (db4158a3) replaced by [[unlinked-code-sharing]]: ByVal ICs could use Handler IC once Int32, String, and Symbol property checks were emitted inside handlers, so Baseline no longer needed the polymorphic compile path for Handler IC. (sourced)
