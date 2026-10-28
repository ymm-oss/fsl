// SPDX-License-Identifier: Apache-2.0

#[path = "support/edition_dependency_cases.rs"]
mod cases;
#[path = "support/edition_cases.rs"]
mod common;
use cases::{Observations, Variant};
use serde_json::{Value, json};
use std::collections::BTreeMap;
use std::fmt::Write as _;
use std::path::Path;

fn run(s: &common::Scratch, command: &str) -> std::process::Output {
    common::command(env!("CARGO_BIN_EXE_fslc"), &s.0, command, true)
}

fn git(s: &common::Scratch, args: &[&str]) {
    let output = std::process::Command::new("git")
        .args([
            "-c",
            "user.name=Dependency Test",
            "-c",
            "user.email=dependency@example.invalid",
            "-c",
            "commit.gpgsign=false",
        ])
        .args(args)
        .current_dir(&s.0)
        .env("GIT_CONFIG_GLOBAL", "/dev/null")
        .env("GIT_CONFIG_NOSYSTEM", "1")
        .output()
        .unwrap();
    assert!(
        output.status.success(),
        "scratch Git {args:?}: {}",
        String::from_utf8_lossy(&output.stderr)
    );
}

fn payload(output: &std::process::Output) -> Value {
    serde_json::from_slice(&output.stdout).map_or_else(
        |_| json!(String::from_utf8(output.stdout.clone()).unwrap()),
        cases::native_value,
    )
}

fn tree(path: &Path) -> BTreeMap<String, (usize, String)> {
    use sha2::Digest;
    fn visit(base: &Path, path: &Path, files: &mut BTreeMap<String, (usize, String)>) {
        if !path.exists() {
            return;
        }
        if path.is_dir() {
            files.insert(
                format!("{}/", path.strip_prefix(base).unwrap().display()),
                (0, String::new()),
            );
            for entry in std::fs::read_dir(path).unwrap() {
                visit(base, &entry.unwrap().path(), files);
            }
        } else {
            let bytes = std::fs::read(path).unwrap();
            files.insert(
                path.strip_prefix(base)
                    .unwrap()
                    .to_string_lossy()
                    .into_owned(),
                (bytes.len(), format!("{:x}", sha2::Sha256::digest(bytes))),
            );
        }
    }
    let mut files = BTreeMap::new();
    visit(path, path, &mut files);
    files
}

fn stable_artifact(path: &Path) -> (Value, std::collections::BTreeSet<String>) {
    let bytes = std::fs::read(path).unwrap();
    cases::stable(
        serde_json::from_slice(&bytes).unwrap_or_else(|_| json!(String::from_utf8(bytes).unwrap())),
    )
}

fn preserve(
    observations: &mut Observations,
    id: &str,
    s: &common::Scratch,
    command: &str,
    status: i32,
    result: &str,
) -> Value {
    let first = run(s, command);
    let second = run(s, command);
    let a = cases::stable(payload(&first));
    let b = cases::stable(payload(&second));
    assert_eq!(
        a.0["result"], result,
        "{id} canonical route must reach its real consumer"
    );
    observations.compare(
        &format!("{id}/canonical"),
        (first.status.code(), &a, &first.stderr),
        (Some(status), &b, &second.stderr),
        first.status.code() == Some(status)
            && second.status.code() == Some(status)
            && a == b
            && first.stderr == second.stderr,
    );
    a.0
}

fn reject(
    observations: &mut Observations,
    id: &str,
    s: &common::Scratch,
    command: &str,
    child: &str,
    source: &str,
) {
    let actual = run(s, command);
    let value = payload(&actual);
    let expected = cases::refusal(
        source,
        s.0.join(child).canonicalize().unwrap().to_str().unwrap(),
        serde_json::from_value(json!({"fsl":"1.0"})).unwrap(),
    );
    observations.compare(
        id,
        (actual.status.code(), &value, &actual.stderr),
        (Some(2), &expected, Vec::<u8>::new()),
        actual.status.code() == Some(2) && value == expected && actual.stderr.is_empty(),
    );
}

fn dependency_edition_causal_routes_and_all_observation_outputs() {
    let mut observations = Observations::default();
    let repo = common::root();
    for row in cases::flat_rows()
        .into_iter()
        .filter(|r| matches!(r.id.split('/').next().unwrap(), "E07" | "E08" | "E09"))
    {
        let s = common::Scratch::new();
        let fixture = cases::fixture(&row);
        for (name, source) in &fixture.files {
            s.write(name, source);
        }
        s.write("root.fsl", &fixture.root);
        let observe = format!(
            "causal observe-expectations root.fsl --from-log {}/examples/causal/evidence/incident-observation-log.jsonl --mapping {}/examples/causal/evidence/incident-log-mapping.fsl --scope {}/examples/causal/evidence/incident-replay-scope.json --period-start 2026-01-01 --period-end 2026-03-31 --out evidence.json --lifecycle-out lifecycle.json",
            repo.display(),
            repo.display(),
            repo.display()
        );
        let commands = match row.id.split('/').next().unwrap() {
            "E07" => vec![
                "causal check root.fsl".into(),
                "causal analyze root.fsl --projection causal_graph".into(),
                "causal diff root.fsl root.fsl".into(),
                "causal ledger root.fsl".into(),
                "causal verify-expectations root.fsl --depth 2".into(),
                observe,
            ],
            "E08" => vec![
                "causal verify-expectations root.fsl --depth 2".into(),
                observe,
            ],
            _ => vec![observe],
        };
        for command in commands {
            let source = &fixture.files[&fixture.child];
            if row.variant == Variant::N {
                let before = tree(&s.0);
                reject(
                    &mut observations,
                    &format!("{}/{}", row.id, command.split_whitespace().nth(1).unwrap()),
                    &s,
                    &command,
                    &fixture.child,
                    source,
                );
                let after = tree(&s.0);
                observations.compare(
                    &format!("{}/artifacts", row.id),
                    &after,
                    &before,
                    after == before,
                );
            } else {
                let mut twin = row.clone();
                twin.variant = Variant::P;
                let twin = cases::fixture(&twin);
                s.write(&fixture.child, &twin.files[&fixture.child]);
                let mut expected = payload(&run(&s, &command));
                assert_ne!(
                    expected["result"], "error",
                    "canonical causal route {command}: {expected}"
                );
                if row.variant == Variant::W {
                    if expected.get("warnings").is_none() {
                        expected["warnings"] = json!([]);
                    }
                    expected["warnings"]
                        .as_array_mut()
                        .unwrap()
                        .extend(cases::findings(
                            source,
                            s.0.join(&fixture.child)
                                .canonicalize()
                                .unwrap()
                                .to_str()
                                .unwrap(),
                            "current",
                        ));
                }
                s.write(&fixture.child, source);
                let actual = run(&s, &command);
                let value = payload(&actual);
                observations.compare(
                    &format!("{}/{}", row.id, command.split_whitespace().nth(1).unwrap()),
                    (actual.status.code(), &value),
                    (Some(0), &expected),
                    actual.status.code() == Some(0) && value == expected,
                );
            }
        }
    }
    observations.finish();
}

