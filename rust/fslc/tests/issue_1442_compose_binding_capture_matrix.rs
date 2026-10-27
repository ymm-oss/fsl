// SPDX-License-Identifier: Apache-2.0
// Copyright 2026 Ryoichi Izumita

//! Alpha-equivalence across every finite compose/sync binding-source product.
//! Fresh controls must violate Capture at step1 in both engines. Only statement
//! forall/Collection may instead report the exact known BMC unsupported error;
//! check must still succeed and explicit must still violate Capture at step1.

#[path = "support/compose_aggregate_pattern.rs"]
mod aggregate;
#[path = "support/compose_scratch.rs"]
mod compose_scratch;
#[path = "support/compose_binding_extra.rs"]
mod extra;
#[path = "support/compose_binding_matrix.rs"]
mod matrix;
#[path = "support/compose_owner_refusal.rs"]
mod owner_refusal;
#[path = "support/compose_source_owner.rs"]
mod source_owner;

use std::collections::{BTreeMap, BTreeSet};
use std::fmt::Write as _;
use std::path::Path;
use std::process::Command;
use std::time::Instant;

use serde_json::{Value, json};

type Observation = (Value, i32);
type Observations = Vec<Observation>;

enum Cell {
    Legacy(matrix::Cell),
    Added(extra::Cell),
}
impl Cell {
    fn id(&self) -> String {
        match self {
            Self::Legacy(c) => c.id(),
            Self::Added(c) => c.id(),
        }
    }
    fn family(&self) -> &str {
        match self {
            Self::Legacy(c) => c.family(),
            Self::Added(c) => c.family(),
        }
    }
    fn render(&self, fresh: bool) -> matrix::SourceBundle {
        match self {
            Self::Legacy(c) => c.render(fresh),
            Self::Added(c) => c.render(fresh),
        }
    }
    fn rejection(&self) -> bool {
        matches!(self,Self::Added(c) if c.rejection())
    }
    fn invariant(&self) -> &str {
        match self {
            Self::Legacy(_) => "Capture",
            Self::Added(c) => c.invariant(),
        }
    }
    fn known_error(&self, mode: usize) -> Option<&'static str> {
        match self {
            Self::Legacy(c) if mode == 1 && c.collection_statement() => Some(COLLECTION_BMC_ERROR),
            Self::Added(c) => c.unsupported(mode),
            Self::Legacy(_) => None,
        }
    }
    fn explain(&self) -> bool {
        match self {
            Self::Added(c) => c.explain(),
            Self::Legacy(c) => {
                matches!(
                    c.family(),
                    "prefix-top"
                        | "prefix-nested"
                        | "alias-top"
                        | "alias-nested"
                        | "alias-local-pattern"
                ) || matches!(c.family(), "substitution" | "merge-argument")
                    && c.columns[3] == "alias-root"
            }
        }
    }
    fn source_owner(&self) -> aggregate::Owner {
        use aggregate::Owner;
        let id = self.id();
        let parts: Vec<_> = id.split('|').collect();
        match self.family() {
            "alias-top" => {
                if parts[1].starts_with("glue-") {
                    Owner::Glue
                } else if parts[1].starts_with("header-component-") {
                    Owner::Arguments
                } else {
                    Owner::Additional
                }
            }
            "alias-nested" | "alias-local-pattern" => {
                if parts[1].starts_with("glue-") {
                    Owner::Glue
                } else if parts[1].starts_with("argument-") {
                    Owner::Arguments
                } else {
                    Owner::Additional
                }
            }
            "merge-guard" | "merge-local-pattern" => {
                if parts[2].starts_with("c-") {
                    Owner::Component("c.fsl")
                } else if parts[2].starts_with("additional-") {
                    Owner::Additional
                } else {
                    Owner::Component("b.fsl")
                }
            }
            "merge-argument" | "argument-scope" | "argument-no-interference" => Owner::Arguments,
            _ => Owner::Component("b.fsl"),
        }
    }
}

fn original_cells() -> Vec<Cell> {
    matrix::cells()
        .into_iter()
        .map(Cell::Legacy)
        .chain(extra::cells().into_iter().map(Cell::Added))
        .collect()
}

