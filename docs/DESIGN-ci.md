<!-- SPDX-License-Identifier: Apache-2.0 -->

# Optimistic merge and post-merge product validation

Status: Accepted

## Decision

Pull requests into `main` run the **complete Linux evidence** — `merge readiness` as a
short-latency fail-fast lane, plus `rust workspace` and `WASM`. Only the **cross-platform matrix**,
`native Z3 4.16` on macOS and Windows, is deferred to after the merge, together with the aggregate
`product gate` context; that matrix also runs on schedule, on manual dispatch, and before promotion
to `production`.

This changes `main` from "every supported platform was green before merge" to "every change was
fully tested on Linux before merge, and the remaining platforms are validated immediately after."
A post-merge failure can therefore still expose a temporarily broken `main`, but only for a
platform-specific defect. The failed check and its deduplicated issue are blocking evidence for
production/release promotion and must be repaired or reverted; they are not informational warnings.

### Why the split falls here

Measured on the batch that motivated this revision:

- **The tests live in the crates the bounded lane excludes.** 103 of 124 integration test files sit
  in `fslc` (77), `fsl-tools` (16), `fsl-verifier` (8) and `fsl-lsp` (2). `merge readiness` runs 21.
- **Its compile lane does not build test targets at all** — `cargo check` without `--all-targets` —
  so a type error or a failing assertion in any of those 103 files reached `main` and surfaced only
  post-merge. Only `cargo fmt --check` covered them, and only for syntactic breakage. Adding
  `--all-targets` there was tried and **reverted**: measured at 12m42s in CI, it destroys the lane's
  reason to exist, and it is redundant once `rust workspace` compiles and runs those targets
  pre-merge.
- **Platform failures have been Windows-specific**: a `python` executable absent from `PATH`, and
  `core.autocrlf` turning `depth = 2` into `depth = 2\r`. No macOS-specific failure has occurred.
- **Windows is the slowest job in the gate** (≈17 min, against ≈12 min for the Linux workspace on a
  warm cache and ≈18 min cold), so deferring the matrix is what buys the merge throughput this
  design is for.

The cost is stated plainly: a pull request into `main` now waits for `rust workspace`, measured at
18m17s on a cold cache and around twelve minutes warm, rather than forty seconds. Because a branch
that falls behind `main` must re-run its checks, a serial chain of rebases pays that repeatedly. Two
mitigations already exist and are worth using before the cost is treated as inherent: independent
changes can share one pull request with one commit per topic, and `merge-readiness.yml` already
handles `merge_group`, so a merge queue can validate several candidates as one batch once `ci.yml`
gains the same trigger.

## Merge readiness contract

`.github/workflows/merge-readiness.yml` runs for `pull_request` and `merge_group` events targeting
`main`. It is now the **fail-fast lane**: its slowest measured contract lane is 33s and the aggregator
4s, so obvious breakage is reported in well under a minute rather than after the Linux workspace job.
Keeping it that fast is the reason `--all-targets` was reverted below. It is no longer the only
pre-merge evidence, and it was never sufficient on its own. The aggregator fails unless all of these
independent lanes succeed:

1. `cargo check --workspace --exclude fsl-solver-z3 --exclude fslc-rust --no-default-features
   --locked` catches compile and dependency-integration drift across the authoritative
   native-Z3-free Rust surface without paying the vendored native-Z3 build cost before merge.
   Excluding the native CLI package as a workspace root avoids its Z3-only helper binaries; its
   library still compiles transitively through the LSP and WASM crates. **`--all-targets` is
   deliberately absent**: it was added, measured at 12m42s, and reverted, because this lane's whole
   value is sub-minute feedback and `rust workspace` already compiles and runs those targets on the
   same pull request.
2. Formatting, the `fsl-syntax`, `fsl-core`, `fsl-runtime`, and backend-neutral `fsl-solver` tests,
   plus the runtime/WASM dependency negative controls, protect the solver-independent semantic
   foundation.
3. The automation contract lane protects post-merge failure creation, duplicate suppression,
   recurrence updates, recovery closure, and workflow-level failure handling. It also runs the
   checked-in Codex/Claude task-harness contracts with the system Python: a discovered soundness
   finding must remain in the worktree ledger until fixed, linked to an issue, or explicitly marked
   as awaiting issue-creation authorization.

