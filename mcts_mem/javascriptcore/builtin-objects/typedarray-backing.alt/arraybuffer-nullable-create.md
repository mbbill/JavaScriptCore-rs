- ArrayBuffer creation APIs returned nullable pointers for both callers that could handle OOM and callers that assumed success.
- Failure-capable allocation was not visible in the API name or type contract.

## Moves

- 2016-03-21 (fa3ed404) replaced by [[typedarray-backing]]: ArrayBuffer allocation split into non-null create APIs that CRASH on allocation failure and nullable tryCreate APIs so OOM-capable callers must opt into the type that can represent failure. (code)
