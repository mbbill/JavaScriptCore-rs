//! JetStream 3 Octane manifest and scoring contracts for the shell.
//!
//! This module intentionally does not execute JavaScript. It mirrors the
//! driver-owned Octane plan metadata, loads source text into shell provenance
//! records, prepares runner-owned sources, and keeps the synchronous
//! `DefaultBenchmark` scoring math used by JetStream 3.

use std::cmp::Ordering;
use std::path::{Path, PathBuf};

use super::{
    ShellMode, ShellSourceAppendRequest, ShellSourceKind, ShellSourceLoadError, ShellSourceLoader,
};
use crate::bytecode::{SourceOriginId, SourceProviderId};
use crate::syntax::source::SourceText;
use crate::vm::SourceSessionSource;

pub const OCTANE_DEFAULT_ITERATION_COUNT: usize = 120;
pub const OCTANE_DEFAULT_WORST_CASE_COUNT: usize = 4;

#[derive(Clone, Copy, Debug, Eq, PartialEq)]
pub enum OctaneBenchmarkClass {
    DefaultBenchmark,
}

#[derive(Clone, Copy, Debug, Eq, PartialEq)]
pub struct OctaneBenchmarkPlan {
    pub name: &'static str,
    pub files: &'static [&'static str],
    pub deterministic_random: bool,
    pub iterations: Option<usize>,
    pub worst_case_count: Option<usize>,
    pub benchmark_class: OctaneBenchmarkClass,
}

impl OctaneBenchmarkPlan {
    pub fn run_config(
        self,
        overrides: OctaneBenchmarkRunOverrides,
    ) -> Result<OctaneDefaultBenchmarkRunConfig, OctaneScoringError> {
        OctaneDefaultBenchmarkRunConfig::new(
            overrides
                .iterations
                .or(self.iterations)
                .unwrap_or(OCTANE_DEFAULT_ITERATION_COUNT),
            overrides
                .worst_case_count
                .or(self.worst_case_count)
                .unwrap_or(OCTANE_DEFAULT_WORST_CASE_COUNT),
        )
    }

    pub fn default_run_config(self) -> Result<OctaneDefaultBenchmarkRunConfig, OctaneScoringError> {
        self.run_config(OctaneBenchmarkRunOverrides::none())
    }

    pub fn score_default_results(
        self,
        overrides: OctaneBenchmarkRunOverrides,
        result_times_ms: &[f64],
    ) -> Result<OctaneDefaultBenchmarkScores, OctaneScoringError> {
        self.run_config(overrides)?.score_results(result_times_ms)
    }
}

#[derive(Clone, Copy, Debug, Default, Eq, PartialEq)]
pub struct OctaneBenchmarkRunOverrides {
    pub iterations: Option<usize>,
    pub worst_case_count: Option<usize>,
}

impl OctaneBenchmarkRunOverrides {
    pub const fn none() -> Self {
        Self {
            iterations: None,
            worst_case_count: None,
        }
    }

    pub const fn new(iterations: Option<usize>, worst_case_count: Option<usize>) -> Self {
        Self {
            iterations,
            worst_case_count,
        }
    }
}

#[derive(Clone, Copy, Debug, Eq, PartialEq)]
pub struct OctaneDefaultBenchmarkRunConfig {
    pub iterations: usize,
    pub worst_case_count: usize,
}

impl OctaneDefaultBenchmarkRunConfig {
    pub fn new(iterations: usize, worst_case_count: usize) -> Result<Self, OctaneScoringError> {
        if iterations <= worst_case_count {
            return Err(OctaneScoringError::IterationsMustExceedWorstCase {
                iterations,
                worst_case_count,
            });
        }

        Ok(Self {
            iterations,
            worst_case_count,
        })
    }

    pub fn score_results(
        self,
        result_times_ms: &[f64],
    ) -> Result<OctaneDefaultBenchmarkScores, OctaneScoringError> {
        if result_times_ms.len() < self.iterations {
            return Err(OctaneScoringError::TooFewResults {
                expected: self.iterations,
                actual: result_times_ms.len(),
            });
        }
        if result_times_ms.len() > self.iterations {
            return Err(OctaneScoringError::TooManyResults {
                expected: self.iterations,
                actual: result_times_ms.len(),
            });
        }

        let (first_time, remaining_times) =
            result_times_ms
                .split_first()
                .ok_or(OctaneScoringError::TooFewResults {
                    expected: self.iterations,
                    actual: 0,
                })?;

        if remaining_times.len() < self.worst_case_count {
            return Err(OctaneScoringError::TooFewResultsForWorstCase {
                expected: self.worst_case_count,
                actual: remaining_times.len(),
            });
        }

        let first_iteration = octane_default_to_score(*first_time);
        let average = octane_default_to_score(arithmetic_mean(remaining_times));

        let mut slowest_times = remaining_times.to_vec();
        slowest_times.sort_by(|left, right| right.partial_cmp(left).unwrap_or(Ordering::Equal));
        let worst_case =
            octane_default_to_score(arithmetic_mean(&slowest_times[..self.worst_case_count]));

        let score = geometric_mean(&[first_iteration, worst_case, average]);
        Ok(OctaneDefaultBenchmarkScores {
            first_iteration,
            worst_case,
            average,
            score,
        })
    }
}

#[derive(Clone, Copy, Debug, PartialEq)]
pub struct OctaneDefaultBenchmarkScores {
    pub first_iteration: f64,
    pub worst_case: f64,
    pub average: f64,
    pub score: f64,
}

