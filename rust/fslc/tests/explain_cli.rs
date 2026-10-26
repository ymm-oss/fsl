// SPDX-License-Identifier: Apache-2.0

//! Controls for `fslc explain` reporting-surface defects (issues
//! #528, #530, #1350): the verdict/verification machinery is untouched, but the
//! explain skeleton and `--readable` projection previously dropped or
//! mis-rendered structural information. Detector tests reject a reverted fix;
//! explicitly labelled preservation controls protect previously working inputs.

use std::process::Command;

fn run_cli(args: &[&str]) -> (serde_json::Value, i32) {
    let root = std::path::Path::new(env!("CARGO_MANIFEST_DIR"))
        .parent()
        .and_then(std::path::Path::parent)
        .expect("workspace root");
    let output = Command::new(env!("CARGO_BIN_EXE_fslc"))
        .args(args)
        .current_dir(root)
        .output()
        .expect("run native CLI");
    let value = serde_json::from_slice(&output.stdout).unwrap_or_else(|error| {
        panic!(
            "invalid JSON: {error}; stderr={}",
            String::from_utf8_lossy(&output.stderr)
        )
    });
    (value, output.status.code().expect("native exit status"))
}

fn run_cli_text(args: &[&str]) -> String {
    let root = std::path::Path::new(env!("CARGO_MANIFEST_DIR"))
        .parent()
        .and_then(std::path::Path::parent)
        .expect("workspace root");
    let output = Command::new(env!("CARGO_BIN_EXE_fslc"))
        .args(args)
        .current_dir(root)
        .output()
        .expect("run native CLI");
    assert!(
        output.status.success(),
        "explain --readable failed: {}",
        String::from_utf8_lossy(&output.stderr)
    );
    // `--readable` prints the plain text view to stdout, not the JSON envelope.
    String::from_utf8(output.stdout).expect("UTF-8 readable output")
}

fn run_explain_source(source: &str) -> (serde_json::Value, i32) {
    run_explain_source_at_depth(source, "3")
}

fn run_explain_source_at_depth(source: &str, depth: &str) -> (serde_json::Value, i32) {
    let path = std::env::temp_dir().join(format!(
        "fslc-explain-counterfactual-scope-{}-{:?}.fsl",
        std::process::id(),
        std::thread::current().id()
    ));
    std::fs::write(&path, source).expect("write explain fixture");
    let result = run_cli(&[
        "explain",
        path.to_str().expect("UTF-8 fixture path"),
        "--depth",
        depth,
    ]);
    std::fs::remove_file(path).expect("remove explain fixture");
    result
}

// ---------------------------------------------------------------------
// #1350 — scoped kernel domains must reach both counterfactual rebuilds.
// ---------------------------------------------------------------------

const ENTITY_MAP_COUNTERFACTUAL: &str = r"spec C {
  entity R
  type N = 0..3
  state { x: Map<R, N> }
  init { forall r: R { x[r] = 0 } }
  action inc(r: R) {
    requires x[r] < 2
    x[r] = x[r] + 1
  }
  invariant AtMostTwo { forall r: R { x[r] <= 2 } }
}
verify { instances R = 2 }
";

const ENTITY_PARAMETER_COUNTERFACTUAL: &str = r"spec C {
  entity R
  type N = 0..3
  state { x: N }
  init { x = 0 }
  action inc(r: R) {
    requires x < 2
    x = x + 1
  }
  invariant AtMostTwo { x <= 2 }
}
verify { instances R = 2 }
";

const NUMBER_COUNTERFACTUAL: &str = r"spec C {
  number N
  state { x: N }
  init { x = 0 }
  action inc() {
    requires x < 2
    x = x + 1
  }
  invariant AtMostTwo { x <= 2 }
}
verify { values N = 0..3 }
";

const ENTITY_REACHABLE_COUNTERFACTUAL: &str = r"spec C {
  entity R
  state { done: Bool }
  init { done = false }
  action finish(r: R) { done = true }
  reachable Done { done }
}
verify { instances R = 2 }
";

