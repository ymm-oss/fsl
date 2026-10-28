// SPDX-License-Identifier: Apache-2.0

//! #1542: a reached undefined decreases measure is not a rank value.
//! Refusal expectations follow the approved pending/path-sensitive contract;
//! BMC/explicit verdicts and non-partial solver counts preserve c79016ef.

use std::path::PathBuf;
use std::process::Command;
use std::time::{SystemTime, UNIX_EPOCH};

use serde_json::{Value, json};

struct Fixture(PathBuf);

impl Fixture {
    fn new(cell: &str, source: &str) -> Self {
        use std::sync::atomic::{AtomicUsize, Ordering};
        static FIXTURE_SEQUENCE: AtomicUsize = AtomicUsize::new(0);
        let nonce = SystemTime::now()
            .duration_since(UNIX_EPOCH)
            .expect("clock after epoch")
            .as_nanos();
        let path = std::env::temp_dir().join(format!(
            "fsl-issue-1542-{cell}-{}-{nonce}-{}.fsl",
            std::process::id(),
            FIXTURE_SEQUENCE.fetch_add(1, Ordering::Relaxed),
        ));
        std::fs::write(&path, source).expect("write fixture");
        Self(path)
    }

    fn run(&self, engine: &str, extra: &[&str]) -> (Value, i32) {
        let mut command = Command::new(env!("CARGO_BIN_EXE_fslc"));
        command.env_remove("FSLC_CACHE");
        if engine == "check" {
            command.arg("check").arg(&self.0);
        } else {
            command
                .arg("verify")
                .arg(&self.0)
                .args([
                    "--engine",
                    engine,
                    "--depth",
                    "4",
                    "--no-cache",
                    "--deadlock",
                    "ignore",
                ])
                .args(extra);
        }
        let output = command.output().expect("run working-tree CLI");
        let value = serde_json::from_slice(&output.stdout).unwrap_or_else(|error| {
            panic!(
                "invalid JSON: {error}; stdout={}; stderr={}",
                String::from_utf8_lossy(&output.stdout),
                String::from_utf8_lossy(&output.stderr)
            )
        });
        (value, output.status.code().expect("native exit status"))
    }
}

impl Drop for Fixture {
    fn drop(&mut self) {
        let _ = std::fs::remove_file(&self.0);
    }
}

fn rank_source(capacity: usize, measure: &str, empty: bool) -> String {
    let invariant = if empty {
        "invariant Empty { xs.size() == 0 }"
    } else {
        ""
    };
    format!(
        "spec RankRead {{
  type K = 0..1
  state {{ xs: Seq<K, {capacity}>, n: 0..2 }}
  init {{ xs = Seq {{}} n = 0 }}
  action tick() {{ requires n < 2 n = n + 1 }}
  action idle() {{ requires n == 2 }}
  {invariant}
  leadsTo L {{ n < 2 ~> n == 2 decreases {measure} }}
}}"
    )
}

// ND excludes done in rank0, not rank1: only take can reach the read.
// k=2 proves ND before the rank proof. No Empty invariant may mask this site.
const S1: &str = "spec S1 {
  type K = 0..1
  state { xs: Seq<K, 1>, n: 0..2, go: Bool, done: Bool }
  init { xs = Seq {} n = 0 go = false done = false }
  action tick() { requires n < 2 and not go n = n + 1 }
  action idle() { requires n == 2 and not go }
  action take() { requires go done = true go = false }
  invariant ND { not done }
  leadsTo L { n < 2 ~> n == 2 decreases if done then xs.head() else 2 - n }
}";

// Enabled transitions imply not dead at both ends. Only rank_deadlock can
// choose dead=true; init and reachable states never do.
const S2: &str = "spec D1 {
  type K = 0..1
  state { xs: Seq<K, 1>, n: 0..2, dead: Bool }
  init { xs = Seq {} n = 0 dead = false }
  action tick() { requires n < 2 and not dead n = n + 1 }
  action idle() { requires n == 2 and not dead }
  leadsTo L { n < 2 ~> n == 2 decreases if dead then xs.head() else 2 - n }
}";

// Only the version/build identity is ambient in error envelopes. Exact equality
// of every remaining key rejects rank_failure, measure_value, CTI, witnesses,
// proof, or any future carrier of an invented hidden-slot value.
fn without_versions(mut output: Value) -> Value {
    let versions = output
        .as_object_mut()
        .expect("CLI object")
        .remove("versions")
        .expect("live versions metadata");
    assert!(versions.is_object(), "versions: {versions}");
    output
}

