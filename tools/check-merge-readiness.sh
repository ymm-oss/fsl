#!/usr/bin/env bash
(( BASH_VERSINFO[0] >= 4 )) || { echo "check-merge-readiness.sh requires Bash 4 or newer" >&2; exit 1; }
# SPDX-License-Identifier: Apache-2.0
#
# Every step of a lane runs, and the lane fails afterwards if any step failed
# (issue #1135). In CI a lane is a gate and stopping at the first failure is
# cheap; locally it is a checklist, and a checklist that stops at item 1 hides
# the other thirty. `--fail-fast` restores the stop-at-the-first-failure
# behaviour for anyone who wants it.
#
# `set -e` stays on deliberately. The only command exempted from it is the
# measured one inside `step` (`"$@" || status=$?`), whose exit status is
# recorded and re-raised by the summary; every other command in this script
# still aborts the lane, so this change adds exactly one place where a non-zero
# status does not abort, and that place reports it.
#
# A step whose tool is absent is neither a pass nor a failure: it is reported
# as SKIP, listed separately, and the lane still exits non-zero -- a skip is
# not a pass. In CI skipping is not offered at all: the missing tool is a hard
# failure, because a CI image that quietly loses a tool and still goes green is
# exactly the false-green class docs/DESIGN-ci.md exists to prevent.

set -euo pipefail

# A step's non-zero status no longer aborts the lane, so an interrupt must abort it
# explicitly: without this trap, Ctrl-C would merely record the running step as failed
# and continue with the next one.
trap 'echo "check-merge-readiness: interrupted" >&2; exit 130' INT TERM

root="$(cd "$(dirname "$0")/.." && pwd)"
cd "$root"

fail_fast=0
steps_ran=0
failed_steps=()
skipped_steps=()

# CI is detected, not opted into: GitHub Actions sets both `CI` and
# `GITHUB_ACTIONS`, and `FSL_READINESS_CI` lets any other runner (or a local
# reproduction of the CI behaviour) say so explicitly. There is deliberately no
# variable that re-enables skipping in CI.
in_ci() {
  case "${FSL_READINESS_CI:-}" in
    1 | true | enabled) return 0 ;;
  esac
  case "${GITHUB_ACTIONS:-}" in
    true) return 0 ;;
  esac
  case "${CI:-}" in
    "" | 0 | false) return 1 ;;
    *) return 0 ;;
  esac
}

