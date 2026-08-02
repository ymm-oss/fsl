#!/usr/bin/env bash
# SPDX-License-Identifier: Apache-2.0

set -euo pipefail

root="$(cd "$(dirname "$0")/.." && pwd)"
cd "$root"

check_compile() {
  # Deliberately no `--all-targets`. It was tried and measured at 12m42s in CI,
  # which destroys this lane's reason to exist — it is the sub-minute fail-fast
  # signal, not the gate. Test targets are compiled and run by `rust workspace`,
  # which now runs on every pull request, so `--all-targets` here buys nothing
  # and costs the fast feedback.
  cargo check \
    --manifest-path rust/Cargo.toml \
    --workspace \
    --exclude fsl-solver-z3 \
    --exclude fslc-rust \
    --no-default-features \
    --locked
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
  # The agent environment is repository-hook infrastructure, not frozen Python
  # product behavior. These zero-argument contract tests use only the standard
  # library, so run them directly without adding pytest to the fail-fast lane.
  python3 -c 'from tests.test_codex_environment import test_semantic_findings_cannot_disappear_between_agents_and_checkpoint as codex_contract; from tests.test_claude_environment import test_semantic_findings_cannot_disappear_between_agents_and_checkpoint as claude_contract; codex_contract(); claude_contract()'
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
