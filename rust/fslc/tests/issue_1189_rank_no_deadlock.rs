// SPDX-License-Identifier: Apache-2.0

//! CLI-level coverage for issue #1189: a ranked `leadsTo` without `helpful`
//! that can be pending in a state with no enabled action must not be reported
//! `proved`. The ranking proof's `no_deadlock` obligation
//! (docs/design/DESIGN-induction.md §2.3/§2.4) fails as `unknown_cti` /
//! `rank_failure: "deadlock"`, independent of `--deadlock` (which governs only
//! the ordinary, reachability-based deadlock report).

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
            "fsl-issue-1189-{name}-{}-{nonce}.fsl",
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

fn verify(path: &str, engine: &str, depth: &str, deadlock: &str) -> (Value, i32) {
    let output = Command::new(env!("CARGO_BIN_EXE_fslc"))
        .args([
            "verify",
            path,
            "--engine",
            engine,
            "--depth",
            depth,
            "--deadlock",
            deadlock,
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

/// The issue's repro: `x = 2` is pending and `dec` is disabled there.
const DEADLOCK_RANK: &str = r"
spec DeadlockRank {
  type X = 0..5
  state { x: X }
  init { x = 5 }
  action dec() { requires x > 2  x = x - 1 }
  leadsTo Drain { x > 0 ~> x == 0 decreases x }
}
";

/// No-false-alarm control: `dec` is enabled in every pending state.
const DRAIN_RANK: &str = r"
spec DrainRank {
  type X = 0..5
  state { x: X }
  init { x = 5 }
  action dec() { requires x > 0  x = x - 1 }
  leadsTo Drain { x > 0 ~> x == 0 decreases x }
}
";

#[test]
fn a_ranked_leadsto_pending_in_a_deadlock_is_never_proved_by_induction() {
    let fixture = Fixture::new("deadlock", DEADLOCK_RANK);
    for depth in ["1", "2", "3", "8"] {
        for deadlock in ["ignore", "warn", "error"] {
            let (output, status) = verify(fixture.text(), "induction", depth, deadlock);
            assert_ne!(output["result"], "proved", "depth {depth}: {output}");
            assert_eq!(status, 1, "depth {depth} --deadlock {deadlock}: {output}");
            if deadlock == "ignore" {
                assert_eq!(output["result"], "unknown_cti", "depth {depth}: {output}");
                assert_eq!(output["violation_kind"], "leadsTo_rank");
                assert_eq!(output["rank_failure"], "deadlock");
                assert_eq!(output["invariant"], "Drain");
                assert_eq!(output["completeness"], "bounded");
                assert_eq!(
                    output["message"],
                    "leadsTo 'Drain' can be pending in a state with no enabled action"
                );
                let x = output["cti"]["states"][0]["state"]["x"]
                    .as_i64()
                    .expect("CTI x");
                assert!((1..=2).contains(&x), "{output}");
                assert_eq!(output["measure_value"], x);
            }
        }
    }

    let (output, status) = verify(fixture.text(), "bmc", "8", "ignore");
    assert_eq!(status, 1, "{output}");
    assert_eq!(output["result"], "violated");
    assert_eq!(output["violation_kind"], "leadsTo");
}

#[test]
fn a_ranked_leadsto_enabled_in_every_pending_state_is_still_proved() {
    let fixture = Fixture::new("drain", DRAIN_RANK);
    for depth in ["1", "2", "3", "8"] {
        let (output, status) = verify(fixture.text(), "induction", depth, "ignore");
        assert_eq!(status, 0, "depth {depth}: {output}");
        assert_eq!(output["result"], "proved");
        assert_eq!(output["completeness"], "unbounded");
        assert_eq!(output["leads_to"]["Drain"]["proof"], "ranking");
        assert_eq!(output["leads_to"]["Drain"]["decreases"], "x");
    }
}
