// SPDX-License-Identifier: Apache-2.0
// Copyright 2026 Ryoichi Izumita

use std::path::{Path, PathBuf};
use std::process::Command;

use serde_json::json;

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

fn verify_auto(path: &str, depth: usize, extra: &[&str]) -> (serde_json::Value, i32) {
    let mut arguments = vec![
        "verify".to_owned(),
        path.to_owned(),
        "--engine".to_owned(),
        "auto".to_owned(),
        "--depth".to_owned(),
        depth.to_string(),
        "--deadlock".to_owned(),
        "ignore".to_owned(),
        "--no-cache".to_owned(),
    ];
    arguments.extend(extra.iter().map(|argument| (*argument).to_owned()));
    run_cli(&arguments)
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

#[test]
fn auto_uses_explicit_and_reports_closure_when_explicit_can_decide() {
    let (output, status) = verify_auto("specs/cart_v1.fsl", 8, &[]);
    assert_eq!(status, 0);
    assert_eq!(output["result"], "proved");
    assert_eq!(output["closure"], true);
    assert_eq!(output["engine"], "explicit");
    assert!(output.get("engine_fallback").is_none());
}

#[test]
fn auto_falls_back_to_bmc_for_leadsto_and_records_the_reason() {
    let (output, status) = verify_auto("specs/mutex_queue.fsl", 8, &[]);
    assert_eq!(status, 0);
    assert_eq!(output["engine"], "bmc");
    assert_eq!(output["engine_fallback"]["from"], "explicit");
    assert_eq!(output["engine_fallback"]["kind"], "unsupported");
    assert!(
        output["engine_fallback"]["reason"]
            .as_str()
            .is_some_and(|reason| reason.contains("does not support leadsTo"))
    );
    let (plain_bmc, plain_status) = verify("specs/mutex_queue.fsl", "bmc", 8, &[]);
    assert_eq!(plain_status, status);
    assert_eq!(plain_bmc["result"], output["result"]);
}

#[test]
fn auto_falls_back_to_bmc_for_nondeterministic_init() {
    let fixture = Path::new(env!("CARGO_MANIFEST_DIR"))
        .join("tests/fixtures/explicit_nondeterministic_init.fsl");
    let fixture = fixture.to_str().expect("UTF-8 fixture path");
    let (output, status) = verify_auto(fixture, 4, &[]);
    assert_eq!(status, 0);
    assert_eq!(output["engine"], "bmc");
    assert_eq!(output["engine_fallback"]["kind"], "unsupported");
    assert!(
        output["engine_fallback"]["reason"].as_str().is_some_and(
            |reason| reason.contains("init does not assign state variable(s): omitted")
        )
    );
}

#[test]
fn auto_falls_back_to_bmc_when_the_explicit_budget_is_exceeded() {
    let fixture =
        Path::new(env!("CARGO_MANIFEST_DIR")).join("tests/fixtures/explicit_finite_toggle.fsl");
    let fixture = fixture.to_str().expect("UTF-8 fixture path");
    let (output, status) = verify_auto(fixture, 4, &["--explicit-budget", "1"]);
    assert_eq!(status, 0);
    assert_eq!(output["engine"], "bmc");
    assert_eq!(output["engine_fallback"]["kind"], "budget");
    assert!(
        output["engine_fallback"]["reason"]
            .as_str()
            .is_some_and(|reason| reason.contains("1-state budget"))
    );
}

#[test]
fn auto_reports_a_violation_found_by_explicit_without_a_fallback_stamp() {
    let (output, status) = verify_auto("specs/cart_buggy.fsl", 8, &[]);
    assert_eq!(status, 1);
    assert_eq!(output["result"], "violated");
    assert_eq!(output["engine"], "explicit");
    assert!(output.get("engine_fallback").is_none());
}

#[test]
fn auto_reports_an_explicit_deadlock_verdict_without_a_fallback_stamp() {
    let fixture =
        Path::new(env!("CARGO_MANIFEST_DIR")).join("tests/fixtures/explicit_deadlock.fsl");
    let (output, status) = run_cli(&[
        "verify".to_owned(),
        fixture.to_str().expect("UTF-8 fixture path").to_owned(),
        "--engine".to_owned(),
        "auto".to_owned(),
        "--depth".to_owned(),
        "4".to_owned(),
        "--deadlock".to_owned(),
        "error".to_owned(),
        "--no-cache".to_owned(),
    ]);
    assert_eq!(status, 1);
    assert_eq!(output["violation_kind"], "deadlock");
    assert_eq!(output["engine"], "explicit");
    assert!(output.get("engine_fallback").is_none());
}

#[test]
fn auto_rejects_lemma_and_from_state_the_same_way_explicit_and_induction_do() {
    let (output, status) = verify_auto("specs/cart_v1.fsl", 8, &["--lemma", "true"]);
    assert_eq!(status, 2);
    assert_eq!(output["kind"], "usage");

    let (output, status) = verify_auto("specs/cart_v1.fsl", 8, &["--from-state", "/dev/null"]);
    assert_eq!(status, 2);
    assert_eq!(output["kind"], "semantics");
}

struct CacheDir {
    path: PathBuf,
}

impl CacheDir {
    fn new(name: &str) -> Self {
        let path =
            std::env::temp_dir().join(format!("fslc-issue-226-{name}-{}", std::process::id()));
        let _ = std::fs::remove_dir_all(&path);
        Self { path }
    }

    fn run(&self, arguments: &[String]) -> (serde_json::Value, i32) {
        let output = Command::new(env!("CARGO_BIN_EXE_fslc"))
            .args(arguments)
            .current_dir(repository_root())
            .env("FSLC_CACHE_DIR", &self.path)
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
}

impl Drop for CacheDir {
    fn drop(&mut self) {
        let _ = std::fs::remove_dir_all(&self.path);
    }
}

#[test]
fn auto_cache_shares_entries_with_plain_explicit_and_bmc_runs() {
    {
        let cache = CacheDir::new("closure");
        let arguments = |engine: &str| {
            vec![
                "verify".to_owned(),
                "specs/cart_v1.fsl".to_owned(),
                "--engine".to_owned(),
                engine.to_owned(),
                "--depth".to_owned(),
                "8".to_owned(),
                "--deadlock".to_owned(),
                "ignore".to_owned(),
            ]
        };
        let (first, _) = cache.run(&arguments("auto"));
        assert_eq!(first["engine"], "explicit");
        assert!(first.get("cache").is_none());

        let (second, _) = cache.run(&arguments("auto"));
        assert_eq!(second["cache"]["hit"], true);
        assert_eq!(second["engine"], "explicit");

        let (plain_explicit, _) = cache.run(&arguments("explicit"));
        assert_eq!(plain_explicit["cache"]["hit"], true);
    }

    {
        let cache = CacheDir::new("fallback");
        let arguments = |engine: &str| {
            vec![
                "verify".to_owned(),
                "specs/mutex_queue.fsl".to_owned(),
                "--engine".to_owned(),
                engine.to_owned(),
                "--depth".to_owned(),
                "8".to_owned(),
                "--deadlock".to_owned(),
                "ignore".to_owned(),
            ]
        };
        let (first, _) = cache.run(&arguments("auto"));
        assert_eq!(first["engine"], "bmc");
        assert!(first["engine_fallback"].is_object());

        let (second, _) = cache.run(&arguments("auto"));
        assert_eq!(second["cache"]["hit"], true);
        assert_eq!(second["engine"], "bmc");
        assert!(second["engine_fallback"].is_object());
        // The fallback trace is persisted on the cache entry itself, so a
        // repeat `auto` cache hit restores the exact original reason/kind
        // rather than a freshly recomputed (and potentially different) one.
        assert_eq!(
            first["engine_fallback"]["reason"],
            second["engine_fallback"]["reason"]
        );
        assert_eq!(
            first["engine_fallback"]["kind"],
            second["engine_fallback"]["kind"]
        );

        // Cache purity: a plain `--engine bmc` run hits the exact entry the
        // fallback wrote, and must never see `engine`/`engine_fallback`
        // stamped onto it — those fields are a post-cache annotation, not
        // part of the cached bmc output itself.
        let (plain_bmc, _) = cache.run(&arguments("bmc"));
        assert_eq!(plain_bmc["cache"]["hit"], true);
        assert!(plain_bmc.get("engine").is_none());
        assert!(plain_bmc.get("engine_fallback").is_none());
    }
}

#[test]
fn engine_choice_lists_auto_alongside_the_existing_engines() {
    let (output, status) = run_cli(&[
        "verify".to_owned(),
        "specs/cart_v1.fsl".to_owned(),
        "--engine".to_owned(),
        "bogus".to_owned(),
    ]);
    assert_eq!(status, 2);
    assert_eq!(
        output["message"],
        json!("--engine must be bmc, induction, explicit, or auto")
    );
}

/// Acceptance criterion: `--engine auto` always returns a terminal verdict
/// for every accepted top-level corpus spec (never `unknown_budget`, and
/// never anything other than the engine that actually decided it), and its
/// verdict always matches whichever single engine
/// `explicit_and_bmc_agree_on_every_accepted_top_level_corpus_spec`
/// (`explicit_engine.rs`) already proved would have decided it alone.
#[test]
fn auto_always_terminates_and_matches_the_deciding_engine_across_the_spec_corpus() {
    let root = repository_root();
    let mut paths = std::fs::read_dir(root.join("specs"))
        .expect("read specs corpus")
        .map(|entry| entry.expect("corpus entry").path())
        .filter(|path| path.extension().and_then(std::ffi::OsStr::to_str) == Some("fsl"))
        .collect::<Vec<_>>();
    paths.sort();

    let mut accepted = 0_usize;
    for path in paths {
        let relative = path
            .strip_prefix(&root)
            .expect("repository-relative spec")
            .to_string_lossy()
            .into_owned();
        let (auto, auto_status) = verify_auto(&relative, 8, &[]);
        if auto_status == 2 && auto["result"] == "error" {
            continue;
        }
        accepted += 1;
        assert_ne!(
            auto["result"], "unknown_budget",
            "auto must never surface unknown_budget for {relative}"
        );
        let engine = auto["engine"]
            .as_str()
            .unwrap_or_else(|| panic!("auto result for {relative} has no engine field: {auto:#}"));
        assert!(
            matches!(engine, "explicit" | "bmc"),
            "unexpected engine {engine:?} for {relative}"
        );
        let (single, single_status) = verify(&relative, engine, 8, &[]);
        assert_eq!(
            auto["result"], single["result"],
            "auto result for {relative} disagreed with plain --engine {engine}: auto={auto:#} single={single:#}"
        );
        assert_eq!(
            auto_status, single_status,
            "exit code mismatch for {relative}"
        );
    }
    assert!(accepted > 0, "no corpus spec was accepted by auto");
}
