// SPDX-License-Identifier: Apache-2.0

//! #713: saga compensation must guard on BOTH the trigger event flag and the
//! `after_event` flag (`docs/DESIGN-domain.md:72` — "saga compensation ->
//! kernel action guarded by trigger/after event flags"). Before this fix,
//! both lowering paths (`lower_saga_actions` in `domain_lowering.rs` and
//! `render_saga_actions` in `domain.rs`) only required the trigger flag, so a
//! compensation action could fire on a trace that never observed the
//! `after_event` it is supposed to be gated on.
//!
//! `rust/fslc/tests/fixtures/domain_saga_compensation_dual_guard.fsl` is
//! built so a single `decide` can emit both the trigger and after events in
//! one transition (`emits TriggerHappened, AfterHappened`), which is the only
//! way to satisfy a dual-flag guard given the one-hot `event_*` flag scheme
//! (`domain_lowering.rs`, `generated `event_*` flags are one-hot per
//! transition`). That makes the fixture's compensation actually reachable,
//! so control 2 below is non-vacuous, not merely "never fires either way".

use std::path::{Path, PathBuf};
use std::process::Command;

use fsl_core::{FsResolver, build_model, domain_kernel_source, lower_domain, parse_kernel_source};
use fsl_runtime::{Monitor, bfs, verification_warnings};
use fsl_syntax::{DomainSpec, SurfaceDocument, parse_surface_document};
use serde_json::{Value, json};

fn repo_root() -> PathBuf {
    Path::new(env!("CARGO_MANIFEST_DIR"))
        .parent()
        .expect("rust/ directory")
        .parent()
        .expect("repository root")
        .to_path_buf()
}

fn fixture_source() -> String {
    std::fs::read_to_string(
        repo_root().join("rust/fslc/tests/fixtures/domain_saga_compensation_dual_guard.fsl"),
    )
    .expect("read dual-guard fixture")
}

fn fixture_domain() -> DomainSpec {
    match parse_surface_document(&fixture_source()).expect("parse dual-guard fixture") {
        SurfaceDocument::Domain(domain) => domain,
        _ => panic!("expected a domain document"),
    }
}

const OBSERVE_TRIGGER: &str = "saga_compensate_both_observe_trigger_happened";
const TRIGGER_ACTION: &str = "order_trigger";
const COMPENSATE_ACTION: &str =
    "saga_compensate_both_compensate_trigger_happened_after_after_happened";

/// Control 1 (rejecting, mandated by #713): a trace that observes ONLY the
/// trigger event, then attempts the compensation action, must be rejected on
/// a guard failure because the `after_event` flag is still false. Reverting
/// the dual-guard fix (leaving only the trigger `requires`) makes the Monitor
/// accept this trace.
#[test]
fn compensation_rejects_when_only_trigger_event_was_observed() {
    let domain = fixture_domain();
    let kernel = lower_domain(&domain).expect("lower dual-guard fixture (path A)");
    let model = build_model(kernel).expect("build dual-guard model (path A)");
    let mut monitor = Monitor::new(model).expect("initialize monitor");

    let observed = monitor
        .attempt(OBSERVE_TRIGGER, &std::collections::BTreeMap::new())
        .expect("observe trigger action must not error");
    assert!(
        observed.violation.is_none(),
        "observing the trigger event alone must not violate anything: {:?}",
        observed.violation
    );

    let compensated = monitor
        .attempt(COMPENSATE_ACTION, &std::collections::BTreeMap::new())
        .expect("attempting the compensation action must not error");
    let violation = compensated
        .violation
        .as_ref()
        .expect("compensation must be rejected when the after_event was never observed");
    assert_eq!(violation.kind, "requires_failed");
}

