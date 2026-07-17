// SPDX-License-Identifier: Apache-2.0

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
            "fsl-issue-260-{name}-{}-{nonce}.fsl",
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

fn run(args: &[&str]) -> (Value, i32) {
    let output = Command::new(env!("CARGO_BIN_EXE_fslc"))
        .args(args)
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

const BROWSER_LEAK: &str = r#"
business BrowserLeak {
  actor System
  entity Job
  process Job {
    stages Idle, Pending, Done, Dropped
    initial Idle
    transition submit Idle    -> Pending by System
    transition finish Pending -> Done    by System
    transition drop   Pending -> Dropped by System
  }
  policy POL-DONE "every submitted job must eventually complete"
    every Job in Pending must eventually be Done
}
verify { instances Job = 1 }
"#;

const STUCK_KERNEL: &str = r"
spec BrowserStuckKernel {
  enum Stage { Idle, Pending, Done }
  state { stage: Stage }
  init { stage = Idle }
  action submit() {
    requires stage == Idle
    stage = Pending
  }
  leadsTo Progress { stage == Pending ~> stage == Done }
}
";

#[test]
fn leadsto_deadlock_stagnation_is_detected_beyond_the_deadlock_step() {
    let fixture = Fixture::new("business", BROWSER_LEAK);
    for depth in ["2", "3", "4", "6"] {
        let (value, status) = run(&[
            "verify",
            fixture.text(),
            "--depth",
            depth,
            "--deadlock",
            "ignore",
            "--no-cache",
        ]);
        assert_eq!(status, 1, "depth {depth}: {value}");
        assert_eq!(value["result"], "violated", "depth {depth}: {value}");
        assert_eq!(value["violation_kind"], "leadsTo", "depth {depth}: {value}");
        assert_eq!(value["invariant"], "POL-DONE", "depth {depth}: {value}");
        assert_eq!(value["pending_since"], 1, "depth {depth}: {value}");
        assert_eq!(value["stutter"], true, "depth {depth}: {value}");
        assert!(
            value["trace"]
                .as_array()
                .is_some_and(|trace| !trace.is_empty()),
            "depth {depth}: expected a non-empty trace, got {value}"
        );
    }
}

#[test]
fn leadsto_deadlock_stagnation_is_detected_beyond_the_deadlock_step_for_plain_kernel_spec() {
    let fixture = Fixture::new("kernel", STUCK_KERNEL);
    for depth in ["1", "2", "4"] {
        let (value, status) = run(&[
            "verify",
            fixture.text(),
            "--depth",
            depth,
            "--deadlock",
            "ignore",
            "--no-cache",
        ]);
        assert_eq!(status, 1, "depth {depth}: {value}");
        assert_eq!(value["result"], "violated", "depth {depth}: {value}");
        assert_eq!(value["violation_kind"], "leadsTo", "depth {depth}: {value}");
        assert_eq!(value["invariant"], "Progress", "depth {depth}: {value}");
        assert_eq!(value["stutter"], true, "depth {depth}: {value}");
    }
}

#[test]
fn deadlock_error_still_wins_over_leadsto_beyond_the_deadlock_step() {
    let fixture = Fixture::new("kernel-error", STUCK_KERNEL);
    let (value, status) = run(&[
        "verify",
        fixture.text(),
        "--depth",
        "3",
        "--deadlock",
        "error",
        "--no-cache",
    ]);
    assert_eq!(status, 1, "{value}");
    assert_eq!(value["result"], "violated", "{value}");
    assert_eq!(value["violation_kind"], "deadlock", "{value}");
}

#[test]
fn leadsto_stagnation_is_detected_when_the_deadlock_is_the_initial_state() {
    let source = r"
spec StuckAtInit {
  enum Stage { Idle, Done }
  state { stage: Stage }
  init { stage = Idle }
  action unreachable() {
    requires stage == Done
    stage = Done
  }
  leadsTo Progress { stage == Idle ~> stage == Done }
}
";
    let fixture = Fixture::new("stuck-at-init", source);
    for depth in ["0", "1", "3"] {
        let (value, status) = run(&[
            "verify",
            fixture.text(),
            "--depth",
            depth,
            "--deadlock",
            "ignore",
            "--no-cache",
        ]);
        assert_eq!(status, 1, "depth {depth}: {value}");
        assert_eq!(value["result"], "violated", "depth {depth}: {value}");
        assert_eq!(value["violation_kind"], "leadsTo", "depth {depth}: {value}");
        assert_eq!(value["pending_since"], 0, "depth {depth}: {value}");
        assert_eq!(value["stutter"], true, "depth {depth}: {value}");
    }
}
