- JSMap and JSSet owned a wrapper around HashMapImpl storage.
- Accessing collection storage paid one extra allocation and indirection.

## Moves

- 2017-05-20 (dd0087fd) replaced by [[map-set-table]]: JSMap and JSSet can directly inherit HashMapImpl, eliminating one indirection when accessing the map implementation and one allocation per Map or Set. (code)
