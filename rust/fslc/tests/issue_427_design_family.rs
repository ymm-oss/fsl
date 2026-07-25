// SPDX-License-Identifier: Apache-2.0
// Copyright 2026 Ryoichi Izumita

//! Evidence-gated Phase 0 prototype for issue #427.
//!
//! This is deliberately a test-owned sidecar harness, not an `fslc family`
//! product command. It composes the existing native process contracts without
//! adding grammar, Kernel, runtime, solver, or verifier semantics.

use std::collections::{BTreeMap, BTreeSet};
use std::path::{Path, PathBuf};
use std::process::{Command, Output};

use serde_json::{Map, Value, json};
use sha2::{Digest, Sha256};

const SCHEMA: &str = "schemas/fslc/design-family/design-family.v0.schema.json";
const FAMILIES: [&str; 3] = [
    "order_processing",
    "concurrent_ownership",
    "persistence_model",
];

struct PrototypeReport {
    stable: Value,
    evidence: Vec<Value>,
    exit_code: i32,
}

fn repository_file(relative: &str) -> PathBuf {
    Path::new(env!("CARGO_MANIFEST_DIR"))
        .join("../..")
        .join(relative)
}

fn fixture(family: &str) -> PathBuf {
    Path::new(env!("CARGO_MANIFEST_DIR"))
        .join("tests/fixtures/design_family")
        .join(family)
}

fn read_json(path: &Path) -> Value {
    read_json_result(path).unwrap_or_else(|error| panic!("{error}"))
}

fn read_json_result(path: &Path) -> Result<Value, String> {
    let source = std::fs::read_to_string(path)
        .map_err(|error| format!("read {}: {error}", path.display()))?;
    serde_json::from_str(&source).map_err(|error| format!("parse JSON {}: {error}", path.display()))
}

fn compiled_schema() -> jsonschema::Validator {
    jsonschema::validator_for(&read_json(&repository_file(SCHEMA))).expect("schema compiles")
}

fn assert_schema_valid(value: &Value) {
    schema_result(value).unwrap_or_else(|error| panic!("{error}"));
}

fn schema_result(value: &Value) -> Result<(), String> {
    let errors = compiled_schema()
        .iter_errors(value)
        .map(|error| error.to_string())
        .collect::<Vec<_>>();
    if errors.is_empty() {
        Ok(())
    } else {
        Err(format!("schema validation errors: {errors:?}"))
    }
}

fn run(cwd: &Path, arguments: &[String]) -> Output {
    Command::new(env!("CARGO_BIN_EXE_fslc"))
        .args(arguments)
        .current_dir(cwd)
        .output()
        .expect("run native fslc")
}

fn json_stdout(output: &Output) -> Value {
    serde_json::from_slice(&output.stdout).unwrap_or_else(|error| {
        panic!(
            "stdout is not JSON: {error}\n{}\nstderr:\n{}",
            String::from_utf8_lossy(&output.stdout),
            String::from_utf8_lossy(&output.stderr)
        );
    })
}

fn command_evidence(cwd: &Path, phase: &str, subject: &str, arguments: Vec<String>) -> Value {
    let output = run(cwd, &arguments);
    let exit_code = output.status.code().unwrap_or(3);
    let stdout_raw = String::from_utf8(output.stdout).expect("fslc stdout is UTF-8");
    let stdout_json: Value = serde_json::from_str(&stdout_raw).unwrap_or(Value::Null);
    let json_parsed = !stdout_json.is_null();
    let argv = arguments.into_iter().map(Value::String).collect::<Vec<_>>();
    json!({
        "phase": phase,
        "subject": subject,
        "argv": argv,
        "exit_code": exit_code,
        "stdout_raw": stdout_raw,
        "stdout_json": stdout_json,
        "json_parsed": json_parsed,
        "stderr_raw": String::from_utf8(output.stderr).expect("fslc stderr is UTF-8"),
    })
}

fn nested_implements_gate(result: &Value) -> i32 {
    let Some(implements) = result.get("implements") else {
        return 0;
    };
    let Some(nested) = implements
        .as_object()
        .and_then(|object| object.get("result"))
        .and_then(Value::as_str)
    else {
        return 2;
    };
    match nested {
        "refines" => 0,
        "refinement_failed" => 1,
        _ => 2,
    }
}

fn canonical(value: &Value) -> Value {
    match value {
        Value::Object(object) => {
            let sorted = object
                .iter()
                .map(|(key, value)| (key.clone(), canonical(value)))
                .collect::<BTreeMap<_, _>>();
            Value::Object(sorted.into_iter().collect::<Map<_, _>>())
        }
        Value::Array(items) => Value::Array(items.iter().map(canonical).collect()),
        _ => value.clone(),
    }
}