fn dependency_edition_warnings_preserve_canonical_identity_and_distinct_findings() {
    let mut observations = Observations::default();
    for id in ["W01", "W02", "W03", "W04", "D04"] {
        let s = common::Scratch::new();
        std::fs::create_dir_all(s.0.join("dir")).unwrap();
        let body = if id == "W03" {
            cases::LEGACY_LEAF.replace(
                "invariant Typed",
                "invariant Other \"REQ-OTHER-001: other\" { true } invariant Typed",
            )
        } else {
            cases::LEGACY_LEAF.into()
        };
        s.write("dep.fsl", &body);
        s.write("dir/dep.fsl", &body);
        let mut source = "compose C {\n use S as a from \"dep.fsl\"\n".to_owned();
        if id != "D04" {
            source.push_str(if id == "W02" {
                " use S as b from \"./dep.fsl\"\n use S as c from \"dir/../dep.fsl\"\n"
            } else if id == "W04" {
                " use S as b from \"dir/dep.fsl\"\n"
            } else {
                " use S as b from \"dep.fsl\"\n"
            });
        }
        source.push_str("}\n");
        if id == "D04" {
            source = format!("fsl 5\n{source}");
        }
        s.write("root.fsl", &source);
        let command = if id == "D04" {
            "check root.fsl --edition next"
        } else {
            "check root.fsl"
        };
        let actual = run(&s, command);
        let value = payload(&actual);
        let mut expected = json!({"fsl":"1.0","result":"ok","spec":"C","warnings":[]});
        let identity = s.0.join("dep.fsl").canonicalize().unwrap();
        let mut findings = cases::findings(&body, identity.to_str().unwrap(), "current");
        if id == "W04" {
            findings.push(cases::metadata_finding(
                &body,
                s.0.join("dir/dep.fsl")
                    .canonicalize()
                    .unwrap()
                    .to_str()
                    .unwrap(),
                "current",
            ));
        }
        if id == "W03" {
            let admitted = fslc_rust::migration::admit_source(
                &body,
                identity.to_str().unwrap(),
                None,
                &common::gated(),
                &serde_json::Map::new,
                false,
            )
            .unwrap();
            assert_eq!(
                admitted.warnings(),
                findings.as_slice(),
                "independent two-finding oracle calibration"
            );
        }
        expected["warnings"] = json!(findings);
        if id == "D04" {
            expected["edition"] = json!("next");
            expected["fsl_edition"] = json!(5);
        }
        observations.compare(
            id,
            (actual.status.code(), &value),
            (Some(0), &expected),
            actual.status.code() == Some(0) && value == expected,
        );
    }
    observations.finish();
}

