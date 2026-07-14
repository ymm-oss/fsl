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
            "fsl-issue-266-{name}-{}-{nonce}.fsl",
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

// The `within 1` deadline is missed at step 2 (x == 2), and the path then
// deadlocks at x == 3. Before the fix, any --depth beyond 3 asserted a forced
// forward transition out of the deadlocked step, making the whole branch
// unsatisfiable for the post-loop deadline probe: depth 3 reported violated,
// depth 4+ reported a false verified.
const LATE_Q: &str = r"
spec LateQ {
  type Count = 0..3
  state { x: Count }
  init { x = 0 }
  action step() {
    requires x < 3
    x = x + 1
  }
  leadsTo Progress { x == 1 ~> within 1 x == 3 }
}
";

// The same missed deadline on a path that never deadlocks within the bound:
// the pre-#266 post-loop probe already caught this, and the inline probe must
// keep catching it.
const SLOW_Q: &str = r"
spec SlowQ {
  type Count = 0..6
  state { x: Count }
  init { x = 0 }
  action step() {
    requires x < 6
    x = x + 1
  }
  leadsTo Progress { x == 1 ~> within 1 x == 4 }
}
";

#[test]
fn within_deadline_miss_is_detected_at_every_depth_beyond_the_deadlock_step() {
    let fixture = Fixture::new("late-q", LATE_Q);
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
        assert_eq!(value["invariant"], "Progress", "depth {depth}: {value}");
        assert_eq!(value["pending_since"], 1, "depth {depth}: {value}");
        assert_eq!(value["deadline"], 2, "depth {depth}: {value}");
        assert_eq!(value["stutter"], false, "depth {depth}: {value}");
        assert!(
            value["trace"]
                .as_array()
                .is_some_and(|trace| !trace.is_empty()),
            "depth {depth}: expected a non-empty trace, got {value}"
        );
    }
}

#[test]
fn within_deadline_miss_without_deadlock_is_still_detected() {
    let fixture = Fixture::new("slow-q", SLOW_Q);
    for depth in ["2", "4", "6"] {
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
        assert_eq!(value["pending_since"], 1, "depth {depth}: {value}");
        assert_eq!(value["deadline"], 2, "depth {depth}: {value}");
    }
}

// One branch (sprint) satisfies Q exactly on the deadline; the other (step)
// misses it and then deadlocks. The probe must find the violating branch,
// not settle for the surviving one.
const BRANCH_Q: &str = r"
spec BranchQ {
  type Count = 0..3
  state { x: Count }
  init { x = 0 }
  action step() {
    requires x < 3
    x = x + 1
  }
  action sprint() {
    requires x == 1
    x = 3
  }
  leadsTo Progress { x == 1 ~> within 1 x == 3 }
}
";

#[test]
fn within_deadline_miss_on_one_branch_is_detected_despite_a_satisfying_branch() {
    let fixture = Fixture::new("branch-q", BRANCH_Q);
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
        assert_eq!(value["pending_since"], 1, "depth {depth}: {value}");
        assert_eq!(value["deadline"], 2, "depth {depth}: {value}");
    }
}

#[test]
fn deadlock_before_the_deadline_is_still_reported_as_stagnation() {
    // The path deadlocks at step 1 with the obligation pending and the
    // `within 5` deadline still in the future: the deadline probe is
    // unsatisfiable there, and the stagnation check must report the
    // violation (stutter) instead of letting the spec verify.
    let source = r"
spec StuckBeforeDeadline {
  enum Stage { Idle, Pending, Done }
  state { stage: Stage }
  init { stage = Idle }
  action submit() {
    requires stage == Idle
    stage = Pending
  }
  leadsTo Progress { stage == Pending ~> within 5 stage == Done }
}
";
    let fixture = Fixture::new("stuck-before-deadline", source);
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
        assert_eq!(value["pending_since"], 1, "depth {depth}: {value}");
        assert_eq!(value["stutter"], true, "depth {depth}: {value}");
    }
}

#[test]
fn within_deadline_met_before_a_trailing_deadlock_stays_verified() {
    let source = LATE_Q.replace("within 1", "within 2");
    let fixture = Fixture::new("late-q-ok", &source);
    for depth in ["3", "4", "6"] {
        let (value, status) = run(&[
            "verify",
            fixture.text(),
            "--depth",
            depth,
            "--deadlock",
            "ignore",
            "--no-cache",
        ]);
        assert_eq!(status, 0, "depth {depth}: {value}");
        assert_eq!(value["result"], "verified", "depth {depth}: {value}");
    }
}
