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
use std::sync::atomic::{AtomicUsize, Ordering};

use serde_json::Value;

const SPEC_PLACEHOLDER: &str = "{spec}";
const PARSE_KERNEL_FIXTURE: &str = "examples/gallery/errors/parse_missing_expression.fsl";
const PARSE_DOMAIN_FIXTURE: &str =
    "rust/fslc/tests/fixtures/domain_characterization/invalid_broken_expression.fsl";
const PARSE_DB_FIXTURE: &str = "rust/fslc/tests/fixtures/error_envelope_broken_dbsystem.fsl";
const PARSE_AI_FIXTURE: &str = "rust/fslc/tests/fixtures/error_envelope_broken_ai_component.fsl";
const PARSE_CAUSAL_FIXTURE: &str = "rust/fslc/tests/fixtures/error_envelope_broken_causal.fsl";
const AI_GUARD_FIXTURE: &str = "rust/fslc/tests/fixtures/error_envelope_ai_invalid_rule.fsl";
const AI_NAME_FIXTURE: &str = "rust/fslc/tests/fixtures/error_envelope_ai_unknown_tool.fsl";
const AI_PROJECT_GUARD_FIXTURE: &str =
    "rust/fslc/tests/fixtures/error_envelope_ai_project_invalid_rule.fsl";
const AI_PROJECT_NAME_FIXTURE: &str =
    "rust/fslc/tests/fixtures/error_envelope_ai_project_unknown_tool.fsl";
const DB_GUARD_FIXTURE: &str = "rust/fslc/tests/fixtures/error_envelope_db_invalid_rule.fsl";
const DB_NAME_FIXTURE: &str = "rust/fslc/tests/fixtures/error_envelope_db_unknown_column.fsl";
const CAUSAL_NAME_FIXTURE: &str =
    "rust/fslc/tests/fixtures/error_envelope_causal_unknown_reference.fsl";
const CAUSAL_GUARD_FIXTURE: &str =
    "rust/fslc/tests/fixtures/error_envelope_causal_import_guard.fsl";
const DOCUMENT_GUARD_FIXTURE: &str =
    "rust/fslc/tests/fixtures/error_envelope_document_lowering_guard.fsl";
const DOCUMENT_NAME_FIXTURE: &str =
    "rust/fslc/tests/fixtures/error_envelope_document_unknown_name.fsl";
const GUARD_FIXTURE: &str = "rust/fslc/tests/fixtures/domain_await_routing_rejected.fsl";
const NAME_FIXTURE: &str =
    "rust/fslc/tests/fixtures/domain_characterization/invalid_unknown_name.fsl";
const LITERATE_FIXTURE: &str = "examples/literate/toggle.md";
const DOMAIN_REPLAY_LOG: &str = "rust/fslc/tests/fixtures/issue_518_clean.jsonl";
const EMPTY_RECORDS: &str = "rust/fslc/tests/fixtures/error_envelope_empty_records.json";
const DOCUMENT_ARTIFACT: &str = "rust/fslc/tests/fixtures/error_envelope_document.md";
const REPLAY_TRACE: &str = "rust/fslc/tests/fixtures/replay_trace.valid.v1.json";
const APPROVAL_RECORD_PLACEHOLDER: &str = "{approval-record}";
const DOCUMENT_ARTIFACT_PLACEHOLDER: &str = "{document-artifact}";

static APPROVAL_RECORD_SEQUENCE: AtomicUsize = AtomicUsize::new(0);

