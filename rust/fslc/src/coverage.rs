// SPDX-License-Identifier: Apache-2.0
// Copyright 2026 Ryoichi Izumita

//! Conformance corpus feature coverage matrix (issue #223).
//!
//! Cross-references the two checked-in public-Kernel contract fixtures
//! (`kernel_contract.fsl`, `conformance_failures.fsl`) against the
//! conformance vectors they generate to answer, for every documented kernel
//! semantic, outcome kind, value encoding, and structural feature: is it
//! actually exercised by the corpus, or merely declared/possible? Every
//! detector inspects the generated JSON structurally; nothing here is
//! hardcoded to a fixed `true`.
//!
//! Evidence has two levels. `exercised` means a concrete vector or state in
//! the generated corpus demonstrates the feature. `declared` means the
//! contract states the feature exists (a type, a `partial_operations`
//! entry, `fair: true`, a `terminal` expression, ...) but the corpus cannot
//! produce a firing vector for it — reserved for features that are
//! structurally unobservable in a single bounded transition vector
//! (terminal-state deadlock exclusion, weak fairness) or where the design
//! only requires a declaration (partial operations; see
//! [`PARTIAL_OPERATION_KEYS`]).
//!
//! [`coverage_matrix`] itself enforces the coverage bar: if any feature row
//! falls short of its required level, it returns `Err` naming every
//! shortfall instead of producing a matrix that quietly under-reports
//! coverage.

use std::collections::{BTreeMap, BTreeSet};
use std::fmt::Write as _;
use std::path::{Path, PathBuf};

use fsl_core::{FsResolver, KernelModel, TypeDef, TypeRef, build_model, parse_kernel_source};
use serde_json::{Value, json};

pub const COVERAGE_SCHEMA_VERSION: &str = "1.0.0";
pub const COVERAGE_SCHEMA_ID: &str =
    "https://fsl.dev/schemas/fslc/kernel/conformance-coverage.v1.schema.json";

/// Fixed fixture manifest: `(file name under tests/fixtures, BFS depth)`.
///
/// Depths are the smallest values at which every semantics/outcome/value
/// feature the fixture is meant to carry actually fires at least once.
const FIXTURE_MANIFEST: &[(&str, usize)] =
    &[("kernel_contract.fsl", 2), ("conformance_failures.fsl", 1)];

/// Kernel schema `semantics` key -> coverage-matrix feature key.
///
/// Kept in sync with `schemas/fslc/kernel/kernel.v1.schema.json`'s
/// `semantics.required` list by `tests/conformance_coverage.rs`.
pub const SEMANTICS_FEATURE_KEYS: &[(&str, &str)] = &[
    ("assignment", "assignment_simultaneous"),
    ("reads", "reads_pre_state"),
    ("requires_false", "requires_false_not_enabled"),
    ("failure_state", "failure_rollback"),
    ("old", "old_pre_state"),
    ("integer_division", "integer_division_euclidean"),
    ("terminal_deadlock", "terminal_deadlock"),
    ("fairness", "fairness_weak"),
];

/// Monitor violation `outcome.kind` -> coverage-matrix feature key.
///
/// Kept in sync with every kind `fsl_runtime::Monitor` can emit by
/// `tests/conformance_coverage.rs`; an unrecognized kind fails
/// [`coverage_matrix`] loudly instead of being silently ignored.
pub const OUTCOME_FEATURE_KEYS: &[(&str, &str)] = &[
    ("ok", "outcome_ok"),
    ("requires_failed", "outcome_requires_failed"),
    ("partial_op", "outcome_partial_op"),
    ("type_bound", "outcome_type_bound"),
    ("invariant", "outcome_invariant"),
    ("trans", "outcome_trans"),
    ("ensures", "outcome_ensures"),
];

/// Partial operation name -> coverage-matrix feature key. Declared-only
/// evidence (a `partial_operations` entry in the contract) satisfies these
/// rows; a firing `partial_op` vector attributable to the operation is
/// recorded as bonus exercised evidence.
const PARTIAL_OPERATION_KEYS: &[(&str, &str)] = &[
    ("head", "partial_op_head"),
    ("pop", "partial_op_pop"),
    ("at", "partial_op_at"),
    ("index", "partial_op_index"),
    ("divide", "partial_op_divide"),
    ("remainder", "partial_op_remainder"),
];

/// Evidence strength for one coverage-matrix feature row.
#[derive(Clone, Copy, Debug, Eq, Ord, PartialEq, PartialOrd)]
enum Level {
    Missing,
    Declared,
    Exercised,
}

impl Level {
    const fn as_str(self) -> &'static str {
        match self {
            Self::Missing => "missing",
            Self::Declared => "declared",
            Self::Exercised => "exercised",
        }
    }
}

#[derive(Clone, Debug)]
struct Evidence {
    fixture: &'static str,
    detail: String,
    count: usize,
}

