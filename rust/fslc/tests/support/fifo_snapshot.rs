// SPDX-License-Identifier: Apache-2.0
// Copyright 2026 Ryoichi Izumita

//! Shared Unix CLI-level #808/#796 read-count oracle.
//!
//! `TwoSnapshotFifo` binds a first writer to a first-created FIFO inode
//! while atomically replacing the pathname with a second FIFO, so the CLI
//! process cannot see source A concatenated with source B: only a second
//! `open` of the pathname would reach B. Extracted from
//! `issue_808_run_verify_snapshot.rs` (PR 1, #811) so
//! `issue_808_testgen_scenarios_snapshot.rs` (PR 3, #808) and
//! `issue_808_mutate_snapshot.rs` can reuse the same oracle instead of
//! re-deriving it. Pulled in per-file with
//! `#[path = "support/fifo_snapshot.rs"] mod fifo_snapshot;`, matching the
//! existing `support/self_conformance_mapping.rs` convention -- not through
//! `support/mod.rs`, which is unrelated corpus-walk tooling.
//!
//! #819 ported the cleanup hardening `issue_796_domain_command_validation.rs`
//! grew during its own review into this shared copy and deleted that file's
//! duplicate implementation, so all six FIFO-oracle tests now share one
//! hardened oracle instead of an unhardened one drifting from a hardened one.
//! Three properties motivate every piece of the hardening below:
//!
//! 1. `release_writer`'s cleanup path must never perform a *blocking* `open`
//!    of the FIFO. Opening a FIFO for reading blocks until a writer attaches;
//!    if the writer thread never reached its second `open` (it panicked, or
//!    the CLI under test exited before opening the path at all -- see #813's
//!    development, where a rejected CLI argument caused exactly this), a
//!    blocking cleanup `open` hangs forever.
//! 2. `Drop` funnels into that same cleanup path via `catch_unwind`, which
//!    cannot interrupt a blocking syscall. A blocking `open` in `Drop` turns
//!    an assertion failure mid-test into a silent hang instead of a reported
//!    failure.
//! 3. A timed-out child must still be reaped even if `kill` itself fails
//!    (e.g. `ESRCH` because the child exited between the timeout poll and the
//!    kill): `ReapedChild::terminate_and_reap_with` calls `wait`
//!    unconditionally, and `Drop for ReapedChild` is the backstop if the
//!    caller never reaps explicitly.

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

/// Outcome of the FIFO writer thread, reported by `release_writer` and by
/// `Drop`'s best-effort cleanup so callers can tell an orderly finish apart
/// from a caught writer panic.
#[allow(dead_code)]
#[derive(Debug, Eq, PartialEq)]
pub enum WriterOutcome {
    Finished,
    Panicked,
}

/// Selects whether the writer thread completes normally or panics after
/// writing source A but before the atomic rename to the replacement FIFO.
/// `PanicBeforeRename` drives the cleanup-hardening control that proves
/// `release_writer` terminates even when the writer never reaches its second
/// `open`.
#[allow(dead_code)]
#[derive(Clone, Copy)]
pub enum WriterMode {
    Normal,
    PanicBeforeRename,
}

/// The first writer remains attached to the first inode while the pathname is
/// atomically replaced with the second FIFO. Thus source A cannot be
/// concatenated with source B: only a second open of the pathname reaches B.
#[allow(dead_code)]
pub struct TwoSnapshotFifo {
    directory: PathBuf,
    pub fifo: PathBuf,
    second_opened: std::sync::mpsc::Receiver<()>,
    writer_outcome: std::sync::mpsc::Receiver<WriterOutcome>,
    phase: std::sync::Arc<std::sync::atomic::AtomicUsize>,
    writer: Option<std::thread::JoinHandle<()>>,
}

