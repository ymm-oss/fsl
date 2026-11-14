// SPDX-License-Identifier: Apache-2.0

//! The one way a CLI test spawns the native `fslc` binary (issue #1872).
//!
//! The verify cache is on by default and lives under `FSLC_CACHE_DIR`, then
//! `XDG_CACHE_HOME`, then `HOME`. A child that inherits the environment reads
//! and writes the developer's (or CI runner's) cache, so its result can depend
//! on what an earlier run left there. [`command`] gives every child a
//! per-test `FSLC_CACHE_DIR` under `CARGO_TARGET_TMPDIR` and removes
//! `FSLC_CACHE` and `FSLC_CACHE_VERIFY`. A test that asserts cache behaviour
//! shares a directory across commands explicitly with
//! [`command_with_cache_dir`].
//!
//! "Per test" is per thread: libtest runs each test on its own thread, and
//! the directory is removed when that thread exits. A consumer pulls this in
//! with `#[path = "support/fslc_cli.rs"] mod fslc_cli;`. A module may be
//! compiled into one test binary more than once, so directories are claimed
//! with an exclusive `create_dir` rather than a per-copy counter.
//! `tests/cli_cache_isolation_guard.rs` fails when a test file spawns the
//! binary without this module and without its own cache setup.

use std::path::{Path, PathBuf};
use std::process::Command;

struct TestCacheDir(PathBuf);

impl TestCacheDir {
    fn claim() -> Self {
        let root = Path::new(env!("CARGO_TARGET_TMPDIR")).join("fslc-test-cache");
        std::fs::create_dir_all(&root).expect("create test cache root");
        let process = std::process::id();
        let mut attempt = 0_u64;
        loop {
            let dir = root.join(format!("{process}-{attempt}"));
            match std::fs::create_dir(&dir) {
                Ok(()) => return Self(dir),
                Err(error) if error.kind() == std::io::ErrorKind::AlreadyExists => attempt += 1,
                Err(error) => panic!("create test cache dir {}: {error}", dir.display()),
            }
        }
    }
}

impl Drop for TestCacheDir {
    fn drop(&mut self) {
        let _ = std::fs::remove_dir_all(&self.0);
    }
}

thread_local! {
    static TEST_CACHE_DIR: TestCacheDir = TestCacheDir::claim();
}

/// The current test's private verify-cache directory.
#[allow(dead_code)]
pub fn cache_dir() -> PathBuf {
    TEST_CACHE_DIR.with(|dir| dir.0.clone())
}

/// `fslc` with the current test's private verify cache.
#[allow(dead_code)]
pub fn command() -> Command {
    command_with_cache_dir(cache_dir())
}

/// `fslc` with the verify cache at `dir`, for a test that shares one cache
/// across commands on purpose.
#[allow(dead_code)]
pub fn command_with_cache_dir(dir: impl AsRef<Path>) -> Command {
    let mut command = Command::new(env!("CARGO_BIN_EXE_fslc"));
    isolate_with_cache_dir(&mut command, dir);
    command
}

/// Give a command that runs `fslc` indirectly (a script handed the binary
/// path, or a binary chosen by an override variable) the current test's
/// private verify cache.
#[allow(dead_code)]
pub fn isolate(command: &mut Command) -> &mut Command {
    isolate_with_cache_dir(command, cache_dir())
}

fn isolate_with_cache_dir(command: &mut Command, dir: impl AsRef<Path>) -> &mut Command {
    command
        .env("FSLC_CACHE_DIR", dir.as_ref())
        .env_remove("FSLC_CACHE")
        .env_remove("FSLC_CACHE_VERIFY")
}
