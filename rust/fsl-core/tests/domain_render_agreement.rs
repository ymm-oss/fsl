// SPDX-License-Identifier: Apache-2.0

//! Agreement gate between the two independent `domain` lowering paths
//! (issue #664).
//!
//! `domain` semantics is implemented twice, and both implementations are
//! production paths:
//!
//! - path A: [`fsl_core::lower_domain`] (`domain_lowering.rs`) produces a
//!   typed [`fsl_core::KernelSpec`] directly. `check`/`verify` use this path.
//! - path B: [`fsl_core::domain_kernel_source`] (`domain.rs`) renders the
//!   same `DomainSpec` to `.fsl` **text**, which is then re-parsed with
//!   [`fsl_core::parse_kernel_source`] into a `KernelSpec`. `domain expand`
//!   and `check_domain` use the renderer in production; this gate additionally
//!   re-parses its output so the comparison reaches a checked model.
//!
//! Before this file, no test checked that the two paths produce the same
//! checked model for any spec, so a rule fixed on one side (e.g. PR #661)
//! could silently regress on the other with every pre-existing domain test
//! staying green (see issue #664 for the reverted-rendering-side
//! experiment that demonstrated exactly this).
//!
//! # Comparison design
//!
//! For every domain spec in the corpus below, both paths are lowered all
//! the way to a checked [`fsl_core::KernelModel`] and then projected through
//! [`fsl_core::public_kernel_contract`] — the repository's stable JSON
//! projection of a checked model — using the *same* `source_path` and
//! `dialect` argument for both calls (those are caller-supplied labels, not
//! derived from either side, so holding them equal is fixture setup, not an
//! exclusion). The two JSON trees are then walked recursively and must be
//! structurally equal, key for key, value for value, including array order
//! (action/property lists are already sorted by name inside
//! `public_kernel_contract` itself, so positional array comparison is exact,
//! not an ordering artifact; statement/guard lists inside a single action or
//! `init` are execution-ordered and must match positionally for the same
//! reason).
//!
//! ## The exclusion set — and why it is exactly this and nothing else
//!
//! [`classify_field`] is the single, named place that decides whether a JSON
//! object key participates in the comparison, and it defaults to
//! [`FieldClass::Compared`]: a key is excluded only if a match arm
//! explicitly names it, not the other way around. This default direction is
//! the single most important property of this design, and it must not be
//! flipped by a future edit. Issue #689 documents the shape this inverts:
//! `tools/check_rust_phase3_commands.py`'s `project()` used an *inclusion*
//! allow-list, so a field the projection forgot to list was silently
//! dropped from comparison rather than failing loudly, and #663's root
//! cause went undetected from the migration commit onward as a result. Here
//! a forgotten field is compared by default, so forgetting to update
//! [`classify_field`] for a new contract field cannot silently create a
//! blind spot; only a deliberate, reviewable addition to the match can. It
//! excludes exactly one key today: `"span"`. Every `"span"` value in a
//! `public_kernel_contract` v1 document
//! is a byte/line/column coordinate into the *source text that produced it*
//! (see `span_json` in `rust/fsl-core/src/public_kernel.rs`). Path B's source
//! text is the ephemeral string returned by `domain_kernel_source` — it does
//! not exist as a file, has different line/column numbers for every
//! construct than the original `.fsl` domain source, and is discarded after
//! parsing. Path A's spans point at the real domain source. These two
//! coordinate spaces cannot agree *in principle*, independent of any
//! lowering or rendering bug: this is condition 1 from the issue ("the field
//! cannot agree because path B's information is destroyed by serializing to
//! text and reparsing"). Excluding `"span"` does not leave provenance
//! unguarded: `rust/fsl-core/tests/origin_chain.rs` and
//! `rust/fslc/tests/origin_coverage.rs` separately gate the
//! `OriginRegistry`/provenance surface, which this v1 contract does not even
//! project (v1 has no `provenance` key; that is v2-only).
//!
//! Everything else — names, parameters, guards, `requires`/`let` order,
//! assignment targets and values, statement order, types, enum members,
//! invariants, transition properties, initial values, terminal conditions,
//! reachability properties, and the small `origin`/`requirement` metadata
//! objects (`declaration`, `lowered`, `generated`, requirement id/text) — is
//! compared. None of it is excluded, because none of it meets condition 1:
//! action names, guard text, and requirement ids are exactly the same
//! deterministic strings on both sides when the two implementations agree,
//! and a difference in any of them is a real lowering/rendering divergence,
//! not a serialization artifact.
//!
//! `exclusion_set_is_exactly_span_and_nothing_else` pins both directions of
//! that claim: the exclusion set is exercised (not dead/unused) and does not
//! silently grow to cover a real semantic field, by asserting
//! [`classify_field`] against the full, explicit vocabulary of keys that
//! `public_kernel_contract` v1 is known to emit.
//!
//! ## Corpus
//!
//! [`VALID_DOMAIN_FIXTURES`], [`SEMANTICALLY_INVALID_DOMAIN_FIXTURES`], and
//! [`SYNTAX_INVALID_DOMAIN_FIXTURES`] are the single named registration of
//! every `.fsl` file this gate knows about, split by what outcome is
//! expected. `corpus_discovery_matches_registered_classification` walks
//! `examples/` and `rust/fslc/tests/fixtures/` for every file containing a
//! top-level `domain <Name> {` declaration and fails loudly if that scan
//! finds a file absent from all three lists (a new spec silently outside
//! the gate) or a registered file the scan no longer finds (a stale
//! registration), mirroring the discipline `rust/fslc/src/outcome.rs` uses
//! for kernel-key classification.

use std::collections::BTreeSet;
use std::fs;
use std::path::{Path, PathBuf};

