// SPDX-License-Identifier: Apache-2.0
// Copyright 2026 Ryoichi Izumita

//! Drift control for #1285 stage 1: acceptance / forbidden traces reach a
//! consumer only through the requirement-trace gate, or through a read that
//! names itself unchecked.
//!
//! The mechanism is the type: `RequirementsTraceContract`'s fields are private
//! (its cases are readable only through `unchecked_acceptance` /
//! `unchecked_forbidden`), and `CheckedTraceContract` has no constructor
//! outside `validate_requirement_trace_source` (both pinned by `compile_fail`
//! doc tests). This census pins where the unchecked entry points are used, so
//! a new ungated consumer fails here and has to record why it skips the gate.
//! It counts tokens in source text, so a call through an alias is not seen.

use std::collections::BTreeMap;
use std::path::{Path, PathBuf};

const PARSE: &str = "requirements_trace_contract";
const UNCHECKED: [&str; 2] = ["unchecked_acceptance", "unchecked_forbidden"];

/// `(workspace-relative file, entry, count, reason)`. `entry` is [`PARSE`] or
/// `"unchecked"` (either `UNCHECKED` accessor).
const INVENTORY: &[(&str, &str, usize, &str)] = &[
    (
        "fsl-core/src/diagnostics.rs",
        PARSE,
        1,
        "no_user_invariants suppression: any forbidden suppresses the warning; check reports a rejected one separately",
    ),
    (
        "fsl-core/src/diagnostics.rs",
        "unchecked",
        1,
        "no_user_invariants suppression reads whether a forbidden exists",
    ),
    (
        "fsl-tools/src/document_project.rs",
        PARSE,
        1,
        "document claims (#1243): fsl-tools does not depend on fsl-runtime, which the gate replays on",
    ),
    (
        "fsl-tools/src/document_project.rs",
        "unchecked",
        4,
        "document claims (#1243): one claim per declared case",
    ),
    (
        "fsl-tools/src/document_render.rs",
        PARSE,
        1,
        "document render (#1243): fsl-tools does not depend on fsl-runtime, which the gate replays on",
    ),
    (
        "fsl-tools/src/document_render.rs",
        "unchecked",
        2,
        "document render (#1243): finds the case a claim renders",
    ),
    (
        "fslc/src/main.rs",
        PARSE,
        2,
        "analyze project graph (#1243), diff OLD forbidden replay (#1239)",
    ),
    (
        "fslc/src/main.rs",
        "unchecked",
        6,
        "the two ungated consumers above, and mutate attributing a gate rejection to its case's requirements",
    ),
    (
        "fslc/src/strict_tags.rs",
        PARSE,
        1,
        "check --strict-tags referenced ids",
    ),
    (
        "fslc/src/strict_tags.rs",
        "unchecked",
        2,
        "the --strict-tags consumer above",
    ),
    (
        "fslc/src/verification_output.rs",
        PARSE,
        2,
        "the gate itself: validate_requirement_trace_source and its scoped form",
    ),
    (
        "fslc/src/verification_output.rs",
        "unchecked",
        5,
        "the gate replaying the cases, and CheckedTraceContract's accessors",
    ),
];

fn workspace() -> PathBuf {
    Path::new(env!("CARGO_MANIFEST_DIR"))
        .parent()
        .expect("rust workspace directory")
        .to_path_buf()
}

/// Every product (`src/`) Rust file of every workspace crate.
fn product_sources() -> Vec<PathBuf> {
    let mut files = Vec::new();
    for entry in std::fs::read_dir(workspace()).expect("read workspace") {
        let source = entry.expect("workspace entry").path().join("src");
        if !source.is_dir() {
            continue;
        }
        let mut stack = vec![source];
        while let Some(directory) = stack.pop() {
            for entry in std::fs::read_dir(&directory)
                .unwrap_or_else(|error| panic!("read {}: {error}", directory.display()))
            {
                let path = entry.expect("directory entry").path();
                if path.is_dir() {
                    stack.push(path);
                } else if path.extension().is_some_and(|extension| extension == "rs") {
                    files.push(path);
                }
            }
        }
    }
    files.sort();
    files
}

/// The text before the first `#[cfg(test)]`, with comments removed line by
/// line, so a test module or a comment naming an entry does not count.
fn product_text(path: &Path) -> String {
    let text = std::fs::read_to_string(path)
        .expect("read source")
        .replace("\r\n", "\n");
    let product = text.split("#[cfg(test)]").next().expect("product body");
    product
        .lines()
        .map(|line| line.split("//").next().unwrap_or_default())
        .collect::<Vec<_>>()
        .join("\n")
}

/// Calls of `name`: the identifier followed by `(`, not preceded by another
/// identifier character or by `fn `.
fn calls(text: &str, name: &str) -> usize {
    let needle = format!("{name}(");
    text.match_indices(&needle)
        .filter(|(at, _)| {
            let before = &text[..*at];
            !before.ends_with(|character: char| character.is_alphanumeric() || character == '_')
                && !before.ends_with("fn ")
        })
        .count()
}

fn census() -> BTreeMap<(String, &'static str), usize> {
    let root = workspace();
    let mut found = BTreeMap::new();
    for path in product_sources() {
        let text = product_text(&path);
        let relative = path
            .strip_prefix(&root)
            .expect("source under workspace")
            .to_string_lossy()
            .replace('\\', "/");
        let parses = calls(&text, PARSE);
        if parses > 0 {
            found.insert((relative.clone(), PARSE), parses);
        }
        let unchecked = UNCHECKED.iter().map(|name| calls(&text, name)).sum();
        if unchecked > 0 {
            found.insert((relative, "unchecked"), unchecked);
        }
    }
    found
}

#[test]
fn every_ungated_trace_read_is_inventoried() {
    let expected = INVENTORY
        .iter()
        .map(|(file, entry, count, reason)| {
            assert!(!reason.is_empty(), "{file} {entry} must state its reason");
            (((*file).to_owned(), *entry), *count)
        })
        .collect::<BTreeMap<_, _>>();
    assert_eq!(
        census(),
        expected,
        "an unchecked trace entry point gained or lost a call site. A consumer \
         that needs acceptance / forbidden cases takes a CheckedTraceContract \
         from validate_requirement_trace_source; one that must skip the gate \
         needs an INVENTORY row stating why"
    );
}

/// Explain, html, and ledger embed the scenarios envelope even when it is a
/// gate failure (#1242). That discard stays in one named helper instead of a
/// `let (scenarios, _) = run_scenarios_mode_from_source(..)` at each caller.
#[test]
fn the_scenarios_status_is_discarded_only_by_name() {
    let text = product_text(&workspace().join("fslc/src/main.rs"));
    let compact = text.split_whitespace().collect::<String>();
    assert_eq!(
        compact
            .matches(",_)=run_scenarios_mode_from_source(")
            .count(),
        0,
        "a caller drops run_scenarios_mode_from_source's status anonymously; \
         use scenarios_discarding_gate_status"
    );
    assert_eq!(
        compact
            .matches("let(scenarios,_discarded_status)=run_scenarios_mode_from_source(")
            .count(),
        1,
        "scenarios_discarding_gate_status is the one place that drops the status"
    );
    assert_eq!(calls(&text, "scenarios_discarding_gate_status"), 2);
}

#[test]
fn calls_ignores_definitions_and_longer_identifiers() {
    let sample = "pub fn requirements_trace_contract(\n\
                  x::requirements_trace_contract(a)\n\
                  my_requirements_trace_contract(b)\n\
                  requirements_trace_contract_v2(c)";
    assert_eq!(calls(sample, PARSE), 1);
}
