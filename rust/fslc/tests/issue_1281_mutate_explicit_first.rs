// SPDX-License-Identifier: Apache-2.0

//! #1281 PR B: stderr-only explicit-first progress; complete stdout identity.
//! Hashes pin raw stdout (including its newline) from main ddce25fe8ae0f15f031e130b362bd99de65b0c1c.
//! Capture: `FSLC_CACHE=off rust/target/debug/fslc mutate <case args below>`,
//! from the repository root, default native debug build, no field exclusions.
//! Progress tests are detectors (red before PR B); identity/error tests are
//! preservation controls and must already pass before production changes.

use std::path::Path;
use std::process::{Command, Output};

use serde_json::Value;
use sha2::{Digest, Sha256};

#[path = "support/mutate_progress.rs"]
mod mutate_progress;
use mutate_progress::{assert_report_progress, progress};

const RBAC: &[&str] = &[
    "examples/structural/rbac.fsl",
    "--depth",
    "8",
    "--max-mutants",
    "0",
];
const POINTER: &[&str] = &[
    "rust/fslc/tests/fixtures/issue_1283_pointer_registry.fsl",
    "--depth",
    "3",
    "--max-mutants",
    "6",
];
const BUILTIN: &[&str] = &[
    "rust/fslc/tests/fixtures/sweep_clean.fsl",
    "--depth",
    "2",
    "--max-mutants",
    "1000",
];
const EXTERNAL: &[&str] = &[
    "rust/fslc/tests/fixtures/sweep_clean.fsl",
    "--depth",
    "2",
    "--max-mutants",
    "0",
    "--from",
    "rust/fslc/tests/fixtures/issue_1281_progress.jsonl",
];
const EXTERNAL_GATE: &[&str] = &[
    "rust/fslc/tests/fixtures/sweep_clean.fsl",
    "--depth",
    "2",
    "--max-mutants",
    "1",
    "--from",
    "rust/fslc/tests/fixtures/issue_1281_progress.jsonl",
    "--min-kill-rate",
    "1",
];
const UNREADABLE_CATALOG: &str = "does/not/exist.jsonl";
const UNREADABLE: &[&str] = &[
    "rust/fslc/tests/fixtures/sweep_clean.fsl",
    "--depth",
    "2",
    "--max-mutants",
    "1",
    "--from",
    UNREADABLE_CATALOG,
];

fn run(args: &[&str]) -> Output {
    Command::new(env!("CARGO_BIN_EXE_fslc"))
        .arg("mutate")
        .args(args)
        .current_dir(
            Path::new(env!("CARGO_MANIFEST_DIR"))
                .parent()
                .and_then(Path::parent)
                .expect("repository root"),
        )
        .env("FSLC_CACHE", "off")
        .env_remove("FSLC_UNSTABLE")
        .output()
        .expect("run native mutate")
}

fn report(output: &Output, exit: i32) -> Value {
    assert_eq!(output.status.code(), Some(exit), "{output:?}");
    let value: Value = serde_json::from_slice(&output.stdout).expect("single JSON stdout");
    assert_eq!(value["result"], "mutated", "{value}");
    assert_eq!(value["baseline"], "verified", "{value}");
    value
}

#[test]
fn rbac_baseline_uses_explicit_and_keeps_legacy_verified_literal() {
    let output = run(RBAC);
    let value = report(&output, 0);
    let lines = progress(&output);
    assert_eq!(lines.len(), 1, "cap zero evaluates no mutants");
    assert!(matches!(
        lines[0].0,
        "mutate baseline: proved" | "mutate baseline: verified"
    ));
    assert_eq!(lines[0].1, "explicit");
    assert_eq!(value["baseline"], "verified");
}

