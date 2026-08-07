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
`[Unreleased]` section body is 442 lines of a 4,735-line file at the measurement revision
(`git show d72ac8d:CHANGELOG.md`; the body count excludes the `## [Unreleased]` heading line).

### The replay measurement (load-bearing)

Method, re-runnable — every definition below is part of the procedure; varying the interval
end or the ancestor filter changes the pair count (both variations are quantified where they
are used):

- enumerate the 209 first-parent merges on `origin/main`;
- derive each merge's side-branch active interval as **[the earliest committer date among
  `git log <merge>^1..<merge>^2`, the merge commit's own committer date]** — the interval ends
  when the branch lands on `main`, not at its last side-branch commit (ending it at the last
  side-branch committer date shrinks rebased branches' intervals and yields 4 pairs, not 22);
- keep pairs of side branches whose intervals overlap **and** where
  `git merge-base --is-ancestor` is false in both directions between the two side tips
  (neither branch had seen the other) — 22 genuinely concurrent pairs;
- run `git merge-tree --write-tree --name-only <A^2> <B^2>` on each pair (exit 1 = conflict);
- where a both-sides-touch tally is cited, it intersects the per-branch touch sets
  `git diff --name-only <merge>^1...<merge>^2` over the pair.

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
- Touching is not conflicting, in the tallies as well as the counter-example: over the 22
  pairs the both-sides-touch counts are `CHANGELOG.md` 21, `docs/LANGUAGE.md` 16,
  `skills/fsl/reference.md` 16, `docs/LANGUAGE.ja.md` 0, against 16/10/10/0 conflicting.

### Honest limitation: serial rebase hides the true denominator

Rebase rewrites committer time, so the 22-pair set understates real concurrency. Re-deriving
the intervals from **author** time — same merge-date interval end — **and dropping the
ancestor filter** yields 397 concurrent pairs, of which 384 have both sides touching
`CHANGELOG.md` by the per-branch tally (110 for `docs/LANGUAGE.md`). Both deviations from the
committer-time procedure are deliberate and both are required to reach 397: a rebased branch
contains the other branch as an ancestor, so the filter must be dropped to see exactly the
concurrency rebasing hides — keeping it returns the same 22 pairs even under author time.
This repository's serial-rebase practice
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

Fragment shape: one file per change, `changelog.d/<id>-<slug>.<category>.md`, where `<id>` is
the issue or pull-request number.

**`<category>` must follow this repository's actual convention, not Keep a Changelog's.**
Measured at the baseline: `## [Unreleased]`'s 460-line body contains **zero `### ` subheadings**,
and `## [4.2.0]` contains zero as well. Categories are expressed as the **lead word of each
bullet** — `- Added (#707):`, `- Fixed (#713):`, `- Documented (#722):`, `- Decided (…):` — and
that vocabulary is already wider than Keep a Changelog's six, since `Documented` and `Decided`
are not among them. Two consequences the specification must honour:

- `<category>` is the bullet lead word, and its permitted set is **this repository's**, extracted
  from the existing `[Unreleased]` and released sections rather than imported. Forcing a mapping
  onto six Keep a Changelog names would be a visible content change, and a steady source of false
  rejections — exactly the condition reversal condition (a) treats as grounds for no-go.
- The aggregator emits bullets, **not `### ` groups**. Introducing subheadings would be a visible
  shape change to the file, which this decision does not authorize; "no behaviour change" has to
  mean the rendered section keeps the shape it has today.

The ordering key's first component is therefore a **declared order over that lead-word set**,
which must be written down in the aggregator next to the set itself, because it is repository
convention and not a standard anyone can look up.

The aggregator must be **stdlib-only**, so its contract tests can run in
`merge readiness / automation contracts`, whose lane is deliberately stdlib-only with no
pytest and no third-party dependency (`tools/check-merge-readiness.sh`'s own comment;
reaffirmed in `docs/DESIGN-docs-site.md` D7's addendum).

### Six fail-closed negative controls

This decision adopts, as its own rule for every control below, the requirement `AGENTS.md`
states for formal-to-implementation conformance anchors: an anchor must include a negative
control that rejects a known violation, because a green accepting path alone establishes
nothing. Each control therefore ships with a **calibrated rejecting fixture** proving it
detects the failure it exists for, alongside its accepting fixture.

1. **Missing or empty fragment.** A pull-request diff touching a product surface with no new
   file under `changelog.d/` must fail. The surface list is the coupled-change contract's
   list, not a subset of it: `rust/`, `src/fslc/`, `specs/`, `examples/`, `docs/LANGUAGE*`,
   and `skills/fsl/reference.md` (`AGENTS.md`, "A language feature moves with …";
   `CONTRIBUTING.md`, "Language or semantics"). Rejecting fixture: exactly such a synthetic
   diff → exit 1, `changelog-fragment-missing: <changed paths>`. A fragment must also carry
   content: a whitespace-only fragment body → exit 1, `changelog-fragment-empty: <file>`.
   The release-time net — aggregating an entirely empty `changelog.d/` → exit 1,
   `no-fragments-to-aggregate` — stays as a sanity floor only; it cannot see a single missing
   entry, which is control 5's job.
2. **Duplicate id.** The duplicate key is the pair **(numeric id, section)**, where the id is
   the decimal integer parsed from the fragment filename's leading digits — `0691-…` and
   `691-…` therefore declare the same id, and control 3's sort key uses the same parsed
   integer. Two fragments declaring the same (id, section) must fail; silent last-wins or
   silent concatenation is forbidden. One issue legitimately producing entries in two
   sections (`691-x.added.md` together with `691-y.fixed.md`) is the accepting fixture.
   Rejecting fixtures: `691-a.added.md` with `691-b.added.md`, and the zero-padded alias
   `0691-a.added.md` with `691-b.added.md` → exit 1,
   `duplicate-fragment-id: 691 added (0691-a.added.md, 691-b.added.md)` — naming the files as
   given, since the id shown is the folded value and the names are not — the diagnostic
   carries the same (id, section) key the rule is defined on.
3. **Nondeterministic or nonconforming order.** The aggregator must sort by (declared category
   order, numeric id, filename bytes) and must never depend on directory enumeration order.
   Rejecting fixture: inject a shuffled or reversed enumeration; output must be
   byte-identical or the test fails with `aggregation-not-deterministic` — and the control
   itself is calibrated by running it against a deliberately readdir-ordered sham
   implementation, which it must reject. An idempotence snapshot (two consecutive runs,
   byte-identical output) is the accepting counterpart. Determinism alone is not conformance:
   a deterministic but lexicographic enumeration (`sorted(os.listdir())`) passes both the
   shuffle and idempotence checks while ordering ids `10`, `100` before `9`. A fixture whose
   ids make numeric and byte order diverge — `9-…`, `10-…`, `100-…` in one category —
   therefore pins the aggregated output as a golden in numeric order 9, 10, 100; the
   lexicographic sham must fail that golden comparison (`aggregation-order-wrong`) and the
   conforming sort must match it byte-for-byte.

   **The same divergence exists in the first sort component and needs its own golden.** A fixture
   pinning only the id component leaves a sham that sorts categories lexicographically passing
   every check, because a lexicographic category order is deterministic and idempotent too. The
   declared order over this repository's lead words must therefore be pinned by a second golden
   whose fragments span two categories that lexicographic order would swap. Choose the pair from
   the declared order when it is written down; the point is that the fixture must **fail** for a
   sham that sorts the category names as strings. Without it, control 3 covers one of its three
   sort components and leaves the class of defect this control exists to catch — deterministic but
   nonconforming — open in the other two.
4. **Unaggregated at release, and post-migration direct edits.** A guard in
   `.github/workflows/release.yml` before its "Extract release notes" step: fragments still
   present under `changelog.d/` at tag time → exit 1, `stale-fragments-present: <files>`. The
   workflow's existing `test -s release-notes.md` stays as the second net. Continuously after
   migration, the merge-readiness checker (migration site 6 below) — the same pre-merge job
   that runs control 1 on every pull request — also rejects a pull-request diff that **adds or
   deletes** any line inside `CHANGELOG.md`'s `## [Unreleased]` body → exit 1,
   `changelog-direct-edit-forbidden`. Both directions are load-bearing: an added line
   duplicates authority, and a deleted line erases someone else's pending entry; the
   rejecting fixtures are one diff of each kind. This check is what keeps authority single
   (next section).

   **Two diffs must be excluded, or the check blocks its own rollout.** The migration pull
   request converts the existing `[Unreleased]` body — 442 lines at the measurement
   revision — into fragments, and the first post-migration release moves whatever remains
   under a version heading. Both are deletions from the `[Unreleased]` body and both would
   be rejected by the rule as stated. The exclusion must be explicit and narrow: a diff
   that empties the `[Unreleased]` body while adding the same content under `changelog.d/`
   or under a new `## [X.Y.Z]` heading, and nothing else. Anything broader reopens the
   direction this control exists to close. The `## [Unreleased]` heading line itself is
   outside the body and therefore unprotected; deleting or renaming it would unanchor every
   later check, so the rule must reject a diff that touches that line too.
5. **Silent drop at aggregation (conservation).** Controls 1–4 leave one direction open: an
   aggregation that deletes a fragment whose content never reaches the version section. At
   release `changelog.d/` ends up empty, so `stale-fragments-present` stays silent, and the
   remaining entries keep `test -s release-notes.md` green while one entry vanishes. Because
   the same-commit authority handover below is load-bearing, the handover itself must be
   checked: the aggregator verifies, before the aggregation commit is created, that every
   fragment it deletes reaches the version section it writes, and that the entry count equals
   the deleted-fragment count → otherwise exit 1, `fragment-dropped: <files>`.

   **Both predicates need a definition, or the control cannot be implemented.** "Reaches" means
   the fragment's body appears in the written section **in full and byte-for-byte**, modulo one
   declared normalization: the bullet marker and indentation the aggregator adds, and trailing
   whitespace. Anything looser lets a truncating aggregator through — one that copies only a
   multi-line fragment's first line contributes *something* from every fragment and keeps the
   counts equal, so a "contributes" test with no byte scope passes it. That truncating sham is
   itself a required rejecting fixture, alongside the dropping one.

   "Entry" means **one fragment, one bullet**: a fragment file produces exactly one top-level
   bullet in the aggregated section, whatever its internal structure. Without that, a fragment
   with three sub-bullets makes the count comparison ambiguous and the check unimplementable as
   written. It also means a fragment must not be authored as several independent entries — split
   them into separate files, which is what gives each its own id and keeps the conflict surface
   per-change.

   Rejecting fixtures, calibrating the check itself: a sham aggregator that silently drops one of
   three fragments must be rejected with `fragment-dropped` naming the dropped file; a sham that
   copies only each fragment's first line must be rejected too. The accepting fixture is the
   faithful aggregation of the same three, including one multi-line fragment.
6. **Nonconforming fragment name.** The only shape control the set otherwise lacks. A file under
   `changelog.d/` whose name has no leading digits, or whose `<category>` is outside the
   declared set, has no defined id and no defined sort position — controls 2 and 3 are both
   defined on quantities it does not have. It must be rejected at the earliest point that sees
   it, which is the same pre-merge job as control 1 → exit 1,
   `changelog-fragment-name-invalid: <file>`. Rejecting fixtures: `foo-bar.added.md` (no leading
   digits) and `691-x.chore.md` (category outside the declared set). Accepting fixture: a
   conforming name for each category in the declared set. Without this control both defects reach
   release and surface as `stale-fragments-present`, which fails loudly but late and names the
   wrong cause.

### Single authoritative source, preserved

One change fact lives in exactly one place at every instant: in its fragment from merge until
release, then in the versioned `CHANGELOG.md` section — and the fragment is deleted in the
**same commit** that aggregates it, a handover whose completeness is checked by control 5.
Authority is handed over, never duplicated. Aggregated version sections become ordinary
reviewed history, not a regenerated artifact — `AGENTS.md`'s no-hand-edits rule is scoped to
generated **compatibility snapshots** and never applied to them — and hand edits to
`[Unreleased]` are refused by control 4. The release-notes consumer (`release.yml`'s `awk`
over `## [<version>]` headings plus `test -s`) is never changed, which is what makes the
rollback below trivial.

## Migration: six named sites

1. New `tools/aggregate_changelog` (stdlib-only) plus executable tests for all six controls,
   accepting and rejecting fixtures both.
2. `tools/check-product-gate-scope.sh` — add a `changelog.d/` **directory prefix** to
   `is_exempt_path`. The current list matches `CLAUDE.md`/`AGENTS.md`/`CHANGELOG.md` as exact
   root filenames, not prefixes (`docs/DESIGN-ci.md`, "Agent-configuration exemption":
   "`CLAUDE.md.d/x` does not match"), so without this every fragment-only change would start
   all four heavy product-gate jobs. The script's `selftest` gains the matching pair:
   accepting `changelog.d/691-a.added.md` → exempt, and the prefix near-miss
   `changelog.dx/y` → **product**, mirroring the existing `CLAUDE.md.d/x` rejecting case.
3. `.github/workflows/release.yml` — the control-4 aggregation guard before "Extract release
   notes". The `awk` consumer itself is unchanged.
4. The surfaces that hard-code the `CHANGELOG.md` entry shape, all migrated in the same pull
   request. The obligation itself — a changelog entry moves with the feature, in the same
   pull request — is unchanged everywhere:
   - `AGENTS.md`, in **both** places: the commit convention ("add notable changes under
     `CHANGELOG.md` `[Unreleased]`") and the feature-moves list ("… a design note, and
     `CHANGELOG.md`") take the fragment wording.
   - `CONTRIBUTING.md`, the same two shapes: the "Language or semantics" guideline and the
     commits/pull-requests checklist.
   - `.claude/hooks/changelog_reminder.py` — `needs_reminder` matches `path ==
     "CHANGELOG.md"` exactly. Unmigrated, every fragment-only product change would trip its
     "product source changed but CHANGELOG.md did not" reminder permanently, manufacturing
     exactly the routine false positives that reversal condition (a) below treats as grounds
     for no-go; it must accept new files under `changelog.d/`.
   - `docs/RELEASE.md` — **step 7, "Move all current `[Unreleased]` entries under
     `## [X.Y.Z] - YYYY-MM-DD`, leaving an empty `## [Unreleased]`", is the manual procedure
     the aggregator replaces**, so it is the most consequential entry on this list; and
     release step "Review the non-empty `CHANGELOG.md` `[Unreleased]`
     section" becomes a review of the aggregated version section produced from the fragments.
   - `.claude/agents/fsl-coupled-change-reviewer.md` (a `CHANGELOG.md` dependency; it does
     not name `[Unreleased]`) and `.claude/skills/add-language-feature/SKILL.md` (which does)
     — both instruct a `CHANGELOG.md`
     `[Unreleased]` entry and take the fragment wording.
5. `docs/DESIGN-ci.md` — amend the "Agent-configuration exemption" decision for site 2.
   That document states growing the exempt list "is a contract change to this decision, not a
   script tweak" and requires naming every path that reads the new entry with its unfiltered
   or fail-loud coverage: for `changelog.d/` those readers are `release.yml` (fail-loud via
   control 4 and `test -s`) and the merge-readiness fragment checker of site 6 (unfiltered,
   runs pre-merge on every pull request).
6. `.github/workflows/merge-readiness.yml` — the pull-request-level checker for control 1
   (missing or empty fragment) and for control 4's direct-edit half, stdlib-only to respect
   the `automation contracts` lane's dependency contract.

The existing 4,735-line `CHANGELOG.md` needs no migration; released sections stay as history.
Only future `[Unreleased]` entries change shape.

## Rollback and reversal condition

Rollback: aggregate any remaining fragments into `[Unreleased]` once, delete `changelog.d/`,
revert the mechanism sites (1, 2, 3, 6), restore the direct-edit wording on every site-4
surface, and
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

- Building the aggregator, creating `changelog.d/`, or editing any of migration site 4's
  surfaces — `AGENTS.md`, `CONTRIBUTING.md`, `docs/RELEASE.md`,
  `.claude/hooks/changelog_reminder.py`, `.claude/agents/fsl-coupled-change-reviewer.md`,
  `.claude/skills/add-language-feature/SKILL.md` — in the same change as this record.
  Implementation is #737's follow-up, gated on the six controls above.
- Deferring documentation updates out of feature pull requests, or making `CHANGELOG.md`
  optional (#737's stated non-goals).
- Treating generation as proof of paragraph-level semantic agreement between the two language
  files (#737's stated non-goal), or fixing #741 here.
