- collectBoyerMooreInfo iterated only the current alternative's top-level terms.
- ParenthesesSubpattern, back/forward references, parenthetical assertions, and DotStarEnclosure all caused collection to stop and shorten at the current cursor.
- Only one-character PatternCharacter and CharacterClass terms with fixed or greedy max-count 1 contributed characters to the BoyerMooreInfo.

## Moves

- 2023-02-09 (bb136cc4) replaced by [[boyer-moore-prefilter]]: The recursive collector supports nested disjunctions such as /aaa|(bbb|cccc)/ that the old flat collector explicitly refused at ParenthesesSubpattern terms. (code)