#[allow(clippy::too_many_lines)] // One writer/raw-delivery table and its shared sentinel protocol.
fn dependency_edition_artifact_and_raw_routes_refuse_before_effects() {
    let mut observations = Observations::default();
    for (id, command, violated) in [
        ("A01", "testgen root.fsl --depth 2 -o output/result", false),
        (
            "A02",
            "html root.fsl --depth 2 --no-timing -o output/result",
            false,
        ),
        ("A02", "html root.fsl --depth 2 --no-timing", false),
        ("A03", "ledger root.fsl --depth 2 -o output/result", false),
        ("A03", "ledger root.fsl --depth 2", false),
        (
            "A04",
            "counterexample export root.fsl --depth 2 -o output/result",
            true,
        ),
        ("A10", "kernel root.fsl --kernel-version 2", false),
        (
            "A10",
            "conformance root.fsl --depth 2 --kernel-version 2",
            false,
        ),
        ("A10", "testplan root.fsl --depth 2", false),
        ("A10", "typestate root.fsl --ts", false),
        ("A10", "explain root.fsl --depth 2 --readable", false),
        ("A10", "analyze root.fsl --format dot", false),
        ("T01", "check root.fsl", false),
        (
            "T02",
            "verify root.fsl --depth 2 --engine bmc --no-cache",
            false,
        ),
        (
            "T02",
            "verify root.fsl --depth 2 --engine auto --explicit-budget 1 --no-cache",
            false,
        ),
        ("T03", "sweep root.fsl --depth 2..2", false),
        ("T07", "mutate root.fsl --depth 2 --max-mutants 0", false),
        ("T07", "mutate root.fsl --depth 2 --max-mutants 1", false),
        (
            "T08",
            "counterexample export root.fsl --depth 2 --engine bmc -o output/result",
            true,
        ),
        (
            "T09",
            "html root.fsl --depth 2 --engine bmc --no-timing -o output/result",
            false,
        ),
        (
            "T10",
            "ledger root.fsl --depth 2 --engine bmc -o output/result",
            false,
        ),
        ("T14", "scenarios root.fsl --depth 2", true),
        ("T15", "testgen root.fsl --depth 2 -o output/result", true),
        ("T17", "explain root.fsl --depth 2", true),
    ] {
        let s = common::Scratch::new();
        std::fs::create_dir_all(s.0.join("output")).unwrap();
        let body = if violated {
            cases::LEAF.replace("invariant Typed { true }", "invariant Typed { not done }")
        } else {
            cases::LEAF.into()
        };
        let canonical_body = body.replace(
            "invariant Typed",
            "@requirement(\"REQ-DEP-001\", \"日本語😀\") invariant Typed",
        );
        s.write("dep.fsl", &canonical_body);
        s.write("root.fsl", "compose C { use S as s from \"dep.fsl\" }\n");
        let canonical_command = command.replace("output/result", "output/canonical");
        let canonical = run(&s, &canonical_command);
        let value = payload(&canonical);
        let first_artifact = if command.contains("-o") && id != "T15" {
            Some(stable_artifact(&s.0.join("output/canonical")))
        } else {
            None
        };
        let repeated = run(&s, &canonical_command);
        let repeated_value = payload(&repeated);
        let repeated_artifact = if command.contains("-o") && id != "T15" {
            Some(stable_artifact(&s.0.join("output/canonical")))
        } else {
            None
        };
        let first = cases::stable(value.clone());
        let second = cases::stable(repeated_value);
        observations.compare(
            &format!("{id}/canonical-full/{command}"),
            (
                canonical.status.code(),
                &first,
                &canonical.stderr,
                &first_artifact,
            ),
            (
                repeated.status.code(),
                &second,
                &repeated.stderr,
                &repeated_artifact,
            ),
            canonical.status.code() == repeated.status.code()
                && first == second
                && canonical.stderr == repeated.stderr
                && first_artifact == repeated_artifact,
        );
        if command.starts_with("kernel ")
            || command.starts_with("testplan ")
            || command.starts_with("typestate ")
        {
            let version = if command.contains("--kernel-version 2") {
                "v2"
            } else {
                "v1"
            };
            let message = if command.starts_with("testplan ") {
                "advanced test planning requires truthful Public Kernel export; compose unsupported"
                    .into()
            } else {
                format!(
                    "public Kernel {version} cannot preserve component source filenames after compose lowering"
                )
            };
            let expected =
                json!({"fsl":"1.0","result":"error","kind":"semantics","message":message});
            observations.compare(
                &format!("{id}/canonical-export"),
                (canonical.status.code(), &value),
                (Some(2), &expected),
                canonical.status.code() == Some(2) && value == expected,
            );
        } else if command.starts_with("conformance ") {
            let expected = json!({"$schema":"https://fsl.dev/schemas/fslc/kernel/conformance.v2.schema.json","schema_version":"2.0.0","kernel_schema_version":"2.0.0","result":"conformance","spec":"C","depth":2,
                "states":[{"id":"s0","depth":0,"state":{"s.done":false}},{"id":"s1","depth":1,"state":{"s.done":true}}],
                "vectors":[{"state":"s0","action":{"name":"s.finish","params":{}},"outcome":{"kind":"ok","state_changed":true,"state":{"s.done":true},"changes":{"s.done":{"from":false,"to":true}}}},
                    {"state":"s1","action":{"name":"s.finish","params":{}},"outcome":{"kind":"ok","state_changed":false,"state":{"s.done":true},"changes":{}}}],"fsl":"1.0"});
            observations.compare(
                "A10/canonical-conformance",
                (canonical.status.code(), &value),
                (Some(0), &expected),
                canonical.status.code() == Some(0) && value == expected,
            );
        } else {
            assert!(
                canonical.status.code() == Some(i32::from(violated && id != "T17")),
                "{id} canonical control {command}: {value}"
            );
        }
        if command.contains("--engine auto") {
            assert_eq!(
                value["engine"], "bmc",
                "T02 auto control must exercise BMC, not only explicit closure"
            );
        }
        // Every writer is first exercised, then given its own sentinel tree.
        if command.contains("-o") && id != "T15" {
            assert!(
                s.0.join("output/canonical").exists(),
                "canonical writer did not write"
            );
        }
        s.write("output/result", "unchanged sentinel\n");
        let before = tree(&s.0.join("output"));
        if id == "T15" {
            assert!(
                !s.0.join("output/canonical").exists(),
                "violated testgen must not create an artifact"
            );
        }
        let rejected = format!(
            "fsl 5\n{}",
            body.replace(
                "invariant Typed",
                "invariant Typed \"REQ-DEP-001: 日本語😀\""
            )
        );
        s.write("dep.fsl", &rejected);
        reject(
            &mut observations,
            &format!("{id}/{command}"),
            &s,
            command,
            "dep.fsl",
            &rejected,
        );
        let after = tree(&s.0.join("output"));
        observations.compare(
            &format!("{id}/sentinel/{command}"),
            &after,
            &before,
            after == before,
        );
        if command.contains("-o") {
            let missing_parent = command.replace("output/result", "missing-parent/result");
            reject(
                &mut observations,
                &format!("{id}/missing-parent"),
                &s,
                &missing_parent,
                "dep.fsl",
                &rejected,
            );
            assert!(!s.0.join("missing-parent").exists());
        }
    }
    let s = common::Scratch::new();
    let root = "compose C { use S as s from \"dep.fsl\" }\n";
    s.write("root.fsl", root);
    s.write("dep.fsl", cases::LEAF);
    let baseline = run(&s, "fmt root.fsl");
    assert_eq!(baseline.status.code(), Some(0));
    let child = format!("fsl 5\n{}", cases::LEGACY_LEAF);
    s.write("dep.fsl", &child);
    let actual = run(&s, "fmt root.fsl");
    let finding = cases::metadata_finding(
        &child,
        s.0.join("dep.fsl")
            .canonicalize()
            .unwrap()
            .to_str()
            .unwrap(),
        "next",
    );
    let stderr = format!("warning: {finding}\n").into_bytes();
    observations.compare(
        "A15/fmt",
        (actual.status.code(), &actual.stdout, &actual.stderr),
        (Some(0), &baseline.stdout, &stderr),
        actual.status.code() == Some(0)
            && actual.stdout == baseline.stdout
            && actual.stderr == stderr,
    );
    assert_eq!(std::fs::read_to_string(s.0.join("root.fsl")).unwrap(), root);
    assert_eq!(std::fs::read_to_string(s.0.join("dep.fsl")).unwrap(), child);
    observations.finish();
}

