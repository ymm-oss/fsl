# MyST over `docs/` — spike stage 1, and the decision not to adopt

Status: **spike record, stage 1 of issue #1126. Recommendation: do not adopt
mystmd for `docs/`.** No document was migrated. The measurements below are the
deliverable; the counter-proposal (`tools/check-doc-links.py`) is committed but
is not wired into any gate.

## What this record claims, and what it does not

Claimed: the four destruction rows the issue asked for (L1-L4) plus two extra
reference shapes, each measured with an exit code, both before and after the
mystmd configuration that is supposed to make them fail; and the FSL row F3.

Not claimed: F1, F2, F4 and the backward direction (every FSL element discussed
by at least one hand-written document) were not built and are not measured here.
Whole-corpus migration was not attempted beyond one throwaway build. Nothing
here says what would happen to `tools/build_site_reference.py`'s 50 generated
pages under a real migration; that pipeline was not touched.

## Environment

| | |
|---|---|
| date | 2026-09-28 |
| mystmd | **v1.11.0**, installed with `npm i -g --prefix "$HOME/.npm-global" mystmd` (the default prefix `/usr` is not writable in this container) |
| node / npm | v22.23.2 / 10.9.8 |
| fslc | 4.7.0 (prebuilt binary) |
| corpus | `docs/`, 105 Markdown files before this record was added |

## The project that was stood up

A `docs/myst.yml` is **not** committed. Leaving an inert site configuration next
to the real generated site (`docs/intro/*.html`, owned by
`tools/build_site_reference.py`) would read as a second site. The file used for
every measurement below was, verbatim:

```yaml
version: 1
project:
  id: fsl-docs-myst-spike
  title: fsl docs — MyST reference gate spike
  error_rules:
    - id: reference-target-resolves
      severity: error
    - id: reference-target-explicit
      severity: error
  toc:
    - file: README.md
    - file: DESIGN-myst-spike.md
    - file: DESIGN-bridge.md
    - file: DESIGN-kernel-contract.md
    - file: DESIGN-ci.md
    - file: INTRO-formal-methods-and-fsl.md
site:
  template: book-theme
```

