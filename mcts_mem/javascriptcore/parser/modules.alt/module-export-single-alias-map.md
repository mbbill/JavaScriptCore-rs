- Module export collection stored at most one exported name for each local binding.
- A later alias for the same local binding overwrote the previous exported name.

## Moves

- 2016-07-31 (a34b8996) replaced by [[modules]]: A module local binding can be exported under multiple names, so export collection needs a per-module local-name to exported-name set instead of a single alias per local binding. (code)
