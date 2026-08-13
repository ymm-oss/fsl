// SPDX-License-Identifier: Apache-2.0

//! Cross-command error-envelope parity for issue #781.
//!
//! `rust/fslc/src/literate_access.rs` owns a different question: whether a
//! command accepts literate Markdown input.  This file owns the independent
//! question of whether sibling command entry points preserve the same error
//! envelope after they do accept an FSL-shaped input.  Its registry is
//! deliberately not shared with the literate registry: a command can be in
//! scope here yet have a different literate-input policy.
//!
//! Known differences are pinned rather than normalized away.  Each pin names
//! its tracking issue and asserts both the current observation and the
//! post-fix uniform expectation.  When a fix makes a pin uniform, the test
//! fails loudly so the entry must be deliberately moved into the uniform
//! matrix.

use std::collections::BTreeSet;
use std::path::{Path, PathBuf};
use std::process::Command;

use serde_json::Value;

const SPEC_PLACEHOLDER: &str = "{spec}";
const PARSE_KERNEL_FIXTURE: &str = "examples/gallery/errors/parse_missing_expression.fsl";
const PARSE_DOMAIN_FIXTURE: &str =
    "rust/fslc/tests/fixtures/domain_characterization/invalid_broken_expression.fsl";
const PARSE_DB_FIXTURE: &str = "rust/fslc/tests/fixtures/error_envelope_broken_dbsystem.fsl";
const PARSE_AI_FIXTURE: &str = "rust/fslc/tests/fixtures/error_envelope_broken_ai_component.fsl";
const PARSE_CAUSAL_FIXTURE: &str = "rust/fslc/tests/fixtures/error_envelope_broken_causal.fsl";
const GUARD_FIXTURE: &str = "rust/fslc/tests/fixtures/domain_await_routing_rejected.fsl";
const NAME_FIXTURE: &str =
    "rust/fslc/tests/fixtures/domain_characterization/invalid_unknown_name.fsl";
const LITERATE_FIXTURE: &str = "examples/literate/toggle.md";
const DOMAIN_REPLAY_LOG: &str = "rust/fslc/tests/fixtures/issue_518_clean.jsonl";

/// The one ownership classification for every leaf in `cli-contract.json`.
///
/// A `SpecPath` invocation contains [`SPEC_PLACEHOLDER`] at the shared FSL
/// frontend input position.  Such a command cannot be excluded from this
/// matrix merely because it produces a profile or has command-specific
/// semantics.  An `Excluded` command is still deliberate: its primary input
/// is a manifest, generated artifact, evidence log, records, or a `--kind`-
/// selected contract that does not reach that frontend path.
#[derive(Clone, Copy, Debug)]
enum ParityScope {
    SpecPath { invoke: &'static [&'static str] },
    Excluded { reason: &'static str },
}

#[derive(Clone, Copy, Debug, PartialEq, Eq)]
enum LiterateCoverage {
    /// #665's already-uniform fail-closed commands.
    UniformUnsupported,
    /// The command materializes Markdown and has success-path coverage
    /// elsewhere; it is not an error-envelope cell.
    Supported,
    /// This command's Markdown behavior is one of #694's pinned dialect
    /// asymmetries.
    PinnedDialect,
    NotApplicable,
}

#[derive(Clone, Copy, Debug)]
struct FailureCoverage {
    class: FailureClass,
    fixture: &'static str,
    uniform: Expectation,
}

struct CommandRegistration {
    key: &'static str,
    scope: ParityScope,
    literate: LiterateCoverage,
    coverage: &'static [FailureCoverage],
}

