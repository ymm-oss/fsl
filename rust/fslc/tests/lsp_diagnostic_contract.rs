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
            Some((1, 30)),
            None,
        ),
        (
            "type-hint",
            "spec Broken { type K = 0..1 struct Bag { members: Set<K> } state { bag: Bag } init { bag.members = Set {} } }",
            None,
            Some(
                "struct fields must be scalar (domain type, enum, Bool, Int) or Option<scalar>; use a separate Map for Set/Map/Seq/struct fields",
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
        } else if let Some(expected) = expected_location {
            assert_eq!((shared.span.start.line, shared.span.start.column), expected);
        }
        assert_eq!(shared.span.start.line, 1, "{name}");
        assert!(shared.span.start.offset <= source.len(), "{name}");
    }

    std::fs::remove_dir_all(directory).expect("remove diagnostic fixture directory");
}
