- Double-to-float reduction collected local DoubleToFloat candidates.
- Candidates were removed when a non-local use still required Double.
- Cleanup relied on later ReduceStrength passes.

## Moves

- 2016-04-18 (60e36af4) replaced by [[reduce-strength]]: Local candidate elimination could not propagate float precision through Phi/Upsilon loops, so the replacement uses backward and forward analyses plus cleanup to convert only double values and phis whose precision and uses allow float form. (code)
