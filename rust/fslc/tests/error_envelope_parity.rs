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

use std::collections::{BTreeMap, BTreeSet};
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
const PARSE_AI_PROJECT_FIXTURE: &str =
    "rust/fslc/tests/fixtures/error_envelope_broken_ai_project.fsl";
const PARSE_AI_PROJECT_DUPLICATE_FIXTURE: &str =
    "rust/fslc/tests/fixtures/error_envelope_duplicate_ai_project.fsl";
const PARSE_CAUSAL_FIXTURE: &str = "rust/fslc/tests/fixtures/error_envelope_broken_causal.fsl";
const PARSE_APPROVAL_REQUIREMENTS_DOCUMENT_FIXTURE: &str =
    "rust/fslc/tests/fixtures/error_envelope_broken_approval_requirements_document.fsl";
const AI_GUARD_FIXTURE: &str = "rust/fslc/tests/fixtures/error_envelope_ai_invalid_rule.fsl";
const AI_NAME_FIXTURE: &str = "rust/fslc/tests/fixtures/error_envelope_ai_unknown_tool.fsl";
const EMPTY_MEMBERSHIP_FIXTURE: &str = "rust/fslc/tests/fixtures/domain_empty_membership.fsl";
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
const LITERATE_AI_COMPONENT_FIXTURE: &str =
    "rust/fslc/tests/fixtures/error_envelope_literate_ai_component.md";
const LITERATE_AI_PROJECT_FIXTURE: &str =
    "rust/fslc/tests/fixtures/error_envelope_literate_ai_project.md";
const DOMAIN_REPLAY_LOG: &str = "rust/fslc/tests/fixtures/issue_518_clean.jsonl";
const EMPTY_RECORDS: &str = "rust/fslc/tests/fixtures/error_envelope_empty_records.json";
const DOCUMENT_ARTIFACT: &str = "rust/fslc/tests/fixtures/error_envelope_document.md";
const COUNTEREXAMPLE_OUTPUT: &str = "rust/target/counterexample-export-parity.json";
const REPLAY_TRACE: &str = "rust/fslc/tests/fixtures/replay_trace.valid.v1.json";
const APPROVAL_RECORD_PLACEHOLDER: &str = "{approval-record}";
const DOCUMENT_ARTIFACT_PLACEHOLDER: &str = "{document-artifact}";
const COUNTEREXAMPLE_OUTPUT_PLACEHOLDER: &str = "{counterexample-output}";

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
    /// A registered command whose Markdown behavior is a known, temporary
    /// asymmetry from the uniform envelope, tracked by a `KNOWN_ASYMMETRIES`
    /// pin. #980 retired the last real user of this variant (the `approval`
    /// commands); it remains for calibration coverage
    /// (`missing_ai_parse_project_classification_is_rejected`).
    Pinned,
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
    shape: InputShape,
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
        literate: LiterateCoverage::UniformUnsupported,
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
        literate: LiterateCoverage::UniformUnsupported,
        coverage: AI_COMPAT_COVERAGE,
        not_applicable: &[],
    },
    CommandRegistration {
        key: "ai drift",
        scope: ParityScope::SpecPath {
            invoke: &["ai", "drift", SPEC_PLACEHOLDER, "--logs", EMPTY_RECORDS],
        },
        literate: LiterateCoverage::NotApplicable {
            reason: "load_ai_project's .md branch calls parse_ai_project independently of literate_access; valid literate AI project .md succeeds and other Markdown gets a clean semantic error, not the #665/#694 lie",
        },
        coverage: AI_DRIFT_COVERAGE,
        not_applicable: AI_DRIFT_COMPONENT_NOT_APPLICABLE,
    },
    CommandRegistration {
        key: "ai eval",
        scope: ParityScope::SpecPath {
            invoke: &["ai", "eval", SPEC_PLACEHOLDER, "--records", EMPTY_RECORDS],
        },
        literate: LiterateCoverage::NotApplicable {
            reason: "load_ai_project's .md branch calls parse_ai_project independently of literate_access; valid literate AI project .md succeeds and other Markdown gets a clean semantic error, not the #665/#694 lie",
        },
        coverage: AI_EVAL_COVERAGE,
        not_applicable: AI_EVAL_COMPONENT_NOT_APPLICABLE,
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
        literate: LiterateCoverage::NotApplicable {
            reason: "load_ai_project's .md branch calls parse_ai_project independently of literate_access; valid literate AI project .md succeeds and other Markdown gets a clean semantic error, not the #665/#694 lie",
        },
        coverage: AI_REGRESS_COVERAGE,
        not_applicable: AI_REGRESS_COMPONENT_NOT_APPLICABLE,
    },
    CommandRegistration {
        key: "ai replay",
        scope: ParityScope::SpecPath {
            invoke: &["ai", "replay", SPEC_PLACEHOLDER, "--logs", EMPTY_RECORDS],
        },
        literate: LiterateCoverage::UniformUnsupported,
        coverage: AI_REPLAY_COVERAGE,
        not_applicable: &[],
    },
    CommandRegistration {
        key: "analyze",
        scope: ParityScope::SpecPath {
            invoke: &["analyze", SPEC_PLACEHOLDER],
        },
        literate: LiterateCoverage::UniformUnsupported,
        coverage: AI_LOWERING_KERNEL_COVERAGE,
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
        literate: LiterateCoverage::UniformUnsupported,
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
        literate: LiterateCoverage::UniformUnsupported,
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
        literate: LiterateCoverage::UniformUnsupported,
        coverage: APPROVAL_DIFF_COVERAGE,
        not_applicable: &[],
    },
    CommandRegistration {
        key: "causal analyze",
        scope: ParityScope::SpecPath {
            invoke: &["causal", "analyze", SPEC_PLACEHOLDER],
        },
        literate: LiterateCoverage::UniformUnsupported,
        coverage: CAUSAL_COVERAGE,
        not_applicable: &[],
    },
    CommandRegistration {
        key: "causal check",
        scope: ParityScope::SpecPath {
            invoke: &["causal", "check", SPEC_PLACEHOLDER],
        },
        literate: LiterateCoverage::UniformUnsupported,
        coverage: CAUSAL_COVERAGE,
        not_applicable: &[],
    },
    CommandRegistration {
        key: "causal diff",
        scope: ParityScope::SpecPath {
            invoke: &["causal", "diff", SPEC_PLACEHOLDER, SPEC_PLACEHOLDER],
        },
        literate: LiterateCoverage::UniformUnsupported,
        coverage: CAUSAL_COVERAGE,
        not_applicable: &[],
    },
    CommandRegistration {
        key: "causal ledger",
        scope: ParityScope::SpecPath {
            invoke: &["causal", "ledger", SPEC_PLACEHOLDER],
        },
        literate: LiterateCoverage::UniformUnsupported,
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
        literate: LiterateCoverage::UniformUnsupported,
        coverage: CAUSAL_COVERAGE,
        not_applicable: &[],
    },
    CommandRegistration {
        key: "causal verify-expectations",
        scope: ParityScope::SpecPath {
            invoke: &["causal", "verify-expectations", SPEC_PLACEHOLDER],
        },
        literate: LiterateCoverage::UniformUnsupported,
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
        not_applicable: CHECK_PARSE_SHAPE_BOUNDARIES,
    },
    CommandRegistration {
        key: "compat check",
        scope: ParityScope::SpecPath {
            invoke: &["compat", "check", SPEC_PLACEHOLDER],
        },
        literate: LiterateCoverage::UniformUnsupported,
        coverage: PARSE_KERNEL_COVERAGE,
        not_applicable: &[],
    },
    CommandRegistration {
        key: "conformance",
        scope: ParityScope::SpecPath {
            invoke: &["conformance", SPEC_PLACEHOLDER],
        },
        literate: LiterateCoverage::UniformUnsupported,
        coverage: AI_LOWERING_KERNEL_COVERAGE,
        not_applicable: &[],
    },
    CommandRegistration {
        key: "counterexample export",
        scope: ParityScope::SpecPath {
            invoke: &[
                "counterexample",
                "export",
                SPEC_PLACEHOLDER,
                "--depth",
                "4",
                "-o",
                COUNTEREXAMPLE_OUTPUT_PLACEHOLDER,
            ],
        },
        literate: LiterateCoverage::UniformUnsupported,
        coverage: AI_LOWERING_KERNEL_COVERAGE,
        not_applicable: &[],
    },
    CommandRegistration {
        key: "db check",
        scope: ParityScope::SpecPath {
            invoke: &["db", "check", SPEC_PLACEHOLDER],
        },
        literate: LiterateCoverage::UniformUnsupported,
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
        literate: LiterateCoverage::UniformUnsupported,
        coverage: DB_OBSERVE_COVERAGE,
        not_applicable: &[],
    },
    CommandRegistration {
        key: "diff",
        scope: ParityScope::SpecPath {
            invoke: &["diff", SPEC_PLACEHOLDER, SPEC_PLACEHOLDER],
        },
        literate: LiterateCoverage::UniformUnsupported,
        coverage: AI_LOWERING_KERNEL_COVERAGE,
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
        literate: LiterateCoverage::UniformUnsupported,
        coverage: DOMAIN_COVERAGE,
        not_applicable: &[],
    },
    CommandRegistration {
        key: "domain check",
        scope: ParityScope::SpecPath {
            invoke: &["domain", "check", SPEC_PLACEHOLDER],
        },
        literate: LiterateCoverage::UniformUnsupported,
        coverage: DOMAIN_COVERAGE,
        not_applicable: &[],
    },
    CommandRegistration {
        key: "domain expand",
        scope: ParityScope::SpecPath {
            invoke: &["domain", "expand", SPEC_PLACEHOLDER],
        },
        literate: LiterateCoverage::UniformUnsupported,
        coverage: DOMAIN_COVERAGE,
        not_applicable: &[],
    },
    CommandRegistration {
        key: "domain generate",
        scope: ParityScope::SpecPath {
            invoke: &["domain", "generate", SPEC_PLACEHOLDER],
        },
        literate: LiterateCoverage::UniformUnsupported,
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
        literate: LiterateCoverage::UniformUnsupported,
        coverage: DOMAIN_COVERAGE,
        not_applicable: &[],
    },
    CommandRegistration {
        key: "domain testgen",
        scope: ParityScope::SpecPath {
            invoke: &["domain", "testgen", SPEC_PLACEHOLDER],
        },
        literate: LiterateCoverage::UniformUnsupported,
        coverage: DOMAIN_COVERAGE,
        not_applicable: &[],
    },
    CommandRegistration {
        key: "explain",
        scope: ParityScope::SpecPath {
            invoke: &["explain", SPEC_PLACEHOLDER],
        },
        literate: LiterateCoverage::UniformUnsupported,
        coverage: AI_LOWERING_KERNEL_COVERAGE,
        not_applicable: &[],
    },
    CommandRegistration {
        key: "fmt",
        scope: ParityScope::SpecPath {
            invoke: &["fmt", SPEC_PLACEHOLDER],
        },
        literate: LiterateCoverage::UniformUnsupported,
        coverage: AI_LOWERING_KERNEL_COVERAGE,
        not_applicable: &[],
    },
    CommandRegistration {
        key: "html",
        scope: ParityScope::SpecPath {
            invoke: &["html", SPEC_PLACEHOLDER],
        },
        literate: LiterateCoverage::UniformUnsupported,
        coverage: AI_LOWERING_KERNEL_COVERAGE,
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
        coverage: AI_LOWERING_KERNEL_COVERAGE,
        not_applicable: &[],
    },
    CommandRegistration {
        key: "lint",
        scope: ParityScope::SpecPath {
            invoke: &["lint", SPEC_PLACEHOLDER],
        },
        literate: LiterateCoverage::UniformUnsupported,
        coverage: AI_LOWERING_KERNEL_COVERAGE,
        not_applicable: &[],
    },
    CommandRegistration {
        key: "migrate",
        scope: ParityScope::SpecPath {
            invoke: &["migrate", SPEC_PLACEHOLDER],
        },
        literate: LiterateCoverage::UniformUnsupported,
        coverage: AI_LOWERING_KERNEL_COVERAGE,
        not_applicable: &[],
    },
    CommandRegistration {
        key: "mutate",
        scope: ParityScope::SpecPath {
            invoke: &["mutate", SPEC_PLACEHOLDER],
        },
        literate: LiterateCoverage::UniformUnsupported,
        coverage: MUTATE_COVERAGE,
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
        coverage: AI_LOWERING_KERNEL_COVERAGE,
        not_applicable: &[],
    },
    CommandRegistration {
        key: "replay",
        scope: ParityScope::SpecPath {
            invoke: &["replay", SPEC_PLACEHOLDER, "--trace", REPLAY_TRACE],
        },
        literate: LiterateCoverage::UniformUnsupported,
        coverage: AI_LOWERING_KERNEL_COVERAGE,
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
        coverage: AI_LOWERING_KERNEL_COVERAGE,
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
        coverage: AI_LOWERING_KERNEL_COVERAGE,
        not_applicable: &[],
    },
    CommandRegistration {
        key: "testplan",
        scope: ParityScope::SpecPath {
            invoke: &["testplan", SPEC_PLACEHOLDER],
        },
        literate: LiterateCoverage::UniformUnsupported,
        coverage: AI_LOWERING_KERNEL_COVERAGE,
        not_applicable: &[],
    },
    CommandRegistration {
        key: "typestate",
        scope: ParityScope::SpecPath {
            invoke: &["typestate", SPEC_PLACEHOLDER],
        },
        literate: LiterateCoverage::UniformUnsupported,
        coverage: AI_LOWERING_KERNEL_COVERAGE,
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
        not_applicable: CHECK_PARSE_SHAPE_BOUNDARIES,
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
/// `check` and `verify` additionally expose distinct AI-project and causal
/// parse frontends. `mutate` reaches the causal parse frontend through its
/// baseline verification. Requirements-document approval creation is a
/// separate `--kind` frontend selection. Compose nested-component parsing is
/// a documented semantic-resolution boundary, not a direct Parse cell.
/// AI Literate cells additionally retain the generic Markdown source form and
/// exercise Markdown documents whose extracted FSL body is each AI shape.
macro_rules! input_shapes {
    ($($variant:ident),+ $(,)?) => {
        #[derive(Clone, Copy, Debug, PartialEq, Eq, PartialOrd, Ord)]
        enum InputShape {
            $($variant,)+
        }

        impl InputShape {
            const ALL: &[Self] = &[$(Self::$variant,)+];

            const fn name(self) -> &'static str {
                match self {
                    $(Self::$variant => stringify!($variant),)+
                }
            }
        }
    };
}