fn counterfactual_type_control(source: &str) -> String {
    // Keep action/property/guard lines aligned for full-JSON comparisons.
    source
        .replace("entity R", "type R = 0..1")
        .replace("number N", "type N = 0..3")
        .replace("verify { instances R = 2 }\n", "")
        .replace("verify { values N = 0..3 }\n", "")
}

fn assert_kernel_counterfactual_envelope(value: &serde_json::Value, status: i32, depth: usize) {
    assert_eq!(status, 0, "{value}");
    assert_eq!(value["fsl"], "1.0", "{value}");
    assert_eq!(value["result"], "explained", "{value}");
    assert_eq!(value["spec"], "C", "{value}");
    assert_eq!(value["depth"], depth, "{value}");
    assert_eq!(
        value["counterfactual_scope"],
        serde_json::json!({
            "properties": ["invariant", "reachable"],
            "liveness": "skipped",
            "reason": "counterfactual weakening is safety/reachability analysis; leadsTo remains verified by the normal verify command",
        }),
        "{value}"
    );
}

fn assert_protecting_guard_counterfactual(value: &serde_json::Value, guard: &str, line: usize) {
    let items = value["counterfactuals"]
        .as_array()
        .expect("counterfactuals");
    assert_eq!(
        items.len(),
        1,
        "expected AtMostTwo protecting-guard counterfactual, got {items:?}"
    );
    let item = &items[0];
    assert_eq!(item["invariant"], "AtMostTwo", "{item}");
    assert_eq!(
        item["weakening"],
        serde_json::json!({
            "op": "requires-removal",
            "loc": {"line": line, "column": 5},
            "target": "inc requires #1",
            "source_text": guard,
        }),
        "{item}"
    );
    assert_eq!(item.get("requirement"), Some(&serde_json::Value::Null));
    assert_eq!(item["violation"]["violation_kind"], "invariant", "{item}");
    assert_eq!(item["violation"]["invariant"], "AtMostTwo", "{item}");
    assert_eq!(item["violation"]["violated_at_step"], 3, "{item}");
    let trace = item["trace"].as_array().expect("counterfactual trace");
    assert!(!trace.is_empty(), "{item}");
    assert_eq!(trace.len(), 4, "{item}");
    // The dedicated counterfactual_violating_step_records_successor_and_changes
    // test is the correctness oracle for #1350's violating trace evidence.
}

fn counterfactuals_without_elapsed(mut value: serde_json::Value) -> serde_json::Value {
    let items = value["counterfactuals"]
        .as_array_mut()
        .expect("counterfactuals");
    assert!(
        !items.is_empty(),
        "elapsed-time exclusion must not be vacuous"
    );
    for item in items {
        let cost = item["violation"]["cost"]
            .as_object_mut()
            .expect("counterfactual violation cost");
        // Recovery observed this value changing between otherwise identical
        // runs. Check presence on each side before excluding only this key.
        let elapsed = cost.remove("elapsed_s").expect("live elapsed_s exclusion");
        assert!(elapsed.as_f64().is_some_and(|seconds| seconds >= 0.0));
    }
    value
}

/// Detector: a verify-scoped Map key/action parameter must yield its protecting guard.
#[test]
fn explain_kernel_entity_map_counterfactual_names_protecting_guard() {
    let (scoped, status) = run_explain_source(ENTITY_MAP_COUNTERFACTUAL);
    assert_kernel_counterfactual_envelope(&scoped, status, 3);
    assert_protecting_guard_counterfactual(&scoped, "requires x[r] < 2", 7);

    let (typed, status) =
        run_explain_source(&counterfactual_type_control(ENTITY_MAP_COUNTERFACTUAL));
    assert_kernel_counterfactual_envelope(&typed, status, 3);
    assert_protecting_guard_counterfactual(&typed, "requires x[r] < 2", 7);
    assert_eq!(
        counterfactuals_without_elapsed(scoped),
        counterfactuals_without_elapsed(typed)
    );
}

