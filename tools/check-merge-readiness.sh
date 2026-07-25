#!/usr/bin/env bash
# SPDX-License-Identifier: Apache-2.0

set -euo pipefail

root="$(cd "$(dirname "$0")/.." && pwd)"
cd "$root"

check_compile() {
  cargo check --manifest-path rust/Cargo.toml --workspace --all-targets --locked
}

check_core_contracts() {
  cargo fmt --manifest-path rust/Cargo.toml --all -- --check
  cargo test \
    --manifest-path rust/Cargo.toml \
    --locked \
    -p fsl-syntax \
    -p fsl-core \
    -p fsl-runtime \
    -p fsl-solver
  ./tools/check-native-integration.sh boundaries
}

check_automation() {
  node --test .github/scripts/report-post-merge-ci.test.mjs
}

case "${1:-all}" in
  compile)
    check_compile
    ;;
  core)
    check_core_contracts
    ;;
  automation)
    check_automation
    ;;
  all)
    check_core_contracts
    check_compile
    check_automation
    ;;
  *)
    echo "usage: $0 [all|compile|core|automation]" >&2
    exit 2
    ;;
esac
