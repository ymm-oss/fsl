// SPDX-License-Identifier: Apache-2.0
// Copyright 2026 Ryoichi Izumita

//! Drift control for #1285: acceptance / forbidden traces reach a consumer
//! only through the requirement-trace gate, or through a read that names
//! itself unchecked (stage 1), and every consumer that reports a result about
//! those traces fails closed through `require_trace_gate` (stage 2).
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

#[path = "support/product_source.rs"]
mod product_source;

const PARSE: &str = "requirements_trace_contract";
/// The same parse under explicit `ParseOptions`; its calls count as [`PARSE`].
const PARSE_WITH_OPTIONS: &str = "requirements_trace_contract_with_options";
const UNCHECKED: [&str; 2] = ["unchecked_acceptance", "unchecked_forbidden"];

/// `(workspace-relative file, entry, count, reason)`. `entry` is [`PARSE`]
/// (either spelling) or `"unchecked"` (either `UNCHECKED` accessor).
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
        "fsl-core/src/dialect.rs",
        PARSE,
        1,
        "the published-language requirements_trace_contract delegating to its options form; no consumer",
    ),
    (
        "fsl-tools/src/document_project.rs",
        PARSE,
        1,
        "document claims: fsl-tools does not depend on fsl-runtime, which the gate replays on; fslc runs document_trace_gate first",
    ),
    (
        "fsl-tools/src/document_project.rs",
        "unchecked",
        4,
        "document claims: one claim per declared case, after fslc's document_trace_gate passed",
    ),
    (
        "fsl-tools/src/document_render.rs",
        PARSE,
        1,
        "document render: fsl-tools does not depend on fsl-runtime, which the gate replays on; fslc runs document_trace_gate first",
    ),
    (
        "fsl-tools/src/document_render.rs",
        "unchecked",
        2,
        "document render: finds the case a claim renders, after fslc's document_trace_gate passed",
    ),
    (
        "fslc/src/main.rs",
        PARSE,
        2,
        "analyze graph projecting a contract the gate refused (its cases marked trace_gate: unchecked), diff OLD forbidden replay (#1239)",
    ),
    (
        "fslc/src/main.rs",
        "unchecked",
        6,
        "the analyze graph's unchecked cases, the diff OLD replay, and mutate attributing a gate rejection to its case's requirements",
    ),
    (
        "fslc/src/strict_tags.rs",
        PARSE,
        1,
        "check / verify --strict-tags referenced ids: runs only on an ok / verified / proved result, which passed the gate (under --instances / --values the scoped gate may skip a case, so the full gate is not re-run here)",
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

/// Product tokens, excluding comments and test-only items without truncating
/// any product code that follows them.
fn product_text(path: &Path) -> String {
    let text = std::fs::read_to_string(path)
        .expect("read source")
        .replace("\r\n", "\n");
    product_source::product_source(&text)
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
        let parses = calls(&text, PARSE) + calls(&text, PARSE_WITH_OPTIONS);
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

/// `require_trace_gate(` calls per product file: each consumer that reports a
/// result about acceptance / forbidden cases fails closed on a gate refusal
/// (#1285 stage 2, #1242, #1243). `(workspace-relative file, count, consumers)`.
const FAIL_CLOSED: &[(&str, usize, &str)] = &[
    ("fslc/src/check_stages.rs", 1, "check"),
    (
        "fslc/src/main.rs",
        5,
        "document (generate / claims / check, requirements_document approval), scenarios / testgen, explain / html, mutate, ledger (and ledger approval); verification baselines delegate to verification.rs (#1417)",
    ),
    ("fslc/src/source_diagnostic.rs", 1, "LSP diagnostics"),
    (
        "fslc/src/spec_loading.rs",
        1,
        "the process-parse-options wrapper",
    ),
    (
        "fslc/src/verification.rs",
        1,
        "shared verify engines and no-cache baselines (#1417)",
    ),
];

#[test]
fn every_trace_consumer_fails_closed_through_the_helper() {
    let root = workspace();
    let found = product_sources()
        .into_iter()
        .filter_map(|path| {
            let count = calls(&product_text(&path), "require_trace_gate");
            let relative = path
                .strip_prefix(&root)
                .expect("source under workspace")
                .to_string_lossy()
                .replace('\\', "/");
            (count > 0).then_some((relative, count))
        })
        .collect::<BTreeMap<_, _>>();
    let expected = FAIL_CLOSED
        .iter()
        .map(|(file, count, consumers)| {
            assert!(!consumers.is_empty(), "{file} must name its consumers");
            ((*file).to_owned(), *count)
        })
        .collect::<BTreeMap<_, _>>();
    assert_eq!(
        found, expected,
        "a trace consumer gained or lost its fail-closed gate. A consumer that \
         reports a result about acceptance / forbidden cases calls \
         require_trace_gate and reports its refusal (exit 2)"
    );
}

/// Explain, html, and ledger build their witnesses from the checked contract
/// they hold, so the scenarios status is never dropped before the gate
/// (#1242): no caller drops `run_scenarios_mode_from_source`'s status, and the
/// scenarios body is reachable only with a contract the gate handed out.
#[test]
fn witnesses_are_built_only_from_a_checked_contract() {
    let text = product_text(&workspace().join("fslc/src/main.rs"));
    let compact = text.split_whitespace().collect::<String>();
    assert_eq!(
        compact
            .matches(",_)=run_scenarios_mode_from_source(")
            .count()
            + compact
                .matches("_status)=run_scenarios_mode_from_source(")
                .count(),
        0,
        "a caller drops run_scenarios_mode_from_source's status, which may be a gate refusal"
    );
    assert!(
        compact.contains("fnscenarios_from_checked_traces(path:&Path,source:&str,model:&KernelModel,trace_contract:Option<&CheckedTraceContract>,"),
        "the scenarios body takes the checked contract"
    );
    assert_eq!(
        calls(&text, "scenarios_from_checked_traces"),
        2,
        "run_scenarios_mode_from_source (after its gate) and witness_scenarios are its only callers"
    );
    assert_eq!(
        calls(&text, "witness_scenarios"),
        2,
        "explain and ledger, each after require_trace_gate"
    );
}

#[test]
fn calls_ignores_definitions_and_longer_identifiers() {
    let sample = "pub fn requirements_trace_contract(\n\
                  x::requirements_trace_contract(a)\n\
                  my_requirements_trace_contract(b)\n\
                  requirements_trace_contract_v2(c)";
    assert_eq!(calls(sample, PARSE), 1);
}
