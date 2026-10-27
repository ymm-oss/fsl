// SPDX-License-Identifier: Apache-2.0
// Copyright 2026 Ryoichi Izumita

//! #1506: exact maximum-time sums or located refusal, without changing check acceptance.
//! Expectations are mathematical integer literals, independent of production helpers.

use std::fmt::Write;
use std::path::{Path, PathBuf};
use std::process::{Command, Output};
use std::sync::atomic::{AtomicU64, Ordering};

use serde_json::{Value, json};

const BASE: &str = "5738cc2c711b691a9d453b0190ead1a2ff77fc50";
const M: u64 = 9_223_372_036_854_775_807;
const Q: u64 = 6_148_914_691_236_517_205;
const MAX: u64 = 18_446_744_073_709_551_615;
static NEXT: AtomicU64 = AtomicU64::new(0);

fn assert_base_golden(name: &str, actual: &Value) {
    let path = Path::new(env!("CARGO_MANIFEST_DIR"))
        .join("tests/fixtures/issue_1506_golden")
        .join(format!("{name}.json"));
    // Frozen output from the named base; repeating this binary is only a
    // separate determinism control, never the preservation oracle.
    let expected: Value = serde_json::from_slice(&std::fs::read(path).expect("read base golden"))
        .expect("base golden JSON");
    assert_eq!(expected["base"], BASE);
    assert_eq!(actual, &expected["observed"], "full base output: {name}");
}

struct Model {
    root: PathBuf,
    source: String,
    warnings: Vec<Value>,
}

impl Drop for Model {
    fn drop(&mut self) {
        std::fs::remove_dir_all(&self.root).expect("remove test model");
    }
}

impl Model {
    fn new(source: String) -> Self {
        let root = std::env::temp_dir().join(format!(
            "fslc-causal-max-{}-{}",
            std::process::id(),
            NEXT.fetch_add(1, Ordering::Relaxed)
        ));
        std::fs::create_dir_all(&root).expect("create model directory");
        std::fs::write(root.join("model.fsl"), &source).expect("write model");
        std::fs::write(
            root.join("biz.fsl"),
            "spec Biz { state { x: 0..3 } init { x = 0 } action enable() { requires x < 3 x = x + 1 } invariant Ok { x >= 0 } }",
        )
        .expect("write observable binding");
        Self {
            root,
            source,
            warnings: Vec::new(),
        }
    }

    fn location(&self, keyword: &str, id: &str) -> Value {
        let needle = format!("{keyword} {id} ");
        let (line, text) = self
            .source
            .lines()
            .enumerate()
            .find(|(_, text)| text.contains(&needle))
            .expect("source declaration");
        let column = text.find(&needle).expect("declaration keyword") + keyword.len() + 2;
        json!({"line": line + 1, "column": column})
    }

    fn undeclared_cycle(mut self, variables: &[&str], claims: &[&str]) -> Self {
        let involved: Vec<String> = variables
            .iter()
            .map(|id| format!("variable:{id}"))
            .chain(claims.iter().map(|id| format!("claim:{id}")))
            .collect();
        self.warnings.push(json!({
            "kind": "causal_unacknowledged_feedback", "severity": "warning",
            "message": format!("cycle through [{}] contains claims not covered by any feedback declaration", variables.join(", ")),
            "loc": self.location("claim", claims[0]), "involved_nodes": involved
        }));
        self
    }

    fn output(&self, args: &[&str], exit: i32) -> Output {
        let output = Command::new(env!("CARGO_BIN_EXE_fslc"))
            .args(args)
            .current_dir(&self.root)
            .env("FSLC_CACHE", "off")
            .output()
            .expect("run native fslc");
        assert_eq!(
            output.status.code(),
            Some(exit),
            "{args:?}; stdout={} stderr={}",
            String::from_utf8_lossy(&output.stdout),
            String::from_utf8_lossy(&output.stderr)
        );
        assert!(output.stderr.is_empty(), "{args:?}: {:?}", output.stderr);
        output
    }

    fn run(&self, args: &[&str], exit: i32) -> Value {
        serde_json::from_slice(&self.output(args, exit).stdout).expect("single JSON stdout")
    }

    fn check(&self) -> Value {
        let value = self.run(&["check", "model.fsl"], 0);
        assert_eq!(
            value,
            self.run(&["causal", "check", "model.fsl"], 0),
            "full check envelopes, no exclusions"
        );
        assert_eq!(value["result"], "causal_model_checked");
        assert_eq!(value["schema_version"], "causal-check.v0");
        assert_eq!(value["formal_result"], "not_run");
        assert_eq!(
            value["warnings"],
            json!(self.warnings),
            "all check warnings"
        );
        value
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
        assert_eq!(value, self.analyze(consumer, 0), "same-binary determinism");
        value
    }

