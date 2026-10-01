// SPDX-License-Identifier: Apache-2.0

//! Issue #1196: `--engine induction` must not report `proved` for a spec whose
//! guard, body, or `ensures` reaches a partial operation in a state that
//! satisfies the proved invariants. BMC and the explicit engine fail such a
//! spec once the state is within depth; the step case evaluates with a
//! totalizing evaluator, so it needs its own definedness obligation.

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
            "fsl-issue-1196-{name}-{}-{nonce}.fsl",
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

/// The issue's `PartialGuard2`: `x / d` is reached in `dec`'s guard once
/// `zap` has set `d = 0` at `x == 3` (BMC step 4).
const PARTIAL_GUARD: &str = r"
spec PartialGuard2 {
  state { x: Int, d: Int }
  init { x = 5  d = 1 }
  action dec() {
    requires x > 0 and x / d < 100
    x = x - 1
  }
  action zap() {
    requires x == 3
    d = 0
  }
  invariant NonNeg { x >= 0 }
}
";

fn assert_partial_cti(output: &Value, status: i32, name: &str, action: &str) {
    assert_eq!(status, 1, "{output:#}");
    assert_eq!(output["result"], "unknown_cti", "{output:#}");
    assert_eq!(output["violation_kind"], "partial_op", "{output:#}");
    assert_eq!(output["invariant"], name, "{output:#}");
    assert_eq!(output["completeness"], "bounded", "{output:#}");
    assert_eq!(output["trace_type"], "induction_cti", "{output:#}");
    assert_eq!(output["last_action"]["name"], action, "{output:#}");
    let cti_states = output["cti"]["states"].as_array().expect("CTI states");
    assert_eq!(
        cti_states.last().expect("attempted step")["action"]["name"],
        action,
        "{output:#}"
    );
    assert!(
        output["hint"]
            .as_str()
            .expect("hint")
            .contains("partial_op"),
        "{output:#}"
    );
}

#[test]
fn partial_guard_is_never_proved_at_any_depth() {
    let fixture = Fixture::new("guard", PARTIAL_GUARD);
    for depth in [1, 2, 3] {
        let (output, status) = verify(&fixture, "induction", depth);
        assert_partial_cti(&output, status, "_partial_dec", "dec");
        // The witness is the undefined pre-state itself: d == 0, guard reached.
        assert_eq!(output["cti"]["states"][0]["state"]["d"], 0, "{output:#}");
        assert!(
            output["cti"]["states"][0]["state"]["x"]
                .as_i64()
                .expect("x")
                > 0
        );
    }
    // Within depth the base case owns the verdict (its `semantics` envelope
    // is #1191); it must still not be `proved`.
    let (output, status) = verify(&fixture, "induction", 8);
    assert_ne!(output["result"], "proved", "{output:#}");
    assert_ne!(status, 0, "{output:#}");
    let (bmc, _) = verify(&fixture, "bmc", 8);
    assert_ne!(bmc["result"], "verified", "{bmc:#}");
}

#[test]
fn short_circuit_protected_guards_stay_proved() {
    for (name, guard) in [
        ("and", "x > 0 and d != 0 and x / d < 100"),
        ("implies", "x > 0 and (d != 0 => x / d < 100) and d != 0"),
        ("or", "x > 0 and (d == 0 or x / d < 100) and d != 0"),
        ("if", "x > 0 and (if d != 0 then x / d < 100 else false)"),
    ] {
        let fixture = Fixture::new(name, &PARTIAL_GUARD.replace("x > 0 and x / d < 100", guard));
        for depth in [1, 2, 3, 8] {
            let (output, status) = verify(&fixture, "induction", depth);
            assert_eq!(output["result"], "proved", "{name} {depth}: {output:#}");
            assert_eq!(status, 0, "{name} {depth}: {output:#}");
        }
    }
}

#[test]
fn partial_body_and_ensures_are_obligations_too() {
    let body = Fixture::new(
        "body",
        r"
spec PartialBody {
  state { x: Int, d: Int, y: Int }
  init { x = 5  d = 1  y = 0 }
  action dec() {
    requires x > 0
    x = x - 1
    y = x / d
  }
  action zap() {
    requires x == 3
    d = 0
  }
  invariant NonNeg { x >= 0 }
}
",
    );
    let (output, status) = verify(&body, "induction", 2);
    assert_partial_cti(&output, status, "_partial_dec", "dec");

    let ensures = Fixture::new(
        "ensures",
        r"
spec PartialEnsures {
  state { x: Int, d: Int }
  init { x = 5  d = 1 }
  action dec() {
    requires x > 0
    x = x - 1
    ensures x < 100 / d
  }
  action zap() {
    requires x == 3
    d = 0
  }
  invariant NonNeg { x >= 0 }
}
",
    );
    let (output, status) = verify(&ensures, "induction", 2);
    assert_eq!(status, 1, "{output:#}");
    assert_eq!(output["result"], "unknown_cti", "{output:#}");
    assert_eq!(output["violation_kind"], "partial_op", "{output:#}");
    assert_eq!(output["invariant"], "_partial_dec", "{output:#}");
    assert_eq!(output["cti"]["states"][1]["state"]["d"], 0, "{output:#}");
}

/// An undefined guard state that is unreachable is a CTI like any other: the
/// verdict is `unknown_cti`, and an auxiliary invariant that excludes the
/// state restores `proved` (it is never silently `proved`).
#[test]
fn unreachable_partial_state_is_excluded_by_an_auxiliary_invariant() {
    let without = PARTIAL_GUARD.replace("requires x == 3\n    d = 0", "requires x == 3\n    d = 2");
    let fixture = Fixture::new("unreachable", &without);
    let (output, status) = verify(&fixture, "induction", 2);
    assert_partial_cti(&output, status, "_partial_dec", "dec");

    let fixture = Fixture::new(
        "strengthened",
        &without.replace(
            "invariant NonNeg { x >= 0 }",
            "invariant NonNeg { x >= 0 }\n  invariant DivisorNonZero { d != 0 }",
        ),
    );
    let (output, status) = verify(&fixture, "induction", 2);
    assert_eq!(output["result"], "proved", "{output:#}");
    assert_eq!(status, 0, "{output:#}");
}
