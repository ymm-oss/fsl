// SPDX-License-Identifier: Apache-2.0

//! #779: a saga step/timeout/compensation action that emits an event must
//! apply that event's declared `evolve` in the same action. Before this
//! fix, `lower_saga_actions` (`domain_lowering.rs`) and
//! `render_saga_actions` (`domain.rs`) called `event_assignments` for a
//! step/timeout/compensation action but never `evolve_items`
//! (`saga_emit_evolve`/`saga_emit_evolve_lines` after the fix), so the
//! event's one-hot flag flipped true while the aggregate state it was
//! declared to evolve stayed frozen at its initial value forever.
//!
//! `rust/fslc/tests/fixtures/issue_779_saga_emit_evolve_negative_controls.fsl`
//! is `examples/domain/order_fulfillment_saga.fsl` plus three invariants
//! that exist only to witness the fix (see the fixture's own header comment
//! for why they are not added to the corpus example itself: #772).

use std::path::{Path, PathBuf};
use std::process::Command;

use fsl_core::domain_kernel_source;
use fsl_syntax::{SurfaceDocument, parse_surface_document};
use serde_json::Value;

fn repo_root() -> PathBuf {
    Path::new(env!("CARGO_MANIFEST_DIR"))
        .parent()
        .expect("rust/ directory")
        .parent()
        .expect("repository root")
        .to_path_buf()
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

const FIXTURE: &str = "rust/fslc/tests/fixtures/issue_779_saga_emit_evolve_negative_controls.fsl";

/// Flip control 1 (mandated by #779, evidenced pre-fix via `git stash` on
/// `rust/fsl-core/src/domain_lowering.rs`/`domain.rs`/`lib.rs`:
/// `--engine induction --property Order_inventoryNeverReservationPending`
/// reports `"proved"` before this fix, because
/// `saga_order_fulfillment_reserve_inventory` sets
/// `event_InventoryReservationRequested := true` without ever writing
/// `order_inventory_status`, so `ReservationPending` is unreachable by
/// construction). After the fix it must flip to `"violated"`, reachable via
/// `order_approve_order` -> `saga_order_fulfillment_reserve_inventory`.
#[test]
fn inventory_reservation_pending_flips_to_violated() {
    let (status, output) = run_cli(&[
        "verify",
        FIXTURE,
        "--engine",
        "induction",
        "--property",
        "Order_inventoryNeverReservationPending",
    ]);
    assert_eq!(status, 1, "{output:#}");
    assert_eq!(output["result"], "violated", "{output:#}");
}

/// Flip control 2 (mandated by #779): same shape as control 1, for the
/// independent `PaymentCaptureRequested` evolve
/// (`saga_order_fulfillment_capture_payment`). Proved pre-fix, must flip to
/// violated post-fix, reachable via `order_approve_order` ->
/// `saga_order_fulfillment_reserve_inventory` ->
/// `saga_order_fulfillment_observe_inventory_reserved` ->
/// `saga_order_fulfillment_capture_payment`.
#[test]
fn payment_pending_flips_to_violated() {
    let (status, output) = run_cli(&[
        "verify",
        FIXTURE,
        "--engine",
        "induction",
        "--property",
        "Order_paymentNeverPaymentPending",
    ]);
    assert_eq!(status, 1, "{output:#}");
    assert_eq!(output["result"], "violated", "{output:#}");
}

/// Non-flip control (mandated by #779, "the control that must NOT flip"):
/// `inventory_status != ReleaseRequested` must stay `"proved"` both before
/// and after this fix. The compensation action's one-hot dual guard
/// (`requires event_PaymentFailed` and `requires event_InventoryReserved`,
/// mutually exclusive one-hot flags under `#713`/PR #725's dual guard) keeps
/// `InventoryReleaseRequested` structurally unreachable until #679 changes
/// the guard shape; #779 only adds the missing `evolve` call and must not
/// touch that guard. Without this control, a fix that (wrongly) also
/// loosened the compensation guard would not be caught by the two flip
/// controls above.
///
/// `--k 2` (rather than the default `--k 1`): post-fix, the two newly
/// reachable evolves (`ReservationPending`, `PaymentPending`) widen the
/// aggregate's reachable state space (#679's design-authority ruling on
/// this issue: 825 states after #779, static upper bound), so `k=1`
/// induction can no longer discharge this property on its own
/// (`unknown_cti`) even though it is still true; `k=2` proves it. This is
/// expected induction-depth growth from the state space widening the fix
/// itself causes, not a guard change -- verified directly: pre-fix, `k=1`
/// alone already proves this property (smaller reachable set); post-fix,
/// `k=1` returns `unknown_cti` and `k=2` returns `proved`.
#[test]
fn inventory_release_requested_stays_proved() {
    let (status, output) = run_cli(&[
        "verify",
        FIXTURE,
        "--engine",
        "induction",
        "--property",
        "Order_inventoryNeverReleaseRequested",
        "--k",
        "2",
    ]);
    assert_eq!(status, 0, "{output:#}");
    assert_eq!(output["result"], "proved", "{output:#}");
}

/// Positive structural evidence on the real corpus example (not the
/// fixture): rendering `examples/domain/order_fulfillment_saga.fsl` through
/// `domain_kernel_source` (path B -- the same renderer `domain expand` and
/// `check_domain` use in production) must show every one of the six
/// previously-broken saga step/timeout/compensation actions writing the
/// aggregate state its emitted event's declared `evolve` names, in the same
/// action block that sets the event flag.
#[test]
fn order_fulfillment_saga_expand_applies_every_saga_emit_evolve() {
    let source_path = repo_root().join("examples/domain/order_fulfillment_saga.fsl");
    let text = std::fs::read_to_string(&source_path).expect("read order_fulfillment_saga.fsl");
    let SurfaceDocument::Domain(domain) =
        parse_surface_document(&text).expect("parse order_fulfillment_saga.fsl")
    else {
        panic!("expected a domain document");
    };
    let source = domain_kernel_source(&domain).expect("render order_fulfillment_saga.fsl");

    let expectations: &[(&str, &str)] = &[
        (
            "action saga_order_fulfillment_reserve_inventory(",
            "order_inventory_status = InventoryStatus_ReservationPending",
        ),
        (
            "action saga_order_fulfillment_reserve_inventory_timeout(",
            "order_inventory_status = InventoryStatus_Failed",
        ),
        (
            "action saga_order_fulfillment_capture_payment(",
            "order_payment_status = PaymentStatus_PaymentPending",
        ),
        (
            "action saga_order_fulfillment_capture_payment_timeout(",
            "order_payment_status = PaymentStatus_TimedOut",
        ),
        (
            "action saga_order_fulfillment_ship_order(",
            "order_status = OrderStatus_Approved",
        ),
        (
            "action saga_order_fulfillment_compensate_payment_failed_after_inventory_reserved(",
            "order_inventory_status = InventoryStatus_ReleaseRequested",
        ),
    ];

    for (action_header, expected_assignment) in expectations {
        let block_start = source.find(action_header).unwrap_or_else(|| {
            panic!("rendered kernel is missing action block '{action_header}': {source}")
        });
        let block_end = source[block_start..]
            .find('}')
            .map(|offset| block_start + offset)
            .expect("action block must close");
        let block = &source[block_start..block_end];
        assert!(
            block.contains(expected_assignment),
            "action block '{action_header}' is missing '{expected_assignment}': {block}"
        );
    }
}
