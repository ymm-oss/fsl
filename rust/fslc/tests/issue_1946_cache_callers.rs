// SPDX-License-Identifier: Apache-2.0

//! Issue #1946: the verify cache's callers, through the CLI.
//!
//! - `FSLC_CACHE_VERIFY=1` reports a divergence from a stored entry for
//!   `--engine auto` as it does for the other engines. Before #1946 the auto
//!   store wrote over the entry without comparing it.
//! - The commands whose internal verify runs with the cache off (`mutate`,
//!   `explain`, `html`, `ledger`) leave the cache empty, so a mutant's verdict
//!   is never stored or served (`docs/design/DESIGN-incremental-verify.md`
//!   §1 and §8).
//!
//! The in-process regressions for the key itself (a captured root against a
//! file that changed) are `issue_1946_*` in `rust/fslc/src/verification.rs`.

use std::path::{Path, PathBuf};
use std::sync::atomic::{AtomicU64, Ordering};

use serde_json::Value;

#[path = "support/fslc_cli.rs"]
mod fslc_cli;

static NEXT: AtomicU64 = AtomicU64::new(0);

struct Scratch(PathBuf);

impl Scratch {
    fn new(name: &str) -> Self {
        let dir = std::env::temp_dir().join(format!(
            "fsl-issue-1946-{name}-{}-{}",
            std::process::id(),
            NEXT.fetch_add(1, Ordering::Relaxed)
        ));
        let _ = std::fs::remove_dir_all(&dir);
        std::fs::create_dir_all(dir.join("cache")).expect("create scratch");
        std::fs::write(dir.join("proved.fsl"), PROVED).expect("write proved spec");
        std::fs::write(dir.join("violated.fsl"), VIOLATED).expect("write violated spec");
        Self(dir)
    }

    fn cache(&self) -> PathBuf {
        self.0.join("cache")
    }

    fn fslc(&self, args: &[&str], cache_verify: bool) -> (Value, i32) {
        let mut command = fslc_cli::command_with_cache_dir(self.cache());
        command.current_dir(&self.0).args(args);
        if cache_verify {
            command.env("FSLC_CACHE_VERIFY", "1");
        }
        let output = command.output().expect("run fslc");
        let stdout = String::from_utf8_lossy(&output.stdout);
        let value = serde_json::from_str(&stdout).unwrap_or(Value::String(stdout.into_owned()));
        (value, output.status.code().expect("exit code"))
    }

    /// Every stored verify entry, cross-depth pointers excluded.
    fn entries(&self) -> Vec<PathBuf> {
        let mut entries = Vec::new();
        let Ok(generations) = std::fs::read_dir(self.cache().join("verify")) else {
            return entries;
        };
        for generation in generations.flatten() {
            for shard in std::fs::read_dir(generation.path())
                .into_iter()
                .flatten()
                .flatten()
            {
                if shard.file_name() == "xdepth" {
                    continue;
                }
                for entry in std::fs::read_dir(shard.path())
                    .into_iter()
                    .flatten()
                    .flatten()
                {
                    entries.push(entry.path());
                }
            }
        }
        entries.sort();
        entries
    }
}

impl Drop for Scratch {
    fn drop(&mut self) {
        let _ = std::fs::remove_dir_all(&self.0);
    }
}

const PROVED: &str = "spec Callers1946 {
  state { a: 0..3 }
  init { a = 0 }
  action inc() {
    requires a < 3
    a = a + 1
  }
  invariant Bounded { a <= 3 }
}
";

const VIOLATED: &str = "spec Callers1946 {
  state { a: 0..3 }
  init { a = 0 }
  action inc() {
    requires a < 3
    a = a + 1
  }
  invariant Bounded { a <= 1 }
}
";

fn read_json(path: &Path) -> Value {
    serde_json::from_slice(&std::fs::read(path).expect("read entry")).expect("entry JSON")
}

fn entry_with_result(entries: &[PathBuf], result: &str) -> PathBuf {
    entries
        .iter()
        .find(|path| read_json(path)["output"]["result"] == result)
        .unwrap_or_else(|| panic!("no {result} entry among {entries:?}"))
        .clone()
}

#[test]
fn cache_verify_mode_reports_an_auto_store_divergence() {
    let scratch = Scratch::new("divergence");
    let explicit = ["verify", "proved.fsl", "--engine", "explicit"];
    let (proved, status) = scratch.fslc(&explicit, false);
    assert_eq!(
        (proved["result"].clone(), status),
        ("proved".into(), 0),
        "{proved:#}"
    );
    let (violated, status) =
        scratch.fslc(&["verify", "violated.fsl", "--engine", "explicit"], false);
    assert_eq!(
        (violated["result"].clone(), status),
        ("violated".into(), 1),
        "{violated:#}"
    );

    // Poison the proved spec's entry with the violated spec's output, keeping
    // the entry's own key, as an entry written under the wrong key looks.
    let entries = scratch.entries();
    assert_eq!(entries.len(), 2, "{entries:?}");
    let target = entry_with_result(&entries, "proved");
    let source = entry_with_result(&entries, "violated");
    let mut poisoned = read_json(&target);
    poisoned["output"] = read_json(&source)["output"].clone();
    std::fs::write(&target, serde_json::to_vec(&poisoned).expect("encode")).expect("poison");

    // Calibration: the poisoned entry is served to both engines that share it.
    for engine in ["explicit", "auto"] {
        let (served, status) = scratch.fslc(&["verify", "proved.fsl", "--engine", engine], false);
        assert_eq!(served["cache"]["hit"], true, "{engine}: {served:#}");
        assert_eq!(
            (served["result"].clone(), status),
            ("violated".into(), 1),
            "{engine}"
        );
    }

    // Under FSLC_CACHE_VERIFY=1 both engines re-run and report the divergence.
    for engine in ["explicit", "auto"] {
        let (checked, status) = scratch.fslc(&["verify", "proved.fsl", "--engine", engine], true);
        assert_eq!(checked["result"], "error", "{engine}: {checked:#}");
        assert_eq!(checked["kind"], "internal", "{engine}: {checked:#}");
        assert!(
            checked["message"]
                .as_str()
                .is_some_and(|message| message.starts_with("verify cache divergence")),
            "{engine}: {checked:#}"
        );
        assert_eq!(status, 3, "{engine}: {checked:#}");
    }
}

#[test]
fn cache_off_internal_verifies_leave_the_cache_empty() {
    let scratch = Scratch::new("cache-off-callers");
    for args in [
        &[
            "mutate",
            "violated.fsl",
            "--depth",
            "3",
            "--max-mutants",
            "3",
        ][..],
        &["mutate", "proved.fsl", "--depth", "3", "--max-mutants", "3"][..],
        &["explain", "violated.fsl", "--depth", "3"][..],
        &["html", "proved.fsl", "--depth", "3", "-o", "report.html"][..],
        &["ledger", "proved.fsl", "--depth", "3", "-o", "ledger.json"][..],
    ] {
        let (output, status) = scratch.fslc(args, false);
        assert!(status <= 1, "{args:?}: exit {status}: {output:#}");
        assert_eq!(
            scratch.entries(),
            Vec::<PathBuf>::new(),
            "{args:?} stored an entry"
        );
    }

    // Calibration: `verify` on the same cache stores its verdict.
    let (verified, status) = scratch.fslc(&["verify", "proved.fsl", "--depth", "3"], false);
    assert_eq!(status, 0, "{verified:#}");
    assert_eq!(scratch.entries().len(), 1);
}
