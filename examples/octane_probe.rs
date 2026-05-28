use std::env;
use std::fmt::Debug;
use std::fs;
use std::io::{self, Write};
use std::path::PathBuf;
use std::sync::Arc;

use javascriptcore::bytecode::CoreOpcode;
use javascriptcore::gc::CellId;
use javascriptcore::interpreter::{DispatchConfig, ExecutionCompletion};
use javascriptcore::runtime::CodeBlockId;
use javascriptcore::shell::octane::{
    execute_prepared_octane_benchmark, execute_prepared_octane_benchmark_with_progress,
    execute_prepared_octane_suite, execute_prepared_octane_suite_with_progress,
    octane_plan_by_name, prepare_octane_benchmark, prepare_octane_suite,
    OctaneBenchmarkExecutionReport, OctaneBenchmarkRunOverrides, OctaneExecutionConfig,
    OctaneExecutionMode, OctaneExecutionOutcome, OctaneExecutionProgress, OctanePreparationConfig,
    OctanePreparedBenchmark, OctanePreparedGeneratedSourceKind, OctanePreparedSourceOrderEntry,
    OctaneRunConfig, OctaneSuite, OctaneSuiteFailurePolicy,
};
use javascriptcore::syntax::{
    AstBuilder, ParseMode, Parser, ParserArena, SourceCode, SourceOrigin, SourcePosition,
    SourceProvider, SourceSpan, SourceText,
};
use javascriptcore::vm::{SourceSessionHostGlobalConfig, Vm, VmConfig};
use javascriptcore::vm::{
    VmOwnedCallTargetValidationOutcome, VmOwnedCallTargetValidationRejection,
    VmPropertyInlineCacheEvolutionDecision, VmPropertyInlineCacheEvolutionTerminalState,
};

fn main() {
    let mut jetstream_root = default_jetstream_root();
    let mut suite = OctaneSuite::Full;
    let mut mode = OctaneExecutionMode::InterpreterOnly;
    let mut failure_policy = OctaneSuiteFailurePolicy::FailFast;
    let mut iterations = None;
    let mut worst_case_count = None;
    let mut benchmark = None;
    let mut benchmark_eval = None;
    let mut dump_code_block = None;
    let mut dump_identifier = None;
    let mut eval = None;
    let mut eval_file = None;
    let mut eval_append = String::new();
    let mut progress = false;
    let mut tiering_summary = false;
    let mut dispatch_steps = None;

    let mut args = env::args().skip(1);
    while let Some(arg) = args.next() {
        match arg.as_str() {
            "--jetstream-root" => {
                let Some(value) = args.next() else {
                    usage_and_exit("--jetstream-root requires a path");
                };
                jetstream_root = PathBuf::from(value);
            }
            "--core" => suite = OctaneSuite::Core,
            "--full" => suite = OctaneSuite::Full,
            "--smoke" => {
                suite = OctaneSuite::Core;
                failure_policy = OctaneSuiteFailurePolicy::CollectAll;
                iterations = Some(5);
                worst_case_count = Some(1);
            }
            "--baseline" => mode = OctaneExecutionMode::BaselineAllowed,
            "--interpreter" => mode = OctaneExecutionMode::InterpreterOnly,
            "--benchmark" => {
                let Some(value) = args.next() else {
                    usage_and_exit("--benchmark requires an Octane benchmark name");
                };
                benchmark = Some(value);
            }
            "--benchmark-eval" => {
                let Some(value) = args.next() else {
                    usage_and_exit("--benchmark-eval requires source text");
                };
                benchmark_eval = Some(value);
            }
            "--dump-identifier" => {
                let Some(value) = args.next() else {
                    usage_and_exit("--dump-identifier requires an identifier index");
                };
                dump_identifier = Some(parse_u32("--dump-identifier", &value));
            }
            "--dump-code-block" => {
                let Some(value) = args.next() else {
                    usage_and_exit("--dump-code-block requires a CodeBlock CellId");
                };
                dump_code_block = Some(parse_u32("--dump-code-block", &value));
            }
            "--eval" => {
                let Some(value) = args.next() else {
                    usage_and_exit("--eval requires source text");
                };
                eval = Some(value);
            }
            "--eval-file" => {
                let Some(value) = args.next() else {
                    usage_and_exit("--eval-file requires a path");
                };
                eval_file = Some(PathBuf::from(value));
            }
            "--eval-append" => {
                let Some(value) = args.next() else {
                    usage_and_exit("--eval-append requires source text");
                };
                eval_append.push_str(&value);
            }
            "--fail-fast" => failure_policy = OctaneSuiteFailurePolicy::FailFast,
            "--collect-all" => failure_policy = OctaneSuiteFailurePolicy::CollectAll,
            "--progress" => progress = true,
            "--tiering-summary" => tiering_summary = true,
            "--official-iterations" => {
                iterations = None;
                worst_case_count = None;
            }
            "--iterations" => {
                let Some(value) = args.next() else {
                    usage_and_exit("--iterations requires a positive integer");
                };
                iterations = Some(parse_usize("--iterations", &value));
            }
            "--worst-case-count" => {
                let Some(value) = args.next() else {
                    usage_and_exit("--worst-case-count requires a positive integer");
                };
                worst_case_count = Some(parse_usize("--worst-case-count", &value));
            }
            "--dispatch-steps" => {
                let Some(value) = args.next() else {
                    usage_and_exit("--dispatch-steps requires an unsigned integer");
                };
                dispatch_steps = Some(parse_usize_allow_zero("--dispatch-steps", &value));
            }
            "--help" | "-h" => usage_and_exit(""),
            _ => usage_and_exit(&format!("unknown argument: {arg}")),
        }
    }

    if eval.is_some() && eval_file.is_some() {
        usage_and_exit("--eval and --eval-file are mutually exclusive");
    }
    if benchmark_eval.is_some() && benchmark.is_none() {
        usage_and_exit("--benchmark-eval requires --benchmark");
    }
    if benchmark_eval.is_some() && (eval.is_some() || eval_file.is_some()) {
        usage_and_exit("--benchmark-eval cannot be combined with --eval or --eval-file");
    }
    if dump_code_block.is_some()
        && benchmark_eval.is_none()
        && eval.is_none()
        && eval_file.is_none()
    {
        usage_and_exit(
            "--dump-code-block currently requires --benchmark-eval, --eval, or --eval-file",
        );
    }
    if !eval_append.is_empty() && eval.is_none() && eval_file.is_none() {
        usage_and_exit("--eval-append requires --eval or --eval-file");
    }
    if dispatch_steps.is_some() && (eval.is_some() || eval_file.is_some()) {
        usage_and_exit("--dispatch-steps is only supported for benchmark and suite runs");
    }

    let run = match (iterations, worst_case_count) {
        (Some(iterations), Some(worst_case_count)) => OctaneRunConfig::with_overrides(
            suite,
            OctaneBenchmarkRunOverrides::new(Some(iterations), Some(worst_case_count)),
        ),
        (None, None) => OctaneRunConfig::new(suite),
        _ => usage_and_exit("--iterations and --worst-case-count must be overridden together"),
    };

    if let Some(identifier) = dump_identifier {
        let Some(benchmark) = benchmark.as_deref() else {
            usage_and_exit("--dump-identifier requires --benchmark");
        };
        dump_source_identifier(&jetstream_root, benchmark, identifier);
        return;
    }

    if let Some(mut source) = eval {
        source.push_str(&eval_append);
        execute_eval_source(source, mode, tiering_summary, dump_code_block);
        return;
    }

    if let Some(path) = eval_file {
        let mut source = fs::read_to_string(&path).unwrap_or_else(|error| {
            eprintln!("failed to read {}: {error}", path.display());
            std::process::exit(2);
        });
        source.push_str(&eval_append);
        execute_eval_source(source, mode, tiering_summary, dump_code_block);
        return;
    }

    if let Some(benchmark) = benchmark {
        let Some(plan) = octane_plan_by_name(&benchmark) else {
            eprintln!("unknown Octane benchmark: {benchmark}");
            std::process::exit(2);
        };
        let prepared =
            match prepare_octane_benchmark(&jetstream_root, plan, run.effective_overrides()) {
                Ok(prepared) => prepared,
                Err(error) => {
                    eprintln!("prepare failed at {:?}: {error:?}", error.phase());
                    std::process::exit(2);
                }
            };
        let config = octane_execution_config(mode, failure_policy, dispatch_steps);
        if let Some(source) = benchmark_eval {
            execute_prepared_benchmark_eval_source(
                &prepared,
                source,
                mode,
                dispatch_steps,
                tiering_summary,
                dump_code_block,
            );
            return;
        }
        let report = if progress {
            let mut stderr = io::stderr().lock();
            let mut progress_printer = |event| print_progress_event(&mut stderr, event);
            execute_prepared_octane_benchmark_with_progress(
                &prepared,
                config,
                &mut progress_printer,
            )
        } else {
            execute_prepared_octane_benchmark(&prepared, config)
        };
        print_benchmark_report(&report);
        if tiering_summary {
            print_benchmark_tiering_summary(&report);
        }
        std::process::exit(if report.outcome.is_success() { 0 } else { 1 });
    }

    let prepared = match prepare_octane_suite(OctanePreparationConfig::new(jetstream_root, run)) {
        Ok(prepared) => prepared,
        Err(error) => {
            eprintln!("prepare failed at {:?}: {error:?}", error.phase());
            std::process::exit(2);
        }
    };

    let config = octane_execution_config(mode, failure_policy, dispatch_steps);
    let report = if progress {
        let mut stderr = io::stderr().lock();
        let mut progress_printer = |event| print_progress_event(&mut stderr, event);
        execute_prepared_octane_suite_with_progress(&prepared, config, &mut progress_printer)
    } else {
        execute_prepared_octane_suite(&prepared, config)
    };

    println!(
        "suite={:?} mode={:?} failure_policy={:?} stopped_early={}",
        report.suite, report.mode, report.failure_policy, report.stopped_early
    );

    for benchmark in &report.benchmarks {
        print_benchmark_report(benchmark);
        if tiering_summary {
            print_benchmark_tiering_summary(benchmark);
        }
    }

    if let Some(score) = report.suite_score {
        println!("suite score={}", score.score);
    }
}

