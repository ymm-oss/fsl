// SPDX-License-Identifier: Apache-2.0

use std::path::Path;
use std::process::Command;

use serde_json::Value;

#[test]
fn trace_step_type_errors_share_native_and_lsp_identity() {
    let directory = Path::new(env!("CARGO_TARGET_TMPDIR")).join(format!(
        "issue-1439-step-diagnostics-{}",
        std::process::id()
    ));
    std::fs::create_dir_all(&directory).expect("create step diagnostic fixtures");
    for (name, source, unstable, type_name, line, word) in [
        (
            "published",
            "requirements E {\n  enum Color { Red, Blue }\n  state { c: Color }\n  init { c = Red }\n  requirement REQ-1 \"paint\" {\n    action paint(x: Color) { requires x != c c = x }\n  }\n  forbidden FB-1 \"internal type\" {\n    paint(if c != 1 then Red else Blue)\n    expect rejected\n  }\n}\n",
            false,
            "Color",
            9,
            "paint",
        ),
        (
            "fsl5-phrase",
            "fsl 5\nrequirements R {\n  number Amount\n  actor Employee\n  entity Claim { amount: Amount = 0 lifecycle Draft -> Submitted }\n  event submit = Employee submits Claim with amount Draft -> Submitted\n  scenario AC-1 \"internal type\" {\n    Employee submits claim1 with amount (if claim_stage[0] != 1 then 1 else 2)\n    then claim1 is Submitted\n  }\n}\nverify { instances Claim = 2 values Amount = 0..3 }\n",
            true,
            "ClaimStage",
            8,
            "Employee",
        ),
    ] {
        let path = directory.join(format!("{name}.fsl"));
        std::fs::write(&path, source).expect("write step fixture");
        let path = path.to_str().expect("UTF-8 path");
        let mut command = Command::new(env!("CARGO_BIN_EXE_fslc"));
        command.args(["check", path]).env_remove("FSLC_UNSTABLE");
        let options = if unstable {
            command.env("FSLC_UNSTABLE", "fsl5");
            fsl_syntax::ParseOptions::with_unstable(
                fsl_syntax::UnstableFeatures::NONE.with(fsl_syntax::UnstableFeature::Fsl5),
            )
        } else {
            fsl_syntax::ParseOptions::stable()
        };
        let output = command.output().expect("native check");
        assert_eq!(output.status.code(), Some(2));
        assert!(output.stderr.is_empty());
        let cli: Value = serde_json::from_slice(&output.stdout).expect("native envelope");
        let shared = fslc_rust::source_diagnostic::diagnostics(
            source,
            path,
            &fsl_core::FsResolver::new(&directory),
            &options,
        )
        .into_iter()
        .find(|diagnostic| diagnostic.kind != "migration")
        .expect("shared semantic diagnostic");
        assert_eq!(cli["result"], "error");
        assert_eq!(cli["kind"], "semantics");
        assert_eq!(cli["kind"], shared.kind);
        assert_eq!(cli["message"], shared.message);
        assert_eq!(cli["loc"], shared.span.python_loc());
        assert_eq!(cli["loc"], serde_json::json!({"line": line, "column": 5}));
        assert!(shared.message.contains(&format!(
            "expression of type Int is not assignable to Named(\"{type_name}\")"
        )));
        assert_eq!(shared.code, "FSL-SEMANTIC");
        assert!(shared.located);
        assert_eq!(
            &source[shared.span.start.offset..shared.span.end.offset],
            word
        );
        if unstable {
            assert_eq!(cli["edition"], "next");
            assert_eq!(cli["fsl_edition"], 5);
        }
    }
    std::fs::remove_dir_all(directory).expect("remove owned step fixtures");
}

