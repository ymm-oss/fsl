// SPDX-License-Identifier: Apache-2.0
// Copyright 2026 Ryoichi Izumita

//! Corpus-walk helpers shared by every `specs/`+`examples/` corpus test
//! (issue #645, #537 C4). Before this module, `corpus_check_sweep.rs` and
//! `refine_corpus_parity.rs` each carried an identical copy of `root`,
//! `collect_fsl_files`, `repo_relative`, `headers`, and `top_level_keyword`;
//! adding `corpus_expectation_manifest.rs` and `evidence_corpus_manifest.rs`
//! as two more copies is exactly the "same logic in 2+ places" duplication
//! the project's implementation policy forbids. One copy here, reused by
//! all four.
//!
//! Placed at `tests/support/mod.rs` (not `tests/support.rs`) so Cargo does
//! not compile it as its own top-level integration-test binary; each
//! consumer pulls it in with `mod support;`. Every item is `pub` because a
//! given consumer only needs a subset, and `#[allow(dead_code)]` is applied
//! per item because each integration test is compiled as its own binary
//! crate, where an unused `pub` item still triggers the lint.

use std::collections::BTreeMap;
use std::path::{Path, PathBuf};

/// Workspace root, resolved from `CARGO_MANIFEST_DIR` (`rust/fslc`) two
/// levels up.
#[allow(dead_code)]
pub fn root() -> PathBuf {
    Path::new(env!("CARGO_MANIFEST_DIR"))
        .parent()
        .and_then(Path::parent)
        .expect("workspace root")
        .to_owned()
}

/// Recursively collect every `.fsl` file under `dir`.
#[allow(dead_code)]
pub fn collect_fsl_files(dir: &Path, out: &mut Vec<PathBuf>) {
    for entry in std::fs::read_dir(dir).expect("read corpus directory") {
        let path = entry.expect("read corpus entry").path();
        if path.is_dir() {
            collect_fsl_files(&path, out);
        } else if path.extension().is_some_and(|extension| extension == "fsl") {
            out.push(path);
        }
    }
}

/// Every `.fsl` file under `specs/` + `examples/`, repo-relative, sorted.
#[allow(dead_code)]
pub fn corpus_files(root: &Path) -> Vec<PathBuf> {
    let mut files = Vec::new();
    collect_fsl_files(&root.join("specs"), &mut files);
    collect_fsl_files(&root.join("examples"), &mut files);
    files.sort();
    files
}

/// `path`, relative to `root`, with forward slashes on every platform.
#[allow(dead_code)]
pub fn repo_relative(root: &Path, path: &Path) -> String {
    path.strip_prefix(root)
        .expect("path under workspace root")
        .to_string_lossy()
        .replace('\\', "/")
}

/// The file's top-level dialect keyword (`spec`, `requirements`,
/// `refinement`, `governance`, `causal`, ...): the first token on the first
/// non-blank, non-`//`-comment line.
#[allow(dead_code)]
pub fn top_level_keyword(source: &str) -> Option<&str> {
    source
        .lines()
        .map(str::trim)
        .find(|line| !line.is_empty() && !line.starts_with("//"))
        .and_then(|line| line.split_whitespace().next())
}

/// Parse the `// key: value` header comments in the first 10 lines: the
/// `expected-command` / `expected-result` / `expected-kind` /
/// `expected-helper` convention `examples/gallery/{valid,errors,adversarial}`
/// use.
#[allow(dead_code)]
pub fn headers(source: &str) -> BTreeMap<String, String> {
    let mut out = BTreeMap::new();
    for line in source.lines().take(10) {
        let Some(body) = line.trim().strip_prefix("//") else {
            continue;
        };
        if let Some((key, value)) = body.trim().split_once(':') {
            out.insert(key.trim().to_owned(), value.trim().to_owned());
        }
    }
    out
}
