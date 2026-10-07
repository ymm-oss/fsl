# Binding one hand-written claim to one FSL element — spike record for #1128

Status: **spike record. The mechanism holds for one claim; nothing is wired
into a gate.** One claim was annotated, in `docs/design/DESIGN-testplan.md`. The
measurements below are the deliverable.

## What this record claims, and what it does not

Claimed: a prose claim in a `DESIGN-*.md` document can be bound to one named
FSL element so that changing that element raises the claim for re-verification
(calibration 2) while changing another part of the same specification does not
(calibration 3); the answer to the third case — the binding is untouched and
only the claim's prose is rewritten — with its reason; and a measured
comparison of the three annotation syntaxes as GitHub renders them.

Not claimed: that the claim is *true*. This mechanism never decides that. It
decides only whether the recorded human verification still covers what is
written now. Not claimed either: anything about the other 97 `DESIGN-*.md`
documents, any coverage or completeness property (no backward direction: an
FSL element with no claim is not reported), and any cost estimate for
annotating the corpus.

## Environment

| | |
|---|---|
| date | 2026-09-28 |
| base commit | `1397c173cef3a98b6a71f799777a9c0a2143ba09` |
| Python | 3.12.3, standard library only |
| `fslc` | 4.7.0 (prebuilt binary) |
| projector | `fsl-claim-facts@0`, `tools/spike-1128-claim-binding/fsl_claim_facts.py` |

## The subject

`docs/design/DESIGN-testplan.md`'s "Boundary case selection" states that
guards outside the *parameter op integer-literal* shape yield no boundary case,
"That is why `specs/job_pipeline.fsl` produces none." That sentence is true
only as long as the guards of that specification stay outside the recognized
shape. It is exactly the class of sentence this spike is about: its referent
keeps existing, so a link check can never see it go wrong.

It is bound to one element, `action submit`, whose two guards the sentence
quotes. The evidence recorded with the claim is the command that exhibits the
absence, and it currently holds:

```
$ fslc testplan specs/job_pipeline.fsl --depth 0
cases: ['failure_s0_v0', 'failure_s0_v1', 'failure_s0_v2', 'failure_s0_v3']
selection_coverage: {"vectors_available": 7, "vectors_selected": 4, "uncovered": ["v4", "v5", "v6"]}
```

No `boundary_accept_*` or `boundary_reject_*` case is produced.

## Why the shipped projector could not be used

The `myst-fsl` skill ships `dct` with the projector `ts-claim-facts@0`. It
rejects a binding to a specification outright:

```
$ PYTHONPATH=$K python3 -m dct fingerprint docs/_dct-probe.md --root .
unsupported bind language: fsl
exit=2
```

That is the right failure — it refuses rather than guessing — but it means the
shipped projector cannot serve this repository at all. Its fact grammar
(`x.y === z`, `x.y = v;`, `throw new E`) is TypeScript, and this repository has
373 Rust and 232 Python files against three TypeScript files, one of which is
the VS Code extension and two of which are test fixtures. A projector for FSL
was therefore written for this spike. It is 429 lines, standard library only,
and is not installed anywhere.

`fsl-claim-facts@0` differs from `ts-claim-facts@0` on one point deliberately:
the facts block is a **full-set assertion, not a selector**. `ts-claim-facts@0`
compares only the facts the document names, so a statement *added* to the bound
code is invisible to it. Here every fact of a listed kind found in the bound
element is compared, which is what makes calibration 2a below detect an added
guard. The element name in the binding is what scopes the comparison instead.

## Calibration

Every row is one edit to the working tree, then
`python3 tools/spike-1128-claim-binding/fsl_claim_facts.py check
docs/design/DESIGN-testplan.md --root .`, then a revert. Exit 0 = nothing to do,
3 = `stale`, 2 = indeterminate.

| # | edit | status | exit |
|---|---|---|---|
| baseline | none | `verified` | 0 |
| 2a | add `requires j <= 1` to the bound `action submit` | `stale`, `facts_changed` | 3 |
| 2b | change the bound guard `queue.size() < QCAP` to `< 3` | `stale`, `facts_changed` | 3 |
| 3a | change another action's guard (`finish_retry`, `tries < 1` → `< 2`) | `verified` | 0 |
| 3b | change an invariant body (`RunningMatches`) | `verified` | 0 |
| 3c | change `const QCAP = 3` to `5` | `verified` | 0 |
| 4 | reword the claim's prose only, binding untouched | `stale`, `prose_changed` | 3 |
| — | point the binding at a nonexistent `action submitt` | error | 2 |

Row 2a is the one that matters: `requires j <= 1` *is* the
parameter-op-integer-literal shape, so that edit makes the sentence false, and
the diff names the guard that appeared:

```json
"diff": {"requires": {"head": ["j <= 1", "jobs[j].st == New", "queue.size() < QCAP"],
                      "verified_at": ["jobs[j].st == New", "queue.size() < QCAP"]}}
```

Row 3c is a **false negative and is by design**: `QCAP` moving from 3 to 5
changes what the bound guard means while leaving its text identical, and the
binding does not follow the constant. Scoping to a named element is what buys
row 3; it is the same property that loses row 3c.

## The third case: prose changed, binding unchanged

**Decision: it should raise the claim, as a separately named reason
(`prose_changed`), and the reason must never be folded into the one used for a
code change.**

Reasons:

1. `verified-at` records that a person compared *this text* with *that element*
   at *that commit*. Rewriting the text changes the subject of that comparison.
   The attestation does not transfer to a sentence nobody attested to, even
   when the world did not move.