The lanes run in parallel through `tools/check-merge-readiness.sh`. Native-CLI/default-feature
compilation, all-target compilation, Clippy, native Z3 verification, the complete LSP/corpus suites,
the full workspace test/build, JavaScript solver probes, and browser Worker validation remain
product-gate evidence. A green readiness check must never be rendered or documented as a fully
verified product.

Superseded runs for the same pull request are cancelled. Merge-group runs are not cancelled and the
workflow handles GitHub's `merge_group` event directly, so a merge queue can validate the combined
candidate against current `main`.

## Product gate contract

`.github/workflows/ci.yml` is named `product gate`. A trusted `main` push runs all of these jobs in
parallel:

- the Rust-native integration phase, sharded across three jobs: `rust-checks` runs
  `tools/check-native-integration.sh rust-checks` once (formatting, Clippy, the workspace doctests,
  the full workspace build, and the boundary/stack-parity controls); `rust-tests` shards the
  workspace's non-doctest tests three ways with `cargo-nextest` via
  `tools/check-native-integration.sh rust-tests K/3`; the `rust-workspace` aggregator requires both
  and enforces shard-union completeness (`rust workspace`; see "Sharded pre-merge Linux evidence"
  below);
- the production WASM/browser phase from `tools/check-native-integration.sh wasm` (`WASM`);
- focused native-Z3 tests on macOS and Windows (`native Z3 4.16`);
- the M13 semantic mutation gate, sharded across three jobs: `semantic-mutation-operators` runs the
  curated fault-operator controls three ways via `tools/run-semantic-mutation-gate.sh <mode> --lane
  operators --shard K/3`; `semantic-mutation-mutants` runs the generic cargo-mutants half, complete
  and unsharded, via `tools/run-semantic-mutation-gate.sh <mode> --lane mutants`; the
  `semantic-mutation` aggregator requires both and enforces operator-union completeness (`semantic
  mutation`; see "Sharded pre-merge Linux evidence" below);
- the deterministic finite-model agreement gate from `tools/check-native-integration.sh fsl-logic`
  (`FSL Logic Test`).

**The first two carry no event condition beyond scope evidence and therefore also run on every
pull request today**, which is what makes the Linux evidence pre-merge. Only `native Z3 4.16` and
the aggregate `product gate` context honour `FSL_OPTIMISTIC_CI` and skip on pull requests into
`main`.

All four heavy jobs (`rust workspace`, `WASM`, `semantic mutation`, `FSL Logic Test`) are required
contexts on the `main` ruleset, and all four also carry a `merge_group` trigger that is **inert**:
no merge queue is configured on `main`, and the accepted decision below records that one was tried
and rejected, so `merge_group` never fires. The same is true of
`tools/check-product-gate-scope.sh`'s `queue-entry-stub` branch, which is gated on a
`FSL_MERGE_QUEUE_CI` variable that does not exist. Every pull request therefore runs the complete
Linux evidence in full, and that is the live contract — see "Required pre-merge contexts, and why
the merge queue was rejected" below before assuming otherwise.

### Agent-configuration exemption

Each of the four heavy jobs runs `tools/check-product-gate-scope.sh` as its first step (named
"Decide required evidence scope"), reads its `run`/`reason` output, and gates every later step on
`steps.scope.outputs.run == 'true'`. This replaced a workflow-level `paths-ignore` list on
`ci.yml`'s `pull_request` trigger that named the same five surfaces: `.claude/**`, `.agents/**`,
`CLAUDE.md`, `AGENTS.md`, and `CHANGELOG.md` (exact root filenames, not prefixes —
`CLAUDE.md.d/x` does not match). `CHANGELOG.md` stays on the list for the exemption to fire at
all — the coupled-change convention gives nearly every notable agent-configuration change a
changelog entry.

The mechanism moved, not the accepted scope, for a concrete reason: a `paths-ignore`-skipped
workflow never emits its job's context at all. If that context were ever made a required status
check — issue #707 tracks exactly this gap — or ever needed to satisfy merge-queue entry, a
workflow-level skip would leave it permanently `Expected`, unfixable even by an admin merge. This
was independently observed while merging PR #715. An in-job early exit does not have that failure
mode: the job always starts, checks out, computes the diff, and reports success either from the
stub path or from real evidence. None of the four contexts are required status checks today —
`merge readiness` remains the only one — so this is a latent-risk fix, not a response to an
already-broken required check.