#[allow(dead_code)]
impl TwoSnapshotFifo {
    const FIRST_WRITER_OPENING: usize = 1;
    const REPLACED: usize = 2;
    const SECOND_WRITER_OPENING: usize = 3;
    const CLEANUP_TIMEOUT: std::time::Duration = std::time::Duration::from_secs(5);

    /// `label` distinguishes concurrent fixture directories across test
    /// binaries. `source_a` and `source_b` should normally be two *distinct
    /// valid* FSL documents (not merely valid vs. invalid): a caller wants to
    /// pin the CLI's output content to A, and a fallback that reread the path
    /// and got a different-but-still-valid B would otherwise look identical
    /// to a correct run under a status/`result != "error"` check alone.
    pub fn new(label: &str, source_a: &str, source_b: &str) -> Self {
        Self::new_with_writer_mode(label, source_a, source_b, WriterMode::Normal)
    }

    /// As `new`, but drives the writer thread through `mode`. Used by the
    /// cleanup-hardening control to force a writer panic before the FIFO path
    /// is ever replaced, so `release_writer` must terminate without a second
    /// `open` ever becoming reachable.
    pub fn new_with_writer_mode(
        label: &str,
        source_a: &str,
        source_b: &str,
        mode: WriterMode,
    ) -> Self {
        use std::io::Write;
        use std::sync::atomic::Ordering;
        use std::sync::{Arc, mpsc};

        let directory = fixture_directory(label);
        let fifo = directory.join("input.fsl");
        let replacement = directory.join("input-replacement.fsl");
        create_fifo(&fifo);
        create_fifo(&replacement);

        let source_a = source_a.to_owned();
        let source_b = source_b.to_owned();
        let writer_fifo = fifo.clone();
        let phase = Arc::new(std::sync::atomic::AtomicUsize::new(0));
        let writer_phase = Arc::clone(&phase);
        let (second_opened, second_opened_receiver) = mpsc::channel();
        let (writer_outcome, writer_outcome_receiver) = mpsc::channel();
        let writer = std::thread::spawn(move || {
            let result = std::panic::catch_unwind(std::panic::AssertUnwindSafe(|| {
                writer_phase.store(Self::FIRST_WRITER_OPENING, Ordering::SeqCst);
                let mut first = std::fs::OpenOptions::new()
                    .write(true)
                    .open(&writer_fifo)
                    .expect("open first FIFO for source A");
                first
                    .write_all(source_a.as_bytes())
                    .expect("write source A");
                if matches!(mode, WriterMode::PanicBeforeRename) {
                    panic!("intentional FIFO writer panic before rename");
                }

                // `rename` replaces the directory entry, while `first` remains
                // connected to the old FIFO inode until this explicit drop.
                std::fs::rename(&replacement, &writer_fifo).expect("replace FIFO path");
                writer_phase.store(Self::REPLACED, Ordering::SeqCst);
                drop(first);

                writer_phase.store(Self::SECOND_WRITER_OPENING, Ordering::SeqCst);
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
            }));
            let outcome = if result.is_ok() {
                WriterOutcome::Finished
            } else {
                WriterOutcome::Panicked
            };
            let _ = writer_outcome.send(outcome);
        });

        Self {
            directory,
            fifo,
            second_opened: second_opened_receiver,
            writer_outcome: writer_outcome_receiver,
            phase,
            writer: Some(writer),
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
                panic!("the CLI opened source B, proving a second root-path read");
            }
            Err(TryRecvError::Disconnected) => {
                panic!("FIFO writer stopped before opening the replacement source")
            }
        }
    }

    /// A nonblocking reader on the current FIFO path. `O_NONBLOCK` makes this
    /// `open` return immediately (with `ENXIO`-free semantics on a FIFO that
    /// already has a writer, or simply no data yet) instead of blocking until
    /// a writer attaches -- the property a cleanup path must have, since a
    /// writer may never attach at all (see the module doc's point 1).
    fn open_nonblocking_control_fifo(&self) -> std::io::Result<std::fs::File> {
        use std::os::unix::fs::OpenOptionsExt;

        std::fs::OpenOptions::new()
            .read(true)
            .custom_flags(libc::O_NONBLOCK)
            .open(&self.fifo)
    }

    fn drain_nonblocking_control_fifo(control: &mut std::fs::File) -> std::io::Result<()> {
        use std::io::Read;

        let mut bytes = [0; 4096];
        loop {
            match control.read(&mut bytes) {
                Ok(0) => return Ok(()),
                Ok(_) => {}
                Err(error) if error.kind() == std::io::ErrorKind::WouldBlock => return Ok(()),
                Err(error) => return Err(error),
            }
        }
    }

    /// Releases the writer thread without ever performing a blocking `open`.
    /// Holds nonblocking reader descriptors on both the original and (once
    /// reachable) the replacement FIFO path, draining them so the writer's
    /// blocking `open`/`write_all` calls can complete, and bounds the wait by
    /// `CLEANUP_TIMEOUT` instead of joining unconditionally. This cannot hang
    /// even if the writer panics before ever reaching the rename: the
    /// nonblocking descriptors do not depend on a second writer existing, and
    /// the loop below only joins once `writer.is_finished()` is true.
    pub fn release_writer(&mut self) -> WriterOutcome {
        use std::sync::atomic::Ordering;
        use std::sync::mpsc::TryRecvError;

        let Some(writer) = self.writer.take() else {
            return WriterOutcome::Finished;
        };

        let mut controls = vec![
            self.open_nonblocking_control_fifo()
                .expect("open nonblocking FIFO cleanup control"),
        ];
        // Always start `false`, even though `phase` may already read
        // `REPLACED` here: control #1 above was opened against whichever
        // inode the pathname pointed to at that moment, which can race
        // ahead of this read and land on the *first* inode while `phase`
        // has already advanced. Deciding `true` from `phase` alone can
        // therefore skip opening a control on the replacement inode
        // entirely, stranding the writer's second `open` and failing the
        // `CLEANUP_TIMEOUT` assert below instead of draining it. Starting
        // `false` lets the loop's own `phase` check decide on its next
        // iteration, guaranteeing a control on the replacement inode.
        // Duplicating a control on the same (first) inode when control #1
        // already happened to land on the replacement is harmless: both
        // are nonblocking, cleanup only discards what it reads, and it
        // does not matter which of two draining readers gets which bytes.
        let mut replacement_control_opened = false;
        let deadline = std::time::Instant::now() + Self::CLEANUP_TIMEOUT;
        let mut outcome = None;

        loop {
            if !replacement_control_opened && self.phase.load(Ordering::SeqCst) >= Self::REPLACED {
                controls.push(
                    self.open_nonblocking_control_fifo()
                        .expect("open replacement FIFO cleanup control"),
                );
                replacement_control_opened = true;
            }

            for control in &mut controls {
                Self::drain_nonblocking_control_fifo(control)
                    .expect("drain nonblocking FIFO cleanup control");
            }

            if outcome.is_none() {
                match self.writer_outcome.try_recv() {
                    Ok(received) => outcome = Some(received),
                    Err(TryRecvError::Empty) => {}
                    Err(TryRecvError::Disconnected) => {
                        panic!("FIFO writer exited without reporting an outcome");
                    }
                }
            }

            if writer.is_finished() {
                let outcome = outcome.expect("finished FIFO writer must report an outcome");
                writer.join().expect("join finished FIFO writer");
                return outcome;
            }

            assert!(
                std::time::Instant::now() < deadline,
                "FIFO writer cleanup exceeded {:?}",
                Self::CLEANUP_TIMEOUT
            );
            std::thread::sleep(std::time::Duration::from_millis(1));
        }
    }
}