#[test]
fn cli_and_lsp_source_diagnostics_share_identity_without_changing_cli_envelopes() {
    let directory =
        std::env::temp_dir().join(format!("fsl-lsp-diagnostics-{}", std::process::id()));
    std::fs::create_dir_all(&directory).expect("create diagnostic fixture directory");
    let cases = [
        ("parse", "spec Broken { state {", None, None),
        (
            "parse-outside-bmp",
            "spec Broken {\n  state { 𠮷: Int }\n}",
            Some((2, 11)),
            None,
        ),
        (
            "type",
            "spec Broken { state { value: Missing } init { value = 0 } }",
            Some((1, 23)),
            None,
        ),
        (
            "type-hint",
            "spec Broken { type K = 0..1 struct Bag { members: Set<K> } state { bag: Bag } init { bag.members = Set {} } }",
            None,
            Some(
                "struct fields must be a scalar (domain type, enum, Bool, Int) or nested Option around a scalar; use a separate Map for Set, Map, Seq, relation, or struct fields",
            ),
        ),
        (
            "state-type-hint",
            "spec Broken {\n  type Key = 0..1\n  state {\n    nested: Map<Key, Map<Key, Bool>>\n  }\n}",
            Some((4, 5)),
            Some(
                "state types allow scalars, nested Option around a scalar, structs with those fields, Map<bounded scalar, scalar-or-nested-Option-or-struct>, Set<bounded scalar>, Seq<scalar,N>, and bounded-scalar relations; Option cannot wrap a collection or struct",
            ),
        ),
        (
            "semantics",
            "spec Broken { const value = 1 }",
            Some((1, 1)),
            None,
        ),
        (
            "acceptance-expect-type",
            "requirements E {\n  enum Color { Red, Blue }\n  state { c: Color }\n  init { c = Red }\n  requirement REQ-1 \"paint\" {\n    action paint(x: Color) { c = x }\n  }\n  acceptance AC-1 \"type comparison\" {\n    paint(Blue)\n    expect c != 1\n  }\n}\n",
            Some((10, 5)),
            None,
        ),
    ];

    for (name, source, expected_location, expected_hint) in cases {
        let path = directory.join(format!("{name}.fsl"));
        std::fs::write(&path, source).expect("write diagnostic fixture");
        let output = Command::new(env!("CARGO_BIN_EXE_fslc"))
            .args(["check", path.to_str().expect("UTF-8 fixture path")])
            .output()
            .expect("run native check");
        assert_eq!(output.status.code(), Some(2), "{name}");
        let cli: Value = serde_json::from_slice(&output.stdout).expect("parse CLI envelope");
        let resolver = fsl_core::FsResolver::new(Path::new(&directory));
        let shared = fslc_rust::source_diagnostic::diagnostics(
            source,
            path.to_str().expect("UTF-8 fixture path"),
            &resolver,
            &fsl_syntax::ParseOptions::stable(),
        )
        .into_iter()
        .find(|diagnostic| diagnostic.kind != "migration")
        .expect("shared source diagnostic");

        assert_eq!(cli["kind"], shared.kind, "{name}");
        assert_eq!(cli["message"], shared.message, "{name}");
        assert_eq!(cli["hint"].as_str(), expected_hint, "{name}");
        if name == "acceptance-expect-type" {
            assert_eq!(cli["result"], "error");
            assert_eq!(cli["kind"], "semantics");
            assert_eq!(shared.code, "FSL-SEMANTIC");
            assert!(shared.located);
            assert_eq!((shared.span.end.line, shared.span.end.column), (10, 11));
            assert_eq!(
                &source[shared.span.start.offset..shared.span.end.offset],
                "expect"
            );
        }
        if !cli["loc"].is_null() {
            assert_eq!(cli["loc"], shared.span.python_loc(), "{name}");
        }
        if let Some(expected) = expected_location {
            assert_eq!((shared.span.start.line, shared.span.start.column), expected);
        } else {
            assert_eq!(shared.span.start.line, 1, "{name}");
        }
        assert!(shared.span.start.offset <= source.len(), "{name}");
    }

    std::fs::remove_dir_all(directory).expect("remove diagnostic fixture directory");
}

