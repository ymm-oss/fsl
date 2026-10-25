// SPDX-License-Identifier: Apache-2.0

use std::path::{Path, PathBuf};
use std::process::{Command, Output};
use std::sync::atomic::{AtomicU64, Ordering};
use std::time::{SystemTime, UNIX_EPOCH};

#[cfg(unix)]
use std::os::unix::fs::PermissionsExt as _;

use fsl_core::FslValue;
use fsl_runtime::{EnabledAction, Monitor};
use serde_json::Value;

#[path = "support/id_spelling.rs"]
mod id_spelling;
use id_spelling::compact_hyphen_runs;

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

fn edition_command(verb: &str, headed: bool) -> Command {
    let mut command = Command::new(env!("CARGO_BIN_EXE_fslc"));
    command.env_remove("FSLC_UNSTABLE").arg(verb);
    if headed {
        command.env("FSLC_UNSTABLE", "fsl5");
    }
    command
}

fn gated_options() -> fsl_syntax::ParseOptions {
    fsl_syntax::ParseOptions::with_unstable(
        fsl_syntax::UnstableFeatures::NONE.with(fsl_syntax::UnstableFeature::Fsl5),
    )
}

fn run_edition(
    path: &str,
    verb: &str,
    edition: Option<&str>,
    headed: bool,
    extra: &[&str],
) -> Output {
    let mut command = edition_command(verb, headed);
    command.arg(path);
    if let Some(edition) = edition {
        command.args(["--edition", edition]);
    }
    command.args(extra).output().expect("run fslc")
}

#[test]
fn lint_and_migrate_share_next_operator_diagnostics_without_rewriting() {
    let directory = FixtureDir::new();
    for operator in ["||", "->"] {
        let source = format!(
            "spec S {{ state {{ flag: Bool = false }} action finish() {{ flag = true }} invariant I {{ true {operator} false }} }}\n"
        );
        let path = directory.write("legacy.fsl", &source);
        for verb in ["lint", "migrate"] {
            let output = run_edition(path.to_str().expect("path"), verb, Some("next"), false, &[]);
            assert_eq!(output.status.code(), Some(2));
            let output = json(&output);
            assert_eq!(output["result"], "error");
            assert_eq!(output["kind"], "parse");
            assert_eq!(
                output["diagnostic_code"], "unsupported_in_edition",
                "{verb}: {output:#}"
            );
            assert_eq!(
                text(&path),
                source,
                "invalid expressions require manual repair"
            );
        }
    }
}

#[test]
fn relation_range_arrows_remain_structural_in_every_edition_entry() {
    let directory = FixtureDir::new();
    for endpoints in [
        "0..1 -> 0..1",
        "0..1 -> Bool",
        "0..(1) -> Bool",
        "0..N -> Bool",
        "Node -> Node",
    ] {
        let body = format!(
            "spec Relation {{ const N = 1 type Node = 0..1 state {{ edges: relation {endpoints} = Set {{}} }} action hold() {{ edges = edges }} invariant Typed {{ true }} }}\n"
        );
        for (prefix, edition) in [("fsl 5\n", None), ("", None), ("", Some("next"))] {
            let source = format!("{prefix}{body}");
            let path = directory.write("relation.fsl", &source);
            let output = run_edition(
                path.to_str().expect("path"),
                "check",
                edition,
                !prefix.is_empty(),
                &[],
            );
            assert_eq!(
                output.status.code(),
                Some(0),
                "expected accepted {endpoints}, produced {}",
                String::from_utf8_lossy(&output.stdout)
            );
            assert_eq!(json(&output)["result"], "ok");
            let options = gated_options();
            assert!(
                fsl_syntax::legacy_expression_operator_error(&source, &options).is_none(),
                "relation arrow is not an expression operator"
            );
            let malformed =
                source.replace("invariant Typed { true }", "invariant Typed { true == }");
            assert!(
                fsl_syntax::legacy_expression_operator_error(&malformed, &options).is_none(),
                "must not replace a later real parse error with a relation diagnostic"
            );
            let path = directory.write("malformed.fsl", &malformed);
            let error = run_edition(
                path.to_str().expect("path"),
                "check",
                edition,
                !prefix.is_empty(),
                &[],
            );
            assert_eq!(error.status.code(), Some(2));
            assert_eq!(json(&error)["diagnostic_code"], "FSL-PARSE");
        }
        // Explicit gated conversion control: the migrator inserts a header;
        // this is separate from the ungated current-edition checks above.
        let path = directory.write("migrating.fsl", &body);
        let migrated = run_edition(
            path.to_str().expect("path"),
            "migrate",
            Some("next"),
            true,
            &["--write"],
        );
        assert_eq!(
            migrated.status.code(),
            Some(0),
            "{}",
            String::from_utf8_lossy(&migrated.stdout)
        );
        assert_eq!(json(&migrated)["result"], "migrated");
        assert!(text(&path).starts_with("fsl 5\n"));
        let checked = run_edition(path.to_str().expect("path"), "check", None, true, &[]);
        assert_eq!(checked.status.code(), Some(0));
        assert_eq!(json(&checked)["result"], "ok");
    }
}

#[test]
fn action_correspondence_range_parameter_precedes_a_structural_arrow() {
    let directory = FixtureDir::new();
    directory.write("abstract.fsl", "spec A { state { done: Bool = false } action finish(i in 0..1) { done = true } invariant Typed { true } }\n");
    let body = "requirements C { implements A from \"abstract.fsl\" { map done = done action finish(i: 0..1) -> finish(i) } state { done: Bool = false } action finish(i in 0..1) { done = true } invariant Typed { true } }\n";
    for (prefix, edition) in [("fsl 5\n", None), ("", None), ("", Some("next"))] {
        let source = format!("{prefix}{body}");
        let path = directory.write("parameter.fsl", &source);
        let output = run_edition(
            path.to_str().expect("path"),
            "check",
            edition,
            !prefix.is_empty(),
            &[],
        );
        assert_eq!(
            output.status.code(),
            Some(0),
            "{}",
            String::from_utf8_lossy(&output.stdout)
        );
        assert_eq!(json(&output)["result"], "ok");
        let options = gated_options();
        assert!(fsl_syntax::legacy_expression_operator_error(&source, &options).is_none());
    }
}

