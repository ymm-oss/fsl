// SPDX-License-Identifier: Apache-2.0

//! Issue #1283: removing the init assignment of an enum-valued state leaves it
//! unconstrained. BMC finds the `_bounds_<state>` violation, but the solver may
//! pick a *negative* ordinal for the witness, and the counterexample projection
//! used to fail with `negative enum ordinal in solver model`:
//!
//! - `verify --engine bmc` reported `error` / `semantics` (exit 2) instead of
//!   `violated` / `_bounds_st`;
//! - built-in `mutate` turned that `Err` into `build_spec`, which the init
//!   special case then overwrote with `_bounds_st` (only `--oracle-attribution`
//!   leaked `"build_spec"` into `killers`);
//! - the same mutation given as an external mutant was `invalid`.
//!
//! `issue_1283_pr_no_st.fsl` is `issue_1283_pointer_registry.fsl` with the
//! `st[i] = ...` init line removed. Whether the solver picks a negative ordinal
//! depends on the depth, so the verify check runs at depths 1, 2 and 10.

use std::path::Path;
use std::process::Command;

use serde_json::Value;

const REGISTRY: &str = "rust/fslc/tests/fixtures/issue_1283_pointer_registry.fsl";
const NO_ST_INIT: &str = "rust/fslc/tests/fixtures/issue_1283_pr_no_st.fsl";
const RM_ST_INIT: &str = "rust/fslc/tests/fixtures/issue_1283_rm_st_init.jsonl";

fn workspace_root() -> &'static Path {
    Path::new(env!("CARGO_MANIFEST_DIR"))
        .parent()
        .and_then(Path::parent)
        .expect("workspace root")
}

fn run(args: &[&str]) -> (Value, i32) {
    let output = Command::new(env!("CARGO_BIN_EXE_fslc"))
        .args(args)
        .env("FSLC_CACHE", "off")
        .current_dir(workspace_root())
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

#[test]
fn bmc_reports_type_bound_violation_for_unconstrained_enum_state() {
    for depth in ["1", "2", "10"] {
        let (output, status) = run(&["verify", NO_ST_INIT, "--engine", "bmc", "--depth", depth]);
        assert_eq!(
            output["result"], "violated",
            "depth {depth}: projection must not fail: {output}"
        );
        assert_eq!(output["invariant"], "_bounds_st", "depth {depth}: {output}");
        assert_eq!(output["violation_kind"], "type_bound", "depth {depth}");
        assert_eq!(status, 1, "depth {depth}");
    }
}

/// The built-in `assignment_remove` of the `st` init line (fixture line 13) is
/// the fifth built-in mutant of the registry, after the four `type_bound_*`
/// mutants.
#[test]
fn builtin_init_removal_is_not_attributed_to_build_spec() {
    let (output, status) = run(&[
        "mutate",
        REGISTRY,
        "--depth",
        "2",
        "--max-mutants",
        "5",
        "--oracle-attribution",
    ]);
    assert_eq!(status, 0, "{output}");
    let mutant = output["mutants"]
        .as_array()
        .expect("mutants")
        .iter()
        .find(|mutant| mutant["op"] == "assignment_remove" && mutant["loc"]["line"] == 13)
        .unwrap_or_else(|| panic!("st init removal mutant: {output}"));
    assert_eq!(mutant["status"], "killed", "{mutant}");
    assert_eq!(mutant["killed_by"], "_bounds_st", "{mutant}");
    let killers = mutant["killers"].as_array().expect("killers");
    assert!(
        !killers.iter().any(|killer| killer == "build_spec"),
        "an oracle failure leaked into killers: {mutant}"
    );
    assert!(
        killers.iter().any(|killer| killer == "_bounds_st"),
        "{mutant}"
    );
}

/// The same mutation given externally is judged like the built-in one.
#[test]
fn external_init_removal_matches_builtin_verdict() {
    let (output, status) = run(&[
        "mutate",
        REGISTRY,
        "--depth",
        "2",
        "--max-mutants",
        "0",
        "--from",
        RM_ST_INIT,
    ]);
    assert_eq!(status, 0, "{output}");
    let mutant = &output["mutants"][0];
    assert_eq!(mutant["id"], "rm-st-init", "{output}");
    assert_eq!(mutant["status"], "killed", "{mutant}");
    assert_eq!(mutant["killed_by"], "_bounds_st", "{mutant}");
}
