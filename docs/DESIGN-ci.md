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
that falls behind `main` must re-run its checks, a serial chain of rebases pays that repeatedly. One
mitigation exists and is worth using before the cost is treated as inherent: independent changes can
share one pull request with one commit per topic.

A merge queue would be the other, batching several candidates into one validation, and both
`merge-readiness.yml` and `ci.yml` carry a `merge_group` trigger for exactly that. **It is not an
available mitigation.** The queue itself was configured on the `main` ruleset on 2026-08-05 and
removed the same day; both triggers are inert and enabling one changes nothing, because no
`merge_group` event fires without a queue. Reviving it is a human-review-policy decision, not a CI
one — see "Required pre-merge contexts, and why the merge queue was rejected" below, and the
"Non-goals" entry that follows from it. This paragraph previously recommended the queue as a
mitigation "once `ci.yml` gains the same trigger"; `ci.yml` gained it, the queue was then measured
and rejected, and the recommendation was left behind. It is corrected here rather than deleted so the
sequence stays legible.

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
stub path or from real evidence. None of the four contexts were required status checks *at the
time this mechanism moved* — `merge readiness` was the only one — so this was a latent-risk fix,
not a response to an already-broken required check. The requirement change landed 2026-08-05,
making `rust workspace`, `WASM`, `semantic mutation (changed)`, and `FSL Logic Test (pr)` required
alongside `merge readiness`; `site reference freshness` joined the required set afterward, closing
issue #707's drift-detection half (see "Required pre-merge contexts, and why the merge queue was
rejected" below for the live set, and "Ruleset drift audit" for why a required context can no
longer drift silently).

No **product-gate lane** reads those files. The paths that do read them keep their own
unfiltered pre-merge coverage: `merge readiness / automation contracts` executes the `.claude/`
environment contract test from `tests/test_claude_environment.py`, and `release.yml` extracts
release notes from `CHANGELOG.md` at tag time and fails loudly (`test -s`) on a malformed file.
A pull request whose every changed file matches the list therefore still gets `run=false` from the
scope script, each heavy job exits its remaining steps fast, and the pull request merges once all
six required contexts report: real evidence from `merge readiness` and `site reference freshness`
(neither reads the exempt paths, so neither is affected by the stub), and fast stub-success
evidence from `rust workspace`, `WASM`, `semantic mutation (changed)`, and `FSL Logic Test (pr)` —
exactly as under the retired `paths-ignore` list.

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