impl Evidence {
    fn to_json(&self) -> Value {
        json!({"fixture": self.fixture, "detail": self.detail, "count": self.count})
    }
}

struct FeatureRow {
    key: &'static str,
    description: String,
    required: Level,
    level: Level,
    evidence: Vec<Evidence>,
}

impl FeatureRow {
    fn to_json(&self) -> Value {
        json!({
            "key": self.key,
            "description": self.description,
            "level": self.level.as_str(),
            "evidence": self.evidence.iter().map(Evidence::to_json).collect::<Vec<_>>(),
        })
    }
}

struct FixtureRun {
    file: &'static str,
    depth: usize,
    model: KernelModel,
    contract: Value,
    conformance: Value,
}

impl FixtureRun {
    fn vectors(&self) -> &[Value] {
        self.conformance["vectors"]
            .as_array()
            .map_or(&[], Vec::as_slice)
    }

    fn states(&self) -> &[Value] {
        self.conformance["states"]
            .as_array()
            .map_or(&[], Vec::as_slice)
    }

    fn actions(&self) -> &[Value] {
        self.contract["actions"]
            .as_array()
            .map_or(&[], Vec::as_slice)
    }
}

fn fixtures_dir() -> PathBuf {
    Path::new(env!("CARGO_MANIFEST_DIR")).join("tests/fixtures")
}

fn load_fixtures() -> Result<Vec<FixtureRun>, String> {
    FIXTURE_MANIFEST
        .iter()
        .map(|&(file, depth)| {
            let path = fixtures_dir().join(file);
            let source =
                std::fs::read_to_string(&path).map_err(|error| format!("{file}: {error}"))?;
            let base = path
                .parent()
                .ok_or_else(|| format!("{file}: fixture has no parent directory"))?;
            let resolver = FsResolver::new(base);
            let kernel = parse_kernel_source(&source, &resolver)
                .map_err(|error| format!("{file}: {error}"))?;
            let model = build_model(kernel.clone()).map_err(|error| format!("{file}: {error}"))?;
            let contract = fsl_core::public_kernel_contract(&kernel, &model, file, "kernel")
                .map_err(|error| format!("{file}: {error}"))?;
            let conformance = crate::conformance_vectors(&model, depth)
                .map_err(|error| format!("{file}: {error}"))?;
            Ok(FixtureRun {
                file,
                depth,
                model,
                contract,
                conformance,
            })
        })
        .collect()
}

/// Collect every genuine *expression* node beneath `value`: an object
/// carrying `kind`, `type`, and `span` together, where `type.kind` is not
/// the statement marker. This distinguishes real expressions (which can be
/// `forall`/`exists`/`old`/...) from statement nodes that happen to share
/// the same three key names (a statement `forall` binder), and from plain
/// type descriptors (which never carry a sibling `span`).
fn expression_nodes<'a>(value: &'a Value, sink: &mut Vec<&'a Value>) {
    match value {
        Value::Object(map) => {
            if map.contains_key("kind") && map.contains_key("span") {
                let is_statement = map
                    .get("type")
                    .and_then(|ty| ty.get("kind"))
                    .and_then(Value::as_str)
                    == Some("statement");
                if !is_statement {
                    sink.push(value);
                }
            }
            for nested in map.values() {
                expression_nodes(nested, sink);
            }
        }
        Value::Array(items) => {
            for nested in items {
                expression_nodes(nested, sink);
            }
        }
        _ => {}
    }
}

/// Collect every `"name"` string reachable beneath `value` (variable
/// references, indexed/field lvalue base names, ...). Used to detect
/// self-referential updates (`total = total + 1`), which prove an update's
/// right-hand side observes the pre-state value of a variable it also
/// writes.
fn collect_names(value: &Value, sink: &mut BTreeSet<String>) {
    match value {
        Value::Object(map) => {
            if let Some(Value::String(name)) = map.get("name") {
                sink.insert(name.clone());
            }
            for nested in map.values() {
                collect_names(nested, sink);
            }
        }
        Value::Array(items) => {
            for nested in items {
                collect_names(nested, sink);
            }
        }
        _ => {}
    }
}

/// Evaluate a literal integer expression (`num`, `neg`, and `+`/`-`/`*`
/// `binary` combinations of literals). Returns `None` for anything that
/// depends on a variable, parameter, or method call.
fn literal_int(expr: &Value) -> Option<i64> {
    let map = expr.as_object()?;
    match map.get("kind")?.as_str()? {
        "num" => map.get("value")?.as_i64(),
        "neg" => Some(-literal_int(map.get("operand")?)?),
        "binary" => {
            let operator = map.get("operator")?.as_str()?;
            let left = literal_int(map.get("left")?)?;
            let right = literal_int(map.get("right")?)?;
            match operator {
                "+" => Some(left + right),
                "-" => Some(left - right),
                "*" => Some(left * right),
                _ => None,
            }
        }
        _ => None,
    }
}

