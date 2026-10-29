// SPDX-License-Identifier: Apache-2.0

#[path = "support/edition_cases.rs"]
mod common;
#[path = "support/edition_dependency_cases.rs"]
mod diagnostic;

use common::{Results, Scratch, commit};
use serde_json::{Value, json};
use std::fmt::{Debug, Write as _};
use std::process::Output;

const COMPOSE: &str = "compose C { use S as s from \"dep.fsl\" @requirement(\"REQ-APPROVAL-001\",\"Keep\") invariant Reviewed { true } }\n";
const MAP: &str = "refinement M { impl S abs S map done = done action finish() -> finish() }\n";
const COLON_MAP: &str = "refinement M {\n impl S\n abs S\n map done = forall i in 0..1: done\n action finish() -> finish()\n}\n";

fn run(s: &Scratch, command: &str) -> Output {
    run_args(s, &common::split(command))
}

fn run_args(s: &Scratch, args: &[String]) -> Output {
    common::invoke(env!("CARGO_BIN_EXE_fslc"), args, &s.0, true)
}

fn payload(output: &Output) -> Value {
    diagnostic::native_value(common::value(output))
}

fn stderr(value: &Value) -> Vec<u8> {
    let Some(layers) = value.get("layers").and_then(Value::as_array) else {
        return vec![];
    };
    let mut table =
        "Layer  Check  Status  Result  Detail\n-----  -----  ------  ------  ------\n".to_owned();
    for layer in layers {
        let detail = layer
            .get("depth")
            .map_or_else(|| "-".into(), |depth| format!("depth={depth}"));
        writeln!(
            table,
            "{}  {}  {}  {}  {detail}",
            layer["layer"].as_str().unwrap(),
            layer["kind"].as_str().unwrap(),
            layer["status"].as_str().unwrap(),
            layer["result"].as_str().unwrap()
        )
        .unwrap();
    }
    table.into_bytes()
}

fn expect(r: &mut Results, label: &str, actual: &Output, expected: Value, exit: i32) {
    let expected_stderr = stderr(&expected);
    let actual_value = diagnostic::stable(payload(actual));
    let expected_value = diagnostic::stable(expected);
    r.check(
        label,
        (actual.status.code(), &actual_value, &actual.stderr),
        (Some(exit), &expected_value, &expected_stderr),
        actual.status.code() == Some(exit)
            && actual_value == expected_value
            && actual.stderr == expected_stderr,
    );
}

fn finish(r: Results) {
    // These new controls must fail even while the older population is deferred.
    let failures = r.failures;
    assert!(failures.is_empty(), "multi-root mismatches: {failures:?}");
}

fn equal<T: Debug + PartialEq + ?Sized>(r: &mut Results, label: &str, actual: &T, expected: &T) {
    r.check(label, actual, expected, actual == expected);
}

fn bytes(s: &Scratch, path: &str) -> Vec<u8> {
    std::fs::read(s.0.join(path)).unwrap()
}

fn canonical(r: &mut Results, s: &Scratch, command: &str, result: &str) -> Value {
    let output = run(s, command);
    let value = payload(&output);
    r.check(
        &format!("preserved/{command}"),
        (output.status.code(), &value, &output.stderr),
        (0, result, stderr(&value)),
        output.status.code() == Some(0)
            && value["result"] == result
            && output.stderr == stderr(&value),
    );
    value
}

fn warning(source: &str, path: &str) -> Value {
    diagnostic::metadata_finding(source, path, "current")
}

fn admitted(mut value: Value, header: &str, warnings: Vec<Value>) -> Value {
    if header == "fsl 5\n" {
        value["edition"] = json!("next");
        value["fsl_edition"] = json!(5);
    } else if header == "fsl 4\n" {
        value["fsl_edition"] = json!(4);
    }
    if !warnings.is_empty() {
        value = diagnostic::with_child_warnings(value, warnings);
    }
    value
}

fn relation(command: &str) -> Value {
    let args = common::split(command);
    if args[0] == "diff" {
        json!({"fsl":"1.0","result":"semantic_diff","old":{"file":args[1],"spec":"S"},"new":{"file":args[2],"spec":"S"},"bounded":{"depth":2,"completeness":"bounded"},"scope":{"old":{"instances":{},"values":{}},"new":{"instances":{},"values":{}},"comparison":"new","applied_to_old":{"instances":{},"values":{}}},"directions":{"new_to_old":{"result":"refines","checked_to_depth":2},"old_to_new":{"result":"refines","checked_to_depth":2}},"summary":["no_semantic_change"],"findings":[],"gate":{"forbidden":[],"violations":[],"passed":true}})
    } else {
        let mut value = json!({"fsl":"1.0","impl":"S","abs":"S","result":"refines","checked_to_depth":2,"action_map":{"finish":"finish"}});
        if args.len() == 8 {
            value["chain"] = json!(["S", "S", "S"]);
        }
        value
    }
}

fn canonical_relation(r: &mut Results, s: &Scratch, command: &str) -> Value {
    let expected = relation(command);
    canonical(r, s, command, expected["result"].as_str().unwrap());
    expected
}

fn write_files(s: &Scratch, paths: &[&str], source: &str) {
    for path in paths {
        s.write(path, source);
    }
}