    fn overflow(&self, consumer: &str, claim: &str) {
        self.check();
        assert_eq!(
            self.analyze(consumer, 2),
            json!({
                "fsl": "1.0", "result": "error", "kind": "semantics",
                "diagnostic": "causal_invalid_model",
                "message": format!("causal time range overflow while adding claim '{claim}'"),
                "loc": self.location("claim", claim)
            })
        );
    }

    fn preserve(&self, name: &str) {
        let mut actual = json!({"check": self.check(), "consumers": {}});
        for consumer in [
            "causal_graph",
            "causal_timeline",
            "causal_traceability_graph",
            "causal-review",
        ] {
            actual["consumers"][consumer] = self.success(consumer);
        }
        assert_base_golden(name, &actual);
    }
}

fn header(name: &str, variables: &str) -> String {
    format!(
        "// SPDX-License-Identifier: Apache-2.0\ncausal {name} {{\n  uses biz from \"biz.fsl\"\n  timebase day\n  horizon {M}\n{variables}"
    )
}

const CHAIN_VARIABLES: &str = "  variable a { role intervention binds action biz.enable }\n  variable b { role mediator }\n  variable c { role mediator }\n  variable d { role outcome observes state biz.x }\n";

fn claim(id: &str, from: &str, to: &str, min: u64, max: u64) -> String {
    format!(
        "  claim {id} {from} -> {to} {{\n    version 1\n    status active\n    polarity positive\n    lag {min}..{max}\n    persists 0..0\n    basis hypothesis\n  }}\n"
    )
}

fn chain(last: u64) -> Model {
    let mut source = header("MaxChain", CHAIN_VARIABLES);
    for (id, from, to, max) in [
        ("AB", "a", "b", Q),
        ("BC", "b", "c", Q),
        ("CD", "c", "d", last),
    ] {
        source.push_str(&claim(id, from, to, 0, max));
    }
    source.push_str("}\n");
    Model::new(source)
}

fn cross_component(back_max: u64) -> Model {
    let mut source = header("MaxCrossComponent", CHAIN_VARIABLES);
    for (id, from, to, min, max) in [
        ("AB", "a", "b", 0, M),
        ("BC", "b", "c", 0, M),
        ("CB", "c", "b", 1, back_max),
        ("CD", "c", "d", 0, 0),
    ] {
        source.push_str(&claim(id, from, to, min, max));
    }
    source.push_str("}\n");
    Model::new(source).undeclared_cycle(&["b", "c"], &["BC", "CB"])
}

fn reachable_scc(last_max: u64) -> Model {
    let mut source = header("MaxReachableScc", CHAIN_VARIABLES);
    source.push_str("  variable e { role mediator }\n");
    // Declaration order differs from active_claims' ID order A_CE, B_EB, Z_BC.
    for (id, from, to, min, max) in [
        ("AB", "a", "b", 0, 0),
        ("Z_BC", "b", "c", 0, last_max),
        ("A_CE", "c", "e", 0, Q),
        ("B_EB", "e", "b", 1, Q),
        ("BD", "b", "d", 0, 0),
    ] {
        source.push_str(&claim(id, from, to, min, max));
    }
    source.push_str("}\n");
    Model::new(source).undeclared_cycle(&["b", "c", "e"], &["A_CE", "B_EB", "Z_BC"])
}

fn disconnected_scc(include_scc: bool) -> Model {
    disconnected_scc_with_bridge(include_scc, None)
}

fn disconnected_scc_with_bridge(include_scc: bool, bridge_lag: Option<&str>) -> Model {
    let mut source = header("MaxDisconnectedScc", CHAIN_VARIABLES);
    source.push_str(&claim("AB", "a", "b", 0, 1));
    source.push_str(&claim("BC", "b", "c", 0, 1));
    source.push_str(&claim("CD", "c", "d", 0, 1));
    if include_scc {
        source.push_str("  variable x { role mediator }\n  variable y { role mediator }\n  variable z { role mediator }\n");
        source.push_str(&claim("XY", "x", "y", 0, Q));
        source.push_str(&claim("YZ", "y", "z", 0, Q));
        source.push_str(&claim("ZY", "z", "x", 1, Q + 1));
        if let Some(lag) = bridge_lag {
            write!(source, "  claim AX a -> x {{\n    version 1\n    status active\n    polarity positive\n    lag {lag}\n    persists 0..0\n    basis hypothesis\n  }}\n").expect("write bridge claim");
        }
    }
    source.push_str("}\n");
    let model = Model::new(source);
    if include_scc {
        model.undeclared_cycle(&["x", "y", "z"], &["XY", "YZ", "ZY"])
    } else {
        model
    }
}

