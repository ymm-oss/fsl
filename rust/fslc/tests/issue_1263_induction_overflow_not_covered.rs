// SPDX-License-Identifier: Apache-2.0

//! Issue #1263: a `proved` k-induction verdict does not cover i64 overflow.
//! The step reasons over unbounded integers and only the bounded base case
//! checks overflow, so a counter that overflows beyond `--depth` is `proved`
//! while BMC and explicit report `integer overflow` once it is within depth.
//! The verdict stays `proved` (decision 044); the envelope says so in a
//! machine-readable `not_covered: ["int_overflow"]`, present exactly when a
//! proof site holds an operator that can overflow.

use std::path::PathBuf;
use std::process::Command;
use std::time::{SystemTime, UNIX_EPOCH};

use serde_json::{Value, json};

struct Fixture(PathBuf);

impl Fixture {
    fn new(name: &str, source: &str) -> Self {
        let nonce = SystemTime::now()
            .duration_since(UNIX_EPOCH)
            .expect("clock after epoch")
            .as_nanos();
        let path = std::env::temp_dir().join(format!(
            "fsl-issue-1263-{name}-{}-{nonce}.fsl",
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

fn assert_proved(output: &Value, status: i32) {
    assert_eq!(status, 0, "{output:#}");
    assert_eq!(output["result"], "proved", "{output:#}");
    assert_eq!(output["completeness"], "unbounded", "{output:#}");
}

/// The issue's reproducer: `x` overflows i64 at step 32.
const ADV_LATE: &str = r"
spec AdvLate {
  state { x: Int }
  init { x = 0 }
  action inc() { x = x + 288230376151711744 }
  invariant Ok { x >= 0 }
}
";

#[test]
fn proved_with_unchecked_overflow_says_so() {
    let fixture = Fixture::new("adv-late", ADV_LATE);
    let (output, status) = verify(&fixture, "induction", 8);
    assert_proved(&output, status);
    assert_eq!(output["not_covered"], json!(["int_overflow"]), "{output:#}");

    // The verdicts themselves are unchanged: within depth, every engine
    // reports the overflow.
    for engine in ["induction", "bmc", "explicit"] {
        let (output, status) = verify(&fixture, engine, 32);
        assert_eq!(status, 2, "{engine}: {output:#}");
        assert_eq!(output["result"], "error", "{engine}: {output:#}");
        assert_eq!(
            output["message"], "integer overflow in addition",
            "{engine}: {output:#}"
        );
    }
}

#[test]
fn overflow_in_a_property_or_guard_is_not_covered_either() {
    // No arithmetic in any body: the operators sit in a guard and an
    // invariant, which the proof covers as well.
    let fixture = Fixture::new(
        "property",
        r"
spec Flags {
  state { x: Int, on: Bool }
  init { x = 3  on = false }
  action flip() { requires x * 2 > 0  on = not on }
  invariant Pos { x - 1 >= 0 }
}
",
    );
    let (output, status) = verify(&fixture, "induction", 2);
    assert_proved(&output, status);
    assert_eq!(output["not_covered"], json!(["int_overflow"]), "{output:#}");
}

#[test]
fn proof_without_overflowing_operators_has_no_not_covered() {
    // No `+ - * / %`, unary `-`, `abs` or `sum` at any proof site; the
    // only arithmetic is in `init`, which the base case checks.
    let fixture = Fixture::new(
        "no-arithmetic",
        r"
spec Toggle {
  enum Mode { Idle, Busy }
  state { mode: Mode, count: Int }
  init { mode = Idle  count = 1 + 1 }
  action start() { requires mode == Idle  mode = Busy }
  action stop() { requires mode == Busy  mode = Idle  count = 7 }
  invariant CountPos { count > 0 }
}
",
    );
    let (output, status) = verify(&fixture, "induction", 2);
    assert_proved(&output, status);
    assert!(output.get("not_covered").is_none(), "{output:#}");
}

#[test]
fn the_field_is_syntactic_and_induction_only() {
    // `x + 1` on a range can never overflow, but the step does not ask it,
    // so induction still lists it. The explicit engine evaluates every
    // reachable state with overflow checked, so its `proved` has nothing
    // left uncovered.
    let fixture = Fixture::new(
        "bounded",
        r"
spec BoundedCounter {
  state { x: 0..3 }
  init { x = 0 }
  action inc() { requires x < 3  x = x + 1 }
  invariant Range { x <= 3 }
}
",
    );
    let (output, status) = verify(&fixture, "induction", 2);
    assert_proved(&output, status);
    assert_eq!(output["not_covered"], json!(["int_overflow"]), "{output:#}");

    let (output, status) = verify(&fixture, "explicit", 8);
    assert_proved(&output, status);
    assert!(output.get("not_covered").is_none(), "{output:#}");
}