#[test]
fn next_edition_legacy_expression_operators_have_actionable_parse_diagnostics() {
    let directory = FixtureDir::new();
    for body in [
        "spec Probe { state { flag: Bool = false } action finish() { flag = true } invariant I { (flag or false) and (flag => true) } }",
        "requirements Probe { state { flag: Bool = false } action finish() { flag = true } invariant I { (flag or false) and (flag => true) } }",
        "compose Probe { state { flag: Bool = false } invariant I { (flag or false) and (flag => true) } }",
        "business Probe { actor User entity Case process Case { stages New, Done initial New transition finish New -> Done by User } policy POL-PROBE-001 \"operator probe\" invariant { (true or false) and (true => true) } } verify { instances Case = 1 }",
        "refinement Probe { impl I abs A map flag = (flag or false) and (flag => true) action finish() -> finish() }",
        "requirements Probe { entity Case { flag: Bool = false lifecycle New -> Done } concept Ready = Case where (flag or false) and (flag => true) } verify { instances Case = 1 }",
    ] {
        let readable = body.contains("concept Ready");
        let verb = if body.starts_with("refinement") {
            "fmt"
        } else {
            "check"
        };
        for (canonical, legacy, replacement) in [(" or ", " || ", "or"), (" => ", " -> ", "=>")] {
            for headed in [false, true] {
                if readable && !headed {
                    continue;
                }
                let prefix = if headed {
                    "fsl 5\n// 日本語😀\n"
                } else {
                    "// 日本語😀\n"
                };
                let invoke = |source: &str, edition: Option<&str>| {
                    let path = directory.write("operator.fsl", source);
                    run_edition(
                        path.to_str().expect("path"),
                        verb,
                        edition,
                        headed,
                        if verb == "fmt" { &["--check"] } else { &[] },
                    )
                };
                assert_canonical_operator_twin(&directory, &format!("{prefix}{body}\n"), headed);
                let source = format!("{prefix}{}\n", body.replace(canonical, legacy));
                let output = invoke(&source, (!headed).then_some("next"));
                assert_eq!(output.status.code(), Some(2));
                let output = json(&output);
                assert_eq!(output["result"], "error");
                assert_eq!(output["kind"], "parse");
                assert_eq!(
                    output["diagnostic_code"], "unsupported_in_edition",
                    "expected edition diagnostic, produced {output:#}"
                );
                let start = prefix.len() + body.find(canonical).expect("expression operator") + 1;
                let line_start = source[..start].rfind('\n').map_or(0, |i| i + 1);
                assert_eq!(
                    output["loc"]["line"],
                    source[..start].matches('\n').count() + 1
                );
                assert_eq!(
                    output["loc"]["column"],
                    source[line_start..start].chars().count() + 1
                );
                assert_operator_endpoints(&source, start);
                let message = output["message"].as_str().expect("message");
                assert!(
                    message.contains(&format!("use '{replacement}'"))
                        && message.contains("fslc migrate <file> --edition next"),
                    "{message}"
                );
                if !headed {
                    let current = invoke(&source, Some("current"));
                    assert_eq!(current.status.code(), Some(2));
                    assert_eq!(json(&current)["diagnostic_code"], "FSL-PARSE");
                }
            }
        }
    }
}

fn assert_token_location(finding: &Value, source: &str, token: &str) {
    let offset = source.find(token).expect("legacy token");
    let line_start = source[..offset].rfind('\n').map_or(0, |i| i + 1);
    assert_eq!(
        finding["loc"]["line"],
        source[..offset].matches('\n').count() + 1
    );
    assert_eq!(
        finding["loc"]["column"],
        source[line_start..offset].chars().count() + 1
    );
    assert_eq!(
        finding["loc"]["end_column"],
        source[line_start..offset].chars().count() + token.chars().count() + 1
    );
}

#[test]
fn current_edition_exposes_legacy_findings_without_changing_success() {
    let directory = FixtureDir::new();
    directory.write(
        "abstract.fsl",
        "spec Abstract { state { done: Bool = false } action finish() { done = true } }\n",
    );
    for (body, code, token) in [
        (
            "spec S { state { done: Bool = false } action finish() \"REQ-PROBE-001: 完了\" { done = true } }\n",
            "legacy_string_metadata",
            "\"REQ-PROBE-001: 完了\"",
        ),
        (
            "spec S { state { done: Bool = false } action finish() { done = true } invariant Typed { forall i in 0..1: done == true or done == false } }\n",
            "legacy_quantifier_colon",
            "forall",
        ),
        (
            "requirements S { implements Abstract from \"abstract.fsl\" { map done = done } state { done: Bool = false } action complete() maps finish() { done = true } }\n",
            "inline_action_maps",
            "maps finish()",
        ),
    ] {
        for (prefix, edition) in [
            ("// 日本語\n", None),
            ("// 日本語\n", Some("current")),
            ("fsl 4\n// 日本語\n", None),
        ] {
            let source = format!("{prefix}{body}");
            let path = directory.write("current.fsl", &source);
            let path = path.to_str().expect("path");
            let invoke = |verb: &str, extra: &[&str]| {
                let mut command = Command::new(env!("CARGO_BIN_EXE_fslc"));
                command.env("FSLC_UNSTABLE", "fsl5").args([verb, path]);
                if let Some(edition) = edition {
                    command.args(["--edition", edition]);
                }
                command.args(extra).output().expect("run fslc")
            };
            // Preservation: lint already reports these findings, including exit 1.
            let lint = invoke("lint", &[]);
            assert_eq!(lint.status.code(), Some(1));
            let lint = json(&lint);
            let findings = lint["files"][0]["findings"].as_array().expect("findings");
            assert_eq!(lint["finding_count"], findings.len());
            let expected = findings
                .iter()
                .filter(|f| f["code"] == code)
                .collect::<Vec<_>>();
            assert_eq!(expected.len(), 1, "{lint:#}");
            let expected = expected[0];
            assert_token_location(expected, &source, token);
            assert_eq!(expected["severity"], "warning");
            assert_eq!(expected["edition"], "current");
            for (verb, extra) in [
                ("check", vec![]),
                ("verify", vec!["--depth", "2", "--no-cache"]),
            ] {
                let output = invoke(verb, &extra);
                assert_eq!(
                    output.status.code(),
                    Some(0),
                    "{}",
                    String::from_utf8_lossy(&output.stdout)
                );
                let output = json(&output);
                let warnings = output["warnings"].as_array().expect("warnings");
                let actual = warnings
                    .iter()
                    .filter(|f| f["code"] == code)
                    .collect::<Vec<_>>();
                assert_eq!(
                    actual,
                    vec![expected],
                    "{verb}: expected {code}, produced {output:#}"
                );
                assert_eq!(
                    output.get("fsl_edition").is_some(),
                    prefix.starts_with("fsl 4")
                );
                if code == "legacy_string_metadata" {
                    assert!(
                        warnings.iter().any(|f| f["kind"] == "no_user_invariants"),
                        "existing warning retained: {output:#}"
                    );
                }
            }
            // Preservation: the existing next-edition rejection stays exit 2.
            let next = run(&["check", path, "--edition", "next"]);
            assert_eq!(next.status.code(), Some(2));
        }
    }
}

fn assert_operator_endpoints(source: &str, offset: usize) {
    let options = gated_options();
    let error = fsl_syntax::parse_surface_document_with(source, &options)
        .expect_err("legacy operator")
        .for_edition(source, fsl_syntax::Edition::Fsl5, &options);
    let line = source[..offset].matches('\n').count() + 1;
    let start = source[..offset].rfind('\n').map_or(0, |i| i + 1);
    let column = source[start..offset].chars().count() + 1;
    assert_eq!(error.span.start.offset, offset);
    assert_eq!(error.span.end.offset, offset + 2);
    assert_eq!(
        (error.span.start.line as usize, error.span.end.line as usize),
        (line, line)
    );
    assert_eq!(
        (
            error.span.start.column as usize,
            error.span.end.column as usize
        ),
        (column, column + 2)
    );
}

