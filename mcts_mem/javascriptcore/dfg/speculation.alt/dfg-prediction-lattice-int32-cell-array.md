- The prediction lattice had PredictInt32, PredictCell, and PredictArray but no double prediction.
- Double-observed arguments were indistinguishable from no information.

## Moves

- 2011-07-28 (99e030ef) replaced by [[speculation]]: The old lattice had no PredictDouble value; arguments observed as doubles at compile time were typed as PredictNone (no information), causing the speculative JIT to attempt Int32 speculation on known-double arguments and fail; adding PredictDouble and PredictNumber allows the speculative JIT to skip Int32 speculation when the prediction says double. (code)
