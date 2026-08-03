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
                let path = entry.path();
                path.extension()
                    .and_then(std::ffi::OsStr::to_str)
                    .is_some_and(|extension| extension.eq_ignore_ascii_case("fsl"))
                    && entry
                        .file_name()
                        .to_str()
                        .is_some_and(|name| name.contains(".literate-"))
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

// --- Issue #665: unsupported commands fail closed as an input-kind error --

/// The Markdown fixture the issue itself reproduced against
/// (`examples/literate/toggle.md`), not the `tests/fixtures` copy used above.
fn examples_literate_toggle() -> String {
    repository_root()
        .join("examples/literate/toggle.md")
        .to_str()
        .expect("UTF-8 path")
        .to_owned()
}

/// `command_key`'s space-joined pieces, followed by `doc` as its positional
/// spec argument(s). `refine` alone needs three positionals (`IMPL ABS
/// MAPPING`) before its own arm ever reaches the literate-input decision, so
/// it repeats `doc` three times; every other registered command's decision
/// point sits before any other required argument.
fn minimal_arguments<'a>(command_key: &'a str, doc: &'a str) -> Vec<&'a str> {
    let mut arguments: Vec<&str> = command_key.split(' ').collect();
    if command_key == "refine" {
        arguments.extend([doc, doc, doc]);
    } else {
        arguments.push(doc);
    }
    arguments
}

/// Positive: the registry's `Supported` commands still accept
/// `examples/literate/toggle.md` unchanged. Exercises
/// [`fslc_rust::literate_access::LITERATE_SUPPORTED_COMMANDS`] directly so a
/// command added to that list without a matching case here is at least
/// asserted not to error, even before a dedicated test is written for it.
#[test]
fn registry_supported_commands_still_accept_the_measured_repro_file() {
    let doc = examples_literate_toggle();
    for &command in fslc_rust::literate_access::LITERATE_SUPPORTED_COMMANDS {
        let (output, status) = match command {
            "verify" => run_cli(&["verify", &doc, "--depth", "4", "--no-cache"]),
            "scenarios" => run_cli(&["scenarios", &doc, "--depth", "4"]),
            _ => run_cli(&[command, &doc]),
        };
        assert_eq!(status, 0, "{command}: {output:#}");
        assert_ne!(
            output["result"], "error",
            "{command} should still accept literate input: {output:#}"
        );
    }
}

/// Negative control per unsupported command, driven entirely from
/// [`fslc_rust::literate_access::LITERATE_REGISTRY`] rather than a
/// hand-copied command list (issue #665 design constraint 2): a command
/// newly registered as `Unsupported` is covered by this test the moment it
/// is added, with no test-file edit required.
///
/// Also the test that "the lie must not come back": the message must never
/// contain the Markdown's own `1:2` position, and the diagnostic code must
/// never be `FSL-PARSE` -- the two symptoms the issue reported before this
/// fix. This is what fails if a future change re-routes an unsupported
/// command back into the surface parser.
#[test]
fn every_unsupported_registry_command_fails_closed_as_an_input_kind_error() {
    use fslc_rust::literate_access::{LITERATE_REGISTRY, LiterateSupport};

    let doc = examples_literate_toggle();
    let mut exercised = 0;
    for (command_key, support) in LITERATE_REGISTRY {
        if *support != LiterateSupport::Unsupported {
            continue;
        }
        exercised += 1;
        let arguments = minimal_arguments(command_key, &doc);
        let (output, status) = run_cli(&arguments);

        assert_eq!(
            status, 2,
            "{command_key}: exit code must not move: {output:#}"
        );
        assert_eq!(output["result"], "error", "{command_key}: {output:#}");
        assert_eq!(
            output["diagnostic_code"], "FSL-INPUT-LITERATE-UNSUPPORTED",
            "{command_key}: {output:#}"
        );
        assert_ne!(
            output["kind"], "parse",
            "{command_key}: kind must not be parse: {output:#}"
        );
        assert_ne!(
            output["kind"], "format",
            "{command_key}: kind must not be format: {output:#}"
        );
        let message = output["message"]
            .as_str()
            .unwrap_or_else(|| panic!("{command_key} missing message: {output:#}"));
        for supported in fslc_rust::literate_access::LITERATE_SUPPORTED_COMMANDS {
            assert!(
                message.contains(*supported),
                "{command_key}: message should name '{supported}': {message}"
            );
        }
        // loc identifies the input file, never a spec position.
        assert!(
            output["loc"].get("line").is_none() && output["loc"].get("column").is_none(),
            "{command_key}: loc must not carry a spec line/column: {output:#}"
        );
        assert_eq!(
            output["loc"]["file"], doc,
            "{command_key}: loc should name the input file: {output:#}"
        );
        // The lie must not come back.
        assert!(
            !message.contains("1:2"),
            "{command_key}: the Markdown-as-syntax-error lie returned: {message}"
        );
        assert_ne!(
            output["diagnostic_code"], "FSL-PARSE",
            "{command_key}: {output:#}"
        );
    }
    assert!(
        exercised >= 19,
        "expected at least the 19 commands issue #665 classified Unsupported, got {exercised}"
    );
}

