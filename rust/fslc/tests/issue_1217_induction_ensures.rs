// SPDX-License-Identifier: Apache-2.0

//! Issue #1217: `--engine induction` must not report `proved` while an
//! action's reached `ensures` can be false on a step between states that
//! satisfy the proved invariants. BMC checks `ensures` per step; before this
//! fix the induction step case never asked it, so a violation beyond the
//! base depth was reported `proved`/`unbounded`.

use std::path::PathBuf;
use std::process::Command;
use std::time::{SystemTime, UNIX_EPOCH};

use serde_json::Value;

struct Fixture(PathBuf);

impl Fixture {
    fn new(name: &str, source: &str) -> Self {
        let nonce = SystemTime::now()
            .duration_since(UNIX_EPOCH)
            .expect("clock after epoch")
            .as_nanos();
        let path = std::env::temp_dir().join(format!(
            "fsl-issue-1217-{name}-{}-{nonce}.fsl",
            std::process::id()
        ));
        std::fs::write(&path, source).expect("write fixture");
        Self(path)
    }

    fn text(&self) -> &str {
        self.0.to_str().expect("UTF-8 temporary path")
    }
}

impl Drop for Fixture {
    fn drop(&mut self) {
        let _ = std::fs::remove_file(&self.0);
    }
}

fn verify(fixture: &Fixture, engine: &str, depth: usize) -> (Value, i32) {
    verify_with(fixture, engine, depth, &[])
}

fn verify_with(fixture: &Fixture, engine: &str, depth: usize, extra: &[&str]) -> (Value, i32) {
    let depth = depth.to_string();
    let output = Command::new(env!("CARGO_BIN_EXE_fslc"))
        .args([
            "verify",
            fixture.text(),
            "--engine",
            engine,
            "--depth",
            &depth,
            "--deadlock",
            "ignore",
            "--no-cache",
        ])
        .args(extra)
        .output()
        .expect("run native CLI");
    let value = serde_json::from_slice(&output.stdout).unwrap_or_else(|error| {
        panic!(
            "invalid JSON: {error}; stderr={}",
            String::from_utf8_lossy(&output.stderr)
        )
    });
    (value, output.status.code().expect("native exit status"))
}

const ENSURES_HOLE: &str = r"
spec EnsuresHole {
  state { x: 0..10 }
  init { x = 0 }
  action inc() { requires x < 10  x = x + 1  ensures x != 6 }
  invariant Range { 0 <= x and x <= 10 }
}
";

fn assert_ensures_cti(output: &Value, status: i32, label: &str) {
    assert_eq!(status, 1, "{label}: {output:#}");
    assert_eq!(output["result"], "unknown_cti", "{label}: {output:#}");
    assert_eq!(output["violation_kind"], "ensures", "{label}: {output:#}");
    assert_eq!(output["invariant"], "inc", "{label}: {output:#}");
    assert_eq!(output["last_action"]["name"], "inc", "{label}: {output:#}");
    assert_eq!(output["completeness"], "bounded", "{label}: {output:#}");
    let cti_states = output["cti"]["states"].as_array().expect("CTI states");
    assert_eq!(cti_states.len(), 2, "{label}: {output:#}");
    assert_eq!(
        cti_states[1]["action"]["name"], "inc",
        "{label}: {output:#}"
    );
}

#[test]
fn a_false_ensures_is_never_proved_at_any_depth_or_k() {
    for (label, source, deep) in [
        ("x != 6", ENSURES_HOLE.to_owned(), 6),
        ("x != 5", ENSURES_HOLE.replace("x != 6", "x != 5"), 5),
    ] {
        let fixture = Fixture::new("hole", &source);
        for depth in [0, 1, 2, 3, 8] {
            for k in ["1", "2", "3"] {
                let (output, status) = verify_with(&fixture, "induction", depth, &["--k", k]);
                let label = format!("{label} depth={depth} k={k}");
                assert_ne!(output["result"], "proved", "{label}: {output:#}");
                if depth < deep {
                    assert_ensures_cti(&output, status, &label);
                    let witness = &output["cti"]["states"][1]["state"]["x"];
                    assert_ne!(
                        output["cti"]["states"][1]["state"]["x"],
                        output["cti"]["states"][0]["state"]["x"],
                        "{label}: {witness}"
                    );
                } else {
                    // Within depth the base case owns the real trace.
                    assert_eq!(output["result"], "violated", "{label}: {output:#}");
                    assert_eq!(output["violation_kind"], "ensures", "{label}: {output:#}");
                }
            }
        }
        let (bmc, _) = verify(&fixture, "bmc", 8);
        assert_eq!(bmc["result"], "violated", "{label}: {bmc:#}");
        assert_eq!(bmc["violation_kind"], "ensures", "{label}: {bmc:#}");
    }
}

