- Non-greedy parenthesized subpatterns cause JIT compilation to bail out.
- Such patterns run through the interpreter or bytecode path.
- The JIT has no skip-first-try and backtrack re-entry path for non-greedy nested parentheses.

## Moves

- 2018-08-24 (37f6320c) replaced by [[jit-fallback]]: Non-greedy parenthesized subpatterns previously caused a JIT bail-out (fell back to interpreter); the new implementation adds JIT code generation for non-greedy nested parens by extending the existing greedy paren infrastructure with a skip-first-try jump and backtrack re-entry path, expanding what patterns the JIT can compile. (code)
