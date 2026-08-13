Fixed (#747): `main`'s scheduled product gate is no longer cancelled by a
self-perpetuating cache-budget loop. PR #752 restricted `Swatinem/rust-cache`
saving in `ci.yml` to non-pull-request events so pull requests could not keep
evicting `main`'s caches, but `merge-readiness.yml`'s two `rust-cache` steps
(`rust-compile`, `core-contracts`) carried no such guard. Because that
workflow runs on every pull request, each one kept saving a fresh
ref-scoped copy of both keys. Measured on 2026-08-11: 15 pull-request refs
(#743-#792, all closed or merged by then) still held 29 entries (~1.90 GiB)
on those two keys alone, part of 38 entries / 10.03 GiB total against the
repository's 10 GiB budget -- retained not because anything kept restoring
them, but because GitHub only evicts an unused cache after 7 days without a
read, and measurement landed inside that window. The LRU evictor does not
distinguish "small but many" from "few but large": it had removed
`refs/heads/main`'s `native Z3 4.16 (windows-latest)` cache down to zero
entries. `merge-readiness.yml` has no `push` trigger, so the `ci.yml`-style
`save-if: ${{ github.event_name != 'pull_request' }}` guard would not close
this path -- it would leave every pull request permanently cold, since
nothing would ever save a `main` copy under this workflow's own key for it to
restore. Both `Swatinem/rust-cache` steps instead go restore-only against
`ci.yml`'s own `rust-workspace` key (`shared-key: rust-workspace`,
`save-if: false`). The shared toolchain, runner, and checkout predict an exact
derived-key match, and four direct pull-request restore logs confirm it: run
`31581715093` attempt 1 logged `full match: true` for core contracts at 09:11:00.77Z
and Rust compile at 09:11:04.76Z; run `31583381471` attempt 1 logged the same result at
09:33:03.06Z and 09:33:05.55Z. Each identified
`v0-rust-rust-workspace-Linux-x64-e8b3ee54-09fbaf53`; the logs, rather than
the mechanism, establish the observed full-key match.
`tools/run-semantic-mutation-gate.sh`'s
mutants lane also moves its per-run scratch `CARGO_TARGET_DIR` out from under
`rust/target` (to `${RUNNER_TEMP:-${TMPDIR:-/tmp}}`), and the script now
clears any `rust/target/semantic-mutation.*`/`semantic-mutation-build` left by
a restored cache unconditionally, right after mode validation, before either
lane runs. That placement matters because the current
`semantic-mutation-mutants` `save-if: false` actively makes the operators
shards the key's only configured saver; it does not merely record an
operators-only historical race. The counterexample is run `31086907528`,
attempt 1: mutants job `92568586155` restored `No cache found.` and its
successful post step uploaded 2,922,378,363 B while all three operators shards
were cancelled with their post steps skipped. An earlier version of this
cleanup ran only in the mutants lane's own path, after the operators lane's
early exit, so the then-save-enabled operators lane never ran it.
`semantic-mutation-mutants`'s `Swatinem/rust-cache` step is now
`save-if: false` (with `cache-on-failure` removed, since `save-if: false`
already makes it inert), closing its historically observed save path; the
operators shards remain the only save-enabled path. **This is a
closed-ingress-path fix, not a size fix**: measured directly (product-gate run `31210570118` attempt 1, job
`92972117510`, `mutation operators (3/3)`), the `semantic-mutation` entry's
current 2.719 GiB was created by a *cold* operators run (`No cache found.` at
19:14:17Z, saved at 19:48:12Z). That operators job did not run the mutants
lane; the same run's successful mutants job `92972117519` ran on a separate
runner and could not contribute to this cache archive. Neither the mutants
scratch build nor its evidence paths therefore contributed to this entry's
size, so there was no dead weight to recover, and two earlier size predictions
in this fragment and in `docs/DESIGN-ci.md` (~0.9-1.4 GiB from an assumed
accumulating scratch tree, then ~2.2 GiB from `rust/target/fault-operators`'
deliberately persistent build tree treated as a designed minimum size) were
both wrong about what this entry actually is.
This one cold-start save is evidence of what this key can legitimately hold,
not a proven minimum across every shard and revision. `semantic-mutation` is
not resized by this change and is not touched by the budget lever below.
Separately, the observed `rust-native-z3` **`windows-latest` leg** had the same
cancel-skips-save deadlock already fixed for the `semantic-mutation` lanes: a
cold build (measured warm at 27–33 min) exceeded the 40-minute budget, the job
was cancelled on all six consecutive scheduled runs from 2026-08-07 through 2026-08-11
(`9 skipped Post Run Swatinem/rust-cache@v2`, run 31527197290 attempt 1), and the
skipped post step meant no cache was ever written to recover from. That step
now carries `cache-on-failure: true`. On run `31565897267` attempt 1, the cancelled
Windows job's `Post Run Swatinem/rust-cache@v2` step succeeded and created
`v0-rust-rust-native-z3-Windows_NT-x64-af4551b0-09fbaf53` at
2026-08-12T06:16:19.271024Z (619,429,238 B), four seconds before the job
completed; this directly contrasts with run `31527197290` attempt 1, whose cancelled
job logged `9 skipped Post Run Swatinem/rust-cache@v2`. This is an observed
cache save after timeout cancellation under the combined change, not isolated
proof that `cache-on-failure` alone caused it: that changeset also raised this
job's timeout from 40 to 60 minutes, and commit `877fe8c` (#752) likewise
coupled the flag with timeout increases for the semantic-mutation lanes.
macOS recovery is not established by this Windows observation. The direct
scheduled recovery record is run `31632094255` attempt 1 (`event: schedule`):
both `native Z3 4.16 (windows-latest)` and `product gate` concluded `success`.

`.github/scripts/audit-cache-budget.mjs`'s `sharedKeyOf` regex matched the
GitHub Actions `runner.os` platform spellings (`Linux`/`macOS`/`Windows`), but
`Swatinem/rust-cache` derives its key from `os.type()`, which reports
`Linux`/`Darwin`/`Windows_NT` -- so `rust-native-z3`'s cache, on either
platform, was invisible to every rule in this audit, including the one that
should have reported `main`'s Windows entry evicted to zero during this
incident. The regex is corrected (matching `Windows_NT` before the `Windows`
it is a strict superset of), and the default-branch requirement is now
per-`{key, platform}` pair rather than per-key, so `rust-native-z3` must be
present on both `Windows_NT` and `Darwin` independently -- one platform's
cache can no longer hide the other's absence. A new general rule also flags
any `v0-rust-*`-prefixed cache on a pull-request ref regardless of whether its
shared key is one `ci.yml` declares, closing the same blind spot for any
future workflow's unguarded `Swatinem/rust-cache` step (and, retroactively,
for `merge-readiness.yml`'s own now-removed per-job keys, which this audit
never flagged for the same reason). Its runner treats usage as conservative
bytes-only evidence (`max(usage bytes, listing sum)`); GitHub refreshes its
count approximately every five minutes, so `active_caches_count` is not an
identity condition. It collects the full `created_at`-ascending listing twice,
including a validated empty sentinel each time, and requires the ID set plus
each ID's `key`, `ref`, and `size_in_bytes` to agree. One disagreement retries
the paired collection; a second fails closed. This detects page-boundary
mixing whenever it changes the two observed sets, but does not claim an atomic
snapshot: two collections could still receive the same mixed state. Sentinel
counts must be zero (the observed out-of-range envelope) or repeat the first
count, never an arbitrary valid integer. The retry-safe request bound counts
HTTP-successful usage plus listing requests: `1 + 4 × (pages + sentinel)`, capped at 900
to reserve 100 of the standard 1,000-request Actions-token quota, with
missing/empty/invalid `x-ratelimit-remaining` rejected before conversion and
headroom checked before every request and against the current collection's
remaining requests, never a later small collection's bound minus cumulative
requests. Malformed listing envelopes or usage bytes exit as `api-unreadable`;
absent usage bytes become non-PASS `usage-unobserved`. GitHub documents
`created_at` only as the primary sort key: the repeated live sample had unique
timestamps, so it establishes no tie order. A tied boundary reorder can make
the listing unauditable through a duplicate ID or different paired collection
and fail closed; an identical mixed collection can repeat and is not evidence
of a stable healthy collection.