fn sha256_bytes(bytes: &[u8]) -> String {
    let mut hasher = Sha256::new();
    hasher.update(bytes);
    format!("sha256:{:x}", hasher.finalize())
}

fn stable_digest(value: &Value) -> String {
    let bytes = serde_json::to_vec(&canonical(value)).expect("canonical JSON serializes");
    sha256_bytes(&bytes)
}

fn framed_input_digest(
    cwd: &Path,
    manifest_name: &str,
    inputs: &[String],
    override_input: Option<(&str, &[u8])>,
) -> Result<String, String> {
    let mut paths = inputs.to_vec();
    paths.push(manifest_name.to_owned());
    paths.sort();
    paths.dedup();
    let mut framed = b"fsl-design-family-source-bundle-v0\0".to_vec();
    for relative in paths {
        let bytes = match override_input {
            Some((path, bytes)) if path == relative => bytes.to_vec(),
            _ => std::fs::read(cwd.join(&relative))
                .map_err(|error| format!("read bundle input {relative}: {error}"))?,
        };
        framed.extend((relative.len() as u64).to_be_bytes());
        framed.extend(relative.as_bytes());
        framed.extend((bytes.len() as u64).to_be_bytes());
        framed.extend(bytes);
    }
    Ok(sha256_bytes(&framed))
}

fn strings(value: &Value) -> Vec<String> {
    value
        .as_array()
        .expect("array")
        .iter()
        .map(|item| item.as_str().expect("string").to_owned())
        .collect()
}

fn semantic_digest_summary<'a>(models: impl IntoIterator<Item = &'a Value>) -> (usize, Vec<Value>) {
    let mut groups = BTreeMap::<String, Vec<String>>::new();
    for model in models {
        let (Some(id), Some(digest)) = (model["id"].as_str(), model["semantic_digest"].as_str())
        else {
            continue;
        };
        groups
            .entry(digest.to_owned())
            .or_default()
            .push(id.to_owned());
    }
    let distinct = groups.len();
    let warnings = groups
        .into_iter()
        .filter_map(|(digest, mut variants)| {
            if variants.len() < 2 {
                return None;
            }
            variants.sort();
            Some(json!({
                "kind": "duplicate_semantic_digest",
                "semantic_digest": digest,
                "variants": variants,
            }))
        })
        .collect();
    (distinct, warnings)
}

fn manifest_consistency(manifest: &Value) -> Result<(), String> {
    let contract = &manifest["contract"];
    require_declared_input(contract, "source", "contract")?;
    let variants = manifest["variants"].as_array().expect("schema validated");
    let mut ids = BTreeSet::new();
    for variant in variants {
        let id = variant["id"].as_str().expect("schema validated");
        if !ids.insert(id) {
            return Err(format!("duplicate variant id '{id}'"));
        }
        require_declared_input(variant, "source", id)?;
        require_declared_input(variant, "refinement", id)?;
    }
    let mut comparison_ids = BTreeSet::new();
    for comparison in manifest["comparisons"]
        .as_array()
        .expect("schema validated")
    {
        let id = comparison["id"].as_str().expect("schema validated");
        if !comparison_ids.insert(id) {
            return Err(format!("duplicate comparison id '{id}'"));
        }
        for endpoint in ["old", "new"] {
            let variant = comparison[endpoint].as_str().expect("schema validated");
            if !ids.contains(variant) {
                return Err(format!(
                    "comparison '{id}' references unknown {endpoint} '{variant}'"
                ));
            }
        }
        if comparison["old"] == comparison["new"] {
            return Err(format!("comparison '{id}' must name two distinct variants"));
        }
    }
    let bundle = &manifest["bundle_control"];
    require_declared_input(bundle, "entry", "bundle_control")?;
    require_declared_input(bundle, "dependency", "bundle_control")?;
    Ok(())
}

fn require_declared_input(owner: &Value, field: &str, context: &str) -> Result<(), String> {
    let required = owner[field].as_str().expect("schema validated");
    if strings(&owner["inputs"])
        .iter()
        .any(|input| input == required)
    {
        Ok(())
    } else {
        Err(format!(
            "{context} {field} '{required}' is absent from its exact inputs"
        ))
    }
}

fn assurance_projection(result: &Value, requested_engine: &str) -> Value {
    let completeness = result
        .get("completeness")
        .and_then(Value::as_str)
        .unwrap_or("not_run");
    json!({
        "result": result.get("result").cloned().unwrap_or(Value::Null),
        "assurance": match completeness {
            "bounded" => "bounded",
            "unbounded" => "proved",
            _ => "not_run",
        },
        "completeness": result.get("completeness").cloned().unwrap_or(Value::Null),
        "requested_engine": requested_engine,
        "producer_engine": result.get("engine").cloned().unwrap_or(Value::Null),
        "checked_to_depth": result.get("checked_to_depth").cloned().unwrap_or(Value::Null),
    })
}

