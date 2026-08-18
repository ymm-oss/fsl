// SPDX-License-Identifier: Apache-2.0
// Copyright 2026 Ryoichi Izumita

//! Controls on `support/fifo_snapshot.rs`'s own cleanup hardening, not on any
//! particular CLI command. #819 ported these from
//! `issue_796_domain_command_validation.rs`'s independently-hardened copy
//! into the shared module every #808 snapshot control now uses, and this
//! file is where they now belong: they exercise `TwoSnapshotFifo` and
//! `ReapedChild` directly rather than a specific `fslc` subcommand's
//! behavior.
//!
//! `fifo_cleanup_finishes_when_writer_panics_before_rename` and
//! `child_guard_reaps_after_esrch_kill_error` were raised as a major finding
//! during #803's round-4 review: a blocking cleanup `open` inside a
//! `catch_unwind`-guarded `Drop` cannot be interrupted by an assertion
//! failure, so the failure itself is silently swallowed by an indefinite
//! hang instead of being reported. `release_writer_completes_when_cli_never_
//! opens_the_path` is #819's own closing control: the exact #813-development
//! scenario (a CLI argument the parser rejects, so the process exits before
//! ever opening the FIFO path at all) reproduced directly, proving cleanup
//! still terminates.

#[cfg(unix)]
#[path = "support/fifo_snapshot.rs"]
mod fifo_snapshot;

#[cfg(unix)]
use std::process::Command;

#[cfg(unix)]
use fifo_snapshot::{ReapedChild, TwoSnapshotFifo, WriterMode, WriterOutcome};
#[cfg(unix)]
use serde_json::Value;

/// The pre-rename panic is the cleanup failure mode that used to leave the
/// second blocking FIFO open waiting forever. The writer outcome and
/// nonblocking control descriptors make completion independent of that
/// missing second writer.
#[cfg(unix)]
#[test]
fn fifo_cleanup_finishes_when_writer_panics_before_rename() {
    let mut fixture = TwoSnapshotFifo::new_with_writer_mode(
        "writer-panics-before-rename",
        "source A: never fully consumed",
        "source B: never reached",
        WriterMode::PanicBeforeRename,
    );

    assert_eq!(fixture.release_writer(), WriterOutcome::Panicked);
}

/// A deterministic ESRCH control for the timeout guard. Reading the pipe to
/// EOF proves the child has exited without reaping it; injecting its
/// possible kill result verifies that the guard still calls `wait` and
/// completes.
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

/// #819's closing negative control, reproducing #813's development incident
/// directly: `mutate` rejects an option its parser does not know before it
/// ever opens the spec path (see `main.rs`'s `"explain" | "mutate" |
/// "typestate"` arm, which returns `Err(format!("unknown {command} option
/// '{option}'"))` from argument parsing -- before `run_mutate` is ever
/// called, so nothing opens `path`). The FIFO writer thread is consequently
/// left blocked forever in its first `open`, since no reader ever attaches
/// to the pathname.
///
/// Before #819, `release_writer`'s cleanup itself performed a *blocking*
/// `open` gated on `self.second_writer_opened` (false here, since the writer
/// never even reaches its first successful open/write/rename), and
/// `writer.join()` afterward waited unconditionally -- so this exact
/// scenario is the one that hung CI for 30 minutes and a local run for 1h33m
/// during #813's development (the wrong-argument product bug was already
/// fixed; the hang was purely this cleanup defect). This test's oracle is
/// *termination*, not a particular writer outcome, so a straightforward
/// `assert_eq!(fixture.release_writer(), ...)` on the test thread would not
/// be faithful: were the hang regression to reappear, that call would block
/// the test thread forever and the whole run would need Ctrl-C or a harness
/// timeout to end -- exactly the failure mode #819 exists to convert into an
/// ordinary failing assertion. Running `release_writer` on a helper thread
/// and bounding the *test's own* wait with `recv_timeout` performs that
/// inversion faithfully: a regression now surfaces as a timed-out `recv`
/// (still inside this process, no external timeout needed) rather than a
/// hang, while the fixed implementation is expected to return quickly with a
/// determinate outcome -- draining the writer to completion through
/// nonblocking reader descriptors on both the original and (once reachable)
/// replacement FIFO paths is exactly what lets the writer's own blocking
/// opens succeed even though no CLI process ever attached as a reader, so
/// green here is `WriterOutcome::Finished`, not merely "did not panic".
/// A version of this test that skipped the helper thread and asserted
/// `WriterOutcome::Finished` directly on the test thread would have a green
/// result compatible with a hang (it simply would never reach the
/// assertion); the `recv_timeout` bound is what makes a hang observable as
/// red instead of silence.
#[cfg(unix)]
#[test]
fn release_writer_completes_when_cli_never_opens_the_path() {
    let mut fixture = TwoSnapshotFifo::new(
        "cli-exits-before-open",
        "source A: the CLI never opens the path at all",
        "source B: never reached either",
    );
    let path = fixture.fifo.to_string_lossy().into_owned();

    let output = Command::new(env!("CARGO_BIN_EXE_fslc"))
        .args(["mutate", &path, "--this-option-does-not-exist"])
        .current_dir(fifo_snapshot::root())
        .output()
        .expect("run native fslc with a rejected option");
    let value: Value = serde_json::from_slice(&output.stdout).unwrap_or_else(|error| {
        panic!(
            "invalid JSON: {error}; stderr={}",
            String::from_utf8_lossy(&output.stderr)
        )
    });
    assert_eq!(
        value["kind"], "usage",
        "the CLI must reject the unsupported option before opening the path: {value:#}"
    );
    assert_eq!(
        output.status.code(),
        Some(2),
        "usage rejection must exit 2: {value:#}"
    );

    // Nothing has opened `fixture.fifo`: the CLI's argument parser rejected
    // the command before `run_mutate` (and therefore before any file open)
    // ever ran. `assert_no_second_open` is consequently vacuous here (it
    // would also pass under the old hanging cleanup, which never returned
    // control at all) -- the real oracle is the bounded `recv_timeout` below.
    fixture.assert_no_second_open();

    let (sender, receiver) = std::sync::mpsc::channel();
    let handle = std::thread::spawn(move || {
        let outcome = fixture.release_writer();
        let _ = sender.send(outcome);
        // `fixture` drops here, on the helper thread, after `release_writer`
        // already joined the writer thread -- `Drop` finds `writer: None`
        // and does not attempt a second release.
    });

    let outcome = receiver
        .recv_timeout(std::time::Duration::from_secs(10))
        .unwrap_or_else(|_| {
            panic!(
                "release_writer did not return within 10s after the CLI exited \
                 without ever opening the FIFO path -- this is the exact #813/#819 \
                 hang this control exists to catch"
            )
        });

    assert_eq!(
        outcome,
        WriterOutcome::Finished,
        "the hardened cleanup must drain the writer to completion via nonblocking \
         reader descriptors even though no CLI process ever opened the path"
    );

    handle
        .join()
        .expect("join release_writer helper thread after it reported an outcome");
}

/// Windows does not implement the FIFO read-count control above. This
/// passing marker makes the platform exclusion visible in filtered test
/// output; it does not claim the Unix TOCTOU control ran on this target.
#[cfg(not(unix))]
#[test]
fn fifo_snapshot_hardening_control_is_unavailable_on_non_unix() {
    assert!(cfg!(not(unix)));
}
