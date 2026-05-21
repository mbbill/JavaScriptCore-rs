//! JetStream 3 Octane manifest and scoring contracts for the shell.
//!
//! This module intentionally does not load benchmark files or execute
//! JavaScript. It only mirrors the driver-owned Octane plan metadata and the
//! synchronous `DefaultBenchmark` scoring math used by JetStream 3.

use std::cmp::Ordering;

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
    use super::*;

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
}
