// SPDX-License-Identifier: Apache-2.0
// Copyright 2026 Ryoichi Izumita

//! The documented `business` `policy ID "text" invariant { … }` examples are
//! falsifiable (#1784).
//!
//! Every stage of a pure stage graph is reachable, each case independently,
//! so an invariant over `stage(c)` alone either always holds or is violated;
//! the design note's earlier example (`… => true`) constrained nothing. Each
//! documented example now reads the history flag a no-bypass policy
//! synthesizes (`return_stage_via_Approved`) to say that an approval is
//! final. The contract: the checked example verifies and proves, a
//! transition `Approved -> Rejected` violates exactly that policy, and the
//! flag does not exist without its no-bypass policy.

use std::path::{Path, PathBuf};
use std::process::Command;

use serde_json::Value;

/// The documents whose one `fsl check` fence is a business spec with a
/// `policy … invariant { }` reading a history flag, and that policy's id.
const EXAMPLES: [(&str, &str); 2] = [
    ("skills/fsl-business/SKILL.md", "POL-RET-003"),
    ("docs/design/DESIGN-dialects.md", "PAY-3"),
];

fn repository_root() -> PathBuf {
    Path::new(env!("CARGO_MANIFEST_DIR"))
        .parent()
        .and_then(Path::parent)
        .expect("repository root")
        .to_path_buf()
}

fn checked_example(document: &str) -> String {
    let text = std::fs::read_to_string(repository_root().join(document))
        .unwrap_or_else(|error| panic!("{document}: {error}"));
    let fences = text.split("```fsl check\n").skip(1).collect::<Vec<_>>();
    assert_eq!(fences.len(), 1, "{document}: one checked fence");
    fences[0]
        .split("```\n")
        .next()
        .expect("closed fence")
        .to_owned()
}

fn scratch(label: &str, source: &str) -> PathBuf {
    let directory = Path::new(env!("CARGO_TARGET_TMPDIR"))
        .join("issue_1784")
        .join(label);
    let _ = std::fs::remove_dir_all(&directory);
    std::fs::create_dir_all(&directory).expect("create scratch directory");
    std::fs::write(directory.join("spec.fsl"), source).expect("write spec");
    directory
}

fn label(document: &str, variant: &str) -> String {
    format!("{}-{variant}", document.replace(['/', '.'], "-"))
}

fn fslc(directory: &Path, args: &[&str]) -> (Value, i32) {
    let output = Command::new(env!("CARGO_BIN_EXE_fslc"))
        .args(args)
        .current_dir(directory)
        .output()
        .expect("run native fslc");
    let value = serde_json::from_slice(&output.stdout).unwrap_or_else(|error| {
        panic!(
            "{args:?}: invalid JSON: {error}; stderr={}",
            String::from_utf8_lossy(&output.stderr)
        )
    });
    (value, output.status.code().expect("native exit status"))
}

/// The example with a transition from `Approved` to `Rejected`: an approval
/// that is not final.
fn with_revoke(source: &str) -> String {
    let refund = source
        .lines()
        .find(|line| line.trim_start().starts_with("transition refund "))
        .expect("refund transition");
    let indent = &refund[..refund.len() - refund.trim_start().len()];
    source.replacen(
        refund,
        &format!("{refund}\n{indent}transition revoke Approved -> Rejected by Manager"),
        1,
    )
}

/// The example without its no-bypass policy, the one that synthesizes the
/// history flag.
fn without_no_bypass_policy(source: &str) -> String {
    let lines = source.lines().collect::<Vec<_>>();
    let end = lines
        .iter()
        .position(|line| line.contains("must have passed through"))
        .expect("no-bypass policy");
    let start = (0..end)
        .rev()
        .find(|&index| lines[index].trim_start().starts_with("policy "))
        .expect("its policy line");
    let mut kept = lines[..start].to_vec();
    kept.extend(&lines[end + 1..]);
    kept.join("\n") + "\n"
}

#[test]
fn the_documented_invariant_policy_holds_and_a_revoke_transition_violates_it() {
    for (document, policy) in EXAMPLES {
        let source = checked_example(document);
        assert!(
            source.contains(&format!("policy {policy} ")),
            "{document}: {source}"
        );

        let directory = scratch(&label(document, "as-written"), &source);
        let (verified, status) = fslc(&directory, &["verify", "spec.fsl", "--depth", "8"]);
        assert_eq!(
            (verified["result"].as_str(), status),
            (Some("verified"), 0),
            "{document}: {verified}"
        );
        let (proved, status) = fslc(&directory, &["verify", "spec.fsl", "--engine", "induction"]);
        assert_eq!(
            (proved["result"].as_str(), status),
            (Some("proved"), 0),
            "{document}: {proved}"
        );
        assert!(
            proved["k_used"].get(policy).is_some(),
            "{document}: {policy} was not proved: {proved}"
        );

        let mutant = scratch(&label(document, "revoke"), &with_revoke(&source));
        let (violated, status) = fslc(&mutant, &["verify", "spec.fsl", "--depth", "8"]);
        assert_eq!(
            (violated["result"].as_str(), status),
            (Some("violated"), 1),
            "{document}: {violated}"
        );
        assert_eq!(
            violated["requirement"]["id"], policy,
            "{document}: {violated}"
        );
    }
}

#[test]
fn the_history_flag_exists_only_with_its_no_bypass_policy() {
    for (document, _) in EXAMPLES {
        let source = without_no_bypass_policy(&checked_example(document));
        assert!(!source.contains("must have passed through"), "{source}");
        let directory = scratch(&label(document, "no-flag"), &source);
        let (checked, status) = fslc(&directory, &["check", "spec.fsl"]);
        assert_eq!(
            (checked["result"].as_str(), status),
            (Some("error"), 2),
            "{document}: {checked}"
        );
        assert!(
            checked["message"]
                .as_str()
                .is_some_and(|message| message.contains("return_stage_via_Approved")),
            "{document}: {checked}"
        );
    }
}
