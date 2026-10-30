// SPDX-License-Identifier: Apache-2.0

//! #1473 preservation controls: a rule-vocabulary gate is not full DB
//! validation, and repair tools still accept their intentional input domain.
//! Kernel controls assert acceptance, not full Kernel JSON equivalence.

use std::path::{Path, PathBuf};
use std::process::{Command, Output};
use std::sync::atomic::{AtomicUsize, Ordering};

use serde_json::Value;

// Independent language-contract oracle, deliberately not a production const.
const RULES: &[&str] = &[
    "all_active_reads_exist",
    "all_active_writes_exist",
    "api_calls_accepted",
    "api_responses_expected",
    "artifact_capabilities_provided",
    "data_preserved",
    "destructive_operations_annotated",
    "not_null_after_backfill",
    "offline_payloads_accepted",
    "preservation_transforms_annotated",
    "removed_only_after_unused",
    "rollback_equivalent",
];
const INVALID_DB: &str = include_str!("fixtures/error_envelope_db_invalid_rule.fsl");
const RULE_BLOCK: &str =
    "\n  check compatibility {\n    rule unregistered_compatibility_rule;\n  }\n";
const DB04: &str = "// SPDX-License-Identifier: Apache-2.0\n\ndbsystem Db04 {\n  database app {\n    schema 0\n    table users { column id: Int present backfilled not_null; column nickname: Text absent; }\n  }\n  migration add_nickname from 0 to 1 { add users.nickname nullable; }\n  artifact server_v1 { reads users.id; }\n  environment prod { schema 0..2; active server_v1 when schema 0..1; }\n}\n";

static SEQUENCE: AtomicUsize = AtomicUsize::new(0);

#[path = "support/scratch_identity.rs"]
mod scratch_identity;
struct FixtureDirectory(PathBuf);

impl FixtureDirectory {
    fn new() -> Self {
        let path = std::env::temp_dir().join(format!(
            "fslc-specialized-preservation-{}-{}",
            std::process::id(),
            SEQUENCE.fetch_add(1, Ordering::Relaxed)
        ));
        let path = scratch_identity::unique(path);
        std::fs::create_dir(&path).expect("test-owned directory");
        Self(path)
    }

    fn write(&self, name: &str, source: &str) {
        std::fs::write(self.0.join(name), source).expect("test-owned fixture");
    }

    fn run(&self, arguments: &[&str]) -> Output {
        run(&self.0, arguments)
    }
}

impl Drop for FixtureDirectory {
    fn drop(&mut self) {
        let _ = std::fs::remove_dir_all(&self.0);
    }
}

fn run(cwd: &Path, arguments: &[&str]) -> Output {
    let output = Command::new(env!("CARGO_BIN_EXE_fslc"))
        .args(arguments)
        .current_dir(cwd)
        .output()
        .expect("run fslc");
    assert!(
        output.stderr.is_empty(),
        "{arguments:?}: {:?}",
        output.stderr
    );
    output
}

fn json(output: &Output) -> Value {
    serde_json::from_slice(&output.stdout).unwrap_or_else(|error| {
        panic!(
            "JSON stdout: {error}: {}",
            String::from_utf8_lossy(&output.stdout)
        )
    })
}

fn native_versions_match(output: &Value) -> bool {
    // Native check metadata is independent of cache use. Keep this oracle
    // separate from the production metadata builder and compare every field.
    output.get("versions")
        == Some(&serde_json::json!({
            "verifier": {"name": "fslc-rust", "version": env!("CARGO_PKG_VERSION")},
            "core": {"name": "fsl-core", "version": env!("CARGO_PKG_VERSION")},
            "solver": {"name": "z3", "backend": "native-z3", "version": "Z3 4.16.0.0"}
        }))
}