fn semantics_error(output: Value, exit: i32, message: &str, loc: Option<Value>) {
    let mut expected = json!({
        "fsl": "1.0", "result": "error", "kind": "semantics", "message": message,
    });
    if let Some(loc) = loc {
        expected["loc"] = loc;
    }
    // Compare the envelope even if exit is wrong, so fail-first shows the
    // actual bogus rank value/proof rather than only a numeric exit mismatch.
    assert_eq!(
        without_versions(output),
        expected,
        "no rank evidence may escape"
    );
    assert_eq!(exit, 2, "semantics errors must exit 2");
}

// This is a verdict contract, not a full-envelope equality claim across engines.
// The full output remains in failures. Cost/proof/coverage differ by engine;
// L2 and N1/N2 below separately own the relevant cost observables.
fn success(output: &Value, exit: i32, result: &str) {
    assert_eq!(
        json!({"exit": exit, "result": output["result"],
            "kind": output["kind"], "message": output["message"],
            "violation_kind": output["violation_kind"],
            "rank_failure": output["rank_failure"]}),
        json!({"exit": 0, "result": result, "kind": null, "message": null,
            "violation_kind": null, "rank_failure": null}),
        "verdict: {output:#}"
    );
}

fn ranking(output: &Value, name: &str) {
    assert_eq!(output["leads_to"][name]["proved"], true, "{output:#}");
    assert_eq!(output["leads_to"][name]["proof"], "ranking", "{output:#}");
    assert_eq!(
        output["leads_to"][name]["completeness"], "unbounded",
        "{output:#}"
    );
}

fn cell(cell: &str, source: &str, engine: &str, refusal: Option<&str>, extra: &[&str]) {
    let fixture = Fixture::new(cell, source);
    // Preservation controls run twice to expose cache/environment-dependent
    // expectations. A refusal detector needs one observation on unfixed base.
    let runs = if engine == "induction" && refusal.is_some() {
        1
    } else {
        2
    };
    for _ in 0..runs {
        let (output, exit) = fixture.run(engine, if engine == "induction" { extra } else { &[] });
        eprintln!("{cell}/{engine}: exit={exit}, envelope={output}");
        match engine {
            "explicit" => semantics_error(
                output,
                exit,
                "the explicit engine does not support leadsTo properties; use --engine bmc or exclude the leadsTo property",
                None,
            ),
            "induction" => {
                if let Some(message) = refusal {
                    semantics_error(output, exit, message, None);
                } else {
                    success(&output, exit, "proved");
                    ranking(&output, "L");
                }
            }
            "check" => success(&output, exit, "ok"),
            "bmc" => success(&output, exit, "verified"),
            _ => panic!("unexpected engine"),
        }
    }
}

macro_rules! measure_cell {
    ($cell:ident, $source:expr, $refusal:expr, $extra:expr) => {
        mod $cell {
            use super::*;
            #[test]
            fn check() {
                cell(stringify!($cell), &$source, "check", $refusal, &[]);
            }
            #[test]
            fn bmc() {
                cell(stringify!($cell), &$source, "bmc", $refusal, &[]);
            }
            #[test]
            fn induction() {
                cell(stringify!($cell), &$source, "induction", $refusal, $extra);
            }
            #[test]
            fn explicit() {
                cell(stringify!($cell), &$source, "explicit", $refusal, &[]);
            }
        }
    };
}

