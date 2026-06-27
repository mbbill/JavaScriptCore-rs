- CallLinkStatus represented at most one likely executable or callee.
- The inliner could emit one callee check and inline one target, with no per-callsite callee switch.

## Moves

- 2014-08-25 (888178b2) replaced by [[call-dispatch]]: A single executable/callee status could not express multiple likely callees at one callsite, so FTL adopted precise call-edge profiles and a callee switch to inline several alternatives. (code)