#[test]
fn positional_metadata_warnings_and_intermediate_reuse() {
    let mut r = Results::default();
    for (command, slots) in [
        ("diff a.fsl b.fsl --depth 2", vec!["a.fsl", "b.fsl"]),
        (
            "diff a.fsl b.fsl --mapping map.fsl --depth 2",
            vec!["a.fsl", "b.fsl"],
        ),
        (
            "refine a.fsl b.fsl map.fsl --depth 2",
            vec!["a.fsl", "b.fsl"],
        ),
        (
            "refine a.fsl b.fsl map.fsl c.fsl map2.fsl --depth 2",
            vec!["a.fsl", "b.fsl", "c.fsl"],
        ),
        ("diff a.fsl a.fsl --depth 2", vec!["a.fsl", "a.fsl"]),
        (
            "refine a.fsl a.fsl map.fsl --depth 2",
            vec!["a.fsl", "a.fsl"],
        ),
    ] {
        let s = Scratch::new();
        write_files(
            &s,
            &["a.fsl", "b.fsl", "c.fsl"],
            &diagnostic::annotated(diagnostic::LEAF),
        );
        write_files(&s, &["map.fsl", "map2.fsl"], MAP);
        let baseline = canonical_relation(&mut r, &s, command);
        for header in ["", "fsl 4\n", "fsl 5\n"] {
            for legacy in [false, true] {
                // A next primary must be canonical; the secondary may be current
                // legacy. Repeated occurrences contribute independently.
                let mut findings = Vec::new();
                for name in ["a.fsl", "b.fsl", "c.fsl"] {
                    let head = if name == "a.fsl" { header } else { "" };
                    let body = if legacy && head != "fsl 5\n" {
                        diagnostic::LEGACY_LEAF.to_owned()
                    } else {
                        diagnostic::annotated(diagnostic::LEAF)
                    };
                    let source = format!("{head}{body}");
                    s.write(name, &source);
                    for _ in slots.iter().filter(|slot| **slot == name) {
                        if legacy && head != "fsl 5\n" {
                            findings.push(warning(&source, name));
                        }
                    }
                }
                let expected = admitted(baseline.clone(), header, findings);
                expect(
                    &mut r,
                    &format!("slots/{command}/{header:?}/{legacy}"),
                    &run(&s, command),
                    expected,
                    0,
                );
            }
        }
    }
    finish(r);
}

#[test]
fn secondary_mapping_warning_and_refusal_are_not_model_errors() {
    let mut r = Results::default();
    let s = Scratch::new();
    write_files(&s, &["a.fsl", "b.fsl"], diagnostic::LEAF);
    let mapping = COLON_MAP.to_owned();
    let repaired = mapping.replace(": done", " { done }");
    for command in [
        "diff a.fsl b.fsl --mapping map.fsl --depth 2",
        "refine a.fsl b.fsl map.fsl --depth 2",
    ] {
        s.write("map.fsl", &repaired);
        let baseline = canonical_relation(&mut r, &s, command);
        let rejected = format!("fsl 5\n{mapping}");
        for (label, source, expected, exit) in [
            (
                "warning",
                mapping.as_str(),
                admitted(
                    baseline,
                    "",
                    diagnostic::findings(&mapping, "map.fsl", "current"),
                ),
                0,
            ),
            (
                "refusal",
                rejected.as_str(),
                diagnostic::cli_refusal(&rejected, "map.fsl"),
                2,
            ),
        ] {
            s.write("map.fsl", source);
            expect(
                &mut r,
                &format!("mapping/{label}/{command}"),
                &run(&s, command),
                expected,
                exit,
            );
        }
    }
    finish(r);
}

fn batch_entry(file: &str, value: &Value) -> Value {
    let mut entry = json!({"file":file,"result":value["result"]});
    for key in [
        "spec",
        "refinement",
        "projection",
        "profile",
        "schema_version",
        "formal_status",
        "edition",
        "fsl_edition",
        "warnings",
        "kind",
        "message",
        "loc",
        "expected",
        "hint",
    ] {
        if let Some(value) = value.get(key) {
            entry[key] = value.clone();
        }
    }
    if value["result"] == "analyzed" {
        let mut summary = json!({});
        for key in [
            "nodes",
            "edges",
            "findings",
            "components",
            "cycles",
            "errors",
        ] {
            if let Some(items) = value.get(key).and_then(Value::as_array) {
                summary[key] = json!(items.len());
            }
        }
        entry["summary"] = summary;
    } else if let Some(findings) = value.get("findings") {
        entry["findings"] = findings.clone();
    }
    entry
}

fn batch(files: Vec<Value>, errors: Vec<Value>, projection: &str) -> Value {
    let failed = !errors.is_empty();
    let mut value = json!({"fsl":"1.0","result":if failed {"error"} else {"analyzed"},"analysis":"structure","mode":"batch","projection":projection,"profile":null,"files":Value::Array(files),"errors":Value::Array(errors)});
    if failed {
        value["kind"] = json!("batch");
        value["message"] = json!("one or more files failed structural analysis");
    }
    value
}

#[test]
#[allow(clippy::too_many_lines)] // Cross-file ownership and mixed-project acceptance matrix.
fn batch_scopes_are_per_file_and_project_is_not_a_primary_fsl_root() {
    let mut r = Results::default();
    let s = Scratch::new();
    s.write("dep.fsl", &diagnostic::annotated(diagnostic::LEAF));
    write_files(&s, &["a.fsl", "b.fsl"], COMPOSE);
    s.write("project.toml", "[design]\nfile = \"a.fsl\"\n");
    let a = canonical(&mut r, &s, "analyze a.fsl --projection tsg", "analyzed");
    let b = canonical(&mut r, &s, "analyze b.fsl --projection tsg", "analyzed");
    let project = canonical(
        &mut r,
        &s,
        "analyze project.toml --projection traceability_graph",
        "analyzed",
    );
    for mixed in [false, true] {
        let projection = if mixed { "traceability_graph" } else { "tsg" };
        let command = if mixed {
            "analyze b.fsl project.toml --projection traceability_graph"
        } else {
            "analyze b.fsl a.fsl a.fsl --projection tsg"
        };
        let b = if mixed {
            json!({"fsl":"1.0","result":"error","kind":"semantics","message":"unsupported projection: traceability_graph"})
        } else {
            b.clone()
        };
        let errors = if mixed {
            vec![
                json!({"file":"b.fsl","result":"error","kind":"semantics","message":"unsupported projection: traceability_graph","loc":null}),
            ]
        } else {
            vec![]
        };
        let ordered = if mixed {
            vec![("b.fsl", b), ("project.toml", project.clone())]
        } else {
            vec![("a.fsl", a.clone()), ("b.fsl", b)]
        };
        let entries = |warning: Option<&Value>| {
            ordered
                .iter()
                .map(|(file, value)| {
                    let mut entry = batch_entry(file, value);
                    if let Some(finding) = warning {
                        entry["warnings"] = json!([finding]);
                    }
                    entry
                })
                .collect()
        };
        expect(
            &mut r,
            &format!("preserved/batch/{mixed}"),
            &run(&s, command),
            batch(entries(None), errors.clone(), projection),
            if mixed { 2 } else { 0 },
        );
        s.write("dep.fsl", diagnostic::LEGACY_LEAF);
        let finding = warning(diagnostic::LEGACY_LEAF, "dep.fsl");
        expect(
            &mut r,
            &format!("batch/two-independent-warning-owners/{mixed}"),
            &run(&s, command),
            batch(entries(Some(&finding)), errors, projection),
            if mixed { 2 } else { 0 },
        );
        s.write("dep.fsl", &diagnostic::annotated(diagnostic::LEAF));
    }
    // A's rejected dependency cannot replace B or the batch envelope.
    s.write("b.fsl", diagnostic::LEAF);
    let b = canonical(&mut r, &s, "analyze b.fsl --projection tsg", "analyzed");
    let child = diagnostic::next_leaf();
    s.write("dep.fsl", &child);
    let refusal = diagnostic::cli_refusal(&child, "dep.fsl");
    let expected = batch(
        vec![batch_entry("a.fsl", &refusal), batch_entry("b.fsl", &b)],
        vec![
            json!({"file":"a.fsl","result":"error","kind":"unsupported_in_edition","message":null,"loc":null}),
        ],
        "tsg",
    );
    expect(
        &mut r,
        "batch/refused-child-does-not-poison-b",
        &run(&s, "analyze a.fsl b.fsl --projection tsg"),
        expected,
        2,
    );
    std::fs::create_dir(s.0.join("empty")).unwrap();
    s.write("empty/ignored.toml", "[design]\nfile = \"missing.fsl\"\n");
    expect(
        &mut r,
        "preserved/batch/empty-directory",
        &run(&s, "analyze empty --projection tsg"),
        batch(vec![], vec![], "tsg"),
        0,
    );
    finish(r);
}

