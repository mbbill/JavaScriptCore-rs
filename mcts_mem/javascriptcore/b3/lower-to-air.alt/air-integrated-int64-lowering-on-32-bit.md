- Air-integrated lowering emitted selected 32-bit instruction sequences directly from Int64 B3 nodes.
- The lowering tracked parallel high/low bitwise operations, split loads, variable shifts with block splitting, and Add64/Sub64 forms inside Air lowering.

## Moves

- 2024-07-09 (e4b1106b) replaced by [[lower-to-air]]: The separate B3 pass can split Int64 values into explicit Int32 high/low values before Air lowering while temporarily stitching values back for Patchpoints, CCalls, and Add/Sub, avoiding generic B3/Air parallel-iteration and carry-tracking complications. (sourced)
