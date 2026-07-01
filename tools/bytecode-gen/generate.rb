#!/usr/bin/env ruby
# tools/bytecode-gen/generate.rb
#
# Generates the FULL Rust opcode-descriptor table for JSC's :Bytecode section
# by running JSC's OWN Ruby bytecode generator over JSC's OWN
# bytecode/BytecodeList.rb, then walking the evaluated sections. ID assignment,
# ordering, lengths, and the metadata/checkpoint partitions are therefore
# JSC's verbatim -- reproduced by executing the C++ project's generator
# machinery, never re-derived by hand.
#
# Pipeline reused verbatim from WebKit (read-only; nothing in the WebKit
# checkout is modified):
#   - generator/DSL.rb:181-195   DSL.run: evals BytecodeList.rb (it is
#                                executable Ruby) in the DSL module binding,
#                                then writes the derived C++/asm files.
#   - generator/DSL.rb:43-57     end_section: :Bytecode is declared
#                                `preserve_order: true` (BytecodeList.rb:79-87),
#                                so Section#validate runs (Section.rb:72-97)
#                                and Section#sort! is SKIPPED.
#   - generator/Section.rb:99-101  create_ids!: sequential numbering ...
#   - generator/Opcode.rb:41-47,59-61  ... from the Opcode @@id class counter,
#                                in exact declaration order.
#   - generator/Opcode.rb:372-374  length = args.length + (metadata? 1 : 0)
#                                (the +1 is the m_metadataID operand slot).
#   - generator/Section.rb:107-181  header_helpers: the FOR_EACH_BYTECODE_ID /
#                                NUMBER_OF_* macro emission we verify against.
#   - generator/main.rb:29-35    the lower-case primitive types bound before
#                                eval (bool, int, unsigned, uintptr_t, uint8_t).
#
# Verification (fail-loud, before any output is written): the id/name/length
# triples of all opcodes, plus NUMBER_OF_BYTECODE_IDS /
# NUMBER_OF_BYTECODE_WITH_METADATA / NUMBER_OF_BYTECODE_WITH_CHECKPOINTS /
# MAX_LENGTH_OF_BYTECODE_IDS, must EXACTLY match the local generated build
# artifact Bytecodes.h that the measuring-instrument C++ jsc was built from --
# both the artifact on disk AND the Bytecodes.h this run just regenerated
# through DSL.run's own write path.
#
# Usage (from the repo root; regeneration is manual, output is checked in):
#   ruby tools/bytecode-gen/generate.rb
# Environment overrides:
#   WEBKIT_DIR   WebKit checkout root   (default /Users/bytedance/Dev/WebKit)
#   BYTECODES_H  generated-build-artifact Bytecodes.h to verify against
#                (default $WEBKIT_DIR/WebKitBuild/Release/DerivedSources/JavaScriptCore/Bytecodes.h)

require 'tmpdir'
require 'fileutils'

WEBKIT_DIR = ENV['WEBKIT_DIR'] || '/Users/bytedance/Dev/WebKit'
JSC_DIR = File.join(WEBKIT_DIR, 'Source', 'JavaScriptCore')
GENERATOR_DIR = File.join(JSC_DIR, 'generator')
BYTECODE_LIST = File.join(JSC_DIR, 'bytecode', 'BytecodeList.rb')
WASM_JSON = File.join(JSC_DIR, 'wasm', 'wasm.json')
ARTIFACT_BYTECODES_H = ENV['BYTECODES_H'] ||
                       File.join(WEBKIT_DIR, 'WebKitBuild', 'Release', 'DerivedSources', 'JavaScriptCore', 'Bytecodes.h')
OUTPUT_RS = File.join(__dir__, 'generated', 'opcode_table.generated.rs')

[GENERATOR_DIR, BYTECODE_LIST, WASM_JSON, ARTIFACT_BYTECODES_H].each do |path|
  abort "bytecode-gen: missing input: #{path}" unless File.exist?(path)
end

# Load JSC's actual generator modules by path, out of tree. Their internal
# require_relative calls resolve against the WebKit checkout, so the whole
# module graph (DSL -> Section -> Opcode -> Argument/Fits/Metadata, ...) is
# WebKit's, byte for byte.
require File.join(GENERATOR_DIR, 'DSL.rb')