/// Control 2 (accepting, non-vacuity): a trace that fires the dual-emit
/// `decide` (setting both event flags in a single transition), then attempts
/// the compensation action, must be accepted, and the post-state must show
/// the compensation's own emitted event flag true. This proves the dual
/// guard is satisfiable, not structurally disabled by construction.
#[test]
fn compensation_accepts_when_both_events_were_observed_in_one_transition() {
    let domain = fixture_domain();
    let kernel = lower_domain(&domain).expect("lower dual-guard fixture (path A)");
    let model = build_model(kernel).expect("build dual-guard model (path A)");
    let mut monitor = Monitor::new(model).expect("initialize monitor");

    let triggered = monitor
        .attempt(TRIGGER_ACTION, &std::collections::BTreeMap::new())
        .expect("trigger action must not error");
    assert!(
        triggered.violation.is_none(),
        "the dual-emit decide must not violate anything: {:?}",
        triggered.violation
    );

    let compensated = monitor
        .attempt(COMPENSATE_ACTION, &std::collections::BTreeMap::new())
        .expect("attempting the compensation action must not error");
    assert!(
        compensated.violation.is_none(),
        "compensation must be accepted once both events were observed: {:?}",
        compensated.violation
    );
    assert_eq!(
        compensated.state.get("event_Released"),
        Some(&fsl_core::FslValue::Bool(true)),
        "the compensation's own emitted event flag must be set post-state"
    );
}

/// Control 3 (path-B textual control with calibration, the #708 pattern):
/// the rendered kernel text for the compensation action block must contain
/// BOTH `requires` lines. Then, to prove the control actually detects the
/// historical fault (not merely passes), the after-event `requires` line is
/// mechanically stripped out of the rendered text, the weakened text is
/// re-parsed and built into a model, and the SAME rejecting trace from
/// control 1 is replayed against it -- the weakened model must ACCEPT the
/// bad trace, reproducing the pre-fix defect.
#[test]
fn rendered_kernel_source_carries_both_requires_and_calibration_detects_their_absence() {
    let domain = fixture_domain();
    let source = domain_kernel_source(&domain).expect("render dual-guard fixture (path B)");

    let block_start = source
        .find(&format!("action {COMPENSATE_ACTION}("))
        .unwrap_or_else(|| panic!("rendered kernel is missing action {COMPENSATE_ACTION}"));
    let block_end = source[block_start..]
        .find('}')
        .map(|offset| block_start + offset)
        .expect("compensation action block must close");
    let block = &source[block_start..block_end];

    assert!(
        block.contains("requires event_TriggerHappened"),
        "compensation action block is missing the trigger requires: {block}"
    );
    assert!(
        block.contains("requires event_AfterHappened"),
        "compensation action block is missing the after requires: {block}"
    );

    // Calibration: mechanically remove the after-event requires line and
    // prove the weakened model reproduces the pre-fix, historically-real
    // false accept.
    let after_requires_line = block
        .lines()
        .find(|line| line.trim() == "requires event_AfterHappened")
        .expect("locate the exact after-event requires line to strip");
    let weakened_source = source.replacen(after_requires_line, "", 1);
    assert_ne!(
        weakened_source, source,
        "the replacen calibration step must actually remove a line"
    );

    let weakened_kernel = parse_kernel_source(&weakened_source, &FsResolver::new("."))
        .expect("parse weakened kernel source");
    let weakened_model = build_model(weakened_kernel).expect("build weakened model");
    let mut weakened_monitor = Monitor::new(weakened_model).expect("initialize weakened monitor");

    let observed = weakened_monitor
        .attempt(OBSERVE_TRIGGER, &std::collections::BTreeMap::new())
        .expect("observe trigger action must not error on weakened model");
    assert!(observed.violation.is_none());

    let compensated = weakened_monitor
        .attempt(COMPENSATE_ACTION, &std::collections::BTreeMap::new())
        .expect("attempting compensation must not error on weakened model");
    assert!(
        compensated.violation.is_none(),
        "calibration failed: the weakened (trigger-only-guarded) model must \
         reproduce the pre-fix false accept, but it still rejected: {:?}",
        compensated.violation
    );
}

fn run_cli(args: &[&str]) -> (i32, Value) {
    let output = Command::new(env!("CARGO_BIN_EXE_fslc"))
        .current_dir(repo_root())
        .args(args)
        .output()
        .expect("run native fslc");
    let value = serde_json::from_slice(&output.stdout).unwrap_or_else(|error| {
        panic!(
            "parse CLI JSON for {args:?}: {error}; stderr={}",
            String::from_utf8_lossy(&output.stderr)
        )
    });
    (output.status.code().expect("fslc exit code"), value)
}