/// fsl 5 plan S11: a requirements document written with the keyword
/// table's Japanese keywords and a glossary alias holding a character outside
/// the BMP. Behind the gate the CLI and the language server report the same
/// diagnostic, and the CLI's character column and the shared span's byte
/// offset name the same word, the one the server converts to UTF-16. Without
/// the gate the Japanese keyword is a lexical error at the same place for
/// both.
#[test]
fn japanese_keyword_diagnostics_share_cli_and_lsp_identity() {
    let directory = std::env::temp_dir().join(format!(
        "fsl-lsp-japanese-keyword-diagnostics-{}",
        std::process::id()
    ));
    std::fs::create_dir_all(&directory).expect("create fixture directory");
    let source = "fsl 5\n要件 R {\n  用語集 {\n    Claim = 𠮷申請\n  }\n  \
                  エンティティ 𠮷申請 { 金額: Int = 0  ライフサイクル A -> B }\n}\n";
    let path = directory.join("japanese.fsl");
    std::fs::write(&path, source).expect("write fixture");
    let path = path.to_str().expect("UTF-8 fixture path");
    let gated = fsl_syntax::ParseOptions::with_unstable(
        fsl_syntax::UnstableFeatures::NONE.with(fsl_syntax::UnstableFeature::Fsl5),
    );
    for (unstable, options, code, location, word) in [
        (
            Some("fsl5"),
            gated,
            "canonical_name_not_ascii",
            (6, 16),
            "金額",
        ),
        (
            None,
            fsl_syntax::ParseOptions::stable(),
            "FSL-PARSE",
            (2, 1),
            "要件",
        ),
    ] {
        let mut command = Command::new(env!("CARGO_BIN_EXE_fslc"));
        command
            .args(["check", path])
            .env_remove(fslc_rust::unstable_gate::ENVIRONMENT_VARIABLE);
        if let Some(features) = unstable {
            command.env(fslc_rust::unstable_gate::ENVIRONMENT_VARIABLE, features);
        }
        let output = command.output().expect("run native check");
        assert_eq!(output.status.code(), Some(2), "{unstable:?}");
        let cli: Value = serde_json::from_slice(&output.stdout).expect("parse CLI envelope");
        let resolver = fsl_core::FsResolver::new(Path::new(&directory));
        let shared = fslc_rust::source_diagnostic::diagnostics(source, path, &resolver, &options)
            .into_iter()
            .find(|diagnostic| diagnostic.kind != "migration")
            .expect("shared source diagnostic");
        assert_eq!(cli["kind"], shared.kind, "{unstable:?}");
        assert_eq!(cli["message"], shared.message, "{unstable:?}");
        assert_eq!(cli["diagnostic_code"], code, "{unstable:?}");
        assert_eq!(shared.code, code, "{unstable:?}");
        assert_eq!(cli["loc"], shared.span.python_loc(), "{unstable:?}");
        assert_eq!(
            (shared.span.start.line, shared.span.start.column),
            location,
            "{unstable:?}"
        );
        assert!(
            source.is_char_boundary(shared.span.start.offset)
                && source[shared.span.start.offset..].starts_with(word),
            "{unstable:?}: the span starts at byte {}",
            shared.span.start.offset
        );
    }
    std::fs::remove_dir_all(directory).expect("remove fixture directory");
}

#[test]
fn distinctness_unproved_diagnostic_shares_cli_and_lsp_identity() {
    let fixture = format!(
        "{}/tests/fixtures/issue_698_affine_index.fsl",
        env!("CARGO_MANIFEST_DIR")
    );
    let source = std::fs::read_to_string(&fixture).expect("read fixture");
    let output = Command::new(env!("CARGO_BIN_EXE_fslc"))
        .args(["check", &fixture])
        .output()
        .expect("run native check");
    assert_eq!(output.status.code(), Some(2));
    let cli: Value = serde_json::from_slice(&output.stdout).expect("parse CLI envelope");
    let shared = fslc_rust::source_diagnostic::diagnostics(
        &source,
        &fixture,
        &fsl_core::FsResolver::new(Path::new(".")),
        &fsl_syntax::ParseOptions::stable(),
    )
    .into_iter()
    .find(|diagnostic| diagnostic.kind != "migration")
    .expect("shared source diagnostic");
    assert_eq!(cli["kind"], shared.kind);
    assert_eq!(cli["message"], shared.message);
    assert_eq!(
        cli["diagnostic_code"],
        fsl_core::WRITE_DISTINCTNESS_UNPROVED_CODE
    );
    assert_eq!(shared.code, fsl_core::WRITE_DISTINCTNESS_UNPROVED_CODE);
    assert_eq!(cli["loc"], shared.span.python_loc());
    assert_eq!(cli["hint"].as_str(), shared.hint.as_deref());
    assert!(shared.quick_fix.is_some());
}

