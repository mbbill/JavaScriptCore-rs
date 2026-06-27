- Stack bounds are estimated from the origin pointer by subtracting a fixed 128 * sizeof(void*) * 1024 byte allowance.
- Darwin, Windows, QNX, and generic Unix use the same estimate rather than querying the OS.
- Expression-depth limits are constrained by a conservative fixed stack-size guess.

## Moves

- 2010-12-21 (f75765bf) replaced by [[threading]]: The estimated stack bound (origin minus fixed 128*sizeof(void*)*1024) was replaced with accurate OS-queried stack bounds on Darwin (pthread_get_stacksize_np), Windows (TIB StackLimit), QNX, and generic Unix (pthread_attr_getstack), increasing the size of expressions that can be processed; SOLARIS/OPENBSD/SYMBIAN/HAIKU/WINCE still use the estimate. (sourced)
