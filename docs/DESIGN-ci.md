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

`changelog.d/` joined the exempt list as a directory prefix
(`docs/DESIGN-changelog-fragments.md`, issue #737): `tools/check-product-gate-scope.sh`'s
`is_exempt_path` matches `changelog.d/*`, with the filename-prefix near-miss `changelog.dx/y`
still classifying as `product` (the same shape as the existing `CLAUDE.md.d/x` case). Its two
readers keep the coverage this decision requires of every exempt path:

- `.github/workflows/release.yml` reads it fail-loud, at tag time, before "Extract release
  notes": a fragment still present under `changelog.d/` fails the guard
  (`stale-fragments-present`) rather than silently shipping notes with a missing entry.
- `.github/workflows/merge-readiness.yml`'s `automation contracts` job reads it unfiltered, on
  every pull request and merge-group event, via `tools/aggregate_changelog.sh check-pr`: a
  fragment's name and (id, category) uniqueness are validated regardless of the exemption
  above, and a product-surface change with no corresponding fragment fails closed
  (`changelog-fragment-missing`) even when every other changed path is exempt.

Neither reader is ever skipped by this exemption: it only ever skips the four **heavy**
product-gate jobs (`rust workspace`, `WASM`, `semantic mutation`, `FSL Logic Test`), never
`merge readiness` or `release.yml`.

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

- `rust workspace` = `rust-checks` (once) + `rust-tests` (`cargo-nextest`, 3-way, duration-aware —
  see "Duration-aware `rust-tests` shard pinning" below) + the `rust-workspace` aggregator.
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

Two costs were visible in that table and were the honest limits of that first change, not
incidental noise:

- **`cargo-nextest --partition count:K/N` balances by test count, not duration.** The three shards
  received 518/460/411 tests and took 18.1/8.3/12.6 min — shard 1 is 2.2x shard 2, so `rust workspace`
  finished on its slowest shard at ≈18 min rather than the ≈13 min a duration-balanced split would
  give — which it now does, measured: see "Duration-aware `rust-tests` shard pinning" below. A
  rerun of the same commit gave 15.6/5.0/8.7 min, a **3.1x** spread: the skew is not a fixed
  property of the split but varies run to run, which is what makes an explicit assignment worth more
  than a better hash. Issue #720 Finding 1 addressed this — see "Duration-aware `rust-tests` shard
  pinning" below —
  by pinning the known-slow binaries to distinct shards explicitly, which `count:` alone cannot
  express.
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
exactly. It has a second mode, `check-groups`, described under "Duration-aware `rust-tests` shard
pinning" below. Its `selftest` subcommand exercises the union form's accepting three-way split and
five rejecting cases (an entry covered by no shard, an entry duplicated across shards, an invented
shard entry, an empty shard list, and an entire binary's tests dropped from every shard), plus
`check-groups`'s accepting case and two rejecting cases (a pin naming a binary-id the live workspace
no longer has, and one binary pinned to two shards). It is wired into
`tools/check-merge-readiness.sh`'s `check_automation`, alongside
`check-product-gate-scope.sh selftest`.

**Agent-configuration-exempt pull requests still work.** Every shard job runs
`check-product-gate-scope.sh` itself and early-exits its own later steps when `run=false`, so the
shard's job result is still `success` and no artifact is ever uploaded. Both aggregators therefore
also run the scope step themselves: when `run=false` they trust the (trivially successful) shard
results and skip the artifact-download/union-validation steps outright, because there is nothing to
download.

**Floors — sharding buys parallelism, not a lower bound.** `refine_corpus_parity`'s slowest single
test is an indivisible 458.8s (7.65 min) under this scheme. A `rust-tests` shard's wall clock is
`pinned phase + leftover phase + fixed cost`, because the two `cargo nextest run` invocations execute
**serially** inside the shard — so the floor for whichever shard holds that test is 7.65 min plus
its share of the leftover plus the fixed ≈1m50s of checkout, toolchain and build. An earlier version
of this paragraph put the floor at ≈12 min "no matter how the remaining 175 binaries are distributed",
adding a ≈3.5 min compile to the 7.3 min test. Both parts were wrong: the fixed cost is ≈1m50s, not
3.5 min, and the leftover term was missing entirely — leftover distribution is exactly what run
31076668077's shard 2 lost nine minutes to. See "Duration-aware `rust-tests` shard pinning" below for
the measured decomposition and what the floor actually is.
Each `semantic-mutation-operators` shard independently pays the cold build in its own synced scratch
checkout (`tools/run-fault-operators.sh`'s `sync_scratch`), and that cost dominates the lane. Locally,
a 6-operator shard's no-op control took 760s where all 17 took 912s; in CI the sharded lane landed at
18.4–20.3 min against 22.5 min unsharded, so the fixed scratch build is an even larger share there.
Raising the shard count cannot fix this. Two levers would move it further:

- caching `rust/target/fault-operators` so the scratch build starts warm — the same
  `Swatinem/rust-cache` treatment the main lanes already get, and the one place in this gate where
  caching genuinely is the bottleneck (it is not, for `rust workspace`: compilation there is ~3.5 min
  of 33 on a warm cache). **Not attempted** — issue #720 Finding 2, tracked separately because it
  changes a different mechanism (`tools/run-fault-operators.sh`'s scratch checkout) with its own
  patch-isolation contract, and because #720 asks that a possible revert of the operator sharding be
  evaluated in the same change once this lands, which needs its own review;
- a duration-aware `rust-tests` assignment, worth **3.4 min measured**. **Landed** — issue #720
  Finding 1. Its first form delivered about 0.1 min and its second delivers 3.4; see
  "Duration-aware `rust-tests` shard pinning" below for both measurements and what changed between
  them.

With Finding 1 landed, `rust workspace`'s slowest shard is **12.2 min measured**, against ≈15.6 before
it and the ≈13 min this section projected. The remaining floor is ≈10.4 min — the indivisible 458.8s
test plus the measured ≈113s of fixed cost — so most of what is left is that one test. Finding 2
remains 20.3 min at best until it lands; nobody should
expect either lane to shrink further without changing what it measures or how its scratch build is
warmed.

### Duration-aware `rust-tests` shard pinning

Issue #720 Finding 1. No other heading in this document carries a parenthetical issue tag, and the
four citations of this section elsewhere in the file quote the heading without one, so the tag lives
here in the body instead.

`cargo-nextest --partition count:K/N` assigns tests to shards by count, with no notion of how long
each test takes, so the handful of binaries holding most of the suite's sequential wall clock could
land in the same shard, or unevenly across shards, by chance. `check_rust_tests` in
`tools/check-native-integration.sh` replaces the single `--partition` invocation with two, unioned:

1. `tools/rust-test-shard-groups.txt`, a checked-in text file pinning specific binary-ids to specific
   shards (`<shard> <binary-id>`, comments with `#`), read once per shard invocation. Each shard's
   pinned binaries run through `cargo nextest run -E 'binary_id(=…) or binary_id(=…) …'` — the
   exact-match `=` name-matcher (`cargo nextest help filterset`) — unpartitioned, so a pinned binary is
   never split across shards or forced to share a shard with another pin by count-hash chance. The
   assignment and the measurements behind it live in that file; the cost model it uses is below.
2. Every test *not* in any pinned binary still goes through the original `--partition count:K/N`,
   scoped by `-E 'not binary_id(=…) and not binary_id(=…) …'` excluding every pinned binary (not just
   this shard's), so a pinned binary's tests are never double-counted into another shard's leftover
   share. Proven end to end by run 31076668077's aggregator, whose guard reported
   `check-shard-union: PASS -- 1419 entries, 3 shard(s), union matches exactly`, with the three shard
   logs showing 4+519, 15+457 and 16+408 tests — 1419, matching the unfiltered inventory exactly. That
   run used the five-binary assignment; the pin list has since changed but the mechanism has not, so
   the next `product gate` run re-proves the union for the current list.

**Measured, in two forms.** The first form did not deliver; the second does. All figures below
are warm-cache runs, which matters: an eviction-induced cold build adds 6-12 min per shard and
makes any comparison across cache states meaningless (see issue #747).

| | shard 1 | shard 2 | shard 3 | slowest | spread |
|---|---|---|---|---|---|
| baseline run 1 | 18.1 | 8.3 | 12.6 | **18.1** | 2.2x |
| baseline run 2 | 15.6 | 5.0 | 8.7 | **15.6** | 3.1x |
| first form, 5 pins (run 31076668077) | 15.5 | 14.6 | 8.75 | **15.5** | 1.77x |
| second form, 8 pins (run 31081427765 attempt 2) | **12.2** | **10.85** | **10.46** | **12.2** | **1.17x** |

The slowest shard — the only quantity `rust workspace` waits on — fell from 15.6 min to
**12.2 min**, and the spread from 3.1x to 1.17x. `tools/check-shard-union.sh` reported a clean
union on that run.

**The first form failed for two measurable reasons, both since fixed.**

*The cost model was wrong, twice over.* The original assignment packed by the sum of each
binary's sequential minutes. Corrected to the slowest single test, it then underestimated a shard
holding several long binaries by 52%. The model that fits all three shards is
**`1.11 × max(slowest single test, sequential sum / 3)`**:

| shard | pinned sum | slowest | model | actual | error |
|---|---|---|---|---|---|
| 1 | 683.4s | 458.8s | 509s | 508.1s | −0.2% |
| 2 | 922.3s | 371.8s | 413s | 465.2s | +12.7% |
| 3 | 1130.4s | 275.6s | 418s | 418.5s | +0.1% |

Neither term alone works, so adding a pin is **not** free: it raises the sum, and once `sum/3`
exceeds the slowest test the phase grows with each addition. Shard 2 is the one outlier at +12.7%,
and the reason is specific: `issue_697_all_properties_memory` is memory-bound by construction
(`CONCRETE_PROBE_BUDGET`, issue #697), so its tests contend for memory rather than CPU and overlap
less than the model assumes. Treat +15% as the planning margin for whichever shard holds it.

*The pinning file was stale before the first form merged.*
`fslc-rust::issue_697_all_properties_memory`, whose 371.8s test is the workspace's second slowest,
arrived with the #739 merge for issue #697 and was not pinned. Its test landed in shard 2's count
partition, which is exactly what took shard 2 from 5.0 min to 14.6 min.

**The leftover skew is reduced, not removed.** Pinning is the only lever currently available
against it, because every pin takes its binary's whole sequential duration out of the
count-partitioned remainder. The second form's leftover phases are 75.4s / 43.0s / 63.9s — a
**1.75x spread, down from 5.3x**. Making the leftover itself duration-aware is not attempted:
`--partition count:` takes no duration input, so it would mean a checked-in assignment covering
every test binary, trading a small maintained file for a large one. At 1.75x that trade is not
currently worth making; if the spread returns above roughly 2x, it is the next thing to evaluate,
as its own change.

**Where the floor is.** `refine_corpus_parity`'s 458.8s single test is indivisible under this
scheme, and each shard pays a constant ≈113s of checkout, toolchain and build (measured, all three
shards). So whichever shard holds that test cannot go below about **10.4 min** even with an empty
leftover, and the measured 12.2 min is within two minutes of that. The ≈13 min this document
projected earlier for `rust workspace` **is met, measured at 12.2 min** — it was unreachable with
the wrong pinning, not unreachable in principle. Going materially below 10 min needs one of:
splitting that test (and `issue_697_all_properties_memory`'s 371.8s one), or running the pinned and
leftover phases concurrently instead of serially. Neither is attempted here; the second would need
care because the two phases currently write one `shard.txt` between them.

**Coverage cannot silently drop, by construction, independent of this file's accuracy.** A binary this
file does not name is not "unhandled" — it simply is not pinned to anyone, so it falls into the
ordinary count-partitioned leftover exactly as before. A new test binary landing with nobody updating
`tools/rust-test-shard-groups.txt` therefore still runs, in whichever shard the count partition puts
it; only duration balance, not coverage, depends on the file being current. Coverage is still proven
the same way as before this change: `check_rust_tests` writes `full.txt` (unfiltered) and `shard.txt`
(this shard's union of pinned + leftover) exactly as it always has, and `rust-workspace`'s aggregator
still runs `tools/check-shard-union.sh full.txt shard1.txt shard2.txt shard3.txt` against them with
that invocation's behaviour unchanged — the script gained a `check-groups` mode, a fifth rejecting
selftest case for the union form, and four rejecting plus two accepting cases for `check-groups`, but
the union form itself was not touched, and the shape of the files it consumes did
not change, so that guard keeps validating the new mechanism without alteration.

**When to update `tools/rust-test-shard-groups.txt`.** Coverage never depends on it, but two cases do
require an edit, and neither is discoverable from the file alone:

- **Renaming or removing a pinned binary hard-fails every shard.** `check-groups` runs after the one
  unfiltered `cargo nextest list` (which supplies both `full.txt` and the live binary set) and before
  the partition, and rejects a pin whose binary-id is absent from that live set, so
  `rust workspace` goes red until the pin is updated. That is deliberate — a silently stale pin would
  degrade balance invisibly — but it means a rename is a two-file change: the test binary and this
  file. The failure message names the file and the offending id.
- **Adding a binary whose slowest single test exceeds roughly a minute.** The quantity that matters
  is the slowest *individual* test, not the binary's sequential total — see the cost model above. If
  that test is slower than every test already pinned to some shard, pin it to the shard with the
  smallest current maximum, because it will become that shard's pinned-phase cost. If it is faster
  than an existing pin's slowest test, pinning it is nearly free on that shard and still worth doing:
  it takes the binary's whole sequential time out of the duration-blind leftover. Adding a pin is a
  single data edit and no code change, the same shape as adding a row to
  `rust/fslc/tests/fault_operators/operators.txt`.

What *is* new is a second, narrower guard: `tools/check-shard-union.sh check-groups
tools/rust-test-shard-groups.txt <live-binary-ids> <shard-total>`, run once per shard before any
listing or partition happens. It fails closed if the grouping file names a binary-id the live
workspace's `cargo nextest list` no longer reports (a stale pin surviving a rename or removal) or pins
the same binary-id to more than one shard. Its `selftest` cases (in `tools/check-shard-union.sh`,
wired into `merge readiness / automation contracts` alongside the pre-existing shard-union selftest)
cover an accepting config, an unknown pinned binary-id, and a duplicate pin; a further rejecting case
added to the pre-existing `check_union` selftest proves that guard catches an entire binary's tests
(not just one stray entry) dropped from every shard, since that is the failure shape a whole-binary
pin actually risks.

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
that is exceeded. Caches are also **ref-scoped**: a run restores its current ref's caches, its base
branch's caches, and the default branch's caches. For a pull request targeting `main`, base and
default are both `main`, so a sibling pull request's cache remains worthless while still counting
against the shared limit.

`ci.yml` declared four shared keys at the time (`fsl-logic` was one of them; it does not declare one
today, see below), measured: `semantic-mutation` 2.72 GiB, `rust-workspace`
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

**Decision (#747): only non-pull-request events save.** At the time, every `Swatinem/rust-cache` step
in `ci.yml` carried `save-if: ${{ github.event_name != 'pull_request' }}`. Pull requests still
*restore*, because `main` is the default branch and therefore readable from every ref. What a pull
request gives up is a warm second run of itself; what it gains is that `main`'s caches stay resident,
which is the only cache any pull request could ever share.

**That is no longer a description of every step -- the current contract has two shapes, not one, and
which key a restore-only step reads is not the same for all of them.** A step that saves its own
declared key still carries the event guard above (`save-if: ${{ github.event_name != 'pull_request'
}}`); a step below carries `save-if: false` unconditionally and saves nothing, ever, instead. Naming
each: `merge-readiness.yml`'s `rust-compile` job reads `rust-workspace` (`shared-key: rust-workspace`,
`save-if: false`). `merge-readiness.yml`'s `core-contracts` job reads `rust-workspace` the same way.
`ci.yml`'s `fsl-logic` job reads `rust-workspace` the same way again. `ci.yml`'s
`semantic-mutation-mutants` job is different: it reads `semantic-mutation`
(`shared-key: semantic-mutation`, `save-if: false`), the key `semantic-mutation-operators` owns and
saves (below), not `rust-workspace`. `CI_SHARED_KEYS` in `audit-cache-budget.mjs` lists the
identities rule 3 attributes by name today: `rust-workspace`, `wasm`, `semantic-mutation` -- three,
not the four this section originally measured, since `fsl-logic`'s dedicated key was retired when
that job went restore-only. That is not the complete list of keys any workflow actually saves:
`ci.yml`'s own `rust-native-z3` job also saves, on non-pull-request events, one key per platform
(`Windows_NT` and `Darwin`) -- it just is not one of `CI_SHARED_KEYS`' three named identities. Its
presence on `main` is required separately, by `REQUIRED_MAIN_ENTRIES`'s per-`{key, platform}` check;
a leak of it onto a pull-request ref is caught separately too, by rule 4's generic `/^v\d+-rust-/`
detection rather than rule 3's name-based one.

`merge-readiness.yml` was deliberately left unchanged at that point: its two keys total about
131 MiB, they were not *that* incident's pressure, and its lanes are the sub-minute fast path —
making them cold would defeat the reason that workflow exists. That reasoning held for concurrent
pressure from a handful of pull requests at once; it did not hold over time. `merge-readiness.yml`
runs on every pull request and had no `save-if` guard, so every pull request saved its own
ref-scoped copy of both keys. By 2026-08-11 this had accumulated to 15 pull-request refs (#743-#792,
all closed or merged by measurement time) holding 29 cache entries — about 1.90 GiB — entirely on
the merge-readiness keys, on top of pull-request-scoped residue that predated PR #752's
`ci.yml`-only fix, for **38 entries / 10.03 GiB** total against the 10 GiB budget. None of these
refs were being re-run any longer to refresh their own last-accessed time; they persisted simply
because GitHub only evicts a cache for staleness after **7 days without a read**, and measurement
landed inside that window for refs that closed more recently than that. The LRU evictor does not
distinguish "small but many" from "few but large": it evicted `refs/heads/main`'s
`native Z3 4.16 (windows-latest)` cache down to **zero entries**, observed via
`gh api actions/caches`. **The `save-if` guard `ci.yml` uses is the wrong fix here and was
reverted before merging**: that guard only stops saving on `pull_request` events, and it relies on
some other event on the same workflow saving a `main`-branch copy to restore from. `ci.yml` has a
`push: branches: [main]` trigger to do that; `merge-readiness.yml` has none (`on:` above lists only
`pull_request` and `merge_group`), so adding the same guard here would not close the eviction path,
it would make every pull request permanently cold — the workflow would never save a cache under its
own key, from any event. `merge-readiness.yml`'s two `Swatinem/rust-cache` steps instead go
**restore-only against `ci.yml`'s own `rust-workspace` key** (`shared-key: rust-workspace`,
`save-if: false`): both jobs run `dtolnay/rust-toolchain@stable` on `ubuntu-latest` against the same
checkout as `ci.yml`'s `rust workspace` job, and `Swatinem/rust-cache` derives its key from the
rustc version, `CARGO`/`CC`/`CFLAGS`/`CXX`/`CMAKE`/`RUST`-prefixed env vars, and the workspace
lockfiles — none of which differ between the two workflows. That mechanism-derived expectation is now
supported by direct pull-request log evidence: on run `31581715093` attempt 1, `merge readiness / core
contracts` logged `Restored from cache key
"v0-rust-rust-workspace-Linux-x64-e8b3ee54-09fbaf53" full match: true.` at 09:11:00.77Z and
`merge readiness / Rust compile` logged the same full match at 09:11:04.76Z; on run `31583381471` attempt 1,
the same two jobs logged it at 09:33:03.06Z and 09:33:05.55Z, respectively. These four restore logs,
rather than the derivation mechanism, establish the observed full-key match. A future lockfile change
could still produce `Swatinem/rust-cache`'s ordinary prefix-restore fallback rather than a cold build,
but that is not what these runs observed. Neither job ever writes to this key: save-enabled `ci.yml`
steps write on non-`pull_request` events, so no `pull_request` event can grow it, and
merge-readiness gains no eviction pressure of its own to reintroduce.

This also qualifies a claim made elsewhere in this document. Cache hit rates were measured to have
no headroom for `rust workspace` — compile is only ~3.5 min of ~33 min **on a warm cache**. That
remains true warm, and it is exactly the premise that fails under concurrency: the question is not
how much a better hit rate buys, but whether a hit happens at all.

**`semantic-mutation`'s measured 2.719 GiB is an observed clean size, not accumulated dead weight --
two successive size predictions to the contrary were both wrong, and this document previously
carried both.** Direct measurement settled it: on product-gate run `31210570118` attempt 1, job `92972117510`
(`mutation operators (3/3)`) restored with `No cache found.` at 19:14:17Z -- a fully cold start, so it
carried forward nothing from any prior run -- built for ~35 minutes, and saved the cache at 19:48:12Z.
`gh api actions/caches` shows the resulting entry, `v0-rust-semantic-mutation-Linux-x64-...`, created
at `2026-08-07T19:48:56.674225000Z` and sized 2,919,716,751 bytes (2.719 GiB). That operators job did not run the
mutants lane; the same run's successful mutants job `92972117519` ran on a different runner, so it
could not contribute to this operators job's cache archive. Thus neither the mutants scratch build
nor its evidence generation contributed a byte to this entry: there is no dead weight here to
recover, and both the ~0.9-1.4 GiB and ~2.2 GiB sizes this document previously predicted
(attributing the size to, respectively, an accumulating scratch-build tree and to
`fault-operators`' persistent build tree as a designed minimum size) do not describe what this entry
actually is. This one cold-start save on one shard is evidence of what this key *can* legitimately
hold, not a proven minimum across every shard and every future revision -- `semantic-mutation` is
not touched by this fix and is not a lever in the budget arithmetic below.

**The scratch-build-tree and evidence-path changes in this branch are a closed-ingress-path fix, not
a size fix, and are kept for that reason alone.** `tools/run-semantic-mutation-gate.sh`'s mutants
lane's scratch `CARGO_TARGET_DIR` (isolated per run so a stale mutant artifact's timestamp can never
look newer than a freshly copied baseline checkout) used to be created under
`rust/target/semantic-mutation-build/run.XXXXXX`, inside the tree `Swatinem/rust-cache` saves
wholesale, and now lives under `${RUNNER_TEMP:-${TMPDIR:-/tmp}}` instead. The script also clears any
`rust/target/semantic-mutation.*`/`semantic-mutation-build` a restored cache may carry forward,
unconditionally and before either lane does anything -- an earlier version of this cleanup ran only
in the mutants lane's own path, below `run_operators_lane`'s early `exit 0` for `--lane operators`
invocations, so the currently save-enabled operators lane (see below) never ran it. Both changes
close a path by which a *future* run could let leftover scratch/evidence ride into a save; neither
changes what the entry, as actually observed, already contains.

**Ownership: `save-if: false` now makes the operators shards this key's only configured saver, and
the mutants job restore-only.** This is a property created by the configuration change, not a
historical inevitability. The decisive counterexample is run `31086907528`, attempt 1: mutants job
`92568586155` restored `No cache found.`, then its successful `Post Run
Swatinem/rust-cache@v2` step uploaded **2,922,378,363 B**; all three
`semantic-mutation-operators` shards were cancelled and their post steps skipped. That observed
save occurred before the current `save-if: false` guard, so it disproves a claim that an operators
shard always won the historical race. The guard instead closes the mutants save path now, ensuring
that future timing cannot make its `rust/target`-scoped evidence directory (kept under `rust/target`
solely because the artifact-upload glob requires it) enter this shared cache.
`semantic-mutation-operators` remains save-enabled on non-pull-request events with
`cache-on-failure: true`, and is therefore the sole configured owner and recovery path.

**Budget lever: `fsl-logic` goes restore-only against `rust-workspace`, taking its dedicated key out
of the budget entirely (measured, not predicted).** `fsl-logic`'s entire build is
`cargo test -p fslc-rust --test typed_agreement --locked` (`tools/run-fsl-logic-test.sh:19-22`), one
test target that is a strict subset of what `rust workspace`'s shards already build.
`Swatinem/rust-cache` prunes workspace-member build artifacts at save time and keeps only external
dependency output (upstream `src/cleanup.ts`), so every lane's cache is substantively "the same
lockfile's external deps" regardless of which job saved it -- restoring `rust-workspace` here loses
nothing `fsl-logic` needed a dedicated key for. The main-branch entry listing on 2026-08-12, before
the deletion, measured `rust-workspace` at 1,605,761,517 B, `fsl-logic` at 1,470,489,603 B
(1.369 GiB), `wasm` at 1,452,450,563 B, `rust-native-z3` Darwin at 1,239,235,056 B, and
`semantic-mutation` at 2,919,716,751 B, plus ~41 MB of small tool-binary caches -- **8.130 GiB
total**. After the human-authorized deletion of the now-orphaned `fsl-logic` entry and the observed
recreation of the Windows native-z3 entry, the cache listing measured **7.469 GiB**, below the
8.500 GiB warning threshold, after all six audit failures and before the PR-cache deletion. That
post-failure capacity observation was not itself audited. The six failed
audits have three observed finding types across two periods, not one continuous single cause: scheduled runs
`31239888526` attempt 1 (2026-08-08T04:37Z), `31295386890` attempt 1 (2026-08-09T04:49Z),
`31357678690` attempt 1 (2026-08-10T05:09Z), and `31459843075` attempt 1 (2026-08-11T04:52Z) each reported only
`budget-exhausted` (respectively 8.94 / 10.00 GiB, 10.13 / 10.00 GiB, 10.03 / 10.00 GiB, and
10.03 / 10.00 GiB). The final two failures were run `31565897238` attempt 1 (a **push**, 2026-08-12T05:14Z)
and scheduled run `31566055925` attempt 1 (2026-08-12T05:17Z); each reported three findings:
`main-cache-absent` for `rust-native-z3`, plus two `pull-request-rust-cache-present` findings for
`refs/pull/793/merge`, the 0.08 GiB
`v0-rust-rust-compile-Linux-x64-e8b3ee54-09fbaf53` and 0.05 GiB
`v0-rust-core-contracts-Linux-x64-e8b3ee54-09fbaf53`. The observed recreation of the Windows
native-z3 entry at 2026-08-12T06:16:19.271024Z resolved the former finding. Those orphaned PR entries were
saved before #793 branched from a base containing the restore-only correction; after human
authorization on 2026-08-13, their deletion resolved the two remaining findings. The listing then
measured **7.337 GiB**, and workflow-dispatch audit run `31654305398` attempt 1 at 2026-08-13T00:24Z
succeeded. The restore-only `FSL Logic Test` recorded two shared-key full matches: first, the
scheduled job in push run `31565897267` attempt 1 at 2026-08-12T05:14:43Z (cache hit) and 05:15:05Z
(`full match: true`), completing in **2m54s**; then run `31570480618` attempt 1 at 06:34:15Z and 06:34:36Z,
completing in **3m02s**. Both restored
`v0-rust-rust-workspace-Linux-x64-e8b3ee54-09fbaf53`, compared with 2m48s / 2m53s / 2m51s for
warm runs with its former dedicated key on 2026-08-09/10/11. The cache logs, rather than duration,
establish the observed full matches; duration alone does not establish a cold-build duration. If the shared
`rust-workspace` cache were ever missing or evicted instead, this job would build cold under the
existing 30-minute timeout. That timeout bounds resource consumption, not job success: past it, the
job is killed and reported as a failed required context, the same outcome this whole section exists to
stop happening to a different job. Whether a cold build here would finish inside 30 minutes is not
observed -- this job has never run cold under this configuration, so its baseline warm duration
describes only the typical cached case, not a cold-run bound, and is not evidence that a cold run
would succeed rather than time out. A `cache-targets: false` lever on the
mutation lanes was considered and rejected: it would cost the operators shards a cold build every
run (~35 min measured cold vs. ~5 min warm) for less budget relief than `fsl-logic` gives for
near-zero cost.

**Historical cold-run observations and the combined #752 change.** `gh api` reports
`run_attempt=1` for `main` run `31086789147`, PR #743 run `31077948474`, and PR #744 run
`31083643650`; each recorded cold `mutation operators` shard(s) cancelled at the old 30-minute
budget with `Post Run Swatinem/rust-cache@v2` `skipped`. On `main`, shard (1/3) ran 30m19s and
`mutation mutants` also ran 60m24s before cancellation; both post steps were `skipped`. The other
two operators shards in that same `main` run succeeded and their post steps succeeded, providing an
in-run contrast. PR #743 and #744 each recorded all three operators shards with the
cold-cancelled/skipped shape. PR #745 run `31086907528` has `run_attempt=3`; its **attempt 3**
recorded that shape only for operators shard (2/3). Its earlier attempts are distinct observations:
attempt 1 had all three operators shards cancelled with skipped post steps (while its mutants job
saved the counterexample cache recorded above), and attempt 2 had shard (2/3) fail with a skipped
post step while shards (1/3) and (3/3) succeeded. Commit `877fe8c` (#752) changed two levers in the
same changeset: it set `cache-on-failure: true` on both then-saving semantic-mutation cache steps
and raised the corresponding budgets from 30 to 50 minutes and from 60 to 90 minutes:

| job | budget before | warm | cold | budget now |
|---|---|---|---|---|
| `mutation operators (K/3)` | 30 min | 18.0–19.5 min | **>30** (cancelled at 30.2) | **50 min** |
| `mutation mutants` | 60 min | 17.2–34.2 min | **>60** (cancelled at ~61) | **90 min** |

`Swatinem/rust-cache`'s `action.yml` describes `cache-on-failure` as a non-success post-save path:
`post-if: success() || env.CACHE_ON_FAILURE == 'true'`. That is mechanism evidence, not an
observed semantic-mutation timeout-cancelled save. Since #752 paired the flag with both timeout
increases, subsequent outcomes cannot isolate either change's contribution.

With the current configuration, the `semantic-mutation` key has exactly one class of savers: the
three `semantic-mutation-operators` shards (`save-if` on non-pull-request events,
`cache-on-failure: true` -- the owner keeps the recovery path for a cold start that fails or times
out). `semantic-mutation-mutants` is restore-only (`save-if: false`); it carries no
`cache-on-failure`, which would be meaningless there, since `Swatinem/rust-cache`'s save step exits
on `save-if` before that flag is ever consulted. Both budgets are raised past a measured cold run
regardless, for the reason this document already gives for the promotion-only native-Z3 job: a gate
that runs out of wall clock reports a failure it did not observe. (`rust-native-z3`'s own
`cache-on-failure: true` and 60-minute budget, discussed elsewhere in this section, are a separate
lane and a separate decision.)

The combined change's job-level recovery is now observed, not an expectation: run `31097824729` attempt 1
recorded successful recoveries for #721 (`mutation mutants`) and #678 (`semantic mutation
(complete)`). The post-merge reporter recorded no later #721 occurrence. It did record later #678
occurrences as `failure` on runs `31101190847` attempt 1 and `31135753393` attempt 1, rather than `cancelled`, before
recording a later recovery on `31237852598` attempt 1. Thus the old `cancelled` conclusion for these jobs did
not recur in that evidence; this does not establish that the combined change fixed the distinct
`failure` conclusion, nor can it separate the flag's contribution from the simultaneous timeout
increases.

The later run `31097824729` attempt 1 saved its semantic-mutation cache only after the job reached `success`,
using `post-if`'s ordinary `success()` branch. No observed semantic-mutation run has a cache written
specifically following a timeout-driven cancellation.

**The same closed loop appeared independently in `rust-native-z3`'s `windows-latest`/`macos-15`
matrix. Windows recovery is now observed; macOS recovery is not established by this observation.**
That job's `Swatinem/rust-cache` step carried `save-if` but not `cache-on-failure`. Warm Windows runs
measured 32m56s / 32m45s (2026-08-04) and 31m49s / 29m44s / 26m56s (2026-08-05), comfortably inside
the former 40-minute budget; six scheduled `windows-latest` observations from 2026-08-07 00:23
through 2026-08-11 timed out at the configured 40-minute limit. They span four revisions and test
volumes:

| revision | observed scheduled run(s) |
| --- | --- |
| `0590975` | `31133932323` |
| `bcb0a4d` | `31210570118` |
| `1050a76` | `31273202771` |
| `60705fd` | `31330456273`, `31423201873`, `31527197290` |

Each observed run had a cancelled Windows job with its `Post Run Swatinem/rust-cache@v2` step
skipped. Cache absence is an observed material correlate across those observations, not evidence
that one self-perpetuating deadlock or any single change caused every timeout. The last successful
run (2026-08-05) restored the cache
(`Cache hit for: v0-rust-rust-native-z3-Windows_NT-x64-...`, 591 MiB); the 2026-08-11 cancellation
reported `No cache found.` instead, and its job steps show
`9 skipped Post Run Swatinem/rust-cache@v2` (run `31527197290` attempt 1).

The post-merge Windows runs directly observe the recovery path. In run `31565897267` attempt 1, job
`94017507983` (`native Z3 4.16 (windows-latest)`) was cancelled while testing the pinned native-solver
and BMC crates, but its `Post Run Swatinem/rust-cache@v2` step succeeded and created
`v0-rust-rust-native-z3-Windows_NT-x64-af4551b0-09fbaf53` at 619,429,238 B. This is a direct comparison
with run `31527197290` attempt 1's cancelled Windows job, whose corresponding post step was skipped: under the
same timeout-cancelled condition, the corrected run's `cache-on-failure: true` post step ran. This
comparison does not isolate that flag as the cause: the same changeset also raised this job's timeout
from 40 to 60 minutes. The corrected run took 61m46s (05:14:37–06:16:23), so a 60-minute budget did
**not** make this cold vendored-Z3 build complete; a successful cold-build duration on the current
tree and test volume remains unobserved. That scope does not erase an earlier observation: run
`30156566539`, attempt 1, job `89675360913` was a cold Windows success on 2026-07-25, logging `No
cache found.` at 11:38:31.77Z, completing in 37m08s (11:38:01–12:15:09), and saving a
602,055,210-byte cache in its successful post step. That success predates the 2026-08-12
cancellation and used a different tree and test volume, so it does not show that the current tree
can complete cold within 60 minutes. The saved entry made the next Windows run warm: run
`31570480618` attempt 1 completed the job in 34m39s (06:34:04–07:08:43), and the run concluded successfully.
The direct scheduled recovery record is run `31632094255` attempt 1 (`event: schedule`): its
`native Z3 4.16 (windows-latest)` job and its `product gate` job both concluded `success`.
These runs observe a recovery path after the combined change: a timeout-cancelled run saved a cache
and the subsequent warm run completed. They do not attribute that save to `cache-on-failure` alone
or make a 60-minute cold build pass. The 60-minute budget remains a resource bound, while the saved
cache provides the recovery path.

**The control.** `.github/scripts/audit-cache-budget.mjs` is a pure function over a fetched cache
listing; `.github/workflows/cache-budget-audit.yml` fetches and runs it on a schedule, on dispatch,
and on `main` pushes that touch it, `ci.yml`, or `merge-readiness.yml`. The runner first obtains
usage **bytes** only. GitHub documents the usage count as refreshed approximately every five
minutes, so `active_caches_count` is deliberately not a completeness condition; when supplied, it
is nevertheless required to be a nonnegative safe integer so a malformed usage envelope cannot be
treated as valid. The pure audit
continues to use `max(usage bytes, summed listing bytes)`, so either observed byte total at or above
85% rejects.

For completeness, each stability attempt obtains two entire cache listings with fixed
`per_page=100&sort=created_at&direction=asc`. `created_at` does not change on restore/access, but
GitHub documents only that primary sort key, not a secondary tie order or page-to-page stability.
The current repeated live observation had nine distinct `created_at` values; it is not evidence
about tied values. A tie reordered across a page boundary can produce a duplicate ID, rejected
immediately, or different paired observations, which retry once and fail closed if still different;
an identical mixed state can repeat as an undetectable residual. A collection requires a
safe-integer first `total_count`; each declared page repeats it, has
its exact implied entry count, and contains unique safe-integer IDs with nonempty `key` and `ref`
and nonnegative safe-integer `size_in_bytes`. It then requests an empty sentinel. GitHub's observed
out-of-range envelope is `{ "total_count": 0, "actions_caches": [] }`; the sentinel may therefore
have count zero or repeat the initial count, but any other count, a non-array envelope, malformed
count, or nonempty array fails closed. The two collections must have exactly the same ID set and,
for every ID, the same `key`, `ref`, and `size_in_bytes`. A disagreement waits exactly one second
before repeating the complete pair once; a second disagreement exits `api-unreadable` rather than
reporting health. That second is bounded pacing between requests: it creates a later observation
without claiming a fresh or independent backend snapshot. A five-minute wait would add audit delay
without acquiring an atomicity or freshness guarantee GitHub does not document.

This detects a page-boundary replacement or count-preserving mixed collection whenever it makes the
two full observations differ, plus malformed/internally inconsistent envelopes and an undercount
exposed by its sentinel. It is not an atomic GitHub snapshot: two full collections can still return
the same mixed state, or the repository can mutate only before/between/after the paired reads in a
way that leaves their compared IDs and fields equal. The audit makes no claim to detect those
residuals; that would require a server-provided snapshot token or equivalent API guarantee.

The internal request budget counts HTTP-successful requests: one usage request plus
`2 × (pages + sentinel)` for one paired observation; the one permitted retry raises the bounded
worst case to `1 + 4 × (pages + sentinel)`. The standard Actions `GITHUB_TOKEN` allowance is 1,000
requests/hour/repository. The runner caps itself at 900 requests, reserves 100 for other workflow
work, and aborts at the first HTTP failure (which therefore cannot permit a later request). It rejects
a missing/empty/non-integer `x-ratelimit-remaining` header before numeric conversion,
and stops before each request when the known remaining bucket is at or below that headroom. After a
page-one response, it reserves the current collection's remaining pages and sentinel plus the
headroom from its current control state; it does not subtract cumulative requests from a later
smaller collection's bound and thereby manufacture negative future work. With 100-entry pages, at
most 22,300 entries (223 pages) fit: the worst case is `1 + 4 × 224 = 897`; 22,301 entries require
901 and are rejected. Counts 99,800–99,901 are therefore rejected before a continuation request,
rather than approaching the repository quota.

Given a usable observation, the pure audit fails closed on four states: usage at or above 85% of
the limit, a missing `refs/heads/main` cache for any critical-path `{key, platform}` pair, — the
rejecting control for the `save-if` guard itself — any pull-request-scoped cache for one of
`ci.yml`'s shared keys, and — the general form of that same control — any `v0-rust-*`-prefixed
cache on a pull-request ref at all, whether or not its shared key is one `ci.yml` declares.

The critical-path check is per-`{key, platform}` pair, not per-key, because `rust-native-z3` is one
shared key backed by a `[macos-15, windows-latest]` matrix in `ci.yml`: a key-only set lets either
platform's presence on `main` hide the other's absence. That was a live blind spot, not a
hypothetical one -- the predecessor `sharedKeyOf`'s regex matched `Linux`/`macOS`/`Windows` (the GitHub Actions
`runner.os` spellings), but `Swatinem/rust-cache` composes its key from `os.type()`, which reports
`Linux`, `Darwin`, and `Windows_NT`. `Darwin` and `Windows_NT` never matched, so both `rust-native-z3`
entries were invisible to every rule in this audit -- including the one that would have reported
`main`'s Windows cache evicted to zero entries during the incident this section documents. The
current `entryIdentity`, which replaced `sharedKeyOf`, accepts only the observed `os.type()` spellings and
`REQUIRED_MAIN_ENTRIES` now requires `rust-native-z3` on both `Windows_NT` and `Darwin` explicitly,
and the independently saved `semantic-mutation` key on `Linux`; the latter is the configured
restore source for every PR's restore-only mutants lane.

The general pull-request-rust-cache rule exists because `merge-readiness.yml`'s restore-only fix
(above) makes "no workflow saves a rust cache on a pull-request event" a repository-wide invariant,
not something scoped to `CI_SHARED_KEYS`' three declared keys (`rust-workspace`, `wasm`,
`semantic-mutation`). Before this rule, `merge-readiness.yml`'s own
now-removed per-job keys (`rust-compile`, `core-contracts`) were invisible to this audit for the same
reason `rust-native-z3` was: an unlisted shared key is never attributed, so a pull-request-scoped
entry for it was silently treated as normal. The general rule catches that shape regardless of
whether the key is one this file has ever heard of, so a future workflow's unguarded
`Swatinem/rust-cache` step cannot reopen the same blind spot under a new name.

`.github/scripts/audit-cache-budget.test.mjs` calibrates all of it offline, including a fixture that
reproduces the 2026-08-06 listing verbatim and must fail; actual 200-entry page-boundary mixed
collections (deleted ID on page one, appended ID on page two, and a missing boundary ID); differing
mixed pairs that remain failing; tied-`created_at` boundary reordering; per-field identity changes;
every `CI_SHARED_KEYS` key missing from `main`; and direct live-wrapper controls for 22,300/22,301,
HTTP failure, malformed ordinary pages, the 900-request cap, and rate-limit headers/headroom.
There are also rejecting fixtures for a non-`ci.yml` rust cache on a pull-request ref and for each
half of `rust-native-z3` missing from `main`.
`tools/check-merge-readiness.sh`'s `check_automation` lane runs that suite on every pull request, so
a change to the checker is covered pre-merge even though the live audit deliberately is not a
required context: the shared cache state can change after a pull request's own checks pass, so
gating a merge on it would gate on something outside the change under review.

Issue #747 records the incident. Issue #720's Finding 2 — warming the fault-operator scratch build —
**adds** a cache and therefore depends on this budget holding first.

### Cache-budget audit failure reporting

`.github/workflows/cache-budget-audit.yml` deliberately remains an observer: it has only
`actions: read` and `contents: read`, including on `workflow_dispatch`, so it cannot turn a live
cache observation into repository mutation. `.github/workflows/cache-budget-audit-reporter.yml` is
the separate, least-privilege writer. It has exactly Actions/contents read plus issues write and
listens only for completed `cache budget audit` runs. Its job additionally requires the same
repository, the default branch, and one of `push`, `schedule`, or `workflow_dispatch`; it checks out
the default branch rather than the triggering SHA. The reporter workflow, trigger, job set, job, and
each step are checked as approved mappings: unknown keys fail closed, not merely known dangerous
fields. The job has no permission override and exactly the pinned checkout, pinned Node setup, and
reporter invocation steps, so a mutable action, alternate checkout source/path, runner shell, extra
job, or inserted pre-run shell step cannot alter the checked-out reporter before it receives the
write token. Its YAML loader also rejects duplicate keys while constructing every mapping, before a
last-key-wins parser could hide an unapproved value from those mapping checks.

The reporter maintains one canonical issue, identified by the stable hidden marker
`<!-- cache-budget-audit -->`. The currently reconciled failure has an occurrence marker keyed by
`run_id:run_attempt`; the initial failure creates the issue, a later failure records a recurrence,
a successful trusted audit closes the issue with a recovery comment, and a later failure reopens the
same issue. A human close does not change identity: the next distinct failure reopens the canonical
issue. Comment markers count only when authored by `github-actions[bot]`, so a user cannot suppress
the reporter's audit trail by posting a predictable marker.

Before mutation the reporter lists completed default-branch audit runs and chooses the greatest
`(run_number, run_attempt)` among trusted runs. It intentionally does not use completion timestamps:
run numbers establish workflow order and attempts establish re-run order, while completion delivery
can arrive out of order. A delayed event therefore reconciles current completed health rather than
reopening an issue for stale failure. This is deliberately **latest-health coalescing**, not a claim
that every completed attempt receives a durable individual comment: the reporter stores a cursor,
the cumulative number of skipped failing attempts observable through either the triggering
`workflow_run` event or the workflow-runs list endpoint, and up to 20 recent
`run_id:run_attempt` identities from those sources in its rolling recurrence summary. An earlier
superseded attempt is not recoverable when it is absent from the list endpoint and did not trigger a
reporter run. Thus a queued reporter that observes failures 41, 42, and 43 records 43 directly and
preserves 41/42 as coalesced evidence. `cancel-in-progress: false` does not promise that every
pending run executes—GitHub retains one pending concurrency-group run and may replace an older
pending run—so this bounded, observable evidence is required independently of the concurrency
setting.

The reporter paginates issues, runs, and comments rather than assuming page one is complete. This is
a scheduled operational path rather than a merge hot path, so the potentially unbounded API scan is
acceptable to preserve marker idempotency and all evidence the endpoint makes available. For valid
reporter-authored state, comment volume is bounded: it keeps at most 20 detailed recurrence comments,
then creates and updates one rolling recurrence/coalescing summary; it also updates one rolling
recovery summary, for at most 22 reporter-authored comments. Identical occurrence-summary duplicates
are consolidated. A missing, malformed, or unsafe cursor/count/identity field, or duplicate summaries
that disagree, instead fails reconciliation without mutation; it is not silently repaired or reset.
Summary integers must be canonical decimal safe integers; cursor and identity parts are positive,
identities are unique, and the count covers every retained identity. A cursor newer than currently
observable trusted health is rejected rather than moved backward. The visible count is cumulative
only for its summary interval: deleting the complete summary intentionally resets that interval, and
the next summary starts a new count rather than claiming to reconstruct deleted evidence. These rules
preserve the meaning of a retained cumulative count instead of presenting a false repaired value. The
audit's `push` paths include the reporter workflow, script, and tests, so a merged reporter change
runs the live audit immediately instead of waiting for the next schedule or manual dispatch.

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
