// SPDX-License-Identifier: Apache-2.0

//! #1413 / O4 real-executor watchdog.
//! Phase I support API in `tests/support/test_executor.rs`:
//! `pub(crate) fn block_on_until<F: std::future::Future>(future: F,
//!     deadline: std::time::Instant) -> F::Output`.
//! The stdio `block_on` must delegate to this same deadline-driven executor;
//! the test must not get its own implementation. Deadline expiry must panic
//! with a message containing "deadline", including when Pending never wakes.

use std::sync::mpsc;
use std::time::{Duration, Instant};

#[path = "support/test_executor.rs"]
mod test_executor;

#[test]
fn a_never_waking_future_fails_within_the_executor_deadline() {
    let (send, receive) = mpsc::channel();
    let worker = std::thread::spawn(move || {
        let started = Instant::now();
        let result = std::panic::catch_unwind(|| {
            test_executor::block_on_until(
                std::future::pending::<()>(),
                started + Duration::from_millis(50),
            );
        });
        let _ = send.send((started.elapsed(), result));
    });
    let (elapsed, result) = receive
        .recv_timeout(Duration::from_secs(2))
        .expect("outer watchdog: block_on parked forever on a never-waking Pending future");
    worker.join().expect("watchdog worker");
    assert!(
        elapsed < Duration::from_secs(2),
        "executor bound: {elapsed:?}"
    );
    let panic = result.expect_err("a never-ready future must fail on deadline");
    let message = panic
        .downcast_ref::<String>()
        .map(String::as_str)
        .or_else(|| panic.downcast_ref::<&str>().copied())
        .expect("deadline panic has a readable message");
    assert!(message.contains("deadline"), "deadline failure: {message}");
}

#[test]
fn the_shared_executor_completes_a_future_that_yields_once() {
    struct YieldOnce(bool);
    impl std::future::Future for YieldOnce {
        type Output = usize;
        fn poll(
            mut self: std::pin::Pin<&mut Self>,
            cx: &mut std::task::Context<'_>,
        ) -> std::task::Poll<usize> {
            if self.0 {
                std::task::Poll::Ready(42)
            } else {
                self.0 = true;
                cx.waker().wake_by_ref();
                std::task::Poll::Pending
            }
        }
    }
    assert_eq!(
        test_executor::block_on_until(YieldOnce(false), Instant::now() + Duration::from_secs(2)),
        42
    );
}