#[allow(dead_code)]
impl Drop for TwoSnapshotFifo {
    fn drop(&mut self) {
        // This runs during assertion unwinding as well, so the fixture cannot
        // leave FIFOs (or a hung writer thread) behind merely because a check
        // failed. `catch_unwind` only guards against `release_writer` itself
        // panicking (e.g. its internal timeout assertion); it does not need
        // to guard against a blocking syscall because `release_writer` no
        // longer performs one.
        if self.writer.is_some() {
            let _ = std::panic::catch_unwind(std::panic::AssertUnwindSafe(|| {
                let _ = self.release_writer();
            }));
        }
        let _ = std::fs::remove_file(&self.fifo);
        let _ = std::fs::remove_dir_all(&self.directory);
    }
}

/// Wraps a spawned child so a reap is never skipped, including when `kill`
/// itself fails (e.g. `ESRCH` because the child already exited) and including
/// on drop if the caller never reaps explicitly.
#[allow(dead_code)]
pub struct ReapedChild {
    pub child: std::process::Child,
    reaped: bool,
}

#[allow(dead_code)]
impl ReapedChild {
    pub fn new(child: std::process::Child) -> Self {
        Self {
            child,
            reaped: false,
        }
    }

    pub fn try_wait(&mut self) -> std::io::Result<Option<std::process::ExitStatus>> {
        let status = self.child.try_wait()?;
        if status.is_some() {
            self.reaped = true;
        }
        Ok(status)
    }