/// Over-firing control: an ordinary `.fsl` spec must be unaffected on every
/// registered command, including the `Supported` ones -- the guard keys on
/// the input's extension, not on which command is running.
#[test]
fn ordinary_fsl_input_is_unaffected_on_every_registered_command() {
    use fslc_rust::literate_access::LITERATE_REGISTRY;

    let dir = std::env::temp_dir().join(format!("fslc-literate-overfire-{}", std::process::id()));
    let _ = std::fs::create_dir_all(&dir);
    let plain = dir.join("toggle.fsl");
    std::fs::write(
        &plain,
        "\
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
",
    )
    .expect("write plain fsl fixture");
    let plain_str = plain.to_str().expect("UTF-8 path").to_owned();

    // `testgen`/`html`/`ledger`/`document generate` print their generated
    // artifact raw to stdout on success instead of the JSON envelope
    // (`main.rs`'s `raw_delivery_allowed` bypass) -- redirect to a file with
    // `-o` so a *successful* run here still parses as JSON like every other
    // command's.
    let redirect = dir.join("out.txt");
    let redirect_str = redirect.to_str().expect("UTF-8 path");

    for (command_key, _) in LITERATE_REGISTRY {
        let mut arguments = minimal_arguments(command_key, &plain_str);
        if matches!(
            *command_key,
            "testgen" | "html" | "ledger" | "document generate"
        ) {
            arguments.extend(["-o", redirect_str]);
        }
        // `fmt` prints the raw formatted source on success unless `--check`
        // asks for the JSON envelope instead (same reason as the `-o`
        // redirects above).
        if *command_key == "fmt" {
            arguments.push("--check");
        }
        let (output, _status) = run_cli(&arguments);
        assert_ne!(
            output["diagnostic_code"], "FSL-INPUT-LITERATE-UNSUPPORTED",
            "{command_key}: the literate guard must not fire on a .fsl input: {output:#}"
        );
    }

    let _ = std::fs::remove_dir_all(&dir);
}

/// Unregistered-command gate (issue #665 design constraint 2): a command key
/// nobody classified must fail closed as an internal inconsistency rather
/// than silently falling through to either behavior. The registry's own
/// totality gate (`literate_registry_is_total_over_the_cli_surface` in
/// `rust/fslc/src/literate_access.rs`) is what actually keeps every real CLI
/// command classified; this test pins the *fallback* that function's
/// enumeration exists to make unreachable in production.
#[test]
fn an_unregistered_command_key_fails_closed_not_silently() {
    use fslc_rust::literate_access::literate_access;

    let doc = PathBuf::from(examples_literate_toggle());
    let Err((output, status)) = literate_access("a_command_nobody_registered", &doc) else {
        panic!("an unregistered command key must not resolve to Ok");
    };
    assert_eq!(status, 3, "{output:#}");
    assert_eq!(output["kind"], "internal", "{output:#}");
    assert_ne!(
        output["result"], "ok",
        "an internal inconsistency must never read as a pass: {output:#}"
    );
}