fn variant_by_id<'a>(manifest: &'a Value, id: &str) -> &'a Value {
    manifest["variants"]
        .as_array()
        .expect("variants")
        .iter()
        .find(|variant| variant["id"] == id)
        .unwrap_or_else(|| panic!("unknown variant {id}"))
}

fn aggregate_exit(codes: impl IntoIterator<Item = i32>) -> i32 {
    let mut result = 0;
    for code in codes {
        result = result.max(match code {
            2 => 2,
            1 => 1,
            0 => 0,
            _ => 3,
        });
    }
    result
}

fn evidence_exit(row: &Value) -> i32 {
    i32::try_from(row["exit_code"].as_i64().unwrap_or(3)).unwrap_or(3)
}

fn producer_gate(row: &Value, accepted_results: &[&str]) -> i32 {
    if !row["json_parsed"].as_bool().unwrap_or(false) {
        return 3;
    }
    match evidence_exit(row) {
        2 => 2,
        0 | 1 if !producer_envelope_valid(row) => 2,
        0 | 1 if !producer_result_exit_valid(row, accepted_results) => 2,
        1 => 1,
        0 => nested_implements_gate(&row["stdout_json"]),
        _ => 3,
    }
}

fn producer_result_exit_valid(row: &Value, accepted_results: &[&str]) -> bool {
    let exit = evidence_exit(row);
    let Some(result) = row["stdout_json"]["result"].as_str() else {
        return false;
    };
    match exit {
        0 => accepted_results.contains(&result),
        1 => match row["phase"].as_str() {
            Some("verify") => matches!(result, "violated" | "reachable_failed"),
            Some("refine" | "comparison_mapping") => result == "refinement_failed",
            _ => false,
        },
        _ => false,
    }
}

fn producer_envelope_valid(row: &Value) -> bool {
    let output = &row["stdout_json"];
    if !output["result"].is_string() {
        return false;
    }
    if output.get("implements").is_some() && nested_implements_gate(output) == 2 {
        return false;
    }
    match row["phase"].as_str() {
        Some("semantic_digest") => {
            output["spec"]["name"].is_string()
                && output["spec"]["spec_digest"].is_string()
                && output["spec"]["spec_digest_algorithm"].is_string()
        }
        Some("check" | "bundle_control_check") => output["versions"].is_object(),
        Some("verify") => match output["result"].as_str() {
            Some("verified") => {
                output["completeness"] == "bounded"
                    && output["checked_to_depth"].is_u64()
                    && output["versions"].is_object()
            }
            Some("violated") => {
                output["completeness"] == "bounded"
                    && output["checked_to_depth"].is_u64()
                    && output["versions"].is_object()
                    && output["violated_at_step"].is_u64()
                    && output["trace"].is_array()
            }
            Some("reachable_failed") => {
                output["completeness"] == "bounded"
                    && output["checked_to_depth"].is_u64()
                    && output["versions"].is_object()
                    && output["unreached"].is_array()
            }
            _ => false,
        },
        Some("refine" | "comparison_mapping") => {
            output["impl"].is_string()
                && output["abs"].is_string()
                && match output["result"].as_str() {
                    Some("refines") => output["checked_to_depth"].is_u64(),
                    Some("refinement_failed") => {
                        output["kind"].is_string() && output["violated_at_step"].is_u64()
                    }
                    _ => false,
                }
        }
        Some("diff") => {
            output["old"]["spec"].is_string()
                && output["new"]["spec"].is_string()
                && output["scope"]["comparison"].is_string()
                && output["bounded"]["completeness"].is_string()
                && output["summary"].is_array()
        }
        _ => false,
    }
}

fn failed_manifest_report(family: &str, manifest_name: &str, message: &str) -> PrototypeReport {
    let mut stable = json!({
        "schema_version": "fsl-design-family-report.v0",
        "family_id": family,
        "result": "failed",
        "gates": {
            "catalog_eligibility": "failed",
            "pair_comparison": "not_run",
            "distinct_semantic_candidates": 0,
        },
        "error": {
            "kind": "manifest_contract",
            "manifest": manifest_name,
            "message": message,
        },
    });
    let report_digest = stable_digest(&stable);
    stable
        .as_object_mut()
        .expect("report object")
        .insert("report_digest".to_owned(), json!(report_digest));
    PrototypeReport {
        stable,
        evidence: Vec::new(),
        exit_code: 2,
    }
}