/// Whether `target` appears anywhere (as a number) inside `value`.
fn json_contains_int(value: &Value, target: i64) -> bool {
    match value {
        Value::Number(number) => number.as_i64() == Some(target),
        Value::Object(map) => map.values().any(|nested| json_contains_int(nested, target)),
        Value::Array(items) => items.iter().any(|nested| json_contains_int(nested, target)),
        _ => false,
    }
}

fn fired(fixture: &FixtureRun, action_name: &str, kind: &str) -> bool {
    fixture
        .vectors()
        .iter()
        .any(|vector| vector["action"]["name"] == action_name && vector["outcome"]["kind"] == kind)
}

fn count_vectors_matching(
    fixtures: &[FixtureRun],
    predicate: impl Fn(&Value) -> bool,
    detail: &str,
) -> (Level, Vec<Evidence>) {
    let mut evidence = Vec::new();
    for fixture in fixtures {
        let count = fixture
            .vectors()
            .iter()
            .filter(|vector| predicate(vector))
            .count();
        if count > 0 {
            evidence.push(Evidence {
                fixture: fixture.file,
                detail: detail.to_owned(),
                count,
            });
        }
    }
    let level = if evidence.is_empty() {
        Level::Missing
    } else {
        Level::Exercised
    };
    (level, evidence)
}

// --- Kernel semantics (8 keys from kernel.v1.schema.json `semantics`) ------

fn detect_assignment_simultaneous(fixtures: &[FixtureRun]) -> (Level, Vec<Evidence>) {
    count_vectors_matching(
        fixtures,
        |vector| {
            vector["outcome"]["kind"] == "ok"
                && vector["outcome"]["changes"]
                    .as_object()
                    .is_some_and(|changes| {
                        changes
                            .keys()
                            .map(|key| key.split('[').next().unwrap_or(key.as_str()))
                            .collect::<BTreeSet<_>>()
                            .len()
                            >= 2
                    })
        },
        "ok vectors whose `changes` touch >=2 state fields in one atomic step",
    )
}

fn detect_reads_pre_state(fixtures: &[FixtureRun]) -> (Level, Vec<Evidence>) {
    let mut evidence = Vec::new();
    for fixture in fixtures {
        for action in fixture.actions() {
            let Some(name) = action["name"].as_str() else {
                continue;
            };
            let Some(updates) = action["updates"].as_array() else {
                continue;
            };
            let self_referential = updates.iter().any(|statement| {
                let mut written = BTreeSet::new();
                collect_names(&statement["target"], &mut written);
                let mut read = BTreeSet::new();
                collect_names(&statement["value"], &mut read);
                written.intersection(&read).next().is_some()
            });
            if self_referential && fired(fixture, name, "ok") {
                evidence.push(Evidence {
                    fixture: fixture.file,
                    detail: format!(
                        "action `{name}` updates a field using its own pre-update value and fired an ok vector"
                    ),
                    count: 1,
                });
            }
        }
    }
    let level = if evidence.is_empty() {
        Level::Missing
    } else {
        Level::Exercised
    };
    (level, evidence)
}

fn detect_requires_false_not_enabled(fixtures: &[FixtureRun]) -> (Level, Vec<Evidence>) {
    count_vectors_matching(
        fixtures,
        |vector| vector["outcome"]["kind"] == "requires_failed",
        "vectors with outcome.kind == \"requires_failed\"",
    )
}

fn detect_failure_rollback(fixtures: &[FixtureRun]) -> (Level, Vec<Evidence>) {
    count_vectors_matching(
        fixtures,
        |vector| {
            vector["outcome"]["kind"] != "ok"
                && vector["outcome"]["state_changed"].as_bool() == Some(false)
        },
        "violation vectors that roll back to the unchanged committed state",
    )
}

fn detect_old_pre_state(fixtures: &[FixtureRun]) -> (Level, Vec<Evidence>) {
    let mut evidence = Vec::new();
    let mut declared = false;
    for fixture in fixtures {
        let mut nodes = Vec::new();
        expression_nodes(&fixture.contract, &mut nodes);
        let old_count = nodes.iter().filter(|node| node["kind"] == "old").count();
        if old_count == 0 {
            continue;
        }
        declared = true;
        let fired_count = fixture
            .vectors()
            .iter()
            .filter(|vector| {
                !matches!(
                    vector["outcome"]["kind"].as_str(),
                    Some("requires_failed" | "partial_op")
                )
            })
            .count();
        if fired_count > 0 {
            evidence.push(Evidence {
                fixture: fixture.file,
                detail: format!(
                    "{old_count} `old(...)` expression(s) reached by a checked transition or the violation it causes"
                ),
                count: fired_count,
            });
        }
    }
    let level = if !evidence.is_empty() {
        Level::Exercised
    } else if declared {
        Level::Declared
    } else {
        Level::Missing
    };
    (level, evidence)
}

