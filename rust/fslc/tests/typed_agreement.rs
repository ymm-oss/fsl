// SPDX-License-Identifier: Apache-2.0
// Copyright 2026 Ryoichi Izumita

//! C6 typed generative / metamorphic cross-engine agreement suite (#537 C6
//! slice 1, issue #648).
//!
//! Generates checked `KernelModel`s (never string fuzz) from a deterministic
//! structural axis enumeration (`generator.rs`), compares Monitor BFS /
//! explicit / BMC bounded verdicts and successors (`engines.rs`), and checks
//! seven metamorphic relations with a negative control each (`relations.rs`).
//! `sweep_summary.rs` aggregates what each sweep actually exercised. Slice 2
//! adds an expression-variant family that is also the exercising evidence for
//! the C3 `expr` and `types` axes.
//!
//! See `docs/design/DESIGN-conformance-harness.md`'s "Typed generative /
//! metamorphic agreement (#537 C6)" section for the accepted design this
//! implements, including why Z3js/Worker parity is out of scope here.

#[path = "typed_agreement/engines.rs"]
mod engines;
#[path = "assurance/enum_rows.rs"]
mod enum_rows;
#[path = "typed_agreement/generator.rs"]
mod generator;
#[path = "typed_agreement/inventory.rs"]
mod inventory;
#[path = "typed_agreement/logic_test.rs"]
mod logic_test;
#[path = "typed_agreement/regression_corpus.rs"]
mod regression_corpus;
#[path = "typed_agreement/relations.rs"]
mod relations;
#[path = "typed_agreement/shrink.rs"]
mod shrink;
#[path = "typed_agreement/sweep_summary.rs"]
mod sweep_summary;

use std::collections::BTreeSet;

use enum_rows::{
    aggregate_kind_row, aggregate_kind_rows, checked_expr_variant_rows, expr_variant_row,
    type_def_row, type_ref_row, type_rows,
};
use fsl_core::{KernelModel, TypeDef, TypeRef};
use fsl_syntax::{Binder, Expr};
use generator::{
    PARTIAL_INVENTORY_PLACEMENTS, PropertyKind, domain_sweep, expression_sweep, operation_sweep,
    partial_inventory_source, partial_inventory_sweep,
};
use sweep_summary::SweepSummary;

include!("typed_agreement/nested_options.rs");

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
/// 21 non-aggregate executable variants plus four separate aggregate-kind
/// models. `Call` and `Stage` are fail-closed before evaluator entry.
const EXPRESSION_SWEEP_FLOOR: usize = 25;

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

fn visit_binder(
    binder: &Binder,
    expr_rows: &mut BTreeSet<&'static str>,
    aggregate_rows: &mut BTreeSet<&'static str>,
) {
    match binder {
        Binder::Typed { where_expr, .. } => {
            if let Some(expression) = where_expr {
                visit_expr(expression, expr_rows, aggregate_rows);
            }
        }
        Binder::Range {
            lo, hi, where_expr, ..
        } => {
            visit_expr(lo, expr_rows, aggregate_rows);
            visit_expr(hi, expr_rows, aggregate_rows);
            if let Some(expression) = where_expr {
                visit_expr(expression, expr_rows, aggregate_rows);
            }
        }
        Binder::Collection {
            collection,
            where_expr,
            ..
        } => {
            visit_expr(collection, expr_rows, aggregate_rows);
            if let Some(expression) = where_expr {
                visit_expr(expression, expr_rows, aggregate_rows);
            }
        }
    }
}

