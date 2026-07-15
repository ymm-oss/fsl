<!-- SPDX-License-Identifier: Apache-2.0 -->

# Rust-native integration gate

Status: Accepted

## Decision

The required product integration gate is `./tools/check-native-integration.sh`. It executes the
authoritative Rust workspace, dependency boundaries, the JavaScript solver Worker probe, and the
production WASM browser contract. It does not install or execute Python.

The frozen Python package remains an optional compatibility reference and the retained LSP surface.
Its tests are manual, focused evidence for changes explicitly scoped to compatibility or LSP; they
are not an evolving product specification and are not part of required CI.

## Contract inventory

| Contract | Required native evidence |
|---|---|
| CLI grammar, help, exit 2, envelopes, versions, cost | `fslc/tests/native_integration.rs` |
| Published JSON schema inventory and IDs | `native_integration.rs`, `kernel_contract.rs`, `conformance_coverage.rs` |
| Public Kernel, testgen, and code generation | `kernel_contract.rs`, `public_kernel_v2.rs`, `testgen_contract.rs`, `domain_codegen_contract.rs` |
| Registered FSL corpus formatting and semantics | `formatter_cli.rs` |
| Symbolic, Monitor, and explicit-state agreement | `fsl-verifier/tests/*agreement.rs`, `explicit_engine.rs`, `issue_226_auto_engine.rs` |
| Replayable evidence | `replay_trace_contract.rs` and native/WASM corpus replay tests |
| Native and browser envelope parity | `fsl-wasm/test-browser.mjs` |
| Runtime and WASM dependency boundaries | `check-native-integration.sh` |
| Claude/Codex repository hook environment | Manual `tests/test_claude_environment.py` and `tests/test_codex_environment.py` when those hooks change; non-product exception |

The removed `tests/test_rust_cli_contract.py` compared the evolving Rust CLI with a frozen Python
parser plus a large exception file. That made the frozen implementation a second authority. The
native tests instead compare every live help path with the checked-in embedded contract and directly
validate the published result/schema contracts. Existing Python parity utilities remain historical
or explicitly invoked compatibility evidence only.

The Claude/Codex environment tests execute Python repository hooks, not the FSL compiler product.
They remain focused manual tooling checks and are deliberately outside the required native product
gate; changes to those hooks must invoke the two named tests directly.

## Timing and portability

Immediately before this change, the separate Python contract job took 67 seconds on GitHub Actions,
including 44 seconds in pytest, while the Rust/WASM job took about 10 minutes on a cold runner. The
single gate removes the Python environment and job rather than hiding it behind another wrapper.
Native solver tests continue on Linux, macOS ARM, macOS Intel, and Windows for every change; generated
artifact digests normalize path separators, checked-in FSL source uses LF on every runner, and
testgen templates normalize line endings so identical text has one cross-platform identity.
