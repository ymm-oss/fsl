// SPDX-License-Identifier: Apache-2.0

//! Every spec-reading command must classify a syntax error the way `check`
//! does.
//!
//! `docs/DESIGN-v1.md` §7.2 fixes the error classification as a closed set and
//! guarantees `loc` for `parse`; `docs/DESIGN-rust-port.md` requires the JSON
//! envelope to be preserved; `docs/DESIGN-rust-lsp.md` promises that "CLI and
//! LSP tests compare diagnostic kind and message for parse, type, and semantic
//! failures". Before this matrix only `check` was exercised, so the CLI could
//! (and did) report the same unparseable file as `kind:"semantics"` with no
//! `loc` from every command that loaded the spec through `load_kernel_model`.

use std::path::{Path, PathBuf};
use std::process::Command;

use serde_json::{Value, json};

/// The corpus parse-error golden. `docs/RUST-PORTING.md` names it as the
/// shared parse-location parity case.
const PARSE_FIXTURE: &str = "examples/gallery/errors/parse_missing_expression.fsl";
/// A spec that parses but declares no state: a genuine `semantics` diagnostic
/// that must keep reaching the message-string classifier.
const SEMANTIC_SOURCE: &str = "spec NoState {\n  const value = 1\n}\n";
/// A spec that parses and lowers but references an undeclared type: issue 497
/// recorded `kind:"type"` as already correct, so it must not move.
const TYPE_SOURCE: &str = "spec BadType {\n  state { value: Missing }\n  init { value = 0 }\n}\n";
const VALID_SOURCE: &str = "spec Valid {\n  state { value: 0..2 }\n  init { value = 0 }\n  action bump() {\n    requires value < 2\n    value = value + 1\n  }\n  invariant Bounded { value <= 2 }\n}\n";

fn repository_root() -> PathBuf {
    Path::new(env!("CARGO_MANIFEST_DIR"))
        .parent()
        .and_then(Path::parent)
        .expect("repository root")
        .to_path_buf()
}

fn run_cli(arguments: &[&str]) -> (Value, i32) {
    let output = Command::new(env!("CARGO_BIN_EXE_fslc"))
        .args(arguments)
        .current_dir(repository_root())
        .output()
        .expect("run native fslc");
    let value = serde_json::from_slice(&output.stdout).unwrap_or_else(|error| {
        panic!(
            "invalid JSON: {error}; args={arguments:?}; stderr={}",
            String::from_utf8_lossy(&output.stderr)
        )
    });
    (value, output.status.code().expect("native exit status"))
}

fn run_cli_json(arguments: &[&str]) -> Option<Value> {
    let output = Command::new(env!("CARGO_BIN_EXE_fslc"))
        .args(arguments)
        .current_dir(repository_root())
        .output()
        .expect("run native fslc");
    serde_json::from_slice(&output.stdout).ok()
}

fn fixture_directory(name: &str) -> PathBuf {
    let directory = std::env::temp_dir().join(format!("fsl-{name}-{}", std::process::id()));
    std::fs::create_dir_all(&directory).expect("create fixture directory");
    directory
}

fn write_fixture(directory: &Path, name: &str, contents: &str) -> String {
    let path = directory.join(name);
    std::fs::write(&path, contents).expect("write fixture");
    path.to_str().expect("UTF-8 fixture path").to_owned()
}

/// Every command that reads a specification, with the auxiliary arguments it
/// needs to reach its spec loader (`replay` needs a trace, `diff` a second
/// spec).
fn spec_reading_commands<'a>(
    spec: &'a str,
    trace: &'a str,
    other: &'a str,
) -> Vec<(&'a str, Vec<&'a str>)> {
    vec![
        ("check", vec!["check", spec]),
        ("lint", vec!["lint", spec]),
        ("fmt", vec!["fmt", spec, "--check"]),
        ("mutate", vec!["mutate", spec]),
        ("verify", vec!["verify", spec, "--depth", "3"]),
        ("kernel", vec!["kernel", spec]),
        ("conformance", vec!["conformance", spec]),
        ("scenarios", vec!["scenarios", spec]),
        ("explain", vec!["explain", spec]),
        ("analyze", vec!["analyze", spec]),
        ("typestate", vec!["typestate", spec]),
        ("ledger", vec!["ledger", spec]),
        ("testgen", vec!["testgen", spec]),
        ("document", vec!["document", "generate", spec]),
        ("diff", vec!["diff", spec, other]),
        ("replay", vec!["replay", spec, "--trace", trace]),
        ("html", vec!["html", spec]),
        ("sweep", vec!["sweep", spec]),
    ]
}