#[test]
fn project_direct_roots_mapping_and_empty_project_have_warning_only_metadata() {
    let mut r = Results::default();
    let s = Scratch::new();
    s.write("project.toml", "[requirements]\nfile = \"a.fsl\"\n[design]\nfile = \"b.fsl\"\nrefine_against = \"requirements\"\nmapping = \"map.fsl\"\n");
    write_files(
        &s,
        &["a.fsl", "b.fsl"],
        &diagnostic::annotated(diagnostic::LEAF),
    );
    s.write("map.fsl", MAP);
    let command = "analyze project.toml --projection traceability_graph";
    let baseline = canonical(&mut r, &s, command, "analyzed");
    for header in ["", "fsl 4\n"] {
        let source = format!("{header}{}", diagnostic::LEGACY_LEAF);
        s.write("a.fsl", &source);
        s.write("b.fsl", &source);
        let findings = vec![warning(&source, "a.fsl"), warning(&source, "b.fsl")];
        let mut expected = baseline.clone();
        // Graph spans belong to these bytes, not the annotated canonical twin.
        for node in expected["nodes"].as_array_mut().unwrap() {
            let needle = match node["kind"].as_str() {
                Some("invariant") => "invariant Typed",
                Some("action") => "action finish",
                Some("effect") => "done = true",
                _ => continue,
            };
            let (line, column, _) = diagnostic::position(&source, source.find(needle).unwrap());
            node["loc"] = json!({"line":line + 1,"column":column + 1});
        }
        expect(
            &mut r,
            &format!("project/warnings/{header:?}"),
            &run(&s, command),
            admitted(expected, "", findings),
            0,
        );
    }
    write_files(
        &s,
        &["a.fsl", "b.fsl"],
        &diagnostic::annotated(diagnostic::LEAF),
    );
    s.write("map.fsl", &COLON_MAP.replace(": done", " { done }"));
    let mapped = canonical(&mut r, &s, command, "analyzed");
    s.write("map.fsl", COLON_MAP);
    expect(
        &mut r,
        "project/mapping-warning",
        &run(&s, command),
        admitted(
            mapped,
            "",
            diagnostic::findings(COLON_MAP, "map.fsl", "current"),
        ),
        0,
    );
    s.write("project.toml", "# no FSL roots\n");
    let empty = canonical(&mut r, &s, command, "analyzed");
    r.check(
        "preserved/project/empty-no-artificial-edition",
        &empty,
        "no edition metadata",
        empty.get("edition").is_none() && empty.get("fsl_edition").is_none(),
    );
    finish(r);
}

#[test]
fn later_io_precedes_earlier_model_or_parse_errors_outside_chain() {
    let mut r = Results::default();
    for broken in [
        "not valid FSL",
        "spec S { state {} invariant Bad { missing } }\n",
    ] {
        let s = Scratch::new();
        s.write("a.fsl", broken);
        s.write("b.fsl", diagnostic::LEAF);
        s.write("map.fsl", MAP);
        let io = std::io::Error::from_raw_os_error(2).to_string();
        for command in [
            "refine a.fsl b.fsl missing-map.fsl --depth 2",
            "refine a.fsl b.fsl map.fsl missing.fsl map.fsl --depth 2",
            "diff a.fsl b.fsl --mapping missing-map.fsl --depth 2",
        ] {
            let message = if command.contains(" missing.fsl ") {
                format!("missing.fsl: {io}")
            } else {
                io.clone()
            };
            expect(
                &mut r,
                &format!("priority/{command}/{broken:?}"),
                &run(&s, command),
                json!({"fsl":"1.0","result":"error","kind":"io","message":message}),
                2,
            );
        }
        s.write(
            "project.toml",
            "[business]\nfile = \"a.fsl\"\n[design]\nfile = \"missing.fsl\"\n",
        );
        // Project uses the established source loader's path-bearing error.
        let expected = json!({"fsl":"1.0","result":"error","kind":"io","message":format!("missing.fsl: {io}")});
        expect(
            &mut r,
            &format!("priority/project/{broken:?}"),
            &run(&s, "analyze project.toml --projection traceability_graph"),
            expected,
            2,
        );
        s.write("z.toml", "[design]\nfile = \"missing.fsl\"\n");
        expect(
            &mut r,
            &format!("priority/batch-project/{broken:?}"),
            &run(&s, "analyze a.fsl z.toml --projection traceability_graph"),
            json!({"fsl":"1.0","result":"error","kind":"io","message":format!("missing.fsl: {io}")}),
            2,
        );
    }
    finish(r);
}