#[derive(Clone, Copy, Debug, Eq, PartialEq)]
pub enum OctaneScoringError {
    IterationsMustExceedWorstCase {
        iterations: usize,
        worst_case_count: usize,
    },
    TooFewResults {
        expected: usize,
        actual: usize,
    },
    TooManyResults {
        expected: usize,
        actual: usize,
    },
    TooFewResultsForWorstCase {
        expected: usize,
        actual: usize,
    },
}

#[derive(Clone, Copy, Debug, Eq, PartialEq)]
pub struct OctaneBenchmarkSelection {
    pub name: &'static str,
    pub benchmark_names: &'static [&'static str],
}

impl OctaneBenchmarkSelection {
    pub fn resolved_plans(self) -> Result<Vec<&'static OctaneBenchmarkPlan>, OctaneManifestError> {
        let mut plans = Vec::with_capacity(self.benchmark_names.len());
        for name in self.benchmark_names {
            plans.push(
                octane_plan_by_name(name).ok_or(OctaneManifestError::BenchmarkNotFound(name))?,
            );
        }
        Ok(plans)
    }
}

#[derive(Clone, Copy, Debug, Eq, PartialEq)]
pub enum OctaneManifestError {
    BenchmarkNotFound(&'static str),
}

#[derive(Clone, Copy, Debug, Eq, PartialEq)]
pub enum OctaneSuite {
    Core,
    Full,
}

impl OctaneSuite {
    pub fn name(self) -> &'static str {
        match self {
            Self::Core => OCTANE_CORE_SELECTION.name,
            Self::Full => "Octane-full",
        }
    }

    pub fn resolved_plans(self) -> Result<Vec<&'static OctaneBenchmarkPlan>, OctaneManifestError> {
        match self {
            Self::Core => OCTANE_CORE_SELECTION.resolved_plans(),
            Self::Full => Ok(OCTANE_DRIVER_PLANS.iter().collect()),
        }
    }
}

#[derive(Clone, Copy, Debug, Eq, PartialEq)]
pub struct OctaneRunConfig {
    pub suite: OctaneSuite,
    pub overrides: Option<OctaneBenchmarkRunOverrides>,
}

impl OctaneRunConfig {
    pub const fn new(suite: OctaneSuite) -> Self {
        Self {
            suite,
            overrides: None,
        }
    }

    pub const fn with_overrides(
        suite: OctaneSuite,
        overrides: OctaneBenchmarkRunOverrides,
    ) -> Self {
        Self {
            suite,
            overrides: Some(overrides),
        }
    }

    pub const fn effective_overrides(self) -> OctaneBenchmarkRunOverrides {
        match self.overrides {
            Some(overrides) => overrides,
            None => OctaneBenchmarkRunOverrides::none(),
        }
    }
}

#[derive(Clone, Debug, Eq, PartialEq)]
pub struct OctanePreparationConfig {
    pub jetstream_root: PathBuf,
    pub run: OctaneRunConfig,
}

impl OctanePreparationConfig {
    pub fn new(jetstream_root: impl Into<PathBuf>, run: OctaneRunConfig) -> Self {
        Self {
            jetstream_root: jetstream_root.into(),
            run,
        }
    }
}

#[derive(Clone, Debug)]
pub struct OctanePreparedSuite {
    pub config: OctanePreparationConfig,
    pub benchmarks: Vec<OctanePreparedBenchmark>,
}

#[derive(Clone, Debug)]
pub struct OctanePreparedBenchmark {
    pub plan: &'static OctaneBenchmarkPlan,
    pub run_config: OctaneDefaultBenchmarkRunConfig,
    pub benchmark_sources: Vec<OctanePreparedBenchmarkSource>,
    pub generated_sources: Vec<OctanePreparedGeneratedSource>,
    pub source_order: Vec<OctanePreparedSourceOrderEntry>,
}

impl OctanePreparedBenchmark {
    pub fn generated_source(
        &self,
        kind: OctanePreparedGeneratedSourceKind,
    ) -> Option<&OctanePreparedGeneratedSource> {
        self.generated_sources
            .iter()
            .find(|source| source.kind == kind)
    }
}

#[derive(Clone, Debug)]
pub struct OctanePreparedBenchmarkSource {
    pub manifest_path: &'static str,
    pub resolved_path: PathBuf,
    pub canonical_path: PathBuf,
    pub label: String,
    pub text: String,
    pub source: SourceSessionSource,
    pub provider_id: SourceProviderId,
    pub origin_id: SourceOriginId,
}

#[derive(Clone, Debug)]
pub struct OctanePreparedGeneratedSource {
    pub kind: OctanePreparedGeneratedSourceKind,
    pub label: String,
    pub text: String,
    pub source: SourceSessionSource,
    pub provider_id: SourceProviderId,
    pub origin_id: SourceOriginId,
}

#[derive(Clone, Copy, Debug, Eq, PartialEq)]
pub enum OctanePreparedGeneratedSourceKind {
    Prelude,
    DeterministicRandom,
    Runner,
}

impl OctanePreparedGeneratedSourceKind {
    fn label_component(self) -> &'static str {
        match self {
            Self::Prelude => "prelude",
            Self::DeterministicRandom => "deterministic-random",
            Self::Runner => "runner",
        }
    }
}

#[derive(Clone, Copy, Debug, Eq, PartialEq)]
pub enum OctanePreparedSourceOrderEntry {
    Generated(OctanePreparedGeneratedSourceKind),
    BenchmarkFile(usize),
}

#[derive(Clone, Copy, Debug, Eq, PartialEq)]
pub enum OctanePreparationPhase {
    BenchmarkLookup,
    Config,
    SourceLoad,
    SourceAppend,
    GeneratedSource,
}

