// SPDX-License-Identifier: Apache-2.0
// Copyright 2026 Ryoichi Izumita

//! Native contract for #1234: a compose component action has one public name,
//! `alias.action`, built from the `use` alias and the component's action name.
//!
//! testgen, conformance, and the Monitor already emitted `bank.settle`, while a
//! v1 replay trace only accepted the internal `bank__settle` and echoed it back
//! in `state_mismatch.action`, so a generated test's actions could not be
//! replayed. A v1 trace now accepts `alias.action`, still accepts the
//! pre-#1234 `alias__action` (looked up in the model's structural table, not
//! rewritten), and every output uses `alias.action`. A non-compose action keeps
//! exact Kernel-name matching.
//!
//! Detectors fail on `origin/main` (458040f3); the negative and non-compose
//! controls are preservation controls that pass before and after.

use std::path::{Path, PathBuf};
use std::process::{Command, Output};
use std::sync::atomic::{AtomicU64, Ordering};

use serde_json::{Value, json};

const BANK: &str = "specs/bank_system.fsl";

static NEXT_SCRATCH: AtomicU64 = AtomicU64::new(0);

fn root() -> PathBuf {
    Path::new(env!("CARGO_MANIFEST_DIR"))
        .parent()
        .and_then(Path::parent)
        .expect("workspace root")
        .to_owned()
}

fn scratch_dir(name: &str) -> PathBuf {
    let id = NEXT_SCRATCH.fetch_add(1, Ordering::Relaxed);
    let dir = root().join(format!(
        "rust/target/issue-1234-{name}-{}-{id}",
        std::process::id()
    ));
    if dir.exists() {
        std::fs::remove_dir_all(&dir).expect("clean stale scratch dir");
    }
    std::fs::create_dir_all(&dir).expect("create scratch dir");
    dir
}

fn fslc(arguments: &[&str]) -> Output {
    Command::new(env!("CARGO_BIN_EXE_fslc"))
        .args(arguments)
        .current_dir(root())
        .output()
        .expect("run native fslc")
}

fn json_of(output: &Output) -> Value {
    serde_json::from_slice(&output.stdout).unwrap_or_else(|error| {
        panic!(
            "invalid JSON: {error}; stdout={}; stderr={}",
            String::from_utf8_lossy(&output.stdout),
            String::from_utf8_lossy(&output.stderr)
        )
    })
}

fn replay(spec: &str, trace: &Value, name: &str) -> (Value, i32) {
    let path = scratch_dir(name).join("trace.json");
    std::fs::write(&path, serde_json::to_vec_pretty(trace).expect("trace JSON"))
        .expect("write trace");
    let output = fslc(&[
        "replay",
        spec,
        "--trace",
        path.to_str().expect("utf-8 path"),
    ]);
    (json_of(&output), output.status.code().expect("exit status"))
}

fn v1_trace(spec: &str, initial: &Value, events: Vec<(Value, Value, Value)>) -> Value {
    json!({
        "$schema": "https://fsl.dev/schemas/fslc/kernel/replay-trace.v1.schema.json",
        "schema_version": "1.2.0",
        "kernel_schema_version": "1.0.0",
        "spec": spec,
        "initial": initial,
        "events": events
            .into_iter()
            .enumerate()
            .map(|(index, (action, params, state))| json!({
                "tick": index + 1,
                "action": action,
                "params": params,
                "state": state,
            }))
            .collect::<Vec<_>>(),
    })
}

fn bank_state(cleared: i64, pending: i64, log: &[i64]) -> Value {
    json!({
        "bank.cleared": cleared,
        "bank.pending": pending,
        "audit.balance": log.iter().sum::<i64>(),
        "audit.log": log,
        "withdrawn": 0,
    })
}

/// `deposit_audited(1)` then `<settle>`, whose observed post-state is `settled`.
fn bank_settle_trace(settle: &str, settled: Value) -> Value {
    v1_trace(
        "BankSystem",
        &bank_state(0, 0, &[]),
        vec![
            (
                json!("deposit_audited"),
                json!({"a": 1}),
                bank_state(0, 1, &[1]),
            ),
            (json!(settle), json!({}), settled),
        ],
    )
}

fn bank_testgen_walk() -> Value {
    let kernel = fsl_core::parse_kernel_source(
        &std::fs::read_to_string(root().join(BANK)).expect("read bank_system"),
        &fsl_core::FsResolver::new(root().join("specs")),
    )
    .expect("lower bank_system");
    let model = fsl_core::build_model(kernel).expect("build bank_system");
    let fslc_rust::TestgenWalk::Clean(walk) =
        fslc_rust::testgen_trace_vectors(&model).expect("testgen walk")
    else {
        panic!("bank_system's testgen walk must not violate");
    };
    walk
}