const PARITY_REGISTRY: &[CommandRegistration] = &[
    CommandRegistration {
        key: "ai check",
        scope: ParityScope::SpecPath {
            invoke: &["ai", "check", SPEC_PLACEHOLDER],
        },
        literate: LiterateCoverage::PinnedDialect,
        coverage: AI_PARSE_COVERAGE,
    },
    CommandRegistration {
        key: "ai compare",
        scope: ParityScope::Excluded {
            reason: "compares two precomputed evaluation-record inputs",
        },
        literate: LiterateCoverage::NotApplicable,
        coverage: NO_COVERAGE,
    },
    CommandRegistration {
        key: "ai compat",
        scope: ParityScope::SpecPath {
            invoke: &["ai", "compat", SPEC_PLACEHOLDER],
        },
        literate: LiterateCoverage::PinnedDialect,
        coverage: AI_PARSE_COVERAGE,
    },
    CommandRegistration {
        key: "ai drift",
        scope: ParityScope::Excluded {
            reason: "requires runtime telemetry logs in addition to the component",
        },
        literate: LiterateCoverage::NotApplicable,
        coverage: NO_COVERAGE,
    },
    CommandRegistration {
        key: "ai eval",
        scope: ParityScope::Excluded {
            reason: "evaluates precomputed AI evaluation records",
        },
        literate: LiterateCoverage::NotApplicable,
        coverage: NO_COVERAGE,
    },
    CommandRegistration {
        key: "ai regress",
        scope: ParityScope::Excluded {
            reason: "compares before/after record sets for an AI migration",
        },
        literate: LiterateCoverage::NotApplicable,
        coverage: NO_COVERAGE,
    },
    CommandRegistration {
        key: "ai replay",
        scope: ParityScope::Excluded {
            reason: "requires AI runtime JSONL evidence in addition to the component",
        },
        literate: LiterateCoverage::NotApplicable,
        coverage: NO_COVERAGE,
    },
    CommandRegistration {
        key: "analyze",
        scope: ParityScope::SpecPath {
            invoke: &["analyze", SPEC_PLACEHOLDER],
        },
        literate: LiterateCoverage::UniformUnsupported,
        coverage: NO_COVERAGE,
    },
    CommandRegistration {
        key: "approval check",
        scope: ParityScope::Excluded {
            reason: "its spec-shaped positional is selected by --kind and may legitimately be Markdown",
        },
        literate: LiterateCoverage::NotApplicable,
        coverage: NO_COVERAGE,
    },
    CommandRegistration {
        key: "approval create",
        scope: ParityScope::Excluded {
            reason: "its spec-shaped positional is selected by --kind and may legitimately be Markdown",
        },
        literate: LiterateCoverage::NotApplicable,
        coverage: NO_COVERAGE,
    },
    CommandRegistration {
        key: "approval diff",
        scope: ParityScope::Excluded {
            reason: "its spec-shaped positional is selected by --kind and may legitimately be Markdown",
        },
        literate: LiterateCoverage::NotApplicable,
        coverage: NO_COVERAGE,
    },
    CommandRegistration {
        key: "causal analyze",
        scope: ParityScope::Excluded {
            reason: "requires a causal projection/profile contract rather than a shared frontend input",
        },
        literate: LiterateCoverage::NotApplicable,
        coverage: NO_COVERAGE,
    },
    CommandRegistration {
        key: "causal check",
        scope: ParityScope::SpecPath {
            invoke: &["causal", "check", SPEC_PLACEHOLDER],
        },
        literate: LiterateCoverage::PinnedDialect,
        coverage: CAUSAL_PARSE_COVERAGE,
    },
    CommandRegistration {
        key: "causal diff",
        scope: ParityScope::Excluded {
            reason: "compares two causal models rather than one frontend input",
        },
        literate: LiterateCoverage::NotApplicable,
        coverage: NO_COVERAGE,
    },
    CommandRegistration {
        key: "causal ledger",
        scope: ParityScope::Excluded {
            reason: "projects a causal ledger whose evidence inputs alter the command contract",
        },
        literate: LiterateCoverage::NotApplicable,
        coverage: NO_COVERAGE,
    },
    CommandRegistration {
        key: "causal observe-expectations",
        scope: ParityScope::Excluded {
            reason: "requires an observation log, mapping, scope, and time window",
        },
        literate: LiterateCoverage::NotApplicable,
        coverage: NO_COVERAGE,
    },
    CommandRegistration {
        key: "causal verify-expectations",
        scope: ParityScope::SpecPath {
            invoke: &["causal", "verify-expectations", SPEC_PLACEHOLDER],
        },
        literate: LiterateCoverage::PinnedDialect,
        coverage: CAUSAL_PARSE_COVERAGE,
    },
    CommandRegistration {
        key: "chain",
        scope: ParityScope::Excluded {
            reason: "positional is an fsl-project.toml manifest, not an FSL source document",
        },
        literate: LiterateCoverage::NotApplicable,
        coverage: NO_COVERAGE,
    },
    CommandRegistration {
        key: "check",
        scope: ParityScope::SpecPath {
            invoke: &["check", SPEC_PLACEHOLDER],
        },
        literate: LiterateCoverage::Supported,
        coverage: CHECK_COVERAGE,
    },
    CommandRegistration {
        key: "compat check",
        scope: ParityScope::SpecPath {
            invoke: &["compat", "check", SPEC_PLACEHOLDER],
        },
        literate: LiterateCoverage::PinnedDialect,
        coverage: NO_COVERAGE,
    },
    CommandRegistration {
        key: "conformance",
        scope: ParityScope::SpecPath {
            invoke: &["conformance", SPEC_PLACEHOLDER],
        },
        literate: LiterateCoverage::UniformUnsupported,
        coverage: NO_COVERAGE,
    },
    CommandRegistration {
        key: "db check",
        scope: ParityScope::SpecPath {
            invoke: &["db", "check", SPEC_PLACEHOLDER],
        },
        literate: LiterateCoverage::PinnedDialect,
        coverage: DB_PARSE_COVERAGE,
    },
    CommandRegistration {
        key: "db import",
        scope: ParityScope::Excluded {
            reason: "imports SQL or ORM schema artifacts rather than an FSL frontend document",
        },
        literate: LiterateCoverage::NotApplicable,
        coverage: NO_COVERAGE,
    },
    CommandRegistration {
        key: "db observe",
        scope: ParityScope::Excluded {
            reason: "requires database runtime observation evidence",
        },
        literate: LiterateCoverage::NotApplicable,
        coverage: NO_COVERAGE,
    },
    CommandRegistration {
        key: "diff",
        scope: ParityScope::SpecPath {
            invoke: &["diff", SPEC_PLACEHOLDER],
        },
        literate: LiterateCoverage::UniformUnsupported,
        coverage: NO_COVERAGE,
    },
    CommandRegistration {
        key: "document check",
        scope: ParityScope::SpecPath {
            invoke: &["document", "check", SPEC_PLACEHOLDER],
        },
        literate: LiterateCoverage::UniformUnsupported,
        coverage: NO_COVERAGE,
    },
    CommandRegistration {
        key: "document claims",
        scope: ParityScope::SpecPath {
            invoke: &["document", "claims", SPEC_PLACEHOLDER],
        },
        literate: LiterateCoverage::UniformUnsupported,
        coverage: NO_COVERAGE,
    },
    CommandRegistration {
        key: "document generate",
        scope: ParityScope::SpecPath {
            invoke: &["document", "generate", SPEC_PLACEHOLDER],
        },
        literate: LiterateCoverage::UniformUnsupported,
        coverage: NO_COVERAGE,
    },
    CommandRegistration {
        key: "domain analyze",
        scope: ParityScope::SpecPath {
            invoke: &["domain", "analyze", SPEC_PLACEHOLDER],
        },
        literate: LiterateCoverage::NotApplicable,
        coverage: DOMAIN_COVERAGE,
    },
    CommandRegistration {
        key: "domain check",
        scope: ParityScope::SpecPath {
            invoke: &["domain", "check", SPEC_PLACEHOLDER],
        },
        literate: LiterateCoverage::PinnedDialect,
        coverage: DOMAIN_COVERAGE,
    },
    CommandRegistration {
        key: "domain expand",
        scope: ParityScope::SpecPath {
            invoke: &["domain", "expand", SPEC_PLACEHOLDER],
        },
        literate: LiterateCoverage::NotApplicable,
        coverage: DOMAIN_COVERAGE,
    },
    CommandRegistration {
        key: "domain generate",
        scope: ParityScope::SpecPath {
            invoke: &["domain", "generate", SPEC_PLACEHOLDER],
        },
        literate: LiterateCoverage::NotApplicable,
        coverage: DOMAIN_COVERAGE,
    },
    CommandRegistration {
        key: "domain replay",
        scope: ParityScope::SpecPath {
            invoke: &[
                "domain",
                "replay",
                SPEC_PLACEHOLDER,
                "--logs",
                DOMAIN_REPLAY_LOG,
            ],
        },
        literate: LiterateCoverage::NotApplicable,
        coverage: DOMAIN_COVERAGE,
    },
    CommandRegistration {
        key: "domain testgen",
        scope: ParityScope::SpecPath {
            invoke: &["domain", "testgen", SPEC_PLACEHOLDER],
        },
        literate: LiterateCoverage::NotApplicable,
        coverage: DOMAIN_COVERAGE,
    },
    CommandRegistration {
        key: "explain",
        scope: ParityScope::SpecPath {
            invoke: &["explain", SPEC_PLACEHOLDER],
        },
        literate: LiterateCoverage::UniformUnsupported,
        coverage: NO_COVERAGE,
    },
    CommandRegistration {
        key: "fmt",
        scope: ParityScope::SpecPath {
            invoke: &["fmt", SPEC_PLACEHOLDER],
        },
        literate: LiterateCoverage::UniformUnsupported,
        coverage: NO_COVERAGE,
    },
    CommandRegistration {
        key: "html",
        scope: ParityScope::SpecPath {
            invoke: &["html", SPEC_PLACEHOLDER],
        },
        literate: LiterateCoverage::UniformUnsupported,
        coverage: NO_COVERAGE,
    },
    CommandRegistration {
        key: "kernel",
        scope: ParityScope::SpecPath {
            invoke: &["kernel", SPEC_PLACEHOLDER],
        },
        literate: LiterateCoverage::UniformUnsupported,
        coverage: KERNEL_GUARD_COVERAGE,
    },
    CommandRegistration {
        key: "ledger",
        scope: ParityScope::SpecPath {
            invoke: &["ledger", SPEC_PLACEHOLDER],
        },
        literate: LiterateCoverage::UniformUnsupported,
        coverage: NO_COVERAGE,
    },
    CommandRegistration {
        key: "lint",
        scope: ParityScope::SpecPath {
            invoke: &["lint", SPEC_PLACEHOLDER],
        },
        literate: LiterateCoverage::UniformUnsupported,
        coverage: NO_COVERAGE,
    },
    CommandRegistration {
        key: "migrate",
        scope: ParityScope::SpecPath {
            invoke: &["migrate", SPEC_PLACEHOLDER],
        },
        literate: LiterateCoverage::UniformUnsupported,
        coverage: NO_COVERAGE,
    },
    CommandRegistration {
        key: "mutate",
        scope: ParityScope::SpecPath {
            invoke: &["mutate", SPEC_PLACEHOLDER],
        },
        literate: LiterateCoverage::UniformUnsupported,
        coverage: NO_COVERAGE,
    },
    CommandRegistration {
        key: "refine",
        scope: ParityScope::SpecPath {
            invoke: &[
                "refine",
                SPEC_PLACEHOLDER,
                SPEC_PLACEHOLDER,
                SPEC_PLACEHOLDER,
            ],
        },
        literate: LiterateCoverage::UniformUnsupported,
        coverage: NO_COVERAGE,
    },
    CommandRegistration {
        key: "replay",
        scope: ParityScope::SpecPath {
            invoke: &["replay", SPEC_PLACEHOLDER],
        },
        literate: LiterateCoverage::UniformUnsupported,
        coverage: NO_COVERAGE,
    },
    CommandRegistration {
        key: "scenarios",
        scope: ParityScope::SpecPath {
            invoke: &["scenarios", SPEC_PLACEHOLDER],
        },
        literate: LiterateCoverage::Supported,
        coverage: NO_COVERAGE,
    },
    CommandRegistration {
        key: "sweep",
        scope: ParityScope::SpecPath {
            invoke: &["sweep", SPEC_PLACEHOLDER],
        },
        literate: LiterateCoverage::UniformUnsupported,
        coverage: NO_COVERAGE,
    },
    CommandRegistration {
        key: "testgen",
        scope: ParityScope::SpecPath {
            invoke: &["testgen", SPEC_PLACEHOLDER],
        },
        literate: LiterateCoverage::UniformUnsupported,
        coverage: NO_COVERAGE,
    },
    CommandRegistration {
        key: "typestate",
        scope: ParityScope::SpecPath {
            invoke: &["typestate", SPEC_PLACEHOLDER],
        },
        literate: LiterateCoverage::UniformUnsupported,
        coverage: NO_COVERAGE,
    },
    CommandRegistration {
        key: "verify",
        scope: ParityScope::SpecPath {
            invoke: &["verify", SPEC_PLACEHOLDER],
        },
        literate: LiterateCoverage::Supported,
        coverage: VERIFY_COVERAGE,
    },
    CommandRegistration {
        key: "version",
        scope: ParityScope::Excluded {
            reason: "has no input path or frontend entry point",
        },
        literate: LiterateCoverage::NotApplicable,
        coverage: NO_COVERAGE,
    },
];

