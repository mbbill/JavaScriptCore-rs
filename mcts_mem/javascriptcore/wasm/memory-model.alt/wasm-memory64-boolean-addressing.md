- Memory information represents address width as a boolean memory64 flag. (`MemoryInformation`)
- Memory allocation chooses signaling fast memory without carrying address width.

## Moves

- 2026-02-27 (65324979) replaced by [[memory-model]]: Memory address width became an explicit AddressType carried into Memory allocation so i64 memories can be forced away from signaling fast memory at creation time. (code)
