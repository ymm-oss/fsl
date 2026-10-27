// SPDX-License-Identifier: Apache-2.0

//! #1459 R2: action-local relation guarantees capture the old binding before
//! shadowing, survive later rebinding of an alias source, and are replaceable.

use std::path::Path;
use std::process::Command;

use serde_json::{Value, json};

const SELF_HINT: &str = "hint: use a relation with the same declared source and target type";

fn source(bindings: &str, name: &str, operand: &str) -> String {
    let predicate = match name {
        "acyclic" => format!("acyclic({operand})"),
        "reachable" => format!("not reachable({operand}, 0, 1)"),
        _ => panic!("unknown self-relation function {name}"),
    };
    format!(
        r"spec LetShadowing {{
  type N = 0..2
  type M = 0..2
  state {{ r: relation N -> N, q: relation N -> M, flag: Bool }}
  init {{ r = Set {{}} q = Set {{}} flag = false }}
  action stay() {{
{bindings}
    ensures {predicate}
  }}
  invariant P {{ true }}
}}
"
    )
}

fn run(label: &str, text: &str) -> [(&'static str, Value, i32); 3] {
    let root = Path::new(env!("CARGO_MANIFEST_DIR"))
        .parent()
        .and_then(Path::parent)
        .expect("repository root");
    let directory = root.join(".codex/tmp/issue_1459_let_shadowing").join(label);
    std::fs::create_dir_all(&directory).expect("create owned shadowing fixture directory");
    std::fs::write(directory.join("shadowing.fsl"), text).expect("write shadowing fixture");
    let outputs = ["check", "bmc", "explicit"].map(|mode| {
        let mut command = Command::new(env!("CARGO_BIN_EXE_fslc"));
        command.args([
            if mode == "check" { "check" } else { "verify" },
            "shadowing.fsl",
        ]);
        if mode != "check" {
            command.args(["--depth", "2", "--engine", mode, "--no-cache"]);
        }
        let output = command
            .current_dir(&directory)
            .env_remove(fslc_rust::unstable_gate::ENVIRONMENT_VARIABLE)
            .env("FSLC_CACHE", "off")
            .output()
            .expect("run shadowing regression");
        assert!(output.stderr.is_empty(), "{label} {mode}: {output:?}");
        let value = serde_json::from_slice(&output.stdout)
            .unwrap_or_else(|error| panic!("{label} {mode}: invalid JSON {error}: {output:?}"));
        (
            mode,
            value,
            output.status.code().expect("native process exit"),
        )
    });
    std::fs::write(
        directory.join("observations.json"),
        serde_json::to_vec_pretty(&outputs).expect("serialize shadowing observations"),
    )
    .expect("retain observations without deleting fixtures");
    outputs
}

fn reject(label: &str, bindings: &str, name: &str, operand: &str) {
    let outputs = run(label, &source(bindings, name, operand));
    for (mode, value, status) in &outputs {
        assert_eq!(
            (*status, value["result"].as_str(), value["kind"].as_str()),
            (2, Some("error"), Some("semantics")),
            "{label} {mode}: {outputs:#?}"
        );
        let message = value["message"].as_str().expect("self-relation diagnostic");
        assert!(
            message.contains(&format!(
                "{name}() requires a self-relation (relation T -> T)"
            )) && message.contains(SELF_HINT),
            "{label} {mode}: {outputs:#?}"
        );
        assert_eq!(
            value["loc"],
            json!({"line": 6, "column": 3}),
            "{label} {mode}: {outputs:#?}"
        );
    }
}

fn replacement_passes(label: &str, name: &str) {
    let text = source(
        "    let selected = if flag then r else q\n    let selected = r",
        name,
        "selected",
    );
    let outputs = run(label, &text);
    for (mode, value, status) in &outputs {
        assert_eq!(*status, 0, "{label} {mode}: {outputs:#?}");
        assert!(
            value.get("kind").is_none() && value.get("message").is_none(),
            "{label}: {outputs:#?}"
        );
        match *mode {
            "check" => assert_eq!(value["result"], "ok", "{label}: {outputs:#?}"),
            "bmc" => {
                assert_eq!(value["result"], "verified", "{label}: {outputs:#?}");
                assert_eq!(value["completeness"], "bounded", "{label}: {outputs:#?}");
                assert_eq!(value["checked_to_depth"], 2, "{label}: {outputs:#?}");
            }
            "explicit" => {
                assert_eq!(value["result"], "proved", "{label}: {outputs:#?}");
                assert_eq!(value["completeness"], "unbounded", "{label}: {outputs:#?}");
                assert_eq!(value["closure"], true, "{label}: {outputs:#?}");
                assert_eq!(value["states_explored"], 1, "{label}: {outputs:#?}");
            }
            _ => panic!("unknown mode {mode}"),
        }
    }
}

#[test]
fn acyclic_rejects_let_shadowing_self_state_relation() {
    reject(
        "shadow-r-acyclic",
        "    let r = if flag then r else q",
        "acyclic",
        "r",
    );
}

#[test]
fn reachable_rejects_let_shadowing_self_state_relation() {
    reject(
        "shadow-r-reachable",
        "    let r = if flag then r else q",
        "reachable",
        "r",
    );
}

#[test]
fn acyclic_alias_capture_survives_rebinding_its_source() {
    reject(
        "capture-acyclic",
        "    let selected = if flag then r else q\n    let chosen = selected\n    let selected = r",
        "acyclic",
        "chosen",
    );
}

#[test]
fn reachable_alias_capture_survives_rebinding_its_source() {
    reject(
        "capture-reachable",
        "    let selected = if flag then r else q\n    let chosen = selected\n    let selected = r",
        "reachable",
        "chosen",
    );
}

#[test]
fn acyclic_rebinding_to_self_relation_replaces_bad_guarantee() {
    replacement_passes("replace-acyclic", "acyclic");
}

#[test]
fn reachable_rebinding_to_self_relation_replaces_bad_guarantee() {
    replacement_passes("replace-reachable", "reachable");
}

#[test]
fn acyclic_rejects_let_shadowing_nonself_state_relation() {
    reject(
        "shadow-q-acyclic",
        "    let q = if flag then r else q",
        "acyclic",
        "q",
    );
}

#[test]
fn reachable_rejects_let_shadowing_nonself_state_relation() {
    reject(
        "shadow-q-reachable",
        "    let q = if flag then r else q",
        "reachable",
        "q",
    );
}

#[test]
fn acyclic_rejects_scalar_let_rebound_to_nonself_conditional() {
    reject(
        "scalar-acyclic",
        "    let selected = 0\n    let selected = if flag then r else q",
        "acyclic",
        "selected",
    );
}

#[test]
fn reachable_rejects_scalar_let_rebound_to_nonself_conditional() {
    reject(
        "scalar-reachable",
        "    let selected = 0\n    let selected = if flag then r else q",
        "reachable",
        "selected",
    );
}