#[derive(Clone, Copy, Debug, PartialEq, Eq, PartialOrd, Ord)]
enum FailureClass {
    Parse,
    Guard,
    Name,
    Literate,
}

#[derive(Clone, Copy, Debug, PartialEq, Eq)]
enum Payload {
    Json,
    Text,
}

#[derive(Clone, Copy, Debug, PartialEq, Eq)]
enum Diagnostic {
    None,
    Code(&'static str),
    Alias(&'static str),
}

#[derive(Clone, Copy, Debug, PartialEq, Eq)]
enum LocationShape {
    Absent,
    LineColumn,
    FileOnly,
}

#[derive(Clone, Copy, Debug, PartialEq, Eq)]
struct Expectation {
    payload: Payload,
    result: Option<&'static str>,
    kind: Option<&'static str>,
    location: LocationShape,
    diagnostic: Diagnostic,
    exit: i32,
    dialect: Option<&'static str>,
    message_mentions_input: Option<bool>,
    exact_stdout: Option<&'static str>,
}

const PARSE_UNIFORM: Expectation = Expectation {
    payload: Payload::Json,
    result: Some("error"),
    kind: Some("parse"),
    location: LocationShape::LineColumn,
    diagnostic: Diagnostic::Code("FSL-PARSE"),
    exit: 2,
    dialect: None,
    message_mentions_input: None,
    exact_stdout: None,
};

const SEMANTIC_UNIFORM: Expectation = Expectation {
    payload: Payload::Json,
    result: Some("error"),
    kind: Some("semantics"),
    location: LocationShape::LineColumn,
    diagnostic: Diagnostic::None,
    exit: 2,
    dialect: None,
    message_mentions_input: None,
    exact_stdout: None,
};

const SEMANTIC_WITH_INPUT_PATH: Expectation = Expectation {
    message_mentions_input: Some(true),
    ..SEMANTIC_UNIFORM
};

const SEMANTIC_WITHOUT_LOCATION: Expectation = Expectation {
    location: LocationShape::Absent,
    ..SEMANTIC_UNIFORM
};

const PARSE_WITH_DIAGNOSTIC_ALIAS: Expectation = Expectation {
    diagnostic: Diagnostic::Alias("parse"),
    ..PARSE_UNIFORM
};

const LITERATE_UNIFORM: Expectation = Expectation {
    payload: Payload::Json,
    result: Some("error"),
    kind: Some("usage"),
    location: LocationShape::FileOnly,
    diagnostic: Diagnostic::Code("FSL-INPUT-LITERATE-UNSUPPORTED"),
    exit: 2,
    dialect: None,
    message_mentions_input: None,
    exact_stdout: None,
};

/// #796 pins a product false negative: an invalid spec exits 0 and is accepted.
/// This is a soundness defect, not an endorsed behavior; the pin detects
/// regressions or changes until the first follow-up queue item, #796, fixes it.
/// It may share a root with `KNOWN_DIVERGENT_DOMAIN_FIXTURES` entry 1 (#690):
/// #690 is closed, but its symptom 2 divergence remained live when measured
/// on 2026-08-13 and is now tracked by open #798.
const ANALYZED_NAME_FALSE_GREEN: Expectation = Expectation {
    payload: Payload::Json,
    result: Some("analyzed"),
    kind: None,
    location: LocationShape::Absent,
    diagnostic: Diagnostic::None,
    exit: 0,
    dialect: Some("fsl-domain-effect.v0"),
    message_mentions_input: None,
    exact_stdout: None,
};

/// See [`ANALYZED_NAME_FALSE_GREEN`]: this is the same #796 false-negative
/// soundness defect and is pinned solely to detect change, not to approve it.
const EXPANDED_NAME_FALSE_GREEN: Expectation = Expectation {
    payload: Payload::Text,
    result: None,
    kind: None,
    location: LocationShape::Absent,
    diagnostic: Diagnostic::None,
    exit: 0,
    dialect: None,
    message_mentions_input: None,
    exact_stdout: Some(
        r#"spec InvalidUnknownName "domain: generated from fsl-domain/fsl-effect" {
  enum Status { Status_Draft, Status_Approved }
  state {
    order_status: Status,
    event_Touched: Bool,
  }
  init {
    order_status = Status_Draft
    event_Touched = false
  }
  action order_touch() {
    event_Touched = true
    order_status = order_status
  }
  invariant Order_unknownName "DOMAIN-INVARIANT: Order.unknownName" { missing_status == Status_Draft }
  terminal { false }
}
"#,
    ),
};

const CAUSAL_PARSE_WITHOUT_DIAGNOSTIC: Expectation = Expectation {
    diagnostic: Diagnostic::None,
    ..PARSE_UNIFORM
};

const NO_COVERAGE: &[FailureCoverage] = &[];
const AI_PARSE_COVERAGE: &[FailureCoverage] = &[FailureCoverage {
    class: FailureClass::Parse,
    fixture: PARSE_AI_FIXTURE,
    uniform: PARSE_UNIFORM,
}];
const CAUSAL_PARSE_COVERAGE: &[FailureCoverage] = &[FailureCoverage {
    class: FailureClass::Parse,
    fixture: PARSE_CAUSAL_FIXTURE,
    uniform: PARSE_UNIFORM,
}];
const CHECK_COVERAGE: &[FailureCoverage] = &[
    FailureCoverage {
        class: FailureClass::Parse,
        fixture: PARSE_KERNEL_FIXTURE,
        uniform: PARSE_UNIFORM,
    },
    FailureCoverage {
        class: FailureClass::Guard,
        fixture: GUARD_FIXTURE,
        uniform: SEMANTIC_UNIFORM,
    },
    FailureCoverage {
        class: FailureClass::Name,
        fixture: NAME_FIXTURE,
        uniform: SEMANTIC_UNIFORM,
    },
];
const VERIFY_COVERAGE: &[FailureCoverage] = CHECK_COVERAGE;
const DB_PARSE_COVERAGE: &[FailureCoverage] = &[FailureCoverage {
    class: FailureClass::Parse,
    fixture: PARSE_DB_FIXTURE,
    uniform: PARSE_UNIFORM,
}];
const DOMAIN_COVERAGE: &[FailureCoverage] = &[
    FailureCoverage {
        class: FailureClass::Parse,
        fixture: PARSE_DOMAIN_FIXTURE,
        uniform: PARSE_UNIFORM,
    },
    FailureCoverage {
        class: FailureClass::Guard,
        fixture: GUARD_FIXTURE,
        uniform: SEMANTIC_UNIFORM,
    },
    FailureCoverage {
        class: FailureClass::Name,
        fixture: NAME_FIXTURE,
        uniform: SEMANTIC_UNIFORM,
    },
];
const KERNEL_GUARD_COVERAGE: &[FailureCoverage] = &[FailureCoverage {
    class: FailureClass::Guard,
    fixture: GUARD_FIXTURE,
    uniform: SEMANTIC_UNIFORM,
}];

struct KnownAsymmetry {
    class: FailureClass,
    command: &'static str,
    observed: Expectation,
    uniform: Expectation,
    issue: &'static str,
    resolution: &'static str,
}

/// Existing nonuniform output is pinned rather than allowlisted.  The
/// `uniform` member is deliberately distinct from `observed`: when a fix
/// reaches it, [`assert_known_asymmetry`] tells the maintainer to move the
/// cell into the uniform table instead of silently retaining stale debt.
const KNOWN_ASYMMETRIES: &[KnownAsymmetry] = &[
    KnownAsymmetry {
        class: FailureClass::Parse,
        command: "domain check",
        observed: SEMANTIC_WITHOUT_LOCATION,
        uniform: PARSE_UNIFORM,
        issue: "#780",
        resolution: "fix completed; move this entry to the uniform table",
    },
    KnownAsymmetry {
        class: FailureClass::Parse,
        command: "domain analyze",
        observed: SEMANTIC_WITHOUT_LOCATION,
        uniform: PARSE_UNIFORM,
        issue: "#780",
        resolution: "fix completed; move this entry to the uniform table",
    },
    KnownAsymmetry {
        class: FailureClass::Parse,
        command: "domain expand",
        observed: SEMANTIC_WITHOUT_LOCATION,
        uniform: PARSE_UNIFORM,
        issue: "#780",
        resolution: "fix completed; move this entry to the uniform table",
    },
    KnownAsymmetry {
        class: FailureClass::Parse,
        command: "domain generate",
        observed: SEMANTIC_WITHOUT_LOCATION,
        uniform: PARSE_UNIFORM,
        issue: "#780",
        resolution: "fix completed; move this entry to the uniform table",
    },
    KnownAsymmetry {
        class: FailureClass::Parse,
        command: "domain replay",
        observed: SEMANTIC_WITHOUT_LOCATION,
        uniform: PARSE_UNIFORM,
        issue: "#780",
        resolution: "fix completed; move this entry to the uniform table",
    },
    KnownAsymmetry {
        class: FailureClass::Parse,
        command: "domain testgen",
        observed: SEMANTIC_WITHOUT_LOCATION,
        uniform: PARSE_UNIFORM,
        issue: "#780",
        resolution: "fix completed; move this entry to the uniform table",
    },
    KnownAsymmetry {
        class: FailureClass::Parse,
        command: "db check",
        observed: SEMANTIC_WITHOUT_LOCATION,
        uniform: PARSE_UNIFORM,
        issue: "#780",
        resolution: "fix completed; move this entry to the uniform table",
    },
    KnownAsymmetry {
        class: FailureClass::Parse,
        command: "ai check",
        observed: SEMANTIC_WITHOUT_LOCATION,
        uniform: PARSE_UNIFORM,
        issue: "#780",
        resolution: "fix completed; move this entry to the uniform table",
    },
    KnownAsymmetry {
        class: FailureClass::Parse,
        command: "ai compat",
        observed: SEMANTIC_WITHOUT_LOCATION,
        uniform: PARSE_UNIFORM,
        issue: "#780",
        resolution: "fix completed; move this entry to the uniform table",
    },
    KnownAsymmetry {
        class: FailureClass::Parse,
        command: "causal check",
        observed: PARSE_WITH_DIAGNOSTIC_ALIAS,
        uniform: PARSE_UNIFORM,
        issue: "#780",
        resolution: "fix completed; move this entry to the uniform table",
    },
    KnownAsymmetry {
        class: FailureClass::Parse,
        command: "causal verify-expectations",
        observed: CAUSAL_PARSE_WITHOUT_DIAGNOSTIC,
        uniform: PARSE_UNIFORM,
        issue: "#780",
        resolution: "fix completed; move this entry to the uniform table",
    },
    KnownAsymmetry {
        class: FailureClass::Guard,
        command: "kernel",
        observed: Expectation {
            message_mentions_input: Some(false),
            ..SEMANTIC_UNIFORM
        },
        uniform: SEMANTIC_WITH_INPUT_PATH,
        issue: "#780",
        resolution: "fix completed; move this entry to the uniform table",
    },
    KnownAsymmetry {
        class: FailureClass::Guard,
        command: "domain expand",
        observed: Expectation {
            message_mentions_input: Some(false),
            ..SEMANTIC_UNIFORM
        },
        uniform: SEMANTIC_WITH_INPUT_PATH,
        issue: "#780",
        resolution: "fix completed; move this entry to the uniform table",
    },
    KnownAsymmetry {
        class: FailureClass::Guard,
        command: "domain generate",
        observed: SEMANTIC_WITHOUT_LOCATION,
        uniform: SEMANTIC_UNIFORM,
        issue: "#773",
        resolution: "fix completed; move this entry to the uniform table",
    },
    KnownAsymmetry {
        class: FailureClass::Name,
        command: "domain analyze",
        observed: ANALYZED_NAME_FALSE_GREEN,
        uniform: SEMANTIC_UNIFORM,
        issue: "#796",
        resolution: "fix completed; move this entry to the uniform table",
    },
    KnownAsymmetry {
        class: FailureClass::Name,
        command: "domain expand",
        observed: EXPANDED_NAME_FALSE_GREEN,
        uniform: SEMANTIC_UNIFORM,
        issue: "#796",
        resolution: "fix completed; move this entry to the uniform table",
    },
    KnownAsymmetry {
        class: FailureClass::Name,
        command: "domain generate",
        observed: SEMANTIC_WITHOUT_LOCATION,
        uniform: SEMANTIC_UNIFORM,
        issue: "#773",
        resolution: "fix completed; move this entry to the uniform table",
    },
    KnownAsymmetry {
        class: FailureClass::Literate,
        command: "domain check",
        observed: SEMANTIC_WITHOUT_LOCATION,
        uniform: LITERATE_UNIFORM,
        issue: "#694",
        resolution: "fix completed; move this entry to the uniform table",
    },
    KnownAsymmetry {
        class: FailureClass::Literate,
        command: "db check",
        observed: SEMANTIC_WITHOUT_LOCATION,
        uniform: LITERATE_UNIFORM,
        issue: "#694",
        resolution: "fix completed; move this entry to the uniform table",
    },
    KnownAsymmetry {
        class: FailureClass::Literate,
        command: "ai check",
        observed: SEMANTIC_WITHOUT_LOCATION,
        uniform: LITERATE_UNIFORM,
        issue: "#694",
        resolution: "fix completed; move this entry to the uniform table",
    },
    KnownAsymmetry {
        class: FailureClass::Literate,
        command: "ai compat",
        observed: SEMANTIC_WITHOUT_LOCATION,
        uniform: LITERATE_UNIFORM,
        issue: "#780",
        resolution: "fix completed; move this entry to the uniform table",
    },
    KnownAsymmetry {
        class: FailureClass::Literate,
        command: "compat check",
        observed: SEMANTIC_WITHOUT_LOCATION,
        uniform: LITERATE_UNIFORM,
        issue: "#694",
        resolution: "fix completed; move this entry to the uniform table",
    },
    KnownAsymmetry {
        class: FailureClass::Literate,
        command: "causal check",
        observed: PARSE_WITH_DIAGNOSTIC_ALIAS,
        uniform: LITERATE_UNIFORM,
        issue: "#694",
        resolution: "fix completed; move this entry to the uniform table",
    },
    KnownAsymmetry {
        class: FailureClass::Literate,
        command: "causal verify-expectations",
        observed: CAUSAL_PARSE_WITHOUT_DIAGNOSTIC,
        uniform: LITERATE_UNIFORM,
        issue: "#780",
        resolution: "fix completed; move this entry to the uniform table",
    },
];

struct Actual {
    stdout: String,
    json: Option<Value>,
    exit: i32,
}

#[derive(Clone, Copy, Debug)]
struct Cell {
    class: FailureClass,
    command: &'static str,
    fixture: &'static str,
    uniform: Expectation,
}

fn cells(class: FailureClass) -> Vec<Cell> {
    PARITY_REGISTRY
        .iter()
        .flat_map(|entry| {
            let class_coverage = entry
                .coverage
                .iter()
                .filter(move |coverage| coverage.class == class)
                .map(move |coverage| Cell {
                    class,
                    command: entry.key,
                    fixture: coverage.fixture,
                    uniform: coverage.uniform,
                });
            let literate_coverage = (class == FailureClass::Literate)
                .then_some(entry)
                .filter(|entry| {
                    matches!(
                        entry.literate,
                        LiterateCoverage::UniformUnsupported | LiterateCoverage::PinnedDialect
                    )
                })
                .into_iter()
                .map(move |entry| Cell {
                    class,
                    command: entry.key,
                    fixture: LITERATE_FIXTURE,
                    uniform: LITERATE_UNIFORM,
                });
            class_coverage.chain(literate_coverage)
        })
        .collect()
}

fn workspace_root() -> PathBuf {
    Path::new(env!("CARGO_MANIFEST_DIR"))
        .parent()
        .and_then(Path::parent)
        .expect("workspace root")
        .to_owned()
}

fn registration(command: &str) -> &'static CommandRegistration {
    PARITY_REGISTRY
        .iter()
        .find(|entry| entry.key == command)
        .unwrap_or_else(|| panic!("{command} is absent from PARITY_REGISTRY"))
}

