# bytecode-gen: JSC-generator-backed Rust opcode table

Generates `generated/opcode_table.generated.rs` — the full 193-opcode Rust
descriptor table for JSC's `:Bytecode` section — by running **JSC's own Ruby
bytecode generator** over **JSC's own `bytecode/BytecodeList.rb`**, then
walking the evaluated sections and emitting Rust. Nothing about opcode
identity is re-derived by hand: ID assignment, ordering, operand schemas,
lengths, and the metadata/checkpoint partitions all come out of the same
machinery that produces the C++ build's `Bytecodes.h`/`BytecodeStructs.h`.

## Why generation, not hand-rolling

`BytecodeList.rb` is the literal source of truth for the JS bytecode set: it
is executable Ruby, evaluated by `generator/DSL.rb` (`DSL.run`,
`DSL.rb:181-195`), and every opcode's ID is nothing more than its sequential
declaration position (`:Bytecode` is `preserve_order: true`,
`BytecodeList.rb:79-87`; `end_section` skips `Section#sort!` and only
validates the `[checkpoints][metadata][plain][simd]` ordering,
`DSL.rb:43-57` / `Section.rb:72-97`; `create_ids!` numbers from the `Opcode`
class counter, `Section.rb:99-101` / `Opcode.rb:41-47,59-61`). The mcts_mem
node `instruction-format.md` records the generator-owned table as JSC's
settled decision — C++ JSC itself never hand-writes this table, so a faithful
rewrite must not either. Any hand-rolled copy would silently re-derive (and
eventually diverge from) exactly the invariants the JIT and interpreter
dispatch depend on.

## What the tool does

`generate.rb`:

1. Loads the WebKit generator modules **by path, read-only**
   (`require $WEBKIT_DIR/Source/JavaScriptCore/generator/DSL.rb`; its internal
   `require_relative`s pull in `Section`/`Opcode`/`Argument`/`Metadata`/`Fits`
   etc. from the WebKit checkout). Two small shims live in our wrapper only:
   a reader for the module-private `DSL @sections`, and `attr_reader :type`
   on `Argument` (WebKit keeps `@type` private). No WebKit file is modified.
2. Binds the primitive types exactly as `generator/main.rb:29-35` does, then
   runs the verbatim `DSL.run` pipeline with the five derived outputs pointed
   at a temp dir (the freshly written `Bytecodes.h` doubles as a verification
   input).
3. Walks the `:Bytecode` section and maps each stream arg's C++ type to
   `OperandKind::<CamelCased C++ type name>` (`unsigned` → `UnsignedImmediate`,
   `int` → `SignedImmediate`, `bool` → `Bool`; unknown non-CamelCase types fail
   loudly). `metadata: {}` fields do **not** become operands — they set
   `has_metadata`, the single `m_metadataID` length slot
   (`generator/Opcode.rb:372-374`).
4. **Verifies before writing** (fail-loud, exit 1, no output on mismatch):
   - all id/name/length triples match the local generated build artifact
     `Bytecodes.h` (`FOR_EACH_BYTECODE_ID`) that the measuring-instrument C++
     `jsc` was built from, and the fresh `DSL.run` output agrees with that
     artifact;
   - `NUMBER_OF_BYTECODE_IDS` (193), `NUMBER_OF_BYTECODE_WITH_METADATA` (49),
     `NUMBER_OF_BYTECODE_WITH_CHECKPOINTS` (7), `MAX_LENGTH_OF_BYTECODE_IDS`
     (10), the metadata/checkpoint id-prefix partitions, and the per-op
     `bytecodeCheckpointCountTable` entries;
   - the opcode ids already hand-declared in
     `src/bytecode/instruction_stream.rs` (`jmp=69` ... `sub=161`) appear
     identically.
5. Emits `generated/opcode_table.generated.rs` with a DO-NOT-EDIT header
   recording the invocation, WebKit revision, and inputs, then prints the
   operand-type census.

The generated file is **not** wired into the crate build yet (that is a
separate integration unit); `OperandKind` is expected to be in scope at the
eventual inclusion site.

## Regenerating

Manual, from the repo root:

```sh
ruby tools/bytecode-gen/generate.rb
```

Requires a local WebKit checkout **with a Release build's DerivedSources**
(the verification oracle). Defaults, overridable via environment:

- `WEBKIT_DIR` — WebKit checkout root (default `/Users/bytedance/Dev/WebKit`)
- `BYTECODES_H` — generated artifact to verify against (default
  `$WEBKIT_DIR/WebKitBuild/Release/DerivedSources/JavaScriptCore/Bytecodes.h`)

Rerun whenever the WebKit checkout (and its Release build) moves; the tool
refuses to emit if the checkout and the built artifact disagree.
