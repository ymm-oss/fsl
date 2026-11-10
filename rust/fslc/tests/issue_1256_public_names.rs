// SPDX-License-Identifier: Apache-2.0
// Copyright 2026 Ryoichi Izumita

//! #1284 stage 2: every output spells an internal Kernel name through the
//! one owner (`fsl_core::naming`), so a name has one public spelling.
//!
//! - #1256 (decisions 051): check rejects a declared name containing `__`
//!   (exit 2, located at the declaration). A name a lowering generates still
//!   carries `__` (a requirements branch split `submit__b1`) and may still be
//!   referenced; it publishes as `submit.b1` in human-facing and identifier
//!   fields alike, replay accepts that spelling, and testplan matches the
//!   Public Kernel's internal name to it instead of dropping its boundary
//!   cases.
//! - #1255: conformance names a compose component invariant `acct.Inv`, and
//!   mutate's dead-action note compares the same spelling on both sides.
//! - Diagnostics and `explain` spell a compose component declaration
//!   `alias.name`.
//!
//! Detectors fail on 9d2a0922 (the base of this change); controls pass before
//! and after.

use std::path::{Path, PathBuf};
use std::process::{Command, Output};
use std::sync::atomic::{AtomicU64, Ordering};

use serde_json::{Value, json};

const BRANCHES: &str = "rust/fslc/tests/fixtures/legacy_return_branches.fsl";
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
        "rust/target/issue-1256-{name}-{}-{id}",
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

fn write(dir: &Path, file: &str, source: &str) -> String {
    let path = dir.join(file);
    std::fs::write(&path, source).expect("write fixture");
    path.to_str().expect("utf-8 path").to_owned()
}

/// The #1255 component: `thaw` is never enabled, `Inv` breaks at `bal == 2`.
const ACCT: &str = "spec Acct {\n  state { bal: Int, frozen: Bool }\n  init {\n    bal = 0\n    frozen = false\n  }\n  @requirement(\"REQ-DEP\", \"deposit increases balance\")\n  action dep() {\n    requires bal < 2\n    bal = bal + 1\n  }\n  @requirement(\"REQ-DEAD\", \"never enabled\")\n  action thaw() {\n    requires frozen and not frozen\n    frozen = false\n  }\n  @requirement(\"REQ-INV\", \"balance stays small\")\n  invariant Inv { bal <= 1 }\n}\n";
const SYS: &str = "compose Sys {\n  use Acct as acct from \"acct.fsl\"\n  state { n: Int }\n  init { n = 0 }\n  action step() = acct.dep() {\n    n = n + 1\n  }\n  invariant Small { n <= 5 }\n}\n";