fn invoke(command: &str, fixture: &str) -> Vec<String> {
    let ParityScope::SpecPath { invoke } = registration(command).scope else {
        panic!("{command} is not a runnable SpecPath matrix command");
    };
    invoke
        .iter()
        .map(|argument| {
            if *argument == SPEC_PLACEHOLDER {
                fixture.to_owned()
            } else {
                (*argument).to_owned()
            }
        })
        .collect()
}

fn run(command: &str, fixture: &str) -> Actual {
    let arguments = invoke(command, fixture);
    let output = Command::new(env!("CARGO_BIN_EXE_fslc"))
        .args(&arguments)
        .current_dir(workspace_root())
        .output()
        .unwrap_or_else(|error| panic!("run {command} {arguments:?}: {error}"));
    Actual {
        json: serde_json::from_slice(&output.stdout).ok(),
        stdout: String::from_utf8(output.stdout)
            .unwrap_or_else(|error| panic!("{command} stdout is not UTF-8: {error}")),
        exit: output.status.code().expect("native exit status"),
    }
}

fn matches_expectation(actual: &Actual, expected: Expectation, fixture: &str) -> bool {
    if actual.exit != expected.exit {
        return false;
    }
    match expected.payload {
        Payload::Text => {
            if actual.json.is_some() {
                return false;
            }
            expected
                .exact_stdout
                .is_none_or(|expected_stdout| actual.stdout == expected_stdout)
        }
        Payload::Json => {
            let Some(output) = &actual.json else {
                return false;
            };
            if output.get("result").and_then(Value::as_str) != expected.result
                || output.get("kind").and_then(Value::as_str) != expected.kind
                || output.get("dialect").and_then(Value::as_str) != expected.dialect
            {
                return false;
            }
            let location_matches = match expected.location {
                LocationShape::Absent => output.get("loc").is_none(),
                LocationShape::LineColumn => output.get("loc").is_some_and(|location| {
                    location.get("line").and_then(Value::as_u64).is_some()
                        && location.get("column").and_then(Value::as_u64).is_some()
                }),
                LocationShape::FileOnly => output.get("loc").is_some_and(|location| {
                    location.get("file").and_then(Value::as_str) == Some(fixture)
                        && location.get("line").is_none()
                        && location.get("column").is_none()
                }),
            };
            if !location_matches {
                return false;
            }
            let diagnostic_matches = match expected.diagnostic {
                Diagnostic::None => {
                    output.get("diagnostic_code").is_none() && output.get("diagnostic").is_none()
                }
                Diagnostic::Code(code) => {
                    output.get("diagnostic_code").and_then(Value::as_str) == Some(code)
                        && output.get("diagnostic").is_none()
                }
                Diagnostic::Alias(kind) => {
                    output.get("diagnostic").and_then(Value::as_str) == Some(kind)
                        && output.get("diagnostic_code").is_none()
                }
            };
            if !diagnostic_matches {
                return false;
            }
            expected
                .message_mentions_input
                .is_none_or(|should_mention| {
                    output["message"]
                        .as_str()
                        .is_some_and(|message| message.contains(fixture) == should_mention)
                })
        }
    }
}

