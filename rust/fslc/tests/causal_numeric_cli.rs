// SPDX-License-Identifier: Apache-2.0
// Copyright 2026 Ryoichi Izumita

//! #1475: located sum failures, exact scaled comparisons, and preservation.
//! Integer expectations below are literal mathematical values, not tool output.

use std::path::PathBuf;
use std::process::Command;
use std::sync::atomic::{AtomicU64, Ordering};

use serde_json::{Value, json};

const M: u64 = 9_223_372_036_854_775_807;
const Q: u64 = 6_148_914_691_236_517_205;
static NEXT: AtomicU64 = AtomicU64::new(0);

fn assert_base_golden(name: &str, actual: &Value) {
    let path = std::path::Path::new(env!("CARGO_MANIFEST_DIR"))
        .join("tests/fixtures/issue_1475_golden")
        .join(format!("{name}.json"));
    // Frozen output from c25f8653; repeating this binary is only a separate
    // determinism control, never the preservation oracle.
    let expected: Value = serde_json::from_slice(&std::fs::read(path).expect("read base golden"))
        .expect("base golden JSON");
    assert_eq!(expected["base"], "c25f8653b9c83a247321f375f98cd59f430b6326");
    assert_eq!(actual, &expected["observed"], "full base output: {name}");
}

struct Model {
    root: PathBuf,
    source: String,
}

impl Drop for Model {
    fn drop(&mut self) {
        std::fs::remove_dir_all(&self.root).expect("remove test model");
    }
}

impl Model {
    fn new(source: String) -> Self {
        let root = std::env::temp_dir().join(format!(
            "fslc-causal-numeric-{}-{}",
            std::process::id(),
            NEXT.fetch_add(1, Ordering::Relaxed)
        ));
        std::fs::create_dir_all(&root).expect("create model directory");
        std::fs::write(root.join("model.fsl"), &source).expect("write model");
        std::fs::write(
            root.join("biz.fsl"),
            "spec Biz { state { x: 0..3 } init { x = 0 } action enable() { requires x < 3 x = x + 1 } invariant Ok { x >= 0 } }",
        )
        .expect("write real observable binding");
        Self { root, source }
    }

    fn run(&self, args: &[&str], expected_exit: i32) -> Value {
        let output = Command::new(env!("CARGO_BIN_EXE_fslc"))
            .args(args)
            .current_dir(&self.root)
            .env("FSLC_CACHE", "off")
            .output()
            .expect("run native fslc");
        assert_eq!(
            output.status.code(),
            Some(expected_exit),
            "{args:?}; stdout={} stderr={}",
            String::from_utf8_lossy(&output.stdout),
            String::from_utf8_lossy(&output.stderr)
        );
        assert!(output.stderr.is_empty(), "{args:?}: {:?}", output.stderr);
        serde_json::from_slice(&output.stdout).expect("single JSON stdout")
    }

    fn check(&self) {
        let plain = self.run(&["check", "model.fsl"], 0);
        let explicit = self.run(&["causal", "check", "model.fsl"], 0);
        assert_eq!(plain, explicit, "full check envelopes, no exclusions");
        assert_eq!(plain["result"], "causal_model_checked");
        assert_eq!(plain["schema_version"], "causal-check.v0");
        assert_eq!(plain["formal_result"], "not_run");
        println!("CAUSAL CHECK {plain}");
    }

    fn analyze(&self, consumer: &str, exit: i32) -> Value {
        let flag = if consumer == "causal-review" {
            "--profile"
        } else {
            "--projection"
        };
        self.run(&["causal", "analyze", "model.fsl", flag, consumer], exit)
    }

    fn success(&self, consumer: &str) -> Value {
        let value = self.analyze(consumer, 0);
        assert_eq!(value["result"], "causal_analyzed");
        assert_eq!(value["formal_result"], "not_run");
        assert_eq!(
            value["schema_version"],
            if consumer == "causal-review" {
                "causal-findings.v0"
            } else {
                "causal-graph.v0"
            }
        );
        assert_eq!(
            value,
            self.analyze(consumer, 0),
            "same-binary repeat determinism"
        );
        println!("CAUSAL SUCCESS {consumer} {value}");
        value
    }

