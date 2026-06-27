- RegExp flags are represented as an integer bitmask in runtime code.
- Invalid flag combinations are represented by an InvalidFlags sentinel.
- YARR pattern construction receives already-parsed RegExpFlags rather than owning flag validation.

## Moves

- 2019-03-11 (71aac694) replaced by [[pattern-analysis]]: Moving flag parsing from runtime/ to yarr/ as OptionSet<Flags> enables early (parse-time) SyntaxError detection for invalid RegExp flags, which the old RegExpFlags int bitmask in runtime/ could not surface until bytecode emit time. (code)
