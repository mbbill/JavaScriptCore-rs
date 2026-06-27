- Interpreter::testCharacterClass first handled m_anyCharacter, then searched match and range vectors with linear search for small vectors and binary search above a threshold of six.
- ASCII and non-ASCII characters used separate match/range vectors and no m_table fast path in the interpreter.

## Moves

- 2026-01-10 (75ec3982) replaced by [[character-classes]]: The Yarr interpreter now uses CharacterClass::m_table for characters below CharacterClass::tableSize before falling back to vector match/range searches, adopting the same table representation already used by the JIT. (code)
