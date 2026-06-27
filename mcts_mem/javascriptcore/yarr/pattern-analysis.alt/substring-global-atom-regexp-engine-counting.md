- collectMatches resumed from cached substring state, then repeatedly called RegExpGlobalData::performMatch, advanced startIndex to result.end or result.end + 1 for empty matches, counted matches, and cached m_lastNumberOfMatches/m_lastMatchEnd before constructing a pattern-filled array.

## Moves

- 2024-10-15 (5d4d9792) replaced by [[pattern-analysis]]: For substring global atom regular expressions with one-character patterns, direct span scanning replaces repeated RegExpGlobalData::performMatch calls while preserving the substring match cache. (code)
