// SPDX-License-Identifier: Apache-2.0

use std::path::{Path, PathBuf};
use std::process::{Command, Output};
use std::sync::atomic::{AtomicU64, Ordering};
use std::time::{SystemTime, UNIX_EPOCH};

#[cfg(unix)]
use std::os::unix::fs::PermissionsExt as _;

use serde_json::Value;

struct FixtureDir(PathBuf);

static FIXTURE_SEQUENCE: AtomicU64 = AtomicU64::new(0);

impl FixtureDir {
    fn new() -> Self {
        let nonce = SystemTime::now()
            .duration_since(UNIX_EPOCH)
            .expect("clock after epoch")
            .as_nanos();
        let path = std::env::temp_dir().join(format!(
            "fsl-edition-migrator-{}-{nonce}-{}",
            std::process::id(),
            FIXTURE_SEQUENCE.fetch_add(1, Ordering::Relaxed),
        ));
        std::fs::create_dir(&path).expect("create fixture directory");
        Self(path)
    }

    fn write(&self, name: &str, source: &str) -> PathBuf {
        let path = self.0.join(name);
        std::fs::write(&path, source).expect("write fixture");
        path
    }
}

impl Drop for FixtureDir {
    fn drop(&mut self) {
        let _ = std::fs::remove_dir_all(&self.0);
    }
}

fn run(args: &[&str]) -> Output {
    Command::new(env!("CARGO_BIN_EXE_fslc"))
        .args(args)
        .output()
        .expect("run fslc")
}

fn json(output: &Output) -> Value {
    serde_json::from_slice(&output.stdout).unwrap_or_else(|error| {
        panic!(
            "invalid JSON: {error}; stderr={}",
            String::from_utf8_lossy(&output.stderr)
        )
    })
}

fn text(path: &Path) -> String {
    std::fs::read_to_string(path).expect("read fixture")
}

#[test]
fn dry_run_write_repeat_and_formatter_contracts_are_consistent() {
    let directory = FixtureDir::new();
    let original = r"domain Legacy {
  type Status = Pending | Done
  aggregate Job {
    state { status: Status }
    command Finish {}
    event Finished {}
    decide Finish {
      requires status == Pending || status == Done
      emits Finished
    }
    evolve Finished { status = Done }
    invariant valid { status == Pending -> status == Pending }
  }
}
";
    let path = directory.write("legacy.fsl", original);
    let path_text = path.to_str().expect("UTF-8 path");

    let dry = run(&["migrate", path_text, "--edition", "next"]);
    assert!(
        dry.status.success(),
        "{}",
        String::from_utf8_lossy(&dry.stdout)
    );
    let dry_json = json(&dry);
    assert_eq!(dry_json["result"], "migrated");
    assert_eq!(dry_json["written"], false);
    assert_eq!(text(&path), original);
    let codes = dry_json["files"][0]["findings"]
        .as_array()
        .expect("findings")
        .iter()
        .filter_map(|finding| finding["code"].as_str())
        .collect::<Vec<_>>();
    assert!(codes.contains(&"deprecated_domain_enum_union"));
    assert!(codes.contains(&"legacy_logical_operator"));
    assert!(codes.contains(&"implicit_initial_value"));

    let write = run(&["migrate", path_text, "--edition", "next", "--write"]);
    assert!(
        write.status.success(),
        "{}",
        String::from_utf8_lossy(&write.stdout)
    );
    assert_eq!(json(&write)["written"], true);
    let migrated = text(&path);
    assert!(migrated.contains("enum Status"));
    assert!(migrated.contains("status: Status = Pending"));
    assert!(migrated.contains(" or "));
    assert!(migrated.contains(" => "));

    let repeated = run(&["migrate", path_text, "--edition", "next"]);
    assert!(repeated.status.success());
    assert_eq!(json(&repeated)["changed"], 0);
    assert_eq!(text(&path), migrated);

    let formatted = run(&["fmt", path_text, "--check", "--edition", "next"]);
    assert!(
        formatted.status.success(),
        "{}",
        String::from_utf8_lossy(&formatted.stdout)
    );
    let checked = run(&["check", path_text, "--edition", "next"]);
    assert!(
        checked.status.success(),
        "{}",
        String::from_utf8_lossy(&checked.stdout)
    );
}

