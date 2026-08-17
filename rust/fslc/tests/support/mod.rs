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

/// Two-inode FIFO read-count oracle shared by every #808 CLI-level snapshot
/// control (`issue_808_run_verify_snapshot.rs`, `issue_808_mutate_snapshot.rs`).
/// The first writer stays attached to the first inode while the pathname is
/// atomically replaced with a second FIFO, so source A can never be
/// concatenated with source B: only a second `open` of the pathname reaches
/// B. `assert_no_second_open` is the correctness oracle a caller pins its
/// assertions on *before* releasing the writer, since cleanup itself must
/// open the second inode to drain and join the writer thread.
#[cfg(unix)]
#[allow(dead_code)]
pub fn fifo_fixture_directory(prefix: &str) -> PathBuf {
    use std::sync::atomic::{AtomicUsize, Ordering};

    static NEXT_FIFO: AtomicUsize = AtomicUsize::new(0);
    let directory = std::env::temp_dir().join(format!(
        "fslc-{prefix}-fifo-{}-{}-{}",
        std::process::id(),
        std::time::SystemTime::now()
            .duration_since(std::time::UNIX_EPOCH)
            .expect("system clock after Unix epoch")
            .as_nanos(),
        NEXT_FIFO.fetch_add(1, Ordering::Relaxed),
    ));
    std::fs::create_dir(&directory).expect("create FIFO fixture directory");
    directory
}

#[cfg(unix)]
#[allow(dead_code)]
pub fn create_fifo(path: &Path) {
    let status = std::process::Command::new("mkfifo")
        .arg(path)
        .status()
        .expect("run mkfifo");
    assert!(status.success(), "mkfifo failed with {status}");
}

#[cfg(unix)]
#[allow(dead_code)]
pub struct TwoSnapshotFifo {
    directory: PathBuf,
    fifo: PathBuf,
    second_opened: std::sync::mpsc::Receiver<()>,
    writer: Option<std::thread::JoinHandle<()>>,
    second_writer_opened: bool,
}

#[cfg(unix)]
#[allow(dead_code)]
impl TwoSnapshotFifo {
    /// `prefix` names the temporary fixture directory (e.g. `"verify"`,
    /// `"mutate"`); `source_a`/`source_b` are the two snapshots the pathname
    /// exposes in sequence.
    pub fn new(prefix: &str, source_a: String, source_b: String) -> Self {
        use std::io::Write;
        use std::sync::mpsc;

        let directory = fifo_fixture_directory(prefix);
        let fifo = directory.join("root.fsl");
        let replacement = directory.join("root-replacement.fsl");
        create_fifo(&fifo);
        create_fifo(&replacement);

        let writer_fifo = fifo.clone();
        let (second_opened, receiver) = mpsc::channel();
        let writer = std::thread::spawn(move || {
            let mut first = std::fs::OpenOptions::new()
                .write(true)
                .open(&writer_fifo)
                .expect("open first FIFO for source A");
            first
                .write_all(source_a.as_bytes())
                .expect("write source A");
            std::fs::rename(&replacement, &writer_fifo).expect("replace FIFO path");
            drop(first);

            let mut second = std::fs::OpenOptions::new()
                .write(true)
                .open(&writer_fifo)
                .expect("open replacement FIFO for source B");
            second_opened
                .send(())
                .expect("test must retain second-open receiver");
            second
                .write_all(source_b.as_bytes())
                .expect("write source B");
        });

        Self {
            directory,
            fifo,
            second_opened: receiver,
            writer: Some(writer),
            second_writer_opened: false,
        }
    }

    pub fn path(&self) -> &Path {
        &self.fifo
    }

    /// The correctness oracle: fails if the pathname was opened a second
    /// time, proving a second root-path read reached source B.
    pub fn assert_no_second_open(&mut self) {
        use std::sync::mpsc::TryRecvError;

        match self.second_opened.try_recv() {
            Err(TryRecvError::Empty) => {}
            Ok(()) => {
                self.second_writer_opened = true;
                panic!("the CLI opened source B, proving a second root-path read");
            }
            Err(TryRecvError::Disconnected) => {
                panic!("FIFO writer stopped before opening the replacement source")
            }
        }
    }

    pub fn release_writer(&mut self) {
        use std::io::Read;

        let Some(writer) = self.writer.take() else {
            return;
        };
        if !self.second_writer_opened {
            self.second_writer_opened = self.second_opened.try_recv().is_ok();
        }
        if !self.second_writer_opened {
            let mut reader = std::fs::OpenOptions::new()
                .read(true)
                .open(&self.fifo)
                .expect("open replacement FIFO during cleanup");
            let mut source = String::new();
            reader
                .read_to_string(&mut source)
                .expect("drain replacement FIFO during cleanup");
        }
        writer.join().expect("join FIFO writer");
    }
}

#[cfg(unix)]
impl Drop for TwoSnapshotFifo {
    fn drop(&mut self) {
        if self.writer.is_some() {
            let _ = std::panic::catch_unwind(std::panic::AssertUnwindSafe(|| {
                self.release_writer();
            }));
        }
        let _ = std::fs::remove_file(&self.fifo);
        let _ = std::fs::remove_dir_all(&self.directory);
    }
}

/// Poll a spawned child to completion (5s deadline) and capture its output.
/// Shared by every #808 CLI-level FIFO test.
#[cfg(unix)]
#[allow(dead_code)]
pub fn wait_for_output(child: &mut std::process::Child) -> std::process::Output {
    use std::io::Read;

    let deadline = std::time::Instant::now() + std::time::Duration::from_secs(5);
    let status = loop {
        if let Some(status) = child.try_wait().expect("poll FIFO child") {
            break status;
        }
        if std::time::Instant::now() >= deadline {
            child.kill().expect("kill timed-out FIFO child");
            let status = child.wait().expect("reap timed-out FIFO child");
            panic!("fslc against FIFO timed out after 5s; status={status}");
        }
        std::thread::sleep(std::time::Duration::from_millis(10));
    };
    let mut stdout = Vec::new();
    let mut stderr = Vec::new();
    child
        .stdout
        .take()
        .expect("capture FIFO child stdout")
        .read_to_end(&mut stdout)
        .expect("read FIFO child stdout");
    child
        .stderr
        .take()
        .expect("capture FIFO child stderr")
        .read_to_end(&mut stderr)
        .expect("read FIFO child stderr");
    std::process::Output {
        status,
        stdout,
        stderr,
    }
}