#[allow(clippy::too_many_lines)]
fn visit_expr(
    expr: &Expr,
    expr_rows: &mut BTreeSet<&'static str>,
    aggregate_rows: &mut BTreeSet<&'static str>,
) {
    expr_rows.insert(expr_variant_row(expr));
    match expr {
        Expr::Some(value)
        | Expr::Neg(value)
        | Expr::Not(value)
        | Expr::Field(value, _)
        | Expr::Stage { entity: value, .. }
        | Expr::UnaryNamed { expr: value, .. }
        | Expr::Is { expr: value, .. } => visit_expr(value, expr_rows, aggregate_rows),
        Expr::Set(values) | Expr::Seq(values) => {
            for value in values {
                visit_expr(value, expr_rows, aggregate_rows);
            }
        }
        Expr::Struct { fields, .. } => {
            for (_, value) in fields {
                visit_expr(value, expr_rows, aggregate_rows);
            }
        }
        Expr::Call { args, .. } => {
            for argument in args {
                visit_expr(argument, expr_rows, aggregate_rows);
            }
        }
        Expr::Index(left, right)
        | Expr::Binary { left, right, .. }
        | Expr::BinaryNamed { left, right, .. } => {
            visit_expr(left, expr_rows, aggregate_rows);
            visit_expr(right, expr_rows, aggregate_rows);
        }
        Expr::Method { receiver, args, .. } => {
            visit_expr(receiver, expr_rows, aggregate_rows);
            for argument in args {
                visit_expr(argument, expr_rows, aggregate_rows);
            }
        }
        Expr::Conditional {
            condition,
            then_expr,
            else_expr,
            ..
        } => {
            visit_expr(condition, expr_rows, aggregate_rows);
            visit_expr(then_expr, expr_rows, aggregate_rows);
            visit_expr(else_expr, expr_rows, aggregate_rows);
        }
        Expr::Quantified { binder, body, .. } => {
            visit_binder(binder, expr_rows, aggregate_rows);
            visit_expr(body, expr_rows, aggregate_rows);
        }
        Expr::Aggregate {
            kind,
            binder,
            value,
        } => {
            aggregate_rows.insert(aggregate_kind_row(kind));
            visit_binder(binder, expr_rows, aggregate_rows);
            if let Some(value) = value {
                visit_expr(value, expr_rows, aggregate_rows);
            }
        }
        Expr::TernaryNamed {
            first,
            second,
            third,
            ..
        } => {
            visit_expr(first, expr_rows, aggregate_rows);
            visit_expr(second, expr_rows, aggregate_rows);
            visit_expr(third, expr_rows, aggregate_rows);
        }
        Expr::Num(_) | Expr::Bool(_) | Expr::None | Expr::Var(_) | Expr::EnumMember { .. } => {}
    }
}

fn visit_type_ref(ty: &TypeRef, rows: &mut BTreeSet<&'static str>) {
    rows.insert(type_ref_row(ty));
    match ty {
        TypeRef::Map(key, value) | TypeRef::Relation(key, value) => {
            visit_type_ref(key, rows);
            visit_type_ref(value, rows);
        }
        TypeRef::Set(item) | TypeRef::Seq(item, _) | TypeRef::Option(item) => {
            visit_type_ref(item, rows);
        }
        TypeRef::Int | TypeRef::Bool | TypeRef::Named(_) | TypeRef::Range(_, _) => {}
    }
}

fn model_type_rows(model: &KernelModel) -> BTreeSet<&'static str> {
    let mut rows = BTreeSet::new();
    for (_, ty) in &model.state {
        visit_type_ref(ty, &mut rows);
    }
    for definition in model.types.values() {
        rows.insert(type_def_row(definition));
        if let TypeDef::Struct { fields } = definition {
            for (_, ty) in fields {
                visit_type_ref(ty, &mut rows);
            }
        }
    }
    rows
}