/// Detector: an entity used only as an action parameter must not erase counterfactuals.
#[test]
fn explain_kernel_entity_parameter_counterfactual_names_protecting_guard() {
    let (value, status) = run_explain_source(ENTITY_PARAMETER_COUNTERFACTUAL);
    assert_kernel_counterfactual_envelope(&value, status, 3);
    assert_protecting_guard_counterfactual(&value, "requires x < 2", 7);
}

/// Detector: a verify-scoped number state must yield its protecting guard.
#[test]
fn explain_kernel_number_counterfactual_names_protecting_guard() {
    let (scoped, status) = run_explain_source(NUMBER_COUNTERFACTUAL);
    assert_kernel_counterfactual_envelope(&scoped, status, 3);
    assert_protecting_guard_counterfactual(&scoped, "requires x < 2", 6);

    let (typed, status) = run_explain_source(&counterfactual_type_control(NUMBER_COUNTERFACTUAL));
    assert_kernel_counterfactual_envelope(&typed, status, 3);
    assert_protecting_guard_counterfactual(&typed, "requires x < 2", 6);
    assert_eq!(
        counterfactuals_without_elapsed(scoped),
        counterfactuals_without_elapsed(typed)
    );
}

/// Preservation control: directly bounded Map/scalar specs already select these guards.
#[test]
fn explain_kernel_type_counterfactual_control_matches_current_behavior() {
    for (source, guard, line) in [
        (ENTITY_MAP_COUNTERFACTUAL, "requires x[r] < 2", 7),
        (NUMBER_COUNTERFACTUAL, "requires x < 2", 6),
    ] {
        let source = counterfactual_type_control(source);
        let (first, status) = run_explain_source(&source);
        assert_kernel_counterfactual_envelope(&first, status, 3);
        assert_eq!(first["skeleton"]["spec_kind"], "kernel");
        assert_protecting_guard_counterfactual(&first, guard, line);
        let (second, status) = run_explain_source(&source);
        assert_kernel_counterfactual_envelope(&second, status, 3);
        assert_protecting_guard_counterfactual(&second, guard, line);
        assert_eq!(
            counterfactuals_without_elapsed(first),
            counterfactuals_without_elapsed(second)
        );
    }
}

/// Preservation control: unused entities and lowered requirements already yield candidates.
#[test]
fn explain_kernel_unused_entity_and_requirements_controls_keep_counterfactuals() {
    let unused = ENTITY_PARAMETER_COUNTERFACTUAL.replace("inc(r: R)", "inc()");
    let (value, status) = run_explain_source(&unused);
    assert_kernel_counterfactual_envelope(&value, status, 3);
    assert_eq!(value["skeleton"]["spec_kind"], "kernel");
    assert_protecting_guard_counterfactual(&value, "requires x < 2", 7);

    let requirements = ENTITY_MAP_COUNTERFACTUAL.replacen("spec C", "requirements C", 1);
    let (value, status) = run_explain_source(&requirements);
    assert_kernel_counterfactual_envelope(&value, status, 3);
    assert_eq!(value["skeleton"]["spec_kind"], "requirements");
    assert_protecting_guard_counterfactual(&value, "requires x[r] < 2", 7);
}

