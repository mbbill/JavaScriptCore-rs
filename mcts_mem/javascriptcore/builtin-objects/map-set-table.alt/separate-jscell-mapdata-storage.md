- JSMap and JSSet pointed to separate MapData or SetData JSCells.
- Set storage carried a value slot even though Set entries need only the key.

## Moves

- 2015-03-12 (4ce89cb4) replaced by [[map-set-table]]: Embedding specialized MapData/SetData into JSMap/JSSet removes two object allocations per collection and lets SetData omit the dummy value field, halving set entry storage. (code)
