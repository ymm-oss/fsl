// SPDX-License-Identifier: Apache-2.0
//! Independent publication oracles: no removed Rust API and no inherited opt-in.
use std::path::Path;
use std::process::Command;

use serde_json::{Value, json};

const BODY: &str = "spec S { state { flag: Bool = false } action stay() { flag = false } invariant Safe { not flag } }\n";
const ALIAS: &str = "fsl 5\nrequirements R { glossary { Flag = 注文 } state { 注文: Bool = false } requirement REQ-1 \"test\" { action step() { 注文 = true } } }\n";
const OPERATOR: &str = "fsl 5\nspec S { state { flag: Bool = false }\n  @requirement(\"REQ-1\", \"日本語😀\") invariant I { true || false }\n}\n";
const WARNING: &str =
    "spec S { state { flag: Bool = false } invariant Safe \"REQ-1: 日本語😀\" { true } }\n";

fn versions() -> Value {
    json!({"verifier":{"name":"fslc-rust","version":env!("CARGO_PKG_VERSION")},
        "core":{"name":"fsl-core","version":env!("CARGO_PKG_VERSION")},
        "solver":{"name":"z3","backend":"native-z3","version":"Z3 4.16.0.0"}})
}

fn ok(header: Option<u8>, next: bool) -> Value {
    let mut v = json!({"fsl":"1.0","result":"ok","spec":"S","warnings":[],"versions":versions()});
    if let Some(edition) = header {
        v["fsl_edition"] = json!(edition);
    }
    if next {
        v["edition"] = json!("next");
    }
    v
}

fn refusal(code: &str, message: &str, line: u32, column: u32) -> Value {
    json!({"fsl":"1.0","result":"error","kind":"parse","message":message,
        "diagnostic_code":code,"loc":{"line":line,"column":column},"versions":versions()})
}

fn missing_header() -> Value {
    json!({"code":"missing_edition_header","kind":"missing_edition_header","taxonomy":"non_canonical",
        "severity":"info","edition":"next","message":"the document has no edition header; add 'fsl 5'",
        "loc":{"file":"plain.fsl","line":1,"column":1,"end_line":1,"end_column":1},
        "canonical_replacement":"fsl 5","machine_applicable":true,
        "edits":[{"start":0,"end":0,"replacement":"fsl 5\n"}],
        "suggestion":{"kind":"replace","replacement":"fsl 5\n","span":{"start":0,"end":0},"machine_applicable":true}})
}

fn migrated(written: bool, changed: bool) -> Value {
    let findings = if changed {
        vec![missing_header()]
    } else {
        vec![]
    };
    let edits = if changed {
        vec![json!({"start":0,"end":99,"replacement":format!("fsl 5\n{BODY}")})]
    } else {
        vec![]
    };
    json!({"fsl":"1.0","result":"migrated","edition":"next","changed":usize::from(changed),
        "written":written,"files":[{"path":"plain.fsl","changed":changed,"findings":findings,"edits":edits}]})
}

// Only these ten numeric timings varied in two d83 gate-on probes. Preserve
// their slots and finite/nonnegative type; compare ALL other keys/values.
// `memory_mb` is masked too: it was 17.46 on the d83 binaries and 17.68 in
// 5/5 runs of the #1434 head binary, so it follows the build, not the
// verdict (`edition_all_commands.rs` masks it for the same reason).
// No verdict, location, assurance, cost count or cache masks.
fn stable_verify(mut value: Value) -> Value {
    let mut paths = vec![
        "/cost/elapsed_s".to_owned(),
        "/cost/solver/check_elapsed_s".to_owned(),
        "/cost/solver/memory_mb".to_owned(),
    ];
    paths.extend((0..8).map(|i| format!("/cost/properties/{i}/elapsed_s")));
    for path in paths {
        let slot = value
            .pointer_mut(&path)
            .unwrap_or_else(|| panic!("missing ambient slot {path}"));
        let number = slot.as_f64().expect("numeric ambient timing");
        assert!(number.is_finite() && number >= 0.0, "{path}: {number}");
        *slot = json!(0);
    }
    value
}

fn verified() -> Value {
    let properties: Vec<_> = [
        ("action_coverage", "stay", 1),
        ("deadlock", "deadlock", 3),
        ("init", "initial_state", 1),
        ("invariant", "Safe", 3),
        ("partial_op", "Safe", 3),
        ("partial_op", "actions", 2),
        ("type_bound", "_bounds_flag", 3),
        ("vacuity", "session", 1),
    ]
    .into_iter()
    .map(|(kind, name, checks)| json!({"kind":kind,"name":name,"checks":checks,"elapsed_s":0}))
    .collect();
    json!({"fsl":"1.0","spec":"S","result":"verified","depth":2,"checked_to_depth":2,
        "completeness":"bounded","invariants_checked":["Safe"],"transitions_checked":[],"reachables":{},
        "action_coverage":{"stay":true},"deadlock":{"found":false},"warnings":[],
        "note":"bounded verification: no violation within depth 2","edition":"next","fsl_edition":5,
        "versions":versions(),"cost":{"elapsed_s":0,"solver":{"checks":17,"check_elapsed_s":0,
            "conflicts":2,"decisions":1,"propagations":4,"memory_mb":0},"properties":properties}})
}

