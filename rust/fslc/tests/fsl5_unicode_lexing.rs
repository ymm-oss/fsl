// SPDX-License-Identifier: Apache-2.0
// Copyright 2026 Ryoichi Izumita

//! fsl 5 plan §3 S2, through the native CLI: identifiers widen to UAX #31
//! only in a document headed `fsl 5`; the
//! look-alike characters of design C.4 fail with their own code there, and
//! every other combination reads identifiers exactly as the published language
//! does.
//!
//! `fslc lint` is the command driven here because its parse already receives
//! the process's options; `check` and `kernel` parse in modules shared with
//! the Worker, which receive the options from the edition-wiring stage
//! (`docs/design/parse-entry-inventory.md`).

use std::path::{Path, PathBuf};
use std::process::Command;

use serde_json::Value;

struct Scratch(PathBuf);

impl Scratch {
    fn new(label: &str) -> Self {
        let directory =
            std::env::temp_dir().join(format!("fslc-fsl5-unicode-{label}-{}", std::process::id()));
        std::fs::create_dir_all(&directory).expect("create scratch directory");
        Self(directory)
    }

    fn write(&self, name: &str, source: &str) -> PathBuf {
        let path = self.0.join(name);
        std::fs::write(&path, source).expect("write fixture");
        path
    }
}

impl Drop for Scratch {
    fn drop(&mut self) {
        let _ = std::fs::remove_dir_all(&self.0);
    }
}

/// `(stdout, exit)` of `fslc <subcommand> <path>`.
fn fslc(subcommand: &str, path: &Path) -> (Vec<u8>, i32) {
    let mut command = Command::new(env!("CARGO_BIN_EXE_fslc"));
    command.arg(subcommand).arg(path);
    let output = command.output().expect("run fslc");
    (output.stdout, output.status.code().expect("fslc exit code"))
}

fn envelope(stdout: &[u8]) -> Value {
    serde_json::from_slice(stdout).expect("fslc prints one JSON envelope")
}

fn line_column(output: &Value) -> (u64, u64) {
    (
        output["loc"]["line"].as_u64().expect("loc.line"),
        output["loc"]["column"].as_u64().expect("loc.column"),
    )
}

fn spec_with_state_field(header: &str, field: &str) -> String {
    format!("{header}spec S {{\n  state {{ {field}: Int }}\n}}\n")
}

/// Acceptance 1: `経費` stays an unexpected character unless the `fsl 5`
/// header is present (#1434 removed the gate this also needed).
#[test]
fn japanese_identifier_needs_the_fsl5_header() {
    let scratch = Scratch::new("acceptance-1");
    for (name, header) in [("no-header.fsl", ""), ("fsl4.fsl", "fsl 4\n")] {
        let path = scratch.write(name, &spec_with_state_field(header, "経費"));
        for subcommand in ["lint", "check"] {
            let (stdout, exit) = fslc(subcommand, &path);
            assert_eq!(exit, 2, "{subcommand} {name}");
            let output = envelope(&stdout);
            assert_eq!(output["kind"], "parse", "{name}: {output:#}");
            assert_eq!(output["diagnostic_code"], "FSL-PARSE", "{name}: {output:#}");
            assert!(
                output["message"]
                    .as_str()
                    .is_some_and(|message| message.starts_with("unexpected character '経'")),
                "{name}: {output:#}"
            );
        }
    }

    // The lexer accepts the identifier and the header is read: what stops
    // the document now is the naming rule, not an unexpected character. A
    // spec has no glossary, so its names are canonical and must be ASCII.
    let path = scratch.write("fsl5.fsl", &spec_with_state_field("fsl 5\n", "経費"));
    for subcommand in ["lint", "check"] {
        let (stdout, exit) = fslc(subcommand, &path);
        let output = envelope(&stdout);
        assert_eq!(exit, 2, "{subcommand}: {output:#}");
        assert_eq!(
            output["diagnostic_code"], "canonical_name_not_ascii",
            "{subcommand}: {output:#}"
        );
        assert_eq!(line_column(&output), (3, 11), "{subcommand}: {output:#}");
    }
}

/// One `(subcommand output) warning` as `(code, line, column, replacement)`.
fn warning_at(warning: &Value) -> (String, u64, u64, String) {
    (
        warning["code"].as_str().expect("warning code").to_owned(),
        warning["loc"]["line"].as_u64().expect("warning line"),
        warning["loc"]["column"].as_u64().expect("warning column"),
        warning["replacement"]
            .as_str()
            .or_else(|| warning["canonical_replacement"].as_str())
            .expect("warning replacement")
            .to_owned(),
    )
}

/// A requirements document the lexer reads with two warnings: the glossary
/// writes its alias in NFD (`カ` + U+3099, read as `ガ`), and a U+3000 stands
/// between `actor` and its name (read as a space).
const LEXICAL_WARNINGS: &str = "fsl 5\nrequirements R {\n  glossary {\n    Claim = \
                                \u{30ab}\u{3099}\u{30a4}\u{30c9}\n  }\n  actor\u{3000}Employee\n  \
                                entity ガイド {\n    lifecycle Draft -> Done\n  }\n  \
                                event finish = Employee finishes ガイド Draft -> Done\n  \
                                invariant Typed { true }\n}\nverify {\n  instances Claim = 1\n}\n";

