// SPDX-License-Identifier: Apache-2.0
// Copyright 2026 Ryoichi Izumita

use std::path::{Path, PathBuf};
use std::process::Command;

use serde_json::json;

const BUDGET: &str = "1000000";

fn repository_root() -> PathBuf {
    Path::new(env!("CARGO_MANIFEST_DIR"))
        .parent()
        .and_then(Path::parent)
        .expect("repository root")
        .to_path_buf()
}

fn run_cli(arguments: &[String]) -> (serde_json::Value, i32) {
    let output = Command::new(env!("CARGO_BIN_EXE_fslc"))
        .args(arguments)
        .current_dir(repository_root())
        .output()
        .expect("run native fslc");
    let value = serde_json::from_slice(&output.stdout).unwrap_or_else(|error| {
        panic!(
            "invalid JSON: {error}; args={arguments:?}; stderr={}",
            String::from_utf8_lossy(&output.stderr)
        )
    });
    (value, output.status.code().expect("native exit status"))
}

fn verify(path: &str, engine: &str, depth: usize, extra: &[&str]) -> (serde_json::Value, i32) {
    let mut arguments = vec![
        "verify".to_owned(),
        path.to_owned(),
        "--engine".to_owned(),
        engine.to_owned(),
        "--depth".to_owned(),
        depth.to_string(),
        "--deadlock".to_owned(),
        "ignore".to_owned(),
        "--no-cache".to_owned(),
    ];
    arguments.extend(extra.iter().map(|argument| (*argument).to_owned()));
    run_cli(&arguments)
}

fn load_model(path: &Path) -> fsl_core::KernelModel {
    let source = std::fs::read_to_string(path).expect("read FSL corpus file");
    let resolver = fsl_core::FsResolver::new(path.parent().expect("spec parent"));
    let kernel = fsl_core::parse_kernel_source(&source, &resolver).expect("lower kernel");
    fsl_core::build_model(kernel).expect("build kernel model")
}