use fsl_core::{
    FsResolver, KernelModel, KernelSpec, build_model, domain_kernel_source, lower_domain,
    parse_kernel_source, public_kernel_contract,
};
use fsl_syntax::{DomainSpec, SurfaceDocument, parse_surface_document};
use serde_json::Value;

// ---------------------------------------------------------------------
// Corpus registration
// ---------------------------------------------------------------------

/// Domain specs that must parse, lower on both paths, and produce agreeing
/// checked models.
const VALID_DOMAIN_FIXTURES: &[&str] = &[
    "examples/annotations/annotated_domain.fsl",
    "examples/domain/order_async_effect.fsl",
    "examples/domain/order_fulfillment_saga.fsl",
    "examples/domain/order_functional_ddd.fsl",
    "examples/domain/unsafe_irreversible_effect_without_idempotency.fsl",
    "rust/fslc/tests/fixtures/domain_canonical_enum.fsl",
    "rust/fslc/tests/fixtures/domain_characterization/can_expansion_precedence.fsl",
    "rust/fslc/tests/fixtures/domain_characterization/container_defaults_surface.fsl",
    "rust/fslc/tests/fixtures/domain_characterization/effect_saga_valid.fsl",
    "rust/fslc/tests/fixtures/domain_characterization/lvalues_surface.fsl",
    "rust/fslc/tests/fixtures/domain_legacy_enum_union.fsl",
    "rust/fslc/tests/fixtures/domain_origin_violation.fsl",
    "rust/fslc/tests/fixtures/issue_515_domain_broken_invariant.fsl",
    "rust/fslc/tests/fixtures/issue_515_domain_clean_invariant.fsl",
    "rust/fslc/tests/fixtures/issue_518_domain_replay.fsl",
    "rust/fslc/tests/fixtures/issue_641_domain_clean.fsl",
    "rust/fslc/tests/fixtures/issue_641_domain_unreachable_decide.fsl",
];

/// Domain specs that parse into a [`DomainSpec`] but must be rejected by
/// both lowering pipelines (path A at `lower_domain`, path B somewhere in
/// `domain_kernel_source` -> `parse_kernel_source` -> `build_model`) because
/// they are semantically invalid (type mismatch / unknown symbol).
const SEMANTICALLY_INVALID_DOMAIN_FIXTURES: &[&str] = &[
    "rust/fslc/tests/fixtures/domain_characterization/invalid_duplicate_enum.fsl",
    "rust/fslc/tests/fixtures/domain_characterization/invalid_empty_enum_containers.fsl",
    "rust/fslc/tests/fixtures/domain_characterization/invalid_type_mismatch.fsl",
    "rust/fslc/tests/fixtures/domain_characterization/invalid_unknown_member.fsl",
    "rust/fslc/tests/fixtures/domain_characterization/invalid_unknown_name.fsl",
];

/// Domain specs that fail at the single shared surface parser
/// ([`parse_surface_document`]) before a [`DomainSpec`] exists at all. There
/// is only one parser, so there is no A/B pipeline divergence to check here;
/// both "paths" reduce to the same parse call, and it must reject.
const SYNTAX_INVALID_DOMAIN_FIXTURES: &[&str] = &[
    "rust/fslc/tests/fixtures/domain_characterization/invalid_broken_expression.fsl",
    "rust/fslc/tests/fixtures/domain_characterization/invalid_operator.fsl",
    "rust/fslc/tests/fixtures/domain_characterization/legacy_logical_parse_error.fsl",
];

/// Which side rejects a [`KnownDivergence`] fixture, or whether both accept
/// but the projected contracts genuinely disagree.
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
enum DivergenceShape {
    /// `lower_domain` (path A) rejects; the `domain_kernel_source` pipeline
    /// (path B) accepts and produces a checked model.
    PathARejects,
    /// The `domain_kernel_source` pipeline (path B) rejects; `lower_domain`
    /// (path A) accepts and produces a checked model.
    ///
    /// No [`KNOWN_DIVERGENT_DOMAIN_FIXTURES`] entry currently has this shape
    /// (the sole example, `lvalues_surface.fsl` / #691, was fixed and moved
    /// to [`VALID_DOMAIN_FIXTURES`]) -- kept for the next fixture that needs
    /// it, since `known_divergent_domain_fixture_pins_the_open_finding` and
    /// `assert_rejection_pinned` are already written generically over this
    /// enum's full three-shape taxonomy.
    #[allow(dead_code)]
    PathBRejects,
    /// Both paths accept and produce a checked model, but the two
    /// `public_kernel_contract` projections are not structurally equal.
    ContractsDisagree,
}

struct KnownDivergence {
    fixture: &'static str,
    shape: DivergenceShape,
    /// For [`DivergenceShape::PathARejects`] / [`DivergenceShape::PathBRejects`]:
    /// the single substring the rejecting side's error message must
    /// contain. For [`DivergenceShape::ContractsDisagree`]: every substring
    /// that must appear somewhere in the joined structural-diff output, so
    /// a change in *why* the two disagree (not just *whether*) also fails
    /// loudly.
    expected_contains: &'static [&'static str],
    /// The tracking issue that owns resolving (not just pinning) this
    /// divergence. A durable finding must not survive only in a review
    /// transcript or agent memory (AGENTS.md); this field is the pointer
    /// that keeps it from doing that.
    tracking_issue: &'static str,
}