    pub fn wait(&mut self) -> std::io::Result<std::process::ExitStatus> {
        let status = self.child.wait()?;
        self.reaped = true;
        Ok(status)
    }

    /// `wait` is deliberately unconditional: a process can exit between a
    /// timeout poll and kill, making kill return `ESRCH` while still leaving
    /// a child for this parent to reap. Returning the terminate error
    /// separately (rather than `.expect()`-ing it) is exactly what lets the
    /// reap proceed regardless of whether termination itself succeeded.
    pub fn terminate_and_reap_with<F>(
        &mut self,
        terminate: F,
    ) -> (
        Option<std::io::Error>,
        std::io::Result<std::process::ExitStatus>,
    )
    where
        F: FnOnce(&mut std::process::Child) -> std::io::Result<()>,
    {
        let terminate_error = terminate(&mut self.child).err();
        let status = self.wait();
        (terminate_error, status)
    }

    fn capture_output(&mut self, status: std::process::ExitStatus) -> std::process::Output {
        use std::io::Read;

        let mut stdout = Vec::new();
        let mut stderr = Vec::new();
        self.child
            .stdout
            .take()
            .expect("capture FIFO child stdout")
            .read_to_end(&mut stdout)
            .expect("read FIFO child stdout");
        self.child
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
}

#[allow(dead_code)]
impl Drop for ReapedChild {
    fn drop(&mut self) {
        if !self.reaped {
            // Ignore errors during unwinding, but never let a failed kill
            // skip the reap attempt.
            let _ = self.child.kill();
            let _ = self.wait();
        }
    }
}

#[allow(dead_code)]
pub fn wait_for_output(child: &mut ReapedChild) -> std::process::Output {
    let deadline = std::time::Instant::now() + std::time::Duration::from_secs(5);
    let status = loop {
        if let Some(status) = child.try_wait().expect("poll FIFO child") {
            break status;
        }
        if std::time::Instant::now() >= deadline {
            let (kill_error, status) = child.terminate_and_reap_with(std::process::Child::kill);
            let status = status.unwrap_or_else(|wait_error| {
                panic!("reap timed-out FIFO child after kill result {kill_error:?}: {wait_error}")
            });
            let output = child.capture_output(status);
            panic!(
                "fslc against FIFO timed out after 5s; kill_error={kill_error:?}; status={status}; stdout={}; stderr={}",
                String::from_utf8_lossy(&output.stdout),
                String::from_utf8_lossy(&output.stderr)
            );
        }
        std::thread::sleep(std::time::Duration::from_millis(10));
    };
    child.capture_output(status)
}