fn walk_as_v1_trace(walk: &Value, rename: impl Fn(&str) -> String) -> Value {
    let steps = walk["steps"].as_array().expect("walk steps");
    v1_trace(
        walk["spec"].as_str().expect("walk spec"),
        &walk["initial"],
        steps
            .iter()
            .map(|step| {
                (
                    json!(rename(step["action"].as_str().expect("step action"))),
                    step["params"].clone(),
                    step["expected"].clone(),
                )
            })
            .collect(),
    )
}

/// Detector: on 458040f3 the first `bank.` step was `bad_call` / exit 1.
#[test]
fn testgen_walk_replays_conformant_as_a_v1_trace() {
    let walk = bank_testgen_walk();
    let component_steps = walk["steps"]
        .as_array()
        .expect("walk steps")
        .iter()
        .filter(|step| {
            step["action"]
                .as_str()
                .is_some_and(|name| name.starts_with("bank."))
        })
        .count();
    assert!(
        component_steps > 0,
        "the walk must exercise a compose component action: {walk:#}"
    );

    let (output, status) = replay(BANK, &walk_as_v1_trace(&walk, str::to_owned), "walk-dot");
    assert_eq!(status, 0, "{output:#}");
    assert_eq!(output["result"], "conformant", "{output:#}");
    assert_eq!(
        output["steps_checked"],
        walk["steps"].as_array().unwrap().len()
    );

    // Preservation: the same walk spelled with the pre-#1234 `bank__` form.
    let legacy = walk_as_v1_trace(&walk, |name| name.replacen("bank.", "bank__", 1));
    let (output, status) = replay(BANK, &legacy, "walk-legacy");
    assert_eq!(status, 0, "{output:#}");
    assert_eq!(output["result"], "conformant", "{output:#}");
}

/// Detector: on 458040f3 `state_mismatch.action` echoed the internal
/// `bank__settle`, and `bank.settle` was `bad_call`.
#[test]
fn state_mismatch_reports_the_canonical_name_for_either_spelling() {
    for spelling in ["bank__settle", "bank.settle"] {
        let (output, status) = replay(
            BANK,
            &bank_settle_trace(spelling, bank_state(3, 0, &[1])),
            "mismatch",
        );
        assert_eq!(status, 1, "{spelling}: {output:#}");
        assert_eq!(output["violation"]["kind"], "state_mismatch", "{output:#}");
        assert_eq!(output["violation"]["action"], "bank.settle", "{output:#}");
        assert_eq!(output["violation"]["tick"], 2, "{output:#}");

        let (output, status) = replay(
            BANK,
            &bank_settle_trace(spelling, bank_state(1, 0, &[1])),
            "settle",
        );
        assert_eq!(status, 0, "{spelling}: {output:#}");
        assert_eq!(output["result"], "conformant", "{output:#}");
    }
}

/// Preservation: rejected before and after #1234.
#[test]
fn unknown_unqualified_misaliased_and_internal_actions_are_rejected() {
    for spelling in [
        "bank.nosuch",
        "bank__nosuch",
        "settle",
        "audit.settle",
        "audit__settle",
        // `internal bank.submit_deposit` removes the action from the model.
        "bank.submit_deposit",
        "bank__submit_deposit",
    ] {
        let (output, status) = replay(
            BANK,
            &bank_settle_trace(spelling, bank_state(1, 0, &[1])),
            "negative",
        );
        assert_eq!(status, 1, "{spelling}: {output:#}");
        assert_eq!(output["result"], "nonconformant", "{output:#}");
        assert_eq!(output["violation"]["kind"], "bad_call", "{output:#}");
        assert_eq!(
            output["violation"]["message"],
            format!("unknown action '{spelling}'"),
            "{output:#}"
        );
    }
}