/// Domain specs where path A ([`lower_domain`]) and path B
/// (`domain_kernel_source` -> `parse_kernel_source` -> `build_model`) are
/// **already known to disagree**, discovered while building this gate
/// (issue #664). Per the task instruction covering this discovery, a real
/// divergence found while building the agreement test is not fixed,
/// narrowed away, or silently normalized here: it is *pinned*, so the
/// disagreement stays visible and reviewable instead of silently
/// disappearing back into the untested gap this gate exists to close.
/// Resolving which side is wrong is out of this gate's scope. Each entry
/// below carries a `tracking_issue` -- the finding does not survive only in
/// a review transcript (AGENTS.md: "do not let the finding survive only in
/// chat, a review transcript, or agent memory").
///
/// **#690** <https://github.com/ymm-oss/fsl/issues/690> covers both entries
/// below as one root cause: `domain.rs`'s `Context::normalize`
/// (`rust/fsl-core/src/domain.rs:301`) is a chain of `str::replace` calls
/// over rendered text with no syntax tree, so it cannot be scope-aware
/// (entry 2's `quantity` shadowing) or precedence-aware (entry 2's
/// `can(...)` expansion) the way a typed AST composition can. Entry 1's
/// generated-name leak is a symptom of the same string-level substitution
/// having no notion of what is and is not a legal domain-level reference.
///
/// **#691** <https://github.com/ymm-oss/fsl/issues/691> covered a third,
/// now-resolved entry (`lvalues_surface.fsl`, a `Map<K, V>` domain state
/// field with no explicit default): a missing match arm in
/// `Context::default` rather than a substitution-order problem, fixed by
/// making `Context::default`/`Context::default_for_type` total over
/// `SyntaxTypeExprKind` with no catch-all arm. The two paths now agree on
/// that fixture; it has moved to [`VALID_DOMAIN_FIXTURES`].
///
/// 1. `ai_internal_name_misuse.fsl` writes the invariant
///    `status == Status_Draft`, directly naming the *generated*
///    kernel-level enum member (`Status_Draft`) instead of the
///    domain-level member (`Draft`). `lower_domain` (path A -- what
///    `check`/`verify` run) rejects this as
///    `unknown domain symbol 'Status_Draft'`, matching
///    `rust/fslc/tests/fixtures/domain_characterization/baseline.v1.json`'s
///    recorded `check`/`verify` outcome and the fixture's own documented
///    purpose (`ai_native_cases.v1.json` case
///    `generated-enum-name-check-gap`, `misuse.internal_generated_name: 1`):
///    domain-level code must not reference compiler-generated names.
///    `domain_kernel_source` (path B -- what `domain expand` /
///    `check_domain` run) does not
///    reject it: its textual substitution only rewrites *bare* enum member
///    names it recognizes (`Draft` -> `Status_Draft`); the already-qualified
///    name the fixture writes is left untouched, and happens to be
///    byte-identical to the name path A's own generator would have
///    produced, so the rendered text parses and type-checks as an ordinary
///    valid kernel spec.
///
/// 2. `expressions_valid.fsl` both paths accept, but the projected
///    contracts still disagree at one point (name-shadowing, #690 symptom
///    2). A second point that used to disagree here -- `can(...)`
///    operator-precedence misgrouping, #690 symptom 1 -- was fixed and is
///    described below, after this list, rather than removed from the
///    historical record:
///
///    - `command Approve { quantity: Quantity }` shares a name with the
///      aggregate's own `quantity` state field. Inside `evolve Approved`,
///      `quantity = quantity` means "copy the incoming event field into
///      state". `lower_domain`'s resolver (path A) is scope-aware and keeps
///      the right-hand `quantity` as the event-field reference.
///      `domain.rs`'s `Context::normalize` (path B,
///      `replace_identifier`/`evolve_assignments`) renames *every* lexical
///      occurrence of a state field name it finds, with no notion of a
///      shadowing local/event-field binding, and rewrites the right-hand
///      side too -- producing `order_quantity = order_quantity`, a no-op
///      that silently drops the incoming event payload. The same
///      mis-substitution reaches the `decide Approve` guard
///      `quantity >= 0`, which refers to the *command input* `quantity`,
///      not the state field. This needs a scope-aware substitution (design
///      option B/C in #690) and is out of scope for the `can(...)` fix.
///
///    Before #690's fix, this fixture's
///    `invariant legacyImplication { status == Cancelled -> not can(Cancel) }`
///    also disagreed at the projected `and`/`or` operator shape:
///    `domain.rs`'s `can(...)` expansion (path B, `Context::normalize`
///    around line 328) joined the requires/rejects pieces with literal
///    `" and "` without individually parenthesizing each piece, so the
///    rendered text read
///    `status == Draft or status == Approved and not (status == Cancelled)`
///    -- `and` binds tighter than `or` in FSL's grammar, so this re-parsed
///    as `Draft or (Approved and not Cancelled)`, not the intended
///    `(Draft or Approved) and not Cancelled` that `lower_domain`'s typed
///    AST composition (path A) builds directly and therefore could not get
///    wrong the same way. In *this* fixture, `decide Cancel`'s pieces are
///    over a single-valued enum and mutually exclusive, so the misgrouping
///    only changed the JSON AST shape here, not the truth value. It was
///    worse in general: the same misgrouping could flip a verdict once the
///    pieces are over independent `Bool` state -- see
///    `can_expansion_precedence.fsl` in [`VALID_DOMAIN_FIXTURES`], which
///    pins exactly that (`decide Open { requires a or b  requires c  emits
///    Opened }` with `invariant aImpliesCanOpen { a => can(Open) }` used to
///    render the tautology `gate_a => (gate_a or gate_b and gate_c)`
///    instead of the intended, sometimes-false `gate_a => ((gate_a or
///    gate_b) and gate_c)`, so `fslc verify` returned `violated` on the
///    checked model and `verified` on the rendered/re-parsed one for the
///    identical domain spec -- a false green, the class AGENTS.md ranks
///    above a crash). #690's fix parenthesizes each piece individually
///    before joining, which is why this fixture's `can(Cancel)` fingerprint
///    is gone from [`KNOWN_DIVERGENT_DOMAIN_FIXTURES`] below while the
///    `quantity`/`order_quantity` fingerprint remains.
///
/// `known_divergent_domain_fixture_pins_the_open_finding` asserts each
/// fixture's exact shape so an incidental change that makes the two agree --
/// in either direction -- fails this test and forces a deliberate move of
/// the fixture into [`VALID_DOMAIN_FIXTURES`] or
/// [`SEMANTICALLY_INVALID_DOMAIN_FIXTURES`] instead of a silent behavior
/// change sliding past this gate.
const KNOWN_DIVERGENT_DOMAIN_FIXTURES: &[KnownDivergence] = &[
    KnownDivergence {
        fixture: "rust/fslc/tests/fixtures/domain_characterization/ai_internal_name_misuse.fsl",
        shape: DivergenceShape::PathARejects,
        expected_contains: &["unknown domain symbol 'Status_Draft'"],
        tracking_issue: "https://github.com/ymm-oss/fsl/issues/690",
    },
    KnownDivergence {
        fixture: "rust/fslc/tests/fixtures/domain_characterization/expressions_valid.fsl",
        shape: DivergenceShape::ContractsDisagree,
        // #690 symptom 1 (the `can(...)` operator-precedence misgrouping,
        // previously pinned here as `"operator: path A = \"and\", path B =
        // \"or\""`) is fixed: `Context::normalize` now parenthesizes each
        // `requires`/`rejects` piece individually before joining, so this
        // fixture's `can(Cancel)` projection no longer disagrees with path
        // A. Only #690 symptom 2 remains here: the name-shadowing rewrite
        // still needs a scope-aware substitution (design option B/C, still
        // open), so `quantity`/`order_quantity` still disagrees.
        expected_contains: &["path A = \"quantity\", path B = \"order_quantity\""],
        tracking_issue: "https://github.com/ymm-oss/fsl/issues/690",
    },
];

