// SPDX-License-Identifier: Apache-2.0
// Copyright 2026 Ryoichi Izumita

//! fsl 5 plan §3 S5, through the native CLI: the English / Japanese keyword
//! table and the particles が・を・に・で・は.
//!
//! Acceptance 1 (K: `fsl5_en.fsl`, `fsl5_ja_particles.fsl` and
//! `fsl5_ja_full.fsl` lower to the one `lowered.fsl`) lives in
//! `fsl5_lowering_contract.rs`, acceptance 2 (the table itself) in
//! `fsl-syntax/src/keywords.rs`. This file holds acceptance 3: one failing
//! document per particle diagnostic, each a one-line change of
//! `fsl5/e2e/fsl5_ja_full.fsl`, failing `check` with its code at the text it
//! names, including a phrase that marks some words and not others
//! (decisions/048 J9).

use std::path::{Path, PathBuf};
use std::process::Command;

use serde_json::Value;

fn e2e() -> PathBuf {
    Path::new(env!("CARGO_MANIFEST_DIR")).join("tests/fixtures/fsl5/e2e")
}

/// `(stdout as JSON, exit)` of `fslc check <file>` in `directory`, with the
/// `fsl5` gate set when `gate`.
fn check(directory: &Path, file: &str, gate: bool) -> (Value, i32) {
    let mut command = Command::new(env!("CARGO_BIN_EXE_fslc"));
    command
        .args(["check", file])
        .current_dir(directory)
        .env_remove(fslc_rust::unstable_gate::ENVIRONMENT_VARIABLE);
    if gate {
        command.env(fslc_rust::unstable_gate::ENVIRONMENT_VARIABLE, "fsl5");
    }
    let output = command.output().expect("run fslc check");
    let value = serde_json::from_slice(&output.stdout).unwrap_or_else(|error| {
        panic!(
            "check {file}: invalid JSON ({error}): {}{}",
            String::from_utf8_lossy(&output.stdout),
            String::from_utf8_lossy(&output.stderr)
        )
    });
    (value, output.status.code().expect("exit code"))
}

/// A scratch directory holding `1_business.fsl` (the fixtures' `implements`
/// dependency) and `source` as `doc.fsl`.
fn scratch(label: &str, source: &str) -> PathBuf {
    let directory = Path::new(env!("CARGO_TARGET_TMPDIR"))
        .join("fsl5_keywords")
        .join(label);
    let _ = std::fs::remove_dir_all(&directory);
    std::fs::create_dir_all(&directory).expect("create scratch directory");
    std::fs::copy(
        e2e().join("1_business.fsl"),
        directory.join("1_business.fsl"),
    )
    .expect("copy the business layer");
    std::fs::write(directory.join("doc.fsl"), source).expect("write scratch source");
    directory
}

/// `(line, column)` of the `occurrence`-th match of `at`, as diagnostics
/// count them (1-based, Unicode scalar columns).
fn position(source: &str, at: &str, occurrence: usize) -> (u64, u64) {
    let (offset, _) = source
        .match_indices(at)
        .nth(occurrence)
        .unwrap_or_else(|| panic!("'{at}' #{occurrence} in the broken source"));
    let before = &source[..offset];
    let line = before.matches('\n').count() + 1;
    let column = before
        .rsplit('\n')
        .next()
        .expect("line prefix")
        .chars()
        .count()
        + 1;
    (line as u64, column as u64)
}

/// `fsl5_ja_full.fsl` with `from` replaced by `to` (exactly once in the
/// fixture) must fail `check` with `code` at the `occurrence`-th match of
/// `at` in the changed source.
struct Broken {
    label: &'static str,
    code: &'static str,
    from: &'static str,
    to: &'static str,
    at: &'static str,
    occurrence: usize,
}