fn assert_canonical_operator_twin(directory: &FixtureDir, source: &str, headed: bool) {
    let path = directory.write("canonical.fsl", source);
    let verb = if source.contains("refinement Probe") {
        "refine"
    } else {
        "check"
    };
    let mut command = edition_command(verb, headed);
    let expected = if source.contains("refinement Probe") {
        // Match the negative twin's fmt --check entry as well as proving the
        // mapping below. Normalize layout first so a positive check exits 0.
        let mut layout = edition_command("fmt", headed);
        layout.arg(path.to_str().expect("path"));
        if !headed {
            layout.args(["--edition", "next"]);
        }
        let formatted = layout.output().expect("format canonical twin");
        assert_eq!(formatted.status.code(), Some(0));
        let formatted = String::from_utf8(formatted.stdout).expect("formatted FSL");
        let formatted = directory.write("canonical-format.fsl", &formatted);
        let checked = run_edition(
            formatted.to_str().expect("path"),
            "fmt",
            (!headed).then_some("next"),
            headed,
            &["--check"],
        );
        assert_eq!(checked.status.code(), Some(0));
        assert_eq!(json(&checked)["result"], "format_check");
        assert_eq!(json(&checked)["changed"], false);
        let model = |name| {
            format!(
                "spec {name} {{ state {{ flag: Bool = false }} action finish() {{ flag = true }} invariant Typed {{ true }} }}\n"
            )
        };
        let implementation = directory.write("implementation.fsl", &model("I"));
        let abstraction = directory.write("abstraction.fsl", &model("A"));
        command.args([
            implementation.to_str().expect("path"),
            abstraction.to_str().expect("path"),
            path.to_str().expect("path"),
            "--depth",
            "2",
        ]);
        "refines"
    } else {
        command.arg(path.to_str().expect("path"));
        if !headed {
            command.args(["--edition", "next"]);
        }
        "ok"
    };
    let output = command.output().expect("canonical command");
    assert_eq!(
        output.status.code(),
        Some(0),
        "canonical twin: {}",
        String::from_utf8_lossy(&output.stdout)
    );
    assert_eq!(json(&output)["result"], expected);
}

