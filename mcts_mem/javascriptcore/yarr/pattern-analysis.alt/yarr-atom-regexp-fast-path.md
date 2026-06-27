- Special regexp classification is represented by a nullable atom string.
- Only literal fixed-character regexps can use the special path.
- Anchored trim-like and other specialized non-atom patterns fall back to generic regexp execution.

## Moves

- 2025-02-28 (04edf771) replaced by [[pattern-analysis]]: A nullable atom string could only represent literal fixed-character regexps, so Yarr changed the special-case representation to an enum that can also carry anchored whitespace trim patterns. (code)