fn dependency_edition_repair_roles_report_but_do_not_reject_or_edit_children() {
    let mut observations = Observations::default();
    for (id, command, status) in [
        ("R01", "lint root.fsl --edition next", 1),
        ("R02", "migrate root.fsl --edition next", 0),
        ("R03", "fmt root.fsl --check", 1),
        ("A15", "migrate root.fsl --edition next --write", 0),
    ] {
        let s = common::Scratch::new();
        let root = if id == "R03" {
            "compose C { use S as s from \"dep.fsl\" invariant Root \"REQ-ROOT-001: root\" { true } }\n".into()
        } else {
            cases::IMPLEMENTS.replace(
                "invariant Typed { true }",
                "invariant Typed \"REQ-ROOT-001: root\" { true }",
            )
        };
        s.write("root.fsl", &root);
        s.write("dep.fsl", cases::LEAF);
        let baseline = run(&s, command);
        let mut expected = payload(&baseline);
        assert_eq!(
            baseline.status.code(),
            Some(status),
            "{id} canonical repair: {expected}"
        );
        let expected_root = std::fs::read(s.0.join("root.fsl")).unwrap();
        s.write("root.fsl", &root);
        let child = format!("fsl 5\n{}", cases::LEGACY_LEAF);
        s.write("dep.fsl", &child);
        let finding = cases::metadata_finding(
            &child,
            s.0.join("dep.fsl")
                .canonicalize()
                .unwrap()
                .to_str()
                .unwrap(),
            "next",
        );
        if expected.get("warnings").is_none() {
            expected["warnings"] = json!([]);
        }
        expected["warnings"].as_array_mut().unwrap().push(finding);
        let actual = run(&s, command);
        let value = payload(&actual);
        observations.compare(
            id,
            (actual.status.code(), &value),
            (Some(status), &expected),
            actual.status.code() == Some(status) && value == expected,
        );
        assert_eq!(std::fs::read_to_string(s.0.join("dep.fsl")).unwrap(), child);
        assert_eq!(std::fs::read(s.0.join("root.fsl")).unwrap(), expected_root);
    }
    observations.finish();
}

#[allow(clippy::too_many_lines)] // Full nonreader envelopes and specialized dialect controls.
fn dependency_edition_document_nonreaders_and_specialized_controls_are_preserved() {
    let mut observations = Observations::default();
    let s = common::Scratch::new();
    let root=common::DOCUMENT_LEGACY.replace("requirements S {", "requirements S { implements S from \"dep.fsl\" { map done = done action finish() -> finish() }");
    s.write("root.fsl", &root);
    s.write("dep.fsl", cases::LEAF);
    for (id, command) in [
        ("A11", "document generate root.fsl -o document.md"),
        ("A12", "document claims root.fsl -o claims.json"),
        ("A13", "document check root.fsl document.md"),
    ] {
        s.write("dep.fsl", cases::LEAF);
        let canonical = run(&s, command);
        let expected = payload(&canonical);
        assert_eq!(
            canonical.status.code(),
            Some(0),
            "{id} canonical document: {expected}"
        );
        let before = (
            std::fs::read(s.0.join("document.md")).unwrap(),
            std::fs::read(s.0.join("claims.json")).ok(),
        );
        s.write("dep.fsl", &format!("fsl 5\n{}", cases::LEGACY_LEAF));
        let actual = run(&s, command);
        let value = payload(&actual);
        let after = (
            std::fs::read(s.0.join("document.md")).unwrap(),
            std::fs::read(s.0.join("claims.json")).ok(),
        );
        observations.compare(
            id,
            (actual.status.code(), &value, &after),
            (Some(0), &expected, &before),
            actual.status.code() == Some(0) && value == expected && after == before,
        );
    }
    let root_refused = format!("fsl 5\n{}", cases::LEGACY_LEAF);
    s.write("root.fsl", &root_refused);
    let actual = run(&s, "document check root.fsl absent-document.md");
    let value = payload(&actual);
    let expected = cases::refusal(
        &root_refused,
        "root.fsl",
        serde_json::from_value(json!({"fsl":"1.0"})).unwrap(),
    );
    observations.compare(
        "A13/root-priority",
        (actual.status.code(), &value),
        (Some(2), &expected),
        actual.status.code() == Some(2) && value == expected,
    );
    let repo = common::root();
    s.write("domain.fsl", common::DOMAIN);
    for (id, command, result) in [
        (
            "A14",
            "domain expand domain.fsl -o expanded.fsl".into(),
            "expanded",
        ),
        (
            "A14",
            "domain generate domain.fsl -o generated".into(),
            "generated",
        ),
        (
            "A14",
            "db import schema.sql --source sql -o imported.fsl".into(),
            "imported",
        ),
        (
            "T11",
            format!(
                "db check {}/examples/db/safe_add_nullable_column.fsl --depth 2 --engine bmc",
                repo.display()
            ),
            "verified_under_assumptions",
        ),
        (
            "T12",
            format!(
                "ai check {}/examples/ai/refund_agent_tool_safety.fsl --depth 2 --engine bmc",
                repo.display()
            ),
            "verified_under_assumptions",
        ),
        (
            "T13",
            "domain check domain.fsl --depth 2 --engine bmc".into(),
            "verified_under_assumptions",
        ),
        (
            "T16",
            "domain testgen domain.fsl --depth 2 -o domain-tests".into(),
            "generated",
        ),
    ] {
        preserve(&mut observations, id, &s, &command, 0, result);
    }
    s.write(
        "violated-domain.fsl",
        &common::DOMAIN.replace("status == Open or status == Closed", "status == Open"),
    );
    preserve(
        &mut observations,
        "T16/fallback",
        &s,
        "domain testgen violated-domain.fsl --depth 2 -o domain-failure-tests",
        1,
        "violated",
    );
    assert!(!s.0.join("domain-failure-tests").exists());
    observations.finish();
}