fn intervention_scc(last_max: u64) -> Model {
    let mut source = header("MaxInterventionScc", CHAIN_VARIABLES);
    // Declaration order CA, AB, BC differs from internal summation order AB, BC, CA.
    for (id, from, to, min, max) in [
        ("CA", "c", "a", 1, last_max),
        ("AB", "a", "b", 0, Q),
        ("BC", "b", "c", 0, Q),
        ("CD", "c", "d", 0, 0),
    ] {
        source.push_str(&claim(id, from, to, min, max));
    }
    source.push_str("}\n");
    Model::new(source).undeclared_cycle(&["a", "b", "c"], &["AB", "BC", "CA"])
}

fn feedback(last_max: u64) -> Model {
    let mut source = header(
        "MaxFeedback",
        "  variable a { role mediator }\n  variable b { role mediator }\n  variable c { role mediator }\n",
    );
    source.push_str(&claim("AB", "a", "b", 0, Q));
    source.push_str(&claim("BC", "b", "c", 0, last_max));
    source.push_str(&claim("CA", "c", "a", 1, Q));
    // The overflow witness follows this Vec, not the model's sorted IDs.
    source.push_str("  feedback F { claims CA, AB, BC }\n}\n");
    Model::new(source)
}

fn response_window() -> Model {
    let mut source = header(
        "MaxResponseWindow",
        "  variable a { role intervention binds action biz.enable }\n  variable d { role outcome observes state biz.x window 0..1 }\n",
    );
    write!(source, "  claim AD a -> d {{\n    version 1\n    status active\n    polarity positive\n    lag {M}..{M}\n    persists 0..{M}\n    basis hypothesis\n  }}\n}}\n").expect("write response claim");
    Model::new(source)
}

#[test]
fn independent_integer_boundaries() {
    assert_eq!(3 * u128::from(Q), 18_446_744_073_709_551_615);
    assert_eq!(3 * u128::from(Q) + 1, 18_446_744_073_709_551_616);
    assert_eq!(2 * u128::from(M) + 1, 18_446_744_073_709_551_615);
    assert_eq!(2 * u128::from(M) + 2, 18_446_744_073_709_551_616);
}

#[test]
fn s1_path_max_boundary_and_base_preservation() {
    let model = chain(Q);
    model.check();
    assert_eq!(
        model.success("causal_graph")["paths"][0]["first_response"],
        json!({"min": 0, "max": MAX})
    );
    model.preserve("max-chain");
}

#[test]
fn s1_path_max_overflow() {
    chain(Q + 1).overflow("causal_graph", "CD");
}

#[test]
fn s3a_edge_max_boundary() {
    let model = chain(Q);
    model.check();
    assert_eq!(
        model.success("causal_timeline")["timelines"][0],
        json!({
            "intervention": "variable:a", "outcome": "variable:d",
            "first_pass": {"min": 0, "max": MAX}, "via_feedback": false
        })
    );
}

#[test]
fn s3a_edge_max_overflow() {
    chain(Q + 1).overflow("causal_timeline", "CD");
}

#[test]
fn s3b_target_internal_max_boundary_and_base_preservation() {
    let model = cross_component(1);
    model.check();
    assert_eq!(
        model.success("causal_timeline")["timelines"][0],
        json!({
            "intervention": "variable:a", "outcome": "variable:d",
            "first_pass": {"min": 0, "max": MAX}, "via_feedback": true
        })
    );
    model.preserve("max-cross-component");
}

#[test]
fn s3b_target_internal_max_overflow_locates_crossing_edge() {
    cross_component(2).overflow("causal_timeline", "AB");
}

// S2 reachable/unreachable controls are adjacent: only explored SCC sums may refuse.
#[test]
fn s2_reachable_internal_max_boundary_and_base_preservation() {
    let model = reachable_scc(Q);
    model.check();
    assert_eq!(
        model.success("causal_timeline")["timelines"][0],
        json!({
            "intervention": "variable:a", "outcome": "variable:d",
            "first_pass": {"min": 0, "max": MAX}, "via_feedback": true
        })
    );
    model.preserve("max-reachable-scc");
}

#[test]
fn s2_reachable_internal_max_overflow_in_claim_id_order() {
    reachable_scc(Q + 1).overflow("causal_timeline", "Z_BC");
}

