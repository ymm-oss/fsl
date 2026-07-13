# FSL — `fslc html` self-contained review report

## Goal

`fslc html <file.fsl> [-o report.html]` renders a `.fsl` spec as a human-readable
document — the project philosophy is that the spec source *is* the
documentation, and `fslc html` is its rendering for human readers (PMs, new
engineers), not just a verification-tool dump. The report is a presentation
layer over `explain` and `verify`; it does not introduce a second parser,
evaluator, or verifier.

## Inputs

- `explain_file(file, depth)` supplies the review skeleton: state, actions,
  properties, automatic checks, witnesses, counterfactuals, and — where the
  spec has them — KPI declarations, entity/domain instance counts, enum member
  lists, and derived stage-transition flows (`stage_flows`, reconstructed from
  enum-valued state plus guard/assignment shape; this is how a business
  `process` block compiles down, but the derivation itself is dialect-agnostic).
  Action `requires`/`ensures` text and property bodies are rendered from the
  AST (`_expr_to_text`), not a single truncated source line, so multi-line
  predicates show in full. Declarations synthesized by a dialect frontend (a
  `deadline`-derived invariant, the time-block-generated `tick` action) are
  tagged `"generated"` in the skeleton; the report routes them into the
  auto-checks table instead of the human Actions/Properties tables.
- `run_verify(file, depth, deadlock)` supplies the current verification result:
  status, warnings, action coverage, reachable witnesses, counterexample traces,
  and boundedness metadata.
- The original source text is included with escaped line-numbered rendering so
  review comments can refer back to the `.fsl` file.

## CLI contract

```
fslc html spec.fsl --depth 8 -o report.html
```

- With `-o`, writes the HTML file and prints the standard JSON envelope with
  `result:"generated"` and `kind:"html_report"`.
- Without `-o`, writes the HTML directly to stdout, mirroring `testgen`'s
  "artifact to stdout" behavior.
- Parse/type/semantic/io errors use the existing CLI error envelope and exit-code
  convention.

## Report structure

The HTML is intentionally self-contained: no CDN, no external JavaScript, and no
runtime dependency. Sections are ordered model-first — the written model before
the verifier's verdict on it:

- a hero with a spec-derived one-line summary (state/action/property counts,
  entity/KPI counts when present), plus result, depth, and coverage
- state table (enum-typed state shows its member list) plus an inline SVG
  action-to-state write graph; when present, an Entities & Domains panel, a
  KPI table, and a stage-transition ("process flow") panel per enum-valued
  state var
- action table (only human-authored actions; an optional Actor column when
  any action has one) and property table (`invariant`/`trans`/`leadsTo`/
  `reachable`), plus an automatic-checks table that also holds anything
  dialect-generated (type-bound checks, partial-op checks, generated actions,
  deadline invariants)
- when present, an **Intentional Undecided Decisions** table listing each
  `undecided:` declaration, its reason, and state-dependency-derived affected
  requirement IDs; the section explicitly says the marker is metadata rather
  than a verification condition
- verification status and warnings
- counterexample or reachable trace timeline, including relation edge summaries
  when trace state contains `relation A -> B` values
- refinement evidence when `verify` reports an inline `implements` failure:
  implementation-side action/state/trace next to the abstract-side mapped state
  and mismatch payload
- witness examples with state snapshots
- counterfactual table
- escaped source with line numbers
- collapsed raw `explain` and `verify` JSON

`ID: sentence` requirement/policy meta renders as a caption under the
declaration's own name (not a trailing "Requirement" column), and is omitted
entirely for a row that has none — no `none` filler cells. The same principle
applies elsewhere: the property table's "Deadline" column only appears when at
least one property has a `leadsTo ... within`, and an action's Ensures cell is
left empty rather than a "none" chip when the action has no `ensures` clause.

## Design constraints

The report is a dense product-review surface rather than a marketing page. It
uses restrained surfaces, fixed-radius panels, semantic status colors, tables for
scanability, and SVG only for the model relationship view. All source, formulas,
JSON, and requirement text are HTML-escaped before rendering.

Relation and refinement evidence are display-only views over existing verifier
JSON. `fslc html` does not invent graph semantics: relation graphs render the
pair lists already emitted in traces, and refinement panels render
`implements.violation` / `refinement_failed` payloads side by side.

## Non-goals

- Editing `.fsl` files in the browser
- Running verification inside the browser
- Replacing JSON output for automation
- Proving that the written spec matches stakeholder intent; the report only makes
  the existing evidence easier to inspect
