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
//! [`command_with_cache_dir`]. It also removes the unstable gate
//! `FSLC_UNSTABLE` (#1818 D1), so a gate in the developer's shell never
//! reaches a child; a test that runs behind the gate sets it itself.
//!
//! "Per test" is per thread: libtest runs each test on its own thread, and
//! the directory is removed when that thread exits. A thread still running
//! when the process exits leaves its directory behind; the process id in the
//! name keeps a later run from reusing it. Directories are claimed with an
//! exclusive `create_dir`, so two copies of this module in one binary cannot
//! share one either.
//!
//! A test crate root declares `#[path = "support/fslc_cli.rs"] mod fslc_cli;`
//! once; a module nested under it uses `crate::fslc_cli`.
//! `tests/cli_cache_isolation_guard.rs` fails when a function spawns the
//! binary bare without its own cache setup.

use std::path::{Path, PathBuf};
use std::process::Command;
use std::sync::atomic::{AtomicU64, Ordering};

// Never recycle a name after Drop: Windows may still be completing deletion.
static NEXT_CACHE_DIR: AtomicU64 = AtomicU64::new(0);

struct TestCacheDir(PathBuf);

impl TestCacheDir {
    fn claim() -> Self {
        let root = Path::new(env!("CARGO_TARGET_TMPDIR")).join("fslc-test-cache");
        std::fs::create_dir_all(&root).expect("create test cache root");
        let process = std::process::id();
        for _ in 0..1024 {
            let attempt = NEXT_CACHE_DIR.fetch_add(1, Ordering::Relaxed);
            let dir = root.join(format!("{process}-{attempt}"));
            match std::fs::create_dir(&dir) {
                Ok(()) => return Self(dir),
                Err(error)
                    if matches!(
                        error.kind(),
                        std::io::ErrorKind::AlreadyExists | std::io::ErrorKind::PermissionDenied
                    ) => {}
                Err(error) => panic!("create test cache dir {}: {error}", dir.display()),
            }
        }
        panic!(
            "could not claim test cache dir under {} after 1024 attempts",
            root.display()
        );
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
    command
        .env("FSLC_CACHE_DIR", dir.as_ref())
        .env_remove("FSLC_CACHE")
        .env_remove("FSLC_CACHE_VERIFY")
        .env_remove("FSLC_UNSTABLE");
    command
}

/// Give a command that runs `fslc` indirectly (a script handed the binary
/// path, or a binary chosen by an override variable) the current test's
/// private verify cache.
#[allow(dead_code)]
pub fn isolate(target: &mut Command) -> &mut Command {
    for (key, value) in command().get_envs() {
        match value {
            Some(value) => target.env(key, value),
            None => target.env_remove(key),
        };
    }
    target
}