#[test]
fn s2_unreachable_internal_max_overflow_does_not_refuse_reachable_pair() {
    let plain = disconnected_scc(false);
    let disconnected = disconnected_scc(true);
    plain.check();
    disconnected.check();
    // Compare the complete reachable-pair collection, not the unrelated graph nodes.
    let expected = json!([{"intervention": "variable:a", "outcome": "variable:d",
        "first_pass": {"min": 0, "max": 3}, "via_feedback": false}]);
    assert_eq!(plain.success("causal_timeline")["timelines"], expected);
    assert_eq!(
        disconnected.success("causal_timeline")["timelines"],
        expected
    );
}

#[test]
fn s2_unknown_bridge_does_not_explore_overflowing_scc() {
    // XY + YZ + ZY overflows, but an unknown-lag bridge must not explore its SCC.
    assert_eq!(3 * u128::from(Q) + 1, 18_446_744_073_709_551_616);
    let plain = disconnected_scc(false);
    let bridged = disconnected_scc_with_bridge(true, Some("unknown"));
    plain.check();
    bridged.check();
    // Compare the complete reachable-pair collection, as in the disconnected control.
    let expected = json!([{"intervention": "variable:a", "outcome": "variable:d",
        "first_pass": {"min": 0, "max": 3}, "via_feedback": false}]);
    assert_eq!(plain.success("causal_timeline")["timelines"], expected);
    assert_eq!(bridged.success("causal_timeline")["timelines"], expected);
}

#[test]
fn s2_known_bridge_explores_overflowing_scc_in_claim_id_order() {
    // Only the bridge lag differs from the unknown case; XY + YZ + ZY overflows at ZY.
    assert_eq!(3 * u128::from(Q) + 1, 18_446_744_073_709_551_616);
    disconnected_scc_with_bridge(true, Some("0..0")).overflow("causal_timeline", "ZY");
}

#[test]
fn s2_intervention_inside_scc_internal_max_boundary() {
    assert_eq!(3 * u128::from(Q), 18_446_744_073_709_551_615);
    let model = intervention_scc(Q);
    model.check();
    assert_eq!(
        model.success("causal_timeline")["timelines"],
        json!([{
            "intervention": "variable:a", "outcome": "variable:d",
            "first_pass": {"min": 0, "max": MAX}, "via_feedback": true
        }])
    );
}

#[test]
fn s2_intervention_inside_scc_internal_max_overflow_in_claim_id_order() {
    // AB + BC + CA: the third ID overflows, even though CA is declared first.
    assert_eq!(3 * u128::from(Q) + 1, 18_446_744_073_709_551_616);
    intervention_scc(Q + 1).overflow("causal_timeline", "CA");
}

#[test]
fn s5_feedback_max_boundary_and_base_preservation() {
    let model = feedback(Q);
    model.check();
    let graph = model.success("causal_graph");
    assert_eq!(
        graph["feedbacks"][0]["cycle_lag"],
        json!({"min": 1, "max": MAX})
    );
    assert_eq!(
        graph["feedbacks"][0]["max_repetitions_within_horizon"],
        9_223_372_036_854_775_807u64
    );
    model.success("causal-review");
    model.preserve("max-feedback");
}

#[test]
fn s5_feedback_max_overflow_graph_in_declaration_order() {
    feedback(Q + 1).overflow("causal_graph", "BC");
}

#[test]
fn s5_feedback_max_overflow_review_in_declaration_order() {
    feedback(Q + 1).overflow("causal-review", "BC");
}

/// S4 cannot overflow: the lexer limits integers to i64 and intervals to horizon, so lag.max + persists.max <= 2^64-2.
#[test]
fn s4_response_end_exact_boundary_and_base_preservation() {
    let model = response_window();
    model.check();
    let review = model.success("causal-review");
    let findings: Vec<&Value> = review["findings"]
        .as_array()
        .expect("findings")
        .iter()
        .filter(|finding| finding["finding_type"] == "observation_window_misses_effect")
        .collect();
    assert_eq!(findings.len(), 1);
    assert_eq!(
        findings[0]["witness"]["response_window"],
        json!({
            "min": 9_223_372_036_854_775_807u64, "max": 18_446_744_073_709_551_614u64
        })
    );
    model.preserve("max-response-window");
}

#[test]
fn representable_max_dot_and_mermaid_succeed() {
    let model = chain(Q);
    model.check();
    for format in ["dot", "mermaid"] {
        let output = model.output(
            &[
                "causal",
                "analyze",
                "model.fsl",
                "--projection",
                "causal_graph",
                "--format",
                format,
            ],
            0,
        );
        assert!(!output.stdout.is_empty(), "nonempty {format} rendering");
    }
}
