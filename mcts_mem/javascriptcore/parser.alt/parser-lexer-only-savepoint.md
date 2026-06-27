- Parser savepoints rewound lexer position and line state only.
- Assignment, non-LHS, expression, phase, and last-name parser state had to be saved through separate ad-hoc state objects.

## Moves

- 2016-01-22 (c143d94a) replaced by [[parser]]: The old SavePoint name implied whole-parser rollback while it only restored lexer position, so parser state mutated by speculative parsing had to be saved separately and was easy to misuse. (code)
