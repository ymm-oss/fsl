// SPDX-License-Identifier: Apache-2.0
// Copyright 2026 Ryoichi Izumita

//! Cross-command validation controls for #796.
//!
//! `domain analyze` projects the domain AST and `domain expand` renders
//! generated Kernel text, but neither may accept a source document that typed
//! domain lowering rejects. In particular, renderer string normalization can
//! leave an unknown authored identifier unchanged and previously produced a
//! Kernel source that `fslc check` rejected. Both commands must instead return
//! the same located semantic diagnostic as `check`.

use std::path::{Path, PathBuf};
use std::process::Command;

use serde_json::Value;

fn root() -> PathBuf {
    Path::new(env!("CARGO_MANIFEST_DIR"))
        .parent()
        .and_then(Path::parent)
        .expect("workspace root")
        .to_owned()
}

fn run(args: &[&str]) -> (Value, i32) {
    let output = Command::new(env!("CARGO_BIN_EXE_fslc"))
        .args(args)
        .current_dir(root())
        .output()
        .expect("run native fslc");
    let value = serde_json::from_slice(&output.stdout).unwrap_or_else(|error| {
        panic!(
            "invalid JSON: {error}; args={args:?}; stderr={}",
            String::from_utf8_lossy(&output.stderr)
        )
    });
    (value, output.status.code().expect("exit status"))
}

fn run_text(args: &[&str]) -> (String, i32) {
    let output = Command::new(env!("CARGO_BIN_EXE_fslc"))
        .args(args)
        .current_dir(root())
        .output()
        .expect("run native fslc");
    (
        String::from_utf8(output.stdout).expect("UTF-8 stdout"),
        output.status.code().expect("exit status"),
    )
}

