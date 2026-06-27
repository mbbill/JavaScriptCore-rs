- JavaScript execution uses a contiguous register-slot stack shared by interpreter entry, LLInt, JIT tiers, unwinding, and stack introspection.
- Native-to-JavaScript entries are delimited by VM entry records, and live frames are walked through the linked call-frame chain rather than a separate shadow stack.
- Long-running execution is polled at VM-entry, loop/backedge, and trap-service boundaries rather than at every bytecode dispatch.
- Detailed execution-state layout, entry API shape, exception unwinding, and scope/activation representation live in [[call-frame-layout]], [[entry-api]], [[exception-unwind]], and [[scope-chain-and-activation]].

## Facts

- 2008-05-23 (fdfff648) pitfall: RegisterFile::uncheckedGrow could silently exceed the maximum size when the file was already near capacity; the fix removed uncheckedGrow and required grow() failure to throw stack overflow. (code)
- 2008-06-24 (2343899d) rationale: loop-specific opcodes were separated from forward jumps so the interpreter could insert slow-script timeout checks only on backward edges. (sourced)
- 2008-06-27 (fd6ba21a) rationale: mmap reservation for the register area lets physical pages be allocated lazily while preventing reallocation and pointer invalidation in the interpreter loop. (sourced)
- 2008-06-27 (fd6ba21a) measurement: replacing RegisterFileStack with one mmap-backed RegisterFile yielded a 0.2% SunSpider speedup. (sourced)
- 2008-06-30 (4b675f72) pitfall: RegisterFile::~RegisterFile passed byte counts without multiplying by sizeof(Register), leaving most of the reserved region mapped on 32-bit threaded runs. (code)
- 2008-09-09 (2c24e5d5) pitfall: the CTI timeout slow path wrote the reloaded tick count through a bogus combined offset and some loop slow cases omitted emitSlowScriptCheck, making timeout ineffective in CTI mode. (code)
- 2008-10-04 (cc123e06) pitfall: register-file offsets used signed int where pointer arithmetic on 64-bit needed ptrdiff_t to avoid sign-extension mismatches. (sourced)
- 2009-03-20 (3447927f) pitfall: MaxReentryDepth=128 left too little native stack on PowerPC to throw stack overflow once JavaScript recursion was already deep; reducing it to 64 reserved runtime headroom. (sourced)
- 2010-04-07 (90edf104) rationale: termination checks reuse the timeout polling site, adding no new dispatch polling while still causing a full throwException-based unwind. (code)
- 2011-03-16 (03e63256) rationale: machine-stack roots and register-file roots are collected separately because only ambiguous machine-stack values need pinning, while register-file roots can be filtered to JSCell-tagged values. (sourced)
- 2012-03-16 rationale: saved bytecode offsets are computed lazily during frame inspection so bytecode movement does not require eager per-call bookkeeping. (code)
- 2012-04-25 (6041ef69) rationale: synthetic VM entry records give native-to-JS boundaries the same caller-frame chain shape as ordinary JavaScript calls. (code)
- 2013-03-29 (2540d792) rationale: loop opcodes existed solely for timeout checking and were removed instead of repaired because future timeout polling would not depend on opcode identity. (sourced)
- 2014-08-29 rationale: the stack visitor owns frame traversal so clients can recover call-frame metadata without depending on begin/end iterator lifetime. (code)
- 2022-02-28 (007c58ef) pitfall: concurrent termination must be deferred during ordinary exception unwinding because handler search snapshots whether the original exception is a TerminationException before walking frames. (sourced)
- 2022-02-28 (007c58ef) pitfall: when the original exception is already a TerminationException, unwind must not install the deferral scope because it can restore NeedTermination after traps have cleared it. (sourced)

## Moves

- 2008-10-02 (6d4e2a5a) replaced [[register-file-size-integer-tracking]]: Tracking RegisterFile extent as a Register* end pointer instead of a size_t integer eliminates per-call pointer arithmetic (base + size), yielding 2–3% speedup on V8 DeltaBlue and Raytrace. (sourced)
- 2013-09-04 (0441e5cb) replaced [[stack-iterator-range-for]]: The classic begin/end/operator++ iterator interface exposed StackIterator as a value that callers could store and manipulate outside the iteration loop; replacing it with a typed-functor callback (operator() returning Status) confines iterator lifetime to the iterate() call and allows early termination via Done/Continue return values without exposing iterator state. (sourced)
- 2020-10-01 (b63a0f1b) replaced [[host-call-return-c-function-glue]]: JIT-caging restricts JIT-related PtrTags to JIT code, so getHostCallReturnValue could not remain a C function tagged as a JIT entrypoint. (sourced)