fn execute_eval_source(
    source: String,
    mode: OctaneExecutionMode,
    tiering_summary: bool,
    dump_code_block: Option<u32>,
) {
    let config = match mode {
        OctaneExecutionMode::InterpreterOnly => VmConfig::default(),
        OctaneExecutionMode::BaselineAllowed => VmConfig::baseline_allowed(),
    };
    let mut vm = Vm::new(config);
    match vm.execute_source_session_with_host_globals(
        [source_code_from_text(source)],
        SourceSessionHostGlobalConfig::safe_benchmark_host_globals(),
    ) {
        Ok(execution) => {
            println!("{:?}", execution.completions());
            if !execution.host_output_records().is_empty() {
                println!("{:?}", execution.host_output_records());
            }
            if tiering_summary {
                print_tiering_summary(&vm);
            }
            if let Some(code_block) = dump_code_block {
                dump_vm_code_block(&vm, code_block);
            }
            std::process::exit(
                if matches!(
                    execution.completions(),
                    [javascriptcore::interpreter::ExecutionCompletion::Returned(
                        _
                    )]
                ) {
                    0
                } else {
                    1
                },
            );
        }
        Err(error) => {
            if tiering_summary {
                print_tiering_summary(&vm);
            }
            if let Some(code_block) = dump_code_block {
                dump_vm_code_block(&vm, code_block);
            }
            eprintln!("{error:?}");
            std::process::exit(1);
        }
    }
}

fn execute_prepared_benchmark_eval_source(
    prepared: &OctanePreparedBenchmark,
    source: String,
    mode: OctaneExecutionMode,
    dispatch_steps: Option<usize>,
    tiering_summary: bool,
    dump_code_block: Option<u32>,
) {
    let config = match mode {
        OctaneExecutionMode::InterpreterOnly => VmConfig::default(),
        OctaneExecutionMode::BaselineAllowed => VmConfig::baseline_allowed(),
    };
    let dispatch_config = dispatch_steps
        .map(DispatchConfig::new)
        .unwrap_or_else(DispatchConfig::default);
    let mut vm = Vm::new(config);
    let mut session = match vm.open_source_session_with_host_globals_and_dispatch_config(
        SourceSessionHostGlobalConfig::safe_benchmark_host_globals(),
        dispatch_config,
    ) {
        Ok(session) => session,
        Err(error) => {
            eprintln!("{error:?}");
            std::process::exit(1);
        }
    };

    for order_entry in prepared.source_order.iter().copied() {
        if order_entry
            == OctanePreparedSourceOrderEntry::Generated(OctanePreparedGeneratedSourceKind::Runner)
        {
            continue;
        }
        let prepared_source = match order_entry {
            OctanePreparedSourceOrderEntry::Generated(kind) => {
                prepared.generated_source(kind).map(|source| &source.source)
            }
            OctanePreparedSourceOrderEntry::BenchmarkFile(index) => prepared
                .benchmark_sources
                .get(index)
                .map(|source| &source.source),
        };
        let Some(prepared_source) = prepared_source else {
            eprintln!("missing prepared source for {order_entry:?}");
            std::process::exit(1);
        };
        if let Err(error) = vm.append_source_session_source(&mut session, prepared_source.clone()) {
            if tiering_summary {
                print_tiering_summary(&vm);
            }
            if let Some(code_block) = dump_code_block {
                dump_vm_code_block(&vm, code_block);
            }
            eprintln!("{error:?}");
            std::process::exit(1);
        }
    }

    if let Err(error) = vm.append_source_session_source(&mut session, source_code_from_text(source))
    {
        if tiering_summary {
            print_tiering_summary(&vm);
        }
        if let Some(code_block) = dump_code_block {
            dump_vm_code_block(&vm, code_block);
        }
        eprintln!("{error:?}");
        std::process::exit(1);
    }

    let execution = session.finish();
    println!("{:?}", execution.completions());
    if !execution.host_output_records().is_empty() {
        println!("{:?}", execution.host_output_records());
    }
    if tiering_summary {
        print_tiering_summary(&vm);
    }
    if let Some(code_block) = dump_code_block {
        dump_vm_code_block(&vm, code_block);
    }
    std::process::exit(
        if matches!(
            execution.completions().last(),
            Some(ExecutionCompletion::Returned(_))
        ) {
            0
        } else {
            1
        },
    );
}

fn octane_execution_config(
    mode: OctaneExecutionMode,
    failure_policy: OctaneSuiteFailurePolicy,
    dispatch_steps: Option<usize>,
) -> OctaneExecutionConfig {
    let config = OctaneExecutionConfig::new(mode, failure_policy);
    match dispatch_steps {
        Some(max_steps) => config.with_dispatch_config(DispatchConfig::new(max_steps)),
        None => config,
    }
}

fn dump_vm_code_block(vm: &Vm, raw_cell_id: u32) {
    let owner = CodeBlockId(CellId(raw_cell_id));
    let mut stderr = io::stderr().lock();
    let Some(code_block) = vm.code_block_for_diagnostics(owner) else {
        let _ = writeln!(stderr, "code-block-dump owner={owner:?} missing");
        return;
    };
    let unlinked = code_block.unlinked();
    let _ = writeln!(
        stderr,
        "code-block-dump owner={owner:?} kind={:?} link_context={:?} lifecycle={:?} frame={:?} source={:?} executable={:?} features={:?}",
        unlinked.kind(),
        code_block.link_context(),
        code_block.lifecycle(),
        unlinked.frame(),
        unlinked.source(),
        unlinked.executable_info(),
        unlinked.features()
    );
    for decoded in unlinked.instructions().decoded_instructions() {
        match decoded {
            Ok(instruction) => {
                let opcode = CoreOpcode::from_opcode(instruction.opcode);
                let source = unlinked
                    .side_tables()
                    .source_notes
                    .lookup(instruction.bytecode_index)
                    .map(|note| {
                        format!(
                            " source_offset={} line={} column={} range={}..{}",
                            note.position.offset,
                            note.position.line,
                            note.position.column,
                            note.range.start.offset,
                            note.range.end.offset
                        )
                    })
                    .unwrap_or_default();
                let _ = writeln!(
                    stderr,
                    "code-block-dump instruction bytecode_index={:?} opcode={:?} core_opcode={:?} operands={:?}{}",
                    instruction.bytecode_index,
                    instruction.opcode,
                    opcode,
                    instruction.operands,
                    source
                );
            }
            Err(error) => {
                let _ = writeln!(stderr, "code-block-dump decode-error {error:?}");
            }
        }
    }
}

