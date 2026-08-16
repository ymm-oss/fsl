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
    writer_outcome: std::sync::mpsc::Receiver<WriterOutcome>,
    phase: std::sync::Arc<std::sync::atomic::AtomicUsize>,
    writer: Option<std::thread::JoinHandle<()>>,
}

#[cfg(unix)]
#[derive(Debug, Eq, PartialEq)]
enum WriterOutcome {
    Finished,
    Panicked,
}

#[cfg(unix)]
#[derive(Clone, Copy)]
enum WriterMode {
    Normal,
    PanicBeforeRename,
}

#[cfg(unix)]
impl TwoSnapshotFifo {
    const FIRST_WRITER_OPENING: usize = 1;
    const REPLACED: usize = 2;
    const SECOND_WRITER_OPENING: usize = 3;
    const CLEANUP_TIMEOUT: std::time::Duration = std::time::Duration::from_secs(5);

    fn new() -> Self {
        Self::new_with_writer_mode(WriterMode::Normal)
    }

    fn new_with_writer_mode(mode: WriterMode) -> Self {
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
        let (writer_outcome, writer_outcome_receiver) = mpsc::channel();
        let writer = std::thread::spawn(move || {
            let result = std::panic::catch_unwind(std::panic::AssertUnwindSafe(|| {
                writer_phase.store(Self::FIRST_WRITER_OPENING, Ordering::SeqCst);
                let mut first = std::fs::OpenOptions::new()
                    .write(true)
                    .open(&writer_fifo)
                    .expect("open first FIFO for invalid source");
                first
                    .write_all(invalid.as_bytes())
                    .expect("write invalid source");
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
                    .expect("open replacement FIFO for valid source");
                second_opened
                    .send(())
                    .expect("test must retain second-open receiver");
                second
                    .write_all(valid.as_bytes())
                    .expect("write valid source");
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

    fn assert_no_second_open(&mut self) {
        use std::sync::mpsc::TryRecvError;

        match self.second_opened.try_recv() {
            Err(TryRecvError::Empty) => {}
            Ok(()) => {
                panic!("the CLI opened the replacement FIFO, proving a second path read");
            }
            Err(TryRecvError::Disconnected) => {
                panic!("FIFO writer stopped before opening the replacement source");
            }
        }
    }

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

    fn release_writer(&mut self) -> WriterOutcome {
        use std::sync::atomic::Ordering;
        use std::sync::mpsc::TryRecvError;

        let Some(writer) = self.writer.take() else {
            return WriterOutcome::Finished;
        };

        // These retained, nonblocking descriptors release only test-owned FIFO
        // writers. They cannot hang if the writer panicked before rename: the
        // outcome channel closes/completes, and the bounded wait below joins
        // only after `is_finished()` proves the thread has terminated.
        let mut controls = vec![
            self.open_nonblocking_control_fifo()
                .expect("open nonblocking FIFO cleanup control"),
        ];
        let mut replacement_control_opened = self.phase.load(Ordering::SeqCst) >= Self::REPLACED;
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

#[cfg(unix)]
impl Drop for TwoSnapshotFifo {
    fn drop(&mut self) {
        // This runs during assertion unwinding as well, so the fixture cannot
        // leave FIFOs behind merely because a check failed.
        if self.writer.is_some() {
            let _ = std::panic::catch_unwind(std::panic::AssertUnwindSafe(|| {
                let _ = self.release_writer();
            }));
        }
        let _ = std::fs::remove_file(&self.fifo);
        let _ = std::fs::remove_dir_all(&self.directory);
    }
}

#[cfg(unix)]
struct ReapedChild {
    child: std::process::Child,
    reaped: bool,
}

#[cfg(unix)]
impl ReapedChild {
    fn new(child: std::process::Child) -> Self {
        Self {
            child,
            reaped: false,
        }
    }

    fn try_wait(&mut self) -> std::io::Result<Option<std::process::ExitStatus>> {
        let status = self.child.try_wait()?;
        if status.is_some() {
            self.reaped = true;
        }
        Ok(status)
    }

    fn wait(&mut self) -> std::io::Result<std::process::ExitStatus> {
        let status = self.child.wait()?;
        self.reaped = true;
        Ok(status)
    }

    fn terminate_and_reap_with<F>(
        &mut self,
        terminate: F,
    ) -> (
        Option<std::io::Error>,
        std::io::Result<std::process::ExitStatus>,
    )
    where
        F: FnOnce(&mut std::process::Child) -> std::io::Result<()>,
    {
        // `wait` is deliberately unconditional: a process can exit between a
        // timeout poll and kill, making kill return ESRCH while still leaving
        // a child for this parent to reap.
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

#[cfg(unix)]
impl Drop for ReapedChild {
    fn drop(&mut self) {
        if !self.reaped {
            // Ignore errors during unwinding, but never let a failed kill skip
            // the reap attempt.
            let _ = self.child.kill();
            let _ = self.wait();
        }
    }
}

#[cfg(unix)]
fn wait_for_output_with_timeout(
    child: &mut ReapedChild,
    timeout: std::time::Duration,
) -> std::process::Output {
    use std::io::Read;

    let deadline = std::time::Instant::now() + timeout;
    let status = loop {
        if let Some(status) = child.try_wait().expect("poll FIFO child") {
            break status;
        }
        if std::time::Instant::now() >= deadline {
            let (kill_error, status) = child.terminate_and_reap_with(std::process::Child::kill);
            let status = status.unwrap_or_else(|wait_error| {
                panic!("reap timed-out FIFO child after kill result {kill_error:?}: {wait_error}")
            });
            let mut stdout = Vec::new();
            let mut stderr = Vec::new();
            child
                .child
                .stdout
                .take()
                .expect("capture FIFO child stdout")
                .read_to_end(&mut stdout)
                .expect("read timed-out FIFO child stdout");
            child
                .child
                .stderr
                .take()
                .expect("capture FIFO child stderr")
                .read_to_end(&mut stderr)
                .expect("read timed-out FIFO child stderr");
            panic!(
                "fslc against FIFO timed out after {timeout:?}; kill_error={kill_error:?}; status={status}; stdout={}; stderr={}",
                String::from_utf8_lossy(&stdout),
                String::from_utf8_lossy(&stderr)
            );
        }
        std::thread::sleep(std::time::Duration::from_millis(10));
    };

    child.capture_output(status)
}

/// Runs a command against a rejecting FIFO inode, then makes a valid source
/// available only through a replacement inode for an illegal second path read.
#[cfg(unix)]
fn run_against_two_snapshot_fifo(command: &str) -> (String, i32) {
    use std::process::Stdio;

    let mut fixture = TwoSnapshotFifo::new();
    let fifo_argument = fixture.fifo.to_string_lossy().into_owned();
    let mut child = ReapedChild::new(
        Command::new(env!("CARGO_BIN_EXE_fslc"))
            .args(["domain", command, &fifo_argument])
            .current_dir(root())
            .stdout(Stdio::piped())
            .stderr(Stdio::piped())
            .spawn()
            .expect("spawn native fslc against FIFO"),
    );
    let output = wait_for_output_with_timeout(&mut child, std::time::Duration::from_secs(5));

    // This assertion occurs before fixture cleanup opens the replacement FIFO.
    // Receiving here is therefore proof that the CLI itself opened it.
    fixture.assert_no_second_open();
    assert_eq!(fixture.release_writer(), WriterOutcome::Finished);

    (
        String::from_utf8(output.stdout).expect("UTF-8 stdout"),
        output.status.code().expect("exit status"),
    )
}

/// The pre-rename panic is the cleanup failure mode that used to leave the
/// second blocking FIFO open waiting forever. The writer outcome and retained
/// nonblocking control FD make completion independent of that missing writer.
#[cfg(unix)]
#[test]
fn fifo_cleanup_finishes_when_writer_panics_before_rename() {
    let mut fixture = TwoSnapshotFifo::new_with_writer_mode(WriterMode::PanicBeforeRename);

    assert_eq!(fixture.release_writer(), WriterOutcome::Panicked);
}

/// A deterministic ESRCH control for the timeout guard. Reading the pipe to
/// EOF proves the child has exited without reaping it; injecting its possible
/// kill result verifies that the guard still calls `wait` and completes.
#[cfg(unix)]
#[test]
fn child_guard_reaps_after_esrch_kill_error() {
    use std::io::Read;
    use std::process::Stdio;

    let mut child = ReapedChild::new(
        Command::new("sh")
            .args(["-c", "exit 0"])
            .stdout(Stdio::piped())
            .stderr(Stdio::piped())
            .spawn()
            .expect("spawn already-exiting child"),
    );
    let mut stdout = Vec::new();
    child
        .child
        .stdout
        .as_mut()
        .expect("capture child stdout")
        .read_to_end(&mut stdout)
        .expect("wait for child stdout EOF");
    let (kill_error, status) =
        child.terminate_and_reap_with(|_| Err(std::io::Error::from_raw_os_error(libc::ESRCH)));

    assert_eq!(
        kill_error.and_then(|error| error.raw_os_error()),
        Some(libc::ESRCH),
        "the control must exercise the ESRCH kill result"
    );
    assert!(
        status.expect("reap child after ESRCH").success(),
        "already-exited child must reap successfully"
    );
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
/// use of a generated enum member, but the renderer still leaves that already
/// qualified text unchanged. Before #796 the CLI emitted that falsely valid
/// Kernel; shared lowering validation now rejects it before either command
/// emits a success envelope.
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
