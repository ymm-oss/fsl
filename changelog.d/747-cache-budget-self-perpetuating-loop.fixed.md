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
`31581715093` logged `full match: true` for core contracts at 09:11:00.77Z
and Rust compile at 09:11:04.76Z; run `31583381471` logged the same result at
09:33:03.06Z and 09:33:05.55Z. Each identified
`v0-rust-rust-workspace-Linux-x64-e8b3ee54-09fbaf53`; the logs, rather than
the mechanism, establish the observed full-key match.
`tools/run-semantic-mutation-gate.sh`'s
mutants lane also moves its per-run scratch `CARGO_TARGET_DIR` out from under
`rust/target` (to `${RUNNER_TEMP:-${TMPDIR:-/tmp}}`), and the script now
clears any `rust/target/semantic-mutation.*`/`semantic-mutation-build` left by
a restored cache unconditionally, right after mode validation, before either
lane runs. That placement matters: `Swatinem/rust-cache` saves a key once and
whichever job reaches its post step first wins, and measured on product-gate
run `31527197290` all three `semantic-mutation-operators` shards reach their
post step tens of minutes before `semantic-mutation-mutants` does -- the
operators shards are this key's only actual saver. An earlier version of this
cleanup ran only in the mutants lane's own path, after the operators lane's
early exit, so the one lane that saves the key never ran it.
`semantic-mutation-mutants`'s `Swatinem/rust-cache` step is now
`save-if: false` (with `cache-on-failure` removed, since `save-if: false`
already makes it inert) to make that ownership explicit;
`semantic-mutation-operators` is unchanged. **This is a closed-ingress-path
fix, not a size fix**: measured directly (product-gate run `31210570118`, job
`92972117510`, `mutation operators (3/3)`), the `semantic-mutation` entry's
current 2.719 GiB was created by a *cold* operators run (`No cache found.` at
19:14:17Z, saved at 19:48:12Z) that never touched the mutants lane's scratch
build or evidence paths at all -- neither could have contributed to this
entry's size, so there was no dead weight to recover, and two earlier size
predictions in this fragment and in `docs/DESIGN-ci.md` (~0.9-1.4 GiB from an
assumed accumulating scratch tree, then ~2.2 GiB from
`rust/target/fault-operators`' deliberately persistent build tree treated as
a designed minimum size) were both wrong about what this entry actually is.
This one cold-start save is evidence of what this key can legitimately hold,
not a proven minimum across every shard and revision. `semantic-mutation` is
not resized by this change and is not touched by the budget lever below.
Separately, `rust-native-z3`'s
`windows-latest`/`macos-15` matrix had the same cancel-skips-save deadlock
already fixed for the `semantic-mutation` lanes: a cold build (measured warm
at 27–33 min) exceeded the 40-minute budget, the job was cancelled on all six
consecutive scheduled runs from 2026-08-07 through 2026-08-11
(`9 skipped Post Run Swatinem/rust-cache@v2`, run 31527197290), and the
skipped post step meant no cache was ever written to recover from. That step
now carries `cache-on-failure: true`. On run `31565897267`, the cancelled
Windows job's `Post Run Swatinem/rust-cache@v2` step succeeded and created
`v0-rust-rust-native-z3-Windows_NT-x64-af4551b0-09fbaf53` at
2026-08-12T06:16:19.271024Z (619,429,238 B), four seconds before the job
completed; this directly contrasts with run `31527197290`, whose cancelled
job logged `9 skipped Post Run Swatinem/rust-cache@v2`. This is an observed
cache save after timeout cancellation under the combined change, not isolated
proof that `cache-on-failure` alone caused it: that changeset also raised this
job's timeout from 40 to 60 minutes, and commit `877fe8c` (#752) likewise
coupled the flag with timeout increases for the semantic-mutation lanes.

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
never flagged for the same reason).

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
8.5 GiB warn threshold. The actual 7.469 GiB listing still failed six
scheduled audits through run `31566055925` (2026-08-12T05:17Z) because
`refs/pull/793/merge` retained two orphaned Rust entries; after human-authorized
deletion on 2026-08-13, the listing was 7.337 GiB and audit run `31654305398`
succeeded. `CI_SHARED_KEYS` and
`REQUIRED_MAIN_ENTRIES` both drop `fsl-logic` accordingly; the generic
pull-request-rust-cache rule covers any regression the same way it already
covers `merge-readiness.yml`'s former per-job keys. Separately, the shared-key
regex now parses from the tail of the cache key (anchored at the end) instead
of lazily from the head: a reviewer reproduced a case where a shared key
containing a platform-like substring earlier in its name (e.g. a hypothetical
`foo-Linux-bar`) misparsed into the wrong shared key, which caused a real,
present main-branch entry to be reported `main-cache-absent`. See
`docs/DESIGN-ci.md`, "Actions cache budget".