#[test]
fn native_versions_matcher_rejects_missing_wrong_and_extra_metadata() {
    let valid = serde_json::json!({"versions": {
        "verifier": {"name": "fslc-rust", "version": env!("CARGO_PKG_VERSION")},
        "core": {"name": "fsl-core", "version": env!("CARGO_PKG_VERSION")},
        "solver": {"name": "z3", "backend": "native-z3", "version": "Z3 4.16.0.0"}
    }});
    assert!(native_versions_match(&valid));
    assert!(!native_versions_match(&serde_json::json!({})));
    for component in ["verifier", "core", "solver"] {
        let mut wrong = valid.clone();
        wrong["versions"][component]["version"] = "incorrect".into();
        assert!(!native_versions_match(&wrong), "{component}: {wrong}");
    }
    let mut extra = valid;
    extra["versions"]["unexpected"] = true.into();
    assert!(!native_versions_match(&extra), "{extra}");
}

fn assert_success(output: &Output, result: &str, cell: &str) {
    assert_eq!(
        output.status.code(),
        Some(0),
        "{cell}: {}",
        String::from_utf8_lossy(&output.stdout)
    );
    let value = json(output);
    assert_eq!(value["fsl"], "1.0", "{cell}: {value}");
    assert_eq!(value["result"], result, "{cell}: {value}");
}

// Byte-for-byte stdout baselines, including all keys, order and whitespace.
// Inputs have a stable relative label; no output field is excluded.
const LINT_STDOUT: &str = r#"{
  "fsl": "1.0",
  "result": "lint",
  "edition": "current",
  "finding_count": 0,
  "id_policy": {
    "source": "builtin",
    "patterns": {
      "requirement": [
        "REQ-{scope}-{number:3}",
        "NFR-{scope}-{number:3}",
        "INV-{scope}-{number:3}"
      ],
      "acceptance": [
        "AC-{scope}-{number:3}"
      ],
      "forbidden": [
        "FB-{scope}-{number:3}"
      ],
      "policy": [
        "POL-{scope}-{number:3}"
      ],
      "goal": [
        "GOAL-{scope}-{number:3}"
      ],
      "control": [
        "CTRL-{scope}-{number:3}"
      ],
      "model": [
        "MODEL-{scope}-{number:3}"
      ],
      "assumption": [
        "ASSUME-{scope}-{number:3}"
      ]
    }
  },
  "files": [
    {
      "path": "case.fsl",
      "findings": []
    }
  ]
}
"#;

const MIGRATE_STDOUT: &str = r#"{
  "fsl": "1.0",
  "result": "migrated",
  "edition": "next",
  "changed": 0,
  "written": false,
  "files": [
    {
      "path": "case.fsl",
      "changed": false,
      "findings": [],
      "edits": []
    }
  ]
}
"#;

#[test]
fn supported_db_rules_omitted_and_empty_checks_preserve_check_kernel_and_repair_tools() {
    let mut cases = RULES
        .iter()
        .map(|rule| {
            (
                (*rule).to_owned(),
                INVALID_DB.replace("unregistered_compatibility_rule", rule),
            )
        })
        .collect::<Vec<_>>();
    assert!(INVALID_DB.contains(RULE_BLOCK));
    cases.push(("omitted".to_owned(), INVALID_DB.replace(RULE_BLOCK, "")));
    cases.push((
        "empty".to_owned(),
        INVALID_DB.replace("    rule unregistered_compatibility_rule;\n", ""),
    ));
    assert_eq!(cases.len(), 14);
    let directory = FixtureDirectory::new();
    for (name, source) in cases {
        directory.write("case.fsl", &source);
        assert_success(
            &directory.run(&["check", "case.fsl"]),
            "ok",
            &format!("{name}/check"),
        );
        for version in ["1", "2"] {
            assert_success(
                &directory.run(&["kernel", "case.fsl", "--kernel-version", version]),
                "kernel",
                &format!("{name}/kernel v{version}"),
            );
        }
        for (command, edition, expected) in [
            ("fmt", "current", source.as_str()),
            ("lint", "current", LINT_STDOUT),
            ("migrate", "next", MIGRATE_STDOUT),
        ] {
            let actual = directory.run(&[command, "case.fsl", "--edition", edition]);
            assert_eq!(
                actual.status.code(),
                Some(0),
                "{name}/{command}: {}",
                String::from_utf8_lossy(&actual.stdout)
            );
            assert_eq!(
                actual.stdout,
                expected.as_bytes(),
                "{name}/{command}: full stdout changed"
            );
        }
        assert_eq!(
            std::fs::read_to_string(directory.0.join("case.fsl")).expect("unchanged input"),
            source
        );
        eprintln!(
            "1473 PASS preservation/{name}: check=0 kernel-v1=0 kernel-v2=0 fmt/lint/migrate=0 full stdout matches"
        );
    }
}