/// Detector: shallow scoped searches need an invariant note, not an empty array.
/// The typed cases are preservation controls for the existing depth-limit note.
#[test]
fn explain_kernel_scoped_counterfactuals_report_depth_limit() {
    let expected = serde_json::json!([{
        "invariant": "AtMostTwo",
        "weakening": null,
        "trace": null,
        "requirement": null,
        "note": "no counterfactual within depth 2; init-assignment removal of bounded state x was not analyzed (skipped: nondeterministic initialization is unsupported by concrete search)",
    }]);
    let mut scoped_outputs = Vec::new();
    for source in [ENTITY_MAP_COUNTERFACTUAL, NUMBER_COUNTERFACTUAL] {
        let (typed, status) =
            run_explain_source_at_depth(&counterfactual_type_control(source), "2");
        assert_kernel_counterfactual_envelope(&typed, status, 2);
        assert_eq!(typed["counterfactuals"], expected, "typed control: {typed}");
        let (scoped, status) = run_explain_source_at_depth(source, "2");
        assert_kernel_counterfactual_envelope(&scoped, status, 2);
        scoped_outputs.push((scoped, typed));
    }
    for (scoped, typed) in scoped_outputs {
        assert_eq!(
            scoped["counterfactuals"], expected,
            "expected scoped AtMostTwo depth-limit note, got {}",
            scoped["counterfactuals"]
        );
        assert_eq!(scoped, typed);
    }
}

/// Detector: reachable candidate rebuilds must retain an entity parameter's verify scope.
#[test]
fn explain_kernel_entity_reachable_counterfactual_matches_type_control() {
    let (typed, status) = run_explain_source(&counterfactual_type_control(
        ENTITY_REACHABLE_COUNTERFACTUAL,
    ));
    assert_kernel_counterfactual_envelope(&typed, status, 3);
    let expected = serde_json::json!([{
        "property": "Done",
        "weakening": {
            "op": "assignment-removal",
            "loc": {"line": 5, "column": 25},
            "target": "finish assignment",
            "source_text": "action finish(r: R) { done = true }",
        },
        "result": "reachable_failed",
        "requirement": null,
        "faithfulness_class": "intent_unexercised",
        "recommended_action": "add a single-shot reachable for the action / raise --depth",
    }]);
    assert_eq!(typed["reachable_counterfactuals"], expected);
    let (scoped, status) = run_explain_source(ENTITY_REACHABLE_COUNTERFACTUAL);
    assert_kernel_counterfactual_envelope(&scoped, status, 3);
    assert_eq!(
        scoped.get("reachable_counterfactuals"),
        Some(&expected),
        "expected scoped Done reachable_counterfactuals, got {}",
        scoped
            .get("reachable_counterfactuals")
            .unwrap_or(&serde_json::Value::Null)
    );
    assert_eq!(scoped, typed);
}

/// Detector: reachable candidate rebuilds must retain a number state's verify scope.
#[test]
fn explain_kernel_number_reachable_counterfactual_matches_type_control() {
    let source =
        NUMBER_COUNTERFACTUAL.replace("invariant AtMostTwo { x <= 2 }", "reachable Two { x == 2 }");
    let (typed, status) = run_explain_source(&counterfactual_type_control(&source));
    assert_kernel_counterfactual_envelope(&typed, status, 3);
    let expected = serde_json::json!([{
        "property": "Two",
        "weakening": {
            "op": "assignment-removal",
            "loc": {"line": 7, "column": 5},
            "target": "inc assignment",
            "source_text": "x = x + 1",
        },
        "result": "reachable_failed",
        "requirement": null,
        "faithfulness_class": "intent_unexercised",
        "recommended_action": "add a single-shot reachable for the action / raise --depth",
    }]);
    assert_eq!(typed["reachable_counterfactuals"], expected);
    let (scoped, status) = run_explain_source(&source);
    assert_kernel_counterfactual_envelope(&scoped, status, 3);
    assert_eq!(
        scoped.get("reachable_counterfactuals"),
        Some(&expected),
        "expected scoped Two reachable_counterfactuals, got {}",
        scoped
            .get("reachable_counterfactuals")
            .unwrap_or(&serde_json::Value::Null)
    );
    assert_eq!(scoped, typed);
}

const BRANCHED_SPEC: &str = "rust/fslc/tests/fixtures/legacy_return_branches.fsl";