#[test]
fn baseline_claims_unbounded_only_on_explicit_closure() {
    let closed = run(&[
        "rust/fslc/tests/fixtures/issue_1281_closed.fsl",
        "--depth",
        "2",
        "--max-mutants",
        "0",
    ]);
    report(&closed, 0);
    assert_eq!(
        progress(&closed),
        [("mutate baseline: proved", "explicit")],
        "a closed state space is the only proved baseline"
    );
    assert!(!String::from_utf8_lossy(&closed.stdout).contains("proved"));

    let cut = run(&[
        "rust/fslc/tests/fixtures/issue_1281_depth_cut.fsl",
        "--depth",
        "3",
        "--max-mutants",
        "0",
    ]);
    report(&cut, 0);
    assert_eq!(
        progress(&cut),
        [("mutate baseline: verified", "explicit")],
        "a depth-cut explicit baseline must stay bounded"
    );
    assert!(!String::from_utf8_lossy(&cut.stdout).contains("proved"));
}

#[test]
fn builtin_kill_by_a_later_check_is_not_labelled_with_the_main_route() {
    let forbidden = run(&[
        "examples/gallery/valid/small_forbidden_guarded_cancel.fsl",
        "--max-mutants",
        "2",
    ]);
    let value = report(&forbidden, 0);
    assert_report_progress(&forbidden, &value);
    assert_eq!(
        progress(&forbidden)[1..],
        [
            (
                "mutate [1/2] type_bound_lo_minus1 type OrderId lo: survived",
                "explicit"
            ),
            (
                "mutate [2/2] type_bound_lo_plus1 type OrderId lo: killed forbidden",
                "secondary, oracle=explicit"
            ),
        ]
    );

    let acceptance = run(&[
        "examples/self/monitor_action_boundary.fsl",
        "--max-mutants",
        "3",
    ]);
    let value = report(&acceptance, 0);
    assert_report_progress(&acceptance, &value);
    assert_eq!(
        progress(&acceptance)[3],
        (
            "mutate [3/3] type_bound_hi_minus1 type Count hi: killed acceptance",
            "secondary, oracle=explicit"
        )
    );
}

#[test]
fn external_kill_by_refinement_is_not_labelled_with_the_main_route() {
    let output = run(&[
        "examples/pm/cancel_system.fsl",
        "--max-mutants",
        "0",
        "--from",
        "rust/fslc/tests/fixtures/issue_1281_refinement_kill.jsonl",
    ]);
    let value = report(&output, 0);
    assert_report_progress(&output, &value);
    assert_eq!(
        progress(&output)[1],
        (
            "mutate external [1/1] drop-cancel-guard: killed refinement",
            "secondary, oracle=explicit"
        )
    );
}

/// The budget belongs to the implements walk, so `inconclusive` is the later
/// check's status, not the route's (#1262).
#[test]
fn later_inconclusive_is_not_labelled_as_the_main_route() {
    let builtin = run(&[
        "rust/fslc/tests/fixtures/issue_1281_budget_req.fsl",
        "--depth",
        "6",
        "--max-mutants",
        "6",
    ]);
    let value = report(&builtin, 0);
    assert_report_progress(&builtin, &value);
    assert_eq!(value["mutants"][5]["status"], "inconclusive", "{value}");
    assert_eq!(
        progress(&builtin)[6],
        (
            "mutate [6/6] requires_remove push requires #1: inconclusive",
            "secondary, oracle=bmc"
        )
    );

    let external = run(&[
        "rust/fslc/tests/fixtures/issue_1281_budget_req.fsl",
        "--depth",
        "6",
        "--max-mutants",
        "0",
        "--from",
        "rust/fslc/tests/fixtures/issue_1281_budget_external.jsonl",
    ]);
    let value = report(&external, 0);
    assert_report_progress(&external, &value);
    assert_eq!(
        progress(&external)[1],
        (
            "mutate external [1/1] widen: inconclusive",
            "secondary, oracle=bmc"
        )
    );
}

/// BMC accepts the removed `Set` init, then the implements walk refuses the
/// incomplete concrete init, so `error` is the later check's status.
#[test]
fn later_error_is_not_labelled_as_the_main_route() {
    let output = run(&[
        "rust/fslc/tests/fixtures/issue_1281_later_error_req.fsl",
        "--depth",
        "2",
    ]);
    let value = report(&output, 0);
    assert_report_progress(&output, &value);
    assert_eq!(value["mutants"][4]["status"], "error", "{value}");
    assert_eq!(
        value["mutants"][4]["error"]["stage"], "implements",
        "{value}"
    );
    assert_eq!(
        progress(&output)[5],
        (
            "mutate [5/6] assignment_remove init assignment: error",
            "secondary, oracle=bmc"
        )
    );
}

