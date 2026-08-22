// SPDX-License-Identifier: Apache-2.0

use std::path::{Path, PathBuf};
use std::process::{Command, Output};

use serde_json::{Value, json};

fn repository_root() -> PathBuf {
    Path::new(env!("CARGO_MANIFEST_DIR"))
        .parent()
        .and_then(Path::parent)
        .expect("repository root")
        .to_owned()
}

fn run(arguments: &[&str]) -> Output {
    Command::new(env!("CARGO_BIN_EXE_fslc"))
        .args(arguments)
        .current_dir(repository_root())
        .output()
        .expect("run native fslc")
}

struct CacheDir {
    path: PathBuf,
}

impl CacheDir {
    fn new(name: &str) -> Self {
        let path = std::env::temp_dir().join(format!(
            "fslc-verification-ownership-cache-{name}-{}",
            std::process::id()
        ));
        let _ = std::fs::remove_dir_all(&path);
        Self { path }
    }

    fn run(&self) -> (Value, i32) {
        self.run_args(&[
            "verify",
            "examples/gallery/valid/tiny_turnstile.fsl",
            "--engine",
            "bmc",
            "--depth",
            "2",
            "--deadlock",
            "ignore",
        ])
    }

    fn run_args(&self, arguments: &[&str]) -> (Value, i32) {
        let output = Command::new(env!("CARGO_BIN_EXE_fslc"))
            .args(arguments)
            .current_dir(repository_root())
            .env("FSLC_CACHE_DIR", &self.path)
            .output()
            .expect("run cached native fslc");
        (
            serde_json::from_slice(&output.stdout).expect("verification JSON"),
            output.status.code().expect("native exit status"),
        )
    }

    fn exact_entry(&self) -> PathBuf {
        let root = self.path.join("verify/v2");
        std::fs::read_dir(root)
            .expect("read cache root")
            .filter_map(Result::ok)
            .filter(|entry| entry.file_name() != "xdepth")
            .flat_map(|entry| {
                std::fs::read_dir(entry.path())
                    .expect("read cache shard")
                    .filter_map(Result::ok)
            })
            .map(|entry| entry.path())
            .find(|path| {
                path.extension()
                    .is_some_and(|extension| extension == "json")
            })
            .expect("exact cache entry")
    }
}

impl Drop for CacheDir {
    fn drop(&mut self) {
        let _ = std::fs::remove_dir_all(&self.path);
    }
}

#[test]
fn verification_rejects_an_unknown_engine_with_the_stable_usage_contract() {
    let output = run(&[
        "verify",
        "examples/gallery/valid/tiny_turnstile.fsl",
        "--engine",
        "unknown",
        "--no-cache",
    ]);
    assert_eq!(output.status.code(), Some(2));
    let value: Value = serde_json::from_slice(&output.stdout).expect("usage JSON");
    assert_eq!(value["result"], "error");
    assert_eq!(value["kind"], "usage");
    assert_eq!(
        value["message"],
        "--engine must be bmc, induction, explicit, or auto"
    );
}

fn verification_json(arguments: &[&str]) -> (Value, i32) {
    let output = run(arguments);
    (
        serde_json::from_slice(&output.stdout).unwrap_or_else(|error| {
            panic!(
                "invalid verification JSON: {error}; stderr={}",
                String::from_utf8_lossy(&output.stderr)
            )
        }),
        output.status.code().expect("native exit status"),
    )
}

#[test]
fn selected_transition_induction_matches_the_all_properties_proof() {
    let fixture = "rust/fslc/tests/fixtures/issue_701_induction_trans_hypothesis.fsl";
    let common = [
        "--engine",
        "induction",
        "--depth",
        "2",
        "--deadlock",
        "ignore",
        "--no-cache",
    ];
    let mut all_arguments = vec!["verify", fixture];
    all_arguments.extend(common);
    let (all, all_status) = verification_json(&all_arguments);

    let mut selected_arguments = vec!["verify", fixture, "--property", "StartsZero"];
    selected_arguments.extend(common);
    let (selected, selected_status) = verification_json(&selected_arguments);

    assert_eq!(all_status, 0, "{all}");
    assert_eq!(
        selected_status, all_status,
        "selected={selected}; all={all}"
    );
    assert_eq!(selected["result"], all["result"]);
    assert_eq!(selected["result"], "proved");
    assert_eq!(selected["engine"], "induction");
    assert_eq!(
        selected["transitions_checked"],
        json!(["StartsZero"]),
        "{selected}"
    );
    assert_eq!(selected["transitions_checked"], all["transitions_checked"]);
    assert_eq!(
        selected["invariants_checked"],
        json!(["_bounds_x", "Zero"]),
        "the selected transition relies on the full invariant hypothesis: {selected}"
    );
}