fn repo_root() -> PathBuf {
    Path::new(env!("CARGO_MANIFEST_DIR"))
        .parent()
        .expect("rust/ directory")
        .parent()
        .expect("repository root")
        .to_path_buf()
}

/// True when `line` is (ignoring leading whitespace) a top-level
/// `domain <Name>` declaration.
fn is_domain_declaration_line(line: &str) -> bool {
    let Some(rest) = line.trim_start().strip_prefix("domain") else {
        return false;
    };
    let mut chars = rest.chars();
    let Some(separator) = chars.next() else {
        return false;
    };
    separator.is_whitespace() && chars.next().is_some_and(char::is_alphabetic)
}

fn walk_fsl_files(dir: &Path, out: &mut Vec<PathBuf>) {
    let Ok(entries) = fs::read_dir(dir) else {
        return;
    };
    for entry in entries.filter_map(Result::ok) {
        let path = entry.path();
        if path.is_dir() {
            walk_fsl_files(&path, out);
        } else if path.extension().is_some_and(|ext| ext == "fsl") {
            out.push(path);
        }
    }
}

/// Every `.fsl` file under `examples/` and `rust/fslc/tests/fixtures/`
/// containing a top-level `domain` declaration, as a repo-relative,
/// forward-slash path, sorted.
fn discover_domain_fixtures() -> Vec<String> {
    let root = repo_root();
    let mut files = Vec::new();
    walk_fsl_files(&root.join("examples"), &mut files);
    walk_fsl_files(&root.join("rust/fslc/tests/fixtures"), &mut files);
    let mut discovered = files
        .into_iter()
        .filter(|path| {
            fs::read_to_string(path)
                .ok()
                .is_some_and(|content| content.lines().any(is_domain_declaration_line))
        })
        .map(|path| {
            path.strip_prefix(&root)
                .expect("fixture path under repo root")
                .components()
                .map(|component| component.as_os_str().to_string_lossy().into_owned())
                .collect::<Vec<_>>()
                .join("/")
        })
        .collect::<Vec<_>>();
    discovered.sort();
    discovered
}

#[test]
fn corpus_discovery_matches_registered_classification() {
    let discovered = discover_domain_fixtures();
    let mut registered = VALID_DOMAIN_FIXTURES
        .iter()
        .copied()
        .chain(SEMANTICALLY_INVALID_DOMAIN_FIXTURES.iter().copied())
        .chain(SYNTAX_INVALID_DOMAIN_FIXTURES.iter().copied())
        .chain(
            KNOWN_DIVERGENT_DOMAIN_FIXTURES
                .iter()
                .map(|entry| entry.fixture),
        )
        .map(str::to_owned)
        .collect::<Vec<_>>();
    registered.sort();

    let discovered_set = discovered.iter().cloned().collect::<BTreeSet<_>>();
    let registered_set = registered.iter().cloned().collect::<BTreeSet<_>>();

    let unregistered = discovered_set
        .difference(&registered_set)
        .collect::<Vec<_>>();
    let stale = registered_set
        .difference(&discovered_set)
        .collect::<Vec<_>>();
    assert!(
        unregistered.is_empty() && stale.is_empty(),
        "domain fixture corpus drifted from registration in \
         rust/fsl-core/tests/domain_render_agreement.rs.\n\
         Discovered but NOT registered (add to one of the three lists): {unregistered:?}\n\
         Registered but NOT discovered (fixture removed or renamed): {stale:?}"
    );
}

// ---------------------------------------------------------------------
// Field classification (the gated exclusion set)
// ---------------------------------------------------------------------

#[derive(Clone, Copy, Debug, PartialEq, Eq)]
enum FieldClass {
    /// Compared for structural equality between path A and path B.
    Compared,
    /// Excluded because it is a source-text coordinate that cannot agree in
    /// principle once path B has gone through text serialization/reparse.
    ExcludedSourceSpan,
}