fn assert_expectation(
    actual: &Actual,
    expected: Expectation,
    fixture: &str,
    class: FailureClass,
    command: &str,
) {
    assert!(
        matches_expectation(actual, expected, fixture),
        "{class:?}/{command} mismatched {expected:?}; exit={} stdout={}",
        actual.exit,
        actual.stdout
    );
}

fn assert_known_asymmetry(
    actual: &Actual,
    known: &KnownAsymmetry,
    fixture: &str,
    class: FailureClass,
    command: &str,
) {
    assert!(
        !matches_expectation(actual, known.uniform, fixture),
        "{}/{command} now matches the uniform envelope: {} is {}. {}",
        known.issue,
        known.issue,
        known.resolution,
        actual.stdout
    );
    assert_expectation(actual, known.observed, fixture, class, command);
}

fn assert_cell(cell: Cell) {
    let actual = run(cell.command, cell.fixture);
    let pins = KNOWN_ASYMMETRIES
        .iter()
        .filter(|known| known.class == cell.class && known.command == cell.command)
        .collect::<Vec<_>>();
    assert!(
        pins.len() <= 1,
        "{:?}/{} has more than one known-asymmetry pin",
        cell.class,
        cell.command
    );
    if let Some(known) = pins.first() {
        assert_known_asymmetry(&actual, known, cell.fixture, cell.class, cell.command);
    } else {
        assert_expectation(
            &actual,
            cell.uniform,
            cell.fixture,
            cell.class,
            cell.command,
        );
    }
}

