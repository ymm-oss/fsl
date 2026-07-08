# FSL - `fslc analyze` structural observation layer

Motivation: issues #103, #100, #102, #101, and #110-#116. `verify` and `refine` answer
whether declared contracts hold. `analyze` answers a different question: what
shape does the spec have, and which parts look weakly connected enough to deserve
review?

`analyze` is intentionally not a verifier. Structural findings are review
signals and carry `formal_status: "not_a_violation"` unless a future finding is
explicitly backed by existing `verify`, `refine`, or `replay` semantics.

## 1. CLI

```bash
fslc analyze spec.fsl --projection tsg --format json
fslc analyze spec.fsl --projection action_state_graph --format json
fslc analyze spec.fsl --projection action_dependency_graph --format json
fslc analyze spec.fsl --projection impact_graph --focus state:stock --format json
fslc analyze spec.fsl --projection requirement_property_graph --format json
fslc analyze spec.fsl --projection property_state_graph --format json
fslc analyze spec.fsl --profile ai-review --format json
fslc analyze specs/ examples/e2e/ --profile ai-review --format json
fslc analyze specs/cart_refines.fsl --projection refinement_graph --format json
fslc analyze tests/fixtures/chain/fsl-project.toml --projection traceability_graph --format json
fslc analyze spec.fsl --projection action_state_graph --format dot
fslc analyze spec.fsl --projection requirement_property_graph --format mermaid
```

`--format json` remains the default. `--format dot` and `--format mermaid` are
review-aid graph exports for graph-shaped projections; they do not add a
Graphviz or Mermaid runtime dependency.

Single-file success is `result: "analyzed"` and exits 0. Parse, name, type,
semantics, io, and internal failures reuse the normal fslc error envelope.
`impact_graph` requires `--focus <node-id>` where the id comes from the TSG
(`state:x`, `action:checkout`, `requirement:REQ-3`, etc.). Unknown focus ids
use the normal `kind: "name"` error envelope. `--focus` is single-file only and
is not accepted with `--profile`.
Batch mode accepts files and directories. Directories are expanded recursively
for `*.fsl`, sorted by normalized path, and emitted as one deterministic JSON
envelope with `mode: "batch"`. If any file fails, successful entries remain in
`files[]`, failed entries are also summarized in `errors[]`, and the command
exits 2.

## 2. Typed Semantic Graph (TSG)

The TSG is built from the validated spec dict returned by `build_spec`, not from
raw grammar tuples. This keeps the analysis aligned with the same semantic view
used by `verify`, `scenarios`, `replay`, and `explain`.

Output shape:

```json
{
  "result": "analyzed",
  "analysis": "structure",
  "projection": "tsg",
  "schema_version": "tsg.v0",
  "nodes": [{"id": "action:submit", "kind": "action"}],
  "edges": [{"id": "edge:action:submit:writes:state:stage", "kind": "writes"}]
}
```

Stable node kinds include `spec`, `requirement`, `state`, `phys_state`,
`action`, `guard`, `effect`, `ensures`, `invariant`, `trans`, `leadsTo`,
`reachable`, `acceptance`, and `forbidden`. KPI and control nodes are emitted
when the validated spec carries that metadata.

Stable edge kinds include `declares`, `covers`, `has_guard`, `has_effect`,
`has_ensures`, `reads`, `writes`, `checks`, `starts_with`, and `precedes`.

## 3. Graph projections

Graph projections are deterministic summaries over the TSG or over other
structural sources:

- `action_state_graph`: actions connected to state variables they read/write.
- `action_dependency_graph`: action-to-action structural `enables` edges through
  read/write state bridges, plus write/write `conflicts_with` edges over shared
  state. These are over-approximations, not scheduling semantics.
- `impact_graph`: the induced TSG slice around `--focus`, with upstream and
  downstream closure annotations (`direction`, `directions`, hop distances) for
  review impact analysis. `direction` is one of `focus`, `upstream`, or
  `downstream`; `directions` records both upstream/downstream when a node is in
  both closures.
- `requirement_property_graph`: requirements connected to covered actions,
  properties, scenarios, KPI/control nodes.
- `property_state_graph`: user properties connected to state variables they read.
- `refinement_graph`: standalone refinement mappings with impl/abs spec names,
  state maps, action maps, stutters, and preserve-progress declarations.
