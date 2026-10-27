// SPDX-License-Identifier: Apache-2.0
// Copyright 2026 Ryoichi Izumita

//! #1427 acceptance tests. Names use the reported CLI path. Integer extremes use
//! `build_test_plan_v1`: finite CLI conformance cannot enumerate the i64 domain,
//! and negative literals lower to unary expressions, not numeric guard operands.
//! The JSON pair is captured from the real limit fixture's kernel/conformance CLI
//! output, then its single guard and boundary vectors are adapted, not reimplemented.

use std::collections::BTreeSet;
use std::path::{Path, PathBuf};
use std::process::Command;

use serde_json::{Value, json};

fn root() -> PathBuf {
    Path::new(env!("CARGO_MANIFEST_DIR"))
        .parent()
        .and_then(Path::parent)
        .expect("repository root")
        .to_path_buf()
}

fn cli(args: &[&str]) -> Value {
    let output = Command::new(env!("CARGO_BIN_EXE_fslc"))
        .args(args)
        .current_dir(root())
        .output()
        .expect("run fslc");
    assert!(
        output.status.success(),
        "args={args:?}; status={:?}; stdout={}; stderr={}",
        output.status.code(),
        String::from_utf8_lossy(&output.stdout),
        String::from_utf8_lossy(&output.stderr)
    );
    serde_json::from_slice(&output.stdout)
        .unwrap_or_else(|error| panic!("args={args:?}; JSON={error}; stdout={:?}", output.stdout))
}

fn plan(source: &str) -> Value {
    let directory = root().join(".codex/tmp").join(format!(
        "issue-1427-{}-{:?}",
        std::process::id(),
        std::thread::current().id()
    ));
    std::fs::create_dir_all(&directory).expect("create local scratch");
    let path = directory.join("model.fsl");
    std::fs::write(&path, source).expect("write model");
    cli(&["testplan", path.to_str().expect("UTF-8"), "--depth", "0"])
}

fn boundary_names(plan: &Value) -> Vec<&str> {
    plan["cases"]
        .as_array()
        .unwrap_or_else(|| panic!("cases missing: {plan}"))
        .iter()
        .filter_map(|case| case["name"].as_str())
        .filter(|name| name.starts_with("boundary_"))
        .collect()
}

fn assert_unique(plan: &Value) {
    let names: Vec<_> = plan["cases"]
        .as_array()
        .unwrap_or_else(|| panic!("cases missing: {plan}"))
        .iter()
        .map(|case| case["name"].clone())
        .collect();
    let unique: BTreeSet<_> = names.iter().map(Value::to_string).collect();
    assert_eq!(
        unique.len(),
        names.len(),
        "produced names={names:?}; plan={plan}"
    );
}

/// Detector (fails on 9ad07c7a): both actions contribute accept/reject boundaries
/// for n; first keeps the plain name, later uses its public action spelling.
#[test]
fn two_actions_qualify_only_colliding_boundary_names() {
    let plan = plan(
        "spec Shared { type N = 0..4 state { done: Bool } init { done = false }
         action first(n: N) { requires n <= 3 done = true }
         action second(n: N) { requires n <= 3 done = true } }",
    );
    assert_unique(&plan);
    assert_eq!(
        boundary_names(&plan),
        [
            "boundary_accept_n",
            "boundary_accept_second_n",
            "boundary_reject_n",
            "boundary_reject_second_n"
        ],
        "produced plan={plan}"
    );
}

/// Detector (fails on 9ad07c7a): guard order in a single action must allocate
/// the action-qualified name to the second pair rather than duplicate names.
#[test]
fn two_guards_on_one_parameter_get_ordered_suffixes() {
    let plan = plan(
        "spec Between { type N = 0..4 state { done: Bool } init { done = false }
         action submit(n: N) { requires n >= 1 requires n <= 3 done = true } }",
    );
    assert_unique(&plan);
    assert_eq!(
        boundary_names(&plan),
        [
            "boundary_accept_n",
            "boundary_accept_submit_n",
            "boundary_reject_n",
            "boundary_reject_submit_n"
        ],
        "produced plan={plan}"
    );
}

/// Detector (fails on 9ad07c7a): public branch punctuation is sanitized only
/// for the later duplicate. The first branch keeps #1256's plain reject name.
#[test]
fn branch_boundaries_keep_first_plain_name_and_sanitize_later_action() {
    let plan = cli(&[
        "testplan",
        "rust/fslc/tests/fixtures/legacy_return_branches.fsl",
        "--depth",
        "1",
    ]);
    assert_unique(&plan);
    for (action, name) in [
        ("submit.b1", "boundary_reject_a"),
        ("submit.b2", "boundary_reject_submit_b2_a"),
    ] {
        assert!(
            plan["cases"]
                .as_array()
                .expect("cases")
                .iter()
                .any(|case| case["name"] == name
                    && case["target"]["action"] == action
                    && case["target"]["params"]["a"] == 0),
            "missing {name} for {action}; produced plan={plan}"
        );
    }
}