fn cli_contract_leaves(node: &Value, leaves: &mut BTreeSet<String>) {
    let commands = node["commands"].as_array().expect("command array");
    if commands.is_empty() {
        let key = node["path"]
            .as_array()
            .expect("command path")
            .iter()
            .map(|segment| segment.as_str().expect("path segment"))
            .collect::<Vec<_>>()
            .join(" ");
        if !key.is_empty() {
            leaves.insert(key);
        }
        return;
    }
    for command in commands {
        cli_contract_leaves(command, leaves);
    }
}

#[test]
fn parity_registry_is_total_and_has_no_orphaned_commands() {
    let contract: Value =
        serde_json::from_str(include_str!("../cli-contract.json")).expect("valid CLI contract");
    let mut contract_leaves = BTreeSet::new();
    cli_contract_leaves(&contract["root"], &mut contract_leaves);

    let registry_keys = PARITY_REGISTRY
        .iter()
        .map(|entry| entry.key.to_owned())
        .collect::<BTreeSet<_>>();
    let missing = contract_leaves
        .difference(&registry_keys)
        .collect::<Vec<_>>();
    let orphaned = registry_keys
        .difference(&contract_leaves)
        .collect::<Vec<_>>();

    assert!(
        missing.is_empty() && orphaned.is_empty(),
        "PARITY_REGISTRY must classify every cli-contract.json leaf exactly once; \
         missing={missing:?}, orphaned={orphaned:?}"
    );
    assert_eq!(
        registry_keys.len(),
        PARITY_REGISTRY.len(),
        "PARITY_REGISTRY contains duplicate command keys"
    );
}