/// The single named place deciding what a `public_kernel_contract` v1 JSON
/// object key means for this comparison. Anything not named here is
/// [`FieldClass::Compared`] by default — this is a closed exclusion
/// allow-list, not an inclusion allow-list, so a newly added kernel field is
/// automatically compared instead of silently dropped.
fn classify_field(key: &str) -> FieldClass {
    match key {
        "span" => FieldClass::ExcludedSourceSpan,
        _ => FieldClass::Compared,
    }
}

/// Every object key `public_kernel_contract` v1 is known to emit today
/// (enumerated from `rust/fsl-core/src/public_kernel.rs`), used only to keep
/// [`classify_field`] honest: each of these must classify as `Compared`. If
/// a future change makes one of them `ExcludedSourceSpan` instead, this test
/// fails and forces a reviewed, deliberate edit rather than a silent
/// widening of the exclusion set.
const KNOWN_COMPARED_KEYS: &[&str] = &[
    "$schema",
    "schema_version",
    "language_version",
    "spec",
    "name",
    "source",
    "file",
    "dialect",
    "semantics",
    "assignment",
    "reads",
    "requires_false",
    "failure_state",
    "old",
    "integer_division",
    "terminal_deadlock",
    "fairness",
    "constants",
    "types",
    "state",
    "type",
    "value",
    "symmetric",
    "definition",
    "kind",
    "lo",
    "hi",
    "members",
    "fields",
    "origin",
    "declaration",
    "lowered",
    "generated",
    "init",
    "statements",
    "requirement",
    "id",
    "text",
    "actions",
    "parameters",
    "finite_domain",
    "fair",
    "guards",
    "requires",
    "lets",
    "updates",
    "update_semantics",
    "ensures",
    "partial_operations",
    "expression",
    "target",
    "condition",
    "then",
    "else",
    "binder",
    "properties",
    "invariants",
    "transitions",
    "reachables",
    "source_kind",
    "leads_to",
    "binders",
    "before",
    "after",
    "within",
    "decreases",
    "terminal",
];

#[test]
fn exclusion_set_is_exactly_span_and_nothing_else() {
    for key in KNOWN_COMPARED_KEYS {
        assert_eq!(
            classify_field(key),
            FieldClass::Compared,
            "known kernel-contract field '{key}' must stay compared; if it \
             was intentionally moved to the exclusion set it must meet \
             condition 1 (source-coordinate destroyed by text round-trip) \
             and this constant must be updated to say so explicitly",
        );
    }
    assert_eq!(classify_field("span"), FieldClass::ExcludedSourceSpan);
}

// ---------------------------------------------------------------------
// Structural diff
// ---------------------------------------------------------------------

/// Recursively compare `a` (path A) against `b` (path B), skipping only
/// keys classified [`FieldClass::ExcludedSourceSpan`] by [`classify_field`].
/// Records every excluded key actually encountered in `excluded_seen`, and
/// every disagreement as a human-readable JSON-pointer-style message in
/// `mismatches`.
fn diff_json(
    pointer: &str,
    a: &Value,
    b: &Value,
    excluded_seen: &mut BTreeSet<String>,
    mismatches: &mut Vec<String>,
) {
    match (a, b) {
        (Value::Object(map_a), Value::Object(map_b)) => {
            let mut keys = map_a.keys().cloned().collect::<BTreeSet<_>>();
            keys.extend(map_b.keys().cloned());
            for key in keys {
                let child_pointer = format!("{pointer}/{key}");
                if classify_field(&key) == FieldClass::ExcludedSourceSpan {
                    excluded_seen.insert(key.clone());
                    match (map_a.contains_key(&key), map_b.contains_key(&key)) {
                        (true, true) | (false, false) => {}
                        _ => mismatches.push(format!(
                            "{child_pointer}: excluded field present on one side only"
                        )),
                    }
                    continue;
                }
                match (map_a.get(&key), map_b.get(&key)) {
                    (Some(value_a), Some(value_b)) => {
                        diff_json(&child_pointer, value_a, value_b, excluded_seen, mismatches);
                    }
                    (Some(_), None) => mismatches.push(format!(
                        "{child_pointer}: present only via lower_domain (path A)"
                    )),
                    (None, Some(_)) => mismatches.push(format!(
                        "{child_pointer}: present only via domain_kernel_source (path B)"
                    )),
                    (None, None) => unreachable!("key came from at least one map"),
                }
            }
        }
        (Value::Array(items_a), Value::Array(items_b)) => {
            if items_a.len() != items_b.len() {
                mismatches.push(format!(
                    "{pointer}: array length differs (path A={}, path B={})",
                    items_a.len(),
                    items_b.len()
                ));
            }
            for (index, (value_a, value_b)) in items_a.iter().zip(items_b.iter()).enumerate() {
                diff_json(
                    &format!("{pointer}/{index}"),
                    value_a,
                    value_b,
                    excluded_seen,
                    mismatches,
                );
            }
        }
        _ => {
            if a != b {
                mismatches.push(format!("{pointer}: path A = {a}, path B = {b}"));
            }
        }
    }
}

// ---------------------------------------------------------------------
// Lowering pipelines
// ---------------------------------------------------------------------

enum PipelineOutcome {
    // Boxed: `KernelModel` is large relative to `Rejected(String)`, and
    // every non-trivial use of this variant already goes through
    // `&kernel`/`&model` call sites where `Box` derefs transparently.
    Checked(Box<KernelSpec>, Box<KernelModel>),
    Rejected(String),
}

fn parse_domain_spec(source: &str) -> Result<DomainSpec, String> {
    match parse_surface_document(source) {
        Ok(SurfaceDocument::Domain(domain)) => Ok(domain),
        Ok(_) => Err("expected a domain document".to_owned()),
        Err(error) => Err(error.to_string()),
    }
}