#[allow(clippy::too_many_lines)] // Shared jobs/refinement fixture plus complete layered envelopes.
fn dependency_edition_chain_steps_refuse_before_implementation_effects() {
    let mut observations = Observations::default();
    for (id, jobs, depth, refine) in [
        ("T04", 1, false, false),
        ("T05", 2, true, false),
        ("T06", 1, true, true),
        ("T06", 2, true, true),
    ] {
        let s = common::Scratch::new();
        let parallel = id == "T05";
        let source = "compose C { use S as s from \"dep.fsl\" }\n";
        s.write("root.fsl", source);
        s.write(
            "abstract.fsl",
            &source.replace(
                "dep.fsl",
                if parallel { "other-dep.fsl" } else { "dep.fsl" },
            ),
        );
        s.write("dep.fsl", cases::LEAF);
        s.write("other-dep.fsl", cases::LEAF);
        s.write("map.fsl", "refinement M { impl C abs C maps auto }\n");
        let mut manifest = format!(
            "{}[design]\nfile = \"root.fsl\"\n{}{}[impl]\ncommand = \"touch marker\"\nevidence = \"exit_code\"\n",
            if refine || parallel {
                "[requirements]\nfile = \"abstract.fsl\"\ndepth = 2\n"
            } else {
                ""
            },
            if depth { "depth = 2\n" } else { "" },
            if refine {
                "refine_against = \"requirements\"\nmapping = \"map.fsl\"\n"
            } else {
                ""
            }
        );
        if parallel {
            manifest = manifest.split_once("[impl]").unwrap().0.into();
        }
        s.write(
            "chain.toml",
            &manifest.replace("touch marker", "touch canonical-marker"),
        );
        let command = format!(
            "chain chain.toml --jobs {jobs} {}",
            if parallel { "--keep-going" } else { "" }
        );
        let positive = run(&s, &command);
        assert_eq!(
            positive.status.code(),
            Some(0),
            "canonical {id}: {:?}",
            payload(&positive)
        );
        let repeated = run(&s, &command);
        assert_eq!(repeated.status.code(), Some(0));
        assert_eq!(
            cases::stable(payload(&positive)),
            cases::stable(payload(&repeated))
        );
        if !parallel {
            assert!(s.0.join("canonical-marker").exists());
        }
        s.write("chain.toml", &manifest);
        let child = format!("fsl 5\n{}", cases::LEGACY_LEAF);
        s.write("dep.fsl", &child);
        if parallel {
            s.write("other-dep.fsl", &child);
        }
        let actual = run(&s, &command);
        let value = payload(&actual);
        let failed = if parallel {
            vec!["requirements", "design"]
        } else if refine {
            vec!["requirements"]
        } else {
            vec!["design"]
        };
        let mut layers = Vec::new();
        for layer in &failed {
            let child_path = if parallel && *layer == "requirements" {
                "other-dep.fsl"
            } else {
                "dep.fsl"
            };
            let detail = cases::refusal(
                &child,
                s.0.join(child_path)
                    .canonicalize()
                    .unwrap()
                    .to_str()
                    .unwrap(),
                serde_json::from_value(json!({"fsl":"1.0"})).unwrap(),
            );
            let mut entry = json!({"layer":layer,"kind":if depth||refine||parallel {"verify"} else {"check"},"file":if *layer=="requirements" {"./abstract.fsl"} else {"./root.fsl"},"status":"failed","result":"error","exit_code":2,"detail":detail});
            if depth || refine || parallel {
                entry["depth"] = json!(2);
            }
            layers.push(entry);
        }
        if !parallel {
            if refine {
                for (layer, kind) in [("design", "spec"), ("design->requirements", "refine")] {
                    layers.push(json!({"layer":layer,"kind":kind,"status":"skipped","result":"skipped","exit_code":0}));
                }
            }
            layers.push(json!({"layer":"impl","kind":"impl","status":"skipped","result":"skipped","exit_code":0}));
        }
        let expected = json!({"fsl":"1.0","result":"error","manifest":"chain.toml","keep_going":parallel,"layers":layers,"failed":failed,"kind":"chain","message":"one or more chain layers returned an error"});
        observations.compare(
            id,
            (actual.status.code(), &value, s.0.join("marker").exists()),
            (Some(2), &expected, false),
            actual.status.code() == Some(2) && value == expected && !s.0.join("marker").exists(),
        );
        if !parallel {
            observations.compare(
                "A09",
                s.0.join("marker").exists(),
                false,
                !s.0.join("marker").exists(),
            );
        }
    }
    observations.finish();
}

