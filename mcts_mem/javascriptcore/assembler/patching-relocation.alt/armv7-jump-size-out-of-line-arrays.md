- ARMv7 jump-size and padding data lived in static arrays indexed by jump enums.
- LinkRecord stored jump type and link type as compact bitfields and loaded sizes from those arrays.

## Moves

- 2011-07-07 (61a9b3cb) replaced by [[patching-relocation]]: Static out-of-line arrays required a memory load and defeated constant folding of jump-delta arithmetic; encoding sizes directly into the enum values as high bits (JUMP_ENUM_WITH_SIZE macro) lets the compiler constant-fold all linking arithmetic and eliminates the array loads. (sourced)
