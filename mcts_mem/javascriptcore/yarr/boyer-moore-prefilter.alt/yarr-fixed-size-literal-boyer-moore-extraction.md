- Boyer-Moore lookahead is attempted only when the whole disjunction has fixed size.
- Unsupported terms make collection fail rather than shortening to an extractable prefix.
- Pattern characters must be fixed-count, aligned, and single-code-unit for extraction.

## Moves

- 2021-08-02 (9c6fee78) replaced by [[boyer-moore-prefilter]]: The extractor was changed to use fixed prefixes and fixed-count character classes so regexps such as jQuery TodoMVC patterns could still get Boyer-Moore lookahead when a later term was unsupported or the whole disjunction was not fixed-size. (code)
