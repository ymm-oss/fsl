// SPDX-License-Identifier: Apache-2.0

//! Issue #1237: `fslc mutate --fail-on-survivors` / `--min-kill-rate R` turn a
//! mutation run into an opt-in CI gate. `result` stays `"mutated"`; the
//! envelope gains `gate{..., violations, passed}` only when a flag is given,
//! and `gate.passed` decides the exit code.
//!
//! The mutant set is pinned with external-only runs (`--max-mutants 0
//! --from`), so the expected counts below do not move with the built-in
//! catalog: `killed.jsonl` has three mutants the net of checks kills, and
//! `mixed.jsonl` adds one survivor (kill rate exactly 0.75).

use std::path::Path;
use std::process::Command;

use serde_json::{Value, json};

const HEALTHY_SPEC: &str = "specs/cart_v1.fsl";
const KILLED_ONLY: &str = "rust/fslc/tests/fixtures/issue_1237_killed.jsonl";
const THREE_KILLED_ONE_SURVIVOR: &str = "rust/fslc/tests/fixtures/issue_1237_mixed.jsonl";
const VIOLATED_BASELINE: &str = "examples/gallery/errors/violated_invariant_counter.fsl";
const BANK_HEALTHY: &str = "rust/fslc/tests/fixtures/issue_848_bank_healthy.fsl";

fn workspace_root() -> &'static Path {
    Path::new(env!("CARGO_MANIFEST_DIR"))
        .parent()
        .and_then(Path::parent)
        .expect("workspace root")
}

fn run_raw(args: &[&str]) -> (Vec<u8>, String, i32) {
    let output = Command::new(env!("CARGO_BIN_EXE_fslc"))
        .args(args)
        .current_dir(workspace_root())
        .output()
        .expect("run native CLI");
    (
        output.stdout,
        String::from_utf8_lossy(&output.stderr).into_owned(),
        output.status.code().expect("native exit status"),
    )
}

fn run(args: &[&str]) -> (Value, i32) {
    let (stdout, stderr, status) = run_raw(args);
    let value = serde_json::from_slice(&stdout)
        .unwrap_or_else(|error| panic!("invalid JSON: {error}; stderr={stderr}"));
    (value, status)
}

fn mutate_external(from: &str, extra: &[&str]) -> (Value, i32) {
    let mut args = vec!["mutate", HEALTHY_SPEC, "--max-mutants", "0", "--from", from];
    args.extend_from_slice(extra);
    run(&args)
}

/// The fixtures mean what the test names say: guards the counts every other
/// test relies on, without consulting the gate.
#[test]
fn fixtures_pin_three_killed_and_one_survivor() {
    let (killed, status) = mutate_external(KILLED_ONLY, &[]);
    assert_eq!((killed["result"].as_str(), status), (Some("mutated"), 0));
    assert_eq!(
        (
            killed["summary"]["killed"].as_u64(),
            killed["summary"]["survived"].as_u64(),
            killed["summary"]["invalid"].as_u64(),
        ),
        (Some(3), Some(0), Some(0)),
        "{killed}"
    );
    let (mixed, status) = mutate_external(THREE_KILLED_ONE_SURVIVOR, &[]);
    assert_eq!((mixed["result"].as_str(), status), (Some("mutated"), 0));
    assert_eq!(
        (
            mixed["summary"]["killed"].as_u64(),
            mixed["summary"]["survived"].as_u64(),
            mixed["summary"]["invalid"].as_u64(),
        ),
        (Some(3), Some(1), Some(0)),
        "{mixed}"
    );
    assert_eq!(mixed["summary"]["kill_rate"], json!(0.75), "{mixed}");
}

// --- --fail-on-survivors ---------------------------------------------------

#[test]
fn fail_on_survivors_exits_one_when_a_mutant_survives() {
    let (value, status) = mutate_external(THREE_KILLED_ONE_SURVIVOR, &["--fail-on-survivors"]);
    assert_eq!(value["result"], "mutated", "{value}");
    assert_eq!(status, 1, "{value}");
    // `--max-mutants 0` drops the whole built-in catalog; that count moves
    // with the catalog, so it is checked against `notes`, not a constant.
    let mut gate = value["gate"].clone();
    let dropped = gate
        .as_object_mut()
        .and_then(|gate| gate.remove("dropped"))
        .and_then(|dropped| dropped.as_u64())
        .expect("gate.dropped");
    assert!(
        value["notes"]
            .as_array()
            .expect("notes")
            .contains(&json!(format!("mutant cap 0 reached: {dropped} dropped"))),
        "{value}"
    );
    assert_eq!(
        gate,
        json!({
            "fail_on_survivors": true,
            "min_kill_rate": null,
            "judged": 4,
            "survived": 1,
            "errored": 0,
            "kill_rate": 0.75,
            "violations": ["survivors"],
            "passed": false,
        }),
        "{value}"
    );
}

