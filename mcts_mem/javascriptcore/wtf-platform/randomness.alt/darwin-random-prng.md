- Darwin random numbers use `random()` seeded by `srandomdev()`.
- The generator exposes a 31-bit range scaled to a JavaScript double.
- Seed initialization is a separate runtime step.

## Moves

- 2008-12-30 (874abb7f) replaced by [[randomness]]: random() output is predictable and led to user tracking via Math.random(); arc4random() is cryptographically strong and self-seeding, eliminating the need for srandomdev() initialization. (sourced)
