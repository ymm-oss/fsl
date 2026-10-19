// SPDX-License-Identifier: Apache-2.0

//! Process-wide settings every command runs under: the worker stack size, the
//! parse options, and the native executor for the solver futures.

use std::future::Future;
use std::pin::pin;
use std::task::{Context, Poll, Waker};

/// The stack every `fslc` run gets, on every platform.
///
/// 8 MiB is the Linux and macOS default for a process's main thread. Windows
/// gives 1 MiB, and `fslc refine` needs more than that on
/// `examples/agentic_rag` and `examples/multi_agent_system`: the same command
/// on the same bytes returned `refines` on Linux and aborted with
/// `has overflowed its stack` on Windows (issue #617). A verifier whose answer
/// depends on which machine ran it is the failure this repository spends most
/// of its effort on, and platform-dependent *crashing* is that failure in its
/// crudest form.
///
/// This makes the platforms agree at the Unix value. It does not make the
/// recursion bounded: `refine`'s depth grows with the spec's structure rather
/// than with `--depth` (measured — it aborts at `--depth 2` exactly as at 4,
/// while `specs/cart_*` survives at `--depth 8`), so a large enough spec will
/// exhaust any fixed stack. That is issue #620, and it wants a depth guard
/// that reports `kind:"internal"` instead of aborting.
pub(crate) const STACK_SIZE: usize = 8 * 1024 * 1024;

/// Parse options for this process, fixed once in [`main`](crate::main) before any parse.
static PARSE_OPTIONS: std::sync::OnceLock<fsl_syntax::ParseOptions> = std::sync::OnceLock::new();

/// The process's parse options. Every document parse in this binary's own
/// modules receives them (`docs/design/parse-entry-inventory.md`).
pub(crate) fn parse_options() -> &'static fsl_syntax::ParseOptions {
    PARSE_OPTIONS.get_or_init(fslc_rust::unstable_gate::from_environment)
}

pub(crate) fn block_on_native<F: Future>(future: F) -> F::Output {
    let mut future = pin!(future);
    let waker = Waker::noop();
    let mut context = Context::from_waker(waker);
    match future.as_mut().poll(&mut context) {
        Poll::Ready(result) => result,
        Poll::Pending => panic!("native Z3 backend unexpectedly yielded Pending"),
    }
}
