// SPDX-License-Identifier: Apache-2.0

//! #1413: a non-answering server causes a bounded deadline failure, not a hung
//! test. The outer watchdog is test-owned and still works with the legacy
//! receiver. A self-spawned test supplies the stub without a shell dependency.

use std::io::{Read, Write};
use std::process::{Command, Stdio};
use std::sync::mpsc;
use std::time::{Duration, Instant};

use serde_json::{Value, json};

#[path = "support/lsp_receiver.rs"]
mod lsp_receiver;

use lsp_receiver::LspReceiver;

const STUB_MODE: &str = "FSL_TEST_RECEIVE_DEADLINE_STUB";
const READY: &[u8] = b"DEADLINE-STUB-READY\n";
const DEADLINE: Duration = Duration::from_millis(250);
const OUTER_BOUND: Duration = Duration::from_secs(5);

#[test]
fn deadline_stub_server() {
    let Ok(mode) = std::env::var(STUB_MODE) else {
        return;
    };
    let mut output = std::io::stdout().lock();
    // The parent consumes the libtest preamble through this marker, one byte at
    // a time, before handing the still-open pipe to the receiver under test.
    output.write_all(READY).unwrap();
    match mode.as_str() {
        "silent" => {}
        "partial-header" => output.write_all(b"Content-Length: ").unwrap(),
        "partial-body" => output
            .write_all(b"Content-Length: 40\r\n\r\n{\"jsonrpc\":")
            .unwrap(),
        "complete" => {
            for value in [json!({"id":1}), json!({"id":2})] {
                let body = serde_json::to_vec(&value).unwrap();
                write!(output, "Content-Length: {}\r\n\r\n", body.len()).unwrap();
                output.write_all(&body).unwrap();
            }
        }
        other => panic!("unknown stub mode {other}"),
    }
    output.flush().unwrap();
    // Keep stdout open indefinitely. Only the parent closes/kills this stub;
    // there is no self-expiry that could disguise an EOF as a deadline.
    std::io::stdin().read_to_end(&mut Vec::new()).unwrap();
}

fn probe(mode: &str, frames: usize) -> Vec<Result<Value, String>> {
    let mut child = Command::new(std::env::current_exe().unwrap())
        .args(["--exact", "deadline_stub_server", "--nocapture"])
        .env(STUB_MODE, mode)
        .stdin(Stdio::piped())
        .stdout(Stdio::piped())
        .stderr(Stdio::inherit())
        .spawn()
        .expect("spawn stub server");
    let mut stdout = child.stdout.take().expect("stub stdout");
    let (send, receive) = mpsc::channel();
    let worker = std::thread::spawn(move || {
        let mut preamble = Vec::new();
        while !preamble.ends_with(READY) {
            let mut byte = [0];
            stdout.read_exact(&mut byte).expect("stub readiness marker");
            preamble.push(byte[0]);
        }
        let mut receiver = LspReceiver::new(stdout);
        let deadline = Instant::now() + DEADLINE;
        let results = (0..frames)
            .map(|_| receiver.recv(deadline))
            .collect::<Vec<_>>();
        let _ = send.send(results);
    });
    let result = receive.recv_timeout(OUTER_BOUND);
    // Release blocked reads before joining, on success AND on watchdog expiry.
    child.kill().expect("stop owned stub server");
    child.wait().expect("reap owned stub server");
    worker.join().expect("receiver worker did not panic");
    result.expect(
        "outer timeout after 5s: receiver ignored its 250ms deadline while stub stdout stayed open",
    )
}

#[test]
fn a_server_that_never_answers_hits_the_receive_deadline() {
    let result = probe("silent", 1).pop().unwrap();
    let error = result.expect_err("silent server must hit its receive deadline");
    assert!(
        error.contains("deadline"),
        "deadline failure, not EOF: {error}"
    );
}

#[test]
fn an_unfinished_header_is_reported_on_deadline_expiry() {
    let result = probe("partial-header", 1).pop().unwrap();
    let error = result.expect_err("partial header must hit its receive deadline");
    assert!(error.contains("deadline"), "deadline failure: {error}");
    assert!(
        error.contains("Content-Length: "),
        "partial header is retained: {error}"
    );
}

#[test]
fn an_unfinished_body_is_reported_on_deadline_expiry() {
    let result = probe("partial-body", 1).pop().unwrap();
    let error = result.expect_err("partial body must hit its receive deadline");
    assert!(error.contains("deadline"), "deadline failure: {error}");
    assert!(
        error.contains("{\"jsonrpc\":"),
        "partial body is retained: {error}"
    );
}

#[test]
fn two_buffered_frames_are_received_in_order() {
    assert_eq!(
        probe("complete", 2),
        [Ok(json!({"id":1})), Ok(json!({"id":2}))]
    );
}