#[test]
fn nested_option_payload_diagnostics_have_cli_lsp_identity() {
    const STATE_HINT: &str = "state types allow scalars, nested Option around a scalar, structs with those fields, Map<bounded scalar, scalar-or-nested-Option-or-struct>, Set<bounded scalar>, Seq<scalar,N>, and bounded-scalar relations; Option cannot wrap a collection or struct";
    const STRUCT_HINT: &str = "struct fields must be a scalar (domain type, enum, Bool, Int) or nested Option around a scalar; use a separate Map for Set, Map, Seq, relation, or struct fields";
    let directory = std::env::temp_dir().join(format!(
        "fsl-lsp-nested-option-payloads-{}",
        std::process::id()
    ));
    std::fs::create_dir_all(&directory).expect("create nested Option fixture directory");

    for (position, payload) in [
        ("state", "Inner"),
        ("state", "Set<Bit>"),
        ("state", "Map<Key, Bit>"),
        ("state", "Seq<Bit, 1>"),
        ("state", "relation Key -> Key"),
        ("struct", "Inner"),
        ("struct", "Set<Bit>"),
        ("struct", "Map<Key, Bit>"),
        ("struct", "Seq<Bit, 1>"),
        ("struct", "relation Key -> Key"),
        ("map_value", "Inner"),
        ("map_value", "Set<Bit>"),
        ("map_value", "Map<Key, Bit>"),
        ("map_value", "Seq<Bit, 1>"),
        ("map_value", "relation Key -> Key"),
    ] {
        let (source, hint, expected_location) = match position {
            "state" => (
                format!(
                    "spec Unsupported {{\n  type Bit = 0..1\n  type Key = 0..1\n  struct Inner {{ value: Bit }}\n  state {{\n    value: Option<{payload}>\n  }}\n}}\n"
                ),
                STATE_HINT,
                (6, 5),
            ),
            "struct" => (
                format!(
                    "spec Unsupported {{\n  type Bit = 0..1\n  type Key = 0..1\n  struct Inner {{ value: Bit }}\n  struct Outer {{\n    value: Option<{payload}>\n  }}\n  state {{ outer: Outer }}\n}}\n"
                ),
                STRUCT_HINT,
                (5, 3),
            ),
            "map_value" => (
                format!(
                    "spec Unsupported {{\n  type Bit = 0..1\n  type Key = 0..1\n  struct Inner {{ value: Bit }}\n  state {{\n    values: Map<Key, Option<{payload}>>\n  }}\n}}\n"
                ),
                STATE_HINT,
                (6, 5),
            ),
            _ => unreachable!("listed unsupported nested Option position"),
        };
        let path =
            directory.join(format!("{position}-{payload}.fsl").replace(['<', '>', ',', ' '], "_"));
        std::fs::write(&path, &source).expect("write nested Option fixture");
        let path = path.to_str().expect("UTF-8 fixture path");
        let output = Command::new(env!("CARGO_BIN_EXE_fslc"))
            .args(["check", path])
            .output()
            .expect("run native check");
        assert_eq!(
            output.status.code(),
            Some(2),
            "{position} Option<{payload}>"
        );
        let cli: Value = serde_json::from_slice(&output.stdout).expect("parse CLI envelope");
        let resolver = fsl_core::FsResolver::new(Path::new(&directory));
        let shared = fslc_rust::source_diagnostic::diagnostics(
            &source,
            path,
            &resolver,
            &fsl_syntax::ParseOptions::stable(),
        )
        .into_iter()
        .find(|diagnostic| diagnostic.kind != "migration")
        .expect("shared source diagnostic");

        assert_eq!(cli["kind"], shared.kind, "{position} Option<{payload}>");
        assert_eq!(
            cli["message"], shared.message,
            "{position} Option<{payload}>"
        );
        assert_eq!(cli["hint"], hint, "{position} Option<{payload}>");
        assert_eq!(
            cli["loc"],
            shared.span.python_loc(),
            "{position} Option<{payload}>"
        );
        assert_eq!(
            (shared.span.start.line, shared.span.start.column),
            expected_location,
            "{position} Option<{payload}>"
        );
    }

    std::fs::remove_dir_all(directory).expect("remove nested Option fixture directory");
}