/// Slice-2 expression/type family: every evaluator-reachable `Expr` variant
/// is present in a checked model and passes Monitor BFS / explicit / BMC
/// verdict, replay, and successor agreement. Four aggregate models make the
/// `AggregateKind` inventory explicit. The same model schema contains all 9
/// `TypeRef` and all 3 `TypeDef` variants, so this is also the C3 `types`
/// axis's concrete/symbolic value-generation evidence.
#[test]
fn expression_variant_sweep_agrees_across_all_three_engines_and_covers_all_types() {
    let models = expression_sweep();
    assert!(
        models.len() >= EXPRESSION_SWEEP_FLOOR,
        "expression sweep floor: expected >= {EXPRESSION_SWEEP_FLOOR}, got {}",
        models.len()
    );

    let mut designated_expr_rows = BTreeSet::new();
    let mut designated_aggregate_rows = BTreeSet::new();
    let mut observed_type_rows = BTreeSet::new();
    let mut summary = SweepSummary::default();

    for generated in models {
        let model = engines::build_expression(&generated.id, &generated.source, generated.build);
        let property = model
            .invariants
            .iter()
            .find(|property| property.name == "Variant")
            .unwrap_or_else(|| panic!("'{}': Variant invariant disappeared", generated.id));
        let mut expr_rows = BTreeSet::new();
        let mut aggregate_rows = BTreeSet::new();
        visit_expr(&property.expr, &mut expr_rows, &mut aggregate_rows);
        assert!(
            expr_rows.contains(generated.expr_variant),
            "'{}': generated model does not contain designated row {}; observed={expr_rows:?}",
            generated.id,
            generated.expr_variant
        );
        if let Some(kind) = generated.aggregate_kind {
            assert!(
                aggregate_rows.contains(kind),
                "'{}': generated model does not contain designated row {kind}; observed={aggregate_rows:?}",
                generated.id
            );
            designated_aggregate_rows.insert(kind);
        }

        let verdict = engines::run_agreement(&generated.id, &model, generated.depth);
        assert_eq!(
            verdict,
            engines::Verdict::Clean,
            "'{}': the positive expression model must satisfy Variant",
            generated.id
        );
        let mut negative = model.clone();
        let property = negative
            .invariants
            .iter_mut()
            .find(|property| property.name == "Variant")
            .unwrap_or_else(|| panic!("'{}': Variant invariant disappeared", generated.id));
        let positive = std::mem::replace(&mut property.expr, Expr::Bool(false));
        property.expr = Expr::Not(Box::new(positive));
        let negative_id = format!("{}_negative_control", generated.id);
        let negative_verdict = engines::run_agreement(&negative_id, &negative, generated.depth);
        assert_eq!(
            negative_verdict,
            engines::Verdict::Violated {
                kind: "invariant".to_owned(),
                name: "Variant".to_owned(),
                step: 0,
            },
            "'{}': negating the known-true expression must be detected as an initial invariant violation by all three engines",
            generated.id
        );
        designated_expr_rows.insert(generated.expr_variant);
        let type_rows = model_type_rows(&model);
        observed_type_rows.extend(type_rows.iter().copied());
        summary.record_expression_model(
            generated.expr_variant,
            generated.aggregate_kind,
            type_rows,
        );
    }

    let expected_expr_rows = checked_expr_variant_rows().into_iter().collect();
    assert_eq!(
        designated_expr_rows, expected_expr_rows,
        "the expression family must designate exactly the source-coupled evaluator-reachable Expr rows"
    );
    let expected_aggregate_rows = aggregate_kind_rows().into_iter().collect();
    assert_eq!(
        designated_aggregate_rows, expected_aggregate_rows,
        "every source-coupled AggregateKind row must have a generated model"
    );
    let expected_type_rows = type_rows().into_iter().collect();
    assert_eq!(
        observed_type_rows, expected_type_rows,
        "the expression family must carry exactly the source-coupled TypeRef/TypeDef rows through concrete and symbolic evaluation"
    );
    eprintln!("expression/type sweep summary: {summary}");
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

/// A Public Kernel JSON expression with its `span`s removed, for structural
/// comparison.
fn without_spans(value: &serde_json::Value) -> serde_json::Value {
    match value {
        serde_json::Value::Object(fields) => fields
            .iter()
            .filter(|(key, _)| key.as_str() != "span")
            .map(|(key, item)| (key.clone(), without_spans(item)))
            .collect(),
        serde_json::Value::Array(items) => items.iter().map(without_spans).collect(),
        other => other.clone(),
    }
}

fn binary<'v>(
    value: &'v serde_json::Value,
    operator: &str,
) -> Option<(&'v serde_json::Value, &'v serde_json::Value)> {
    (value["kind"] == "binary" && value["operator"] == operator)
        .then(|| (&value["left"], &value["right"]))
}

fn conjuncts<'v>(value: &'v serde_json::Value, out: &mut Vec<&'v serde_json::Value>) {
    if let Some((left, right)) = binary(value, "and") {
        conjuncts(left, out);
        conjuncts(right, out);
    } else {
        out.push(value);
    }
}