/// Path A: `lower_domain` -> `build_model`.
fn run_path_a(domain: &DomainSpec) -> PipelineOutcome {
    match lower_domain(domain) {
        Ok(kernel) => match build_model(kernel.clone()) {
            Ok(model) => PipelineOutcome::Checked(Box::new(kernel), Box::new(model)),
            Err(error) => PipelineOutcome::Rejected(error.to_string()),
        },
        Err(error) => PipelineOutcome::Rejected(error.to_string()),
    }
}

/// Path B: `domain_kernel_source` -> `parse_kernel_source` -> `build_model`.
fn run_path_b(domain: &DomainSpec) -> PipelineOutcome {
    let source = match domain_kernel_source(domain) {
        Ok(source) => source,
        Err(error) => return PipelineOutcome::Rejected(error.to_string()),
    };
    run_rendered_kernel(&source)
}

fn run_rendered_kernel(source: &str) -> PipelineOutcome {
    let kernel = match parse_kernel_source(source, &FsResolver::new(".")) {
        Ok(kernel) => kernel,
        Err(error) => return PipelineOutcome::Rejected(error.to_string()),
    };
    match build_model(kernel.clone()) {
        Ok(model) => PipelineOutcome::Checked(Box::new(kernel), Box::new(model)),
        Err(error) => PipelineOutcome::Rejected(error.to_string()),
    }
}

// ---------------------------------------------------------------------
// Tests
// ---------------------------------------------------------------------

/// #691's rejecting controls: reintroducing the former `= 0` rendering for
/// any accepted container default must be detected before an agreement claim
/// can be made. Each case starts from the real path-B output, applies only the
/// historical faulty initializer, and proves the checked kernel rejects it.
#[test]
fn container_default_zero_regressions_are_rejected() {
    let root = repo_root();
    let cases = [
        (
            "rust/fslc/tests/fixtures/domain_characterization/container_defaults_surface.fsl",
            "basket_picked = none",
            "basket_picked = 0",
        ),
        (
            "rust/fslc/tests/fixtures/domain_characterization/container_defaults_surface.fsl",
            "basket_seen = Set {}",
            "basket_seen = 0",
        ),
        (
            "rust/fslc/tests/fixtures/domain_characterization/lvalues_surface.fsl",
            "forall k: ItemId { inventory_counts[k] = 0 }",
            "inventory_counts = 0",
        ),
    ];

    for (relative, accepted, faulty) in cases {
        let source = fs::read_to_string(root.join(relative))
            .unwrap_or_else(|error| panic!("read {relative}: {error}"));
        let domain = parse_domain_spec(&source).unwrap_or_else(|error| {
            panic!("{relative}: expected a parseable domain document: {error}")
        });
        let rendered = domain_kernel_source(&domain)
            .unwrap_or_else(|error| panic!("{relative}: render path B: {error}"));
        assert!(
            rendered.contains(accepted),
            "{relative}: accepting control '{accepted}' is absent from path-B output"
        );
        let faulty_source = rendered.replacen(accepted, faulty, 1);
        match run_rendered_kernel(&faulty_source) {
            PipelineOutcome::Rejected(message) => assert!(
                message.contains("is not assignable"),
                "{relative}: faulty initializer '{faulty}' was rejected for an unexpected reason: {message}"
            ),
            PipelineOutcome::Checked(..) => {
                panic!("{relative}: faulty initializer '{faulty}' produced a checked kernel")
            }
        }
    }
}

/// Rejecting controls for every fail-closed branch added by #691. These are
/// intentionally separate cases: a single invalid fixture would stop at its
/// first error and leave the later type-shape branches uncalibrated.
#[test]
fn unsupported_default_shapes_fail_closed_with_original_origins() {
    let cases = [
        (
            "nested Map value",
            "Map<Id, Map<Id, Id>>",
            "",
            "Map state requires explicit initialization through supported semantics",
        ),
        (
            "Seq state",
            "Seq<Id>",
            "",
            "unsupported domain type constructor 'Seq'/1",
        ),
        (
            "unknown constructor",
            "Bag<Id>",
            "",
            "unsupported domain type constructor 'Bag'/1",
        ),
        (
            "malformed Map arity",
            "Map<Id>",
            "",
            "unsupported domain type constructor 'Map'/1",
        ),
        (
            "explicit whole-Map default",
            "Map<Id, Id>",
            " = 0",
            "whole-Map domain defaults are not supported",
        ),
        (
            "non-scalar Map key",
            "Map<Option<Id>, Id>",
            "",
            "map keys require a scalar or named type",
        ),
    ];

    for (label, type_name, explicit_default, expected) in cases {
        let source = format!(
            "domain InvalidDefaultShape {{\n  type Id = 0..1\n  aggregate A {{\n    state {{\n      value: {type_name}{explicit_default};\n    }}\n  }}\n}}\n"
        );
        let domain = parse_domain_spec(&source)
            .unwrap_or_else(|error| panic!("{label}: expected parseable domain: {error}"));
        let field_span = domain.aggregates[0].state[0].span;
        let Err(error) = domain_kernel_source(&domain) else {
            panic!("{label}: renderer unexpectedly accepted invalid shape");
        };
        assert_eq!(error.message, expected, "{label}");
        assert_eq!(
            (error.line, error.column),
            (field_span.start.line, field_span.start.column),
            "{label}"
        );
        let origin = error
            .origin
            .as_deref()
            .unwrap_or_else(|| panic!("{label}: renderer discarded the domain origin"));
        assert_eq!(
            origin.primary.as_ref().and_then(|site| site.span),
            Some(field_span),
            "{label}"
        );
        assert_eq!(origin.lowering_steps.len(), 1, "{label}");
        assert_eq!(
            origin.lowering_steps[0].kind, "render_domain_kernel_source",
            "{label}"
        );
    }
}