#[test]
fn selected_transition_keeps_bounds_without_checking_sibling_transitions() {
    let (selected, status) = verification_json(&[
        "verify",
        "rust/fslc/tests/fixtures/issue_701_induction_trans_siblings.fsl",
        "--engine",
        "induction",
        "--property",
        "StartsZero",
        "--depth",
        "2",
        "--deadlock",
        "ignore",
        "--no-cache",
    ]);
    assert_eq!(status, 0, "{selected}");
    assert_eq!(selected["result"], "proved");
    assert_eq!(selected["transitions_checked"], json!(["StartsZero"]));
    assert_eq!(selected["invariants_checked"], json!(["_bounds_x", "Zero"]));
    assert!(
        selected["cost"]["properties"]
            .as_array()
            .is_some_and(|properties| properties.iter().any(|property| {
                property["kind"] == "type_bound" && property["name"] == "_bounds_x"
            })),
        "selected transition must retain its implicit type bound: {selected}"
    );
}

#[test]
fn selected_transition_exclusion_still_wins() {
    let (selected, status) = verification_json(&[
        "verify",
        "rust/fslc/tests/fixtures/issue_701_induction_trans_hypothesis.fsl",
        "--engine",
        "induction",
        "--property",
        "StartsZero",
        "--exclude-property",
        "StartsZero",
        "--depth",
        "2",
        "--deadlock",
        "ignore",
        "--no-cache",
    ]);
    assert_eq!(status, 0, "{selected}");
    assert_eq!(selected["result"], "proved");
    assert_eq!(selected["transitions_checked"], json!([]));
}

#[test]
fn selected_transition_lemma_path_honors_scope_overrides() {
    let (selected, status) = verification_json(&[
        "verify",
        "rust/fslc/tests/fixtures/issue_701_induction_trans_scope.fsl",
        "--engine",
        "induction",
        "--property",
        "StableRange",
        "--lemma",
        "true",
        "--values",
        "Count=0..0",
        "--depth",
        "2",
        "--deadlock",
        "ignore",
        "--no-cache",
    ]);
    assert_eq!(status, 0, "{selected}");
    assert_eq!(selected["result"], "proved");
    assert_eq!(selected["transitions_checked"], json!(["StableRange"]));
    assert_eq!(
        selected["bounds_overrides"]["values"]["Count"],
        json!([0, 0])
    );

    let (invalid, invalid_status) = verification_json(&[
        "verify",
        "rust/fslc/tests/fixtures/issue_701_induction_trans_hypothesis.fsl",
        "--engine",
        "induction",
        "--property",
        "StartsZero",
        "--lemma",
        "true",
        "--values",
        "Count=0..0",
        "--no-cache",
    ]);
    assert_eq!(invalid_status, 2, "{invalid}");
    assert_eq!(invalid["result"], "error");
}

#[test]
fn inductive_transition_property_row_reports_proved_assurance() {
    let output_dir =
        std::env::temp_dir().join(format!("fslc-issue-701-assurance-{}", std::process::id()));
    let _ = std::fs::remove_dir_all(&output_dir);
    std::fs::create_dir_all(&output_dir).expect("create issue 701 output directory");
    let output_path = output_dir.join("report.html");
    let output_path = output_path.to_string_lossy().into_owned();
    let output = run(&[
        "html",
        "rust/fslc/tests/fixtures/issue_701_induction_trans_hypothesis.fsl",
        "--engine",
        "induction",
        "-o",
        &output_path,
    ]);
    assert_eq!(output.status.code(), Some(0));
    let envelope: Value = serde_json::from_slice(&output.stdout).expect("html result JSON");
    assert_eq!(envelope["result"], "generated", "{envelope}");
    let html = std::fs::read_to_string(&output_path).expect("read HTML report");
    let row = html
        .split("<tr>")
        .find(|row| row.contains("<code>StartsZero</code>"))
        .expect("StartsZero property row");
    assert!(row.contains("<td>proved(induction)</td>"), "{row}");
    std::fs::remove_dir_all(output_dir).expect("remove issue 701 output directory");
}

#[test]
fn selected_invariant_induction_keeps_its_existing_isolated_model_semantics() {
    let (selected, status) = verification_json(&[
        "verify",
        "rust/fslc/tests/fixtures/explicit_property_narrow.fsl",
        "--engine",
        "induction",
        "--property",
        "AlwaysBoolean",
        "--depth",
        "2",
        "--deadlock",
        "ignore",
        "--no-cache",
    ]);
    assert_eq!(status, 0, "{selected}");
    assert_eq!(selected["result"], "proved");
    assert_eq!(selected["invariants_checked"], json!(["AlwaysBoolean"]));
}