measure_cell!(
    m1_direct_head,
    rank_source(1, "xs.head()", false),
    Some("leadsTo 'L' decreases measure may be undefined: head()"),
    &[]
);
// Arithmetic masking does not short-circuit the read.
measure_cell!(
    m2_masked_head,
    rank_source(1, "2 - n + 0 * xs.head()", false),
    Some("leadsTo 'L' decreases measure may be undefined: head()"),
    &[]
);
measure_cell!(
    m3_direct_at,
    rank_source(1, "xs.at(0)", false),
    Some("leadsTo 'L' decreases measure may be undefined: at()"),
    &[]
);
measure_cell!(
    m4_direct_index,
    rank_source(1, "xs[0]", false),
    Some("leadsTo 'L' decreases measure may be undefined: index"),
    &[]
);
measure_cell!(
    m5_masked_at,
    rank_source(1, "2 - n + 0 * xs.at(0)", false),
    Some("leadsTo 'L' decreases measure may be undefined: at()"),
    &[]
);
// Capacity zero, but these temporary/merged sequences bypass the narrow
// zero-slot preflight. They require the new message, unlike unchanged DM1–DM10.
measure_cell!(
    p09_temporary_head,
    rank_source(0, "xs.push(0).pop().head()", false),
    Some("leadsTo 'L' decreases measure may be undefined: head()"),
    &[]
);
measure_cell!(
    p10b_merged_head,
    rank_source(0, "(if n > 0 then xs else xs.push(1)).head()", false),
    Some("leadsTo 'L' decreases measure may be undefined: head()"),
    &[]
);
measure_cell!(
    g1_guarded_if,
    rank_source(1, "if xs.size() > 0 then xs.head() else 2 - n", true),
    None,
    &[]
);
measure_cell!(
    g2_zero_capacity_if,
    rank_source(0, "if xs.size() > 0 then xs.head() else 2 - n", false),
    None,
    &[]
);
measure_cell!(
    g3_guarded_and,
    rank_source(
        1,
        "if xs.size() > 0 and xs.head() > 0 then 0 else 2 - n",
        true
    ),
    None,
    &[]
);
measure_cell!(
    g4_guarded_or,
    rank_source(
        1,
        "if xs.size() == 0 or xs.head() > 0 then 2 - n else 0",
        true
    ),
    None,
    &[]
);
measure_cell!(
    s1_successor_only,
    S1,
    Some("leadsTo 'L' decreases measure may be undefined: head()"),
    &["--k", "2"]
);
measure_cell!(
    s2_deadlock_only,
    S2,
    Some("leadsTo 'L' decreases measure may be undefined: head()"),
    &[]
);
// n is nonnegative: this reaches the then branch, not the else branch.
measure_cell!(
    r1_reached_then,
    rank_source(1, "if n >= 0 then xs.head() else 0", false),
    Some("leadsTo 'L' decreases measure may be undefined: head()"),
    &[]
);
measure_cell!(n1_total_measure, rank_source(1, "2 - n", false), None, &[]);

// F1: the successor resolves Q, so the rank query never reads its empty head.
const E1: &str = "spec Drain {
  type K = 0..1
  state { xs: Seq<K, 1> }
  init { xs = Seq { 1 } }
  action take() { requires xs.size() > 0  xs = xs.pop() }
  action idle() { requires xs.size() == 0 }
  leadsTo L { xs.size() > 0 ~> xs.size() == 0 decreases xs.head() + 1 }
}";

const E1B: &str = "spec Drain {
  type K = 0..1
  state { xs: Seq<K, 1> }
  init { xs = Seq { 1 } }
  action take() { requires xs.size() > 0  xs = xs.pop() }
  action idle() { requires xs.size() == 0 }
  leadsTo L { xs.size() > 0 ~> xs.size() == 0 decreases if xs.size() > 0 then xs.head() + 1 else 0 }
}";

// F3: unlike the Empty controls, this head is reached AND defined.
const E11: &str = "spec RankRead {
  type K = 0..1
  state { xs: Seq<K, 1>, n: 0..2 }
  init { xs = Seq { 0 } n = 0 }
  action tick() { requires n < 2 n = n + 1 }
  action idle() { requires n == 2 }
  invariant Full { xs.size() == 1 }
  leadsTo L { n < 2 ~> n == 2 decreases 2 - n + 0 * xs.head() }
}";

// F4/R2: syntactically earlier head is guarded; only at can fail first.
const E14: &str = "spec RankRead {
  type K = 0..1
  state { xs: Seq<K, 2>, n: 0..2 }
  init { xs = Seq {} n = 0 }
  action tick() { requires n < 2 n = n + 1 }
  action idle() { requires n == 2 }
  leadsTo L { n < 2 ~> n == 2 decreases 2 - n + (if xs.size() > 0 then 0 * xs.head() else 0) + 0 * xs.at(1) }
}";

const E4B: &str = "spec RankRead {
  type K = 0..1
  state { xs: Seq<K, 2>, n: 0..2 }
  init { xs = Seq {} n = 0 }
  action tick() { requires n < 2 n = n + 1 }
  action idle() { requires n == 2 }

  leadsTo L { n < 2 ~> n == 2 decreases 2 - n + 0 * xs.head() + 0 * xs.at(1) }
}";

// R1: a real base-case lasso takes precedence over optional rank diagnosis.
const BV: &str = "spec BaseViolation {
  type K = 0..1
  state { xs: Seq<K, 1>, n: 0..2 }
  init { xs = Seq {} n = 0 }
  action idle() { }
  leadsTo L { n < 2 ~> n == 2 decreases 2 - n + 0 * xs.head() }
}";