/// Preservation: a non-compose action keeps exact Kernel-name matching.
#[test]
fn non_compose_actions_keep_exact_matching() {
    let output = fslc(&[
        "replay",
        "rust/fslc/tests/fixtures/replay_trace.fsl",
        "--trace",
        "rust/fslc/tests/fixtures/replay_trace.valid.v1.json",
    ]);
    assert_eq!(output.status.code(), Some(0), "{:#}", json_of(&output));
    assert_eq!(json_of(&output)["result"], "conformant");

    let dir = scratch_dir("plain");
    let spec = dir.join("plain.fsl");
    std::fs::write(
        &spec,
        "spec PlainDunder {\n  state { n: 0..1 }\n  init { n = 0 }\n  action foo__bar() {\n    requires n < 1\n    n = n + 1\n  }\n}\n",
    )
    .expect("write spec");
    let spec = spec.to_str().expect("utf-8 path");
    let trace = |action: &str, n: i64| {
        v1_trace(
            "PlainDunder",
            &json!({"n": 0}),
            vec![(json!(action), json!({}), json!({"n": n}))],
        )
    };

    let (output, status) = replay(spec, &trace("foo__bar", 1), "plain-exact");
    assert_eq!(status, 0, "{output:#}");
    assert_eq!(output["result"], "conformant", "{output:#}");

    let (output, status) = replay(spec, &trace("foo__bar", 0), "plain-mismatch");
    assert_eq!(status, 1, "{output:#}");
    assert_eq!(output["violation"]["action"], "foo__bar", "{output:#}");

    let (output, status) = replay(spec, &trace("foo.bar", 1), "plain-dot");
    assert_eq!(status, 1, "{output:#}");
    assert_eq!(output["violation"]["kind"], "bad_call", "{output:#}");
}

fn write_alias_fixture(dir: &Path) {
    std::fs::write(
        dir.join("counter.fsl"),
        "spec Counter {\n  state { n: 0..2 }\n  init { n = 0 }\n  action c() {\n    requires n < 2\n    n = n + 1\n  }\n}\n",
    )
    .expect("write component");
    std::fs::write(
        dir.join("other.fsl"),
        "spec Other {\n  state { m: 0..1 }\n  init { m = 0 }\n  action b__c() {\n    requires m < 1\n    m = m + 1\n  }\n}\n",
    )
    .expect("write component");
    std::fs::write(
        dir.join("alias.fsl"),
        "compose DoubleUnderscoreAlias {\n  use Counter as a__b from \"counter.fsl\"\n}\n",
    )
    .expect("write compose");
    std::fs::write(
        dir.join("collide.fsl"),
        "compose AliasCollision {\n  use Counter as a__b from \"counter.fsl\"\n  use Other as a from \"other.fsl\"\n}\n",
    )
    .expect("write compose");
}

/// Detector: on 458040f3 alias `a__b` with action `c` was published as
/// `a.b__c`; the structural name (and the frozen Python reference) is `a__b.c`.
#[test]
fn double_underscore_alias_is_named_from_structure() {
    let dir = scratch_dir("alias");
    write_alias_fixture(&dir);
    let spec = dir.join("alias.fsl");
    let spec = spec.to_str().expect("utf-8 path");

    let output = fslc(&["conformance", spec, "--depth", "1"]);
    assert_eq!(output.status.code(), Some(0), "{:#}", json_of(&output));
    let conformance = json_of(&output);
    let names = conformance["vectors"]
        .as_array()
        .expect("vectors")
        .iter()
        .filter_map(|vector| vector["action"]["name"].as_str())
        .collect::<std::collections::BTreeSet<_>>();
    assert_eq!(names, ["a__b.c"].into_iter().collect());

    // State keys still use the string rule (`a.b__n`, unlike Python's
    // `a__b.n`); they are outside #1234, so the trace reuses the keys
    // conformance printed instead of pinning them.
    let initial = &conformance["states"][0]["state"];
    let vector = &conformance["vectors"][0];
    assert_eq!(vector["state"], "s0", "{conformance:#}");
    let stepped = &vector["outcome"]["state"];
    for spelling in ["a__b.c", "a__b__c"] {
        let trace = v1_trace(
            "DoubleUnderscoreAlias",
            initial,
            vec![(json!(spelling), json!({}), stepped.clone())],
        );
        let (output, status) = replay(spec, &trace, "alias-replay");
        assert_eq!(status, 0, "{spelling}: {output:#}");
        assert_eq!(output["result"], "conformant", "{output:#}");
    }
}

/// Preservation: the two structures that share `a__b__c` cannot coexist, so a
/// checked model never holds an ambiguous replay table.
#[test]
fn colliding_internal_names_are_rejected_before_replay() {
    let dir = scratch_dir("collide");
    write_alias_fixture(&dir);
    let spec = dir.join("collide.fsl");
    let output = fslc(&["check", spec.to_str().expect("utf-8 path")]);
    assert_eq!(output.status.code(), Some(2), "{:#}", json_of(&output));
    assert!(
        json_of(&output)["message"]
            .as_str()
            .is_some_and(|message| message.contains("duplicate action")),
        "{:#}",
        json_of(&output)
    );
}

