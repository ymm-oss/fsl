// SPDX-License-Identifier: Apache-2.0
// Copyright 2026 Ryoichi Izumita

//! Shared Unix CLI-level #808 read-count oracle.
//!
//! `TwoSnapshotFifo` binds a first writer to a first-created FIFO inode
//! while atomically replacing the pathname with a second FIFO, so the CLI
//! process cannot see source A concatenated with source B: only a second
//! `open` of the pathname would reach B. Extracted from
//! `issue_808_run_verify_snapshot.rs` (PR 1, #811) so
//! `issue_808_testgen_scenarios_snapshot.rs` (PR 3, #808) can reuse the same
//! oracle instead of re-deriving it. Pulled in per-file with
//! `#[path = "support/fifo_snapshot.rs"] mod fifo_snapshot;`, matching the
//! existing `support/self_conformance_mapping.rs` convention -- not through
//! `support/mod.rs`, which is unrelated corpus-walk tooling.
//!
//! A future #808 PR touching `mutate` may want the same oracle; extracting
//! it here risks a merge conflict with that PR if it adds its own copy or
//! its own extraction concurrently.

#![cfg(unix)]

use std::path::{Path, PathBuf};
use std::process::Command;

#[allow(dead_code)]
pub fn root() -> PathBuf {
    Path::new(env!("CARGO_MANIFEST_DIR"))
        .parent()
        .and_then(Path::parent)
        .expect("workspace root")
        .to_owned()
}

#[allow(dead_code)]
fn fixture_directory(label: &str) -> PathBuf {
    use std::sync::atomic::{AtomicUsize, Ordering};

    static NEXT_FIFO: AtomicUsize = AtomicUsize::new(0);
    let directory = std::env::temp_dir().join(format!(
        "fslc-issue-808-{label}-fifo-{}-{}-{}",
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

#[allow(dead_code)]
fn create_fifo(path: &Path) {
    let status = Command::new("mkfifo")
        .arg(path)
        .status()
        .expect("run mkfifo");
    assert!(status.success(), "mkfifo failed with {status}");
}

/// The first writer remains attached to the first inode while the pathname is
/// atomically replaced with the second FIFO. Thus source A cannot be
/// concatenated with source B: only a second open of the pathname reaches B.
#[allow(dead_code)]
pub struct TwoSnapshotFifo {
    directory: PathBuf,
    pub fifo: PathBuf,
    second_opened: std::sync::mpsc::Receiver<()>,
    writer: Option<std::thread::JoinHandle<()>>,
    second_writer_opened: bool,
}

#[allow(dead_code)]
impl TwoSnapshotFifo {
    /// `label` distinguishes concurrent fixture directories across test
    /// binaries. `source_a` and `source_b` should normally be two *distinct
    /// valid* FSL documents (not merely valid vs. invalid): a caller wants to
    /// pin the CLI's output content to A, and a fallback that reread the path
    /// and got a different-but-still-valid B would otherwise look identical
    /// to a correct run under a status/`result != "error"` check alone.
    pub fn new(label: &str, source_a: &str, source_b: &str) -> Self {
        use std::io::Write;
        use std::sync::mpsc;

        let directory = fixture_directory(label);
        let fifo = directory.join("input.fsl");
        let replacement = directory.join("input-replacement.fsl");
        create_fifo(&fifo);
        create_fifo(&replacement);

        let source_a = source_a.to_owned();
        let source_b = source_b.to_owned();
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

    /// The correctness oracle: must be called before `release_writer`, so
    /// cleanup opening the replacement FIFO cannot be mistaken for the CLI
    /// itself having read source B.
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

#[allow(dead_code)]
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