fn detect_integer_division_euclidean(fixtures: &[FixtureRun]) -> (Level, Vec<Evidence>) {
    let mut evidence = Vec::new();
    let mut declared = false;
    for fixture in fixtures {
        let mut nodes = Vec::new();
        expression_nodes(&fixture.contract, &mut nodes);
        let divisions = nodes
            .iter()
            .filter(|node| node["kind"] == "binary" && node["operator"] == "/")
            .filter_map(|node| Some((literal_int(&node["left"])?, literal_int(&node["right"])?)))
            .collect::<BTreeSet<_>>();
        if divisions.is_empty() {
            continue;
        }
        declared = true;
        for (left, right) in divisions {
            if right == 0 {
                continue;
            }
            let euclidean = left.div_euclid(right);
            let truncating = left / right;
            if euclidean == truncating {
                continue;
            }
            let hits = fixture
                .vectors()
                .iter()
                .filter(|vector| {
                    vector["outcome"]["kind"] == "ok"
                        && json_contains_int(&vector["outcome"]["state"], euclidean)
                })
                .count();
            if hits > 0 {
                evidence.push(Evidence {
                    fixture: fixture.file,
                    detail: format!(
                        "{left} / {right} == {euclidean} (Euclidean; truncating division would give {truncating})"
                    ),
                    count: hits,
                });
            }
        }
    }
    let level = if !evidence.is_empty() {
        Level::Exercised
    } else if declared {
        Level::Declared
    } else {
        Level::Missing
    };
    (level, evidence)
}

fn detect_terminal_deadlock(fixtures: &[FixtureRun]) -> (Level, Vec<Evidence>) {
    let mut evidence = Vec::new();
    for fixture in fixtures {
        if fixture.contract["properties"]["terminal"].is_null() {
            continue;
        }
        evidence.push(Evidence {
            fixture: fixture.file,
            detail: "contract declares a `terminal` expression excluding matching states from deadlock reporting".to_owned(),
            count: 1,
        });
    }
    let level = if evidence.is_empty() {
        Level::Missing
    } else {
        Level::Declared
    };
    (level, evidence)
}

fn detect_fairness_weak(fixtures: &[FixtureRun]) -> (Level, Vec<Evidence>) {
    let mut evidence = Vec::new();
    for fixture in fixtures {
        let fair_actions = fixture
            .actions()
            .iter()
            .filter(|action| action["fair"] == true)
            .count();
        if fair_actions > 0 {
            evidence.push(Evidence {
                fixture: fixture.file,
                detail: format!("{fair_actions} action(s) declared `fair`"),
                count: fair_actions,
            });
        }
    }
    let level = if evidence.is_empty() {
        Level::Missing
    } else {
        Level::Declared
    };
    (level, evidence)
}

// --- Outcome kinds (7 kinds `fsl_runtime::Monitor` can emit) ---------------

fn outcome_kind_evidence(fixtures: &[FixtureRun]) -> BTreeMap<String, Vec<Evidence>> {
    let mut per_kind: BTreeMap<String, BTreeMap<&'static str, usize>> = BTreeMap::new();
    for fixture in fixtures {
        for vector in fixture.vectors() {
            if let Some(kind) = vector["outcome"]["kind"].as_str() {
                *per_kind
                    .entry(kind.to_owned())
                    .or_default()
                    .entry(fixture.file)
                    .or_insert(0) += 1;
            }
        }
    }
    per_kind
        .into_iter()
        .map(|(kind, counts)| {
            let evidence = counts
                .into_iter()
                .map(|(fixture, count)| Evidence {
                    fixture,
                    detail: format!("vectors with outcome.kind == \"{kind}\""),
                    count,
                })
                .collect();
            (kind, evidence)
        })
        .collect()
}

// --- Value semantics (8 rows) ----------------------------------------------