fn invoke(dir: &Path, args: &[&str]) -> (Value, i32) {
    if let Some(binary) = std::env::var_os("FSL_MATRIX_BINARY") {
        // Explicit test configuration for a separately built binary. An
        // invalid configured path is an error, never a fallback to the base.
        let binary = std::fs::canonicalize(binary).expect("configured matrix binary");
        let output = Command::new(binary)
            .args(args)
            .current_dir(dir)
            .output()
            .expect("configured fslc");
        return (
            serde_json::from_slice(&output.stdout).expect("configured fslc JSON"),
            output.status.code().expect("fslc exit"),
        );
    }
    match args {
        ["check", file] => compose_scratch::check(dir, file),
        ["verify", file, "--depth", "4", "--no-cache"] => compose_scratch::verify(dir, file),
        _ => compose_scratch::run(dir, args),
    }
}

fn materialize(bundle: &matrix::SourceBundle) -> std::path::PathBuf {
    let files: Vec<_> = bundle
        .files
        .iter()
        .map(|(name, source)| (name.as_str(), source.as_str()))
        .collect();
    compose_scratch::scratch("issue-1442-capture-matrix", &files)
}

fn observe(
    bundle: &matrix::SourceBundle,
    cell: &Cell,
    with_explain: bool,
) -> (Observations, Option<Observation>, Option<Observation>) {
    let dir = materialize(bundle);
    let check = invoke(&dir, &["check", "s.fsl"]);
    if cell.rejection() {
        return (vec![check, invoke(&dir, &["check", "b.fsl"])], None, None);
    }
    let direct = if let aggregate::Owner::Component(file) = cell.source_owner() {
        Some(invoke(&dir, &["check", file]))
    } else {
        None
    };
    let explain = (with_explain && cell.explain())
        .then(|| invoke(&dir, &["explain", "s.fsl", "--depth", "4"]));
    (
        vec![
            check,
            invoke(&dir, &["verify", "s.fsl", "--depth", "4", "--no-cache"]),
            invoke(
                &dir,
                &[
                    "verify",
                    "s.fsl",
                    "--engine",
                    "explicit",
                    "--depth",
                    "4",
                    "--no-cache",
                ],
            ),
        ],
        explain,
        direct,
    )
}

const COLLECTION_BMC_ERROR: &str =
    "collection binders are not implemented in the current verifier slice";

fn collection_bmc_error((value, code): &(Value, i32)) -> bool {
    *code == 2 && value["result"] == "error" && value["message"] == COLLECTION_BMC_ERROR
}

fn violated((value, code): &(Value, i32)) -> bool {
    *code == 1
        && value["result"] == "violated"
        && value["invariant"] == "Capture"
        && value["violation_kind"] == "invariant"
        && value["violated_at_step"] == 1
}

fn calibrated(observations: &[(Value, i32)], collection_statement: bool) -> bool {
    observations[0].1 == 0
        && observations[0].0["result"] == "ok"
        && violated(&observations[2])
        && (violated(&observations[1])
            || (collection_statement && collection_bmc_error(&observations[1])))
}

fn exact_error(observation: &(Value, i32), message: &str) -> bool {
    observation.1 == 2 && observation.0["result"] == "error" && observation.0["message"] == message
}

fn valid_violation(observation: &(Value, i32), invariant: &str) -> bool {
    observation.1 == 1
        && observation.0["result"] == "violated"
        && observation.0["violation_kind"] == "invariant"
        && observation.0["invariant"] == invariant
        && observation.0["violated_at_step"] == 1
}

fn accepted(observations: &[(Value, i32)], cell: &Cell) -> bool {
    observations[0].1 == 0
        && observations[0].0["result"] == "ok"
        && [1, 2].into_iter().all(|mode| {
            valid_violation(&observations[mode], cell.invariant())
                || cell
                    .known_error(mode)
                    .is_some_and(|message| exact_error(&observations[mode], message))
        })
}

fn valid_rejection(observation: &(Value, i32), cell: &extra::Cell) -> bool {
    let base = if cell.rejected_update() {
        format!(
            "invalid model expression: unknown update target '{}'",
            cell.rejected_name()
        )
    } else {
        format!(
            "invalid model expression: public Kernel cannot type identifier '{}'",
            cell.rejected_name()
        )
    };
    let j = &observation.0;
    let at = format!("{base} at {}:{}", j["loc"]["line"], j["loc"]["column"]);
    observation.1 == 2
        && j["result"] == "error"
        && j["kind"] == "semantics"
        && (j["message"] == base || j["message"] == at)
}

