<!-- SPDX-License-Identifier: Apache-2.0 -->

# Evidence/assurance overlay for `fslc document`

Status: accepted. Implements issue #332.

This design overlays saved external verification evidence onto a generated
requirements document (#326), at requirement granularity, using the exact
assurance vocabulary and classifier `fslc ledger` already established (issue
#171, `docs/DESIGN-assurance-classes.md`, `rust/fsl-tools/src/ledger.rs`). It
adds no new classification logic: a `bounded` BMC run classifies as `bounded`
here for exactly the reason it classifies as `bounded` in the audit ledger —
the same function decides both.

## Why this reuses `fslc ledger`'s classifier instead of inventing one

`fslc ledger` already solved "what assurance class does this JSON evidence
envelope represent?" (`ledger::assurance_token`/`assurance_label`,
`proved`/`bounded`/`replay-observed`/`statistical`/`not_run`). A document-level
overlay needs the identical judgment — the risk of a second, independently
written classifier is exactly the risk this design must avoid: two paths that
usually agree but diverge on some input, silently showing a stronger class in
one place than the other. `assurance_token`/`assurance_label` were changed
from private `fn` to `pub(crate)` so `rust/fsl-tools/src/document_evidence.rs`
(this issue) calls them directly rather than re-deriving the judgment from the
same JSON shape. `evidence_requirement_ids` (extracting the `requirements`
array and/or singular `requirement.id` from an evidence envelope) was factored
out of its two previously-duplicated inline call sites in `ledger.rs` for the
same reason — one matching rule, shared.

## Why v1 is evidence-file-only, with no live verify pass

`fslc document generate` for a given spec always produces byte-identical
Markdown (issue #326's own determinism contract, and the reason `fslc document
check` can do a purely structural re-render-and-compare). A live verify pass
inside `generate` would break that: the same spec could render differently run
to run as the solver's findings change, and `generate`'s own digest fields
would stop meaning "this exact spec projects to this exact document." v1
therefore computes every requirement's assurance purely from `--evidence PATH`
envelopes the caller supplies — the same envelope shape `fslc ledger
--evidence` already accepts (a saved stdout envelope from `fslc verify`, `fslc
ai eval/replay/drift`, `fslc db observe`, `fslc domain replay`, ...) — matched
by requirement ID, defaulting every dimension to `not_run` when nothing
matches. This is a deliberate scope boundary, not a missing feature: a future
issue that wants `generate` to shell out to a live solver run is a different,
larger design with its own determinism story to work out.

## `RequirementAssurance`: three fixed dimensions, never omitted

`rust/fsl-tools/src/document_evidence.rs` (a new file, matching the
established one-file-per-concern convention) owns:

```rust
pub struct RequirementAssurance {
    pub formal: Vec<EvidenceEntry>,        // proved / bounded
    pub conformance: Vec<EvidenceEntry>,   // replay-observed
    pub statistical: Vec<EvidenceEntry>,   // statistical
}
```

`requirement_assurance(requirement_id, evidence) -> RequirementAssurance`
classifies every envelope naming `requirement_id` (via
`evidence_requirement_ids`) with `assurance_token`, dropping only `not_run`
envelopes (an envelope that classifies to nothing is not evidence of
anything); everything else routes into its fixed dimension unconditionally.
The renderer always shows all three dimensions, defaulting each to a literal
`not_run` line when its vector is empty (acceptance criterion 1: an aspect
with no supplied evidence is never silently omitted, always shown explicitly).

Acceptance criterion 3 — a `violated` BMC run must still show `bounded`, never
be silently downgraded because the run found a counterexample — falls
directly out of reusing `assurance_token` unchanged: `completeness: "bounded"`
classifies as `bounded` regardless of what `result` says, exactly as it
already does for the ledger. The `result` field is carried through onto each
`EvidenceEntry` and rendered alongside the label (`` `bounded(BMC depth 5)`
（結果: `violated`、...） ``), so the verdict is visible without ever affecting
the class.

## Where the overlay renders, and why

`assurance_block` renders **after** `claim_blocks`, appended as residue
outside every `<!-- fsl:claim -->` marker pair — never passed into
`wrap_claim_block`. This is acceptance criterion 2: a claim's own digest
(`CLAIM_BLOCK_DIGEST_ALGORITHM` framed over the claim body text, issue #329)
must never depend on whether evidence was supplied, since the assurance
overlay is about verification *coverage*, not about what the FSL itself
specifies. Rendering at requirement granularity (not claim granularity) also
sidesteps a second problem: a claim shared by two requirements (issue #329's
back-reference case) renders its own body only once, but its two owning
requirements are two independent audiences for "how far has this actually
been verified" — folding assurance into the shared claim body would either
duplicate it oddly or attribute one artifact's evidence to a claim in a way
that outruns what was actually checked.

`Ctx` (the renderer's per-render context) gained one field, `evidence: &'a
[(String, Value)]`, populated once at the top of
`render_requirements_document` from the new `evidence: Option<&AppliedEvidence<'_>>`
parameter (mirroring `AppliedGlossary`'s own bundling: `fsl-tools` never
touches a filesystem, so the CLI reads/hashes the files and passes both the
parsed envelopes and a whole-set digest in).

### The liveness caveat

A requirement whose claims include a `ProgressRule` or `ReachabilityGoal`
(resolved by looking up `requirement.claim_ids` against `claims.claims` and
checking each resolved claim's own `.kind` — **not** `Requirement.kinds`,
which holds `@kind(...)` annotation tag ids, an unrelated vocabulary this
design initially conflated with claim kinds before verifying against
`document_project.rs`) gets one extra line when its only formal evidence is
`bounded`: `bounded` means no counterexample was found up to some depth, which
is not a liveness proof (a liveness property can fail only beyond the checked
depth in a way a safety-style BMC bound does not detect the same way). This
caveat is additive text, not a class change — the dimension still literally
says `bounded(BMC depth k)`; the note only reminds a reader what that means
for *this kind* of requirement, reusing the same disclaimer the renderer
already gives progress rules and reachability goals unconditionally elsewhere
(`docs/DESIGN-document-requirement-claim-ir.md`'s existing "not itself a
verification result" framing).

### Fixed document-level sections

Two new sections are unconditional structure, not evidence-dependent content:

- **"検証結果の読み方" / "How to read verification results"** (right after
  "Global semantic conventions", before "Requirements"): the assurance
  vocabulary itself (five fixed lines, one per token), and five fixed
  principles — class/verdict orthogonality, `fair` as a scheduling assumption
  (not immediate execution), `leadsTo` as a demand rather than a result,
  refinement preserving safety but not automatically liveness, and the
  explicit-`not_run`-by-default rule. This section renders identically whether
  or not any `--evidence` was supplied, so a reader always has the vocabulary
  in hand before reaching a requirement's own assurance class — including on
  a document that has never been given any evidence at all.
- **"検証エビデンス出典" / "Verification evidence sources"** (after
  "Glossary", before "Generation info"; entirely absent when no `--evidence`
  was given — same convention as the Glossary section): every evidence file's
  path, its own `result` field, and which requirement IDs (if any, filtered
  against the ones actually declared in this spec) it matched. Rows are
  sorted by rendered text, not by CLI argument order, since the document's own
  determinism contract extends to this section too.

Two pre-existing #326 template lines — "本書は検証結果を含まない" / "this
document contains no verification results" (in `render_reachability_goal`'s
and `render_leadsto`'s fixed disclaimers) — become literally false once an
evidence overlay can exist, and were reworded to describe what the disclaimer
actually means (a `leadsTo`/`reachable` *claim* is a demand, not itself a
verification result) rather than an absolute claim about the whole document.
Every substring the existing negative-control tests pin (`progress_rule_
never_claims_established_evidence`, which additionally bans `proved`/
`証明済み`/`bounded` from appearing near a progress-rule heading) was checked
to still hold after the rewording.

## Digests: `evidence_digest` frontmatter key, order-independent

Frontmatter gains one new optional key, `evidence_digest` (same shape as
`glossary_digest`: a plain `sha256:`-prefixed digest of the raw evidence file
bytes, computed by the CLI, not RCIR's canonical-JSON `framed_digest` scheme).
`DOCUMENT_RENDERER_VERSION` advances from glossary rendering's `1.1.0` to `1.2.0` for this new frontmatter
key, the same reasoning issue #329 already established for a frontmatter
schema change.

Unlike a single glossary file, `--evidence` is repeatable, so "the digest of
the evidence" needs its own combination rule. `load_evidence` (`fslc/src/
main.rs`) hashes each file's raw bytes individually, **sorts** those
per-file digests, then hashes the sorted list joined by `\n` — so the same
file *set* given in a different `--evidence` order on the command line always
yields the same `evidence_digest`, which matters because `fslc document check`
must not report `evidence_changed` merely because a caller listed the same
files in a different order than `generate` did.

## `fslc document generate --evidence PATH` (repeatable)

Each file must parse as JSON and be an object (`FSL-DOC-EVIDENCE-INVALID`
otherwise, mirroring `FSL-DOC-GLOSSARY-INVALID`'s error shape) — this module
classifies nothing about the file's *content* beyond that; `assurance_token`
degrades any unrecognized shape to `not_run` on its own. An evidence file
naming at least one requirement ID that matches none declared in this spec is
`FSL-DOC-EVIDENCE-UNMATCHED` — a warning by default, an error under
`--strict` (checked immediately after evidence loading, using only
`claims.requirements`, before the model build — evidence-unmatched detection
needs no `KernelModel`, unlike glossary target validation). An evidence file
naming *no* requirement ID at all is legitimate whole-spec evidence (the same
convention `fslc ledger` already renders as "（仕様全体）") and is never
flagged as unmatched.

The envelope gains an `"evidence": {"digest": "sha256:...", "files": N}`
object when `--evidence` was given, and unmatched-file warnings share the same
`"warnings"` array `--glossary`'s `label_unknown` entries already use (`{"kind":
"evidence_unmatched", "code": "FSL-DOC-EVIDENCE-UNMATCHED", "target": path}`).

## `fslc document check` evidence parity, and the `skip_claim_text`/`skip_residue_text` split

`check` gains its own repeatable `--evidence PATH` flag, loaded through the
same `load_evidence` helper, and re-renders with whatever it was given (or
without) before calling `check_requirements_document` — the same pattern
issue #330 established for `--glossary`.

The comparison needed no new parameter on `check_requirements_document`
itself: the fresh re-render's own `evidence_digest` already reflects whatever
`--evidence` `check`'s caller supplied. Comparing `frontmatter.evidence_digest`
against `fresh_frontmatter.evidence_digest` is the entire mechanism, with the
same three-way detail split issue #330 established for glossaries
(`generated_with_evidence` / `generated_without_evidence` /
`evidence_digest_mismatch`), reported as `kind: "evidence_changed"`, code
`FSL-DOC-EVIDENCE-CHANGED`.

Issue #329's existing `skip_text` boolean (`renderer_changed ||
glossary_changed`, which skips both per-claim-body comparison and residue
comparison together) does **not** simply gain `|| evidence_changed`. Unlike a
renderer or glossary change, evidence can never appear inside a claim block —
the assurance overlay is residue by construction (see above) — so an
evidence-only change is not a reason to stop comparing claim bodies: doing so
would let a genuine hand-edit inside a claim slip past `check` merely because
the caller's `--evidence` set didn't match what `generate` was given. `skip_text`
is therefore split into two gates:

- `skip_claim_text = renderer_changed || glossary_changed` (unchanged from
  issue #329) — gates `check_claims`'s per-claim-body/digest comparison.
- `skip_residue_text = renderer_changed || glossary_changed ||
  evidence_changed` — gates `check_residue`'s position-by-position comparison.

So a pure evidence change (only `--evidence` differs between `generate` and
`check`) reports exactly one meaningful reason, `evidence_changed`, and claim
bodies are still fully compared — a hand-edit inside a claim is caught exactly
as before, evidence or no evidence. `rust/fslc/tests/document_evidence_cli.rs`'s
`a_hand_edit_inside_a_claim_block_is_still_caught_even_when_evidence_also_changed`
is this design's own regression guard for the split.

## What v1 does not do

- No live verify pass inside `generate` (see above) — v1 is evidence-file-only.
- No new assurance classification logic — `ledger::assurance_token`/
  `assurance_label` remain the sole source of truth.
- No claim-level assurance (only requirement-level) — see the shared-claim
  rationale above.
- No `--approval` interaction beyond what already exists elsewhere; approval
  records remain out of scope for `fslc document` (tracked separately, issue
  #333).

## Verification evidence

`rust/fsl-tools/tests/document_evidence.rs` (19 tests): classification for
each of the five tokens including the violated-but-bounded criterion-3 case;
matching via both the `requirements` array and singular `requirement.id`;
evidence-file-order independence; `unmatched_evidence_paths` for the known/
unknown/whole-spec cases; the claim-block-digest-unaffected criterion-2 test
(parsing both a with- and without-evidence render and comparing every claim
segment); every-dimension-always-renders (criterion 1); the liveness caveat
appearing only on a requirement that actually links a progress-rule/
reachability-goal claim; and a direct confirmation that passing `None` renders
byte-identically to before this issue.

`rust/fslc/tests/document_evidence_cli.rs` (14 tests): the happy path
(envelope shape, frontmatter, assurance text); unmatched-evidence as a
default-mode warning and (on a fixture with no pre-existing coverage issues) a
`--strict` error; malformed and non-object evidence JSON as
`FSL-DOC-EVIDENCE-INVALID`; a missing evidence file as an `"io"`-kind error;
the digest-separation test (`claim_set_digest`/`spec_digest` unchanged,
`artifact_digest` changed, across three evidence states) plus file-order
independence of the recorded `evidence_digest`; `check` conformant with the
same evidence; `evidence_changed` for each of the three mismatch shapes, with
an explicit assertion that no `edit_outside_slot` noise accompanies it; a
regression guard that an evidence-less artifact checks exactly as it did
under issue #329; and the split-gate payoff test proving a genuine hand-edit
inside a claim block is still caught by `check` even when `--evidence` was
also omitted.