#[test]
fn direct_diff_refine_and_multilink_own_complete_child_refusals() {
    let mut r = Results::default();
    for command in [
        "diff a.fsl b.fsl --depth 2",
        "refine a.fsl b.fsl map.fsl --depth 2",
        "refine a.fsl b.fsl map.fsl c.fsl map.fsl --depth 2",
    ] {
        let s = Scratch::new();
        write_files(&s, &["a.fsl", "b.fsl", "c.fsl"], COMPOSE);
        s.write("map.fsl", "refinement M { impl C abs C maps auto }\n");
        s.write("dep.fsl", diagnostic::LEAF);
        let baseline = canonical(
            &mut r,
            &s,
            command,
            relation(command)["result"].as_str().unwrap(),
        );
        s.write("dep.fsl", diagnostic::LEGACY_LEAF);
        expect(
            &mut r,
            &format!("child-warning/{command}"),
            &run(&s, command),
            admitted(
                baseline,
                "",
                vec![warning(diagnostic::LEGACY_LEAF, "dep.fsl")],
            ),
            0,
        );
        let child = diagnostic::next_leaf();
        s.write("dep.fsl", &child);
        expect(
            &mut r,
            &format!("child/{command}"),
            &run(&s, command),
            diagnostic::cli_refusal(&child, "dep.fsl"),
            2,
        );
    }
    finish(r);
}

#[cfg(unix)]
#[test]
fn direct_consumers_use_the_captured_stream_instead_of_reopening_it() {
    let mut r = Results::default();
    let s = Scratch::new();
    s.write("map.fsl", MAP);
    for (command, source) in [
        ("diff a.fsl b.fsl --depth 2", diagnostic::LEAF),
        ("refine a.fsl b.fsl map.fsl --depth 2", diagnostic::LEAF),
        (
            "diff a.fsl b.fsl --depth 2",
            "requirements S { number Amount const LO = 0 const HI = 2 state { amount: Amount } init { amount = 0 } }\nverify { values Amount = LO..HI }\n",
        ),
    ] {
        s.write("a.fsl", source);
        s.write("b.fsl", &source.replace("HI = 2", "HI = 5"));
        let mut expected = canonical_relation(&mut r, &s, command);
        let mode = if source == diagnostic::LEAF {
            ""
        } else {
            "scoped/"
        };
        if !mode.is_empty() {
            let old = json!({"instances":{},"values":{"Amount":[0,2]}});
            let new = json!({"instances":{},"values":{"Amount":[0,5]}});
            expected["scope"] =
                json!({"old":old,"new":new,"comparison":"new","applied_to_old":new});
            expected["findings"] =
                json!([{"kind":"scope_changed","old":old,"new":new,"comparison":"new"}]);
            expected["summary"] = json!(["scope_changed"]);
        }
        if command.starts_with("diff") {
            expected["old"]["file"] = json!("/dev/stdin");
        }
        let command = command.replace("a.fsl", "/dev/stdin");
        let actual = common::invoke_input(
            env!("CARGO_BIN_EXE_fslc"),
            &common::split(&command),
            &s.0,
            &format!("fsl 5\n{source}"),
        );
        expect(
            &mut r,
            &format!("capture/{mode}{command}"),
            &actual,
            admitted(expected, "fsl 5\n", vec![]),
            0,
        );
    }
    finish(r);
}

#[test]
fn failed_multilink_finishes_all_explicit_slots_only_once() {
    let mut r = Results::default();
    let s = Scratch::new();
    write_files(
        &s,
        &["a.fsl", "b.fsl", "c.fsl"],
        &diagnostic::annotated(diagnostic::LEAF),
    );
    s.write("map.fsl", MAP);
    s.write(
        "bad-map.fsl",
        "refinement M { impl S abs S map done = not done action finish() -> finish() }\n",
    );
    let command = "refine a.fsl b.fsl map.fsl c.fsl bad-map.fsl --depth 2";
    let first = run(&s, command);
    let value = payload(&first);
    r.check(
        "preserved/multi-link/real-failed-link",
        (first.status.code(), &value),
        "refinement_failed/1 with failed_link",
        first.status.code() == Some(1)
            && value["result"] == "refinement_failed"
            && value.get("failed_link").is_some(),
    );
    let mut findings = Vec::new();
    for name in ["a.fsl", "b.fsl", "c.fsl"] {
        s.write(name, diagnostic::LEGACY_LEAF);
        findings.push(warning(diagnostic::LEGACY_LEAF, name));
    }
    expect(
        &mut r,
        "multi-link/failed-link-warning-cardinality",
        &run(&s, command),
        admitted(value, "", findings),
        1,
    );
    finish(r);
}

#[test]
fn git_mapping_origin_distinguishes_live_from_materialized_inputs() {
    let mut r = Results::default();
    let s = Scratch::new();
    s.write("root.fsl", diagnostic::LEAF);
    let first = commit(&s, &["root.fsl"]);
    s.write("root.fsl", &format!("fsl 5\n{}", diagnostic::LEAF));
    let second = commit(&s, &["root.fsl"]);
    let mapping = COLON_MAP.to_owned();
    let rejected = format!("fsl 5\n{mapping}");
    for absolute in [false, true] {
        let path = s.write("live map.fsl", &rejected);
        let path = if absolute {
            path.to_str().unwrap().to_owned()
        } else {
            "live map.fsl".into()
        };
        let mut command =
            common::split(&format!("diff --git {first}..{second} root.fsl --mapping"));
        command.push(path.clone());
        command.extend(common::split("--depth 2"));
        expect(
            &mut r,
            &format!("git/live-mapping/{absolute}"),
            &run_args(&s, &command),
            diagnostic::cli_refusal(&rejected, &path),
            2,
        );
        s.write("live map.fsl", COLON_MAP);
        let mut expected = git_diff(&first, &second);
        expected = diagnostic::with_child_warnings(
            expected,
            diagnostic::findings(COLON_MAP, &path, "current"),
        );
        expect(
            &mut r,
            &format!("git/live-mapping-warning/{absolute}"),
            &run_args(&s, &command),
            expected,
            0,
        );
    }
    s.write("tracked-map.fsl", &rejected);
    let third = commit(&s, &["tracked-map.fsl"]);
    let command =
        format!("diff --git {first}..{third} root.fsl --mapping tracked-map.fsl --depth 2");
    let actual = run(&s, &command);
    // Historical mapping remains a raw input, not an extra admitted live slot.
    let expected = git_diff(&first, &third);
    expect(
        &mut r,
        "preserved/git/materialized-mapping-not-admitted",
        &actual,
        expected.clone(),
        0,
    );
    // Changing the live homonym must not change the materialized selection.
    s.write("tracked-map.fsl", "invalid live mapping");
    expect(
        &mut r,
        "preserved/git/materialized-mapping-wins-over-live",
        &run(&s, &command),
        expected,
        0,
    );
    finish(r);
}