#[test]
#[allow(clippy::too_many_lines)]
fn explicit_cli_exit_codes_cover_bounded_proved_violated_budget_and_semantics() {
    let finite =
        Path::new(env!("CARGO_MANIFEST_DIR")).join("tests/fixtures/explicit_finite_toggle.fsl");
    let finite = finite.to_str().expect("UTF-8 fixture path");
    let (bounded, status) = verify(finite, "explicit", 0, &[]);
    assert_eq!(status, 0);
    assert_eq!(bounded["result"], "verified");
    assert_eq!(bounded["completeness"], "bounded");

    let (proved, status) = verify(finite, "explicit", 4, &[]);
    assert_eq!(status, 0);
    assert_eq!(proved["result"], "proved");
    assert_eq!(proved["engine"], "explicit");
    assert_eq!(proved["completeness"], "unbounded");
    assert_eq!(proved["closure"], true);
    assert_eq!(proved["cost"]["solver"]["checks"], 0);
    assert_eq!(proved["cost"]["solver"]["conflicts"], json!(null));
    assert_eq!(proved["cost"]["properties"], json!([]));

    let (ignored_k, status) = verify(finite, "explicit", 4, &["--k", "99"]);
    assert_eq!(status, 0);
    assert_eq!(ignored_k["result"], "proved");

    let (lemma_rejected, status) =
        verify(finite, "explicit", 4, &["--lemma", "active or not active"]);
    assert_eq!(status, 2);
    assert_eq!(lemma_rejected["kind"], "usage");

    let (snapshot_rejected, status) = verify(
        finite,
        "explicit",
        4,
        &["--from-state", "missing-state.json"],
    );
    assert_eq!(status, 2);
    assert_eq!(snapshot_rejected["kind"], "semantics");
    assert!(
        snapshot_rejected["message"]
            .as_str()
            .is_some_and(|message| message.contains("only with the BMC engine"))
    );

    let (unsupported_liveness, status) = verify("specs/mutex_queue.fsl", "explicit", 8, &[]);
    assert_eq!(status, 2);
    assert_eq!(unsupported_liveness["kind"], "semantics");
    assert!(
        unsupported_liveness["message"]
            .as_str()
            .is_some_and(|message| message.contains("does not support leadsTo"))
    );

    let (violated, status) = verify("specs/cart_buggy.fsl", "explicit", 8, &[]);
    assert_eq!(status, 1);
    assert_eq!(violated["result"], "violated");

    let (budget, status) = verify(finite, "explicit", 4, &["--explicit-budget", "1"]);
    assert_eq!(status, 1);
    assert_eq!(budget["result"], "unknown_budget");
    assert_eq!(budget["states_explored"], 1);
    assert_eq!(budget["depth_reached"], 0);

    let fixture = Path::new(env!("CARGO_MANIFEST_DIR"))
        .join("tests/fixtures/explicit_nondeterministic_init.fsl");
    let (rejected, status) = verify(
        fixture.to_str().expect("UTF-8 fixture path"),
        "explicit",
        4,
        &[],
    );
    assert_eq!(status, 2);
    assert_eq!(rejected["result"], "error");
    assert_eq!(rejected["kind"], "semantics");
    assert!(rejected["message"].as_str().is_some_and(|message| {
        message.contains("init does not assign state variable(s): omitted")
    }));

    let deadlock =
        Path::new(env!("CARGO_MANIFEST_DIR")).join("tests/fixtures/explicit_deadlock.fsl");
    let deadlock = deadlock.to_str().expect("UTF-8 fixture path");
    let (warning, status) = run_cli(&[
        "verify".to_owned(),
        deadlock.to_owned(),
        "--engine".to_owned(),
        "explicit".to_owned(),
        "--depth".to_owned(),
        "4".to_owned(),
        "--deadlock".to_owned(),
        "warn".to_owned(),
        "--no-cache".to_owned(),
    ]);
    assert_eq!(status, 0);
    assert!(warning["warnings"].as_array().is_some_and(|warnings| {
        warnings.iter().any(|warning| {
            warning["message"]
                .as_str()
                .is_some_and(|message| message.contains("deadlock reachable"))
        })
    }));

    let (deadlock_error, status) = run_cli(&[
        "verify".to_owned(),
        deadlock.to_owned(),
        "--engine".to_owned(),
        "explicit".to_owned(),
        "--depth".to_owned(),
        "4".to_owned(),
        "--deadlock".to_owned(),
        "error".to_owned(),
        "--no-cache".to_owned(),
    ]);
    assert_eq!(status, 1);
    assert_eq!(deadlock_error["violation_kind"], "deadlock");

    let vacuous = Path::new(env!("CARGO_MANIFEST_DIR")).join("tests/fixtures/explicit_vacuous.fsl");
    let (vacuous_error, status) = verify(
        vacuous.to_str().expect("UTF-8 fixture path"),
        "explicit",
        4,
        &["--vacuity", "error"],
    );
    assert_eq!(status, 2);
    assert_eq!(vacuous_error["result"], "error");
    assert_eq!(vacuous_error["kind"], "vacuous_implication");
}

