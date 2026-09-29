<!-- SPDX-License-Identifier: Apache-2.0 -->

# Backward documentation coverage: the measurement, and what it decides

**Spike record for #1138. Status: measured; the outcome is a periodic report,
not a gate.** `tools/report-doc-backward-coverage.py` is the report. It is
deliberately not wired into `tools/check-merge-readiness.sh`, and the reason is
the number below, not a preference.

Two directions connect hand-written documents to FSL:

- **Forward** — every FSL element a document names exists.
  [`tools/check-doc-links.py`](../../tools/check-doc-links.py) resolves link
  targets including `../specs/cart_v1.fsl#action:add_to_cart` (#1127).
- **Backward** — every FSL element is discussed by at least one hand-written
  document. Nothing did this, and [`README.md`](../README.md) said so.

The backward direction catches what the forward one cannot: a behaviour lands,
a specification is written, the generated documents follow it automatically,
and nobody writes the rationale, the out-of-scope note, or the runbook. Every
gate stays green because nothing is broken — something is simply absent.

## The denominator

The repository holds **541 `.fsl` files**. Almost none of them is a claim this
repository makes about itself; most exist to be consumed by something that
already owns their meaning. The denominator is therefore

> every FSL element declared under `examples/`, except `examples/gallery/`
> — **516 elements in 120 files**

where an *element* is one `spec` / `refinement` / `domain` / `dbsystem` /
`action` / `invariant` / `trans` / `forbidden` declaration, counted with the
same regular expression `check-doc-links.py` resolves `#kind:name` against, so
both directions share one notion of "FSL element".

### What is excluded, and why

| Excluded | Files | Elements | Reason |
|---|---:|---:|---|
| `tests/`-style fixtures (`rust/**/tests/fixtures/`, `tests/fixtures/`) | 323 | 702 | A regression fixture is owned by the test that reads it, and that test is itself checked. Prose about it would be a second, unchecked copy of the assertion. |
| `specs/` | 23 | 134 | The conformance corpus ([`DESIGN-conformance-harness.md`](DESIGN-conformance-harness.md)): written to be verified, not to be documented. #1138 states this exclusion itself. |
| `examples/gallery/` | 71 | 213 | Deliberately invalid, adversarial and injected inputs. Their meaning is "must be rejected"; a per-element rationale is an anti-goal, and the gallery's own harness already asserts the rejection. |
| `docs/intro/specs/` | 3 | 19 | Payloads embedded in the generated manual site — content of a generated page, not a separately documentable element. |

Two inclusions are worth stating, because the easy move was to drop them and
report a smaller number:

- `examples/self/` (7 files, 84 elements) is **in scope** and is the sharpest
  case: these are specifications of `fslc`'s own shipped behaviour, exactly the
  scenario #1138 describes.
- `examples/<feature>/` (113 files, 432 elements) is **in scope** even though
  including it adds 432 to the denominator and worsens every ratio below. An
  example nobody discusses teaches nobody, so it belongs in the measurement.

## What "discussed" means

Three mechanical levels, weakest first. The numerator is **hand-written**
Markdown only: the 177 tracked `.md` files left after removing `CHANGELOG.md`,
`changelog.d/` (assembled from fragments) and anything under a `fixtures` or
`snapshots` directory.

| Level | Rule |
|---|---|
| `file` | Some hand-written document names the declaring `.fsl` file. A repository-prefixed path (`examples/self/fslc_fold.fsl`, optionally `../`-relative) counts anywhere, in prose and inside fenced blocks alike — that is the convention [`README.md`](../README.md) states. A bare `fslc_fold.fsl` also counts, but only in a document whose own directory holds that file, which is how `examples/*/README.md` names its specifications. |
| `file-nonindex` | The same, from a document that is not an index: not `docs/README.md`, and not the `README.md` in the element's own directory. An index row is cheap to write, so this separates a catalogue entry from a discussion. |
| `element` | A hand-written document links the element itself, `<path>.fsl#kind:name`. This is the only level that is about the element rather than about the file that happens to contain it, and it is the reference form `check-doc-links.py` already resolves in the forward direction. |

## The first measurement

`python3 tools/report-doc-backward-coverage.py report`, at the commit that adds
this document:

| Denominator: 516 elements, 120 files | Discussed | Undiscussed | Undiscussed share |
|---|---:|---:|---:|
| `element` | 0 | **516** | 100% |
| `file-nonindex` | 267 | 249 | 48% |
| `file` | 444 | 72 | 14% |

At file granularity: **35 of 120 in-scope files are named by no hand-written
document at all**, and a further 35 are named only by an index.

The full layer table, including the excluded layers, is printed by the same
command; `--json` emits every undiscussed element.

## The decision, from the number

**A periodic report. Not a gate, and not a ratchet either — yet.**

- **Not a gate.** At the only level that is about an element, coverage is
  `0 / 516`. A gate would fail on every specification in `examples/` on the day
  it was wired, which is not a gate, it is a stop-work order.
- **Not a ratchet at the `element` level.** A ratchet forbids the undiscussed
  count from rising. With 516 of 516 undiscussed and a baseline of zero
  discussed elements, every newly added `action` raises the count, so the
  ratchet would demand an `#kind:name` link for every new declaration — a gate
  in disguise, imposed on a notation used **3 times** in the entire repository
  (and only once on an element that still exists; the other two are
  `check-doc-links.py`'s own negative fixtures).
- **A report, at all three levels.** The `file` row is the one that looks
  ratchetable — 72 undiscussed of 516, 35 named files short — and that is the
  level to revisit once the outstanding 35 are named. Doing it now would freeze
  a number nobody has yet had a chance to move, which measures the ratchet's
  luck rather than the documentation's health.

The next decision point is therefore arithmetic, not taste: when the `file`
row reaches 0 undiscussed, a "must not regress" ratchet at the `file` level
costs nothing and is worth wiring. The `element` row has no threshold proposed
here, because 0/516 does not support one.

## What is still not claimed

- **A reference is a reference.** That a document names an element says nothing
  about whether the surrounding prose is correct, complete, or current. That
  second property is #1128's subject, not this one.
- **The denominator is a judgement, not a derivation.** The excluded layers are
  excluded for the reasons tabulated above; nothing in the repository computes
  the "is this a claim about the product" predicate, and the report re-applies
  the table as written.
- **No gate.** `tools/check-merge-readiness.sh` does not run this report, and no
  CI job fails on its numbers.