fn git_diff(first: &str, second: &str) -> Value {
    let mut value = relation("diff root.fsl root.fsl --depth 2");
    value["old"]["file"] = json!(format!("{first}:root.fsl"));
    value["new"]["file"] = json!(format!("{second}:root.fsl"));
    value["vcs"] = json!({"kind":"git","range":format!("{first}..{second}"),"base":{"revision":first,"commit":first},"head":{"revision":second,"commit":second},"materialization":"git_archive_full_tree"});
    value
}

#[test]
fn inactive_project_target_never_prepares_its_mapping() {
    let mut r = Results::default();
    let s = Scratch::new();
    write_files(
        &s,
        &["a.fsl", "b.fsl"],
        &diagnostic::annotated(diagnostic::LEAF),
    );
    s.write("project.toml", "[design]\nfile = \"a.fsl\"\n");
    let command = "analyze project.toml --projection traceability_graph";
    let expected = canonical(&mut r, &s, command, "analyzed");
    s.write("ignored.fsl", "not valid FSL");
    s.write("project.toml", "[design]\nfile = \"a.fsl\"\nrefine_against = \"external\"\nmapping = \"unused.fsl\"\n[external]\nfile = \"ignored.fsl\"\n");
    let rejected = format!("fsl 5\n{COLON_MAP}");
    for (label, source) in [
        ("absent", None),
        ("legacy", Some(COLON_MAP)),
        ("next", Some(rejected.as_str())),
    ] {
        if let Some(source) = source {
            s.write("unused.fsl", source);
        }
        expect(
            &mut r,
            &format!("inactive-project/{label}"),
            &run(&s, command),
            expected.clone(),
            0,
        );
    }
    s.write("project.toml", "[requirements]\nfile = \"b.fsl\"\n[design]\nfile = \"a.fsl\"\nrefine_against = \"requirements\"\nmapping = \"unused.fsl\"\n");
    s.write("unused.fsl", &COLON_MAP.replace(": done", " { done }"));
    let active = canonical(&mut r, &s, command, "analyzed");
    s.write("unused.fsl", COLON_MAP);
    expect(
        &mut r,
        "active-project/warning",
        &run(&s, command),
        admitted(
            active,
            "",
            diagnostic::findings(COLON_MAP, "unused.fsl", "current"),
        ),
        0,
    );
    s.write("unused.fsl", &rejected);
    expect(
        &mut r,
        "active-project/refusal",
        &run(&s, command),
        diagnostic::cli_refusal(&rejected, "unused.fsl"),
        2,
    );
    finish(r);
}

fn git_batch(first: &str, second: &str, warnings: &[Value]) -> Value {
    let template = git_diff(first, second);
    let comparisons = ["a.fsl", "b.fsl"]
        .iter()
        .map(|file| {
            let mut value = template.clone();
            value["old"]["file"] = json!(format!("{first}:{file}"));
            value["new"]["file"] = json!(format!("{second}:{file}"));
            admitted(value, "", warnings.to_vec())
        })
        .collect::<Vec<_>>();
    json!({"fsl":"1.0","result":"semantic_diff_batch","vcs":template["vcs"],"specs":["a.fsl","b.fsl"],"comparisons":comparisons,"gate":{"violations":[],"passed":true}})
}

#[cfg(unix)]
#[test]
#[allow(clippy::too_many_lines)] // Complete batch, stream, warning and IO-precedence table.
fn git_batch_borrows_one_live_mapping_for_every_comparison() {
    let mut r = Results::default();
    let s = Scratch::new();
    write_files(&s, &["a.fsl", "b.fsl"], diagnostic::LEAF);
    let first = commit(&s, &["a.fsl", "b.fsl"]);
    write_files(&s, &["a.fsl", "b.fsl"], &format!("{}\n", diagnostic::LEAF));
    let second = commit(&s, &["a.fsl", "b.fsl"]);
    for (label, source) in [("canonical", MAP), ("warning", COLON_MAP)] {
        let path = s.write("live map.fsl", source);
        for stream in [false, true] {
            let mapping = if stream {
                "/dev/stdin"
            } else {
                path.to_str().unwrap()
            };
            let mut args =
                common::split(&format!("diff --git {first}..{second} --depth 2 --mapping"));
            args.push(mapping.into());
            let warnings = if label == "warning" {
                diagnostic::findings(source, mapping, "current")
            } else {
                vec![]
            };
            for single in [false, true] {
                let mut expected = git_batch(&first, &second, &warnings);
                let mut selected = args.clone();
                if single {
                    selected.insert(3, "a.fsl".into());
                    expected = expected["comparisons"][0].clone();
                }
                let actual = if stream {
                    common::invoke_input(env!("CARGO_BIN_EXE_fslc"), &selected, &s.0, source)
                } else {
                    run_args(&s, &selected)
                };
                expect(
                    &mut r,
                    &format!("git-batch/{label}/{stream}/{single}"),
                    &actual,
                    expected,
                    0,
                );
            }
        }
    }
    let missing = s.0.join("missing-map.fsl");
    let mut expected = git_batch(&first, &second, &[]);
    for value in expected["comparisons"].as_array_mut().unwrap() {
        *value = json!({"fsl":"1.0","result":"error","kind":"io","message":std::io::Error::from_raw_os_error(2).to_string(),"old":{"file":value["old"]["file"],"spec":null},"new":{"file":value["new"]["file"],"spec":null},"vcs":value["vcs"]});
    }
    expect(
        &mut r,
        "git-batch/mapping-io-envelope",
        &run_args(
            &s,
            &[
                "diff".into(),
                "--git".into(),
                format!("{first}..{second}"),
                "--mapping".into(),
                missing.to_string_lossy().into_owned(),
                "--depth".into(),
                "2".into(),
            ],
        ),
        expected,
        2,
    );
    for (label, range, spec, message) in [
        (
            "empty",
            format!("{first}..{first}"),
            None,
            "Git range contains no changed .fsl files",
        ),
        (
            "missing",
            format!("{first}..{second}"),
            Some("missing.fsl"),
            "'missing.fsl' must exist in both revisions for semantic diff",
        ),
    ] {
        let mut args = common::split(&format!(
            "diff --git {range} --mapping /dev/stdin --depth 2"
        ));
        if let Some(spec) = spec {
            args.insert(3, spec.into());
        }
        let actual = common::invoke_input(
            env!("CARGO_BIN_EXE_fslc"),
            &args,
            &s.0,
            &format!("fsl 5\n{COLON_MAP}"),
        );
        expect(
            &mut r,
            &format!("git-batch/precedence/{label}"),
            &actual,
            json!({"fsl":"1.0","result":"error","kind":"io","message":message}),
            2,
        );
    }
    finish(r);
}

