// SPDX-License-Identifier: Apache-2.0
// Copyright 2026 Ryoichi Izumita

use std::path::{Path, PathBuf};
use std::process::Command;

use serde_json::Value;

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

fn repository_root() -> PathBuf {
    Path::new(env!("CARGO_MANIFEST_DIR"))
        .parent()
        .and_then(Path::parent)
        .expect("repository root")
        .to_path_buf()
}

fn fixture_path(name: &str) -> String {
    Path::new(env!("CARGO_MANIFEST_DIR"))
        .join("tests/fixtures")
        .join(name)
        .to_str()
        .expect("UTF-8 fixture path")
        .to_owned()
}

#[test]
fn literate_check_succeeds_on_valid_spec() {
    let path = fixture_path("literate_toggle.md");
    let (output, status) = run_cli(&["check", &path]);
    assert_eq!(status, 0, "check failed: {output:#}");
    assert_eq!(output["result"], "ok");
    assert_eq!(output["spec"], "Toggle");
}

#[test]
fn literate_verify_produces_a_bounded_verdict() {
    let path = fixture_path("literate_toggle.md");
    let (output, status) = run_cli(&["verify", &path, "--depth", "4", "--no-cache"]);
    assert_eq!(status, 0, "verify failed: {output:#}");
    assert_eq!(output["result"], "verified");
    assert_eq!(output["completeness"], "bounded");
}

#[test]
fn literate_parse_error_loc_points_to_the_markdown_line() {
    let dir = std::env::temp_dir().join(format!("fslc-literate-{}", std::process::id()));
    let _ = std::fs::create_dir_all(&dir);
    let bad = dir.join("bad.md");
    std::fs::write(
        &bad,
        "# Bad spec\n\n```fsl\nspec Bad {\n  state { x: Bool\n}\n```\n",
    )
    .unwrap();
    let (output, status) = run_cli(&["check", bad.to_str().unwrap()]);
    assert_eq!(status, 2);
    assert_eq!(output["result"], "error");
    assert_eq!(output["kind"], "parse");
    // The closing brace is at md line 6, the ``` at 7, EOF at 8 — the parser
    // should report an error on a line >= 4 (inside or past the fsl block),
    // not on line 1 (which would mean position mapping is broken).
    let line = output["loc"]["line"].as_u64().expect("error loc line");
    assert!(
        line >= 4,
        "error loc should point into the fsl block: {output:#}"
    );
    let _ = std::fs::remove_dir_all(&dir);
}

#[test]
fn markdown_without_fsl_fences_is_rejected() {
    let dir = std::env::temp_dir().join(format!("fslc-literate-nofsl-{}", std::process::id()));
    let _ = std::fs::create_dir_all(&dir);
    let readme = dir.join("readme.md");
    std::fs::write(&readme, "# Just a readme\n\nNo fsl here.\n").unwrap();
    let (output, status) = run_cli(&["check", readme.to_str().unwrap()]);
    assert_eq!(status, 2);
    assert_eq!(output["result"], "error");
    assert!(
        output["message"]
            .as_str()
            .is_some_and(|message| message.contains("does not contain any")),
        "expected fsl-fence-missing error: {output:#}"
    );
    let _ = std::fs::remove_dir_all(&dir);
}

#[test]
fn non_fsl_fenced_blocks_are_ignored() {
    let dir = std::env::temp_dir().join(format!("fslc-literate-other-{}", std::process::id()));
    let _ = std::fs::create_dir_all(&dir);
    let doc = dir.join("python_only.md");
    std::fs::write(&doc, "# Python example\n\n```python\nprint('hello')\n```\n").unwrap();
    let (output, status) = run_cli(&["check", doc.to_str().unwrap()]);
    assert_eq!(status, 2);
    assert!(
        output["message"]
            .as_str()
            .is_some_and(|message| message.contains("does not contain any")),
        "python-only doc should be rejected: {output:#}"
    );
    let _ = std::fs::remove_dir_all(&dir);
}

#[test]
fn multi_block_spec_matches_single_block_verification() {
    let multi_path = fixture_path("literate_toggle.md");

    let single_dir =
        std::env::temp_dir().join(format!("fslc-literate-single-{}", std::process::id()));
    let _ = std::fs::create_dir_all(&single_dir);
    let single = single_dir.join("toggle_single.md");
    std::fs::write(
        &single,
        "\
# Toggle

```fsl
spec Toggle {
  state { active: Bool }
  init  { active = false }
  action toggle() {
    active = not active
  }
  invariant AlwaysBool {
    active or not active
  }
}
```
",
    )
    .unwrap();

    let (multi, multi_status) = run_cli(&["verify", &multi_path, "--depth", "4", "--no-cache"]);
    let (single, single_status) = run_cli(&[
        "verify",
        single.to_str().unwrap(),
        "--depth",
        "4",
        "--no-cache",
    ]);

    assert_eq!(multi_status, single_status);
    assert_eq!(multi["result"], single["result"]);
    assert_eq!(multi["completeness"], single["completeness"]);
    let _ = std::fs::remove_dir_all(&single_dir);
}

#[test]
fn materialized_file_is_cleaned_up_after_check_and_verify() {
    let dir = std::env::temp_dir().join(format!("fslc-literate-cleanup-{}", std::process::id()));
    let _ = std::fs::create_dir_all(&dir);
    let doc = dir.join("cleanup_test.md");
    std::fs::copy(
        Path::new(env!("CARGO_MANIFEST_DIR")).join("tests/fixtures/literate_toggle.md"),
        &doc,
    )
    .expect("copy fixture");
    let doc_str = doc.to_str().expect("UTF-8 path");

    let _ = run_cli(&["check", doc_str]);
    assert!(
        !has_literate_sibling(&dir),
        "materialized file leaked after check"
    );

    let _ = run_cli(&["verify", doc_str, "--depth", "2", "--no-cache"]);
    assert!(
        !has_literate_sibling(&dir),
        "materialized file leaked after verify"
    );

    let _ = std::fs::remove_dir_all(&dir);
}

fn has_literate_sibling(directory: &Path) -> bool {
    std::fs::read_dir(directory)
        .into_iter()
        .flatten()
        .any(|entry| {
            entry.is_ok_and(|entry| {
                entry
                    .file_name()
                    .to_str()
                    .is_some_and(|name| name.contains("literate.fsl"))
            })
        })
}

#[test]
fn literate_scenarios_produces_output() {
    let path = fixture_path("literate_toggle.md");
    let (output, status) = run_cli(&["scenarios", &path, "--depth", "4"]);
    assert_eq!(status, 0, "scenarios failed: {output:#}");
    assert!(
        output.get("scenarios").is_some() || output.get("result").is_some(),
        "scenarios should produce structured output: {output:#}"
    );
}