fn packet(dir: &Path, argv: &[&str], gate: Option<&str>) -> (i32, Value, String) {
    let mut command = Command::new(env!("CARGO_BIN_EXE_fslc"));
    command
        .args(argv)
        .current_dir(dir)
        .env_remove("FSLC_UNSTABLE");
    // Packet-only determinism; cache cold/warm suite contracts remain intact.
    command.env("FSLC_CACHE", "off");
    if let Some(value) = gate {
        command.env("FSLC_UNSTABLE", value);
    }
    let output = command.output().expect("execute own working-tree CLI");
    let stdout = String::from_utf8(output.stdout).expect("UTF8 stdout");
    let stderr = String::from_utf8(output.stderr).expect("UTF8 stderr");
    let exit = output.status.code().expect("normal CLI exit");
    let value: Value = serde_json::from_str(&stdout).expect("JSON stdout");
    assert_eq!(
        stdout,
        format!("{}\n", serde_json::to_string_pretty(&value).unwrap()),
        "complete stdout serialization"
    );
    println!(
        "{}",
        json!({"argv":argv,"gate":gate,"cwd":dir,"exit":exit,"stdout":stdout,"stderr":stderr})
    );
    (exit, value, stderr)
}

fn compare(dir: &Path, argv: &[&str], expected: &Value, exit: i32, timing: bool) -> bool {
    let mut failures = Vec::new();
    let mut packets = Vec::new();
    // Known, absent, empty and unknown are inputs, never a test opt-in.
    for gate in [
        Some("fsl5"),
        None,
        Some(""),
        Some("later"),
        Some("later,fsl5"),
    ] {
        let (status, actual, stderr) = packet(dir, argv, gate);
        // Keep semantic RED visible without failing the timing presence guard
        // on a baseline parse error that has no cost block.
        let actual = if timing && actual["result"] == "verified" {
            stable_verify(actual)
        } else {
            actual
        };
        packets.push(serde_json::to_string(&actual).unwrap());
        if (status, &actual, stderr.as_str()) != (exit, expected, "") {
            failures.push(json!({"gate":gate,"status":status,"actual":actual,"expected":expected,"stderr":stderr}));
        }
    }
    println!("{argv:?}: mismatches={failures:#?}");
    failures.is_empty() && packets.windows(2).all(|pair| pair[0] == pair[1])
}

fn scratch(label: &str) -> std::path::PathBuf {
    let dir = std::env::temp_dir().join(format!("issue-1434-{}-{label}", std::process::id()));
    std::fs::create_dir_all(&dir).unwrap();
    dir
}

#[test]
fn publication_check_has_literal_edition_and_unicode_packets() {
    let dir = scratch("check");
    let mut agrees = true;
    let mut alias = ok(Some(5), true);
    alias["spec"] = json!("R");
    alias["warnings"] = json!([{"kind":"no_user_invariants","message":"spec declares no user invariants (only implicit type bounds are checked)"}]);
    for (file, source, expected) in [
        ("plain.fsl", BODY.to_owned(), ok(None, false)),
        ("four.fsl", format!("fsl 4\n{BODY}"), ok(Some(4), false)),
        ("five.fsl", format!("fsl 5\n{BODY}"), ok(Some(5), true)),
        ("alias.fsl", ALIAS.to_owned(), alias),
    ] {
        std::fs::write(dir.join(file), source).unwrap();
        agrees &= compare(&dir, &["check", file], &expected, 0, false);
    }
    for (edition, next) in [
        ("current", false),
        ("4", false),
        ("next", true),
        ("5", true),
    ] {
        agrees &= compare(
            &dir,
            &["check", "plain.fsl", "--edition", edition],
            &ok(None, next),
            0,
            false,
        );
    }
    assert!(
        agrees,
        "complete edition/Unicode packets differ; see raw cells"
    );
}

