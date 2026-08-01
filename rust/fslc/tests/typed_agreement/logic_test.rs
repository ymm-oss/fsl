// SPDX-License-Identifier: Apache-2.0

use std::collections::{BTreeMap, BTreeSet};
use std::path::{Path, PathBuf};

use serde_json::{Value, json};

use super::{engines, generator, inventory, shrink};

fn compare_case(
    case: &generator::LogicCase,
    model: &fsl_core::KernelModel,
) -> Result<engines::AgreementObservation, engines::AgreementFailure> {
    let mut observation = engines::compare_agreement(&case.case_id, model, case.depth)?;
    engines::require_expected_violation(&case.case_id, &observation, case.expected_violation_step)?;
    observation.required_edges.push("generated_expectation");
    Ok(observation)
}

fn failure_key(failure: &engines::AgreementFailure) -> String {
    format!("{}:{}", failure.edge, failure.field)
}

fn report_path() -> PathBuf {
    let repository = Path::new(env!("CARGO_MANIFEST_DIR"))
        .parent()
        .and_then(Path::parent)
        .expect("repository root");
    std::env::var_os("FSL_LOGIC_REPORT").map_or_else(
        || repository.join("rust/target/fsl-logic/report.json"),
        |value| {
            let configured = PathBuf::from(value);
            if configured.is_absolute() {
                configured
            } else {
                repository.join(configured)
            }
        },
    )
}

fn write_report(path: &Path, report: &Value) {
    if let Some(parent) = path.parent() {
        std::fs::create_dir_all(parent).expect("create FSL Logic report directory");
    }
    std::fs::write(
        path,
        serde_json::to_vec_pretty(report).expect("serialize FSL Logic report"),
    )
    .expect("write FSL Logic report");
}

fn validate_report(report: &Value) {
    let schema: Value = serde_json::from_slice(
        &std::fs::read(
            Path::new(env!("CARGO_MANIFEST_DIR"))
                .parent()
                .and_then(Path::parent)
                .expect("repository root")
                .join("schemas/fslc/assurance/fsl-logic-report.v1.schema.json"),
        )
        .expect("read FSL Logic report schema"),
    )
    .expect("parse FSL Logic report schema");
    jsonschema::options()
        .build(&schema)
        .expect("compile FSL Logic report schema")
        .validate(report)
        .expect("FSL Logic report matches schema");
}

fn parse_replay_case(value: &str) -> (u64, usize, usize) {
    let coordinates = value
        .strip_prefix("fsl-logic-v1-s")
        .and_then(|rest| rest.split_once("-c"))
        .unwrap_or_else(|| panic!("invalid FSL_LOGIC_CASE '{value}'"));
    let (index, depth) = coordinates
        .1
        .split_once("-d")
        .unwrap_or_else(|| panic!("invalid FSL_LOGIC_CASE depth '{value}'"));
    (
        coordinates.0.parse().expect("case seed is u64"),
        index.parse().expect("case index is usize"),
        depth.parse().expect("case depth is usize"),
    )
}