/// Detector (fails on 9ad07c7a): a qualified name can collide with a previous
/// plain name, and _2 can itself be taken. Re-check until _3 is available.
#[test]
fn qualified_names_recheck_suffixes_against_all_previous_cases() {
    let plan = plan(
        "spec Suffixes { type N = 0..4 state { done: Bool } init { done = false }
         action seed(bar_n: N) { requires bar_n <= 3 done = true }
         action seed_two(bar_n_2: N) { requires bar_n_2 <= 3 done = true }
         action a_first(n: N) { requires n <= 3 done = true }
         action bar(n: N) { requires n <= 3 done = true } }",
    );
    assert_unique(&plan);
    for name in ["boundary_accept_bar_n_3", "boundary_reject_bar_n_3"] {
        assert!(
            plan["cases"]
                .as_array()
                .expect("cases")
                .iter()
                .any(|case| case["name"] == name && case["target"]["action"] == "bar"),
            "missing rechecked {name}; produced plan={plan}"
        );
    }
}

/// Control (passes before/after): #844's unique names remain byte-identical.
#[test]
fn unique_limit_boundary_names_keep_the_documented_spelling() {
    let plan = cli(&[
        "testplan",
        "rust/fslc/tests/fixtures/testplan_limit.fsl",
        "--depth",
        "0",
    ]);
    assert_eq!(
        boundary_names(&plan),
        ["boundary_accept_q", "boundary_reject_q"],
        "produced plan={plan}"
    );
    assert_unique(&plan);
}

fn extreme_plan(operator: &str, threshold: i64, kind: &str) -> Value {
    let fixture = "rust/fslc/tests/fixtures/testplan_limit.fsl";
    let mut kernel = cli(&["kernel", fixture]);
    let mut conformance = cli(&["conformance", fixture, "--depth", "0"]);
    kernel["actions"][0]["guards"][0]["expression"] = json!({
        "kind": "binary", "operator": operator,
        "left": {"kind": "var", "name": "q"},
        "right": {"kind": "num", "value": threshold}
    });
    // Retain a real outcome envelope of the appropriate kind. The only
    // representable boundary is MAX or MIN; there is no phantom opposite vector.
    let mut vector = conformance["vectors"]
        .as_array()
        .expect("real vectors")
        .iter()
        .find(|vector| vector["outcome"]["kind"] == kind)
        .unwrap_or_else(|| panic!("missing {kind}: {conformance}"))
        .clone();
    vector["action"]["params"]["q"] = json!(threshold);
    conformance["vectors"] = json!([vector]);
    let result = fsl_tools::build_test_plan_v1(&kernel, &conformance);
    assert!(
        result.is_ok(),
        "produced result={result:?}; kernel={kernel}; conformance={conformance}"
    );
    result.expect("checked result")
}

fn assert_extreme(operator: &str, threshold: i64, kind: &str, name: &str) {
    let plan = extreme_plan(operator, threshold, kind);
    assert_eq!(boundary_names(&plan), [name], "produced plan={plan}");
    let case = plan["cases"]
        .as_array()
        .expect("cases")
        .iter()
        .find(|case| case["name"] == name)
        .unwrap_or_else(|| panic!("missing {name}: {plan}"));
    assert_eq!(
        case["target"]["params"]["q"], threshold,
        "produced plan={plan}"
    );
    assert_eq!(
        case["expected"]["outcome_kind"], kind,
        "produced plan={plan}"
    );
}

/// Detector (fails on 9ad07c7a): <= MAX panics in debug while computing MAX+1;
/// it must retain the representable accept side and omit the impossible reject.
#[test]
fn less_equal_max_omits_unrepresentable_reject_without_panicking() {
    assert_extreme("<=", i64::MAX, "ok", "boundary_accept_q");
}

/// Detector (fails on 9ad07c7a): > MAX panics in debug computing its accept;
/// only the representable `requires_failed` vector may become a boundary case.
#[test]
fn greater_max_omits_unrepresentable_accept_without_panicking() {
    assert_extreme(">", i64::MAX, "requires_failed", "boundary_reject_q");
}

/// Control (passes before/after): < MIN has no representable accepting value;
/// the real rejecting MIN vector is preserved, with no spurious accepting case.
#[test]
fn less_min_preserves_only_representable_reject() {
    assert_extreme("<", i64::MIN, "requires_failed", "boundary_reject_q");
}

/// Control (passes before/after): >= MIN has no representable rejecting value;
/// the real accepting MIN vector is preserved, with no spurious rejecting case.
#[test]
fn greater_equal_min_preserves_only_representable_accept() {
    assert_extreme(">=", i64::MIN, "ok", "boundary_accept_q");
}