Measured wall clock after sharding is **20.7 min**, down from 38m15s — 1.85x, saving ~17.5 min per
pull request. Recorded from the first sharded run (30989320577, PR #719), all lanes green:

| lane | wall clock |
| --- | --- |
| `mutation operators (1/3)` | **20.3 min** ← critical path |
| `mutation operators (2/3)` / `(3/3)` | 19.0 / 18.4 min |
| `rust tests (1/3)` | **18.1 min** |
| `rust tests (3/3)` / `(2/3)` | 12.6 / 8.3 min |
| `mutation mutants` | 16.7 min |
| `WASM` | 5.9 min |
| `rust checks` | 2.7 min |
| `FSL Logic Test (pr)` | 1.1 min |
| both aggregators | 0.1 min each |

Two costs are visible in that table and are the honest limits of this change, not incidental noise:

- **`cargo-nextest --partition count:K/N` balances by test count, not duration.** The three shards
  received 518/460/411 tests and took 18.1/8.3/12.6 min — shard 1 is 2.2x shard 2, so `rust workspace`
  finishes on its slowest shard at ≈18 min rather than the ≈13 min a duration-balanced split would
  give. Recovering that ~5 min needs a duration-aware assignment (pinning the known-slow binaries to
  separate shards), which `count:` cannot express.
- **Sharding the curated operator lane bought about 10%, not two thirds.** 22.5 min unsharded became
  20.3 min at `K/3` — three times the compute for ~2 min of wall clock — because the fixed cold build
  in each shard's synced scratch checkout dominates. It is retained because runner minutes are free on
  a public repository and it is the current critical path's only lever in this change, but it is a
  poor trade and the floors below say what would actually move it.

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
checkout (`tools/run-fault-operators.sh`'s `sync_scratch`), and that cost dominates the lane. Locally,
a 6-operator shard's no-op control took 760s where all 17 took 912s; in CI the sharded lane landed at
18.4–20.3 min against 22.5 min unsharded, so the fixed scratch build is an even larger share there.
Raising the shard count cannot fix this. The two levers that would, neither attempted here, are:

- caching `rust/target/fault-operators` so the scratch build starts warm — the same
  `Swatinem/rust-cache` treatment the main lanes already get, and the one place in this gate where
  caching genuinely is the bottleneck (it is not, for `rust workspace`: compilation there is ~3.5 min
  of 33 on a warm cache);
- a duration-aware `rust-tests` assignment, worth ~5 min on its own (see the imbalance above).

With both, the gate would plausibly reach ≈13 min; without them, 20.7 min is the floor this design
delivers. Nobody should expect either lane to shrink further without changing what it measures or how
its scratch build is warmed.

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

### Actions cache budget

GitHub gives a repository **10 GiB** of Actions cache and evicts least-recently-used entries once
that is exceeded. Caches are also **ref-scoped**: a run restores only its own ref's caches and the
default branch's, so a pull request's cache is worthless to a sibling pull request while still
counting against the shared limit.

`ci.yml` declares four shared keys, measured: `semantic-mutation` 2.72 GiB, `rust-workspace`
1.50 GiB, `fsl-logic` 1.37 GiB, `wasm` 1.35 GiB — about **6.9 GiB per ref**. Two concurrent pull
requests therefore exceed the limit on their own, and on 2026-08-06 they did: usage stood at
9.96 GiB across 12 entries, every large cache belonged to `refs/pull/743/merge` or
`refs/pull/745/merge`, and `refs/heads/main` held only three tool binaries totalling 26 MiB — no
Rust build cache at all. A `main` push four hours earlier had restored one in 25–50 s, so it had
existed and been evicted.

The consequence is measured, not inferred. Two runs of the same commit on the same branch:

| | shard 1 | shard 2 | shard 3 | `rust-cache` restore |
|---|---|---|---|---|
| cold | 27.88 | 20.81 | 20.61 | 0–1 s (miss) |
| warm | 12.2 | 10.85 | 10.46 | 24–25 s (hit) |

**+8 to +16 min per shard**, independently on each of `rust workspace`'s three shards, `WASM`,
`FSL Logic Test` and `semantic mutation`. And it is self-reinforcing: each cold run saves a fresh
ref-scoped copy, which evicts more. `main` can heal — a miss there does save — but the pressure
from concurrent pull requests outran the healing.

**Decision: only non-pull-request events save.** Every `Swatinem/rust-cache` step in `ci.yml`
carries `save-if: ${{ github.event_name != 'pull_request' }}`. Pull requests still *restore*,
because `main` is the default branch and therefore readable from every ref. What a pull request
gives up is a warm second run of itself; what it gains is that `main`'s caches stay resident, which
is the only cache any pull request could ever share.

`merge-readiness.yml` is deliberately **not** changed. Its two keys total about 131 MiB, they are not
the pressure, and its lanes are the sub-minute fast path — making them cold would defeat the reason
that workflow exists.

This also qualifies a claim made elsewhere in this document. Cache hit rates were measured to have
no headroom for `rust workspace` — compile is only ~3.5 min of ~33 min **on a warm cache**. That
remains true warm, and it is exactly the premise that fails under concurrency: the question is not
how much a better hit rate buys, but whether a hit happens at all.

**Eviction started this; `cache-on-failure: false` made it unrecoverable.** The
`semantic mutation` lane fell into a closed loop, measured on `main` and on three pull requests:

1. a cold scratch build exceeds the job's budget, so the job is cancelled;
2. `Swatinem/rust-cache` does not save from a failed job (`cache-on-failure` defaults to false),
   so nothing is written;
3. the next run is cold again.

**The cache can then only be created by a run that succeeds, and a run can only succeed once the
cache exists.** Measured budgets against measured durations:

| job | budget before | warm | cold | budget now |
|---|---|---|---|---|
| `mutation operators (K/3)` | 30 min | 18.0–19.5 min | **>30** (cancelled at 30.2) | **50 min** |
| `mutation mutants` | 60 min | 17.2–34.2 min | **>60** (cancelled at ~61) | **90 min** |

Both semantic-mutation cache steps now carry `cache-on-failure: true`, so a cold run that runs out
of budget still leaves a warm cache behind, and both budgets are raised past a measured cold run.
Raised rather than narrowed, for the reason this document already gives for the promotion-only
native-Z3 job: a gate that runs out of wall clock reports a failure it did not observe.

This is also the most likely explanation for `main`'s standing post-merge failures #721
(`mutation mutants`) and #678 (`semantic mutation (complete)`), whose cancellations sit exactly at
the old budgets. Whether they clear once this lands is the test of that reading, and #747's
acceptance criteria record it as such.

**The control.** `.github/scripts/audit-cache-budget.mjs` is a pure function over a fetched cache
listing; `.github/workflows/cache-budget-audit.yml` fetches and runs it on a schedule, on dispatch,
and on `main` pushes that touch it or `ci.yml`. It fails closed on three states: usage at or above
85% of the limit, a missing `refs/heads/main` cache for any critical-path shared key, and — the
rejecting control for the `save-if` guard itself — **any pull-request-scoped cache for one of
`ci.yml`'s shared keys**, which can only appear if that guard is removed. An unreadable listing or
an absent usage total fail closed too; neither is read as headroom.

`.github/scripts/audit-cache-budget.test.mjs` calibrates all of it offline, including a fixture that
reproduces the 2026-08-06 listing verbatim and must fail. `tools/check-merge-readiness.sh`'s
`check_automation` lane runs that suite on every pull request, so a change to the checker is covered
pre-merge even though the live audit deliberately is not a required context: the shared cache state
can change after a pull request's own checks pass, so gating a merge on it would gate on something
outside the change under review.

Issue #747 records the incident. Issue #720's Finding 2 — warming the fault-operator scratch build —
**adds** a cache and therefore depends on this budget holding first.

## Required pre-merge contexts, and why the merge queue was rejected

The `main` ruleset (`main safety and CI`, id `19090811`) requires six contexts: `merge readiness`,
`rust workspace`, `WASM`, `semantic mutation (changed)`, and `FSL Logic Test (pr)` (applied
2026-08-05), plus `site reference freshness` (added to the contract as part of closing issue
#707's drift-detection half; see "Ruleset drift audit" below). `site reference freshness` reports
on every pull request with no path filter — `.github/workflows/site-reference-freshness.yml`'s
`pull_request`/`merge_group` triggers carry no `paths:` restriction — so requiring it cannot
deadlock a pull request the way requiring `native Z3 4.16` or `product gate` would; those stay
deferred precisely because they never report on an ordinary pull request.
`strict_required_status_checks_policy` is `true` and `bypass_actors` is empty, so
`current_user_can_bypass` is `"never"` for every account — an administrator cannot merge past a
failing or missing required context.

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

### Ruleset drift audit

Issue #707 has two halves. The first — making the Linux evidence *required*, not merely running —
is the section above. The second, closed here, is the reason the first half was needed at all: the
accepted design required five (now six) contexts to be required on the `main` ruleset, and the live
ruleset silently required only `merge readiness` until 2026-08-05. Nothing detected that gap; it
was found by manual inspection. `.github/scripts/audit-ruleset-drift.mjs`, its calibration suite
`.github/scripts/audit-ruleset-drift.test.mjs`, and the checked-in `.github/ruleset-contract.json`
exist so the same silent drift cannot happen again undetected.

**Scope, by discovery-by-id.** The audit fetches ruleset `19090811` by id (`GET
/repos/ymm-oss/fsl/rulesets/19090811`, readable unauthenticated for this public repository) and
compares it against the matching entry in `.github/ruleset-contract.json`. Every context the
contract or the live ruleset can mention falls into exactly one of four kinds, and the contract
records which:

- **required** — the six contexts above; missing, renamed, or extra membership is drift.
- **deferred** — `native Z3 4.16 (macos-15)`, `native Z3 4.16 (windows-latest)`, and `product gate`
  (the `FSL_OPTIMISTIC_CI`-gated cross-platform matrix and its aggregate, "Product gate contract"
  above): these never report on an ordinary pull request, so requiring them would deadlock every
  one; an audit finding that one of them is present in the live required set gets a sharper message
  saying exactly that, rather than being reported as an ordinary unexpected context.
- **constituent** — the sharded lane names (`rust checks`, `rust tests (K/3)`, `mutation operators
  (K/3)`, `mutation mutants`, "Sharded pre-merge Linux evidence" above): only their `if: always()`
  aggregator carries the required-context name, so the contract records these explicitly to stop a
  future reviewer from "fixing" the ruleset by requiring a shard directly — that would let a single
  skipped shard silently satisfy the ruleset, the exact false-negative shape that section's
  `if: always()` discussion warns about.
- **unaudited** — the `pull_request` rule's own parameters (`required_approving_review_count`,
  `allowed_merge_methods`, review dismissal, …). These are **deliberately never read**. Human
  review policy is a separate accepted process decision (ruleset `19090821`, "The merge queue was
  tried…" above); this audit's job is CI-evidence configuration, not review policy. An editable
  `allowed_merge_methods` change is the audit's blind control — its calibration suite proves the
  audit stays clean when that field changes, so a green audit is never accidentally load-bearing
  evidence for review-policy drift it was never designed to catch.

**`bypass_actors` is in scope, and its absence is a failure, not a pass.** An added bypass actor is
exactly what would let a failing required check merge anyway — it defeats every other guarantee
this audit checks. But GitHub returns `bypass_actors` only to a caller with write access to the
ruleset, and the workflow's own `GITHUB_TOKEN` has no `administration` permission at all, so it
cannot observe the field even though the rest of the ruleset is public. `RULESET_AUDIT_TOKEN`, a
fine-grained PAT scoped to read that permission, exists for exactly this leg. Absence of the field
is never read as an empty (safe) list — "absence of evidence is not evidence of emptiness" — so an
unset or revoked `RULESET_AUDIT_TOKEN` makes the audit fail closed with `bypass-actors-unobserved`
rather than silently skip the check it exists for.

**The seam.** `compareRuleset(contractEntry, observationJson)` and `validateContract(contract)` are
pure — no network, no filesystem, no process access — so every case in the calibration suite is
built by `structuredClone`-ing the checked-in fixture (`.github/scripts/fixtures/ruleset-19090811.json`,
a verbatim capture) and mutating exactly one field, with no live ruleset ever touched by a test.
The suite covers the accepting case (the fixture is clean against the contract, which also freezes
contract/fixture agreement — editing one without the other fails pre-merge), two rejecting cases
(a dropped context; a renamed context, which must surface as one `missing` plus one `unexpected`
rather than being silently satisfied by name), the blind control above, and the fail-closed guards
(empty rules, empty required-context list, missing/added `bypass_actors`, an unexpected rule type,
wrong enforcement, flipped strict policy, retargeted conditions, a schema-invalid contract) —
including, for the network-facing wrapper, an injected fetch failure and a 404 against a fake
client, both of which must still create the failure issue. This suite runs in
`tools/check-merge-readiness.sh`'s `check_automation` lane (so a contract, fixture, or checker
change gets pre-merge evidence — `.github/**` is not on the agent-configuration exemption's path
list) and again as the live workflow's first step.

**Issue lifecycle.** `reconcileRulesetDrift` in `audit-ruleset-drift.mjs` imports
`report-post-merge-ci.mjs`'s exported `GitHubRestClient` rather than building a second REST client
and a second dedupe mechanism, and keeps that file's `reconcilePostMerge` *shape* — one canonical
issue per key (`ci/ruleset-drift` label, marker `<!-- ruleset-drift:19090811 -->`), an occurrence
marker per run so re-runs are idempotent, reopen on recurrence, close with a recovery comment on
the next clean audit — without trying to parameterize `reconcilePostMerge` itself: its keying,
recovery predicate, and out-of-order run redirection are shaped around a workflow run's jobs and do
not mean anything for a single ruleset comparison. The issue body lists finding classes and details
plus a link to the run's raw-observation artifact, never token material or log text, and its footer
states the two legitimate exits — revert the live ruleset, or amend the contract, the fixture, and
this document together in one pull request — and names `RULESET_AUDIT_TOKEN` as the secret to
rotate when the finding is `bypass-actors-unobserved`.

**A legitimate ruleset change is a three-surface move in one pull request:** this document, the
contract, and a re-captured fixture, with the live ruleset edit applied before that pull request
merges. Editing `.github/ruleset-contract.json` alone to make the audit agree with unexplained live
state is precisely the drift this audit exists to catch, not a fix for it — the same principle
"Growing the exempt list is a contract change to this decision, not a script tweak" states above
for the agent-configuration exemption applies here to the required-context set.

`.github/workflows/ruleset-drift-audit.yml` runs on a daily `schedule`, `workflow_dispatch`, and a
path-filtered `push` to `main` (the four files above, so a legitimate contract change self-verifies
minutes after merge). It deliberately carries no `pull_request` trigger: a fork pull request has no
access to `RULESET_AUDIT_TOKEN` or a writable `GITHUB_TOKEN`, so running there would produce
spurious failures indistinguishable from real drift. Pre-merge coverage is the offline calibration
suite above, not a live run.

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