#[allow(clippy::too_many_lines)]
fn run_family(family: &str) -> PrototypeReport {
    run_family_manifest(family, "manifest.json")
}

#[allow(clippy::too_many_lines)]
fn run_family_manifest(family: &str, manifest_name: &str) -> PrototypeReport {
    let cwd = fixture(family);
    let manifest = match read_json_result(&cwd.join(manifest_name)) {
        Ok(manifest) => manifest,
        Err(error) => return failed_manifest_report(family, manifest_name, &error),
    };
    run_family_value(family, manifest_name, &manifest)
}

#[allow(clippy::too_many_lines)]
fn run_family_value(family: &str, manifest_name: &str, manifest: &Value) -> PrototypeReport {
    if let Err(error) = schema_result(manifest).and_then(|()| manifest_consistency(manifest)) {
        return failed_manifest_report(family, manifest_name, &error);
    }
    let cwd = fixture(family);
    let depth = manifest["verification"]["depth"]
        .as_u64()
        .expect("depth")
        .to_string();
    let engine = manifest["verification"]["engine"]
        .as_str()
        .expect("engine")
        .to_owned();
    let mut evidence = Vec::new();
    let mut catalog_codes = Vec::new();
    let mut comparison_codes = Vec::new();
    let mut stable_models = Vec::new();
    let contract = &manifest["contract"];
    let mut models = vec![("contract".to_owned(), contract.clone())];
    models.extend(
        manifest["variants"]
            .as_array()
            .expect("variants")
            .iter()
            .map(|variant| {
                (
                    variant["id"].as_str().expect("id").to_owned(),
                    variant.clone(),
                )
            }),
    );

    for (id, model) in &models {
        let source = model["source"].as_str().expect("source").to_owned();
        let claims = command_evidence(
            &cwd,
            "semantic_digest",
            id,
            vec!["document".into(), "claims".into(), source.clone()],
        );
        let check = command_evidence(&cwd, "check", id, vec!["check".into(), source.clone()]);
        let verify = command_evidence(
            &cwd,
            "verify",
            id,
            vec![
                "verify".into(),
                source,
                "--engine".into(),
                engine.clone(),
                "--depth".into(),
                depth.clone(),
            ],
        );
        let claims_gate = producer_gate(&claims, &["requirement_claims"]);
        let check_gate = producer_gate(&check, &["ok"]);
        let verify_gate = producer_gate(&verify, &["verified"]);
        let symbol_gate = i32::from(claims["stdout_json"]["spec"]["name"] != model["symbol"]) * 2;
        let model_exit = aggregate_exit([claims_gate, check_gate, verify_gate, symbol_gate]);
        catalog_codes.push(model_exit);
        stable_models.push(json!({
            "id": id,
            "symbol": model["symbol"],
            "source": model["source"],
            "status": if model_exit == 0 { "eligible" } else { "failed" },
            "semantic_digest": claims["stdout_json"]["spec"]["spec_digest"],
            "semantic_digest_algorithm": claims["stdout_json"]["spec"]["spec_digest_algorithm"],
            "verification": assurance_projection(&verify["stdout_json"], &engine),
            "check_result": check["stdout_json"]["result"],
        }));
        evidence.extend([claims, check, verify]);
    }

    let (distinct_candidates, warnings) = semantic_digest_summary(stable_models.iter().skip(1));
    if distinct_candidates < 2 {
        catalog_codes.push(2);
    }

    let contract_source = contract["source"].as_str().expect("contract source");
    let mut stable_refinements = Vec::new();
    let mut mapping_digests = BTreeMap::new();
    for variant in manifest["variants"].as_array().expect("variants") {
        let id = variant["id"].as_str().expect("id");
        let mapping = variant["refinement"].as_str().expect("refinement");
        let row = command_evidence(
            &cwd,
            "refine",
            id,
            vec![
                "refine".into(),
                variant["source"].as_str().expect("source").into(),
                contract_source.into(),
                mapping.into(),
                "--depth".into(),
                depth.clone(),
            ],
        );
        let mut row_gate = producer_gate(&row, &["refines"]);
        if row["stdout_json"]["abs"] != contract["symbol"] {
            row_gate = row_gate.max(2);
        }
        if let Ok(bytes) = std::fs::read(cwd.join(mapping)) {
            mapping_digests.insert(mapping.to_owned(), json!(sha256_bytes(&bytes)));
        } else {
            row_gate = row_gate.max(2);
            mapping_digests.insert(mapping.to_owned(), Value::Null);
        }
        catalog_codes.push(row_gate);
        stable_refinements.push(json!({
            "variant": id,
            "status": if row_gate == 0 { "passed" } else { "failed" },
            "result": row["stdout_json"]["result"],
            "checked_to_depth": row["stdout_json"]["checked_to_depth"],
        }));
        evidence.push(row);
    }

    let mut stable_comparisons = Vec::new();
    for comparison in manifest["comparisons"].as_array().expect("comparisons") {
        let id = comparison["id"].as_str().expect("comparison id");
        let old = variant_by_id(manifest, comparison["old"].as_str().expect("old"));
        let new = variant_by_id(manifest, comparison["new"].as_str().expect("new"));
        let mapping = comparison["mapping"].as_str().expect("mapping");
        let mapping_probe = command_evidence(
            &cwd,
            "comparison_mapping",
            id,
            vec![
                "refine".into(),
                new["source"].as_str().expect("new source").into(),
                old["source"].as_str().expect("old source").into(),
                mapping.into(),
                "--depth".into(),
                comparison["depth"].as_u64().expect("depth").to_string(),
            ],
        );
        let mut mapping_gate = match producer_gate(&mapping_probe, &["refines"]) {
            1 => 0,
            code => code,
        };
        if mapping_probe["stdout_json"]["impl"] != new["symbol"]
            || mapping_probe["stdout_json"]["abs"] != old["symbol"]
        {
            mapping_gate = mapping_gate.max(2);
        }
        let row = command_evidence(
            &cwd,
            "diff",
            id,
            vec![
                "diff".into(),
                old["source"].as_str().expect("old source").into(),
                new["source"].as_str().expect("new source").into(),
                "--depth".into(),
                comparison["depth"].as_u64().expect("depth").to_string(),
                "--mapping".into(),
                mapping.into(),
            ],
        );
        let mut diff_gate = producer_gate(&row, &["semantic_diff", "no_semantic_change"]);
        if row["stdout_json"]["old"]["spec"] != old["symbol"]
            || row["stdout_json"]["new"]["spec"] != new["symbol"]
            || row["stdout_json"]["scope"]["comparison"] != comparison["scope_owner"]
            || row["stdout_json"]["bounded"]["completeness"] != "bounded"
        {
            diff_gate = diff_gate.max(2);
        }
        if let Ok(bytes) = std::fs::read(cwd.join(mapping)) {
            mapping_digests.insert(mapping.to_owned(), json!(sha256_bytes(&bytes)));
        } else {
            mapping_gate = mapping_gate.max(2);
            mapping_digests.insert(mapping.to_owned(), Value::Null);
        }
        let comparison_gate = aggregate_exit([mapping_gate, diff_gate]);
        comparison_codes.push(comparison_gate);
        stable_comparisons.push(json!({
            "id": id,
            "status": if comparison_gate == 0 { "reported" } else { "failed" },
            "old": comparison["old"],
            "new": comparison["new"],
            "scope_owner": comparison["scope_owner"],
            "mapping": mapping,
            "mapping_direction": comparison["mapping_direction"],
            "depth": comparison["depth"],
            "result": row["stdout_json"]["result"],
            "summary": row["stdout_json"]["summary"],
            "completeness": row["stdout_json"]["bounded"]["completeness"],
            "mapping_probe_result": mapping_probe["stdout_json"]["result"],
        }));
        evidence.extend([mapping_probe, row]);
    }

    let bundle = &manifest["bundle_control"];
    let bundle_check = command_evidence(
        &cwd,
        "bundle_control_check",
        "import_probe",
        vec![
            "check".into(),
            bundle["entry"].as_str().expect("entry").into(),
        ],
    );
    catalog_codes.push(producer_gate(&bundle_check, &["ok"]));
    evidence.push(bundle_check);

    let all_inputs = models
        .iter()
        .flat_map(|(_, model)| strings(&model["inputs"]))
        .chain(strings(&bundle["inputs"]))
        .chain(mapping_digests.keys().cloned())
        .collect::<Vec<_>>();
    let source_bundle_digest =
        if let Ok(digest) = framed_input_digest(&cwd, manifest_name, &all_inputs, None) {
            json!(digest)
        } else {
            catalog_codes.push(2);
            Value::Null
        };
    let catalog_exit = aggregate_exit(catalog_codes);
    let comparison_exit = aggregate_exit(comparison_codes);
    let exit_code = aggregate_exit([catalog_exit, comparison_exit]);
    let producer = evidence
        .iter()
        .find(|row| row["phase"] == "check")
        .map_or(Value::Null, |row| row["stdout_json"]["versions"].clone());
    let mut stable = json!({
        "schema_version": "fsl-design-family-report.v0",
        "family_id": manifest["family_id"],
        "result": if exit_code == 0 { "eligible" } else { "failed" },
        "gates": {
            "catalog_eligibility": if catalog_exit == 0 { "passed" } else { "failed" },
            "pair_comparison": if comparison_exit == 0 { "passed" } else { "failed" },
            "distinct_semantic_candidates": distinct_candidates,
        },
        "contract": stable_models.remove(0),
        "variants": stable_models,
        "refinements": stable_refinements,
        "comparisons": stable_comparisons,
        "warnings": warnings,
        "digests": {
            "source_bundle": source_bundle_digest,
            "source_bundle_algorithm": "fsl-design-family-source-bundle-v0+sha256",
            "mappings": mapping_digests,
        },
        "producer": producer,
    });
    let report_digest = stable_digest(&stable);
    stable
        .as_object_mut()
        .expect("report object")
        .insert("report_digest".to_owned(), json!(report_digest));
    PrototypeReport {
        stable,
        evidence,
        exit_code,
    }
}