#[allow(clippy::too_many_lines)]
fn walk_value_kinds(
    ty: &TypeRef,
    value: &Value,
    model: &KernelModel,
    tags: &mut BTreeSet<&'static str>,
    nested_option: &mut bool,
) {
    match ty {
        TypeRef::Bool => {
            tags.insert("value_bool");
        }
        TypeRef::Range(_, _) => {
            tags.insert("value_int_range");
        }
        TypeRef::Named(name) => match model.types.get(name) {
            Some(TypeDef::Domain { .. }) => {
                tags.insert("value_int_range");
            }
            Some(TypeDef::Enum { .. }) => {
                tags.insert("value_enum");
            }
            Some(TypeDef::Struct { fields }) => {
                tags.insert("value_struct");
                if let Some(fields_json) = value.as_object() {
                    for (field_name, field_ty) in fields {
                        if let Some(field_value) = fields_json.get(field_name) {
                            walk_value_kinds(field_ty, field_value, model, tags, nested_option);
                        }
                    }
                }
            }
            None => {}
        },
        TypeRef::Option(inner) => {
            tags.insert("value_option");
            if value.get("kind").and_then(Value::as_str) == Some("some") {
                if let Some(inner_value) = value.get("value") {
                    if matches!(inner.as_ref(), TypeRef::Option(_))
                        && inner_value.get("kind").and_then(Value::as_str) == Some("none")
                    {
                        *nested_option = true;
                    }
                    walk_value_kinds(inner, inner_value, model, tags, nested_option);
                }
            }
        }
        TypeRef::Map(_, item) => {
            tags.insert("value_map");
            if let Some(entries) = value.as_object() {
                for entry in entries.values() {
                    walk_value_kinds(item, entry, model, tags, nested_option);
                }
            }
        }
        TypeRef::Set(item) => {
            tags.insert("value_set");
            if let Some(entries) = value.as_array() {
                for entry in entries {
                    walk_value_kinds(item, entry, model, tags, nested_option);
                }
            }
        }
        TypeRef::Seq(item, _) => {
            tags.insert("value_seq");
            if let Some(entries) = value.as_array() {
                for entry in entries {
                    walk_value_kinds(item, entry, model, tags, nested_option);
                }
            }
        }
        TypeRef::Int | TypeRef::Relation(_, _) => {}
    }
}

fn value_kind_evidence(fixtures: &[FixtureRun]) -> BTreeMap<&'static str, Vec<Evidence>> {
    let mut per_key: BTreeMap<&'static str, BTreeMap<&'static str, usize>> = BTreeMap::new();
    let mut nested_hits: BTreeMap<&'static str, usize> = BTreeMap::new();
    for fixture in fixtures {
        for state_entry in fixture.states() {
            let Some(state_obj) = state_entry["state"].as_object() else {
                continue;
            };
            for (name, ty) in &fixture.model.state {
                let key = crate::display_name(name);
                let Some(value) = state_obj.get(&key) else {
                    continue;
                };
                let mut tags = BTreeSet::new();
                let mut nested = false;
                walk_value_kinds(ty, value, &fixture.model, &mut tags, &mut nested);
                for tag in tags {
                    *per_key
                        .entry(tag)
                        .or_default()
                        .entry(fixture.file)
                        .or_insert(0) += 1;
                }
                if nested {
                    *nested_hits.entry(fixture.file).or_insert(0) += 1;
                }
            }
        }
    }
    let mut evidence: BTreeMap<&'static str, Vec<Evidence>> = per_key
        .into_iter()
        .map(|(tag, counts)| {
            let entries = counts
                .into_iter()
                .map(|(fixture, count)| Evidence {
                    fixture,
                    detail: format!("state snapshots encoding a `{tag}` value"),
                    count,
                })
                .collect();
            (tag, entries)
        })
        .collect();
    for (fixture, count) in nested_hits {
        evidence.entry("value_option").or_default().push(Evidence {
            fixture,
            detail: "nested Option<Option<_>> value tagged some(none)".to_owned(),
            count,
        });
    }
    evidence
}

// --- Other structural features ---------------------------------------------

fn detect_partial_operation(fixtures: &[FixtureRun], operation: &str) -> (Level, Vec<Evidence>) {
    let mut evidence = Vec::new();
    let mut declared = false;
    for fixture in fixtures {
        for action in fixture.actions() {
            let Some(operations) = action["partial_operations"].as_array() else {
                continue;
            };
            let declared_ops = operations
                .iter()
                .filter_map(|entry| entry["operation"].as_str())
                .collect::<Vec<_>>();
            if !declared_ops.contains(&operation) {
                continue;
            }
            declared = true;
            let Some(name) = action["name"].as_str() else {
                continue;
            };
            let hits = fixture
                .vectors()
                .iter()
                .filter(|vector| {
                    vector["action"]["name"] == name && vector["outcome"]["kind"] == "partial_op"
                })
                .count();
            if hits > 0 {
                let detail = if declared_ops.len() == 1 {
                    format!(
                        "action `{name}` fired outcome.kind == \"partial_op\" (declares only `{operation}`)"
                    )
                } else {
                    format!(
                        "action `{name}` fired outcome.kind == \"partial_op\" (declares {} partial operations; attribution to `{operation}` is not unambiguous)",
                        declared_ops.len()
                    )
                };
                evidence.push(Evidence {
                    fixture: fixture.file,
                    detail,
                    count: hits,
                });
            }
        }
    }
    let level = if !evidence.is_empty() {
        Level::Exercised
    } else if declared {
        Level::Declared
    } else {
        Level::Missing
    };
    (level, evidence)
}