fn print_tiering_summary(vm: &Vm) {
    let tiering = vm.tiering_integration();
    let tail_limit = tiering_tail_record_limit();
    let record_filter = tiering_record_filter();
    let entry_decisions = tiering.entry_decisions();
    let fallback_records = tiering.fallback_records();
    let diagnostics = tiering.diagnostics();
    let profile_records = tiering.profile_records();
    let profile_entry_count = tiering.profile_entry_count();
    let profile_loop_backedge_count = tiering.profile_loop_backedge_count();
    let baseline_installs = tiering.baseline_install_records();
    let baseline_entry_artifacts = tiering.baseline_entry_artifacts();
    let baseline_materializations = tiering.baseline_executable_materializations();
    let baseline_generated_code = tiering.baseline_generated_code_artifacts();
    let baseline_generated_executions = tiering.baseline_generated_execution_records();
    let baseline_generated_execution_summaries = tiering.baseline_generated_execution_summaries();
    let baseline_generated_execution_count = tiering.baseline_generated_execution_count();
    let baseline_generated_executed_bytecodes =
        tiering.baseline_generated_executed_bytecode_count();
    let baseline_auto_materializations = tiering.baseline_entry_auto_materializations();
    let baseline_native_lowering_failures = tiering.baseline_native_lowering_failure_count();
    let baseline_native_semantic_byte_emission_failures =
        tiering.baseline_native_semantic_byte_emission_failure_count();
    let generated_direct_call_transactions = tiering.generated_direct_call_transaction_records();
    let generated_direct_call_transaction_summaries =
        tiering.generated_direct_call_transaction_summaries();
    let generated_direct_call_transaction_count = tiering.generated_direct_call_transaction_count();
    let generated_direct_call_generated_entries =
        tiering.generated_direct_call_generated_entry_count();
    let generated_direct_call_native_entries = tiering.generated_direct_call_native_entry_count();
    let generated_direct_call_native_interpreter_fallbacks =
        tiering.generated_direct_call_native_interpreter_fallback_count();
    let generated_direct_call_nested_interpreter_fallbacks =
        tiering.generated_direct_call_nested_interpreter_fallback_count();
    let generated_direct_call_hot_slot_hits = tiering.generated_direct_call_hot_slot_hit_count();
    let generated_direct_call_sidecar_hot_slot_hits =
        tiering.generated_direct_call_sidecar_hot_slot_hit_count();
    let generated_direct_call_preferred_route_hits =
        tiering.generated_direct_call_preferred_route_hit_count();
    let generated_direct_call_rootless_generated_entries =
        tiering.generated_direct_call_rootless_generated_entry_count();
    let generated_direct_call_rootless_generated_entry_proof_cache_hits =
        tiering.generated_direct_call_rootless_generated_entry_proof_cache_hit_count();
    let generated_direct_call_rootless_native_entries =
        tiering.generated_direct_call_rootless_native_entry_count();
    let generated_direct_call_rootless_rejections =
        tiering.generated_direct_call_rootless_rejection_counts();
    let generated_direct_call_rootless_native_entry_rejections =
        tiering.generated_direct_call_rootless_native_entry_rejection_counts();
    let generated_direct_call_rootless_unsupported_body_opcode_counts =
        tiering.generated_direct_call_rootless_unsupported_body_opcode_counts();
    let generated_direct_call_rootless_native_entry_unsupported_body_opcode_counts =
        tiering.generated_direct_call_rootless_native_entry_unsupported_body_opcode_counts();
    let generated_direct_call_rootless_native_entry_retained_side_exit_counts =
        tiering.generated_direct_call_rootless_native_entry_retained_side_exit_counts();
    let generated_direct_call_rootless_preferred_native_entry_counts =
        tiering.generated_direct_call_rootless_preferred_native_entry_counts();
    let call_observations = tiering.call_observations();
    let vm_owned_call_target_validations = tiering.vm_owned_call_target_validation_records();
    let vm_owned_call_target_validation_counts =
        vm_owned_call_target_validation_counts(vm_owned_call_target_validations);
    let call_link_readiness = tiering.call_link_readiness_records();
    let call_link_descriptors = tiering.call_link_descriptor_records();
    let call_link_boundary_validations = tiering.call_link_boundary_validation_records();
    let call_link_attachment_plans = tiering.call_link_attachment_plan_records();
    let call_link_attachment_rechecks = tiering.call_link_attachment_install_rechecks();
    let call_link_inline_cache_attachments = tiering.call_link_inline_cache_attachment_records();
    let generated_call_link_probe_misses = tiering.generated_call_link_probe_misses();
    let generated_call_link_probe_blocked = tiering.generated_call_link_probe_blocked_records();
    let property_load_observations = tiering.property_load_observations();
    let property_store_observations = tiering.property_store_observations();
    let property_load_access_case_plans = tiering.property_load_access_case_plans();
    let property_store_access_case_plans = tiering.property_store_access_case_plans();
    let property_store_access_case_rechecks = tiering.property_store_access_case_install_rechecks();
    let property_load_guard_plans = tiering.property_load_guard_plans();
    let property_load_guard_dependencies = tiering.property_load_guard_dependencies();
    let property_load_guard_rechecks = tiering.property_load_guard_install_rechecks();
    let property_load_guard_watchpoint_materializations =
        tiering.property_load_guard_watchpoint_materializations();
    let property_load_guard_watchpoint_invalidations =
        tiering.property_load_guard_watchpoint_invalidations();
    let property_load_guard_watchpoint_event_dispatches =
        tiering.property_load_guard_watchpoint_event_dispatches();
    let generated_property_load_probe_misses = tiering.generated_property_load_probe_misses();
    let property_inline_cache_evolution = tiering.property_inline_cache_evolution_records();
    let (
        property_inline_cache_evolution_admitted,
        property_inline_cache_evolution_buffered,
        property_inline_cache_evolution_buffered_duplicates,
        property_inline_cache_evolution_cooldowns,
        property_inline_cache_evolution_final_gave_up,
        property_inline_cache_evolution_gave_up_skips,
        property_inline_cache_evolution_generated_megamorphic_load,
        property_inline_cache_evolution_megamorphic_load_skips,
        property_inline_cache_evolution_generated_megamorphic_store,
        property_inline_cache_evolution_megamorphic_store_skips,
        property_inline_cache_evolution_generated_megamorphic_has,
        property_inline_cache_evolution_megamorphic_has_skips,
    ) = property_inline_cache_evolution_counts(property_inline_cache_evolution);
    let generated_guarded_property_load_probe_misses =
        tiering.generated_guarded_property_load_probe_misses();
    let generated_property_store_probe_misses = tiering.generated_property_store_probe_misses();
    let generated_property_store_mutation_readiness =
        tiering.generated_property_store_mutation_readiness_records();
    let generated_property_store_mutation_rejections =
        tiering.generated_property_store_mutation_rejections();
    let property_inline_cache_attachments = tiering.property_inline_cache_attachment_records();
    let property_inline_cache_clears = tiering.property_inline_cache_clear_records();
    let structure_stub_repatch_transactions = tiering.structure_stub_repatch_transactions();
    let structure_stub_access_case_links = tiering.structure_stub_access_case_link_records();
    let baseline_native_entry_readiness = tiering.baseline_native_entry_readiness_records();
    let baseline_machine_code_emission_provenance =
        tiering.baseline_machine_code_emission_provenance_records();
    let launch_descriptors = vm.entry_state().launch_descriptors();
    let mut stderr = io::stderr().lock();
    let _ = writeln!(
        stderr,
        "tiering-summary entry_decisions={} fallbacks={} diagnostics={} profile_records={} profile_entries={} profile_loop_backedges={} baseline_installs={} baseline_entry_artifacts={} baseline_materializations={} baseline_generated_code_artifacts={} baseline_generated_executions={} baseline_generated_executed_bytecodes={} baseline_entry_auto_materializations={} baseline_native_lowering_failures={} baseline_native_semantic_byte_emission_failures={} generated_direct_call_transactions={} generated_direct_call_generated_entries={} generated_direct_call_native_entries={} generated_direct_call_native_interpreter_fallbacks={} generated_direct_call_nested_interpreter_fallbacks={} generated_direct_call_hot_slot_hits={} generated_direct_call_sidecar_hot_slot_hits={} generated_direct_call_preferred_route_hits={} generated_direct_call_rootless_generated_entries={} generated_direct_call_rootless_generated_entry_proof_cache_hits={} generated_direct_call_rootless_native_entries={} generated_direct_call_rootless_rejections={} generated_direct_call_rootless_reject_hot_slot_miss={} generated_direct_call_rootless_reject_preferred_route_not_generated_entry={} generated_direct_call_rootless_reject_missing_generated_artifact={} generated_direct_call_rootless_reject_invalid_generated_artifact={} generated_direct_call_rootless_reject_snapshot_mismatch={} generated_direct_call_rootless_reject_runtime_helper_plan={} generated_direct_call_rootless_reject_effect_contract={} generated_direct_call_rootless_reject_retained_side_exit={} generated_direct_call_rootless_reject_retained_runtime_helper_exit={} generated_direct_call_rootless_reject_retained_js_call_exit={} generated_direct_call_rootless_reject_retained_loop_backedge_exit={} generated_direct_call_rootless_reject_invalid_body_instruction={} generated_direct_call_rootless_reject_invalid_body_bytecode_index={} generated_direct_call_rootless_reject_unsupported_body_opcode={} generated_direct_call_rootless_reject_missing_return={} generated_direct_call_rootless_native_entry_rejections={} generated_direct_call_rootless_native_entry_reject_hot_slot_miss={} generated_direct_call_rootless_native_entry_reject_preferred_route_not_generated_entry={} generated_direct_call_rootless_native_entry_reject_missing_generated_artifact={} generated_direct_call_rootless_native_entry_reject_invalid_generated_artifact={} generated_direct_call_rootless_native_entry_reject_snapshot_mismatch={} generated_direct_call_rootless_native_entry_reject_runtime_helper_plan={} generated_direct_call_rootless_native_entry_reject_effect_contract={} generated_direct_call_rootless_native_entry_reject_retained_side_exit={} generated_direct_call_rootless_native_entry_reject_retained_runtime_helper_exit={} generated_direct_call_rootless_native_entry_reject_retained_js_call_exit={} generated_direct_call_rootless_native_entry_reject_retained_loop_backedge_exit={} generated_direct_call_rootless_native_entry_reject_invalid_body_instruction={} generated_direct_call_rootless_native_entry_reject_invalid_body_bytecode_index={} generated_direct_call_rootless_native_entry_reject_unsupported_body_opcode={} generated_direct_call_rootless_native_entry_reject_missing_return={} generated_direct_call_rootless_preferred_native_entry={} generated_direct_call_rootless_preferred_native_entry_pure_baseline_shim={} generated_direct_call_rootless_preferred_native_entry_emitted_semantic_c_abi_entry={} generated_direct_call_rootless_preferred_native_entry_unknown={} launch_descriptors={} call_observations={} vm_owned_call_target_validations={} vm_owned_call_target_accepted={} vm_owned_call_target_rejected={} vm_owned_call_target_reject_not_registered={} vm_owned_call_target_reject_request_snapshot={} vm_owned_call_target_reject_registered_snapshot={} vm_owned_call_target_reject_snapshot_mismatch={} vm_owned_call_target_reject_not_live={} vm_owned_call_target_reject_request_specialization={} vm_owned_call_target_reject_registered_specialization={} vm_owned_call_target_reject_missing_executable={} vm_owned_call_target_reject_executable_not_registered={} vm_owned_call_target_reject_executable_installed_missing={} vm_owned_call_target_reject_executable_installed_mismatch={} vm_owned_call_target_reject_executable_mismatch={} call_link_readiness={} call_link_descriptors={} call_link_boundary_validations={} call_link_attachment_plans={} call_link_attachment_rechecks={} call_link_inline_cache_attachments={} generated_call_link_probe_misses={} generated_call_link_probe_blocked={} property_load_observations={} property_store_observations={} property_load_access_case_plans={} property_store_access_case_plans={} property_store_access_case_rechecks={} property_load_guard_plans={} property_load_guard_rechecks={}",
        entry_decisions.len(),
        fallback_records.len(),
        diagnostics.len(),
        profile_records.len(),
        profile_entry_count,
        profile_loop_backedge_count,
        baseline_installs.len(),
        baseline_entry_artifacts.len(),
        baseline_materializations.len(),
        baseline_generated_code.len(),
        baseline_generated_execution_count,
        baseline_generated_executed_bytecodes,
        baseline_auto_materializations.len(),
        baseline_native_lowering_failures,
        baseline_native_semantic_byte_emission_failures,
        generated_direct_call_transaction_count,
        generated_direct_call_generated_entries,
        generated_direct_call_native_entries,
        generated_direct_call_native_interpreter_fallbacks,
        generated_direct_call_nested_interpreter_fallbacks,
        generated_direct_call_hot_slot_hits,
        generated_direct_call_sidecar_hot_slot_hits,
        generated_direct_call_preferred_route_hits,
        generated_direct_call_rootless_generated_entries,
        generated_direct_call_rootless_generated_entry_proof_cache_hits,
        generated_direct_call_rootless_native_entries,
        generated_direct_call_rootless_rejections.total(),
        generated_direct_call_rootless_rejections.hot_slot_miss,
        generated_direct_call_rootless_rejections.preferred_route_not_generated_entry,
        generated_direct_call_rootless_rejections.missing_generated_artifact,
        generated_direct_call_rootless_rejections.invalid_generated_artifact,
        generated_direct_call_rootless_rejections.snapshot_mismatch,
        generated_direct_call_rootless_rejections.runtime_helper_plan,
        generated_direct_call_rootless_rejections.effect_contract,
        generated_direct_call_rootless_rejections.retained_side_exit,
        generated_direct_call_rootless_rejections.retained_runtime_helper_exit,
        generated_direct_call_rootless_rejections.retained_js_call_exit,
        generated_direct_call_rootless_rejections.retained_loop_backedge_exit,
        generated_direct_call_rootless_rejections.invalid_body_instruction,
        generated_direct_call_rootless_rejections.invalid_body_bytecode_index,
        generated_direct_call_rootless_rejections.unsupported_body_opcode,
        generated_direct_call_rootless_rejections.missing_return,
        generated_direct_call_rootless_native_entry_rejections.total(),
        generated_direct_call_rootless_native_entry_rejections.hot_slot_miss,
        generated_direct_call_rootless_native_entry_rejections.preferred_route_not_generated_entry,
        generated_direct_call_rootless_native_entry_rejections.missing_generated_artifact,
        generated_direct_call_rootless_native_entry_rejections.invalid_generated_artifact,
        generated_direct_call_rootless_native_entry_rejections.snapshot_mismatch,
        generated_direct_call_rootless_native_entry_rejections.runtime_helper_plan,
        generated_direct_call_rootless_native_entry_rejections.effect_contract,
        generated_direct_call_rootless_native_entry_rejections.retained_side_exit,
        generated_direct_call_rootless_native_entry_rejections.retained_runtime_helper_exit,
        generated_direct_call_rootless_native_entry_rejections.retained_js_call_exit,
        generated_direct_call_rootless_native_entry_rejections.retained_loop_backedge_exit,
        generated_direct_call_rootless_native_entry_rejections.invalid_body_instruction,
        generated_direct_call_rootless_native_entry_rejections.invalid_body_bytecode_index,
        generated_direct_call_rootless_native_entry_rejections.unsupported_body_opcode,
        generated_direct_call_rootless_native_entry_rejections.missing_return,
        generated_direct_call_rootless_preferred_native_entry_counts.total(),
        generated_direct_call_rootless_preferred_native_entry_counts.pure_baseline_shim,
        generated_direct_call_rootless_preferred_native_entry_counts.emitted_semantic_c_abi_entry,
        generated_direct_call_rootless_preferred_native_entry_counts.unknown,
        launch_descriptors.len(),
        call_observations.len(),
        vm_owned_call_target_validations.len(),
        vm_owned_call_target_validation_counts.accepted,
        vm_owned_call_target_validation_counts.rejected,
        vm_owned_call_target_validation_counts.target_code_block_not_registered,
        vm_owned_call_target_validation_counts.request_snapshot_unavailable,
        vm_owned_call_target_validation_counts.registered_snapshot_unavailable,
        vm_owned_call_target_validation_counts.snapshot_mismatch,
        vm_owned_call_target_validation_counts.target_code_block_not_live,
        vm_owned_call_target_validation_counts.request_specialization_mismatch,
        vm_owned_call_target_validation_counts.registered_specialization_mismatch,
        vm_owned_call_target_validation_counts.missing_registered_executable,
        vm_owned_call_target_validation_counts.executable_not_registered,
        vm_owned_call_target_validation_counts.executable_installed_code_missing,
        vm_owned_call_target_validation_counts.executable_installed_code_block_mismatch,
        vm_owned_call_target_validation_counts.request_executable_mismatch,
        call_link_readiness.len(),
        call_link_descriptors.len(),
        call_link_boundary_validations.len(),
        call_link_attachment_plans.len(),
        call_link_attachment_rechecks.len(),
        call_link_inline_cache_attachments.len(),
        generated_call_link_probe_misses.len(),
        generated_call_link_probe_blocked.len(),
        property_load_observations.len(),
        property_store_observations.len(),
        property_load_access_case_plans.len(),
        property_store_access_case_plans.len(),
        property_store_access_case_rechecks.len(),
        property_load_guard_plans.len(),
        property_load_guard_rechecks.len()
    );
    let _ = writeln!(
        stderr,
        "tiering-hidden-summary record_ordinals={} baseline_native_entry_readiness={} baseline_machine_code_emission_provenance={} property_load_guard_dependencies={} property_load_guard_watchpoint_materializations={} property_load_guard_watchpoint_invalidations={} property_load_guard_watchpoint_event_dispatches={} property_inline_cache_evolution_records={} property_inline_cache_evolution_admitted={} property_inline_cache_evolution_buffered={} property_inline_cache_evolution_buffered_duplicates={} property_inline_cache_evolution_cooldowns={} property_inline_cache_evolution_final_gave_up={} property_inline_cache_evolution_gave_up_skips={} property_inline_cache_evolution_generated_megamorphic_load={} property_inline_cache_evolution_megamorphic_load_skips={} property_inline_cache_evolution_generated_megamorphic_store={} property_inline_cache_evolution_megamorphic_store_skips={} property_inline_cache_evolution_generated_megamorphic_has={} property_inline_cache_evolution_megamorphic_has_skips={} property_load_megamorphic_cache_records={} property_load_megamorphic_cache_current_entries={} property_load_megamorphic_cache_epoch={} property_store_megamorphic_cache_records={} property_store_megamorphic_cache_current_entries={} property_store_megamorphic_cache_epoch={} property_has_megamorphic_cache_records={} property_has_megamorphic_cache_current_entries={} property_has_megamorphic_cache_epoch={} generated_property_load_probe_misses={} generated_property_load_probe_miss_records={} generated_guarded_property_load_probe_misses={} generated_guarded_property_load_probe_miss_records={} generated_property_store_probe_misses={} generated_property_store_probe_miss_records={} generated_property_store_mutation_readiness={} generated_property_store_mutation_rejections={} property_inline_cache_attachments={} property_inline_cache_clears={} structure_stub_repatch_transactions={} structure_stub_access_case_links={}",
        tiering.record_ordinal_count(),
        baseline_native_entry_readiness.len(),
        baseline_machine_code_emission_provenance.len(),
        property_load_guard_dependencies.len(),
        property_load_guard_watchpoint_materializations.len(),
        property_load_guard_watchpoint_invalidations.len(),
        property_load_guard_watchpoint_event_dispatches.len(),
        property_inline_cache_evolution.len(),
        property_inline_cache_evolution_admitted,
        property_inline_cache_evolution_buffered,
        property_inline_cache_evolution_buffered_duplicates,
        property_inline_cache_evolution_cooldowns,
        property_inline_cache_evolution_final_gave_up,
        property_inline_cache_evolution_gave_up_skips,
        property_inline_cache_evolution_generated_megamorphic_load,
        property_inline_cache_evolution_megamorphic_load_skips,
        property_inline_cache_evolution_generated_megamorphic_store,
        property_inline_cache_evolution_megamorphic_store_skips,
        property_inline_cache_evolution_generated_megamorphic_has,
        property_inline_cache_evolution_megamorphic_has_skips,
        tiering.property_load_megamorphic_cache_records().len(),
        tiering.property_load_megamorphic_cache_current_entry_count(),
        tiering.property_load_megamorphic_cache_epoch(),
        tiering.property_store_megamorphic_cache_records().len(),
        tiering.property_store_megamorphic_cache_current_entry_count(),
        tiering.property_store_megamorphic_cache_epoch(),
        tiering.property_has_megamorphic_cache_records().len(),
        tiering.property_has_megamorphic_cache_current_entry_count(),
        tiering.property_has_megamorphic_cache_epoch(),
        tiering.generated_property_load_probe_miss_count(),
        generated_property_load_probe_misses.len(),
        tiering.generated_guarded_property_load_probe_miss_count(),
        generated_guarded_property_load_probe_misses.len(),
        tiering.generated_property_store_probe_miss_count(),
        generated_property_store_probe_misses.len(),
        generated_property_store_mutation_readiness.len(),
        generated_property_store_mutation_rejections.len(),
        property_inline_cache_attachments.len(),
        property_inline_cache_clears.len(),
        structure_stub_repatch_transactions.len(),
        structure_stub_access_case_links.len()
    );
    let mut unsupported_body_opcode_counts =
        generated_direct_call_rootless_unsupported_body_opcode_counts.to_vec();
    unsupported_body_opcode_counts.sort_by(|left, right| {
        right
            .count
            .cmp(&left.count)
            .then_with(|| format!("{:?}", left.opcode).cmp(&format!("{:?}", right.opcode)))
    });
    for count in unsupported_body_opcode_counts {
        let _ = writeln!(
            stderr,
            "generated-direct-call-rootless-unsupported-body-opcode opcode={:?} count={}",
            count.opcode, count.count
        );
    }
    let mut native_entry_unsupported_body_opcode_counts =
        generated_direct_call_rootless_native_entry_unsupported_body_opcode_counts.to_vec();
    native_entry_unsupported_body_opcode_counts.sort_by(|left, right| {
        right
            .count
            .cmp(&left.count)
            .then_with(|| format!("{:?}", left.opcode).cmp(&format!("{:?}", right.opcode)))
    });
    for count in native_entry_unsupported_body_opcode_counts {
        let _ = writeln!(
            stderr,
            "generated-direct-call-rootless-native-entry-unsupported-body-opcode opcode={:?} count={}",
            count.opcode, count.count
        );
    }
    let mut native_entry_retained_side_exit_counts =
        generated_direct_call_rootless_native_entry_retained_side_exit_counts.to_vec();
    native_entry_retained_side_exit_counts.sort_by(|left, right| {
        right
            .count
            .cmp(&left.count)
            .then_with(|| {
                format!("{:?}", left.target_code_block)
                    .cmp(&format!("{:?}", right.target_code_block))
            })
            .then_with(|| left.bytecode_index.cmp(&right.bytecode_index))
            .then_with(|| format!("{:?}", left.opcode).cmp(&format!("{:?}", right.opcode)))
            .then_with(|| format!("{:?}", left.reason).cmp(&format!("{:?}", right.reason)))
    });
    for count in native_entry_retained_side_exit_counts {
        let _ = writeln!(
            stderr,
            "generated-direct-call-rootless-native-entry-retained-side-exit target={:?} bytecode_index={:?} opcode={:?} reason={:?} count={}",
            count.target_code_block,
            count.bytecode_index,
            count.opcode,
            count.reason,
            count.count
        );
    }
    print_top_records(
        &mut stderr,
        "baseline-generated-execution-summary",
        baseline_generated_execution_summaries,
        tail_limit,
        record_filter.as_deref(),
        |left, right| {
            right
                .executed_bytecode_count
                .cmp(&left.executed_bytecode_count)
                .then_with(|| right.execution_count.cmp(&left.execution_count))
                .then_with(|| format!("{:?}", left.owner).cmp(&format!("{:?}", right.owner)))
        },
    );
    print_top_records(
        &mut stderr,
        "generated-direct-call-transaction-summary",
        generated_direct_call_transaction_summaries,
        tail_limit,
        record_filter.as_deref(),
        |left, right| {
            right
                .transaction_count
                .cmp(&left.transaction_count)
                .then_with(|| format!("{:?}", left.caller).cmp(&format!("{:?}", right.caller)))
                .then_with(|| {
                    left.call_bytecode_index
                        .as_bits()
                        .cmp(&right.call_bytecode_index.as_bits())
                })
                .then_with(|| {
                    format!("{:?}", left.target_code_block)
                        .cmp(&format!("{:?}", right.target_code_block))
                })
                .then_with(|| format!("{:?}", left.route).cmp(&format!("{:?}", right.route)))
        },
    );
    print_tail_records(
        &mut stderr,
        "tiering-entry-decision",
        entry_decisions,
        tail_limit,
        record_filter.as_deref(),
    );
    print_tail_records(
        &mut stderr,
        "tiering-fallback",
        fallback_records,
        tail_limit,
        record_filter.as_deref(),
    );
    print_tail_records(
        &mut stderr,
        "tiering-diagnostic",
        diagnostics,
        tail_limit,
        record_filter.as_deref(),
    );
    print_tail_records(
        &mut stderr,
        "tiering-profile",
        profile_records,
        tail_limit,
        record_filter.as_deref(),
    );
    print_tail_records(
        &mut stderr,
        "baseline-install",
        baseline_installs,
        tail_limit,
        record_filter.as_deref(),
    );
    print_tail_records(
        &mut stderr,
        "baseline-entry-auto-materialization",
        baseline_auto_materializations,
        tail_limit,
        record_filter.as_deref(),
    );
    print_tail_records(
        &mut stderr,
        "baseline-generated-execution",
        baseline_generated_executions,
        tail_limit,
        record_filter.as_deref(),
    );
    print_tail_records(
        &mut stderr,
        "generated-direct-call-transaction",
        generated_direct_call_transactions,
        tail_limit,
        record_filter.as_deref(),
    );
    print_tail_records(
        &mut stderr,
        "call-observation",
        call_observations,
        tail_limit,
        record_filter.as_deref(),
    );
    print_tail_records(
        &mut stderr,
        "vm-owned-call-target-validation",
        vm_owned_call_target_validations,
        tail_limit,
        record_filter.as_deref(),
    );
    print_tail_records(
        &mut stderr,
        "call-link-readiness",
        call_link_readiness,
        tail_limit,
        record_filter.as_deref(),
    );
    print_tail_records(
        &mut stderr,
        "call-link-descriptor",
        call_link_descriptors,
        tail_limit,
        record_filter.as_deref(),
    );
    print_tail_records(
        &mut stderr,
        "call-link-boundary-validation",
        call_link_boundary_validations,
        tail_limit,
        record_filter.as_deref(),
    );
    print_tail_records(
        &mut stderr,
        "call-link-attachment-plan",
        call_link_attachment_plans,
        tail_limit,
        record_filter.as_deref(),
    );
    print_tail_records(
        &mut stderr,
        "call-link-attachment-recheck",
        call_link_attachment_rechecks,
        tail_limit,
        record_filter.as_deref(),
    );
    print_tail_records(
        &mut stderr,
        "call-link-inline-cache-attachment",
        call_link_inline_cache_attachments,
        tail_limit,
        record_filter.as_deref(),
    );
    print_tail_records(
        &mut stderr,
        "generated-call-link-probe-miss",
        generated_call_link_probe_misses,
        tail_limit,
        record_filter.as_deref(),
    );
    print_tail_records(
        &mut stderr,
        "generated-call-link-probe-blocked",
        generated_call_link_probe_blocked,
        tail_limit,
        record_filter.as_deref(),
    );
    print_tail_records(
        &mut stderr,
        "property-load-observation",
        property_load_observations,
        tail_limit,
        record_filter.as_deref(),
    );
    print_tail_records(
        &mut stderr,
        "property-store-observation",
        property_store_observations,
        tail_limit,
        record_filter.as_deref(),
    );
    print_tail_records(
        &mut stderr,
        "property-load-access-case-plan",
        property_load_access_case_plans,
        tail_limit,
        record_filter.as_deref(),
    );
    print_tail_records(
        &mut stderr,
        "property-store-access-case-plan",
        property_store_access_case_plans,
        tail_limit,
        record_filter.as_deref(),
    );
    print_tail_records(
        &mut stderr,
        "property-store-access-case-recheck",
        property_store_access_case_rechecks,
        tail_limit,
        record_filter.as_deref(),
    );
    print_tail_records(
        &mut stderr,
        "property-load-guard-plan",
        property_load_guard_plans,
        tail_limit,
        record_filter.as_deref(),
    );
    print_tail_records(
        &mut stderr,
        "property-load-guard-recheck",
        property_load_guard_rechecks,
        tail_limit,
        record_filter.as_deref(),
    );
    print_tail_records(
        &mut stderr,
        "property-load-guard-dependency",
        property_load_guard_dependencies,
        tail_limit,
        record_filter.as_deref(),
    );
    print_tail_records(
        &mut stderr,
        "property-load-guard-watchpoint-materialization",
        property_load_guard_watchpoint_materializations,
        tail_limit,
        record_filter.as_deref(),
    );
    print_tail_records(
        &mut stderr,
        "property-load-guard-watchpoint-invalidation",
        property_load_guard_watchpoint_invalidations,
        tail_limit,
        record_filter.as_deref(),
    );
    print_tail_records(
        &mut stderr,
        "property-load-guard-watchpoint-event-dispatch",
        property_load_guard_watchpoint_event_dispatches,
        tail_limit,
        record_filter.as_deref(),
    );
    print_tail_records(
        &mut stderr,
        "property-inline-cache-evolution",
        property_inline_cache_evolution,
        tail_limit,
        record_filter.as_deref(),
    );
    print_tail_records(
        &mut stderr,
        "generated-property-load-probe-miss",
        generated_property_load_probe_misses,
        tail_limit,
        record_filter.as_deref(),
    );
    print_tail_records(
        &mut stderr,
        "generated-guarded-property-load-probe-miss",
        generated_guarded_property_load_probe_misses,
        tail_limit,
        record_filter.as_deref(),
    );
    print_tail_records(
        &mut stderr,
        "generated-property-store-probe-miss",
        generated_property_store_probe_misses,
        tail_limit,
        record_filter.as_deref(),
    );
    print_tail_records(
        &mut stderr,
        "generated-property-store-mutation-readiness",
        generated_property_store_mutation_readiness,
        tail_limit,
        record_filter.as_deref(),
    );
    print_tail_records(
        &mut stderr,
        "generated-property-store-mutation-rejection",
        generated_property_store_mutation_rejections,
        tail_limit,
        record_filter.as_deref(),
    );
    print_tail_records(
        &mut stderr,
        "property-inline-cache-attachment",
        property_inline_cache_attachments,
        tail_limit,
        record_filter.as_deref(),
    );
    print_tail_records(
        &mut stderr,
        "property-inline-cache-clear",
        property_inline_cache_clears,
        tail_limit,
        record_filter.as_deref(),
    );
    print_tail_records(
        &mut stderr,
        "structure-stub-repatch-transaction",
        structure_stub_repatch_transactions,
        tail_limit,
        record_filter.as_deref(),
    );
    print_tail_records(
        &mut stderr,
        "structure-stub-access-case-link",
        structure_stub_access_case_links,
        tail_limit,
        record_filter.as_deref(),
    );
    print_tail_records(
        &mut stderr,
        "baseline-native-entry-readiness",
        baseline_native_entry_readiness,
        tail_limit,
        record_filter.as_deref(),
    );
    print_tail_records(
        &mut stderr,
        "baseline-machine-code-emission-provenance",
        baseline_machine_code_emission_provenance,
        tail_limit,
        record_filter.as_deref(),
    );
    print_tail_records(
        &mut stderr,
        "vm-entry-launch",
        launch_descriptors,
        tail_limit,
        record_filter.as_deref(),
    );
    let _ = stderr.flush();
}