/// Rejected declaration control for every affected container position. Empty
/// enums are parseable surface ASTs but cannot produce an executable kernel;
/// both paths must reject at the enum declaration before path B serializes
/// invalid text, independent of whether the enum is direct, nested, a Map
/// key, or a Map value.
#[test]
fn empty_enum_declarations_fail_closed_before_rendering() {
    let source = r"
domain EmptyEnumContainers {
  enum Status {}
  aggregate A {
    state {
      direct: Status;
      optional: Option<Status>;
      members: Set<Status>;
      keyed: Map<Status, Bool>;
      by_key: Map<Bool, Status>;
    }
  }
}
";

    let domain = parse_domain_spec(source).expect("parse empty-enum domain");
    let enum_span = domain.types[0].span;
    let path_a = lower_domain(&domain).expect_err("typed path must reject empty enum");
    let path_b = domain_kernel_source(&domain).expect_err("renderer must reject empty enum");
    for (path, error) in [("path A", path_a), ("path B", path_b)] {
        assert_eq!(error.message, "enum 'Status' has no members", "{path}");
        assert_eq!(error.line, enum_span.start.line, "{path}");
        assert_eq!(error.column, enum_span.start.column, "{path}");
    }
}

#[test]
fn syntax_invalid_domain_fixtures_fail_to_parse() {
    let root = repo_root();
    for relative in SYNTAX_INVALID_DOMAIN_FIXTURES {
        let source = fs::read_to_string(root.join(relative))
            .unwrap_or_else(|error| panic!("read {relative}: {error}"));
        let result = parse_surface_document(&source);
        assert!(
            result.is_err(),
            "{relative}: expected the shared surface parser to reject this \
             deliberately invalid fixture, but it parsed successfully"
        );
    }
}

#[test]
fn semantically_invalid_domain_fixtures_are_rejected_by_both_lowering_paths() {
    let root = repo_root();
    for relative in SEMANTICALLY_INVALID_DOMAIN_FIXTURES {
        let source = fs::read_to_string(root.join(relative))
            .unwrap_or_else(|error| panic!("read {relative}: {error}"));
        let domain = parse_domain_spec(&source).unwrap_or_else(|error| {
            panic!("{relative}: expected a parseable domain document: {error}")
        });

        let a = run_path_a(&domain);
        let b = run_path_b(&domain);
        match (a, b) {
            (PipelineOutcome::Rejected(_), PipelineOutcome::Rejected(_)) => {}
            (PipelineOutcome::Checked(..), PipelineOutcome::Rejected(message)) => panic!(
                "{relative}: lower_domain (path A) accepted this deliberately \
                 invalid spec while domain_kernel_source (path B) rejected it \
                 ({message}) -- one path only rejecting is itself a finding, \
                 not something to normalize away"
            ),
            (PipelineOutcome::Rejected(message), PipelineOutcome::Checked(..)) => panic!(
                "{relative}: domain_kernel_source (path B) accepted this \
                 deliberately invalid spec while lower_domain (path A) \
                 rejected it ({message}) -- one path only rejecting is itself \
                 a finding, not something to normalize away"
            ),
            (PipelineOutcome::Checked(..), PipelineOutcome::Checked(..)) => panic!(
                "{relative}: both lowering paths accepted a deliberately \
                 invalid domain spec"
            ),
        }
    }
}

/// Shared assertion for [`DivergenceShape::PathARejects`] /
/// [`DivergenceShape::PathBRejects`]: the rejecting side's error message
/// must still contain every substring the entry pins.
fn assert_rejection_pinned(
    relative: &str,
    rejecting_side: &str,
    message: &str,
    entry: &KnownDivergence,
) {
    for expected in entry.expected_contains {
        assert!(
            message.contains(expected),
            "{relative}: {rejecting_side}'s rejection reason changed shape \
             (now: {message}, expected to contain '{expected}'); re-evaluate \
             whether this fixture still belongs on \
             KNOWN_DIVERGENT_DOMAIN_FIXTURES (tracking: {})",
            entry.tracking_issue
        );
    }
}

/// Shared assertion for [`DivergenceShape::ContractsDisagree`]: both paths
/// accept, but the projected contracts must still disagree, and the diff
/// must still contain every substring the entry pins.
fn assert_contracts_disagree_pinned(
    relative: &str,
    kernel_a: &KernelSpec,
    model_a: &KernelModel,
    kernel_b: &KernelSpec,
    model_b: &KernelModel,
    entry: &KnownDivergence,
) {
    let contract_a = public_kernel_contract(kernel_a, model_a, relative, "domain")
        .unwrap_or_else(|error| panic!("{relative}: project path A contract: {error}"));
    let contract_b = public_kernel_contract(kernel_b, model_b, relative, "domain")
        .unwrap_or_else(|error| panic!("{relative}: project path B contract: {error}"));
    let mut excluded_seen = BTreeSet::new();
    let mut mismatches = Vec::new();
    diff_json(
        "",
        &contract_a,
        &contract_b,
        &mut excluded_seen,
        &mut mismatches,
    );
    assert!(
        !mismatches.is_empty(),
        "{relative}: the pinned contract disagreement is gone (the two \
         projections now agree). Move this fixture to VALID_DOMAIN_FIXTURES \
         instead of leaving it here (tracking: {})",
        entry.tracking_issue
    );
    let joined = mismatches.join("\n");
    for expected in entry.expected_contains {
        assert!(
            joined.contains(expected),
            "{relative}: the pinned disagreement changed shape; expected the \
             diff to contain '{expected}' but got:\n{joined} (tracking: {})",
            entry.tracking_issue
        );
    }
}