measure_cell!(e1_drain, E1, None, &[]);
measure_cell!(e1b_drain_guarded, E1B, None, &[]);
measure_cell!(e11_inv_defined, E11, None, &[]);
measure_cell!(
    e14_attrib,
    E14,
    Some("leadsTo 'L' decreases measure may be undefined: at()"),
    &[]
);
measure_cell!(
    e4b_head_then_at,
    E4B,
    Some("leadsTo 'L' decreases measure may be undefined: head()"),
    &[]
);

fn no_rank_evidence(output: &Value) {
    match output {
        Value::Object(entries) => {
            for (key, value) in entries {
                assert!(
                    !matches!(
                        key.as_str(),
                        "rank_failure" | "measure_value" | "measure_before" | "measure_after"
                    ),
                    "undefined rank evidence {key}: {output:#}"
                );
                no_rank_evidence(value);
            }
        }
        Value::Array(entries) => entries.iter().for_each(no_rank_evidence),
        _ => {}
    }
}

fn base_violation(output: &Value, exit: i32) {
    assert_eq!(exit, 1, "{output:#}");
    assert_eq!(output["result"], "violated", "{output:#}");
    assert_eq!(output["violation_kind"], "leadsTo", "{output:#}");
    assert_eq!(output["trace_type"], "leadsTo", "{output:#}");
    assert_eq!(output["invariant"], "L", "{output:#}");
    assert_eq!(output["pending_since"], 0, "{output:#}");
    assert_eq!(output["loop_start"], 0, "{output:#}");
    assert_eq!(output["stutter"], false, "{output:#}");
    assert_eq!(
        output["trace"],
        json!([
            {"step": 0, "state": {"n": 0, "xs": []}},
            {"step": 1, "state": {"n": 0, "xs": []},
             "action": {"name": "idle", "params": {}, "loc": {"line": 5, "column": 3}},
             "changes": {}}
        ]),
        "real idle lasso, not a rank CTI: {output:#}"
    );
    no_rank_evidence(output);
}

fn without_run_timings(output: Value) -> Value {
    // Two live measurements differed at cost.elapsed_s, solver.check_elapsed_s,
    // and each property's elapsed_s: wall-clock timings cannot agree across
    // processes. Keep all other cost fields and the entire verdict/trace.
    fn remove_timing(object: &mut Value, key: &str) {
        let timing = object
            .as_object_mut()
            .expect("cost object")
            .remove(key)
            .expect("live timing field");
        assert!(timing.as_f64().is_some_and(|value| value >= 0.0));
    }
    let mut output = without_versions(output);
    remove_timing(&mut output["cost"], "elapsed_s");
    remove_timing(&mut output["cost"]["solver"], "check_elapsed_s");
    let properties = output["cost"]["properties"]
        .as_array_mut()
        .expect("live property costs");
    assert!(!properties.is_empty());
    for property in properties {
        remove_timing(property, "elapsed_s");
    }
    output
}

mod bv_base_violation {
    use super::*;

    #[test]
    fn check() {
        cell("bv", BV, "check", None, &[]);
    }

    #[test]
    fn explicit() {
        cell("bv", BV, "explicit", None, &[]);
    }

    #[test]
    fn bmc() {
        let fixture = Fixture::new("bv", BV);
        for _ in 0..2 {
            let (output, exit) = fixture.run("bmc", &[]);
            eprintln!("BV/bmc: exit={exit}, envelope={output}");
            base_violation(&output, exit);
        }
    }

    #[test]
    fn induction() {
        let fixture = Fixture::new("bv", BV);
        for _ in 0..2 {
            let (bmc, bmc_exit) = fixture.run("bmc", &[]);
            let (induction, induction_exit) = fixture.run("induction", &[]);
            eprintln!("BV/bmc: exit={bmc_exit}, envelope={bmc}");
            eprintln!("BV/induction: exit={induction_exit}, envelope={induction}");
            base_violation(&bmc, bmc_exit);
            base_violation(&induction, induction_exit);
            assert_eq!(
                without_run_timings(induction),
                without_run_timings(bmc),
                "induction must retain the complete base-BMC counterexample"
            );
        }
    }
}

fn property_checks(output: &Value, kind: &str, name: &str) -> u64 {
    let entries = output["cost"]["properties"]
        .as_array()
        .expect("live property costs");
    let matching = entries
        .iter()
        .filter(|entry| entry["kind"] == kind && entry["name"] == name)
        .collect::<Vec<_>>();
    assert_eq!(
        matching.len(),
        1,
        "one {kind}/{name} cost entry: {output:#}"
    );
    matching[0]["checks"].as_u64().expect("numeric check count")
}

