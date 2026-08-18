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
  # Accepting/rejecting controls for the agent-configuration-exemption
  # classifier that ci.yml's heavy jobs now run in-job (docs/DESIGN-ci.md).
  ./tools/check-product-gate-scope.sh selftest
  # Accepting/rejecting controls for the shard-completeness guard the sharded
  # `rust workspace` and `semantic mutation` aggregators depend on
  # (docs/DESIGN-ci.md, "Sharded pre-merge Linux evidence").
  ./tools/check-shard-union.sh selftest
  # Accepting/rejecting controls for the ruleset drift audit's compareRuleset/
  # validateContract classifier (docs/DESIGN-ci.md, "Ruleset drift audit").
  node --test .github/scripts/audit-ruleset-drift.test.mjs
  # Accepting/rejecting controls for the Actions cache budget audit, including
  # the rejecting fixture for `ci.yml`'s `save-if` guard: a pull-request-scoped
  # cache for one of its shared keys must fail the audit, so removing that guard
  # cannot pass silently (docs/DESIGN-ci.md, "Actions cache budget").
  node --test .github/scripts/audit-cache-budget.test.mjs
  # Accepting/rejecting controls for all six changelog-fragment fail-closed
  # controls (docs/DESIGN-changelog-fragments.md): nonconforming fragment
  # name, duplicate (id, category), nondeterministic/nonconforming order,
  # unaggregated-at-release plus direct-edit-forbidden, and aggregation
  # conservation. Pure, no BASE_SHA/HEAD_SHA needed here -- the real
  # pull-request-diff checks run directly in merge-readiness.yml, the same
  # split check-product-gate-scope.sh's own `selftest` versus its real
  # `diff_scope` invocation uses.
  ./tools/aggregate_changelog.sh selftest
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
