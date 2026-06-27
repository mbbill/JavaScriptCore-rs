- Interpreter matching functions return bool.
- Match and no-match are the only representable outcomes.
- Recursion-depth or match-limit failure has no distinct result to propagate.

## Moves

- 2010-11-16 (728dc3f9) replaced by [[interpreter-dispatch]]: The bool return type could only distinguish match/no-match and could not propagate a HitLimit error code when unbounded recursion was detected; switching to JSRegExpResult enum (JSRegExpMatch=1, JSRegExpNoMatch=0, JSRegExpErrorHitLimit=-2, etc.) allows the recursion depth counter (remainingMatchCount) check in matchDisjunction to return a distinct error value that propagates up through the call tree without using exceptions or global state. (code)