#[derive(Clone, Debug, Eq, PartialEq)]
pub enum OctanePreparationError {
    BenchmarkLookup {
        suite: OctaneSuite,
        error: OctaneManifestError,
    },
    Config {
        benchmark: &'static str,
        error: OctaneScoringError,
    },
    SourceLoad {
        benchmark: &'static str,
        manifest_path: &'static str,
        resolved_path: PathBuf,
        error: ShellSourceLoadError,
    },
    SourceLoadMissingCanonicalPath {
        benchmark: &'static str,
        manifest_path: &'static str,
        resolved_path: PathBuf,
    },
    SourceAppend {
        benchmark: &'static str,
        generated_kind: OctanePreparedGeneratedSourceKind,
        label: String,
        error: ShellSourceLoadError,
    },
    GeneratedSource {
        benchmark: &'static str,
        generated_kind: OctanePreparedGeneratedSourceKind,
        error: OctaneGeneratedSourceError,
    },
}

impl OctanePreparationError {
    pub const fn phase(&self) -> OctanePreparationPhase {
        match self {
            Self::BenchmarkLookup { .. } => OctanePreparationPhase::BenchmarkLookup,
            Self::Config { .. } => OctanePreparationPhase::Config,
            Self::SourceLoad { .. } | Self::SourceLoadMissingCanonicalPath { .. } => {
                OctanePreparationPhase::SourceLoad
            }
            Self::SourceAppend { .. } => OctanePreparationPhase::SourceAppend,
            Self::GeneratedSource { .. } => OctanePreparationPhase::GeneratedSource,
        }
    }
}

#[derive(Clone, Copy, Debug, Eq, PartialEq)]
pub enum OctaneGeneratedSourceError {
    UnsupportedBenchmarkClass {
        benchmark_class: OctaneBenchmarkClass,
    },
}

pub fn prepare_octane_suite(
    config: OctanePreparationConfig,
) -> Result<OctanePreparedSuite, OctanePreparationError> {
    let plans = config.run.suite.resolved_plans().map_err(|error| {
        OctanePreparationError::BenchmarkLookup {
            suite: config.run.suite,
            error,
        }
    })?;
    let mut loader = ShellSourceLoader::default();
    let mut benchmarks = Vec::with_capacity(plans.len());

    for plan in plans {
        benchmarks.push(prepare_octane_benchmark_with_loader(
            &mut loader,
            &config.jetstream_root,
            plan,
            config.run.effective_overrides(),
        )?);
    }

    Ok(OctanePreparedSuite { config, benchmarks })
}

pub fn prepare_octane_benchmark(
    jetstream_root: impl AsRef<Path>,
    plan: &'static OctaneBenchmarkPlan,
    overrides: OctaneBenchmarkRunOverrides,
) -> Result<OctanePreparedBenchmark, OctanePreparationError> {
    let mut loader = ShellSourceLoader::default();
    prepare_octane_benchmark_with_loader(&mut loader, jetstream_root, plan, overrides)
}

fn prepare_octane_benchmark_with_loader(
    loader: &mut ShellSourceLoader,
    jetstream_root: impl AsRef<Path>,
    plan: &'static OctaneBenchmarkPlan,
    overrides: OctaneBenchmarkRunOverrides,
) -> Result<OctanePreparedBenchmark, OctanePreparationError> {
    let jetstream_root = jetstream_root.as_ref();
    let run_config =
        plan.run_config(overrides)
            .map_err(|error| OctanePreparationError::Config {
                benchmark: plan.name,
                error,
            })?;
    let mut benchmark_sources = Vec::with_capacity(plan.files.len());

    for manifest_path in plan.files {
        let resolved_path = jetstream_root.join(manifest_path);
        let loaded = loader
            .load_file_source(
                &resolved_path,
                ShellSourceKind::File,
                ShellMode::ScriptFile,
                false,
            )
            .map_err(|error| OctanePreparationError::SourceLoad {
                benchmark: plan.name,
                manifest_path,
                resolved_path: resolved_path.clone(),
                error,
            })?;
        let canonical_path = loaded
            .provider_record()
            .canonical_path
            .clone()
            .ok_or_else(|| OctanePreparationError::SourceLoadMissingCanonicalPath {
                benchmark: plan.name,
                manifest_path,
                resolved_path: resolved_path.clone(),
            })?;
        let text = source_text_to_string(loaded.source().provider().text());
        let provider_id = loaded.provider_id();
        let origin_id = loaded.origin_id();
        benchmark_sources.push(OctanePreparedBenchmarkSource {
            manifest_path,
            resolved_path,
            canonical_path,
            label: format!("{}:{}", plan.name, manifest_path),
            text,
            source: SourceSessionSource::with_provenance(
                loaded.source_code(),
                provider_id,
                origin_id,
            ),
            provider_id,
            origin_id,
        });
    }

    let mut generated_sources = Vec::new();
    generated_sources.push(append_generated_source(
        loader,
        plan,
        OctanePreparedGeneratedSourceKind::Prelude,
        generate_octane_prelude_source(),
    )?);
    if plan.deterministic_random {
        generated_sources.push(append_generated_source(
            loader,
            plan,
            OctanePreparedGeneratedSourceKind::DeterministicRandom,
            generate_octane_deterministic_random_source(),
        )?);
    }
    generated_sources.push(append_generated_source(
        loader,
        plan,
        OctanePreparedGeneratedSourceKind::Runner,
        generate_octane_runner_source(plan, run_config).map_err(|error| {
            OctanePreparationError::GeneratedSource {
                benchmark: plan.name,
                generated_kind: OctanePreparedGeneratedSourceKind::Runner,
                error,
            }
        })?,
    )?);

    let mut source_order = Vec::with_capacity(benchmark_sources.len() + generated_sources.len());
    source_order.push(OctanePreparedSourceOrderEntry::Generated(
        OctanePreparedGeneratedSourceKind::Prelude,
    ));
    if plan.deterministic_random {
        source_order.push(OctanePreparedSourceOrderEntry::Generated(
            OctanePreparedGeneratedSourceKind::DeterministicRandom,
        ));
    }
    source_order.extend(
        benchmark_sources
            .iter()
            .enumerate()
            .map(|(index, _)| OctanePreparedSourceOrderEntry::BenchmarkFile(index)),
    );
    source_order.push(OctanePreparedSourceOrderEntry::Generated(
        OctanePreparedGeneratedSourceKind::Runner,
    ));

    Ok(OctanePreparedBenchmark {
        plan,
        run_config,
        benchmark_sources,
        generated_sources,
        source_order,
    })
}