/// Whether an `at` entry's failure condition is `P and (i < 0 or i >= n)` for a
/// literal `i >= 0` where `P` has the conjunct `i < n` -- the membership guard
/// of a `Seq` collection binder's synthesized `collection.at(i)` read. That
/// condition is unsatisfiable, so such an entry is not an authored site.
fn is_guarded_binder_read(failure: &serde_json::Value) -> bool {
    let failure = without_spans(failure);
    let Some((path, out_of_prefix)) = binary(&failure, "and") else {
        return false;
    };
    let Some((negative, beyond)) = binary(out_of_prefix, "or") else {
        return false;
    };
    let (Some((index, zero)), Some((index_again, size))) =
        (binary(negative, "<"), binary(beyond, ">="))
    else {
        return false;
    };
    let literal = |value: &serde_json::Value| {
        if value["kind"] == "num" {
            value["value"].as_i64()
        } else {
            None
        }
    };
    let (Some(position), Some(0)) = (literal(index), literal(zero)) else {
        return false;
    };
    if position < 0 || index_again != index {
        return false;
    }
    let mut guards = Vec::new();
    conjuncts(path, &mut guards);
    guards
        .iter()
        .any(|guard| binary(guard, "<").is_some_and(|(left, right)| left == index && right == size))
}

fn cli_json(args: &[&str]) -> serde_json::Value {
    let output = std::process::Command::new(env!("CARGO_BIN_EXE_fslc"))
        .args(args)
        .output()
        .expect("run native CLI");
    serde_json::from_slice(&output.stdout).unwrap_or_else(|error| {
        panic!(
            "{args:?}: invalid JSON: {error}; stderr={}",
            String::from_utf8_lossy(&output.stderr)
        )
    })
}

/// What each consumer of the partial-operation inventory says about action
/// `a`: explain's `_partial_a` entry count, the Public Kernel's operation
/// names, both CLI verify engines, and the Monitor's own step.
struct InventoryObservation {
    explain_sites: usize,
    /// Every Kernel `partial_operations` entry of action `a`: its operation
    /// name, and whether it is a provably guarded binder read.
    kernel_operations: Vec<(String, bool)>,
    bmc: (String, String),
    explicit: (String, String),
    monitor: Option<String>,
}

fn observe_inventory(id: &str, source: &str) -> InventoryObservation {
    let dir =
        std::env::temp_dir().join(format!("fslc-typed-agreement-1166-{}", std::process::id()));
    std::fs::create_dir_all(&dir).expect("create scratch directory");
    let path = dir.join(format!("{id}.fsl"));
    std::fs::write(&path, source).expect("write generated model");
    let file = path.to_str().expect("utf-8 path");

    let explain = cli_json(&["explain", file]);
    let explain_sites = explain["skeleton"]["auto_checks"]
        .as_array()
        .unwrap_or_else(|| panic!("'{id}': explain has no auto_checks: {explain}"))
        .iter()
        .filter(|check| check["kind"] == "partial_op" && check["name"] == "_partial_a")
        .count();
    let kernel = cli_json(&["kernel", file]);
    let kernel_operations = kernel["actions"]
        .as_array()
        .unwrap_or_else(|| panic!("'{id}': kernel has no actions: {kernel}"))
        .iter()
        .flat_map(|action| {
            action["partial_operations"]
                .as_array()
                .cloned()
                .unwrap_or_default()
        })
        .map(|entry| {
            let name = entry["operation"].as_str().unwrap_or_default().to_owned();
            let binder_read = name == "at" && is_guarded_binder_read(&entry["failure_condition"]);
            (name, binder_read)
        })
        .collect();
    let verdict = |engine: &str| {
        let result = cli_json(&["verify", file, "--depth", "2", "--engine", engine]);
        (
            result["result"].as_str().unwrap_or_default().to_owned(),
            format!(
                "{}/{}",
                result["violation_kind"].as_str().unwrap_or_default(),
                result["invariant"].as_str().unwrap_or_default()
            ),
        )
    };
    let bmc = verdict("bmc");
    let explicit = verdict("explicit");

    let model = engines::build(id, source);
    let mut monitor = fsl_runtime::Monitor::new(model)
        .unwrap_or_else(|error| panic!("'{id}': Monitor rejected the model: {error}"));
    let enabled = monitor
        .enabled()
        .unwrap_or_else(|error| panic!("'{id}': enabledness failed: {error}"));
    let [instance] = enabled.as_slice() else {
        panic!("'{id}': expected exactly one enabled instance, got {enabled:?}");
    };
    let step = monitor.step(instance).unwrap_or_else(|error| {
        panic!("'{id}': Monitor step raised instead of classifying: {error}")
    });
    let monitor = step.violation.map(|violation| violation.kind);
    let _ = std::fs::remove_file(&path);
    InventoryObservation {
        explain_sites,
        kernel_operations,
        bmc,
        explicit,
        monitor,
    }
}