    fn overflow(&self, consumer: &str, claim: &str, format: Option<&str>) {
        self.check();
        let value = if let Some(format) = format {
            self.run(
                &[
                    "causal",
                    "analyze",
                    "model.fsl",
                    "--projection",
                    consumer,
                    "--format",
                    format,
                ],
                2,
            )
        } else {
            self.analyze(consumer, 2)
        };
        // Compute the source witness independently, not from production spans.
        let needle = format!("claim {claim} ");
        let (line, text) = self
            .source
            .lines()
            .enumerate()
            .find(|(_, text)| text.contains(&needle))
            .expect("claim declaration");
        let column = text.find(&needle).expect("claim keyword") + "claim ".len() + 1;
        assert_eq!(
            value,
            json!({
                "fsl": "1.0", "result": "error", "kind": "semantics",
                "diagnostic": "causal_invalid_model",
                "message": format!("causal time range overflow while adding claim '{claim}'"),
                "loc": {"line": line + 1, "column": column}
            })
        );
        if claim == "CD" {
            assert_eq!(value["loc"], json!({"line": 25, "column": 9}));
        }
    }
}

fn claim(id: &str, from: &str, to: &str, lag: Option<u64>) -> String {
    let lag = lag.map_or_else(|| "unknown".to_owned(), |n| format!("{n}..{n}"));
    format!(
        "  claim {id} {from} -> {to} {{\n    version 1\n    status active\n    polarity positive\n    lag {lag}\n    persists 0..0\n    basis hypothesis\n  }}\n"
    )
}

fn chain(lags: [Option<u64>; 3], shortcut: bool) -> Model {
    chain_with_horizon(lags, shortcut, M)
}

fn chain_with_horizon(lags: [Option<u64>; 3], shortcut: bool, horizon: u64) -> Model {
    let mut source = format!(
        "// SPDX-License-Identifier: Apache-2.0\ncausal TimeProbe {{\n  timebase day\n  horizon {horizon}\n  variable a {{ role intervention }}\n  variable b {{ role mediator }}\n  variable c {{ role mediator }}\n  variable d {{ role outcome }}\n"
    );
    for ((id, from, to), lag) in [("AB", "a", "b"), ("BC", "b", "c"), ("CD", "c", "d")]
        .into_iter()
        .zip(lags)
    {
        source.push_str(&claim(id, from, to, lag));
    }
    if shortcut {
        source.push_str(&claim("AD", "a", "d", Some(1)));
    }
    source.push_str("}\n");
    Model::new(source)
}

fn cycle(lag: u64) -> Model {
    let mut source = format!(
        "causal Cycle {{\n timebase day\n horizon {M}\n variable a {{ role mediator }}\n variable b {{ role mediator }}\n variable c {{ role mediator }}\n"
    );
    for (id, from, to) in [("AB", "a", "b"), ("BC", "b", "c"), ("CA", "c", "a")] {
        source.push_str(&claim(id, from, to, Some(lag)));
    }
    source.push_str(" feedback F { claims AB, BC, CA }\n}\n");
    Model::new(source)
}

macro_rules! earliest_detector {
    ($name:ident, $consumer:literal) => {
        #[test]
        fn $name() {
            chain([Some(M); 3], false).overflow($consumer, "CD", None);
        }
    };
}
earliest_detector!(earliest_graph, "causal_graph");
earliest_detector!(earliest_timeline, "causal_timeline");
earliest_detector!(earliest_traceability, "causal_traceability_graph");
earliest_detector!(earliest_review, "causal-review");

macro_rules! path_detector {
    ($name:ident, $format:literal) => {
        #[test]
        fn $name() {
            chain([Some(M); 3], true).overflow("causal_graph", "CD", Some($format));
        }
    };
}
path_detector!(path_json, "json");
path_detector!(path_dot, "dot");
path_detector!(path_mermaid, "mermaid");

#[test]
fn feedback_graph() {
    cycle(M).overflow("causal_graph", "CA", None);
}

#[test]
fn feedback_review() {
    cycle(M).overflow("causal-review", "CA", None);
}

#[test]
fn tight_sum_overflow_graph() {
    chain([Some(Q), Some(Q), Some(Q + 1)], false).overflow("causal_graph", "CD", None);
}