fn detect_quantification(fixtures: &[FixtureRun]) -> (Level, Vec<Evidence>) {
    let mut evidence = Vec::new();
    let mut declared = false;
    for fixture in fixtures {
        let mut nodes = Vec::new();
        expression_nodes(&fixture.contract, &mut nodes);
        let quantifiers = nodes
            .iter()
            .filter(|node| matches!(node["kind"].as_str(), Some("forall" | "exists")))
            .count();
        if quantifiers == 0 {
            continue;
        }
        declared = true;
        let ok_count = fixture
            .vectors()
            .iter()
            .filter(|vector| vector["outcome"]["kind"] == "ok")
            .count();
        if ok_count > 0 {
            evidence.push(Evidence {
                fixture: fixture.file,
                detail: format!("{quantifiers} forall/exists expression(s) evaluated across {ok_count} ok vector(s)"),
                count: ok_count,
            });
        }
    }
    let level = if !evidence.is_empty() {
        Level::Exercised
    } else if declared {
        Level::Declared
    } else {
        Level::Missing
    };
    (level, evidence)
}

fn detect_param_finite_domains(fixtures: &[FixtureRun]) -> (Level, Vec<Evidence>) {
    let mut evidence = Vec::new();
    let mut declared = false;
    for fixture in fixtures {
        for action in fixture.actions() {
            let Some(name) = action["name"].as_str() else {
                continue;
            };
            let Some(parameters) = action["parameters"].as_array() else {
                continue;
            };
            let multi_valued = parameters.iter().any(|parameter| {
                let lo = parameter["finite_domain"]["lo"].as_i64();
                let hi = parameter["finite_domain"]["hi"].as_i64();
                matches!((lo, hi), (Some(lo), Some(hi)) if hi > lo)
            });
            if !multi_valued {
                continue;
            }
            declared = true;
            let distinct_bindings = fixture
                .vectors()
                .iter()
                .filter(|vector| vector["action"]["name"] == name)
                .map(|vector| vector["action"]["params"].to_string())
                .collect::<BTreeSet<_>>()
                .len();
            if distinct_bindings >= 2 {
                evidence.push(Evidence {
                    fixture: fixture.file,
                    detail: format!(
                        "action `{name}` fired {distinct_bindings} distinct typed/range parameter bindings"
                    ),
                    count: distinct_bindings,
                });
            }
        }
    }
    let level = if !evidence.is_empty() {
        Level::Exercised
    } else if declared {
        Level::Declared
    } else {
        Level::Missing
    };
    (level, evidence)
}

fn detect_requirement_traceability(fixtures: &[FixtureRun]) -> (Level, Vec<Evidence>) {
    let mut evidence = Vec::new();
    let mut declared = false;
    for fixture in fixtures {
        for action in fixture.actions() {
            let Some(name) = action["name"].as_str() else {
                continue;
            };
            let Some(id) = action["requirement"]["id"].as_str() else {
                continue;
            };
            declared = true;
            let hits = fixture
                .vectors()
                .iter()
                .filter(|vector| vector["action"]["name"] == name)
                .count();
            if hits > 0 {
                evidence.push(Evidence {
                    fixture: fixture.file,
                    detail: format!(
                        "action `{name}` retains requirement id `{id}` and fired {hits} vector(s)"
                    ),
                    count: hits,
                });
            }
        }
    }
    let level = if !evidence.is_empty() {
        Level::Exercised
    } else if declared {
        Level::Declared
    } else {
        Level::Missing
    };
    (level, evidence)
}