fn forbidden_names(value: &Value) -> BTreeSet<String> {
    value
        .to_string()
        .split(|c: char| !c.is_ascii_alphanumeric() && c != '_')
        .filter(|word| word.starts_with("__") && word.contains("_substitution_"))
        .map(str::to_owned)
        .collect()
}

fn normalize_state(value: &mut Value, from: &str) -> Result<(), String> {
    let trace = value["trace"]
        .as_array_mut()
        .ok_or("missing live trace for state alpha")?;
    for step in trace {
        for field in ["state", "changes"] {
            if let Some(object) = step.get_mut(field).and_then(Value::as_object_mut) {
                if let Some(v) = object.remove(from) {
                    if object.insert("Red".into(), v).is_some() {
                        return Err("state alpha merged keys".into());
                    }
                } else if field == "state" {
                    return Err("missing live alpha state".into());
                }
            }
        }
    }
    if let Some(properties) = value
        .pointer_mut("/cost/properties")
        .and_then(Value::as_array_mut)
    {
        for prop in properties.iter_mut() {
            if prop["kind"].as_str().is_none() || prop["name"].as_str().is_none() {
                return Err("cost property identity must contain live string kind/name".into());
            }
            if prop["name"] == format!("_bounds_{from}") {
                prop["name"] = json!("_bounds_Red");
            }
        }
        properties.sort_by_key(|v| {
            (
                v["kind"].as_str().expect("validated kind").to_owned(),
                v["name"].as_str().expect("validated name").to_owned(),
            )
        });
    }
    Ok(())
}

fn compare(
    same: &[(Value, i32)],
    fresh: &[(Value, i32)],
    same_bundle: &matrix::SourceBundle,
    fresh_bundle: &matrix::SourceBundle,
    cell: &Cell,
) -> Vec<String> {
    let mut errors = Vec::new();
    for (index, (left, right)) in same.iter().zip(fresh).enumerate() {
        let normalize = |observation: &(Value, i32), bundle: &matrix::SourceBundle, fresh: bool| {
            let mut value = observation.0.clone();
            let has_cost = index != 0 && value["result"] != "error";
            let parameter = if has_cost {
                bundle.alpha_parameter.as_ref()
            } else {
                None
            };
            let state = if has_cost {
                if let Cell::Added(c) = cell {
                    if c.state_alpha(true).is_some() {
                        normalize_state(&mut value, if fresh { "z" } else { "Red" })
                    } else {
                        Ok(())
                    }
                } else {
                    Ok(())
                }
            } else {
                Ok(())
            };
            let status = state.and_then(|()| matrix::normalize(&mut value, parameter, has_cost));
            (value, status)
        };
        let (left_json, ls) = normalize(left, same_bundle, false);
        let (right_json, rs) = normalize(right, fresh_bundle, true);
        if left.1 != right.1 || left_json != right_json || ls.is_err() || rs.is_err() {
            errors.push(format!("full outcome differs at mode{index}; normalization={ls:?}/{rs:?}; left={left_json}; right={right_json}"));
        }
    }
    errors
}