#[test]
fn every_spec_reading_command_reports_a_syntax_error_as_parse_with_a_location() {
    let directory = fixture_directory("parse-matrix");
    let trace = write_fixture(
        &directory,
        "trace.json",
        r#"{"schema_version":1,"spec":"GalleryParseMissingExpression","initial":{"x":0},"events":[]}"#,
    );
    let other = write_fixture(&directory, "other.fsl", VALID_SOURCE);

    for (name, arguments) in spec_reading_commands(PARSE_FIXTURE, &trace, &other) {
        let (output, status) = run_cli(&arguments);
        assert_eq!(output["result"], "error", "{name}: {output}");
        assert_eq!(output["kind"], "parse", "{name}: {output}");
        // `lint` adds a `file` key to its own `loc`; the guaranteed fields are
        // `line`/`column` (`docs/DESIGN-v1.md` §7.2).
        assert_eq!(output["loc"]["line"], json!(6), "{name}: {output}");
        assert_eq!(output["loc"]["column"], json!(14), "{name}: {output}");
        assert_eq!(output["diagnostic_code"], "FSL-PARSE", "{name}: {output}");
        assert_eq!(status, 2, "{name}: {output}");
        // Issue 497 recorded "relative-path parse messages do not leak an
        // absolute path" as already correct; keep it pinned.
        let message = output["message"].as_str().expect("parse message");
        assert!(
            !message.contains(repository_root().to_str().expect("UTF-8 root")),
            "{name}: {message}"
        );
    }

    std::fs::remove_dir_all(&directory).expect("remove fixture directory");
}

#[test]
fn a_genuine_semantic_or_type_error_keeps_its_own_classification() {
    let directory = fixture_directory("parse-matrix-negative");
    let trace = write_fixture(
        &directory,
        "trace.json",
        r#"{"schema_version":1,"spec":"NoState","initial":{},"events":[]}"#,
    );
    let other = write_fixture(&directory, "other.fsl", VALID_SOURCE);
    let cases = [
        ("semantics", SEMANTIC_SOURCE, "no_state.fsl"),
        ("type", TYPE_SOURCE, "bad_type.fsl"),
    ];

    for (expected_kind, source, file) in cases {
        let spec = write_fixture(&directory, file, source);
        for (name, arguments) in spec_reading_commands(&spec, &trace, &other) {
            let (output, status) = run_cli(&arguments);
            assert_eq!(
                output["result"], "error",
                "{expected_kind}/{name}: {output}"
            );
            // `fmt`/`lint` never lower to the kernel, so they accept a
            // syntactically valid document; every loader-backed command must
            // still route the failure to the message-string classifier.
            if matches!(name, "fmt" | "lint") {
                continue;
            }
            assert_ne!(output["kind"], "parse", "{expected_kind}/{name}: {output}");
            assert_eq!(
                output["kind"], expected_kind,
                "{expected_kind}/{name}: {output}"
            );
            assert_eq!(status, 2, "{expected_kind}/{name}: {output}");
        }
    }

    std::fs::remove_dir_all(&directory).expect("remove fixture directory");
}

#[test]
fn a_valid_specification_is_never_reclassified_as_a_syntax_error() {
    let directory = fixture_directory("parse-matrix-positive");
    let trace = write_fixture(
        &directory,
        "trace.json",
        r#"{"schema_version":1,"spec":"Valid","initial":{"value":0},"events":[]}"#,
    );
    let other = write_fixture(&directory, "other.fsl", VALID_SOURCE);
    let spec = write_fixture(&directory, "valid.fsl", VALID_SOURCE);

    for (name, arguments) in spec_reading_commands(&spec, &trace, &other) {
        // A success path may emit Markdown or HTML rather than JSON (`ledger`,
        // `html`); that is already proof it did not emit an error envelope.
        if let Some(output) = run_cli_json(&arguments) {
            assert_ne!(output["kind"], "parse", "{name}: {output}");
        }
    }
    let (output, status) = run_cli(&["check", &spec]);
    assert_eq!(output["result"], "ok", "{output}");
    assert_eq!(status, 0, "{output}");
    // `document generate` owns a second loader (`load_document_claims`), so its
    // accepting path is pinned on a real requirements-dialect corpus file. It
    // writes the document to stdout, so only the exit code is asserted.
    let document = Command::new(env!("CARGO_BIN_EXE_fslc"))
        .args([
            "document",
            "generate",
            "examples/agentic_rag/agentic_rag_requirements.fsl",
        ])
        .current_dir(repository_root())
        .output()
        .expect("run native fslc");
    assert_eq!(
        document.status.code(),
        Some(0),
        "{}",
        String::from_utf8_lossy(&document.stdout)
    );

    std::fs::remove_dir_all(&directory).expect("remove fixture directory");
}

