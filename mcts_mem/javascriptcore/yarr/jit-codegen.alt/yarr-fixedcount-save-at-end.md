- FixedCount ParenContext saves iteration state at ParenthesesSubpatternEnd.
- FixedCount paths carry special return-address handling and separate begin/end opcodes.
- Capture clearing and begin-index restoration are special-cased for fixed-count quantifiers.

## Moves

- 2026-06-04 (a92d79b2) replaced by [[jit-codegen]]: FixedCount switched from a special save-at-END ParenContext model to the same save-at-BEGIN snapshot model as Greedy and NonGreedy so quantified-parentheses backtracking could share one state machine. (code)
