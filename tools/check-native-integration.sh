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
  cargo fmt --manifest-path rust/Cargo.toml --all -- --check
  cargo clippy --manifest-path rust/Cargo.toml --workspace --all-targets --locked -- -D warnings
  cargo test --manifest-path rust/Cargo.toml -p fsl-lsp --lib --locked
  cargo test --manifest-path rust/Cargo.toml -p fsl-lsp --test stdio --locked
  cargo test --manifest-path rust/Cargo.toml -p fsl-lsp --test corpus --locked
  cargo test --manifest-path rust/Cargo.toml --workspace --exclude fsl-lsp --locked
  cargo build --manifest-path rust/Cargo.toml --workspace --locked

  assert_dependency_absent fsl-runtime 'fsl-solver|z3' 'fsl-runtime must remain solver-independent'
  assert_dependency_absent fsl-wasm 'fsl-solver-z3 v' 'fsl-wasm must not depend on the native Z3 backend'
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
  all)
    check_rust
    check_wasm
    ;;
  *)
    echo "usage: $0 [all|rust|wasm]" >&2
    exit 2
    ;;
esac