Six of 105 files, chosen because between them they carry every reference shape
the issue tabulates: `README.md` is the hand-maintained index (107 links,
including three links to directories); `DESIGN-ci.md` carries plain
`[text](other.md)` links; `DESIGN-bridge.md` carries a `[text](other.md#anchor)`
reference into `DESIGN-kernel-contract.md`; `INTRO-formal-methods-and-fsl.md`
carries an ` ```fsl ` fence that `fslc check` accepts whole.

**What the narrow scope does not show.** Per-file parse behaviour over the
other 99 files, and the cost of making them MyST-clean, is covered only by the
one throwaway whole-corpus build in "Migration cost" below — 106 pages, but a
single run, not a maintained configuration.

## Destruction cases — the reference gate

The injection site for every row except L4 was the end of the CI design
document; each row was restored with `git checkout` before the next. Exit codes
only; the `⛔️`/`⚠️` markers in the log are not the verdict.

Four configurations were run over the same six rows:

* **A** `myst build --site --strict --ci`, `error_rules` **removed**
* **B** `myst build --site --strict --ci --check-links`, `error_rules` **removed**
* **C** `myst build --site --strict --ci`, `error_rules` as above
* **D** `myst build --site --strict --ci --check-links`, `error_rules` as above

| # | break | expect | A | B | C | D |
|---|---|---|---|---|---|---|
| L4 | none (control) | 0 | **0** | **1** | **0** | **1** |
| L1 | `[broken](DOES-NOT-EXIST.md)` | ≠0 | **0** | 1 | **0** | 1 |
| L2 | `[the missing one](#sec-no-such-label)` | ≠0 | **0** | 1 | 1 | 1 |
| L2b | ``{ref}`sec-no-such-label` `` (the notation these documents do *not* use) | ≠0 | **0** | 1\* | 1 | 1 |
| L3 | `[it](DESIGN-kernel-contract.md#no-such-heading-anchor)` | ≠0 | **0** | 1\* | 1 | 1 |
| L3b | `[it](#decision)` — implicit slug of `## Decision`, same file | ≠0 | **0** | 1\* | 1 | 1 |

\* **not attributable to the break.** In configuration B the control already
exits 1, so every B cell inherits that. Reading the logs instead of the exit
code, the break itself produced only a `⚠️` in B for L2b, L3 and L3b — i.e.
`--check-links` alone does not catch them.

Two things follow, and they are the point of the spike:

1. **Column A is all zeros.** `--strict` is not a gate. Every one of the five
   breaks, including a link to a file that does not exist, builds clean and
   exits 0.
2. **No single switch closes the table.** `error_rules` (column C) closes L2,
   L2b, L3 and L3b and leaves L1 at 0. `--check-links` closes L1 and, on its
   own, leaves L2b/L3/L3b as warnings. They are independent mechanisms;
   `link-resolves` is only evaluated when `--check-links` is passed.

**Warnings from rules that were not promoted, on the control run (row L4,
configuration C): none.** The six-page build emitted zero warnings.

### Exact commands and exit codes

```console
$ cd docs && myst build --site --strict --ci            # C, control
📚 Built 6 pages for project in 197 ms.
=== EXIT: 0 ===

$ printf '\n[broken](DOES-NOT-EXIST.md)\n' >> the CI design document
$ cd docs && myst build --site --strict --ci            # C, L1
=== EXIT: 0 ===                                          # <- the gate misses it

$ cd docs && myst build --site --strict --ci --check-links   # D, L1
⛔️ DESIGN-ci.md:1627 Link for "DOES-NOT-EXIST.md" did not resolve.
=== EXIT: 1 ===

$ printf '\nSee [the missing one](#sec-no-such-label).\n' >> the CI design document
$ cd docs && myst build --site --strict --ci            # A, L2 (no error_rules)
=== EXIT: 0 ===
$ cd docs && myst build --site --strict --ci            # C, L2 (error_rules)
⛔️ DESIGN-ci.md:1627 No target for internal reference "#sec-no-such-label" was found.
=== EXIT: 1 ===

$ printf '\nSee [it](DESIGN-kernel-contract.md#no-such-heading-anchor).\n' >> the CI design document
$ cd docs && myst build --site --strict --ci            # C, L3
⛔️ DESIGN-ci.md:1627 Cross reference target was not found: no-such-heading-anchor
=== EXIT: 1 ===

$ printf '\nSee [it](#decision).\n' >> the CI design document
$ cd docs && myst build --site --strict --ci            # C, L3b
⛔️ DESIGN-ci.md:1627 Linking "decision" to an implicit heading reference, best practice is to create an explicit reference.
=== EXIT: 1 ===
```

## Three findings that are not breaks

These are why the recommendation is negative. None of them is a bug in a
document; all three are mystmd behaving as designed, against a corpus that is
read on GitHub.

### Finding 1 — the control fails under `--check-links`, on links that are correct

```console
$ cd docs && myst build --site --strict --ci --check-links   # nothing broken
⛔️ README.md:132 Link for "../specs/" did not resolve.
⛔️ README.md:132 Link for "../examples/" did not resolve.
⛔️ README.md:132 Link for "../skills/" did not resolve.
Site has 3 errors and 0 warning, stopping build.
=== EXIT: 1 ===
```

All three directories exist. `--check-links` resolves **files** and does not
resolve **directories**, with or without a trailing slash — measured separately
with `[a](../specs)` and `[b](../specs/)`, both errors, while
`[c](../specs/cart_v1.fsl)` and `[d](../tools/build_site_reference.py)` resolve.
So the one switch that closes L1 cannot be turned on over the docs index without
rewriting three links that are correct for the reader.

On the credit side, the same measurement shows `--check-links` does catch a
missing **non-Markdown** target — `[a](../specs/DOES-NOT-EXIST.fsl)` errors —
which is the forward check #1124 wants.

### Finding 2 — mystmd's heading slugs disagree with GitHub's, on anchors that work today

A throwaway whole-corpus build (same configuration, `toc` removed so mystmd
discovers the directory itself) built 106 pages and exited 1 with six findings.
Four are this disagreement:

```
⛔️ DESIGN-seq.md:3       Cross reference target was not found: 5-semantics
⛔️ RUST-PORTING.md:3     Cross reference target was not found: 9-migration-policy
⛔️ DESIGN-assurance-classes.md:56  No target for internal reference "#frozen-python-reference-srcfslcassurancepy" was found.
⛔️ DESIGN-assurance-classes.md:87  No target for internal reference "#envelope-classifier--ledgerassurance_token" was found.
```

`LANGUAGE.md` line 713 is an ordinal-numbered semantics heading;
`DESIGN-rust-port.md` line 345 is an ordinal-numbered migration-policy heading.
GitHub slugs those to `5-semantics` and `9-migration-policy`, which is what the
links use and what works in a browser today. mystmd drops the leading ordinal.
The other two are the same story with punctuation.

Adopting mystmd's resolver therefore means either rewriting these anchors to
mystmd's slugs — breaking them for every reader on GitHub — or adding explicit
`(label)=` targets, which GitHub renders as literal text above the heading. The
cost is paid in the artifact the readers actually read, and the class of
reference it buys coverage for numbers **three** in the whole corpus.

### Finding 3 — mystmd re-parses prose that is valid GitHub Markdown

The remaining two findings of the six:

```
⛔️ DESIGN-document-glossary.md:129:340 unknown role: id
⛔️ DESIGN-document-glossary.md:129:379 unknown role: id
```

The source is a line documenting a heading template, with a code span nested
inside a code span. The inner backticks close the span early, so `{id}` reaches
mystmd's role parser, and `role-unknown` is an error by default. Migration means
making prose mystmd-safe, not only fixing links.

### Migration cost, stated plainly

Six findings over 106 pages is small in volume, and that is a real point in
mystmd's favour. But four of the six are finding 2, and the fix for finding 2 is
visible to readers. That is the trade the recommendation turns on, not the count.

### Two further costs, measured but not tabulated

* **The build reaches the network.** `myst build --site` queries
  `https://api.mystmd.org/templates/site/myst/book-theme` and downloads
  `book-theme` from GitHub. `--check-links` additionally issues HTTP requests to
  every external link — these documents have 22, across `github.com`,
  `fsl.dev`, `ymm-oss.github.io` and `learn.chatgpt.com`. A link check that
  fails when a third-party host is slow is a flaky required gate.
* **The output is not reproducible.** Site JSON carries regenerated AST `key`
  fields, so no `git diff --exit-code` gate over `_build/` is possible.

## F3 — FSL syntax broken inside a fence, line numbers preserved

`INTRO-formal-methods-and-fsl.md` has one ` ```fsl ` fence opening at line 149
and holding a complete `spec EditScreenFlow`. `fslc check` accepts the `.md`
directly, per the literate-Markdown contract. Line **165** was changed from
`requires screen == List` to `requires screen === List`.

```console
$ fslc check docs/INTRO-formal-methods-and-fsl.md
{ "result": "error", "kind": "parse",
  "message": "expected expression at 165:23",
  "diagnostic_code": "FSL-PARSE",
  "loc": { "line": 165, "column": 23 } }
=== EXIT: 2 ===
```

| | line |
|---|---|
| line the author broke | **165** |
| line `fslc` reported | **165** |

The same break, extracted naively into a temporary `.fsl` by concatenating fence
bodies, and checked there:

```console
$ awk '/^```fsl$/{f=1;next} /^```$/{f=0;next} f' docs/INTRO-formal-methods-and-fsl.md > /tmp/naive.fsl
$ fslc check /tmp/naive.fsl
  "message": "expected expression at 16:23",
=== EXIT: 2 ===
```

Reported line **16** against an author line of **165** — a **149-line** offset,
exactly the number of lines above the fence. Give `fslc` the `.md`.

Only three documents here hold fences that form a complete accepted spec
(`INTRO-formal-methods-and-fsl.md`, `DESIGN-nested-option-support.md`,
`GUIDE-analyze.ja.md`); every other ` ```fsl ` fence is a fragment and
`fslc check` on those files already exits 2 with a parse error today. A gate
that runs `fslc check` over every Markdown file here is therefore not available
without first marking which fences are whole specs. That is not attempted.

## Decision

**Do not adopt mystmd at this time.**

The gap the issue identifies is real and worth closing: a plain
`[text](other.md)` link and a `[text](other.md#anchor)` reference are unchecked,
and column A above shows `--strict` does not change that. But mystmd closes it
at the price of three things the corpus cannot pay right now — a slug algorithm
that disagrees with the renderer the documents are read in, a link checker that
rejects correct directory links, and a required gate that depends on four
external hosts. And it subsumes nothing: `tools/build_site_reference.py` still
owns the 50 generated pages, `tests/test_coupled_change_meta.py` still owns index
membership, and `tools/check-design-citation-headings.py` still owns quoted
section citations. Adopting mystmd now adds a fourth mechanism rather than
replacing any of the three, which is the outcome the issue's step 4 rules out.

### What happens to the existing gates

Unchanged, and no overlap is created:

| mechanism | scope | kept |
|---|---|---|
| `tests/test_coupled_change_meta.py:116-123` | `DESIGN-*.md` ↔ docs index membership, both directions | yes |
| `tools/check-design-citation-headings.py` | quoted DESIGN section citations (82) | yes |
| `tools/build_site_reference.py` | generated `intro/*.html` | yes, untouched |
| `tools/check-doc-links.py` (this record) | link-target and anchor existence | proposed, not wired |

The first checks *membership* in an index, the second checks *quoted titles*
against headings, the fourth checks *link targets* resolve. Three different
predicates on three different syntaxes; none subsumes another.

### The counter-proposal

`tools/check-doc-links.py` resolves every in-repository link target found in
Markdown, and every `#anchor` against **GitHub's** slug algorithm, since that is
the renderer these documents are read in. No network, no build, no new markup.
Over the current tree:

```console
$ python3 tools/check-doc-links.py selftest
selftest: 5/5 slug cases pass
=== EXIT: 0 ===

$ python3 tools/check-doc-links.py check
check-doc-links: 106 files, 245 in-repository link targets, 0 findings
=== EXIT: 0 ===
```

Its own destruction rows, same injection site, same method:

| # | break | expect | exit |
|---|---|---|---|
| S4 | none (control) | 0 | **0** |
| S1 | `[broken](DOES-NOT-EXIST.md)` | ≠0 | **1** |
| S1b | `[broken](../specs/DOES-NOT-EXIST.fsl)` | ≠0 | **1** |
| S3 | `[x](DESIGN-kernel-contract.md#no-such-heading-anchor)` | ≠0 | **1** |
| S3b | `[x](#no-such-anchor-in-this-file)` | ≠0 | **1** |
| S5 | `[x](../specs/)` — a directory that exists | 0 | **0** |

S5 is the row `--check-links` fails. S1b is the #1124 forward check for
path-shaped references.

Not covered by this script, and so not claimed: MyST-style `{ref}` and `#label`
cross-references (rows L2/L2b — these documents contain none, because they are
not MyST), reference-style link definitions pointing at anchors in another
repository, and the backward direction (an FSL element no document mentions).

**Wiring, when someone decides to:** two lines beside the existing checker in
`tools/check-merge-readiness.sh`'s `check_automation`:

```bash
python3 tools/check-doc-links.py selftest
python3 tools/check-doc-links.py check
```

It is deliberately left unwired here — turning on a new required gate is an
adoption decision, not a spike result. Run repo-wide
(`python3 tools/check-doc-links.py check .`) it reports one pre-existing
finding outside this directory: `CONTRIBUTING.md` line 115 links a
code-of-conduct file that does not exist. That is not fixed here.

### When to revisit

Adopt mystmd if any of these becomes true: these documents stop being read
primarily on GitHub (a MyST site becomes the canonical rendering, so the slug
disagreement stops mattering); cross-document anchor references grow from three
into the dozens; or the backward FSL direction is actually wanted, since that
needs a parsed AST across pages and a regex family will not get there.