fn append_generated_source(
    loader: &mut ShellSourceLoader,
    plan: &'static OctaneBenchmarkPlan,
    kind: OctanePreparedGeneratedSourceKind,
    text: String,
) -> Result<OctanePreparedGeneratedSource, OctanePreparationError> {
    let label = format!("octane://{}/{}", plan.name, kind.label_component());
    let loaded = loader
        .append_source_text(ShellSourceAppendRequest::eval(
            text.clone(),
            ShellMode::ScriptFile,
            Some(label.clone()),
        ))
        .map_err(|error| OctanePreparationError::SourceAppend {
            benchmark: plan.name,
            generated_kind: kind,
            label: label.clone(),
            error,
        })?;
    let provider_id = loaded.provider_id();
    let origin_id = loaded.origin_id();

    Ok(OctanePreparedGeneratedSource {
        kind,
        label,
        text,
        source: SourceSessionSource::with_provenance(loaded.source_code(), provider_id, origin_id),
        provider_id,
        origin_id,
    })
}

fn source_text_to_string(source_text: &SourceText) -> String {
    match source_text {
        SourceText::Latin1(text) => String::from_utf8_lossy(text).into_owned(),
        SourceText::Utf16(text) => String::from_utf16_lossy(text),
    }
}

fn generate_octane_prelude_source() -> String {
    [
        "var isInBrowser = false;",
        "var self = this;",
        "var top = this;",
        "if (typeof performance === \"undefined\")",
        "    var performance = Date;",
        "",
    ]
    .join("\n")
}

fn generate_octane_deterministic_random_source() -> String {
    [
        "(function() {",
        "    var initialSeed = 49734321;",
        "    var seed = initialSeed;",
        "",
        "    Math.random = function() {",
        "        seed = ((seed + 0x7ed55d16) + (seed << 12)) & 0xffffffff;",
        "        seed = ((seed ^ 0xc761c23c) ^ (seed >>> 19)) & 0xffffffff;",
        "        seed = ((seed + 0x165667b1) + (seed << 5)) & 0xffffffff;",
        "        seed = ((seed + 0xd3a2646c) ^ (seed << 9)) & 0xffffffff;",
        "        seed = ((seed + 0xfd7046c5) + (seed << 3)) & 0xffffffff;",
        "        seed = ((seed ^ 0xb55a4f09) ^ (seed >>> 16)) & 0xffffffff;",
        "        return (seed >>> 0) / 0x100000000;",
        "    };",
        "",
        "    Math.random.__resetSeed = function() {",
        "        seed = initialSeed;",
        "    };",
        "})();",
        "",
    ]
    .join("\n")
}

fn generate_octane_runner_source(
    plan: &'static OctaneBenchmarkPlan,
    run_config: OctaneDefaultBenchmarkRunConfig,
) -> Result<String, OctaneGeneratedSourceError> {
    match plan.benchmark_class {
        OctaneBenchmarkClass::DefaultBenchmark => {
            let random_reset = if plan.deterministic_random {
                "        Math.random.__resetSeed();\n"
            } else {
                ""
            };
            Ok(format!(
                "\
(function() {{
    var __benchmark = new Benchmark({iterations});
    var results = [];
    for (var i = 0; i < {iterations}; i++) {{
        if (__benchmark.prepareForNextIteration)
            __benchmark.prepareForNextIteration();
{random_reset}        var start = performance.now();
        __benchmark.runIteration();
        var end = performance.now();
        results.push(Math.max(1, end - start));
    }}
    if (__benchmark.validate)
        __benchmark.validate();
    return results;
}})();
",
                iterations = run_config.iterations,
                random_reset = random_reset
            ))
        }
    }
}