// Keep the enum and the catalog's exhaustive inventory in one declaration.
// Adding a variant extends `ALL`, so the catalog's exact-once test fails until
// the new shape receives an explicit catalog row.
input_shapes!(
    Source,
    Component,
    Project,
    Causal,
    Compose,
    RequirementsDocument,
);

/// One semantic input-shape catalog for this matrix. The native frontend
/// registry is the authority for registered dialect keywords; this catalog
/// assigns every such keyword to exactly one envelope input shape and records
/// the three non-frontend shapes selected by production dispatch or command
/// arguments. `tests/dialect_registry.py` is deliberately not imported here:
/// it owns corpus/compatibility classification, whereas this catalog owns CLI
/// error-envelope input shapes. Its exact comparison with
/// `fsl_syntax::DIALECT_KEYWORDS` makes a new native dialect fail closed until
/// its envelope shape is reviewed.
#[derive(Clone, Copy)]
struct InputShapeDefinition {
    shape: InputShape,
    native_frontends: &'static [&'static str],
}

const INPUT_SHAPE_CATALOG: &[InputShapeDefinition] = &[
    InputShapeDefinition {
        shape: InputShape::Source,
        native_frontends: &[
            "spec",
            "refinement",
            "compose",
            "business",
            "governance",
            "requirements",
            "domain",
            "dbsystem",
            "agent",
        ],
    },
    InputShapeDefinition {
        shape: InputShape::Component,
        native_frontends: &["ai_component"],
    },
    // `is_ai_project` routes this legacy multi-declaration form before the
    // registered `ai_component` frontend.
    InputShapeDefinition {
        shape: InputShape::Project,
        native_frontends: &[],
    },
    // `is_causal_source` bypasses `fsl_syntax::DIALECT_KEYWORDS`.
    InputShapeDefinition {
        shape: InputShape::Causal,
        native_frontends: &[],
    },
    // A compose dependency is parsed during semantic resolution, not as the
    // parent document's top-level frontend.
    InputShapeDefinition {
        shape: InputShape::Compose,
        native_frontends: &[],
    },
    // `approval create --kind requirements_document` selects this frontend.
    InputShapeDefinition {
        shape: InputShape::RequirementsDocument,
        native_frontends: &[],
    },
];

/// The independent owner of each command/failure-class input population.
/// Coverage entries classify concrete fixtures below; they never decide which
/// shapes are required. Every CLI leaf has an explicit owner row: a new leaf
/// or semantic dispatch cannot acquire a generic Source population by naming
/// convention or fallback.
#[derive(Clone, Copy)]
struct InputShapeProfile {
    parse: &'static [InputShape],
    guard: &'static [InputShape],
    name: &'static [InputShape],
    literate: &'static [InputShape],
}

impl InputShapeProfile {
    const fn shapes(self, class: FailureClass) -> &'static [InputShape] {
        match class {
            FailureClass::Parse => self.parse,
            FailureClass::Guard => self.guard,
            FailureClass::Name => self.name,
            FailureClass::Literate => self.literate,
        }
    }
}

#[derive(Clone, Copy)]
struct CommandInputShapePopulation {
    command: &'static str,
    profile: InputShapeProfile,
}

const SOURCE_INPUT_SHAPES: &[InputShape] = &[InputShape::Source];
const AI_FSL_INPUT_SHAPES: &[InputShape] = &[InputShape::Component, InputShape::Project];
const AI_LITERATE_INPUT_SHAPES: &[InputShape] = &[
    InputShape::Source,
    InputShape::Component,
    InputShape::Project,
];
const CHECK_PARSE_INPUT_SHAPES: &[InputShape] = &[
    InputShape::Source,
    InputShape::Project,
    InputShape::Causal,
    InputShape::Compose,
];
const CHECK_NAME_INPUT_SHAPES: &[InputShape] = &[InputShape::Source, InputShape::Component];
const MUTATE_PARSE_INPUT_SHAPES: &[InputShape] = &[InputShape::Source, InputShape::Causal];
const APPROVAL_CREATE_PARSE_INPUT_SHAPES: &[InputShape] =
    &[InputShape::Source, InputShape::RequirementsDocument];

const SOURCE_INPUT_SHAPE_PROFILE: InputShapeProfile = InputShapeProfile {
    parse: SOURCE_INPUT_SHAPES,
    guard: SOURCE_INPUT_SHAPES,
    name: SOURCE_INPUT_SHAPES,
    literate: SOURCE_INPUT_SHAPES,
};
// #1464: these generic commands accept an AI component at the lowering gate,
// even though they have no specialized component/project dispatch of their own.
const AI_LOWERING_INPUT_SHAPE_PROFILE: InputShapeProfile = InputShapeProfile {
    name: CHECK_NAME_INPUT_SHAPES,
    ..SOURCE_INPUT_SHAPE_PROFILE
};
const AI_INPUT_SHAPE_PROFILE: InputShapeProfile = InputShapeProfile {
    parse: AI_FSL_INPUT_SHAPES,
    guard: AI_FSL_INPUT_SHAPES,
    name: AI_FSL_INPUT_SHAPES,
    literate: AI_LITERATE_INPUT_SHAPES,
};
const CHECK_INPUT_SHAPE_PROFILE: InputShapeProfile = InputShapeProfile {
    parse: CHECK_PARSE_INPUT_SHAPES,
    guard: &[InputShape::Source, InputShape::Project],
    name: &[
        InputShape::Source,
        InputShape::Component,
        InputShape::Project,
    ],
    ..SOURCE_INPUT_SHAPE_PROFILE
};
const VERIFY_INPUT_SHAPE_PROFILE: InputShapeProfile = InputShapeProfile {
    parse: CHECK_PARSE_INPUT_SHAPES,
    ..SOURCE_INPUT_SHAPE_PROFILE
};
const MUTATE_INPUT_SHAPE_PROFILE: InputShapeProfile = InputShapeProfile {
    parse: MUTATE_PARSE_INPUT_SHAPES,
    ..SOURCE_INPUT_SHAPE_PROFILE
};
const APPROVAL_CREATE_INPUT_SHAPE_PROFILE: InputShapeProfile = InputShapeProfile {
    parse: APPROVAL_CREATE_PARSE_INPUT_SHAPES,
    name: CHECK_NAME_INPUT_SHAPES,
    ..SOURCE_INPUT_SHAPE_PROFILE
};
/// `kernel` reaches the same AI-component frontend `check` does (issue
/// #1015), so it needs the same `Component` Name-class population `check`
/// declares in [`CHECK_NAME_INPUT_SHAPES`].
const KERNEL_INPUT_SHAPE_PROFILE: InputShapeProfile = InputShapeProfile {
    name: CHECK_NAME_INPUT_SHAPES,
    ..SOURCE_INPUT_SHAPE_PROFILE
};

// This is the closed set of commands whose production dispatch distinguishes
// fsl-ai component and project documents. It is intentionally not inferred
// from a command-name prefix: adding another semantic dispatch must choose an
// explicit population row and fixture classification below.
const AI_DISPATCH_COMMANDS: &[&str] = &[
    "ai check",
    "ai compat",
    "ai drift",
    "ai eval",
    "ai regress",
    "ai replay",
];

fn is_ai_dispatch_command(command: &str) -> bool {
    AI_DISPATCH_COMMANDS.contains(&command)
}

macro_rules! input_shape_population {
    ($command:literal, $profile:expr) => {
        CommandInputShapePopulation {
            command: $command,
            profile: $profile,
        }
    };
}