#[allow(clippy::too_many_lines)]
fn build_rows(fixtures: &[FixtureRun]) -> Result<Vec<FeatureRow>, String> {
    let mut rows = Vec::new();

    let (level, evidence) = detect_assignment_simultaneous(fixtures);
    rows.push(FeatureRow {
        key: "assignment_simultaneous",
        description: "An action commits >=2 state-field changes atomically in one fired transition (kernel semantics: assignment=simultaneous).".to_owned(),
        required: Level::Exercised,
        level,
        evidence,
    });
    let (level, evidence) = detect_reads_pre_state(fixtures);
    rows.push(FeatureRow {
        key: "reads_pre_state",
        description: "An action's update right-hand side self-references a variable it also writes, proving updates read the pre-state (kernel semantics: reads=pre_state).".to_owned(),
        required: Level::Exercised,
        level,
        evidence,
    });
    let (level, evidence) = detect_requires_false_not_enabled(fixtures);
    rows.push(FeatureRow {
        key: "requires_false_not_enabled",
        description: "A false `requires` clause reports the action instance as not enabled instead of raising an error (kernel semantics: requires_false=not_enabled).".to_owned(),
        required: Level::Exercised,
        level,
        evidence,
    });
    let (level, evidence) = detect_failure_rollback(fixtures);
    rows.push(FeatureRow {
        key: "failure_rollback",
        description: "A violated action leaves the committed state unchanged, retaining only `attempted_state` for diagnostics (kernel semantics: failure_state=rollback).".to_owned(),
        required: Level::Exercised,
        level,
        evidence,
    });
    let (level, evidence) = detect_old_pre_state(fixtures);
    rows.push(FeatureRow {
        key: "old_pre_state",
        description: "An `old(...)` expression is declared and reached by a real fired transition or the violation it causes (kernel semantics: old=pre_state).".to_owned(),
        required: Level::Exercised,
        level,
        evidence,
    });
    let (level, evidence) = detect_integer_division_euclidean(fixtures);
    rows.push(FeatureRow {
        key: "integer_division_euclidean",
        description: "A literal integer division with a negative operand fires a successful transition whose recorded value matches Euclidean division, not truncating division (kernel semantics: integer_division=euclidean).".to_owned(),
        required: Level::Exercised,
        level,
        evidence,
    });
    let (level, evidence) = detect_terminal_deadlock(fixtures);
    rows.push(FeatureRow {
        key: "terminal_deadlock",
        description: "The contract declares a `terminal` expression excluding matching states from deadlock reporting (kernel semantics: terminal_deadlock=terminal_states_excluded). Reaching a state with zero enabled actions at the declared terminal condition needs a BFS depth beyond the coverage corpus's chosen fixture depths, so only declared (not fired) evidence is required.".to_owned(),
        required: Level::Declared,
        level,
        evidence,
    });
    let (level, evidence) = detect_fairness_weak(fixtures);
    rows.push(FeatureRow {
        key: "fairness_weak",
        description: "The contract marks at least one action `fair` (kernel semantics: fairness=weak). Weak fairness is a liveness property over infinite/untaken traces with no finite-vector witness, so only declared evidence is required.".to_owned(),
        required: Level::Declared,
        level,
        evidence,
    });

    let mut outcomes = outcome_kind_evidence(fixtures);
    for &(kind, key) in OUTCOME_FEATURE_KEYS {
        let evidence = outcomes.remove(kind).unwrap_or_default();
        let level = if evidence.is_empty() {
            Level::Missing
        } else {
            Level::Exercised
        };
        rows.push(FeatureRow {
            key,
            description: format!("A conformance vector with outcome.kind == \"{kind}\" is generated by the fixture corpus."),
            required: Level::Exercised,
            level,
            evidence,
        });
    }
    if !outcomes.is_empty() {
        let unknown = outcomes.keys().cloned().collect::<Vec<_>>().join(", ");
        return Err(format!(
            "conformance corpus emits outcome kind(s) not registered in the coverage matrix: {unknown} \
             (add a matching outcome_<kind> row to OUTCOME_FEATURE_KEYS/build_rows)"
        ));
    }

    let mut values = value_kind_evidence(fixtures);
    let value_rows: &[(&str, &str)] = &[
        (
            "value_int_range",
            "A domain/range-typed state field's concrete value appears in a generated state snapshot.",
        ),
        (
            "value_bool",
            "A `Bool`-typed state field's concrete value appears in a generated state snapshot.",
        ),
        (
            "value_enum",
            "An enum-typed value appears in a generated state snapshot.",
        ),
        (
            "value_struct",
            "A struct-typed value appears in a generated state snapshot.",
        ),
        (
            "value_option",
            "An `Option` value appears tagged `{\"kind\":\"none\"}`/`{\"kind\":\"some\",\"value\":...}` in a generated state snapshot. A nested `Option<Option<_>>` tagged `some(none)` is additional, non-required evidence (see also the dedicated `conformance_distinguishes_nested_options_and_guard_partials` unit test, which is not part of the fixed fixture manifest this matrix scans).",
        ),
        (
            "value_map",
            "A `Map` value appears in a generated state snapshot.",
        ),
        (
            "value_set",
            "A `Set` value appears in a generated state snapshot.",
        ),
        (
            "value_seq",
            "A `Seq` value appears in a generated state snapshot.",
        ),
    ];
    for &(key, description) in value_rows {
        let evidence = values.remove(key).unwrap_or_default();
        let level = if evidence.is_empty() {
            Level::Missing
        } else {
            Level::Exercised
        };
        rows.push(FeatureRow {
            key,
            description: description.to_owned(),
            required: Level::Exercised,
            level,
            evidence,
        });
    }

    for &(operation, key) in PARTIAL_OPERATION_KEYS {
        let (level, evidence) = detect_partial_operation(fixtures, operation);
        rows.push(FeatureRow {
            key,
            description: format!(
                "The contract declares a `partial_operations` entry for `{operation}` (a firing outcome.kind == \"partial_op\" vector unambiguously attributable to it is additional, non-required evidence)."
            ),
            required: Level::Declared,
            level,
            evidence,
        });
    }

    let (level, evidence) = detect_quantification(fixtures);
    rows.push(FeatureRow {
        key: "quantification",
        description: "A `forall`/`exists` quantified expression is declared and evaluated across a real fired transition.".to_owned(),
        required: Level::Exercised,
        level,
        evidence,
    });
    let (level, evidence) = detect_param_finite_domains(fixtures);
    rows.push(FeatureRow {
        key: "param_finite_domains",
        description: "A typed/range action parameter's finite domain is declared and enumerated across >=2 distinct fired parameter bindings.".to_owned(),
        required: Level::Exercised,
        level,
        evidence,
    });
    let (level, evidence) = detect_requirement_traceability(fixtures);
    rows.push(FeatureRow {
        key: "requirement_traceability",
        description: "An action retains its requirement ID in the contract and fires at least one conformance vector.".to_owned(),
        required: Level::Exercised,
        level,
        evidence,
    });

    Ok(rows)
}