#[test]
fn parity_registry_exclusions_are_explicit_and_runnable_entries_have_a_spec_slot() {
    for entry in PARITY_REGISTRY {
        match entry.scope {
            ParityScope::SpecPath { invoke } => assert!(
                invoke.contains(&SPEC_PLACEHOLDER),
                "{} lacks a {SPEC_PLACEHOLDER} input slot",
                entry.key
            ),
            ParityScope::Excluded { reason } => assert!(
                !reason.trim().is_empty(),
                "{} is excluded without a reason",
                entry.key
            ),
        }
    }
}

#[test]
fn parse_errors_are_uniform_or_pinned_across_frontend_siblings() {
    for cell in cells(FailureClass::Parse) {
        assert_cell(cell);
    }
}

#[test]
fn lowering_guard_errors_are_uniform_or_pinned_across_frontend_siblings() {
    for cell in cells(FailureClass::Guard) {
        assert_cell(cell);
    }
}

#[test]
fn unresolved_identifier_errors_are_uniform_or_pinned_across_eight_entry_points() {
    for cell in cells(FailureClass::Name) {
        assert_cell(cell);
    }
}

#[test]
fn literate_input_errors_are_uniform_or_pinned_across_frontend_siblings() {
    for cell in cells(FailureClass::Literate) {
        assert_cell(cell);
    }
}