#[test]
fn tight_sum_overflow_timeline() {
    chain([Some(Q), Some(Q), Some(Q + 1)], false).overflow("causal_timeline", "CD", None);
}

/// `first_pass.max` follows the longest path 3M; #1506 rejects rather than saturates it.
#[test]
fn shortcut_longest_path_max_overflow_timeline() {
    chain([Some(M); 3], true).overflow("causal_timeline", "CD", None);
}

#[test]
fn checks_still_accept_overflow_and_masked_models() {
    chain([Some(M); 3], false).check();
    chain([Some(M); 3], true).check();
    cycle(M).check();
}

#[test]
fn nonconsumers_do_not_reject_unused_sums() {
    let shortcut = chain([Some(M); 3], true);
    for consumer in ["causal_traceability_graph", "causal-review"] {
        shortcut.success(consumer);
    }
    let feedback = cycle(M);
    for consumer in ["causal_timeline", "causal_traceability_graph"] {
        feedback.success(consumer);
    }
}

#[test]
fn small_chain_and_explicit_unknown_preserve_meaning() {
    let small = chain_with_horizon([Some(1); 3], false, 730);
    small.check();
    let mut preserved = json!({"check": small.run(&["check", "model.fsl"], 0), "consumers": {}});
    for consumer in [
        "causal_graph",
        "causal_timeline",
        "causal_traceability_graph",
        "causal-review",
    ] {
        let value = small.success(consumer);
        preserved["consumers"][consumer] = value.clone();
        if consumer == "causal_graph" {
            assert_eq!(
                value["paths"][0]["first_response"],
                json!({"min": 3, "max": 3})
            );
        } else if consumer == "causal_timeline" {
            assert_eq!(
                value["timelines"][0]["first_pass"],
                json!({"min": 3, "max": 3})
            );
        }
    }
    assert_base_golden("causal-small-chain", &preserved);
    let unknown = chain([None, Some(1), Some(1)], false);
    unknown.check();
    assert_eq!(
        unknown.success("causal_graph")["paths"][0]["first_response"],
        "unknown"
    );
    assert_eq!(
        unknown.success("causal_timeline")["timelines"][0]["first_pass"],
        "unknown"
    );
}

#[test]
fn representable_sums_graph_and_timeline() {
    for (lags, expected) in [
        ([Some(M), Some(M), Some(0)], 18_446_744_073_709_551_614u64),
        ([Some(Q); 3], 18_446_744_073_709_551_615u64),
    ] {
        let model = chain(lags, false);
        model.check();
        let graph = model.success("causal_graph");
        let timeline = model.success("causal_timeline");
        let traceability = model.success("causal_traceability_graph");
        assert_eq!(
            graph["paths"][0]["first_response"],
            json!({"min": expected, "max": expected})
        );
        assert_eq!(
            timeline["timelines"][0]["first_pass"],
            json!({"min": expected, "max": expected})
        );
        assert_base_golden(
            if expected == u64::MAX {
                "causal-u64-max"
            } else {
                "causal-u64-minus-one"
            },
            &json!({"check": model.run(&["check", "model.fsl"], 0),
                "consumers": {"causal_graph": graph, "causal_timeline": timeline,
                    "causal_traceability_graph": traceability}}),
        );
    }
}

#[test]
fn representable_sums_review_uses_exact_comparison() {
    for lags in [[Some(M), Some(M), Some(0)], [Some(Q); 3]] {
        chain(lags, false).success("causal-review");
    }
}

#[test]
fn feedback_at_u64_max_is_preserved() {
    let model = cycle(Q);
    model.check();
    let graph = model.success("causal_graph");
    assert_eq!(
        graph["feedbacks"][0]["cycle_lag"],
        json!({"min": 18_446_744_073_709_551_615u64, "max": 18_446_744_073_709_551_615u64})
    );
    assert_eq!(graph["feedbacks"][0]["max_repetitions_within_horizon"], 0);
    model.success("causal-review");
}