/// Negative control: a gate that always failed would pass the test above.
#[test]
fn fail_on_survivors_exits_zero_when_every_mutant_is_killed() {
    let (value, status) = mutate_external(KILLED_ONLY, &["--fail-on-survivors"]);
    assert_eq!(value["result"], "mutated", "{value}");
    assert_eq!(status, 0, "{value}");
    assert_eq!(value["gate"]["violations"], json!([]), "{value}");
    assert_eq!(value["gate"]["passed"], true, "{value}");
}

// --- --min-kill-rate boundary (kill_rate == 0.75) -------------------------

#[test]
fn min_kill_rate_just_above_the_published_rate_exits_one() {
    let (value, status) =
        mutate_external(THREE_KILLED_ONE_SURVIVOR, &["--min-kill-rate", "0.7501"]);
    assert_eq!(status, 1, "{value}");
    assert_eq!(
        value["gate"]["violations"],
        json!(["kill_rate_below_min"]),
        "{value}"
    );
    assert_eq!(value["gate"]["passed"], false, "{value}");
    assert_eq!(value["gate"]["fail_on_survivors"], false, "{value}");
    assert_eq!(value["gate"]["min_kill_rate"], json!(0.7501), "{value}");
}

#[test]
fn min_kill_rate_equal_to_the_published_rate_exits_zero() {
    let (value, status) = mutate_external(THREE_KILLED_ONE_SURVIVOR, &["--min-kill-rate", "0.75"]);
    assert_eq!(status, 0, "{value}");
    assert_eq!(value["gate"]["violations"], json!([]), "{value}");
    assert_eq!(value["gate"]["passed"], true, "{value}");
}

#[test]
fn min_kill_rate_just_below_the_published_rate_exits_zero() {
    let (value, status) =
        mutate_external(THREE_KILLED_ONE_SURVIVOR, &["--min-kill-rate", "0.7499"]);
    assert_eq!(status, 0, "{value}");
    assert_eq!(value["gate"]["passed"], true, "{value}");
}

/// The two flags are independent: a threshold alone does not fail on the
/// survivor it tolerates, and both together report both violations.
#[test]
fn flags_are_independent_and_combine() {
    let (value, status) = mutate_external(THREE_KILLED_ONE_SURVIVOR, &["--min-kill-rate", "0"]);
    assert_eq!(status, 0, "{value}");
    assert_eq!(value["gate"]["passed"], true, "{value}");

    let (value, status) = mutate_external(
        THREE_KILLED_ONE_SURVIVOR,
        &["--fail-on-survivors", "--min-kill-rate", "0.9"],
    );
    assert_eq!(status, 1, "{value}");
    assert_eq!(
        value["gate"]["violations"],
        json!(["survivors", "kill_rate_below_min"]),
        "{value}"
    );
}

// --- fail-closed: nothing judged is not a pass (decision 036) ------------

#[test]
fn zero_judged_mutants_fails_either_flag() {
    for flags in [&["--fail-on-survivors"][..], &["--min-kill-rate", "0"][..]] {
        let mut args = vec!["mutate", HEALTHY_SPEC, "--max-mutants", "0"];
        args.extend_from_slice(flags);
        let (value, status) = run(&args);
        assert_eq!(value["result"], "mutated", "{flags:?}: {value}");
        assert_eq!(status, 1, "{flags:?}: {value}");
        assert_eq!(value["gate"]["judged"], 0, "{flags:?}: {value}");
        assert_eq!(
            value["gate"]["kill_rate"],
            Value::Null,
            "{flags:?}: {value}"
        );
        assert_eq!(
            value["gate"]["violations"],
            json!(["no_judged_mutants"]),
            "{flags:?}: {value}"
        );
    }
}

// --- cap truncation is recorded, not failed (decision 036) ----------------

#[test]
fn mutants_dropped_by_the_cap_are_recorded_without_failing() {
    let (value, status) = run(&[
        "mutate",
        HEALTHY_SPEC,
        "--max-mutants",
        "1",
        "--min-kill-rate",
        "0",
    ]);
    assert_eq!(status, 0, "{value}");
    assert_eq!(value["gate"]["judged"], 1, "{value}");
    let dropped = value["gate"]["dropped"].as_u64().expect("gate.dropped");
    assert!(dropped > 0, "{value}");
    assert!(
        value["notes"]
            .as_array()
            .expect("notes")
            .contains(&json!(format!("mutant cap 1 reached: {dropped} dropped"))),
        "{value}"
    );
    assert_eq!(value["gate"]["passed"], true, "{value}");
}

// --- usage errors ----------------------------------------------------------

