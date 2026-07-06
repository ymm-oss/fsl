# FSL - `fslc analyze` structural observation layer

Motivation: issues #103, #100, #102, and #101. `verify` and `refine` answer
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
fslc analyze spec.fsl --projection requirement_property_graph --format json
fslc analyze spec.fsl --projection property_state_graph --format json
fslc analyze spec.fsl --profile ai-review --format json
```

`--format` currently accepts only `json`. Success is `result: "analyzed"` and
exits 0. Parse, name, type, semantics, io, and internal failures reuse the
normal fslc error envelope.

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

Graph projections are deterministic summaries over the TSG:

- `action_state_graph`: actions connected to state variables they read/write.
- `requirement_property_graph`: requirements connected to covered actions,
  properties, scenarios, KPI/control nodes.
- `property_state_graph`: user properties connected to state variables they read.

Each graph projection includes `components`, `sccs`, `cycles`, `degree`, and
`formal_status: "not_a_violation"`. A disconnected component or cycle is not a
proof failure. It is only structure for downstream review.

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

## 5. Implementation notes

The analysis package is in `src/fslc/analysis/`:

- `tsg.py`: spec dict to TSG, expression read extraction, assignment write extraction.
- `graph.py`: deterministic connected components, SCCs, representative cycles.
- `projections.py`: graph projections over TSG.
- `findings.py`: AI-readable review findings.

The implementation does not call Z3 and does not perform bounded reachability.
If a future finding needs proof status, it should explicitly call or consume the
existing verifier/refinement/replay result and state that evidence in the JSON.