#[test]
fn selected_transition_preserves_bmc_violation_and_induction_cti_contracts() {
    let (violated, violated_status) = verification_json(&[
        "verify",
        "rust/fslc/tests/fixtures/assurance_trans_violation.fsl",
        "--engine",
        "induction",
        "--property",
        "NeverDecrease",
        "--depth",
        "3",
        "--deadlock",
        "ignore",
        "--no-cache",
    ]);
    assert_eq!(violated_status, 1, "{violated}");
    assert_eq!(violated["result"], "violated");
    assert_eq!(violated["violation_kind"], "trans");
    assert_eq!(violated["trans"], "NeverDecrease");

    let (cti, cti_status) = verification_json(&[
        "verify",
        "rust/fslc/tests/fixtures/induction_suggestion_trans.fsl",
        "--engine",
        "induction",
        "--property",
        "NeverIncrease",
        "--depth",
        "1",
        "--deadlock",
        "ignore",
        "--no-cache",
    ]);
    assert_eq!(cti_status, 1, "{cti}");
    assert_eq!(cti["result"], "unknown_cti");
    assert_eq!(cti["trans"], "NeverIncrease");
    assert_eq!(cti["trace_type"], "induction_cti");
    assert!(
        cti.get("violation_kind").is_none(),
        "preserve the existing transition CTI envelope: {cti}"
    );
}

#[test]
fn induction_property_selection_keeps_unknown_and_unsupported_kinds_rejected() {
    let cases = [
        (
            "rust/fslc/tests/fixtures/explicit_reachable_witnessed.fsl",
            "HitTwo",
            "reachable",
        ),
        (
            "rust/fslc/tests/fixtures/testgen_leadsto_violation.fsl",
            "Served",
            "leadsTo",
        ),
    ];
    for (fixture, property, kind) in cases {
        for lemma in [None, Some("true")] {
            let mut arguments = vec![
                "verify",
                fixture,
                "--engine",
                "induction",
                "--property",
                property,
                "--no-cache",
            ];
            if let Some(lemma) = lemma {
                arguments.extend(["--lemma", lemma]);
            }
            let (output, status) = verification_json(&arguments);
            assert_eq!(status, 2, "{output}");
            assert_eq!(output["result"], "error");
            assert_eq!(output["kind"], "usage");
            assert_eq!(
                output["message"],
                format!(
                    "--property {property} is a {kind}, which the induction engine cannot prove; check it with the default bmc engine"
                )
            );
        }
    }

    for lemma in [None, Some("true")] {
        let mut arguments = vec![
            "verify",
            "rust/fslc/tests/fixtures/issue_701_induction_trans_hypothesis.fsl",
            "--engine",
            "induction",
            "--property",
            "MissingProperty",
            "--no-cache",
        ];
        if let Some(lemma) = lemma {
            arguments.extend(["--lemma", lemma]);
        }
        let (unknown, unknown_status) = verification_json(&arguments);
        assert_eq!(unknown_status, 2, "{unknown}");
        assert_eq!(unknown["result"], "error");
        assert_eq!(unknown["kind"], "usage");
        assert!(
            unknown["message"]
                .as_str()
                .is_some_and(|message| message.starts_with("no such property: MissingProperty")),
            "{unknown}"
        );
    }
}

#[test]
fn native_verification_has_explicit_dependencies_and_no_renderer_copy() {
    let source = include_str!("../src/verification.rs");
    let production = source
        .split("#[cfg(test)]")
        .next()
        .expect("production verification source");
    assert!(!production.contains("use super::*;"));
    for symbol in [
        "fn render_bmc_result(",
        "fn render_bmc_violation(",
        "fn render_reachable_failure(",
        "fn render_deadlock_failure(",
        "fn render_bmc_success(",
        "fn render_explicit_result(",
        "fn shared_warnings(",
        "fn add_common_verification(",
    ] {
        assert!(
            !production.contains(symbol),
            "native owner retained {symbol}"
        );
    }
    assert!(production.contains("verification_output::render_bmc_output("));
    assert!(production.contains("verification_output::render_explicit_output("));
}

