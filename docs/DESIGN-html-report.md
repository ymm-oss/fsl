# FSL — `fslc html` self-contained review report

## Goal

`fslc html <file.fsl> [-o report.html]` turns existing verifier evidence into a
single HTML file that project members can review without reading raw formulas or
running the CLI. The report is a presentation layer over `explain` and `verify`;
it does not introduce a second parser, evaluator, or verifier.

## Inputs

- `explain_file(file, depth)` supplies the review skeleton: state, actions,
  properties, automatic checks, witnesses, and counterfactuals.
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
runtime dependency. It contains:

- a status hero with result, depth, state/action/property counts, and coverage
- verification status and warnings
- state table plus an inline SVG action-to-state write graph
- action/property/automatic-check tables
- counterexample or reachable trace timeline
- witness examples with state snapshots
- counterfactual table
- escaped source with line numbers
- collapsed raw `explain` and `verify` JSON

## Design constraints

The report is a dense product-review surface rather than a marketing page. It
uses restrained surfaces, fixed-radius panels, semantic status colors, tables for
scanability, and SVG only for the model relationship view. All source, formulas,
JSON, and requirement text are HTML-escaped before rendering.

## Non-goals

- Editing `.fsl` files in the browser
- Running verification inside the browser
- Replacing JSON output for automation
- Proving that the written spec matches stakeholder intent; the report only makes
  the existing evidence easier to inspect