#[allow(clippy::too_many_lines, clippy::default_trait_access)] // Three solver kinds and the existing transitive PEM test-key convention.
fn dependency_edition_approval_writers_and_rerenders_use_strict_loaders() {
    use ed25519_dalek::pkcs8::{EncodePrivateKey, EncodePublicKey};
    let mut observations = Observations::default();
    for (signed, kind, generate, artifact) in [
        (
            false,
            "ledger",
            "ledger root.fsl --depth 2 -o ledger.md",
            "ledger.md",
        ),
        (
            true,
            "ledger",
            "ledger root.fsl --depth 2 -o ledger.md",
            "ledger.md",
        ),
        (
            false,
            "html",
            "html root.fsl --depth 2 -o review.html",
            "review.html",
        ),
        (
            false,
            "scenarios",
            "scenarios root.fsl --depth 2",
            "scenarios.json",
        ),
    ] {
        let s = common::Scratch::new();
        s.write("root.fsl","compose C { use S as s from \"dep.fsl\" @requirement(\"REQ-APPROVAL-001\",\"Keep\") invariant Reviewed { true } }\n");
        s.write(
            "dep.fsl",
            &cases::LEAF.replace(
                "invariant Typed",
                "@requirement(\"REQ-DEP-001\",\"日本語😀\") invariant Typed",
            ),
        );
        for args in [
            vec!["init", "-q"],
            vec!["add", "root.fsl", "dep.fsl"],
            vec!["commit", "-qm", "test baseline"],
        ] {
            git(&s, &args);
        }
        let key = ed25519_dalek::SigningKey::from_bytes(&[7; 32]);
        s.write(
            "test-private.pem",
            &key.to_pkcs8_pem(Default::default()).unwrap(),
        );
        s.write(
            "test-public.pem",
            &key.verifying_key()
                .to_public_key_pem(Default::default())
                .unwrap(),
        );
        let command = format!(
            "approval create root.fsl --kind {kind} --artifact {artifact} --approver dependency-test --depth 2 -o record.json {}",
            if signed {
                "--signing-key test-private.pem"
            } else {
                ""
            }
        );
        let generated = run(&s, generate);
        assert_eq!(generated.status.code(), Some(0));
        if kind == "scenarios" {
            s.write(artifact, &String::from_utf8(generated.stdout).unwrap());
        }
        let positive = run(&s, &command);
        assert_eq!(
            positive.status.code(),
            Some(0),
            "canonical {kind} approval: {:?}",
            payload(&positive)
        );
        assert!(s.0.join("record.json").exists());
        s.write(
            "valid-record.json",
            &std::fs::read_to_string(s.0.join("record.json")).unwrap(),
        );
        let trust = if signed {
            "--trust-key test-public.pem"
        } else {
            ""
        };
        let evaluation = format!("approval check root.fsl --record valid-record.json {trust}");
        let difference =
            format!("approval diff root.fsl --record valid-record.json --depth 2 {trust}");
        for command in [&evaluation, &difference] {
            let actual = run(&s, command);
            assert_eq!(
                actual.status.code(),
                Some(0),
                "canonical approval evaluation/diff: {:?}",
                payload(&actual)
            );
        }
        let overlay = if kind == "ledger" {
            Some(format!(
                "ledger root.fsl --depth 2 --approval valid-record.json -o overlay.md {trust}"
            ))
        } else {
            None
        };
        if let Some(command) = &overlay {
            let actual = run(&s, command);
            assert_eq!(
                actual.status.code(),
                Some(0),
                "canonical approval overlay: {:?}",
                payload(&actual)
            );
            assert!(s.0.join("overlay.md").exists());
            s.write("overlay.md", "overlay sentinel\n");
        }
        s.write("record.json", "sentinel\n");
        let child = format!("fsl 5\n{}", cases::LEGACY_LEAF);
        s.write("dep.fsl", &child);
        for args in [
            vec!["add", "dep.fsl"],
            vec!["commit", "-qm", "test dependency revision"],
        ] {
            git(&s, &args);
        }
        // These entry points have no outer owner before their direct digest
        // load: precise findings are not assumed, but fail-closed is required.
        let actual = run(&s, &command);
        let value = payload(&actual);
        let id = if signed { "A06" } else { "A05" };
        observations.compare(
            id,
            (
                actual.status.code(),
                &value,
                std::fs::read(s.0.join("record.json")).unwrap(),
            ),
            "nonzero and original record bytes",
            !actual.status.success()
                && std::fs::read(s.0.join("record.json")).unwrap() == b"sentinel\n",
        );
        for (id, command) in [("T18", command), ("T19", evaluation), ("T20", difference)] {
            let actual = run(&s, &command);
            let value = payload(&actual);
            observations.compare(
                id,
                (actual.status.code(), &value),
                "strict dependency rejection before records",
                actual.status.code() == Some(2)
                    && value["result"] == "error"
                    && value["kind"] != "io"
                    && value.get("record").is_none()
                    && std::fs::read(s.0.join("record.json")).unwrap() == b"sentinel\n",
            );
        }
        if let Some(command) = overlay {
            let actual = run(&s, &command);
            let value = payload(&actual);
            let bytes = std::fs::read(s.0.join("overlay.md")).unwrap();
            let guarded = actual.status.code() == Some(2)
                && value["result"] == "error"
                && bytes == b"overlay sentinel\n";
            for id in ["A03/approval-overlay", "T10/approval-overlay"] {
                observations.compare(
                    id,
                    (actual.status.code(), &value, &bytes),
                    "strict rejection before overlay write (direct digest may have no owner)",
                    guarded,
                );
            }
        }
    }
    observations.finish();
}

fn dependency_edition_deep_reads_and_governance_rereads() {
    let mut observations = Observations::default();
    for id in ["D01", "D02", "D03"] {
        let s = common::Scratch::new();
        s.write("deep.fsl", cases::LEAF);
        let source: String = match id {
            "D01" => {
                s.write("dep.fsl","compose S { use S as inner from \"deep.fsl\" state { done: Bool = false } action finish() { done = true } invariant Typed { true } }\n");
                s.write("deep.fsl", "spec S { state {} invariant Typed { true } }\n");
                cases::IMPLEMENTS.into()
            }
            "D02" => {
                let leaf = include_str!("../../../examples/causal/incident_system.fsl");
                s.write(
                    "incident_system.fsl",
                    &leaf.replace(
                        "spec IncidentSystem {",
                        "compose IncidentSystem { use S as inner from \"deep.fsl\"",
                    ),
                );
                include_str!("../../../examples/causal/incident_response.fsl").into()
            }
            _ => {
                let composed = "compose S { use S as inner from \"deep.fsl\" state { done: Bool = false } action finish() { done = true } invariant Typed { true } }\n";
                s.write("before.fsl", composed);
                s.write("after.fsl", &composed.replace("deep.fsl", "after-deep.fsl"));
                s.write("after-deep.fsl", cases::LEAF);
                s.write("mapping.fsl", "refinement M { impl S abs S maps auto }\n");
                cases::GOVERNANCE.into()
            }
        };
        s.write("root.fsl", &source);
        let command = if id == "D02" {
            "causal check root.fsl"
        } else {
            "check root.fsl"
        };
        let canonical = run(&s, command);
        assert_eq!(
            canonical.status.code(),
            Some(0),
            "{id} canonical deep chain: {:?}",
            payload(&canonical)
        );
        let rejected = if id == "D01" {
            "fsl 5\nspec S { state {} invariant Typed \"REQ-DEEP-001: deep\" { true } }\n".into()
        } else {
            format!("fsl 5\n{}", cases::LEGACY_LEAF)
        };
        s.write("deep.fsl", &rejected);
        reject(&mut observations, id, &s, command, "deep.fsl", &rejected);
        if id == "D03" {
            s.write("deep.fsl", cases::LEAF);
            assert_eq!(run(&s, command).status.code(), Some(0));
            s.write("after-deep.fsl", &rejected);
            reject(
                &mut observations,
                "D03/after",
                &s,
                command,
                "after-deep.fsl",
                &rejected,
            );
        }
    }
    let row = cases::flat_rows()
        .into_iter()
        .find(|r| r.id == "E04/W")
        .unwrap();
    let fixture = cases::fixture(&row);
    let s = common::Scratch::new();
    for (name, source) in &fixture.files {
        s.write(name, source);
    }
    s.write("root.fsl", &fixture.root);
    let expected = cases::expected_check(
        &row,
        &fixture,
        s.0.join(&fixture.child)
            .canonicalize()
            .unwrap()
            .to_str()
            .unwrap(),
        serde_json::from_value(json!({"fsl":"1.0"})).unwrap(),
    );
    let actual = run(&s, "check root.fsl");
    let value = payload(&actual);
    observations.compare(
        "W05",
        (actual.status.code(), &value),
        (Some(0), &expected),
        actual.status.code() == Some(0) && value == expected,
    );
    observations.finish();
}

