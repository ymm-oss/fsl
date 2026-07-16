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
fn concurrent_literate_commands_keep_their_materializations_isolated() {
    let dir = std::env::temp_dir().join(format!("fslc-literate-concurrent-{}", std::process::id()));
    let _ = std::fs::create_dir_all(&dir);
    let document = dir.join("literate_toggle.md");
    std::fs::copy(fixture_path("literate_toggle.md"), &document).expect("copy literate fixture");
    let path = document.to_str().expect("UTF-8 path").to_owned();
    let barrier = std::sync::Arc::new(std::sync::Barrier::new(6));
    let results = std::thread::scope(|scope| {
        let handles = (0..6)
            .map(|index| {
                let path = path.clone();
                let barrier = std::sync::Arc::clone(&barrier);
                scope.spawn(move || {
                    barrier.wait();
                    match index % 3 {
                        0 => run_cli(&["check", &path]),
                        1 => run_cli(&["verify", &path, "--depth", "4", "--no-cache"]),
                        _ => run_cli(&["scenarios", &path, "--depth", "4"]),
                    }
                })
            })
            .collect::<Vec<_>>();
        handles
            .into_iter()
            .map(|handle| handle.join().expect("literate command thread"))
            .collect::<Vec<_>>()
    });

    for (output, status) in results {
        assert_eq!(status, 0, "concurrent literate command failed: {output:#}");
        assert_ne!(output["result"], "error", "unexpected error: {output:#}");
    }
    assert!(
        !has_literate_sibling(&dir),
        "concurrent commands leaked a materialized source"
    );
    let _ = std::fs::remove_dir_all(dir);
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

/// Isolates the verify cache in a fresh, per-test directory (same pattern as
/// `issue_226_auto_engine.rs`'s `CacheDir`).
struct LiterateCacheDir {
    path: PathBuf,
}

impl LiterateCacheDir {
    fn new(name: &str) -> Self {
        let path = std::env::temp_dir().join(format!(
            "fslc-literate-cachedir-{name}-{}",
            std::process::id()
        ));
        let _ = std::fs::remove_dir_all(&path);
        Self { path }
    }

    fn run(&self, arguments: &[&str]) -> (Value, i32) {
        let output = Command::new(env!("CARGO_BIN_EXE_fslc"))
            .args(arguments)
            .current_dir(repository_root())
            .env("FSLC_CACHE_DIR", &self.path)
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
}

impl Drop for LiterateCacheDir {
    fn drop(&mut self) {
        let _ = std::fs::remove_dir_all(&self.path);
    }
}

#[test]
fn literate_verify_cache_hits_on_the_second_run() {
    // The physical materialization is process-owned, but the cache identity is
    // the original Markdown path and transient siblings are excluded from the
    // dependency walk. The fsl content below embeds a fresh nonce so this test's
    // cache entry cannot collide with any other test's.
    let cache = LiterateCacheDir::new("hit");
    let dir = std::env::temp_dir().join(format!("fslc-literate-cache-doc-{}", std::process::id()));
    let _ = std::fs::create_dir_all(&dir);
    let nonce = std::time::SystemTime::now()
        .duration_since(std::time::UNIX_EPOCH)
        .expect("clock after epoch")
        .as_nanos();
    let doc = dir.join("cache_toggle.md");
    std::fs::write(
        &doc,
        format!(
            "\
# Cache toggle

```fsl
// nonce: {nonce}
spec CacheToggle {{
  state {{ active: Bool }}
  init {{ active = false }}
  action toggle() {{ active = not active }}
  invariant AlwaysBool {{ active or not active }}
}}
```
"
        ),
    )
    .expect("write cache-key test doc");
    let doc_str = doc.to_str().expect("UTF-8 path");

    let (first, status) = cache.run(&["verify", doc_str, "--depth", "4"]);
    assert_eq!(status, 0, "first run failed: {first:#}");
    assert!(
        first.get("cache").is_none(),
        "first run should not report a cache hit: {first:#}"
    );

    let (second, status) = cache.run(&["verify", doc_str, "--depth", "4"]);
    assert_eq!(status, 0, "second run failed: {second:#}");
    assert_eq!(
        second["cache"]["hit"], true,
        "second run should hit the verify cache: {second:#}"
    );

    let _ = std::fs::remove_dir_all(&dir);
}

#[test]
fn literate_check_edition_finding_names_the_markdown_file_not_the_materialization() {
    // Wraps `tests/fixtures/domain_legacy_enum_union.fsl` (which produces a
    // `deprecated_domain_enum_union` warning under the default "current"
    // edition — see `issue_244_domain_enum.rs`) in a literate `.md` fence and
    // asserts the finding's `loc.file` names the `.md` path, never the
    // transient `.literate.fsl` materialization `apply_domain_edition` reads
    // from.
    let dir = std::env::temp_dir().join(format!("fslc-literate-edition-{}", std::process::id()));
    let _ = std::fs::create_dir_all(&dir);
    let inner = std::fs::read_to_string(
        Path::new(env!("CARGO_MANIFEST_DIR")).join("tests/fixtures/domain_legacy_enum_union.fsl"),
    )
    .expect("read domain enum-union fixture");
    let doc = dir.join("legacy_enum.md");
    std::fs::write(&doc, format!("# Legacy enum\n\n```fsl\n{inner}```\n")).expect("write doc");
    let doc_str = doc.to_str().expect("UTF-8 path");

    let (output, status) = run_cli(&["check", doc_str]);
    assert_eq!(status, 0, "check failed: {output:#}");
    let warning = output["warnings"]
        .as_array()
        .and_then(|warnings| {
            warnings
                .iter()
                .find(|warning| warning["code"] == "deprecated_domain_enum_union")
        })
        .unwrap_or_else(|| panic!("missing deprecation warning: {output:#}"));
    assert_eq!(
        warning["loc"]["file"], doc_str,
        "finding file field should name the .md document: {output:#}"
    );
    let file_field = warning["loc"]["file"].as_str().expect("file field");
    assert!(
        !file_field.contains("literate.fsl"),
        "finding file field must not leak the materialized sibling: {output:#}"
    );

    let _ = std::fs::remove_dir_all(&dir);
}

#[test]
fn four_backtick_fence_around_a_triple_backtick_fsl_example_verifies_correctly() {
    // Regression for the CommonMark fence-length bug: a four-backtick "other"
    // fence containing a literal ```fsl example used to be mis-tracked (the
    // old code only ever recognized exactly-3-backtick closers), corrupting
    // extraction and producing a confusing parse error instead of the real
    // verdict. Per CommonMark, the final fsl block below is real spec code:
    // `n` can reach 2 via repeated `inc()`, violating `Low`.
    let dir = std::env::temp_dir().join(format!("fslc-literate-fourtick-{}", std::process::id()));
    let _ = std::fs::create_dir_all(&dir);
    let doc = dir.join("four_backtick.md");
    std::fs::write(
        &doc,
        "\
# Spec

```fsl
spec Counter {
  state { n: 0..3 }
  init { n = 0 }
  action inc() { n = n + 1 }
```

Example (four-backtick fence, inner three backticks are literal):

````text
```fsl
example only
```
````

```fsl
  invariant Low { n < 2 }
}
```
",
    )
    .expect("write four-backtick repro doc");
    let doc_str = doc.to_str().expect("UTF-8 path");

    let (output, status) = run_cli(&["verify", doc_str, "--depth", "6", "--no-cache"]);
    assert_eq!(
        output["result"], "violated",
        "expected violated: {output:#}"
    );
    assert_eq!(status, 1, "{output:#}");

    let _ = std::fs::remove_dir_all(&dir);
}