#[test]
fn explain_counterfactuals_declare_that_liveness_is_skipped() {
    let (value, status) = run_cli(&["explain", "specs/cart_v1.fsl", "--depth", "3"]);
    assert_eq!(status, 0, "{value}");
    assert_eq!(
        value["counterfactual_scope"]["properties"],
        serde_json::json!(["invariant", "reachable"])
    );
    assert_eq!(value["counterfactual_scope"]["liveness"], "skipped");

    let readable = run_cli_text(&["explain", "specs/cart_v1.fsl", "--depth", "3", "--readable"]);
    assert!(readable.contains("leadsTo liveness skipped"), "{readable}");
}

#[test]
fn quantified_liveness_with_mixed_fairness_does_not_block_safety_counterfactuals() {
    let source = r"
spec ExplainLivenessIsolation {
  type P = 0..1
  state { safe: Bool, ready: Bool, done: Map<P, Bool> }
  init {
    safe = true
    ready = false
    forall p: P { done[p] = false }
  }
  action corrupt() {
    requires ready
    safe = false
  }
  fair action complete(p: P) {
    requires not done[p]
    done[p] = true
  }
  action idle() { safe = safe }
  invariant Safe { safe }
  leadsTo EventuallyDone {
    forall p: P { not done[p] ~> done[p] }
  }
}
";
    let (value, status) = run_explain_source(source);
    assert_eq!(status, 0, "{value}");
    assert_eq!(value["counterfactual_scope"]["liveness"], "skipped");
    let safe = value["counterfactuals"]
        .as_array()
        .expect("counterfactuals")
        .iter()
        .find(|item| item["invariant"] == "Safe")
        .expect("Safe counterfactual");
    assert_ne!(safe["weakening"], serde_json::Value::Null, "{value}");
    assert!(
        safe["trace"]
            .as_array()
            .is_some_and(|trace| !trace.is_empty())
    );
}

// ---------------------------------------------------------------------
// #528 — readable branch/refinement detail restored, no internal-name leak.
// ---------------------------------------------------------------------

#[test]
fn explain_readable_branch_action_uses_authored_name_not_lowered_bn_form() {
    let text = run_cli_text(&["explain", BRANCHED_SPEC, "--depth", "3", "--readable"]);
    assert!(
        text.contains("- submit(c: CaseId, a: Amount) [fair]"),
        "authored `submit` identity must appear twice (once per branch): {text}"
    );
    assert!(
        !text.contains(".b1") && !text.contains(".b2"),
        "the lowered branch-suffixed name must not leak into readable output: {text}"
    );
    let submit_occurrences = text.matches("- submit(c: CaseId, a: Amount)").count();
    assert_eq!(
        submit_occurrences, 2,
        "both branches must render under the authored name: {text}"
    );
}

#[test]
fn explain_readable_shows_each_branch_predicate_and_correspondence() {
    let text = run_cli_text(&["explain", BRANCHED_SPEC, "--depth", "3", "--readable"]);
    assert!(
        text.contains("branch: when a <= AUTO_LIMIT maps approve(c)"),
        "{text}"
    );
    assert!(
        text.contains("branch: when a > AUTO_LIMIT maps stutter"),
        "{text}"
    );
}

#[test]
fn explain_readable_shows_the_synthesized_refinement_mapping() {
    let text = run_cli_text(&["explain", BRANCHED_SPEC, "--depth", "3", "--readable"]);
    assert!(text.contains("Implements:"), "{text}");
    assert!(text.contains("ReturnHandling: refines"), "{text}");
}

#[test]
fn explain_readable_unsplit_action_keeps_its_existing_output() {
    // Positive control: an action with no `branches` block, and a spec with
    // no `implements` declaration, must render exactly as before — no
    // "branch:"/"Implements:" noise, no name-lookup regression.
    let text = run_cli_text(&["explain", "specs/cart_v1.fsl", "--depth", "3", "--readable"]);
    assert!(!text.contains("Implements:"), "{text}");
    assert!(!text.contains("branch:"), "{text}");
    assert!(
        text.contains("- add_to_cart(u: UserId, i: ItemId)"),
        "{text}"
    );
}