fn property_inline_cache_evolution_counts(
    records: &[javascriptcore::vm::VmPropertyInlineCacheEvolutionRecord],
) -> (
    usize,
    usize,
    usize,
    usize,
    usize,
    usize,
    usize,
    usize,
    usize,
    usize,
    usize,
    usize,
) {
    let mut admitted = 0;
    let mut buffered = 0;
    let mut buffered_duplicates = 0;
    let mut cooldowns = 0;
    let mut final_gave_up = 0;
    let mut gave_up_skips = 0;
    let mut generated_megamorphic_load = 0;
    let mut megamorphic_load_skips = 0;
    let mut generated_megamorphic_store = 0;
    let mut megamorphic_store_skips = 0;
    let mut generated_megamorphic_has = 0;
    let mut megamorphic_has_skips = 0;
    for record in records {
        match record.decision {
            VmPropertyInlineCacheEvolutionDecision::Admitted => admitted += 1,
            VmPropertyInlineCacheEvolutionDecision::GeneratedMegamorphicLoad => {
                generated_megamorphic_load += 1;
            }
            VmPropertyInlineCacheEvolutionDecision::GeneratedMegamorphicStore => {
                generated_megamorphic_store += 1;
            }
            VmPropertyInlineCacheEvolutionDecision::GeneratedMegamorphicHas => {
                generated_megamorphic_has += 1;
            }
            VmPropertyInlineCacheEvolutionDecision::SkippedBufferedDuplicate => {
                buffered_duplicates += 1;
            }
            VmPropertyInlineCacheEvolutionDecision::SkippedReplacedByExistingCase => {}
            VmPropertyInlineCacheEvolutionDecision::SkippedGaveUp => gave_up_skips += 1,
            VmPropertyInlineCacheEvolutionDecision::SkippedMegamorphicLoad => {
                megamorphic_load_skips += 1;
            }
            VmPropertyInlineCacheEvolutionDecision::SkippedMegamorphicStore => {
                megamorphic_store_skips += 1;
            }
            VmPropertyInlineCacheEvolutionDecision::SkippedMegamorphicHas => {
                megamorphic_has_skips += 1;
            }
            VmPropertyInlineCacheEvolutionDecision::SkippedInitialCountdown => {}
            VmPropertyInlineCacheEvolutionDecision::SkippedCooldown => cooldowns += 1,
        }
        if record.counters_before.terminal
            != Some(VmPropertyInlineCacheEvolutionTerminalState::GaveUp)
            && record.counters_after.terminal
                == Some(VmPropertyInlineCacheEvolutionTerminalState::GaveUp)
        {
            final_gave_up += 1;
        }
        if record.counters_after.buffered_structure_count
            > record.counters_before.buffered_structure_count
        {
            buffered += 1;
        }
    }
    (
        admitted,
        buffered,
        buffered_duplicates,
        cooldowns,
        final_gave_up,
        gave_up_skips,
        generated_megamorphic_load,
        megamorphic_load_skips,
        generated_megamorphic_store,
        megamorphic_store_skips,
        generated_megamorphic_has,
        megamorphic_has_skips,
    )
}

