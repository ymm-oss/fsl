// SPDX-License-Identifier: Apache-2.0

//! #1413 / A-F2: continuous partial output cannot extend one frame's deadline.
//! The watchdog owns/kills/reaps the stub even when the receiver is broken.

use std::io::{Read, Write};
use std::process::{Command, Stdio};
use std::sync::mpsc;
use std::time::{Duration, Instant};

#[path = "support/lsp_receiver.rs"]
mod lsp_receiver;

const STUB: &str = "FSL_TEST_CONTINUOUS_PARTIAL_OUTPUT_STUB";
const READY: &[u8] = b"CONTINUOUS-PARTIAL-READY\n";
const FRAME_BUDGET: Duration = Duration::from_millis(50);
const OUTER_BOUND: Duration = Duration::from_secs(5);

#[test]
fn continuous_partial_output_stub() {
    if std::env::var_os(STUB).is_none() {
        return;
    }
    let mut stdout = std::io::stdout().lock();
    stdout.write_all(READY).unwrap();
    stdout.write_all(b"Content-Length: ").unwrap();
    loop {
        // Never finish the header; keep the receiver's chunk queue populated
        // without unbounded high-rate output from the test server.
        if stdout.write_all(&[b'7'; 4096]).is_err() || stdout.flush().is_err() {
            return;
        }
        std::thread::sleep(Duration::from_millis(1));
    }
}

#[test]
fn continuous_partial_output_hits_the_absolute_frame_deadline() {
    let mut child = Command::new(std::env::current_exe().unwrap())
        .args(["--exact", "continuous_partial_output_stub", "--nocapture"])
        .env(STUB, "1")
        .stdin(Stdio::piped())
        .stdout(Stdio::piped())
        .stderr(Stdio::inherit())
        .spawn()
        .expect("spawn continuous-output stub");
    let mut stdout = child.stdout.take().unwrap();
    let (send, receive) = mpsc::channel();
    let worker = std::thread::spawn(move || {
        let mut preamble = Vec::new();
        while !preamble.ends_with(READY) {
            let mut byte = [0];
            stdout.read_exact(&mut byte).expect("stub readiness marker");
            preamble.push(byte[0]);
        }
        let mut receiver = lsp_receiver::LspReceiver::new(stdout);
        // Prefill the queue so recv_timeout(0) cannot masquerade as an
        // absolute deadline check just because the producer was descheduled.
        std::thread::sleep(Duration::from_millis(100));
        let started = Instant::now();
        let result = receiver.recv(started + FRAME_BUDGET);
        let _ = send.send((started.elapsed(), result));
    });
    let observed = receive.recv_timeout(OUTER_BOUND);
    child.kill().expect("kill owned partial-output stub");
    child.wait().expect("reap owned partial-output stub");
    // Do not join a broken receiver on watchdog expiry: that would make the
    // outer bound itself unbounded while it scans its queued partial bytes.
    let (elapsed, result) = observed.expect(
        "outer watchdog: continuous partial output bypassed the absolute 50ms frame deadline",
    );
    worker.join().expect("receiver worker");
    assert!(
        elapsed < OUTER_BOUND,
        "absolute deadline exceeded: {elapsed:?}"
    );
    let error = result.expect_err("unfinished output must fail on deadline");
    assert!(error.contains("deadline"), "must not fail via EOF: {error}");
    assert!(
        error.contains("Content-Length: "),
        "partial header retained"
    );
}
