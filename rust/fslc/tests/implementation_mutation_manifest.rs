// SPDX-License-Identifier: Apache-2.0

use std::collections::BTreeSet;
use std::path::{Path, PathBuf};
use std::time::{SystemTime, UNIX_EPOCH};

use fslc_rust::mutants_config;
use serde_json::Value;

const REQUIRED_P2_FAULT_CLASSES: &[&str] = &[
    "SAT and UNSAT remain distinct",
    "unknown never becomes verified",
    "backend failure never becomes verified",
    "requested depth is inclusive",
    "witness states are complete",
    "witness actions are complete",
    "every witness passes concrete replay",
    "violation kind agrees independently",
    "violation name agrees independently",
    "failed location is preserved",
    "violations never fold to success",
];

fn repository_root() -> PathBuf {
    Path::new(env!("CARGO_MANIFEST_DIR"))
        .parent()
        .and_then(Path::parent)
        .expect("repository root")
        .to_path_buf()
}

fn read_json(path: &Path) -> Value {
    serde_json::from_slice(
        &std::fs::read(path).unwrap_or_else(|error| panic!("read {}: {error}", path.display())),
    )
    .unwrap_or_else(|error| panic!("parse {}: {error}", path.display()))
}

fn read_source(path: &Path) -> Result<String, std::io::Error> {
    std::fs::read_to_string(path).map(|source| normalize_line_endings(&source))
}

fn normalize_line_endings(source: &str) -> String {
    source.replace("\r\n", "\n").replace('\r', "\n")
}

fn required_string<'a>(value: &'a Value, key: &str) -> Result<&'a str, String> {
    value
        .get(key)
        .and_then(Value::as_str)
        .filter(|value| !value.trim().is_empty())
        .ok_or_else(|| format!("missing non-empty string '{key}'"))
}

fn validate_generic_functions(manifest: &Value, config: &str) -> Result<(), String> {
    let examine_block = config
        .split_once("examine_re = [")
        .and_then(|(_, tail)| tail.split_once(']'))
        .map(|(block, _)| block)
        .ok_or_else(|| "mutation runner config has no closed examine_re list".to_owned())?;
    let actual_patterns = examine_block
        .lines()
        .map(str::trim)
        .filter(|line| line.starts_with('"'))
        .map(|line| line.trim_end_matches(',').trim_matches('"').to_owned())
        .collect::<BTreeSet<_>>();
    let generic_functions = manifest
        .get("generic_functions")
        .and_then(Value::as_array)
        .filter(|functions| !functions.is_empty())
        .ok_or_else(|| "scope declares no generic mutation functions".to_owned())?;
    let expected_patterns = generic_functions
        .iter()
        .map(|function| {
            let function = function
                .as_str()
                .filter(|function| !function.trim().is_empty())
                .ok_or_else(|| "generic mutation function is empty".to_owned())?;
            Ok(format!("(replace {function}( ->| with)| in {function}$)"))
        })
        .collect::<Result<BTreeSet<_>, String>>()?;
    if actual_patterns != expected_patterns {
        return Err(format!(
            "generic mutation config drifted from scope: expected={expected_patterns:?} actual={actual_patterns:?}"
        ));
    }
    Ok(())
}

fn validate_detector_selection(config: &str) -> Result<(), String> {
    let lines = config.lines().map(str::trim).collect::<BTreeSet<_>>();
    if !lines.contains(r#"test_package = ["fslc-rust"]"#) {
        return Err("mutation detector package selection drifted".to_owned());
    }
    if !lines.contains(
        r#"additional_cargo_test_args = ["--package", "fslc-rust", "--test", "solver_fail_closed", "--test", "triangulated_assurance"]"#,
    ) {
        return Err("mutation detector target selection drifted".to_owned());
    }
    Ok(())
}