#[test]
fn db_only_unreachable_schema_rejection_does_not_become_a_kernel_gate() {
    let directory = FixtureDirectory::new();
    directory.write("db04.fsl", DB04);
    let checked = directory.run(&["check", "db04.fsl"]);
    assert_eq!(checked.status.code(), Some(2));
    let checked = json(&checked);
    for (key, expected) in [
        ("fsl", "1.0"),
        ("result", "error"),
        ("kind", "semantics"),
        (
            "message",
            "environment 'prod' includes schema 2, which is not reachable in the declared migration plan",
        ),
    ] {
        assert_eq!(checked[key], expected, "{checked}");
    }
    assert!(checked.get("loc").is_none(), "{checked}");
    assert!(native_versions_match(&checked), "{checked}");
    for version in ["1", "2"] {
        assert_success(
            &directory.run(&["kernel", "db04.fsl", "--kernel-version", version]),
            "kernel",
            &format!("Db04/kernel v{version}"),
        );
    }
    eprintln!("1473 PASS preservation/Db04: check=2 exact error kernel-v1=0 kernel-v2=0");
}

// This agent is check-accepted. It is only a preservation control, not
// evidence that the repair-tool carve-out is needed (see refinement below).
#[test]
fn check_accepted_agent_preserves_lint_and_migrate_success() {
    let root = Path::new(env!("CARGO_MANIFEST_DIR"))
        .parent()
        .and_then(Path::parent)
        .expect("root");
    let path = "examples/ai/recursive_support_agent.fsl";
    let checked = run(root, &["check", path]);
    assert_success(&checked, "ok", "agent/check");
    for command in ["lint", "migrate"] {
        assert_success(
            &run(root, &[command, path, "--edition", "current"]),
            if command == "lint" {
                "lint"
            } else {
                "migrated"
            },
            command,
        );
    }
    eprintln!("1473 PASS preservation/agent: check=0 lint=0 migrate=0 (preservation only)");
}

// A full check preflight would reject this intentional repair-tool input.
// formatter_cli also covers stdin + the opaque agent FSL-FMT-UNSAFE refusal;
// edition_migrator covers the independent legacy-refused-plan bypass.
#[test]
fn check_rejected_refinement_preserves_fmt_lint_and_migrate_carve_out() {
    let root = Path::new(env!("CARGO_MANIFEST_DIR"))
        .parent()
        .and_then(Path::parent)
        .expect("root");
    let path = "specs/cart_refines.fsl";
    let checked = run(root, &["check", path]);
    assert_eq!(checked.status.code(), Some(2));
    let checked = json(&checked);
    for (key, expected) in [
        ("fsl", "1.0"),
        ("result", "error"),
        ("kind", "semantics"),
        ("message", "spec has no state block"),
    ] {
        assert_eq!(checked[key], expected, "{checked}");
    }
    assert!(checked.get("loc").is_none(), "{checked}");
    assert!(native_versions_match(&checked), "{checked}");
    let formatted = "refinement CartImplRefinesCart {\n  impl CartImpl\n  abs ShoppingCart\n\n  map stock[i: ItemId] = impl_stock[i] - reserved[i]\n  map cart[u: UserId] = impl_cart[u]\n\n  action add_to_cart(u, i) -> add_to_cart(u, i)\n  action remove_from_cart(u) -> remove_from_cart(u)\n  action impl_checkout(u) -> checkout(u)\n  action reserve(i) -> stutter\n}\n";
    let lint = LINT_STDOUT.replace("case.fsl", path);
    let migrate = MIGRATE_STDOUT.replace("case.fsl", path);
    for (command, edition, expected) in [
        ("fmt", "current", formatted),
        ("lint", "current", lint.as_str()),
        ("migrate", "next", migrate.as_str()),
    ] {
        let actual = run(root, &[command, path, "--edition", edition]);
        assert_eq!(
            actual.status.code(),
            Some(0),
            "{command}: {}",
            String::from_utf8_lossy(&actual.stdout)
        );
        assert_eq!(
            actual.stdout,
            expected.as_bytes(),
            "refinement/{command}: full stdout changed"
        );
    }
    eprintln!(
        "1473 PASS preservation/refinement: check=2 exact error fmt/lint/migrate=0 full stdout matches (carve-out control)"
    );
}