const BROKEN: &[Broken] = &[
    // Plan S5 acceptance 3: the actor marked を and the entity marked が.
    Broken {
        label: "event-roles-swapped",
        code: "particle_role_mismatch",
        from: "submit       = 従業員 が 申請 を 提出する",
        to: "submit       = 従業員 を 申請 が 提出する",
        at: "従業員 を",
        occurrence: 0,
    },
    Broken {
        label: "rule-roles-swapped",
        code: "particle_role_mismatch",
        from: "許可 従業員 が 申請 を 提出する",
        to: "許可 従業員 を 申請 が 提出する",
        at: "従業員 を",
        occurrence: 0,
    },
    Broken {
        label: "step-roles-swapped",
        code: "particle_role_mismatch",
        from: "    従業員 が 申請1 を 申請金額 1 で 提出する\n",
        to: "    申請1 が 従業員 を 申請金額 1 で 提出する\n",
        at: "従業員 を",
        occurrence: 0,
    },
    // A decision table row names its actor and verbs; the table names the
    // entity, so nothing in a row takes を.
    Broken {
        label: "row-object",
        code: "particle_role_mismatch",
        from: "少額申請 => システム が 承認する",
        to: "少額申請 => システム を 承認する",
        at: "システム を",
        occurrence: 0,
    },
    // No phrase of this edition has a counterpart (に).
    Broken {
        label: "event-counterpart",
        code: "particle_role_mismatch",
        from: "pay          = 経理 が 申請 を 支払う",
        to: "pay          = 経理 が 申請 に 支払う",
        at: "申請 に",
        occurrence: 0,
    },
    // Plan S5 acceptance 3: particles written onto the words they mark.
    Broken {
        label: "event-no-space",
        code: "particle_needs_space",
        from: "submit       = 従業員 が 申請 を 提出する",
        to: "submit       = 従業員が 申請を 提出する",
        at: "従業員が",
        occurrence: 0,
    },
    Broken {
        label: "step-no-space",
        code: "particle_needs_space",
        from: "    システム が 申請1 を 承認する\n",
        to: "    システム が 申請1を 承認する\n",
        at: "申請1を",
        occurrence: 0,
    },
    // J9: with particles every word but the verb takes one. The entity
    // without を leaves two words without a particle.
    Broken {
        label: "event-mixed",
        code: "mixed_particles",
        from: "pay          = 経理 が 申請 を 支払う",
        to: "pay          = 経理 が 申請 支払う",
        at: "支払う",
        occurrence: 1,
    },
    Broken {
        label: "rule-mixed",
        code: "mixed_particles",
        from: "許可 従業員 が 申請 を 提出する",
        to: "許可 従業員 申請 を 提出する",
        at: "提出する 条件",
        occurrence: 0,
    },
    Broken {
        label: "step-mixed",
        code: "mixed_particles",
        from: "    経理 が 申請1 を 支払う\n",
        to: "    経理 が 申請1 支払う\n",
        at: "支払う",
        occurrence: 2,
    },
];

#[test]
fn each_particle_diagnostic_fails_check_at_its_text() {
    let original = std::fs::read_to_string(e2e().join("fsl5_ja_full.fsl")).expect("fixture");
    for broken in BROKEN {
        assert_eq!(
            original.matches(broken.from).count(),
            1,
            "{}: {}",
            broken.label,
            broken.from
        );
        let text = original.replacen(broken.from, broken.to, 1);
        let directory = scratch(broken.label, &text);
        let (output, exit) = check(&directory, "doc.fsl", true);
        assert_eq!(exit, 2, "{}: {output:#}", broken.label);
        assert_eq!(
            output["diagnostic_code"], broken.code,
            "{}: {output:#}",
            broken.label
        );
        let (line, column) = position(&text, broken.at, broken.occurrence);
        assert_eq!(
            (
                output["loc"]["line"].as_u64(),
                output["loc"]["column"].as_u64()
            ),
            (Some(line), Some(column)),
            "{}: {output:#}",
            broken.label
        );
    }
}

/// The negatives above start from a document that checks: each fails for
/// its own change only.
#[test]
fn the_negative_examples_start_from_a_document_that_checks() {
    let original = std::fs::read_to_string(e2e().join("fsl5_ja_full.fsl")).expect("fixture");
    let directory = scratch("clean", &original);
    let (output, exit) = check(&directory, "doc.fsl", true);
    assert_eq!(
        (output["result"].as_str(), exit),
        (Some("ok"), 0),
        "{output:#}"
    );
}

/// The table is read in next-edition requirements only: behind the gate, a
/// `spec` headed `fsl 5` does not read `定数` as `const`, and without the
/// gate a requirements document in Japanese keywords is not read.
#[test]
fn japanese_keywords_are_read_in_next_edition_requirements_only() {
    let spec = "fsl 5\nspec S {\n  定数 N = 1\n  state { x: Int }\n  init { x = 0 }\n}\n";
    let directory = scratch("spec", spec);
    let (output, exit) = check(&directory, "doc.fsl", true);
    assert_eq!(exit, 2, "{output:#}");
    assert_eq!(output["diagnostic_code"], "FSL-PARSE", "{output:#}");
    assert_eq!(
        (
            output["loc"]["line"].as_u64(),
            output["loc"]["column"].as_u64()
        ),
        (Some(3), Some(3)),
        "{output:#}"
    );

    let original = std::fs::read_to_string(e2e().join("fsl5_ja_full.fsl")).expect("fixture");
    let directory = scratch("ungated", &original);
    let (output, exit) = check(&directory, "doc.fsl", false);
    assert_eq!(
        (output["result"].as_str(), exit),
        (Some("error"), 2),
        "{output:#}"
    );
}
