<!-- SPDX-License-Identifier: Apache-2.0 -->

# Changelog fragments: reduce shared-edit merge conflicts on `CHANGELOG.md` only

Status: Accepted (evaluation outcome of the #737 spike; implementation is a separate task)

## Decision

Adopt **checked-in changelog fragments for `CHANGELOG.md`'s `[Unreleased]` entries only**
(option C1 below), aggregated deterministically at release time. Fragmenting the contract
documents — `docs/LANGUAGE.md`, `docs/LANGUAGE.ja.md`, `skills/fsl/reference.md` — is
**rejected** (option C2): the replay measurement below shows it would resolve zero additional
conflicting branch pairs, which is the no-go condition #737 itself set, and it would put a
generated layer upstream of the required `site reference freshness` status check.

This document records the decision, its evidence, the fail-closed controls the implementation
must carry, the migration sites, and the rollback/reversal rule. It changes no behavior by
itself. The coupled-change contract in `AGENTS.md` is not weakened: every notable change still
records a changelog entry in the same pull request — only the *file shape* of an
`[Unreleased]` entry changes, from a shared-file insertion to a per-change fragment.

## Measured evidence

All figures were measured against `origin/main` at
`d72ac8d0f717b16914790fe270566a8a1c407803` with git 2.50.1. Issue #737's opening figures
(381/176/174/144) were stale by the time of measurement, not wrong; the re-measured values are
used throughout.

### Change pressure on the four aggregation files

`git log --no-merges --format=%H origin/main -- <file> | wc -l`:

| file | non-merge commits touching it |
| --- | --- |
| `CHANGELOG.md` | 405 |
| `docs/LANGUAGE.md` | 183 |
| `skills/fsl/reference.md` | 180 |
| `docs/LANGUAGE.ja.md` | 52 (exists since `7946ead9`, 2026-07-16) |

`docs/LANGUAGE.md` and `skills/fsl/reference.md` co-change in 150 commits;
`docs/LANGUAGE.ja.md` co-changes with `docs/LANGUAGE.md` in 50 of its 52. Of 600 non-merge
commits, 439 (73%) touch at least one of the four files and 194 touch two or more
(distribution over how many of the four: 1 → 245, 2 → 49, 3 → 103, 4 → 42). Per-branch, over
the 209 first-parent merges (`git diff --name-only <p1>...<p2>`): `CHANGELOG.md` 200/209
(96%), `skills/fsl/reference.md` 94 (45%), `docs/LANGUAGE.md` 91 (44%), `docs/LANGUAGE.ja.md`
39 (19%), implementation (`rust/`, `src/fslc`) 157 (75%).

`CHANGELOG.md` insertions are small and top-loaded: n=405, p50=9 lines, p90=23, max=391. The
`[Unreleased]` section is 276 lines of a 4,569-line file at the measurement revision.

### The replay measurement (load-bearing)

Method, re-runnable: enumerate the 209 first-parent merges on `origin/main`; derive each
merge's side-branch active interval from the committer dates of `git log <merge>^1..<merge>^2`;
keep pairs of side branches whose intervals overlap **and** where
`git merge-base --is-ancestor` is false in both directions (neither branch had seen the
other) — 22 genuinely concurrent pairs; run
`git merge-tree --write-tree --name-only <A^2> <B^2>` on each pair (exit 1 = conflict).

**16 of 22 pairs conflict.** Conflicting-file frequency across the 16: `CHANGELOG.md` 16/16
(100%), `docs/LANGUAGE.md` 10, `skills/fsl/reference.md` 10, `src/fslc/bmc.py` 10,
`src/fslc/cli.py` 10, `tests/test_self_examples.py` 10, `docs/LANGUAGE.ja.md` 0.

The structure of those 16 matters more than the totals:

- 13 of 16 belong to one early-Python-era cluster sharing a single sync merge (`ba27dd20f`)
  on one side. **Every** `docs/LANGUAGE.md` / `skills/fsl/reference.md` conflict lives in that
  cluster, and every one of them arrives together with an implementation conflict
  (`src/fslc/bmc.py` and friends). Pairs where the contract documents conflicted **on their
  own**: 0.
- The three current-era conflicts (2026-07-02; PR #82 against each of #78, #77, #76) have
  `CHANGELOG.md` as the **only** conflicting file, 3 of 3. Both sides of those pairs also
  edited `docs/LANGUAGE.md` and `skills/fsl/reference.md`, and those edits auto-merged.
- Mechanism, confirmed by reproducing the three-stage blobs with `git merge-file -p`:
  competing top-of-section insertions directly under `## [Unreleased]` — the exact pattern
  fragments eliminate.
- Counter-example: pair #82 × #79 edited `CHANGELOG.md`, `docs/LANGUAGE.md`,
  `skills/fsl/reference.md`, and `src/fslc/grammar.py` on both sides and did **not** conflict.
  Both-sides-touch does not imply conflict; insertion position does.
- One tallying artifact, disclosed: `docs/LANGUAGE.md` shows 10 conflicting pairs against 9 in
  the both-touch tally — a three-dot-diff base artifact against the sync merge. It does not
  affect any conclusion.

### Honest limitation: serial rebase hides the true denominator

Rebase rewrites committer time, so the 22-pair set understates real concurrency. Re-deriving
the intervals from **author** time yields 397 concurrent pairs, of which 384 have both sides
touching `CHANGELOG.md` (110 for `docs/LANGUAGE.md`). This repository's serial-rebase practice
absorbs those collisions as rebase work that never surfaces as a merge conflict, and how many
`CHANGELOG.md` collisions were actually resolved by hand during rebases is **not measurable
from history** — once a branch is rebased it contains the other branch as an ancestor and is
excluded from the concurrent set by construction. The 384-of-397 figure is therefore an
**upper-bound proxy for exposure, not an observation of pain**. The same suppression means the
current era shows only one visible conflict event (2026-07-02, 3 pairs) after the Rust
workspace isolation; post-Rust contract-document conflicts observed: 0, but under suppressed
concurrency.

## The three options, decided on the measurement

- **C0 — status quo.** Keeps the 100%-of-conflicts file exactly as it is. Every measured
  conflict class and the 96% per-branch touch rate persist; rejected because C1 removes the
  dominant class at a bounded, reversible cost.
- **C1 — CHANGELOG fragments only. Adopted.** Removes the only conflict class the replay
  actually demonstrates: the six pairs whose `CHANGELOG.md` conflict is the top-of-section
  insertion race go to zero (current-era rate 3/3 → 0/3), because concurrent changes add
  distinct files under `changelog.d/` instead of competing for the same lines. The residual
  10 conflicting pairs are implementation conflicts (`src/fslc/*.py` era); no documentation
  option removes those, and none should try.
- **C2 — fragment the contract documents too. Rejected on measurement, not taste.** In the
  replay, fragmenting `docs/LANGUAGE.md` / `docs/LANGUAGE.ja.md` / `skills/fsl/reference.md`
  resolves **zero additional pairs**: every contract-document conflict is confined to the
  early-Python cluster and co-occurs with an implementation conflict that fragments cannot
  fix, while all current-era contract-document edits auto-merged. #737's own acceptance
  criteria close the spike as no-go when the measurement does not show an advantage — this is
  that case. Independently, C2 would insert a generated aggregation layer upstream of
  `site reference freshness`, a **required** status check on the `main safety and CI` ruleset
  that regenerates `docs/intro/*.html` from `docs/LANGUAGE.md` / `docs/LANGUAGE.ja.md` on
  every pull request; the canonical sources feeding a blocking gate would themselves become
  build outputs.

## The C1 contract the implementation must satisfy

Fragment shape: one file per change, `changelog.d/<id>-<slug>.<section>.md`, where `<id>` is
the issue or pull-request number and `<section>` comes from a fixed ordered list of Keep a
Changelog section names. The aggregator must be **stdlib-only**, so its contract tests can run
in `merge readiness / automation contracts`, whose lane is deliberately stdlib-only with no
pytest and no third-party dependency (`tools/check-merge-readiness.sh`'s own comment;
reaffirmed in `docs/DESIGN-docs-site.md` D7's addendum).

### Four fail-closed negative controls

This repository does not accept a positive-only control (`AGENTS.md`: every conformance
anchor needs a negative control that rejects a known violation). Each of the four controls
below therefore ships with a **calibrated rejecting fixture** proving it detects the failure
it exists for, alongside its accepting fixture; a green accepting path alone establishes
nothing.

1. **Missing fragment.** A pull-request diff touching a product surface (`rust/`,
   `src/fslc/`, `specs/`, `docs/LANGUAGE*`) with no new file under `changelog.d/` must fail.
   Rejecting fixture: exactly such a synthetic diff → exit 1,
   `changelog-fragment-missing: <changed paths>`. Second net: aggregating an empty
   `changelog.d/` at release → exit 1, `no-fragments-to-aggregate`.
2. **Duplicate id.** Two fragments declaring the same `(id, section)` pair must fail; silent
   last-wins or silent concatenation is forbidden. Rejecting fixture: `691-a.added.md` and
   `691-b.added.md` present together → exit 1,
   `duplicate-fragment-id: 691 (691-a.added.md, 691-b.added.md)`.
3. **Nondeterministic order.** The aggregator must sort by (fixed section order, numeric id,
   filename bytes) and must never depend on directory enumeration order. Rejecting fixture:
   inject a shuffled or reversed enumeration; output must be byte-identical or the test fails
   with `aggregation-not-deterministic` — and the control itself is calibrated by running it
   against a deliberately readdir-ordered sham implementation, which it must reject. An
   idempotence snapshot (two consecutive runs, byte-identical output) is the accepting
   counterpart.
4. **Unaggregated at release, and post-migration direct edits.** A guard in
   `.github/workflows/release.yml` before its "Extract release notes" step: fragments still
   present under `changelog.d/` at tag time → exit 1, `stale-fragments-present: <files>`. The
   workflow's existing `test -s release-notes.md` stays as the second net. Continuously after
   migration, a pull-request diff adding a line under `## [Unreleased]` in `CHANGELOG.md`
   directly → exit 1, `changelog-direct-edit-forbidden`; the rejecting fixture is exactly
   such a diff. This last check is what keeps authority single (next section).

### Single authoritative source, preserved

One change fact lives in exactly one place at every instant: in its fragment from merge until
release, then in the versioned `CHANGELOG.md` section — and the fragment is deleted in the
**same commit** that aggregates it. Authority is handed over, never duplicated. Aggregated
version sections become ordinary reviewed history, not a regenerated artifact — the
no-hand-edits rule for generated files does not apply to them, and hand edits to
`[Unreleased]` are refused by control 4. The release-notes consumer (`release.yml`'s `awk`
over `## [<version>]` headings plus `test -s`) is never changed, which is what makes the
rollback below trivial.

## Migration: six named sites

1. New `tools/aggregate_changelog` (stdlib-only) plus executable tests for all four controls,
   accepting and rejecting fixtures both.
2. `tools/check-product-gate-scope.sh` — add a `changelog.d/` **directory prefix** to
   `is_exempt_path`. The current list matches `CLAUDE.md`/`AGENTS.md`/`CHANGELOG.md` as exact
   root filenames, not prefixes (`docs/DESIGN-ci.md`, "Agent-configuration exemption":
   "`CLAUDE.md.d/x` does not match"), so without this every fragment-only change would start
   all four heavy product-gate jobs. The script's `selftest` gains matching
   accepting/rejecting cases.
3. `.github/workflows/release.yml` — the control-4 aggregation guard before "Extract release
   notes". The `awk` consumer itself is unchanged.
4. `AGENTS.md` — the coupled-change wording: "add notable changes under `CHANGELOG.md`
   `[Unreleased]`" becomes "add a changelog fragment under `changelog.d/`". The obligation
   itself — a changelog entry moves with the feature, in the same pull request — is unchanged.
5. `docs/DESIGN-ci.md` — amend the "Agent-configuration exemption" decision for site 2.
   That document states growing the exempt list "is a contract change to this decision, not a
   script tweak" and requires naming every path that reads the new entry with its unfiltered
   or fail-loud coverage: for `changelog.d/` those readers are `release.yml` (fail-loud via
   control 4 and `test -s`) and the merge-readiness fragment checker of site 6 (unfiltered,
   runs pre-merge on every pull request).
6. `.github/workflows/merge-readiness.yml` — the pull-request-level missing-fragment checker
   (control 1), stdlib-only to respect the `automation contracts` lane's dependency contract.

The existing 4,569-line `CHANGELOG.md` needs no migration; released sections stay as history.
Only future `[Unreleased]` entries change shape.

## Rollback and reversal condition

Rollback: aggregate any remaining fragments into `[Unreleased]` once, delete `changelog.d/`,
revert the mechanism sites (1, 2, 3, 6), restore `AGENTS.md`'s direct-edit wording (4), and
amend this document and the `docs/DESIGN-ci.md` exemption entry (5) to record the no-go. No
history rewrite is involved, and release compatibility cannot break because the release-notes
consumer was never changed.

Reversal condition, turning this outcome back into no-go: if in practice (a) the
missing-fragment checker's false positives become routine, or (b) concurrent-branch merge
conflicts persist at the same rate through some other path so that only the fragment upkeep
cost remains, then revert as above and record #737's outcome as no-go.

## What C1 explicitly does not touch

`docs/LANGUAGE.md`, `docs/LANGUAGE.ja.md`, and `skills/fsl/reference.md` keep direct,
versioned, whole-file editing, and their coupled-change obligations in `AGENTS.md` are
unchanged. In particular, the **section-alignment enforcement between `docs/LANGUAGE.md` and
`docs/LANGUAGE.ja.md` is untouched**, verified at each enforcement point:

- `tools/build_site_reference.py` (`render_language_tree`) raises `SystemExit` when a
  `docs/LANGUAGE.md` `## ` heading has no `SECTION_BLURBS` entry, and again when the two
  language files' `## ` section counts differ (`docs/DESIGN-docs-site.md` D7).
- `tests/test_site_reference_snapshot.py` re-runs the generator in memory and byte-compares
  the committed `docs/intro/{language,cli}.{ja,en}.html`.
- `.github/workflows/site-reference-freshness.yml` runs that test on every pull request with
  no path filter, and its context `site reference freshness` **is a required status check**
  on the `main safety and CI` ruleset (`.github/ruleset-contract.json`; `docs/DESIGN-ci.md`,
  "Required pre-merge contexts, and why the merge queue was rejected"), so a stale or
  misaligned page blocks the merge.

None of these read `CHANGELOG.md` or the future `changelog.d/`, and C1 writes to nothing they
read — that is *why* the enforcement is untouched, not merely an assertion that it is.
Relatedly, `docs/DESIGN-ci.md`'s rule that `skills/**` and `docs/**` must never join the
product-gate exemption list is unaffected; only `changelog.d/` joins it (site 2/5).

A pre-existing weakness in that enforcement was found during this evaluation and is tracked
separately as **#741**: the mechanical check enforces section *count* only, while D7 and the
generator's docstring claim count *and order* — a pure reorder of `docs/LANGUAGE.ja.md`
sections would pair Japanese bodies with the wrong English headings and still pass. That gap
predates this decision, is independent of it, and is not widened or narrowed by C1.

## Non-goals

- Building the aggregator, creating `changelog.d/`, or editing `AGENTS.md`'s wording in the
  same change as this record — implementation is #737's follow-up, gated on the four controls
  above.
- Deferring documentation updates out of feature pull requests, or making `CHANGELOG.md`
  optional (#737's stated non-goals).
- Treating generation as proof of paragraph-level semantic agreement between the two language
  files (#737's stated non-goal), or fixing #741 here.