/// Detector: on 458040f3 `verify` (`cost.properties[].name`), `conformance`
/// (`_requires_failed_bank__settle`), and `replay` (`state_mismatch.action`)
/// published internal `bank__` names.
#[test]
fn bank_system_command_outputs_never_publish_internal_names() {
    let dir = scratch_dir("leaks");
    let generated = dir.join("test_bank.py");
    let mismatch = dir.join("mismatch.json");
    std::fs::write(
        &mismatch,
        serde_json::to_vec(&bank_settle_trace("bank__settle", bank_state(3, 0, &[1])))
            .expect("trace JSON"),
    )
    .expect("write trace");
    // `check` and `testgen` print no action/state names on stdout (the
    // generated harness is checked below); the others must, or the scan is vacuous.
    let commands: Vec<(Vec<&str>, bool)> = vec![
        (vec!["check", BANK], false),
        (vec!["verify", BANK, "--depth", "4", "--no-cache"], true),
        (vec!["scenarios", BANK, "--depth", "4"], true),
        (vec!["conformance", BANK, "--depth", "2"], true),
        (
            vec![
                "testgen",
                BANK,
                "--depth",
                "4",
                "-o",
                generated.to_str().expect("utf-8 path"),
            ],
            false,
        ),
        (
            vec![
                "replay",
                BANK,
                "--trace",
                mismatch.to_str().expect("utf-8 path"),
            ],
            true,
        ),
    ];
    for (arguments, publishes_names) in commands {
        let output = fslc(&arguments);
        let stdout = String::from_utf8_lossy(&output.stdout);
        assert!(
            output.status.code().is_some_and(|code| code <= 1),
            "{arguments:?}: {stdout}"
        );
        if publishes_names {
            assert!(
                stdout.contains("\"bank."),
                "{arguments:?} must publish compose names: {stdout}"
            );
        }
        assert!(!stdout.contains("bank__"), "{arguments:?} leaked: {stdout}");
    }
    let generated = std::fs::read_to_string(generated).expect("read generated test");
    assert!(generated.contains("bank.settle"));
    assert!(!generated.contains("bank__"));
}

const COUNTER: &str = "spec Counter {\n  state { n: 0..3 }\n  init { n = 0 }\n  action go() {\n    requires n < 2\n    n = n + 1\n    ensures n >= 1\n  }\n  action reset() {\n    requires n == 2\n    n = 0\n  }\n  action stuck() {\n    requires n == 3\n    n = 0\n  }\n  invariant Inv { n <= 2 }\n}\n";
/// `div`'s guard divides by zero in the initial state.
const DIVIDER: &str = "spec Divider {\n  state { d: 0..1, q: 0..10 }\n  init {\n    d = 0\n    q = 0\n  }\n  action div() {\n    requires 10 / d > 0\n    q = 1\n  }\n  action bump() {\n    requires d < 1\n    d = d + 1\n  }\n}\n";
/// `split`'s effect divides by zero after `drop`.
const SPLITTER: &str = "spec Splitter {\n  state { d: 0..1, q: 0..10 }\n  init {\n    d = 1\n    q = 0\n  }\n  action split() {\n    q = 10 / d\n  }\n  action drop() {\n    requires d == 1\n    d = 0\n  }\n}\n";
/// `inc`'s `ensures` fails on the second step.
const ENSURES: &str = "spec Ens {\n  state { n: 0..3 }\n  init { n = 0 }\n  action inc() {\n    requires n < 3\n    n = n + 1\n    ensures n <= 1\n  }\n}\n";
/// `split` divides by zero only from the unreachable `d = 0`, so induction
/// reports a `_partial_` counterexample to induction; `reset`'s `requires` is
/// always true, a vacuity warning that names the action.
const SAFE_DIVIDER: &str = "spec SafeDiv {\n  state { d: 0..1, q: 0..10 }\n  init {\n    d = 1\n    q = 0\n  }\n  action split() {\n    q = 10 / d\n  }\n  action reset() {\n    requires q >= 0\n    q = 0\n  }\n}\n";
/// A requirement-tagged action with no `requires`, for the review exports.
const TAGGED: &str = "spec Tagged {\n  state { n: 0..20 }\n  init { n = 0 }\n  action bump() \"REQ-1: raises `n`\" {\n    n = n + 1\n  }\n}\n";

/// Every component action of the fixtures below; a component action may
/// appear only as `<alias>.<action>`.
const FIXTURE_ACTIONS: [&str; 8] = [
    "go", "reset", "stuck", "div", "bump", "split", "drop", "inc",
];