/// Detector (#1256): a declared name containing `__` passed check and was
/// published as `foo.bar` by some outputs and `foo__bar` by others.
#[test]
fn a_declared_double_underscore_name_is_rejected_at_its_declaration() {
    let dir = scratch_dir("reject");
    let cases = [
        (
            "action.fsl",
            "spec S {\n  type N = 0..3\n  state { x: Int }\n  init { x = 0 }\n  action foo__bar(n: N) {\n    requires n >= 2\n    x = n\n  }\n}\n",
            "action name 'foo__bar'",
            (5, 10),
        ),
        (
            "state.fsl",
            "spec S {\n  state { x: Int, a__b: Bool }\n  init {\n    x = 0\n    a__b = false\n  }\n}\n",
            "state variable name 'a__b'",
            (2, 19),
        ),
        (
            "param.fsl",
            "spec S {\n  type N = 0..3\n  state { x: Int }\n  init { x = 0 }\n  action go(n__m: N) {\n    x = n__m\n  }\n}\n",
            "parameter name 'n__m'",
            (5, 13),
        ),
        (
            "requirements.fsl",
            "requirements R {\n  state { x: 0..2 }\n  init { x = 0 }\n  requirement REQ-1 \"bounded\" {\n    invariant Small__Inv { x <= 2 }\n  }\n}\n",
            "property name 'Small__Inv'",
            (5, 15),
        ),
    ];
    for (file, source, needle, (line, column)) in cases {
        let path = write(&dir, file, source);
        for command in ["check", "verify"] {
            let output = fslc(&[command, &path]);
            let value = json_of(&output);
            assert_eq!(output.status.code(), Some(2), "{command} {file}: {value:#}");
            assert_eq!(value["kind"], "name", "{command} {file}: {value:#}");
            assert!(
                value["message"].as_str().is_some_and(
                    |message| message.contains(needle) && message.contains("contains '__'")
                ),
                "{command} {file}: {value:#}"
            );
            assert_eq!(
                value["loc"],
                json!({"line": line, "column": column}),
                "{command} {file}: {value:#}"
            );
        }
    }

    // A compose alias, and a component's own declaration, which is reported
    // at the parent's `use`.
    write(&dir, "acct.fsl", ACCT);
    write(
        &dir,
        "dunder_acct.fsl",
        &ACCT.replace("action thaw()", "action th__aw()"),
    );
    for (file, source, needle, (line, column)) in [
        (
            "alias.fsl",
            SYS.replace("as acct from", "as ac__ct from")
                .replace("acct.dep()", "ac__ct.dep()"),
            "use alias name 'ac__ct'",
            (2, 15),
        ),
        (
            "component.fsl",
            SYS.replace("\"acct.fsl\"", "\"dunder_acct.fsl\""),
            "action name 'th__aw'",
            (2, 3),
        ),
    ] {
        let path = write(&dir, file, &source);
        let output = fslc(&["check", &path]);
        let value = json_of(&output);
        assert_eq!(output.status.code(), Some(2), "{file}: {value:#}");
        assert!(
            value["message"]
                .as_str()
                .is_some_and(|message| message.contains(needle)),
            "{file}: {value:#}"
        );
        assert_eq!(
            value["loc"],
            json!({"line": line, "column": column}),
            "{file}: {value:#}"
        );
    }
}

/// Control: referencing a generated name (a refinement mapping onto the
/// branch split `submit__b1`) is still accepted.
#[test]
fn a_reference_to_a_generated_name_is_still_accepted() {
    let output = fslc(&[
        "refine",
        "examples/layers/return_impl.fsl",
        BRANCHES,
        "rust/fslc/tests/fixtures/legacy_return_impl_refines.fsl",
        "--depth",
        "2",
    ]);
    let value = json_of(&output);
    assert_eq!(output.status.code(), Some(0), "{value:#}");
    assert_eq!(value["result"], "refines", "{value:#}");
    assert_eq!(
        value["action_map"]["submit_small"], "submit.b1",
        "{value:#}"
    );
}

fn keys(value: &Value) -> Vec<String> {
    value
        .as_object()
        .map(|object| object.keys().cloned().collect())
        .unwrap_or_default()
}

