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
    let mut failures = Vec::new();
    for (arguments, publishes_names) in commands {
        let output = fslc(&arguments);
        let stdout = String::from_utf8_lossy(&output.stdout);
        if output.status.code().is_none_or(|code| code > 1) {
            failures.push(format!("{arguments:?}: exit {:?}", output.status.code()));
        }
        if publishes_names && !stdout.contains("\"bank.") {
            failures.push(format!("{arguments:?}: publishes no compose name"));
        }
        if stdout.contains("bank__") {
            failures.push(format!("{arguments:?}: leaked bank__"));
        }
    }
    let generated = std::fs::read_to_string(generated).unwrap_or_default();
    if !generated.contains("bank.settle") || generated.contains("bank__") {
        failures.push("testgen harness: missing bank.settle or leaked bank__".to_owned());
    }
    assert!(failures.is_empty(), "{failures:#?}");
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
/// One invariant, reachable, and leadsTo per alias, so `acct` and `acct2`
/// key every per-property list.
const RANKED_COMPONENT: &str = "spec Rc {\n  state { n: 0..2 }\n  init { n = 0 }\n  fair action go() {\n    requires n < 2\n    n = n + 1\n  }\n  fair action back() {\n    requires n == 2\n    n = 0\n  }\n  invariant Inv { n <= 2 }\n  reachable Top { n == 2 }\n  leadsTo Back { n == 2 ~> n == 0 }\n}\n";
/// Glue action `z__a` is published `z.a` (string rule), component `z.go` is
/// `z.go`: their internal names sort `z__a` after `z.go`'s key.
const RANKED: &str = "compose Ranked {\n  use Rc as acct from \"rc.fsl\"\n  use Rc as acct2 from \"rc.fsl\"\n  use Rc as z from \"rc.fsl\"\n  action z__a() {\n    requires z__n == 0\n    z__n = 1\n  }\n}\n";
/// Aliases `p__q`, `p`, `p2` publish `p.q__Back`, `p.Back`, `p2.Back`, whose
/// internal names `p__q__Back`, `p__Back`, `p2__Back` sort the other way.
const ORD: &str = "compose Ord {\n  use Rc as p__q from \"rc.fsl\"\n  use Rc as p from \"rc.fsl\"\n  use Rc as p2 from \"rc.fsl\"\n}\n";
/// Glue `z__a` and component `z.b` move the two counters in opposite
/// directions, a weighted-sum conservation candidate over both actions.
const PAIR: &str = "spec Pair {\n  state { x: Int, y: Int }\n  init {\n    x = 0\n    y = 0\n  }\n  action b() {\n    x = x - 1\n    y = y + 1\n  }\n}\n";
const CONSERVE: &str = "compose Conserve {\n  use Pair as z from \"pair.fsl\"\n  action z__a() {\n    z__x = z__x + 1\n    z__y = z__y - 1\n  }\n}\n";
/// `left` and `right` are always enabled and disagree on `Low`: a divergent
/// choice the `@undecided` on `left` acknowledges.
const CHOICE: &str = "spec Choice {\n  state { x: 0..2 }\n  init { x = 0 }\n  @undecided(\"which branch wins is still open\")\n  action left() {\n    x = 1\n  }\n  action right() {\n    x = 2\n  }\n  invariant Low { x <= 1 }\n}\n";
/// A requirement-tagged `up`/`down` cycle; with no fairness it is
/// progressless, with `fair` actions it is not.
const LOOP: &str = "spec Loop {\n  state { n: 0..2 }\n  init { n = 0 }\n  @requirement(\"REQ-LOOP\", \"the counter cycles\")\n  action up() {\n    requires n < 2\n    n = n + 1\n  }\n  action down() {\n    requires n == 2\n    n = 0\n  }\n}\n";