#[test]
fn explain_json_skeleton_branch_action_also_uses_authored_name() {
    // The same origin fix also corrects the JSON skeleton's "name" field
    // (previously "submit.b1"/"submit.b2" there too), with the lowered form
    // preserved as "generated_name" for anything that still needs it.
    let (value, status) = run_cli(&["explain", BRANCHED_SPEC, "--depth", "3"]);
    assert_eq!(status, 0, "{value}");
    let actions = value["skeleton"]["actions"]
        .as_array()
        .expect("actions array");
    let submit_actions = actions
        .iter()
        .filter(|action| {
            action["generated_name"]
                .as_str()
                .unwrap_or("")
                .starts_with("submit.")
        })
        .collect::<Vec<_>>();
    assert_eq!(submit_actions.len(), 2, "{value}");
    for action in &submit_actions {
        assert_eq!(action["name"], "submit", "{action}");
        assert_eq!(action["origin"]["generated"], true, "{action}");
        assert_eq!(action["origin"]["dialect"], "requirements", "{action}");
    }
}

// ---------------------------------------------------------------------
// #530 — JSON skeleton restores spec_kind, partial_op checks, and
// generated-provenance metadata.
// ---------------------------------------------------------------------

#[test]
fn explain_skeleton_spec_kind_is_not_null() {
    let (value, status) = run_cli(&["explain", "examples/ui_spike/return_ui.fsl", "--depth", "3"]);
    assert_eq!(status, 0, "{value}");
    assert_eq!(value["skeleton"]["spec_kind"], "kernel", "{value}");
}

#[test]
fn explain_skeleton_spec_kind_reflects_the_requirements_dialect() {
    let (value, status) = run_cli(&["explain", BRANCHED_SPEC, "--depth", "3"]);
    assert_eq!(status, 0, "{value}");
    assert_eq!(value["skeleton"]["spec_kind"], "requirements", "{value}");
}

#[test]
fn explain_skeleton_auto_checks_includes_the_unguarded_partial_op() {
    let (value, status) = run_cli(&[
        "explain",
        "examples/gallery/errors/violated_partial_op_unchecked_pop.fsl",
        "--depth",
        "3",
    ]);
    assert_eq!(status, 0, "{value}");
    let checks = value["skeleton"]["auto_checks"]
        .as_array()
        .expect("auto_checks array");
    let partial_op = checks
        .iter()
        .find(|check| check["kind"] == "partial_op")
        .unwrap_or_else(|| panic!("a partial_op auto_check in {value}"));
    assert_eq!(partial_op["action"], "pop_empty", "{partial_op}");
    assert_eq!(partial_op["name"], "_partial_pop_empty", "{partial_op}");
    assert_eq!(partial_op["text"], "q.pop()", "{partial_op}");
    assert!(partial_op["loc"]["line"].is_u64(), "{partial_op}");
}

#[test]
fn explain_skeleton_still_lists_type_bound_checks() {
    // Positive control: adding partial_op entries must not crowd out the
    // existing type_bound auto-checks.
    let (value, status) = run_cli(&[
        "explain",
        "examples/gallery/errors/violated_partial_op_unchecked_pop.fsl",
        "--depth",
        "3",
    ]);
    assert_eq!(status, 0, "{value}");
    let checks = value["skeleton"]["auto_checks"]
        .as_array()
        .expect("auto_checks array");
    assert!(
        checks
            .iter()
            .any(|check| check["kind"] == "type_bound" && check["target"] == "q"),
        "{value}"
    );
}