fn validate_scope(root: &Path, manifest: &Value) -> Result<(), String> {
    if manifest.get("schema").and_then(Value::as_str)
        != Some("fslc.implementation-mutation-scope.v1")
        || manifest.get("schema_version").and_then(Value::as_u64) != Some(1)
    {
        return Err("scope schema/version mismatch".to_owned());
    }
    let runner = manifest
        .get("runner")
        .and_then(Value::as_object)
        .ok_or_else(|| "missing runner".to_owned())?;
    if runner.get("name").and_then(Value::as_str) != Some("cargo-mutants")
        || runner.get("version").and_then(Value::as_str) != Some("27.1.0")
    {
        return Err("runner must pin cargo-mutants 27.1.0".to_owned());
    }
    let config_path = runner
        .get("config")
        .and_then(Value::as_str)
        .filter(|path| !path.trim().is_empty())
        .ok_or_else(|| "runner has no config path".to_owned())?;
    let config = std::fs::read_to_string(root.join(config_path))
        .map_err(|error| format!("cannot read mutation runner config '{config_path}': {error}"))?;
    mutants_config::check(root)?;
    validate_generic_functions(manifest, &config)?;
    validate_detector_selection(&config)?;

    let decisions = manifest
        .get("decisions")
        .and_then(Value::as_array)
        .filter(|decisions| !decisions.is_empty())
        .ok_or_else(|| "scope declares no decisions".to_owned())?;
    let mut ids = BTreeSet::new();
    let mut decision_functions = BTreeSet::new();
    for decision in decisions {
        let id = required_string(decision, "id")?;
        if !ids.insert(id) {
            return Err(format!("duplicate decision id '{id}'"));
        }
        let path = required_string(decision, "path")?;
        let function = required_string(decision, "function")?;
        decision_functions.insert(function);
        let anchor = required_string(decision, "anchor")?;
        let classes = decision
            .get("fault_classes")
            .and_then(Value::as_array)
            .filter(|classes| !classes.is_empty())
            .ok_or_else(|| format!("decision '{id}' has no fault classes"))?;
        if classes
            .iter()
            .any(|class| class.as_str().is_none_or(|class| class.trim().is_empty()))
        {
            return Err(format!("decision '{id}' has an empty fault class"));
        }
        let source = read_source(&root.join(path))
            .map_err(|error| format!("decision '{id}' cannot read '{path}': {error}"))?;
        let occurrences = source.matches(anchor).count();
        if occurrences != 1 {
            return Err(format!(
                "decision '{id}' anchor in {path} must occur exactly once, found {occurrences}: {anchor}"
            ));
        }
        if !source.contains(&format!("fn {function}"))
            && !source.contains(&format!("fn {function}<"))
        {
            return Err(format!(
                "decision '{id}' function '{function}' is stale in {path}"
            ));
        }
    }
    let generic_functions = manifest["generic_functions"]
        .as_array()
        .expect("generic functions validated above")
        .iter()
        .map(|function| function.as_str().expect("generic function validated above"))
        .collect::<BTreeSet<_>>();
    if decision_functions != generic_functions {
        return Err(format!(
            "generic functions must map to maintained decision anchors: expected={generic_functions:?} actual={decision_functions:?}"
        ));
    }

    let out_of_scope = manifest
        .get("out_of_scope")
        .and_then(Value::as_array)
        .filter(|entries| !entries.is_empty())
        .ok_or_else(|| "scope must state explicit exclusions".to_owned())?;
    for entry in out_of_scope {
        required_string(entry, "path")?;
        required_string(entry, "reason")?;
    }
    Ok(())
}

fn validate_equivalents(root: &Path, manifest: &Value) -> Result<(), String> {
    if manifest.get("schema").and_then(Value::as_str)
        != Some("fslc.implementation-mutation-equivalents.v1")
        || manifest.get("schema_version").and_then(Value::as_u64) != Some(1)
    {
        return Err("equivalence schema/version mismatch".to_owned());
    }
    let entries = manifest
        .get("entries")
        .and_then(Value::as_array)
        .ok_or_else(|| "equivalence entries must be an array".to_owned())?;
    let mut ids = BTreeSet::new();
    for entry in entries {
        let mutant_id = required_string(entry, "mutant_id")?;
        if mutant_id.contains('*') || mutant_id.contains("..") {
            return Err(format!(
                "equivalent mutant '{mutant_id}' must use an exact stable ID"
            ));
        }
        if !ids.insert(mutant_id) {
            return Err(format!("duplicate equivalent mutant '{mutant_id}'"));
        }
        let path = required_string(entry, "path")?;
        let anchor = required_string(entry, "anchor")?;
        required_string(entry, "rationale")?;
        required_string(entry, "reviewer")?;
        required_string(entry, "review_issue")?;
        let source = read_source(&root.join(path))
            .map_err(|error| format!("equivalent '{mutant_id}' cannot read '{path}': {error}"))?;
        let occurrences = source.matches(anchor).count();
        if occurrences != 1 {
            return Err(format!(
                "equivalent '{mutant_id}' anchor must occur exactly once, found {occurrences}"
            ));
        }
    }
    Ok(())
}