#[test]
fn explicit_init_forall_domains_match_bmc_semantics_errors() {
    let fixtures = Path::new(env!("CARGO_MANIFEST_DIR")).join("tests/fixtures");
    for (name, expected_message) in [
        (
            "explicit_init_range_unassigned.fsl",
            "init forall range bounds must be compile-time constants; state variable 'n' is not allowed",
        ),
        (
            "explicit_init_range_assigned.fsl",
            "init forall range bounds must be compile-time constants; state variable 'n' is not allowed",
        ),
        (
            "explicit_init_state_collection.fsl",
            "init forall over a state collection is not supported; state variable 's' is not allowed",
        ),
    ] {
        let path = fixtures.join(name);
        let path = path.to_str().expect("UTF-8 fixture path");

        let (explicit, explicit_status) = verify(path, "explicit", 4, &[]);
        assert_eq!(explicit_status, 2, "explicit accepted {name}");
        assert_eq!(explicit["result"], "error", "explicit result for {name}");
        assert_eq!(explicit["kind"], "semantics", "explicit kind for {name}");
        assert_eq!(
            explicit["message"], expected_message,
            "explicit diagnostic for {name}"
        );

        let (bmc, bmc_status) = verify(path, "bmc", 4, &[]);
        assert_eq!(bmc_status, 2, "BMC accepted {name}: {bmc:#}");
        assert_eq!(bmc["result"], "error", "BMC result for {name}");
        assert_eq!(bmc["kind"], "semantics", "BMC kind for {name}");
    }

    let const_bound = fixtures.join("explicit_init_const_range.fsl");
    let const_bound = const_bound.to_str().expect("UTF-8 fixture path");
    let (explicit, explicit_status) = verify(const_bound, "explicit", 4, &[]);
    assert_eq!(explicit_status, 0);
    assert_eq!(explicit["result"], "proved");
    assert_eq!(explicit["closure"], true);

    let (bmc, bmc_status) = verify(const_bound, "bmc", 4, &[]);
    assert_eq!(bmc_status, 0);
    assert_eq!(bmc["result"], "verified");
}

#[test]
fn explicit_closure_proves_true_noninductive_invariant() {
    let fixture =
        Path::new(env!("CARGO_MANIFEST_DIR")).join("tests/fixtures/explicit_noninductive.fsl");
    let fixture = fixture.to_str().expect("UTF-8 fixture path");

    let (induction, induction_status) = verify(fixture, "induction", 8, &[]);
    assert_eq!(induction_status, 1);
    assert_eq!(induction["result"], "unknown_cti");

    let (explicit, explicit_status) = verify(fixture, "explicit", 8, &[]);
    assert_eq!(explicit_status, 0);
    assert_eq!(explicit["result"], "proved");
    assert_eq!(explicit["closure"], true);
}

#[test]
fn explicit_and_bmc_agree_on_every_accepted_top_level_corpus_spec() {
    let root = repository_root();
    let mut paths = std::fs::read_dir(root.join("specs"))
        .expect("read specs corpus")
        .map(|entry| entry.expect("corpus entry").path())
        .filter(|path| path.extension().and_then(std::ffi::OsStr::to_str) == Some("fsl"))
        .collect::<Vec<_>>();
    paths.sort();

    let mut accepted = 0_usize;
    let mut replayed = 0_usize;
    for path in paths {
        let relative = path
            .strip_prefix(&root)
            .expect("repository-relative spec")
            .to_string_lossy()
            .into_owned();
        let (explicit, explicit_status) =
            verify(&relative, "explicit", 8, &["--explicit-budget", BUDGET]);
        if explicit_status == 2 && explicit["result"] == "error" {
            continue;
        }
        accepted += 1;
        assert_ne!(
            explicit["result"], "unknown_budget",
            "default budget truncated {relative}"
        );

        let (bmc, bmc_status) = verify(&relative, "bmc", 8, &[]);
        let explicit_result = explicit["result"].as_str().expect("explicit result");
        let normalized_explicit = if explicit_result == "proved" {
            "verified"
        } else {
            explicit_result
        };
        assert_eq!(
            normalized_explicit,
            bmc["result"].as_str().expect("BMC result"),
            "verdict mismatch for {relative}: explicit={explicit:#} bmc={bmc:#}"
        );
        assert_eq!(
            explicit_status, bmc_status,
            "exit-code mismatch for {relative}"
        );

        if explicit_result == "violated" {
            assert_eq!(
                explicit["violated_at_step"], bmc["violated_at_step"],
                "violation depth mismatch for {relative}"
            );
            let model = load_model(&path);
            let result = fsl_runtime::verify_explicit(model.clone(), 8, 1_000_000)
                .expect("accepted explicit model");
            let violation = result.violation.expect("explicit violation trace");
            fsl_runtime::replay_trace(model, &violation.trace)
                .unwrap_or_else(|error| panic!("trace did not replay for {relative}: {error}"));
            replayed += 1;
        }
    }

    assert!(accepted > 0, "explicit engine accepted no corpus specs");
    assert!(
        replayed > 0,
        "corpus contained no replayed explicit violation"
    );
}

