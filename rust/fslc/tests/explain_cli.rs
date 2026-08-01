// SPDX-License-Identifier: Apache-2.0

//! Negative controls for `fslc explain` reporting-surface defects (issues
//! #528, #530): the verdict/verification machinery is untouched, but the
//! explain skeleton and `--readable` projection previously dropped or
//! mis-rendered structural information. Every test here fails if the
//! corresponding fix is reverted.

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
        "3",
    ]);
    std::fs::remove_file(path).expect("remove explain fixture");
    result
}

const BRANCHED_SPEC: &str = "examples/layers/return_system.fsl";

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