#[derive(Default)]
struct VmOwnedCallTargetValidationCounts {
    accepted: usize,
    rejected: usize,
    target_code_block_not_registered: usize,
    request_snapshot_unavailable: usize,
    registered_snapshot_unavailable: usize,
    snapshot_mismatch: usize,
    target_code_block_not_live: usize,
    request_specialization_mismatch: usize,
    registered_specialization_mismatch: usize,
    missing_registered_executable: usize,
    executable_not_registered: usize,
    executable_installed_code_missing: usize,
    executable_installed_code_block_mismatch: usize,
    request_executable_mismatch: usize,
}

fn vm_owned_call_target_validation_counts(
    records: &[javascriptcore::vm::VmOwnedCallTargetValidationRecord],
) -> VmOwnedCallTargetValidationCounts {
    let mut counts = VmOwnedCallTargetValidationCounts::default();
    for record in records {
        match record.outcome {
            VmOwnedCallTargetValidationOutcome::Accepted => counts.accepted += 1,
            VmOwnedCallTargetValidationOutcome::Rejected(rejection) => {
                counts.rejected += 1;
                match rejection {
                    VmOwnedCallTargetValidationRejection::TargetCodeBlockNotRegistered {
                        ..
                    } => counts.target_code_block_not_registered += 1,
                    VmOwnedCallTargetValidationRejection::RequestSnapshotUnavailable { .. } => {
                        counts.request_snapshot_unavailable += 1
                    }
                    VmOwnedCallTargetValidationRejection::RegisteredSnapshotUnavailable {
                        ..
                    } => counts.registered_snapshot_unavailable += 1,
                    VmOwnedCallTargetValidationRejection::SnapshotMismatch { .. } => {
                        counts.snapshot_mismatch += 1
                    }
                    VmOwnedCallTargetValidationRejection::TargetCodeBlockNotLive { .. } => {
                        counts.target_code_block_not_live += 1
                    }
                    VmOwnedCallTargetValidationRejection::RequestSpecializationMismatch {
                        ..
                    } => counts.request_specialization_mismatch += 1,
                    VmOwnedCallTargetValidationRejection::RegisteredSpecializationMismatch {
                        ..
                    } => counts.registered_specialization_mismatch += 1,
                    VmOwnedCallTargetValidationRejection::MissingRegisteredExecutable {
                        ..
                    } => counts.missing_registered_executable += 1,
                    VmOwnedCallTargetValidationRejection::ExecutableNotRegistered { .. } => {
                        counts.executable_not_registered += 1
                    }
                    VmOwnedCallTargetValidationRejection::ExecutableInstalledCodeMissing {
                        ..
                    } => counts.executable_installed_code_missing += 1,
                    VmOwnedCallTargetValidationRejection::ExecutableInstalledCodeBlockMismatch {
                        ..
                    } => counts.executable_installed_code_block_mismatch += 1,
                    VmOwnedCallTargetValidationRejection::RequestExecutableMismatch { .. } => {
                        counts.request_executable_mismatch += 1
                    }
                }
            }
        }
    }
    counts
}

