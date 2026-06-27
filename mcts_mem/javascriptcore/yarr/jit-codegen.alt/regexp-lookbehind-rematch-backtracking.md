- Greedy Unicode character-class matching saved the starting input position in BackTrackInfoCharacterClass::begin.
- Forward backtracking reset input to begin, decremented matchAmount, then replayed checkCharacterClass until the shorter match amount was reached.
- Backward backtracking likewise reset to begin and replayed tryUncheckInput/checkCharacterClass for the shorter match.

## Moves

- 2023-06-28 (8e5b3f57) replaced by [[jit-codegen]]: Unicode lookbehind character-class backtracking now unreads one codepoint instead of rematching one fewer codepoint, avoiding repeated rescans when many codepoints must be backed off. (sourced)