/// Detector (#1256 comment, #1255 class): a branch split published as
/// `submit.b1` in `action_coverage` and conformance actions, but as
/// `submit__b1` in `cost.properties[].name`, conformance outcome names
/// (`_requires_failed_submit__b1`) and the analyze graph's action IDs.
#[test]
fn a_branch_action_has_one_public_name_in_every_output() {
    let verify = json_of(&fslc(&["verify", BRANCHES, "--depth", "3", "--no-cache"]));
    let coverage = keys(&verify["action_coverage"]);
    assert!(coverage.contains(&"submit.b1".to_owned()), "{coverage:?}");
    let costed = verify["cost"]["properties"]
        .as_array()
        .expect("cost properties")
        .iter()
        .filter(|property| property["kind"] == "action_coverage")
        .filter_map(|property| property["name"].as_str())
        .collect::<Vec<_>>();
    assert!(costed.contains(&"submit.b1"), "{costed:?}");

    let conformance = json_of(&fslc(&["conformance", BRANCHES, "--depth", "1"]));
    let outcomes = conformance["vectors"]
        .as_array()
        .expect("vectors")
        .iter()
        .filter(|vector| vector["action"]["name"] == "submit.b1")
        .filter_map(|vector| vector["outcome"]["name"].as_str())
        .collect::<std::collections::BTreeSet<_>>();
    assert_eq!(
        outcomes,
        ["_requires_failed_submit.b1"].into_iter().collect(),
        "{outcomes:?}"
    );

    let analyze = json_of(&fslc(&["analyze", BRANCHES]));
    let nodes = analyze["nodes"]
        .as_array()
        .expect("analyze nodes")
        .iter()
        .filter_map(|node| node["id"].as_str())
        .collect::<Vec<_>>();
    assert!(nodes.contains(&"action:submit.b1"), "{nodes:?}");

    let mut leaks = Vec::new();
    for (label, text) in [
        ("verify", serde_json::to_string(&verify).expect("json")),
        (
            "conformance",
            serde_json::to_string(&conformance).expect("json"),
        ),
        ("analyze", serde_json::to_string(&analyze).expect("json")),
    ] {
        if text.contains("submit__b") {
            leaks.push(label);
        }
    }
    assert!(leaks.is_empty(), "internal submit__bN in {leaks:?}");
}

/// Detector (#1256): testplan looked conformance vectors up by the Public
/// Kernel's internal `submit__b1`, found none named that, and silently
/// selected no boundary case for the branch action.
#[test]
fn testplan_selects_the_boundary_cases_of_a_branch_action() {
    let output = fslc(&["testplan", BRANCHES, "--depth", "1"]);
    let plan = json_of(&output);
    assert_eq!(output.status.code(), Some(0), "{plan:#}");
    let boundaries = plan["cases"]
        .as_array()
        .expect("cases")
        .iter()
        .filter(|case| {
            case["name"]
                .as_str()
                .is_some_and(|name| name.starts_with("boundary_"))
        })
        .map(|case| {
            (
                case["name"].as_str().unwrap_or_default().to_owned(),
                case["target"]["action"]
                    .as_str()
                    .unwrap_or_default()
                    .to_owned(),
            )
        })
        .collect::<Vec<_>>();
    assert!(
        boundaries.contains(&("boundary_accept_a".to_owned(), "submit.b1".to_owned()))
            && boundaries.contains(&("boundary_reject_a".to_owned(), "submit.b1".to_owned())),
        "{boundaries:?}"
    );
}

/// Detector (#1256): testplan does not drop a Kernel action it cannot find in
/// the conformance vectors; it fails closed.
#[test]
fn testplan_fails_closed_when_no_vector_names_a_kernel_action() {
    let kernel = json_of(&fslc(&["kernel", BRANCHES]));
    let mut conformance = json_of(&fslc(&["conformance", BRANCHES, "--depth", "1"]));
    conformance["vectors"]
        .as_array_mut()
        .expect("vectors")
        .retain(|vector| vector["action"]["name"] != "submit.b1");
    let error = fsl_tools::build_test_plan_v1(&kernel, &conformance)
        .expect_err("a Kernel action with a boundary guard and no vector");
    assert!(
        error.contains("'submit.b1'") && error.contains("no conformance vector"),
        "{error}"
    );
}

fn replay(trace: &Value, dir: &Path, name: &str) -> Value {
    let path = dir.join(format!("{name}.json"));
    std::fs::write(&path, serde_json::to_vec(trace).expect("trace json")).expect("write trace");
    json_of(&fslc(&[
        "replay",
        BRANCHES,
        "--trace",
        path.to_str().expect("utf-8 path"),
    ]))
}