fn tiering_tail_record_limit() -> usize {
    env::var("OCTANE_PROBE_TIERING_TAIL_LIMIT")
        .ok()
        .and_then(|value| value.parse().ok())
        .unwrap_or(8)
}

fn tiering_record_filter() -> Option<String> {
    env::var("OCTANE_PROBE_TIERING_FILTER")
        .ok()
        .filter(|value| !value.is_empty())
}

fn print_tail_records<T: Debug>(
    output: &mut impl Write,
    label: &str,
    records: &[T],
    limit: usize,
    filter: Option<&str>,
) {
    let mut rendered = Vec::new();
    for (index, record) in records.iter().enumerate() {
        let record = format!("{record:?}");
        if filter.is_some_and(|filter| !record.contains(filter)) {
            continue;
        }
        rendered.push((index, record));
    }
    let start = rendered.len().saturating_sub(limit);
    for (index, record) in rendered.into_iter().skip(start) {
        let _ = writeln!(output, "{label}[{index}]={record}");
    }
}

fn print_top_records<T: Debug>(
    output: &mut impl Write,
    label: &str,
    records: &[T],
    limit: usize,
    filter: Option<&str>,
    compare: impl Fn(&T, &T) -> std::cmp::Ordering,
) {
    if limit == 0 {
        return;
    }
    let mut ordered = records.iter().collect::<Vec<_>>();
    ordered.sort_by(|left, right| compare(left, right));
    let mut printed = 0usize;
    for (rank, record) in ordered.into_iter().enumerate() {
        if printed >= limit {
            break;
        }
        let record = format!("{record:?}");
        if filter.is_some_and(|filter| !record.contains(filter)) {
            continue;
        }
        let _ = writeln!(output, "{label}[{rank}]={record}");
        printed = printed.saturating_add(1);
    }
}

fn print_benchmark_report(benchmark: &OctaneBenchmarkExecutionReport) {
    println!(
        "{}: config mode={:?} iterations={} worst_case_count={}",
        benchmark.benchmark,
        benchmark.mode,
        benchmark.run_config.iterations,
        benchmark.run_config.worst_case_count
    );
    match &benchmark.outcome {
        OctaneExecutionOutcome::Succeeded(success) => {
            println!(
                "{}: ok score={} first={} worst={} avg={}",
                benchmark.benchmark,
                success.scores.score,
                success.scores.first_iteration,
                success.scores.worst_case,
                success.scores.average
            );
        }
        OctaneExecutionOutcome::Failed(failure) => {
            println!(
                "{}: failed phase={:?} order_index={:?} label={:?} detail={:?}",
                benchmark.benchmark,
                failure.phase,
                failure.order_index,
                failure.label,
                failure.detail
            );
        }
    }
}