- `traceability_graph`: project-manifest graph over business/requirements/design
  files and refinement mappings.

Direct `.toml` inputs to `fslc analyze` are treated as project manifests; the
default filename is `fsl-project.toml`, but review copies with other names are
accepted.

Each graph projection includes `components`, `sccs`, `cycles`, `degree`,
`metrics`, and `formal_status: "not_a_violation"`. `metrics` reports
deterministic structural numbers: node/edge/component/SCC counts, undirected
multigraph `cycle_rank` (`E - N + C` over emitted edges), and fan-in/fan-out
hubs. A disconnected component, high fan-out, or cycle is not a proof failure.
These are trend and review-priority signals for downstream tooling.

## 4. AI-review findings

`fslc analyze spec.fsl --profile ai-review` emits deterministic review findings:

- `disconnected_requirement`: a requirement node has no useful anchor such as an
  action, user property, acceptance scenario, forbidden scenario, KPI/control, or
  governance metadata.
- `unanchored_property`: a user invariant/trans/leadsTo/reachable declaration has
  no requirement tag, scenario/governance anchor, or action-state connection.
- `progressless_cycle`: a multi-action structural cycle is linked to a
  requirement tag or acceptance/forbidden scenario, and no explicit progress
  story is attached. The heuristic does not inspect English terms in action or
  state names.
- `unwritten_state`: a state variable is initialized and may be read, but no
  action writes it.
- `unread_state`: a state variable is written, but no transitive relevance chain
  reaches a guard, property, ensures clause, or acceptance/forbidden scenario.
  Relevance propagates backward through effect reads only when the effect's
  write target is already relevant, which catches dead state clusters without
  treating every effect read as meaningful.
- `unguarded_action`: a non-generated action has no explicit `requires` clause.
- `conservation_candidate`: counter-like `Int` effects structurally preserve a
  weighted sum. This is a candidate invariant only; proving the invariant is the
  job of `fslc verify` / `--engine induction`.

Every finding has:

- `finding_id`
- `analysis`
- `finding_type`
- `severity`
- `confidence`
- `formal_status`
- `involved_nodes`
- `witness`
- `why_it_matters`
- `candidate_repairs`
- `do_not_assume`

`progressless_cycle` is deliberately conservative in naming. It does not use
`H1`, `Betti`, or `homology` in public output, and it does not rely on
language-specific words such as "retry" or "pending". A cycle can be valid
retry, review, or compensation behavior; the finding only says that a
requirement/scenario-linked cycle has no visible progress story.

Project-level `traceability_graph` can additionally emit
`traceability_gap` findings when an upper-layer requirement/control ID has no
visible lower-layer structural anchor. This is still review-only; verified
refinement evidence remains the job of `fslc chain` and `fslc refine`.

## 5. Schemas

Versioned schema files live under `schemas/fslc/analysis/`:

- `tsg.v0.schema.json`
- `analysis-graph.v0.schema.json`
- `analysis-findings.v0.schema.json`

Downstream consumers should check `schema_version` before assuming shape.
Additive optional fields can remain in the same schema version; removing or
changing required field semantics should use a new version.

## 6. LSP diagnostics

`fslc-lsp` can surface `--profile ai-review` findings as informational
diagnostics when started with `FSLC_LSP_ANALYSIS_DIAGNOSTICS=1`. These
diagnostics use source locations from TSG nodes when available, fall back to the
best indexed declaration range, and remain clearly marked as structural review
signals from `fslc analyze`, not formal verifier errors.

## 7. Implementation notes

The analysis package is in `src/fslc/analysis/`:

- `tsg.py`: spec dict to TSG, expression read extraction, assignment write extraction.
- `graph.py`: deterministic connected components, SCCs, representative cycles.
- `projections.py`: graph projections over TSG.
- `invariants.py`: structural invariant candidates from restricted counter
  effect patterns.
- `refinement.py`: structural graph projection for standalone mapping files.
- `project.py`: manifest-level traceability graph assembly.
- `export.py`: DOT and Mermaid formatting.
- `findings.py`: AI-readable review findings.

The implementation does not call Z3 and does not perform bounded reachability.
If a future finding needs proof status, it should explicitly call or consume the
existing verifier/refinement/replay result and state that evidence in the JSON.