#[test]
fn analyze_agrees_between_single_and_batch_input() {
    let directory = fixture_directory("analyze-batch-parity");
    let other = write_fixture(&directory, "other.fsl", VALID_SOURCE);
    let missing = directory
        .join("absent.fsl")
        .to_str()
        .expect("UTF-8 fixture path")
        .to_owned();

    let (single, single_status) = run_cli(&["analyze", PARSE_FIXTURE]);
    let (batch, batch_status) = run_cli(&["analyze", PARSE_FIXTURE, &other]);
    assert_eq!(single_status, 2, "{single}");
    assert_eq!(batch_status, 2, "{batch}");
    let entry = batch["errors"]
        .as_array()
        .expect("batch errors")
        .iter()
        .find(|entry| entry["file"] == json!(PARSE_FIXTURE))
        .expect("batch entry for the parse fixture")
        .clone();
    assert_eq!(entry["kind"], single["kind"], "{batch}");
    assert_eq!(entry["loc"], single["loc"], "{batch}");
    assert_eq!(entry["message"], single["message"], "{batch}");
    assert_eq!(entry["kind"], "parse", "{batch}");

    // Issue 497: a missing input classified as `semantics` alone and `io` as
    // soon as a second input was added.
    let (single_missing, single_missing_status) = run_cli(&["analyze", &missing]);
    let (batch_missing, batch_missing_status) = run_cli(&["analyze", &missing, &other]);
    assert_eq!(single_missing["kind"], "io", "{single_missing}");
    assert_eq!(batch_missing["kind"], "io", "{batch_missing}");
    assert_eq!(single_missing_status, 2, "{single_missing}");
    assert_eq!(batch_missing_status, 2, "{batch_missing}");

    std::fs::remove_dir_all(&directory).expect("remove fixture directory");
}

#[test]
fn a_missing_input_is_io_for_every_spec_reading_command() {
    let directory = fixture_directory("missing-input");
    let trace = write_fixture(
        &directory,
        "trace.json",
        r#"{"schema_version":1,"spec":"Valid","initial":{"value":0},"events":[]}"#,
    );
    let other = write_fixture(&directory, "other.fsl", VALID_SOURCE);
    let missing = directory
        .join("absent.fsl")
        .to_str()
        .expect("UTF-8 fixture path")
        .to_owned();

    for (name, arguments) in spec_reading_commands(&missing, &trace, &other) {
        let (output, status) = run_cli(&arguments);
        assert_eq!(output["result"], "error", "{name}: {output}");
        assert_eq!(output["kind"], "io", "{name}: {output}");
        assert_eq!(status, 2, "{name}: {output}");
    }

    std::fs::remove_dir_all(&directory).expect("remove fixture directory");
}

/// The corpus `type`/`semantics` goldens, with the construct each diagnostic
/// must point at.
///
/// The `name` member of that clause is pinned separately, by
/// `NAME_FIXTURE` below (issue 565).
const LOCATED_SEMANTIC_FIXTURES: &[(&str, &str, u64, u64)] = &[
    // `state { owner: UserId }` — the state declaration naming the unknown type.
    (
        "examples/gallery/errors/type_undeclared_type.fsl",
        "type",
        5,
        11,
    ),
    // `struct Bag { members: Set<K> }` — `TypeExpr` carries no span of its own,
    // so the declaration is the closest true location.
    (
        "examples/gallery/errors/type_struct_set_field.fsl",
        "type",
        6,
        3,
    ),
    // The *second* `x = 2`, not the first write and not the enclosing action.
    (
        "examples/gallery/errors/semantics_duplicate_assignment.fsl",
        "semantics",
        9,
        5,
    ),
];