#[test]
fn current_warnings_preserve_refusals_and_ignore_canonical_and_literal_forms() {
    let root = Path::new(env!("CARGO_MANIFEST_DIR"));
    let directory = FixtureDir::new();
    let branch = directory.write("branches.fsl", "requirements Branch { state { done: Bool = false } action complete() { branches { when not done { done = true } maps stutter } } }\n");
    for path in [
        branch,
        root.join("tests/fixtures/action_correspondence_duplicate.fsl"),
    ] {
        let path = path.to_str().expect("path");
        let check = run(&["check", path]);
        let lint = run(&["lint", path]);
        assert_eq!(lint.status.code(), Some(1));
        let lint = json(&lint);
        let expected = lint["files"][0]["findings"]
            .as_array()
            .expect("findings")
            .iter()
            .filter(|f| f["code"] == "inline_action_maps")
            .collect::<Vec<_>>();
        assert!(expected.iter().any(|f| f["machine_applicable"] == false));
        let check = json(&check);
        let actual = check["warnings"]
            .as_array()
            .expect("warnings")
            .iter()
            .filter(|f| f["code"] == "inline_action_maps")
            .collect::<Vec<_>>();
        assert_eq!(actual, expected, "{check:#}");
    }
    let source = r#"// forall i in 0..1: || -> maps finish()
spec Canonical {
  state { done: Bool = false }
  @requirement("REQ-PROBE-001", "literal maps finish() forall i: -> ||")
  action finish() { done = true }
  invariant Typed { forall i in 0..1 { done == true or done == false } }
}
"#;
    let path = directory.write("canonical.fsl", source);
    for verb in ["check", "lint"] {
        let output = run(&[verb, path.to_str().expect("path")]);
        assert_eq!(output.status.code(), Some(0));
        let output = json(&output);
        let findings = if verb == "lint" {
            &output["files"][0]["findings"]
        } else {
            &output["warnings"]
        };
        assert!(
            findings
                .as_array()
                .expect("findings")
                .iter()
                .all(|f| !matches!(
                    f["code"].as_str(),
                    Some(
                        "legacy_string_metadata" | "legacy_quantifier_colon" | "inline_action_maps"
                    )
                ))
        );
    }
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
fn migrate_writes_requirement_ids_as_written() {
    let root = Path::new(env!("CARGO_MANIFEST_DIR"));
    for fixture in [
        "tests/fixtures/fsl5/e2e/original.fsl",
        "../../examples/gallery/injected/bank__boundary_flip.fsl",
    ] {
        let original = text(&root.join(fixture));
        let ids = compact_hyphen_runs(&original);
        assert!(!ids.is_empty(), "{fixture} spells no IDs");
        let directory = FixtureDir::new();
        // original.fsl implements the business layer beside it.
        directory.write(
            "1_business.fsl",
            &text(&root.join("tests/fixtures/fsl5/e2e/1_business.fsl")),
        );
        let path = directory.write("spec.fsl", &original);
        let migrated = run(&[
            "migrate",
            path.to_str().expect("UTF-8 path"),
            "--edition",
            "next",
            "--write",
        ]);
        assert!(
            migrated.status.success(),
            "{fixture}: {}",
            String::from_utf8_lossy(&migrated.stdout)
        );
        assert_eq!(json(&migrated)["written"], true, "{fixture}");
        assert_eq!(compact_hyphen_runs(&text(&path)), ids, "{fixture}");
    }
}

#[test]
fn builtin_id_policy_reports_each_surface_kind_without_rewriting_ids() {
    let directory = FixtureDir::new();
    let path = directory.write(
        "ids.fsl",
        r#"requirements Checkout {
  state { done: Bool }
  init { done = false }
  requirement REQ-1 "requirement" { }
  action reject() { done = true }
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
fn lint_deduplicates_recursive_alias_and_repeated_paths() {
    let directory = FixtureDir::new();
    let nested = directory.0.join("nested");
    std::fs::create_dir(&nested).expect("create nested fixture directory");
    let business = r#"business Repro {
  actor Owner
  entity Case
  process Case {
    stages Open, Closed
    initial Open
    transition close Open -> Closed by Owner
  }
  control CTRL-REPRO-001 "A case must be reviewed before closure."
    owner Owner
    severity high
    applies_to Case
  policy POL-REPRO-001 "A case must be reviewed before closure."
    satisfies CTRL-REPRO-001
    every Case reaching Closed must have passed through Open
  goal GOAL-REPRO-001 "A reviewed case can close."
    satisfies CTRL-REPRO-001
    some Case can reach Closed
}
verify {
  instances Case = 3
}
"#;
    let root_file = directory.write("z.fsl", business);
    let nested_file = nested.join("a.fsl");
    std::fs::write(&nested_file, business).expect("write nested fixture");
    std::fs::write(nested.join("ignored.txt"), "not FSL").expect("write ignored fixture");
    let aliased_root_file = nested.join("..").join("z.fsl");
    let root_file_text = root_file.to_str().expect("UTF-8 root path");

    let output = run(&[
        "lint",
        directory.0.to_str().expect("UTF-8 directory path"),
        root_file_text,
        root_file_text,
        aliased_root_file.to_str().expect("UTF-8 aliased root path"),
        "--edition",
        "next",
    ]);
    assert!(
        output.status.success(),
        "{}",
        String::from_utf8_lossy(&output.stdout)
    );
    let output = json(&output);
    assert_eq!(output["finding_count"], 0);
    assert_eq!(
        output["files"]
            .as_array()
            .expect("lint files")
            .iter()
            .map(|file| file["path"].as_str().expect("path"))
            .collect::<Vec<_>>(),
        [
            nested_file.to_str().expect("UTF-8 nested path"),
            root_file.to_str().expect("UTF-8 root path"),
        ]
    );

    let migrate = run(&["migrate", root_file_text, root_file_text]);
    assert_eq!(migrate.status.code(), Some(2));
    let migrate = json(&migrate);
    assert_eq!(migrate["kind"], "usage");
    assert_eq!(
        migrate["message"],
        "the same migration path cannot be supplied twice"
    );
}

#[test]
fn lint_directory_reports_nested_fsl_errors_instead_of_skipping_them() {
    let directory = FixtureDir::new();
    let nested = directory.0.join("nested");
    std::fs::create_dir(&nested).expect("create nested fixture directory");
    let malformed = nested.join("malformed.fsl");
    std::fs::write(&malformed, "spec Broken { invariant P { x ` y } }")
        .expect("write malformed fixture");
    std::fs::write(nested.join("ignored.txt"), "not FSL").expect("write ignored fixture");

    let output = run(&["lint", directory.0.to_str().expect("UTF-8 directory path")]);
    assert_eq!(output.status.code(), Some(2));
    let output = json(&output);
    assert_eq!(output["kind"], "parse");
    assert_eq!(
        output["file"],
        malformed.to_str().expect("UTF-8 malformed path")
    );
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
  state { done: Bool }
  init { done = false }
  requirement PAY-42 "requirement" { }
  acceptance TEST-7 "acceptance" { expect true }
  action reject() { done = true }
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

// #517: `lint`/`migrate` previously only ran checked-model validation on the
// changed-file path, so an input with no legacy syntax for `plan_migration`
// to find and fix skipped it entirely — a spec that fails `fslc check`
// could still report `lint`/`migrate` exit 0. The fix adds an unconditional
// pre-flight in `load_migration_plan`, shared by both commands, mirroring
// `run_check`'s own kind/location convention exactly.

/// Positive control required alongside the false-green fix: a genuinely
/// valid, already-canonical spec with no legacy syntax at all (an
/// unchanged input, in the issue's own sense — nothing for
/// `plan_migration` to find or fix) must still pass the new unconditional
/// pre-flight and report `lint`/`migrate` exit 0, exactly as it did before
/// this fix. The pre-flight closes a false green; it must not introduce a
/// false red on well-formed input.
#[test]
fn genuinely_valid_unchanged_input_still_passes_lint_and_migrate() {
    let directory = FixtureDir::new();
    let source = "spec Sound {\n  state { n: 0..3 }\n  init { n = 0 }\n  action bump() {\n    requires n < 3\n    n = n + 1\n  }\n  invariant Bound { n >= 0 }\n}\n";
    let path = directory.write("sound.fsl", source);
    let path = path.to_str().expect("UTF-8 path");

    assert_eq!(run(&["check", path]).status.code(), Some(0));

    let migrated = run(&["migrate", path, "--edition", "next"]);
    assert_eq!(migrated.status.code(), Some(0));
    let migrated = json(&migrated);
    assert_eq!(migrated["result"], "migrated", "{migrated:#}");
    assert_eq!(migrated["changed"], 0, "{migrated:#}");
    assert_eq!(migrated["files"][0]["changed"], false, "{migrated:#}");
    assert_eq!(
        migrated["files"][0]["findings"].as_array().map(Vec::len),
        Some(0),
        "{migrated:#}"
    );

    let linted = run(&["lint", path, "--edition", "next"]);
    assert_eq!(linted.status.code(), Some(0));
    assert_eq!(json(&linted)["finding_count"], 0);
}

#[test]
fn check_failing_spec_without_legacy_syntax_still_fails_lint_and_migrate() {
    let directory = FixtureDir::new();
    let source = "spec Broken {\n  state { n: 0..3 }\n  init { n = 0 }\n  action bump() {\n    requires n < 3\n    n = true\n  }\n  invariant Bound { n >= 0 }\n}\n";
    let path = directory.write("type_error_clean.fsl", source);
    let path = path.to_str().expect("UTF-8 path");

    let checked = json(&run(&["check", path]));
    assert_eq!(checked["result"], "error", "{checked:#}");
    assert_eq!(checked["kind"], "semantics", "{checked:#}");

    let migrated = run(&["migrate", path, "--edition", "next"]);
    assert_eq!(migrated.status.code(), Some(2));
    let migrated = json(&migrated);
    assert_eq!(migrated["result"], "error", "{migrated:#}");
    assert_eq!(migrated["kind"], "semantics", "{migrated:#}");
    assert_eq!(migrated["message"], checked["message"], "{migrated:#}");

    let linted = run(&["lint", path, "--edition", "next"]);
    assert_eq!(linted.status.code(), Some(2));
    let linted = json(&linted);
    assert_eq!(linted["result"], "error", "{linted:#}");
    assert_eq!(linted["kind"], "semantics", "{linted:#}");
    assert_eq!(linted["message"], checked["message"], "{linted:#}");
}

/// Pins the asymmetry the issue reports: the exact same type error, with and
/// without one unrelated legacy string-metadata token, must get the exact
/// same `migrate`/`lint` verdict — not exit 0 only because
/// `plan_migration` happened to find a legacy token to fix first.
#[test]
fn legacy_token_presence_does_not_change_the_check_failure_verdict() {
    let directory = FixtureDir::new();
    let clean = directory.write(
        "type_error_clean.fsl",
        "spec Broken {\n  state { n: 0..3 }\n  init { n = 0 }\n  action bump() {\n    requires n < 3\n    n = true\n  }\n  invariant Bound { n >= 0 }\n}\n",
    );
    let legacy = directory.write(
        "type_error_legacy.fsl",
        "spec Broken {\n  state { n: 0..3 }\n  init { n = 0 }\n  action bump() \"REQ-1: bump\" {\n    requires n < 3\n    n = true\n  }\n  invariant Bound { n >= 0 }\n}\n",
    );
    let clean = clean.to_str().expect("UTF-8 path");
    let legacy = legacy.to_str().expect("UTF-8 path");

    let clean_migrate = run(&["migrate", clean, "--edition", "next"]);
    let legacy_migrate = run(&["migrate", legacy, "--edition", "next"]);
    assert_eq!(clean_migrate.status.code(), Some(2));
    assert_eq!(legacy_migrate.status.code(), Some(2));
    let (clean_migrate, legacy_migrate) = (json(&clean_migrate), json(&legacy_migrate));
    assert_eq!(
        clean_migrate["kind"], legacy_migrate["kind"],
        "{legacy_migrate:#}"
    );
    assert_eq!(
        clean_migrate["message"], legacy_migrate["message"],
        "{legacy_migrate:#}"
    );

    let clean_lint = run(&["lint", clean, "--edition", "next"]);
    let legacy_lint = run(&["lint", legacy, "--edition", "next"]);
    assert_eq!(clean_lint.status.code(), Some(2));
    assert_eq!(legacy_lint.status.code(), Some(2));
}

#[test]
fn canonical_requirements_spec_with_a_missing_implements_target_still_fails_lint_and_migrate() {
    let directory = FixtureDir::new();
    let source = "requirements Broken {\n  implements CartPolicy from \"moved_business.fsl\" {\n  }\n\n  state {\n    n: 0..3\n  }\n\n  init {\n    n = 0\n  }\n\n  action bump() {\n    requires n < 3\n    n = n + 1\n  }\n}\n";
    let path = directory.write("moved_canon.fsl", source);
    let path = path.to_str().expect("UTF-8 path");

    let checked = json(&run(&["check", path]));
    assert_eq!(checked["kind"], "type", "{checked:#}");
    assert!(checked["loc"]["line"].as_u64().is_some(), "{checked:#}");

    let migrated = run(&["migrate", path, "--edition", "next"]);
    assert_eq!(migrated.status.code(), Some(2));
    let migrated = json(&migrated);
    assert_eq!(migrated["kind"], "type", "{migrated:#}");
    assert_eq!(migrated["loc"], checked["loc"], "{migrated:#}");

    let linted = run(&["lint", path, "--edition", "next"]);
    assert_eq!(linted.status.code(), Some(2));
    let linted = json(&linted);
    assert_eq!(linted["kind"], "type", "{linted:#}");
    assert_eq!(linted["loc"], checked["loc"], "{linted:#}");

    // Regression control cited in the issue as an adjacent, intentionally
    // separate symptom (`validate_fmt_semantics` does not resolve
    // `requirements_implements`) — `fmt --check` staying exit 0 here is not
    // this fix's concern and must not change.
    let formatted = run(&["fmt", path, "--check", "--edition", "next"]);
    assert_eq!(formatted.status.code(), Some(0));
}

/// Negative control: a valid spec that also has legacy syntax to migrate
/// must keep working exactly as before this fix — the pre-flight must not
/// reject well-formed input.
#[test]
fn valid_spec_with_legacy_syntax_still_migrates_and_lints() {
    let directory = FixtureDir::new();
    let source = "spec Good {\n  state { n: 0..3 }\n  init { n = 0 }\n  action bump() \"REQ-1: bump\" {\n    requires n < 3\n    n = n + 1\n  }\n  invariant Bound { n >= 0 }\n}\n";
    let path = directory.write("good_legacy.fsl", source);
    let path = path.to_str().expect("UTF-8 path");

    assert_eq!(run(&["check", path]).status.code(), Some(0));

    let migrated = run(&["migrate", path, "--edition", "next"]);
    assert_eq!(migrated.status.code(), Some(0));
    let migrated = json(&migrated);
    assert_eq!(migrated["result"], "migrated", "{migrated:#}");
    assert_eq!(migrated["changed"], 1, "{migrated:#}");
    assert_eq!(migrated["files"][0]["changed"], true, "{migrated:#}");

    let linted = run(&["lint", path, "--edition", "next"]);
    assert_eq!(linted.status.code(), Some(1));
    let linted = json(&linted);
    assert!(
        linted["finding_count"].as_u64().unwrap_or(0) > 0,
        "{linted:#}"
    );
}

/// Regression control: a `refinement`-dialect input (no `state` block —
/// `fslc check` itself fails "spec has no state block" for this dialect,
/// since a refinement spec is not a standalone check target) must keep
/// returning exit 0 from `lint`/`migrate`, exactly as before this fix —
/// the same carve-out `validate_fmt_semantics` already uses.
#[test]
fn refinement_dialect_is_still_not_a_standalone_lint_migrate_target() {
    let directory = FixtureDir::new();
    let source = "refinement Empty {\n  impl Impl\n  abs Abs\n}\n";
    let path = directory.write("refinement_only.fsl", source);
    let path = path.to_str().expect("UTF-8 path");

    let checked = json(&run(&["check", path]));
    assert_eq!(checked["result"], "error", "{checked:#}");

    assert_eq!(
        run(&["migrate", path, "--edition", "next"]).status.code(),
        Some(0)
    );
    assert_eq!(
        run(&["lint", path, "--edition", "next"]).status.code(),
        Some(0)
    );
}

// Requirements `process` forms under `--edition 5` (fsl 5 plan §3 S9): the
// migration rewrites them into `entity` / `event` / `rule … allow`, keeps the
// checked kernel, and refuses what it cannot rewrite without touching a file.

const EDITION_GATE: &str = "fsl5";

fn run_gated(args: &[&str]) -> Output {
    Command::new(env!("CARGO_BIN_EXE_fslc"))
        .args(args)
        .env(fslc_rust::unstable_gate::ENVIRONMENT_VARIABLE, EDITION_GATE)
        .output()
        .expect("run fslc")
}

fn e2e_fixture(name: &str) -> String {
    std::fs::read_to_string(
        Path::new(env!("CARGO_MANIFEST_DIR"))
            .join("tests/fixtures/fsl5/e2e")
            .join(name),
    )
    .expect("read e2e fixture")
}

/// `fslc kernel` of `path` without what a rewrite may move: locations,
/// source nodes, origins and the singular legacy requirement projection.
fn located_free_kernel(path: &Path) -> Value {
    fn strip(value: &mut Value) {
        match value {
            Value::Object(object) => {
                for key in [
                    "span",
                    "source_node_id",
                    "source_node_ids",
                    "reverse_index",
                    "origin",
                    "requirement",
                ] {
                    object.remove(key);
                }
                object.values_mut().for_each(strip);
            }
            Value::Array(values) => values.iter_mut().for_each(strip),
            _ => {}
        }
    }
    let output = run_gated(&["kernel", path.to_str().expect("UTF-8 path")]);
    assert_eq!(
        output.status.code(),
        Some(0),
        "{}",
        String::from_utf8_lossy(&output.stdout)
    );
    let mut kernel = json(&output);
    strip(&mut kernel);
    kernel
}

#[test]
fn edition_5_rewrites_the_e2e_process_form_into_entity_events_and_rules_keeping_the_kernel() {
    let directory = FixtureDir::new();
    directory.write("1_business.fsl", &e2e_fixture("1_business.fsl"));
    let original_source = e2e_fixture("original.fsl");
    let original = directory.write("original.fsl", &original_source);
    let path = original.to_str().expect("UTF-8 path");
    let before = located_free_kernel(&original);

    // Without the gate `--edition 5` is refused, and the default edition
    // rewrites no process.
    let ungated = run(&["migrate", path, "--edition", "5"]);
    assert_eq!(ungated.status.code(), Some(2));
    let ungated = json(&run(&["migrate", path]));
    assert!(
        ungated["files"][0]["findings"]
            .as_array()
            .expect("findings")
            .iter()
            .all(|finding| finding["code"] != "legacy_process_form"),
        "{ungated:#}"
    );
    assert_eq!(text(&original), original_source);

    let output = run_gated(&["migrate", path, "--edition", "5", "--write"]);
    let migrated = json(&output);
    assert_eq!(output.status.code(), Some(0), "{migrated:#}");
    assert_eq!(migrated["result"], "migrated", "{migrated:#}");
    let findings = migrated["files"][0]["findings"]
        .as_array()
        .expect("findings");
    let process = findings
        .iter()
        .find(|finding| finding["code"] == "legacy_process_form")
        .unwrap_or_else(|| panic!("no process finding: {migrated:#}"));
    assert_eq!(process["loc"]["line"], 34, "{process:#}");

    let rewritten = text(&original);
    assert!(rewritten.contains("\nfsl 5\nrequirements"), "{rewritten}");
    for expected in [
        "  actor Employee, System, Manager, Finance\n",
        "  entity Claim {\n    amount: Amount = 0\n    lifecycle Draft -> Submitted -> Approved\n    \
         lifecycle Submitted -> Rejected\n    lifecycle Approved -> Paid\n  }\n",
        "  event submit = Employee submit Claim with a: Amount Draft -> Submitted\n    set amount = a\n",
        "  event pay = Finance pay Claim Approved -> Paid\n",
        "    allow Employee submit Claim only when a > 0\n",
        "    allow Manager mgr_approve Claim only when amount > AUTO_LIMIT\n    \
         allow Manager reject Claim only when amount > AUTO_LIMIT\n",
    ] {
        assert!(
            rewritten.contains(expected),
            "missing {expected:?} in:\n{rewritten}"
        );
    }
    assert!(
        rewritten
            .lines()
            .map(str::trim_start)
            .all(|line| { !line.starts_with("process ") && !line.starts_with("transition ") }),
        "{rewritten}"
    );

    let checked = run_gated(&["check", path]);
    assert_eq!(checked.status.code(), Some(0), "{:#}", json(&checked));
    assert_eq!(json(&checked)["result"], "ok");
    // The edition derives the terminal states from the lifecycle's final
    // stages, which the process form never did: the one difference.
    let mut after = located_free_kernel(&original);
    assert!(before["properties"]["terminal"].is_null(), "{before:#}");
    let terminal = after["properties"]["terminal"].take().to_string();
    for stage in ["\"Rejected\"", "\"Paid\""] {
        assert!(terminal.contains(stage), "{stage} in {terminal}");
    }
    assert_eq!(after, before);

    let again = json(&run_gated(&["migrate", path, "--edition", "5"]));
    assert_eq!(again["changed"], 0, "{again:#}");
}

#[test]
fn edition_5_process_forms_it_cannot_rewrite_are_manual_required_and_no_file_is_written() {
    let cases = [
        (
            "guard_without_requirement",
            6,
            "requirements Guarded {\n  number Amount\n  process Claim with amount: Amount = 0 {\n    \
             stages Draft, Submitted\n    initial Draft\n    transition submit Draft -> Submitted by Employee\n      \
             with a: Amount\n      when a > 0\n      set amount = a\n  }\n}\n\
             verify {\n  instances Claim = 2\n  values Amount = 0..2\n}\n",
        ),
        (
            "input_shares_field_name",
            6,
            "requirements Shadow {\n  number Amount\n  process Claim with amount: Amount = 0 {\n    \
             stages Draft, Submitted\n    initial Draft\n    transition submit Draft -> Submitted by Employee\n      \
             with amount: Amount\n      set amount = amount\n      covers REQ-1 \"submit\"\n  }\n}\n\
             verify {\n  instances Claim = 2\n  values Amount = 0..2\n}\n",
        ),
        (
            "process_entity_shared",
            2,
            "requirements Shared {\n  process claims.Claim {\n    stages Draft, Paid\n    initial Draft\n    \
             transition pay Draft -> Paid by Finance\n  }\n  process legacy.Claim {\n    stages Open, Closed\n    \
             initial Open\n    transition close Open -> Closed by Clerk\n  }\n}\n\
             verify { instances Claim = 2 }\n",
        ),
        (
            "initial_not_first_stage",
            2,
            "requirements Late {\n  process Claim {\n    stages Paid, Draft\n    initial Draft\n    \
             transition pay Draft -> Paid by Finance\n  }\n}\nverify { instances Claim = 2 }\n",
        ),
        (
            "transition_annotation",
            6,
            "requirements Annotated {\n  process Claim {\n    stages Draft, Paid\n    initial Draft\n    \
             @requirement(\"REQ-PAY\", \"payment closes the claim\")\n    \
             transition pay Draft -> Paid by Finance\n  }\n}\nverify { instances Claim = 1 }\n",
        ),
    ];
    let directory = FixtureDir::new();
    let convertible_source = "requirements Plain {\n  process Claim {\n    stages Draft, Paid\n    \
                              initial Draft\n    transition pay Draft -> Paid by Finance\n  }\n}\n\
                              verify { instances Claim = 1 }\n";
    let convertible = directory.write("convertible.fsl", convertible_source);
    for (reason, line, source) in cases {
        let path = directory.write(&format!("{reason}.fsl"), source);
        let path = path.as_path();
        let checked = run(&["check", path.to_str().expect("UTF-8 path")]);
        assert_eq!(
            checked.status.code(),
            Some(0),
            "{reason}: {:#}",
            json(&checked)
        );

        let output = run_gated(&[
            "migrate",
            convertible.to_str().expect("UTF-8 path"),
            path.to_str().expect("UTF-8 path"),
            "--edition",
            "5",
            "--write",
        ]);
        let migrated = json(&output);
        assert_eq!(output.status.code(), Some(1), "{reason}: {migrated:#}");
        assert_eq!(migrated["result"], "migrate_manual_required", "{reason}");
        assert_eq!(migrated["written"], false, "{reason}");
        let manual = migrated["files"][1]["findings"]
            .as_array()
            .expect("findings")
            .iter()
            .filter(|finding| finding["code"] == "migrate_manual_required")
            .collect::<Vec<_>>();
        assert!(!manual.is_empty(), "{reason}: {migrated:#}");
        assert!(
            manual.iter().all(
                |finding| finding["reason"] == reason && finding["machine_applicable"] == false
            ),
            "{reason}: {migrated:#}"
        );
        assert_eq!(manual[0]["loc"]["line"], line, "{reason}: {migrated:#}");
        assert_eq!(migrated["files"][1]["changed"], false, "{reason}");
        assert_eq!(text(path), source, "{reason}: the file is left as it was");
        assert_eq!(
            text(&convertible),
            convertible_source,
            "{reason}: no other input of the run is written"
        );
    }
}

/// Acceptance 3: the migration's own kernel contract is what stops a wrong
/// rewrite. A converter that dropped one guard yields a document that still
/// checks, so only the contract can tell.
#[test]
fn dropping_one_guard_in_the_process_form_converter_fails_the_kernel_contract() {
    let path = Path::new(env!("CARGO_MANIFEST_DIR")).join("../../examples/e2e/2_requirements.fsl");
    let source = text(&path);
    let options = fslc_rust::unstable_gate::from_value(Some(EDITION_GATE));
    let conversion = fslc_rust::migration::plan_process_conversion(&source, &options)
        .expect("the e2e process form is rewritable")
        .expect("the e2e document has a process");
    let faithful = conversion.render(&source, &options);
    assert_eq!(
        fslc_rust::migration::process_form_contract(&source, &faithful, &path, &options),
        Ok(true),
        "equal but for the derived terminal"
    );

    let mut faulty = conversion;
    let event = faulty.processes[0]
        .events
        .iter_mut()
        .find(|event| event.name == "mgr_approve")
        .expect("mgr_approve event");
    assert!(event.guard.take().is_some(), "mgr_approve is guarded");
    let dropped = faulty.render(&source, &options);
    assert!(
        !dropped.contains("allow Manager mgr_approve Claim"),
        "{dropped}"
    );
    let (reason, _) =
        fslc_rust::migration::process_form_contract(&source, &dropped, &path, &options)
            .expect_err("a dropped guard changes the kernel");
    assert_eq!(reason, "kernel_changed");
}

fn example(name: &str) -> String {
    text(&example_path(name))
}

fn example_path(name: &str) -> PathBuf {
    Path::new(env!("CARGO_MANIFEST_DIR"))
        .join("../../examples")
        .join(name)
}

fn findings_with_code<'a>(migrated: &'a Value, code: &str) -> Vec<&'a Value> {
    migrated["files"][0]["findings"]
        .as_array()
        .expect("findings")
        .iter()
        .filter(|finding| finding["code"] == code)
        .collect()
}

fn assert_canonical_example(name: &str) -> Value {
    let path = example_path(name);
    let path = path.to_str().expect("UTF-8 path");
    let checked = run(&["check", path]);
    assert_eq!(checked.status.code(), Some(0), "{}", json(&checked));
    assert_eq!(json(&checked)["result"], "ok");
    assert_eq!(json(&checked)["implements"]["result"], "refines");
    let lint = run(&["lint", path, "--edition", "current"]);
    assert_eq!(lint.status.code(), Some(1), "{}", json(&lint));
    let lint = json(&lint);
    assert_eq!(findings_with_code(&lint, "inline_action_maps").len(), 0);
    let migrate = run(&["migrate", path, "--edition", "next"]);
    assert_eq!(migrate.status.code(), Some(0), "{}", json(&migrate));
    assert_eq!(json(&migrate)["changed"], 0);
    assert!(findings_with_code(&json(&migrate), "inline_action_maps").is_empty());
    lint
}

fn example_contract(path: &Path, source: &str) -> Value {
    fslc_rust::migration::kernel_contract(
        source,
        path,
        &fsl_syntax::ParseOptions::default(),
        fslc_rust::migration::Origins::Kept,
    )
    .expect("checked example contract")
}

#[test]
fn canonical_return_example_has_no_inline_maps() {
    let lint = assert_canonical_example("layers/return_system.fsl");
    assert_eq!(findings_with_code(&lint, "non_canonical_id").len(), 4);
}

#[test]
fn canonical_cancel_example_has_no_inline_maps() {
    assert_canonical_example("pm/cancel_system.fsl");
}

// The pre-canonicalization program at 5a539bbe, without comments/layout. Keep
// this independent of the public example so metadata or semantic loss is caught.
const LEGACY_CANCEL_REQUIREMENTS: &str = r#"requirements CancelSystemReq {
  implements CancelFlow from "cancel_flow.fsl" {
    map sub_stage[c: Sub] =
      if scr[c].st == Browsing then Active
      else if scr[c].st == CancelForm then CancelRequested
      else if scr[c].st == OfferDialog then OfferShown
      else if scr[c].st == ThanksStay then Retained
      else Churned
  }
  type Sub = 0..2
  enum Screen { Browsing, CancelForm, OfferDialog, ThanksStay, GoodbyePage }
  struct SubView { st: Screen, offered: Bool }
  state { scr: Map<Sub, SubView>, churn_count: Int, retain_count: Int }
  init {
    forall c: Sub { scr[c] = SubView { st: Browsing, offered: false } }
    churn_count = 0
    retain_count = 0
  }
  requirement REQ-1 "Pressing the cancel button transitions to the cancellation form" {
    fair action tap_cancel(c: Sub) maps request_cancel(c) {
      requires scr[c].st == Browsing
      scr[c].st = CancelForm
    }
  }
  requirement REQ-2 "On cancellation-form submission, show the retention offer exactly once per subscription" {
    fair action submit_cancel(c: Sub) maps show_offer(c) {
      requires scr[c].st == CancelForm
      requires not scr[c].offered
      scr[c] = SubView { st: OfferDialog, offered: true }
    }
  }
  requirement REQ-3 "Accepting the offer goes to retention (thanks screen); declining goes to the cancellation-complete screen" {
    fair action accept(c: Sub) maps accept_offer(c) {
      requires scr[c].st == OfferDialog
      scr[c].st = ThanksStay
      retain_count = retain_count + 1
    }
    fair action decline(c: Sub) maps decline_offer(c) {
      requires scr[c].st == OfferDialog
      scr[c].st = GoodbyePage
      churn_count = churn_count + 1
    }
    invariant CountsMatchScreens {
      churn_count == count(c: Sub where scr[c].st == GoodbyePage)
        and retain_count == count(c: Sub where scr[c].st == ThanksStay)
    }
  }
  requirement REQ-4 "An offer is not re-shown to a subscription that has already been shown one" {
    invariant OfferedFlagConsistent {
      forall c: Sub {
        (scr[c].st == OfferDialog or scr[c].st == ThanksStay or scr[c].st == GoodbyePage)
          => scr[c].offered
      }
    }
  }
  acceptance AC-1 "Acceptance flow: request -> show -> accept results in retention" {
    tap_cancel(0)
    submit_cancel(0)
    accept(0)
    expect scr[0].st == ThanksStay and retain_count == 1
  }
  acceptance AC-2 "Decline flow: request -> show -> decline completes the cancellation" {
    tap_cancel(1)
    submit_cancel(1)
    decline(1)
    expect scr[1].st == GoodbyePage and churn_count == 1
  }
}
"#;