#[test]
fn malformed_exact_cache_outputs_are_misses_not_green_hits() {
    let cache = CacheDir::new("malformed");
    let (first, first_status) = cache.run();
    assert_eq!(first_status, 0);
    assert!(first.get("cache").is_none());

    let entry_path = cache.exact_entry();
    let mut entry: Value =
        serde_json::from_slice(&std::fs::read(&entry_path).expect("read cache entry"))
            .expect("cache entry JSON");
    entry["output"] = Value::Object(serde_json::Map::new());
    std::fs::write(
        &entry_path,
        serde_json::to_vec(&entry).expect("serialize empty cache output"),
    )
    .expect("write empty cache output");
    let (empty, empty_status) = cache.run();
    assert_eq!(empty_status, 0);
    assert!(empty.get("cache").is_none());

    let mut entry: Value =
        serde_json::from_slice(&std::fs::read(&entry_path).expect("read refreshed cache entry"))
            .expect("refreshed cache entry JSON");
    entry["output"]["result"] = Value::String("bogus".to_owned());
    std::fs::write(
        &entry_path,
        serde_json::to_vec(&entry).expect("serialize bogus cache output"),
    )
    .expect("write bogus cache output");
    let (bogus, bogus_status) = cache.run();
    assert_eq!(bogus_status, 0);
    assert!(bogus.get("cache").is_none());
}

#[test]
fn leadsto_violations_remain_exact_cache_hits() {
    let cache = CacheDir::new("leadsto");
    let arguments = [
        "verify",
        "examples/gallery/errors/violated_leads_to_starvation.fsl",
        "--depth",
        "8",
        "--deadlock",
        "ignore",
    ];
    let (first, first_status) = cache.run_args(&arguments);
    assert_eq!(first_status, 1);
    assert_eq!(first["violation_kind"], "leadsTo");
    assert!(first.get("cache").is_none());

    let (second, second_status) = cache.run_args(&arguments);
    assert_eq!(second_status, 1);
    assert_eq!(second["violation_kind"], "leadsTo");
    assert_eq!(second["cache"]["hit"], true);
    assert_eq!(second["cache"]["source"], "exact");
}

#[test]
fn moved_explicit_render_branches_match_the_normalized_raw_golden() {
    let finite = "rust/fslc/tests/fixtures/explicit_finite_toggle.fsl";
    let cases = [
        ("bounded_success", finite, "0", "ignore", None, 0),
        ("closure_proof", finite, "4", "ignore", None, 0),
        ("violation", "specs/cart_buggy.fsl", "8", "ignore", None, 1),
        ("budget", finite, "4", "ignore", Some("1"), 1),
        (
            "deadlock",
            "rust/fslc/tests/fixtures/explicit_deadlock.fsl",
            "4",
            "error",
            None,
            1,
        ),
        (
            "reachable_bounded",
            "rust/fslc/tests/fixtures/explicit_reachable_unreachable_goal.fsl",
            "2",
            "ignore",
            None,
            1,
        ),
        (
            "reachable_closed",
            "rust/fslc/tests/fixtures/explicit_reachable_unreachable_goal.fsl",
            "6",
            "ignore",
            None,
            1,
        ),
    ];
    let mut rendered = Vec::new();
    for (name, path, depth, deadlock, budget, expected_status) in cases {
        let mut arguments = vec![
            "verify",
            path,
            "--engine",
            "explicit",
            "--depth",
            depth,
            "--deadlock",
            deadlock,
            "--no-cache",
        ];
        if let Some(budget) = budget {
            arguments.extend(["--explicit-budget", budget]);
        }
        let output = run(&arguments);
        assert_eq!(output.status.code(), Some(expected_status), "case {name}");
        let mut value: Value = serde_json::from_slice(&output.stdout).expect("verification JSON");
        let envelope = value.as_object_mut().expect("verification envelope");
        envelope.remove("versions");
        envelope.remove("cache");
        envelope
            .get_mut("cost")
            .and_then(Value::as_object_mut)
            .expect("verification cost")
            .insert("elapsed_s".to_owned(), json!(0.0));
        rendered.push(json!({"case": name, "status": expected_status, "output": value}));
    }
    let actual = format!(
        "{}\n",
        serde_json::to_string(&rendered).expect("serialize normalized raw outputs")
    );
    if std::env::var_os("FSLC_DUMP_VERIFICATION_GOLDEN").is_some() {
        eprintln!("{actual}");
    }
    if std::env::var_os("FSLC_UPDATE_VERIFICATION_GOLDEN").is_some() {
        let golden = Path::new(env!("CARGO_MANIFEST_DIR"))
            .join("tests/goldens/verification_explicit_rendering.json");
        std::fs::write(golden, &actual).expect("update verification golden");
        return;
    }
    let expected = str::from_utf8(include_bytes!(
        "goldens/verification_explicit_rendering.json"
    ))
    .expect("UTF-8 verification golden")
    .replace("\r\n", "\n");
    assert_eq!(actual, expected);
}
