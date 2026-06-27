- m_completionTypeRegister RefPtr<RegisterID> as BytecodeGenerator member
- m_completionValueRegister RefPtr<RegisterID> as BytecodeGenerator member
- CompletionRecordScope RAII helper that called allocateCompletionRecordRegisters/releaseCompletionRecordRegisters
- FinallyContext stored by value in ControlFlowScope (not pointer)
- emitSetCompletionType/emitSetCompletionValue member functions on BytecodeGenerator
- CompletionType::Break and CompletionType::Continue enum values
- emitCatch() method (replaced by emitOutOfLineCatchHandler/FinallyHandler/ExceptionHandler)
- CatchEntry tuple with 3 VirtualRegisters (now 4)

## Moves

- 2019-03-07 (2a75b559) replaced by [[bytecode]]: A single pair of m_completionTypeRegister/m_completionValueRegister shared across all FinallyContext instances was clobbered when an inner finally ran (e.g. a continue inside a nested try-finally), destroying the outer try block's saved completion and producing wrong results for nested try-finally. (code)