fn validate_operator_inventory(root: &Path, inventory: &str) -> Result<(), String> {
    let mut names = BTreeSet::new();
    let mut patches = BTreeSet::new();
    let mut p2_contracts = BTreeSet::new();
    for (line_index, line) in inventory.lines().enumerate() {
        let line = line.trim();
        if line.is_empty() || line.starts_with('#') {
            continue;
        }
        let columns = line.split('^').map(str::trim).collect::<Vec<_>>();
        if columns.len() != 12 || columns.iter().any(|column| column.is_empty()) {
            return Err(format!(
                "operators.txt:{} must have twelve non-empty columns",
                line_index + 1
            ));
        }
        let [
            name,
            patch,
            _contract,
            seam_path,
            seam_anchor,
            _primary_target,
            _primary_test,
            _blind_target,
            _blind_test,
            _expected_change,
            calibrated_edge,
            source_scope,
        ] = columns.as_slice()
        else {
            unreachable!("column count checked")
        };
        if !names.insert(*name) {
            return Err(format!("duplicate operator '{name}'"));
        }
        if !patches.insert(*patch) {
            return Err(format!("duplicate operator patch '{patch}'"));
        }
        if !root
            .join("rust/fslc/tests/fault_operators")
            .join(patch)
            .is_file()
        {
            return Err(format!("operator '{name}' patch '{patch}' is missing"));
        }
        let source = read_source(&root.join(seam_path))
            .map_err(|error| format!("operator '{name}' seam path is unreadable: {error}"))?;
        if source.matches(seam_anchor).count() != 1 {
            return Err(format!(
                "operator '{name}' seam anchor must occur exactly once in {seam_path}"
            ));
        }
        if calibrated_edge.trim().is_empty() {
            return Err(format!("operator '{name}' has no calibrated edge"));
        }
        if *source_scope == "p2 symbolic witness" || *source_scope == "p2 public projection" {
            p2_contracts.insert(columns[2]);
        }
    }

    for entry in std::fs::read_dir(root.join("rust/fslc/tests/fault_operators"))
        .map_err(|error| format!("read operator directory: {error}"))?
    {
        let path = entry.map_err(|error| error.to_string())?.path();
        if path.extension().and_then(|extension| extension.to_str()) == Some("patch") {
            let name = path
                .file_name()
                .and_then(|name| name.to_str())
                .ok_or_else(|| "operator patch filename is not UTF-8".to_owned())?;
            if !patches.contains(name) {
                return Err(format!("operator patch '{name}' is not inventoried"));
            }
        }
    }
    for required in REQUIRED_P2_FAULT_CLASSES {
        if !p2_contracts.contains(required) {
            return Err(format!(
                "missing required P2 operator contract '{required}'"
            ));
        }
    }
    Ok(())
}

fn gate_classification(outcome: &str, reviewed_equivalent: bool) -> Result<&'static str, String> {
    match (outcome, reviewed_equivalent) {
        ("caught", _) => Ok("killed"),
        ("unviable", _) => Ok("unbuildable"),
        ("missed", true) => Ok("reviewed_equivalent"),
        ("missed", false) => Err("non-equivalent survivor".to_owned()),
        ("timeout", _) => Err("incomplete mutation evidence: timeout".to_owned()),
        (other, _) => Err(format!("unknown mutation outcome '{other}'")),
    }
}