#[cfg(unix)]
#[test]
fn chain_grouping_and_step_share_one_captured_root_stream() {
    let mut r = Results::default();
    let s = Scratch::new();
    let source = format!("fsl 5\n{}", diagnostic::LEAF);
    s.write("a.fsl", &source);
    for (kind, depth) in [("check", ""), ("verify", "depth = 2\n")] {
        let command = format!(
            "{kind} a.fsl {}",
            if kind == "verify" {
                "--depth 2 --no-cache"
            } else {
                ""
            }
        );
        let detail = canonical(
            &mut r,
            &s,
            &command,
            if kind == "verify" { "verified" } else { "ok" },
        );
        s.write(
            "stream.toml",
            &format!("[design]\nfile = \"/dev/stdin\"\n{depth}"),
        );
        let mut layer = json!({"layer":"design","kind":kind,"file":"/dev/stdin","status":"passed","result":detail["result"],"exit_code":0,"detail":detail});
        if kind == "verify" {
            layer["depth"] = json!(2);
        }
        let expected = json!({"fsl":"1.0","result":"verified","manifest":"stream.toml","keep_going":false,"layers":[layer]});
        for jobs in [1, 3] {
            let command = format!("chain stream.toml --jobs {jobs}");
            let output = common::invoke_input(
                env!("CARGO_BIN_EXE_fslc"),
                &common::split(&command),
                &s.0,
                &source,
            );
            expect(
                &mut r,
                &format!("chain/captured-grouping/{kind}/{jobs}"),
                &output,
                expected.clone(),
                0,
            );
        }
    }
    finish(r);
}

fn assert_chain_shape(value: &Value, files: [&str; 2], kind: &str, refine: bool) {
    let result = if kind == "verify" { "verified" } else { "ok" };
    let mut expected = Vec::new();
    for (layer, file) in [("requirements", files[0]), ("design", files[1])] {
        let mut step = json!({"layer":layer,"kind":kind,"file":file,"status":"passed","result":result,"exit_code":0,"detail":{"result":result}});
        if kind == "verify" {
            step["depth"] = json!(2);
        }
        expected.push(step);
    }
    if refine {
        expected.push(json!({"layer":"design->requirements","kind":"refine","file":"./b.fsl","against":"requirements","abs_file":"./a.fsl","mapping":"./map.fsl","depth":2,"status":"passed","result":"refines","exit_code":0,"detail":{"result":"refines"}}));
    }
    // This extra topology assertion projects details; full result comparisons below do not.
    let mut actual = value["layers"].as_array().unwrap().clone();
    for step in &mut actual {
        step["detail"] = json!({"result":step["detail"]["result"]});
    }
    assert_eq!(
        actual, expected,
        "independent chain wrappers and detail kinds"
    );
}

#[test]
fn chain_dependency_delivery_is_inside_each_actual_step_thread() {
    let mut r = Results::default();
    for jobs in [1, 3] {
        for depth in ["", "depth = 2\n"] {
            let s = Scratch::new();
            s.write("dep.fsl", &diagnostic::annotated(diagnostic::LEAF));
            s.write("a.fsl", COMPOSE);
            s.write("b.fsl", COMPOSE);
            s.write("map.fsl", "refinement M { impl C abs C maps auto }\n");
            s.write("chain.toml", &format!("[requirements]\nfile = \"a.fsl\"\n{depth}[design]\nfile = \"b.fsl\"\n{depth}refine_against = \"requirements\"\nmapping = \"map.fsl\"\nrefine_depth = 2\n"));
            let command = format!("chain chain.toml --jobs {jobs} --keep-going");
            let baseline = canonical(&mut r, &s, &command, "verified");
            assert_chain_shape(
                &baseline,
                ["./a.fsl", "./b.fsl"],
                if depth.is_empty() { "check" } else { "verify" },
                true,
            );
            s.write("dep.fsl", diagnostic::LEGACY_LEAF);
            let mut expected = baseline.clone();
            for layer in expected["layers"].as_array_mut().unwrap() {
                layer["detail"] = diagnostic::with_child_warnings(
                    layer["detail"].clone(),
                    vec![warning(diagnostic::LEGACY_LEAF, "dep.fsl")],
                );
            }
            expect(
                &mut r,
                &format!("chain/per-step-child-warning/{jobs}/{depth:?}"),
                &run(&s, &command),
                expected,
                0,
            );
        }
    }
    finish(r);
}

#[test]
fn chain_retains_ordinary_io_layer_projection_and_cutoff() {
    let mut r = Results::default();
    let s = Scratch::new();
    s.write("a.fsl", diagnostic::LEAF);
    s.write(
        "chain.toml",
        "[business]\nfile = \"missing.fsl\"\n[design]\nfile = \"a.fsl\"\n",
    );
    let expected = json!({"fsl":"1.0","result":"error","manifest":"chain.toml","keep_going":false,"kind":"chain","message":"one or more chain layers returned an error","failed":["business"],"layers":[{"layer":"business","kind":"check","file":"./missing.fsl","status":"failed","result":"error","exit_code":2,"detail":{"fsl":"1.0","result":"error","kind":"io","message":format!("./missing.fsl: {}", std::io::Error::from_raw_os_error(2))}},{"layer":"design","kind":"spec","status":"skipped","result":"skipped","exit_code":0}]});
    for jobs in [1, 3] {
        expect(
            &mut r,
            &format!("preserved/chain/io-cutoff/{jobs}"),
            &run(&s, &format!("chain chain.toml --jobs {jobs}")),
            expected.clone(),
            2,
        );
        let mut continued = expected.clone();
        continued["keep_going"] = json!(true);
        continued["layers"][1] = json!({"layer":"design","kind":"check","file":"./a.fsl","status":"passed","result":"ok","exit_code":0,"detail":{"fsl":"1.0","result":"ok","spec":"S","warnings":[]}});
        expect(
            &mut r,
            &format!("preserved/chain/io-keep-going/{jobs}"),
            &run(&s, &format!("chain chain.toml --jobs {jobs} --keep-going")),
            continued,
            2,
        );
    }
    finish(r);
}

