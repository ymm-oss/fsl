// SPDX-License-Identifier: Apache-2.0
// Copyright 2026 Ryoichi Izumita

//! Regression controls for #640(a): saga observations must not duplicate an
//! effect-owned outcome without the effect's persistent correlation guard.

use std::collections::BTreeSet;
use std::fs;
use std::path::{Path, PathBuf};
use std::process::{Command, Output};

use serde_json::Value;

const FIXTURE: &str = "rust/fslc/tests/fixtures/domain_characterization/effect_saga_valid.fsl";

fn root() -> PathBuf {
    Path::new(env!("CARGO_MANIFEST_DIR"))
        .parent()
        .and_then(Path::parent)
        .expect("workspace root")
        .to_owned()
}

fn run(args: &[&str]) -> Output {
    Command::new(env!("CARGO_BIN_EXE_fslc"))
        .args(args)
        .current_dir(root())
        .output()
        .expect("run native fslc")
}

fn json(output: &Output, args: &[&str]) -> Value {
    serde_json::from_slice(&output.stdout).unwrap_or_else(|error| {
        panic!(
            "invalid JSON: {error}; args={args:?}; stderr={}",
            String::from_utf8_lossy(&output.stderr)
        )
    })
}

fn expand_fixture() -> String {
    let args = ["domain", "expand", FIXTURE];
    let output = run(&args);
    assert_eq!(
        output.status.code(),
        Some(0),
        "domain expand failed: {}",
        String::from_utf8_lossy(&output.stderr)
    );
    String::from_utf8(output.stdout).expect("expanded Kernel is UTF-8")
}

struct TemporarySpec(PathBuf);

impl TemporarySpec {
    fn from_source(label: &str, source: &str) -> Self {
        let directory = root().join("rust/target/issue_640");
        fs::create_dir_all(&directory).expect("create in-worktree test directory");
        let path = directory.join(format!("{label}-{}.fsl", std::process::id()));
        fs::write(&path, source).expect("write in-worktree test specification");
        Self(path)
    }

    fn with_reachable(label: &str, declaration: &str) -> Self {
        let expanded = expand_fixture();
        let closing = expanded
            .rfind('}')
            .expect("expanded Kernel has a closing brace");
        let source = format!(
            "{}  {declaration}\n{}",
            &expanded[..closing],
            &expanded[closing..]
        );
        Self::from_source(label, &source)
    }

    fn verify(&self, depth: &str) -> (Value, i32) {
        let output = Command::new(env!("CARGO_BIN_EXE_fslc"))
            .arg("verify")
            .arg(&self.0)
            .args(["--depth", depth, "--no-cache"])
            .current_dir(root())
            .output()
            .expect("verify expanded Kernel");
        let value = serde_json::from_slice(&output.stdout).unwrap_or_else(|error| {
            panic!(
                "invalid verify JSON: {error}; stderr={}",
                String::from_utf8_lossy(&output.stderr)
            )
        });
        (value, output.status.code().expect("fslc exit status"))
    }
}

impl Drop for TemporarySpec {
    fn drop(&mut self) {
        fs::remove_file(&self.0).expect("remove in-worktree test specification");
    }
}

fn assert_action_registries_match(path: &Path) -> BTreeSet<String> {
    let spec = path.to_str().expect("UTF-8 specification path");
    let check_args = ["domain", "check", spec, "--depth", "1"];
    let output = run(&check_args);
    let check = json(&output, &check_args);
    assert_eq!(output.status.code(), Some(0), "{check:#}");

    let generated = check["generated_actions"]
        .as_array()
        .expect("generated_actions")
        .iter()
        .map(|name| name.as_str().expect("action name").to_owned())
        .collect::<BTreeSet<_>>();

    let expand_args = ["domain", "expand", spec];
    let output = run(&expand_args);
    assert_eq!(
        output.status.code(),
        Some(0),
        "domain expand failed: {}",
        String::from_utf8_lossy(&output.stderr)
    );
    let expanded = String::from_utf8(output.stdout).expect("expanded Kernel is UTF-8");
    let kernel = fsl_core::parse_kernel_source(&expanded, &fsl_core::FsResolver::new("."))
        .expect("parse expanded Kernel");
    let model = fsl_core::build_model(kernel).expect("build expanded Kernel");
    let actual = model
        .actions
        .iter()
        .map(|action| action.name.clone())
        .collect::<BTreeSet<_>>();

    assert_eq!(generated, actual);
    generated
}

#[test]
fn paid_is_not_reachable_without_requesting_the_effect() {
    let spec = TemporarySpec::with_reachable(
        "paid-without-request",
        "reachable ProbePaidNoRequest { order_status == Status_Paid }",
    );
    let (verify, status) = spec.verify("1");

    assert_eq!(status, 1, "{verify:#}");
    assert_eq!(verify["result"], "reachable_failed", "{verify:#}");
    assert!(
        verify["unreached"].as_array().is_some_and(|unreached| {
            unreached
                .iter()
                .any(|property| property["name"] == "ProbePaidNoRequest")
        }),
        "{verify:#}"
    );
}

#[test]
fn paid_remains_reachable_through_request_and_correlated_completion() {
    let spec = TemporarySpec::with_reachable(
        "paid-via-request",
        "reachable ProbePaidViaRequest { order_status == Status_Paid }",
    );
    let (verify, status) = spec.verify("3");

    assert_eq!(status, 0, "{verify:#}");
    assert_eq!(verify["result"], "verified", "{verify:#}");
    let witness = &verify["reachables"]["ProbePaidViaRequest"];
    assert_eq!(witness["witnessed_at_step"], 3, "{verify:#}");
    let trace = witness["witness"].as_array().expect("reachable witness");
    let actions = trace
        .iter()
        .filter_map(|step| step["action"]["name"].as_str())
        .collect::<Vec<_>>();
    assert_eq!(
        actions,
        [
            "order_approve",
            "order_request_payment",
            "capture_payment_complete_payment_captured",
        ],
        "{verify:#}"
    );
    let final_state = &trace.last().expect("final witness step")["state"];
    assert_eq!(final_state["order_status"], "Status_Paid", "{verify:#}");
    assert!(
        final_state["capture_payment_status"]
            .as_object()
            .is_some_and(|statuses| statuses
                .values()
                .any(|status| status == "CapturePaymentEffectStatus_Succeeded")),
        "{verify:#}"
    );
}

#[test]
fn domain_check_generated_actions_bidirectionally_match_expanded_kernel() {
    assert_action_registries_match(Path::new(FIXTURE));

    let fixture = fs::read_to_string(root().join(FIXTURE)).expect("read domain fixture");
    let source = fixture.replacen(
        "    invariant terminalOutcome {",
        "    compensation {\n      when Approved after PaymentRequested {\n        emits Approved\n      }\n    }\n    invariant terminalOutcome {",
        1,
    );
    assert_ne!(source, fixture, "insert compensation-only observations");
    let compensation = TemporarySpec::from_source("compensation-observations", &source);
    let actions = assert_action_registries_match(&compensation.0);
    assert!(actions.contains("saga_payment_flow_observe_approved"));
    assert!(actions.contains("saga_payment_flow_observe_payment_requested"));
}
