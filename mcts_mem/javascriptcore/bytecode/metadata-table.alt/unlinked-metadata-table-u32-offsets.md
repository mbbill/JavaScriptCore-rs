- uint32_t offsetTable[numberOfOpcodes] always allocated at fixed size (~204 bytes minimum per instance)
- single-level lookup: offset = offsetTable32[opcode]; metadata = table[offset] + id

## Moves

- 2019-05-23 (18546474) replaced by [[metadata-table]]: Gmail had 21979-24727 live UnlinkedMetadataTable instances each paying 204 bytes for a full uint32_t offset table; switching to uint16_t offset table (with 0 as sentinel indicating a spilled uint32_t table) reduces per-instance overhead for small tables and should save ~2 MB in Gmail steady state. (sourced)