fn print_benchmark_tiering_summary(benchmark: &OctaneBenchmarkExecutionReport) {
    let summary = &benchmark.tiering_delta;
    let tail_limit = tiering_tail_record_limit();
    let record_filter = tiering_record_filter();
    let mut stderr = io::stderr().lock();
    let _ = writeln!(
        stderr,
        "tiering-summary benchmark={} entry_decisions={} fallbacks={} diagnostics={} baseline_installs={} baseline_entry_artifacts={} baseline_materializations={} baseline_generated_code_artifacts={} baseline_generated_executions={} baseline_generated_executed_bytecodes={} baseline_entry_auto_materializations={} baseline_native_lowering_failures={} baseline_native_semantic_byte_emission_failures={} baseline_native_entry_readiness={} generated_direct_call_transactions={} generated_direct_call_generated_entries={} generated_direct_call_native_entries={} generated_direct_call_native_interpreter_fallbacks={} generated_direct_call_nested_interpreter_fallbacks={} generated_direct_call_hot_slot_hits={} generated_direct_call_sidecar_hot_slot_hits={} generated_direct_call_preferred_route_hits={} generated_direct_call_rootless_generated_entries={} generated_direct_call_rootless_generated_entry_proof_cache_hits={} generated_direct_call_rootless_native_entries={} generated_direct_call_rootless_rejections={} generated_direct_call_rootless_reject_hot_slot_miss={} generated_direct_call_rootless_reject_preferred_route_not_generated_entry={} generated_direct_call_rootless_reject_missing_generated_artifact={} generated_direct_call_rootless_reject_invalid_generated_artifact={} generated_direct_call_rootless_reject_snapshot_mismatch={} generated_direct_call_rootless_reject_runtime_helper_plan={} generated_direct_call_rootless_reject_effect_contract={} generated_direct_call_rootless_reject_retained_side_exit={} generated_direct_call_rootless_reject_retained_runtime_helper_exit={} generated_direct_call_rootless_reject_retained_js_call_exit={} generated_direct_call_rootless_reject_retained_loop_backedge_exit={} generated_direct_call_rootless_reject_invalid_body_instruction={} generated_direct_call_rootless_reject_invalid_body_bytecode_index={} generated_direct_call_rootless_reject_unsupported_body_opcode={} generated_direct_call_rootless_reject_missing_return={} generated_direct_call_rootless_native_entry_rejections={} generated_direct_call_rootless_native_entry_reject_hot_slot_miss={} generated_direct_call_rootless_native_entry_reject_preferred_route_not_generated_entry={} generated_direct_call_rootless_native_entry_reject_missing_generated_artifact={} generated_direct_call_rootless_native_entry_reject_invalid_generated_artifact={} generated_direct_call_rootless_native_entry_reject_snapshot_mismatch={} generated_direct_call_rootless_native_entry_reject_runtime_helper_plan={} generated_direct_call_rootless_native_entry_reject_effect_contract={} generated_direct_call_rootless_native_entry_reject_retained_side_exit={} generated_direct_call_rootless_native_entry_reject_retained_runtime_helper_exit={} generated_direct_call_rootless_native_entry_reject_retained_js_call_exit={} generated_direct_call_rootless_native_entry_reject_retained_loop_backedge_exit={} generated_direct_call_rootless_native_entry_reject_invalid_body_instruction={} generated_direct_call_rootless_native_entry_reject_invalid_body_bytecode_index={} generated_direct_call_rootless_native_entry_reject_unsupported_body_opcode={} generated_direct_call_rootless_native_entry_reject_missing_return={} generated_direct_call_rootless_preferred_native_entry={} generated_direct_call_rootless_preferred_native_entry_pure_baseline_shim={} generated_direct_call_rootless_preferred_native_entry_emitted_semantic_c_abi_entry={} generated_direct_call_rootless_preferred_native_entry_unknown={} launch_descriptors={} call_observations={} call_link_boundary_validations={} call_link_inline_cache_attachments={} property_load_observations={} property_store_observations={} property_inline_cache_evolution_records={} property_inline_cache_evolution_admitted={} property_inline_cache_evolution_buffered={} property_inline_cache_evolution_buffered_duplicates={} property_inline_cache_evolution_cooldowns={} property_inline_cache_evolution_final_gave_up={} property_inline_cache_evolution_gave_up_skips={} property_inline_cache_evolution_generated_megamorphic_load={} property_inline_cache_evolution_megamorphic_load_skips={} property_inline_cache_evolution_generated_megamorphic_store={} property_inline_cache_evolution_megamorphic_store_skips={} property_inline_cache_evolution_generated_megamorphic_has={} property_inline_cache_evolution_megamorphic_has_skips={} property_load_megamorphic_cache_records={} property_store_megamorphic_cache_records={} property_has_megamorphic_cache_records={} property_inline_cache_attachments={}",
        benchmark.benchmark,
        summary.entry_decisions,
        summary.fallback_records,
        summary.diagnostics,
        summary.baseline_installs,
        summary.baseline_entry_artifacts,
        summary.baseline_materializations,
        summary.baseline_generated_code_artifacts,
        summary.baseline_generated_executions,
        summary.baseline_generated_executed_bytecodes,
        summary.baseline_entry_auto_materializations,
        summary.baseline_native_lowering_failures,
        summary.baseline_native_semantic_byte_emission_failures,
        summary.baseline_native_entry_readiness,
        summary.generated_direct_call_transactions,
        summary.generated_direct_call_generated_entries,
        summary.generated_direct_call_native_entries,
        summary.generated_direct_call_native_interpreter_fallbacks,
        summary.generated_direct_call_nested_interpreter_fallbacks,
        summary.generated_direct_call_hot_slot_hits,
        summary.generated_direct_call_sidecar_hot_slot_hits,
        summary.generated_direct_call_preferred_route_hits,
        summary.generated_direct_call_rootless_generated_entries,
        summary.generated_direct_call_rootless_generated_entry_proof_cache_hits,
        summary.generated_direct_call_rootless_native_entries,
        summary.generated_direct_call_rootless_rejections.total(),
        summary.generated_direct_call_rootless_rejections.hot_slot_miss,
        summary
            .generated_direct_call_rootless_rejections
            .preferred_route_not_generated_entry,
        summary
            .generated_direct_call_rootless_rejections
            .missing_generated_artifact,
        summary
            .generated_direct_call_rootless_rejections
            .invalid_generated_artifact,
        summary
            .generated_direct_call_rootless_rejections
            .snapshot_mismatch,
        summary
            .generated_direct_call_rootless_rejections
            .runtime_helper_plan,
        summary.generated_direct_call_rootless_rejections.effect_contract,
        summary
            .generated_direct_call_rootless_rejections
            .retained_side_exit,
        summary
            .generated_direct_call_rootless_rejections
            .retained_runtime_helper_exit,
        summary
            .generated_direct_call_rootless_rejections
            .retained_js_call_exit,
        summary
            .generated_direct_call_rootless_rejections
            .retained_loop_backedge_exit,
        summary
            .generated_direct_call_rootless_rejections
            .invalid_body_instruction,
        summary
            .generated_direct_call_rootless_rejections
            .invalid_body_bytecode_index,
        summary
            .generated_direct_call_rootless_rejections
            .unsupported_body_opcode,
        summary.generated_direct_call_rootless_rejections.missing_return,
        summary
            .generated_direct_call_rootless_native_entry_rejections
            .total(),
        summary
            .generated_direct_call_rootless_native_entry_rejections
            .hot_slot_miss,
        summary
            .generated_direct_call_rootless_native_entry_rejections
            .preferred_route_not_generated_entry,
        summary
            .generated_direct_call_rootless_native_entry_rejections
            .missing_generated_artifact,
        summary
            .generated_direct_call_rootless_native_entry_rejections
            .invalid_generated_artifact,
        summary
            .generated_direct_call_rootless_native_entry_rejections
            .snapshot_mismatch,
        summary
            .generated_direct_call_rootless_native_entry_rejections
            .runtime_helper_plan,
        summary
            .generated_direct_call_rootless_native_entry_rejections
            .effect_contract,
        summary
            .generated_direct_call_rootless_native_entry_rejections
            .retained_side_exit,
        summary
            .generated_direct_call_rootless_native_entry_rejections
            .retained_runtime_helper_exit,
        summary
            .generated_direct_call_rootless_native_entry_rejections
            .retained_js_call_exit,
        summary
            .generated_direct_call_rootless_native_entry_rejections
            .retained_loop_backedge_exit,
        summary
            .generated_direct_call_rootless_native_entry_rejections
            .invalid_body_instruction,
        summary
            .generated_direct_call_rootless_native_entry_rejections
            .invalid_body_bytecode_index,
        summary
            .generated_direct_call_rootless_native_entry_rejections
            .unsupported_body_opcode,
        summary
            .generated_direct_call_rootless_native_entry_rejections
            .missing_return,
        summary
            .generated_direct_call_rootless_preferred_native_entry_counts
            .total(),
        summary
            .generated_direct_call_rootless_preferred_native_entry_counts
            .pure_baseline_shim,
        summary
            .generated_direct_call_rootless_preferred_native_entry_counts
            .emitted_semantic_c_abi_entry,
        summary
            .generated_direct_call_rootless_preferred_native_entry_counts
            .unknown,
        summary.launch_descriptors,
        summary.call_observations,
        summary.call_link_boundary_validations,
        summary.call_link_inline_cache_attachments,
        summary.property_load_observations,
        summary.property_store_observations,
        summary.property_inline_cache_evolution_records,
        summary.property_inline_cache_evolution_admitted,
        summary.property_inline_cache_evolution_buffered,
        summary.property_inline_cache_evolution_buffered_duplicates,
        summary.property_inline_cache_evolution_cooldowns,
        summary.property_inline_cache_evolution_final_gave_up,
        summary.property_inline_cache_evolution_gave_up_skips,
        summary.property_inline_cache_evolution_generated_megamorphic_load,
        summary.property_inline_cache_evolution_megamorphic_load_skips,
        summary.property_inline_cache_evolution_generated_megamorphic_store,
        summary.property_inline_cache_evolution_megamorphic_store_skips,
        summary.property_inline_cache_evolution_generated_megamorphic_has,
        summary.property_inline_cache_evolution_megamorphic_has_skips,
        summary.property_load_megamorphic_cache_records,
        summary.property_store_megamorphic_cache_records,
        summary.property_has_megamorphic_cache_records,
        summary.property_inline_cache_attachments
    );
    let mut unsupported_body_opcode_counts = summary
        .generated_direct_call_rootless_unsupported_body_opcode_counts
        .clone();
    unsupported_body_opcode_counts.sort_by(|left, right| {
        right
            .count
            .cmp(&left.count)
            .then_with(|| format!("{:?}", left.opcode).cmp(&format!("{:?}", right.opcode)))
    });
    for count in unsupported_body_opcode_counts {
        let _ = writeln!(
            stderr,
            "generated-direct-call-rootless-unsupported-body-opcode benchmark={} opcode={:?} count={}",
            benchmark.benchmark, count.opcode, count.count
        );
    }
    let mut native_entry_unsupported_body_opcode_counts = summary
        .generated_direct_call_rootless_native_entry_unsupported_body_opcode_counts
        .clone();
    native_entry_unsupported_body_opcode_counts.sort_by(|left, right| {
        right
            .count
            .cmp(&left.count)
            .then_with(|| format!("{:?}", left.opcode).cmp(&format!("{:?}", right.opcode)))
    });
    for count in native_entry_unsupported_body_opcode_counts {
        let _ = writeln!(
            stderr,
            "generated-direct-call-rootless-native-entry-unsupported-body-opcode benchmark={} opcode={:?} count={}",
            benchmark.benchmark, count.opcode, count.count
        );
    }
    let mut native_entry_retained_side_exit_counts = summary
        .generated_direct_call_rootless_native_entry_retained_side_exit_counts
        .clone();
    native_entry_retained_side_exit_counts.sort_by(|left, right| {
        right
            .count
            .cmp(&left.count)
            .then_with(|| {
                format!("{:?}", left.target_code_block)
                    .cmp(&format!("{:?}", right.target_code_block))
            })
            .then_with(|| left.bytecode_index.cmp(&right.bytecode_index))
            .then_with(|| format!("{:?}", left.opcode).cmp(&format!("{:?}", right.opcode)))
            .then_with(|| format!("{:?}", left.reason).cmp(&format!("{:?}", right.reason)))
    });
    for count in native_entry_retained_side_exit_counts {
        let _ = writeln!(
            stderr,
            "generated-direct-call-rootless-native-entry-retained-side-exit benchmark={} target={:?} bytecode_index={:?} opcode={:?} reason={:?} count={}",
            benchmark.benchmark,
            count.target_code_block,
            count.bytecode_index,
            count.opcode,
            count.reason,
            count.count
        );
    }
    print_top_records(
        &mut stderr,
        "baseline-generated-execution-summary",
        &summary.baseline_generated_execution_summaries,
        tail_limit,
        record_filter.as_deref(),
        |left, right| {
            right
                .executed_bytecode_count
                .cmp(&left.executed_bytecode_count)
                .then_with(|| right.execution_count.cmp(&left.execution_count))
                .then_with(|| format!("{:?}", left.owner).cmp(&format!("{:?}", right.owner)))
        },
    );
    print_top_records(
        &mut stderr,
        "generated-direct-call-transaction-summary",
        &summary.generated_direct_call_transaction_summaries,
        tail_limit,
        record_filter.as_deref(),
        |left, right| {
            right
                .transaction_count
                .cmp(&left.transaction_count)
                .then_with(|| format!("{:?}", left.caller).cmp(&format!("{:?}", right.caller)))
                .then_with(|| {
                    left.call_bytecode_index
                        .as_bits()
                        .cmp(&right.call_bytecode_index.as_bits())
                })
                .then_with(|| {
                    format!("{:?}", left.target_code_block)
                        .cmp(&format!("{:?}", right.target_code_block))
                })
                .then_with(|| format!("{:?}", left.route).cmp(&format!("{:?}", right.route)))
        },
    );
    let _ = stderr.flush();
}