pub const OCTANE_DRIVER_PLANS: &[OctaneBenchmarkPlan] = &[
    OctaneBenchmarkPlan {
        name: "Box2D",
        files: &["./Octane/box2d.js"],
        deterministic_random: true,
        iterations: None,
        worst_case_count: None,
        benchmark_class: OctaneBenchmarkClass::DefaultBenchmark,
    },
    OctaneBenchmarkPlan {
        name: "octane-code-load",
        files: &["./Octane/code-first-load.js"],
        deterministic_random: true,
        iterations: None,
        worst_case_count: None,
        benchmark_class: OctaneBenchmarkClass::DefaultBenchmark,
    },
    OctaneBenchmarkPlan {
        name: "crypto",
        files: &["./Octane/crypto.js"],
        deterministic_random: true,
        iterations: None,
        worst_case_count: None,
        benchmark_class: OctaneBenchmarkClass::DefaultBenchmark,
    },
    OctaneBenchmarkPlan {
        name: "delta-blue",
        files: &["./Octane/deltablue.js"],
        deterministic_random: true,
        iterations: None,
        worst_case_count: None,
        benchmark_class: OctaneBenchmarkClass::DefaultBenchmark,
    },
    OctaneBenchmarkPlan {
        name: "earley-boyer",
        files: &["./Octane/earley-boyer.js"],
        deterministic_random: true,
        iterations: None,
        worst_case_count: None,
        benchmark_class: OctaneBenchmarkClass::DefaultBenchmark,
    },
    OctaneBenchmarkPlan {
        name: "gbemu",
        files: &["./Octane/gbemu-part1.js", "./Octane/gbemu-part2.js"],
        deterministic_random: true,
        iterations: None,
        worst_case_count: None,
        benchmark_class: OctaneBenchmarkClass::DefaultBenchmark,
    },
    OctaneBenchmarkPlan {
        name: "mandreel",
        files: &["./Octane/mandreel.js"],
        deterministic_random: true,
        iterations: Some(80),
        worst_case_count: None,
        benchmark_class: OctaneBenchmarkClass::DefaultBenchmark,
    },
    OctaneBenchmarkPlan {
        name: "navier-stokes",
        files: &["./Octane/navier-stokes.js"],
        deterministic_random: true,
        iterations: None,
        worst_case_count: None,
        benchmark_class: OctaneBenchmarkClass::DefaultBenchmark,
    },
    OctaneBenchmarkPlan {
        name: "pdfjs",
        files: &["./Octane/pdfjs.js"],
        deterministic_random: true,
        iterations: None,
        worst_case_count: None,
        benchmark_class: OctaneBenchmarkClass::DefaultBenchmark,
    },
    OctaneBenchmarkPlan {
        name: "raytrace",
        files: &["./Octane/raytrace.js"],
        deterministic_random: false,
        iterations: None,
        worst_case_count: None,
        benchmark_class: OctaneBenchmarkClass::DefaultBenchmark,
    },
    OctaneBenchmarkPlan {
        name: "regexp",
        files: &["./Octane/regexp.js"],
        deterministic_random: true,
        iterations: None,
        worst_case_count: None,
        benchmark_class: OctaneBenchmarkClass::DefaultBenchmark,
    },
    OctaneBenchmarkPlan {
        name: "richards",
        files: &["./Octane/richards.js"],
        deterministic_random: true,
        iterations: None,
        worst_case_count: None,
        benchmark_class: OctaneBenchmarkClass::DefaultBenchmark,
    },
    OctaneBenchmarkPlan {
        name: "splay",
        files: &["./Octane/splay.js"],
        deterministic_random: true,
        iterations: None,
        worst_case_count: None,
        benchmark_class: OctaneBenchmarkClass::DefaultBenchmark,
    },
    OctaneBenchmarkPlan {
        name: "typescript",
        files: &[
            "./Octane/typescript-compiler.js",
            "./Octane/typescript-input.js",
            "./Octane/typescript.js",
        ],
        deterministic_random: true,
        iterations: Some(15),
        worst_case_count: Some(2),
        benchmark_class: OctaneBenchmarkClass::DefaultBenchmark,
    },
    OctaneBenchmarkPlan {
        name: "octane-zlib",
        files: &["./Octane/zlib-data.js", "./Octane/zlib.js"],
        deterministic_random: true,
        iterations: Some(15),
        worst_case_count: Some(2),
        benchmark_class: OctaneBenchmarkClass::DefaultBenchmark,
    },
];

pub const OCTANE_CORE_SELECTION: OctaneBenchmarkSelection = OctaneBenchmarkSelection {
    name: "Octane-core",
    benchmark_names: &[
        "richards",
        "delta-blue",
        "crypto",
        "splay",
        "navier-stokes",
        "raytrace",
    ],
};

pub fn octane_plan_by_name(name: &str) -> Option<&'static OctaneBenchmarkPlan> {
    OCTANE_DRIVER_PLANS.iter().find(|plan| plan.name == name)
}

pub fn octane_default_to_score(time_ms: f64) -> f64 {
    5000.0 / if time_ms < 1.0 { 1.0 } else { time_ms }
}

fn arithmetic_mean(values: &[f64]) -> f64 {
    values.iter().copied().sum::<f64>() / values.len() as f64
}

fn geometric_mean(values: &[f64]) -> f64 {
    values
        .iter()
        .copied()
        .product::<f64>()
        .powf(1.0 / values.len() as f64)
}

#[cfg(test)]
mod tests {
    use super::super::ShellFilesystemOperation;
    use super::*;
    use std::collections::HashSet;
    use std::fs;
    use std::time::{SystemTime, UNIX_EPOCH};

    struct TempJetStreamRoot {
        path: PathBuf,
    }

    impl TempJetStreamRoot {
        fn new() -> Self {
            let unique = SystemTime::now()
                .duration_since(UNIX_EPOCH)
                .expect("system time should be after unix epoch")
                .as_nanos();
            let path = std::env::temp_dir()
                .join(format!("octane-prep-test-{}-{unique}", std::process::id()));
            fs::create_dir_all(&path).expect("temporary root should be created");
            Self { path }
        }

        fn path(&self) -> &Path {
            &self.path
        }

        fn write_manifest_file(&self, manifest_path: &str, text: &str) {
            let relative_path = manifest_path
                .strip_prefix("./")
                .expect("test manifest path should be explicitly relative");
            let path = self.path.join(relative_path);
            fs::create_dir_all(path.parent().expect("test path should have a parent"))
                .expect("test parent directory should be created");
            fs::write(path, text).expect("test file should be written");
        }
    }

    impl Drop for TempJetStreamRoot {
        fn drop(&mut self) {
            let _ = fs::remove_dir_all(&self.path);
        }
    }

    fn assert_close(actual: f64, expected: f64) {
        assert!(
            (actual - expected).abs() < 1e-9,
            "expected {actual} to be within 1e-9 of {expected}"
        );
    }