fn temporary_mutants_config_root() -> PathBuf {
    let nonce = SystemTime::now()
        .duration_since(UNIX_EPOCH)
        .expect("system clock after Unix epoch")
        .as_nanos();
    let root = std::env::temp_dir().join(format!(
        "fsl-mutants-config-test-{}-{nonce}",
        std::process::id()
    ));
    std::fs::create_dir_all(&root).expect("create temporary generator fixture");
    root
}

#[test]
fn multiline_anchor_matching_is_line_ending_independent() {
    let anchor = "if let Some(violation) = &result.violation {\n    replay(&violation.trace)?;\n}";
    let lf = format!("before\n{anchor}\nafter");
    let crlf = lf.replace('\n', "\r\n");
    let cr = lf.replace('\n', "\r");

    for source in [&lf, &crlf, &cr] {
        assert_eq!(normalize_line_endings(source).matches(anchor).count(), 1);
    }
}

#[test]
fn p2_critical_scope_and_equivalence_manifests_are_current() {
    let root = repository_root();
    let directory = root.join("rust/fslc/tests/implementation_mutation");
    validate_scope(&root, &read_json(&directory.join("scope.v1.json")))
        .expect("valid P2 critical scope");
    validate_equivalents(&root, &read_json(&directory.join("equivalents.v1.json")))
        .expect("valid reviewed equivalence manifest");
    validate_operator_inventory(
        &root,
        &std::fs::read_to_string(root.join("rust/fslc/tests/fault_operators/operators.txt"))
            .expect("read operator inventory"),
    )
    .expect("valid semantic operator inventory");
}

#[test]
fn generated_mutants_config_rejects_line_drift_and_stale_anchor() {
    let root = temporary_mutants_config_root();
    let source_path = root.join("rust/fsl-verifier/src/bmc.rs");
    std::fs::create_dir_all(source_path.parent().expect("source parent"))
        .expect("create source fixture parent");
    std::fs::write(&source_path, "before\nlet anchor = true;\nafter\n")
        .expect("write source fixture");
    let scope_path = root.join("rust/fslc/tests/implementation_mutation/scope.v1.json");
    std::fs::create_dir_all(scope_path.parent().expect("scope parent"))
        .expect("create scope fixture parent");
    let mut scope = serde_json::json!({
        "generic_exclusions": [{
            "id": "calibration.anchor",
            "path": "rust/fsl-verifier/src/bmc.rs",
            "function": "verify_bounded_config",
            "anchor": "let anchor = true;",
            "occurrence": 1,
            "expected_occurrences": 1,
            "reason": "calibration exclusion"
        }]
    });
    std::fs::write(
        &scope_path,
        serde_json::to_vec(&scope).expect("serialize scope fixture"),
    )
    .expect("write scope fixture");
    let template_path = root.join("rust/.cargo/mutants.toml.in");
    std::fs::create_dir_all(template_path.parent().expect("template parent"))
        .expect("create template fixture parent");
    std::fs::write(
        &template_path,
        "exclude_re = [\n{{GENERATED_EXCLUDE_RE}}\n]\n",
    )
    .expect("write template fixture");
    let rendered = mutants_config::render(&root).expect("render fresh fixture");
    assert!(rendered.contains(r"fsl-verifier/src/bmc\\.rs:2:"));
    std::fs::write(root.join("rust/.cargo/mutants.toml"), rendered)
        .expect("write generated fixture");
    mutants_config::check(&root).expect("fresh generated fixture");

    let config_path = root.join("rust/.cargo/mutants.toml");
    let config = std::fs::read_to_string(&config_path).expect("read generated fixture");
    std::fs::write(&config_path, config.replace('\n', "\r\n"))
        .expect("write CRLF generated fixture");
    mutants_config::check(&root).expect("CRLF generated fixture must be fresh");
    std::fs::write(
        &config_path,
        mutants_config::render(&root).expect("restore LF generated fixture"),
    )
    .expect("restore LF generated fixture");

    std::fs::write(
        &source_path,
        "let anchor = true;\nbefore\nlet anchor = true;\nafter\n",
    )
    .expect("insert duplicate anchor before selected anchor");
    let duplicate =
        mutants_config::check(&root).expect_err("duplicate anchor must fail generation");
    assert!(duplicate.contains("anchor occurrence count changed"));
    assert!(duplicate.contains("expected 1, actual 2"));

    std::fs::write(
        &source_path,
        "before\ninserted line\nlet anchor = true;\nafter\n",
    )
    .expect("shift source anchor");
    let drift = mutants_config::check(&root).expect_err("line drift must stale generated config");
    assert!(drift.contains("generated mutation runner configuration is stale"));
    assert!(drift.contains(mutants_config::regenerate_command()));
    assert!(drift.contains("@@ line 2 @@"));
    assert!(drift.contains("-  \"^fsl-verifier/src/bmc\\\\.rs:2:.* in verify_bounded_config$\","));
    assert!(drift.contains("+  \"^fsl-verifier/src/bmc\\\\.rs:3:.* in verify_bounded_config$\","));

    std::fs::write(&source_path, "before\nlet anchor = true;\nafter\n")
        .expect("restore source fixture");
    mutants_config::check(&root).expect("restored generated fixture");
    let config = std::fs::read_to_string(&config_path).expect("read generated fixture");
    std::fs::write(
        &config_path,
        config
            .strip_suffix('\n')
            .expect("generated fixture has trailing newline"),
    )
    .expect("remove generated fixture trailing newline");
    let eof = mutants_config::check(&root).expect_err("missing final newline must stale config");
    assert!(eof.contains("\\ No newline at end of file"));
    assert!(eof.contains("-]\n\\ No newline at end of file"));
    assert!(eof.contains("+]"));

    scope["generic_exclusions"][0]["anchor"] = serde_json::Value::String("stale anchor".to_owned());
    std::fs::write(
        &scope_path,
        serde_json::to_vec(&scope).expect("serialize stale scope fixture"),
    )
    .expect("write stale scope fixture");
    let stale_anchor = mutants_config::check(&root).expect_err("stale anchor must fail generation");
    assert!(stale_anchor.contains("anchor occurrence count changed"));
    assert!(stale_anchor.contains("expected 1, actual 0"));

    std::fs::remove_dir_all(&root).expect("remove temporary generator fixture");
}