fn write_dunder_fixture(dir: &Path) {
    let compose = |name: &str, uses: &[(&str, &str, &str)]| {
        let uses = uses
            .iter()
            .map(|(spec, alias, file)| {
                ["  use ", spec, " as ", alias, " from \"", file, "\"\n"].concat()
            })
            .collect::<String>();
        format!("compose {name} {{\n{uses}}}\n")
    };
    for (file, source) in [
        ("counter.fsl", COUNTER.to_owned()),
        ("divider.fsl", DIVIDER.to_owned()),
        ("splitter.fsl", SPLITTER.to_owned()),
        ("tagged.fsl", TAGGED.to_owned()),
        ("ens.fsl", ENSURES.to_owned()),
        ("safediv.fsl", SAFE_DIVIDER.to_owned()),
        ("ensures.fsl", compose("Ensures", &[("Ens", "a__b", "ens.fsl")])),
        (
            "safe.fsl",
            compose("Safe", &[("SafeDiv", "a__b", "safediv.fsl")]),
        ),
        (
            "dunder.fsl",
            compose(
                "Dunder",
                &[
                    ("Counter", "a__b", "counter.fsl"),
                    ("Counter", "acct", "counter.fsl"),
                    ("Counter", "acct2", "counter.fsl"),
                ],
            ),
        ),
        (
            "partial.fsl",
            compose("Partial", &[("Divider", "a__b", "divider.fsl")]),
        ),
        (
            "calc.fsl",
            compose("Calc", &[("Divider", "calc", "divider.fsl")]),
        ),
        (
            "split.fsl",
            compose("Split", &[("Splitter", "a__b", "splitter.fsl")]),
        ),
        (
            "tagcomp.fsl",
            compose("TagComp", &[("Tagged", "a__b", "tagged.fsl")]),
        ),
        (
            "split-map.fsl",
            "refinement SplitSelf {\n  impl Split\n  abs Split\n  maps auto\n}\n".to_owned(),
        ),
        (
            "log-map.fsl",
            "refinement DunderLogMapping {\n  impl DunderProductionLog\n  abs Dunder\n  maps auto\n}\n"
                .to_owned(),
        ),
    ] {
        std::fs::write(dir.join(file), source).expect("write fixture");
    }
}

fn path_arg(dir: &Path, file: &str) -> String {
    dir.join(file).to_str().expect("utf-8 path").to_owned()
}

/// Wrong spellings of a fixture component action in `text`: the string-rule
/// `a.b__go`, the internal `a__b__go` / `acct__go` / `acct2__go`, and names
/// derived from them. State keys (`a.b__n`) and property names are not
/// action names and are not reported.
fn wrong_action_spellings(text: &str) -> Vec<String> {
    let mut found = Vec::new();
    for action in FIXTURE_ACTIONS {
        for alias in ["a__b", "acct", "acct2", "calc"] {
            for wrong in [
                format!("{alias}__{action}"),
                format!("{}__{action}", alias.replacen("__", ".", 1)),
            ] {
                let mut rest = text;
                while let Some(index) = rest.find(&wrong) {
                    let after = rest[index + wrong.len()..].chars().next();
                    if !after.is_some_and(|c| c.is_ascii_alphanumeric() || c == '_') {
                        found.push(wrong.clone());
                        break;
                    }
                    rest = &rest[index + wrong.len()..];
                }
            }
        }
    }
    found.sort();
    found.dedup();
    found
}

/// Detector (E2): the solver keys `cost.properties` and `action_coverage` by
/// internal name, where `acct2__Inv` sorts before `acct__Inv`; 5b5e2177
/// renamed them to `acct2.Inv`, `acct.Inv` in that order, which the Worker's
/// `parity.mjs` order assertion rejects. Every keyed list follows the
/// published name.
#[test]
fn keyed_outputs_are_ordered_by_published_name() {
    let dir = scratch_dir("order");
    write_dunder_fixture(&dir);
    let spec = path_arg(&dir, "dunder.fsl");
    let mut failures = Vec::new();
    for engine in [
        vec!["--depth", "3"],
        vec!["--engine", "induction"],
        vec!["--engine", "explicit", "--depth", "4"],
    ] {
        let mut arguments = vec!["verify", spec.as_str(), "--no-cache"];
        arguments.extend(engine);
        let output = fslc(&arguments);
        let verify = json_of(&output);
        if output.status.code() != Some(0) {
            failures.push(format!("{arguments:?}: exit {:?}", output.status.code()));
            continue;
        }
        if let Some(properties) = verify["cost"]["properties"].as_array() {
            let keys = properties
                .iter()
                .map(|property| {
                    (
                        property["kind"].as_str().unwrap_or_default().to_owned(),
                        property["name"].as_str().unwrap_or_default().to_owned(),
                    )
                })
                .collect::<Vec<_>>();
            let mut sorted = keys.clone();
            sorted.sort();
            if keys != sorted {
                failures.push(format!("{arguments:?}: cost.properties {keys:?}"));
            }
            let invariants = keys
                .iter()
                .filter(|(kind, name)| kind == "invariant" && name.starts_with("acct"))
                .map(|(_, name)| name.as_str())
                .collect::<Vec<_>>();
            // The explicit engine makes no solver checks, so it has no costs.
            if !keys.is_empty() && invariants != ["acct.Inv", "acct2.Inv"] {
                failures.push(format!("{arguments:?}: invariant costs {invariants:?}"));
            }
        }
        for field in ["action_coverage", "action_profile"] {
            let Some(object) = verify[field].as_object() else {
                continue;
            };
            let keys = object.keys().cloned().collect::<Vec<_>>();
            let mut sorted = keys.clone();
            sorted.sort();
            if keys != sorted || !keys.iter().any(|key| key == "acct2.go") {
                failures.push(format!("{arguments:?}: {field} {keys:?}"));
            }
        }
    }
    assert!(failures.is_empty(), "{failures:#?}");
}