/// Issue #1166: explain, the Public Kernel, the verifier's candidate check (BMC)
/// and the runtime (explicit engine and Monitor) agree on every kind in
/// `fsl_core::PartialOperation::ALL`, wherever it sits -- index reads,
/// quantifier and aggregate binders, assignment targets included.
#[test]
fn partial_inventory_sweep_agrees_across_explain_kernel_verifier_and_runtime() {
    let models = partial_inventory_sweep();
    assert_eq!(
        models.len(),
        fsl_core::PartialOperation::ALL.len() * PARTIAL_INVENTORY_PLACEMENTS.len()
    );
    let mut failures = Vec::new();
    for model in &models {
        let observed = observe_inventory(&model.id, &model.source);
        let violated = ("violated".to_owned(), "partial_op/_partial_a".to_owned());
        let placement = model.placement;
        let (expected_authored, expected_reads) =
            (placement.kernel_authored, placement.kernel_binder_reads);
        let authored = observed
            .kernel_operations
            .iter()
            .filter(|(_, binder_read)| !binder_read)
            .map(|(name, _)| name.as_str())
            .collect::<Vec<_>>();
        let binder_reads = observed
            .kernel_operations
            .iter()
            .filter(|(_, binder_read)| *binder_read)
            .count();
        let mut problems = Vec::new();
        if observed.explain_sites != 1 {
            problems.push(format!("explain lists {} sites", observed.explain_sites));
        }
        // Exact multiset: every authored Kernel entry is this model's one
        // operation, expanded `kernel_authored` times, and nothing else.
        if authored != vec![model.operation.name(); expected_authored]
            || binder_reads != expected_reads
        {
            problems.push(format!(
                "kernel partial_operations {:?}, expected {expected_authored} x '{}' and \
                 {expected_reads} guarded binder reads",
                observed.kernel_operations,
                model.operation.name()
            ));
        }
        if observed.bmc != violated {
            problems.push(format!("bmc {:?}", observed.bmc));
        }
        if observed.explicit != violated {
            problems.push(format!("explicit {:?}", observed.explicit));
        }
        if observed.monitor.as_deref() != Some("partial_op") {
            problems.push(format!("monitor {:?}", observed.monitor));
        }
        if !problems.is_empty() {
            failures.push(format!("{}: {}", model.id, problems.join("; ")));
        }
    }
    assert!(
        failures.is_empty(),
        "partial-operation inventory disagreement:\n{}",
        failures.join("\n")
    );
}

/// Negative control: a `Map` index read is total, so no consumer may treat it
/// as a partial operation.
#[test]
fn partial_inventory_map_index_is_not_partial_for_any_consumer() {
    let observed = observe_inventory(
        "partial_inventory_map_index",
        &partial_inventory_source("y = m[x]"),
    );
    assert_eq!(observed.explain_sites, 0);
    assert!(
        observed.kernel_operations.is_empty(),
        "{:?}",
        observed.kernel_operations
    );
    assert_eq!(observed.bmc.0, "verified");
    assert_eq!(observed.explicit.0, "proved");
    assert_eq!(observed.monitor, None);
}

/// A `Seq` collection binder with no authored partial operation (the case an
/// independent review raised): explain lists nothing, and the Public Kernel's
/// only entries are the binder's own synthesized reads, one per candidate
/// (capacity 2), each provably guarded by its membership.
#[test]
fn partial_inventory_seq_binder_reads_are_guarded_and_not_listed_by_explain() {
    let observed = observe_inventory(
        "partial_inventory_seq_binder_reads",
        &partial_inventory_source("y = if (exists v in s: m[v] == 0) then m[1] else 0"),
    );
    assert_eq!(observed.explain_sites, 0);
    assert_eq!(
        observed.kernel_operations,
        vec![("at".to_owned(), true), ("at".to_owned(), true)]
    );
    assert_eq!(observed.bmc.0, "verified");
    assert_eq!(observed.explicit.0, "proved");
    assert_eq!(observed.monitor, None);
}