#[test]
fn explain_skeleton_generated_sla_tick_and_deadline_carry_provenance() {
    let (value, status) = run_cli(&["explain", "examples/nfr/support_sla.fsl", "--depth", "3"]);
    assert_eq!(status, 0, "{value}");
    let actions = value["skeleton"]["actions"]
        .as_array()
        .expect("actions array");
    let tick = actions
        .iter()
        .find(|action| action["name"] == "tick")
        .unwrap_or_else(|| panic!("a 'tick' action in {value}"));
    assert_eq!(tick["origin"]["generated"], true, "{tick}");

    let properties = value["skeleton"]["properties"]
        .as_array()
        .expect("properties array");
    let deadline = properties
        .iter()
        .find(|property| {
            property["name"]
                .as_str()
                .is_some_and(|name| name.starts_with("_deadline_"))
        })
        .unwrap_or_else(|| panic!("a generated deadline property in {value}"));
    assert_eq!(deadline["origin"]["generated"], true, "{deadline}");
}

#[test]
fn explain_skeleton_authored_declarations_carry_no_generated_marker() {
    // Positive control: an authored action/invariant in the same spec that
    // synthesizes `tick`/`_deadline_*` must not be mistaken for generated.
    let (value, status) = run_cli(&["explain", "examples/nfr/support_sla.fsl", "--depth", "3"]);
    assert_eq!(status, 0, "{value}");
    let actions = value["skeleton"]["actions"]
        .as_array()
        .expect("actions array");
    let accept = actions
        .iter()
        .find(|action| action["name"] == "accept")
        .unwrap_or_else(|| panic!("an authored 'accept' action in {value}"));
    assert!(accept.get("origin").is_none(), "{accept}");
}

/// `(stdout, exit)` of `fslc <args...>` run in the e2e fixture directory of
/// the next edition, behind the gate when `gate`.
fn run_fsl5_e2e(args: &[&str], gate: bool) -> (String, i32) {
    run_fsl5("e2e", args, gate)
}

/// `(stdout, exit)` of `fslc <args...>` run in the fixture directory
/// `tests/fixtures/fsl5/<example>`, behind the gate when `gate`.
fn run_fsl5(example: &str, args: &[&str], gate: bool) -> (String, i32) {
    let mut command = Command::new(env!("CARGO_BIN_EXE_fslc"));
    command
        .args(args)
        .current_dir(
            std::path::Path::new(env!("CARGO_MANIFEST_DIR"))
                .join("tests/fixtures/fsl5")
                .join(example),
        )
        .env_remove(fslc_rust::unstable_gate::ENVIRONMENT_VARIABLE);
    if gate {
        command.env(fslc_rust::unstable_gate::ENVIRONMENT_VARIABLE, "fsl5");
    }
    let output = command.output().expect("run native CLI");
    (
        String::from_utf8(output.stdout).expect("UTF-8 stdout"),
        output.status.code().expect("native exit status"),
    )
}

/// The readable block of one action, from its `  - name(` line up to the next
/// action or section, without the final newline.
fn readable_action<'t>(readable: &'t str, action: &str) -> &'t str {
    let start = readable
        .find(&format!("  - {action}("))
        .unwrap_or_else(|| panic!("action {action} in {readable}"));
    let rest = &readable[start..];
    let end = rest[1..]
        .find("\n  - ")
        .map_or_else(|| rest.find("\n\n").unwrap_or(rest.len()), |end| end + 1);
    &rest[..end]
}

/// fsl 5 plan §3 S10 explain, design draft B.8: in the e2e example each
/// `requires` of `auto_approve` names where it came from: the stage guard the
/// event's move, the amount guard row 1 of the decision table REQ-2-3. The
/// golden was read and fixed by hand.
#[test]
fn explain_readable_names_the_origin_of_each_requires() {
    let (readable, exit) = run_fsl5_e2e(&["explain", "--readable", "fsl5_en.fsl"], true);
    assert_eq!(exit, 0, "{readable}");
    assert_eq!(
        readable_action(&readable, "auto_approve"),
        "  - auto_approve(c: Claim) [fair]
    event: System approves Claim Submitted -> Approved (fsl5_en.fsl:27)
    requirement: REQ-2: Claims at or below AUTO_LIMIT are auto-approved by the system
    requirement: REQ-2-3: Who handles a submitted claim
    requires:
      - requires claim_stage[c] == Submitted  ← move Submitted -> Approved (fsl5_en.fsl:27)
      - requires claim_amount[c] <= AUTO_LIMIT  ← decide REQ-2-3 for Submitted Claim row 1: SmallClaim => System approves (fsl5_en.fsl:37)
    writes: claim_stage"
    );
}