    #[test]
    fn octane_full_manifest_matches_driver_order_and_metadata() {
        let names: Vec<_> = OCTANE_DRIVER_PLANS.iter().map(|plan| plan.name).collect();
        assert_eq!(
            names,
            vec![
                "Box2D",
                "octane-code-load",
                "crypto",
                "delta-blue",
                "earley-boyer",
                "gbemu",
                "mandreel",
                "navier-stokes",
                "pdfjs",
                "raytrace",
                "regexp",
                "richards",
                "splay",
                "typescript",
                "octane-zlib",
            ]
        );

        assert!(OCTANE_DRIVER_PLANS
            .iter()
            .all(|plan| plan.benchmark_class == OctaneBenchmarkClass::DefaultBenchmark));
        assert!(OCTANE_DRIVER_PLANS
            .iter()
            .filter(|plan| plan.name != "raytrace")
            .all(|plan| plan.deterministic_random));

        let raytrace = octane_plan_by_name("raytrace").expect("raytrace plan");
        assert_eq!(raytrace.files, &["./Octane/raytrace.js"]);
        assert!(!raytrace.deterministic_random);

        let gbemu = octane_plan_by_name("gbemu").expect("gbemu plan");
        assert_eq!(
            gbemu.files,
            &["./Octane/gbemu-part1.js", "./Octane/gbemu-part2.js"]
        );

        let mandreel = octane_plan_by_name("mandreel").expect("mandreel plan");
        assert_eq!(mandreel.iterations, Some(80));
        assert_eq!(mandreel.worst_case_count, None);

        let typescript = octane_plan_by_name("typescript").expect("typescript plan");
        assert_eq!(
            typescript.files,
            &[
                "./Octane/typescript-compiler.js",
                "./Octane/typescript-input.js",
                "./Octane/typescript.js",
            ]
        );
        assert_eq!(typescript.iterations, Some(15));
        assert_eq!(typescript.worst_case_count, Some(2));

        let zlib = octane_plan_by_name("octane-zlib").expect("octane-zlib plan");
        assert_eq!(zlib.files, &["./Octane/zlib-data.js", "./Octane/zlib.js"]);
        assert_eq!(zlib.iterations, Some(15));
        assert_eq!(zlib.worst_case_count, Some(2));
    }

    #[test]
    fn octane_core_selection_preserves_accepted_subset_order() {
        assert_eq!(OCTANE_CORE_SELECTION.name, "Octane-core");
        assert_eq!(
            OCTANE_CORE_SELECTION.benchmark_names,
            &[
                "richards",
                "delta-blue",
                "crypto",
                "splay",
                "navier-stokes",
                "raytrace",
            ]
        );

        let plans = OCTANE_CORE_SELECTION
            .resolved_plans()
            .expect("core plans should resolve");
        let names: Vec<_> = plans.iter().map(|plan| plan.name).collect();
        assert_eq!(
            names,
            vec![
                "richards",
                "delta-blue",
                "crypto",
                "splay",
                "navier-stokes",
                "raytrace",
            ]
        );

        let full_driver_names: Vec<_> = OCTANE_DRIVER_PLANS.iter().map(|plan| plan.name).collect();
        assert_ne!(&names, &full_driver_names[..names.len()]);
    }

    #[test]
    fn octane_suite_resolution_preserves_full_and_core_order() {
        assert_eq!(OctaneSuite::Full.name(), "Octane-full");
        let full = OctaneSuite::Full
            .resolved_plans()
            .expect("full suite should resolve");
        assert_eq!(full.len(), OCTANE_DRIVER_PLANS.len());
        assert_eq!(full.first().map(|plan| plan.name), Some("Box2D"));
        assert_eq!(full.last().map(|plan| plan.name), Some("octane-zlib"));

        assert_eq!(OctaneSuite::Core.name(), "Octane-core");
        let core = OctaneSuite::Core
            .resolved_plans()
            .expect("core suite should resolve");
        let names: Vec<_> = core.iter().map(|plan| plan.name).collect();
        assert_eq!(
            names,
            vec![
                "richards",
                "delta-blue",
                "crypto",
                "splay",
                "navier-stokes",
                "raytrace",
            ]
        );
    }

    #[test]
    fn octane_run_config_resolves_defaults_and_overrides() {
        let box2d = octane_plan_by_name("Box2D").expect("Box2D plan");
        assert_eq!(
            box2d.default_run_config(),
            Ok(OctaneDefaultBenchmarkRunConfig {
                iterations: OCTANE_DEFAULT_ITERATION_COUNT,
                worst_case_count: OCTANE_DEFAULT_WORST_CASE_COUNT,
            })
        );

        let mandreel = octane_plan_by_name("mandreel").expect("mandreel plan");
        assert_eq!(
            mandreel.default_run_config(),
            Ok(OctaneDefaultBenchmarkRunConfig {
                iterations: 80,
                worst_case_count: OCTANE_DEFAULT_WORST_CASE_COUNT,
            })
        );

        let typescript = octane_plan_by_name("typescript").expect("typescript plan");
        assert_eq!(
            typescript.default_run_config(),
            Ok(OctaneDefaultBenchmarkRunConfig {
                iterations: 15,
                worst_case_count: 2,
            })
        );
        assert_eq!(
            typescript.run_config(OctaneBenchmarkRunOverrides::new(Some(9), Some(3))),
            Ok(OctaneDefaultBenchmarkRunConfig {
                iterations: 9,
                worst_case_count: 3,
            })
        );
    }

