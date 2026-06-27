- One module parser owns type, import, and code section parsing over a complete contiguous byte buffer. (`ModuleParser`)
- Function locations are recorded for later compilation after whole-module parsing.

## Moves

- 2018-08-28 (5a417755) replaced by [[wasm]]: The monolithic ModuleParser required all wasm bytes to be available before parsing could start; the new streaming parser accepts bytes incrementally via addBytes(), using a state machine with Section as the unit of incrementalism and Function as a finer unit inside the Code section, enabling concurrent compilation while parsing continues. (code)