#[test]
fn prototype_schema_is_closed_and_rejects_malformed_manifests() {
    let mut family_ids = BTreeSet::new();
    for family in FAMILIES {
        let manifest = read_json(&fixture(family).join("manifest.json"));
        assert_schema_valid(&manifest);
        manifest_consistency(&manifest).expect("valid manifest");
        assert!(
            family_ids.insert(
                manifest["family_id"]
                    .as_str()
                    .expect("family id")
                    .to_owned()
            )
        );
    }
    let mut malformed = read_json(&fixture(FAMILIES[0]).join("manifest.json"));
    malformed
        .as_object_mut()
        .expect("object")
        .insert("ranking".to_owned(), json!("best"));
    assert!(!compiled_schema().is_valid(&malformed));
    let malformed_report = run_family_value("order_processing", "inline.json", &malformed);
    assert_eq!(malformed_report.exit_code, 2);
    assert_eq!(
        malformed_report.stable["error"]["kind"],
        "manifest_contract"
    );
    assert!(malformed_report.evidence.is_empty());

    let mut bad_reference = read_json(&fixture(FAMILIES[0]).join("manifest.json"));
    bad_reference["comparisons"][0]["old"] = json!("missing");
    assert!(manifest_consistency(&bad_reference).is_err());
    let bad_reference_report = run_family_value("order_processing", "inline.json", &bad_reference);
    assert_eq!(bad_reference_report.exit_code, 2);

    let mut incomplete_inputs = read_json(&fixture(FAMILIES[0]).join("manifest.json"));
    incomplete_inputs["variants"][0]["inputs"] = json!(["synchronous_refines_contract.fsl"]);
    assert!(manifest_consistency(&incomplete_inputs).is_err());
    let incomplete_report = run_family_value("order_processing", "inline.json", &incomplete_inputs);
    assert_eq!(incomplete_report.exit_code, 2);

    let mut self_comparison = read_json(&fixture(FAMILIES[0]).join("manifest.json"));
    self_comparison["comparisons"][0]["new"] = self_comparison["comparisons"][0]["old"].clone();
    assert!(manifest_consistency(&self_comparison).is_err());

    let mut no_comparisons = read_json(&fixture(FAMILIES[0]).join("manifest.json"));
    no_comparisons["comparisons"] = json!([]);
    assert!(!compiled_schema().is_valid(&no_comparisons));

    let mut unsupported_engine = read_json(&fixture(FAMILIES[0]).join("manifest.json"));
    unsupported_engine["verification"]["engine"] = json!("induction");
    assert!(!compiled_schema().is_valid(&unsupported_engine));
}

