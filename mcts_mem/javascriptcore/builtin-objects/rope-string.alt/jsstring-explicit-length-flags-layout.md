- JSString stored explicit length and flag fields beside the StringImpl value.
- JSRopeString stored full fiber pointers and flags in a larger subclass layout.

## Moves

- 2019-03-01 (1bbd6bf9) replaced by [[rope-string]]: sizeof(JSString) reduced 24->16 and sizeof(JSRopeString) 48->32 by eliminating redundant length/flags fields from JSString (queried from StringImpl instead) and compressing JSRopeString's three fiber pointers + length + is8Bit flag into 48-bit-address-exploiting split storage, fitting both into GC heap cell atoms to cut per-instance allocation by 16 bytes. (sourced)