#[cfg(unix)]
fn fifo_fixture_directory() -> PathBuf {
    use std::sync::atomic::{AtomicUsize, Ordering};

    static NEXT_FIFO: AtomicUsize = AtomicUsize::new(0);

    let directory = std::env::temp_dir().join(format!(
        "fslc-issue-796-fifo-{}-{}-{}",
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

/// A two-inode FIFO fixture that proves a command's source-read count.
///
/// The writer atomically replaces the path with a second FIFO *before* closing
/// the writer on the first FIFO. The command's initial reader is consequently
/// still connected to the old inode and must observe its EOF. Only a later
/// path open can reach the replacement FIFO, so the two sources cannot be
/// concatenated by scheduling.
#[cfg(unix)]
struct TwoSnapshotFifo {
    directory: PathBuf,
    fifo: PathBuf,
    second_opened: std::sync::mpsc::Receiver<()>,
    phase: std::sync::Arc<std::sync::atomic::AtomicUsize>,
    writer: Option<std::thread::JoinHandle<()>>,
    second_writer_opened: bool,
}

#[cfg(unix)]
impl TwoSnapshotFifo {
    const FIRST_WRITER_OPENING: usize = 1;
    const REPLACED: usize = 2;
    const SECOND_WRITER_OPENING: usize = 3;

    fn new() -> Self {
        use std::io::Write;
        use std::sync::atomic::Ordering;
        use std::sync::{Arc, mpsc};

        let directory = fifo_fixture_directory();
        let fifo = directory.join("domain.fsl");
        let replacement = directory.join("domain-replacement.fsl");
        create_fifo(&fifo);
        create_fifo(&replacement);

        let invalid =
            include_str!("fixtures/domain_characterization/invalid_unknown_name.fsl").to_owned();
        let valid =
            include_str!("fixtures/domain_characterization/expressions_valid.fsl").to_owned();
        let writer_fifo = fifo.clone();
        let phase = Arc::new(std::sync::atomic::AtomicUsize::new(0));
        let writer_phase = Arc::clone(&phase);
        let (second_opened, second_opened_receiver) = mpsc::channel();
        let writer = std::thread::spawn(move || {
            writer_phase.store(Self::FIRST_WRITER_OPENING, Ordering::SeqCst);
            let mut first = std::fs::OpenOptions::new()
                .write(true)
                .open(&writer_fifo)
                .expect("open first FIFO for invalid source");
            first
                .write_all(invalid.as_bytes())
                .expect("write invalid source");

            // `rename` replaces the directory entry, while `first` remains
            // connected to the old FIFO inode until this explicit drop.
            std::fs::rename(&replacement, &writer_fifo).expect("replace FIFO path");
            writer_phase.store(Self::REPLACED, Ordering::SeqCst);
            drop(first);

            writer_phase.store(Self::SECOND_WRITER_OPENING, Ordering::SeqCst);
            let mut second = std::fs::OpenOptions::new()
                .write(true)
                .open(&writer_fifo)
                .expect("open replacement FIFO for valid source");
            second_opened
                .send(())
                .expect("test must retain second-open receiver");
            second
                .write_all(valid.as_bytes())
                .expect("write valid source");
        });

        Self {
            directory,
            fifo,
            second_opened: second_opened_receiver,
            phase,
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
                panic!("the CLI opened the replacement FIFO, proving a second path read");
            }
            Err(TryRecvError::Disconnected) => {
                panic!("FIFO writer stopped before opening the replacement source");
            }
        }
    }

    fn drain_current_fifo(&self) -> std::io::Result<()> {
        use std::io::Read;

        let mut reader = std::fs::OpenOptions::new().read(true).open(&self.fifo)?;
        let mut source = String::new();
        reader.read_to_string(&mut source)?;
        Ok(())
    }

    fn release_writer(&mut self) {
        use std::sync::atomic::Ordering;

        let Some(writer) = self.writer.take() else {
            return;
        };

        if !self.second_writer_opened {
            // A timed-out multi-read mutant may already have consumed and
            // closed the replacement source. Do not open a FIFO reader with
            // no writer in that case.
            if self.second_opened.try_recv().is_ok() {
                self.second_writer_opened = true;
            }
            // This only releases test-owned blocked writers after the child
            // exits. It does not contribute evidence about the child's reads.
            if !self.second_writer_opened && self.phase.load(Ordering::SeqCst) < Self::REPLACED {
                self.drain_current_fifo()
                    .expect("drain first FIFO during cleanup");
                if self.second_opened.try_recv().is_ok() {
                    self.second_writer_opened = true;
                }
            }
            if !self.second_writer_opened {
                self.drain_current_fifo()
                    .expect("drain replacement FIFO during cleanup");
                self.second_writer_opened = true;
            }
        }

        writer.join().expect("join FIFO writer");
    }
}

#[cfg(unix)]
impl Drop for TwoSnapshotFifo {
    fn drop(&mut self) {
        // This runs during assertion unwinding as well, so the fixture cannot
        // leave FIFOs behind merely because a check failed.
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
fn wait_for_output_with_timeout(
    child: &mut std::process::Child,
    timeout: std::time::Duration,
) -> std::process::Output {
    use std::io::Read;

    let deadline = std::time::Instant::now() + timeout;
    let status = loop {
        if let Some(status) = child.try_wait().expect("poll FIFO child") {
            break status;
        }
        if std::time::Instant::now() >= deadline {
            child.kill().expect("kill timed-out FIFO child");
            let status = child.wait().expect("reap timed-out FIFO child");
            let mut stdout = Vec::new();
            let mut stderr = Vec::new();
            child
                .stdout
                .take()
                .expect("capture FIFO child stdout")
                .read_to_end(&mut stdout)
                .expect("read timed-out FIFO child stdout");
            child
                .stderr
                .take()
                .expect("capture FIFO child stderr")
                .read_to_end(&mut stderr)
                .expect("read timed-out FIFO child stderr");
            panic!(
                "fslc against FIFO timed out after {timeout:?}; status={status}; stdout={}; stderr={}",
                String::from_utf8_lossy(&stdout),
                String::from_utf8_lossy(&stderr)
            );
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

/// Runs a command against a rejecting FIFO inode, then makes a valid source
/// available only through a replacement inode for an illegal second path read.
#[cfg(unix)]
fn run_against_two_snapshot_fifo(command: &str) -> (String, i32) {
    use std::process::Stdio;

    let mut fixture = TwoSnapshotFifo::new();
    let fifo_argument = fixture.fifo.to_string_lossy().into_owned();
    let mut child = Command::new(env!("CARGO_BIN_EXE_fslc"))
        .args(["domain", command, &fifo_argument])
        .current_dir(root())
        .stdout(Stdio::piped())
        .stderr(Stdio::piped())
        .spawn()
        .expect("spawn native fslc against FIFO");
    let output = wait_for_output_with_timeout(&mut child, std::time::Duration::from_secs(5));

    // This assertion occurs before fixture cleanup opens the replacement FIFO.
    // Receiving here is therefore proof that the CLI itself opened it.
    fixture.assert_no_second_open();
    fixture.release_writer();

    (
        String::from_utf8(output.stdout).expect("UTF-8 stdout"),
        output.status.code().expect("exit status"),
    )
}

fn assert_rejected_like_check(fixture: &str) {
    let (check, check_status) = run(&["check", fixture]);
    assert_eq!(check_status, 2, "check: {check:#}");
    assert_eq!(check["result"], "error", "check: {check:#}");
    assert_eq!(check["kind"], "semantics", "check: {check:#}");
    assert!(check.get("loc").is_some(), "check: {check:#}");
    assert!(
        check.get("diagnostic_code").is_none(),
        "check unexpectedly has a diagnostic code: {check:#}"
    );

    for command in [["domain", "analyze"], ["domain", "expand"]] {
        let (actual, status) = run(&[command[0], command[1], fixture]);
        assert_eq!(status, 2, "{command:?}: {actual:#}");
        for field in ["result", "kind", "message", "loc", "diagnostic_code"] {
            assert_eq!(
                actual.get(field),
                check.get(field),
                "{command:?} must exactly match check's {field} field: actual={actual:#}, check={check:#}"
            );
        }
    }
}

/// Rejecting control: before #796 both commands returned exit 0 for this
/// unknown state reference, while `check` rejected it at the authored
/// expression. Keeping all three envelopes aligned prevents either command
/// from again producing a false-green analysis or an invalid Kernel export.
#[test]
fn domain_analyze_and_expand_reject_unknown_names_like_check() {
    assert_rejected_like_check(
        "rust/fslc/tests/fixtures/domain_characterization/invalid_unknown_name.fsl",
    );
}

/// `domain expand --output` must validate before writing: an invalid document
/// cannot create a partial Kernel file or overwrite an existing one.
#[test]
fn domain_expand_rejection_does_not_write_output() {
    let fixture = "rust/fslc/tests/fixtures/domain_characterization/invalid_unknown_name.fsl";
    let output_path = std::env::temp_dir().join(format!(
        "fslc-issue-796-{}-expand-output.fsl",
        std::process::id()
    ));
    std::fs::write(&output_path, "existing output").expect("write sentinel");
    let output = output_path.to_string_lossy().into_owned();

    let (actual, status) = run(&["domain", "expand", fixture, "--output", &output]);

    assert_eq!(status, 2, "{actual:#}");
    assert_eq!(actual["result"], "error", "{actual:#}");
    assert_eq!(actual["kind"], "semantics", "{actual:#}");
    assert_eq!(
        std::fs::read_to_string(&output_path).expect("read sentinel"),
        "existing output",
        "domain expand overwrote output after validation failed: {actual:#}"
    );
    std::fs::remove_file(&output_path).expect("remove sentinel");
}

/// #798 is the sibling generated-name case: direct lowering rejects authored
/// use of a generated enum member, but the renderer formerly left that already
/// qualified text in place, making a falsely valid Kernel. The shared lowering
/// validation rejects it before either command emits a success envelope.
#[test]
fn domain_analyze_and_expand_reject_generated_kernel_names_like_check() {
    assert_rejected_like_check(
        "rust/fslc/tests/fixtures/domain_characterization/ai_internal_name_misuse.fsl",
    );
}

/// Accepting control: valid documents retain both the structural analysis and
/// inspectable generated Kernel source paths after validation is introduced.
#[test]
fn domain_analyze_and_expand_still_accept_valid_domain_specs() {
    let fixture = "examples/domain/order_fulfillment_saga.fsl";

    let (analyze, analyze_status) = run(&["domain", "analyze", fixture]);
    assert_eq!(analyze_status, 0, "{analyze:#}");
    assert_eq!(analyze["result"], "analyzed");

    let (expand, expand_status) = run_text(&["domain", "expand", fixture]);
    assert_eq!(expand_status, 0, "{expand}");
    assert!(
        expand.starts_with("spec "),
        "domain expand must keep emitting raw Kernel source: {expand}"
    );
}

/// Deterministic external TOCTOU control for both #796 commands. A FIFO
/// observes filesystem reads directly: the fixed implementation reads it
/// once, so the first authored-invalid source must produce exit 2. Restoring
/// `validate_domain_command_input` to `load_kernel_model(path)` performs the
/// second read, receives the valid source, and makes this test fail.
///
/// This control is Unix-only because Windows has no FIFO equivalent. The
/// non-Unix test below deliberately names that absence, so a Windows green
/// result is not evidence that the FIFO read-count regression ran there.
#[cfg(unix)]
#[test]
fn domain_commands_validate_their_single_fifo_source_snapshot() {
    for command in ["analyze", "expand"] {
        let (stdout, status) = run_against_two_snapshot_fifo(command);
        assert_eq!(
            status, 2,
            "domain {command} must reject its first invalid FIFO snapshot; stdout={stdout}"
        );
        assert!(
            !stdout.starts_with("spec "),
            "domain {command} must not emit a Kernel for a rejected FIFO snapshot: {stdout}"
        );
        let output: Value =
            serde_json::from_str(&stdout).expect("rejected command must retain its JSON envelope");
        assert_eq!(output["result"], "error", "domain {command}: {output:#}");
        assert_eq!(output["kind"], "semantics", "domain {command}: {output:#}");
    }
}

/// Windows does not implement the FIFO read-count control above. This passing
/// marker makes the platform exclusion visible in filtered test output; it
/// does not claim the Unix TOCTOU control ran on this target.
#[cfg(not(unix))]
#[test]
fn fifo_source_snapshot_control_is_unavailable_on_non_unix() {
    assert!(cfg!(not(unix)));
}
