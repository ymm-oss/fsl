// SPDX-License-Identifier: Apache-2.0

//! Detection-gate coupled change for issue #469: the missing
//! `column_removed_while_still_written` branch shipped undetected because no
//! fixture anywhere exercised it and no gate compared the finding `kind`s
//! native `fslc db check` can actually emit against the documented,
//! schema-enumerated set. This census mechanically enumerates every `kind`
//! reachable from `fslc db check` over the full `examples/db/` corpus plus
//! the write-drop regression fixtures under `tests/fixtures/` (kept out of
//! `examples/db/` so they do not appear in the frozen Python reference's
//! `tests/snapshots/corpus_snapshot.json` corpus-membership set) and diffs
//! it against the check-time subset of
//! `schemas/fslc/db/finding.v0.schema.json`'s `kind` enum, so a future
//! documented-but-unreachable check-time kind fails this test instead of
//! shipping silently green.
//!
//! Scope note: three schema kinds (`declared_unused_but_observed`,
//! `unsupported_artifact_observed`, `legacy_api_still_called`) are `fslc db
//! observe` runtime-evidence findings, never emitted by `db check`, and are
//! excluded from this static-check census on purpose.

use std::collections::BTreeSet;
use std::path::Path;
use std::process::Command;

use serde_json::Value;

fn workspace_root() -> &'static Path {
    Path::new(env!("CARGO_MANIFEST_DIR"))
        .parent()
        .and_then(Path::parent)
        .expect("workspace root")
}

fn run(args: &[&str]) -> Value {
    let output = Command::new(env!("CARGO_BIN_EXE_fslc"))
        .args(args)
        .current_dir(workspace_root())
        .output()
        .expect("run native CLI");
    serde_json::from_slice(&output.stdout).unwrap_or_else(|error| {
        panic!(
            "invalid JSON for {args:?}: {error}; stderr={}",
            String::from_utf8_lossy(&output.stderr)
        )
    })
}

fn collect_kinds(value: &Value, kinds: &mut BTreeSet<String>) {
    if let Some(findings) = value.get("findings").and_then(Value::as_array) {
        for finding in findings {
            if let Some(kind) = finding.get("kind").and_then(Value::as_str) {
                kinds.insert(kind.to_owned());
            }
        }
    }
}

const OBSERVE_ONLY_KINDS: [&str; 3] = [
    "declared_unused_but_observed",
    "unsupported_artifact_observed",
    "legacy_api_still_called",
];

#[test]
fn db_check_reaches_every_schema_documented_check_time_finding_kind() {
    let schema_source =
        std::fs::read_to_string(workspace_root().join("schemas/fslc/db/finding.v0.schema.json"))
            .expect("read fsl-db-finding schema");
    let schema: Value = serde_json::from_str(&schema_source).expect("parse finding schema");
    let documented_kinds = schema["properties"]["kind"]["enum"]
        .as_array()
        .expect("kind enum")
        .iter()
        .map(|kind| kind.as_str().expect("kind is a string").to_owned())
        .filter(|kind| !OBSERVE_ONLY_KINDS.contains(&kind.as_str()))
        .collect::<BTreeSet<_>>();
    assert!(
        documented_kinds.len() >= 8,
        "sanity: expected a substantial documented check-time kind enum, got {documented_kinds:?}"
    );

    let mut reached_kinds = BTreeSet::new();
    let db_examples_dir = workspace_root().join("examples/db");
    let mut fsl_files = std::fs::read_dir(&db_examples_dir)
        .expect("read examples/db")
        .filter_map(Result::ok)
        .map(|entry| entry.path())
        .filter(|path| path.extension().and_then(std::ffi::OsStr::to_str) == Some("fsl"))
        .collect::<Vec<_>>();
    fsl_files.sort();
    assert!(
        !fsl_files.is_empty(),
        "expected .fsl fixtures under examples/db"
    );

    for path in &fsl_files {
        let relative = path
            .strip_prefix(workspace_root())
            .expect("path under workspace root")
            .to_str()
            .expect("utf8 path");
        let value = run(&["db", "check", relative]);
        collect_kinds(&value, &mut reached_kinds);
    }

    // `column_removed_while_still_written` (issue #469) is exercised only by
    // these regression fixtures, deliberately kept out of `examples/db/` so
    // they do not enter the frozen Python reference's snapshot corpus.
    for name in [
        "issue_469_unsafe_drop_column_with_writer.fsl",
        "issue_469_unsafe_drop_column_with_writer_deep_history.fsl",
    ] {
        let path = Path::new(env!("CARGO_MANIFEST_DIR"))
            .join("tests/fixtures")
            .join(name);
        let value = run(&["db", "check", path.to_str().expect("utf8 path")]);
        collect_kinds(&value, &mut reached_kinds);
    }

    let missing = documented_kinds
        .difference(&reached_kinds)
        .collect::<Vec<_>>();
    assert!(
        missing.is_empty(),
        "finding kind(s) documented in schemas/fslc/db/finding.v0.schema.json are unreachable \
         from native `fslc db check` over the examples/db corpus: {missing:?}"
    );
}