No **product-gate lane** reads those files. The paths that do read them keep their own
unfiltered pre-merge coverage: `merge readiness / automation contracts` executes the `.claude/`
environment contract test from `tests/test_claude_environment.py`, and `release.yml` extracts
release notes from `CHANGELOG.md` at tag time and fails loudly (`test -s`) on a malformed file.
A pull request whose every changed file matches the list therefore still gets `run=false` from the
scope script, each heavy job exits its remaining steps fast, and the pull request merges on
`merge readiness` (plus `site reference freshness`) alone — exactly as under the retired
`paths-ignore` list.

The exemption is fail-closed the same three ways as before, now enforced by the script rather than
by GitHub's path evaluation. A single changed file outside the list restores the full pre-merge
run. A missing base/head SHA, a failed `git diff`, or an event the script does not recognize all
resolve to `run=true` — never to a stub. The `main` push, schedule, and dispatch triggers carry no
exemption at all: `tools/check-product-gate-scope.sh` returns `run=true` unconditionally for
those events, so every merged `main` state still receives the complete product evidence, and a
wrongly exempted defect surfaces through the existing post-merge reporting contract below instead
of reaching `production`. And a pull request into `production` that matches the list is blocked
rather than waved through: the script's `production` branch always returns `run=true`, so the
production ruleset's required contexts (`rust workspace`, `WASM`, the `native Z3 4.16` matrix)
still report real evidence — such a pull request should not exist, and the gate does not invent a
stub for it.

The script's `selftest` subcommand is the executable accepting/rejecting control for its
classifier (all-exempt, all-product, mixed, a filename-prefix near-miss, and fail-closed-on-empty
cases), run by `merge readiness / automation contracts` on every pull request and merge-group
event.

Growing the exempt list is a contract change to this decision, not a script tweak, and each
candidate needs the same evidence sweep: name every path that reads it and show that path keeps
unfiltered pre-merge or fail-loud coverage. `skills/**` and `docs/**` must never join it:
`skills/fsl/reference.md` moves with language features under the coupled-change contract,
`docs/LANGUAGE*.md` feeds the site-reference freshness gate, and product-gate literate
doc-contract tests read documentation files directly.

### Sharded pre-merge Linux evidence

