// SPDX-License-Identifier: Apache-2.0
// Copyright 2026 Ryoichi Izumita

//! Regression coverage for issue #1023: the `verify` cache key's domain is
//! narrower than the domain of inputs the verdict actually depends on.
//!
//! `verify_cache_keys_with_fingerprints` (`rust/fslc/src/verification.rs`)
//! builds the key from every `.fsl` file found by walking the checked
//! spec's *parent directory* (`collect_fsl_sources`), not from the set of
//! files actually read while resolving the inline `implements ... from`
//! seam (`FsResolver` in `rust/fsl-core/src/compose.rs`). The two sets
//! diverge in both directions:
//!
//! - narrower than the read set: a `from "../x.fsl"` dependency living
//!   outside the parent directory is never walked, so changing it does not
//!   change the key. A stale `verified`/exit 0 cache entry keeps being
//!   served after the dependency starts violating the refinement contract.
//! - broader than the read set: an unrelated sibling `.fsl` file that
//!   nothing `from`-imports is still walked and hashed into the key, so
//!   editing it invalidates an entry that never depended on it.
//!
//! This file exercises the four-cell matrix required by the accepted
//! design (`inside`/`outside` the parent directory × `before`/`after`
//! editing the dependency), isolating each cell's own `FSLC_CACHE_DIR` so
//! a hit/miss observation cannot be explained by cross-cell contamination.
//! `inside_after` is a preservation control: the in-parent-directory case
//! already invalidates correctly today because the walk happens to cover
//! it, and the fix must not regress it. `outside_after` is the detector:
//! it is expected to fail (assert on `exit 1`/`refinement_failed`) until
//! the cache key is rebuilt from the resolver's actual read set.
//!
//! A fifth test, `cache_hit_with_removed_dependency_fails_closed`, pins the
//! accepted design's stated behavior change: once a cache hit has to read
//! its dependencies to know its key is still valid, it also fails closed
//! (`error`/exit 2) when a dependency it needs to read has been removed,
//! rather than replaying a verdict computed against a source that no
//! longer exists. Before the fix a cache hit never reads dependencies at
//! all, so this is expected to fail (red) too.
//!
//! Deliberately **not** a `specs/`/`examples/`/`rust/fslc/tests/fixtures/`
//! corpus fixture, following `rust/fslc/tests/issue_697_all_properties_memory.rs`:
//! `rust/fslc/tests/corpus_check_sweep.rs` and `tests/test_dialect_conformance.py`
//! walk those directories with their own tooling, and this reproducer's
//! `from "../..."` layout — one spec living outside its sibling's directory
//! on purpose — has no natural home there. Placing it in the corpus would
//! also make it subject to `tests/dialect_registry.py`'s exclusion/parity
//! bookkeeping for a fixture whose only purpose is exercising this cache
//! key, not dialect coverage.

use std::path::{Path, PathBuf};
use std::process::Command;
use std::time::{SystemTime, UNIX_EPOCH};

use serde_json::Value;

/// A temporary directory tree, removed on drop, for laying out an
/// `implements ... from` pair whose parent-directory relationship the test
/// controls directly (`inside` vs `outside`).
struct DirFixture {
    root: PathBuf,
}

impl DirFixture {
    fn new(name: &str) -> Self {
        let nonce = SystemTime::now()
            .duration_since(UNIX_EPOCH)
            .expect("clock after epoch")
            .as_nanos();
        let root = std::env::temp_dir().join(format!(
            "fsl-issue-1023-{name}-{}-{nonce}",
            std::process::id()
        ));
        std::fs::create_dir_all(&root).expect("create fixture root");
        Self { root }
    }

    fn path(&self, rel: &str) -> PathBuf {
        self.root.join(rel)
    }

    fn write(&self, rel: &str, content: &str) {
        let path = self.path(rel);
        if let Some(parent) = path.parent() {
            std::fs::create_dir_all(parent).expect("create fixture subdirectory");
        }
        std::fs::write(&path, content).expect("write fixture file");
    }
}

impl Drop for DirFixture {
    fn drop(&mut self) {
        let _ = std::fs::remove_dir_all(&self.root);
    }
}

const IMPL_SOURCE: &str = r#"requirements CountedReq {
  implements CountedFlow from "{dep}" { maps auto }

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

/// Runs the native CLI's `verify` against `path` with a caller-owned,
/// isolated `FSLC_CACHE_DIR`, and returns the parsed JSON envelope with its
/// process exit status. Does not pass `--no-cache`: the cache path under
/// test must run.
fn verify_cached(path: &Path, cache_dir: &Path) -> (Value, i32) {
    let output = Command::new(env!("CARGO_BIN_EXE_fslc"))
        .args(["verify"])
        .arg(path)
        .args(["--depth", "8"])
        .env("FSLC_CACHE_DIR", cache_dir)
        .output()
        .expect("run native CLI");
    let value = serde_json::from_slice(&output.stdout).unwrap_or_else(|error| {
        panic!(
            "invalid JSON: {error}; stderr={}",
            String::from_utf8_lossy(&output.stderr)
        )
    });
    (value, output.status.code().expect("native exit status"))
}