/// The override runs after the main oracle: [14/37] was clean there, while
/// [11/37] and [15/37] were main BMC kills of the same bound.
#[test]
fn init_bounds_override_after_a_clean_main_oracle_is_a_later_decision() {
    let output = run(&[
        "examples/gallery/adversarial/option_struct_set_seq_combo.fsl",
        "--depth",
        "3",
        "--max-mutants",
        "40",
    ]);
    let value = report(&output, 0);
    assert_report_progress(&output, &value);
    let lines = progress(&output);
    assert_eq!(
        lines[14],
        (
            "mutate [14/37] assignment_remove init assignment: killed _bounds_active",
            "secondary, oracle=bmc"
        )
    );
    assert_eq!(
        lines[11],
        (
            "mutate [11/37] assignment_remove init assignment: killed _bounds_jobs",
            "bmc"
        )
    );
    assert_eq!(
        lines[15],
        (
            "mutate [15/37] assignment_remove init assignment: killed _bounds_q",
            "bmc"
        )
    );
}

#[test]
fn main_oracle_verdicts_keep_their_route() {
    let output = run(&[
        "rust/fslc/tests/fixtures/issue_1281_main_kill.fsl",
        "--depth",
        "2",
        "--max-mutants",
        "1000",
    ]);
    let value = report(&output, 0);
    assert_report_progress(&output, &value);
    let lines = progress(&output);
    assert!(
        lines.iter().any(|(body, engine)| body
            .ends_with("requires_remove step requires #1: killed Safe")
            && *engine == "bmc"),
        "{lines:?}"
    );
    assert!(
        lines
            .iter()
            .any(|(body, engine)| body.ends_with(": survived") && *engine == "explicit"),
        "{lines:?}"
    );
    assert!(
        lines
            .iter()
            .all(|(_, engine)| !engine.starts_with("secondary")),
        "this spec has no later check that can kill: {lines:?}"
    );
}

#[test]
fn builtin_progress_uses_evaluated_count_not_the_larger_cap() {
    let output = run(BUILTIN);
    let value = report(&output, 0);
    assert_eq!(value["summary"]["total"], 14);
    assert_report_progress(&output, &value);
}

#[test]
fn pointer_registry_has_a_per_mutant_explicit_route() {
    let output = run(POINTER);
    let value = report(&output, 0);
    assert_eq!(value["summary"]["total"], 6);
    assert!(
        progress(&output)
            .iter()
            .any(|(body, engine)| body.starts_with("mutate [") && *engine == "explicit"),
        "baseline alone does not establish an explicit mutant shortcut"
    );
    assert_report_progress(&output, &value);
}

#[test]
fn external_only_progress_includes_invalid_and_duplicate_records() {
    let output = run(EXTERNAL);
    let value = report(&output, 0);
    assert_eq!(value["summary"]["total"], 4);
    assert_eq!(value["summary"]["invalid"], 2);
    // Duplicate IDs are invalid records, not command-level errors on main.
    assert!(
        value["mutants"][3]["invalid"]["message"]
            .as_str()
            .expect("duplicate diagnostic")
            .contains("duplicate external mutant id")
    );
    assert_report_progress(&output, &value);
}

#[test]
fn failing_gate_is_still_a_report_with_builtin_and_external_progress() {
    let output = run(EXTERNAL_GATE);
    let value = report(&output, 1);
    assert_eq!(value["summary"]["total"], 5);
    assert_eq!(value["summary"]["by_source"]["builtin"]["total"], 1);
    assert_eq!(value["summary"]["by_source"]["external"]["total"], 4);
    assert_report_progress(&output, &value);
}