/// `value` without its `line` keys: the source lines of the three spellings
/// of the e2e example differ.
fn strip_lines(value: &mut serde_json::Value) {
    match value {
        serde_json::Value::Array(items) => items.iter_mut().for_each(strip_lines),
        serde_json::Value::Object(object) => {
            object.remove("line");
            object.values_mut().for_each(strip_lines);
        }
        _ => {}
    }
}

/// `explain --lowering` is the same correspondence as JSON, and it is the
/// same for the example written in English, in Japanese keywords and aliases,
/// and in particles: the lowering steps carry canonical names and English
/// keywords (`--names canonical` shows them as recorded), only the source
/// lines differ. Without the gate the option does not exist.
#[test]
fn explain_lowering_is_one_correspondence_for_every_spelling() {
    fn lowering(file: &str) -> serde_json::Value {
        let (stdout, exit) = run_fsl5_e2e(
            &["explain", "--lowering", "--names", "canonical", file],
            true,
        );
        assert_eq!(exit, 0, "{file}: {stdout}");
        let mut value: serde_json::Value = serde_json::from_str(&stdout).expect("JSON");
        strip_lines(&mut value);
        value["lowering"].take()
    }
    let english = lowering("fsl5_en.fsl");
    let auto_approve = english["actions"]
        .as_array()
        .expect("actions")
        .iter()
        .find(|action| action["action"] == "auto_approve")
        .unwrap_or_else(|| panic!("auto_approve in {english}"));
    assert_eq!(
        auto_approve["requires"],
        serde_json::json!([
            {"index": 0, "expr": "claim_stage[c] == Submitted",
             "origins": [{"kind": "move", "detail": "Submitted -> Approved"}]},
            {"index": 1, "expr": "claim_amount[c] <= AUTO_LIMIT",
             "origins": [{"kind": "decide",
                          "detail": "REQ-2-3 for Submitted Claim row 1: SmallClaim => System approves"}]},
        ])
    );
    for file in ["fsl5_ja_full.fsl", "fsl5_ja_particles.fsl"] {
        assert_eq!(lowering(file), english, "{file}");
    }
    let (stdout, exit) = run_fsl5_e2e(&["explain", "--lowering", "original.fsl"], false);
    assert_eq!(exit, 2, "{stdout}");
    assert!(
        stdout.contains("unknown explain option '--lowering'"),
        "{stdout}"
    );
}

/// The rules of the multi-entity example name their origins too: the guard
/// that reads the resource `stock`, and the `has <role>` property, in
/// canonical names whether the source is English or Japanese.
#[test]
fn explain_readable_names_resource_guard_and_role_property_origins() {
    for file in ["fsl5_en.fsl", "fsl5_ja_full.fsl"] {
        let (readable, exit) = run_fsl5(
            "multi_entity",
            &[
                "explain",
                "--readable",
                "--depth",
                "2",
                "--names",
                "canonical",
                file,
            ],
            true,
        );
        assert_eq!(exit, 0, "{file}: {readable}");
        for expected in [
            "      - requires qty > 0 and stock >= qty  ← rule REQ-PLACE allow User places \
             Order only when input qty > 0 and stock >= input qty",
            "  - invariant BuyerKnownAfterPlace\n    body: ",
            "    ← rule REQ-BUYER always Order not in Empty has buyer",
        ] {
            assert!(
                readable.contains(expected),
                "{file}: {expected:?} in {readable}"
            );
        }
    }
}