/// Issue 555: `docs/DESIGN-v1.md` §7.2 guarantees `loc` for `type` and
/// `semantics`, not only for `parse`. Issue 484 delivered the `parse` half; the
/// other half returned `loc: null` from every command, including `check`.
///
/// The line and column are asserted exactly. "`loc` is not null" cannot
/// distinguish a correct location from a wrong one, and a `loc` that points at
/// the wrong construct is a worse outcome for a repair agent than no `loc`.
#[test]
fn every_spec_reading_command_locates_a_type_or_semantic_error() {
    let directory = fixture_directory("semantic-loc-matrix");
    let trace = write_fixture(
        &directory,
        "trace.json",
        r#"{"schema_version":1,"spec":"Valid","initial":{"value":0},"events":[]}"#,
    );
    let other = write_fixture(&directory, "other.fsl", VALID_SOURCE);

    for (fixture, expected_kind, line, column) in LOCATED_SEMANTIC_FIXTURES {
        for (name, arguments) in spec_reading_commands(fixture, &trace, &other) {
            // `fmt` never lowers to the kernel and refuses these inputs as
            // `usage` before any typed-model diagnostic exists.
            if name == "fmt" {
                continue;
            }
            let (output, status) = run_cli(&arguments);
            assert_eq!(output["result"], "error", "{fixture}/{name}: {output}");
            assert_eq!(output["kind"], *expected_kind, "{fixture}/{name}: {output}");
            assert_eq!(status, 2, "{fixture}/{name}: {output}");
            // `lint` adds a `file` key to its own `loc`; the guaranteed fields
            // are `line`/`column` (`docs/DESIGN-v1.md` §7.2).
            assert_eq!(
                output["loc"]["line"].as_u64(),
                Some(*line),
                "{fixture}/{name}: {output}"
            );
            assert_eq!(
                output["loc"]["column"].as_u64(),
                Some(*column),
                "{fixture}/{name}: {output}"
            );
        }
    }

    std::fs::remove_dir_all(&directory).expect("remove fixture directory");
}

/// The corpus `name` golden, with the construct its diagnostic must point at.
///
/// `docs/DESIGN-v1.md` §7.2 fixes `kind` as a closed set including `name`.
/// Native reached every member except that one: name-resolution failures were
/// collapsed into `semantics`, because the only classifier was
/// `semantic_error_kind`, which matches message text and has no `name` pattern
/// to match (issue 565).
const NAME_FIXTURE: &str = "examples/gallery/errors/name_duplicate_state_variable.fsl";
/// `state { x: Bool, x: Bool }` — the *second* `x`, the redeclaration, not the
/// first binding and not the enclosing `state` block.
const NAME_FIXTURE_LINE: u64 = 5;
const NAME_FIXTURE_COLUMN: u64 = 20;

/// Every native name-resolution diagnostic, with the message each must keep
/// **byte for byte**.
///
/// The classifier still falls back to message matching for `type`, and that
/// matching is prefix/suffix-based, so any edit to one of these strings can
/// move a `kind` without touching a classification rule. Pinning the exact text
/// is what makes that impossible to do silently.
const NAME_DIAGNOSTICS: &[(&str, &str)] = &[
    (
        "spec DupVar {\n  state { x: Bool, x: Bool }\n  init { x = true }\n  action flip() { x = not x }\n}\n",
        "duplicate state variable 'x'",
    ),
    (
        "spec DupEnum {\n  enum E { A, B }\n  enum F { B, C }\n  state { e: E }\n  init { e = A }\n  action stay() { e = e }\n}\n",
        "duplicate enum member 'B'",
    ),
];

#[test]
fn every_spec_reading_command_classifies_a_name_resolution_failure_as_name() {
    let directory = fixture_directory("name-kind-matrix");
    let trace = write_fixture(
        &directory,
        "trace.json",
        r#"{"schema_version":1,"spec":"Valid","initial":{"value":0},"events":[]}"#,
    );
    let other = write_fixture(&directory, "other.fsl", VALID_SOURCE);

    for (name, arguments) in spec_reading_commands(NAME_FIXTURE, &trace, &other) {
        // `fmt` never lowers to the kernel, so no typed-model diagnostic exists
        // for it to classify.
        if name == "fmt" {
            continue;
        }
        let (output, status) = run_cli(&arguments);
        assert_eq!(output["result"], "error", "{name}: {output}");
        assert_eq!(output["kind"], "name", "{name}: {output}");
        assert_eq!(status, 2, "{name}: {output}");
        assert_eq!(
            output["message"], "duplicate state variable 'x'",
            "{name}: {output}"
        );
        // Issue 555 gave this half of the clause its `loc`; assert the exact
        // position for the same reason as the fixtures above, and because a
        // `name` diagnostic pointing at the first `x` would name the innocent
        // declaration.
        assert_eq!(
            output["loc"]["line"].as_u64(),
            Some(NAME_FIXTURE_LINE),
            "{name}: {output}"
        );
        assert_eq!(
            output["loc"]["column"].as_u64(),
            Some(NAME_FIXTURE_COLUMN),
            "{name}: {output}"
        );
    }

    std::fs::remove_dir_all(&directory).expect("remove fixture directory");
}