#[test]
// Report mutation is intentionally one ordered transaction: complete must be
// the last write after cases, edge union, axis coverage, and schema validation.
#[allow(clippy::too_many_lines)]
fn fsl_logic_generated_agreement_is_complete_and_replayable() {
    let inventory = inventory::inventory();
    let tier = std::env::var("FSL_LOGIC_TIER").unwrap_or_else(|_| "pr".to_owned());
    let tier_config = inventory["tiers"]
        .get(&tier)
        .unwrap_or_else(|| panic!("FSL_LOGIC_TIER must be 'pr' or 'scheduled', got '{tier}'"));
    let cases_per_configuration = tier_config["cases_per_configuration"]
        .as_u64()
        .expect("tier case count");
    let replay_case = std::env::var("FSL_LOGIC_CASE").ok();
    let seed_override = std::env::var("FSL_LOGIC_SEED").ok();
    let (seeds, depths, cases) = if let Some(case_id) = replay_case.as_deref() {
        let (case_seed, case_index, case_depth) = parse_replay_case(case_id);
        (
            vec![case_seed],
            vec![case_depth],
            vec![generator::logic_case_at_depth(
                case_seed, case_index, case_depth,
            )],
        )
    } else {
        let seeds = seed_override.as_ref().map_or_else(
            || {
                tier_config["seeds"]
                    .as_array()
                    .expect("tier seeds")
                    .iter()
                    .map(|seed| seed.as_u64().expect("seed is u64"))
                    .collect::<Vec<_>>()
            },
            |value| vec![value.parse().expect("FSL_LOGIC_SEED is u64")],
        );
        let depths = tier_config["depths"]
            .as_array()
            .expect("tier depths")
            .iter()
            .map(|depth| {
                usize::try_from(depth.as_u64().expect("depth is u64")).expect("depth fits usize")
            })
            .collect::<Vec<_>>();
        let count = usize::try_from(cases_per_configuration).expect("count fits usize");
        let cases = seeds
            .iter()
            .flat_map(|seed| {
                depths
                    .iter()
                    .flat_map(|depth| generator::logic_cases_at_depth(*seed, count, *depth))
            })
            .collect();
        (seeds, depths, cases)
    };
    let path = report_path();
    let expected = cases.len();
    let mut report = json!({
        "schema": "fslc.fsl-logic-report.v1",
        "schema_version": 1,
        "tier": tier,
        "seeds": seeds,
        "depths": depths,
        "expected": expected,
        "executed": 0,
        "complete": false,
        "semantic_lineages": inventory["semantic_lineages"].clone(),
        "required_edges": inventory["required_edges"].clone(),
        "excluded_observation_fields": inventory["excluded_observation_fields"].clone(),
        "axis_counts": {},
        "cases": []
    });
    write_report(&path, &report);

    let mut axis_counts = BTreeMap::<String, usize>::new();
    let mut observed_edges = BTreeSet::new();
    for case in cases {
        let model = engines::build(&case.case_id, &case.source);
        let replay = format!(
            "FSL_LOGIC_CASE={} FSL_LOGIC_TIER={} cargo test --manifest-path rust/Cargo.toml -p fslc-rust --test typed_agreement --locked logic_test::fsl_logic_generated_agreement_is_complete_and_replayable -- --exact --nocapture",
            case.case_id, tier
        );
        let observation = match compare_case(&case, &model) {
            Ok(observation) => observation,
            Err(failure) => {
                let signature = failure_key(&failure);
                let minimized = shrink::shrink_case(&case, &signature, |candidate| {
                    let candidate_model = engines::build(&candidate.case_id, &candidate.source);
                    compare_case(candidate, &candidate_model)
                        .err()
                        .map(|candidate_failure| failure_key(&candidate_failure))
                });
                let minimized_replay = format!(
                    "FSL_LOGIC_CASE={} FSL_LOGIC_TIER={} cargo test --manifest-path rust/Cargo.toml -p fslc-rust --test typed_agreement --locked logic_test::fsl_logic_generated_agreement_is_complete_and_replayable -- --exact --nocapture",
                    minimized.case_id, tier
                );
                report["cases"]
                    .as_array_mut()
                    .expect("case rows")
                    .push(json!({
                        "case_id": case.case_id,
                        "seed": case.seed,
                        "index": case.index,
                        "coordinates": {
                            "domain_kind": case.domain_kind.label(),
                            "domain_size": case.domain_size,
                            "property_kind": case.property_kind.label(),
                            "state_vars": case.state_vars,
                            "action_count": case.action_count,
                            "guarded": case.guarded,
                            "fair": case.fair,
                            "expected_violation": case.expected_violation,
                            "expected_violation_step": case.expected_violation_step,
                            "depth": case.depth
                        },
                        "status": "disagreement",
                        "failure_signature": failure.to_string(),
                        "replay_command": replay,
                        "source": case.source,
                        "minimized": {
                            "case_id": minimized.case_id,
                            "seed": minimized.seed,
                            "index": minimized.index,
                            "depth": minimized.depth,
                            "domain_kind": minimized.domain_kind.label(),
                            "domain_size": minimized.domain_size,
                            "property_kind": minimized.property_kind.label(),
                            "state_vars": minimized.state_vars,
                            "action_count": minimized.action_count,
                            "guarded": minimized.guarded,
                            "fair": minimized.fair,
                            "expected_violation": minimized.expected_violation,
                            "expected_violation_step": minimized.expected_violation_step,
                            "replay_command": minimized_replay,
                            "source": minimized.source
                        }
                    }));
                validate_report(&report);
                write_report(&path, &report);
                panic!(
                    "FSL Logic disagreement: {failure}\nreplay: {replay}\ncoordinates: seed={} index={} domain={} size={} property={} depth={}\n{}",
                    case.seed,
                    case.index,
                    case.domain_kind.label(),
                    case.domain_size,
                    case.property_kind.label(),
                    case.depth,
                    minimized.source
                );
            }
        };
        for edge in &observation.required_edges {
            observed_edges.insert(*edge);
        }
        for key in [
            format!("domain:{}", case.domain_kind.label()),
            format!("size:{}", case.domain_size),
            format!("property:{}", case.property_kind.label()),
            format!("state_vars:{}", case.state_vars),
            format!("action_count:{}", case.action_count),
            format!("guarded:{}", case.guarded),
            format!("fair:{}", case.fair),
            format!("expected_violation:{}", case.expected_violation),
            format!(
                "expected_violation_step:{}",
                case.expected_violation_step
                    .map_or_else(|| "none".to_owned(), |step| step.to_string())
            ),
            format!("depth:{}", case.depth),
        ] {
            *axis_counts.entry(key).or_default() += 1;
        }
        report["cases"]
            .as_array_mut()
            .expect("case rows")
            .push(json!({
                "case_id": case.case_id,
                "seed": case.seed,
                "index": case.index,
                "status": "agreed",
                "expected_violation": case.expected_violation,
                "expected_violation_step": case.expected_violation_step,
                "verdict": format!("{:?}", observation.verdict),
                "property_location": observation.property_location,
                "completeness": {
                    "requested_depth": observation.completeness.requested_depth,
                    "monitor_depth_reached": observation.completeness.monitor_depth_reached,
                    "explicit_depth_reached": observation.completeness.explicit_depth_reached,
                    "monitor_closure": observation.completeness.monitor_closure,
                    "explicit_closure": observation.completeness.explicit_closure,
                    "bmc_frontier_progress": observation.completeness.bmc_frontier_progress
                },
                "replay_command": replay
            }));
        report["executed"] = json!(report["cases"].as_array().expect("case rows").len());
        write_report(&path, &report);
    }

    report["axis_counts"] = serde_json::to_value(&axis_counts).expect("axis counts JSON");
    if replay_case.is_some() || seed_override.is_some() {
        // A single-case replay or ad-hoc seed is diagnostic evidence, not a
        // complete PR/scheduled tier. It may exit successfully without
        // pretending it covered the accepted seed matrix.
        validate_report(&report);
        write_report(&path, &report);
        return;
    }

    let required = inventory["required_edges"]
        .as_array()
        .expect("required edge inventory")
        .iter()
        .map(|value| value.as_str().expect("edge name"))
        .collect::<BTreeSet<_>>();
    assert_eq!(
        observed_edges, required,
        "all named agreement edges must execute"
    );
    for (axis, values) in inventory["generator"]["axes"]
        .as_object()
        .expect("generation axes")
    {
        let selected_values = if axis == "depth" {
            &tier_config["depths"]
        } else {
            values
        };
        for value in selected_values.as_array().expect("axis values") {
            let rendered = value
                .as_str()
                .map_or_else(|| value.to_string(), str::to_owned);
            let key = match axis.as_str() {
                "domain_kind" => format!("domain:{rendered}"),
                "domain_size" => format!("size:{rendered}"),
                "property_kind" => format!("property:{rendered}"),
                other => format!("{other}:{rendered}"),
            };
            assert!(
                axis_counts.contains_key(&key),
                "selected tier did not exercise inventoried axis value {axis}={rendered}"
            );
        }
    }
    assert_eq!(
        report["executed"].as_u64(),
        Some(u64::try_from(expected).expect("expected fits u64")),
        "partial execution must fail closed"
    );
    report["complete"] = json!(true);
    validate_report(&report);
    write_report(&path, &report);
}