fn assert_db_rejection_preserves_bytes(output: &Output, path: &str, bytes_unchanged: bool) {
    let expected_message = format!(
        "unknown db compatibility rule 'unregistered_compatibility_rule'; supported rules: {} at {path}:12:5",
        RULES.join(", ")
    );
    eprintln!(
        "1473 artifact-preservation/{path}: expected exit=2 bytes_unchanged=true; produced exit={:?} bytes_unchanged={bytes_unchanged} stdout={:?}",
        output.status.code(),
        String::from_utf8_lossy(&output.stdout)
    );
    // Check bytes first as well as the exit: a successful base invocation can
    // overwrite an existing artifact, which is a distinct detector signal.
    assert!(
        bytes_unchanged,
        "{path}: rejected input/output bytes changed"
    );
    assert_eq!(output.status.code(), Some(2), "{path}: expected rejection");
    let actual = json(output);
    for (key, expected) in [
        ("fsl", "1.0"),
        ("result", "error"),
        ("kind", "semantics"),
        ("message", expected_message.as_str()),
    ] {
        assert_eq!(actual[key], expected, "{path}: {actual}");
    }
}

#[test]
fn invalid_db_testgen_preserves_an_existing_output_sentinel() {
    let directory = FixtureDirectory::new();
    directory.write("invalid.fsl", INVALID_DB);
    let sentinel = b"existing generated artifact\n\x00must not be truncated\xff\n";
    std::fs::write(directory.0.join("existing.py"), sentinel).expect("sentinel");
    let output = directory.run(&["testgen", "invalid.fsl", "-o", "existing.py"]);
    let after = std::fs::read(directory.0.join("existing.py")).expect("output still exists");
    assert_db_rejection_preserves_bytes(&output, "invalid.fsl", after == sentinel);
}

#[test]
fn invalid_db_fmt_preserves_its_test_owned_input() {
    let directory = FixtureDirectory::new();
    directory.write("fmt-copy.fsl", INVALID_DB);
    // fmt has stdout/check modes only, not a --write mode.
    let output = directory.run(&["fmt", "fmt-copy.fsl"]);
    let after = std::fs::read(directory.0.join("fmt-copy.fsl")).expect("input still exists");
    assert_db_rejection_preserves_bytes(&output, "fmt-copy.fsl", after == INVALID_DB.as_bytes());
}

#[test]
fn invalid_db_migrate_write_preserves_its_test_owned_input() {
    let directory = FixtureDirectory::new();
    directory.write("migrate-copy.fsl", INVALID_DB);
    let output = directory.run(&["migrate", "migrate-copy.fsl", "--write"]);
    let after = std::fs::read(directory.0.join("migrate-copy.fsl")).expect("input still exists");
    assert_db_rejection_preserves_bytes(
        &output,
        "migrate-copy.fsl",
        after == INVALID_DB.as_bytes(),
    );
}