summarize() {
  local item
  if (( ${#skipped_steps[@]} > 0 )); then
    printf 'check-merge-readiness: %d step(s) could not run:\n' "${#skipped_steps[@]}" >&2
    for item in "${skipped_steps[@]}"; do
      printf '  SKIP %s\n' "$item" >&2
    done
  fi
  if (( ${#failed_steps[@]} > 0 )); then
    printf 'check-merge-readiness: %d step(s) failed:\n' "${#failed_steps[@]}" >&2
    for item in "${failed_steps[@]}"; do
      printf '  FAIL %s\n' "$item" >&2
    done
  fi
  if (( ${#failed_steps[@]} > 0 || ${#skipped_steps[@]} > 0 )); then
    printf 'check-merge-readiness: NOT READY -- %d ran, %d failed, %d skipped\n' \
      "$steps_ran" "${#failed_steps[@]}" "${#skipped_steps[@]}" >&2
  else
    printf 'check-merge-readiness: PASS -- %d step(s)\n' "$steps_ran"
  fi
  return 0
}

# step [--needs <command>]... [--needs-module <python module>]... <command> [args...]
#
# Runs one lane step. A declared requirement that is absent makes the step a
# skip (a failure in CI) instead of letting the step fail for a reason that
# looks like a real finding.
step() {
  local missing=""
  while (( $# > 0 )); do
    case "$1" in
      --needs)
        if [ -z "$missing" ] && ! command -v "$2" >/dev/null 2>&1; then
          missing="command $2"
        fi
        shift 2
        ;;
      --needs-module)
        if [ -z "$missing" ] && ! python3 -c \
          'import importlib.util, sys; sys.exit(0 if importlib.util.find_spec(sys.argv[1]) else 1)' \
          "$2" >/dev/null 2>&1; then
          missing="python module $2"
        fi
        shift 2
        ;;
      *)
        break
        ;;
    esac
  done
  if (( $# == 0 )); then
    echo "check-merge-readiness: internal error: step without a command" >&2
    exit 2
  fi
  local label="$*"
  if [ -n "$missing" ]; then
    if in_ci; then
      printf 'check-merge-readiness: FAIL %s [%s not found; CI does not allow skipping]\n' \
        "$label" "$missing" >&2
      failed_steps+=("$label [$missing not found; CI does not allow skipping]")
    else
      printf 'check-merge-readiness: SKIP %s [%s not found]\n' "$label" "$missing" >&2
      skipped_steps+=("$label [$missing not found]")
    fi
    if (( fail_fast == 1 )); then
      summarize
      exit 1
    fi
    return 0
  fi
  steps_ran=$((steps_ran + 1))
  local status=0
  "$@" || status=$?
  if (( status != 0 )); then
    printf 'check-merge-readiness: FAIL %s [exit %d]\n' "$label" "$status" >&2
    failed_steps+=("$label [exit $status]")
    if (( fail_fast == 1 )); then
      summarize
      exit "$status"
    fi
  fi
  return 0
}

check_compile() {
  # Deliberately no `--all-targets`. It was tried and measured at 12m42s in CI,
  # which destroys this lane's reason to exist — it is the sub-minute fail-fast
  # signal, not the gate. Test targets are compiled and run by `rust workspace`,
  # which now runs on every pull request, so `--all-targets` here buys nothing
  # and costs the fast feedback.
  step --needs cargo cargo check \
    --manifest-path rust/Cargo.toml \
    --workspace \
    --exclude fsl-solver-z3 \
    --exclude fslc-rust \
    --no-default-features \
    --locked
}

check_core_contracts() {
  step --needs cargo cargo fmt --manifest-path rust/Cargo.toml --all -- --check
  step --needs cargo cargo test \
    --manifest-path rust/Cargo.toml \
    --locked \
    -p fsl-syntax \
    -p fsl-core \
    -p fsl-runtime \
    -p fsl-solver
  step --needs cargo ./tools/check-native-integration.sh boundaries
}

check_automation() {
  # ShellCheck's extra masked-return analysis covers the pipeline/process-
  # substitution regressions from #898. The companion stdlib lint requires
  # Bash-4+ scripts to fail closed before executing any other command.
  step python3 tools/check-shell-scripts.py selftest
  step --needs shellcheck python3 tools/check-shell-scripts.py
  # SPDX headers are a source-wide rule.  One common detector serves this
  # required lane and the Codex/Claude early-feedback adapters.
  step python3 tools/check_spdx_headers.py selftest
  step python3 tools/check_spdx_headers.py check
  step python3 tools/check-bash-version-guards.py selftest
  step python3 tools/check-bash-version-guards.py check
  step --needs node node --test .github/scripts/report-post-merge-ci.test.mjs
  # The privileged post-merge reporter has issues: write. Its separate
  # parser-backed workflow-shape controls reject comments, decoys, and shell
  # indirection rather than trying to infer behavior from line substrings.
  step --needs-module pytest python3 -m pytest tests/test_post_merge_reporter_workflow.py -v
  step --needs-module yaml python3 .github/scripts/validate_post_merge_reporter_workflow.py
  # The Rust toolchain pin is an Actions YAML contract.  Its fourteen
  # line-scanner failures showed that a regex verdict is not trustworthy, so
  # this required lane installs PyYAML and runs both the calibrated controls and
  # the live parser-backed audit.
  step --needs-module pytest python3 -m pytest tests/test_toolchain_pin.py -v
  step --needs-module yaml python3 .github/scripts/validate_toolchain_pin.py
  # Keep both VS Code install instructions tied to the release packaging name.
  step --needs-module pytest python3 -m pytest tests/test_vscode_vsix_name.py -v
  step python3 .github/scripts/validate_vscode_vsix_name.py
  # The cache-budget reporter subscribes by workflow display name. Its
  # parser-backed uniqueness control must be able to block a merge, not merely
  # report red in a non-required workflow, so run its calibrated controls and
  # live audit in this required lane.
  step --needs-module pytest python3 -m pytest tests/test_cache_budget_audit_workflow.py -v
  step --needs-module yaml python3 .github/scripts/validate-cache-budget-audit-workflow.py
  # The cache-budget reporter owns a separate issue lifecycle from the
  # read-only audit. Keep its reconciliation controls in this pre-merge lane,
  # alongside the established post-merge reporter controls.
  step --needs node node --test .github/scripts/report-cache-budget-audit.test.mjs
  # The agent environment is repository-hook infrastructure, not frozen Python
  # product behavior. These zero-argument contract tests use only the standard
  # library, so run them directly without adding pytest to the fail-fast lane.
  step python3 -c 'from tests.test_codex_environment import test_semantic_findings_cannot_disappear_between_agents_and_checkpoint as codex_contract; from tests.test_claude_environment import test_semantic_findings_cannot_disappear_between_agents_and_checkpoint as claude_contract; codex_contract(); claude_contract()'
  # Accepting/rejecting controls for the agent-configuration-exemption
  # classifier that ci.yml's heavy jobs now run in-job (docs/DESIGN-ci.md).
  step ./tools/check-product-gate-scope.sh selftest
  # Accepting/rejecting controls for the shard-completeness guard the sharded
  # `rust workspace` and `semantic mutation` aggregators depend on
  # (docs/DESIGN-ci.md, "Sharded pre-merge Linux evidence").
  step ./tools/check-shard-union.sh selftest
  # Stable logical shard artifacts deliberately admit a compatible mixed-
  # attempt cohort after a partial rerun. Calibrate the provenance/checksum/
  # identity policy, then audit the live workflow through its parsed YAML
  # structure so one producer or aggregator cannot drift independently.
  step ./tools/check-shard-artifact-cohort.sh selftest
  step --needs-module pytest python3 -m pytest tests/test_shard_artifact_workflow.py -v
  step --needs-module yaml python3 .github/scripts/validate-shard-artifact-workflow.py
  # The frozen Python compatibility registries and DESIGN-document index must
  # move together. Keep this detector in required pre-merge repository
  # evidence; it is not part of the Rust-native product gate.
  step --needs-module pytest python3 -m pytest tests/test_coupled_change_meta.py -v
  # Accepting/rejecting controls for the ruleset drift audit's compareRuleset/
  # validateContract classifier (docs/DESIGN-ci.md, "Ruleset drift audit").
  step --needs node node --test .github/scripts/audit-ruleset-drift.test.mjs
  # Accepting/rejecting controls for the Actions cache budget audit, including
  # the rejecting fixture for `ci.yml`'s `save-if` guard: a pull-request-scoped
  # cache for one of its shared keys must fail the audit, so removing that guard
  # cannot pass silently (docs/DESIGN-ci.md, "Actions cache budget").
  step --needs node node --test .github/scripts/audit-cache-budget.test.mjs
  # Accepting/rejecting controls for all six changelog-fragment fail-closed
  # controls (docs/DESIGN-changelog-fragments.md): nonconforming fragment
  # name, duplicate (id, category), nondeterministic/nonconforming order,
  # unaggregated-at-release plus direct-edit-forbidden, and aggregation
  # conservation. Pure, no BASE_SHA/HEAD_SHA needed here -- the real
  # pull-request-diff checks run directly in merge-readiness.yml, the same
  # split check-product-gate-scope.sh's own `selftest` versus its real
  # `diff_scope` invocation uses.
  step ./tools/aggregate_changelog.sh selftest
  step python3 tools/check-design-citation-headings.py selftest
  step python3 tools/check-design-citation-headings.py check
  # Parser-backed inventory of pytest validator modules versus required-gate
  # wiring. New unwired modules fail closed; wiring a module is the shortest
  # path to required classification, not inventory-only exempt rows.
  step python3 tools/check_ci_validator_inventory.py selftest
  step python3 tools/check_ci_validator_inventory.py check
  step --needs-module pytest python3 -m pytest tests/test_ci_validator_inventory.py -v
}

usage() {
  echo "usage: $0 [--fail-fast] [all|compile|core|automation]" >&2
  exit 2
}

lane=""
for argument in "$@"; do
  case "$argument" in
    --fail-fast)
      fail_fast=1
      ;;
    compile | core | automation | all)
      if [ -n "$lane" ]; then
        usage
      fi
      lane="$argument"
      ;;
    *)
      usage
      ;;
  esac
done
[ -n "$lane" ] || lane=all

case "$lane" in
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
esac

summarize
if (( ${#failed_steps[@]} > 0 || ${#skipped_steps[@]} > 0 )); then
  exit 1
fi