#[test]
fn a_true_ensures_stays_proved() {
    for (label, ensures) in [
        ("old", "ensures x == old(x) + 1"),
        ("range", "ensures x >= 1 and x <= 10"),
        ("conjunct", "ensures x > old(x)\n    ensures x != 0"),
    ] {
        let fixture = Fixture::new(label, &ENSURES_HOLE.replace("ensures x != 6", ensures));
        for depth in [1, 3, 8] {
            let (output, status) = verify(&fixture, "induction", depth);
            assert_eq!(output["result"], "proved", "{label} {depth}: {output:#}");
            assert_eq!(status, 0, "{label} {depth}: {output:#}");
        }
    }
}

/// `y == x` after `inc` follows only from the invariant `Sync` in the
/// pre-state: the obligation assumes the proved invariants, so this stays
/// `proved`; without `Sync` the same step is an `ensures` CTI.
#[test]
fn an_ensures_that_follows_from_an_invariant_stays_proved() {
    let source = r"
spec EnsuresFromInvariant {
  state { x: 0..10, y: 0..10 }
  init { x = 0  y = 0 }
  action inc() { requires x < 10 and y < 10  x = x + 1  y = y + 1  ensures y == x }
  invariant Sync { x == y }
}
";
    let fixture = Fixture::new("from-invariant", source);
    for depth in [1, 3, 8] {
        let (output, status) = verify(&fixture, "induction", depth);
        assert_eq!(output["result"], "proved", "{depth}: {output:#}");
        assert_eq!(status, 0, "{depth}: {output:#}");
    }
    let fixture = Fixture::new(
        "without-invariant",
        &source.replace("invariant Sync { x == y }", "invariant Any { x >= 0 }"),
    );
    let (output, status) = verify(&fixture, "induction", 2);
    assert_eq!(status, 1, "{output:#}");
    assert_eq!(output["result"], "unknown_cti", "{output:#}");
    assert_eq!(output["violation_kind"], "ensures", "{output:#}");
    assert_eq!(output["invariant"], "inc", "{output:#}");
}

/// #1196 review nit: a division guarded only by a `--lemma` is still checked
/// in the target run (adjudication proves only the lemma's truth). The
/// protecting lemma is used to exclude the `partial_op` CTI; an unrelated
/// lemma leaves it in place.
#[test]
fn a_division_guarded_only_by_a_lemma_is_checked_in_the_main_run() {
    let fixture = Fixture::new(
        "lemma-guard",
        r"
spec LemmaGuard {
  state { x: 0..10, d: 0..5 }
  init { x = 0  d = 1 }
  action step() { requires x < 10 and x / d < 100  x = x + 1 }
  invariant XRange { x >= 0 }
}
",
    );
    let (output, status) = verify(&fixture, "induction", 2);
    assert_eq!(status, 1, "{output:#}");
    assert_eq!(output["violation_kind"], "partial_op", "{output:#}");

    let (output, status) = verify_with(&fixture, "induction", 2, &["--lemma", "d >= 1"]);
    assert_eq!(output["result"], "proved", "{output:#}");
    assert_eq!(status, 0, "{output:#}");
    assert_eq!(output["lemmas"][0]["used"], true, "{output:#}");

    let (output, status) = verify_with(&fixture, "induction", 2, &["--lemma", "x <= 10"]);
    assert_eq!(status, 1, "{output:#}");
    assert_eq!(output["result"], "unknown_cti", "{output:#}");
    assert_eq!(output["violation_kind"], "partial_op", "{output:#}");
    assert_eq!(output["invariant"], "_partial_step", "{output:#}");
}