fn dependency_edition_a16_cache_and_c04_materialized_source_controls() {
    let mut observations = Observations::default();
    let s = common::Scratch::new();
    let source = "compose C { use S as s from \"dep.fsl\" }\n";
    s.write("root.fsl", source);
    s.write("dep.fsl", cases::LEAF);
    let cached = |extra: &[&str]| {
        std::process::Command::new(env!("CARGO_BIN_EXE_fslc"))
            .args(["verify", "root.fsl", "--depth", "2"])
            .args(extra)
            .current_dir(&s.0)
            .env("FSLC_UNSTABLE", "fsl5")
            .env("FSLC_CACHE", "on")
            .env("FSLC_CACHE_DIR", s.0.join("cache"))
            .env("TMPDIR", common::root().join(".codex/tmp"))
            .output()
            .unwrap()
    };
    for _ in 0..2 {
        let canonical = cached(&[]);
        assert_eq!(canonical.status.code(), Some(0));
        assert_eq!(payload(&canonical)["result"], "verified");
    }
    let before = tree(&s.0.join("cache"));
    assert!(before.len() > 1, "cache positive must store a real entry");
    let rejected = format!("fsl 5\n{}", cases::LEGACY_LEAF);
    s.write("dep.fsl", &rejected);
    for args in [vec![], vec!["--engine", "induction", "--lemma", "true"]] {
        let actual = cached(&args);
        let value = payload(&actual);
        let after = tree(&s.0.join("cache"));
        let expected = cases::refusal(
            &rejected,
            s.0.join("dep.fsl")
                .canonicalize()
                .unwrap()
                .to_str()
                .unwrap(),
            serde_json::from_value(json!({"fsl":"1.0"})).unwrap(),
        );
        observations.compare(
            "A16",
            (actual.status.code(), &value, &after),
            (Some(2), &expected, &before),
            actual.status.code() == Some(2) && value == expected && after == before,
        );
    }
    s.write("root.md", &format!("# Test input\n\n```fsl\n{source}```\n"));
    reject(
        &mut observations,
        "C04",
        &s,
        "check root.md",
        "dep.fsl",
        &rejected,
    );
    s.write("unread-next.fsl", &rejected);
    for (label,target,body,detail) in [
        ("parse","dep.fsl",Some("spec S {"),"component \"dep.fsl\" failed to parse (expected kernel spec declaration at dep.fsl:1:9)".to_owned()),
        ("non-Spec","dep.fsl",Some(cases::IMPLEMENTS),"compose use must reference a spec".into()),
        ("nested-compose","dep.fsl",Some("compose Nested { use S as q from \"unread-next.fsl\" }\n"),"compose use must reference a spec".into()),
        ("cycle","root.fsl",None,"compose use must reference a spec".into()),
        ("missing","missing.fsl",None,format!("component \"missing.fsl\" could not be read ({})",std::io::Error::from_raw_os_error(2))),
    ] {
        s.write("root.fsl",&source.replace("dep.fsl",target));if let Some(body)=body {s.write(target,body);}
        // Direct error_at failures have no source origin; component_error
        // failures retain the parent's filename in their display message.
        let message=if matches!(label,"parse"|"missing") {format!("{detail} at root.fsl:1:13")} else {format!("{detail} at 1:13")};
        let expected=json!({"fsl":"1.0","result":"error","kind":"semantics","message":message,"loc":{"line":1,"column":13}});
        for repeat in 0..2 {let actual=run(&s,"check root.fsl");let value=payload(&actual);observations.compare(&format!("C02/{label}/{repeat}"),(actual.status.code(),&value,&actual.stderr),(Some(2),&expected,Vec::<u8>::new()),actual.status.code()==Some(2)&&value==expected&&actual.stderr.is_empty());}
    }
    s.write("root.fsl", cases::IMPLEMENTS);
    s.write("dep.fsl", cases::LEAF);
    let command = "verify root.fsl --depth 2 --no-cache --property Typed";
    let baseline = run(&s, command);
    let expected = cases::stable(payload(&baseline));
    assert_eq!(baseline.status.code(), Some(0));
    assert_eq!(expected.0["implements"]["result"], "not_evaluated");
    s.write("dep.fsl", &rejected);
    let actual = run(&s, command);
    let value = cases::stable(payload(&actual));
    observations.compare(
        "C02/suppression",
        (actual.status.code(), &value),
        (Some(0), &expected),
        actual.status.code() == Some(0) && value == expected,
    );
    observations.finish();
}