#[test]
fn legacy_quantifier_colon_migrates_to_braces() {
    let directory = FixtureDir::new();
    let path = directory.write(
        "quantifier.fsl",
        r"spec Quantifier {
  const MAX = 1
  type Id = 0..1
  state { ready: Map<Id, Bool> }
  init { forall i in 0..MAX: { ready[i] = false } }
  invariant Typed { forall i in 0..MAX: ready[i] == true or ready[i] == false }
}
",
    );
    let path_text = path.to_str().expect("UTF-8 path");
    let output = run(&["migrate", path_text, "--edition", "next", "--write"]);
    assert!(
        output.status.success(),
        "{}",
        String::from_utf8_lossy(&output.stdout)
    );
    assert!(
        json(&output)["files"][0]["findings"]
            .as_array()
            .expect("findings")
            .iter()
            .any(|finding| finding["code"] == "legacy_quantifier_colon")
    );
    let source = text(&path);
    assert!(!source.contains("MAX:"));
    assert!(source.contains("forall i in 0..MAX {"));
}

#[test]
fn string_metadata_uses_typed_annotations_without_changing_public_kernel() {
    let directory = FixtureDir::new();
    let path = directory.write(
        "metadata.fsl",
        r#"spec Tagged "safety: sample" {
  state { ready: Bool }
  init "undecided: initial policy" { ready = false }
  action publish() "REQ-PUBLISH: publishing is traceable" { ready = true }
  invariant Ready "REQ-READY: ready remains Boolean" { ready == true or ready == false }
}
"#,
    );
    let path_text = path.to_str().expect("UTF-8 path");
    let migrated = run(&["migrate", path_text, "--edition", "next", "--write"]);
    assert!(
        migrated.status.success(),
        "{}",
        String::from_utf8_lossy(&migrated.stdout)
    );
    let source = text(&path);
    assert!(source.contains("@kind(\"safety\", \"sample\")\nspec Tagged"));
    assert!(source.contains("@undecided(\"initial policy\")"));
    assert!(source.contains("@requirement(\"REQ-PUBLISH\", \"publishing is traceable\")"));
    assert!(!source.contains("\"REQ-READY:"));
}

#[test]
fn builtin_id_policy_reports_each_surface_kind_without_rewriting_ids() {
    let directory = FixtureDir::new();
    let path = directory.write(
        "ids.fsl",
        r#"requirements Checkout {
  requirement REQ-1 "requirement" { }
  action reject() { }
  acceptance AC-1 "acceptance" { expect true }
  forbidden NEG-1 "forbidden" { reject() expect rejected }
}
"#,
    );
    let path_text = path.to_str().expect("UTF-8 path");
    let output = run(&["lint", path_text]);
    assert_eq!(output.status.code(), Some(1));
    let output = json(&output);
    assert_eq!(output["id_policy"]["source"], "builtin");
    let findings = output["files"][0]["findings"].as_array().expect("findings");
    assert_eq!(
        findings
            .iter()
            .filter(|finding| finding["code"] == "non_canonical_id")
            .count(),
        3
    );
    assert!(findings.iter().all(|finding| {
        finding["code"] != "non_canonical_id"
            || (finding["taxonomy"] == "non_canonical"
                && finding["severity"] == "warning"
                && finding["machine_applicable"] == false
                && finding["edits"].as_array().is_some_and(Vec::is_empty))
    }));

    let migrate = run(&["migrate", path_text, "--edition", "next"]);
    assert!(migrate.status.success());
    assert_eq!(json(&migrate)["changed"], 0);
}

#[test]
fn project_id_policy_partially_overrides_builtin_templates() {
    let directory = FixtureDir::new();
    let project = directory.write(
        "fsl-project.toml",
        r#"[id_policy.patterns]
requirement = ["PAY-{number}", "NFR-{scope}-{number:3}"]
acceptance = "TEST-{number}"
"#,
    );
    let source = directory.write(
        "custom.fsl",
        r#"requirements Checkout {
  requirement PAY-42 "requirement" { }
  acceptance TEST-7 "acceptance" { expect true }
  action reject() { }
  forbidden FB-CHECKOUT-001 "forbidden" { reject() expect rejected }
}
"#,
    );
    let output = run(&[
        "lint",
        source.to_str().expect("UTF-8 path"),
        "--project",
        project.to_str().expect("UTF-8 path"),
    ]);
    assert!(
        output.status.success(),
        "{}",
        String::from_utf8_lossy(&output.stdout)
    );
    let output = json(&output);
    assert_eq!(output["finding_count"], 0);
    assert_eq!(
        output["id_policy"]["source"],
        project.to_str().expect("UTF-8 path")
    );
    assert_eq!(
        output["id_policy"]["patterns"]["forbidden"][0],
        "FB-{scope}-{number:3}"
    );
}