#[test]
fn prototype_runs_three_families_with_deterministic_provenance() {
    let mut report_digests = BTreeSet::new();
    for family in FAMILIES {
        let report = run_family(family);
        assert_eq!(report.exit_code, 0);
        assert_eq!(report.stable["result"], "eligible");
        assert_eq!(report.stable["gates"]["catalog_eligibility"], "passed");
        assert_eq!(report.stable["gates"]["pair_comparison"], "passed");
        assert_eq!(report.stable["warnings"], json!([]));
        assert_eq!(report.stable["variants"].as_array().map(Vec::len), Some(3));
        for model in std::iter::once(&report.stable["contract"]).chain(
            report.stable["variants"]
                .as_array()
                .expect("variants")
                .iter(),
        ) {
            assert_eq!(model["verification"]["assurance"], "bounded");
            assert_eq!(model["verification"]["completeness"], "bounded");
            assert_eq!(model["verification"]["requested_engine"], "bmc");
        }
        assert!(
            report.stable["comparisons"]
                .as_array()
                .expect("comparisons")
                .iter()
                .all(|comparison| comparison["scope_owner"] == "new"
                    && comparison["mapping_direction"] == "new_to_old")
        );
        assert!(report.evidence.iter().all(|row| {
            row.get("stdout_raw").and_then(Value::as_str).is_some()
                && row.get("exit_code").and_then(Value::as_i64).is_some()
        }));
        let mut without_digest = report.stable.clone();
        let declared = without_digest
            .as_object_mut()
            .expect("object")
            .remove("report_digest")
            .expect("digest");
        assert_eq!(declared, stable_digest(&without_digest));
        report_digests.insert(declared.as_str().expect("digest").to_owned());
    }
    assert_eq!(report_digests.len(), FAMILIES.len());
}

