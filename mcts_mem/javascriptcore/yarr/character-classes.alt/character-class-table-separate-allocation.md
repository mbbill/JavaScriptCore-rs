- CharacterClassTable RefCounted struct with m_table and m_inverted
- CharacterClass held RefPtr<CharacterClassTable> m_table
- CharacterClassTable::create() factory method
- CharacterClass(PassRefPtr<CharacterClassTable>) constructor

## Moves

- 2013-04-12 (91d2eafc) replaced by [[character-classes]]: CharacterClassTable was a RefCounted heap object holding only a const char* pointer and a bool, making a separate allocation wasteful for what could be two inline fields directly in CharacterClass. (sourced)
