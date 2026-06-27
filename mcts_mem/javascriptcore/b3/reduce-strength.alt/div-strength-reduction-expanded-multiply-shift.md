- Signed int32 division-by-constant reduction represented high multiplication as a widened multiply followed by a shift.
- The expanded multiply-shift sequence was used even on targets with native high-multiply operations.

## Moves

- 2025-03-21 (a4cc57da) replaced by [[reduce-strength]]: Div/Mod strength reduction needs the upper extended bits of multiplication explicitly in the IR so targets with native high-multiply operations can lower it directly instead of materializing a widened multiply plus shift. (code)
