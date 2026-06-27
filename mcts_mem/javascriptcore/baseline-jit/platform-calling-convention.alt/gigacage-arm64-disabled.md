- ARM64 leaves primitive Gigacage disabled when pointer-authentication high bits would be stripped by caging.

## Moves

- 2019-06-06 (906bc2cf) replaced by [[platform-calling-convention]]: ARM64E uses Pointer Authentication Codes (PAC) in the high bits of pointers; the old Gigacage cage() used a simple AND+ADD that would strip PAC bits. The new cageWithoutUntaging() uses bitFieldInsert64 to preserve high PAC bits while replacing only the Gigacage-controlled low bits. (sourced)
