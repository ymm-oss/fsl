// SPDX-License-Identifier: Apache-2.0
// Copyright 2026 Ryoichi Izumita

use std::path::{Path, PathBuf};
use std::process::Command;

use serde_json::Value;

fn repository_root() -> PathBuf {
    Path::new(env!("CARGO_MANIFEST_DIR"))
        .parent()
        .and_then(Path::parent)
        .expect("repository root")
        .to_path_buf()
}

fn run_cli_with_cache(arguments: &[String], cache_dir: Option<&Path>) -> (Value, i32) {
    let mut command = Command::new(env!("CARGO_BIN_EXE_fslc"));
    command.args(arguments).current_dir(repository_root());
    if let Some(cache_dir) = cache_dir {
        command.env("FSLC_CACHE_DIR", cache_dir);
    }
    let output = command.output().expect("run native fslc");
    let value = serde_json::from_slice(&output.stdout).unwrap_or_else(|error| {
        panic!(
            "invalid JSON: {error}; args={arguments:?}; stderr={}",
            String::from_utf8_lossy(&output.stderr)
        )
    });
    (value, output.status.code().expect("native exit status"))
}

fn verify(path: &str, engine: &str, depth: usize, extra: &[&str]) -> (Value, i32) {
    let mut arguments = base_arguments(path, engine, depth);
    arguments.push("--no-cache".to_owned());
    arguments.extend(extra.iter().map(|argument| (*argument).to_owned()));
    run_cli_with_cache(&arguments, None)
}

fn verify_cached(path: &str, engine: &str, depth: usize, cache_dir: &Path) -> (Value, i32) {
    run_cli_with_cache(&base_arguments(path, engine, depth), Some(cache_dir))
}

fn base_arguments(path: &str, engine: &str, depth: usize) -> Vec<String> {
    vec![
        "verify".to_owned(),
        path.to_owned(),
        "--engine".to_owned(),
        engine.to_owned(),
        "--depth".to_owned(),
        depth.to_string(),
        "--deadlock".to_owned(),
        "ignore".to_owned(),
    ]
}

fn fixture_path(name: &str) -> String {
    Path::new(env!("CARGO_MANIFEST_DIR"))
        .join("tests/fixtures")
        .join(name)
        .to_str()
        .expect("UTF-8 fixture path")
        .to_owned()
}

fn temporary_cache(label: &str) -> PathBuf {
    let directory =
        std::env::temp_dir().join(format!("fslc-engine-auto-{label}-{}", std::process::id()));
    let _ = std::fs::remove_dir_all(&directory);
    std::fs::create_dir_all(&directory).expect("create cache directory");
    directory
}

#[test]
fn auto_returns_the_explicit_verdict_when_explicit_decides() {
    let finite = fixture_path("explicit_finite_toggle.fsl");

    let (proved, status) = verify(&finite, "auto", 4, &[]);
    assert_eq!(status, 0);
    assert_eq!(proved["result"], "proved");
    assert_eq!(proved["engine"], "explicit");
    assert_eq!(proved["closure"], true);
    assert_eq!(proved["completeness"], "unbounded");
    assert!(proved.get("engine_fallback").is_none());

    let (violated, status) = verify("specs/cart_buggy.fsl", "auto", 8, &[]);
    assert_eq!(status, 1);
    assert_eq!(violated["result"], "violated");
    assert_eq!(violated["engine"], "explicit");
    assert!(violated.get("engine_fallback").is_none());
}

#[test]
fn auto_falls_back_to_bmc_on_explicit_semantics_gates() {
    // leadsTo: the explicit engine fail-closes; auto must reach the BMC verdict
    // without user action and record why.
    let (leadsto, status) = verify("specs/mutex_queue.fsl", "auto", 8, &[]);
    let (bmc, bmc_status) = verify("specs/mutex_queue.fsl", "bmc", 8, &[]);
    assert_eq!(status, bmc_status);
    assert_eq!(leadsto["result"], bmc["result"]);
    assert_eq!(leadsto["engine"], "bmc");
    assert_eq!(leadsto["engine_fallback"]["from"], "explicit");
    assert!(
        leadsto["engine_fallback"]["reason"]
            .as_str()
            .is_some_and(|reason| reason.contains("leadsTo"))
    );

    // Nondeterministic init: same fail-closed class.
    let nondeterministic = fixture_path("explicit_nondeterministic_init.fsl");
    let (fallback, status) = verify(&nondeterministic, "auto", 4, &[]);
    let (bmc, bmc_status) = verify(&nondeterministic, "bmc", 4, &[]);
    assert_eq!(status, bmc_status);
    assert_eq!(fallback["result"], bmc["result"]);
    assert_eq!(fallback["engine"], "bmc");
    assert!(
        fallback["engine_fallback"]["reason"]
            .as_str()
            .is_some_and(|reason| reason.contains("init does not assign"))
    );
}