    #[test]
    fn octane_default_benchmark_scoring_matches_known_times() {
        let config = OctaneDefaultBenchmarkRunConfig::new(5, 2).expect("valid config");
        let scores = config
            .score_results(&[0.0, 10.0, 40.0, 30.0, 20.0])
            .expect("scores");

        assert_close(scores.first_iteration, 5000.0);
        assert_close(scores.worst_case, 5000.0 / 35.0);
        assert_close(scores.average, 5000.0 / 25.0);
        assert_close(
            scores.score,
            (scores.first_iteration * scores.worst_case * scores.average).powf(1.0 / 3.0),
        );
    }

    #[test]
    fn octane_scoring_rejects_invalid_iteration_and_result_counts() {
        assert_eq!(
            OctaneDefaultBenchmarkRunConfig::new(4, 4),
            Err(OctaneScoringError::IterationsMustExceedWorstCase {
                iterations: 4,
                worst_case_count: 4,
            })
        );

        let config = OctaneDefaultBenchmarkRunConfig::new(5, 4).expect("valid config");
        assert_eq!(
            config.score_results(&[1.0, 2.0, 3.0, 4.0]),
            Err(OctaneScoringError::TooFewResults {
                expected: 5,
                actual: 4,
            })
        );
        assert_eq!(
            config.score_results(&[1.0, 2.0, 3.0, 4.0, 5.0, 6.0]),
            Err(OctaneScoringError::TooManyResults {
                expected: 5,
                actual: 6,
            })
        );
    }

    #[test]
    fn octane_preparation_loads_manifest_files_in_order_with_provenance() {
        let root = TempJetStreamRoot::new();
        let plan = octane_plan_by_name("gbemu").expect("gbemu plan");
        root.write_manifest_file("./Octane/gbemu-part1.js", "var gbemuPart1 = 1;\n");
        root.write_manifest_file("./Octane/gbemu-part2.js", "var gbemuPart2 = 2;\n");

        let prepared =
            prepare_octane_benchmark(root.path(), plan, OctaneBenchmarkRunOverrides::none())
                .expect("benchmark should prepare");

        assert_eq!(prepared.plan.name, "gbemu");
        assert_eq!(prepared.benchmark_sources.len(), 2);
        assert_eq!(
            prepared
                .benchmark_sources
                .iter()
                .map(|source| source.manifest_path)
                .collect::<Vec<_>>(),
            vec!["./Octane/gbemu-part1.js", "./Octane/gbemu-part2.js"]
        );
        assert_eq!(prepared.benchmark_sources[0].text, "var gbemuPart1 = 1;\n");
        assert_eq!(prepared.benchmark_sources[1].text, "var gbemuPart2 = 2;\n");
        assert_eq!(
            prepared.benchmark_sources[0].canonical_path,
            fs::canonicalize(root.path().join("Octane/gbemu-part1.js"))
                .expect("test file should canonicalize")
        );
        assert_eq!(
            prepared.benchmark_sources[1].canonical_path,
            fs::canonicalize(root.path().join("Octane/gbemu-part2.js"))
                .expect("test file should canonicalize")
        );
        assert_eq!(
            prepared.benchmark_sources[0].source.provider_id(),
            Some(prepared.benchmark_sources[0].provider_id)
        );
        assert_eq!(
            prepared.benchmark_sources[0].source.origin_id(),
            Some(prepared.benchmark_sources[0].origin_id)
        );
        assert_eq!(
            prepared.source_order,
            vec![
                OctanePreparedSourceOrderEntry::Generated(
                    OctanePreparedGeneratedSourceKind::Prelude
                ),
                OctanePreparedSourceOrderEntry::Generated(
                    OctanePreparedGeneratedSourceKind::DeterministicRandom
                ),
                OctanePreparedSourceOrderEntry::BenchmarkFile(0),
                OctanePreparedSourceOrderEntry::BenchmarkFile(1),
                OctanePreparedSourceOrderEntry::Generated(
                    OctanePreparedGeneratedSourceKind::Runner
                ),
            ]
        );
    }

    #[test]
    fn octane_suite_preparation_uses_selected_suite_and_run_overrides() {
        let root = TempJetStreamRoot::new();
        let core_plans = OctaneSuite::Core
            .resolved_plans()
            .expect("core suite should resolve");
        for plan in &core_plans {
            for manifest_path in plan.files {
                root.write_manifest_file(manifest_path, "function Benchmark() {}\n");
            }
        }

        let config = OctanePreparationConfig::new(
            root.path(),
            OctaneRunConfig::with_overrides(
                OctaneSuite::Core,
                OctaneBenchmarkRunOverrides::new(Some(9), Some(3)),
            ),
        );
        let prepared = prepare_octane_suite(config.clone()).expect("core suite should prepare");

        assert_eq!(prepared.config, config);
        assert_eq!(prepared.benchmarks.len(), core_plans.len());
        assert_eq!(
            prepared
                .benchmarks
                .iter()
                .map(|benchmark| benchmark.plan.name)
                .collect::<Vec<_>>(),
            core_plans.iter().map(|plan| plan.name).collect::<Vec<_>>()
        );
        assert!(prepared.benchmarks.iter().all(|benchmark| {
            benchmark.run_config
                == OctaneDefaultBenchmarkRunConfig {
                    iterations: 9,
                    worst_case_count: 3,
                }
        }));

        let mut provider_ids = HashSet::new();
        for benchmark in &prepared.benchmarks {
            for source in &benchmark.benchmark_sources {
                assert!(
                    provider_ids.insert(source.provider_id),
                    "benchmark provider IDs should be unique across a prepared suite"
                );
            }
            for source in &benchmark.generated_sources {
                assert!(
                    provider_ids.insert(source.provider_id),
                    "generated provider IDs should be unique across a prepared suite"
                );
            }
        }
    }

