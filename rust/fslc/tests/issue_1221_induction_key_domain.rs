// SPDX-License-Identifier: Apache-2.0

//! Issue #1221: `--engine induction` must not report `proved` for a spec that
//! indexes a finite-key `Map` outside its key domain in a state that
//! satisfies the proved invariants. BMC and the explicit engine stop such a
//! spec with an error once the state is within depth; the step case
//! evaluates with a totalizing evaluator, and the #1196 definedness
//! obligation asks only the six partial operations, so before #1221 the miss
//! was `proved` whenever it lay beyond the base depth.

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
            "fsl-issue-1221-{name}-{}-{nonce}.fsl",
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

/// The issue's reproducer, on a smaller key domain: `peek_next` reads
/// `m[cur + 1]`, which leaves `0..5` once `advance` has run five times
/// (BMC step 5).
const NEXT_SLOT: &str = r"
spec MapNextSlot {
  type K = 0..5
  state { m: Map<K, Int>, cur: K, y: Int }
  init { forall k: K { m[k] = 0 }  cur = 0  y = 0 }
  action advance() { requires cur < 5  cur = cur + 1 }
  action peek_next() { y = m[cur + 1] }
  invariant MapNonNeg { forall k: K { m[k] >= 0 } }
  invariant YNonNeg { y >= 0 }
}
";

const READ_ERROR: &str = "map index outside finite key domain";
const WRITE_ERROR: &str = "map assignment index outside key domain";

fn assert_key_domain_cti(output: &Value, status: i32, name: &str, action: Option<&str>) {
    assert_eq!(status, 1, "{output:#}");
    assert_eq!(output["result"], "unknown_cti", "{output:#}");
    assert_eq!(output["violation_kind"], "key_domain", "{output:#}");
    assert_eq!(output["invariant"], name, "{output:#}");
    assert_eq!(output["completeness"], "bounded", "{output:#}");
    assert_eq!(output["trace_type"], "induction_cti", "{output:#}");
    match action {
        Some(action) => assert_eq!(output["last_action"]["name"], action, "{output:#}"),
        None => assert!(output.get("last_action").is_none(), "{output:#}"),
    }
}

/// A bounded engine found nothing: `verified`, or `proved` when the explicit
/// engine exhausted the state space within depth.
fn assert_passes(output: &Value, status: i32) {
    assert_eq!(status, 0, "{output:#}");
    assert!(
        matches!(output["result"].as_str(), Some("verified" | "proved")),
        "{output:#}"
    );
}

fn assert_error(output: &Value, status: i32, message: &str) {
    assert_eq!(status, 2, "{output:#}");
    assert_eq!(output["result"], "error", "{output:#}");
    assert_eq!(output["message"], message, "{output:#}");
}

#[test]
fn map_read_past_the_key_domain_is_never_proved_at_any_depth() {
    let fixture = Fixture::new("next-slot", NEXT_SLOT);
    for depth in [1, 3, 5] {
        let (output, status) = verify(&fixture, "induction", depth);
        assert_key_domain_cti(&output, status, "_key_domain_peek_next", Some("peek_next"));
        // The CTI is the reachable state the bounded engines fail on.
        let cti_states = output["cti"]["states"].as_array().expect("CTI states");
        assert_eq!(cti_states[0]["state"]["cur"], 5, "{output:#}");
        assert_eq!(cti_states[1]["action"]["name"], "peek_next", "{output:#}");
    }
    // Once the miss is within depth, every engine reports the same error.
    for engine in ["induction", "bmc", "explicit"] {
        let (output, status) = verify(&fixture, engine, 8);
        assert_error(&output, status, READ_ERROR);
    }
}

#[test]
fn in_domain_map_reads_stay_proved() {
    // Same spec with `peek_next` guarded; the counter's unbounded `x + 1` is
    // there to show the obligation does not ask i64 overflow (#1263).
    let fixture = Fixture::new(
        "guarded",
        r"
spec MapNextSlotGuarded {
  type K = 0..5
  state { m: Map<K, Int>, cur: K, y: Int, x: Int }
  init { forall k: K { m[k] = 0 }  cur = 0  y = 0  x = 0 }
  action advance() { requires cur < 5  cur = cur + 1  x = x + 1 }
  action peek_next() { requires cur < 5  y = m[cur + 1] }
  action peek_here() { y = m[cur] }
  invariant MapNonNeg { forall k: K { m[k] >= 0 } }
  invariant YNonNeg { y >= 0 }
  invariant XNonNeg { x >= 0 }
}
",
    );
    let (output, status) = verify(&fixture, "induction", 2);
    assert_eq!(status, 0, "{output:#}");
    assert_eq!(output["result"], "proved", "{output:#}");
    for engine in ["bmc", "explicit"] {
        let (output, status) = verify(&fixture, engine, 8);
        assert_passes(&output, status);
    }
}

