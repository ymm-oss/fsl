// SPDX-License-Identifier: Apache-2.0

//! Sweep comparisons exclude execution metadata only at verification-envelope
//! boundaries, as approval's `stable_json(.., true)` does. The existing fixtures
//! have no domain fields named `cost` or `cache`; synthetic payloads below pin
//! their preservation without adding or weakening an FSL fixture.

use std::path::{Path, PathBuf};
use std::process::Command;
use std::sync::atomic::{AtomicU64, Ordering};

use serde_json::{Value, json};

static NEXT: AtomicU64 = AtomicU64::new(0);

fn root() -> &'static Path {
    Path::new(env!("CARGO_MANIFEST_DIR"))
        .parent()
        .and_then(Path::parent)
        .expect("repository root")
}

fn fresh_cache(label: &str) -> PathBuf {
    let parent = root().join(".codex/tmp");
    std::fs::create_dir_all(&parent).expect("worktree-local scratch parent");
    let cache = parent.join(format!(
        "issue-1345-{label}-{}-{}-{}",
        std::process::id(),
        std::time::SystemTime::now()
            .duration_since(std::time::UNIX_EPOCH)
            .expect("clock after epoch")
            .as_nanos(),
        NEXT.fetch_add(1, Ordering::Relaxed),
    ));
    std::fs::create_dir(&cache).expect("cache must start empty, never reuse ambient entries");
    cache
}

fn sweep(spec: &str, cache: &Path) -> (i32, Value) {
    let output = Command::new(env!("CARGO_BIN_EXE_fslc"))
        .args(["sweep", spec, "--depth", "2..2", "--deadlock", "ignore"])
        .current_dir(root())
        .env("FSLC_CACHE_DIR", cache)
        .env_remove("FSLC_CACHE")
        .env_remove("FSLC_CACHE_VERIFY")
        .output()
        .expect("run native sweep");
    let value = serde_json::from_slice(&output.stdout).unwrap_or_else(|error| {
        panic!(
            "invalid sweep JSON: {error}; stdout={}; stderr={}",
            String::from_utf8_lossy(&output.stdout),
            String::from_utf8_lossy(&output.stderr),
        )
    });
    (output.status.code().expect("exit status"), value)
}

fn verifications(output: &Value) -> Vec<&Value> {
    let grid = &output["sweep"];
    let mut envelopes = grid["results"]
        .as_array()
        .expect("sweep results")
        .iter()
        .map(|cell| cell.get("verification").expect("cell verification"))
        .collect::<Vec<_>>();
    let minimal = grid
        .get("minimal_counterexample")
        .expect("minimal counterexample key, even when null");
    if !minimal.is_null() {
        envelopes.push(minimal.get("verification").expect("minimal verification"));
    }
    assert!(!envelopes.is_empty(), "empty grid makes exclusions vacuous");
    envelopes
}

/// `cost` includes measured elapsed time; `cache` records lookup provenance.
/// Neither is verdict evidence. Never recurse into reviewed domain state.
fn strip_execution_metadata(verification: &mut Value) {
    let envelope = verification.as_object_mut().expect("verification object");
    envelope.remove("cost");
    envelope.remove("cache");
}

fn comparable_sweep(output: &Value) -> Value {
    let mut stable = output.clone();
    let grid = stable["sweep"].as_object_mut().expect("sweep object");
    for cell in grid
        .get_mut("results")
        .and_then(Value::as_array_mut)
        .expect("sweep results")
    {
        strip_execution_metadata(cell.get_mut("verification").expect("cell verification"));
    }
    let minimal = grid
        .get_mut("minimal_counterexample")
        .expect("minimal counterexample key");
    if !minimal.is_null() {
        strip_execution_metadata(
            minimal
                .get_mut("verification")
                .expect("minimal verification"),
        );
    }
    stable
}

fn assert_live_metadata(cold: &Value, warm: &Value) {
    let cold = verifications(cold);
    let warm = verifications(warm);
    assert_eq!(cold.len(), warm.len(), "verification population changed");
    for (cold, warm) in cold.into_iter().zip(warm) {
        assert!(
            cold.get("cost").is_some(),
            "dead cold cost exclusion: {cold:#}"
        );
        assert!(
            warm.get("cost").is_some(),
            "dead warm cost exclusion: {warm:#}"
        );
        assert!(
            cold.get("cache").is_none(),
            "cold run unexpectedly hit: {cold:#}"
        );
        let cache = warm.get("cache").expect("dead warm cache exclusion");
        assert_eq!(cache["hit"], true, "warm cache must report a hit: {warm:#}");
        assert_eq!(cache["source"], "exact", "single-depth exact hit: {warm:#}");
    }
}