# ---------------------------------------------------------------------------
# Wrapper-side shims (this file only; the WebKit checkout is never edited).
# ---------------------------------------------------------------------------

module DSL
  # Shim: DSL keeps the evaluated sections in module-private @sections
  # (DSL.rb:32,51) and only exposes C++/asm writers. Our Rust backend needs to
  # walk the :Bytecode section, so add a reader.
  def self.sections
    @sections
  end
end

class Argument
  # Shim: Argument stores the arg's C++ type in @type without a reader
  # (Argument.rb:30-35); the OperandKind mapping below needs the type name.
  attr_reader :type
end

# ---------------------------------------------------------------------------
# Run JSC's generator pipeline, verbatim (mirrors generator/main.rb).
# ---------------------------------------------------------------------------

# generator/main.rb:27-35: lower-case type variables must be bound in the DSL
# eval context before BytecodeList.rb is evaluated.
DSL::types [
  :bool,
  :int,
  :unsigned,
  :uintptr_t,
  :uint8_t,
]

scratch = Dir.mktmpdir('jsc-bytecode-gen')
fresh_bytecodes_h = File.join(scratch, 'Bytecodes.h')
begin
  # DSL.run (DSL.rb:181-195) evals BytecodeList.rb -- sections get validated
  # and ids assigned exactly as in the C++ build -- and then writes the same
  # five derived files the build writes. We point them into a scratch dir; the
  # fresh Bytecodes.h doubles as a second verification input.
  DSL::run(
    bytecode_list: BYTECODE_LIST,
    wasm_json_filename: WASM_JSON,
    bytecodes_filename: fresh_bytecodes_h,
    bytecode_structs_filename: File.join(scratch, 'BytecodeStructs.h'),
    bytecode_dumper_filename: File.join(scratch, 'BytecodeDumperGenerated.cpp'),
    init_asm_filename: File.join(scratch, 'InitBytecodes.asm'),
    bytecode_indices_filename: File.join(scratch, 'BytecodeIndices.h'),
  )

  bytecode_section = DSL.sections.find { |s| s.name == :Bytecode }
  abort 'bytecode-gen: no :Bytecode section found' unless bytecode_section
  opcodes = bytecode_section.opcodes

  # -------------------------------------------------------------------------
  # OperandKind mapping (ratified G1/G2 scheme): each BytecodeList.rb arg C++
  # type maps mechanically to OperandKind::<CamelCased C++ type name>; the
  # non-CamelCase primitives are spelled out. Anything else fails loudly.
  # -------------------------------------------------------------------------
  OPERAND_KIND_SPECIAL = {
    'VirtualRegister' => 'VirtualRegister',
    'unsigned' => 'UnsignedImmediate',
    'int' => 'SignedImmediate',
    'bool' => 'Bool',
    'OperandTypes' => 'OperandTypes',
    'BoundLabel' => 'BoundLabel',
  }.freeze

  census = Hash.new(0) # [cpp_type, variant] -> count
  def operand_kind(cpp_type)
    s = cpp_type.to_s
    return OPERAND_KIND_SPECIAL[s] if OPERAND_KIND_SPECIAL.key?(s)
    unless s =~ /\A[A-Z][A-Za-z0-9]*\z/
      abort "bytecode-gen: arg type #{s.inspect} has no OperandKind mapping " \
            '(not CamelCase and not a known primitive); extend OPERAND_KIND_SPECIAL deliberately'
    end
    s
  end

  rows = opcodes.map do |op|
    operands = (op.args || []).map do |arg|
      variant = operand_kind(arg.type)
      census[[arg.type.to_s, variant]] += 1
      variant
    end
    {
      id: op.id,
      cpp_name: op.name,                    # "op_jmp" (section op_prefix applied)
      name: op.unprefixed_name.to_s,        # "jmp"
      operands: operands,
      has_metadata: !op.metadata.empty?,
      num_checkpoints: op.checkpoints ? op.checkpoints.length : 0,
      length: op.length,                    # Opcode.rb:372-374
    }
  end

  # -------------------------------------------------------------------------
  # Verification 1: id/name/length triples + section constants must EXACTLY
  # match the generated build artifact (and the Bytecodes.h we just wrote
  # through DSL.run's own write path). Section.rb:110-115 emits
  # `macro(name, length)` in id order plus NUMBER_OF_/MAX_LENGTH_OF_ macros.
  # -------------------------------------------------------------------------
  def parse_bytecodes_h(path)
    lines = File.readlines(path)
    start = lines.index { |l| l.start_with?('#define FOR_EACH_BYTECODE_ID(macro)') }
    abort "bytecode-gen: FOR_EACH_BYTECODE_ID not found in #{path}" unless start
    triples = []
    j = start + 1
    while j < lines.length && (m = lines[j].match(/\A\s*macro\((\w+), (\d+)\)\s*\\?\s*\z/))
      triples << [triples.length, m[1], m[2].to_i]
      j += 1
    end
    consts = {}
    %w[NUMBER_OF_BYTECODE_IDS MAX_LENGTH_OF_BYTECODE_IDS
       NUMBER_OF_BYTECODE_WITH_METADATA NUMBER_OF_BYTECODE_WITH_CHECKPOINTS].each do |name|
      line = lines.find { |l| l =~ /\A#define #{name} (\d+)\s*\z/ }
      abort "bytecode-gen: #{name} not found in #{path}" unless line
      consts[name] = line[/(\d+)\s*\z/, 1].to_i
    end
    # Per-op checkpoint counts (Section.rb:118-130): one entry per
    # checkpoint-carrying prefix op, in id order.
    start = lines.index { |l| l.include?('bytecodeCheckpointCountTable[] = {') }
    abort "bytecode-gen: bytecodeCheckpointCountTable not found in #{path}" unless start
    checkpoint_counts = []
    j = start + 1
    while j < lines.length && (m = lines[j].match(/\A\s*(\d+),\s*\z/))
      checkpoint_counts << m[1].to_i
      j += 1
    end
    [triples, consts, checkpoint_counts]
  end

  failures = []
  artifact_triples, artifact_consts, artifact_checkpoint_counts = parse_bytecodes_h(ARTIFACT_BYTECODES_H)
  fresh_triples, fresh_consts, fresh_checkpoint_counts = parse_bytecodes_h(fresh_bytecodes_h)

  if artifact_triples != fresh_triples || artifact_consts != fresh_consts ||
     artifact_checkpoint_counts != fresh_checkpoint_counts
    failures << "fresh DSL.run output #{fresh_bytecodes_h} disagrees with build artifact #{ARTIFACT_BYTECODES_H} " \
                '(WebKit checkout has moved since the instrument jsc was built?)'
  end

  if rows.length != artifact_consts['NUMBER_OF_BYTECODE_IDS']
    failures << "opcode count #{rows.length} != NUMBER_OF_BYTECODE_IDS #{artifact_consts['NUMBER_OF_BYTECODE_IDS']}"
  end
  if rows.length != artifact_triples.length
    failures << "opcode count #{rows.length} != artifact FOR_EACH_BYTECODE_ID entry count #{artifact_triples.length}"
  end

  rows.zip(artifact_triples).each do |row, (aid, aname, alength)|
    next if row.nil? || aid.nil?
    unless row[:id] == aid && row[:cpp_name] == aname && row[:length] == alength
      failures << "id #{aid}: generated (#{row[:id]}, #{row[:cpp_name]}, len #{row[:length]}) " \
                  "!= artifact (#{aid}, #{aname}, len #{alength})"
    end
  end

  meta_count = rows.count { |r| r[:has_metadata] }
  if meta_count != artifact_consts['NUMBER_OF_BYTECODE_WITH_METADATA']
    failures << "metadata count #{meta_count} != NUMBER_OF_BYTECODE_WITH_METADATA #{artifact_consts['NUMBER_OF_BYTECODE_WITH_METADATA']}"
  end
  # Instruction.h:98-101 hasMetadata() = opcodeID < numberOfBytecodesWithMetadata,
  # i.e. metadata opcodes must be an id-prefix partition (Section.rb:85-89).
  rows.each do |r|
    if r[:has_metadata] != (r[:id] < meta_count)
      failures << "metadata partition violated at id #{r[:id]} (#{r[:cpp_name]})"
    end
  end

  checkpoint_count = rows.count { |r| r[:num_checkpoints] > 0 }
  if checkpoint_count != artifact_consts['NUMBER_OF_BYTECODE_WITH_CHECKPOINTS']
    failures << "checkpoint count #{checkpoint_count} != NUMBER_OF_BYTECODE_WITH_CHECKPOINTS #{artifact_consts['NUMBER_OF_BYTECODE_WITH_CHECKPOINTS']}"
  end
  rows.each do |r|
    if (r[:num_checkpoints] > 0) != (r[:id] < checkpoint_count)
      failures << "checkpoint partition violated at id #{r[:id]} (#{r[:cpp_name]})"
    end
  end
  generated_checkpoint_counts = rows.take_while { |r| r[:num_checkpoints] > 0 }.map { |r| r[:num_checkpoints] }
  if generated_checkpoint_counts != artifact_checkpoint_counts
    failures << "per-op checkpoint counts #{generated_checkpoint_counts.inspect} != " \
                "artifact bytecodeCheckpointCountTable #{artifact_checkpoint_counts.inspect}"
  end

  max_length = rows.map { |r| r[:length] }.max
  if max_length != artifact_consts['MAX_LENGTH_OF_BYTECODE_IDS']
    failures << "max length #{max_length} != MAX_LENGTH_OF_BYTECODE_IDS #{artifact_consts['MAX_LENGTH_OF_BYTECODE_IDS']}"
  end

  # -------------------------------------------------------------------------
  # Verification 2: the ids already declared by hand in the Rust crate
  # (src/bytecode/instruction_stream.rs opcode_id) must appear identically.
  # -------------------------------------------------------------------------
  KNOWN_RUST_IDS = {
    'jmp' => 69, 'jtrue' => 70, 'ret' => 104, 'wide16' => 128, 'wide32' => 130,
    'enter' => 131, 'mov' => 144, 'eq' => 145, 'add' => 158, 'mul' => 159, 'sub' => 161,
  }.freeze
  KNOWN_RUST_IDS.each do |name, id|
    row = rows.find { |r| r[:name] == name }
    if row.nil?
      failures << "known Rust opcode #{name} not found in generated table"
    elsif row[:id] != id
      failures << "known Rust opcode #{name}: generated id #{row[:id]} != declared Rust id #{id}"
    end
  end

  unless failures.empty?
    warn "bytecode-gen: VERIFICATION FAILED (#{failures.length} finding(s)); no output written:"
    failures.each { |f| warn "  - #{f}" }
    exit 1
  end

  # -------------------------------------------------------------------------
  # Emit the Rust table.
  # -------------------------------------------------------------------------
  webkit_rev = `git -C #{WEBKIT_DIR} rev-parse HEAD 2>/dev/null`.strip
  webkit_rev = 'unknown' if webkit_rev.empty?

  out = +''
  out << <<~HEADER
    // DO NOT EDIT. Generated by tools/bytecode-gen/generate.rb, which runs JSC's
    // own Ruby bytecode generator (generator/DSL.rb pipeline) over JSC's
    // bytecode/BytecodeList.rb and emits the evaluated :Bytecode section.
    // Regenerate manually with:
    //
    //     ruby tools/bytecode-gen/generate.rb
    //
    // Inputs (read-only WebKit checkout):
    //   WebKit revision:  #{webkit_rev}
    //   Bytecode list:    #{BYTECODE_LIST}
    //   Generator:        #{GENERATOR_DIR}
    // Verified against the generated build artifact the measuring-instrument
    // C++ jsc was built from (id/name/length of every opcode, plus the four
    // NUMBER_OF_/MAX_LENGTH_OF_ constants):
    //   #{ARTIFACT_BYTECODES_H}
    //
    // ID assignment is JSC's verbatim: the :Bytecode section is
    // `preserve_order: true` (BytecodeList.rb:79-87), so ids are sequential
    // declaration positions (generator/DSL.rb:43-57, Section.rb:99-101,
    // Opcode.rb:41-47,59-61). Row `operands` are the STREAM args only;
    // `metadata: {}` fields never become operands -- they set `has_metadata`,
    // which contributes the single m_metadataID slot to the C++ opcodeLength
    // (generator/Opcode.rb:372-374), so
    // length = operands.len() + has_metadata as usize.
    // `OperandKind` is expected in scope at the inclusion site
    // (src/bytecode/instruction_stream.rs).

    /// One row per generated JS opcode, in id order. Field shape mirrors the
    /// hand-written `OpcodeDescriptor` rows in src/bytecode/instruction_stream.rs.
    #[derive(Clone, Copy, Debug)]
    pub struct GeneratedOpcodeRow {
        pub id: u8,
        pub name: &'static str,
        pub operands: &'static [OperandKind],
        pub has_metadata: bool,
        /// Checkpoint count for the checkpoint-carrying prefix ops
        /// (Bytecodes.h bytecodeCheckpointCountTable); 0 otherwise.
        pub num_checkpoints: u8,
    }

    /// Bytecodes.h `NUMBER_OF_BYTECODE_IDS`.
    pub const NUMBER_OF_BYTECODE_IDS: usize = #{rows.length};
    /// Bytecodes.h `MAX_LENGTH_OF_BYTECODE_IDS`.
    pub const MAX_LENGTH_OF_BYTECODE_IDS: usize = #{max_length};
    /// Bytecodes.h `NUMBER_OF_BYTECODE_WITH_METADATA`; `hasMetadata()` =
    /// `opcodeID < numberOfBytecodesWithMetadata` (Instruction.h:98-101).
    pub const NUMBER_OF_BYTECODE_WITH_METADATA: usize = #{meta_count};
    /// Bytecodes.h `NUMBER_OF_BYTECODE_WITH_CHECKPOINTS`.
    pub const NUMBER_OF_BYTECODE_WITH_CHECKPOINTS: usize = #{checkpoint_count};

  HEADER

  out << "#[rustfmt::skip]\n"
  out << "pub static GENERATED_OPCODE_TABLE: [GeneratedOpcodeRow; NUMBER_OF_BYTECODE_IDS] = [\n"
  rows.each do |row|
    operands = row[:operands].map { |v| "OperandKind::#{v}" }.join(', ')
    out << "    GeneratedOpcodeRow { id: #{row[:id]}, name: \"#{row[:name]}\", " \
           "operands: &[#{operands}], has_metadata: #{row[:has_metadata]}, " \
           "num_checkpoints: #{row[:num_checkpoints]} }, // length #{row[:length]}\n"
  end
  out << "];\n"

  FileUtils.mkdir_p(File.dirname(OUTPUT_RS))
  File.write(OUTPUT_RS, out)

  # -------------------------------------------------------------------------
  # Report.
  # -------------------------------------------------------------------------
  puts "bytecode-gen: VERIFIED #{rows.length}/#{artifact_triples.length} id/name/length triples " \
       "against #{ARTIFACT_BYTECODES_H}"
  puts "bytecode-gen: fresh DSL.run Bytecodes.h agrees with build artifact"
  puts "bytecode-gen: constants: NUMBER_OF_BYTECODE_IDS=#{rows.length} " \
       "NUMBER_OF_BYTECODE_WITH_METADATA=#{meta_count} " \
       "NUMBER_OF_BYTECODE_WITH_CHECKPOINTS=#{checkpoint_count} " \
       "MAX_LENGTH_OF_BYTECODE_IDS=#{max_length}"
  puts "bytecode-gen: per-op checkpoint counts match bytecodeCheckpointCountTable: #{generated_checkpoint_counts.inspect}"
  puts "bytecode-gen: known Rust ids verified: #{KNOWN_RUST_IDS.map { |n, i| "#{n}=#{i}" }.join(' ')}"
  puts 'bytecode-gen: operand-type census (C++ type -> OperandKind variant -> count):'
  census.sort_by { |(_t, _v), c| -c }.each do |(type, variant), count|
    puts format('  %-24s -> %-20s %4d', type, variant, count)
  end
  total_operands = census.values.sum
  puts "bytecode-gen: total stream operands: #{total_operands}"
  puts "bytecode-gen: wrote #{OUTPUT_RS} (#{rows.length} rows, #{File.size(OUTPUT_RS)} bytes)"
ensure
  FileUtils.remove_entry(scratch) if scratch && File.directory?(scratch)
end