/// Build the feature coverage matrix for the fixed fixture manifest.
///
/// # Errors
///
/// Returns `Err` describing the problem when a fixture fails to parse or
/// build, when the corpus emits an outcome kind unregistered in
/// [`OUTCOME_FEATURE_KEYS`], or when any feature row falls short of its
/// required evidence level (the actual coverage gap this matrix exists to
/// catch).
pub fn coverage_matrix() -> Result<Value, String> {
    let fixtures = load_fixtures()?;
    let rows = build_rows(&fixtures)?;

    let shortfalls = rows
        .iter()
        .filter(|row| row.level < row.required)
        .map(|row| {
            format!(
                "{} (required {}, found {})",
                row.key,
                row.required.as_str(),
                row.level.as_str()
            )
        })
        .collect::<Vec<_>>();
    if !shortfalls.is_empty() {
        return Err(format!(
            "conformance corpus feature coverage matrix has uncovered features: {}",
            shortfalls.join("; ")
        ));
    }

    let fixtures_json = fixtures
        .iter()
        .map(|fixture| {
            json!({
                "file": fixture.file,
                "depth": fixture.depth,
                "states": fixture.states().len(),
                "vectors": fixture.vectors().len(),
            })
        })
        .collect::<Vec<_>>();
    let features_json = rows.iter().map(FeatureRow::to_json).collect::<Vec<_>>();

    Ok(json!({
        "$schema": COVERAGE_SCHEMA_ID,
        "schema_version": COVERAGE_SCHEMA_VERSION,
        "kernel_schema_version": fsl_core::KERNEL_SCHEMA_VERSION,
        "conformance_schema_version": crate::CONFORMANCE_SCHEMA_VERSION,
        "result": "conformance_coverage",
        "fixtures": fixtures_json,
        "features": features_json,
    }))
}

/// Render a `matrix` produced by [`coverage_matrix`] as a Markdown
/// feature-by-fixture table.
#[must_use]
pub fn coverage_matrix_markdown(matrix: &Value) -> String {
    let mut out = String::new();
    let fixtures = matrix["fixtures"].as_array().cloned().unwrap_or_default();
    let features = matrix["features"].as_array().cloned().unwrap_or_default();
    let fixture_files = fixtures
        .iter()
        .filter_map(|fixture| fixture["file"].as_str())
        .collect::<Vec<_>>();

    out.push_str("# Conformance corpus feature coverage matrix\n\n");
    out.push_str("## Fixtures\n\n");
    out.push_str("| File | Depth | States | Vectors |\n");
    out.push_str("|---|---|---|---|\n");
    for fixture in &fixtures {
        let _ = writeln!(
            out,
            "| `{}` | {} | {} | {} |",
            fixture["file"].as_str().unwrap_or_default(),
            fixture["depth"],
            fixture["states"],
            fixture["vectors"],
        );
    }

    out.push_str("\n## Features\n\n");
    let _ = write!(out, "| Feature | Level |");
    for file in &fixture_files {
        let _ = write!(out, " `{file}` |");
    }
    out.push_str(" Description |\n");
    let _ = write!(out, "|---|---|");
    for _ in &fixture_files {
        let _ = write!(out, "---|");
    }
    out.push_str("---|\n");
    for feature in &features {
        let key = feature["key"].as_str().unwrap_or_default();
        let level = feature["level"].as_str().unwrap_or_default();
        let description = feature["description"].as_str().unwrap_or_default();
        let mut per_fixture: BTreeMap<&str, usize> = BTreeMap::new();
        if let Some(evidence) = feature["evidence"].as_array() {
            for entry in evidence {
                if let Some(fixture) = entry["fixture"].as_str() {
                    let count = entry["count"].as_u64().unwrap_or(0);
                    *per_fixture.entry(fixture).or_insert(0) += usize::try_from(count).unwrap_or(0);
                }
            }
        }
        let _ = write!(out, "| `{key}` | {level} |");
        for file in &fixture_files {
            match per_fixture.get(file) {
                Some(count) => {
                    let _ = write!(out, " {count} |");
                }
                None => {
                    let _ = write!(out, " - |");
                }
            }
        }
        let _ = writeln!(out, " {description} |");
    }
    out
}