/// The one ownership classification for every leaf in `cli-contract.json`.
///
/// A `SpecPath` invocation contains [`SPEC_PLACEHOLDER`] at the shared FSL
/// frontend input position. Such a command cannot be excluded merely because
/// it produces a profile or has command-specific semantics: when every
/// required argument is supplied, its spec positional must be observed to
/// reach the shared frontend. An `Excluded` command is still deliberate: its
/// primary input is a manifest, generated artifact, evidence log, records, or
/// a `--kind`-selected contract that cannot reach that frontend path.
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
    Supported { reason: &'static str },
    /// This command's Markdown behavior is one of #694's pinned dialect
    /// asymmetries.
    PinnedDialect,
    /// Markdown is not a meaningful input for this command. The reason is
    /// required so a `SpecPath` command cannot silently opt out of this axis.
    NotApplicable { reason: &'static str },
}

#[derive(Clone, Copy, Debug)]
struct FailureCoverage {
    class: FailureClass,
    fixture: &'static str,
    uniform: Expectation,
}

/// A deliberate axis boundary.  This is part of the registry rather than an
/// implicit omission so adding a CLI leaf cannot leave an error class green
/// merely because no cell happens to execute it.
#[derive(Clone, Copy, Debug)]
struct NotApplicable {
    class: FailureClass,
    reason: &'static str,
}

struct CommandRegistration {
    key: &'static str,
    scope: ParityScope,
    literate: LiterateCoverage,
    coverage: &'static [FailureCoverage],
    not_applicable: &'static [NotApplicable],
}

const PARITY_REGISTRY: &[CommandRegistration] = &[
    CommandRegistration {
        key: "ai check",
        scope: ParityScope::SpecPath {
            invoke: &["ai", "check", SPEC_PLACEHOLDER],
        },
        literate: LiterateCoverage::PinnedDialect,
        coverage: AI_COVERAGE,
        not_applicable: &[],
    },
    CommandRegistration {
        key: "ai compare",
        scope: ParityScope::Excluded {
            reason: "compares two precomputed evaluation-record inputs",
        },
        literate: LiterateCoverage::NotApplicable {
            reason: "takes only evaluation-record inputs, not an FSL document",
        },
        coverage: NO_COVERAGE,
        not_applicable: NOT_APPLICABLE_PARSE_GUARD_NAME,
    },
    CommandRegistration {
        key: "ai compat",
        scope: ParityScope::SpecPath {
            invoke: &["ai", "compat", SPEC_PLACEHOLDER],
        },
        literate: LiterateCoverage::PinnedDialect,
        coverage: AI_COMPAT_COVERAGE,
        not_applicable: &[],
    },
    CommandRegistration {
        key: "ai drift",
        scope: ParityScope::SpecPath {
            invoke: &["ai", "drift", SPEC_PLACEHOLDER, "--logs", EMPTY_RECORDS],
        },
        literate: LiterateCoverage::PinnedDialect,
        coverage: AI_DRIFT_COVERAGE,
        not_applicable: &[],
    },
    CommandRegistration {
        key: "ai eval",
        scope: ParityScope::SpecPath {
            invoke: &["ai", "eval", SPEC_PLACEHOLDER, "--records", EMPTY_RECORDS],
        },
        literate: LiterateCoverage::PinnedDialect,
        coverage: AI_EVAL_COVERAGE,
        not_applicable: &[],
    },
    CommandRegistration {
        key: "ai regress",
        scope: ParityScope::SpecPath {
            invoke: &[
                "ai",
                "regress",
                SPEC_PLACEHOLDER,
                "--before-records",
                EMPTY_RECORDS,
                "--after-records",
                EMPTY_RECORDS,
            ],
        },
        literate: LiterateCoverage::PinnedDialect,
        coverage: AI_REGRESS_COVERAGE,
        not_applicable: &[],
    },
    CommandRegistration {
        key: "ai replay",
        scope: ParityScope::SpecPath {
            invoke: &["ai", "replay", SPEC_PLACEHOLDER, "--logs", EMPTY_RECORDS],
        },
        literate: LiterateCoverage::PinnedDialect,
        coverage: AI_REPLAY_COVERAGE,
        not_applicable: &[],
    },
    CommandRegistration {
        key: "analyze",
        scope: ParityScope::SpecPath {
            invoke: &["analyze", SPEC_PLACEHOLDER],
        },
        literate: LiterateCoverage::UniformUnsupported,
        coverage: PARSE_KERNEL_COVERAGE,
        not_applicable: &[],
    },
    CommandRegistration {
        key: "approval check",
        scope: ParityScope::SpecPath {
            invoke: &[
                "approval",
                "check",
                SPEC_PLACEHOLDER,
                "--record",
                APPROVAL_RECORD_PLACEHOLDER,
            ],
        },
        literate: LiterateCoverage::PinnedDialect,
        coverage: APPROVAL_CHECK_COVERAGE,
        not_applicable: &[],
    },
    CommandRegistration {
        key: "approval create",
        scope: ParityScope::SpecPath {
            invoke: &[
                "approval",
                "create",
                SPEC_PLACEHOLDER,
                "--kind",
                "ledger",
                "--artifact",
                EMPTY_RECORDS,
                "--approver",
                "parity",
            ],
        },
        literate: LiterateCoverage::PinnedDialect,
        coverage: APPROVAL_CREATE_COVERAGE,
        not_applicable: &[],
    },
    CommandRegistration {
        key: "approval diff",
        scope: ParityScope::SpecPath {
            invoke: &[
                "approval",
                "diff",
                SPEC_PLACEHOLDER,
                "--record",
                APPROVAL_RECORD_PLACEHOLDER,
            ],
        },
        literate: LiterateCoverage::PinnedDialect,
        coverage: APPROVAL_DIFF_COVERAGE,
        not_applicable: &[],
    },
    CommandRegistration {
        key: "causal analyze",
        scope: ParityScope::SpecPath {
            invoke: &["causal", "analyze", SPEC_PLACEHOLDER],
        },
        literate: LiterateCoverage::PinnedDialect,
        coverage: CAUSAL_COVERAGE,
        not_applicable: &[],
    },
    CommandRegistration {
        key: "causal check",
        scope: ParityScope::SpecPath {
            invoke: &["causal", "check", SPEC_PLACEHOLDER],
        },
        literate: LiterateCoverage::PinnedDialect,
        coverage: CAUSAL_COVERAGE,
        not_applicable: &[],
    },
    CommandRegistration {
        key: "causal diff",
        scope: ParityScope::SpecPath {
            invoke: &["causal", "diff", SPEC_PLACEHOLDER, SPEC_PLACEHOLDER],
        },
        literate: LiterateCoverage::PinnedDialect,
        coverage: CAUSAL_COVERAGE,
        not_applicable: &[],
    },
    CommandRegistration {
        key: "causal ledger",
        scope: ParityScope::SpecPath {
            invoke: &["causal", "ledger", SPEC_PLACEHOLDER],
        },
        literate: LiterateCoverage::PinnedDialect,
        coverage: CAUSAL_COVERAGE,
        not_applicable: &[],
    },
    CommandRegistration {
        key: "causal observe-expectations",
        scope: ParityScope::SpecPath {
            invoke: &[
                "causal",
                "observe-expectations",
                SPEC_PLACEHOLDER,
                "--from-log",
                EMPTY_RECORDS,
                "--mapping",
                EMPTY_RECORDS,
                "--scope",
                "parity",
                "--period-start",
                "2026-01-01",
                "--period-end",
                "2026-01-02",
            ],
        },
        literate: LiterateCoverage::PinnedDialect,
        coverage: CAUSAL_COVERAGE,
        not_applicable: &[],
    },
    CommandRegistration {
        key: "causal verify-expectations",
        scope: ParityScope::SpecPath {
            invoke: &["causal", "verify-expectations", SPEC_PLACEHOLDER],
        },
        literate: LiterateCoverage::PinnedDialect,
        coverage: CAUSAL_COVERAGE,
        not_applicable: &[],
    },
    CommandRegistration {
        key: "chain",
        scope: ParityScope::Excluded {
            reason: "positional is an fsl-project.toml manifest, not an FSL source document",
        },
        literate: LiterateCoverage::NotApplicable {
            reason: "takes a project manifest, not an FSL document",
        },
        coverage: NO_COVERAGE,
        not_applicable: NOT_APPLICABLE_PARSE_GUARD_NAME,
    },
    CommandRegistration {
        key: "check",
        scope: ParityScope::SpecPath {
            invoke: &["check", SPEC_PLACEHOLDER],
        },
        literate: LiterateCoverage::Supported {
            reason: "check accepts literate Markdown and its successful materialization is covered by the literate contract",
        },
        coverage: CHECK_COVERAGE,
        not_applicable: &[],
    },
    CommandRegistration {
        key: "compat check",
        scope: ParityScope::SpecPath {
            invoke: &["compat", "check", SPEC_PLACEHOLDER],
        },
        literate: LiterateCoverage::PinnedDialect,
        coverage: PARSE_KERNEL_COVERAGE,
        not_applicable: &[],
    },
    CommandRegistration {
        key: "conformance",
        scope: ParityScope::SpecPath {
            invoke: &["conformance", SPEC_PLACEHOLDER],
        },
        literate: LiterateCoverage::UniformUnsupported,
        coverage: PARSE_KERNEL_COVERAGE,
        not_applicable: &[],
    },
    CommandRegistration {
        key: "db check",
        scope: ParityScope::SpecPath {
            invoke: &["db", "check", SPEC_PLACEHOLDER],
        },
        literate: LiterateCoverage::PinnedDialect,
        coverage: DB_CHECK_COVERAGE,
        not_applicable: &[],
    },
    CommandRegistration {
        key: "db import",
        scope: ParityScope::Excluded {
            reason: "imports SQL or ORM schema artifacts rather than an FSL frontend document",
        },
        literate: LiterateCoverage::NotApplicable {
            reason: "takes a SQL or ORM schema artifact, not an FSL document",
        },
        coverage: NO_COVERAGE,
        not_applicable: NOT_APPLICABLE_PARSE_GUARD_NAME,
    },
    CommandRegistration {
        key: "db observe",
        scope: ParityScope::SpecPath {
            invoke: &["db", "observe", SPEC_PLACEHOLDER, "--trace", EMPTY_RECORDS],
        },
        literate: LiterateCoverage::PinnedDialect,
        coverage: DB_OBSERVE_COVERAGE,
        not_applicable: &[],
    },
    CommandRegistration {
        key: "diff",
        scope: ParityScope::SpecPath {
            invoke: &["diff", SPEC_PLACEHOLDER, SPEC_PLACEHOLDER],
        },
        literate: LiterateCoverage::UniformUnsupported,
        coverage: PARSE_KERNEL_COVERAGE,
        not_applicable: &[],
    },
    CommandRegistration {
        key: "document check",
        scope: ParityScope::SpecPath {
            invoke: &[
                "document",
                "check",
                SPEC_PLACEHOLDER,
                DOCUMENT_ARTIFACT_PLACEHOLDER,
            ],
        },
        literate: LiterateCoverage::UniformUnsupported,
        coverage: DOCUMENT_COVERAGE,
        not_applicable: &[],
    },
    CommandRegistration {
        key: "document claims",
        scope: ParityScope::SpecPath {
            invoke: &["document", "claims", SPEC_PLACEHOLDER],
        },
        literate: LiterateCoverage::UniformUnsupported,
        coverage: DOCUMENT_COVERAGE,
        not_applicable: &[],
    },
    CommandRegistration {
        key: "document generate",
        scope: ParityScope::SpecPath {
            invoke: &["document", "generate", SPEC_PLACEHOLDER],
        },
        literate: LiterateCoverage::UniformUnsupported,
        coverage: DOCUMENT_COVERAGE,
        not_applicable: &[],
    },
    CommandRegistration {
        key: "domain analyze",
        scope: ParityScope::SpecPath {
            invoke: &["domain", "analyze", SPEC_PLACEHOLDER],
        },
        literate: LiterateCoverage::PinnedDialect,
        coverage: DOMAIN_COVERAGE,
        not_applicable: &[],
    },
    CommandRegistration {
        key: "domain check",
        scope: ParityScope::SpecPath {
            invoke: &["domain", "check", SPEC_PLACEHOLDER],
        },
        literate: LiterateCoverage::PinnedDialect,
        coverage: DOMAIN_COVERAGE,
        not_applicable: &[],
    },
    CommandRegistration {
        key: "domain expand",
        scope: ParityScope::SpecPath {
            invoke: &["domain", "expand", SPEC_PLACEHOLDER],
        },
        literate: LiterateCoverage::PinnedDialect,
        coverage: DOMAIN_COVERAGE,
        not_applicable: &[],
    },
    CommandRegistration {
        key: "domain generate",
        scope: ParityScope::SpecPath {
            invoke: &["domain", "generate", SPEC_PLACEHOLDER],
        },
        literate: LiterateCoverage::PinnedDialect,
        coverage: DOMAIN_COVERAGE,
        not_applicable: &[],
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
        literate: LiterateCoverage::PinnedDialect,
        coverage: DOMAIN_COVERAGE,
        not_applicable: &[],
    },
    CommandRegistration {
        key: "domain testgen",
        scope: ParityScope::SpecPath {
            invoke: &["domain", "testgen", SPEC_PLACEHOLDER],
        },
        literate: LiterateCoverage::PinnedDialect,
        coverage: DOMAIN_COVERAGE,
        not_applicable: &[],
    },
    CommandRegistration {
        key: "explain",
        scope: ParityScope::SpecPath {
            invoke: &["explain", SPEC_PLACEHOLDER],
        },
        literate: LiterateCoverage::UniformUnsupported,
        coverage: PARSE_KERNEL_COVERAGE,
        not_applicable: &[],
    },
    CommandRegistration {
        key: "fmt",
        scope: ParityScope::SpecPath {
            invoke: &["fmt", SPEC_PLACEHOLDER],
        },
        literate: LiterateCoverage::UniformUnsupported,
        coverage: PARSE_KERNEL_COVERAGE,
        not_applicable: &[],
    },
    CommandRegistration {
        key: "html",
        scope: ParityScope::SpecPath {
            invoke: &["html", SPEC_PLACEHOLDER],
        },
        literate: LiterateCoverage::UniformUnsupported,
        coverage: PARSE_KERNEL_COVERAGE,
        not_applicable: &[],
    },
    CommandRegistration {
        key: "kernel",
        scope: ParityScope::SpecPath {
            invoke: &["kernel", SPEC_PLACEHOLDER],
        },
        literate: LiterateCoverage::UniformUnsupported,
        coverage: KERNEL_GUARD_COVERAGE,
        not_applicable: &[],
    },
    CommandRegistration {
        key: "ledger",
        scope: ParityScope::SpecPath {
            invoke: &["ledger", SPEC_PLACEHOLDER],
        },
        literate: LiterateCoverage::UniformUnsupported,
        coverage: PARSE_KERNEL_COVERAGE,
        not_applicable: &[],
    },
    CommandRegistration {
        key: "lint",
        scope: ParityScope::SpecPath {
            invoke: &["lint", SPEC_PLACEHOLDER],
        },
        literate: LiterateCoverage::UniformUnsupported,
        coverage: PARSE_KERNEL_COVERAGE,
        not_applicable: &[],
    },
    CommandRegistration {
        key: "migrate",
        scope: ParityScope::SpecPath {
            invoke: &["migrate", SPEC_PLACEHOLDER],
        },
        literate: LiterateCoverage::UniformUnsupported,
        coverage: PARSE_KERNEL_COVERAGE,
        not_applicable: &[],
    },
    CommandRegistration {
        key: "mutate",
        scope: ParityScope::SpecPath {
            invoke: &["mutate", SPEC_PLACEHOLDER],
        },
        literate: LiterateCoverage::UniformUnsupported,
        coverage: PARSE_KERNEL_COVERAGE,
        not_applicable: &[],
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
        coverage: PARSE_KERNEL_COVERAGE,
        not_applicable: &[],
    },
    CommandRegistration {
        key: "replay",
        scope: ParityScope::SpecPath {
            invoke: &["replay", SPEC_PLACEHOLDER, "--trace", REPLAY_TRACE],
        },
        literate: LiterateCoverage::UniformUnsupported,
        coverage: PARSE_KERNEL_COVERAGE,
        not_applicable: &[],
    },
    CommandRegistration {
        key: "scenarios",
        scope: ParityScope::SpecPath {
            invoke: &["scenarios", SPEC_PLACEHOLDER],
        },
        literate: LiterateCoverage::Supported {
            reason: "scenarios accepts literate Markdown and its successful materialization is covered by the literate contract",
        },
        coverage: PARSE_KERNEL_COVERAGE,
        not_applicable: &[],
    },
    CommandRegistration {
        key: "sweep",
        scope: ParityScope::SpecPath {
            invoke: &["sweep", SPEC_PLACEHOLDER],
        },
        literate: LiterateCoverage::UniformUnsupported,
        coverage: PARSE_KERNEL_COVERAGE,
        not_applicable: &[],
    },
    CommandRegistration {
        key: "testgen",
        scope: ParityScope::SpecPath {
            invoke: &["testgen", SPEC_PLACEHOLDER],
        },
        literate: LiterateCoverage::UniformUnsupported,
        coverage: PARSE_KERNEL_COVERAGE,
        not_applicable: &[],
    },
    CommandRegistration {
        key: "typestate",
        scope: ParityScope::SpecPath {
            invoke: &["typestate", SPEC_PLACEHOLDER],
        },
        literate: LiterateCoverage::UniformUnsupported,
        coverage: PARSE_KERNEL_COVERAGE,
        not_applicable: &[],
    },
    CommandRegistration {
        key: "verify",
        scope: ParityScope::SpecPath {
            invoke: &["verify", SPEC_PLACEHOLDER],
        },
        literate: LiterateCoverage::Supported {
            reason: "verify accepts literate Markdown and its successful materialization is covered by the literate contract",
        },
        coverage: VERIFY_COVERAGE,
        not_applicable: &[],
    },
    CommandRegistration {
        key: "version",
        scope: ParityScope::Excluded {
            reason: "has no input path or frontend entry point",
        },
        literate: LiterateCoverage::NotApplicable {
            reason: "has no input path",
        },
        coverage: NO_COVERAGE,
        not_applicable: NOT_APPLICABLE_PARSE_GUARD_NAME,
    },
];

#[derive(Clone, Copy, Debug, PartialEq, Eq, PartialOrd, Ord)]
enum FailureClass {
    Parse,
    Guard,
    Name,
    Literate,
}

/// The input form that reaches a command's frontend. Generic FSL commands
/// have one source form; AI semantic cells must cover both component and
/// project documents, because their dispatch paths are observably distinct.
#[derive(Clone, Copy, Debug, PartialEq, Eq, PartialOrd, Ord)]
enum InputShape {
    Source,
    Component,
    Project,
}

#[derive(Clone, Copy, Debug, PartialEq, Eq, PartialOrd, Ord)]
enum Diagnostic {
    None,
    Code(&'static str),
    Alias(&'static str),
}

#[derive(Clone, Copy, Debug, PartialEq, Eq, PartialOrd, Ord)]
enum LocationShape {
    Absent,
    LineColumn,
    FileOnly,
}

#[derive(Clone, Copy, Debug, PartialEq, Eq, PartialOrd, Ord)]
enum ExpectedField {
    Absent,
    Exact(&'static str),
}

#[derive(Clone, Copy, Debug, PartialEq, Eq, PartialOrd, Ord)]
enum MessageExpectation {
    Absent,
    MentionsInput,
    OmitsInput,
}

#[derive(Clone, Copy, Debug, PartialEq, Eq, PartialOrd, Ord)]
struct JsonExpectation {
    result: ExpectedField,
    kind: ExpectedField,
    location: LocationShape,
    diagnostic: Diagnostic,
    exit: i32,
    dialect: ExpectedField,
    message: MessageExpectation,
}

#[derive(Clone, Copy, Debug, PartialEq, Eq, PartialOrd, Ord)]
enum Expectation {
    Json(JsonExpectation),
    Text {
        exit: i32,
        exact_stdout: &'static str,
    },
}

const PARSE_JSON: JsonExpectation = JsonExpectation {
    result: ExpectedField::Exact("error"),
    kind: ExpectedField::Exact("parse"),
    location: LocationShape::LineColumn,
    diagnostic: Diagnostic::Code("FSL-PARSE"),
    exit: 2,
    dialect: ExpectedField::Absent,
    message: MessageExpectation::OmitsInput,
};
const PARSE_UNIFORM: Expectation = Expectation::Json(PARSE_JSON);

const SEMANTIC_JSON: JsonExpectation = JsonExpectation {
    result: ExpectedField::Exact("error"),
    kind: ExpectedField::Exact("semantics"),
    location: LocationShape::LineColumn,
    diagnostic: Diagnostic::None,
    exit: 2,
    dialect: ExpectedField::Absent,
    message: MessageExpectation::MentionsInput,
};
const SEMANTIC_UNIFORM: Expectation = Expectation::Json(SEMANTIC_JSON);

/// The Guard matrix requires the source fixture path in semantic diagnostics.
const SEMANTIC_WITH_INPUT_PATH: Expectation = Expectation::Json(JsonExpectation {
    message: MessageExpectation::MentionsInput,
    ..SEMANTIC_JSON
});

const SEMANTIC_WITHOUT_INPUT_PATH: Expectation = Expectation::Json(JsonExpectation {
    message: MessageExpectation::OmitsInput,
    ..SEMANTIC_JSON
});

const SEMANTIC_WITH_INPUT_PATH_WITHOUT_LOCATION: Expectation = Expectation::Json(JsonExpectation {
    location: LocationShape::Absent,
    ..SEMANTIC_JSON
});

const SEMANTIC_WITHOUT_INPUT_PATH_WITHOUT_LOCATION: Expectation =
    Expectation::Json(JsonExpectation {
        location: LocationShape::Absent,
        message: MessageExpectation::OmitsInput,
        ..SEMANTIC_JSON
    });

const PARSE_WITH_DIAGNOSTIC_ALIAS: Expectation = Expectation::Json(JsonExpectation {
    diagnostic: Diagnostic::Alias("parse"),
    ..PARSE_JSON
});

const LITERATE_UNIFORM: Expectation = Expectation::Json(JsonExpectation {
    result: ExpectedField::Exact("error"),
    kind: ExpectedField::Exact("usage"),
    location: LocationShape::FileOnly,
    diagnostic: Diagnostic::Code("FSL-INPUT-LITERATE-UNSUPPORTED"),
    exit: 2,
    dialect: ExpectedField::Absent,
    message: MessageExpectation::MentionsInput,
});

/// #796 pins a product false negative: an invalid spec exits 0 and is accepted.
/// This is a soundness defect, not an endorsed behavior; the pin detects
/// regressions or changes until the first follow-up queue item, #796, fixes it.
/// It may share a root with `KNOWN_DIVERGENT_DOMAIN_FIXTURES` entry 1 (#690):
/// #690 is closed, but its symptom 2 divergence remained live when measured
/// on 2026-08-13 and is now tracked by open #798.
const ANALYZED_NAME_FALSE_GREEN: Expectation = Expectation::Json(JsonExpectation {
    result: ExpectedField::Exact("analyzed"),
    kind: ExpectedField::Absent,
    location: LocationShape::Absent,
    diagnostic: Diagnostic::None,
    exit: 0,
    dialect: ExpectedField::Exact("fsl-domain-effect.v0"),
    message: MessageExpectation::Absent,
});

/// See [`ANALYZED_NAME_FALSE_GREEN`]: this is the same #796 false-negative
/// soundness defect and is pinned solely to detect change, not to approve it.
const EXPANDED_NAME_FALSE_GREEN: Expectation = Expectation::Text {
    exit: 0,
    exact_stdout: r#"spec InvalidUnknownName "domain: generated from fsl-domain/fsl-effect" {
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
};

const CAUSAL_PARSE_WITHOUT_DIAGNOSTIC: Expectation = Expectation::Json(JsonExpectation {
    diagnostic: Diagnostic::None,
    ..PARSE_JSON
});
const CAUSAL_NAME_WITH_DIAGNOSTIC: Expectation = Expectation::Json(JsonExpectation {
    diagnostic: Diagnostic::Alias("causal_unknown_reference"),
    message: MessageExpectation::OmitsInput,
    ..SEMANTIC_JSON
});
/// #800 tracks these product false negatives. They are pinned to detect drift,
/// not to endorse accepting invalid component declarations.
const AI_PROJECT_CHECK_FALSE_GREEN: Expectation = Expectation::Json(JsonExpectation {
    result: ExpectedField::Exact("ai_project_analyzed"),
    kind: ExpectedField::Absent,
    location: LocationShape::Absent,
    diagnostic: Diagnostic::None,
    exit: 0,
    dialect: ExpectedField::Exact("fsl-ai-project.v0"),
    message: MessageExpectation::Absent,
});
const AI_COMPAT_FALSE_GREEN: Expectation = Expectation::Json(JsonExpectation {
    result: ExpectedField::Exact("compat_profile_generated"),
    kind: ExpectedField::Absent,
    location: LocationShape::Absent,
    diagnostic: Diagnostic::None,
    exit: 0,
    dialect: ExpectedField::Absent,
    message: MessageExpectation::Absent,
});
const AI_DRIFT_FALSE_GREEN: Expectation = Expectation::Json(JsonExpectation {
    result: ExpectedField::Exact("observed_supported"),
    kind: ExpectedField::Absent,
    location: LocationShape::Absent,
    diagnostic: Diagnostic::None,
    exit: 0,
    dialect: ExpectedField::Absent,
    message: MessageExpectation::Absent,
});
const AI_EVAL_FALSE_GREEN: Expectation = Expectation::Json(JsonExpectation {
    result: ExpectedField::Exact("statistically_supported"),
    kind: ExpectedField::Absent,
    location: LocationShape::Absent,
    diagnostic: Diagnostic::None,
    exit: 0,
    dialect: ExpectedField::Absent,
    message: MessageExpectation::Absent,
});
const AI_REGRESS_FALSE_GREEN: Expectation = AI_EVAL_FALSE_GREEN;
const AI_REPLAY_FALSE_GREEN: Expectation = Expectation::Json(JsonExpectation {
    result: ExpectedField::Exact("replay_conformant"),
    kind: ExpectedField::Absent,
    location: LocationShape::Absent,
    diagnostic: Diagnostic::None,
    exit: 0,
    dialect: ExpectedField::Exact("fsl-ai-hard.v0"),
    message: MessageExpectation::Absent,
});

const NO_COVERAGE: &[FailureCoverage] = &[];
const NOT_APPLICABLE_PARSE_GUARD_NAME: &[NotApplicable] = &[
    NotApplicable {
        class: FailureClass::Parse,
        reason: "this command has no FSL source frontend input",
    },
    NotApplicable {
        class: FailureClass::Guard,
        reason: "this command has no FSL source frontend input",
    },
    NotApplicable {
        class: FailureClass::Name,
        reason: "this command has no FSL source frontend input",
    },
];
const AI_PARSE_COVERAGE: &[FailureCoverage] = &[FailureCoverage {
    class: FailureClass::Parse,
    fixture: PARSE_AI_FIXTURE,
    uniform: PARSE_UNIFORM,
}];
const AI_COMPAT_COVERAGE: &[FailureCoverage] = &[
    AI_PARSE_COVERAGE[0],
    FailureCoverage {
        class: FailureClass::Guard,
        fixture: AI_GUARD_FIXTURE,
        uniform: SEMANTIC_WITHOUT_INPUT_PATH_WITHOUT_LOCATION,
    },
    FailureCoverage {
        class: FailureClass::Name,
        fixture: AI_NAME_FIXTURE,
        uniform: SEMANTIC_WITHOUT_INPUT_PATH_WITHOUT_LOCATION,
    },
    FailureCoverage {
        class: FailureClass::Guard,
        fixture: AI_PROJECT_GUARD_FIXTURE,
        uniform: SEMANTIC_WITHOUT_INPUT_PATH_WITHOUT_LOCATION,
    },
    FailureCoverage {
        class: FailureClass::Name,
        fixture: AI_PROJECT_NAME_FIXTURE,
        uniform: SEMANTIC_WITHOUT_INPUT_PATH_WITHOUT_LOCATION,
    },
];
const AI_DRIFT_COVERAGE: &[FailureCoverage] = &[
    AI_PARSE_COVERAGE[0],
    FailureCoverage {
        class: FailureClass::Guard,
        fixture: AI_GUARD_FIXTURE,
        uniform: SEMANTIC_WITHOUT_INPUT_PATH_WITHOUT_LOCATION,
    },
    FailureCoverage {
        class: FailureClass::Name,
        fixture: AI_NAME_FIXTURE,
        uniform: SEMANTIC_WITHOUT_INPUT_PATH_WITHOUT_LOCATION,
    },
    FailureCoverage {
        class: FailureClass::Guard,
        fixture: AI_PROJECT_GUARD_FIXTURE,
        uniform: SEMANTIC_WITHOUT_INPUT_PATH_WITHOUT_LOCATION,
    },
    FailureCoverage {
        class: FailureClass::Name,
        fixture: AI_PROJECT_NAME_FIXTURE,
        uniform: SEMANTIC_WITHOUT_INPUT_PATH_WITHOUT_LOCATION,
    },
];
const AI_EVAL_COVERAGE: &[FailureCoverage] = &[
    AI_PARSE_COVERAGE[0],
    FailureCoverage {
        class: FailureClass::Guard,
        fixture: AI_GUARD_FIXTURE,
        uniform: SEMANTIC_WITHOUT_INPUT_PATH_WITHOUT_LOCATION,
    },
    FailureCoverage {
        class: FailureClass::Name,
        fixture: AI_NAME_FIXTURE,
        uniform: SEMANTIC_WITHOUT_INPUT_PATH_WITHOUT_LOCATION,
    },
    FailureCoverage {
        class: FailureClass::Guard,
        fixture: AI_PROJECT_GUARD_FIXTURE,
        uniform: SEMANTIC_WITHOUT_INPUT_PATH_WITHOUT_LOCATION,
    },
    FailureCoverage {
        class: FailureClass::Name,
        fixture: AI_PROJECT_NAME_FIXTURE,
        uniform: SEMANTIC_WITHOUT_INPUT_PATH_WITHOUT_LOCATION,
    },
];
const AI_REGRESS_COVERAGE: &[FailureCoverage] = &[
    AI_PARSE_COVERAGE[0],
    FailureCoverage {
        class: FailureClass::Guard,
        fixture: AI_GUARD_FIXTURE,
        uniform: SEMANTIC_WITHOUT_INPUT_PATH_WITHOUT_LOCATION,
    },
    FailureCoverage {
        class: FailureClass::Name,
        fixture: AI_NAME_FIXTURE,
        uniform: SEMANTIC_WITHOUT_INPUT_PATH_WITHOUT_LOCATION,
    },
    FailureCoverage {
        class: FailureClass::Guard,
        fixture: AI_PROJECT_GUARD_FIXTURE,
        uniform: SEMANTIC_WITHOUT_INPUT_PATH_WITHOUT_LOCATION,
    },
    FailureCoverage {
        class: FailureClass::Name,
        fixture: AI_PROJECT_NAME_FIXTURE,
        uniform: SEMANTIC_WITHOUT_INPUT_PATH_WITHOUT_LOCATION,
    },
];
const AI_REPLAY_COVERAGE: &[FailureCoverage] = &[
    AI_PARSE_COVERAGE[0],
    FailureCoverage {
        class: FailureClass::Guard,
        fixture: AI_GUARD_FIXTURE,
        uniform: SEMANTIC_WITHOUT_INPUT_PATH_WITHOUT_LOCATION,
    },
    FailureCoverage {
        class: FailureClass::Name,
        fixture: AI_NAME_FIXTURE,
        uniform: SEMANTIC_WITHOUT_INPUT_PATH_WITHOUT_LOCATION,
    },
    FailureCoverage {
        class: FailureClass::Guard,
        fixture: AI_PROJECT_GUARD_FIXTURE,
        uniform: SEMANTIC_WITHOUT_INPUT_PATH_WITHOUT_LOCATION,
    },
    FailureCoverage {
        class: FailureClass::Name,
        fixture: AI_PROJECT_NAME_FIXTURE,
        uniform: SEMANTIC_WITHOUT_INPUT_PATH_WITHOUT_LOCATION,
    },
];
const AI_COVERAGE: &[FailureCoverage] = &[
    AI_PARSE_COVERAGE[0],
    FailureCoverage {
        class: FailureClass::Guard,
        fixture: AI_GUARD_FIXTURE,
        uniform: SEMANTIC_WITHOUT_INPUT_PATH_WITHOUT_LOCATION,
    },
    FailureCoverage {
        class: FailureClass::Name,
        fixture: AI_NAME_FIXTURE,
        uniform: SEMANTIC_WITHOUT_INPUT_PATH_WITHOUT_LOCATION,
    },
    FailureCoverage {
        class: FailureClass::Guard,
        fixture: AI_PROJECT_GUARD_FIXTURE,
        uniform: SEMANTIC_WITHOUT_INPUT_PATH_WITHOUT_LOCATION,
    },
    FailureCoverage {
        class: FailureClass::Name,
        fixture: AI_PROJECT_NAME_FIXTURE,
        uniform: SEMANTIC_WITHOUT_INPUT_PATH_WITHOUT_LOCATION,
    },
];
const PARSE_KERNEL_COVERAGE: &[FailureCoverage] = &[
    FailureCoverage {
        class: FailureClass::Parse,
        fixture: PARSE_KERNEL_FIXTURE,
        uniform: PARSE_UNIFORM,
    },
    FailureCoverage {
        class: FailureClass::Guard,
        fixture: GUARD_FIXTURE,
        uniform: SEMANTIC_WITH_INPUT_PATH,
    },
    FailureCoverage {
        class: FailureClass::Name,
        fixture: NAME_FIXTURE,
        uniform: SEMANTIC_UNIFORM,
    },
];
const DOCUMENT_COVERAGE: &[FailureCoverage] = &[
    FailureCoverage {
        class: FailureClass::Parse,
        fixture: PARSE_KERNEL_FIXTURE,
        uniform: PARSE_UNIFORM,
    },
    FailureCoverage {
        class: FailureClass::Guard,
        fixture: DOCUMENT_GUARD_FIXTURE,
        uniform: SEMANTIC_WITHOUT_INPUT_PATH_WITHOUT_LOCATION,
    },
    FailureCoverage {
        class: FailureClass::Name,
        fixture: DOCUMENT_NAME_FIXTURE,
        uniform: SEMANTIC_WITHOUT_INPUT_PATH,
    },
];
const CHECK_COVERAGE: &[FailureCoverage] = &[
    FailureCoverage {
        class: FailureClass::Parse,
        fixture: PARSE_KERNEL_FIXTURE,
        uniform: PARSE_UNIFORM,
    },
    FailureCoverage {
        class: FailureClass::Guard,
        fixture: GUARD_FIXTURE,
        uniform: SEMANTIC_WITH_INPUT_PATH,
    },
    FailureCoverage {
        class: FailureClass::Name,
        fixture: NAME_FIXTURE,
        uniform: SEMANTIC_UNIFORM,
    },
];
const VERIFY_COVERAGE: &[FailureCoverage] = CHECK_COVERAGE;
const DB_CHECK_COVERAGE: &[FailureCoverage] = &[
    FailureCoverage {
        class: FailureClass::Parse,
        fixture: PARSE_DB_FIXTURE,
        uniform: PARSE_UNIFORM,
    },
    FailureCoverage {
        class: FailureClass::Guard,
        fixture: DB_GUARD_FIXTURE,
        uniform: SEMANTIC_WITHOUT_INPUT_PATH_WITHOUT_LOCATION,
    },
    FailureCoverage {
        class: FailureClass::Name,
        fixture: DB_NAME_FIXTURE,
        uniform: SEMANTIC_WITHOUT_INPUT_PATH_WITHOUT_LOCATION,
    },
];
const DB_OBSERVE_COVERAGE: &[FailureCoverage] = &[
    FailureCoverage {
        class: FailureClass::Parse,
        fixture: PARSE_DB_FIXTURE,
        uniform: PARSE_UNIFORM,
    },
    FailureCoverage {
        class: FailureClass::Guard,
        fixture: DB_GUARD_FIXTURE,
        uniform: SEMANTIC_WITHOUT_INPUT_PATH_WITHOUT_LOCATION,
    },
    FailureCoverage {
        class: FailureClass::Name,
        fixture: DB_NAME_FIXTURE,
        uniform: SEMANTIC_WITHOUT_INPUT_PATH_WITHOUT_LOCATION,
    },
];
const APPROVAL_CREATE_COVERAGE: &[FailureCoverage] = &[
    FailureCoverage {
        class: FailureClass::Parse,
        fixture: PARSE_KERNEL_FIXTURE,
        uniform: PARSE_UNIFORM,
    },
    FailureCoverage {
        class: FailureClass::Guard,
        fixture: GUARD_FIXTURE,
        uniform: SEMANTIC_WITH_INPUT_PATH,
    },
    FailureCoverage {
        class: FailureClass::Name,
        fixture: NAME_FIXTURE,
        uniform: SEMANTIC_UNIFORM,
    },
];
const APPROVAL_CHECK_COVERAGE: &[FailureCoverage] = &[
    FailureCoverage {
        class: FailureClass::Parse,
        fixture: PARSE_KERNEL_FIXTURE,
        uniform: PARSE_UNIFORM,
    },
    FailureCoverage {
        class: FailureClass::Guard,
        fixture: GUARD_FIXTURE,
        uniform: SEMANTIC_WITHOUT_INPUT_PATH_WITHOUT_LOCATION,
    },
    FailureCoverage {
        class: FailureClass::Name,
        fixture: NAME_FIXTURE,
        uniform: SEMANTIC_WITHOUT_INPUT_PATH_WITHOUT_LOCATION,
    },
];
const APPROVAL_DIFF_COVERAGE: &[FailureCoverage] = &[
    FailureCoverage {
        class: FailureClass::Parse,
        fixture: PARSE_KERNEL_FIXTURE,
        uniform: PARSE_UNIFORM,
    },
    FailureCoverage {
        class: FailureClass::Guard,
        fixture: GUARD_FIXTURE,
        uniform: SEMANTIC_WITH_INPUT_PATH,
    },
    FailureCoverage {
        class: FailureClass::Name,
        fixture: NAME_FIXTURE,
        uniform: SEMANTIC_UNIFORM,
    },
];
const CAUSAL_COVERAGE: &[FailureCoverage] = &[
    FailureCoverage {
        class: FailureClass::Parse,
        fixture: PARSE_CAUSAL_FIXTURE,
        uniform: PARSE_UNIFORM,
    },
    FailureCoverage {
        class: FailureClass::Guard,
        fixture: CAUSAL_GUARD_FIXTURE,
        uniform: CAUSAL_NAME_WITH_DIAGNOSTIC,
    },
    FailureCoverage {
        class: FailureClass::Name,
        fixture: CAUSAL_NAME_FIXTURE,
        uniform: SEMANTIC_WITHOUT_INPUT_PATH,
    },
];
const DOMAIN_COVERAGE: &[FailureCoverage] = &[
    FailureCoverage {
        class: FailureClass::Parse,
        fixture: PARSE_DOMAIN_FIXTURE,
        uniform: PARSE_UNIFORM,
    },
    FailureCoverage {
        class: FailureClass::Guard,
        fixture: GUARD_FIXTURE,
        uniform: SEMANTIC_WITH_INPUT_PATH,
    },
    FailureCoverage {
        class: FailureClass::Name,
        fixture: NAME_FIXTURE,
        uniform: SEMANTIC_UNIFORM,
    },
];
const KERNEL_GUARD_COVERAGE: &[FailureCoverage] = &[
    FailureCoverage {
        class: FailureClass::Parse,
        fixture: PARSE_KERNEL_FIXTURE,
        uniform: PARSE_UNIFORM,
    },
    FailureCoverage {
        class: FailureClass::Guard,
        fixture: GUARD_FIXTURE,
        uniform: SEMANTIC_WITH_INPUT_PATH,
    },
    FailureCoverage {
        class: FailureClass::Name,
        fixture: NAME_FIXTURE,
        uniform: SEMANTIC_UNIFORM,
    },
];

struct KnownAsymmetry {
    class: FailureClass,
    command: &'static str,
    shape: InputShape,
    fixture: &'static str,
    observed: Expectation,
    issue: &'static str,
    resolution: &'static str,
}

/// Existing nonuniform output is pinned rather than allowlisted.  The
/// registry, not the pin, owns each cell's uniform expectation. When a fix
/// reaches it, [`assert_known_asymmetry`] tells the maintainer to move the
/// cell into the uniform table instead of silently retaining stale debt.
macro_rules! pin {
    (shape: $shape:expr; $class:expr, $command:literal, $fixture:expr, $observed:expr, $issue:literal) => {
        KnownAsymmetry {
            class: $class,
            command: $command,
            shape: $shape,
            fixture: $fixture,
            observed: $observed,
            issue: $issue,
            resolution: "fix completed; move this entry to the uniform table",
        }
    };
    ($class:expr, $command:literal, $fixture:expr, $observed:expr, $issue:literal) => {
        pin!(
            shape: InputShape::Source;
            $class, $command, $fixture, $observed, $issue
        )
    };
}

const KNOWN_ASYMMETRIES: &[KnownAsymmetry] = &[
    pin!(
        FailureClass::Parse,
        "domain check",
        PARSE_DOMAIN_FIXTURE,
        SEMANTIC_WITH_INPUT_PATH_WITHOUT_LOCATION,
        "#780"
    ),
    pin!(
        FailureClass::Parse,
        "domain analyze",
        PARSE_DOMAIN_FIXTURE,
        SEMANTIC_WITH_INPUT_PATH_WITHOUT_LOCATION,
        "#780"
    ),
    pin!(
        FailureClass::Parse,
        "domain expand",
        PARSE_DOMAIN_FIXTURE,
        SEMANTIC_WITH_INPUT_PATH_WITHOUT_LOCATION,
        "#780"
    ),
    pin!(
        FailureClass::Parse,
        "domain generate",
        PARSE_DOMAIN_FIXTURE,
        SEMANTIC_WITH_INPUT_PATH_WITHOUT_LOCATION,
        "#780"
    ),
    pin!(
        FailureClass::Parse,
        "domain replay",
        PARSE_DOMAIN_FIXTURE,
        SEMANTIC_WITH_INPUT_PATH_WITHOUT_LOCATION,
        "#780"
    ),
    pin!(
        FailureClass::Parse,
        "domain testgen",
        PARSE_DOMAIN_FIXTURE,
        SEMANTIC_WITH_INPUT_PATH_WITHOUT_LOCATION,
        "#780"
    ),
    pin!(
        FailureClass::Parse,
        "db check",
        PARSE_DB_FIXTURE,
        SEMANTIC_WITH_INPUT_PATH_WITHOUT_LOCATION,
        "#780"
    ),
    pin!(
        FailureClass::Parse,
        "db observe",
        PARSE_DB_FIXTURE,
        SEMANTIC_WITH_INPUT_PATH_WITHOUT_LOCATION,
        "#780"
    ),
    pin!(
        shape: InputShape::Component;
        FailureClass::Parse,
        "ai check",
        PARSE_AI_FIXTURE,
        SEMANTIC_WITH_INPUT_PATH_WITHOUT_LOCATION,
        "#780"
    ),
    pin!(
        shape: InputShape::Component;
        FailureClass::Parse,
        "ai compat",
        PARSE_AI_FIXTURE,
        SEMANTIC_WITH_INPUT_PATH_WITHOUT_LOCATION,
        "#780"
    ),
    pin!(
        shape: InputShape::Component;
        FailureClass::Parse,
        "ai drift",
        PARSE_AI_FIXTURE,
        SEMANTIC_WITHOUT_INPUT_PATH_WITHOUT_LOCATION,
        "#780"
    ),
    pin!(
        shape: InputShape::Component;
        FailureClass::Parse,
        "ai eval",
        PARSE_AI_FIXTURE,
        SEMANTIC_WITHOUT_INPUT_PATH_WITHOUT_LOCATION,
        "#780"
    ),
    pin!(
        shape: InputShape::Component;
        FailureClass::Parse,
        "ai regress",
        PARSE_AI_FIXTURE,
        SEMANTIC_WITHOUT_INPUT_PATH_WITHOUT_LOCATION,
        "#780"
    ),
    pin!(
        shape: InputShape::Component;
        FailureClass::Parse,
        "ai replay",
        PARSE_AI_FIXTURE,
        SEMANTIC_WITH_INPUT_PATH_WITHOUT_LOCATION,
        "#780"
    ),
    pin!(
        FailureClass::Parse,
        "compat check",
        PARSE_KERNEL_FIXTURE,
        SEMANTIC_WITH_INPUT_PATH_WITHOUT_LOCATION,
        "#780"
    ),
    pin!(
        FailureClass::Parse,
        "causal check",
        PARSE_CAUSAL_FIXTURE,
        PARSE_WITH_DIAGNOSTIC_ALIAS,
        "#780"
    ),
    pin!(
        FailureClass::Parse,
        "causal verify-expectations",
        PARSE_CAUSAL_FIXTURE,
        CAUSAL_PARSE_WITHOUT_DIAGNOSTIC,
        "#780"
    ),
    pin!(
        FailureClass::Parse,
        "causal analyze",
        PARSE_CAUSAL_FIXTURE,
        PARSE_WITH_DIAGNOSTIC_ALIAS,
        "#780"
    ),
    pin!(
        FailureClass::Parse,
        "causal diff",
        PARSE_CAUSAL_FIXTURE,
        PARSE_WITH_DIAGNOSTIC_ALIAS,
        "#780"
    ),
    pin!(
        FailureClass::Parse,
        "causal ledger",
        PARSE_CAUSAL_FIXTURE,
        PARSE_WITH_DIAGNOSTIC_ALIAS,
        "#780"
    ),
    pin!(
        FailureClass::Parse,
        "causal observe-expectations",
        PARSE_CAUSAL_FIXTURE,
        CAUSAL_PARSE_WITHOUT_DIAGNOSTIC,
        "#780"
    ),
    pin!(
        FailureClass::Name,
        "causal analyze",
        CAUSAL_NAME_FIXTURE,
        CAUSAL_NAME_WITH_DIAGNOSTIC,
        "#780"
    ),
    pin!(
        FailureClass::Name,
        "causal check",
        CAUSAL_NAME_FIXTURE,
        CAUSAL_NAME_WITH_DIAGNOSTIC,
        "#780"
    ),
    pin!(
        FailureClass::Name,
        "causal diff",
        CAUSAL_NAME_FIXTURE,
        CAUSAL_NAME_WITH_DIAGNOSTIC,
        "#780"
    ),
    pin!(
        FailureClass::Name,
        "causal ledger",
        CAUSAL_NAME_FIXTURE,
        CAUSAL_NAME_WITH_DIAGNOSTIC,
        "#780"
    ),
    pin!(
        FailureClass::Name,
        "causal observe-expectations",
        CAUSAL_NAME_FIXTURE,
        CAUSAL_NAME_WITH_DIAGNOSTIC,
        "#780"
    ),
    pin!(
        FailureClass::Name,
        "causal verify-expectations",
        CAUSAL_NAME_FIXTURE,
        CAUSAL_NAME_WITH_DIAGNOSTIC,
        "#780"
    ),
    pin!(
        FailureClass::Parse,
        "approval check",
        PARSE_KERNEL_FIXTURE,
        SEMANTIC_WITHOUT_INPUT_PATH_WITHOUT_LOCATION,
        "#780"
    ),
    // #800 tracks these product false negatives. They are not accepted
    // behavior: the affected command/input-shape pairs report success
    // without validating the malformed component declaration.
    pin!(
        shape: InputShape::Component;
        FailureClass::Guard,
        "ai compat",
        AI_GUARD_FIXTURE,
        AI_COMPAT_FALSE_GREEN,
        "#800"
    ),
    pin!(
        shape: InputShape::Component;
        FailureClass::Name,
        "ai compat",
        AI_NAME_FIXTURE,
        AI_COMPAT_FALSE_GREEN,
        "#800"
    ),
    pin!(
        shape: InputShape::Project;
        FailureClass::Guard,
        "ai compat",
        AI_PROJECT_GUARD_FIXTURE,
        AI_COMPAT_FALSE_GREEN,
        "#800"
    ),
    pin!(
        shape: InputShape::Project;
        FailureClass::Name,
        "ai compat",
        AI_PROJECT_NAME_FIXTURE,
        AI_COMPAT_FALSE_GREEN,
        "#800"
    ),
    pin!(
        shape: InputShape::Project;
        FailureClass::Guard,
        "ai drift",
        AI_PROJECT_GUARD_FIXTURE,
        AI_DRIFT_FALSE_GREEN,
        "#800"
    ),
    pin!(
        shape: InputShape::Project;
        FailureClass::Name,
        "ai drift",
        AI_PROJECT_NAME_FIXTURE,
        AI_DRIFT_FALSE_GREEN,
        "#800"
    ),
    pin!(
        shape: InputShape::Project;
        FailureClass::Guard,
        "ai eval",
        AI_PROJECT_GUARD_FIXTURE,
        AI_EVAL_FALSE_GREEN,
        "#800"
    ),
    pin!(
        shape: InputShape::Project;
        FailureClass::Name,
        "ai eval",
        AI_PROJECT_NAME_FIXTURE,
        AI_EVAL_FALSE_GREEN,
        "#800"
    ),
    pin!(
        shape: InputShape::Project;
        FailureClass::Guard,
        "ai regress",
        AI_PROJECT_GUARD_FIXTURE,
        AI_REGRESS_FALSE_GREEN,
        "#800"
    ),
    pin!(
        shape: InputShape::Project;
        FailureClass::Name,
        "ai regress",
        AI_PROJECT_NAME_FIXTURE,
        AI_REGRESS_FALSE_GREEN,
        "#800"
    ),
    pin!(
        shape: InputShape::Component;
        FailureClass::Guard,
        "ai replay",
        AI_GUARD_FIXTURE,
        AI_REPLAY_FALSE_GREEN,
        "#800"
    ),
    pin!(
        shape: InputShape::Component;
        FailureClass::Name,
        "ai replay",
        AI_NAME_FIXTURE,
        AI_REPLAY_FALSE_GREEN,
        "#800"
    ),
    pin!(
        shape: InputShape::Project;
        FailureClass::Guard,
        "ai replay",
        AI_PROJECT_GUARD_FIXTURE,
        AI_REPLAY_FALSE_GREEN,
        "#800"
    ),
    pin!(
        shape: InputShape::Project;
        FailureClass::Name,
        "ai replay",
        AI_PROJECT_NAME_FIXTURE,
        AI_REPLAY_FALSE_GREEN,
        "#800"
    ),
    pin!(
        shape: InputShape::Project;
        FailureClass::Guard,
        "ai check",
        AI_PROJECT_GUARD_FIXTURE,
        AI_PROJECT_CHECK_FALSE_GREEN,
        "#800"
    ),
    pin!(
        shape: InputShape::Project;
        FailureClass::Name,
        "ai check",
        AI_PROJECT_NAME_FIXTURE,
        AI_PROJECT_CHECK_FALSE_GREEN,
        "#800"
    ),
    pin!(
        FailureClass::Guard,
        "domain generate",
        GUARD_FIXTURE,
        SEMANTIC_WITH_INPUT_PATH_WITHOUT_LOCATION,
        "#773"
    ),
    pin!(
        FailureClass::Guard,
        "domain analyze",
        GUARD_FIXTURE,
        SEMANTIC_WITHOUT_INPUT_PATH,
        "#780"
    ),
    pin!(
        FailureClass::Guard,
        "domain expand",
        GUARD_FIXTURE,
        SEMANTIC_WITHOUT_INPUT_PATH,
        "#780"
    ),
    pin!(
        FailureClass::Guard,
        "kernel",
        GUARD_FIXTURE,
        SEMANTIC_WITHOUT_INPUT_PATH,
        "#780"
    ),
    pin!(
        FailureClass::Guard,
        "compat check",
        GUARD_FIXTURE,
        SEMANTIC_WITHOUT_INPUT_PATH_WITHOUT_LOCATION,
        "#780"
    ),
    pin!(
        FailureClass::Guard,
        "fmt",
        GUARD_FIXTURE,
        SEMANTIC_WITH_INPUT_PATH_WITHOUT_LOCATION,
        "#780"
    ),
    pin!(
        FailureClass::Guard,
        "lint",
        GUARD_FIXTURE,
        SEMANTIC_WITH_INPUT_PATH_WITHOUT_LOCATION,
        "#780"
    ),
    pin!(
        FailureClass::Guard,
        "migrate",
        GUARD_FIXTURE,
        SEMANTIC_WITH_INPUT_PATH_WITHOUT_LOCATION,
        "#780"
    ),
    pin!(
        FailureClass::Guard,
        "typestate",
        GUARD_FIXTURE,
        SEMANTIC_WITHOUT_INPUT_PATH,
        "#780"
    ),
    pin!(
        FailureClass::Name,
        "domain analyze",
        NAME_FIXTURE,
        ANALYZED_NAME_FALSE_GREEN,
        "#796"
    ),
    pin!(
        FailureClass::Name,
        "domain expand",
        NAME_FIXTURE,
        EXPANDED_NAME_FALSE_GREEN,
        "#796"
    ),
    pin!(
        FailureClass::Name,
        "domain generate",
        NAME_FIXTURE,
        SEMANTIC_WITH_INPUT_PATH_WITHOUT_LOCATION,
        "#773"
    ),
    pin!(
        FailureClass::Name,
        "compat check",
        NAME_FIXTURE,
        SEMANTIC_WITHOUT_INPUT_PATH_WITHOUT_LOCATION,
        "#780"
    ),
    pin!(
        FailureClass::Name,
        "fmt",
        NAME_FIXTURE,
        SEMANTIC_WITH_INPUT_PATH_WITHOUT_LOCATION,
        "#780"
    ),
    pin!(
        FailureClass::Name,
        "kernel",
        NAME_FIXTURE,
        SEMANTIC_WITHOUT_INPUT_PATH,
        "#780"
    ),
    pin!(
        FailureClass::Name,
        "lint",
        NAME_FIXTURE,
        SEMANTIC_WITH_INPUT_PATH_WITHOUT_LOCATION,
        "#780"
    ),
    pin!(
        FailureClass::Name,
        "migrate",
        NAME_FIXTURE,
        SEMANTIC_WITH_INPUT_PATH_WITHOUT_LOCATION,
        "#780"
    ),
    pin!(
        FailureClass::Name,
        "typestate",
        NAME_FIXTURE,
        SEMANTIC_WITHOUT_INPUT_PATH,
        "#780"
    ),
    pin!(
        FailureClass::Literate,
        "domain check",
        LITERATE_FIXTURE,
        SEMANTIC_WITH_INPUT_PATH_WITHOUT_LOCATION,
        "#694"
    ),
    pin!(
        FailureClass::Literate,
        "domain analyze",
        LITERATE_FIXTURE,
        SEMANTIC_WITH_INPUT_PATH_WITHOUT_LOCATION,
        "#694"
    ),
    pin!(
        FailureClass::Literate,
        "domain expand",
        LITERATE_FIXTURE,
        SEMANTIC_WITH_INPUT_PATH_WITHOUT_LOCATION,
        "#694"
    ),
    pin!(
        FailureClass::Literate,
        "domain generate",
        LITERATE_FIXTURE,
        SEMANTIC_WITH_INPUT_PATH_WITHOUT_LOCATION,
        "#694"
    ),
    pin!(
        FailureClass::Literate,
        "domain replay",
        LITERATE_FIXTURE,
        SEMANTIC_WITH_INPUT_PATH_WITHOUT_LOCATION,
        "#694"
    ),
    pin!(
        FailureClass::Literate,
        "domain testgen",
        LITERATE_FIXTURE,
        SEMANTIC_WITH_INPUT_PATH_WITHOUT_LOCATION,
        "#694"
    ),
    pin!(
        FailureClass::Literate,
        "db check",
        LITERATE_FIXTURE,
        SEMANTIC_WITH_INPUT_PATH_WITHOUT_LOCATION,
        "#694"
    ),
    pin!(
        FailureClass::Literate,
        "db observe",
        LITERATE_FIXTURE,
        SEMANTIC_WITH_INPUT_PATH_WITHOUT_LOCATION,
        "#694"
    ),
    pin!(
        FailureClass::Literate,
        "ai check",
        LITERATE_FIXTURE,
        SEMANTIC_WITH_INPUT_PATH_WITHOUT_LOCATION,
        "#694"
    ),
    pin!(
        FailureClass::Literate,
        "ai compat",
        LITERATE_FIXTURE,
        SEMANTIC_WITH_INPUT_PATH_WITHOUT_LOCATION,
        "#694"
    ),
    pin!(
        FailureClass::Literate,
        "ai drift",
        LITERATE_FIXTURE,
        SEMANTIC_WITHOUT_INPUT_PATH_WITHOUT_LOCATION,
        "#694"
    ),
    pin!(
        FailureClass::Literate,
        "ai eval",
        LITERATE_FIXTURE,
        SEMANTIC_WITHOUT_INPUT_PATH_WITHOUT_LOCATION,
        "#694"
    ),
    pin!(
        FailureClass::Literate,
        "ai regress",
        LITERATE_FIXTURE,
        SEMANTIC_WITHOUT_INPUT_PATH_WITHOUT_LOCATION,
        "#694"
    ),
    pin!(
        FailureClass::Literate,
        "ai replay",
        LITERATE_FIXTURE,
        SEMANTIC_WITH_INPUT_PATH_WITHOUT_LOCATION,
        "#694"
    ),
    pin!(
        FailureClass::Literate,
        "compat check",
        LITERATE_FIXTURE,
        SEMANTIC_WITH_INPUT_PATH_WITHOUT_LOCATION,
        "#694"
    ),
    pin!(
        FailureClass::Literate,
        "causal check",
        LITERATE_FIXTURE,
        PARSE_WITH_DIAGNOSTIC_ALIAS,
        "#694"
    ),
    pin!(
        FailureClass::Literate,
        "causal verify-expectations",
        LITERATE_FIXTURE,
        CAUSAL_PARSE_WITHOUT_DIAGNOSTIC,
        "#694"
    ),
    pin!(
        FailureClass::Literate,
        "causal analyze",
        LITERATE_FIXTURE,
        PARSE_WITH_DIAGNOSTIC_ALIAS,
        "#694"
    ),
    pin!(
        FailureClass::Literate,
        "causal diff",
        LITERATE_FIXTURE,
        PARSE_WITH_DIAGNOSTIC_ALIAS,
        "#694"
    ),
    pin!(
        FailureClass::Literate,
        "causal ledger",
        LITERATE_FIXTURE,
        PARSE_WITH_DIAGNOSTIC_ALIAS,
        "#694"
    ),
    pin!(
        FailureClass::Literate,
        "causal observe-expectations",
        LITERATE_FIXTURE,
        CAUSAL_PARSE_WITHOUT_DIAGNOSTIC,
        "#694"
    ),
    pin!(
        FailureClass::Literate,
        "approval check",
        LITERATE_FIXTURE,
        SEMANTIC_WITHOUT_INPUT_PATH_WITHOUT_LOCATION,
        "#694"
    ),
    pin!(
        FailureClass::Literate,
        "approval create",
        LITERATE_FIXTURE,
        PARSE_UNIFORM,
        "#694"
    ),
    pin!(
        FailureClass::Literate,
        "approval diff",
        LITERATE_FIXTURE,
        PARSE_UNIFORM,
        "#694"
    ),
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
    shape: InputShape,
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
                    shape: coverage_input_shape(entry.key, coverage.fixture),
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
                    shape: InputShape::Source,
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

fn has_literate_cell(entry: &CommandRegistration) -> bool {
    matches!(
        entry.literate,
        LiterateCoverage::UniformUnsupported | LiterateCoverage::PinnedDialect
    )
}

fn has_literate_not_applicable(entry: &CommandRegistration) -> bool {
    matches!(
        entry.literate,
        LiterateCoverage::NotApplicable { .. } | LiterateCoverage::Supported { .. }
    )
}

fn coverage_input_shape(command: &str, fixture: &str) -> InputShape {
    if command.starts_with("ai ") {
        if fixture == AI_PROJECT_GUARD_FIXTURE || fixture == AI_PROJECT_NAME_FIXTURE {
            InputShape::Project
        } else {
            InputShape::Component
        }
    } else {
        InputShape::Source
    }
}

fn required_input_shapes(
    entry: &CommandRegistration,
    class: FailureClass,
) -> &'static [InputShape] {
    const SOURCE: &[InputShape] = &[InputShape::Source];
    const AI_COMPONENT: &[InputShape] = &[InputShape::Component];
    const AI_SEMANTIC: &[InputShape] = &[InputShape::Component, InputShape::Project];

    if matches!(entry.scope, ParityScope::SpecPath { .. }) && entry.key.starts_with("ai ") {
        match class {
            FailureClass::Parse => AI_COMPONENT,
            FailureClass::Guard | FailureClass::Name => AI_SEMANTIC,
            FailureClass::Literate => SOURCE,
        }
    } else {
        SOURCE
    }
}

fn class_classification_count(
    entry: &CommandRegistration,
    class: FailureClass,
    shape: InputShape,
) -> usize {
    entry
        .coverage
        .iter()
        .filter(|coverage| {
            coverage.class == class && coverage_input_shape(entry.key, coverage.fixture) == shape
        })
        .count()
        + usize::from(
            shape == InputShape::Source
                && class == FailureClass::Literate
                && has_literate_cell(entry),
        )
        + usize::from(
            shape == InputShape::Source
                && class == FailureClass::Literate
                && has_literate_not_applicable(entry),
        )
        + entry
            .not_applicable
            .iter()
            .filter(|not_applicable| shape == InputShape::Source && not_applicable.class == class)
            .count()
}

const APPROVAL_BASELINE: &str = include_str!("../../../tests/fixtures/approval.fsl");

/// A test-owned repository is the only Git surface these tests use. Its
/// destructor also handles `fslc` spawn panics, which the former bare record
/// file cleanup could not.
struct ApprovalFixture {
    root: PathBuf,
    record: PathBuf,
}

impl ApprovalFixture {
    fn new(fixture: &str) -> Self {
        let sequence = APPROVAL_RECORD_SEQUENCE.fetch_add(1, Ordering::Relaxed);
        let root = std::env::temp_dir().join(format!(
            "fslc-error-envelope-approval-{}-{sequence}",
            std::process::id()
        ));
        std::fs::create_dir(&root)
            .unwrap_or_else(|error| panic!("create {}: {error}", root.display()));
        let approval = Self {
            record: root.join("approval.json"),
            root,
        };
        approval.write("spec.fsl", APPROVAL_BASELINE);
        approval.git(&["init", "-q"]);
        approval.git(&["config", "user.email", "envelope-parity@example.com"]);
        approval.git(&["config", "user.name", "Envelope Parity"]);
        approval.git(&["config", "commit.gpgsign", "false"]);
        approval.git(&["add", "spec.fsl"]);
        approval.git(&["commit", "-qm", "approval baseline"]);
        approval.run_success(&["ledger", "spec.fsl", "--depth", "1", "-o", "ledger.md"]);
        approval.run_success(&[
            "approval",
            "create",
            "spec.fsl",
            "--kind",
            "ledger",
            "--artifact",
            "ledger.md",
            "--approver",
            "envelope-parity",
            "--depth",
            "1",
            "-o",
            "approval.json",
        ]);
        approval.write(
            "spec.fsl",
            &std::fs::read_to_string(workspace_root().join(fixture))
                .unwrap_or_else(|error| panic!("read {fixture}: {error}")),
        );
        approval
    }

    fn write(&self, path: &str, contents: &str) {
        std::fs::write(self.root.join(path), contents)
            .unwrap_or_else(|error| panic!("write {}/{}: {error}", self.root.display(), path));
    }

    fn git(&self, arguments: &[&str]) {
        let output = Command::new("git")
            .args(arguments)
            .current_dir(&self.root)
            .output()
            .unwrap_or_else(|error| panic!("run test-owned git {arguments:?}: {error}"));
        assert!(
            output.status.success(),
            "test-owned git {arguments:?} failed: {}",
            String::from_utf8_lossy(&output.stderr)
        );
    }

    fn run_success(&self, arguments: &[&str]) {
        let output = Command::new(env!("CARGO_BIN_EXE_fslc"))
            .args(arguments)
            .current_dir(&self.root)
            .output()
            .unwrap_or_else(|error| panic!("run approval setup {arguments:?}: {error}"));
        assert!(
            output.status.success(),
            "approval setup {arguments:?} failed: {}",
            String::from_utf8_lossy(&output.stdout)
        );
    }

    fn zero_digest_record(&self) -> PathBuf {
        let mut record: Value = serde_json::from_slice(
            &std::fs::read(&self.record)
                .unwrap_or_else(|error| panic!("read {}: {error}", self.record.display())),
        )
        .expect("approval record JSON");
        record["spec"]["digest"] = Value::String(format!("sha256:{}", "0".repeat(64)));
        let path = self.root.join("approval-zero-digest.json");
        std::fs::write(
            &path,
            serde_json::to_vec(&record).expect("serialize zero-digest record"),
        )
        .unwrap_or_else(|error| panic!("write {}: {error}", path.display()));
        path
    }
}

impl Drop for ApprovalFixture {
    fn drop(&mut self) {
        let _ = std::fs::remove_dir_all(&self.root);
    }
}

fn test_git_available() -> bool {
    Command::new("git")
        .arg("--version")
        .output()
        .is_ok_and(|output| output.status.success())
}

fn require_test_git() {
    assert!(
        test_git_available(),
        "error-envelope parity requires a Git executable for its approval cells and calibration"
    );
}

fn approval_command(command: &str) -> bool {
    command.starts_with("approval ")
}

fn invoke(command: &str, fixture: &str, approval_record: Option<&Path>) -> Vec<String> {
    let ParityScope::SpecPath { invoke } = registration(command).scope else {
        panic!("{command} is not a runnable SpecPath matrix command");
    };
    invoke
        .iter()
        .map(|argument| {
            if *argument == SPEC_PLACEHOLDER {
                fixture.to_owned()
            } else if *argument == APPROVAL_RECORD_PLACEHOLDER {
                approval_record
                    .unwrap_or_else(|| panic!("{command} needs an approval record"))
                    .display()
                    .to_string()
            } else if *argument == DOCUMENT_ARTIFACT_PLACEHOLDER {
                DOCUMENT_ARTIFACT.to_owned()
            } else {
                (*argument).to_owned()
            }
        })
        .collect()
}

fn run_from(
    command: &str,
    fixture: &str,
    approval_record: Option<&Path>,
    current_dir: &Path,
) -> Actual {
    let arguments = invoke(command, fixture, approval_record);
    let output = Command::new(env!("CARGO_BIN_EXE_fslc"))
        .args(&arguments)
        .current_dir(current_dir)
        .output()
        .unwrap_or_else(|error| panic!("run {command} {arguments:?}: {error}"));
    Actual {
        json: serde_json::from_slice(&output.stdout).ok(),
        stdout: String::from_utf8(output.stdout)
            .unwrap_or_else(|error| panic!("{command} stdout is not UTF-8: {error}")),
        exit: output.status.code().expect("native exit status"),
    }
}

fn run(command: &str, fixture: &str) -> Actual {
    if approval_command(command) {
        let approval = ApprovalFixture::new(fixture);
        let needs_record = match registration(command).scope {
            ParityScope::SpecPath { invoke } => invoke.contains(&APPROVAL_RECORD_PLACEHOLDER),
            ParityScope::Excluded { .. } => false,
        };
        let record = needs_record.then_some(approval.record.as_path());
        return run_from(command, "spec.fsl", record, &approval.root);
    }
    run_from(command, fixture, None, &workspace_root())
}

fn matches_expectation(actual: &Actual, expected: Expectation, fixture: &str) -> bool {
    match expected {
        Expectation::Text { exit, exact_stdout } => {
            actual.exit == exit && actual.json.is_none() && actual.stdout == exact_stdout
        }
        Expectation::Json(expected) => {
            if actual.exit != expected.exit {
                return false;
            }
            let Some(output) = &actual.json else {
                return false;
            };
            let matches_field = |field: &str, expected: ExpectedField| match expected {
                ExpectedField::Absent => output.get(field).is_none(),
                ExpectedField::Exact(value) => {
                    output.get(field).and_then(Value::as_str) == Some(value)
                }
            };
            if !matches_field("result", expected.result)
                || !matches_field("kind", expected.kind)
                || !matches_field("dialect", expected.dialect)
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
            match expected.message {
                MessageExpectation::Absent => output.get("message").is_none(),
                MessageExpectation::MentionsInput => output["message"]
                    .as_str()
                    .is_some_and(|message| message.contains(fixture)),
                MessageExpectation::OmitsInput => output["message"]
                    .as_str()
                    .is_some_and(|message| !message.contains(fixture)),
            }
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
    uniform: Expectation,
    fixture: &str,
    class: FailureClass,
    command: &str,
) {
    assert!(
        !matches_expectation(actual, uniform, fixture),
        "{}/{command} now matches the uniform envelope: {} is {}. {}",
        known.issue,
        known.issue,
        known.resolution,
        actual.stdout
    );
    assert_expectation(actual, known.observed, fixture, class, command);
}

fn assert_cell(cell: Cell) {
    if approval_command(cell.command) {
        require_test_git();
    }
    let actual = run(cell.command, cell.fixture);
    let expected_input = if approval_command(cell.command) {
        "spec.fsl"
    } else {
        cell.fixture
    };
    let pins = KNOWN_ASYMMETRIES
        .iter()
        .filter(|known| {
            known.class == cell.class
                && known.command == cell.command
                && known.shape == cell.shape
                && known.fixture == cell.fixture
        })
        .collect::<Vec<_>>();
    assert!(
        pins.len() <= 1,
        "{:?}/{}/{:?}/{} has more than one known-asymmetry pin",
        cell.class,
        cell.command,
        cell.shape,
        cell.fixture
    );
    if let Some(known) = pins.first() {
        assert_known_asymmetry(
            &actual,
            known,
            cell.uniform,
            expected_input,
            cell.class,
            cell.command,
        );
    } else {
        assert_expectation(
            &actual,
            cell.uniform,
            expected_input,
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
            ParityScope::SpecPath { invoke } => {
                assert!(
                    invoke.contains(&SPEC_PLACEHOLDER),
                    "{} lacks a {SPEC_PLACEHOLDER} input slot",
                    entry.key
                );
                assert!(
                    !entry.coverage.is_empty(),
                    "{} is a SpecPath command but has no executable failure-class coverage",
                    entry.key
                );
            }
            ParityScope::Excluded { reason } => assert!(
                !reason.trim().is_empty(),
                "{} is excluded without a reason",
                entry.key
            ),
        }
        match entry.literate {
            LiterateCoverage::NotApplicable { reason } | LiterateCoverage::Supported { reason } => {
                assert!(
                    !reason.trim().is_empty(),
                    "{} marks Literate as not applicable without a reason",
                    entry.key
                );
            }
            LiterateCoverage::UniformUnsupported | LiterateCoverage::PinnedDialect => {}
        }
        for class in [
            FailureClass::Parse,
            FailureClass::Guard,
            FailureClass::Name,
            FailureClass::Literate,
        ] {
            for shape in required_input_shapes(entry, class) {
                assert_eq!(
                    class_classification_count(entry, class, *shape),
                    1,
                    "{} must classify {class:?}/{shape:?} as exactly one executable Cell or reasoned NotApplicable",
                    entry.key
                );
            }
        }
        for not_applicable in entry.not_applicable {
            assert!(
                !not_applicable.reason.trim().is_empty(),
                "{} marks {:?} NotApplicable without a concrete reason",
                entry.key,
                not_applicable.class
            );
        }
    }
}

#[test]
#[should_panic(expected = "exactly one executable Cell or reasoned NotApplicable")]
fn missing_classification_spec_path_is_rejected() {
    let empty_coverage = CommandRegistration {
        key: "test missing classification",
        scope: ParityScope::SpecPath {
            invoke: &["test", SPEC_PLACEHOLDER],
        },
        literate: LiterateCoverage::NotApplicable {
            reason: "calibration control",
        },
        coverage: NO_COVERAGE,
        not_applicable: &[],
    };
    assert_eq!(
        class_classification_count(&empty_coverage, FailureClass::Parse, InputShape::Source),
        1,
        "{} must classify Parse/Source as exactly one executable Cell or reasoned NotApplicable",
        empty_coverage.key
    );
}

#[test]
#[should_panic(expected = "has no executable failure-class coverage")]
fn all_reasoned_exclusions_spec_path_is_rejected() {
    let reasoned_only = CommandRegistration {
        key: "test all reasoned exclusions",
        scope: ParityScope::SpecPath {
            invoke: &["test", SPEC_PLACEHOLDER],
        },
        literate: LiterateCoverage::NotApplicable {
            reason: "calibration control",
        },
        coverage: NO_COVERAGE,
        not_applicable: NOT_APPLICABLE_PARSE_GUARD_NAME,
    };
    assert!(
        !reasoned_only.coverage.is_empty(),
        "{} is a SpecPath command but has no executable failure-class coverage",
        reasoned_only.key
    );
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
fn unresolved_identifier_errors_are_uniform_or_pinned_across_frontend_siblings() {
    for cell in cells(FailureClass::Name) {
        assert_cell(cell);
    }
}

#[test]
fn approval_diff_uses_a_valid_baseline_before_exercising_each_failure_class() {
    require_test_git();

    for (fixture, kind, diagnostic, message) in [
        (
            PARSE_KERNEL_FIXTURE,
            "parse",
            Some("FSL-PARSE"),
            "expected expression",
        ),
        (
            GUARD_FIXTURE,
            "semantics",
            None,
            "top-level await 'PaymentResult' has no executable lowering",
        ),
        (
            NAME_FIXTURE,
            "semantics",
            None,
            "unknown domain symbol 'missing_status'",
        ),
    ] {
        let approval = ApprovalFixture::new(fixture);
        let actual = run_from(
            "approval diff",
            "spec.fsl",
            Some(&approval.record),
            &approval.root,
        );
        assert_eq!(actual.exit, 2, "{fixture}: {}", actual.stdout);
        let output = actual.json.expect("approval diff JSON envelope");
        assert_eq!(output["kind"], kind, "{fixture}: {output}");
        assert_eq!(
            output.get("diagnostic_code").and_then(Value::as_str),
            diagnostic,
            "{fixture}: {output}"
        );
        assert!(
            output["message"]
                .as_str()
                .is_some_and(|actual_message| actual_message.contains(message)),
            "{fixture}: {output}"
        );
    }
}

#[test]
fn approval_diff_zero_digest_negative_control_stops_before_the_diff() {
    require_test_git();

    let approval = ApprovalFixture::new(NAME_FIXTURE);
    let zero_digest = approval.zero_digest_record();
    let actual = run_from(
        "approval diff",
        "spec.fsl",
        Some(&zero_digest),
        &approval.root,
    );
    assert_eq!(actual.exit, 2, "{}", actual.stdout);
    let output = actual.json.expect("approval diff JSON envelope");
    assert_eq!(output["kind"], "semantics", "{output}");
    assert!(output.get("loc").is_none(), "{output}");
    assert_eq!(
        output["message"],
        "approval baseline commit does not match the recorded specification digest",
        "{output}"
    );
    assert!(
        !matches_expectation(
            &Actual {
                stdout: actual.stdout,
                json: Some(output),
                exit: actual.exit,
            },
            SEMANTIC_UNIFORM,
            "spec.fsl"
        ),
        "a zero digest must not satisfy the approval-diff Name expectation"
    );
}

#[test]
fn name_matrix_keeps_five_uniform_countercontrols_and_three_pins() {
    let bounded_commands = [
        "check",
        "verify",
        "domain analyze",
        "domain check",
        "domain expand",
        "domain generate",
        "domain replay",
        "domain testgen",
    ];
    let name_cells = cells(FailureClass::Name)
        .into_iter()
        .filter(|cell| bounded_commands.contains(&cell.command))
        .collect::<Vec<_>>();
    let pinned = name_cells
        .iter()
        .filter(|cell| {
            KNOWN_ASYMMETRIES.iter().any(|pin| {
                pin.class == FailureClass::Name
                    && pin.command == cell.command
                    && pin.shape == cell.shape
                    && pin.fixture == cell.fixture
            })
        })
        .count();
    assert_eq!(
        name_cells.len(),
        8,
        "Name matrix must keep eight countercontrols"
    );
    assert_eq!(pinned, 3, "Name matrix must keep three self-retiring pins");
    assert_eq!(
        name_cells.len() - pinned,
        5,
        "Name matrix must keep five uniform countercontrols"
    );
}

#[test]
fn literate_input_errors_are_uniform_or_pinned_across_frontend_siblings() {
    for cell in cells(FailureClass::Literate) {
        assert_cell(cell);
    }
}

#[test]
fn every_pin_is_a_registered_matrix_command_with_a_tracking_issue() {
    let mut exact_cell_keys = BTreeSet::new();
    for class in [
        FailureClass::Parse,
        FailureClass::Guard,
        FailureClass::Name,
        FailureClass::Literate,
    ] {
        for cell in cells(class) {
            assert!(
                exact_cell_keys.insert((
                    cell.class,
                    cell.command,
                    cell.shape,
                    cell.fixture,
                    cell.uniform
                )),
                "duplicate matrix cell: {:?}/{}/{:?}/{}",
                cell.class,
                cell.command,
                cell.shape,
                cell.fixture
            );
        }
    }
    let mut pin_keys = BTreeSet::new();
    for pin in KNOWN_ASYMMETRIES {
        assert!(
            pin_keys.insert((pin.class, pin.command, pin.shape, pin.fixture)),
            "duplicate known-asymmetry pin: {:?}/{}/{:?}/{}",
            pin.class,
            pin.command,
            pin.shape,
            pin.fixture
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
        let matching_cells = cells(pin.class)
            .into_iter()
            .filter(|cell| {
                cell.command == pin.command
                    && cell.shape == pin.shape
                    && cell.fixture == pin.fixture
            })
            .collect::<Vec<_>>();
        assert_eq!(
            matching_cells.len(),
            1,
            "{:?}/{}/{:?}/{} is pinned but not an executable matrix cell",
            pin.class,
            pin.command,
            pin.shape,
            pin.fixture
        );
        let cell = matching_cells[0];
        assert!(
            exact_cell_keys.contains(&(
                pin.class,
                pin.command,
                pin.shape,
                pin.fixture,
                cell.uniform
            )),
            "{:?}/{}/{:?}/{} has a pin whose complete key is absent from the matrix",
            pin.class,
            pin.command,
            pin.shape,
            pin.fixture
        );
    }
}