#[test]
fn chain_duplicate_grouping_keeps_root_warnings_on_cold_and_warm_cache() {
    let mut r = Results::default();
    for jobs in [1, 3] {
        let s = Scratch::new();
        s.write("a.fsl", &diagnostic::annotated(diagnostic::LEAF));
        s.write(
            "cache.toml",
            "[requirements]\nfile = \"a.fsl\"\ndepth = 2\n[design]\nfile = \"a.fsl\"\ndepth = 2\n",
        );
        let command = format!("chain cache.toml --jobs {jobs}");
        let mut expected = canonical(&mut r, &s, &command, "verified");
        assert_chain_shape(&expected, ["./a.fsl", "./a.fsl"], "verify", false);
        let source = format!("fsl 4\n{}", diagnostic::LEGACY_LEAF);
        s.write("a.fsl", &source);
        for layer in expected["layers"].as_array_mut().unwrap() {
            layer["detail"] = admitted(
                layer["detail"].clone(),
                "fsl 4\n",
                vec![warning(&source, "./a.fsl")],
            );
        }
        for warm in [false, true] {
            let actual = std::process::Command::new(env!("CARGO_BIN_EXE_fslc"))
                .args(common::split(&command))
                .current_dir(&s.0)
                .env("FSLC_UNSTABLE", "fsl5")
                .env("FSLC_CACHE", "on")
                .env("FSLC_CACHE_DIR", s.0.join("cache"))
                .env("TMPDIR", common::root().join(".codex/tmp"))
                .output()
                .unwrap();
            let mut value = payload(&actual);
            for (index, layer) in value["layers"]
                .as_array_mut()
                .unwrap()
                .iter_mut()
                .enumerate()
            {
                let cache = layer["detail"].as_object_mut().unwrap().remove("cache");
                let valid = match &cache {
                    Some(cache) => {
                        cache["hit"] == true
                            && cache["source"] == "exact"
                            && cache["key"].as_str().is_some_and(|key| {
                                key.len() == 64 && key.bytes().all(|byte| byte.is_ascii_hexdigit())
                            })
                    }
                    None => !warm,
                };
                r.check(
                    &format!("preserved/chain/cache-membership/{jobs}/{warm}/{index}"),
                    &cache,
                    "cold optional exact hit; warm mandatory exact hit",
                    valid,
                );
            }
            let actual_stable = diagnostic::stable(value);
            let expected_stable = diagnostic::stable(expected.clone());
            r.check(
                &format!("preserved/chain/cache-full-result/{jobs}/{warm}"),
                (actual.status.code(), &actual_stable, &actual.stderr),
                (Some(0), &expected_stable, stderr(&expected)),
                actual.status.code() == Some(0)
                    && actual_stable == expected_stable
                    && actual.stderr == stderr(&expected),
            );
        }
    }
    finish(r);
}

fn sha(bytes: &[u8]) -> String {
    use sha2::Digest;
    format!("sha256:{:x}", sha2::Sha256::digest(bytes))
}

// The artifact wire contract hashes sorted JSON, preserving every finding.
fn sorted(value: Value) -> Value {
    match value {
        Value::Object(fields) => Value::Object(
            fields
                .into_iter()
                .map(|(key, value)| (key, sorted(value)))
                .collect::<std::collections::BTreeMap<_, _>>()
                .into_iter()
                .collect(),
        ),
        Value::Array(items) => Value::Array(items.into_iter().map(sorted).collect()),
        value => value,
    }
}

