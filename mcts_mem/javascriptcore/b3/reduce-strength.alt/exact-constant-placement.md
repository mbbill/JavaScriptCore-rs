- Constant hoisting canonicalized equal constants and chose a materialization owner.
- Reinsertion only materialized child constants whose exact ValueKey matched the use.
- Memory offsets and Add/Sub operations were not rewritten to use nearby equivalent constants.

## Moves

- 2016-09-30 (f906ce1e) replaced by [[reduce-strength]]: Exact constant placement could not reuse a nearby address constant or a negated add/sub constant, so moveConstants began rewriting memory offsets and flipping Add/Sub to use the most dominant equivalent constant. (code)
