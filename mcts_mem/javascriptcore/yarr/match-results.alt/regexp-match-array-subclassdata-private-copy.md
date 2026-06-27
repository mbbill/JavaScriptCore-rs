- RegExpConstructor stored OwnPtr<RegExpConstructorPrivate> d for the global regexp cache/settings.
- RegExpMatchesArray::finishCreation allocated a new RegExpConstructorPrivate, copied lastInput, lastNumSubPatterns, and the active lastOvector into it, then stored it in subclassData().
- RegExpMatchesArray lazily materialized array properties when subclassData() was non-null and then deleted the copied RegExpConstructorPrivate and cleared subclassData().

## Moves

- 2012-01-11 (30377e04) replaced by [[match-results]]: RegExp match arrays only need the input string, subexpression count, and active output vector, so storing that snapshot inline avoids allocating and copying a whole RegExpConstructorPrivate object. (code)