/// Issue #1190: the issue's three statement-level `forall` bodies whose partial
/// operation reads the binder. The Public Kernel used to fail with "cannot
/// type identifier 'k'"; it now lists one closed entry per candidate (no free
/// `k`), explain lists the one authored site, and the verdicts are unchanged:
/// the unguarded `at` fails, the guarded index read and `1 / (k + 1)` do not.
#[test]
fn forall_statement_partial_operations_on_the_binder_are_listed_per_candidate() {
    let cases = [
        (
            "forall_statement_at_binder",
            "forall k: K { m[k] = s.at(k) }",
            "at",
            ("violated", "partial_op/_partial_a"),
            ("violated", "partial_op/_partial_a"),
            Some("partial_op"),
        ),
        (
            "forall_statement_guarded_index_binder",
            "forall k: K { m[k] = if k < s.size() then s[k] else 0 }",
            "index",
            ("verified", "/"),
            ("proved", "/"),
            None,
        ),
        (
            "forall_statement_divide_binder",
            "forall k: K { m[k] = 1 / (k + 1) }",
            "divide",
            ("verified", "/"),
            ("proved", "/"),
            None,
        ),
    ];
    for (id, body, operation, bmc, explicit, monitor) in cases {
        let source = partial_inventory_source(body);
        let observed = observe_inventory(id, &source);
        assert_eq!(observed.explain_sites, 1, "{id}");
        assert_eq!(
            observed.kernel_operations,
            vec![(operation.to_owned(), false); 3],
            "{id}"
        );
        assert_eq!(
            observed.bmc,
            (bmc.0.to_owned(), bmc.1.to_owned()),
            "{id}: bmc"
        );
        assert_eq!(
            observed.explicit,
            (explicit.0.to_owned(), explicit.1.to_owned()),
            "{id}: explicit"
        );
        assert_eq!(observed.monitor.as_deref(), monitor, "{id}: monitor");

        let dir =
            std::env::temp_dir().join(format!("fslc-typed-agreement-1190-{}", std::process::id()));
        std::fs::create_dir_all(&dir).expect("create scratch directory");
        let path = dir.join(format!("{id}.fsl"));
        std::fs::write(&path, &source).expect("write generated model");
        let kernel = cli_json(&["kernel", path.to_str().expect("utf-8 path")]);
        let _ = std::fs::remove_file(&path);
        let listed = kernel["actions"][0]["partial_operations"].to_string();
        assert!(
            !listed.contains("\"name\":\"k\""),
            "{id}: a failure condition still names the binder: {listed}"
        );
    }
}

/// Negative control for #1190: a statement-level `forall` whose body has no
/// partial operation still lists nothing, and is not a violation.
#[test]
fn forall_statement_without_partial_operation_lists_nothing() {
    let observed = observe_inventory(
        "forall_statement_total",
        &partial_inventory_source("forall k: K where k > 0 { m[k] = m[k] }"),
    );
    assert_eq!(observed.explain_sites, 0);
    assert!(
        observed.kernel_operations.is_empty(),
        "{:?}",
        observed.kernel_operations
    );
    assert_eq!(observed.bmc.0, "verified");
    assert_eq!(observed.explicit.0, "proved");
    assert_eq!(observed.monitor, None);
}

/// `fslc kernel` on `partial_inventory_source(body)`.
fn kernel_for_body(id: &str, body: &str) -> serde_json::Value {
    let dir =
        std::env::temp_dir().join(format!("fslc-typed-agreement-1190k-{}", std::process::id()));
    std::fs::create_dir_all(&dir).expect("create scratch directory");
    let path = dir.join(format!("{id}.fsl"));
    std::fs::write(&path, partial_inventory_source(body)).expect("write generated model");
    let kernel = cli_json(&["kernel", path.to_str().expect("utf-8 path")]);
    let _ = std::fs::remove_file(&path);
    kernel
}

