<!-- SPDX-License-Identifier: Apache-2.0 -->

# Changelog fragments

This directory holds one file per notable change, aggregated into
`CHANGELOG.md`'s `[Unreleased]` section at release time by
`tools/aggregate_changelog.sh release`. It exists to remove the single
dominant merge-conflict class this repository's history measures on
`CHANGELOG.md`: concurrent pull requests inserting competing top-of-section
bullets. See `docs/DESIGN-changelog-fragments.md` for the accepted decision,
its evidence, and the six fail-closed controls this mechanism enforces.

This file itself is never treated as a fragment.

## Add a fragment for your change

A notable change (see `AGENTS.md` and `CONTRIBUTING.md` for what counts, and
`CONTRIBUTING.md`'s "Language or semantics" guideline for the language-change
case specifically) moves with a new file:

```
changelog.d/<id>-<slug>.<category>.md
```

- `<id>` is the issue or pull-request number. Leading zeros fold to the same
  id (`0691-x.added.md` and `691-y.added.md` both declare id 691), so do not
  zero-pad unless you mean that.
- `<slug>` is a short, lowercase, hyphenated description: one or more
  lowercase-letter/digit groups separated by single hyphens
  (`[a-z0-9]+(-[a-z0-9]+)*`) -- no uppercase, no doubled `--`, and no leading
  or trailing hyphen. It is not otherwise interpreted.
- `<category>` is one of the declared categories below.

**The file's content is exactly the text that would follow `- ` in the
rendered bullet** -- the aggregator does not invent or rephrase it, it only
prepends the Markdown list marker (`- ` on the first line, two spaces on
every continuation line) and sorts fragments into place. Write it the way it
should read in `CHANGELOG.md`:

```
Fixed (#691): `map` domain defaults now render as `{}` instead of the type
name placeholder. Native BMC, induction, and explicit-state agree; a
rejecting control confirms the old placeholder is not silently accepted.
```

One fragment is one top-level bullet, whatever its internal structure. If a
change has two genuinely separate notable effects, split it into two
fragments (they may share an id if they land in different categories, e.g.
`691-x.added.md` and `691-y.fixed.md`, but never the same (id, category)
pair).

**Fragments must live directly in this directory, not in a subdirectory.**
`changelog.d/sub/2-x.added.md` is rejected outright
(`changelog-fragment-path-invalid`): a nested fragment is invisible to
everything except a naive path check, so it would otherwise be silently
lost rather than aggregated (see `docs/DESIGN-changelog-fragments.md`,
control 6).

A fragment's body must also be renderable as-is: no CRLF line endings, and
the first line must not itself start with a Markdown list marker (`- `,
`* `, `+ `) or a heading marker (`#`), either of which would corrupt the
bullet the aggregator renders around it
(`changelog-fragment-hygiene-invalid`).

**Never hand-edit `CHANGELOG.md`'s `[Unreleased]` section.** Adding or
deleting a line there directly is rejected pre-merge
(`changelog-direct-edit-forbidden`): it either duplicates authority with a
pending fragment or erases someone else's. The existing `[Unreleased]` body
predating this mechanism is left as-is; the next release moves it under a
version heading in the same step that aggregates this directory's fragments
(`docs/RELEASE.md`, step 7) -- see "Migration note" below.

## Declared categories, in aggregation order

The category vocabulary is this repository's own -- the bullet lead word
already used in `CHANGELOG.md` -- not Keep a Changelog's six names. It was
measured from the `[Unreleased]` body at the time this mechanism was
introduced and is declared, in aggregation order, in
`tools/aggregate_changelog.sh`'s `DECLARED_CATEGORY_ORDER`:

1. `added` -- a new capability, tool, or surface.
2. `changed` -- existing behavior modified without being a defect fix
   (`fixed`), a mechanism swap (`replaced`), or an undo (`reverted`).
3. `fixed` -- a defect corrected.
4. `replaced` -- one mechanism substituted for another.
5. `reverted` -- a prior change undone.
6. `required` -- a check, gate, or piece of evidence made mandatory.
7. `exempted` -- a path or case carved out of a requirement, with its reason.
8. `unified` -- two previously-divergent paths merged into one.
9. `sharded` -- work split for parallelism without changing its scope.
10. `documented` -- a design record, decision, or non-behavioral write-up.
11. `decided` -- a design decision recorded (e.g. a `docs/DESIGN-*.md`
    go/no-go).

`changed` was added after this mechanism's introduction: it is measured 12
times across `CHANGELOG.md`'s full history (as a Keep-a-Changelog-style
`### Changed` subheading, predating this mechanism's bullet-lead-word
convention), more than seven of the other ten words -- see
`docs/DESIGN-changelog-fragments.md`, control 1's "Vocabulary correction",
for why excluding it was wrong and why `Removed` (1 occurrence) still is.

The order groups user-facing behavior changes first, then process/CI-shape
changes, then documentation/decision records last. It is arbitrary but
fixed, and is not derivable from the category names themselves -- that is
the point: sorting the names alphabetically would look plausible while
silently disagreeing with this list, which is exactly the defect class
control 3 in `docs/DESIGN-changelog-fragments.md` exists to catch.

**Growing this list is a contract change**, the same way growing
`tools/check-product-gate-scope.sh`'s exempt-path list is
(`docs/DESIGN-ci.md`, "Agent-configuration exemption"): open a pull request
that adds the new word to both `DECLARED_CATEGORY_ORDER` in
`tools/aggregate_changelog.sh` and this file, states where the word is
already used or clearly needed, and picks its position in the order. Do not
force your change into an existing category that does not fit merely to
avoid this step -- a category rejected by `changelog-fragment-name-invalid`
for a genuinely new, reasonable lead word is the routine false positive the
decision's reversal condition (a) treats as grounds for reverting this
mechanism entirely. Do not add a category you are not using yet on the
theory it might be needed (e.g. Keep a Changelog's `Removed`/`Deprecated`):
grow the list from real usage, the same way this one was measured.

## Migration note

The `[Unreleased]` body that existed in `CHANGELOG.md` before this mechanism
was introduced was deliberately **not** converted into fragments: of its
27 top-level bullets, 10 are in `- <Lead> (#NNN):` form; of the other 17,
3 carry an id outside that position and 14 carry no id of any kind. Those 14
alone have no id a fragment name could carry, so control 6 (nonconforming
fragment name) would reject every fragment the conversion tried to produce
for them. That body is left in place, untouched, and moves
under a version heading the ordinary way at the next release
(`docs/RELEASE.md`, step 7), in the same step that aggregates whatever has
accumulated in this directory by then. Only new entries route through
`changelog.d/`.

## Verification

`tools/aggregate_changelog.sh selftest` exercises every control's accepting
and rejecting fixtures. `tools/aggregate_changelog.sh check` validates the
fragments currently in this directory (name shape and duplicate ids) without
aggregating anything.