#[test]
fn canonical_cancel_preserves_the_checked_kernel_and_typed_metadata() {
    let path = example_path("pm/cancel_system.fsl");
    assert_eq!(
        example_contract(&path, LEGACY_CANCEL_REQUIREMENTS),
        example_contract(&path, &text(&path))
    );
}

#[test]
fn canonical_return_preserves_the_checked_kernel_and_typed_metadata() {
    let path = example_path("layers/return_system.fsl");
    let legacy = text(
        &Path::new(env!("CARGO_MANIFEST_DIR")).join("tests/fixtures/legacy_return_branches.fsl"),
    );
    let import = "../../../../examples/layers/return_policy.fsl";
    assert_eq!(legacy.matches(import).count(), 1);
    // Resolve the same policy under one logical source path. Use the existing
    // migration contract: location-free Kernel plus the full typed annotations.
    let mut before = example_contract(&path, &legacy.replace(import, "return_policy.fsl"));
    let after = example_contract(&path, &text(&path));
    for (old, new) in [
        ("submit__b1", "submit_small"),
        ("submit__b2", "submit_large"),
    ] {
        let action = before["kernel"]["actions"]
            .as_array_mut()
            .expect("actions")
            .iter_mut()
            .find(|action| action["name"] == old)
            .expect("legacy branch");
        assert_eq!(action["fair"], true);
        action["name"] = new.into();
        let annotations = before["annotations"].as_object_mut().expect("annotations");
        let inherited = annotations
            .remove(&fsl_core::action_target(old))
            .expect("inherited requirement annotations");
        assert!(
            annotations
                .insert(fsl_core::action_target(new), inherited)
                .is_none()
        );
    }
    before["kernel"]["actions"]
        .as_array_mut()
        .expect("actions")
        .sort_by(|left, right| left["name"].as_str().cmp(&right["name"].as_str()));
    assert_eq!(before, after);
}

