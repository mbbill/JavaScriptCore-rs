- LLInt opcode configuration tables are stored in a JSC-owned allocation.
- JSC freezes or protects its own config storage instead of delegating to OS script configuration storage.
- SDKs exposing OS storage do not change the primary opcode table owner.

## Moves

- 2025-10-28 (231b11ba) replaced by [[llint]]: When the SDK exposes os_script_config_storage, LLInt opcode configuration uses that OS-provided storage and keeps an in-tree allocation only as the fallback for SDKs without the SPI. (code)