#[test]
#[allow(clippy::too_many_lines, clippy::default_trait_access)]
fn approval_distinct_artifacts_keep_standalone_diagnostics_signed_and_unsigned() {
    use ed25519_dalek::pkcs8::{EncodePrivateKey, EncodePublicKey};
    let mut r = Results::default();
    for kind in ["ledger", "html", "scenarios", "requirements_document"] {
        for signed in [false, true] {
            for legacy in [false, true] {
                let kind_label = format!("{kind}/{signed}/{legacy}");
                let s = Scratch::new();
                let root = if kind == "requirements_document" {
                    diagnostic::IMPLEMENTS.replace(
                        "invariant Typed",
                        "@requirement(\"REQ-ROOT-001\",\"Keep\") invariant Typed",
                    )
                } else {
                    COMPOSE.into()
                };
                s.write("root.fsl", &root);
                s.write(
                    "dep.fsl",
                    &if legacy {
                        diagnostic::LEGACY_LEAF.into()
                    } else {
                        diagnostic::annotated(diagnostic::LEAF)
                    },
                );
                // A1 oracle, not a behavior change: documents do not consume implements children.
                let findings = if legacy && kind != "requirements_document" {
                    vec![warning(diagnostic::LEGACY_LEAF, "dep.fsl")]
                } else {
                    vec![]
                };
                let revision = commit(&s, &["root.fsl", "dep.fsl"]);
                let key = ed25519_dalek::SigningKey::from_bytes(&[7; 32]);
                s.write(
                    "private.pem",
                    &key.to_pkcs8_pem(Default::default()).unwrap(),
                );
                s.write(
                    "public.pem",
                    &key.verifying_key()
                        .to_public_key_pem(Default::default())
                        .unwrap(),
                );
                let generate = match kind {
                    "requirements_document" => "document generate root.fsl -o artifact.out".into(),
                    "scenarios" => "scenarios root.fsl --depth 2 --deadlock ignore".into(),
                    _ => format!("{kind} root.fsl --depth 2 --deadlock ignore -o artifact.out"),
                };
                let generated = run(&s, &generate);
                r.check(
                    &format!("preserved/approval/{kind_label}/standalone"),
                    generated.status.code(),
                    0,
                    generated.status.code() == Some(0) && generated.stderr.is_empty(),
                );
                if kind == "scenarios" {
                    let value = payload(&generated);
                    r.check(
                        &format!("approval/scenarios/{signed}/{legacy}/standalone-warning"),
                        &value["warnings"],
                        "one complete child finding",
                        value["warnings"] == json!(findings),
                    );
                    s.write(
                        "artifact.out",
                        &String::from_utf8(generated.stdout).unwrap(),
                    );
                }
                let artifact = bytes(&s, "artifact.out");
                // Fresh standalone bytes are supplied without stripping warnings.
                // A nested renderer which loses diagnostics must fail this roundtrip.
                let command = format!(
                    "approval create root.fsl --kind {kind} --artifact artifact.out --approver edition-test {} -o record.json {}",
                    if kind == "requirements_document" {
                        ""
                    } else {
                        "--depth 2"
                    },
                    if signed {
                        "--signing-key private.pem"
                    } else {
                        ""
                    }
                );
                let created = run(&s, &command);
                r.check(
                    &format!("approval/{kind_label}/fresh-create"),
                    (created.status.code(), payload(&created)),
                    "created/0 and a record",
                    created.status.code() == Some(0) && s.0.join("record.json").is_file(),
                );
                if !s.0.join("record.json").is_file() {
                    continue; // The failed create is already a strict red cell.
                }
                let record: Value = serde_json::from_slice(&bytes(&s, "record.json")).unwrap();
                let expected_create = admitted(
                    json!({"fsl":"1.0","result":"created","kind":"approval_record","output":"record.json","record":record}),
                    "",
                    findings.clone(),
                );
                expect(
                    &mut r,
                    &format!("approval/{kind_label}/create-envelope"),
                    &created,
                    expected_create,
                    0,
                );
                let approved_at = record["approval"]["approved_at"].as_str().unwrap();
                assert!(
                    approved_at.len() == 20
                        && approved_at.ends_with('Z')
                        && approved_at.as_bytes()[10] == b'T'
                        && approved_at.bytes().filter(u8::is_ascii_digit).count() == 14,
                    "live record timestamp"
                );
                equal(
                    &mut r,
                    &format!("approval/{kind_label}/binding"),
                    &json!([
                        record["spec"]["git_commit"],
                        record["spec"]["path"],
                        record["target"]["kind"]
                    ]),
                    &json!([revision, "root.fsl", kind]),
                );
                if matches!(kind, "ledger" | "scenarios") {
                    let normalized = if kind == "scenarios" {
                        let value: Value = serde_json::from_slice(&artifact).unwrap();
                        assert!(
                            value.get("cost").is_none() && value.get("cache").is_none(),
                            "this scenario route has no execution metadata to exclude"
                        );
                        serde_json::to_vec(&sorted(value)).unwrap()
                    } else {
                        artifact.clone()
                    };
                    equal(
                        &mut r,
                        &format!("approval/{kind_label}/artifact-digest"),
                        &record["target"]["digest"],
                        &json!(sha(&normalized)),
                    );
                }
                let trust = if signed { "--trust-key public.pem" } else { "" };
                let mut expected_check = json!({"fsl":"1.0","result":"approval_check","status":"approved","reasons":[],"record":"record.json","target_kind":kind,"approver":"edition-test","approved_at":approved_at,"requirements":record["approval"]["requirements"],"baseline_digest":record["spec"]["digest"],"current_digest":record["spec"]["digest"],"artifact_digest":record["target"]["digest"],"current_artifact_digest":record["target"]["digest"],"diff_base":revision,"semantic_diff_command":"fslc approval diff root.fsl --record record.json","signature_status":if signed {"signed"} else {"unsigned"},"key_id":if signed { record["signature"]["key_id"].clone() } else {Value::Null}});
                if kind == "requirements_document" {
                    for (field, target) in [
                        ("claim_set_digest", "claim_set_digest"),
                        ("current_claim_set_digest", "claim_set_digest"),
                        ("reviewed_digest", "reviewed_digest"),
                        ("current_reviewed_digest", "reviewed_digest"),
                    ] {
                        expected_check[field] = record["target"][target].clone();
                    }
                }
                let expected_check = admitted(expected_check, "", findings);
                expect(
                    &mut r,
                    &format!("approval/{kind_label}/check-envelope"),
                    &run(
                        &s,
                        &format!("approval check root.fsl --record record.json {trust}"),
                    ),
                    expected_check,
                    0,
                );
                if matches!(kind, "html" | "ledger") {
                    let command = format!(
                        "ledger root.fsl --depth 2 --approval record.json -o overlay.md {trust}"
                    );
                    let first = run(&s, &command);
                    let first_artifact = bytes(&s, "overlay.md");
                    let second = run(&s, &command);
                    expect(
                        &mut r,
                        &format!("approval/{kind_label}/overlay-repeat"),
                        &second,
                        payload(&first),
                        0,
                    );
                    let overlay = std::fs::read_to_string(s.0.join("overlay.md")).unwrap();
                    equal(
                        &mut r,
                        &format!("approval/{kind_label}/overlay-full-artifact"),
                        overlay.as_bytes(),
                        first_artifact.as_slice(),
                    );
                    r.check(
                        &format!("approval/{kind_label}/overlay-status"),
                        &overlay,
                        "approved ledger and full dependency diagnostic",
                        overlay.contains("approved")
                            && overlay.contains("legacy_string_metadata") == legacy,
                    );
                }
                equal(
                    &mut r,
                    &format!("approval/{kind_label}/artifact-unchanged"),
                    &bytes(&s, "artifact.out"),
                    &artifact,
                );
                if kind == "scenarios" && !signed && !legacy {
                    let mut expected = relation("diff root.fsl root.fsl --depth 2");
                    for side in ["old", "new"] {
                        expected[side]["spec"] = json!("C");
                    }
                    expected["old"]["file"] = json!(format!("{revision}:root.fsl"));
                    expected["approval"] = json!({"record":"record.json","baseline_digest":record["spec"]["digest"],"baseline_commit":revision,"signature_status":"unsigned","key_id":null});
                    s.write("dep.fsl", diagnostic::LEGACY_LEAF);
                    expect(
                        &mut r,
                        "approval-diff/accepted-child-warning",
                        &run(&s, "approval diff root.fsl --record record.json --depth 2"),
                        admitted(
                            expected,
                            "",
                            vec![warning(diagnostic::LEGACY_LEAF, "dep.fsl")],
                        ),
                        0,
                    );
                }
            }
        }
    }
    finish(r);
}