#[test]
fn duplicate_semantic_candidates_warn_without_failing_eligibility() {
    let candidates = json!([
        {"id": "a", "semantic_digest": "sha256:same"},
        {"id": "b", "semantic_digest": "sha256:same"},
        {"id": "c", "semantic_digest": "sha256:other"}
    ]);
    let (distinct, warnings) =
        semantic_digest_summary(candidates.as_array().expect("candidate array"));
    assert_eq!(distinct, 2);
    assert!(distinct >= 2, "A/A/B remains eligible");
    assert_eq!(
        warnings,
        vec![json!({
            "kind": "duplicate_semantic_digest",
            "semantic_digest": "sha256:same",
            "variants": ["a", "b"]
        })]
    );
}

#[test]
fn negative_controls_reject_false_family_success() {
    let cases = [
        ("order_processing", "synchronous.fsl"),
        ("concurrent_ownership", "shared_lock.fsl"),
        ("persistence_model", "crud.fsl"),
    ];
    for (family, variant) in cases {
        let output = run(
            &fixture(family),
            &[
                "refine".into(),
                variant.into(),
                "contract.fsl".into(),
                "negative_refinement.fsl".into(),
                "--depth".into(),
                "4".into(),
            ],
        );
        assert_eq!(output.status.code(), Some(1));
        let result = json_stdout(&output);
        assert_eq!(result["result"], "refinement_failed");
    }

    let failed_report = run_family_manifest("order_processing", "negative_manifest.json");
    assert_eq!(failed_report.exit_code, 1);
    assert_eq!(failed_report.stable["result"], "failed");
    assert_eq!(
        failed_report.stable["gates"]["catalog_eligibility"],
        "failed"
    );
    assert_eq!(failed_report.stable["gates"]["pair_comparison"], "passed");
    assert_eq!(
        failed_report.stable["variants"].as_array().map(Vec::len),
        Some(3)
    );
    assert_eq!(
        failed_report.stable["refinements"].as_array().map(Vec::len),
        Some(3)
    );
    assert_eq!(failed_report.stable["refinements"][0]["status"], "failed");
    assert!(failed_report.evidence.iter().any(|row| {
        row["phase"] == "refine" && row["subject"] == "synchronous" && row["exit_code"] == 1
    }));

    let inline = run(
        &repository_file("tests/fixtures/chain"),
        &["check".into(), "requirements_broken_implements.fsl".into()],
    );
    assert_eq!(inline.status.code(), Some(0));
    let inline_json = json_stdout(&inline);
    assert_eq!(inline_json["implements"]["result"], "refinement_failed");
    assert_eq!(nested_implements_gate(&inline_json), 1);
    let nested_row = json!({
        "phase": "check",
        "exit_code": 0,
        "json_parsed": true,
        "stdout_json": inline_json,
    });
    assert_eq!(producer_gate(&nested_row, &["ok"]), 1);

    assert_producer_contract_negative_controls();

    assert_eq!(aggregate_exit([0, 1]), 1);
    assert_eq!(aggregate_exit([1, 2]), 2);
    assert_eq!(aggregate_exit([2, 3]), 3);
    assert_eq!(aggregate_exit([0, 4]), 3);
}