fn return_candidates(monitor: &Monitor) -> Vec<EnabledAction> {
    monitor
        .enabled_where(|_, params| params.get("c") == Some(&FslValue::Int(0)))
        .expect("enabled return actions")
}

fn return_step(monitor: &mut Monitor, name: &str, amount: Option<i64>) {
    let action = return_candidates(monitor)
        .into_iter()
        .find(|action| {
            action.action == name
                && amount.is_none_or(|value| action.params.get("a") == Some(&FslValue::Int(value)))
        })
        .expect("expected return action is enabled");
    let result = monitor.step(&action).expect("return step");
    assert_eq!(result.violation, None, "{result:?}");
}

fn return_stage(monitor: &Monitor) -> &str {
    let FslValue::Map(cases) = &monitor.state["sys"] else {
        panic!("case map")
    };
    let FslValue::Struct { fields, .. } = &cases[&FslValue::Int(0)] else {
        panic!("case record")
    };
    let FslValue::Enum { member, .. } = &fields["st"] else {
        panic!("case stage")
    };
    member
}

#[test]
fn canonical_return_threshold_payment_and_manager_guards() {
    let path = example_path("layers/return_system.fsl");
    let resolver = fsl_core::FsResolver::new(path.parent().expect("example directory"));
    let kernel = fsl_core::parse_kernel_source(&text(&path), &resolver).expect("return kernel");
    let model = fsl_core::build_model(kernel).expect("return model");
    for amount in 0..=3 {
        let mut monitor = Monitor::new(model.clone()).expect("deterministic return init");
        let submits = return_candidates(&monitor)
            .into_iter()
            .filter(|action| action.params.get("a") == Some(&FslValue::Int(amount)))
            .map(|action| action.action)
            .collect::<Vec<_>>();
        if amount == 0 {
            assert!(submits.is_empty(), "zero is not a valid submission");
            continue;
        }
        let submit = if amount == 1 {
            "submit_small"
        } else {
            "submit_large"
        };
        assert_eq!(submits, [submit]);
        return_step(&mut monitor, submit, Some(amount));
        assert_eq!(
            return_stage(&monitor),
            if amount == 1 {
                "AutoApproved"
            } else {
                "MgrQueue"
            }
        );
        let enabled = return_candidates(&monitor)
            .into_iter()
            .map(|action| action.action)
            .collect::<Vec<_>>();
        assert_eq!(
            enabled,
            if amount == 1 {
                vec!["pay"]
            } else {
                vec!["mgr_approve", "mgr_reject"]
            }
        );
        if amount > 1 {
            let mut rejected = monitor.clone();
            return_step(&mut rejected, "mgr_reject", None);
            assert_eq!(return_stage(&rejected), "MgrRejected");
            assert!(return_candidates(&rejected).is_empty());
            return_step(&mut monitor, "mgr_approve", None);
            assert_eq!(return_stage(&monitor), "MgrApproved");
        }
        return_step(&mut monitor, "pay", None);
        assert_eq!(return_stage(&monitor), "Paid");
        assert_eq!(monitor.state["paid_count"], FslValue::Int(1));
        assert!(return_candidates(&monitor).is_empty());
    }
}