#[test]
fn publication_refusals_keep_complete_native_locations_and_exit() {
    let dir = scratch("refusals");
    let mut agrees = true;
    let mut operator = refusal(
        "unsupported_in_edition",
        "expression operator '||' is unsupported in this edition; use 'or' manually, then review fslc migrate <file> --edition next for remaining edits at 3:52",
        3,
        52,
    );
    operator["fsl_edition"] = json!(5);
    for (name, source, expected) in [
        ("operator.fsl", OPERATOR.to_owned(), operator),
        (
            "six.fsl",
            format!("fsl 6\n{BODY}"),
            refusal(
                "unsupported_edition",
                "unsupported syntax edition 'fsl 6'; supported editions: 4, 5 at 1:1",
                1,
                1,
            ),
        ),
        (
            "ai.fsl",
            "fsl 5\nai_component A {}\n".to_owned(),
            refusal(
                "edition_header_not_accepted",
                "a 'ai_component' document has no syntax edition; remove the 'fsl 5' header at 1:1",
                1,
                1,
            ),
        ),
    ] {
        std::fs::write(dir.join(name), source).unwrap();
        agrees &= compare(&dir, &["check", name], &expected, 2, false);
    }
    std::fs::write(dir.join("four.fsl"), format!("fsl 4\n{BODY}")).unwrap();
    agrees &= compare(
        &dir,
        &["check", "four.fsl", "--edition", "5"],
        &refusal(
            "edition_conflict",
            "the document declares 'fsl 4' but --edition 5 was requested at 1:1",
            1,
            1,
        ),
        2,
        false,
    );
    assert!(agrees, "complete refusal packets differ; see raw cells");
}

#[test]
fn publication_verify_is_bounded_not_promoted() {
    let dir = scratch("verify");
    std::fs::write(dir.join("five.fsl"), format!("fsl 5\n{BODY}")).unwrap();
    assert!(compare(
        &dir,
        &["verify", "five.fsl", "--depth", "2", "--no-cache"],
        &verified(),
        0,
        true,
    ));
}

#[test]
fn publication_numeric_migration_is_non_destructive_then_idempotent() {
    let dir = scratch("migrate");
    let mut agrees = true;
    for gate in [Some("fsl5"), None, Some("later")] {
        std::fs::write(dir.join("plain.fsl"), BODY).unwrap();
        for write in [false, true] {
            let mut argv = vec!["migrate", "plain.fsl", "--edition", "5"];
            if write {
                argv.push("--write");
            }
            let (status, actual, stderr) = packet(&dir, &argv, gate);
            agrees &= (status, actual, stderr) == (0, migrated(write, true), String::new());
            agrees &= std::fs::read_to_string(dir.join("plain.fsl")).unwrap()
                == if write {
                    format!("fsl 5\n{BODY}")
                } else {
                    BODY.to_owned()
                };
        }
        let (status, actual, stderr) = packet(
            &dir,
            &["migrate", "plain.fsl", "--edition", "5", "--write"],
            gate,
        );
        agrees &= (status, actual, stderr) == (0, migrated(true, false), String::new());
    }
    assert!(
        agrees,
        "numeric migration packets or file bytes differ; see raw cells"
    );
}

#[test]
fn publication_headerless_warning_occurs_once_with_exact_edits() {
    let dir = scratch("warning");
    std::fs::write(dir.join("warning.fsl"), WARNING).unwrap();
    let mut expected = ok(None, false);
    expected["warnings"] = json!([{"code":"legacy_string_metadata","kind":"legacy_string_metadata","taxonomy":"deprecated",
        "severity":"warning","edition":"current","message":"legacy string metadata is deprecated; use a typed annotation",
        "loc":{"file":"warning.fsl","line":1,"column":54,"end_line":1,"end_column":67},
        "canonical_replacement":"@requirement(\"REQ-1\", \"日本語😀\")","machine_applicable":true,
        "edits":[{"start":38,"end":38,"replacement":"@requirement(\"REQ-1\", \"日本語😀\")\n                                      "},{"start":53,"end":75,"replacement":""}]}]);
    assert!(compare(
        &dir,
        &["check", "warning.fsl"],
        &expected,
        0,
        false
    ));
}

#[test]
fn publication_missing_header_info_is_still_one_lint_finding_and_exit_one() {
    let dir = scratch("lint");
    std::fs::write(dir.join("plain.fsl"), BODY).unwrap();
    let expected = json!({"fsl":"1.0","result":"lint","edition":"next","finding_count":1,
        "id_policy":{"source":"builtin","patterns":{
            "requirement":["REQ-{scope}-{number:3}","NFR-{scope}-{number:3}","INV-{scope}-{number:3}"],
            "acceptance":["AC-{scope}-{number:3}"],"forbidden":["FB-{scope}-{number:3}"],
            "policy":["POL-{scope}-{number:3}"],"goal":["GOAL-{scope}-{number:3}"],
            "control":["CTRL-{scope}-{number:3}"],"model":["MODEL-{scope}-{number:3}"],
            "assumption":["ASSUME-{scope}-{number:3}"]}},
        "files":[{"path":"plain.fsl","findings":[missing_header()]}]});
    assert!(compare(
        &dir,
        &["lint", "plain.fsl", "--edition", "5"],
        &expected,
        1,
        false
    ));
}
