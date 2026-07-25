<!-- SPDX-License-Identifier: Apache-2.0 -->

# Optimistic merge and post-merge product validation

Status: Accepted

## Decision

Pull requests into `main` have one stable required context, `merge readiness`. It is a bounded,
short-latency confidence gate, not a claim that the product is fully verified. The complete
Rust/WASM/native-platform product gate runs after every push to `main`, on schedule, on manual
dispatch, and before promotion to `production`.

This deliberately changes `main` from "every supported platform was green before merge" to
"reviewed changes passed a bounded readiness gate and every merged state is then fully validated."
A post-merge failure can therefore expose a temporarily broken `main`. The failed check and its
deduplicated issue are blocking evidence for production/release promotion and must be repaired or
reverted; they are not informational warnings.

## Merge readiness contract

`.github/workflows/merge-readiness.yml` runs for `pull_request` and `merge_group` events targeting
`main`. The ruleset requires only its always-present `merge readiness` aggregator. The aggregator
fails unless all of these independent lanes succeed:

1. `cargo check --workspace --all-targets --locked` catches compile and feature-integration drift
   across the complete authoritative Rust workspace.
2. Formatting, the `fsl-syntax`, `fsl-core`, `fsl-runtime`, and backend-neutral `fsl-solver` tests,
   plus the runtime/WASM dependency negative controls, protect the solver-independent semantic
   foundation.
3. The post-merge issue reporter contract tests protect failure creation, duplicate suppression,
   recurrence updates, recovery closure, and workflow-level failure handling.

The lanes run in parallel through `tools/check-merge-readiness.sh`. Clippy, native Z3 verification,
the complete LSP/corpus suites, the full workspace test/build, JavaScript solver probes, and browser
Worker validation remain product-gate evidence. A green readiness check must never be rendered or
documented as a fully verified product.

Superseded runs for the same pull request are cancelled. Merge-group runs are not cancelled and the
workflow handles GitHub's `merge_group` event directly, so a merge queue can validate the combined
candidate against current `main`.

## Product gate contract

`.github/workflows/ci.yml` is named `product gate`. A trusted `main` push runs all of these jobs in
parallel:

- the complete Rust-native integration phase from `tools/check-native-integration.sh rust`;
- the production WASM/browser phase from `tools/check-native-integration.sh wasm`;
- focused native-Z3 tests on macOS and Windows.

Scheduled and manual runs use the same evidence. Pull requests into `production` also run the
complete product gate and emit the Linux native-Z3 compatibility context expected by the production
ruleset. Release jobs retain their independent four-target build, smoke, ABI, LSP, and packaging
checks. An always-running `product gate` aggregator fails unless every required lane emitted
successful evidence; an accidentally skipped lane cannot make the workflow confidently green.

Product-gate runs for merged commits are not cancelled. Each merged state therefore retains its own
portable evidence and failure attribution even when agents merge changes quickly.

## Failure reporting contract

`.github/workflows/post-merge-ci-reporter.yml` listens for completed `product gate` runs. It acts
only when the triggering run was a push to the repository's default branch. The workflow receives
read access to Actions, contents, and pull requests, and write access only to issues.

For each failed, timed-out, cancelled, stale, startup-failed, or action-required job, the reporter:

- creates the `ci/post-merge` label when necessary;
- creates one canonical issue keyed by the product-gate workflow ID and stable job name, reopening
  the same issue after a later recurrence;
- records the commit, associated merged pull request, run, job, conclusion, and failed step names;
- copies no build log text or secret-bearing output into the issue;
- adds one occurrence comment for a later failing run instead of opening a duplicate;
- closes the matching issue only after that job, or the complete product gate, succeeds on a later
  `main` run.

An unsuccessful workflow with no failed-job metadata gets a workflow-level issue, so startup and
orchestration failures do not disappear. Re-running the reporter for the same product-gate run is
idempotent. Before mutating issues, the reporter queries the latest completed trusted push run for
the same workflow. When an older `run_number`/`run_attempt` event arrives, it reconciles that latest
run instead of the stale trigger; out-of-order platform completion cannot overwrite newer
default-branch health or cancel the only reporter for a persistent newer failure.

Reporter jobs are also serialized to avoid duplicate-creation races. When product gates finish
faster than reporting, GitHub may coalesce pending reporter runs toward the newest default-branch
health state; the underlying product-gate result remains attached to every merged commit, and any
failure that persists at the current `main` head remains an open issue.
When a concrete lane and the aggregate `product gate` job fail together, only the concrete lane is
reported; the aggregate is reported on its own only when it is the sole available failure.

## Safe rollout and rollback

The workflow change must land before the ruleset changes; otherwise existing pull requests cannot
produce the new required context. Roll out in this order:

1. Merge the workflow change under the existing four required product checks. Until activation,
   pull requests into `main` continue running the full product gate.
2. In the `main` repository ruleset, add `merge readiness`, then remove `rust workspace`, `WASM`,
   `native Z3 4.16 (macos-15)`, and `native Z3 4.16 (windows-latest)` from the required contexts.
3. Set the repository Actions variable `FSL_OPTIMISTIC_CI=enabled`. This disables the redundant full
   product gate on pull requests into `main`; main pushes, schedules, manual runs, and production
   promotions ignore the variable.
4. Optionally enable auto-merge and a merge queue after the `merge_group` check is observed.

Rollback is fail-safe: delete or change `FSL_OPTIMISTIC_CI`, restore the four product contexts in the
main ruleset, and keep `merge readiness` as additional evidence. No source or product artifact
migration is involved.

## Non-goals

- Treating issue creation as a substitute for repair, revert, or release blocking.
- Hiding platform failures with `continue-on-error`.
- Trusting agent-authored verification claims in place of independent GitHub-hosted execution.
- Changing FSL language, Kernel, CLI/JSON, LSP, Worker, or frozen Python compatibility behavior.