#[test]
fn min_kill_rate_outside_zero_to_one_is_a_usage_error() {
    for bad in ["1.5", "-0.1", "NaN", "inf", "abc"] {
        let (value, status) = mutate_external(KILLED_ONLY, &["--min-kill-rate", bad]);
        assert_eq!(status, 2, "{bad}: {value}");
        assert_eq!(value["kind"], "usage", "{bad}: {value}");
        assert_eq!(
            value["message"], "--min-kill-rate must be a number between 0 and 1",
            "{bad}: {value}"
        );
    }
    let (value, status) = run(&["mutate", HEALTHY_SPEC, "--min-kill-rate"]);
    assert_eq!(status, 2, "{value}");
    assert_eq!(
        value["message"], "--min-kill-rate requires a value",
        "{value}"
    );
    // Both ends of the closed interval are accepted.
    let (value, status) = mutate_external(KILLED_ONLY, &["--min-kill-rate", "1"]);
    assert_eq!(status, 0, "{value}");
    assert_eq!(value["gate"]["min_kill_rate"], json!(1.0), "{value}");
}

#[test]
fn gate_flags_are_rejected_by_the_commands_sharing_the_option_loop() {
    for command in ["explain", "typestate"] {
        for flags in [
            &["--fail-on-survivors"][..],
            &["--min-kill-rate", "0.5"][..],
        ] {
            let mut args = vec![command, HEALTHY_SPEC];
            args.extend_from_slice(flags);
            let (_, stderr, status) = run_raw(&args);
            assert_eq!(status, 2, "{command} {flags:?}: {stderr}");
        }
    }
}

// --- default unchanged (opt-in) -------------------------------------------

#[test]
fn without_a_flag_there_is_no_gate_and_survivors_exit_zero() {
    let (value, status) = mutate_external(THREE_KILLED_ONE_SURVIVOR, &[]);
    assert_eq!(status, 0, "{value}");
    assert!(value.get("gate").is_none(), "{value}");
}

const UNGATED_NOTE: &str = "possible equivalent mutants should be reviewed manually; survivors are a review queue, not a hard failure";
const GATED_NOTE: &str = "possible equivalent mutants should be reviewed manually; this run requested a gate, so gate.passed decides the exit code and survivors (including possible equivalent mutants) count toward it";

/// With a gate, survivors can fail the run, so the first note must not call
/// them "not a hard failure"; without one the historical wording stays.
#[test]
fn the_survivor_note_matches_whether_a_gate_was_requested() {
    let first_note = |value: &Value| value["notes"][0].as_str().map(str::to_owned);
    let (value, _) = mutate_external(THREE_KILLED_ONE_SURVIVOR, &[]);
    assert_eq!(first_note(&value).as_deref(), Some(UNGATED_NOTE), "{value}");
    for flags in [
        &["--fail-on-survivors"][..],
        &["--min-kill-rate", "0"][..],
        &["--fail-on-survivors", "--min-kill-rate", "0.9"][..],
    ] {
        let (value, _) = mutate_external(THREE_KILLED_ONE_SURVIVOR, flags);
        assert_eq!(
            first_note(&value).as_deref(),
            Some(GATED_NOTE),
            "{flags:?}: {value}"
        );
        assert!(
            !value["notes"]
                .as_array()
                .expect("notes")
                .iter()
                .any(|note| note
                    .as_str()
                    .is_some_and(|n| n.contains("not a hard failure"))),
            "{flags:?}: {value}"
        );
    }
}

/// The default stdout stays byte-identical to the checked-in #848 golden.
#[test]
fn default_stdout_is_byte_identical_to_the_issue_848_golden() {
    let (stdout, _, status) = run_raw(&["mutate", BANK_HEALTHY, "--depth", "8"]);
    assert_eq!(status, 0);
    let golden = include_bytes!("fixtures/issue_848_bank_healthy_default.stdout.json");
    assert_eq!(stdout.as_slice(), golden.as_slice());
}

/// A baseline that does not verify is re-emitted unchanged with its own exit
/// code; the gate neither applies nor appears.
#[test]
fn a_violated_baseline_is_not_touched_by_the_gate() {
    let (plain, _, plain_status) = run_raw(&["mutate", VIOLATED_BASELINE, "--max-mutants", "3"]);
    let (gated, _, gated_status) = run_raw(&[
        "mutate",
        VIOLATED_BASELINE,
        "--max-mutants",
        "3",
        "--fail-on-survivors",
        "--min-kill-rate",
        "1",
    ]);
    let value: Value = serde_json::from_slice(&gated).expect("JSON");
    assert_eq!(value["result"], "violated", "{value}");
    assert!(value.get("gate").is_none(), "{value}");
    assert_eq!((gated_status, plain_status), (1, 1));
    // `cost` carries wall-clock timings that differ between any two runs;
    // every other field must match the run without the gate flags.
    let strip_cost = |bytes: &[u8]| {
        let mut value: Value = serde_json::from_slice(bytes).expect("JSON");
        let removed = value
            .as_object_mut()
            .and_then(|object| object.remove("cost"));
        assert!(
            removed.is_some(),
            "the exclusion must remove a field: {value}"
        );
        value
    };
    assert_eq!(strip_cost(&gated), strip_cost(&plain));
}