/// Detector (#1256): replay rejected `submit.b1`, the spelling conformance
/// prints, as an unknown action. Control: the internal `submit__b1` older
/// traces wrote is still accepted, and an echo uses the public spelling.
#[test]
fn replay_accepts_the_published_branch_name_and_the_internal_one() {
    let conformance = json_of(&fslc(&["conformance", BRANCHES, "--depth", "1"]));
    let initial = conformance["states"][0]["state"].clone();
    let vector = conformance["vectors"]
        .as_array()
        .expect("vectors")
        .iter()
        .find(|vector| {
            vector["state"] == "s0"
                && vector["action"]["name"] == "submit.b1"
                && vector["outcome"]["kind"] == "ok"
        })
        .expect("an ok submit.b1 vector")
        .clone();
    let trace = |action: &str, state: &Value| {
        json!({
            "$schema": "https://fsl.dev/schemas/fslc/kernel/replay-trace.v1.schema.json",
            "schema_version": "1.2.0",
            "kernel_schema_version": "1.0.0",
            "spec": conformance["spec"],
            "initial": initial,
            "events": [{
                "tick": 1,
                "action": action,
                "params": vector["action"]["params"],
                "state": state,
            }],
        })
    };
    let dir = scratch_dir("replay");
    let stepped = vector["outcome"]["state"].clone();
    for spelling in ["submit.b1", "submit__b1"] {
        let output = replay(&trace(spelling, &stepped), &dir, "conformant");
        assert_eq!(output["result"], "conformant", "{spelling}: {output:#}");
        let output = replay(&trace(spelling, &initial), &dir, "mismatch");
        assert_eq!(
            output["violation"]["kind"], "state_mismatch",
            "{spelling}: {output:#}"
        );
        assert_eq!(
            output["violation"]["action"], "submit.b1",
            "{spelling}: {output:#}"
        );
    }
}

/// Detector (#1255): conformance named a component invariant broken while a
/// glue action stepped by its internal `acct__Inv`; verify and replay say
/// `acct.Inv`.
#[test]
fn conformance_names_a_component_invariant_publicly() {
    let dir = scratch_dir("conformance");
    write(&dir, "acct.fsl", ACCT);
    let sys = write(&dir, "sys.fsl", SYS);
    let conformance = json_of(&fslc(&["conformance", &sys, "--depth", "3"]));
    let invariants = conformance["vectors"]
        .as_array()
        .expect("vectors")
        .iter()
        .filter(|vector| vector["outcome"]["kind"] == "invariant")
        .filter_map(|vector| vector["outcome"]["name"].as_str())
        .collect::<std::collections::BTreeSet<_>>();
    assert_eq!(
        invariants,
        ["acct.Inv"].into_iter().collect(),
        "{invariants:?}"
    );
}

/// Detector (#1255): mutate compared the dead `action_coverage` keys
/// (`acct.thaw`) with the internal mutant action (`acct__thaw`), so a compose
/// lost the note a single spec gets. Control: the single spec keeps it.
#[test]
fn mutate_notes_a_dead_component_action() {
    let dir = scratch_dir("mutate");
    let acct = ACCT.replace("bal <= 1", "bal <= 2");
    write(&dir, "acct.fsl", &acct);
    let sys = write(&dir, "sys.fsl", SYS);
    let solo = write(&dir, "solo.fsl", &acct.replace("spec Acct", "spec Solo"));
    for (path, action) in [(sys, "acct.thaw"), (solo, "thaw")] {
        let output = json_of(&fslc(&["mutate", &path, "--depth", "3"]));
        let thaw = output["mutants"]
            .as_array()
            .expect("mutants")
            .iter()
            .filter(|mutant| {
                mutant["target"]
                    .as_str()
                    .is_some_and(|target| target.starts_with(&format!("{action} ")))
            })
            .collect::<Vec<_>>();
        assert!(!thaw.is_empty(), "{action}: {output:#}");
        for mutant in thaw {
            assert_eq!(mutant["status"], "survived", "{mutant:#}");
            assert_eq!(
                mutant["note"], "action dead at baseline — survival expected",
                "{mutant:#}"
            );
        }
    }
}

