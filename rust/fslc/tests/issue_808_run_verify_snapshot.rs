// SPDX-License-Identifier: Apache-2.0
// Copyright 2026 Ryoichi Izumita

//! Deterministic CLI read-count control for #808's `run_verify` foundation.

#[cfg(unix)]
use std::path::{Path, PathBuf};
#[cfg(unix)]
use std::process::Command;

#[cfg(unix)]
use serde_json::Value;

#[cfg(unix)]
fn root() -> PathBuf {
    Path::new(env!("CARGO_MANIFEST_DIR"))
        .parent()
        .and_then(Path::parent)
        .expect("workspace root")
        .to_owned()
}

#[cfg(unix)]
fn fixture_directory() -> PathBuf {
    use std::sync::atomic::{AtomicUsize, Ordering};

    static NEXT_FIFO: AtomicUsize = AtomicUsize::new(0);
    let directory = std::env::temp_dir().join(format!(
        "fslc-issue-808-fifo-{}-{}-{}",
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
#[cfg(unix)]
struct TwoSnapshotFifo {
    directory: PathBuf,
    fifo: PathBuf,
    second_opened: std::sync::mpsc::Receiver<()>,
    writer: Option<std::thread::JoinHandle<()>>,
    second_writer_opened: bool,
}

#[cfg(unix)]
impl TwoSnapshotFifo {
    fn new() -> Self {
        use std::io::Write;
        use std::sync::mpsc;

        let directory = fixture_directory();
        let fifo = directory.join("verify.fsl");
        let replacement = directory.join("verify-replacement.fsl");
        create_fifo(&fifo);
        create_fifo(&replacement);

        let source_a = include_str!("fixtures/vacuous_leadsto.fsl").to_owned();
        let source_b = "not valid FSL source".to_owned();
        let writer_fifo = fifo.clone();
        let (second_opened, receiver) = mpsc::channel();
        let writer = std::thread::spawn(move || {
            let mut first = std::fs::OpenOptions::new()
                .write(true)
                .open(&writer_fifo)
                .expect("open first FIFO for valid source A");
            first
                .write_all(source_a.as_bytes())
                .expect("write valid source A");
            std::fs::rename(&replacement, &writer_fifo).expect("replace FIFO path");
            drop(first);

            let mut second = std::fs::OpenOptions::new()
                .write(true)
                .open(&writer_fifo)
                .expect("open replacement FIFO for invalid source B");
            second_opened
                .send(())
                .expect("test must retain second-open receiver");
            second
                .write_all(source_b.as_bytes())
                .expect("write invalid source B");
        });

        Self {
            directory,
            fifo,
            second_opened: receiver,
            writer: Some(writer),
            second_writer_opened: false,
        }
    }

    fn assert_no_second_open(&mut self) {
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

    fn release_writer(&mut self) {
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

#[cfg(unix)]
fn wait_for_output(child: &mut std::process::Child) -> std::process::Output {
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

#[cfg(unix)]
fn verify_against_two_snapshot_fifo(engine: &str, edition: &str) -> (Value, i32) {
    use std::process::Stdio;

    let mut fixture = TwoSnapshotFifo::new();
    let path = fixture.fifo.to_string_lossy().into_owned();
    let mut child = Command::new(env!("CARGO_BIN_EXE_fslc"))
        .args([
            "verify",
            &path,
            "--depth",
            "4",
            "--engine",
            engine,
            "--edition",
            edition,
            "--no-cache",
        ])
        .current_dir(root())
        .stdout(Stdio::piped())
        .stderr(Stdio::piped())
        .spawn()
        .expect("spawn native fslc against FIFO");
    let output = wait_for_output(&mut child);

    // This is the correctness oracle. Cleanup opens B only after this point.
    fixture.assert_no_second_open();
    fixture.release_writer();

    let value = serde_json::from_slice(&output.stdout).unwrap_or_else(|error| {
        panic!(
            "invalid JSON: {error}; engine={engine}; stderr={}",
            String::from_utf8_lossy(&output.stderr)
        )
    });
    (value, output.status.code().expect("exit status"))
}

/// The BMC control also exercises source A's `leadsTo` property. Induction is
/// separately pinned because it has its own base/step and liveness paths.
///
/// This is intentionally Unix-only: Windows lacks the FIFO read-count oracle.
/// The non-Unix marker below makes a green Windows result explicitly not claim
/// that this CLI-level control ran there (#806).
#[cfg(unix)]
#[test]
fn verify_reads_one_fifo_snapshot_for_bmc_induction_and_liveness() {
    for (engine, edition) in [("bmc", "current"), ("induction", "next")] {
        let (output, status) = verify_against_two_snapshot_fifo(engine, edition);
        assert_eq!(status, 0, "{engine}: {output:#}");
        assert_ne!(
            output["result"], "error",
            "{engine} must verify valid source A, not invalid source B: {output:#}"
        );
    }
}

/// Windows does not implement the FIFO read-count control above. This marker
/// prevents a passing non-Unix test run from being mistaken for its evidence.
#[cfg(not(unix))]
#[test]
fn fifo_source_snapshot_control_is_unavailable_on_non_unix() {
    assert!(cfg!(not(unix)));
}