fn assert_producer_contract_negative_controls() {
    let non_json_failure = json!({
        "exit_code": 1,
        "json_parsed": false,
        "stdout_json": null,
    });
    assert_eq!(producer_gate(&non_json_failure, &["ok"]), 3);
    let unexpected_exit = json!({
        "phase": "check",
        "exit_code": 4,
        "json_parsed": true,
        "stdout_json": {"result": "ok", "versions": {}},
    });
    assert_eq!(producer_gate(&unexpected_exit, &["ok"]), 3);

    let malformed_implements = json!({
        "phase": "check",
        "exit_code": 0,
        "json_parsed": true,
        "stdout_json": {"result": "ok", "versions": {}, "implements": {}},
    });
    assert_eq!(producer_gate(&malformed_implements, &["ok"]), 2);
    let unknown_implements = json!({
        "phase": "check",
        "exit_code": 0,
        "json_parsed": true,
        "stdout_json": {"result": "ok", "versions": {}, "implements": {"result": "maybe"}},
    });
    assert_eq!(producer_gate(&unknown_implements, &["ok"]), 2);
    let incomplete_verify = json!({
        "phase": "verify",
        "exit_code": 0,
        "json_parsed": true,
        "stdout_json": {"result": "verified", "checked_to_depth": 4, "versions": {}},
    });
    assert_eq!(producer_gate(&incomplete_verify, &["verified"]), 2);
    let reachable_failure = json!({
        "phase": "verify",
        "exit_code": 1,
        "json_parsed": true,
        "stdout_json": {
            "result": "reachable_failed",
            "completeness": "bounded",
            "checked_to_depth": 4,
            "versions": {},
            "unreached": ["CanFinish"]
        },
    });
    assert_eq!(producer_gate(&reachable_failure, &["verified"]), 1);
    let incomplete_refine = json!({
        "phase": "refine",
        "exit_code": 0,
        "json_parsed": true,
        "stdout_json": {"result": "refines", "impl": "I", "abs": "A"},
    });
    assert_eq!(producer_gate(&incomplete_refine, &["refines"]), 2);
    let success_with_failure_exit = json!({
        "phase": "comparison_mapping",
        "exit_code": 1,
        "json_parsed": true,
        "stdout_json": {"result": "refines", "impl": "I", "abs": "A", "checked_to_depth": 4},
    });
    assert_eq!(producer_gate(&success_with_failure_exit, &["refines"]), 2);
    let failure_with_success_exit = json!({
        "phase": "comparison_mapping",
        "exit_code": 0,
        "json_parsed": true,
        "stdout_json": {
            "result": "refinement_failed",
            "impl": "I",
            "abs": "A",
            "kind": "stutter_changed_abs",
            "violated_at_step": 1
        },
    });
    assert_eq!(producer_gate(&failure_with_success_exit, &["refines"]), 2);
    let incomplete_claims = json!({
        "phase": "semantic_digest",
        "exit_code": 0,
        "json_parsed": true,
        "stdout_json": {"result": "requirement_claims", "spec": {"name": "S"}},
    });
    assert_eq!(
        producer_gate(&incomplete_claims, &["requirement_claims"]),
        2
    );
    let incomplete_diff = json!({
        "phase": "diff",
        "exit_code": 0,
        "json_parsed": true,
        "stdout_json": {"result": "semantic_diff", "old": {"spec": "O"}, "new": {"spec": "N"}},
    });
    assert_eq!(producer_gate(&incomplete_diff, &["semantic_diff"]), 2);
}

#[test]
fn dependency_drift_and_comparison_orientation_remain_explicit() {
    for family in FAMILIES {
        let cwd = fixture(family);
        let manifest = read_json(&cwd.join("manifest.json"));
        let bundle = &manifest["bundle_control"];
        let inputs = strings(&bundle["inputs"]);
        let baseline =
            framed_input_digest(&cwd, "manifest.json", &inputs, None).expect("baseline digest");
        let dependency = bundle["dependency"].as_str().expect("dependency");
        let mut changed = std::fs::read(cwd.join(dependency)).expect("dependency bytes");
        changed.extend(b"\n// dependency-only drift control\n");
        let drifted =
            framed_input_digest(&cwd, "manifest.json", &inputs, Some((dependency, &changed)))
                .expect("drift digest");
        assert_ne!(baseline, drifted);
    }

    let cwd = fixture("order_processing");
    let forward = run(
        &cwd,
        &[
            "diff".into(),
            "synchronous.fsl".into(),
            "event_driven.fsl".into(),
            "--depth".into(),
            "1".into(),
            "--mapping".into(),
            "event_to_synchronous.fsl".into(),
        ],
    );
    let reversed = run(
        &cwd,
        &[
            "diff".into(),
            "event_driven.fsl".into(),
            "synchronous.fsl".into(),
            "--depth".into(),
            "1".into(),
            "--mapping".into(),
            "event_to_synchronous.fsl".into(),
        ],
    );
    assert!(forward.status.success() && reversed.status.success());
    let forward = json_stdout(&forward);
    let reversed = json_stdout(&reversed);
    assert_eq!(forward["bounded"]["completeness"], "bounded");
    assert_eq!(reversed["bounded"]["completeness"], "bounded");
    assert_eq!(forward["old"]["spec"], "SynchronousOrder");
    assert_eq!(forward["new"]["spec"], "EventDrivenOrder");
    assert_eq!(reversed["old"]["spec"], "EventDrivenOrder");
    assert_eq!(reversed["new"]["spec"], "SynchronousOrder");
    assert_ne!(forward["directions"], reversed["directions"]);
}
