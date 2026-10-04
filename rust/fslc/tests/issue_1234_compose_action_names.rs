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

    for spelling in ["a__b.c", "a__b__c"] {
        let trace = v1_trace(
            "DoubleUnderscoreAlias",
            &json!({"a.b__n": 0}),
            vec![(json!(spelling), json!({}), json!({"a.b__n": 1}))],
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