/// The value of a closed Public Kernel expression built only from literals,
/// `ite` and binary operators -- what an expanded failure condition is once
/// every binder is replaced by its candidate and nothing reads state.
fn eval_closed(expr: &serde_json::Value) -> i64 {
    match expr["kind"].as_str() {
        Some("num") => expr["value"].as_i64().expect("num literal"),
        Some("bool") => i64::from(expr["value"].as_bool().expect("bool literal")),
        Some("ite") => {
            if eval_closed(&expr["condition"]) != 0 {
                eval_closed(&expr["then"])
            } else {
                eval_closed(&expr["else"])
            }
        }
        Some("binary") => {
            let (left, right) = (eval_closed(&expr["left"]), eval_closed(&expr["right"]));
            match expr["operator"].as_str().expect("binary operator") {
                "and" => i64::from(left != 0 && right != 0),
                "or" => i64::from(left != 0 || right != 0),
                "==" => i64::from(left == right),
                "!=" => i64::from(left != right),
                "<" => i64::from(left < right),
                "<=" => i64::from(left <= right),
                ">" => i64::from(left > right),
                ">=" => i64::from(left >= right),
                "+" => left + right,
                "-" => left - right,
                other => panic!("unexpected operator {other} in {expr}"),
            }
        }
        _ => panic!("not a closed literal expression: {expr}"),
    }
}

/// Review r1 M1 for #1190: the body of a statement-level `forall` with a
/// `where` is listed under each candidate's membership-and-`where` guard.
/// `2 / k` fails only for `k == 0`, which `where k > 0` excludes, so every one
/// of the three entries -- the `k == 0` one included -- must be false. An
/// unguarded entry would be `0 == 0`.
#[test]
fn forall_statement_where_guards_each_body_entry() {
    let body = "forall k: K where k > 0 { m[k] = 2 / k }";
    let observed = observe_inventory(
        "forall_statement_where_guard",
        &partial_inventory_source(body),
    );
    assert_eq!(observed.explain_sites, 1);
    assert_eq!(
        observed.kernel_operations,
        vec![("divide".to_owned(), false); 3]
    );
    assert_eq!(observed.bmc.0, "verified");
    assert_eq!(observed.explicit.0, "proved");
    assert_eq!(observed.monitor, None);

    let kernel = kernel_for_body("forall_statement_where_guard", body);
    let entries = kernel["actions"][0]["partial_operations"]
        .as_array()
        .unwrap_or_else(|| panic!("kernel has no partial_operations: {kernel}"));
    assert_eq!(entries.len(), 3);
    for entry in entries {
        let failure = &entry["failure_condition"];
        assert_eq!(
            eval_closed(failure),
            0,
            "an entry fires although `where k > 0` excludes it: {}",
            without_spans(failure)
        );
    }
}

/// The value of a Public Kernel failure condition in `partial_inventory_source`'s
/// initial state (`x == 0`, `y == 0`), for conditions over `x`, `y`, literals,
/// `not`, and the operators [`eval_closed`] knows.
fn eval_in_initial_state(expr: &serde_json::Value) -> i64 {
    match expr["kind"].as_str() {
        Some("var") => match expr["name"].as_str() {
            Some("x" | "y") => 0,
            other => panic!("unexpected variable {other:?} in {expr}"),
        },
        Some("not") => i64::from(eval_in_initial_state(&expr["operand"]) == 0),
        Some("ite" | "binary") => {
            let mut closed = expr.clone();
            for key in ["condition", "then", "else", "left", "right"] {
                if let Some(child) = closed.get_mut(key) {
                    *child = serde_json::json!({
                        "kind": "num",
                        "value": eval_in_initial_state(child),
                    });
                }
            }
            eval_closed(&closed)
        }
        _ => eval_closed(expr),
    }
}