#[test]
// All cells are collected before either oracle assertion is allowed to fail.
#[allow(clippy::too_many_lines)]
fn all_compose_and_sync_binding_sources_preserve_alpha_equivalence() {
    let cells = original_cells();
    assert_eq!(cells.len(), 2368);
    assert_eq!(
        cells.iter().map(Cell::id).collect::<BTreeSet<_>>().len(),
        2368
    );
    assert_eq!(cells.iter().filter(|c| c.explain()).count(), 557);
    let report_dir = compose_scratch::scratch("issue-1442-matrix-report", &[]);
    let mut table = "family\torigin\tposition\tcollision_kind\tshape_or_order\n".to_owned();
    for cell in &cells {
        table.push_str(&cell.id().replace('|', "\t"));
        table.push('\n');
    }
    std::fs::write(report_dir.join("cells.tsv"), table).expect("write observed cell inventory");
    let started = Instant::now();
    let mut reports = Vec::new();
    // Disjoint counts: defect-red / full green / invalid / BMC unsupported.
    let mut families: BTreeMap<String, [usize; 6]> = BTreeMap::new();
    let mut owner_refused = Vec::new();
    let mut refusal_failures = [0; 3];
    let mut bad_controls = 0;
    let mut unsupported_bmc = Vec::new();
    let mut unsupported = Vec::new();
    for cell in cells {
        let same_bundle = cell.render(false);
        let fresh_bundle = cell.render(true);
        let (same, explain, same_source) = observe(&same_bundle, &cell, true);
        let (fresh, _, fresh_source) = observe(&fresh_bundle, &cell, false);
        // Classification comes from actual standalone checks, even when a
        // faulty compose accepts. No list of root names or messages decides it.
        if fresh_source.as_ref().is_some_and(|(_, code)| *code == 2) {
            let checks: Vec<_> = [(&same_source, &same), (&fresh_source, &fresh)]
                .into_iter()
                .map(|(direct, composed)| {
                    owner_refusal::checks(
                        direct.as_ref().expect("component owner"),
                        &composed[0],
                        cell.source_owner().name(),
                        "b.Match",
                        "s.fsl",
                        (2, 3),
                    )
                })
                .collect();
            for check in &checks {
                for (failure, passed) in refusal_failures.iter_mut().zip(check) {
                    *failure += usize::from(!passed);
                }
            }
            owner_refused.push(cell.id());
            families.entry(cell.family().into()).or_default()[5] += 1;
            reports.push(json!({"id":cell.id(),"family":cell.family(),"owner_refused":true,"source_checks":[same_source,fresh_source],"refusal_checks":checks,"same":same,"fresh":fresh,"explain":explain}));
            continue;
        }
        let control_ok = if let Cell::Added(c) = &cell {
            if c.rejection() {
                valid_rejection(&same[1], c) && valid_rejection(&fresh[1], c)
            } else {
                accepted(&fresh, &cell)
            }
        } else {
            accepted(&fresh, &cell)
        };
        let same_ok = !cell.rejection() && accepted(&same, &cell);
        bad_controls += usize::from(!control_ok);
        let mut errors = Vec::new();
        if !control_ok {
            errors.push("fresh control violates the check/BMC/explicit oracle".to_owned());
        }
        if !cell.rejection() && !same_ok {
            errors.push("collision form violates the check/BMC/explicit oracle".to_owned());
        }
        if control_ok && same_ok {
            errors.extend(compare(&same, &fresh, &same_bundle, &fresh_bundle, &cell));
        }
        if cell.rejection() {
            for (label, observations) in [("same", &same), ("fresh", &fresh)] {
                if observations[0] != observations[1] {
                    errors.push(format!("{label} composition did not preserve complete standalone error: composed={:?}; direct={:?}",observations[0],observations[1]));
                }
            }
        }
        let internal = explain
            .as_ref()
            .map_or_else(BTreeSet::new, |(j, _)| forbidden_names(j));
        if let Some((j, code)) = &explain
            && (*code != 0 || j["result"] != "explained" || !internal.is_empty())
        {
            errors.push(format!(
                "unnecessary rename/explain failure: exit={code} names={internal:?}"
            ));
        }
        let red = control_ok && !errors.is_empty();
        let bmc_unsupported = control_ok
            && same_ok
            && !red
            && matches!(&cell, Cell::Legacy(_))
            && collection_bmc_error(&fresh[1]);
        let root_unsupported = control_ok
            && same_ok
            && !red
            && matches!(&cell, Cell::Added(_))
            && fresh.iter().skip(1).any(|(j, _)| j["result"] == "error");
        if root_unsupported {
            eprintln!("MATRIX_ROOT_UNSUPPORTED {}", cell.id());
            unsupported.push(cell.id());
        }
        if bmc_unsupported {
            eprintln!("MATRIX_BMC_UNSUPPORTED {}", cell.id());
            unsupported_bmc.push(cell.id());
        }
        let classification = if control_ok {
            if root_unsupported {
                4
            } else if bmc_unsupported {
                3
            } else {
                usize::from(!red)
            }
        } else {
            2
        };
        families.entry(cell.family().into()).or_default()[classification] += 1;
        if !control_ok || red {
            eprintln!(
                "MATRIX_{} {} control_ok={control_ok} same={} fresh={}",
                if control_ok { "RED" } else { "INVALID" },
                cell.id(),
                summary(&same),
                summary(&fresh)
            );
        }
        reports.push(json!({"id": cell.id(), "family": cell.family(), "red": red, "invalid": !control_ok, "control_ok": control_ok, "rejection":cell.rejection(),"bmc_unsupported": bmc_unsupported,"root_unsupported":root_unsupported,"explain":explain,"internal_names":internal, "same": same, "fresh": fresh, "errors": errors}));
    }
    let elapsed = started.elapsed().as_secs_f64();
    let red: usize = families.values().map(|counts| counts[0]).sum();
    let green: usize = families.values().map(|counts| counts[1]).sum();
    let report = json!({"cells": reports, "families": families, "red": red, "green": green, "invalid": bad_controls, "bad_controls": bad_controls, "unsupported_bmc": unsupported_bmc,"unsupported":unsupported, "owner_refused":owner_refused,"refusal_failures":refusal_failures, "elapsed_s": elapsed});
    std::fs::write(
        report_dir.join("report.json"),
        serde_json::to_vec(&report).expect("serialize complete observations"),
    )
    .expect("write full matrix report");
    eprintln!(
        "MATRIX_RESULT red={red} green={green} invalid={bad_controls} bmc_unsupported={} unsupported={} elapsed_s={elapsed:.3} report={}",
        unsupported_bmc.len(),
        unsupported.len(),
        report_dir.display()
    );
    eprintln!(
        "MATRIX_OWNER_REFUSED old={} checks={refusal_failures:?}",
        owner_refused.len()
    );
    assert_eq!(
        owner_refused.len(),
        16,
        "standalone refusal inventory changed"
    );
    owner_refusal::assert_preserved(refusal_failures);
    assert_eq!(
        bad_controls, 0,
        "invalid fresh controls; see complete report"
    );
    assert_eq!(
        red, 0,
        "binding-capture matrix has red cells; see complete report"
    );
}

