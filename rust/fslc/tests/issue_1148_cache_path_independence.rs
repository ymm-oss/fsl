// SPDX-License-Identifier: Apache-2.0
// Copyright 2026 Ryoichi Izumita

//! Regression coverage for issue #1148: the `verify` cache key embedded the
//! checked spec's canonicalised absolute path, contradicting
//! `docs/DESIGN-incremental-verify.md`, which states that entries embed no
//! absolute paths and are safe across projects and worktrees. The same bytes at a second location were
//! a complete miss.
//!
//! Each test builds the same tree under two roots, shares one
//! `FSLC_CACHE_DIR`, and reads the `cache.hit` annotation the CLI adds to a
//! hit. The tree has an `implements ... from "../abstract.fsl"` dependency
//! outside the spec's own directory, so a hit that ignored dependencies would
//! be visible as a stale `verified`.

use std::path::{Path, PathBuf};
use std::process::Command;
use std::sync::atomic::{AtomicU64, Ordering};

use serde_json::Value;

static NEXT: AtomicU64 = AtomicU64::new(0);

struct Scratch(PathBuf);

impl Scratch {
    fn new(name: &str) -> Self {
        let dir = std::env::temp_dir().join(format!(
            "fsl-issue-1148-{name}-{}-{}",
            std::process::id(),
            NEXT.fetch_add(1, Ordering::Relaxed)
        ));
        std::fs::create_dir(&dir).expect("scratch dir must not already exist");
        Self(dir)
    }
}

impl Drop for Scratch {
    fn drop(&mut self) {
        let _ = std::fs::remove_dir_all(&self.0);
    }
}

const IMPL_SOURCE: &str = r#"requirements CountedReq {
  implements CountedFlow from "../abstract.fsl" { maps auto }

  number Limit
  state { n: Limit }
  init { n = 0 }
  action bump() maps bump() {
    n = 0
  }
}
verify {
  values Limit = 0..3
}
"#;

fn abstract_source(bump_assigns: &str) -> String {
    format!(
        r"spec CountedFlow {{
  number Limit
  state {{ n: Limit }}
  init {{ n = 0 }}
  action bump() {{
    n = {bump_assigns}
  }}
}}
verify {{
  values Limit = 0..3
}}
"
    )
}

/// A tree `<root>/<name>/proj/impl.fsl` + `<root>/<name>/abstract.fsl`.
fn tree(scratch: &Scratch, name: &str, impl_source: &str, dependency: &str) -> PathBuf {
    let base = scratch.0.join(name);
    std::fs::create_dir_all(base.join("proj")).expect("create tree");
    std::fs::write(base.join("proj/impl.fsl"), impl_source).expect("write impl");
    std::fs::write(base.join("abstract.fsl"), dependency).expect("write dependency");
    base.join("proj/impl.fsl")
}

fn verify(path: &Path, cache: &Path) -> (Value, i32) {
    let output = Command::new(env!("CARGO_BIN_EXE_fslc"))
        .arg("verify")
        .arg(path)
        .args(["--depth", "8"])
        .env("FSLC_CACHE_DIR", cache)
        .env_remove("FSLC_CACHE")
        .env_remove("FSLC_CACHE_VERIFY")
        .output()
        .expect("run native CLI");
    let value = serde_json::from_slice(&output.stdout).unwrap_or_else(|error| {
        panic!(
            "invalid JSON: {error}; stderr={}",
            String::from_utf8_lossy(&output.stderr)
        )
    });
    (value, output.status.code().expect("exit status"))
}

fn hit(value: &Value) -> bool {
    value["cache"]["hit"] == true
}

fn without_cache(value: &Value) -> Value {
    let mut value = value.clone();
    value.as_object_mut().expect("object").remove("cache");
    value
}