/// Issue #1260: a division in a statement-level `if` branch is listed under
/// the branch's condition, so whether some Kernel failure condition holds in
/// the initial state agrees with whether the engines and the Monitor find the
/// `partial_op` there (`x` and `s` never change, so every reachable state
/// is the initial one for them). The Kernel listed an unconditional `x == 0`,
/// which holds although a branch that is not taken never divides.
#[test]
fn statement_if_failure_conditions_agree_with_the_engines() {
    for (id, body, fails) in [
        ("if_statement_not_taken", "if x != 0 { y = 2 / x }", false),
        (
            "if_statement_else_not_taken",
            "if x == 0 { y = 1 } else { y = 2 / x }",
            false,
        ),
        ("if_statement_taken", "if x == 0 { y = 2 / x }", true),
        (
            "if_statement_nested_not_taken",
            "if x == 0 { if x != 0 { y = 2 / x } }",
            false,
        ),
        (
            "if_statement_forall_not_taken",
            "if x != 0 { forall k: K { m[k] = 2 / x } }",
            false,
        ),
        (
            "forall_statement_if_not_taken",
            "forall k: K { if x > k { m[k] = 2 / x } }",
            false,
        ),
        (
            "forall_statement_if_taken",
            "forall k: K { if k > x { m[k] = 2 / x } }",
            true,
        ),
    ] {
        let observed = observe_inventory(id, &partial_inventory_source(body));
        let kernel = kernel_for_body(id, body);
        let entries = kernel["actions"][0]["partial_operations"]
            .as_array()
            .unwrap_or_else(|| panic!("{id}: kernel has no partial_operations: {kernel}"));
        assert!(!entries.is_empty(), "{id}: nothing listed");
        let kernel_fails = entries
            .iter()
            .any(|entry| eval_in_initial_state(&entry["failure_condition"]) != 0);
        let conditions = entries
            .iter()
            .map(|entry| without_spans(&entry["failure_condition"]))
            .collect::<Vec<_>>();
        assert_eq!(
            kernel_fails, fails,
            "{id}: Kernel failure conditions {conditions:?}"
        );
        let (bmc, explicit, monitor) = if fails {
            ("violated", "violated", Some("partial_op"))
        } else {
            ("verified", "proved", None)
        };
        assert_eq!(observed.bmc.0, bmc, "{id}: bmc {:?}", observed.bmc);
        assert_eq!(
            observed.explicit.0, explicit,
            "{id}: explicit {:?}",
            observed.explicit
        );
        assert_eq!(observed.monitor.as_deref(), monitor, "{id}: monitor");
    }
}

/// Review r1 M2 for #1190: a statement-level `forall` with no partial
/// operation is not expanded, so a range whose bound is not a constant still
/// produces a Kernel (listing nothing), as it did before #1190. A `Map` index
/// read is not a partial operation.
#[test]
fn forall_statement_without_partial_operation_needs_no_constant_range() {
    for (id, body) in [
        (
            "forall_statement_state_bound",
            "forall k in 0..x { m[k] = 1 }",
        ),
        (
            "forall_statement_state_bound_map_read",
            "forall k in 0..x { m[k] = m[k] }",
        ),
    ] {
        let kernel = kernel_for_body(id, body);
        assert_ne!(kernel["result"], "error", "{id}: {kernel}");
        assert_eq!(
            kernel["actions"][0]["partial_operations"],
            serde_json::json!([]),
            "{id}: {kernel}"
        );
    }
}

/// Review r2 m1 for #1190: the positive control of
/// `forall_statement_without_partial_operation_needs_no_constant_range`. With
/// a partial operation in the body, a range whose bound is not a constant fails
/// closed instead of listing nothing. Swallowing the candidate error would
/// silently produce a false empty list.
fn assert_kernel_fails_on_non_constant_range(id: &str, body: &str) {
    let kernel = kernel_for_body(id, body);
    assert_eq!(kernel["result"], "error", "{id}: {kernel}");
    assert!(
        kernel.to_string().contains("is not an integer const"),
        "{id}: {kernel}"
    );
}

#[test]
fn forall_statement_with_division_needs_a_constant_range() {
    assert_kernel_fails_on_non_constant_range(
        "forall_statement_state_bound_divide",
        "forall k in 0..x { m[k] = 2 / x }",
    );
}

/// A `Seq` index read counts as a partial operation, unlike a `Map` one.
#[test]
fn forall_statement_with_seq_read_needs_a_constant_range() {
    assert_kernel_fails_on_non_constant_range(
        "forall_statement_state_bound_seq_read",
        "forall k in 0..x { m[k] = s[k] }",
    );
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