/// Every component action of the fixtures below; a component action may
/// appear only as `<alias>.<action>`.
const FIXTURE_ACTIONS: [&str; 15] = [
    "go", "reset", "stuck", "div", "bump", "split", "drop", "inc", "back", "left", "right", "up",
    "down", "prepare", "finish",
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
            "safe-map.fsl",
            "refinement SafeSelf {\n  impl Safe\n  abs Safe\n  maps auto\n}\n".to_owned(),
        ),
        (
            "log-map.fsl",
            "refinement DunderLogMapping {\n  impl DunderProductionLog\n  abs Dunder\n  maps auto\n}\n"
                .to_owned(),
        ),
        ("rc.fsl", RANKED_COMPONENT.to_owned()),
        ("ranked.fsl", RANKED.to_owned()),
        ("ord.fsl", ORD.to_owned()),
        (
            "ranked-map.fsl",
            "refinement RankedSelf {\n  impl Ranked\n  abs Ranked\n  maps auto\n  preserve progress {\n    respond acct2__Back by acct2__back\n    respond acct__Back by acct__back\n  }\n}\n"
                .to_owned(),
        ),
        ("pair.fsl", PAIR.to_owned()),
        ("conserve.fsl", CONSERVE.to_owned()),
        ("choice.fsl", CHOICE.to_owned()),
        ("loop.fsl", LOOP.to_owned()),
        (
            "fairloop.fsl",
            LOOP.replace("spec Loop", "spec FairLoop")
                .replace("  action ", "  fair action "),
        ),
        (
            "review.fsl",
            compose(
                "Review",
                &[
                    ("Choice", "a__b", "choice.fsl"),
                    ("Loop", "acct", "loop.fsl"),
                    ("FairLoop", "acct2", "fairloop.fsl"),
                ],
            ),
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
    let review = path_arg(&dir, "review.fsl");
    let ranked = path_arg(&dir, "ranked.fsl");
    let ranked_map = path_arg(&dir, "ranked-map.fsl");
    let safe_map = path_arg(&dir, "safe-map.fsl");
    let review_report = path_arg(&dir, "review.html");
    let review_ledger = path_arg(&dir, "review.md");

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
            vec!["check", &review, "--strict-tags"],
            0,
            None,
            vec!["\"name\": \"a__b.right\""],
        ),
        (
            vec!["analyze", &review, "--profile", "ai-review"],
            0,
            None,
            vec![
                "\"action:a__b.left\"",
                "\"declaration\": \"action a__b.left\"",
            ],
        ),
        (
            vec!["html", &review, "-o", &review_report],
            1,
            Some(&review_report),
            vec!["action a__b.left"],
        ),
        (
            vec!["ledger", &review, "--depth", "2", "-o", &review_ledger],
            1,
            Some(&review_ledger),
            vec!["a__b.left"],
        ),
        (
            vec!["refine", &ranked, &ranked, &ranked_map, "--depth", "3"],
            0,
            None,
            vec!["\"acct.back\""],
        ),
        (
            vec!["refine", &safe, &safe, &safe_map, "--depth", "3"],
            0,
            None,
            vec!["\"a__b.split\": \"a__b.split\""],
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

/// Two refinement projects with a compose layer, as the implementation
/// (`impl/`) or as the abstraction (`abs/`) of the mapping. In each, only the
/// action map anchors `REQ-1` in the lower layer.
fn write_project_fixtures(dir: &Path) {
    let chain = root().join("tests/fixtures/chain");
    let implementation = dir.join("impl");
    let abstraction = dir.join("abs");
    for project in [&implementation, &abstraction] {
        std::fs::create_dir_all(project).expect("create project dir");
    }
    for file in ["business.fsl", "requirements.fsl", "fsl-project.toml"] {
        std::fs::copy(chain.join(file), implementation.join(file)).expect("copy chain fixture");
    }
    std::fs::copy(
        chain.join("design.fsl"),
        implementation.join("component.fsl"),
    )
    .expect("copy chain design");
    for (project, file, source) in [
        (
            &implementation,
            "design.fsl",
            "compose ChainCompose {\n  use ChainDesign as a__b from \"component.fsl\"\n}\n",
        ),
        (
            &implementation,
            "design_refines_requirements.fsl",
            "refinement ChainComposeRefinesRequirements {\n  impl ChainCompose\n  abs ChainRequirements\n\n  map req[i: Item] = if a__b__design[i] == DDone then RDone else RDraft\n\n  action a__b__prepare(i) -> stutter\n  action a__b__finish(i) -> finish(i)\n}\n",
        ),
        (&abstraction, "tagged.fsl", TAGGED),
        (
            &abstraction,
            "requirements.fsl",
            "compose TagComp {\n  use Tagged as a__b from \"tagged.fsl\"\n}\n",
        ),
        (
            &abstraction,
            "design.fsl",
            "spec Impl {\n  state { m: 0..20 }\n  init { m = 0 }\n  action step() {\n    m = m + 1\n  }\n}\n",
        ),
        (
            &abstraction,
            "map.fsl",
            "refinement ImplRefinesTagComp {\n  impl Impl\n  abs TagComp\n\n  map a__b__n = m\n\n  action step() -> a__b__bump()\n}\n",
        ),
        (
            &abstraction,
            "fsl-project.toml",
            "[requirements]\nfile = \"requirements.fsl\"\n\n[design]\nfile = \"design.fsl\"\ndepth = 2\nrefine_against = \"requirements\"\nmapping = \"map.fsl\"\n",
        ),
    ] {
        std::fs::write(project.join(file), source).expect("write project fixture");
    }
}

/// Edges of an analyze graph with an endpoint that is not a node ID.
fn dangling_edges(graph: &Value) -> Vec<String> {
    let ids = graph["nodes"]
        .as_array()
        .into_iter()
        .flatten()
        .filter_map(|node| node["id"].as_str())
        .collect::<std::collections::BTreeSet<_>>();
    graph["edges"]
        .as_array()
        .into_iter()
        .flatten()
        .filter(|edge| {
            [&edge["from"], &edge["to"]]
                .iter()
                .any(|end| !end.as_str().is_some_and(|id| ids.contains(id)))
        })
        .map(|edge| format!("{} -{}-> {}", edge["from"], edge["kind"], edge["to"]))
        .collect()
}

/// Detector (r2 F1): the project traceability graph built its action-map
/// edges from the Kernel name (`design:action:a__b__finish`) while the layer
/// graph names the node `design:action:a__b.finish`. On aca90678 the
/// `maps_action` and `lower_anchor` edges pointed at no node, and with a
/// compose abstraction `REQ-1` lost its lower anchor and was reported as a
/// `traceability_gap`; 458040f3 printed the internal `a__b__finish`.
#[test]
fn project_traceability_graph_edges_end_at_published_action_nodes() {
    let dir = scratch_dir("project");
    write_project_fixtures(&dir);
    let mut failures = Vec::new();
    for (project, action_node) in [
        ("impl", "design:action:a__b.finish"),
        ("abs", "requirements:action:a__b.bump"),
    ] {
        let manifest = path_arg(&dir.join(project), "fsl-project.toml");
        let output = fslc(&["analyze", &manifest, "--projection", "traceability_graph"]);
        if output.status.code() != Some(0) {
            failures.push(format!("{project}: exit {:?}", output.status.code()));
            continue;
        }
        let graph = json_of(&output);
        let dangling = dangling_edges(&graph);
        if !dangling.is_empty() {
            failures.push(format!("{project}: dangling edges {dangling:?}"));
        }
        let edges = graph["edges"].as_array().expect("edges");
        if !edges.iter().any(|edge| {
            edge["kind"] == "maps_action"
                && (edge["from"] == action_node || edge["to"] == action_node)
        }) {
            failures.push(format!("{project}: no maps_action edge at {action_node}"));
        }
        if !edges.iter().any(|edge| {
            edge["kind"] == "lower_anchor" && edge["from"] == "requirements:requirement:REQ-1"
        }) {
            failures.push(format!("{project}: REQ-1 has no lower anchor"));
        }
        let gaps = graph["findings"]
            .as_array()
            .into_iter()
            .flatten()
            .filter(|finding| finding["finding_type"] == "traceability_gap")
            .count();
        if gaps != 0 {
            failures.push(format!("{project}: {gaps} traceability gaps"));
        }
        let wrong = wrong_action_spellings(&String::from_utf8_lossy(&output.stdout));
        if !wrong.is_empty() {
            failures.push(format!("{project}: wrong spellings {wrong:?}"));
        }
    }
    assert!(failures.is_empty(), "{failures:#?}");
}

/// Guard, not a detector: every single-spec projection of the compose design
/// layer and the analyze graph of its refinement end every edge at a declared
/// node. These graphs already built action IDs with `action_node_id`, so they
/// do not reach the r2 F1 defect (the project `traceability_graph` of
/// `project_traceability_output`); with F1 reverted this test still passes,
/// and `project_traceability_graph_edges_end_at_published_action_nodes` is
/// the test that fails.
#[test]
fn compose_layer_projections_end_every_edge_at_a_node() {
    let dir = scratch_dir("projections");
    write_project_fixtures(&dir);
    let mut failures = Vec::new();
    let design = path_arg(&dir.join("impl"), "design.fsl");
    let mapping = path_arg(&dir.join("impl"), "design_refines_requirements.fsl");
    for (arguments, has_edges) in [
        (vec!["analyze", design.as_str()], true),
        (
            vec!["analyze", &design, "--projection", "action_state_graph"],
            true,
        ),
        (
            vec![
                "analyze",
                &design,
                "--projection",
                "action_dependency_graph",
            ],
            true,
        ),
        (
            vec![
                "analyze",
                &design,
                "--projection",
                "requirement_property_graph",
            ],
            false,
        ),
        (
            vec!["analyze", &design, "--projection", "property_state_graph"],
            false,
        ),
        (
            vec![
                "analyze",
                &design,
                "--projection",
                "impact_graph",
                "--focus",
                "action:a__b.finish",
            ],
            true,
        ),
        (vec!["analyze", &mapping], true),
    ] {
        let output = fslc(&arguments);
        let label = arguments[2..].join(" ");
        if output.status.code() != Some(0) {
            failures.push(format!("{label}: exit {:?}", output.status.code()));
            continue;
        }
        let graph = json_of(&output);
        let dangling = dangling_edges(&graph);
        if !dangling.is_empty() {
            failures.push(format!("{label}: dangling edges {dangling:?}"));
        }
        if has_edges && graph["edges"].as_array().is_none_or(Vec::is_empty) {
            failures.push(format!("{label}: no edges"));
        }
    }
    assert!(failures.is_empty(), "{failures:#?}");
}

/// Detector (r3 minor 1): scenarios print `respond_*` and the leadsTo
/// warnings in the order of the property name they print. Aliases `p__q`,
/// `p`, `p2` publish `p.q__Back`, `p.Back`, `p2.Back`, whose internal names
/// `p__q__Back`, `p__Back`, `p2__Back` sort the other way; on 888921a1 both
/// lists came out `p2`, `p`, `p.q__` while `reach_*` was already ascending.
#[test]
fn leadsto_scenarios_and_warnings_are_ordered_by_the_name_they_print() {
    let dir = scratch_dir("respond-order");
    write_dunder_fixture(&dir);
    let ord = path_arg(&dir, "ord.fsl");
    let names = |scenarios: &Value, prefix: &str| {
        scenarios["scenarios"]
            .as_array()
            .into_iter()
            .flatten()
            .filter_map(|scenario| scenario["name"].as_str())
            .filter(|name| name.starts_with(prefix))
            .map(str::to_owned)
            .collect::<Vec<_>>()
    };
    let warned = |scenarios: &Value| {
        scenarios["warnings"]
            .as_array()
            .into_iter()
            .flatten()
            .filter_map(|warning| warning["message"].as_str())
            .filter_map(|message| message.strip_prefix("leadsTo "))
            .filter_map(|rest| rest.split(' ').next())
            .map(str::to_owned)
            .collect::<Vec<_>>()
    };
    let deep = json_of(&fslc(&["scenarios", &ord, "--depth", "4"]));
    let shallow = json_of(&fslc(&["scenarios", &ord, "--depth", "2"]));
    let failures = [
        misordered(
            "scenarios reach_*",
            &names(&deep, "reach_"),
            &["reach_p.Top", "reach_p.q__Top", "reach_p2.Top"],
        ),
        misordered(
            "scenarios respond_*",
            &names(&deep, "respond_"),
            &["respond_p.Back", "respond_p.q__Back", "respond_p2.Back"],
        ),
        misordered(
            "leadsTo warnings",
            &warned(&shallow),
            &["p.Back", "p.q__Back", "p2.Back"],
        ),
    ]
    .into_iter()
    .flatten()
    .collect::<Vec<_>>();
    assert!(failures.is_empty(), "{failures:#?}");
}

fn object_keys(value: &Value) -> Vec<String> {
    value
        .as_object()
        .map(|object| object.keys().cloned().collect())
        .unwrap_or_default()
}

/// `names` in their printed order, when they are not sorted or lack one of
/// `required`.
fn misordered(label: &str, names: &[String], required: &[&str]) -> Option<String> {
    let mut sorted = names.to_vec();
    sorted.sort();
    (names != sorted || required.iter().any(|name| !names.iter().any(|n| n == name)))
        .then(|| format!("{label}: {names:?}"))
}

/// Detector (r2 F2): every list is ordered by the name it prints. On
/// aca90678 the glue action `z__a` (printed `z.a`) sorted by its internal
/// name after `z.go`, scenarios listed `reach_acct2.Top` before
/// `reach_acct.Top`, refine printed `action_map` and `progress` in internal
/// order, and the conservation candidate listed `action:z__a` before
/// `action:z.b`. `k_used` and BMC `reachables` (fixed in a2629636) fail the
/// same way if they return to internal order.
#[test]
fn every_list_is_ordered_by_the_name_it_prints() {
    let dir = scratch_dir("printed-order");
    write_dunder_fixture(&dir);
    let ranked = path_arg(&dir, "ranked.fsl");
    let ranked_map = path_arg(&dir, "ranked-map.fsl");
    let verify = json_of(&fslc(&["verify", &ranked, "--depth", "4", "--no-cache"]));
    let induction = json_of(&fslc(&[
        "verify",
        &ranked,
        "--engine",
        "induction",
        "--no-cache",
    ]));
    let scenarios = json_of(&fslc(&["scenarios", &ranked, "--depth", "4"]));
    let refine = json_of(&fslc(&[
        "refine",
        &ranked,
        &ranked,
        &ranked_map,
        "--depth",
        "3",
    ]));
    let review = json_of(&fslc(&[
        "analyze",
        &path_arg(&dir, "conserve.fsl"),
        "--profile",
        "ai-review",
    ]));
    let reach = scenarios["scenarios"]
        .as_array()
        .into_iter()
        .flatten()
        .filter_map(|scenario| scenario["name"].as_str())
        .filter(|name| name.starts_with("reach_"))
        .map(str::to_owned)
        .collect::<Vec<_>>();
    let effects = review["findings"]
        .as_array()
        .into_iter()
        .flatten()
        .filter(|finding| finding["finding_type"] == "conservation_candidate")
        .flat_map(|finding| {
            finding["witness"]["action_net_effects"]
                .as_array()
                .into_iter()
                .flatten()
        })
        .filter_map(|effect| effect["action"].as_str())
        .map(str::to_owned)
        .collect::<Vec<_>>();
    let failures = [
        misordered(
            "verify reachables",
            &object_keys(&verify["reachables"]),
            &["acct.Top", "acct2.Top"],
        ),
        misordered(
            "verify action_coverage",
            &object_keys(&verify["action_coverage"]),
            &["z.a", "z.go"],
        ),
        misordered(
            "induction k_used",
            &object_keys(&induction["k_used"]),
            &["acct.Inv", "acct2.Inv"],
        ),
        misordered(
            "scenarios reach_*",
            &reach,
            &["reach_acct.Top", "reach_acct2.Top"],
        ),
        misordered(
            "refine action_map",
            &object_keys(&refine["action_map"]),
            &["acct.back", "acct2.back", "z.a"],
        ),
        misordered(
            "refine progress",
            &object_keys(&refine["progress"]),
            &["acct.Back", "acct2.Back"],
        ),
        misordered(
            "conservation action_net_effects",
            &effects,
            &["action:z.b", "action:z__a"],
        ),
    ]
    .into_iter()
    .flatten()
    .collect::<Vec<_>>();
    assert!(failures.is_empty(), "{failures:#?}");
}

/// Detector (r2 F4): ai-review joins findings to component actions by node
/// ID. With the internal ID in the semantic records, `a__b.left`/`right` are
/// reported as unguarded; with it in the `@undecided` record, their divergent
/// choice is not acknowledged; with it in the progressless metadata, the
/// tagged `acct` cycle is missed; with it in the fairness check, the fair
/// `acct2` cycle is reported.
#[test]
fn ai_review_joins_component_actions_by_published_node_id() {
    let dir = scratch_dir("ai-review");
    write_dunder_fixture(&dir);
    let output = fslc(&[
        "analyze",
        &path_arg(&dir, "review.fsl"),
        "--profile",
        "ai-review",
    ]);
    assert_eq!(output.status.code(), Some(0), "{:#}", json_of(&output));
    let review = json_of(&output);
    let findings = review["findings"].as_array().expect("findings");
    let involves = |finding: &Value, node: &str| {
        finding["involved_nodes"]
            .as_array()
            .is_some_and(|nodes| nodes.iter().any(|involved| involved == node))
    };
    let of_kind = |kind: &str| {
        findings
            .iter()
            .filter(|finding| finding["finding_type"] == kind)
            .collect::<Vec<_>>()
    };
    let mut failures = Vec::new();
    let acknowledged = of_kind("divergent_choice").into_iter().any(|finding| {
        involves(finding, "action:a__b.left")
            && involves(finding, "action:a__b.right")
            && finding["acknowledged"] == true
            && finding["acknowledged_by"][0]["declaration"] == "action a__b.left"
    });
    if !acknowledged {
        failures.push("left/right divergent choice is not acknowledged".to_owned());
    }
    let unguarded = of_kind("unguarded_action");
    if !unguarded.is_empty() {
        failures.push(format!("unguarded actions reported: {unguarded:?}"));
    }
    let cycles = of_kind("progressless_cycle");
    if !cycles
        .iter()
        .any(|finding| involves(finding, "action:acct.up") && involves(finding, "action:acct.down"))
    {
        failures.push(format!("acct cycle missed: {cycles:?}"));
    }
    if cycles.iter().any(|finding| {
        involves(finding, "action:acct2.up") || involves(finding, "action:acct2.down")
    }) {
        failures.push(format!("fair acct2 cycle reported: {cycles:?}"));
    }
    assert!(failures.is_empty(), "{failures:#?}");
}

/// Detector (r2 F4): a lemma rejected by a bounded violation names the
/// violation from the action that stepped; 458040f3 printed
/// `_partial_a.b__split`. The lemma's induction checks only the lemma, so its
/// `unknown_cti` branch never names an action.
#[test]
fn rejected_lemma_names_the_violation_from_the_action() {
    let dir = scratch_dir("lemma");
    write_dunder_fixture(&dir);
    let output = fslc(&[
        "verify",
        &path_arg(&dir, "split.fsl"),
        "--engine",
        "induction",
        "--lemma",
        "a__b__q >= 0",
        "--no-cache",
    ]);
    assert_eq!(output.status.code(), Some(1), "{:#}", json_of(&output));
    let verify = json_of(&output);
    let proof = &verify["lemmas"][0]["proof"];
    assert_eq!(proof["result"], "violated", "{verify:#}");
    assert_eq!(proof["invariant"], "_partial_a__b.split", "{verify:#}");
    assert_eq!(
        wrong_action_spellings(&String::from_utf8_lossy(&output.stdout)),
        Vec::<String>::new()
    );
}

/// Preservation: `causal observe-expectations` cannot take a compose
/// companion (expectation lowering requires a plain spec), so its `maps auto`
/// log lookup never sees a component action. This pins that boundary.
#[test]
fn causal_observation_rejects_a_compose_companion() {
    let dir = scratch_dir("causal");
    std::fs::copy(
        root().join("examples/causal/incident_system.fsl"),
        dir.join("incident_system.fsl"),
    )
    .expect("copy companion");
    for (file, source) in [
        (
            "companion.fsl",
            "compose IncidentCompose {\n  use IncidentSystem as a__b from \"incident_system.fsl\"\n}\n",
        ),
        (
            "causal.fsl",
            "causal ComposeIncident {\n  uses ops from \"companion.fsl\"\n\n  timebase day\n  horizon 365\n\n  clock ops_clock {\n    kernel ops\n    1 tick = 1 day\n  }\n\n  variable guardrails {\n    role intervention\n    binds action ops.a__b__deploy_guardrails\n  }\n\n  variable mttr {\n    role outcome\n    observes state ops.a__b__mttr_hours\n    cadence 7\n  }\n\n  claim C guardrails -> mttr {\n    version 1\n    status active\n    polarity negative\n    lag 1..45\n    persists 1..90\n    basis hypothesis\n  }\n\n  expectation E_Visible {\n    trigger action ops.a__b__deploy_guardrails\n    response predicate ops { a__b__guardrails >= 1 }\n    within 1\n    clock ops_clock\n    derived_from_claim C\n  }\n}\n",
        ),
        (
            "log-map.fsl",
            "refinement LogMap {\n  impl ComposeLog\n  abs IncidentCompose\n  maps auto\n}\n",
        ),
        (
            "log.jsonl",
            "{\"action\":\"a__b.deploy_guardrails\",\"params\":{},\"state\":{\"a.b__guardrails\":1,\"a.b__alert_precision\":3,\"a.b__mttr_hours\":24}}\n",
        ),
        (
            "scope.json",
            "{\"population\": [\"all_users\"], \"environment\": [\"production\"]}\n",
        ),
    ] {
        std::fs::write(dir.join(file), source).expect("write causal fixture");
    }
    let output = fslc(&[
        "causal",
        "observe-expectations",
        &path_arg(&dir, "causal.fsl"),
        "--from-log",
        &path_arg(&dir, "log.jsonl"),
        "--mapping",
        &path_arg(&dir, "log-map.fsl"),
        "--scope",
        &path_arg(&dir, "scope.json"),
        "--period-start",
        "2026-01-01",
        "--period-end",
        "2026-03-31",
    ]);
    let result = json_of(&output);
    assert_eq!(output.status.code(), Some(2), "{result:#}");
    assert!(
        result["message"]
            .as_str()
            .is_some_and(|message| message.contains("requires a plain kernel spec target")),
        "{result:#}"
    );
}

/// Detector (r2 F3): a non-compose state named `a__b__c` is published
/// `a.b__c`. On 458040f3 testgen passed that public name through the string
/// rule a second time (`a.b.c`) and rejected its own walk with "unknown state
/// fields" (exit 2); every target now generates a scaffold keyed `a.b__c`.
#[test]
fn non_compose_state_with_two_separators_generates_every_target() {
    let dir = scratch_dir("two-separators");
    let spec = dir.join("state.fsl");
    std::fs::write(
        &spec,
        "spec S {\n  state { a__b__c: 0..1 }\n  init { a__b__c = 0 }\n  action go() {\n    requires a__b__c < 1\n    a__b__c = 1\n  }\n}\n",
    )
    .expect("write spec");
    let mut failures = Vec::new();
    for target in ["pytest", "vitest", "swift", "kotlin", "dart", "phpunit"] {
        let path = dir.join(format!("state.{target}"));
        let output = fslc(&[
            "testgen",
            spec.to_str().expect("utf-8 path"),
            "--target",
            target,
            "-o",
            path.to_str().expect("utf-8 path"),
        ]);
        if output.status.code() != Some(0) {
            failures.push(format!(
                "{target}: exit {:?}: {}",
                output.status.code(),
                String::from_utf8_lossy(&output.stdout)
            ));
            continue;
        }
        let text = std::fs::read_to_string(&path).unwrap_or_default();
        if !text.contains("a.b__c") || text.contains("a.b.c") {
            failures.push(format!("{target}: state key is not a.b__c"));
        }
    }
    assert!(failures.is_empty(), "{failures:#?}");
}

/// A non-compose action whose name contains `__` is printed with `.`, so a
/// keyed list orders it by that form: `step.a` before `step2`.
#[test]
fn non_compose_separator_names_are_ordered_by_the_printed_name() {
    let dir = scratch_dir("non-compose-order");
    let spec = dir.join("order.fsl");
    std::fs::write(
        &spec,
        "spec Order {\n  state { n: 0..2 }\n  init { n = 0 }\n  action step2() {\n    requires n < 2\n    n = n + 1\n  }\n  action step__a() {\n    requires n > 0\n    n = n - 1\n  }\n  invariant Inv { n <= 2 }\n}\n",
    )
    .expect("write spec");
    let output = fslc(&[
        "verify",
        spec.to_str().expect("utf-8 path"),
        "--depth",
        "3",
        "--no-cache",
    ]);
    assert_eq!(output.status.code(), Some(0), "{output:?}");
    assert_eq!(
        object_keys(&json_of(&output)["action_coverage"]),
        ["step.a", "step2"]
    );
}