#[test]
fn auto_falls_back_to_bmc_when_the_explicit_budget_is_exceeded() {
    let finite = fixture_path("explicit_finite_toggle.fsl");
    let (output, status) = verify(&finite, "auto", 4, &["--explicit-budget", "1"]);
    assert_eq!(status, 0);
    assert_eq!(output["result"], "verified");
    assert_eq!(output["engine"], "bmc");
    assert_eq!(output["engine_fallback"]["from"], "explicit");
    assert!(
        output["engine_fallback"]["reason"]
            .as_str()
            .is_some_and(|reason| reason.contains("1-state budget"))
    );
}

#[test]
fn auto_verdicts_are_cached_under_the_deciding_engine_key() {
    let cache = temporary_cache("fallback");

    // Fresh auto run falls back to BMC and stores under the bmc key.
    let (fresh, status) = verify_cached("specs/mutex_queue.fsl", "auto", 8, &cache);
    assert_eq!(status, 0);
    assert_eq!(fresh["engine"], "bmc");
    assert!(fresh.get("cache").is_none());

    // A plain --engine bmc run shares that entry and sees no auto-only fields.
    let (plain, status) = verify_cached("specs/mutex_queue.fsl", "bmc", 8, &cache);
    assert_eq!(status, 0);
    assert_eq!(plain["cache"]["hit"], true);
    assert!(
        plain.get("engine").is_none(),
        "auto fields leaked: {plain:#}"
    );
    assert!(plain.get("engine_fallback").is_none());

    // An auto re-run hits the bmc key and restores the fallback trace.
    let (hit, status) = verify_cached("specs/mutex_queue.fsl", "auto", 8, &cache);
    assert_eq!(status, 0);
    assert_eq!(hit["cache"]["hit"], true);
    assert_eq!(hit["engine"], "bmc");
    assert_eq!(hit["engine_fallback"]["from"], "explicit");

    let _ = std::fs::remove_dir_all(&cache);
}

#[test]
fn auto_closure_proofs_share_the_explicit_engine_cache() {
    let cache = temporary_cache("closure");
    let finite = fixture_path("explicit_finite_toggle.fsl");

    let (fresh, status) = verify_cached(&finite, "auto", 4, &cache);
    assert_eq!(status, 0);
    assert_eq!(fresh["result"], "proved");
    assert_eq!(fresh["engine"], "explicit");
    assert!(fresh.get("cache").is_none());

    // A plain --engine explicit run hits the same entry.
    let (plain, status) = verify_cached(&finite, "explicit", 4, &cache);
    assert_eq!(status, 0);
    assert_eq!(plain["cache"]["hit"], true);
    assert_eq!(plain["result"], "proved");
    assert_eq!(plain["engine"], "explicit");

    // And the auto re-run hits it with correct tracking fields.
    let (hit, status) = verify_cached(&finite, "auto", 4, &cache);
    assert_eq!(status, 0);
    assert_eq!(hit["cache"]["hit"], true);
    assert_eq!(hit["result"], "proved");
    assert_eq!(hit["engine"], "explicit");
    assert!(hit.get("engine_fallback").is_none());

    let _ = std::fs::remove_dir_all(&cache);
}

#[test]
fn auto_reuses_a_plain_bmc_entry_without_fabricating_a_fallback() {
    let cache = temporary_cache("shared-bmc");

    let (_, status) = verify_cached("specs/cart_v1.fsl", "bmc", 8, &cache);
    assert_eq!(status, 0);

    let (hit, status) = verify_cached("specs/cart_v1.fsl", "auto", 8, &cache);
    assert_eq!(status, 0);
    assert_eq!(hit["cache"]["hit"], true);
    assert_eq!(hit["engine"], "bmc");
    assert!(
        hit.get("engine_fallback").is_none(),
        "fallback must not be invented for a shared bmc entry: {hit:#}"
    );

    let _ = std::fs::remove_dir_all(&cache);
}