#[test]
fn successful_report_stdout_is_byte_identical_to_main() {
    for (args, exit, expected) in [
        (
            RBAC,
            0,
            "0cf2ab9e73ebf5070f6be5ce4963fdc40d165cd4a3a83f46a8f05cd45e890494",
        ),
        (
            POINTER,
            0,
            "2b2a659698d848bf2bfa374916177015425bcfc6f5b534ebf613ce1ee148b43f",
        ),
        (
            BUILTIN,
            0,
            "0398133ba8110fe9ac57f5bd60f46e13b370a98f6d9cc5f1415a8c5b1caa09c4",
        ),
        (
            EXTERNAL,
            0,
            "9be842f189b8dc798d137baeb40ca36c852e78193acfad4b6ebd81196e4756b6",
        ),
        (
            EXTERNAL_GATE,
            1,
            "924572cefbd289bf5ac3a670b518cdd1a82d65ff73672efec77273b29f8d8aa6",
        ),
    ] {
        let output = run(args);
        report(&output, exit);
        assert_eq!(
            format!("{:x}", Sha256::digest(&output.stdout)),
            expected,
            "complete stdout drifted for {args:?}; no public engine fields or exclusions"
        );
    }
}

#[test]
fn unreadable_catalog_after_a_clean_baseline_is_quiet_and_byte_identical() {
    // Cap one matters: preloading must fail before even a built-in progress
    // line, not merely happen to stay quiet in an external-only invocation.
    let root = Path::new(env!("CARGO_MANIFEST_DIR"))
        .parent()
        .and_then(Path::parent)
        .expect("repository root");
    // A missing directory is OS error 3 on Windows, not missing-file error 2.
    let error = std::fs::read_to_string(root.join(UNREADABLE_CATALOG))
        .expect_err("catalog must remain unreadable");
    assert_eq!(error.kind(), std::io::ErrorKind::NotFound);
    let expected = format!(
        concat!(
            "{{\n",
            "  \"fsl\": \"1.0\",\n",
            "  \"result\": \"error\",\n",
            "  \"kind\": \"io\",\n",
            "  \"message\": {}\n",
            "}}\n",
        ),
        // Issue #1170 route c: the catalog is named with the path the caller
        // passed (it was the bare OS error without it).
        serde_json::to_string(&format!("{UNREADABLE_CATALOG}: {error}"))
            .expect("serialize OS error message"),
    );
    // Retain main's Unix identity while allowing the platform's own OS text.
    // #1170 moved it from 0378857a… (the bare OS error) to this digest.
    #[cfg(unix)]
    assert_eq!(
        format!("{:x}", Sha256::digest(expected.as_bytes())),
        "553cc70deb689d5597c5e29cee0d26b780f7cc30b0e3599caa94f1aefa92b55d"
    );
    let output = run(UNREADABLE);
    assert_eq!(output.status.code(), Some(2));
    assert!(output.stderr.is_empty(), "{output:?}");
    let value: Value = serde_json::from_slice(&output.stdout).expect("error JSON");
    assert_eq!(value["result"], "error");
    assert_eq!(value["kind"], "io");
    assert_eq!(
        output.stdout.as_slice(),
        expected.as_bytes(),
        "complete unreadable-catalog stdout drifted; no fields or bytes excluded"
    );
}

#[test]
fn refused_baseline_has_no_progress() {
    let output = run(&[
        "examples/gallery/errors/violated_invariant_counter.fsl",
        "--depth",
        "3",
        "--max-mutants",
        "1",
    ]);
    assert_eq!(output.status.code(), Some(1));
    assert!(output.stderr.is_empty(), "{output:?}");
    let value: Value = serde_json::from_slice(&output.stdout).expect("baseline JSON");
    assert_eq!(value["result"], "violated");
    // Main bc14d885's BMC refusal has no engine key; rejected auto instead
    // carries engine:"explicit". Pin the route without comparing run metrics.
    assert!(
        value.get("engine").is_none(),
        "must emit the BMC rerun's envelope, not the rejected auto envelope: {value}"
    );
    assert!(value.get("mutants").is_none());
}