/// The classification travels on the diagnostic, so it must not depend on the
/// message text — and the messages themselves must not drift, because the
/// surviving `type` rules still match on prefixes and suffixes.
#[test]
fn name_resolution_messages_are_unchanged_and_classify_without_message_matching() {
    let directory = fixture_directory("name-kind-messages");

    for (index, (source, message)) in NAME_DIAGNOSTICS.iter().enumerate() {
        let spec = write_fixture(&directory, &format!("name_{index}.fsl"), source);
        let (output, status) = run_cli(&["check", &spec]);
        assert_eq!(status, 2, "{message}: {output}");
        assert_eq!(output["kind"], "name", "{message}: {output}");
        assert_eq!(output["message"], *message, "{message}: {output}");
    }

    // The positive control for the same edge: `semantic_error_kind`'s surviving
    // message rules must still produce `type`, not be swept into `name`.
    let bad_type = write_fixture(&directory, "bad_type.fsl", TYPE_SOURCE);
    let (output, status) = run_cli(&["check", &bad_type]);
    assert_eq!(status, 2, "{output}");
    assert_eq!(output["kind"], "type", "{output}");
    assert_eq!(output["message"], "unknown type 'Missing'", "{output}");

    let no_state = write_fixture(&directory, "no_state.fsl", SEMANTIC_SOURCE);
    let (output, status) = run_cli(&["check", &no_state]);
    assert_eq!(status, 2, "{output}");
    assert_eq!(output["kind"], "semantics", "{output}");

    std::fs::remove_dir_all(&directory).expect("remove fixture directory");
}
/// A duplicate declaration and the exact position that must be reported for it.
///
/// For a *duplicate* diagnostic, `source_diagnostic`'s message-derived fallback
/// — the first token matching the quoted name — is wrong by construction: a
/// duplicate is the same name appearing twice, so the first match is always the
/// earlier, innocent declaration. Carrying the span on the diagnostic is the
/// only correct direction, and the one issues 484, 555 and 565 all took
/// (issue 576).
const DUPLICATE_DECLARATION_POSITIONS: &[(&str, &str, u64, u64)] = &[
    // `enum F { B, C }` on line 3 — the redeclaration of `B`, not the `B` in
    // `enum E { A, B }` on line 2 at column 15.
    (
        "spec DupEnum {\n  enum E { A, B }\n  enum F { B, C }\n  state { e: E }\n  init { e = A }\n  action stay() { e = e }\n}\n",
        "duplicate enum member 'B'",
        3,
        12,
    ),
    // The sibling case, `duplicate state variable`, landed with issue 565 and
    // is pinned by `NAME_FIXTURE_LINE`/`NAME_FIXTURE_COLUMN` in the
    // every-command test above, so it is deliberately not repeated here.
];

/// A `loc` that exists but names the wrong construct is worse than no `loc`:
/// `docs/DESIGN-v1.md` G2 assumes the position is *correct*, and for a
/// duplicate the wrong one accuses the declaration that is not the problem.
///
/// The classification is deliberately not asserted here — it is issue 565's
/// subject and moves independently — so this stays a pure position contract.
#[test]
fn a_duplicate_declaration_is_located_at_the_repeated_occurrence() {
    let directory = fixture_directory("duplicate-declaration-loc");

    for (index, (source, message, line, column)) in
        DUPLICATE_DECLARATION_POSITIONS.iter().enumerate()
    {
        let spec = write_fixture(&directory, &format!("duplicate_{index}.fsl"), source);
        let (output, status) = run_cli(&["check", &spec]);
        assert_eq!(status, 2, "{message}: {output}");
        assert_eq!(output["message"], *message, "{message}: {output}");
        assert_eq!(
            output["loc"]["line"].as_u64(),
            Some(*line),
            "{message}: {output}"
        );
        assert_eq!(
            output["loc"]["column"].as_u64(),
            Some(*column),
            "{message}: {output}"
        );
    }

    std::fs::remove_dir_all(&directory).expect("remove fixture directory");
}