/// Pins the exact shape of each divergence documented on
/// [`KNOWN_DIVERGENT_DOMAIN_FIXTURES`]. This test is expected to fail loudly
/// the moment a pinned shape changes, whether by an intentional fix or an
/// incidental regression, so the change gets reviewed instead of silently
/// landing.
#[test]
fn known_divergent_domain_fixture_pins_the_open_finding() {
    let root = repo_root();
    for entry in KNOWN_DIVERGENT_DOMAIN_FIXTURES {
        let relative = entry.fixture;
        let source = fs::read_to_string(root.join(relative))
            .unwrap_or_else(|error| panic!("read {relative}: {error}"));
        let domain = parse_domain_spec(&source).unwrap_or_else(|error| {
            panic!("{relative}: expected a parseable domain document: {error}")
        });

        match (entry.shape, run_path_a(&domain), run_path_b(&domain)) {
            (
                DivergenceShape::PathARejects,
                PipelineOutcome::Rejected(message),
                PipelineOutcome::Checked(..),
            ) => assert_rejection_pinned(relative, "path A", &message, entry),
            (
                DivergenceShape::PathBRejects,
                PipelineOutcome::Checked(..),
                PipelineOutcome::Rejected(message),
            ) => assert_rejection_pinned(relative, "path B", &message, entry),
            (
                DivergenceShape::ContractsDisagree,
                PipelineOutcome::Checked(kernel_a, model_a),
                PipelineOutcome::Checked(kernel_b, model_b),
            ) => assert_contracts_disagree_pinned(
                relative, &kernel_a, &model_a, &kernel_b, &model_b, entry,
            ),
            (_, PipelineOutcome::Checked(..), PipelineOutcome::Checked(..)) => panic!(
                "{relative}: both paths now accept this fixture and the \
                 shape is not ContractsDisagree -- the pinned divergence is \
                 gone. Move this fixture to VALID_DOMAIN_FIXTURES (and \
                 confirm the two contracts actually agree) instead of \
                 leaving it here."
            ),
            (_, PipelineOutcome::Rejected(_), PipelineOutcome::Rejected(_)) => panic!(
                "{relative}: both paths now reject this fixture -- the \
                 pinned divergence is gone. Move this fixture to \
                 SEMANTICALLY_INVALID_DOMAIN_FIXTURES instead of leaving it \
                 here."
            ),
            (_, PipelineOutcome::Rejected(message), PipelineOutcome::Checked(..)) => panic!(
                "{relative}: the divergence flipped shape (path A now \
                 rejects with: {message}, path B accepts) -- re-pin \
                 deliberately instead of leaving a stale description"
            ),
            (_, PipelineOutcome::Checked(..), PipelineOutcome::Rejected(message)) => panic!(
                "{relative}: the divergence flipped shape (path A now \
                 accepts, path B rejects with: {message}) -- re-pin \
                 deliberately instead of leaving a stale description"
            ),
        }
    }
}

#[test]
fn valid_domain_corpus_lowering_and_rendering_agree() {
    let root = repo_root();
    let mut failures = Vec::new();
    let mut excluded_seen = BTreeSet::new();

    for relative in VALID_DOMAIN_FIXTURES {
        let source = fs::read_to_string(root.join(relative))
            .unwrap_or_else(|error| panic!("read {relative}: {error}"));
        let domain = parse_domain_spec(&source).unwrap_or_else(|error| {
            panic!("{relative}: expected a parseable domain document: {error}")
        });

        let (kernel_a, model_a) = match run_path_a(&domain) {
            PipelineOutcome::Checked(kernel, model) => (kernel, model),
            PipelineOutcome::Rejected(message) => {
                panic!(
                    "{relative}: lower_domain (path A) unexpectedly rejected a valid corpus spec: {message}"
                );
            }
        };
        let (kernel_b, model_b) = match run_path_b(&domain) {
            PipelineOutcome::Checked(kernel, model) => (kernel, model),
            PipelineOutcome::Rejected(message) => {
                panic!(
                    "{relative}: domain_kernel_source (path B) unexpectedly \
                     rejected a valid corpus spec: {message}"
                );
            }
        };

        // Both projections use the identical caller-supplied source_path
        // and dialect so that fixture setup, not the thing under test,
        // decides those two fields.
        let contract_a = public_kernel_contract(&kernel_a, &model_a, relative, "domain")
            .unwrap_or_else(|error| panic!("{relative}: project path A contract: {error}"));
        let contract_b = public_kernel_contract(&kernel_b, &model_b, relative, "domain")
            .unwrap_or_else(|error| panic!("{relative}: project path B contract: {error}"));

        let mut mismatches = Vec::new();
        diff_json(
            "",
            &contract_a,
            &contract_b,
            &mut excluded_seen,
            &mut mismatches,
        );
        if !mismatches.is_empty() {
            failures.push(format!("{relative}:\n  {}", mismatches.join("\n  ")));
        }
    }

    assert!(
        failures.is_empty(),
        "lower_domain (path A) and domain_kernel_source (path B) disagree \
         for {} of {} corpus spec(s):\n\n{}",
        failures.len(),
        VALID_DOMAIN_FIXTURES.len(),
        failures.join("\n\n")
    );

    // The exclusion set must actually be exercised by real corpus data, or
    // it is dead configuration that could hide a future silent widening.
    let expected_excluded = ["span".to_owned()].into_iter().collect::<BTreeSet<_>>();
    assert_eq!(
        excluded_seen, expected_excluded,
        "the exclusion set exercised while comparing the full valid corpus \
         must be exactly {{\"span\"}}: got {excluded_seen:?}. An empty set \
         means the corpus never exercised the exclusion (dead \
         configuration); anything beyond \"span\" means a field started \
         being excluded outside classify_field's declared allow-list."
    );
}
