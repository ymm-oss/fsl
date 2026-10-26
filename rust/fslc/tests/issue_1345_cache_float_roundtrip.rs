// SPDX-License-Identifier: Apache-2.0

//! #1345 E2E cost-byte preservation/regression controls, not base-rejecting detectors.
//! The `jsonschema` dev-dependency enables `float_roundtrip` in this graph, so base passes too.

use std::path::{Path, PathBuf};
use std::process::Command;

use serde_json::Value;

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
        "issue-1345-float-{label}-{}-{}",
        std::process::id(),
        std::time::SystemTime::now()
            .duration_since(std::time::UNIX_EPOCH)
            .expect("clock after epoch")
            .as_nanos(),
    ));
    std::fs::create_dir(&cache).expect("new worktree-local cache directory");
    cache
}

fn sweep(spec: &str, cache: &Path, status: i32) -> Vec<u8> {
    let output = Command::new(env!("CARGO_BIN_EXE_fslc"))
        .args(["sweep", spec, "--depth", "2..2", "--deadlock", "ignore"])
        .current_dir(root())
        .env("FSLC_CACHE_DIR", cache)
        .env_remove("FSLC_CACHE")
        .env_remove("FSLC_CACHE_VERIFY")
        .output()
        .expect("run native sweep");
    assert_eq!(
        output.status.code(),
        Some(status),
        "{}",
        String::from_utf8_lossy(&output.stdout)
    );
    output.stdout
}

/// Remove only pretty-printed verification-root cache objects, preserving all
/// remaining bytes, especially numeric tokens. Parsing and reserializing the
/// whole output would run the faulty reader over the expected cost as well.
/// The known fixtures have no domain `verification` field; fail closed if that
/// assumption or the native pretty-JSON layout changes.
fn without_cache_bytes(bytes: &[u8], expected_envelopes: usize, expected_hits: usize) -> Vec<u8> {
    let text = std::str::from_utf8(bytes).expect("UTF-8 CLI output");
    let mut lines = text.lines();
    let mut kept = Vec::<String>::new();
    let mut verification_indent = None;
    let mut envelopes = 0;
    let mut removed = 0;
    while let Some(line) = lines.next() {
        let trimmed = line.trim_start();
        let indent = line.len() - trimmed.len();
        if trimmed == "\"verification\": {" {
            assert!(
                verification_indent.is_none(),
                "unexpected nested verification field"
            );
            verification_indent = Some(indent);
            envelopes += 1;
        }
        if let Some(boundary) = verification_indent {
            if indent == boundary + 2 && trimmed == "\"cache\": {" {
                let end = lines
                    .by_ref()
                    .find(|line| {
                        let closing = line.trim_start();
                        line.len() - closing.len() == indent && matches!(closing, "}" | "},")
                    })
                    .expect("cache object's closing line");
                if !end.ends_with(',') {
                    let previous = kept.last_mut().expect("field before cache");
                    assert!(
                        previous.ends_with(','),
                        "cache must follow a field separator"
                    );
                    previous.pop();
                }
                removed += 1;
                continue;
            }
            if indent == boundary && matches!(trimmed, "}" | "},") {
                verification_indent = None;
            }
        }
        kept.push(line.to_owned());
    }
    assert!(verification_indent.is_none(), "unterminated verification");
    assert_eq!(
        envelopes, expected_envelopes,
        "only known envelope boundaries"
    );
    assert_eq!(
        removed, expected_hits,
        "cache exclusion must be live only on hits"
    );
    format!("{}\n", kept.join("\n")).into_bytes()
}

fn assert_cached_cost_bytes(label: &str, spec: &str, status: i32, envelope_count: usize) {
    let cache = fresh_cache(label);
    let cold = sweep(spec, &cache, status);
    let warm = sweep(spec, &cache, status);
    // The parsed view checks shape/provenance only, never cost equality.
    let cold_value: Value = serde_json::from_slice(&cold).expect("cold JSON");
    let warm_value: Value = serde_json::from_slice(&warm).expect("warm JSON");
    for (name, value, hit) in [("cold", &cold_value, false), ("warm", &warm_value, true)] {
        let grid = &value["sweep"];
        let mut envelopes = grid["results"]
            .as_array()
            .expect("results")
            .iter()
            .map(|cell| &cell["verification"])
            .collect::<Vec<_>>();
        if !grid["minimal_counterexample"].is_null() {
            envelopes.push(&grid["minimal_counterexample"]["verification"]);
        }
        assert_eq!(envelopes.len(), envelope_count);
        for envelope in envelopes {
            assert!(
                envelope.get("cost").is_some(),
                "{name} must retain solve cost"
            );
            if hit {
                assert_eq!(envelope["cache"]["hit"], true);
                assert_eq!(envelope["cache"]["source"], "exact");
            } else {
                assert!(envelope.get("cache").is_none(), "cold cache must be empty");
            }
        }
    }
    assert_eq!(
        without_cache_bytes(&cold, envelope_count, 0),
        without_cache_bytes(&warm, envelope_count, envelope_count),
        "all envelope bytes, including original cost tokens, must survive cache replay"
    );
}

#[test]
fn cache_hit_preserves_clean_sweep_cost_bytes() {
    assert_cached_cost_bytes("clean", "rust/fslc/tests/fixtures/sweep_clean.fsl", 0, 1);
}

#[test]
fn cache_hit_preserves_violated_sweep_and_minimal_cost_bytes() {
    assert_cached_cost_bytes(
        "violated",
        "rust/fslc/tests/fixtures/sweep_violating.fsl",
        1,
        2,
    );
}