/// Detector (E5): `compose_testgen_input` takes public state names, so a
/// compose scenario's expected state is ordered by declaration in every
/// target. a2629636 passed Kernel names (`bank__cleared`), which never matched
/// the scenarios' `bank.cleared` keys; pytest re-rendered them itself, while
/// the other five targets put the only matching key, `withdrawn`, first.
#[test]
fn compose_scenario_expected_state_follows_declaration_order() {
    const DECLARED: [&str; 5] = [
        "bank.cleared",
        "bank.pending",
        "audit.balance",
        "audit.log",
        "withdrawn",
    ];
    let dir = scratch_dir("testgen-order");
    let mut failures = Vec::new();
    for target in ["pytest", "vitest", "swift", "kotlin", "dart", "phpunit"] {
        let path = dir.join(format!("bank.{target}"));
        let output = fslc(&[
            "testgen",
            BANK,
            "--depth",
            "2",
            "--target",
            target,
            "-o",
            path.to_str().expect("utf-8 path"),
        ]);
        assert_eq!(output.status.code(), Some(0), "{target}: {output:?}");
        let text = std::fs::read_to_string(&path).expect("read generated scaffold");
        let lines = text
            .lines()
            .filter(|line| {
                (line.contains("assertPartial(") || line.contains("_assert_partial_expected("))
                    && line.contains("bank.cleared")
            })
            .collect::<Vec<_>>();
        if lines.is_empty() {
            failures.push(format!("{target}: no scenario expected-state assertion"));
        }
        for line in lines {
            let positions = DECLARED
                .iter()
                .map(|key| {
                    line.find(&format!("\"{key}\""))
                        .or_else(|| line.find(&format!("'{key}'")))
                })
                .collect::<Option<Vec<_>>>();
            if !positions.is_some_and(|positions| positions.is_sorted()) {
                failures.push(format!("{target}: {}", line.trim()));
            }
        }
    }
    assert!(failures.is_empty(), "{failures:#?}");
}

/// Detector (E3): a component guard that divides by zero is a `partial_op`
/// conformance outcome; 5b5e2177 renamed outcome names through a prefix
/// table that lacked `_partial_op_`, so it printed `_partial_op_calc__div`.
#[test]
fn guard_division_by_zero_outcome_is_named_from_the_action() {
    let dir = scratch_dir("guard-partial");
    write_dunder_fixture(&dir);
    let output = fslc(&["conformance", &path_arg(&dir, "calc.fsl"), "--depth", "2"]);
    assert_eq!(output.status.code(), Some(0), "{:#}", json_of(&output));
    let stdout = String::from_utf8_lossy(&output.stdout);
    assert!(stdout.contains("\"_partial_op_calc.div\""), "{stdout}");
    assert!(!stdout.contains("__"), "{stdout}");

    let output = fslc(&[
        "conformance",
        &path_arg(&dir, "partial.fsl"),
        "--depth",
        "2",
    ]);
    let stdout = String::from_utf8_lossy(&output.stdout);
    assert!(stdout.contains("\"_partial_op_a__b.div\""), "{stdout}");
    assert_eq!(wrong_action_spellings(&stdout), Vec::<String>::new());
}

/// One command run: arguments, expected exit, generated file to scan too, and
/// the names it must print.
type Probe<'a> = (Vec<&'a str>, i32, Option<&'a str>, Vec<&'a str>);

