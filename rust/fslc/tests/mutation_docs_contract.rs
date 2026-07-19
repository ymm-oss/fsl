// SPDX-License-Identifier: Apache-2.0
// Copyright 2026 Ryoichi Izumita

//! Documentation-consistency regression for the mutation score's epistemic
//! claims (issue #338). `fslc mutate`'s kill rate is bounded mutant-set
//! sensitivity — `killed / (killed + survived)` over a selected finite mutant
//! set, depth, and oracle. Earlier documentation overstated a surviving
//! mutant as "behavior constrained by no property = a missing invariant";
//! these tests keep that stronger claim from returning and keep the
//! calibrated definition present in every mutation-facing document.

use std::path::{Path, PathBuf};

fn workspace_root() -> PathBuf {
    Path::new(env!("CARGO_MANIFEST_DIR"))
        .parent()
        .and_then(Path::parent)
        .expect("workspace root")
        .to_path_buf()
}

fn read(relative: &str) -> String {
    let path = workspace_root().join(relative);
    std::fs::read_to_string(&path).unwrap_or_else(|error| panic!("read {relative}: {error}"))
}

/// Collapse all whitespace runs to single spaces so hard-wrapped prose is
/// matched by the same substring as single-line prose.
fn normalized(relative: &str) -> String {
    read(relative)
        .split_whitespace()
        .collect::<Vec<_>>()
        .join(" ")
}

/// Every document that explains the mutation score must state the exact
/// denominator convention so `invalid` exclusion stays a documented contract.
#[test]
fn kill_rate_definition_is_stated_in_mutation_docs() {
    for relative in [
        "docs/DESIGN-mutate.md",
        "docs/LANGUAGE.md",
        "skills/fsl/reference.md",
    ] {
        let text = normalized(relative);
        assert!(
            text.contains("killed / (killed + survived)"),
            "{relative} must define kill rate as killed / (killed + survived)"
        );
    }
}

/// The calibrated framing ("bounded mutant-set sensitivity", survivors as a
/// review queue that includes equivalent mutants) must stay present where the
/// score is explained to users and agents.
#[test]
fn mutation_score_is_framed_as_bounded_sensitivity() {
    for relative in [
        "docs/DESIGN-mutate.md",
        "docs/LANGUAGE.md",
        "skills/fsl/reference.md",
    ] {
        let text = normalized(relative);
        assert!(
            text.contains("bounded mutant-set sensitivity"),
            "{relative} must frame the score as bounded mutant-set sensitivity"
        );
        assert!(
            text.contains("equivalent mutant"),
            "{relative} must name equivalent mutants as a survivor explanation"
        );
    }
}

/// The overstated survivor claim ("constrained by no property", i.e. survivor
/// = missing invariant) must not return to any mutation-facing document.
#[test]
fn stronger_survivor_claim_does_not_return() {
    for relative in [
        "docs/DESIGN-mutate.md",
        "docs/LANGUAGE.md",
        "docs/LANGUAGE.ja.md",
        "skills/fsl/reference.md",
        "README.md",
    ] {
        // Normalize hard-wrapped prose so a reflowed reintroduction of the
        // claim cannot slip past a literal substring check.
        let text: String = read(relative)
            .chars()
            .filter(|character| !character.is_whitespace())
            .collect();
        for banned in [
            "constrainedbynoproperty",
            "どのプロパティにも制約されない振る舞い",
        ] {
            assert!(
                !text.contains(banned),
                "{relative} reintroduces the overstated survivor claim: {banned:?}"
            );
        }
    }
}