#[test]
fn every_pin_is_a_registered_matrix_command_with_a_tracking_issue() {
    let mut cell_keys = BTreeSet::new();
    for class in [
        FailureClass::Parse,
        FailureClass::Guard,
        FailureClass::Name,
        FailureClass::Literate,
    ] {
        for cell in cells(class) {
            assert!(
                cell_keys.insert((cell.class, cell.command)),
                "duplicate matrix cell: {:?}/{}",
                cell.class,
                cell.command
            );
        }
    }
    let mut pin_keys = BTreeSet::new();
    for pin in KNOWN_ASYMMETRIES {
        assert!(
            pin_keys.insert((pin.class, pin.command)),
            "duplicate known-asymmetry pin: {:?}/{}",
            pin.class,
            pin.command
        );
        assert!(
            pin.issue.starts_with('#'),
            "{:?}/{} has no tracking issue",
            pin.class,
            pin.command
        );
        assert!(
            pin.resolution
                .contains("move this entry to the uniform table"),
            "{} must be self-retiring",
            pin.issue
        );
        assert!(
            matches!(
                registration(pin.command).scope,
                ParityScope::SpecPath { .. }
            ),
            "{:?}/{} is pinned but not a runnable matrix command",
            pin.class,
            pin.command
        );
        assert!(
            cell_keys.contains(&(pin.class, pin.command)),
            "{:?}/{} is pinned but not an executable matrix cell",
            pin.class,
            pin.command
        );
    }
}
