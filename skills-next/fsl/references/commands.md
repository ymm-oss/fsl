# Native commands and JSON contracts

Contents: [catalog](#command-catalog), [engines](#engines-and-proof-scope),
[replay](#kernel-and-replay), [change](#diff-sweep-and-chain),
[strength](#mutation-and-explanation), [analysis](#analysis-and-typestate),
[reports](#reports-documents-and-approvals), [Markdown](#literate-markdown).
For choosing runs and reporting results use [fsl-verify](../../fsl-verify/SKILL.md).
Operands below are placeholders, not filenames to create blindly.

## Command catalog

```text
fslc check FILE [--strict-tags] [--requirements ids.txt] [--edition current|next]
fslc lint PATH... [--edition current|next] [--project fsl-project.toml]
fslc migrate PATH... --edition 5 [--write]
fslc fmt FILE|- [--edition current|next]
fslc fmt PATH... --check
fslc kernel FILE [--kernel-version 1|2]
fslc conformance FILE [--depth K=4] [--kernel-version 1|2]
fslc verify FILE [--depth K=8] [--engine bmc|induction|explicit|auto] [--k N=1]
  [--explicit-budget N=1000000] [--deadlock warn|error|ignore]
  [--vacuity warn|error|ignore] [--property NAME] [--exclude-property NAME]...
  [--instances NAME=N]... [--values NAME=LO..HI]... [--from-state state.json]
  [--strict-tags] [--requirements ids.txt] [--no-cache] [--lemma "EXPR"]...
fslc sweep FILE --depth LO..HI [--instances NAME=LO..HI] [--values NAME=LO..HI]
  [--property NAME]
fslc explain FILE [--depth K=8] [--readable] [--lowering] [--names canonical]
fslc mutate FILE [--depth K=8] [--by-requirement] [--oracle-attribution]
  [--max-mutants N=200] [--from mutants.jsonl] [--fail-on-survivors] [--min-kill-rate R]
fslc scenarios FILE [--depth K]
fslc testgen FILE [--depth K] [--strict]
  [--target pytest|vitest|swift|kotlin|dart|phpunit] [--allow-unwired] [-o OUT]
fslc testplan FILE [--depth K=4]
fslc counterexample export FILE [--depth K] [--engine bmc|explicit|auto] -o OUT
fslc replay FILE --trace events.json
fslc replay FILE --from-log events.jsonl --mapping mapping.fsl
fslc refine IMPL ABS MAPPING [--depth K]
fslc diff OLD NEW [--depth K] [--mapping MAPPING] [--forbid KIND,...]
fslc diff --git BASE..HEAD [spec.fsl] [--depth K] [--forbid KIND,...]
fslc chain [fsl-project.toml] [--keep-going] [--jobs N]
fslc analyze FILE_OR_DIR... [--projection PROJECTION] [--code FILE_OR_DIR]
  [--focus NODE] [--profile ai-review] [--export tag-review] [--format json|dot|mermaid]
fslc typestate FILE [--ts]
fslc html FILE [--depth K] [-o report.html] [--engine bmc|induction] [--no-timing]
fslc ledger FILE [--depth K] [--impl-log run.json] [-o ledger.md]
  [--engine bmc|induction] [--evidence result.json]... [--approval record.json]...
fslc document generate FILE [--view requirements] [--lang ja|en] [--strict]
  [--strict-rendering] [--glossary glossary.json] [--evidence result.json]...
  [--approval record.json]... [--trust-key public.pem]... [-o requirements.md]
fslc document claims FILE [--view requirements] [-o requirements.claims.json]
fslc document check FILE document.md [--glossary glossary.json]
  [--evidence result.json]... [--approval record.json]...
fslc approval create FILE --kind ledger|html|scenarios|requirements_document
  --artifact REVIEWED --approver NAME [--requirement ID]...
  [--glossary glossary.json] [--evidence result.json]... [-o record.json]
fslc approval check FILE --record record.json
fslc approval diff FILE --record record.json [--depth K]
```

`check` includes inline seam/trace contracts; it is not a full safety proof.
`lint` never mutates: exit 0 no findings, 1 findings, 2 input/check failure.
Directory lint inputs recursively select sorted deduplicated regular `.fsl`
files, skipping symlink entries; explicit files retain their supplied extension.
`migrate` is a dry run unless `--write`, with an atomic validated write set;
manual-required/unsafe rewrites refuse without writing. `fmt` prints source;
`--check` reports clean/changed/error with exit 0/1/2.

Stricter rules follow each source's own header. Consumed child files retain their
own edition; do not assume a parent's edition flag propagates to dependencies.
Migration edits the requested roots, not imported children. Canonical mixed
editions are allowed where the consumer supports those files.

## Engines and proof scope

| Engine/result | Interpretation |
|---|---|
| BMC verified | No violation through inclusive depth K, fixed declared world. |
| Induction proved | All-depth checked safety at those sizes; inspect per-property progress scope and not_covered. |
| Explicit verified, closure false | Concrete BFS reached depth but not closure: bounded only. |
| Explicit proved, closure true | Full reachable-state closure, no induction lemmas needed. |
| Explicit unknown_budget | State cap exceeded, exit 1; never silently verified. |
| Auto | Tries explicit, then BMC only for explicit unsupported/budget. Read engine and engine_fallback. |

Explicit is solver-free BFS. It rejects leadsTo and nondeterministic/free init,
state-dependent init forall sources and contradictory init. Use BMC for unsupported
models, not to bypass static scope/read-order errors. Results carry states_explored,
max_frontier_width, depth_reached; explicit-decided action_profile counts enabled
states, fired unique edges and no_op self-loop edges separately. It has no --lemma,
--from-state or --k. Auto records the deciding engine, shares that engine's cache,
and never changes the default engine.

`unknown_cti` means undecided; CTI start is not established reachable or unreachable.
Try explicit closure where supported, deeper bounded exploration, or repeatable
`--lemma "EXPR"` in induction. Candidates are recursively Bool-type-checked and
proved independently without original user invariants. Only proved candidates
excluding a current CTI can be used; inspect lemma_cti_exclusions and copy a final
auxiliary_invariant_recommendation back only after review. Invalid/rejected
candidates retain evidence; they are not injected assumptions.

Induction `proved` carries k_used and base depth; unranked response/coverage/
reachability remain bounded. Arithmetic may add `not_covered: ["int_overflow"]`:
the step uses mathematical integers; only base checks i64 overflow. Ranked leadsTo
entries explicitly identify ranking/unboundedness. Report sizes beside every proof.

Property selection restricts named invariant/trans/leadsTo/reachable checks;
induction selection supports invariant/trans, not selected leadsTo/reachable.
Selected trans retains invariant hypotheses; induction base still checks implicit
bounds. Exclusion wins over selection. `_bounds_root` may be selected but cannot
be excluded with --exclude-property. Selection/exclusion/from-state marks inline
implements `not_evaluated` with reasons, not passed. Use unfiltered completion runs.

`--instances`/`--values` override declared entity/number bounds, not raw range
types. Read bounds_overrides. Only references removed by an active override can
skip a scenario: warnings acceptance_skipped/forbidden_skipped and
requirement_traces.result:not_evaluated list them. Inputs already outside declared
scope and in-range failures still fail. Matching upper entity/number bounds are
overridden at inline seams too. Always complete an unscoped run.

Snapshot verification takes a **complete** logical Monitor state, replaces init,
bypasses verdict cache and concrete-identity symmetry, and is BMC-only. Results
stamp bounded_from_snapshot / spec_init:not_used / induction:not_applicable.
Never fill missing fields or call the result a proof from source init.

Ordinary verification cache keys include lowered kernel, raw entry text, flags/
overrides and implementation fingerprint. Hits add cache metadata; cross_depth
can reuse an earlier counterexample. Comments miss intentionally for locations.
`--no-cache` disables it. Cache provenance and ambient solver cost are not verdict
evidence. Do not normalize away same-named domain-state fields in comparisons.

## Kernel and replay

`kernel` is the compiler boundary: typed structural expressions, spans,
requirements/origins and rollback contracts. Default v1 stays public; opt into
v2 provenance graph only when needed and inspect completeness/origin assurance.
Use matching conformance major; neither private models nor expression reparsing
are generator APIs. Compose export has incomplete multi-file provenance and must
not be advertised as truthful complete Kernel export.

Versioned `replay-trace.v1` has Kernel version, exact spec identity, complete
tick-0 initial state, canonical ticks 1..N, exact action/params and complete
post-state. Schema 1.1 supports `action:null`, empty params, unchanged-state
stutters. Timestamp is opaque. Trace v1 accepts Kernel 1.0.0/2.0.0. Ill-shaped
input is exit 2; typed mismatch exit 1. Determined init is compared; free roots
do not get compared against arbitrary defaults. Public compose/branch action
names are `alias.action` / `submit.b1`; internal aliases can be accepted but
outputs stay public. Verifier/testgen traces are not automatically replay input.

Schema 1.2 adds solver-free bounded-liveness monitoring at tick 0 and each
action/stutter: Q at the inclusive deadline succeeds; unfinished obligations
are pending; unbounded leadsTo is unchecked. Safety is separate and first.
Schemas 1.0/1.1 remain safety-only. Unversioned action-only arrays/events remain
compatibility inputs, not complete-state evidence.

Production JSONL replay requires action, params and complete observed post-state
per nonempty line. A refinement-shaped mapping names external impl and target abs,
maps all target state and each external action to an abstract call/stutter.
Missing fields/keys fail log_mapping; read failed_at_record (zero-based), log_line
(one-based) and mismatch. Raw log maps have no typed impl for enum conversions.
Finite log replay does not prove unbounded leadsTo.

## Diff, sweep and chain

Diff is bounded bidirectional refinement, invariant implication and OLD forbidden
replay on NEW. Findings: behavior_added/removed, invariant_weakened/strengthened,
forbidden_relaxed, scope_changed, unknown, or no_semantic_change. Default exit 0
reports findings; list **every** forbidden kind in --forbid, including unknown.
Impl-invalid/budget failures fail regardless of --forbid. A mapping resolves only
its declared direction, never an automatically inverted map. Git mode materializes
full trees so each revision resolves its own imports; two git-show files are not
equivalent. No path selects the changed `.fsl` batch.

Forbidden replay preserves a genuinely rejected final call, not one that violates
after enabled execution. A NEW disabled setup never reaches the tested call and
is unknown, not preservation; --forbid forbidden_relaxed alone misses it. Unknown
actions/arity/kinds or scope-unrelatable inputs are unknown too. Comparison uses
NEW shared entity/number bounds and reports scope changes explicitly.

Sweep preserves every cell under results. A true failure sets sweep_failed and
minimal_counterexample. Only all-insufficient_depth reachability cells can be
inconclusive; each size combination needs a determinate success at some depth.
A success at another size does not settle it. Otherwise sweep_inconclusive exits
1 with null minimal counterexample and inconclusive_scopes. Errors return their
original envelope. Skipped trace/seam sections remain in each summary and
sweep.not_evaluated; sweep_passed says nothing about them. Value sweeps fix LO
and grow upper endpoints LO..LO, …, LO..HI. Compare full stable envelopes; do not
erase nested domain cost/cache fields as execution metadata.

Chain manifest layers [business]/[requirements]/[design] specify file; depth means
verify, absent depth means check. refine_against requires mapping. [impl] accepts
only command/report/evidence and runs last from the manifest directory. Fresh JUnit
evidence needs ≥1 executed test, no failures/errors; missing, unchanged, non-JUnit,
empty or all-skipped report is indeterminate/exit 1. Exit-code-only evidence is
flagged impl_exit_code_only and is for non-test commands, not conformance. Neither
evidence key also means indeterminate. Unknown/empty sections or malformed present
depth/refine_depth reject; absent values alone default. --keep-going retains later
work; otherwise failed layers skip successors. --jobs multiplies solver memory,
not assurance. JSON stdout, table stderr.

## Mutation and explanation

Mutate uses deterministic single changes (guard deletion/negation, assignment
deletion, enum/integer/bound changes, branch swap, fair deletion) against model,
scenario/forbidden and refinement oracles. A refused baseline returns its result,
not a mutation report. Baseline auto uses 1,000,000 states with a fresh BMC retry
when needed. Main checks use a boundary probe, a clean-only 50,000-state explicit
route with reachables witnessed through K, then existing BMC; secondary checks
remain. There is no mutation engine/budget flag. Route/timing progress is stderr,
JSON stdout; explicit-clean can survive an encoding-only BMC build_spec issue.

Survivors are a review queue: equivalent, dead baseline, beyond depth or genuine
under-constraint—not automatic missing invariants. kill_rate = killed/(killed +
survived) at this depth/operator set; no real-bug probability or completeness.
Compare accepted baselines, inspect target/loc/killed_by, and source-ground repairs.
--by-requirement records observed lower bounds; case IDs alone are not requirement
relations. Requirements rule/decide guard attribution differs from event/row covers;
see [fsl-verify](../../fsl-verify/SKILL.md#6-spec-strength-mutate).
--oracle-attribution adds killers and sole/shared by_obligation counts.

--from appends JSONL full mutated_spec (spec alias) or exact replace instruction
(target/replacement/optional occurrence). --max-mutants caps builtins only; 0 permits
external-only. Malformed/static-invalid mutants, including ill-typed seam maps,
are invalid, not kills and not in denominator. Oracle errors remain error;
budget cutoffs inconclusive. Precedence killed > error > inconclusive > survived.
Requested --fail-on-survivors/--min-kill-rate gates fail on no judged mutants,
oracle errors or any inconclusive; threshold uses published four-decimal rate.
Domain mutations target returned kernel_source, not authored domain locations;
do not compare absolute rates across dialects or call dead compensations killed.

Explain is deterministic, not LLM interpretation. It shows skeleton, locations,
implicit bounds/partial operations, writes, fairness, bounds, projections, branch
lowering, synthesized mappings and shortest found invariant counterfactuals.
Counterfactual search skips liveness; no witness or preempted/unsupported concrete
startup notes are limits, not proof of necessity. --lowering and readable origin
arrows connect requirements rules/rows to generated clauses; confirm every rule
landed on exactly its intended events using the layer's origin self-check.

## Analysis and typestate

Analyze projections: tsg, action_state_graph, action_dependency_graph, impact_graph,
requirement_property_graph, property_state_graph, refinement_graph,
traceability_graph, code_audit. Graph modes support DOT/Mermaid where applicable;
code_audit is single-spec JSON with --code and exact @fsl.trace target matching.
Requirements/relations on init can anchor graph coverage; connectivity is not proof.
Refused requirement traces may appear with trace_gate:unchecked.

Batch files/directories stay deterministic and retain partial errors; analyzing
nothing cannot pass. --focus accepts raw node ID or displayed name. Dependency
edges retain every state bridge in states; public action identities stay visible.
ai-review findings include disconnected_requirement, unanchored_property,
progressless_cycle, unwritten/unread_state, unguarded_action, conservation_candidate,
plus depth-4 explicit reachable probes for divergent_choice/unconstrained_effect
(the evidence_basis name bounded_bmc is frozen vocabulary). Ask spec_question;
absence of a depth-4 witness is not determinism. Acknowledged deferrals stay visible.

Tag-review compares exact identifiers, not natural-language semantics. Cite exact
tag text, declaration tuple and TSG IDs when adding agent judgment, retain
formal_status:not_a_violation, and never turn suggestions into verifier failures.
No external source/comment transmission without explicit permission.

Typestate classifies local state/struct-field/Option machines as derivable,
branching or relational; same-named fields of distinct structs are distinct.
All disjuncts must constrain the entity for an or-guard to be local. Guarded
read/query actions are explicit self-loops. Full applicability needs all transitions
derivable/branching; --ts emits only derivable scaffolding, not implementation
proof. Native generation consumes Kernel v1, not private ASTs.

## Reports, documents and approvals

Ledger groups findings by requirement/control without new verification. Assurance
is per element and independent of verdict: proved(induction), bounded, replay-observed,
statistical, not_run. Unranked responses/coverage/reachables in induction remain
bounded. Unknown CTIs are not reachable counterexamples. Verification errors
return complete JSON/exit 2/3 before direct ledger/HTML creation. Non-error failures
can render with nonzero exit. Trace-contract errors refuse dependent reports too.

--impl-log errors fail ledger rather than disappearing; nonconformance adds a
finding while exit remains tied to verify. --evidence failures add attributed or
spec-level findings without changing assurance. Appended raw findings omit only
their top-level execution cost/cache, not nested domain values. Migration/vacuity/
coverage/deadlock/incomplete findings remain different categories. Corrected
renderings can invalidate approval digests; review and re-approve.

HTML --no-timing omits elapsed timing and ambient solver memory/conflicts/decisions/
propagations from presentation copies, retaining solver checks and domain values.
It does not change verify JSON, exit, or normalize paths/backends. HTML approval
expects the default timing-bearing rendering and normalizes values with keys
retained, so --no-timing is not an approval artifact shortcut.

Document claims emits deterministic RCIR; generate renders ja/en; check detects
structural drift (conformant 0 / drifted 1 / input 2 / internal 3), not prose truth.
Only spec/requirements project; unsupported dialect is a coded scope boundary.
Evidence/glossary/approval inputs must match generation/check. The retained
`fsl-requirements-document` skill from the base bundle edits only background,
never generated claims, assurance or approvals.

Approval create requires a clean tracked Git baseline and a reviewed artifact
matching fresh rendering. Records bind normalized spec/kernel/artifact inputs;
requirements documents also bind claim-set digests. Check reports approved/drifted
with reasons; diff compares approved commit to current spec. Approver names are
attribution; authenticity depends on review/signing policy. Approval is neither
intent fidelity nor formal assurance. Drift requires review, not fabricated records.

## Literate Markdown

`check`, `verify`, `scenarios` accept `.md` FSL fences directly, preserving Markdown
line positions; all extracted fences are one unit. Imports resolve relative to
the document, but a compose target cannot itself be Markdown. Use checked fences
for complete units, `fsl-fragment` for non-compiling excerpts, never to hide a
broken complete program.

Most other spec-path commands (lint/migrate/fmt/kernel/conformance/explain/mutate/
typestate/testgen/testplan/reports/analyze/diff/refine/replay/sweep/counterexample,
DB/domain/AI hard checks, causal/document/approval commands) reject Markdown as
usage, FSL-INPUT-LITERATE-UNSUPPORTED. Save a proper source file for those commands.
Chain reads a manifest; DB import reads SQL/Prisma. AI eval/regress/drift use their
separate project loader. An approval positional is always a spec, even when its
--artifact is a requirements Markdown document.

Cross-dialect command families and their syntax/evidence limits are retained in
[advanced](advanced.md) and [causal/NFR implementation guidance](impl.md).