/// Control 4 (interim-evidence control): `verify`ing the corpus saga whose
/// trigger and after events differ
/// (`examples/domain/order_fulfillment_saga.fsl`: `when PaymentFailed after
/// InventoryReserved`) must still reach the same verdict class, but its
/// warnings must now include the typed never-enabled warning naming the
/// compensation action -- because under one-hot `event_*` flags, a
/// trigger != `after_event` compensation is structurally disabled by the dual
/// guard (accepted interim state, `docs/DESIGN-saga-history.md:60-62`).
/// Reverting the fix removes this warning.
#[test]
fn order_fulfillment_saga_verify_surfaces_never_enabled_compensation_warning() {
    let (exit_code, output) = run_cli(&[
        "verify",
        "examples/domain/order_fulfillment_saga.fsl",
        "--depth",
        "3",
    ]);
    assert_eq!(exit_code, 0, "verify exit code: {output}");
    assert_eq!(output["result"], "verified", "verify verdict: {output}");

    let warnings = output["warnings"]
        .as_array()
        .expect("verify output carries a warnings array");
    let expected_action =
        "saga_order_fulfillment_compensate_payment_failed_after_inventory_reserved";
    let warning = warnings
        .iter()
        .find(|warning| warning["generated_name"] == expected_action)
        .unwrap_or_else(|| panic!("expected warning for '{expected_action}', got: {warnings:?}"));
    assert_eq!(warning["kind"], "never_enabled_action", "{warning}");
    assert!(warning["name"].is_string(), "{warning}");
    assert!(warning["origin"].is_object(), "{warning}");
    assert_eq!(
        warning["loc"],
        json!({"line": 102, "column": 7}),
        "the lowered action must report its authored compensation location: {warning}"
    );
    assert!(
        warning["message"]
            .as_str()
            .is_some_and(|message| message.contains("never enabled")),
        "expected a never-enabled warning: {warning}"
    );
}

/// Calibration for #728: this detector must stop reporting the compensation
/// action when the trigger-side event guard is mechanically removed. The
/// after-event still makes the weakened action reachable, so this is a real
/// detector-sensitivity control rather than merely asserting a warning exists.
#[test]
fn compensation_never_enabled_detector_changes_when_trigger_guard_is_removed() {
    let source_path = repo_root().join("examples/domain/order_fulfillment_saga.fsl");
    let source = std::fs::read_to_string(source_path).expect("read order saga fixture");
    let SurfaceDocument::Domain(domain) =
        parse_surface_document(&source).expect("parse order saga fixture")
    else {
        panic!("expected a domain document");
    };
    let kernel_source = domain_kernel_source(&domain).expect("render order saga kernel");
    let expected_action =
        "saga_order_fulfillment_compensate_payment_failed_after_inventory_reserved";
    let block_start = kernel_source
        .find(&format!("action {expected_action}("))
        .unwrap_or_else(|| panic!("rendered kernel is missing {expected_action}"));
    let block_end = kernel_source[block_start..]
        .find('}')
        .map(|offset| block_start + offset)
        .expect("compensation action block must close");
    let block = &kernel_source[block_start..block_end];
    let trigger_requires = block
        .lines()
        .find(|line| line.trim() == "requires event_PaymentFailed")
        .expect("locate trigger-side compensation guard");
    let weakened_block = block.replacen(trigger_requires, "", 1);
    let weakened_source = format!(
        "{}{}{}",
        &kernel_source[..block_start],
        weakened_block,
        &kernel_source[block_end..]
    );
    assert_ne!(
        weakened_source, kernel_source,
        "calibration must remove a guard"
    );

    let weakened = build_model(
        parse_kernel_source(&weakened_source, &FsResolver::new("."))
            .expect("parse weakened rendered kernel"),
    )
    .expect("build weakened rendered kernel");
    let result = bfs(weakened.clone(), 3).expect("explore weakened rendered kernel");
    assert_eq!(
        result.action_coverage.get(expected_action),
        Some(&true),
        "removing the trigger guard must make the after-event path cover the compensation"
    );
    let warnings = verification_warnings(
        &weakened,
        3,
        false,
        result.deadlock_step,
        None,
        &result.action_coverage,
        &[],
        false,
    );
    assert!(
        !warnings.iter().any(|warning| {
            warning["kind"] == "never_enabled_action"
                && warning["message"]
                    .as_str()
                    .is_some_and(|message| message.contains(expected_action))
        }),
        "the detector must clear once the action has coverage: {warnings:?}"
    );
}
