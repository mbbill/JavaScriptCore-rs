- rdepth local variable in match()
- ++rdepth / --rdepth increment/decrement around RMATCH macro
- if (rdepth >= MATCH_LIMIT_RECURSION) check at top of RECURSE label

## Moves

- 2007-11-29 (09bb02e6) replaced by [[yarr]]: Moving the recursion-depth check from the top of the RECURSE label (executed on every frame re-entry including returns) to inside pushNewFrame (executed only when a new frame is allocated) removes the check from the super-hot steady-state path, contributing to the 8% speedup. (code)
