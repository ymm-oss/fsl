// SPDX-License-Identifier: Apache-2.0
// Copyright 2026 Ryoichi Izumita

//! Documentation-consistency regression for the causal profile's epistemic
//! boundary (issue #321 acceptance criterion 15). The agent-facing reference
//! must state the review-only rule — causal claims and expectation results
//! are never described as `proved`/`verified` causality — and the language
//! reference must document the profile in both canonical languages.

use std::path::{Path, PathBuf};

fn workspace_root() -> PathBuf {
    Path::new(env!("CARGO_MANIFEST_DIR"))
        .parent()
        .and_then(Path::parent)
        .expect("workspace root")
        .to_path_buf()
}

fn normalized(relative: &str) -> String {
    std::fs::read_to_string(workspace_root().join(relative))
        .unwrap_or_else(|error| panic!("read {relative}: {error}"))
        .split_whitespace()
        .collect::<Vec<_>>()
        .join(" ")
}

#[test]
fn skill_reference_states_the_review_only_hard_rule() {
    let text = normalized("skills/fsl/reference.md");
    for required in [
        "never describe a causal claim, causal model, or expectation result as `proved`, `verified`, or otherwise formally established real-world causality",
        "`formal_assurance` (what the verifier checked) and `causal_support` (what external evidence says) are two separate axes",
        "There is deliberately no `fslc causal verify` command",
        "decline that framing",
    ] {
        let required = required.split_whitespace().collect::<Vec<_>>().join(" ");
        assert!(
            text.contains(&required),
            "skills/fsl/reference.md must contain: {required}"
        );
    }
}

#[test]
fn language_references_document_the_profile_in_both_languages() {
    for relative in ["docs/LANGUAGE.md", "docs/LANGUAGE.ja.md"] {
        let text = normalized(relative);
        assert!(
            text.contains("fslc causal check"),
            "{relative} must document the causal CLI"
        );
        assert!(
            text.contains("do_not_assume"),
            "{relative} must document the do_not_assume contract"
        );
    }
    let english = normalized("docs/LANGUAGE.md");
    assert!(english.contains("FSL never proves real-world causality"));
    let japanese = normalized("docs/LANGUAGE.ja.md");
    assert!(japanese.contains("FSL は現実世界の因果関係を証明しません"));
}
