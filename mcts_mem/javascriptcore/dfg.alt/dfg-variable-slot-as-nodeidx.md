- Variable slots stored a single producer node index.
- Reads of arguments and locals returned the stored node index directly, lazily creating argument nodes when needed.

## Moves

- 2011-04-15 (3a4c6219) replaced by [[dfg]]: A single NodeIndex per variable slot could not express the case where a GetLocal in one basic block needs to reference the most-recent SetLocal from a prior block; the VariableRecord{get,set} pair plus explicit GetLocal/SetLocal graph nodes makes the producer-consumer relationship explicit and persistent across block boundaries. (code)