fn fixture_path(name: &str) -> String {
    Path::new(env!("CARGO_MANIFEST_DIR"))
        .join("tests/fixtures")
        .join(name)
        .to_str()
        .expect("UTF-8 fixture path")
        .to_owned()
}

#[test]
fn explicit_rejects_partial_component_init_coverage() {
    // A single concrete map-key write only covers that key; the rest of the
    // map's key domain (here `B`) is left uninitialized. Root-granularity
    // tracking would have (unsoundly) treated the whole map as assigned as
    // soon as any one key was written.
    let (result, status) = verify(
        &fixture_path("explicit_partial_map_key.fsl"),
        "explicit",
        4,
        &[],
    );
    assert_eq!(status, 2);
    assert_eq!(result["result"], "error");
    assert_eq!(result["kind"], "semantics");
    assert!(result["message"].as_str().is_some_and(|message| {
        message.contains("init does not assign state variable(s): values")
            && message.contains("partial component initialization is rejected")
    }));

    // A forall over a const-bounded subrange (0..1) leaves key 2 of the
    // 0..2 domain uncovered.
    let (result, status) = verify(
        &fixture_path("explicit_partial_subrange.fsl"),
        "explicit",
        4,
        &[],
    );
    assert_eq!(status, 2);
    assert_eq!(result["result"], "error");
    assert_eq!(result["kind"], "semantics");
    assert!(result["message"].as_str().is_some_and(|message| {
        message.contains("init does not assign state variable(s): m")
            && message.contains("partial component initialization is rejected")
    }));

    // A where-filtered forall can skip iterations at runtime, so it can
    // never be used to prove full coverage even though every reachable
    // binding happens to satisfy the filter here.
    let (result, status) = verify(
        &fixture_path("explicit_partial_where.fsl"),
        "explicit",
        4,
        &[],
    );
    assert_eq!(status, 2);
    assert_eq!(result["result"], "error");
    assert_eq!(result["kind"], "semantics");
    assert!(
        result["message"]
            .as_str()
            .is_some_and(|message| message.contains("init does not assign state variable(s): m"))
    );
}

#[test]
fn explicit_accepts_map_fully_covered_by_separate_concrete_key_statements() {
    // The fsl-db "per-column" pattern: distinct concrete-key writes that
    // together cover the whole enum key domain must count as full coverage.
    let (result, status) = verify(
        &fixture_path("explicit_full_enum_keys.fsl"),
        "explicit",
        4,
        &[],
    );
    assert_eq!(status, 0);
    assert!(matches!(
        result["result"].as_str(),
        Some("proved" | "verified")
    ));
}

#[test]
fn explicit_reachable_witness_step_matches_bmc() {
    let path = fixture_path("explicit_reachable_witnessed.fsl");
    let (explicit, explicit_status) = verify(&path, "explicit", 4, &[]);
    assert_eq!(explicit_status, 0);
    let (bmc, bmc_status) = verify(&path, "bmc", 4, &[]);
    assert_eq!(bmc_status, 0);

    let explicit_step = explicit["reachables"]["HitTwo"]["witnessed_at_step"].clone();
    let bmc_step = bmc["reachables"]["HitTwo"]["witnessed_at_step"].clone();
    assert!(!explicit_step.is_null(), "explicit did not witness HitTwo");
    assert_eq!(
        explicit_step, bmc_step,
        "explicit and BMC disagree on the shortest HitTwo witness"
    );
}

