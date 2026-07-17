// SPDX-License-Identifier: Apache-2.0
// Copyright 2026 Ryoichi Izumita

//! Coverage registry for the RCIR v1 projector (issue #328): an explicit,
//! inspectable table of every authored semantic target *kind* the projector
//! recognizes, cross-referencing FSL's semantic element kinds (action /
//! property / trace kind / ...) against whether RCIR v1 renders them as a
//! claim or reports them as an unsupported target. `document_project.rs`'s
//! own runtime invariant already fails closed if a projected target isn't
//! classified into `rendered`/`unattributed`/`unsupported`; this registry
//! makes the *set of kinds that classification can ever produce* an
//! explicit, statically checkable fact, so a newly added FSL construct
//! cannot ship with silently unmeasured document coverage. See
//! `rust/fsl-tools/tests/document_coverage.rs` for the tests that keep this
//! registry in sync with the Public Kernel v1 schema and the fixture
//! corpus, and `docs/DESIGN-document-coverage-registry.md` for the design.

/// Whether RCIR v1 renders a semantic target kind as a claim, or reports it
/// as an unsupported target (never both, and never silently dropped).
#[derive(Clone, Copy, Debug, Eq, PartialEq)]
pub enum TargetTreatment {
    Rendered,
    Unsupported,
}

pub struct TargetKindRow {
    /// The prefix `crate::document_coverage::target_kind` extracts from a
    /// raw semantic-target string (e.g. `"action"`, `"property:invariant"`).
    pub kind: &'static str,
    pub treatment: TargetTreatment,
    pub description: &'static str,
}

/// Every semantic-target kind `document_project.rs` can produce. Adding a
/// new authored-element kind to the projector without adding a row here is
/// exactly the silent-omission failure mode this issue closes: the
/// coupled-change tests in `document_coverage.rs` fail until a row is added.
pub const RCIR_TARGET_KIND_REGISTRY: &[TargetKindRow] = &[
    TargetKindRow {
        kind: "action",
        treatment: TargetTreatment::Rendered,
        description: "an executable operation -> an `operation` claim",
    },
    TargetKindRow {
        kind: "property:invariant",
        treatment: TargetTreatment::Rendered,
        description: "a state invariant -> a `state_rule` claim (or a `deadline_rule` claim \
                       for a requirements-dialect deadline invariant)",
    },
    TargetKindRow {
        kind: "property:trans",
        treatment: TargetTreatment::Rendered,
        description: "a transition rule -> a `transition_rule` claim",
    },
    TargetKindRow {
        kind: "property:reachable",
        treatment: TargetTreatment::Rendered,
        description: "a reachability goal -> a `reachability_goal` claim",
    },
    TargetKindRow {
        kind: "property:leadsTo",
        treatment: TargetTreatment::Rendered,
        description: "a progress/liveness property -> a `progress_rule` claim",
    },
    TargetKindRow {
        kind: "terminal",
        treatment: TargetTreatment::Rendered,
        description: "the terminal-state expression -> a `terminal_rule` claim",
    },
    TargetKindRow {
        kind: "acceptance",
        treatment: TargetTreatment::Rendered,
        description: "a requirements acceptance trace case -> an `acceptance_trace` claim",
    },
    TargetKindRow {
        kind: "forbidden",
        treatment: TargetTreatment::Rendered,
        description: "a requirements forbidden trace case -> a `forbidden_trace` claim",
    },
    TargetKindRow {
        kind: "init",
        treatment: TargetTreatment::Unsupported,
        description: "no v1 claim kind projects initial-state definitions",
    },
    TargetKindRow {
        kind: "projection",
        treatment: TargetTreatment::Unsupported,
        description: "a KPI projection is a business-layer artifact; not projected in RCIR v1",
    },
    TargetKindRow {
        kind: "refinement",
        treatment: TargetTreatment::Unsupported,
        description: "the `implements` correspondence belongs to the refinement contract, not \
                       the requirement claim projection, in v1",
    },
];

/// Extract the kind prefix `RCIR_TARGET_KIND_REGISTRY` classifies a raw
/// semantic-target string by: `"action:submit_cancel"` -> `"action"`,
/// `"property:invariant:NoReopen"` -> `"property:invariant"`,
/// `"acceptance:AC-1"` -> `"acceptance"`, `"terminal"`/`"init"` -> unchanged.
#[must_use]
pub fn target_kind(target: &str) -> String {
    if let Some(rest) = target.strip_prefix("property:") {
        let sub = rest.split(':').next().unwrap_or(rest);
        format!("property:{sub}")
    } else {
        target.split(':').next().unwrap_or(target).to_owned()
    }
}
