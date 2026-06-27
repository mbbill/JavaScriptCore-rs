- Fast allocation is provided by inheriting a base class that defines fast-malloc operator new and delete.
- Classes opt into fast allocation through object inheritance.
- The fast-allocation helper participates in C++ object layout.

## Moves

- 2010-10-18 (bb218350) replaced by [[wtf-platform]]: Inheriting from FastAllocBase could increase object sizes due to C++ base-class layout rules, causing memory regressions (investigated in bug #33896); a macro that injects operator new/delete directly into the target class avoids any size increase while delivering the same fast-malloc routing. (sourced)
