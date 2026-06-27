- Bytecode instructions use a flat byte stream with opcode-specific fixed operand counts; consumers advance the program counter from the opcode declaration rather than from self-describing instruction records.
- A generator-owned opcode declaration table defines operands, metadata, temporary operands, and checkpoint substeps, and generated C++ accessors keep decoders, dumpers, and execution tiers in sync.
- Operand width is selected per instruction: narrow, wide16, and wide32 forms share one operand width for every operand field in that instruction.
- Wide instructions keep a prefix plus a narrow opcode and widen only operand fields, minimizing code size while preserving large-function addressability.
- VirtualRegister is the single signed operand space for locals, arguments, temporaries, and constants; invalid-register and constant encodings must not collide with valid frame offsets.

## Facts

- 2009-07-07 (42fc4c6b) rationale: constants use bit 30 as the constant-pool flag, splitting negative call-frame entries, positive locals/temporaries, and constant-pool offsets so ExecState::r() can branch on one bit test without growing operand words (code).
- 2013-11-18 (fe6295f1) pitfall: bytecode liveness analysis must handle spread and call argument ranges in the correct register direction, including skipping the construct callee slot (code).
- 2014-08-21 (5d8e460c) pitfall: bytecode dumping must skip every op_profile_type operand or the iterator is misaligned whenever that opcode appears (sourced).
- 2015-01-06 (1bff9e76) pitfall: the use-def table must classify op_create_lexical_environment as using its scope-chain operand, not its output local (sourced).
- 2018-10-26 (83d30124) rationale: narrow vs wide bytecode encoding reduces instruction-stream size when operands fit in one byte while retaining full range for large functions; a size prefix selects the encoding (sourced).
- 2018-10-31 (f8587934) pitfall: exception handler linking must select the narrow or wide op_catch code pointer according to the target instruction width (sourced).
- 2019-05-30 (bb678b97) measurement: adding the 16-bit bytecode width tier improved Gmail memory by at least 7MB because many large-function operands overflowed 8-bit but fit in 16-bit (sourced).
- 2020-01-09 (b9d9e329) pitfall: BaseInstruction::size() must do prefix and operand-size arithmetic in size_t or the size computation can overflow before widening (code).
- 2020-01-17 (198b075b) pitfall: checkpoint OSR side state is populated by an OSR-exit probe rather than normal stack flushing because tmp operands are skipped during stack restoration and do not correspond to baseline stack slots (code).
- 2024-10-03 (2ad9fbc5) rationale: switch bytecodes keep their default branch offset in the switch table rather than as an instruction operand, reducing bytecode size because dispatch already loads the table (code).

## Moves

- 2010-05-24 (203ccb5c) replaced [[caller-allocates-this-in-construct]]: Caller passing proto+thisRegister operands to op_construct could not support callee-side prototype lookup or a per-callee native-constructor thunk; moving this-creation into op_create_this planted in the callee enables NativeExecutable to carry a separate constructor NativeFunction and mirrors the call path already used for non-host functions. (sourced)
- 2013-09-10 (f5514288) replaced [[invalid-virtual-register-sentinel-minus-one]]: When the stack grows downward, local register indices become negative, making -1 a valid virtual register offset; INT_MAX (0x7fffffff) is used instead as a sentinel that can never collide with any valid positive or negative register index. (sourced)
- 2013-09-26 (027ced83) replaced [[virtual-register-enum]]: VirtualRegister enum could not encapsulate operand-classification methods (isLocal/isArgument/toLocal/toArgument), which were scattered as free functions in Operands.h; a class enables those predicates to live on the type itself. (Rolled out by 13385, re-landed at 13395.) (code)
- 2015-12-06 (2c4dd62d) replaced [[arrow-function-bound-this-field]]: The lexical-scope representation can carry this, new.target, and the derived constructor through arrow functions and eval using ordinary scope loads/stores, while the old JSArrowFunction bound-this field only carried this at arrow function creation. (code)
- 2019-12-25 (24b088b7) replaced [[bytecode-wide-instruction-opcode-same-width]]: In wide16/wide32 instructions, the opcode was also emitted at 16/32 bits, wasting space because opcodes always fit in 8 bits; always emitting a narrow (1-byte) opcode saves one byte per operand-slot in every wide instruction. (sourced)