/// Calibration: identical content at two absolute paths. The first run is a
/// miss, the second a hit, and the replayed verdict is the first's.
#[test]
fn the_same_tree_at_a_second_path_hits_the_cache() {
    let scratch = Scratch::new("relocated");
    let cache = scratch.0.join("cache");
    let first = tree(&scratch, "one", IMPL_SOURCE, &abstract_source("0"));
    let second = tree(
        &scratch,
        "two/nested/deeper",
        IMPL_SOURCE,
        &abstract_source("0"),
    );

    let (cold, cold_status) = verify(&first, &cache);
    assert_eq!(
        (cold_status, cold["result"].as_str()),
        (0, Some("verified")),
        "{cold:#}"
    );
    assert!(!hit(&cold), "the first run has nothing to hit: {cold:#}");

    let (warm, warm_status) = verify(&second, &cache);
    assert_eq!(warm_status, 0, "{warm:#}");
    assert!(
        hit(&warm),
        "same content at another path must hit: {warm:#}"
    );
    assert_eq!(without_cache(&warm), cold);
}

/// A failure verdict is shared across locations the same way, and is replayed
/// as a violation rather than as `verified`. (`refinement_failed` is not a
/// cacheable verdict class, so this uses a plain invariant violation.)
#[test]
fn a_violation_is_shared_across_paths_as_a_violation() {
    const VIOLATING: &str = "spec Counter {
  state { n: 0..5 }
  init { n = 0 }
  action bump() {
    requires n < 5
    n = n + 1
  }
  invariant Small { n < 2 }
}
";
    let scratch = Scratch::new("violation");
    let cache = scratch.0.join("cache");
    let first = tree(&scratch, "one", VIOLATING, &abstract_source("0"));
    let second = tree(&scratch, "two/elsewhere", VIOLATING, &abstract_source("0"));

    let (cold, cold_status) = verify(&first, &cache);
    assert_eq!(
        (cold_status, cold["result"].as_str()),
        (1, Some("violated")),
        "{cold:#}"
    );
    assert!(!hit(&cold), "{cold:#}");
    let (warm, warm_status) = verify(&second, &cache);
    assert_eq!(warm_status, 1, "{warm:#}");
    assert!(hit(&warm), "{warm:#}");
    assert_eq!(without_cache(&warm), cold);
}

/// Over-detection control: a byte changed in the entry spec still misses at a
/// second location, and the new verdict is computed, not replayed.
#[test]
fn a_changed_spec_at_a_second_path_misses() {
    let scratch = Scratch::new("edited-spec");
    let cache = scratch.0.join("cache");
    let first = tree(&scratch, "one", IMPL_SOURCE, &abstract_source("0"));
    let edited = format!("{IMPL_SOURCE}\n// edited\n");
    let second = tree(&scratch, "two", &edited, &abstract_source("0"));

    assert_eq!(verify(&first, &cache).1, 0);
    let (after, status) = verify(&second, &cache);
    assert_eq!(status, 0, "{after:#}");
    assert!(
        !hit(&after),
        "an edited spec must not be served from cache: {after:#}"
    );
}

/// Over-detection control: a changed dependency invalidates at a second
/// location. The dependency now violates the refinement, and that must be
/// reported instead of the first tree's `verified`.
#[test]
fn a_changed_dependency_at_a_second_path_invalidates() {
    let scratch = Scratch::new("edited-dependency");
    let cache = scratch.0.join("cache");
    let first = tree(&scratch, "one", IMPL_SOURCE, &abstract_source("0"));
    let second = tree(&scratch, "two", IMPL_SOURCE, &abstract_source("1"));

    assert_eq!(verify(&first, &cache).1, 0);
    let (after, status) = verify(&second, &cache);
    assert_eq!(status, 1, "a stale `verified` was replayed: {after:#}");
    assert_eq!(after["result"], "refinement_failed", "{after:#}");
    assert!(!hit(&after), "{after:#}");
}

/// A move within one tree (same content, new file name) also hits: the entry
/// spec's own path is not an input.
#[test]
fn a_renamed_spec_hits_the_cache() {
    let scratch = Scratch::new("renamed");
    let cache = scratch.0.join("cache");
    let original = tree(&scratch, "one", IMPL_SOURCE, &abstract_source("0"));
    assert_eq!(verify(&original, &cache).1, 0);
    let renamed = original.with_file_name("renamed.fsl");
    std::fs::rename(&original, &renamed).expect("rename spec");
    let (after, status) = verify(&renamed, &cache);
    assert_eq!(status, 0, "{after:#}");
    assert!(hit(&after), "{after:#}");
}