An earlier revision of this change added an `entry-oversized` finding
(`SINGLE_ENTRY_WARN_BYTES = 2.5 GiB`) calibrated against the wrong ~2.2 GiB
size above; since the observed clean size is 2.719 GiB, that control would
have fired on a healthy `semantic-mutation` entry and has been removed rather than
recalibrated, since no measured defect signature exists to calibrate it
against (raising the threshold to paper over this would only be guessing at
an unmeasured defect). The actual budget lever is `ci.yml`'s `fsl-logic` job,
which now goes restore-only against `rust-workspace`
(`shared-key: rust-workspace`, `save-if: false`) instead of saving its own
key: its entire build (`cargo test -p fslc-rust --test typed_agreement
--locked`) is a strict subset of what `rust workspace` already builds, and
`Swatinem/rust-cache` prunes workspace-member artifacts at save time regardless
of which job saves, so every lane's cache is substantively the same external
dependency set. Measured main-branch entries (2026-08-12): `rust-workspace`
1,605,761,517 B, `fsl-logic` 1,470,489,603 B, `wasm` 1,452,450,563 B,
`rust-native-z3` Darwin 1,239,235,056 B, `semantic-mutation` 2,919,716,751 B,
plus ~41 MB of tool-binary caches -- 8.130 GiB total. Deleting the now-orphaned
`fsl-logic` entry (separate, human-authorized) and re-adding Windows
native-z3 (historical 0.577 GiB) estimated 7.338 GiB (73.4%), under the
8.5 GiB warn threshold. The 7.469 GiB listing was measured only after the
six audit failures and was never itself audited. Four scheduled audits
(`31239888526`, `31295386890`, `31357678690`, and `31459843075`, each attempt 1,
2026-08-08 through 2026-08-11) reported only `budget-exhausted`; the two
2026-08-12 failures (`31565897238` attempt 1, push, and `31566055925` attempt 1,
schedule) reported both two orphaned `refs/pull/793/merge` Rust
entries and `main-cache-absent` for `rust-native-z3`. Recreating the Windows
entry at 2026-08-12T06:16:19Z resolved the latter; human-authorized deletion of
the two PR entries on 2026-08-13 resolved the remaining findings. The listing
was then 7.337 GiB and audit run `31654305398` attempt 1 succeeded. `CI_SHARED_KEYS` and
`REQUIRED_MAIN_ENTRIES` both drop `fsl-logic` accordingly, while the latter now explicitly
requires the independently saved Linux `semantic-mutation` entry; the generic
pull-request-rust-cache rule covers any regression the same way it already
covers `merge-readiness.yml`'s former per-job keys. Separately, the shared-key
regex now parses from the tail of the cache key (anchored at the end) instead
of lazily from the head: a reviewer reproduced a case where a shared key
containing a platform-like substring earlier in its name (e.g. a hypothetical
`foo-Linux-bar`) misparsed into the wrong shared key, which caused a real,
present main-branch entry to be reported `main-cache-absent`. See
`docs/DESIGN-ci.md`, "Actions cache budget".
