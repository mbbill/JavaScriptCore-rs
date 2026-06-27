- move(X86::edi, input) // arg1 edi->eax
- move(X86::ecx, output) // arg4 ecx->edi
- move(X86::edx, length) // arg3 edx->ecx
- move(X86::esi, index) // arg2 esi->edx
- input=X86::eax, index=X86::edx, length=X86::ecx, output=X86::edi on x86-64

## Moves

- 2009-02-10 (a4e24f08) replaced by [[yarr]]: When WREC was ported to x86-64 it reused x86 register assignments and emitted argument-shuffle moves (rdi->eax, rsi->edx, rdx->ecx, rcx->edi) in generateEnter because x86-64 SysV ABI passes args in rdi/rsi/rdx/rcx, not via regparm(3); switching to native ABI register names eliminates these shuffles and aligns the register allocation with the calling convention. (sourced)