    #[test]
    fn octane_preparation_adds_deterministic_random_only_for_marked_plans() {
        let root = TempJetStreamRoot::new();
        let crypto = octane_plan_by_name("crypto").expect("crypto plan");
        let raytrace = octane_plan_by_name("raytrace").expect("raytrace plan");
        root.write_manifest_file("./Octane/crypto.js", "function Benchmark() {}\n");
        root.write_manifest_file("./Octane/raytrace.js", "function Benchmark() {}\n");

        let prepared_crypto =
            prepare_octane_benchmark(root.path(), crypto, OctaneBenchmarkRunOverrides::none())
                .expect("crypto should prepare");
        let random_source = prepared_crypto
            .generated_source(OctanePreparedGeneratedSourceKind::DeterministicRandom)
            .expect("crypto should include deterministic random source");
        assert!(random_source.text.contains("var initialSeed = 49734321;"));
        assert!(random_source.text.contains("0x100000000"));
        assert_eq!(
            random_source.source.provider_id(),
            Some(random_source.provider_id)
        );
        assert_eq!(
            random_source.source.origin_id(),
            Some(random_source.origin_id)
        );

        let prepared_raytrace =
            prepare_octane_benchmark(root.path(), raytrace, OctaneBenchmarkRunOverrides::none())
                .expect("raytrace should prepare");
        assert!(prepared_raytrace
            .generated_source(OctanePreparedGeneratedSourceKind::DeterministicRandom)
            .is_none());
        assert_eq!(
            prepared_raytrace.source_order,
            vec![
                OctanePreparedSourceOrderEntry::Generated(
                    OctanePreparedGeneratedSourceKind::Prelude
                ),
                OctanePreparedSourceOrderEntry::BenchmarkFile(0),
                OctanePreparedSourceOrderEntry::Generated(
                    OctanePreparedGeneratedSourceKind::Runner
                ),
            ]
        );
    }

    #[test]
    fn octane_runner_resets_random_before_measured_iteration_when_deterministic() {
        let root = TempJetStreamRoot::new();
        let crypto = octane_plan_by_name("crypto").expect("crypto plan");
        let raytrace = octane_plan_by_name("raytrace").expect("raytrace plan");
        root.write_manifest_file("./Octane/crypto.js", "function Benchmark() {}\n");
        root.write_manifest_file("./Octane/raytrace.js", "function Benchmark() {}\n");

        let prepared_crypto =
            prepare_octane_benchmark(root.path(), crypto, OctaneBenchmarkRunOverrides::none())
                .expect("crypto should prepare");
        let crypto_runner = &prepared_crypto
            .generated_source(OctanePreparedGeneratedSourceKind::Runner)
            .expect("crypto runner source")
            .text;
        let reset_index = crypto_runner
            .find("Math.random.__resetSeed();")
            .expect("runner should reset random");
        let run_index = crypto_runner
            .find("__benchmark.runIteration();")
            .expect("runner should run benchmark");
        assert!(reset_index < run_index);
        assert!(crypto_runner.contains("var __benchmark = new Benchmark(120);"));
        assert!(crypto_runner.contains("return results;"));

        let prepared_raytrace =
            prepare_octane_benchmark(root.path(), raytrace, OctaneBenchmarkRunOverrides::none())
                .expect("raytrace should prepare");
        let raytrace_runner = &prepared_raytrace
            .generated_source(OctanePreparedGeneratedSourceKind::Runner)
            .expect("raytrace runner source")
            .text;
        assert!(!raytrace_runner.contains("Math.random.__resetSeed();"));
        assert!(raytrace_runner.contains("__benchmark.runIteration();"));
    }

    #[test]
    fn octane_preparation_classifies_missing_manifest_file_as_source_load() {
        let root = TempJetStreamRoot::new();
        let plan = octane_plan_by_name("crypto").expect("crypto plan");

        let error =
            prepare_octane_benchmark(root.path(), plan, OctaneBenchmarkRunOverrides::none())
                .expect_err("missing file should fail");

        assert_eq!(error.phase(), OctanePreparationPhase::SourceLoad);
        match error {
            OctanePreparationError::SourceLoad {
                benchmark,
                manifest_path,
                resolved_path,
                error:
                    ShellSourceLoadError::Filesystem {
                        operation,
                        path,
                        message: _,
                    },
            } => {
                assert_eq!(benchmark, "crypto");
                assert_eq!(manifest_path, "./Octane/crypto.js");
                assert_eq!(resolved_path, root.path().join("./Octane/crypto.js"));
                assert_eq!(operation, ShellFilesystemOperation::Canonicalize);
                assert_eq!(path, root.path().join("./Octane/crypto.js"));
            }
            error => panic!("unexpected preparation error: {error:?}"),
        }
    }

    #[test]
    fn octane_preparation_never_adds_stale_octane_run_js_source() {
        let root = TempJetStreamRoot::new();
        let plan = octane_plan_by_name("crypto").expect("crypto plan");
        root.write_manifest_file("./Octane/crypto.js", "function Benchmark() {}\n");

        let prepared =
            prepare_octane_benchmark(root.path(), plan, OctaneBenchmarkRunOverrides::none())
                .expect("crypto should prepare");

        for source in &prepared.benchmark_sources {
            assert!(!source.manifest_path.contains("Octane/run.js"));
            assert!(!source
                .resolved_path
                .to_string_lossy()
                .contains("Octane/run.js"));
            assert!(!source.label.contains("Octane/run.js"));
            assert!(!source.text.contains("Octane/run.js"));
        }
        for source in &prepared.generated_sources {
            assert!(!source.label.contains("Octane/run.js"));
            assert!(!source.text.contains("Octane/run.js"));
        }
    }
}
