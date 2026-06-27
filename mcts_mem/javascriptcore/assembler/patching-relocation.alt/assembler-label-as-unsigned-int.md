- label() returned a raw unsigned integer buffer offset.
- Jump lists stored raw integer offsets and call sites manually wrapped offsets into AssemblerLabel values.

## Moves

- 2011-05-02 (02f3ae07) replaced by [[patching-relocation]]: The old unsigned int return type from label() was implicitly convertible to/from raw integers, allowing accidental arithmetic on offsets and making jump lists store raw ints; AssemblerLabel provides a type wall so consumers cannot silently mix offset values with unrelated integers. (sourced)