#[test]
fn stale_ambiguous_or_unreasoned_exclusions_fail_closed() {
    let root = repository_root();
    let missing_rationale = serde_json::json!({
        "schema": "fslc.implementation-mutation-equivalents.v1",
        "schema_version": 1,
        "entries": [{
            "mutant_id": "exact-mutant-id",
            "path": "rust/fsl-verifier/src/bmc.rs",
            "anchor": "pub async fn verify_bounded<S: SmtSolver>(",
            "rationale": "",
            "reviewer": "reviewer:test",
            "review_issue": "#672"
        }]
    });
    assert!(validate_equivalents(&root, &missing_rationale).is_err());

    let stale = serde_json::json!({
        "schema": "fslc.implementation-mutation-equivalents.v1",
        "schema_version": 1,
        "entries": [{
            "mutant_id": "exact-mutant-id",
            "path": "rust/fsl-verifier/src/bmc.rs",
            "anchor": "a semantic decision that no longer exists",
            "rationale": "reviewed semantic equivalence",
            "reviewer": "reviewer:test",
            "review_issue": "#672"
        }]
    });
    assert!(validate_equivalents(&root, &stale).is_err());

    let ambiguous = serde_json::json!({
        "schema": "fslc.implementation-mutation-equivalents.v1",
        "schema_version": 1,
        "entries": [{
            "mutant_id": "exact-mutant-id",
            "path": "rust/fsl-verifier/src/bmc.rs",
            "anchor": "return Ok(result);",
            "rationale": "reviewed semantic equivalence",
            "reviewer": "reviewer:test",
            "review_issue": "#672"
        }]
    });
    assert!(validate_equivalents(&root, &ambiguous).is_err());
}