const INPUT_SHAPE_POPULATIONS: &[CommandInputShapePopulation] = &[
    input_shape_population!("ai check", AI_INPUT_SHAPE_PROFILE),
    input_shape_population!("ai compare", SOURCE_INPUT_SHAPE_PROFILE),
    input_shape_population!("ai compat", AI_INPUT_SHAPE_PROFILE),
    input_shape_population!("ai drift", AI_INPUT_SHAPE_PROFILE),
    input_shape_population!("ai eval", AI_INPUT_SHAPE_PROFILE),
    input_shape_population!("ai regress", AI_INPUT_SHAPE_PROFILE),
    input_shape_population!("ai replay", AI_INPUT_SHAPE_PROFILE),
    input_shape_population!("analyze", AI_LOWERING_INPUT_SHAPE_PROFILE),
    input_shape_population!("approval check", SOURCE_INPUT_SHAPE_PROFILE),
    input_shape_population!("approval create", APPROVAL_CREATE_INPUT_SHAPE_PROFILE),
    input_shape_population!("approval diff", SOURCE_INPUT_SHAPE_PROFILE),
    input_shape_population!("causal analyze", SOURCE_INPUT_SHAPE_PROFILE),
    input_shape_population!("causal check", SOURCE_INPUT_SHAPE_PROFILE),
    input_shape_population!("causal diff", SOURCE_INPUT_SHAPE_PROFILE),
    input_shape_population!("causal ledger", SOURCE_INPUT_SHAPE_PROFILE),
    input_shape_population!("causal observe-expectations", SOURCE_INPUT_SHAPE_PROFILE),
    input_shape_population!("causal verify-expectations", SOURCE_INPUT_SHAPE_PROFILE),
    input_shape_population!("chain", SOURCE_INPUT_SHAPE_PROFILE),
    input_shape_population!("check", CHECK_INPUT_SHAPE_PROFILE),
    input_shape_population!("compat check", SOURCE_INPUT_SHAPE_PROFILE),
    input_shape_population!("conformance", AI_LOWERING_INPUT_SHAPE_PROFILE),
    input_shape_population!("counterexample export", AI_LOWERING_INPUT_SHAPE_PROFILE),
    input_shape_population!("db check", SOURCE_INPUT_SHAPE_PROFILE),
    input_shape_population!("db import", SOURCE_INPUT_SHAPE_PROFILE),
    input_shape_population!("db observe", SOURCE_INPUT_SHAPE_PROFILE),
    input_shape_population!("diff", AI_LOWERING_INPUT_SHAPE_PROFILE),
    input_shape_population!("document check", AI_LOWERING_INPUT_SHAPE_PROFILE),
    input_shape_population!("document claims", AI_LOWERING_INPUT_SHAPE_PROFILE),
    input_shape_population!("document generate", AI_LOWERING_INPUT_SHAPE_PROFILE),
    input_shape_population!("domain analyze", SOURCE_INPUT_SHAPE_PROFILE),
    input_shape_population!("domain check", SOURCE_INPUT_SHAPE_PROFILE),
    input_shape_population!("domain expand", SOURCE_INPUT_SHAPE_PROFILE),
    input_shape_population!("domain generate", SOURCE_INPUT_SHAPE_PROFILE),
    input_shape_population!("domain replay", SOURCE_INPUT_SHAPE_PROFILE),
    input_shape_population!("domain testgen", SOURCE_INPUT_SHAPE_PROFILE),
    input_shape_population!("explain", AI_LOWERING_INPUT_SHAPE_PROFILE),
    input_shape_population!("fmt", AI_LOWERING_INPUT_SHAPE_PROFILE),
    input_shape_population!("html", AI_LOWERING_INPUT_SHAPE_PROFILE),
    input_shape_population!("kernel", KERNEL_INPUT_SHAPE_PROFILE),
    input_shape_population!("ledger", AI_LOWERING_INPUT_SHAPE_PROFILE),
    input_shape_population!("lint", AI_LOWERING_INPUT_SHAPE_PROFILE),
    input_shape_population!("migrate", AI_LOWERING_INPUT_SHAPE_PROFILE),
    input_shape_population!("mutate", MUTATE_INPUT_SHAPE_PROFILE),
    input_shape_population!("refine", AI_LOWERING_INPUT_SHAPE_PROFILE),
    input_shape_population!("replay", AI_LOWERING_INPUT_SHAPE_PROFILE),
    input_shape_population!("scenarios", AI_LOWERING_INPUT_SHAPE_PROFILE),
    input_shape_population!("sweep", SOURCE_INPUT_SHAPE_PROFILE),
    input_shape_population!("testgen", AI_LOWERING_INPUT_SHAPE_PROFILE),
    input_shape_population!("testplan", AI_LOWERING_INPUT_SHAPE_PROFILE),
    input_shape_population!("typestate", AI_LOWERING_INPUT_SHAPE_PROFILE),
    input_shape_population!("verify", VERIFY_INPUT_SHAPE_PROFILE),
    input_shape_population!("version", SOURCE_INPUT_SHAPE_PROFILE),
];

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
    /// #1464's CLI contract permits wrappers such as fmt/migrate to discard
    /// the lowering location. Core and LSP tests separately require its origin.
    OptionalLineColumn,
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
    Exact(&'static str),
    Contains(&'static str),
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
const AI_UNKNOWN_TOOL_SEMANTIC: Expectation = Expectation::Json(JsonExpectation {
    location: LocationShape::Absent,
    message: MessageExpectation::Exact("unknown tool 'MissingTool' in authority block"),
    ..SEMANTIC_JSON
});
const AI_UNKNOWN_TOOL_MESSAGE: &str = "unknown tool 'MissingTool' in authority block";
const AI_LOWERING_NAME_COVERAGE: FailureCoverage = FailureCoverage {
    class: FailureClass::Name,
    fixture: AI_NAME_FIXTURE,
    uniform: Expectation::Json(JsonExpectation {
        location: LocationShape::OptionalLineColumn,
        message: MessageExpectation::Contains(AI_UNKNOWN_TOOL_MESSAGE),
        ..SEMANTIC_JSON
    }),
};
// The projector rejects AI components before lowering. The former panic was
// in document_model_failure's diagnostic recovery, not an RCIR name check.
const AI_UNSUPPORTED_DOCUMENT_MESSAGE: &str =
    "document projection does not support dialect 'ai_component' in RCIR v1";
const AI_DOCUMENT_NAME_COVERAGE: FailureCoverage = FailureCoverage {
    class: FailureClass::Name,
    fixture: AI_NAME_FIXTURE,
    uniform: Expectation::Json(JsonExpectation {
        kind: ExpectedField::Exact("document"),
        location: LocationShape::Absent,
        dialect: ExpectedField::Exact("ai_component"),
        message: MessageExpectation::Exact(AI_UNSUPPORTED_DOCUMENT_MESSAGE),
        ..SEMANTIC_JSON
    }),
};

const LITERATE_UNIFORM: Expectation = Expectation::Json(JsonExpectation {
    result: ExpectedField::Exact("error"),
    kind: ExpectedField::Exact("usage"),
    location: LocationShape::FileOnly,
    diagnostic: Diagnostic::Code("FSL-INPUT-LITERATE-UNSUPPORTED"),
    exit: 2,
    dialect: ExpectedField::Absent,
    message: MessageExpectation::MentionsInput,
});

const CAUSAL_NAME_WITH_DIAGNOSTIC: Expectation = Expectation::Json(JsonExpectation {
    diagnostic: Diagnostic::Alias("causal_unknown_reference"),
    message: MessageExpectation::OmitsInput,
    ..SEMANTIC_JSON
});
/// #800 tracks these product false negatives for commands outside the
/// `check`/`compat`/`replay` fix in this change.
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

const NO_COVERAGE: &[FailureCoverage] = &[];
const NOT_APPLICABLE_PARSE_GUARD_NAME: &[NotApplicable] = &[
    NotApplicable {
        class: FailureClass::Parse,
        shape: InputShape::Source,
        reason: "this command has no FSL source frontend input",
    },
    NotApplicable {
        class: FailureClass::Guard,
        shape: InputShape::Source,
        reason: "this command has no FSL source frontend input",
    },
    NotApplicable {
        class: FailureClass::Name,
        shape: InputShape::Source,
        reason: "this command has no FSL source frontend input",
    },
];
const AI_DRIFT_COMPONENT_NOT_APPLICABLE: &[NotApplicable] = &[
    NotApplicable {
        class: FailureClass::Guard,
        shape: InputShape::Component,
        reason: "component input stops at missing observed_property selection before Guard validation",
    },
    NotApplicable {
        class: FailureClass::Name,
        shape: InputShape::Component,
        reason: "component input stops at missing observed_property selection before Name validation",
    },
];
const AI_EVAL_COMPONENT_NOT_APPLICABLE: &[NotApplicable] = &[
    NotApplicable {
        class: FailureClass::Guard,
        shape: InputShape::Component,
        reason: "component input stops at missing statistical_property selection before Guard validation",
    },
    NotApplicable {
        class: FailureClass::Name,
        shape: InputShape::Component,
        reason: "component input stops at missing statistical_property selection before Name validation",
    },
];
const AI_REGRESS_COMPONENT_NOT_APPLICABLE: &[NotApplicable] = &[
    NotApplicable {
        class: FailureClass::Guard,
        shape: InputShape::Component,
        reason: "component input stops at missing ai_migration selection before Guard validation",
    },
    NotApplicable {
        class: FailureClass::Name,
        shape: InputShape::Component,
        reason: "component input stops at missing ai_migration selection before Name validation",
    },
];
/// Component failures loaded from a `compose use ... from` declaration are
/// resolution failures of the parent document, not direct parser input. #567
/// independently asserts their semantic kind and parent `use` location across
/// spec-reading commands, so this Parse-shape boundary is deliberate.
const CHECK_PARSE_SHAPE_BOUNDARIES: &[NotApplicable] = &[NotApplicable {
    class: FailureClass::Parse,
    shape: InputShape::Compose,
    reason: "a nested component parse failure is reported as the parent compose document's semantic resolution error; issue_567_cross_file_diagnostic_loc asserts its semantic kind and parent use location across spec-reading commands",
}];
const AI_PARSE_COVERAGE: &[FailureCoverage] = &[
    FailureCoverage {
        class: FailureClass::Parse,
        fixture: PARSE_AI_FIXTURE,
        uniform: PARSE_UNIFORM,
    },
    FailureCoverage {
        class: FailureClass::Parse,
        fixture: PARSE_AI_PROJECT_FIXTURE,
        uniform: PARSE_UNIFORM,
    },
];
const AI_COMPAT_COVERAGE: &[FailureCoverage] = &[
    AI_PARSE_COVERAGE[0],
    AI_PARSE_COVERAGE[1],
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
    AI_PARSE_COVERAGE[1],
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
    AI_PARSE_COVERAGE[1],
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
    AI_PARSE_COVERAGE[1],
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
    AI_PARSE_COVERAGE[1],
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
    AI_PARSE_COVERAGE[1],
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
const AI_LOWERING_KERNEL_COVERAGE: &[FailureCoverage] = &[
    PARSE_KERNEL_COVERAGE[0],
    PARSE_KERNEL_COVERAGE[1],
    PARSE_KERNEL_COVERAGE[2],
    AI_LOWERING_NAME_COVERAGE,
];
const MUTATE_COVERAGE: &[FailureCoverage] = &[
    PARSE_KERNEL_COVERAGE[0],
    FailureCoverage {
        class: FailureClass::Parse,
        fixture: PARSE_CAUSAL_FIXTURE,
        uniform: PARSE_UNIFORM,
    },
    PARSE_KERNEL_COVERAGE[1],
    PARSE_KERNEL_COVERAGE[2],
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
    AI_DOCUMENT_NAME_COVERAGE,
];
const CHECK_COVERAGE: &[FailureCoverage] = &[
    FailureCoverage {
        class: FailureClass::Parse,
        fixture: PARSE_KERNEL_FIXTURE,
        uniform: PARSE_UNIFORM,
    },
    FailureCoverage {
        class: FailureClass::Parse,
        fixture: PARSE_AI_PROJECT_FIXTURE,
        uniform: PARSE_UNIFORM,
    },
    FailureCoverage {
        class: FailureClass::Parse,
        fixture: PARSE_CAUSAL_FIXTURE,
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
    FailureCoverage {
        class: FailureClass::Name,
        fixture: AI_NAME_FIXTURE,
        uniform: AI_UNKNOWN_TOOL_SEMANTIC,
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
const VERIFY_COVERAGE: &[FailureCoverage] = &[
    CHECK_COVERAGE[0],
    CHECK_COVERAGE[1],
    CHECK_COVERAGE[2],
    CHECK_COVERAGE[3],
    CHECK_COVERAGE[4],
];
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
        class: FailureClass::Parse,
        fixture: PARSE_APPROVAL_REQUIREMENTS_DOCUMENT_FIXTURE,
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
    AI_LOWERING_NAME_COVERAGE,
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
        uniform: CAUSAL_NAME_WITH_DIAGNOSTIC,
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
    // issue #1015: `kernel` reaches `lower_ai_component` through the same
    // shared lowering gate as `check`, so an authority block naming an
    // undeclared tool must return the same envelope `check` already pins
    // for this fixture in `CHECK_COVERAGE`, not panic before any JSON is
    // written.
    FailureCoverage {
        class: FailureClass::Name,
        fixture: AI_NAME_FIXTURE,
        uniform: AI_UNKNOWN_TOOL_SEMANTIC,
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
    // #800 tracks these product parse-path false negatives and envelope
    // differences. They are not accepted behavior: replay reports success
    // for a malformed project while its siblings reject it inconsistently.
    pin!(
        shape: InputShape::Project;
        FailureClass::Parse,
        "ai compat",
        PARSE_AI_PROJECT_FIXTURE,
        SEMANTIC_WITHOUT_INPUT_PATH_WITHOUT_LOCATION,
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
];

struct Actual {
    stdout: String,
    stderr: String,
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
                        LiterateCoverage::UniformUnsupported | LiterateCoverage::Pinned
                    )
                })
                .into_iter()
                .flat_map(move |entry| {
                    required_input_shapes(entry, class)
                        .iter()
                        .copied()
                        .map(move |shape| Cell {
                            class,
                            command: entry.key,
                            shape,
                            fixture: literate_fixture(entry.key, shape),
                            uniform: LITERATE_UNIFORM,
                        })
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
        LiterateCoverage::UniformUnsupported | LiterateCoverage::Pinned
    )
}

fn has_literate_not_applicable(entry: &CommandRegistration) -> bool {
    matches!(
        entry.literate,
        LiterateCoverage::NotApplicable { .. } | LiterateCoverage::Supported { .. }
    )
}

fn coverage_input_shape(command: &str, fixture: &str) -> InputShape {
    if fixture == AI_NAME_FIXTURE {
        return InputShape::Component;
    }
    if matches!(command, "check" | "mutate" | "verify") {
        return match fixture {
            PARSE_AI_PROJECT_FIXTURE
            | PARSE_AI_PROJECT_DUPLICATE_FIXTURE
            | AI_PROJECT_GUARD_FIXTURE
            | AI_PROJECT_NAME_FIXTURE => InputShape::Project,
            PARSE_CAUSAL_FIXTURE => InputShape::Causal,
            _ => InputShape::Source,
        };
    }
    if command == "approval create" && fixture == PARSE_APPROVAL_REQUIREMENTS_DOCUMENT_FIXTURE {
        return InputShape::RequirementsDocument;
    }
    if is_ai_dispatch_command(command) {
        if fixture == PARSE_AI_PROJECT_FIXTURE
            || fixture == PARSE_AI_PROJECT_DUPLICATE_FIXTURE
            || fixture == AI_PROJECT_GUARD_FIXTURE
            || fixture == AI_PROJECT_NAME_FIXTURE
        {
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
    INPUT_SHAPE_POPULATIONS
        .iter()
        .find(|population| population.command == entry.key)
        .map_or_else(
            || panic!("{} is absent from INPUT_SHAPE_POPULATIONS", entry.key),
            |population| population.profile.shapes(class),
        )
}

fn owner_contains(command: &str, class: FailureClass, shape: InputShape) -> bool {
    INPUT_SHAPE_POPULATIONS
        .iter()
        .find(|population| population.command == command)
        .is_some_and(|population| population.profile.shapes(class).contains(&shape))
}

fn literate_fixture(command: &str, shape: InputShape) -> &'static str {
    match (is_ai_dispatch_command(command), shape) {
        (true, InputShape::Component) => LITERATE_AI_COMPONENT_FIXTURE,
        (true, InputShape::Project) => LITERATE_AI_PROJECT_FIXTURE,
        (true | false, InputShape::Source) => LITERATE_FIXTURE,
        (true, shape) => panic!("ai literate input shape {shape:?} requires an explicit fixture"),
        (false, shape) => {
            panic!("non-AI literate input shape {shape:?} requires an explicit fixture")
        }
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
        + usize::from(class == FailureClass::Literate && has_literate_cell(entry))
        + usize::from(
            class == FailureClass::Literate
                && has_literate_not_applicable(entry)
                && required_input_shapes(entry, FailureClass::Literate).contains(&shape),
        )
        + entry
            .not_applicable
            .iter()
            .filter(|not_applicable| not_applicable.shape == shape && not_applicable.class == class)
            .count()
}

#[path = "support/scratch_identity.rs"]
mod scratch_identity;
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
        let root = scratch_identity::unique(root);
        std::fs::create_dir(&root)
            .unwrap_or_else(|error| panic!("create {}: {error}", root.display()));
        let approval = Self {
            record: root.join("approval.json"),
            root,
        };
        approval.write("spec.fsl", APPROVAL_BASELINE);
        approval.git(&["init", "-q"]);
        // Keep LF-written fixtures independent of the runner's Git defaults.
        approval.git(&["config", "core.autocrlf", "false"]);
        approval.git(&["config", "core.eol", "lf"]);
        approval.git(&["config", "user.email", "envelope-parity@example.com"]);
        approval.git(&["config", "user.name", "Envelope Parity"]);
        approval.git(&["config", "commit.gpgsign", "false"]);
        approval.git(&["add", "spec.fsl"]);
        approval.git(&["commit", "-qm", "approval baseline"]);
        approval.run_success(&["ledger", "spec.fsl", "--depth", "1", "-o", "ledger.md"]);
        approval.run_success(&[
            "document",
            "generate",
            "spec.fsl",
            "--lang",
            "ja",
            "-o",
            "requirements.md",
        ]);
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
            approval_positional_name(fixture),
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

/// The on-disk positional filename an approval fixture is written to.
///
/// `.fsl` fixtures keep the pre-existing `spec.fsl` name, matching the valid
/// baseline `ApprovalFixture::new` already committed and recorded. A `.md`
/// fixture (the Literate class) is written to a sibling `spec.md` instead of
/// overwriting `spec.fsl`, so the positional this test invokes actually has
/// the `.md` extension `literate_access` gates on -- overwriting `spec.fsl`'s
/// *content* with Markdown left the on-disk file `.fsl`-named, which never
/// reached the extension check this test exists to exercise (#980).
fn approval_positional_name(fixture: &str) -> &'static str {
    if Path::new(fixture)
        .extension()
        .and_then(std::ffi::OsStr::to_str)
        == Some("md")
    {
        "spec.md"
    } else {
        "spec.fsl"
    }
}

fn invoke(
    command: &str,
    shape_fixture: &str,
    spec_path: &str,
    approval_record: Option<&Path>,
) -> Vec<String> {
    let ParityScope::SpecPath { invoke } = registration(command).scope else {
        panic!("{command} is not a runnable SpecPath matrix command");
    };
    let invoke = if command == "approval create"
        && shape_fixture == PARSE_APPROVAL_REQUIREMENTS_DOCUMENT_FIXTURE
    {
        &[
            "approval",
            "create",
            SPEC_PLACEHOLDER,
            "--kind",
            "requirements_document",
            "--artifact",
            "requirements.md",
            "--approver",
            "parity",
        ]
    } else {
        invoke
    };
    invoke
        .iter()
        .map(|argument| {
            if *argument == SPEC_PLACEHOLDER {
                spec_path.to_owned()
            } else if *argument == APPROVAL_RECORD_PLACEHOLDER {
                approval_record
                    .unwrap_or_else(|| panic!("{command} needs an approval record"))
                    .display()
                    .to_string()
            } else if *argument == DOCUMENT_ARTIFACT_PLACEHOLDER {
                DOCUMENT_ARTIFACT.to_owned()
            } else if *argument == COUNTEREXAMPLE_OUTPUT_PLACEHOLDER {
                COUNTEREXAMPLE_OUTPUT.to_owned()
            } else {
                (*argument).to_owned()
            }
        })
        .collect()
}

fn assert_requirements_document_invocation(arguments: &[String]) {
    assert!(
        arguments
            .windows(2)
            .any(|pair| pair[0] == "--kind" && pair[1] == "requirements_document"),
        "requirements-document parse cell must invoke --kind requirements_document, got {arguments:?}"
    );
}

fn run_from(
    command: &str,
    shape_fixture: &str,
    spec_path: &str,
    approval_record: Option<&Path>,
    current_dir: &Path,
) -> Actual {
    let arguments = invoke(command, shape_fixture, spec_path, approval_record);
    run_arguments(command, &arguments, current_dir)
}

fn run_arguments(command: &str, arguments: &[String], current_dir: &Path) -> Actual {
    let output = Command::new(env!("CARGO_BIN_EXE_fslc"))
        .args(arguments)
        .current_dir(current_dir)
        .output()
        .unwrap_or_else(|error| panic!("run {command} {arguments:?}: {error}"));
    Actual {
        json: serde_json::from_slice(&output.stdout).ok(),
        stderr: String::from_utf8_lossy(&output.stderr).into_owned(),
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
        return run_from(
            command,
            fixture,
            approval_positional_name(fixture),
            record,
            &approval.root,
        );
    }
    run_from(command, fixture, fixture, None, &workspace_root())
}

fn matches_expectation(actual: &Actual, expected: Expectation, fixture: &str) -> bool {
    match expected {
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
                LocationShape::OptionalLineColumn => output.get("loc").is_none_or(|location| {
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
                MessageExpectation::Exact(message) => {
                    output.get("message").and_then(Value::as_str) == Some(message)
                }
                MessageExpectation::Contains(message) => output["message"]
                    .as_str()
                    .is_some_and(|actual| actual.contains(message)),
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
        approval_positional_name(cell.fixture)
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
            LiterateCoverage::UniformUnsupported | LiterateCoverage::Pinned => {}
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
                "{} marks {:?}/{:?} NotApplicable without a concrete reason",
                entry.key,
                not_applicable.class,
                not_applicable.shape
            );
        }
    }
}

#[test]
fn input_shape_catalog_is_total_over_native_dialect_registration() {
    let mut catalog_shape_counts = BTreeMap::new();
    let mut frontend_counts = BTreeMap::new();
    for definition in INPUT_SHAPE_CATALOG {
        *catalog_shape_counts
            .entry(definition.shape)
            .or_insert(0usize) += 1;
        for frontend in definition.native_frontends {
            *frontend_counts.entry(*frontend).or_insert(0usize) += 1;
        }
    }
    for shape in InputShape::ALL {
        assert_eq!(
            catalog_shape_counts.get(shape),
            Some(&1),
            "INPUT_SHAPE_CATALOG must contain {} exactly once",
            shape.name()
        );
    }
    assert_eq!(
        catalog_shape_counts.len(),
        InputShape::ALL.len(),
        "INPUT_SHAPE_CATALOG contains a shape outside InputShape::ALL"
    );

    let catalog_frontends = frontend_counts.keys().copied().collect::<BTreeSet<_>>();
    let native_frontends = fsl_syntax::DIALECT_KEYWORDS
        .iter()
        .copied()
        .collect::<BTreeSet<_>>();
    assert_eq!(
        catalog_frontends, native_frontends,
        "INPUT_SHAPE_CATALOG must classify every registered native dialect exactly once; \
         tests/dialect_registry.py is a separate corpus/compatibility projection, not this owner"
    );
    let duplicate_frontends = frontend_counts
        .iter()
        .filter_map(|(frontend, count)| (*count != 1).then_some((*frontend, *count)))
        .collect::<Vec<_>>();
    assert!(
        duplicate_frontends.is_empty(),
        "INPUT_SHAPE_CATALOG assigns native frontends more than once: {duplicate_frontends:?}"
    );
}

#[test]
fn input_shape_owner_is_bijective_with_cells_and_not_applicable_tuples() {
    let owner_commands = INPUT_SHAPE_POPULATIONS
        .iter()
        .map(|population| population.command)
        .collect::<BTreeSet<_>>();
    let registry_commands = PARITY_REGISTRY
        .iter()
        .map(|entry| entry.key)
        .collect::<BTreeSet<_>>();
    assert_eq!(
        owner_commands, registry_commands,
        "INPUT_SHAPE_POPULATIONS must explicitly classify every parity command and no others"
    );
    assert_eq!(
        owner_commands.len(),
        INPUT_SHAPE_POPULATIONS.len(),
        "INPUT_SHAPE_POPULATIONS contains duplicate command rows"
    );

    for entry in PARITY_REGISTRY {
        for class in [
            FailureClass::Parse,
            FailureClass::Guard,
            FailureClass::Name,
            FailureClass::Literate,
        ] {
            let shapes = required_input_shapes(entry, class);
            assert!(
                !shapes.is_empty(),
                "{} has no input-shape population for {class:?}",
                entry.key
            );
            assert!(
                shapes.iter().all(|shape| INPUT_SHAPE_CATALOG
                    .iter()
                    .any(|definition| definition.shape == *shape)),
                "{} declares an input shape absent from INPUT_SHAPE_CATALOG",
                entry.key
            );
            for shape in shapes {
                assert_eq!(
                    class_classification_count(entry, class, *shape),
                    1,
                    "owner tuple {}/{class:?}/{shape:?} must have exactly one classification",
                    entry.key
                );
            }
        }
        for coverage in entry.coverage {
            let shape = coverage_input_shape(entry.key, coverage.fixture);
            assert!(
                owner_contains(entry.key, coverage.class, shape),
                "executable coverage {}/{:?}/{shape:?} is outside INPUT_SHAPE_POPULATIONS",
                entry.key,
                coverage.class
            );
        }
        for not_applicable in entry.not_applicable {
            assert!(
                owner_contains(entry.key, not_applicable.class, not_applicable.shape),
                "NotApplicable {}/{:?}/{:?} is outside INPUT_SHAPE_POPULATIONS",
                entry.key,
                not_applicable.class,
                not_applicable.shape
            );
        }
        if has_literate_not_applicable(entry) {
            assert!(
                owner_contains(entry.key, FailureClass::Literate, InputShape::Source),
                "Literate NotApplicable {}/Source is outside INPUT_SHAPE_POPULATIONS",
                entry.key
            );
        }
    }
    for class in [
        FailureClass::Parse,
        FailureClass::Guard,
        FailureClass::Name,
        FailureClass::Literate,
    ] {
        for cell in cells(class) {
            assert!(
                owner_contains(cell.command, cell.class, cell.shape),
                "executable cell {}/{:?}/{:?} is outside INPUT_SHAPE_POPULATIONS",
                cell.command,
                cell.class,
                cell.shape
            );
        }
    }
}

#[test]
fn input_shape_population_is_independent_and_complete_for_dispatch() {
    // These are production semantic-dispatch facts, named independently from
    // both coverage fixtures and INPUT_SHAPE_POPULATIONS. The issue #801
    // mutation (remove a cell/pin and narrow AI_FSL_INPUT_SHAPES) must fail
    // here even though it changes the owner and its classifications together.
    for command in AI_DISPATCH_COMMANDS {
        let entry = registration(command);
        for class in [FailureClass::Parse, FailureClass::Guard, FailureClass::Name] {
            assert_eq!(
                required_input_shapes(entry, class),
                &[InputShape::Component, InputShape::Project],
                "{} must cover both ai_component and fsl-ai project dispatch for {class:?}",
                entry.key
            );
        }
        assert_eq!(
            required_input_shapes(entry, FailureClass::Literate),
            &[
                InputShape::Source,
                InputShape::Component,
                InputShape::Project,
            ],
            "{} must retain generic, component, and project literate shapes",
            entry.key
        );
    }
    for command in ["check", "verify"] {
        assert_eq!(
            required_input_shapes(registration(command), FailureClass::Parse),
            &[
                InputShape::Source,
                InputShape::Project,
                InputShape::Causal,
                InputShape::Compose,
            ],
            "{command} must retain Source, Project, Causal, and Compose Parse shapes"
        );
    }
    assert_eq!(
        required_input_shapes(registration("mutate"), FailureClass::Parse),
        &[InputShape::Source, InputShape::Causal],
        "mutate must retain its Causal Parse path"
    );
    assert_eq!(
        required_input_shapes(registration("approval create"), FailureClass::Parse),
        &[InputShape::Source, InputShape::RequirementsDocument],
        "approval create must retain requirements-document Parse selection"
    );
}

#[test]
fn mutate_coverage_includes_every_kernel_coverage_entry() {
    // Prevent index-based sharing from leaving mutate narrower when the shared
    // Kernel coverage grows or is reordered.
    for expected in PARSE_KERNEL_COVERAGE {
        assert!(
            MUTATE_COVERAGE.iter().any(|actual| {
                actual.class == expected.class && actual.fixture == expected.fixture
            }),
            "mutate coverage must include {:?}/{} from PARSE_KERNEL_COVERAGE",
            expected.class,
            expected.fixture
        );
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
#[should_panic(
    expected = "must classify Parse/Project as exactly one executable Cell or reasoned NotApplicable"
)]
fn missing_ai_parse_project_classification_is_rejected() {
    let component_only = CommandRegistration {
        key: "ai parse calibration",
        scope: ParityScope::SpecPath {
            invoke: &["ai", "check", SPEC_PLACEHOLDER],
        },
        literate: LiterateCoverage::Pinned,
        coverage: &AI_PARSE_COVERAGE[..1],
        not_applicable: &[],
    };
    assert_eq!(
        class_classification_count(&component_only, FailureClass::Parse, InputShape::Project),
        1,
        "{} must classify Parse/Project as exactly one executable Cell or reasoned NotApplicable",
        component_only.key
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
fn ai_project_parser_failures_keep_their_own_spans_across_evidence_commands() {
    for fixture in [PARSE_AI_PROJECT_FIXTURE, PARSE_AI_PROJECT_DUPLICATE_FIXTURE] {
        let outputs = ["ai check", "ai drift", "ai eval", "ai regress"]
            .into_iter()
            .map(|command| {
                let actual = run(command, fixture);
                assert_expectation(
                    &actual,
                    PARSE_UNIFORM,
                    fixture,
                    FailureClass::Parse,
                    command,
                );
                actual.json.expect("parse error JSON envelope")
            })
            .collect::<Vec<_>>();
        assert!(
            outputs.windows(2).all(|pair| pair[0] == pair[1]),
            "{fixture} must retain one parser-owned envelope: {outputs:?}"
        );
        if fixture == PARSE_AI_PROJECT_DUPLICATE_FIXTURE {
            assert_eq!(
                outputs[0]["loc"],
                serde_json::json!({"line": 13, "column": 1}),
                "the duplicate declaration, not a preceding sibling, owns the diagnostic"
            );
            assert_eq!(
                outputs[0]["message"],
                "duplicate statistical_property 'DuplicateQuality' at 13:1"
            );
        }
    }
}

#[test]
fn surface_loader_read_failures_keep_the_legacy_semantic_envelope() {
    let missing = "rust/fslc/tests/fixtures/error_envelope_missing_surface_input.fsl";
    assert!(
        !workspace_root().join(missing).exists(),
        "{missing} is reserved as a missing-input calibration path"
    );
    let actual = run_from(
        "domain analyze",
        PARSE_DOMAIN_FIXTURE,
        missing,
        None,
        &workspace_root(),
    );
    assert_eq!(actual.exit, 2, "{}", actual.stdout);
    let output = actual.json.expect("missing input JSON envelope");
    assert_eq!(output["result"], "error", "{output}");
    assert_eq!(output["kind"], "semantics", "{output}");
    assert!(output.get("loc").is_none(), "{output}");
    assert!(output.get("diagnostic").is_none(), "{output}");
    assert!(output.get("diagnostic_code").is_none(), "{output}");
}

#[test]
fn lowering_guard_errors_are_uniform_or_pinned_across_frontend_siblings() {
    for cell in cells(FailureClass::Guard) {
        assert_cell(cell);
    }
}

/// #773: the generation path reaches the same checked-Kernel lowering failure
/// as `domain check`. This is a full envelope comparison: no field is excluded.
/// The population test above independently keeps guard cells for all frontend
/// siblings, including commands with their own deliberately pinned envelopes.
#[test]
fn domain_generate_guard_envelope_matches_domain_check_exactly() {
    let checked = run("domain check", GUARD_FIXTURE);
    let generated = run("domain generate", GUARD_FIXTURE);
    assert_eq!(
        generated.exit, checked.exit,
        "generated exit={} stdout={} checked exit={} stdout={}",
        generated.exit, generated.stdout, checked.exit, checked.stdout,
    );
    assert_eq!(
        generated.json, checked.json,
        "domain generate must preserve the full checked-Kernel guard envelope; generated stdout={} checked stdout={}",
        generated.stdout, checked.stdout,
    );
}

#[test]
fn unresolved_identifier_errors_are_uniform_or_pinned_across_frontend_siblings() {
    for cell in cells(FailureClass::Name) {
        assert_cell(cell);
    }
}

#[test]
fn unknown_authority_tool_has_component_coverage_on_prevalidation_lowering_paths() {
    let new_cells = cells(FailureClass::Name)
        .into_iter()
        .filter(|cell| {
            cell.fixture == AI_NAME_FIXTURE
                && matches!(
                    cell.uniform,
                    expected if expected == AI_LOWERING_NAME_COVERAGE.uniform
                        || expected == AI_DOCUMENT_NAME_COVERAGE.uniform
                )
        })
        .collect::<Vec<_>>();
    assert_eq!(new_cells.len(), 20, "review the #1464 lowering population");
    for cell in new_cells {
        assert_eq!(cell.shape, InputShape::Component, "{}", cell.command);
        assert!(
            !KNOWN_ASYMMETRIES.iter().any(|pin| {
                pin.command == cell.command
                    && pin.class == cell.class
                    && pin.fixture == cell.fixture
            }),
            "{} must reject this input, not pin its panic",
            cell.command
        );
    }
}

/// Expectations follow each command's existing frontend, not a universal AI
/// validator inserted ahead of it. Unknown entries fail closed in the sweep.
fn unknown_authority_tool_expected_error(
    command: &str,
    input: &str,
) -> Option<(&'static str, String)> {
    let (kind, message) = match command {
        "ai check" | "ai compat" | "ai replay" | "check" | "html" | "kernel" | "ledger"
        | "mutate" | "sweep" | "verify" => ("semantics", AI_UNKNOWN_TOOL_MESSAGE.to_owned()),
        "ai drift" => (
            "semantics",
            "no observed_property declaration found".to_owned(),
        ),
        "ai eval" => (
            "semantics",
            "no statistical_property declaration found".to_owned(),
        ),
        "ai regress" => ("semantics", "no ai_migration declaration found".to_owned()),
        "compat check" | "db check" | "db observe" => {
            ("semantics", "expected a dbsystem document".to_owned())
        }
        "domain analyze" | "domain check" | "domain expand" | "domain generate"
        | "domain replay" | "domain testgen" => {
            ("semantics", "expected a domain document".to_owned())
        }
        "causal analyze" | "causal check" | "causal diff" | "causal ledger" => {
            ("parse", "expected 'causal'".to_owned())
        }
        "causal observe-expectations" | "causal verify-expectations" => {
            ("parse", "expected 'causal' at 3:1".to_owned())
        }
        "document check" | "document claims" | "document generate" => {
            ("document", AI_UNSUPPORTED_DOCUMENT_MESSAGE.to_owned())
        }
        "approval check" | "typestate" => {
            ("semantics", format!("{AI_UNKNOWN_TOOL_MESSAGE} at 14:5"))
        }
        "analyze"
        | "approval create"
        | "approval diff"
        | "conformance"
        | "counterexample export"
        | "diff"
        | "explain"
        | "fmt"
        | "lint"
        | "migrate"
        | "refine"
        | "replay"
        | "scenarios"
        | "testgen"
        | "testplan" => (
            "semantics",
            format!("{AI_UNKNOWN_TOOL_MESSAGE} at {input}:14:5"),
        ),
        _ => return None,
    };
    Some((kind, message))
}

#[test]
fn unknown_authority_tool_returns_an_error_for_every_document_command() {
    require_test_git();
    let contract: Value =
        serde_json::from_str(include_str!("../cli-contract.json")).expect("valid CLI contract");
    let mut leaves = BTreeSet::new();
    cli_contract_leaves(&contract["root"], &mut leaves);
    let mut failures = Vec::new();
    let mut tested = 0;
    for command in leaves {
        let entry = registration(&command);
        if !matches!(entry.scope, ParityScope::SpecPath { .. }) {
            continue;
        }
        tested += 1;
        let input = if approval_command(&command) {
            approval_positional_name(AI_NAME_FIXTURE)
        } else {
            AI_NAME_FIXTURE
        };
        let Some((kind, message)) = unknown_authority_tool_expected_error(&command, input) else {
            failures.push(format!(
                "{command}: missing unknown-tool fixture expectation"
            ));
            continue;
        };
        // run() supplies a valid test-owned approval baseline/record. An early
        // missing/invalid-record io error must not masquerade as coverage.
        let mut variants = vec![(command.clone(), run(&command, AI_NAME_FIXTURE))];
        if command == "approval create" {
            // HTML is another kind of this leaf, not a 49th document command.
            // Reuse run()'s baseline/record setup before replacing the kind.
            let approval = ApprovalFixture::new(AI_NAME_FIXTURE);
            let mut arguments = invoke(&command, AI_NAME_FIXTURE, input, Some(&approval.record));
            let kind_index = arguments
                .iter()
                .position(|argument| argument == "--kind")
                .expect("approval create kind argument");
            arguments[kind_index + 1] = "html".to_owned();
            variants.push((
                format!("{command} --kind html"),
                run_arguments(&command, &arguments, &approval.root),
            ));
        }
        for (variant, actual) in variants {
            let envelope_matches = actual.json.as_ref().is_some_and(|output| {
                output["fsl"] == "1.0"
                    && output["result"] == "error"
                    && output["kind"] == kind
                    && output["message"] == message
                    && match kind {
                        "document" => {
                            // This established envelope uses `code`, not the
                            // syntax diagnostic's `diagnostic_code` field.
                            output["code"] == "FSL-DOC-DIALECT-UNSUPPORTED"
                                && output["dialect"] == "ai_component"
                                && output["supported_dialects"]
                                    == serde_json::json!(["requirements", "spec"])
                                && output.get("diagnostic_code").is_none()
                        }
                        "parse" => output["diagnostic_code"] == "FSL-PARSE",
                        _ => true,
                    }
            });
            if actual.exit != 2 || !actual.stderr.is_empty() || !envelope_matches {
                failures.push(format!(
                    "{variant}: expected exit=2 result=error kind={kind} message={message:?}; \
                     produced exit={} stdout={:?} stderr={:?}",
                    actual.exit, actual.stdout, actual.stderr
                ));
            }
        }
    }
    assert_eq!(tested, 48, "review additions/removals of document commands");
    assert!(
        failures.is_empty(),
        "unknown authority tool: {}/{tested} document commands failed:\n{}",
        failures.len(),
        failures.join("\n")
    );
}

// Independent specification vocabulary: do not derive this oracle from the
// production rule registry (#1473).
const DB_UNKNOWN_RULE_MESSAGE: &str = "unknown db compatibility rule 'unregistered_compatibility_rule'; supported rules: all_active_reads_exist, all_active_writes_exist, api_calls_accepted, api_responses_expected, artifact_capabilities_provided, data_preserved, destructive_operations_annotated, not_null_after_backfill, offline_payloads_accepted, preservation_transforms_annotated, removed_only_after_unused, rollback_equivalent";

struct SpecializedOutputDirectory(PathBuf);

impl SpecializedOutputDirectory {
    fn new() -> Self {
        let sequence = APPROVAL_RECORD_SEQUENCE.fetch_add(1, Ordering::Relaxed);
        let path = std::env::temp_dir().join(format!(
            "fslc-specialized-rejection-{}-{sequence}",
            std::process::id()
        ));
        let path = scratch_identity::unique(path);
        std::fs::create_dir(&path).expect("test-owned output directory");
        Self(path)
    }
}

impl Drop for SpecializedOutputDirectory {
    fn drop(&mut self) {
        let _ = std::fs::remove_dir_all(&self.0);
    }
}

fn specialized_error(kind: &str, message: &str) -> Value {
    serde_json::json!({"fsl": "1.0", "result": "error", "kind": kind, "message": message})
}

fn specialized_expected_error(command: &str, fixture: &str, input: &str) -> Option<Value> {
    let db = fixture == DB_GUARD_FIXTURE;
    let (kind, message) = if db {
        match command {
            "ai check" => (
                "semantics",
                "expected an ai_component or agent document".to_owned(),
            ),
            "ai compat" => (
                "semantics",
                "expected an ai_component or fsl-ai project document".to_owned(),
            ),
            "ai replay" => ("semantics", "expected an ai_component document".to_owned()),
            "ai drift" | "ai eval" | "ai regress" => (
                "parse",
                "expected fsl-ai project declarations at 1:1".to_owned(),
            ),
            "check" | "verify" | "sweep" | "mutate" | "db check" | "compat check"
            | "db observe" | "html" | "ledger" => ("semantics", DB_UNKNOWN_RULE_MESSAGE.to_owned()),
            "kernel" => ("semantics", format!("{DB_UNKNOWN_RULE_MESSAGE} at 12:5")),
            _ => {
                let (kind, message) = unknown_authority_tool_expected_error(command, input)?;
                (
                    kind,
                    message
                        .replace(AI_UNKNOWN_TOOL_MESSAGE, DB_UNKNOWN_RULE_MESSAGE)
                        .replace("14:5", "12:5")
                        .replace("'ai_component'", "'dbsystem'"),
                )
            }
        }
    } else {
        assert_eq!(fixture, AI_NAME_FIXTURE);
        unknown_authority_tool_expected_error(command, input)?
    };
    let mut expected = specialized_error(kind, &message);
    match kind {
        "document" => {
            expected["code"] = "FSL-DOC-DIALECT-UNSUPPORTED".into();
            expected["dialect"] = if db { "dbsystem" } else { "ai_component" }.into();
            expected["supported_dialects"] = serde_json::json!(["requirements", "spec"]);
        }
        "parse" => {
            expected["diagnostic_code"] = "FSL-PARSE".into();
            expected["loc"] = serde_json::json!({
                "line": if db && command.starts_with("ai ") { 1 } else { 3 },
                "column": 1
            });
        }
        _ if (message.contains(" at ")
            && !matches!(command, "fmt" | "lint" | "migrate" | "approval check")) =>
        {
            expected["loc"] = serde_json::json!({"line": if db { 12 } else { 14 }, "column": 5});
        }
        _ => {}
    }
    Some(expected)
}

fn specialized_envelope_matches(
    actual: &Actual,
    expected: &Value,
    command: &str,
    approval: Option<&ApprovalFixture>,
) -> bool {
    let Some(output) = actual.json.as_ref() else {
        return false;
    };
    // Match the established #1464 walk: the diagnostic is exact, while
    // command metadata is checked separately, never mistaken for an artifact.
    if actual.exit != 2
        || !actual.stderr.is_empty()
        || !expected
            .as_object()
            .expect("error object")
            .iter()
            .all(|(key, value)| output.get(key) == Some(value))
        || output.get("loc") != expected.get("loc")
    {
        return false;
    }
    for key in [
        "$schema",
        "schema_version",
        "kernel_schema_version",
        "kernel",
        "spec",
        "semantics",
        "types",
        "constants",
        "state",
        "actions",
        "states",
        "vectors",
        "content",
        "output",
        "scenarios",
        "analysis",
        "projection",
        "nodes",
        "edges",
        "files",
        "findings",
        "changed",
        "written",
        "target",
        "summary",
        "coverage",
        "artifact",
        "html",
        "markdown",
        "test_plan",
        "action_map",
        "directions",
    ] {
        if output.get(key).is_some() {
            return false;
        }
    }
    // Observed with FSLC_CACHE unset: check/verify attach versions, other
    // commands do not. Version values belong to the installed backend, not
    // this malformed-input contract. Presence remains a live assertion.
    let versions = matches!(command, "check" | "verify")
        && std::env::var("FSLC_CACHE").as_deref() != Ok("off");
    if output.get("versions").is_some() != versions {
        return false;
    }
    if command == "approval diff" {
        let approval = approval.expect("approval fixture");
        let record: Value =
            serde_json::from_slice(&std::fs::read(&approval.record).expect("record"))
                .expect("record JSON");
        let metadata = &output["approval"];
        if metadata["record"] != approval.record.display().to_string()
            || metadata["baseline_digest"] != record["spec"]["digest"]
            || metadata["baseline_commit"] != record["spec"]["git_commit"]
            || metadata["signature_status"] != "unsigned"
            || metadata.get("key_id") != Some(&Value::Null)
        {
            return false;
        }
    } else if output.get("approval").is_some() {
        return false;
    }
    true
}

fn contract_command<'a>(contract: &'a Value, command: &str) -> &'a Value {
    let mut node = &contract["root"];
    for segment in command.split_whitespace() {
        node = node["commands"]
            .as_array()
            .expect("commands")
            .iter()
            .find(|child| {
                child["path"]
                    .as_array()
                    .expect("path")
                    .last()
                    .and_then(Value::as_str)
                    == Some(segment)
            })
            .unwrap_or_else(|| panic!("missing contract node for {command}"));
    }
    node
}

fn specialized_observation(actual: &Actual) -> Value {
    let value = actual.json.clone().unwrap_or(Value::Null);
    serde_json::json!({
        "exit": actual.exit, "result": value["result"], "kind": value["kind"],
        "message": value["message"], "loc": value["loc"],
        "stdout_bytes": actual.stdout.len(), "stderr": actual.stderr,
    })
}

fn specialized_rejection_walk(fixture: &str) {
    require_test_git();
    parity_registry_is_total_and_has_no_orphaned_commands();
    let contract: Value =
        serde_json::from_str(include_str!("../cli-contract.json")).expect("CLI contract");
    let mut leaves = BTreeSet::new();
    cli_contract_leaves(&contract["root"], &mut leaves);
    let outputs = SpecializedOutputDirectory::new();
    let mut failures = Vec::new();
    let mut tested = 0;
    for command in leaves {
        if let ParityScope::Excluded { reason } = registration(&command).scope {
            assert!(
                !reason.trim().is_empty(),
                "{command}: exclusion needs a reason"
            );
            continue;
        }
        tested += 1;
        let approval = approval_command(&command).then(|| ApprovalFixture::new(fixture));
        let input = approval
            .as_ref()
            .map_or(fixture, |_| approval_positional_name(fixture));
        let expected = specialized_expected_error(&command, fixture, input).unwrap_or_else(|| {
            panic!("{command}: missing specialized fixture expectation for {fixture}")
        });
        let cwd = approval
            .as_ref()
            .map_or_else(workspace_root, |fixture| fixture.root.clone());
        let mut arguments = invoke(
            &command,
            fixture,
            input,
            approval.as_ref().map(|fixture| fixture.record.as_path()),
        );
        // A real reviewed artifact and valid record, not an early missing-file
        // error, must lead approval through its digest/lowering gate.
        if command == "approval create" {
            let artifact = arguments
                .iter()
                .position(|arg| arg == "--artifact")
                .expect("artifact");
            "ledger.md".clone_into(&mut arguments[artifact + 1]);
            let approval = approval.as_ref().expect("approval fixture");
            approval.git(&["add", "spec.fsl"]);
            approval.git(&["commit", "-qm", "malformed current spec"]);
        }
        let output = outputs.0.join(format!("{}.out", command.replace(' ', "-")));
        assert!(!output.exists());
        let supports_output = contract_command(&contract, &command)["actions"]
            .as_array()
            .expect("actions")
            .iter()
            .any(|action| action["dest"] == "output");
        if supports_output {
            if let Some(index) = arguments
                .iter()
                .position(|arg| matches!(arg.as_str(), "-o" | "--output"))
            {
                arguments[index + 1] = output.display().to_string();
            } else {
                arguments.extend(["-o".to_owned(), output.display().to_string()]);
            }
        }
        let actual = run_arguments(&command, &arguments, &cwd);
        let matches = specialized_envelope_matches(&actual, &expected, &command, approval.as_ref())
            && !output.exists();
        eprintln!(
            "1473 {} {fixture}/{command}: expected exit=2 json={expected} file=false; produced {} file={} file_bytes={}",
            if matches { "PASS" } else { "FAIL" },
            specialized_observation(&actual),
            output.exists(),
            std::fs::metadata(&output).map_or(0, |metadata| metadata.len())
        );
        if !matches {
            failures.push(format!(
                "{command}: expected exit=2 json={expected} file=false; produced {} file={}",
                specialized_observation(&actual),
                output.exists()
            ));
        }
    }
    assert_eq!(tested, 48, "review additions/removals of document commands");
    assert!(
        failures.is_empty(),
        "{fixture}: {}/{tested} cells failed:\n{}",
        failures.len(),
        failures.join("\n")
    );
}

#[test]
fn specialized_preflight_rejects_invalid_db_rule_for_every_document_command() {
    // The authored span starts at the `rule` keyword, not at the unknown name.
    let fsl_syntax::SurfaceDocument::Db(system) = fsl_syntax::parse_surface_document(include_str!(
        "fixtures/error_envelope_db_invalid_rule.fsl"
    ))
    .expect("DB fixture") else {
        panic!("DB fixture");
    };
    assert_eq!(system.check.rules[0].span.start.line, 12);
    assert_eq!(system.check.rules[0].span.start.column, 5);
    specialized_rejection_walk(DB_GUARD_FIXTURE);
}

#[test]
fn specialized_preflight_preserves_unknown_ai_tool_for_every_document_command() {
    specialized_rejection_walk(AI_NAME_FIXTURE);
}

#[test]
fn specialized_preflight_reaches_db_gate_with_valid_replay_and_refinement_inputs() {
    let directory = SpecializedOutputDirectory::new();
    let invalid = include_str!("fixtures/error_envelope_db_invalid_rule.fsl");
    let valid = invalid.replace("unregistered_compatibility_rule", "all_active_reads_exist");
    std::fs::write(directory.0.join("valid.fsl"), valid).expect("valid DB");
    std::fs::write(directory.0.join("invalid.fsl"), invalid).expect("invalid DB");
    let state = serde_json::json!({"schema_version": 0, "column_exists": {"col_users_id": true}, "column_backfilled": {"col_users_id": true}, "column_not_null": {"col_users_id": true}});
    let trace = serde_json::json!({
        "$schema": "https://fsl.dev/schemas/fslc/kernel/replay-trace.v1.schema.json",
        "schema_version": "1.0.0", "kernel_schema_version": "1.0.0",
        "spec": "EnvelopeParityInvalidDbRule", "initial": state,
        "events": [{"tick": 1, "action": "observe_schema_0", "params": {}, "state": state}]
    });
    std::fs::write(
        directory.0.join("trace.json"),
        serde_json::to_vec(&trace).expect("trace JSON"),
    )
    .expect("trace");
    std::fs::write(directory.0.join("mapping.fsl"), "refinement Identity {\n  impl EnvelopeParityInvalidDbRule\n  abs EnvelopeParityInvalidDbRule\n  map schema_version = schema_version\n  map column_exists[c: Column] = column_exists[c]\n  map column_backfilled[c: Column] = column_backfilled[c]\n  map column_not_null[c: Column] = column_not_null[c]\n  action observe_schema_0() -> observe_schema_0()\n}\n").expect("mapping");
    let mut failures = Vec::new();
    for (command, arguments, result) in [
        (
            "replay",
            vec!["replay", "valid.fsl", "--trace", "trace.json"],
            "conformant",
        ),
        (
            "refine",
            vec![
                "refine",
                "valid.fsl",
                "valid.fsl",
                "mapping.fsl",
                "--depth",
                "1",
            ],
            "refines",
        ),
    ] {
        let arguments = arguments.into_iter().map(str::to_owned).collect::<Vec<_>>();
        let control = run_arguments(command, &arguments, &directory.0);
        assert_eq!(
            control.exit, 0,
            "valid {command} control: {}",
            control.stdout
        );
        assert!(control.stderr.is_empty(), "{}", control.stderr);
        assert_eq!(
            control.json.as_ref().expect("JSON")["result"],
            result,
            "{}",
            control.stdout
        );
        let arguments = arguments
            .into_iter()
            .map(|arg| {
                if arg == "valid.fsl" {
                    "invalid.fsl".to_owned()
                } else {
                    arg
                }
            })
            .collect::<Vec<_>>();
        let actual = run_arguments(command, &arguments, &directory.0);
        let expected = specialized_expected_error(command, DB_GUARD_FIXTURE, "invalid.fsl")
            .expect("gate expectation");
        let matches = specialized_envelope_matches(&actual, &expected, command, None);
        eprintln!(
            "1473 {} valid-side-input/{command}: expected exit=2 json={expected}; produced {}",
            if matches { "PASS" } else { "FAIL" },
            specialized_observation(&actual)
        );
        if !matches {
            failures.push(format!(
                "{command}: expected {expected}; produced {}",
                specialized_observation(&actual)
            ));
        }
    }
    assert!(failures.is_empty(), "{}", failures.join("\n"));
}

fn empty_membership_invariant_is_false(source: &str) -> bool {
    fsl_core::parse_kernel_source(source, &fsl_core::FsResolver::new("."))
        .and_then(|kernel| {
            fsl_core::build_model(kernel)
                .map_err(|error| fsl_core::CoreError::unlocated(error.message))
        })
        .is_ok_and(|model| {
            model.invariants.iter().any(|property| {
                property.name == "Order_empty"
                    && matches!(property.expr, fsl_core::KernelExpr::Bool(false))
            })
        })
}

#[test]
fn empty_membership_source_checker_rejects_a_true_invariant() {
    let source = include_str!("fixtures/domain_empty_membership.fsl");
    assert!(empty_membership_invariant_is_false(source));
    assert!(!empty_membership_invariant_is_false(
        &source.replace("status in []", "true")
    ));
}

fn expanded_empty_membership_invariant_is_false(source: &str) -> bool {
    fsl_core::parse_direct_kernel_spec(source)
        .and_then(|kernel| {
            fsl_core::build_model(kernel)
                .map_err(|error| fsl_core::CoreError::unlocated(error.message))
        })
        .is_ok_and(|model| {
            model.invariants.iter().any(|property| {
                property.name == "Order_empty"
                    && matches!(property.expr, fsl_core::KernelExpr::Bool(false))
            })
        })
}

#[test]
fn empty_membership_expansion_checker_rejects_an_unexpanded_domain() {
    let source = include_str!("fixtures/domain_empty_membership.fsl");
    assert!(!expanded_empty_membership_invariant_is_false(source));
}

fn empty_membership_warning_stderr() -> String {
    format!(
        "warning: {}\n",
        serde_json::json!({
            "code": "deprecated_domain_enum_union",
            "kind": "deprecated_domain_enum_union",
            "taxonomy": "deprecated",
            "severity": "warning",
            "edition": "current",
            "message": "legacy domain enum union syntax is deprecated",
            "loc": {
                "file": EMPTY_MEMBERSHIP_FIXTURE,
                "line": 2, "column": 3, "end_line": 2, "end_column": 33
            },
            "canonical_replacement": "enum Status { Draft, Approved }",
            "machine_applicable": true,
            "edits": [{
                "start": 66, "end": 96,
                "replacement": "enum Status { Draft, Approved }"
            }],
            "suggestion": {
                "kind": "replace",
                "replacement": "enum Status { Draft, Approved }",
                "span": { "start": 66, "end": 96 },
                "machine_applicable": true
            }
        })
    )
}

#[allow(clippy::too_many_lines)]
fn empty_membership_result_matches(command: &str, actual: &Actual) -> bool {
    let expected_stderr = if matches!(
        command,
        "kernel" | "conformance" | "testplan" | "domain expand" | "html" | "ledger"
    ) {
        empty_membership_warning_stderr()
    } else {
        String::new()
    };
    if actual.stderr != expected_stderr {
        return false;
    }
    match command {
        "domain expand" => {
            return actual.exit == 0
                && expanded_empty_membership_invariant_is_false(&actual.stdout);
        }
        "fmt" => {
            return actual.exit == 0 && empty_membership_invariant_is_false(&actual.stdout);
        }
        "html" => {
            return actual.exit == 1
                && actual.stdout.contains("<html")
                && actual.stdout.contains("EmptyMembership")
                && actual.stdout.contains("violated");
        }
        "ledger" => {
            return actual.exit == 1
                && actual
                    .stdout
                    .starts_with("# 意図ずれ監査台帳: EmptyMembership")
                && actual.stdout.contains("violated");
        }
        _ => {}
    }
    let Some(output) = &actual.json else {
        return false;
    };
    if output["fsl"] != "1.0" {
        return false;
    }
    let error = |kind: &str, message: &str| {
        actual.exit == 2
            && output["result"] == "error"
            && output["kind"] == kind
            && output["message"] == message
    };
    match command {
        "ai check" => error("semantics", "expected an ai_component or agent document"),
        "ai compat" => error(
            "semantics",
            "expected an ai_component or fsl-ai project document",
        ),
        "ai replay" => error("semantics", "expected an ai_component document"),
        "ai drift" | "ai eval" | "ai regress" => {
            error("parse", "expected fsl-ai project declarations at 1:1")
                && output["diagnostic_code"] == "FSL-PARSE"
        }
        "causal analyze" | "causal check" | "causal diff" | "causal ledger" => {
            error("parse", "expected 'causal'") && output["diagnostic_code"] == "FSL-PARSE"
        }
        "causal observe-expectations" | "causal verify-expectations" => {
            error("parse", "expected 'causal' at 1:1") && output["diagnostic_code"] == "FSL-PARSE"
        }
        "compat check" | "db check" | "db observe" => {
            error("semantics", "expected a dbsystem document")
        }
        "document check" | "document claims" | "document generate" => {
            error(
                "document",
                "document projection does not support dialect 'domain' in RCIR v1",
            ) && output["code"] == "FSL-DOC-DIALECT-UNSUPPORTED"
                && output["dialect"] == "domain"
                && output["supported_dialects"] == serde_json::json!(["requirements", "spec"])
        }
        "approval create" => error(
            "io",
            "approval creation requires a clean tracked worktree so the baseline can be reconstructed",
        ),
        "approval check" => {
            actual.exit == 0
                && output["result"] == "approval_check"
                && output["status"] == "drifted"
                && output["reasons"] == serde_json::json!(["spec_changed", "rendering_changed"])
        }
        "approval diff" => {
            // The valid approved baseline has different state/action names.
            // run_diff reports unknown directions, not a failed opt-in gate.
            actual.exit == 0
                && output["result"] == "semantic_diff"
                && output["directions"]["new_to_old"]["result"] == "unknown"
                && output["directions"]["new_to_old"]["reason"] == "state_or_action_names_differ"
                && output["directions"]["old_to_new"]["result"] == "unknown"
                && output["gate"]["passed"] == true
        }
        "diff" => actual.exit == 1 && output["result"] == "semantic_diff",
        "refine" => error("type", "expected refinement document"),
        "replay" => error(
            "io",
            "replay trace spec 'ReplayTrace' does not match checked spec 'EmptyMembership'",
        ),
        "domain check" => {
            actual.exit == 1
                && output["result"] == "violated"
                && output["formal_result"] == "violated"
                && output["kernel"]["result"] == "violated"
                && output["kernel"]["invariant"] == "empty"
                && output["kernel"]["violated_at_step"] == 0
        }
        "domain analyze" => {
            actual.exit == 0
                && output["result"] == "analyzed"
                && output["domain"] == "EmptyMembership"
                && output["aggregates"][0]["name"] == "Order"
                && output["aggregates"][0]["invariants"] == serde_json::json!(["empty"])
        }
        "domain generate" => {
            actual.exit == 0
                && output["result"] == "generated"
                && output["domain"] == "EmptyMembership"
                && output["files"]
                    .as_array()
                    .is_some_and(|files| !files.is_empty())
        }
        "domain replay" => actual.exit == 1 && output["result"] == "nonconformant",
        "kernel" => {
            actual.exit == 0
                && output["result"] == "kernel"
                && output["properties"]["invariants"][0]["expression"]["kind"] == "bool"
                && output["properties"]["invariants"][0]["expression"]["value"] == false
        }
        "counterexample export"
        | "domain testgen"
        | "mutate"
        | "scenarios"
        | "testgen"
        | "verify" => {
            actual.exit == 1
                && output["result"] == "violated"
                && output["invariant"] == "empty"
                && output["violated_at_step"] == 0
        }
        "check" => actual.exit == 0 && output["result"] == "ok",
        "analyze" => actual.exit == 0 && output["result"] == "analyzed",
        "conformance" => actual.exit == 0 && output["result"] == "conformance",
        "explain" => actual.exit == 0 && output["result"] == "explained",
        "lint" => actual.exit == 1 && output["result"] == "lint",
        "migrate" => actual.exit == 0 && output["result"] == "migrated",
        "sweep" => actual.exit == 1 && output["result"] == "sweep_failed",
        "testplan" => {
            actual.exit == 0
                && output["result"] == "testplan"
                && output["formal_result"] == "not_run"
        }
        "typestate" => actual.exit == 0 && output["result"] == "typestate",
        _ => panic!("missing empty-membership expectation for {command}"),
    }
}

#[test]
fn empty_membership_has_a_defined_result_for_every_document_command() {
    require_test_git();
    let contract: Value =
        serde_json::from_str(include_str!("../cli-contract.json")).expect("CLI contract");
    let mut leaves = BTreeSet::new();
    cli_contract_leaves(&contract["root"], &mut leaves);
    // Reuse the established valid baseline/record and its TMPDIR-owned cleanup.
    // The current domain spec differs from that approved baseline deliberately.
    let approval = ApprovalFixture::new(EMPTY_MEMBERSHIP_FIXTURE);
    let artifact = approval.root.join("empty-membership-reproducer.json");
    let mut tested = 0;
    let mut failures = Vec::new();
    for command in leaves {
        if !matches!(registration(&command).scope, ParityScope::SpecPath { .. }) {
            continue;
        }
        tested += 1;
        let actual = if approval_command(&command) {
            run_from(
                &command,
                EMPTY_MEMBERSHIP_FIXTURE,
                "spec.fsl",
                Some(&approval.record),
                &approval.root,
            )
        } else if command == "counterexample export" {
            let mut arguments = invoke(
                &command,
                EMPTY_MEMBERSHIP_FIXTURE,
                EMPTY_MEMBERSHIP_FIXTURE,
                None,
            );
            let output_index = arguments
                .iter()
                .position(|arg| arg == COUNTEREXAMPLE_OUTPUT)
                .expect("output argument");
            arguments[output_index] = artifact.display().to_string();
            let output = Command::new(env!("CARGO_BIN_EXE_fslc"))
                .args(arguments)
                .current_dir(workspace_root())
                .output()
                .expect("run counterexample export");
            Actual {
                json: serde_json::from_slice(&output.stdout).ok(),
                stdout: String::from_utf8(output.stdout).expect("UTF-8 stdout"),
                stderr: String::from_utf8(output.stderr).expect("UTF-8 stderr"),
                exit: output.status.code().expect("exit status"),
            }
        } else {
            run(&command, EMPTY_MEMBERSHIP_FIXTURE)
        };
        if !empty_membership_result_matches(&command, &actual) {
            failures.push(format!(
                "{command}: exit={} stdout={:?} stderr={:?}",
                actual.exit, actual.stdout, actual.stderr
            ));
        }
        if command == "counterexample export" {
            let exported: Value =
                serde_json::from_slice(&std::fs::read(&artifact).expect("reproducer was written"))
                    .expect("reproducer JSON");
            assert_eq!(exported["schema_version"], "1.0.0", "{exported}");
            assert_eq!(
                actual.json.as_ref().expect("export result")["reproducer"]["exported_to"],
                artifact.display().to_string()
            );
        }
    }
    assert_eq!(tested, 48, "review document leaf additions/removals");
    assert!(
        failures.is_empty(),
        "empty membership: {}/{tested} commands failed:\n{}",
        failures.len(),
        failures.join("\n")
    );
}

#[test]
fn domain_expand_empty_membership_emits_a_checked_false_invariant() {
    let actual = run("domain expand", EMPTY_MEMBERSHIP_FIXTURE);
    assert_eq!(
        actual.exit, 0,
        "stdout={} stderr={}",
        actual.stdout, actual.stderr
    );
    assert_eq!(actual.stderr, empty_membership_warning_stderr());
    assert!(
        expanded_empty_membership_invariant_is_false(&actual.stdout),
        "{}",
        actual.stdout
    );
}

#[test]
fn domain_analyze_empty_membership_preserves_the_structural_projection() {
    let actual = run("domain analyze", EMPTY_MEMBERSHIP_FIXTURE);
    assert!(
        empty_membership_result_matches("domain analyze", &actual),
        "exit={} stdout={} stderr={}",
        actual.exit,
        actual.stdout,
        actual.stderr
    );
    let output = actual.json.expect("analysis JSON");
    assert_eq!(output["effects"], serde_json::json!([]));
    assert_eq!(output["sagas"], serde_json::json!([]));
    assert_eq!(output["findings"], serde_json::json!([]));
}

#[test]
fn empty_membership_warning_is_rejected_for_an_unapproved_command() {
    let output = serde_json::json!({
        "fsl":"1.0", "result":"error", "kind":"semantics",
        "message":"expected an ai_component or agent document"
    });
    let mut actual = Actual {
        stdout: output.to_string(),
        json: Some(output),
        stderr: String::new(),
        exit: 2,
    };
    assert!(empty_membership_result_matches("ai check", &actual));
    actual.stderr = empty_membership_warning_stderr();
    assert!(!empty_membership_result_matches("ai check", &actual));
}

#[test]
fn domain_check_empty_membership_keeps_the_initial_violation() {
    let actual = run("domain check", EMPTY_MEMBERSHIP_FIXTURE);
    assert!(
        empty_membership_result_matches("domain check", &actual),
        "exit={} stdout={} stderr={}",
        actual.exit,
        actual.stdout,
        actual.stderr
    );
    let output = actual.json.expect("domain check JSON");
    assert_eq!(output["kernel"]["blame"]["conjuncts"][0]["holds"], false);
    assert_eq!(output["kernel"]["blame"]["conjuncts"][0]["text"], "false");
}

#[test]
fn generic_check_matches_ai_check_for_an_unknown_authority_tool() {
    let generic = run("check", AI_NAME_FIXTURE);
    let specialized = run("ai check", AI_NAME_FIXTURE);

    assert_eq!(
        generic.exit, specialized.exit,
        "generic stdout={} specialized stdout={}",
        generic.stdout, specialized.stdout
    );
    let generic = generic.json.expect("generic JSON envelope");
    let specialized = specialized.json.expect("specialized JSON envelope");
    for field in ["result", "kind", "message", "loc"] {
        assert_eq!(
            generic.get(field),
            specialized.get(field),
            "{field} differs: generic={generic} specialized={specialized}"
        );
    }
    assert!(
        generic.get("loc").is_none(),
        "the shared unknown-tool diagnostic must remain unlocated: {generic}"
    );
    // `check` receives `versions` from command()'s common
    // `with_version_metadata` wrapper; `ai check` returns its specialized
    // envelope directly. This observed, command-wide metadata difference is
    // outside the shared failure contract. Keep its asymmetric presence
    // explicit so deleting both keys cannot make this comparison look fuller
    // than it is.
    assert!(generic.get("versions").is_some(), "generic={generic}");
    assert!(
        specialized.get("versions").is_none(),
        "specialized={specialized}"
    );
}

#[test]
fn generic_check_matches_ai_check_for_invalid_project_components() {
    for fixture in [AI_PROJECT_NAME_FIXTURE, AI_PROJECT_GUARD_FIXTURE] {
        let generic = run("check", fixture);
        let specialized = run("ai check", fixture);
        assert_eq!(generic.exit, 2, "{}", generic.stdout);
        assert_eq!(specialized.exit, 2, "{}", specialized.stdout);
        let mut generic = generic.json.expect("check JSON");
        let specialized = specialized.json.expect("AI check JSON");
        // check publishes native build metadata; AI check does not. Check that
        // observed asymmetric field before comparing every diagnostic field.
        let versions = generic.as_object_mut().unwrap().remove("versions").unwrap();
        assert_eq!(
            versions,
            serde_json::json!({
                "verifier": {"name": "fslc-rust", "version": env!("CARGO_PKG_VERSION")},
                "core": {"name": "fsl-core", "version": env!("CARGO_PKG_VERSION")},
                "solver": {"name": "z3", "backend": "native-z3", "version": "Z3 4.16.0.0"}
            })
        );
        assert!(specialized.get("versions").is_none());
        assert_eq!(generic, specialized, "{fixture}");
    }
}

#[test]
fn requirements_document_approval_invocation_uses_the_selected_kind() {
    let arguments = invoke(
        "approval create",
        PARSE_APPROVAL_REQUIREMENTS_DOCUMENT_FIXTURE,
        "spec.fsl",
        None,
    );
    eprintln!("requirements-document argv: {arguments:?}");
    assert_eq!(
        arguments,
        [
            "approval",
            "create",
            "spec.fsl",
            "--kind",
            "requirements_document",
            "--artifact",
            "requirements.md",
            "--approver",
            "parity",
        ]
    );
    assert_requirements_document_invocation(&arguments);
}

#[test]
#[should_panic(expected = "must invoke --kind requirements_document")]
fn requirements_document_approval_ledger_invocation_is_rejected() {
    let mut arguments = invoke(
        "approval create",
        PARSE_APPROVAL_REQUIREMENTS_DOCUMENT_FIXTURE,
        "spec.fsl",
        None,
    );
    let kind = arguments
        .iter_mut()
        .find(|argument| argument.as_str() == "requirements_document")
        .expect("requirements-document invocation has kind argument");
    *kind = "ledger".to_owned();
    eprintln!("requirements-document argv negative control: {arguments:?}");
    assert_requirements_document_invocation(&arguments);
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
            fixture,
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
        NAME_FIXTURE,
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
                stderr: actual.stderr,
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
fn name_matrix_keeps_ten_uniform_countercontrols() {
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
    assert_eq!(
        name_cells.len(),
        10,
        "Name matrix must keep the original nine countercontrols plus AI project check"
    );
    assert!(name_cells.iter().any(|cell| cell.command == "check"
        && cell.shape == InputShape::Project
        && cell.fixture == AI_PROJECT_NAME_FIXTURE));
    assert_eq!(
        name_cells
            .iter()
            .filter(|cell| {
                KNOWN_ASYMMETRIES.iter().any(|pin| {
                    pin.class == FailureClass::Name
                        && pin.command == cell.command
                        && pin.shape == cell.shape
                        && pin.fixture == cell.fixture
                })
            })
            .count(),
        0,
        "Name matrix must keep all ten countercontrols uniform"
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