/// decisions 053: the edition derives the terminal states of a process form
/// without `terminal { }` from its lifecycle's final stages, which changes
/// verify's deadlock verdict on the e2e requirements. The migration says so,
/// in the plan and when it writes, without changing its exit status.
#[test]
fn edition_5_migration_reports_the_terminal_it_derives_for_a_process_form_without_one() {
    let directory = FixtureDir::new();
    directory.write("1_business.fsl", &example("e2e/1_business.fsl"));
    let source = example("e2e/2_requirements.fsl");
    assert!(
        !source.contains("terminal"),
        "the e2e process form has none"
    );
    let path = directory.write("2_requirements.fsl", &source);
    let path = path.to_str().expect("UTF-8 path");

    let mut reported = Vec::new();
    for write in [false, true] {
        let mut args = vec!["migrate", path, "--edition", "5"];
        if write {
            args.push("--write");
        }
        let output = run_gated(&args);
        let migrated = json(&output);
        assert_eq!(output.status.code(), Some(0), "{migrated:#}");
        assert_eq!(migrated["result"], "migrated", "{migrated:#}");
        assert_eq!(migrated["written"], write, "{migrated:#}");
        let added = findings_with_code(&migrated, "derived_terminal_added");
        assert_eq!(added.len(), 1, "{migrated:#}");
        let finding = added[0];
        assert_eq!(finding["severity"], "info", "{finding:#}");
        assert_eq!(finding["machine_applicable"], true, "{finding:#}");
        assert_eq!(finding["loc"]["line"], 34, "at the process: {finding:#}");
        let message = finding["message"].as_str().expect("message");
        for expected in ["'Claim' in Rejected or Paid", "deadlock"] {
            assert!(message.contains(expected), "{expected:?} in {message}");
        }
        assert_eq!(
            finding["canonical_replacement"],
            "terminal { forall c: Claim { stage(c) == Rejected or stage(c) == Paid } }"
        );
        reported.push(finding.clone());
    }
    assert_eq!(
        reported[0], reported[1],
        "the plan reports what --write does"
    );

    // The written document is the edition's own form: nothing more to derive.
    let again = json(&run_gated(&["migrate", path, "--edition", "5"]));
    assert_eq!(again["changed"], 0, "{again:#}");
    assert!(
        findings_with_code(&again, "derived_terminal_added").is_empty(),
        "{again:#}"
    );
}

