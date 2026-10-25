// SPDX-License-Identifier: Apache-2.0
// Copyright 2026 Ryoichi Izumita

//! Small CLI fixture shared only by the compose rewrite regression tests.

use std::path::{Path, PathBuf};
use std::process::Command;
use std::sync::atomic::{AtomicU64, Ordering};

use serde_json::Value;

static NEXT_SCRATCH: AtomicU64 = AtomicU64::new(0);

pub fn scratch(label: &str, files: &[(&str, &str)]) -> PathBuf {
    let id = NEXT_SCRATCH.fetch_add(1, Ordering::Relaxed);
    let dir = Path::new(env!("CARGO_MANIFEST_DIR"))
        .parent()
        .expect("workspace")
        .join(format!("target/{label}-{}-{id}", std::process::id()));
    // No deletion or stale-fixture reuse: an unexpected existing directory is
    // a test setup failure, not a reason to overwrite someone else's files.
    std::fs::create_dir(&dir).expect("create fresh scratch dir");
    for (name, source) in files {
        std::fs::write(dir.join(name), source).expect("write fixture");
    }
    dir
}

pub fn run(dir: &Path, args: &[&str]) -> (Value, i32) {
    let output = Command::new(env!("CARGO_BIN_EXE_fslc"))
        .args(args)
        .current_dir(dir)
        .output()
        .expect("run native fslc");
    let json = serde_json::from_slice(&output.stdout).unwrap_or_else(|error| {
        panic!(
            "invalid JSON: {error}; stdout={}; stderr={}",
            String::from_utf8_lossy(&output.stdout),
            String::from_utf8_lossy(&output.stderr)
        )
    });
    (json, output.status.code().expect("exit status"))
}

pub fn check(dir: &Path, file: &str) -> (Value, i32) {
    run(dir, &["check", file])
}

pub fn verify(dir: &Path, file: &str) -> (Value, i32) {
    run(dir, &["verify", file, "--depth", "4", "--no-cache"])
}

#[allow(dead_code)]
pub fn verify_induction(dir: &Path, file: &str) -> (Value, i32) {
    run(
        dir,
        &["verify", file, "--engine", "induction", "--no-cache"],
    )
}