#[test]
fn auto_skips_a_cached_explicit_unknown_budget_entry_and_stays_terminal() {
    let cache = temporary_cache("budget-skip");
    let finite = fixture_path("explicit_finite_toggle.fsl");

    // A plain explicit run stores unknown_budget under the explicit key for
    // this exact --explicit-budget configuration.
    let mut arguments = base_arguments(&finite, "explicit", 4);
    arguments.extend(["--explicit-budget".to_owned(), "1".to_owned()]);
    let (stored, status) = run_cli_with_cache(&arguments, Some(&cache));
    assert_eq!(status, 1);
    assert_eq!(stored["result"], "unknown_budget");

    // Auto must not surface that entry: it skips the cached unknown_budget,
    // re-tries, and returns the terminal BMC verdict via fallback.
    let mut arguments = base_arguments(&finite, "auto", 4);
    arguments.extend(["--explicit-budget".to_owned(), "1".to_owned()]);
    let (auto, status) = run_cli_with_cache(&arguments, Some(&cache));
    assert_eq!(status, 0);
    assert_eq!(auto["result"], "verified");
    assert_eq!(auto["engine"], "bmc");
    assert_eq!(auto["engine_fallback"]["from"], "explicit");

    let _ = std::fs::remove_dir_all(&cache);
}

#[test]
fn auto_reports_an_explicit_deadlock_verdict_without_fallback() {
    let deadlock = fixture_path("explicit_deadlock.fsl");
    let arguments = vec![
        "verify".to_owned(),
        deadlock,
        "--engine".to_owned(),
        "auto".to_owned(),
        "--depth".to_owned(),
        "4".to_owned(),
        "--deadlock".to_owned(),
        "error".to_owned(),
        "--no-cache".to_owned(),
    ];
    let (output, status) = run_cli_with_cache(&arguments, None);
    assert_eq!(status, 1);
    assert_eq!(output["violation_kind"], "deadlock");
    assert_eq!(output["engine"], "explicit");
    assert!(output.get("engine_fallback").is_none());
}

#[test]
fn auto_reuses_a_cached_violation_across_depths() {
    let cache = temporary_cache("cross-depth");

    let (violated, status) = verify_cached("specs/cart_buggy.fsl", "auto", 8, &cache);
    assert_eq!(status, 1);
    assert_eq!(violated["result"], "violated");
    assert_eq!(violated["engine"], "explicit");
    let step = violated["violated_at_step"]
        .as_u64()
        .expect("violation step");

    // A deeper auto run reuses the concrete witness via the cross-depth
    // pointer instead of re-exploring.
    let (deeper, status) = verify_cached("specs/cart_buggy.fsl", "auto", 12, &cache);
    assert_eq!(status, 1);
    assert_eq!(deeper["result"], "violated");
    assert_eq!(deeper["cache"]["source"], "cross_depth");
    assert_eq!(deeper["violated_at_step"], step);

    let _ = std::fs::remove_dir_all(&cache);
}

#[test]
fn auto_returns_a_terminal_verdict_on_every_accepted_top_level_corpus_spec() {
    let root = repository_root();
    let mut paths = std::fs::read_dir(root.join("specs"))
        .expect("read specs corpus")
        .map(|entry| entry.expect("corpus entry").path())
        .filter(|path| path.extension().and_then(std::ffi::OsStr::to_str) == Some("fsl"))
        .collect::<Vec<_>>();
    paths.sort();

    let mut accepted = 0_usize;
    let mut fallbacks = 0_usize;
    for path in paths {
        let relative = path
            .strip_prefix(&root)
            .expect("repository-relative spec")
            .to_string_lossy()
            .into_owned();
        let (bmc, bmc_status) = verify(&relative, "bmc", 8, &[]);
        let (auto, auto_status) = verify(&relative, "auto", 8, &[]);
        if bmc["result"] == "error" {
            // Files no engine supports stay errors under auto too.
            assert_eq!(auto["result"], "error", "auto hid an error for {relative}");
            continue;
        }
        accepted += 1;
        let auto_result = auto["result"].as_str().expect("auto result");
        assert_ne!(auto_result, "error", "auto errored on {relative}: {auto:#}");
        assert_ne!(
            auto_result, "unknown_budget",
            "auto returned a non-terminal budget verdict for {relative}"
        );
        match auto["engine"].as_str() {
            Some("explicit") => {
                let normalized = if auto_result == "proved" {
                    "verified"
                } else {
                    auto_result
                };
                assert_eq!(
                    normalized,
                    bmc["result"].as_str().expect("BMC result"),
                    "auto/bmc verdict mismatch for {relative}"
                );
                assert!(auto.get("engine_fallback").is_none());
            }
            Some("bmc") => {
                fallbacks += 1;
                assert_eq!(
                    auto["result"], bmc["result"],
                    "fallback verdict drift for {relative}"
                );
                assert_eq!(
                    auto_status, bmc_status,
                    "fallback exit drift for {relative}"
                );
                assert_eq!(auto["engine_fallback"]["from"], "explicit");
            }
            other => panic!("auto did not stamp a deciding engine for {relative}: {other:?}"),
        }
    }

    assert!(accepted > 0, "auto accepted no corpus specs");
    assert!(fallbacks > 0, "corpus exercised no auto fallback");
}