2. This is the only place in the mechanism where "the writer moved" is visible
   at all. The fact fingerprint cannot see prose; it would pass a rewrite that
   quietly widens a claim from one action to a whole specification. If a prose
   edit does not raise the claim, that failure mode has no detector anywhere.
3. The costs are asymmetric. Clearing a prose-only raise is one `stamp`
   invocation and a one-line diff, and the reviewer does not have to re-read
   the specification to clear it — the tool has already told them the facts are
   unchanged. Missing a broadened claim is undetectable afterwards, because the
   next verification stamps the broadened text as verified.
4. The two reasons must stay distinct precisely because the response differs:
   `facts_changed` means read the diff and decide whether the sentence still
   holds; `prose_changed` alone means re-read the sentence. Merging them would
   make the frequent, cheap case train reviewers to dismiss the rare, expensive
   one. The shipped `ts-claim-facts@0` hashes the prose too, but reports both
   as a bare `stale`; distinguishing them is the correction this spike makes.

The cost accepted with this decision: a typo fix inside a claim region costs a
re-stamp. That is a real tax, and it is the reason the claim region should be
kept to the sentences that actually make the claim.

## Which annotation syntax, and how GitHub renders it

Rendered through GitHub's own Markdown API (`gh api --method POST /markdown -f
mode=gfm`), the three candidate syntaxes give:

| syntax | GitHub output |
|---|---|
| colon fence `:::{claim}` | the fence markers and every option line appear as literal text in the paragraph — and **`:id:` renders as the 🆔 emoji**, because GFM reads it as an emoji shortcode |
| backtick directive ` ```{claim} ` | the whole block, claim sentence included, becomes a code block |
| paired HTML comments | nothing of the metadata renders; the claim sentence renders as the ordinary paragraph it is |

**Recommendation: paired HTML comments** (` <!-- claim … --> ` … `<!-- /claim
-->`), which is the form used for the annotated claim. The claim's text has to
stay readable prose, because that text is the document's actual content; the
other two syntaxes damage it, and the colon fence damages it unpredictably
(the emoji substitution is not something a reader can reverse-engineer).

The cost of the HTML comment form is that a reader on GitHub cannot see that
the paragraph is attested, or to what. If that visibility is wanted later, the
place to add it is a visible Markdown link in the prose rather than a visible
directive: `tools/check-doc-links.py` already resolves
`../specs/job_pipeline.fsl#action:submit` against the declarations in the
specification, so a visible link would carry the same target *and* be checked
for existence.

## Overlap with the existing document checks

`tools/check-design-citation-headings.py` checks **existence**: a quoted
section title cited next to a `docs/design/DESIGN-*.md` path must match a heading in
that document. This mechanism checks **staleness**: a bound element and the
claim's own text must not have moved since the recorded verification. They
share no state and cannot disagree about a verdict — but they do meet:

- One edit, two gates, two files. Renaming a heading that sits inside a claim
  region raises `prose_changed` here, and fails the citation check in every
  *other* file that cites the old title. The cheapest fix for one is work for
  the other, and neither tool mentions the other.
- The two parsers see the annotation differently. Both the citation check and
  this projector are line-based, so a heading inside a claim region is still a
  heading to the citation check; under the MyST directive syntax it would be
  directive content instead. Choosing that syntax would split the two tools'
  idea of what the document contains — another reason not to choose it.

`tools/check-doc-links.py` (#1127) is the sharper interaction, measured:

```
$ python3 tools/check-doc-links.py check docs/_probe-links.md
docs/_probe-links.md:5: no such path in the repository: DESIGN-does-not-exist.md
docs/_probe-links.md:7: the specification declares no such element: ../specs/job_pipeline.fsl#action:nope
check-doc-links: 1 files, 3 in-repository link targets, 2 findings
exit=1
```

- A link **inside an HTML comment is checked** (line 5). Under the recommended
  syntax, a link written into a claim header is an invisible obligation: it can
  fail the link gate with nothing on the rendered page to look at.
- A link inside a **backtick fence is not checked** — the fourth link in that
  probe went unreported, and only three targets were counted. So the syntax
  that turns the claim into a code block also removes it from the link gate
  silently. The two failures compound rather than cancel.
- The `#action:submit` fragment grammar is shared by accident: the link check
  already resolves it, and this projector's `bind:` uses the same shape. If
  this mechanism is ever adopted, the two should be made to share one resolver
  rather than two regexes that agree today.

## What this mechanism does not detect

- **A claim whose binding is unchanged but whose meaning drifted.** The
  clearest case measured is row 3c: a constant referenced by the bound guard
  changes, the guard's text does not, and the claim silently becomes wrong.
  Anything the fact grammar does not extract — comments in the specification,
  types, the specification's other elements, everything outside the bound
  element — is in this class.
- **A claim about something with no FSL element to bind to.** Most of the
  corpus's prose is about the compiler's behavior, not about a specification;
  for those, an FSL binding is not available at all and the question of what to
  bind to is open.
- **Missing claims.** There is no backward direction here: an FSL element that
  no document discusses is not reported, and a document paragraph that makes an
  unbound claim looks identical to prose that claims nothing.
- **Whether the claim was ever true.** `verified-at` records that someone said
  so, not that it was checked by a machine.

## What adoption would need, and why it is not proposed here

The projector is a spike artifact: a line-based FSL reader, no parser, no
handling of nested or multi-line clauses, and no tests beyond the calibration
above. Wiring it to a gate would require at minimum a real parse (the `fslc`
AST export already exists), a decision about who re-stamps a claim in review,
and a cost measurement on more than one claim. None of that is justified by a
single subject; what this record establishes is only that the two calibration
directions separate cleanly, which is the precondition for asking the cost
question at all.