#[test]
fn invalid_project_id_policy_fails_closed() {
    let directory = FixtureDir::new();
    let source = directory.write(
        "source.fsl",
        "requirements Checkout { requirement REQ-CHECKOUT-001 \"ok\" { } }\n",
    );
    for project_source in [
        "[id_policy.patterns]\nunknown = \"X-{number}\"\n",
        "[id_policy.patterns]\nrequirement = \"REQ-{unknown}\"\n",
        "[id_policy.patterns]\nrequirement = []\n",
        "[id_policy.patterns]\nrequirement = 'REQ-{number}'\n",
        "[id_policy.patterns]\nmodel = \"{scope}-MODEL-{number}\"\n",
        "[id_policy.patterns]\nmodel = \"TRACE-{number}\"\nassumption = \"TRACE-A-{number}\"\n",
        "[id_policy.patterns]\nmodel = \"REQ-{scope}-{number:3}\"\n",
    ] {
        let project = directory.write("fsl-project.toml", project_source);
        let output = run(&[
            "lint",
            source.to_str().expect("UTF-8 path"),
            "--project",
            project.to_str().expect("UTF-8 path"),
        ]);
        assert_eq!(output.status.code(), Some(2));
        assert_eq!(json(&output)["kind"], "config");
    }
}

#[test]
fn invalid_double_ampersand_and_ambiguous_maps_are_explicit_refusals() {
    let directory = FixtureDir::new();
    let invalid = directory.write(
        "invalid.fsl",
        "domain Bad { aggregate A { invariant bad { true && false } } }\n",
    );
    let invalid_text = invalid.to_str().expect("UTF-8 path");
    let lint = run(&["lint", invalid_text, "--edition", "next"]);
    assert_eq!(lint.status.code(), Some(1));
    let finding = &json(&lint)["files"][0]["findings"][0];
    assert_eq!(finding["taxonomy"], "unsupported_in_edition");
    assert_eq!(finding["canonical_replacement"], "and");
    assert_eq!(finding["machine_applicable"], false);
    let migrate = run(&["migrate", invalid_text, "--edition", "next", "--write"]);
    assert_eq!(migrate.status.code(), Some(2));
    assert_eq!(json(&migrate)["result"], "migration_refused");

    let root = Path::new(env!("CARGO_MANIFEST_DIR"));
    let duplicate = root.join("tests/fixtures/action_correspondence_duplicate.fsl");
    let duplicate_output = run(&[
        "migrate",
        duplicate.to_str().expect("UTF-8 path"),
        "--edition",
        "next",
    ]);
    assert_eq!(duplicate_output.status.code(), Some(2));
    assert_eq!(
        json(&duplicate_output)["files"][0]["findings"][0]["taxonomy"],
        "ambiguous_intent"
    );
}

#[test]
fn ai_native_legacy_operator_attempt_reports_each_unsupported_token() {
    let root = Path::new(env!("CARGO_MANIFEST_DIR"))
        .join("tests/fixtures/domain_characterization/legacy_logical_parse_error.fsl");
    let output = run(&[
        "lint",
        root.to_str().expect("UTF-8 path"),
        "--edition",
        "next",
    ]);
    assert_eq!(output.status.code(), Some(1));
    let findings = json(&output)["files"][0]["findings"]
        .as_array()
        .expect("findings")
        .clone();
    assert_eq!(findings.len(), 2);
    assert_eq!(
        findings
            .iter()
            .map(|finding| finding["code"].as_str().expect("code"))
            .collect::<Vec<_>>(),
        ["unsupported_double_ampersand", "legacy_logical_operator"]
    );
    assert!(
        findings
            .iter()
            .all(|finding| finding["machine_applicable"] == false)
    );
}

#[test]
fn malformed_input_reports_the_file_and_parse_location() {
    let directory = FixtureDir::new();
    let malformed = directory.write(
        "malformed.fsl",
        "spec Broken { state { ready: Bool } invariant Missing { ready == } }\n",
    );
    let path = malformed.to_str().expect("UTF-8 path");
    let output = run(&["lint", path, "--edition", "next"]);
    assert_eq!(output.status.code(), Some(2));
    let error = json(&output);
    assert_eq!(error["result"], "error");
    assert_eq!(error["kind"], "parse");
    assert_eq!(error["file"], path);
    assert_eq!(error["loc"]["file"], path);
    assert!(error["loc"]["line"].as_u64().is_some());
    assert!(error["loc"]["column"].as_u64().is_some());
}