#[test]
fn n1_no_probe_for_total_measure() {
    let fixture = Fixture::new("n1_count", &rank_source(1, "2 - n", false));
    for _ in 0..2 {
        let (output, exit) = fixture.run("induction", &[]);
        success(&output, exit, "proved");
        ranking(&output, "L");
        assert_eq!(output["cost"]["solver"]["checks"], 78, "{output:#}");
        assert_eq!(
            property_checks(&output, "leadsTo_rank", "L"),
            8,
            "{output:#}"
        );
    }
}

#[test]
fn n2_existing_ranked_contract_check_count() {
    // Same source/options as induction_cli_contract, which must not be edited.
    let fixture = Fixture::new(
        "n2_count",
        include_str!("../../../tests/fixtures/rust_port/ranked_leadsto.fsl"),
    );
    for _ in 0..2 {
        let (output, exit) = fixture.run("induction", &["--depth", "5", "--k", "1"]);
        success(&output, exit, "proved");
        ranking(&output, "ReachFive");
        assert_eq!(output["cost"]["solver"]["checks"], 79, "{output:#}");
        assert_eq!(
            property_checks(&output, "leadsTo_rank", "ReachFive"),
            6,
            "{output:#}"
        );
    }
}

#[test]
fn l2_undefined_measures_both_search_lassos() {
    let m1 = Fixture::new("l2_m1", &rank_source(1, "xs.head()", false));
    let m2 = Fixture::new("l2_m2", &rank_source(1, "2 - n + 0 * xs.head()", false));
    for _ in 0..2 {
        let (direct, direct_exit) = m1.run("bmc", &[]);
        let (masked, masked_exit) = m2.run("bmc", &[]);
        success(&direct, direct_exit, "verified");
        success(&masked, masked_exit, "verified");
        let direct_checks = property_checks(&direct, "leadsTo", "L");
        let masked_checks = property_checks(&masked, "leadsTo", "L");
        assert!(direct_checks > 0, "live lasso-search reference: {direct:#}");
        assert_eq!(
            masked_checks, direct_checks,
            "both undefined measures must retain lasso probes; M1={direct:#}; M2={masked:#}"
        );
    }
}

#[test]
fn l2_total_measure_still_discharges() {
    lasso_control("l2_n1", "2 - n", false);
}

#[test]
fn l2_guarded_measure_still_discharges() {
    lasso_control("l2_g1", "if xs.size() > 0 then xs.head() else 2 - n", true);
}

fn lasso_control(cell: &str, measure: &str, empty: bool) {
    let fixture = Fixture::new(cell, &rank_source(1, measure, empty));
    for _ in 0..2 {
        let (output, exit) = fixture.run("bmc", &[]);
        success(&output, exit, "verified");
        // c79016ef: 15 without lasso probes (versus M1's 45 with probes).
        assert_eq!(property_checks(&output, "leadsTo", "L"), 15, "{output:#}");
    }
}

fn corpus(source: &str, engine: &str, typo: bool) {
    let fixture = Fixture::new("corpus", source);
    for _ in 0..2 {
        let (output, exit) = fixture.run(engine, &[]);
        if typo {
            semantics_error(
                output,
                exit,
                "invalid model expression: leadsTo 'Responds' helpful action 'nosuch' is not declared at 13:5",
                Some(json!({"line": 13, "column": 5})),
            );
        } else if engine == "induction" {
            success(&output, exit, "proved");
            ranking(&output, "Responds");
            assert_eq!(
                output["leads_to"]["Responds"]["decreases"], "level[c]",
                "{output:#}"
            );
        } else {
            success(&output, exit, "verified");
        }
    }
}

#[test]
fn corpus_helpful_map_index_induction() {
    corpus(
        include_str!("fixtures/issue_473_helpful_leadsto.fsl"),
        "induction",
        false,
    );
}
#[test]
fn corpus_helpful_map_index_bmc() {
    corpus(
        include_str!("fixtures/issue_473_helpful_leadsto.fsl"),
        "bmc",
        false,
    );
}
#[test]
fn corpus_helpful_typo_induction() {
    corpus(
        include_str!("fixtures/issue_473_helpful_typo.fsl"),
        "induction",
        true,
    );
}
#[test]
fn corpus_helpful_typo_bmc() {
    corpus(
        include_str!("fixtures/issue_473_helpful_typo.fsl"),
        "bmc",
        true,
    );
}
