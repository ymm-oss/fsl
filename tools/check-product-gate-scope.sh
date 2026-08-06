#!/usr/bin/env bash
# SPDX-License-Identifier: Apache-2.0
#
# Decides whether a product-gate job needs to execute real evidence for the
# current change. This replaces the workflow-level `paths-ignore`
# agent-configuration exemption that `ci.yml`'s `pull_request` trigger used
# to carry (see docs/DESIGN-ci.md, "Agent-configuration exemption"). A
# workflow-level path skip never emits its job's context at all; if that
# context is ever made a required status check, or ever needs to satisfy
# merge-queue entry, an exempted pull request is stuck `Expected` forever --
# unfixable even by an admin merge. This script instead lets the job start,
# check out, diff, and exit fast, so its context always reports something.
#
# It also owns the (currently inert) `merge_group`/`FSL_MERGE_QUEUE_CI`
# decision described in docs/DESIGN-ci.md, "The merge queue was tried,
# measured against this repository's workflow, and rejected". A merge queue
# was configured on the `main` ruleset on 2026-08-05 and removed the same
# day: an admin merge bypasses the queue entirely, and the ordinary
# `enqueuePullRequest` path is unsatisfiable under the single-approver review
# policy. Neither the repository variable nor a `merge_queue` ruleset rule
# exists, so the `queue-entry-stub` branch below never runs in production.
# It is kept because it is harmless and ready if that *human-review-policy*
# question is ever answered differently -- not because a rollout is pending.
#
# Usage:
#   ./tools/check-product-gate-scope.sh            # decide scope; prints
#                                                   #   run=<bool>
#                                                   #   reason=<token>
#                                                   # to stdout, suitable for
#                                                   # redirection into
#                                                   # $GITHUB_OUTPUT
#   ./tools/check-product-gate-scope.sh selftest    # exercise the classifier
#                                                   # (accepting/rejecting
#                                                   # controls)

set -euo pipefail

# Exempt paths: the five entries from the retired `paths-ignore` list.
# `.claude/**` and `.agents/**` are directory prefixes; `CLAUDE.md`,
# `AGENTS.md`, and `CHANGELOG.md` are exact repository-root filenames, not
# prefixes -- "CLAUDE.md.d/x" must NOT match.
is_exempt_path() {
  case "$1" in
    .claude/*|.agents/*) return 0 ;;
    CLAUDE.md|AGENTS.md|CHANGELOG.md) return 0 ;;
    *) return 1 ;;
  esac
}

# Reads a newline-separated path list on stdin and prints "exempt" when the
# list is non-empty and every path matches is_exempt_path, otherwise prints
# "product". An empty list is fail-closed to "product", never "exempt".
classify_diff() {
  local line total=0 exempt=0
  while IFS= read -r line; do
    [ -z "$line" ] && continue
    total=$((total + 1))
    if is_exempt_path "$line"; then
      exempt=$((exempt + 1))
    fi
  done
  if [ "$total" -gt 0 ] && [ "$total" -eq "$exempt" ]; then
    echo exempt
  else
    echo product
  fi
}

# Prints the two-line run=/reason= result and, on a stub (run=false), leaves
# a one-line breadcrumb in the job summary so a skipped context is never
# misread as product evidence.
emit() {
  local run="$1" reason="$2"
  echo "run=$run"
  echo "reason=$reason"
  if [ "$run" = "false" ] && [ -n "${GITHUB_STEP_SUMMARY:-}" ]; then
    echo "early exit (\`$reason\`): evidence for this context is not required for this change; see docs/DESIGN-ci.md" >>"$GITHUB_STEP_SUMMARY"
  fi
}

# Diff-based exemption shared by `pull_request` (once FSL_MERGE_QUEUE_CI is
# not "enabled") and `merge_group` events -- the latter cannot be path-
# filtered at the trigger level, so this in-job check is the only place the
# exemption can apply once a merge queue exists. Fails closed to a full run
# when either SHA is missing or the diff itself cannot be computed.
diff_scope() {
  if [ -z "${BASE_SHA:-}" ] || [ -z "${HEAD_SHA:-}" ]; then
    emit true diff-unavailable-fail-closed
    return
  fi
  local diff
  if ! diff="$(git diff --name-only "$BASE_SHA...$HEAD_SHA" 2>/dev/null)"; then
    emit true diff-unavailable-fail-closed
    return
  fi
  case "$(printf '%s\n' "$diff" | classify_diff)" in
    exempt) emit false agent-configuration-exemption ;;
    *) emit true product-paths-changed ;;
  esac
}

decide() {
  case "${GITHUB_EVENT_NAME:-}" in
    pull_request)
      if [ "${GITHUB_BASE_REF:-}" = "production" ]; then
        # Never stub or skip promotion evidence.
        emit true production-promotion-evidence
      elif [ "${FSL_MERGE_QUEUE_CI:-}" = "enabled" ]; then
        # Unreachable today: FSL_MERGE_QUEUE_CI does not exist yet. Correct
        # ahead of time so enabling the variable is the only remaining step.
        emit false queue-entry-stub
      else
        diff_scope
      fi
      ;;
    merge_group)
      diff_scope
      ;;
    push|schedule|workflow_dispatch)
      emit true complete-evidence-event
      ;;
    *)
      emit true unknown-event-fail-closed
      ;;
  esac
}

check() {
  local desc="$1" expected="$2" actual="$3"
  if [ "$actual" != "$expected" ]; then
    echo "check-product-gate-scope.sh selftest: FAIL ($desc): expected '$expected', got '$actual'" >&2
    return 1
  fi
  return 0
}

selftest() {
  local failures=0

  check "all-product-paths" product \
    "$(printf 'src/main.rs\nrust/fsl-core/src/lib.rs\n' | classify_diff)" || failures=$((failures + 1))
  check "mixed exempt + CHANGELOG.md" exempt \
    "$(printf '.claude/skills/x/SKILL.md\nCHANGELOG.md\n' | classify_diff)" || failures=$((failures + 1))
  check "mixed exempt + product" product \
    "$(printf '.claude/skills/x/SKILL.md\nrust/fsl-core/src/lib.rs\n' | classify_diff)" || failures=$((failures + 1))
  check "filename-prefix near-miss" product \
    "$(printf 'CLAUDE.md.d/x\n' | classify_diff)" || failures=$((failures + 1))
  check "empty input fail-closed" product \
    "$(printf '' | classify_diff)" || failures=$((failures + 1))

  if [ "$failures" -ne 0 ]; then
    echo "check-product-gate-scope.sh selftest: $failures assertion(s) failed" >&2
    exit 1
  fi
  echo "check-product-gate-scope.sh selftest: all assertions passed"
}

case "${1:-}" in
  selftest)
    selftest
    ;;
  "")
    decide
    ;;
  *)
    echo "usage: $0 [selftest]" >&2
    exit 2
    ;;
esac