Measured on run 30968645971 (PR #717): `semantic mutation (changed)` was the critical-path job at
38m15s, `rust workspace` was 32m43s, `WASM` was 6m08s, and `FSL Logic Test (pr)` was 1m09s. Inside
`rust workspace`, cache restore and compilation together were only ~3.5 min on a warm cache; the
remaining ~29 min was `cargo test` running 176 test binaries **strictly sequentially** — the
harness-sum across binaries (28.6 min) matches the observed wall clock, and five binaries
(`refine_corpus_parity` 7.34 min/4 tests, `explicit_engine` 4.58 min/12, `injection_detector_matrix`
3.75 min/1 test, `corpus_check_sweep` 3.16 min/3, `issue_226_auto_engine` 3.13 min/15) are 77% of
that time while 146 of the 176 binaries finish in under a second. Inside `semantic mutation`, the
curated fault-operator half's no-op control alone (`control no-op: all 17 operators' detectors
green`) took 912s and the full curated loop (`fault-operators: 17 operators calibrated`) took 1350s;
the generic cargo-mutants half took roughly the remaining ~14.3 min.

Both jobs are dominated by work that parallelizes cleanly across independent shards, so each is
split into a sharded lane plus an aggregator that keeps the exact required-context name:

- `rust workspace` = `rust-checks` (once) + `rust-tests` (`cargo-nextest`, 3-way `--partition
  count:K/3`) + the `rust-workspace` aggregator.
- `semantic mutation (…)` = `semantic-mutation-operators` (3-way round-robin shard of
  `operators.txt`) + `semantic-mutation-mutants` (generic cargo-mutants, complete and **deliberately
  unsharded** — see below) + the `semantic-mutation` aggregator.

Expected wall clock after sharding is **≈17 min**, down from 38m15s — a little over 2x, not 3x,
because both lanes carry a large unshardable fixed cost measured during this work and stated in the
floors below. The first sharded operator run measured it directly: a 6-operator shard's no-op control
took **760s** against 912s for all 17, so splitting the lane three ways removed only ~150s of
detector time, not two thirds of the lane. Do not quote a lower figure than ≈17 min without a fresh
measurement.

**Doctests moved to `rust-checks`, explicitly, because `cargo-nextest` cannot run them at all.**
Silently dropping them to make the sharded lane faster would be exactly the "weaken a gate for
speed" move this repository's invariants forbid, so
`cargo test --manifest-path rust/Cargo.toml --workspace --doc --locked` runs once, unsharded, in
`rust-checks`, alongside formatting, Clippy, the full workspace build, and the boundary/stack-parity
controls. `rust-tests` then shards only what `cargo-nextest` can run: everything else.

**The generic mutants lane is deliberately unsharded.** At ~14.3 min it is already close to the
curated operator lane's floor once that lane is split three ways (see below), and cargo-mutants
sharding would add real risk — mutant-inventory slicing, uncertain `--shard`/partition index-base
semantics in the pinned runner, and migrating the stale-reviewed-equivalents check to a sharded
world — for no measurable wall-clock gain. `semantic-mutation-mutants` therefore runs
`tools/run-semantic-mutation-gate.sh <mode> --lane mutants`, complete, on one runner, with the same
fresh-`CARGO_TARGET_DIR` anti-contamination behavior it has always had.

**Why `if: always()` is load-bearing on both aggregators.** GitHub treats a *skipped* required
status check as satisfied — it is not "pending" or "failing," it reads as passed. If `rust-workspace`
or `semantic-mutation` inherited the default skip-on-upstream-failure behavior, a failing shard would
skip the aggregator, and the skip would silently satisfy the `main` ruleset's required context instead
of blocking the merge: a confidently-green false negative on the exact two contexts this split
touches. `if: always()` forces the aggregator to run and evaluate its dependencies' results even when
one failed, so it can fail loudly instead of disappearing. The aggregator job **ids** are unchanged
(`rust-workspace`, `semantic-mutation`), so `product-gate`'s `needs:` list and the `main` ruleset's
required-context set (`rust workspace`, `semantic mutation (changed)`) both keep working without
their own edit.

**Union-completeness guards.** A scheduling change must not be able to silently narrow what runs, so
each aggregator downloads its shards' inventories and proves the split is a true partition of the
unsharded set before trusting a `success` result:

- `rust-workspace` downloads the three `rust-tests` shards' `full.txt`/`shard.txt` (written by
  `check_rust_tests` in `tools/check-native-integration.sh` *before* the shard's tests run, so the
  inventory exists even when a test in it fails), asserts the three `full.txt` files are
  byte-identical — three independent `cargo nextest list` invocations agreeing — then runs
  `tools/check-shard-union.sh full.txt shard1.txt shard2.txt shard3.txt`.
- `semantic-mutation` downloads the three operator shards' `shard-manifest.v1.json` (written by
  `tools/run-fault-operators.sh` only after a successful shard run), asserts identical
  `base_revision` and `table_operators` across all three, then checks that the disjoint union of
  their `executed_operators` equals `table_operators` with the same `check-shard-union.sh` logic.

`tools/check-shard-union.sh` is the generic, reusable primitive both checks build on: given one full
list and N shard lists, it fails closed — naming the offending entries — unless every shard is a
subset of the full list, the shards are pairwise disjoint, and their union equals the full list
exactly. Its `selftest` subcommand exercises an accepting three-way split and four rejecting cases
(an entry covered by no shard, an entry duplicated across shards, an invented shard entry, an empty
shard list) and is wired into `tools/check-merge-readiness.sh`'s `check_automation`, alongside
`check-product-gate-scope.sh selftest`.

**Agent-configuration-exempt pull requests still work.** Every shard job runs
`check-product-gate-scope.sh` itself and early-exits its own later steps when `run=false`, so the
shard's job result is still `success` and no artifact is ever uploaded. Both aggregators therefore
also run the scope step themselves: when `run=false` they trust the (trivially successful) shard
results and skip the artifact-download/union-validation steps outright, because there is nothing to
download.

**Floors — sharding buys parallelism, not a lower bound.** `refine_corpus_parity`'s slowest single
test (≈7.3 min) cannot be split further by this scheme, so together with the ≈3.5 min compile it
bounds `rust-workspace` at roughly ≈12 min no matter how the remaining 175 binaries are distributed.
Each `semantic-mutation-operators` shard independently pays the cold build in its own synced scratch
checkout (`tools/run-fault-operators.sh`'s `sync_scratch`), and that cost dominates the lane:
measured, a 6-operator shard's no-op control took 760s where all 17 operators took 912s, so roughly
700s of the 912s is the fixed scratch build and only ~12s per detector run scales with the shard.
The lane's floor is therefore ≈760s plus its ~26s-per-assigned-operator loop — about 15–16 min at
`K/3`, against 22.5 min unsharded, **not** 1350s ÷ 3. Raising the shard count barely helps; only
making that scratch build cheaper (caching `rust/target/fault-operators`, or sharing a warm target
dir without breaking the patch-isolation contract) would move this floor, and neither is attempted
here. Nobody should expect either lane to shrink further without changing what it measures.

`semantic mutation` is required on pull requests and every product-gate event. Ordinary pull
requests run all curated controls plus generic mutants intersecting the recorded base-to-head diff;
pushes, schedules, manual runs, and pull requests targeting `production` run the complete accepted
P2 pilot. The runner is pinned and
its raw machine-readable outcomes are retained as an artifact. A survivor, timeout, stale operator,
missing output, or incomplete job fails the lane; the aggregate gate never treats it as a deliberate
skip. [`DESIGN-semantic-mutation-gate.md`](DESIGN-semantic-mutation-gate.md) owns the detailed scope
and classification contract.

`FSL Logic Test` is likewise required everywhere. Ordinary pull requests run the bounded `pr` case
budget; other product events, including pull requests targeting `production`, run the larger
3-seed/3-depth `scheduled` matrix. Its artifact begins incomplete and is valid
only when expected, executed, and recorded case counts agree and every named semantic edge ran. A
missing or truncated report therefore cannot be mistaken for agreement. The detailed generation,
lineage, replay, shrink, and exclusion contract is in
[`DESIGN-fsl-logic-test.md`](DESIGN-fsl-logic-test.md).

Scheduled and manual runs use the same evidence. Pull requests into `production` also run the
complete product gate and emit the Linux native-Z3 compatibility context expected by the production
ruleset. Release jobs retain their independent four-target build, smoke, ABI, LSP, and packaging
checks. An always-running `product gate` aggregator fails unless every required lane emitted
successful evidence; an accidentally skipped lane cannot make the workflow confidently green.

Product-gate runs for merged commits are not cancelled. Each merged state therefore retains its own
portable evidence and failure attribution even when agents merge changes quickly.

## Required pre-merge contexts, and why the merge queue was rejected

The `main` ruleset (`main safety and CI`, id `19090811`) requires five contexts, applied
2026-08-05: `merge readiness`, `rust workspace`, `WASM`, `semantic mutation (changed)`, and
`FSL Logic Test (pr)`. `strict_required_status_checks_policy` is `true` and `bypass_actors` is
empty, so `current_user_can_bypass` is `"never"` for every account — an administrator cannot merge
past a failing or missing required context.

This closes the gap issue #707 opened: the Safe rollout section below has always required the
Linux evidence to be *required*, not merely running, and until this change only `merge readiness`
was. Making the other four required was blocked before, because `ci.yml`'s `paths-ignore` exemption
meant an agent-configuration-only pull request emitted none of those contexts at all, leaving them
permanently `Expected`. The in-job scope check that replaced `paths-ignore` (see
"Agent-configuration exemption" above) is what makes them safe to require: every gated job now
always reports, either from real evidence or from a fast early exit.

### The merge queue was tried, measured against this repository's workflow, and rejected

A GitHub merge queue was configured on this ruleset on 2026-08-05 and removed the same day. The
intent was to move the heavy Linux evidence from once per pull-request push to once per landed
change, using `ci.yml`'s `merge_group` trigger and a queue-entry stub path in
`tools/check-product-gate-scope.sh` gated on a `FSL_MERGE_QUEUE_CI` variable. Two measurements
killed it, both observed directly rather than reasoned about:

- **An administrator's merge bypasses the queue entirely.** `gh pr merge --admin` on #717 merged it
  directly (`7b140af`); the workflow-run history shows no `merge_group` event at any point, only the
  `push` run on `main`. The queue never validated anything.
- **The normal enqueue path is unsatisfiable in this repository.** `enqueuePullRequest` was refused
  with `New changes require approval from someone other than the last pusher`, from the separate
  `main review for non-admins` ruleset (id `19090821`: one approving review,
  `require_last_push_approval: true`). GitHub does not let an author approve their own pull request,
  and its admin bypass does not extend to the enqueue path, so a single-maintainer workflow cannot
  enqueue at all.

Together those mean every merge here would take the bypass path, so a queue would sit configured
and never run. Had `FSL_MERGE_QUEUE_CI=enabled` been set on top of that, the four heavy jobs would
have reported cheap stubs on every pull request while no `merge_group` run ever replaced them:
changes would land on `main` with **no pre-merge Linux evidence at all**, detected only by the
post-merge push run. That is the confidently-green false negative this repository's invariants
forbid, and it is what the canary pull request (#717) was for — the failure was found before the
variable was created, not after.

Reviving the queue requires changing *human review policy*, not CI configuration: the review
requirement in ruleset `19090821` would have to drop to zero approvals (removing review enforcement
for non-admin contributors too), or a second approving identity would have to exist. Neither is a
CI decision, so neither is made here. `ci.yml` keeps its `merge_group` trigger and the scope
script keeps its `queue-entry-stub` branch — both inert, both harmless, and both ready if that
policy question is ever answered differently. Until then, per-push cost is unchanged from a
human-review-policy standpoint. Of the ordinary levers named here previously, job splitting is now
exercised — see "Sharded pre-merge Linux evidence" above — and cache hit rates were measured to have
no headroom: compile is only ~3.5 min of the ~33 min `rust workspace` measured on a warm cache, so a
better hit rate could not have bought back more than that. The remaining lever is moving specific
lanes post-merge under a new accepted decision.

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

1. Merge the workflow change. `rust workspace` and `WASM` begin running on pull requests
   immediately, as visible but not-yet-required evidence.
2. In the `main` repository ruleset, add `rust workspace` and `WASM` alongside `merge readiness`.
   `native Z3 4.16 (macos-15)` and `native Z3 4.16 (windows-latest)` stay out of the required set —
   they are the deferred cross-platform matrix. **Completed 2026-08-05**, together with
   `semantic mutation (changed)` and `FSL Logic Test (pr)`; see "Required pre-merge contexts, and
   why the merge queue was rejected" for the live set and for why requiring them was blocked until
   the `paths-ignore` exemption moved in-job.
3. Leave `FSL_OPTIMISTIC_CI=enabled`. It now governs only the cross-platform matrix and the
   aggregate `product gate` context; main pushes, schedules, manual runs, and production promotions
   ignore it as before.

Rollback is fail-safe and has two independent levers. To return the Linux evidence to post-merge,
restore the `if:` conditions on `rust workspace` and `WASM` and drop them from the ruleset. To return
to a fully pre-merge gate, delete or change `FSL_OPTIMISTIC_CI` and restore all four product contexts
in the main ruleset. No source or product artifact migration is involved either way.

### Why the ruleset step is not optional

A job that runs on a pull request but is not a required context is *advisory*: it can fail while the
merge still succeeds. Until step 2 lands, this change buys visibility, not enforcement — and the
gap it exists to close is precisely one that a disciplined developer already covers by running
`./tools/check-native-integration.sh` locally. The point of the change is to stop depending on that
discipline, which step 1 alone does not achieve.

## Non-goals

- Treating issue creation as a substitute for repair, revert, or release blocking.
- Hiding platform failures with `continue-on-error`.
- Trusting agent-authored verification claims in place of independent GitHub-hosted execution.
- Changing FSL language, Kernel, CLI/JSON, LSP, Worker, or frozen Python compatibility behavior.
- Running a GitHub merge queue. One was configured and removed on 2026-08-05 after direct
  measurement; reviving it is a human-review-policy decision, not a CI one. See "Required
  pre-merge contexts, and why the merge queue was rejected".
- Relaxing the review requirement in ruleset `main review for non-admins` (id `19090821`) to make
  a merge queue enqueueable. That trades review enforcement for non-admin contributors against CI
  latency and needs its own accepted decision.