const GUARDED_BANK: &str = r#"compose GuardedBank {
  use BankAccountImpl as bank from "bank_impl.fsl"
  use AuditLog as audit from "audit_log.fsl"

  action deposit_audited(a: bank.Amount) =
      bank.submit_deposit(a) || audit.deposit(a) {
    requires audit.log.size() < 2
  }

  internal bank.submit_deposit
  internal audit.deposit

  invariant ShortAudit { audit.log.size() <= 2 }
}
"#;

/// Detector: readable `explain` printed component types as `bank__Amount`,
/// and the blame effect text kept the internal assignment root
/// `audit__log = audit.log.push(a)`.
#[test]
fn explain_spells_component_types_and_assignment_roots_publicly() {
    let readable = fslc(&["explain", BANK, "--depth", "3", "--readable"]);
    let text = String::from_utf8_lossy(&readable.stdout);
    assert!(
        text.contains("deposit_audited(a: bank.Amount)") && !text.contains("bank__"),
        "{text}"
    );
    let explain = fslc(&["explain", BANK, "--depth", "3"]);
    let text = String::from_utf8_lossy(&explain.stdout);
    assert!(!text.contains("audit__"), "{text}");

    // A bounded log invariant forces a selected action trace even when BANK's
    // other invariants now select shorter init-weakening counterfactuals.
    let dir = scratch_dir("explain-effect");
    write(
        &dir,
        "bank_impl.fsl",
        include_str!("../../../specs/bank_impl.fsl"),
    );
    write(
        &dir,
        "audit_log.fsl",
        include_str!("../../../specs/audit_log.fsl"),
    );
    let path = write(&dir, "guarded_bank.fsl", GUARDED_BANK);
    let explain = fslc(&["explain", &path, "--depth", "3"]);
    assert!(explain.status.success(), "{explain:?}");
    let value = json_of(&explain);
    let candidate = value["counterfactuals"]
        .as_array()
        .expect("counterfactuals")
        .iter()
        .find(|item| item["invariant"] == "ShortAudit")
        .expect("ShortAudit counterfactual");
    assert_eq!(candidate["weakening"]["op"], "requires-removal");
    assert_eq!(candidate["violation"]["violated_at_step"], 3);
    let trace = candidate["trace"].as_array().expect("selected trace");
    assert_eq!(trace.len(), 4);
    assert!(
        trace[1..]
            .iter()
            .all(|step| step["action"]["name"] == "deposit_audited")
    );
    let text = String::from_utf8_lossy(&explain.stdout);
    assert!(
        text.contains("\"audit.log = audit.log.push(a)\"") && !text.contains("audit__"),
        "{text}"
    );
}

/// Detector: a compose diagnostic named a component declaration by its
/// internal name (`'acct__MAXX'`, `Named("t__Amount")`).
#[test]
fn compose_diagnostics_name_component_declarations_publicly() {
    let dir = scratch_dir("diagnostics");
    write(&dir, "acct.fsl", ACCT);
    write(
        &dir,
        "t.fsl",
        "spec T {\n  type Amount = 0..3\n  state { x: Amount }\n  init { x = 0 }\n  action a() {\n    x = true\n  }\n}\n",
    );
    for (file, source, needle) in [
        (
            "unknown.fsl",
            SYS.replace(
                "invariant Small { n <= 5 }",
                "invariant Small { n <= acct.MAXX }",
            ),
            "public Kernel cannot type identifier 'acct.MAXX'",
        ),
        (
            "mismatch.fsl",
            "compose M {\n  use T as t from \"t.fsl\"\n}\n".to_owned(),
            "expression of type Bool is not assignable to Named(\"t.Amount\")",
        ),
    ] {
        let path = write(&dir, file, &source);
        let output = fslc(&["check", &path]);
        let value = json_of(&output);
        assert_eq!(output.status.code(), Some(2), "{file}: {value:#}");
        let message = value["message"].as_str().unwrap_or_default();
        assert!(
            message.contains(needle) && !message.contains("__"),
            "{file}: {value:#}"
        );
    }
}
