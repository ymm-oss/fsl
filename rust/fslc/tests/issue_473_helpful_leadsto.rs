// SPDX-License-Identifier: Apache-2.0

//! CLI-level coupled-change coverage for issue #473: `leadsTo ... helpful`
//! ranking proofs, end to end through the native `fslc` binary's JSON
//! envelope and exit-code contract (docs/LANGUAGE.md:383, :441-452,
//! docs/DESIGN-induction.md).

use std::process::Command;

fn run(args: &[&str]) -> (serde_json::Value, i32) {
    let root = std::path::Path::new(env!("CARGO_MANIFEST_DIR"))
        .parent()
        .and_then(std::path::Path::parent)
        .expect("workspace root");
    let output = Command::new(env!("CARGO_BIN_EXE_fslc"))
        .args(args)
        .current_dir(root)
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
fn helpful_leadsto_ranking_proof_is_exit_0_proved_unbounded() {
    // Positive path: the documented per-entity `helpful` idiom
    // (docs/LANGUAGE.md:441-452) must produce the same verdict shape as the
    // frozen Python reference: exit 0, `proved`, `completeness:"unbounded"`,
    // and `leads_to.<name>.helpful` echoing the declared helpful action(s).
    let (output, status) = run(&[
        "verify",
        "rust/fslc/tests/fixtures/issue_473_helpful_leadsto.fsl",
        "--engine",
        "induction",
        "--depth",
        "1",
        "--no-cache",
    ]);

    assert_eq!(status, 0, "{output}");
    assert_eq!(output["result"], "proved");
    assert_eq!(output["completeness"], "unbounded");
    let responds = &output["leads_to"]["Responds"];
    assert_eq!(responds["proof"], "ranking");
    assert_eq!(responds["decreases"], "level[c]");
    assert_eq!(responds["helpful"], serde_json::json!(["step(c)"]));
}

#[test]
fn helpful_action_naming_an_undeclared_action_is_rejected_at_check_time() {
    // Negative control: a misspelled `helpful` action name must fail
    // `fslc check` with a located type error -- not be silently accepted
    // (the false-green this issue reports for the pre-fix native binary).
    let (output, status) = run(&[
        "check",
        "rust/fslc/tests/fixtures/issue_473_helpful_typo.fsl",
    ]);

    assert_eq!(status, 2, "{output}");
    assert_eq!(output["result"], "error");
    assert_eq!(output["kind"], "semantics");
    let message = output["message"].as_str().unwrap_or_default();
    assert!(
        message.contains("helpful action 'nosuch' is not declared"),
        "{message}"
    );
}