#[test]
fn explicit_reachable_failure_classifies_unreachable_only_at_closure() {
    let path = fixture_path("explicit_reachable_unreachable_goal.fsl");

    // At a depth past the point where the finite state space closes
    // (count saturates at 5, one step past the last `inc()`), explicit
    // exploration proves HitSix can never happen — mark_reachables_
    // definitively_unreachable should stamp the unreached goal and the
    // top-level hint accordingly.
    let (closed, closed_status) = verify(&path, "explicit", 6, &[]);
    assert_eq!(closed_status, 1);
    assert_eq!(closed["result"], "reachable_failed");
    assert_eq!(closed["closure"], true);
    let unreached = closed["unreached"].as_array().expect("unreached array");
    assert_eq!(unreached.len(), 1);
    assert_eq!(unreached[0]["name"], "HitSix");
    assert_eq!(unreached[0]["classification"], "unreachable");
    assert!(unreached[0]["hint"].as_str().is_some_and(|hint| {
        hint.contains("not witnessed before explicit state-space closure")
    }));
    assert!(closed["hint"].as_str().is_some_and(|hint| {
        hint.contains("explicit state-space closure proves that the requested reachable goal cannot be reached")
    }));

    // At a depth too small to reach closure, explicit must not claim
    // definitive unreachability — it should classify the same way BMC
    // does when it also cannot witness the goal within that depth.
    let (bounded, bounded_status) = verify(&path, "explicit", 2, &[]);
    assert_eq!(bounded_status, 1);
    assert_eq!(bounded["result"], "reachable_failed");
    assert_eq!(bounded["closure"], false);
    let bounded_unreached = bounded["unreached"].as_array().expect("unreached array");
    assert_eq!(bounded_unreached[0]["classification"], "insufficient_depth");

    let (bmc_bounded, bmc_bounded_status) = verify(&path, "bmc", 2, &[]);
    assert_eq!(bmc_bounded_status, 1);
    assert_eq!(bmc_bounded["result"], "reachable_failed");
    let bmc_unreached = bmc_bounded["unreached"]
        .as_array()
        .expect("unreached array");
    assert_eq!(
        bounded_unreached[0]["classification"],
        bmc_unreached[0]["classification"]
    );
}

#[test]
fn explicit_excluding_a_leadsto_property_is_accepted_and_verifies_the_rest() {
    let path = fixture_path("explicit_exclude_leadsto.fsl");

    // Without exclusion, the explicit engine rejects the spec outright
    // because it does not support leadsTo.
    let (rejected, rejected_status) = verify(&path, "explicit", 4, &[]);
    assert_eq!(rejected_status, 2);
    assert_eq!(rejected["kind"], "semantics");

    // Excluding the leadsTo property is accepted (no semantics error) and
    // the remaining invariant verifies/proves.
    let (accepted, accepted_status) = verify(
        &path,
        "explicit",
        4,
        &["--exclude-property", "FlagLeadsToDone"],
    );
    assert_eq!(accepted_status, 0);
    assert!(matches!(
        accepted["result"].as_str(),
        Some("proved" | "verified")
    ));
    assert_eq!(accepted["invariants_checked"], json!(["DoneImpliesFlag"]));
}

#[test]
fn explicit_property_narrowing_matches_bmc() {
    let path = fixture_path("explicit_property_narrow.fsl");

    // Unfiltered, NeverTrue is violated once the toggle runs.
    let (unfiltered, unfiltered_status) = verify(&path, "explicit", 4, &[]);
    assert_eq!(unfiltered_status, 1);
    assert_eq!(unfiltered["result"], "violated");
    assert_eq!(unfiltered["invariant"], "NeverTrue");

    // Narrowing to the other (always-true) invariant verifies/proves on
    // both engines.
    let (explicit, explicit_status) =
        verify(&path, "explicit", 4, &["--property", "AlwaysBoolean"]);
    assert_eq!(explicit_status, 0);
    assert!(matches!(
        explicit["result"].as_str(),
        Some("proved" | "verified")
    ));
    assert_eq!(explicit["invariants_checked"], json!(["AlwaysBoolean"]));

    let (bmc, bmc_status) = verify(&path, "bmc", 4, &["--property", "AlwaysBoolean"]);
    assert_eq!(bmc_status, 0);
    assert_eq!(bmc["result"], "verified");
    assert_eq!(bmc["invariants_checked"], json!(["AlwaysBoolean"]));
}