fn print_progress_event(output: &mut impl Write, event: OctaneExecutionProgress) {
    match event {
        OctaneExecutionProgress::BenchmarkStarted { benchmark, mode } => {
            let _ = writeln!(
                output,
                "progress benchmark={benchmark} mode={mode:?} phase=benchmark-start"
            );
        }
        OctaneExecutionProgress::SourceSessionStarted { benchmark, mode } => {
            let _ = writeln!(
                output,
                "progress benchmark={benchmark} mode={mode:?} phase=session-open-start"
            );
        }
        OctaneExecutionProgress::SourceSessionOpened { benchmark, mode } => {
            let _ = writeln!(
                output,
                "progress benchmark={benchmark} mode={mode:?} phase=session-open-done"
            );
        }
        OctaneExecutionProgress::SourceSessionErrored {
            benchmark,
            mode,
            error,
        } => {
            let _ = writeln!(
                output,
                "progress benchmark={benchmark} mode={mode:?} phase=session-open-error error={error:?}"
            );
        }
        OctaneExecutionProgress::SourceStarted {
            benchmark,
            mode,
            order_index,
            order_entry,
            label,
        } => {
            let _ = writeln!(
                output,
                "progress benchmark={benchmark} mode={mode:?} phase=source-start order_index={order_index} order_entry={order_entry:?} label={label:?}"
            );
        }
        OctaneExecutionProgress::SourceCompleted {
            benchmark,
            mode,
            order_index,
            order_entry,
            label,
            completion,
        } => {
            let _ = writeln!(
                output,
                "progress benchmark={benchmark} mode={mode:?} phase=source-done order_index={order_index} order_entry={order_entry:?} label={label:?} completion={}",
                completion_kind(&completion)
            );
        }
        OctaneExecutionProgress::SourceErrored {
            benchmark,
            mode,
            order_index,
            order_entry,
            label,
            error,
        } => {
            let _ = writeln!(
                output,
                "progress benchmark={benchmark} mode={mode:?} phase=source-error order_index={order_index} order_entry={order_entry:?} label={label:?} error={error:?}"
            );
        }
        OctaneExecutionProgress::ScoreTelemetryStarted { benchmark, mode } => {
            let _ = writeln!(
                output,
                "progress benchmark={benchmark} mode={mode:?} phase=score-telemetry-start"
            );
        }
    }
    let _ = output.flush();
}

fn completion_kind(completion: &ExecutionCompletion) -> &'static str {
    match completion {
        ExecutionCompletion::Returned(_) => "returned",
        ExecutionCompletion::Threw(_) => "threw",
        ExecutionCompletion::OrdinaryBytecodeCall(_) => "ordinary-bytecode-call",
        ExecutionCompletion::OrdinaryBytecodeConstruct(_) => "ordinary-bytecode-construct",
        ExecutionCompletion::BaselineLoopHandoff(_) => "baseline-loop-handoff",
        ExecutionCompletion::FunctionValueCall(_) => "function-value-call",
        ExecutionCompletion::Terminated(_) => "terminated",
        ExecutionCompletion::Suspended(_) => "suspended",
        ExecutionCompletion::Failed(_) => "failed",
    }
}

fn default_jetstream_root() -> PathBuf {
    PathBuf::from("../../../PerformanceTests/JetStream3")
}

fn parse_usize(label: &str, value: &str) -> usize {
    match value.parse::<usize>() {
        Ok(parsed) if parsed > 0 => parsed,
        _ => usage_and_exit(&format!("{label} requires a positive integer")),
    }
}

fn parse_usize_allow_zero(label: &str, value: &str) -> usize {
    match value.parse::<usize>() {
        Ok(parsed) => parsed,
        _ => usage_and_exit(&format!("{label} requires an unsigned integer")),
    }
}

fn parse_u32(label: &str, value: &str) -> u32 {
    match value.parse::<u32>() {
        Ok(parsed) => parsed,
        _ => usage_and_exit(&format!("{label} requires an unsigned integer")),
    }
}

fn dump_source_identifier(jetstream_root: &PathBuf, benchmark: &str, identifier: u32) {
    let Some(plan) = octane_plan_by_name(benchmark) else {
        eprintln!("unknown Octane benchmark: {benchmark}");
        std::process::exit(2);
    };
    for manifest_path in plan.files {
        let path = jetstream_root.join(manifest_path);
        let text = fs::read_to_string(&path).unwrap_or_else(|error| {
            eprintln!("failed to read {}: {error}", path.display());
            std::process::exit(2);
        });
        let source = source_code_from_text(text);
        let mut arena = ParserArena::new();
        if let Err(error) = Parser::with_mode(
            &mut arena,
            AstBuilder::default(),
            &source,
            ParseMode::Program,
        )
        .parse()
        {
            eprintln!("failed to parse {}: {error:?}", path.display());
            std::process::exit(2);
        }
        let text = arena
            .identifiers()
            .identifier_text(javascriptcore::syntax::ParserIdentifier(identifier));
        println!("{manifest_path}: {identifier} -> {text:?}");
    }
}

fn source_code_from_text(text: String) -> SourceCode {
    let length = text.len().try_into().unwrap_or(u32::MAX);
    SourceCode::new(
        Arc::new(SourceProvider::new(
            SourceOrigin::default(),
            SourceText::Latin1(text.into_bytes()),
        )),
        SourceSpan::new(SourcePosition(0), SourcePosition(length)),
    )
}

fn usage_and_exit(message: &str) -> ! {
    if !message.is_empty() {
        eprintln!("{message}");
    }
    eprintln!(
        "usage: cargo run --example octane_probe -- [--jetstream-root PATH] [--benchmark NAME|--core|--full|--smoke|--eval SOURCE|--eval-file PATH [--eval-append SOURCE]] [--interpreter|--baseline] [--collect-all|--fail-fast] [--progress] [--tiering-summary] [--iterations N --worst-case-count N|--official-iterations] [--dispatch-steps N] [--dump-identifier ID] [--dump-code-block CELL_ID]"
    );
    std::process::exit(if message.is_empty() { 0 } else { 2 });
}