/// One (fixture, impl path, cache dir) triple, laid out with the dependency
/// either inside or outside the impl's parent directory.
fn build(
    name: &str,
    dep_rel_from_impl: &str,
    dep_rel_from_root: &str,
) -> (DirFixture, PathBuf, PathBuf) {
    let fixture = DirFixture::new(name);
    let cache_dir = fixture.path("cache");
    std::fs::create_dir_all(&cache_dir).expect("create cache dir");
    fixture.write(
        "proj/impl.fsl",
        &IMPL_SOURCE.replace("{dep}", dep_rel_from_impl),
    );
    fixture.write(dep_rel_from_root, &abstract_source("0"));
    let impl_path = fixture.path("proj/impl.fsl");
    (fixture, impl_path, cache_dir)
}

fn build_inside() -> (DirFixture, PathBuf, PathBuf) {
    build("inside", "abstract.fsl", "proj/abstract.fsl")
}

fn build_outside() -> (DirFixture, PathBuf, PathBuf) {
    build("outside", "../abstract.fsl", "abstract.fsl")
}

/// Matrix cell 1/4 (inside × before): a fresh, matching dependency verifies.
#[test]
fn inside_before_edit_verifies() {
    let (_fixture, impl_path, cache_dir) = build_inside();
    let (output, status) = verify_cached(&impl_path, &cache_dir);
    assert_eq!(status, 0, "{output:#}");
    assert_eq!(output["result"], "verified", "{output:#}");
}

/// Matrix cell 2/4 (inside × after). Preservation control: the dependency
/// lives inside the checked spec's own parent directory, so
/// `collect_fsl_sources`'s directory walk already covers it today. The fix
/// must not change this cell's outcome.
#[test]
fn inside_after_edit_invalidates_the_cache() {
    let (fixture, impl_path, cache_dir) = build_inside();
    let (before, before_status) = verify_cached(&impl_path, &cache_dir);
    assert_eq!(before_status, 0, "{before:#}");
    assert_eq!(before["result"], "verified", "{before:#}");

    fixture.write("proj/abstract.fsl", &abstract_source("1"));
    let (after, after_status) = verify_cached(&impl_path, &cache_dir);
    assert_eq!(
        after_status, 1,
        "in-parent dependency change must invalidate the cache: {after:#}"
    );
    assert_eq!(after["result"], "refinement_failed", "{after:#}");
    assert!(
        after.get("cache").is_none(),
        "a fresh (non-stale) verdict must not report a cache hit: {after:#}"
    );
}

/// Matrix cell 3/4 (outside × before): a fresh, matching dependency
/// verifies, exactly like the inside case — only file placement differs.
#[test]
fn outside_before_edit_verifies() {
    let (_fixture, impl_path, cache_dir) = build_outside();
    let (output, status) = verify_cached(&impl_path, &cache_dir);
    assert_eq!(status, 0, "{output:#}");
    assert_eq!(output["result"], "verified", "{output:#}");
}

/// Matrix cell 4/4 (outside × after). Detector: the dependency lives
/// outside the checked spec's parent directory, so
/// `collect_fsl_sources`'s directory walk never sees it. The cache key does
/// not change when the dependency does, so the lookup keeps serving the
/// `before` entry's stale `verified`/exit 0 verdict.
///
/// This assertion is expected to fail (red) until the cache key is built
/// from the resolver's actual read set instead of the parent-directory
/// walk.
#[test]
fn outside_after_edit_invalidates_the_cache() {
    let (fixture, impl_path, cache_dir) = build_outside();
    let (before, before_status) = verify_cached(&impl_path, &cache_dir);
    assert_eq!(before_status, 0, "{before:#}");
    assert_eq!(before["result"], "verified", "{before:#}");

    fixture.write("abstract.fsl", &abstract_source("1"));
    let (after, after_status) = verify_cached(&impl_path, &cache_dir);
    assert_eq!(
        after_status, 1,
        "out-of-parent dependency change must invalidate the cache, \
         not keep serving the stale pre-change verdict: {after:#}"
    );
    assert_eq!(after["result"], "refinement_failed", "{after:#}");
    assert!(
        after.get("cache").is_none(),
        "a fresh (non-stale) verdict must not report a cache hit: {after:#}"
    );
}

/// Behavior-change control (accepted plan, condition 2): once a cache hit
/// has to read its dependencies to know its key is still valid, it must
/// fail closed if a dependency it needs to read has been removed, instead
/// of replaying a verdict computed against a source that no longer exists.
///
/// Expected to fail (red) before the fix: today a cache hit never reads
/// dependencies at all, so removing the out-of-parent dependency after
/// priming a hit is invisible and the stale `verified`/exit 0 keeps being
/// served.
#[test]
fn cache_hit_with_removed_dependency_fails_closed() {
    let (fixture, impl_path, cache_dir) = build_outside();
    let (first, first_status) = verify_cached(&impl_path, &cache_dir);
    assert_eq!(first_status, 0, "{first:#}");
    assert_eq!(first["result"], "verified", "{first:#}");

    // Prime an exact cache hit before removing the dependency.
    let (primed, primed_status) = verify_cached(&impl_path, &cache_dir);
    assert_eq!(primed_status, 0, "{primed:#}");
    assert_eq!(
        primed.get("cache").and_then(|cache| cache.get("hit")),
        Some(&Value::Bool(true)),
        "{primed:#}"
    );

    std::fs::remove_file(fixture.path("abstract.fsl")).expect("remove dependency");
    let (after, after_status) = verify_cached(&impl_path, &cache_dir);
    assert_eq!(
        after_status, 2,
        "a cache hit must fail closed when a dependency it needs to read \
         can no longer be read, not replay a verdict computed against a \
         source that no longer exists: {after:#}"
    );
    assert_eq!(after["result"], "error", "{after:#}");
}
