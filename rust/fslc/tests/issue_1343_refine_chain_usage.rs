// SPDX-License-Identifier: Apache-2.0

//! Issue #1343: odd refinement-chain operands are usage errors, not I/O.

use std::path::Path;
use std::process::Command;

use serde_json::{Value, json};

const IMPL: &str = "specs/cart_impl.fsl";
const ABS: &str = "specs/cart_v1.fsl";
const MAP: &str = "specs/cart_refines.fsl";
const ODD_MESSAGE: &str = "refine chain must list (abs map) pairs after the first mapping";

fn root() -> &'static Path {
    Path::new(env!("CARGO_MANIFEST_DIR"))
        .parent()
        .and_then(Path::parent)
        .expect("workspace root")
}

fn run(args: &[&str]) -> (Option<i32>, Value) {
    let output = Command::new(env!("CARGO_BIN_EXE_fslc"))
        .args(args)
        .current_dir(root())
        .env("FSLC_CACHE", "off")
        .env_remove("FSLC_UNSTABLE")
        .output()
        .expect("run native CLI");
    assert!(
        output.stderr.is_empty(),
        "{args:?}: unexpected stderr: {}",
        String::from_utf8_lossy(&output.stderr)
    );
    (
        output.status.code(),
        serde_json::from_slice(&output.stdout).expect("JSON stdout"),
    )
}

fn usage(args: &[&str], message: &str) {
    assert_eq!(
        run(args),
        (
            Some(2),
            json!({"fsl":"1.0", "result":"error", "kind":"usage", "message":message}),
        ),
        "{args:?}"
    );
}

#[test]
fn one_unpaired_chain_operand_is_exact_usage() {
    for args in [
        vec!["refine", "--depth", "2", IMPL, ABS, MAP, ABS],
        vec!["refine", IMPL, ABS, "--depth", "2", MAP, ABS],
        vec!["refine", IMPL, ABS, MAP, ABS, "--depth", "2"],
    ] {
        usage(&args, ODD_MESSAGE);
    }
}

#[test]
fn three_unpaired_chain_operands_are_exact_usage() {
    usage(&["refine", IMPL, ABS, MAP, ABS, MAP, ABS], ODD_MESSAGE);
}

#[test]
fn odd_operand_usage_does_not_depend_on_file_existence() {
    usage(&["refine", "a", "b", "c", "d"], ODD_MESSAGE);
}

#[test]
fn other_refine_argument_errors_remain_usage() {
    usage(&["refine"], "refine requires IMPL ABS MAPPING");
    usage(&["refine", IMPL, ABS], "refine requires IMPL ABS MAPPING");
    usage(
        &["refine", IMPL, ABS, MAP, "--depth"],
        "--depth requires a value",
    );
    usage(
        &["refine", "--depth", "x", IMPL, ABS, MAP],
        "--depth must be a non-negative integer",
    );
    usage(
        &["refine", IMPL, ABS, MAP, "--bogus"],
        "unknown refine option '--bogus'",
    );
}

#[test]
fn genuine_missing_spec_remains_io() {
    let missing = format!("rust/target/issue-1343-missing-{}.fsl", std::process::id());
    let path = root().join(&missing);
    let error = std::fs::read_to_string(&path).expect_err("missing fixture must not exist");
    assert_eq!(
        run(&["refine", &missing, ABS, MAP]),
        (
            Some(2),
            json!({
                "fsl":"1.0", "result":"error", "kind":"io",
                "message":format!("{missing}: {error}"),
            }),
        )
    );
}

#[test]
fn literate_input_gate_keeps_precedence_over_odd_arity() {
    assert_eq!(
        run(&["refine", "a.md", "b", "c", "d"]),
        (
            Some(2),
            json!({
                "fsl":"1.0", "result":"error", "kind":"usage",
                "message":"fslc refine does not support literate Markdown input; only check, verify, scenarios extract ```fsl fenced code from a .md file -- pass 'a.md' to one of those, or extract the fenced code into a .fsl file first",
                "diagnostic_code":"FSL-INPUT-LITERATE-UNSUPPORTED",
                "loc":{"file":"a.md"},
            }),
        )
    );
}

#[test]
fn three_required_operands_still_refine() {
    let (status, value) = run(&["refine", IMPL, ABS, MAP, "--depth", "2"]);
    assert_eq!(status, Some(0), "{value}");
    assert_eq!(value["result"], "refines", "{value}");
}
