#!/usr/bin/env bash
# SPDX-License-Identifier: Apache-2.0

set -euo pipefail

root="$(cd "$(dirname "$0")/.." && pwd)"
cd "$root"

assert_dependency_absent() {
  local package="$1" pattern="$2" message="$3" tree
  tree="$(cargo tree --manifest-path rust/Cargo.toml -p "$package" --edges normal)"
  if grep -E "$pattern" <<<"$tree"; then
    echo "$message" >&2
    exit 1
  fi
}

check_rust() {
  check_stack_parity
  cargo fmt --manifest-path rust/Cargo.toml --all -- --check
  cargo clippy --manifest-path rust/Cargo.toml --workspace --all-targets --locked -- -D warnings
  cargo test --manifest-path rust/Cargo.toml -p fsl-lsp --lib --locked
  cargo test --manifest-path rust/Cargo.toml -p fsl-lsp --test stdio --locked
  cargo test --manifest-path rust/Cargo.toml -p fsl-lsp --test corpus --locked
  cargo test --manifest-path rust/Cargo.toml --workspace --exclude fsl-lsp --locked
  cargo build --manifest-path rust/Cargo.toml --workspace --locked

  check_boundaries
}

check_boundaries() {
  assert_dependency_absent fsl-runtime 'fsl-solver|z3' 'fsl-runtime must remain solver-independent'
  assert_dependency_absent fsl-wasm 'fsl-solver-z3 v' 'fsl-wasm must not depend on the native Z3 backend'
}

# Implementation fault operators (#537 C5): would the suite notice if the
# verifier started lying? Deliberately kept out of `check_rust` — it patches a
# scratch checkout and rebuilds `fslc` there once per operator, and
# `docs/DESIGN-conformance-harness.md` puts that rebuild cost in the product
# gate rather than in the phase every pull request runs.
check_fault_operators() {
  ./tools/run-fault-operators.sh
}

check_semantic_mutation() {
  ./tools/run-semantic-mutation-gate.sh "${1:-complete}"
}

check_fsl_logic() {
  ./tools/run-fsl-logic-test.sh "${1:-pr}"
}

# Negative control for issue #617: `fslc` must not depend on the operating
# system's choice of main-thread stack.
#
# Windows gives 1 MiB where Linux and macOS give 8, and `fslc refine` needed
# more than 1 on `examples/agentic_rag`: it answered `refines` on Linux and
# aborted with `has overflowed its stack` on Windows, from the same bytes. The
# fix gives every platform 8 MiB explicitly; this asserts it, on the platforms
# that can ask for a smaller one.
#
# `ulimit -s 1024` makes a Unix runner reproduce the Windows condition, so the
# regression is caught here rather than only post-merge on the Windows matrix
# — which is where it actually escaped to, unnoticed for three merges. Depth 2
# is deliberate: the abort does not depend on `--depth` (it happened at 2 and
# at 4 alike), so the cheap run is as good a control as the expensive one.
check_stack_parity() {
  if ! (ulimit -s 1024) 2>/dev/null; then
    echo "check-native-integration: cannot lower the stack limit here; \
issue #617's control needs a shell that can (Linux/macOS). Not skipping \
silently: run this phase on a platform that can, or the control is absent." >&2
    return 1
  fi
  cargo build --manifest-path rust/Cargo.toml -p fslc-rust --bin fslc --locked
  local out
  out=$(
    ulimit -s 1024
    ./rust/target/debug/fslc refine \
      examples/agentic_rag/agentic_rag_design.fsl \
      examples/agentic_rag/agentic_rag_requirements.fsl \
      examples/agentic_rag/agentic_rag_design_refines_requirements.fsl \
      --depth 2 2>&1
  )
  local status=$?
  if [ "$status" -ne 0 ] || printf '%s' "$out" | grep -q "overflowed its stack"; then
    printf '%s\n' "$out" >&2
    echo "check-native-integration: \`fslc refine\` did not survive a 1 MiB \
stack (exit $status). That is the Windows default, so this is issue #617 \
returning: the CLI is back on whatever stack the OS picked." >&2
    return 1
  fi
  echo "stack parity: refine survives a 1 MiB (Windows-sized) stack"
}

check_wasm() {
  npm --prefix rust/spikes/z3js-worker ci
  npm --prefix rust/spikes/z3js-worker run probe
  npm --prefix rust/spikes/z3js-worker run probe:browser
  cargo build --manifest-path rust/Cargo.toml -p fslc-rust --bin fslc --locked
  npm --prefix rust/fsl-wasm ci
  npm --prefix rust/fsl-wasm run test:browser
}

case "${1:-all}" in
  rust)
    check_rust
    ;;
  wasm)
    check_wasm
    ;;
  boundaries)
    check_boundaries
    ;;
  fault-operators)
    check_fault_operators
    ;;
  semantic-mutation)
    check_semantic_mutation "${2:-complete}"
    ;;
  fsl-logic)
    check_fsl_logic "${2:-pr}"
    ;;
  stack-parity)
    check_stack_parity
    ;;
  all)
    check_rust
    check_wasm
    check_semantic_mutation complete
    check_fsl_logic scheduled
    ;;
  *)
    echo "usage: $0 [all|rust|wasm|boundaries|fault-operators|semantic-mutation [changed|complete]|fsl-logic [pr|scheduled]|stack-parity]" >&2
    exit 2
    ;;
esac
