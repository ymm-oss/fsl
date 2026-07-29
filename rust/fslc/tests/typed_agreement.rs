// SPDX-License-Identifier: Apache-2.0
// Copyright 2026 Ryoichi Izumita

//! C6 typed generative / metamorphic cross-engine agreement suite (#537 C6
//! slice 1, issue #648).
//!
//! Generates checked `KernelModel`s (never string fuzz) from a deterministic
//! structural axis enumeration (`generator.rs`), compares Monitor BFS /
//! explicit / BMC bounded verdicts and successors (`engines.rs`), and checks
//! seven metamorphic relations with a negative control each (`relations.rs`).
//! `sweep_summary.rs` aggregates what each sweep actually exercised.
//!
//! See `docs/DESIGN-conformance-harness.md`'s "Typed generative /
//! metamorphic agreement (#537 C6)" section for the accepted design this
//! implements, including why Z3js/Worker parity is out of scope here.

#[path = "typed_agreement/engines.rs"]
mod engines;
#[path = "typed_agreement/generator.rs"]
mod generator;
#[path = "typed_agreement/relations.rs"]
mod relations;
#[path = "typed_agreement/sweep_summary.rs"]
mod sweep_summary;

use generator::{PropertyKind, domain_sweep, operation_sweep};
use sweep_summary::SweepSummary;

/// Generator floor, asserted per the brief and design's "assert model
/// count is at least N" requirement: `domain_axis` has 15 `(kind, size)`
/// pairs (S2's four scalar domain kinds), so anything below that means the
/// axis enumeration itself regressed.
const DOMAIN_SWEEP_FLOOR: usize = 15;
/// `divide`/`remainder` guarded-action-context plus property-context
/// entries. `head`/`pop`/`at`/index and the unguarded divide/remainder
/// action-context boundary are exercised as dedicated `relations.rs` R6
/// tests instead of this sweep; see `generator.rs::operation_sweep`'s doc.
const OPERATION_SWEEP_FLOOR: usize = 4;

#[test]
fn domain_sweep_meets_its_generator_floor_and_covers_every_property_kind() {
    let models = domain_sweep();
    assert!(
        models.len() >= DOMAIN_SWEEP_FLOOR,
        "domain sweep floor: expected >= {DOMAIN_SWEEP_FLOOR}, got {}",
        models.len()
    );
    for kind in [
        PropertyKind::Invariant,
        PropertyKind::Reachable,
        PropertyKind::LeadsTo,
        PropertyKind::Trans,
        PropertyKind::Terminal,
    ] {
        assert!(
            models.iter().any(|model| model.property_kind == kind),
            "domain sweep must exercise property kind '{}' at least once",
            kind.label()
        );
    }
}

#[test]
fn operation_sweep_meets_its_generator_floor() {
    let models = operation_sweep();
    assert!(
        models.len() >= OPERATION_SWEEP_FLOOR,
        "operation sweep floor: expected >= {OPERATION_SWEEP_FLOOR}, got {}",
        models.len()
    );
}

/// The main sweep: every domain-axis model must build and its Monitor
/// BFS / explicit / BMC verdicts must agree (`engines::run_agreement`
/// panics on disagreement, so a clean run here already *is* the "zero
/// cross-engine disagreements" evidence the brief asks to report).
#[test]
fn domain_sweep_agrees_across_all_three_engines() {
    let mut summary = SweepSummary::default();
    for model in domain_sweep() {
        let built = engines::build(&model.id, &model.source);
        engines::run_agreement(&model.id, &built, model.depth);
        summary.record_domain_model(
            model.domain_kind.label(),
            model.domain_size,
            model.property_kind.label(),
            model.state_vars,
            model.action_count,
            model.guarded,
            model.fair,
        );
    }
    eprintln!("domain sweep summary: {summary}");
}

#[test]
fn operation_sweep_agrees_across_all_three_engines() {
    let mut summary = SweepSummary::default();
    for model in operation_sweep() {
        let built = engines::build(&model.id, &model.source);
        engines::run_agreement(&model.id, &built, model.depth);
        summary.record_operation_model(model.operation, model.context);
    }
    eprintln!("operation sweep summary: {summary}");
}