#[test]
fn unsupported_bmc_requires_the_exact_factor_message_exit_and_explicit_control() {
    let control = vec![
        (json!({"result":"ok"}), 0),
        (json!({"result":"error","message":COLLECTION_BMC_ERROR}), 2),
        (
            json!({"result":"violated","invariant":"Capture","violation_kind":"invariant","violated_at_step":1}),
            1,
        ),
    ];
    for cell in matrix::cells() {
        assert_eq!(
            calibrated(&control, cell.collection_statement()),
            cell.collection_statement(),
            "{}",
            cell.id()
        );
    }
    let mut changed = control.clone();
    changed[1].0["message"] = json!(format!("{COLLECTION_BMC_ERROR}."));
    assert!(
        !calibrated(&changed, true),
        "no other error message admitted"
    );
    changed = control.clone();
    changed[1].1 = 1;
    assert!(!calibrated(&changed, true), "exit2 is mandatory");
    changed = control;
    changed[2] = (json!({"result":"proved"}), 0);
    assert!(!calibrated(&changed, true), "explicit may not be waived");
}

fn summary(observations: &[(Value, i32)]) -> Value {
    Value::Array(observations.iter().map(|(value, code)| json!({"exit":code,"result":value["result"],"invariant":value.get("invariant"),"step":value.get("violated_at_step")})).collect())
}

#[test]
fn root_limits_are_factor_and_exact_message_gated_and_do_not_waive_supported_results() {
    let mut messages = BTreeSet::new();
    for added in extra::cells().into_iter().filter(|c| !c.rejection()) {
        let cell = Cell::Added(added);
        let normal = vec![
            (json!({"result":"ok"}), 0),
            (
                json!({"result":"violated","violation_kind":"invariant","invariant":cell.invariant(),"violated_at_step":1}),
                1,
            ),
            (
                json!({"result":"violated","violation_kind":"invariant","invariant":cell.invariant(),"violated_at_step":1}),
                1,
            ),
        ];
        assert!(accepted(&normal, &cell));
        for mode in [1, 2] {
            if let Some(message) = cell.known_error(mode) {
                messages.insert(message);
                let mut limited = normal.clone();
                limited[mode] = (json!({"result":"error","message":message}), 2);
                assert!(accepted(&limited, &cell));
                limited[mode].0["message"] = json!(format!("{message}."));
                assert!(!accepted(&limited, &cell));
                limited[mode].0["message"] = json!(message);
                limited[mode].1 = 1;
                assert!(!accepted(&limited, &cell));
            }
        }
    }
    assert_eq!(messages.len(), 5);
    for added in extra::cells().into_iter().filter(|c| !c.rejection()) {
        let cell = Cell::Added(added);
        for mode in [1, 2] {
            for message in &messages {
                let mut values = vec![
                    (json!({"result":"ok"}), 0),
                    (
                        json!({"result":"violated","violation_kind":"invariant","invariant":cell.invariant(),"violated_at_step":1}),
                        1,
                    ),
                    (
                        json!({"result":"violated","violation_kind":"invariant","invariant":cell.invariant(),"violated_at_step":1}),
                        1,
                    ),
                ];
                values[mode] = (json!({"result":"error","message":message}), 2);
                assert_eq!(
                    accepted(&values, &cell),
                    cell.known_error(mode) == Some(*message),
                    "wrong root/engine factor: {} mode{mode}",
                    cell.id()
                );
            }
        }
    }
    assert_eq!(
        forbidden_names(&json!({"guard":"some(true) is some(__u_substitution_0)"})),
        BTreeSet::from(["__u_substitution_0".into()])
    );
    assert!(forbidden_names(&json!({"state":"b__flag","label":"u_substitution_0"})).is_empty());
}

