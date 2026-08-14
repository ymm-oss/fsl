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
fn fifo_path() -> PathBuf {
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
    directory.join("domain.fsl")
}

/// Runs a command against a FIFO that serves a rejecting domain source on its
/// first open and an accepting source on its second open.
///
/// The writer's second open blocks until a reader exists. Thus the CLI either
/// validates the first source snapshot and exits before that open (the fixed
/// behavior), or it performs a second path read and necessarily consumes the
/// accepting replacement (the pre-fix mutant). No scheduling delay is used.
#[cfg(unix)]
fn run_against_two_snapshot_fifo(command: &str) -> (String, i32) {
    use std::io::{Read, Write};
    use std::sync::mpsc;

    let fifo = fifo_path();
    let status = Command::new("mkfifo")
        .arg(&fifo)
        .status()
        .expect("run mkfifo");
    assert!(status.success(), "mkfifo failed with {status}");

    let invalid =
        include_str!("fixtures/domain_characterization/invalid_unknown_name.fsl").to_owned();
    let valid = include_str!("fixtures/domain_characterization/expressions_valid.fsl").to_owned();
    let writer_fifo = fifo.clone();
    let (second_opened, second_opened_receiver) = mpsc::channel();
    let writer = std::thread::spawn(move || {
        let mut first = std::fs::OpenOptions::new()
            .write(true)
            .open(&writer_fifo)
            .expect("open FIFO for invalid source");
        first
            .write_all(invalid.as_bytes())
            .expect("write invalid source");
        drop(first);

        let mut second = std::fs::OpenOptions::new()
            .write(true)
            .open(&writer_fifo)
            .expect("open FIFO for valid source");
        // A second CLI read cannot finish until this signal has been sent, so
        // `try_recv` below distinguishes the mutant without a timing race.
        second_opened
            .send(())
            .expect("test must retain second-open receiver");
        second
            .write_all(valid.as_bytes())
            .expect("write valid source");
    });

    let fifo_argument = fifo.to_string_lossy().into_owned();
    let output = Command::new(env!("CARGO_BIN_EXE_fslc"))
        .args(["domain", command, &fifo_argument])
        .current_dir(root())
        .output()
        .expect("run native fslc against FIFO");

    match second_opened_receiver.try_recv() {
        Ok(()) => {
            writer.join().expect("join second FIFO writer");
        }
        Err(mpsc::TryRecvError::Empty) => {
            // The fixed command has no second read. Open once solely to drain
            // and join the intentionally blocked writer; this is after the
            // CLI has exited and cannot affect its observed input.
            let mut drain = std::fs::OpenOptions::new()
                .read(true)
                .open(&fifo)
                .expect("open FIFO to drain unused valid source");
            let mut valid_source = String::new();
            drain
                .read_to_string(&mut valid_source)
                .expect("drain unused valid source");
            assert!(
                valid_source.starts_with("domain "),
                "cleanup must receive the valid second source"
            );
            writer.join().expect("join FIFO writer");
        }
        Err(mpsc::TryRecvError::Disconnected) => {
            panic!("FIFO writer stopped before opening the valid source");
        }
    }

    std::fs::remove_file(&fifo).expect("remove FIFO");
    std::fs::remove_dir(
        fifo.parent()
            .expect("FIFO fixture must have a parent directory"),
    )
    .expect("remove FIFO fixture directory");

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