/// A process form that writes its terminal states keeps them: the rewrite
/// adds no terminal and reports none.
#[test]
fn edition_5_migration_of_a_process_form_with_terminal_reports_no_derived_terminal() {
    let directory = FixtureDir::new();
    let source = example("requirements_stage.fsl");
    assert!(source.contains("terminal {"), "the fixture writes one");
    let path = directory.write("requirements_stage.fsl", &source);
    let path = path.to_str().expect("UTF-8 path");
    for write in [false, true] {
        let mut args = vec!["migrate", path, "--edition", "5"];
        if write {
            args.push("--write");
        }
        let output = run_gated(&args);
        let migrated = json(&output);
        assert_eq!(output.status.code(), Some(0), "{migrated:#}");
        assert_eq!(
            findings_with_code(&migrated, "legacy_process_form").len(),
            1,
            "the process is rewritten: {migrated:#}"
        );
        assert!(
            findings_with_code(&migrated, "derived_terminal_added").is_empty(),
            "{migrated:#}"
        );
    }
}

/// The derived terminal is the one kernel difference the process-form
/// contract forgives, and only where the process form wrote no terminal: a
/// rewrite that changed a written terminal fails it.
#[test]
fn the_process_form_contract_compares_a_terminal_the_process_form_writes() {
    let path = Path::new(env!("CARGO_MANIFEST_DIR")).join("../../examples/requirements_stage.fsl");
    let source = text(&path);
    let options = fslc_rust::unstable_gate::from_value(Some(EDITION_GATE));
    let conversion = fslc_rust::migration::plan_process_conversion(&source, &options)
        .expect("the process form is rewritable")
        .expect("the document has a process");
    let faithful = conversion.render(&source, &options);
    assert_eq!(
        fslc_rust::migration::process_form_contract(&source, &faithful, &path, &options),
        Ok(false),
        "equal, terminal included"
    );

    let written = "forall c: Claim { stage(c) == Approved or stage(c) == Rejected }";
    assert_eq!(faithful.matches(written).count(), 1, "{faithful}");
    let narrowed = faithful.replace(written, "forall c: Claim { stage(c) == Approved }");
    let (reason, _) =
        fslc_rust::migration::process_form_contract(&source, &narrowed, &path, &options)
            .expect_err("a changed written terminal changes the kernel");
    assert_eq!(reason, "kernel_changed");
}
