// SPDX-License-Identifier: Apache-2.0

//! Stack discipline for recursion over user-controlled spec structure (#620).
//!
//! Recursion whose depth tracks the *spec's* structure -- not a `--depth` bound
//! the user chose -- must not be able to abort the process. An abort returns
//! neither the JSON envelope nor an exit code, so it leaves the outcome
//! projection contract entirely: it is the one failure mode the delivery layer
//! cannot even report. See `docs/DESIGN-rust-component-internals.md` 4.4.
//!
//! The accepted mechanism is segmented stack growth, not a depth limit. A limit
//! would put an arbitrary constant into the language contract and reject
//! legitimate machine-generated specs. Growth preserves every current answer
//! and adds none.
//!
//! This crate owns the constants because it is the workspace's root crate:
//! `fsl-core`, `fsl-verifier`, `fsl-runtime`, and `fsl-tools` all reach it, and
//! a per-crate copy of the numbers would drift.
//!
//! # Placement rule
//!
//! Call [`guard`] at a recursion **cycle entry** -- the one function every path
//! around the cycle passes through -- not on each recursive arm. Guarding
//! `SyntaxParser::expression` guards the whole grammar because `prefix`,
//! `atom`, and `postfix` all re-enter it; guarding `eval` guards
//! `eval_binary`/`eval_equality_operands` for the same reason. A guard on every
//! arm buys nothing and costs a stack probe per node.
//!
//! Adding a *new* recursion over expression structure means adding its guard:
//! the deep-nesting regression in `rust/fslc/tests/deep_nesting.rs` is what
//! catches an unguarded one, and the `unguarded-recursion` fault operator
//! proves that test still detects the guard's absence.

/// Headroom required before recursing one more level.
///
/// Measured on a debug arm64 build, one level of a right-nested `if` chain
/// costs ~25-30 KiB in the parser and in `into_kernel`, and more under symbolic
/// evaluation. The check runs once per level, so the red zone has to cover one
/// level plus the un-guarded helper frames between two checks; 256 KiB leaves
/// roughly an order of magnitude of margin over the measured worst level, which
/// matters because a release build and a different target move those constants.
const RED_ZONE: usize = 256 * 1024;

/// Stack segment allocated when the red zone is not available. Large enough
/// that growth is amortized over tens of levels rather than paid per level.
const NEW_STACK_SIZE: usize = 4 * 1024 * 1024;

/// Runs `body` with at least [`RED_ZONE`] bytes of stack available, allocating
/// a new segment if the current stack is closer to its limit than that.
///
/// On targets where the stack cannot be grown -- `wasm32`, which is how
/// `fsl-wasm` compiles `fsl-verifier` for the browser -- `stacker` calls `body`
/// directly, which is exactly today's behavior there. Browser-side depth stays
/// bounded by the host, as before.
pub fn guard<R>(body: impl FnOnce() -> R) -> R {
    stacker::maybe_grow(RED_ZONE, NEW_STACK_SIZE, body)
}