#[test]
fn safe_inline_maps_move_to_the_single_local_implements_block() {
    let directory = FixtureDir::new();
    let abstraction = directory.write(
        "abstract.fsl",
        r"spec Abstract {
  type Id = 0..1
  state { done: Map<Id, Bool> }
  init { forall i: Id { done[i] = false } }
  action finish(i: Id) { done[i] = true }
  invariant Typed { forall i: Id { done[i] == true or done[i] == false } }
}
",
    );
    assert!(abstraction.exists());
    let requirements = directory.write(
        "requirements.fsl",
        r#"requirements Concrete {
  type Id = 0..1
  state { done: Map<Id, Bool> }
  init { forall i: Id { done[i] = false } }
  implements Abstract from "abstract.fsl" {
    map done[i: Id] = done[i]
  }
  action complete(i: Id) maps finish(i) { done[i] = true }
  invariant Typed { forall i: Id { done[i] == true or done[i] == false } }
}
"#,
    );
    let path = requirements.to_str().expect("UTF-8 path");
    let before_verify = run(&["verify", path, "--depth", "2"]);
    assert!(
        before_verify.status.success(),
        "{}",
        String::from_utf8_lossy(&before_verify.stdout)
    );
    let output = run(&["migrate", path, "--edition", "next", "--write"]);
    assert!(
        output.status.success(),
        "{}",
        String::from_utf8_lossy(&output.stdout)
    );
    let source = text(&requirements);
    assert!(!source.contains("maps finish"));
    assert!(source.contains("action complete(i: Id) -> finish(i)"));
    assert_eq!(source.matches("action complete").count(), 2);

    let after_verify = run(&["verify", path, "--depth", "2"]);
    assert!(
        after_verify.status.success(),
        "{}",
        String::from_utf8_lossy(&after_verify.stdout)
    );
    let before = json(&before_verify);
    let after = json(&after_verify);
    assert_eq!(before["result"], after["result"]);
    assert_eq!(before["implements"], after["implements"]);
}

#[test]
fn inline_maps_with_parameter_comments_are_refused_without_source_changes() {
    let directory = FixtureDir::new();
    directory.write(
        "abstract.fsl",
        r"spec Abstract {
  type Id = 0..1
  state { done: Map<Id, Bool> }
  init { forall i: Id { done[i] = false } }
  action finish(i: Id) { done[i] = true }
  invariant Typed { forall i: Id { done[i] == true or done[i] == false } }
}
",
    );
    let original = r#"requirements Concrete {
  type Id = 0..1
  state { done: Map<Id, Bool> }
  init { forall i: Id { done[i] = false } }
  implements Abstract from "abstract.fsl" {
    map done[i: Id] = done[i]
  }
  action complete(
    i: Id // entity id
  ) maps finish(i) { done[i] = true }
  invariant Typed { forall i: Id { done[i] == true or done[i] == false } }
}
"#;
    let requirements = directory.write("requirements.fsl", original);
    let path = requirements.to_str().expect("UTF-8 path");
    let output = run(&["migrate", path, "--edition", "next", "--write"]);
    assert_eq!(output.status.code(), Some(2));
    let finding = &json(&output)["files"][0]["findings"][0];
    assert_eq!(finding["taxonomy"], "ambiguous_intent");
    assert_eq!(finding["machine_applicable"], false);
    assert_eq!(text(&requirements), original);
}

#[test]
fn multi_file_refusal_leaves_every_input_unchanged() {
    let directory = FixtureDir::new();
    let safe_source = "domain Safe { type E = A | B }\n";
    let refused_source = "domain Refused { aggregate A { invariant bad { true && false } } }\n";
    let safe = directory.write("safe.fsl", safe_source);
    let refused = directory.write("refused.fsl", refused_source);
    let output = run(&[
        "migrate",
        safe.to_str().expect("UTF-8 path"),
        refused.to_str().expect("UTF-8 path"),
        "--edition",
        "next",
        "--write",
    ]);
    assert_eq!(output.status.code(), Some(2));
    assert_eq!(text(&safe), safe_source);
    assert_eq!(text(&refused), refused_source);
}

#[cfg(unix)]
#[test]
fn multi_file_prepare_io_failure_leaves_every_input_unchanged() {
    let directory = FixtureDir::new();
    let writable = directory.0.join("writable");
    let locked = directory.0.join("locked");
    std::fs::create_dir(&writable).expect("writable directory");
    std::fs::create_dir(&locked).expect("locked directory");
    let first = writable.join("first.fsl");
    let second = locked.join("second.fsl");
    let first_source = "domain First { type E = A | B }\n";
    let second_source = "domain Second { type E = A | B }\n";
    std::fs::write(&first, first_source).expect("first fixture");
    std::fs::write(&second, second_source).expect("second fixture");
    std::fs::set_permissions(&locked, std::fs::Permissions::from_mode(0o555))
        .expect("lock directory");

    let output = run(&[
        "migrate",
        first.to_str().expect("UTF-8 path"),
        second.to_str().expect("UTF-8 path"),
        "--edition",
        "next",
        "--write",
    ]);
    std::fs::set_permissions(&locked, std::fs::Permissions::from_mode(0o755))
        .expect("unlock directory");
    assert_eq!(output.status.code(), Some(2));
    assert_eq!(text(&first), first_source);
    assert_eq!(text(&second), second_source);
}
