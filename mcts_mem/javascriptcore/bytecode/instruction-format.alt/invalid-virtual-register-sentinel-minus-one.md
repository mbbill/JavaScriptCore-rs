- enum VirtualRegister { InvalidVirtualRegister = -1 }
- comparisons against -1 in usesArguments(), LazyOperandValueProfileKey

## Moves

- 2013-09-10 (f5514288) replaced by [[instruction-format]]: When the stack grows downward, local register indices become negative, making -1 a valid virtual register offset; INT_MAX (0x7fffffff) is used instead as a sentinel that can never collide with any valid positive or negative register index. (sourced)