#[test]
fn mutation_detector_selection_requires_explicit_package() {
    let root = repository_root();
    let config = std::fs::read_to_string(root.join("rust/.cargo/mutants.toml"))
        .expect("read mutation runner config");
    validate_detector_selection(&config).expect("valid detector selection");
    assert!(
        validate_detector_selection(&config.replace(r#""--package", "fslc-rust", "#, "")).is_err()
    );
}

#[test]
fn survivor_timeout_and_unknown_outcomes_fail_closed() {
    assert_eq!(gate_classification("caught", false), Ok("killed"));
    assert_eq!(gate_classification("unviable", false), Ok("unbuildable"));
    assert_eq!(
        gate_classification("missed", true),
        Ok("reviewed_equivalent")
    );
    assert!(gate_classification("missed", false).is_err());
    assert!(gate_classification("timeout", false).is_err());
    assert!(gate_classification("new-tool-outcome", false).is_err());
}

#[test]
fn report_schema_accepts_complete_replayable_evidence_and_rejects_omissions() {
    let root = repository_root();
    let schema = read_json(
        &root.join("schemas/fslc/assurance/implementation-mutation-report.v1.schema.json"),
    );
    let validator = jsonschema::options()
        .build(&schema)
        .expect("compile implementation mutation report schema");
    let report = serde_json::json!({
        "schema": "fslc.implementation-mutation-report.v1",
        "schema_version": 1,
        "base_revision": "0123456789abcdef",
        "diff_scope": {"mode": "complete", "base": null, "paths": ["rust/fsl-verifier/src/bmc.rs"]},
        "runner": {"name": "cargo-mutants", "version": "27.1.0"},
        "configuration": "rust/.cargo/mutants.toml",
        "complete": true,
        "mutants": [{
            "id": "p2.bmc.rs:127:replace-verify-bounded-config",
            "source_decision_anchor": "p2.bmc.result-classification",
            "mutation": "replace verify_bounded_config with Ok(Default::default())",
            "test_command": ["cargo", "test", "-p", "fslc-rust"],
            "classification": "killed",
            "elapsed_ms": 1,
            "primary_failing_test": "p2_cli_observation_preserves_full_witness_identity",
            "reproducer": "cargo mutants --config .cargo/mutants.toml --re exact-mutant-id",
            "reviewed_rationale": null
        }]
    });
    validator
        .validate(&report)
        .expect("complete report is valid");

    let mut detector_omitted = report.clone();
    detector_omitted["mutants"][0]["primary_failing_test"] = Value::Null;
    assert!(validator.validate(&detector_omitted).is_err());

    let mut replay_omitted = report.clone();
    replay_omitted["mutants"][0]
        .as_object_mut()
        .expect("mutant row")
        .remove("reproducer");
    assert!(validator.validate(&replay_omitted).is_err());

    let mut incomplete = report;
    incomplete
        .as_object_mut()
        .expect("report object")
        .remove("base_revision");
    assert!(validator.validate(&incomplete).is_err());
}

#[test]
fn emitted_report_matches_schema_when_requested() {
    let Some(path) = std::env::var_os("FSL_IMPLEMENTATION_MUTATION_REPORT") else {
        return;
    };
    let root = repository_root();
    let schema = read_json(
        &root.join("schemas/fslc/assurance/implementation-mutation-report.v1.schema.json"),
    );
    let report = read_json(Path::new(&path));
    jsonschema::options()
        .build(&schema)
        .expect("compile implementation mutation report schema")
        .validate(&report)
        .expect("emitted implementation mutation report matches schema");
    let scope = read_json(&root.join("rust/fslc/tests/implementation_mutation/scope.v1.json"));
    let decision_ids = scope["decisions"]
        .as_array()
        .expect("scope decisions")
        .iter()
        .map(|decision| decision["id"].as_str().expect("decision ID"))
        .collect::<BTreeSet<_>>();
    for mutant in report["mutants"].as_array().expect("reported mutants") {
        let anchor = mutant["source_decision_anchor"]
            .as_str()
            .expect("reported source decision anchor");
        assert!(
            decision_ids.contains(anchor),
            "reported mutant does not map to a maintained decision: {anchor}"
        );
    }
    assert_eq!(
        report.get("complete").and_then(Value::as_bool),
        Some(true),
        "an interrupted or partial raw run must not produce accepted evidence"
    );
}