// Two-edge models keep each parser literal <= i64::MAX even when E > i64::MAX.
fn comparison(first: u64, second: u64, observable: Option<&str>, horizon: u64) -> Model {
    let b = if observable == Some("b") {
        " observes state biz.x"
    } else {
        ""
    };
    let d = if observable == Some("d") {
        " observes state biz.x"
    } else {
        ""
    };
    Model::new(format!(
        "causal Compare {{\n uses biz from \"biz.fsl\"\n timebase day\n horizon {horizon}\n variable a {{ role intervention }}\n variable b {{ role mediator{b} }}\n variable d {{ role outcome{d} }}\n{}{} }}\n",
        claim("AB", "a", "b", Some(first)),
        claim("BD", "b", "d", Some(second))
    ))
}

fn long_horizon(value: &Value) -> bool {
    value["findings"]
        .as_array()
        .expect("findings")
        .iter()
        .any(|finding| finding["finding_type"] == "long_horizon_without_leading_indicator")
}

#[test]
fn review_factor_two_threshold_and_overflow() {
    for (first, second, expected) in [
        (4_611_686_018_427_387_903, 0, false),
        (4_611_686_018_427_387_904, 0, true),
        (M, 1, true),
    ] {
        let model = comparison(first, second, None, M);
        model.check();
        assert_eq!(long_horizon(&model.success("causal-review")), expected);
    }
}

#[test]
fn review_factor_four_threshold_and_overflow() {
    for (first, second, expected) in [
        (2_305_843_009_213_693_951, 3_694_156_990_786_306_049, false),
        (2_305_843_009_213_693_952, 3_694_156_990_786_306_048, true),
        (5_000_000_000_000_000_000, 1_000_000_000_000_000_000, true),
    ] {
        let model = comparison(first, second, Some("b"), M);
        model.check();
        assert_eq!(long_horizon(&model.success("causal-review")), expected);
    }
}

fn assert_indicator(model: &Model, consumer: &str, node: &str, expected: &str) {
    model.check();
    let value = model.success(consumer);
    let node = value["nodes"]
        .as_array()
        .expect("nodes")
        .iter()
        .find(|entry| entry["id"] == node)
        .expect("observable node");
    assert_eq!(node["indicator_class"], expected);
}

macro_rules! indicator_detectors {
    ($thirds:ident, $twothirds:ident, $reference:ident, $consumer:literal) => {
        #[test]
        fn $thirds() {
            for (e, suffix, expected) in [
                (
                    3_074_457_345_618_258_602,
                    6_148_914_691_236_517_204,
                    "leading",
                ),
                (
                    3_074_457_345_618_258_603,
                    6_148_914_691_236_517_203,
                    "intermediate",
                ),
            ] {
                assert_indicator(
                    &comparison(e, suffix, Some("b"), M),
                    $consumer,
                    "variable:b",
                    expected,
                );
            }
            assert_indicator(
                &comparison(M, 0, Some("d"), M),
                $consumer,
                "variable:d",
                "lagging",
            );
        }
        #[test]
        fn $twothirds() {
            for (e, suffix, expected) in [
                (
                    7_999_999_999_999_999_999,
                    4_000_000_000_000_000_001,
                    "intermediate",
                ),
                (
                    8_000_000_000_000_000_000,
                    4_000_000_000_000_000_000,
                    "lagging",
                ),
                (
                    8_000_000_000_000_000_001,
                    3_999_999_999_999_999_999,
                    "lagging",
                ),
            ] {
                assert_indicator(
                    &comparison(e, suffix, Some("b"), M),
                    $consumer,
                    "variable:b",
                    expected,
                );
            }
        }
        #[test]
        fn $reference() {
            assert_indicator(
                &comparison(
                    4_000_000_000_000_000_000,
                    6_000_000_000_000_000_000,
                    Some("b"),
                    M,
                ),
                $consumer,
                "variable:b",
                "intermediate",
            );
        }
    };
}
indicator_detectors!(
    thirds_graph,
    two_thirds_graph,
    reference_graph,
    "causal_graph"
);
indicator_detectors!(
    thirds_timeline,
    two_thirds_timeline,
    reference_timeline,
    "causal_timeline"
);
indicator_detectors!(
    thirds_traceability,
    two_thirds_traceability,
    reference_traceability,
    "causal_traceability_graph"
);
