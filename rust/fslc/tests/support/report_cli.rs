// SPDX-License-Identifier: Apache-2.0

use std::path::{Path, PathBuf};
use std::process::Command;

use serde_json::Value;

pub fn repository_root() -> PathBuf {
    Path::new(env!("CARGO_MANIFEST_DIR"))
        .parent()
        .and_then(Path::parent)
        .expect("repository root")
        .to_path_buf()
}

pub fn run(command: &str, fixture: &str, depth: usize, extra: &[&str]) -> (String, i32) {
    let mut command_line = Command::new(env!("CARGO_BIN_EXE_fslc"));
    command_line
        .args([
            command,
            fixture,
            "--depth",
            &depth.to_string(),
            "--deadlock",
            "warn",
        ])
        .args(extra)
        .current_dir(repository_root())
        .env("FSLC_CACHE", "off")
        .env_remove("FSLC_CACHE_VERIFY");
    if command == "verify" {
        command_line.arg("--no-cache");
    }
    let output = command_line.output().expect("run native fslc");
    let stdout = String::from_utf8(output.stdout).expect("UTF-8 CLI output");
    assert!(
        !stdout.is_empty(),
        "{command} {fixture}: empty stdout; stderr={}",
        String::from_utf8_lossy(&output.stderr)
    );
    (stdout, output.status.code().expect("native exit status"))
}

pub fn run_json(command: &str, fixture: &str, depth: usize, extra: &[&str]) -> (Value, i32) {
    let (stdout, status) = run(command, fixture, depth, extra);
    let value = serde_json::from_str(&stdout)
        .unwrap_or_else(|error| panic!("{command} {fixture}: invalid JSON ({error}): {stdout}"));
    (value, status)
}