#[test]
fn map_assignment_past_the_key_domain_is_an_obligation() {
    let fixture = Fixture::new(
        "write",
        r"
spec MapMarkNext {
  type K = 0..5
  state { m: Map<K, Int>, cur: K }
  init { forall k: K { m[k] = 0 }  cur = 0 }
  action advance() { requires cur < 5  cur = cur + 1 }
  action mark_next() { m[cur + 1] = 1 }
  invariant MapNonNeg { forall k: K { m[k] >= 0 } }
}
",
    );
    let (output, status) = verify(&fixture, "induction", 2);
    assert_key_domain_cti(&output, status, "_key_domain_mark_next", Some("mark_next"));
    for engine in ["induction", "bmc", "explicit"] {
        let (output, status) = verify(&fixture, engine, 8);
        assert_error(&output, status, WRITE_ERROR);
    }
}

#[test]
fn property_reads_past_the_key_domain_are_obligations() {
    // `or true` keeps the invariant's value true; only its evaluation fails.
    // The short-circuit guarded form stays proved.
    let unguarded = Fixture::new(
        "property",
        r"
spec MapPropertyRead {
  type K = 0..5
  state { m: Map<K, Int>, cur: K }
  init { forall k: K { m[k] = 0 }  cur = 0 }
  action advance() { requires cur < 5  cur = cur + 1 }
  invariant MapNonNeg { forall k: K { m[k] >= 0 } }
  invariant ReadOk { m[cur + 1] >= 0 or true }
}
",
    );
    let (output, status) = verify(&unguarded, "induction", 2);
    assert_key_domain_cti(&output, status, "_key_domain_property_ReadOk", None);
    for engine in ["induction", "bmc", "explicit"] {
        let (output, status) = verify(&unguarded, engine, 8);
        assert_error(&output, status, READ_ERROR);
    }

    let guarded = Fixture::new(
        "property-guarded",
        r"
spec MapPropertyReadGuarded {
  type K = 0..5
  state { m: Map<K, Int>, cur: K }
  init { forall k: K { m[k] = 0 }  cur = 0 }
  action advance() { requires cur < 5  cur = cur + 1 }
  invariant MapNonNeg { forall k: K { m[k] >= 0 } }
  invariant ReadOk { cur < 5 => m[cur + 1] >= 0 }
}
",
    );
    let (output, status) = verify(&guarded, "induction", 2);
    assert_eq!(status, 0, "{output:#}");
    assert_eq!(output["result"], "proved", "{output:#}");
}

#[test]
fn unreachable_key_miss_is_excluded_by_an_auxiliary_invariant() {
    // `cur` never passes 3, so `m[cur + 1]` stays in `0..5`; the CTI's start
    // state (`cur == 5`) is unreachable, and an invariant excluding it
    // restores `proved`.
    let source = |extra: &str| {
        format!(
            r"
spec MapNextSlotBounded {{
  type K = 0..5
  state {{ m: Map<K, Int>, cur: K, y: Int }}
  init {{ forall k: K {{ m[k] = 0 }}  cur = 0  y = 0 }}
  action advance() {{ requires cur < 3  cur = cur + 1 }}
  action peek_next() {{ y = m[cur + 1] }}
  invariant MapNonNeg {{ forall k: K {{ m[k] >= 0 }} }}
  {extra}
}}
"
        )
    };
    let without = Fixture::new("bounded", &source(""));
    let (output, status) = verify(&without, "induction", 8);
    assert_key_domain_cti(&output, status, "_key_domain_peek_next", Some("peek_next"));
    let (output, status) = verify(&without, "explicit", 8);
    assert_passes(&output, status);

    let with = Fixture::new("bounded-aux", &source("invariant CurLow { cur <= 3 }"));
    let (output, status) = verify(&with, "induction", 2);
    assert_eq!(status, 0, "{output:#}");
    assert_eq!(output["result"], "proved", "{output:#}");
}