struct SourceCheckCase {
    id: String,
    bundle: matrix::SourceBundle,
    owner: aggregate::Owner,
    aggregate_accepts: Option<bool>,
    owner_refusal_candidate: bool,
}

#[test]
#[allow(clippy::too_many_lines)] // Collect all source owners before failing either contract.
fn aggregate_pattern_and_owner_source_checks() {
    let mut cases = Vec::new();
    for cell in original_cells() {
        for fresh in [false, true] {
            cases.push(SourceCheckCase {
                id: format!("{}|{}", cell.id(), if fresh { "fresh" } else { "same" }),
                bundle: cell.render(fresh),
                owner: cell.source_owner(),
                aggregate_accepts: None,
                owner_refusal_candidate: !cell.rejection(),
            });
        }
    }
    for cell in aggregate::cells() {
        cases.push(SourceCheckCase {
            id: cell.id.clone(),
            owner: cell.owner(),
            bundle: cell.render(),
            aggregate_accepts: Some(cell.accepts()),
            owner_refusal_candidate: cell.accepts(),
        });
    }
    assert_eq!(cases.len(), 5960);
    let report_dir = compose_scratch::scratch("aggregate-pattern-report", &[]);
    let started = Instant::now();
    let mut counts: BTreeMap<String, usize> = BTreeMap::new();
    let mut aggregate_counts: BTreeMap<String, [usize; 4]> = BTreeMap::new();
    let mut owner_refused_old = BTreeSet::new();
    let mut owner_refused_new = BTreeSet::new();
    let mut refusal_failures = [0; 3];
    let mut reference_errors = 0;
    let mut forbidden = 0;
    let mut reports = Vec::new();
    let mut table = "case\towner\tstandalone\ttype-identities-or-reason\n".to_owned();
    for case in cases {
        let dir = materialize(&case.bundle);
        let composed = invoke(&dir, &["check", "s.fsl"]);
        let holder = source_owner::standalone(&case.bundle, case.owner);
        let mut error = None;
        let mut refusal_checks = None;
        let (standalone, classification) = match holder {
            Err(reason) => {
                writeln!(
                    table,
                    "{}\t{}\tunrepresentable\t{}",
                    case.id,
                    case.owner.name(),
                    reason
                )
                .expect("owner table");
                (None, "unrepresentable".to_owned())
            }
            Ok(holder) => {
                std::fs::write(dir.join("owner.fsl"), &holder.source)
                    .expect("standalone owner source");
                let direct = invoke(&dir, &["check", "owner.fsl"]);
                writeln!(
                    table,
                    "{}\t{}\towner.fsl\t{:?}",
                    case.id, holder.description, holder.type_names
                )
                .expect("owner table");
                let classification = if case.owner_refusal_candidate && direct.1 == 2 {
                    let check = owner_refusal::checks(
                        &direct,
                        &composed,
                        case.owner.name(),
                        "b.Match",
                        "s.fsl",
                        (2, 3),
                    );
                    for (failure, passed) in refusal_failures.iter_mut().zip(check) {
                        *failure += usize::from(!passed);
                    }
                    if case.aggregate_accepts.is_some() {
                        owner_refused_new.insert(case.id.clone());
                    } else {
                        owner_refused_old
                            .insert(case.id.rsplit_once('|').expect("old shape").0.to_owned());
                    }
                    if check != [true; 3] {
                        error = Some("owner refusal diagnostic not preserved");
                    }
                    refusal_checks = Some(check);
                    "owner_refused"
                } else {
                    match (direct.1, composed.1) {
                        (0, 0) => "ok_ok",
                        (0, _) => {
                            forbidden += 1;
                            error = Some("owner ok but composition rejected");
                            "ok_error"
                        }
                        (2, 2) if direct.0["kind"] == composed.0["kind"] => {
                            if direct.0["message"] == composed.0["message"] {
                                "error_error_same_message"
                            } else {
                                "error_error_different_message"
                            }
                        }
                        (2, _) => {
                            forbidden += 1;
                            error = Some("owner refusal or kind not preserved");
                            "error_not_preserved"
                        }
                        _ => {
                            reference_errors += 1;
                            error = Some("unexpected standalone check exit");
                            "invalid_reference"
                        }
                    }
                };
                if let Some(expected) = case.aggregate_accepts
                    && refusal_checks.is_none()
                    && ((direct.1 == 0) != expected || !matches!(direct.1, 0 | 2))
                {
                    reference_errors += 1;
                    error = Some(
                        "standalone aggregate expectation disagrees with the existing scope contract",
                    );
                }
                (Some(direct), classification.to_owned())
            }
        };
        *counts.entry(classification.clone()).or_default() += 1;
        if let Some(expected) = case.aggregate_accepts {
            let kind = case
                .id
                .split('/')
                .nth(1)
                .expect("aggregate kind")
                .to_owned();
            let entry = aggregate_counts.entry(kind).or_default();
            let valid = standalone
                .as_ref()
                .is_some_and(|(_, code)| (*code == 0) == expected && matches!(*code, 0 | 2));
            entry[if refusal_checks.is_some() {
                3
            } else if valid {
                usize::from(error.is_none())
            } else {
                2
            }] += 1;
            if !valid && standalone.is_none() {
                reference_errors += 1;
                error = Some("new aggregate owner scope unrepresentable");
            }
        }
        if error.is_some() {
            eprintln!(
                "AGG_PATTERN_RED {} classification={} reason={:?} standalone={} composed={}",
                case.id,
                classification,
                error,
                standalone.as_ref().map_or(
                    Value::Null,
                    |(j, c)| json!({"exit":c,"message":j.get("message"),"result":j["result"]})
                ),
                json!({"exit":composed.1,"message":composed.0.get("message"),"result":composed.0["result"]})
            );
        }
        reports.push(json!({"id":case.id,"owner":case.owner.name(),"aggregate_accepts":case.aggregate_accepts,"standalone":standalone,"composed":composed,"classification":classification,"refusal_checks":refusal_checks,"error":error,"directory":dir}));
    }
    std::fs::write(report_dir.join("owners.tsv"), table).expect("owner map table");
    let elapsed = started.elapsed().as_secs_f64();
    std::fs::write(report_dir.join("report.json"),serde_json::to_vec(&json!({"cases":reports,"counts":counts,"aggregate_counts":aggregate_counts,"forbidden":forbidden,"invalid":reference_errors,"owner_refused_old":owner_refused_old,"owner_refused_new":owner_refused_new,"refusal_failures":refusal_failures,"inapplicable":aggregate::INAPPLICABLE_COUNT,"inapplicable_reason":aggregate::INAPPLICABLE_REASON,"elapsed_s":elapsed})).expect("report JSON")).expect("owner report");
    eprintln!(
        "AGG_PATTERN_N count={} reason={}",
        aggregate::INAPPLICABLE_COUNT,
        aggregate::INAPPLICABLE_REASON
    );
    eprintln!(
        "OWNER_REFUSED total={} old={} new={} checks={refusal_failures:?}",
        owner_refused_old.len() + owner_refused_new.len(),
        owner_refused_old.len(),
        owner_refused_new.len()
    );
    eprintln!(
        "AGG_PATTERN_RESULT counts={counts:?} aggregate={aggregate_counts:?} forbidden={forbidden} invalid={reference_errors} n={} elapsed_s={elapsed:.3} report={} owners={}",
        aggregate::INAPPLICABLE_COUNT,
        report_dir.display(),
        report_dir.join("owners.tsv").display()
    );
    assert_eq!(
        reference_errors, 0,
        "standalone expectation mismatch: stop before changing the aggregate scope contract"
    );
    assert_eq!(
        counts.get("unrepresentable"),
        Some(&394),
        "owner-scope coverage changed"
    );
    assert_eq!(
        (owner_refused_old.len(), owner_refused_new.len()),
        (16, 64),
        "standalone refusal inventory changed"
    );
    owner_refusal::assert_preserved(refusal_failures);
    assert_eq!(forbidden, 0, "standalone-to-compose admissibility drift");
}
