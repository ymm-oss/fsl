// SPDX-License-Identifier: Apache-2.0

use std::path::Path;
use std::process::Command;

use serde_json::Value;

#[test]
fn cli_and_lsp_source_diagnostics_share_identity_without_changing_cli_envelopes() {
    let directory =
        std::env::temp_dir().join(format!("fsl-lsp-diagnostics-{}", std::process::id()));
    std::fs::create_dir_all(&directory).expect("create diagnostic fixture directory");
    let cases = [
        ("parse", "spec Broken { state {", None, None),
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
        )
        .into_iter()
        .find(|diagnostic| diagnostic.kind != "migration")
        .expect("shared source diagnostic");

        assert_eq!(cli["kind"], shared.kind, "{name}");
        assert_eq!(cli["message"], shared.message, "{name}");
        assert_eq!(cli["hint"].as_str(), expected_hint, "{name}");
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
        let shared = fslc_rust::source_diagnostic::diagnostics(&source, path, &resolver)
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