fn assert_cold_warm_agreement(label: &str, spec: &str, result: &str, status: i32) {
    let cache = fresh_cache(label);
    let (cold_status, cold) = sweep(spec, &cache);
    let (warm_status, warm) = sweep(spec, &cache);
    assert_eq!((cold_status, warm_status), (status, status));
    assert_eq!(cold["result"], result, "{cold:#}");
    assert_eq!(warm["result"], result, "{warm:#}");
    assert_live_metadata(&cold, &warm);
    assert_eq!(
        comparable_sweep(&cold),
        comparable_sweep(&warm),
        "stable sweep output differs between cold and warm"
    );

    // A cache hit reuses the original solve cost, so it need not change any
    // timing. An independent cold solve exercises the timing exclusion too.
    let (fresh_status, fresh) = sweep(spec, &fresh_cache(label));
    assert_eq!(fresh_status, status, "{fresh:#}");
    for verification in verifications(&fresh) {
        assert!(
            verification.get("cost").is_some(),
            "dead fresh cost exclusion"
        );
        assert!(
            verification.get("cache").is_none(),
            "fresh cache was not empty"
        );
    }
    assert_eq!(
        comparable_sweep(&cold),
        comparable_sweep(&fresh),
        "stable sweep output differs between independent cold solves"
    );
}

#[test]
fn cold_and_warm_clean_sweeps_agree_without_execution_metadata() {
    assert_cold_warm_agreement(
        "clean",
        "rust/fslc/tests/fixtures/sweep_clean.fsl",
        "sweep_passed",
        0,
    );
}

#[test]
fn cold_and_warm_violated_sweeps_compare_the_minimal_counterexample() {
    assert_cold_warm_agreement(
        "violated",
        "rust/fslc/tests/fixtures/sweep_violating.fsl",
        "sweep_failed",
        1,
    );
}

#[test]
fn cold_and_warm_inconclusive_sweeps_agree_without_execution_metadata() {
    assert_cold_warm_agreement("inconclusive", "specs/cart_v1.fsl", "sweep_inconclusive", 1);
}

fn synthetic_sweep() -> Value {
    let cell = json!({
        "scope": {"depth": 2},
        "summary": {"result": "violated"},
        "verification": {
            "result": "violated",
            "loc": {"line": 11, "column": 3},
            "cost": {"elapsed_s": 0.01},
            "cache": {"hit": true, "source": "exact"},
            "trace": [{"state": {
                "cost": {"elapsed_s": 7, "cache": false},
                "cache": {"cost": 8},
                "elapsed_s": 9,
                "check_elapsed_s": 10
            }}]
        }
    });
    json!({
        "result": "sweep_failed",
        "sweep": {"results": [cell.clone()], "minimal_counterexample": cell}
    })
}

#[test]
fn projection_preserves_domain_cost_and_cache_fields() {
    let original = synthetic_sweep();
    let mut expected = original.clone();
    let verification = json!({
        "result": "violated",
        "loc": {"line": 11, "column": 3},
        "trace": [{"state": {
            "cost": {"elapsed_s": 7, "cache": false},
            "cache": {"cost": 8},
            "elapsed_s": 9,
            "check_elapsed_s": 10
        }}]
    });
    expected["sweep"]["results"][0]["verification"] = verification.clone();
    expected["sweep"]["minimal_counterexample"]["verification"] = verification;
    assert_eq!(comparable_sweep(&original), expected);
    assert!(
        original["sweep"]["results"][0]["verification"]
            .get("cost")
            .is_some()
    );
    assert!(
        original["sweep"]["results"][0]["verification"]
            .get("cache")
            .is_some()
    );
}

#[test]
fn projection_keeps_verdict_and_location_differences() {
    let original = synthetic_sweep();
    for pointer in [
        "/sweep/results/0/verification/result",
        "/sweep/results/0/verification/loc/line",
        "/sweep/minimal_counterexample/verification/result",
        "/sweep/minimal_counterexample/verification/loc/line",
    ] {
        let mut changed = original.clone();
        let field = changed
            .pointer_mut(pointer)
            .expect("negative-control field");
        *field = if field.is_string() {
            json!("verified")
        } else {
            json!(99)
        };
        assert_ne!(
            comparable_sweep(&original),
            comparable_sweep(&changed),
            "projection hid a verdict/location change at {pointer}"
        );
    }
}