fn dependency_edition_observation_writers_and_three_read_dedup() {
    let mut observations = Observations::default();
    let repo = common::root();
    for single in [true, false] {
        let id = if single { "A07" } else { "A08" };
        let s = common::Scratch::new();
        let row = cases::flat_rows()
            .into_iter()
            .find(|r| r.id == "E09/P")
            .unwrap();
        let fixture = cases::fixture(&row);
        let root = if single {
            format!(
                "{} }}\n",
                &fixture.root[..fixture.root.find("  // Violated golden:").unwrap()]
            )
        } else {
            fixture.root
        };
        s.write("root.fsl", &root);
        s.write(&fixture.child, &fixture.files[&fixture.child]);
        std::fs::create_dir_all(s.0.join("output")).unwrap();
        let command = format!(
            "causal observe-expectations root.fsl --from-log {}/examples/causal/evidence/incident-observation-log.jsonl --mapping {}/examples/causal/evidence/incident-log-mapping.fsl --scope {}/examples/causal/evidence/incident-replay-scope.json --period-start 2026-01-01 --period-end 2026-03-31 --out output/evidence.json {}",
            repo.display(),
            repo.display(),
            repo.display(),
            if single {
                ""
            } else {
                "--lifecycle-out output/lifecycle.json"
            }
        );
        let canonical = preserve(
            &mut observations,
            id,
            &s,
            &command,
            0,
            "causal_expectations_observed",
        );
        assert_eq!(canonical["artifacts_generated"], if single { 1 } else { 2 });
        let files = tree(&s.0.join("output"));
        assert_eq!(
            files.len(),
            if single { 2 } else { 5 },
            "all evidence/lifecycle files must be produced"
        );
        for name in files.keys().filter(|name| !name.ends_with('/')) {
            s.write(&format!("output/{name}"), "sentinel\n");
        }
        let child = format!(
            "fsl 5\n{}",
            include_str!("../../../examples/causal/incident_system.fsl").replace(
                "invariant PrecisionBounded",
                "invariant PrecisionBounded \"REQ-DEP-001: 日本語😀\""
            )
        );
        s.write(&fixture.child, &child);
        let before = tree(&s.0.join("output"));
        reject(&mut observations, id, &s, &command, &fixture.child, &child);
        let after = tree(&s.0.join("output"));
        observations.compare(&format!("{id}/sentinels"), &after, &before, after == before);
        if !single {
            let current = child.replacen("fsl 5", "fsl 4", 1);
            s.write(&fixture.child, &current);
            let actual = run(&s, &command);
            let value = payload(&actual);
            let mut expected = canonical;
            expected["warnings"] = json!(cases::findings(
                &current,
                s.0.join(&fixture.child)
                    .canonicalize()
                    .unwrap()
                    .to_str()
                    .unwrap(),
                "current"
            ));
            observations.compare(
                "W06",
                (actual.status.code(), &value),
                (Some(0), &expected),
                actual.status.code() == Some(0) && value == expected,
            );
        }
    }
    observations.finish();
}

fn dependency_edition_plain_kernel_schema_controls() {
    let root = common::root();
    let path = "rust/fslc/tests/fixtures/kernel_contract.fsl";
    let mut observations = Observations::default();
    let expected_stderr = cases::findings(
        include_str!("fixtures/kernel_contract.fsl"),
        path,
        "current",
    )
    .iter()
    .fold(String::new(), |mut text, finding| {
        writeln!(&mut text, "warning: {finding}").unwrap();
        text
    })
    .into_bytes();
    for version in [1, 2] {
        let args = vec![
            "kernel".into(),
            path.into(),
            "--kernel-version".into(),
            version.to_string(),
        ];
        let first = common::invoke(env!("CARGO_BIN_EXE_fslc"), &args, &root, true);
        let second = common::invoke(env!("CARGO_BIN_EXE_fslc"), &args, &root, true);
        let actual = payload(&first);
        let expected = if version == 1 {
            serde_json::from_str(include_str!("fixtures/kernel_contract.v1.json")).unwrap()
        } else {
            payload(&second)
        };
        assert!(
            actual["$schema"]
                .as_str()
                .is_some_and(|s| s.contains("kernel"))
        );
        assert_eq!(actual["spec"]["name"], "KernelContract");
        observations.compare(
            &format!("A17/plain-v{version}"),
            (first.status.code(), &actual, &first.stderr),
            (Some(0), &expected, &expected_stderr),
            first.status.code() == Some(0)
                && second.status.code() == Some(0)
                && actual == expected
                && payload(&second) == expected
                && first.stderr == expected_stderr
                && second.stderr == expected_stderr,
        );
    }
    observations.finish();
}

fn dependency_edition_p03_document_recovery_reads_without_writing_artifacts() {
    let s = common::Scratch::new();
    s.write("root.fsl", "compose C { use S as s from \"dep.fsl\" }\n");
    s.write("dep.fsl", cases::LEAF);
    let command = "document generate root.fsl -o document.md";
    let canonical = run(&s, command);
    let value = payload(&canonical);
    let expected = json!({"fsl":"1.0","result":"error","kind":"document","message":"document projection does not support dialect 'compose' in RCIR v1","code":"FSL-DOC-DIALECT-UNSUPPORTED","dialect":"compose","supported_dialects":["requirements","spec"]});
    assert_eq!((canonical.status.code(), value), (Some(2), expected));
    assert!(!s.0.join("document.md").exists());
    let child = format!("fsl 5\n{}", cases::LEGACY_LEAF);
    s.write("dep.fsl", &child);
    let mut observations = Observations::default();
    reject(
        &mut observations,
        "P03/document",
        &s,
        command,
        "dep.fsl",
        &child,
    );
    assert!(!s.0.join("document.md").exists());
    observations.finish();
}

#[test]
fn dependency_edition_all_existing_api_cli_routes_execute_their_assigned_rows() {
    cases::exercise(
        "routes",
        &[
            dependency_edition_causal_routes_and_all_observation_outputs,
            dependency_edition_warnings_preserve_canonical_identity_and_distinct_findings,
            dependency_edition_artifact_and_raw_routes_refuse_before_effects,
            dependency_edition_repair_roles_report_but_do_not_reject_or_edit_children,
            dependency_edition_document_nonreaders_and_specialized_controls_are_preserved,
            dependency_edition_chain_steps_refuse_before_implementation_effects,
            dependency_edition_approval_writers_and_rerenders_use_strict_loaders,
            dependency_edition_deep_reads_and_governance_rereads,
            dependency_edition_a16_cache_and_c04_materialized_source_controls,
            dependency_edition_observation_writers_and_three_read_dedup,
            dependency_edition_plain_kernel_schema_controls,
            dependency_edition_p03_document_recovery_reads_without_writing_artifacts,
        ],
    );
}