/// fsl 5 plan S2: `non_nfc_identifier` and `fullwidth_space` are warnings
/// of every command that reads the document, not only of the language
/// server: `check` lists them in `warnings` and still accepts the document,
/// and `lint` reports them as findings with the text that reads the same.
#[test]
fn lexical_warnings_reach_check_and_lint() {
    let scratch = Scratch::new("lexical-warnings");
    let path = scratch.write("doc.fsl", LEXICAL_WARNINGS);
    let expected = vec![
        ("non_nfc_identifier".to_owned(), 4, 13, "ガイド".to_owned()),
        ("fullwidth_space".to_owned(), 6, 8, " ".to_owned()),
    ];

    let (stdout, exit) = fslc("check", &path);
    let output = envelope(&stdout);
    assert_eq!(
        (exit, output["result"].as_str()),
        (0, Some("ok")),
        "{output:#}"
    );
    let warnings = output["warnings"].as_array().expect("warnings array");
    assert_eq!(
        warnings.iter().map(warning_at).collect::<Vec<_>>(),
        expected,
        "{output:#}"
    );
    assert!(
        warnings
            .iter()
            .all(|warning| warning["severity"] == "warning"),
        "{output:#}"
    );

    let (stdout, exit) = fslc("lint", &path);
    let output = envelope(&stdout);
    assert_eq!(exit, 1, "{output:#}");
    assert_eq!(output["finding_count"], 2, "{output:#}");
    let findings = output["files"][0]["findings"]
        .as_array()
        .expect("findings array");
    assert_eq!(
        findings.iter().map(warning_at).collect::<Vec<_>>(),
        expected,
        "{output:#}"
    );
    assert!(
        findings
            .iter()
            .all(|finding| finding["severity"] == "warning"),
        "{output:#}"
    );
}

/// decisions/045 and plan S3: a canonical name is ASCII, and only a
/// requirements document has a glossary to give it a Japanese alias. Every
/// other document read in `fsl 5` declares canonical names only, so a
/// non-ASCII name there is `canonical_name_not_ascii` at the name, as it is
/// for a requirements name with no glossary entry. The message says where a
/// Japanese name belongs instead.
#[test]
fn a_non_ascii_name_outside_requirements_is_canonical_name_not_ascii() {
    let scratch = Scratch::new("non-ascii-names");
    for (dialect, source, at) in [
        (
            "business",
            "fsl 5\nbusiness B {\n  actor 顧客\n  entity Return\n  process Return {\n    \
             stages Requested, Approved\n    initial Requested\n    \
             transition approve Requested -> Approved by 顧客\n  }\n}\nverify {\n  \
             instances Return = 1\n}\n"
                .to_owned(),
            (3, 9),
        ),
        ("spec", spec_with_state_field("fsl 5\n", "経費"), (3, 11)),
        (
            "domain",
            "fsl 5\ndomain D {\n  enum 状態 { Pending }\n}\n".to_owned(),
            (3, 8),
        ),
    ] {
        let path = scratch.write(&format!("{dialect}.fsl"), &source);
        for subcommand in ["check", "lint"] {
            let (stdout, exit) = fslc(subcommand, &path);
            let output = envelope(&stdout);
            assert_eq!(exit, 2, "{dialect} {subcommand}: {output:#}");
            assert_eq!(
                output["diagnostic_code"], "canonical_name_not_ascii",
                "{dialect} {subcommand}: {output:#}"
            );
            assert_eq!(
                line_column(&output),
                at,
                "{dialect} {subcommand}: {output:#}"
            );
            assert!(
                output["message"]
                    .as_str()
                    .is_some_and(|message| message.contains("glossary")),
                "{dialect} {subcommand}: {output:#}"
            );
        }
    }
}

/// Acceptance 2 (lexical rows of design C.4): each look-alike fails with its
/// own code at its own location under the `fsl 5` header, and stays the
/// published `FSL-PARSE` unexpected character without the header.
#[test]
fn look_alike_characters_fail_with_their_own_code_under_the_fsl5_header() {
    let scratch = Scratch::new("acceptance-2");
    for (name, field, code, column, hint) in [
        (
            "fullwidth-letters",
            "Ｃｌａｉｍ",
            "fullwidth_ascii_identifier",
            11,
            "'Claim'",
        ),
        (
            "fullwidth-digit",
            "x１",
            "fullwidth_ascii_identifier",
            11,
            "'x1'",
        ),
        ("fullwidth-hyphen", "a－b", "fullwidth_symbol", 12, "'-'"),
        (
            "middle-dot",
            "申請・承認",
            "middle_dot_identifier",
            13,
            "'・'",
        ),
        (
            "latin-cyrillic",
            "Сlaim",
            "mixed_script_identifier",
            11,
            "'Сlaim'",
        ),
    ] {
        let path = scratch.write(
            &format!("{name}.fsl"),
            &spec_with_state_field("fsl 5\n", field),
        );
        let (stdout, exit) = fslc("lint", &path);
        assert_eq!(exit, 2, "{name}");
        let output = envelope(&stdout);
        assert_eq!(output["kind"], "parse", "{name}: {output:#}");
        assert_eq!(output["diagnostic_code"], code, "{name}: {output:#}");
        assert_eq!(line_column(&output), (3, column), "{name}: {output:#}");
        assert!(
            output["message"]
                .as_str()
                .is_some_and(|message| message.contains(hint)),
            "{name}: message names {hint}: {output:#}"
        );

        let headerless = scratch.write(
            &format!("{name}-headerless.fsl"),
            &spec_with_state_field("", field),
        );
        let (stdout, exit) = fslc("lint", &headerless);
        assert_eq!(exit, 2, "{name}");
        let output = envelope(&stdout);
        assert_eq!(output["diagnostic_code"], "FSL-PARSE", "{name}: {output:#}");
        assert!(
            output["message"]
                .as_str()
                .is_some_and(|message| message.starts_with("unexpected character")),
            "{name}: {output:#}"
        );
    }
}