/// Detector (E4): with alias `a__b`, the string rule printed `a.b__go` and
/// some outputs printed the internal `a__b__go`; both differ from the
/// structural `a__b.go`. Each command is judged on its own (a failure does not
/// hide the next one): it must exit as expected, print the structural name
/// where it names a component action, and print no wrong spelling.
#[test]
#[allow(clippy::too_many_lines)]
fn every_command_names_double_underscore_alias_actions_structurally() {
    let dir = scratch_dir("commands");
    write_dunder_fixture(&dir);
    let dunder = path_arg(&dir, "dunder.fsl");
    let split = path_arg(&dir, "split.fsl");
    let partial = path_arg(&dir, "partial.fsl");
    let tagcomp = path_arg(&dir, "tagcomp.fsl");
    let ensures = path_arg(&dir, "ensures.fsl");
    let safe = path_arg(&dir, "safe.fsl");

    // Log and trace inputs reuse the state keys conformance prints.
    let conformance = json_of(&fslc(&["conformance", &dunder, "--depth", "1"]));
    let initial = conformance["states"][0]["state"].clone();
    let go = conformance["vectors"]
        .as_array()
        .expect("vectors")
        .iter()
        .find(|vector| vector["state"] == "s0" && vector["action"]["name"] == "a__b.go")
        .unwrap_or_else(|| panic!("no a__b.go vector: {conformance:#}"));
    let after_go = go["outcome"]["state"].clone();
    let counter_key = go["outcome"]["changes"]
        .as_object()
        .and_then(|changes| changes.keys().next())
        .expect("a__b.go changes one key")
        .clone();
    let mut overshoot = after_go.clone();
    overshoot[&counter_key] = json!(3);
    let log = |events: &[(&str, &Value)]| {
        events
            .iter()
            .map(|(action, state)| json!({"action": action, "params": {}, "state": state}))
            .map(|event| event.to_string() + "\n")
            .collect::<String>()
    };
    for (file, contents) in [
        (
            "mismatch.jsonl",
            log(&[("a__b.go", &after_go), ("a__b.go", &overshoot)]),
        ),
        ("requires.jsonl", log(&[("a__b.reset", &initial)])),
        (
            "requires.json",
            json!([{"action": "a__b.reset", "params": {}}]).to_string(),
        ),
        (
            "badcall.json",
            json!([{"action": "a__b__go", "params": {"x": 1}}]).to_string(),
        ),
    ] {
        std::fs::write(dir.join(file), contents).expect("write log");
    }
    let log_map = path_arg(&dir, "log-map.fsl");
    let split_map = path_arg(&dir, "split-map.fsl");
    let mismatch_log = path_arg(&dir, "mismatch.jsonl");
    let requires_log = path_arg(&dir, "requires.jsonl");
    let requires_trace = path_arg(&dir, "requires.json");
    let badcall_trace = path_arg(&dir, "badcall.json");
    let generated = path_arg(&dir, "test_dunder.py");
    let report = path_arg(&dir, "report.html");
    let ledger = path_arg(&dir, "ledger.md");

    let probes: Vec<Probe> = vec![
        (
            vec!["testgen", &dunder, "--depth", "4", "-o", &generated],
            0,
            Some(&generated),
            vec!["a__b.go"],
        ),
        (
            vec!["verify", &dunder, "--depth", "4", "--no-cache"],
            0,
            None,
            vec!["\"a__b.go\"", "action 'a__b.stuck' is never enabled"],
        ),
        (
            vec!["verify", &dunder, "--engine", "induction", "--no-cache"],
            0,
            None,
            vec!["\"a__b.go\""],
        ),
        (
            vec![
                "verify",
                &dunder,
                "--engine",
                "explicit",
                "--depth",
                "6",
                "--no-cache",
            ],
            0,
            None,
            vec!["\"a__b.go\""],
        ),
        (
            vec!["verify", &split, "--depth", "3", "--no-cache"],
            1,
            None,
            vec!["\"_partial_a__b.split\"", "\"a__b.drop\""],
        ),
        (
            vec!["verify", &split, "--engine", "induction", "--no-cache"],
            1,
            None,
            vec!["\"_partial_a__b.split\""],
        ),
        (
            vec![
                "verify",
                &split,
                "--engine",
                "explicit",
                "--depth",
                "3",
                "--no-cache",
            ],
            1,
            None,
            vec!["\"_partial_a__b.split\""],
        ),
        (
            vec!["verify", &ensures, "--depth", "3", "--no-cache"],
            1,
            None,
            vec!["\"invariant\": \"a__b.inc\""],
        ),
        (
            vec!["verify", &ensures, "--engine", "induction", "--no-cache"],
            1,
            None,
            vec!["\"invariant\": \"a__b.inc\""],
        ),
        (
            vec![
                "verify",
                &ensures,
                "--engine",
                "explicit",
                "--depth",
                "3",
                "--no-cache",
            ],
            1,
            None,
            vec!["\"invariant\": \"a__b.inc\""],
        ),
        (
            vec!["verify", &safe, "--depth", "3", "--no-cache"],
            0,
            None,
            vec!["action 'a__b.reset' has a requires clause that is always true"],
        ),
        (
            vec!["verify", &safe, "--engine", "induction", "--no-cache"],
            1,
            None,
            vec!["\"invariant\": \"_partial_a__b.split\""],
        ),
        (
            vec!["sweep", &dunder, "--depth", "2..3"],
            0,
            None,
            vec!["\"a__b.go\""],
        ),
        (
            vec!["scenarios", &dunder, "--depth", "4"],
            0,
            None,
            vec!["\"a__b.go\""],
        ),
        (
            vec!["conformance", &dunder, "--depth", "2"],
            0,
            None,
            vec!["\"a__b.go\""],
        ),
        (
            vec!["conformance", &partial, "--depth", "2"],
            0,
            None,
            vec!["\"_partial_op_a__b.div\"", "\"_requires_failed_a__b.bump\""],
        ),
        (
            vec!["conformance", &split, "--depth", "2"],
            0,
            None,
            vec!["\"_partial_a__b.split\""],
        ),
        (
            vec!["explain", &dunder, "--depth", "4"],
            0,
            None,
            vec!["\"a__b.go\""],
        ),
        (
            vec!["explain", &partial, "--depth", "3"],
            0,
            None,
            vec!["\"_partial_a__b.div\""],
        ),
        (
            vec!["explain", &split, "--depth", "3"],
            0,
            None,
            vec!["\"_partial_a__b.split\""],
        ),
        (
            vec!["mutate", &dunder, "--depth", "3"],
            0,
            None,
            vec!["\"a__b.go requires #1\"", "\"killed_by\": \"a__b.go\""],
        ),
        (
            vec!["mutate", &split, "--depth", "3"],
            1,
            None,
            vec!["\"_partial_a__b.split\""],
        ),
        (
            vec!["analyze", &dunder],
            0,
            None,
            vec!["\"action:a__b.go\"", "\"guard:a__b.go:0\""],
        ),
        (
            vec!["analyze", &tagcomp, "--profile", "ai-review"],
            0,
            None,
            vec!["\"action:a__b.bump\""],
        ),
        (
            vec!["analyze", &tagcomp, "--export", "tag-review"],
            0,
            None,
            vec!["a__b.bump"],
        ),
        (
            vec!["html", &dunder, "-o", &report],
            0,
            Some(&report),
            vec!["a__b.go"],
        ),
        (
            vec!["ledger", &dunder, "--depth", "3", "-o", &ledger],
            0,
            Some(&ledger),
            vec!["a__b.stuck"],
        ),
        (
            vec!["diff", &split, &split],
            1,
            None,
            vec!["\"_partial_a__b.split\""],
        ),
        (
            vec!["refine", &split, &split, &split_map, "--depth", "3"],
            1,
            None,
            vec!["\"_partial_a__b.split\""],
        ),
        (
            vec![
                "replay",
                &dunder,
                "--from-log",
                &mismatch_log,
                "--mapping",
                &log_map,
            ],
            1,
            None,
            vec!["\"action\": \"a__b.go\""],
        ),
        (
            vec![
                "replay",
                &dunder,
                "--from-log",
                &requires_log,
                "--mapping",
                &log_map,
            ],
            1,
            None,
            vec!["\"a__b.reset\""],
        ),
        (
            vec!["replay", &dunder, "--trace", &requires_trace],
            1,
            None,
            vec!["\"name\": \"_requires_failed_a__b.reset\""],
        ),
        (
            vec!["replay", &dunder, "--trace", &badcall_trace],
            1,
            None,
            vec!["\"action\": \"a__b.go\""],
        ),
    ];
    let mut failures = Vec::new();
    for (arguments, exit, file, required) in probes {
        let output = fslc(&arguments);
        let mut text = String::from_utf8_lossy(&output.stdout).into_owned();
        if let Some(file) = file {
            text.push_str(&std::fs::read_to_string(file).unwrap_or_default());
        }
        let label = arguments
            .join(" ")
            .replace(&format!("{}/", dir.display()), "");
        if output.status.code() != Some(exit) {
            failures.push(format!(
                "{label}: exit {:?}, expected {exit}: {}",
                output.status.code(),
                String::from_utf8_lossy(&output.stderr)
            ));
        }
        for name in required {
            if !text.contains(name) {
                failures.push(format!("{label}: missing {name}"));
            }
        }
        let wrong = wrong_action_spellings(&text);
        if !wrong.is_empty() {
            failures.push(format!("{label}: wrong spellings {wrong:?}"));
        }
    }
    assert!(failures.is_empty(), "{failures:#?}");
}
