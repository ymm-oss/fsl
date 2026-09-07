## Extended workflow commands

After the core skill's `check` → bounded `verify` → induction loop, use these
commands only when the task calls for them.

As needed: `fslc explain file.fsl --depth 8 --readable`
   (emits, as deterministic JSON, the spec skeleton, implicit type-bound/partial_op
   checks, a "what if this rule were absent" counterfactual for each user
   invariant, and reachable/scenarios witnesses; `--readable` emits a text view
   that surfaces verification bounds, fairness, KPI projections, branch lowering,
   and synthesized refinement mappings. For PMs/consultants, ask them to
   adjudicate concrete traces rather than logical formulas),
   `fslc analyze file.fsl --profile ai-review`
   (emits structural review findings over the Typed Semantic Graph, such as
   disconnected requirements, unanchored properties, progressless cycles,
   unwritten state, and unguarded actions, plus depth-4 BMC-backed
   `divergent_choice` / `unconstrained_effect` questions. Present
   `spec_question` to the specification owner instead of choosing a branch or
   inventing a constraint. If `acknowledged:true`, retain the finding and show
   its `acknowledged_by` declaration/reason as an intentional deferral; if
   the fields are absent, keep it in the unresolved review queue. `analyze` also supports batch
   file/directory review, standalone `refinement_graph`, project
   `traceability_graph`, DOT/Mermaid graph exports, and JSON schemas under
   `schemas/fslc/analysis/`. These are review signals with
   `formal_status:"not_a_violation"`, not proof failures),
   When you add natural-language judgment on top of `analyze` output, keep it
   agent-side: cite exact source text and TSG node ids, keep
   `formal_status:"not_a_violation"`, do not turn suggestions into fslc
   violations or CI failures, and do not send source/requirement/comment text to
   an external model unless the user or environment has explicitly opted in,
   For tag/formula alignment, first run
   `fslc analyze file.fsl --export tag-review` and compare one declaration tuple
   at a time. Treat `tag_stale_reference` / `tag_formula_disjoint` as exact
   identifier evidence only, not semantic proof,
   `fslc mutate file.fsl --depth 8 --by-requirement`
   (shows how many model mutations the spec's properties kill; a survivor is not a
   failure but a candidate for a missing invariant / acceptance / forbidden. For a
   spec whose baseline is not verified, it emits no mutation report and returns the
   baseline result. Track survivors and kill-rate against an accepted baseline: a
   regression in that delta is the signal; one absolute survivor count is not),
   `fslc scenarios` (integration-test skeleton JSON),
   `fslc testgen -o test_x.py`
   (implementation-conformance pytest skeleton), `fslc counterexample export -o bug.reproducer.json`
   (export a bounded safety-invariant counterexample to reproducer.v1; slice 1 of #885),
   `fslc replay --trace events.json`
   (normalized spec-action log conformance), or `fslc replay --from-log
   events.jsonl --mapping log_mapping.fsl` (production action/state mapped through
   refinement syntax), `fslc refine impl.fsl abs.fsl mapping.fsl` (faithfulness check
   of a detailed spec), and `fslc diff old.fsl new.fsl --depth 8` (bounded
   semantic change analysis with behavior/invariant/forbidden witnesses). Diff
   findings are informational by default; add an explicit comma-separated
   `--forbid` policy to make selected kinds fail CI.
   In a Git/PR workflow use `fslc diff --git BASE..HEAD [spec.fsl]`: both full
   trees are materialized so imports resolve at their own revision. Omit the
   path to compare all changed `.fsl` files. Do not replace this with two
   `git show` temporary files for imported or composed specifications.
   For AI tool-boundary contracts, use `fslc ai check file.fsl` on
   `ai_component` specs and `fslc ai replay file.fsl --logs events.jsonl` for
   runtime event evidence. For recursive fsl-ai agent composition, use
   `fslc ai check file.fsl` on `agent` specs; it returns `agent_analyzed` and
   deterministic `agent_ir` / graph summaries for lexical scope, explicit
   authority/context grants, visibility, orchestration, tool reachability, and
   failure policy. These check syntactic/structural hard facts such as tool
   authority, forbidden tools, human approval, and agent graph boundaries;
   evaluator-backed and statistical AI claims are evidence, not formal proof,
   and are out of Phase 1.
   Note: what verify/induction guarantees is the **internal consistency of the
   spec**, which is separate from **whether the implementation honors the spec
   contract**. If implementation conformance is also required, anchor to the
   implementation with `testgen` (pytest via an Adapter) / `replay` (matching
   against execution logs).
   For scope-sensitive failures, use `fslc sweep file.fsl --instances Case=1..3
   --depth 1..8 [--property Name]`; it reports each run under `sweep.results` and
   the first failing scope under `sweep.minimal_counterexample`.

## 7. CLI and JSON essentials

```
fslc check <f>                                  # syntax / names / types only; f = .fsl or .md (literate)
fslc lint <path>... [--edition current|next] [--project fsl-project.toml] # edition + ID-policy findings; never mutates
                                                 # exit 0 no findings, 1 findings exist, 2 I/O or check failure (unconditional per input, refused legacy tokens excepted)
fslc migrate <path>... --edition next [--write] # dry run by default; atomic validated write set
                                                 # exit 0 migrated, 2 refused/I/O/check failure (same check-failure contract as lint)
fslc fmt <f|-> [--edition current|next]         # canonical source on stdout; input is never mutated
fslc fmt <path>... --check                      # JSON; exit 0 clean, 1 changed, 2 error
fslc kernel <f> [--kernel-version 1|2]          # normalized typed Kernel JSON (default v1)
fslc conformance <f> [--depth K=4] [--kernel-version 1|2] # matching vectors (default v1)
fslc verify <f> [--depth K=8] [--engine bmc|induction|explicit|auto] [--k N=1]
               [--explicit-budget N=1000000]        # explicit/auto; max visited states
               [--deadlock warn|error|ignore] [--vacuity warn|error|ignore]
               [--property <Name>]                  # check one named property obligation
                                                    #   (invariant / trans / leadsTo / reachable;
                                                    #    selected trans keeps induction invariants)
               [--exclude-property <Name>]...       # skip named invariant/trans/leadsTo/reachable
               [--instances NAME=N]...              # override verify-block `instances NAME = N`
               [--values NAME=LO..HI]...            # override verify-block `values NAME = LO..HI`
               [--from-state state.json]            # complete Monitor/replay state; replaces init (BMC only)
               [--strict-tags] [--requirements ids.txt] [--no-cache]
               [--lemma "<expr>"]...                 # induction only; independently adjudicated
fslc sweep <f> --instances NAME=LO..HI --depth LO..HI [--property Name]
                                                     # grid of verify runs; JSON sweep.results/minimal_counterexample
fslc explain <f> [--depth K=8] [--readable]    # JSON by default; --readable emits a text review view
fslc mutate <f> [--depth K=8] [--by-requirement] [--oracle-attribution] [--max-mutants N=200]
              [--from mutants.jsonl]
fslc scenarios <f> [--depth K]                  # reach_* / cover_* / respond_* / deadlock_terminal
fslc replay <f> --trace <events.json>           # conformant | nonconformant
fslc counterexample export <f> [--depth K] [--engine bmc|explicit|auto] -o <reproducer.json>
                                                # reproducer.v1 artifact from a safety-invariant violation
fslc replay <f> --from-log <events.jsonl> --mapping <mapping.fsl>
                                                # production JSONL -> mapped action/state -> Monitor
fslc testgen <f> [--depth K] [--strict] [--target pytest|vitest|swift|kotlin|dart|phpunit] [-o out]  # Adapter skeleton + conformance tests (pytest default / Vitest / Swift Testing / kotlin.test / package:test / PHPUnit)
fslc testplan <f> [--depth K=4]                 # closed test-plan.v1 selection of conformance vectors
                                                # (accepting + requires_failed); formal_result:"not_run",
                                                # assurance_effect:"none"; pass a spec at the
                                                # implementation's layer granularity
fslc refine <impl> <abs> <mapping> [--depth K]  # refines | refinement_failed
fslc diff <old> <new> [--depth K] [--mapping <mapping>]
          [--forbid behavior_added,invariant_weakened,forbidden_relaxed]
                                                  # bounded semantic change report
fslc diff --git BASE..HEAD [spec.fsl] [--depth K]
                                                  # materialize both full revision trees; omit spec for changed .fsl batch
fslc chain [fsl-project.toml] [--keep-going]     # manifest-driven business -> req -> design -> impl table + JSON
fslc analyze <file-or-dir>... [--projection tsg|action_state_graph|action_dependency_graph|code_audit|impact_graph|requirement_property_graph|property_state_graph|refinement_graph|traceability_graph] [--code FILE_OR_DIR] [--focus NODE] [--profile ai-review] [--export tag-review] [--format json|dot|mermaid]  # structural/tag/code review
fslc typestate <f> [--ts]                       # state machine -> ghost-type applicability + TS skeleton
fslc html <f> [--depth K] [-o report.html] [--engine bmc|induction]  # self-contained HTML review report (dev audience)
fslc ledger <f> [--depth K] [--impl-log run.json] [-o ledger.md] [--engine bmc|induction] [--evidence result.json]... [--approval record.json]...
                                                        # business audit ledger by requirement id (PM/audit)
fslc document generate <f> [--view requirements] [--lang ja|en] [--strict] [--strict-rendering]
               [--glossary glossary.json] [--evidence evidence.json]... [--approval record.json]... [--trust-key public.pem]... [-o requirements.md]
                                                        # deterministic ja/en requirements document from RCIR (Requirement Claim IR);
                                                        # --glossary applies presentation-only display labels (FSL-DOC-LABEL-UNKNOWN/-CONFLICT);
                                                        # --evidence overlays a per-requirement assurance class (proved/bounded/
                                                        # replay-observed/statistical/not_run), same envelope shape as `fslc ledger --evidence`;
                                                        # --approval displays a verified requirements_document approval record, failing
                                                        # closed (FSL-DOC-APPROVAL-DRIFTED) if it does not match the current rendering;
                                                        # only spec/requirements dialects project (others: FSL-DOC-DIALECT-UNSUPPORTED)
fslc document claims <f> [--view requirements] [-o requirements.claims.json]
                                                        # emit the RCIR claim set as JSON; agents/tools consume this instead of re-parsing .fsl
fslc document check <f> <document.md> [--glossary glossary.json] [--evidence evidence.json]... [--approval record.json]...
                                                        # structural drift check: generated claim blocks vs a fresh re-render;
                                                        # document_conformant (0) | document_drifted (1); never interprets prose
fslc approval create <f> --kind ledger|html|scenarios|requirements_document --artifact <reviewed> --approver <name>
               [--requirement ID]... [--glossary glossary.json] [--evidence evidence.json]... [-o record.json]
                                                        # bind the reviewed artifact to normalized spec + Git baseline;
                                                        # requirements_document records schema v3/v4 with a claim_set_digest
fslc approval check <f> --record <record.json>          # approved | drifted with machine reasons
fslc approval diff <f> --record <record.json> [--depth K]
                                                        # semantic diff from approved commit to current working spec
fslc domain check <f> [--depth K] [--engine bmc|induction]  # Functional DDD / effect findings
fslc domain analyze <f>                                      # aggregate/effect ownership summary
fslc domain expand <f> [-o out.fsl]                          # generated kernel FSL
fslc domain generate <f> --target typescript|python|kotlin|swift|rust [-o dir] # Functional DDD scaffold
fslc domain testgen <f> [--target vitest] [-o out]           # adapter/conformance scaffold
fslc domain replay <f> --logs events.jsonl                  # runtime command/event/effect evidence
fslc db check <f> [--depth K] [--engine bmc|induction]  # dbsystem compatibility findings
fslc db observe <f> --trace events.json                 # runtime observation evidence
fslc db import <sql|schema.prisma> [--source auto|sql|prisma] [--name Name] [-o out.fsl]
                                                        # SQL DDL / minimal Prisma -> dbsystem
fslc ai check <f> [--depth K] [--engine bmc|induction]  # ai_component hard-contract findings
fslc ai replay <f> --logs events.jsonl                  # AI runtime replay evidence, not proof
                                                        # check/compat/replay fail closed on invalid ai_component (exit 2)
fslc ai eval <f> [--records <path>] [--dataset <Name>] [--slice <Name>] [--property <Name>]
                                                        # Wilson-bound check over precomputed eval JSONL
fslc ai regress <f> [--migration <Name>] --before-records <p> --after-records <p> [--dataset <Name>]
                                                        # ai_migration.no_regression metric drop/increase check
fslc ai compare --from <records> --to <records> [--from-label L] [--to-label L] [--dataset <Name>]
                                                        # metric deltas between two eval JSONL files, no threshold claim
fslc ai drift <f> --logs events.jsonl [--baseline-logs p] [--window N] [--baseline p] [--property <Name>]
                                                        # observed_property threshold/drift check from runtime telemetry
fslc ai compat <f> [--environment <env>]                # emit a dbsystem artifact capability profile for AI compat
fslc compat check <f> [--include-ai]                    # dbsystem compatibility check, optionally folding in AI capability profiles
```

Each `lint` path may be a file or directory. Directories recursively expand to
regular `*.fsl` files; symlink entries and other extensions are skipped while
walking them, and the combined file set is deduplicated and sorted
deterministically. Explicit file paths retain their existing extension-agnostic
behavior.

Native generated-code replay uses `replay-trace.v1`: a closed root carrying
trace and Kernel versions, exact spec identity, complete tick-0 `initial`, and
events with exact Public Kernel `action`/`params`, canonical ticks `1..N`, and
complete post-transition `state`. Trace schema 1.1 adds explicit stutter as
`action:null` plus empty params; its state must equal the unchanged Monitor
state. Equal-state stutters may be inserted/deleted, while unreported concrete
intermediates are outside invariant judgment. Optional `timestamp` is opaque
and ignored. Trace v1 accepts Kernel 1.0.0/2.0.0. Ill-shaped/incomplete input is exit 2; typed
state divergence is exit 1 with leaf mismatches. `initial` is checked against
`init`'s own computed initial state only when `init` fully determines one; if
`init` leaves any state variable free (BMC explores every admissible value
there), `initial` is trusted as the concrete starting point directly instead
of failing `initial_state_mismatch` against an arbitrary default value for
that variable. Bare arrays/`{events}` are the
unversioned action-only compatibility adapter; testgen/verifier traces are not
replay input. See `docs/DESIGN-replay-trace.md`.

Schema 1.2 opts into solver-free bounded-liveness replay. Every
`leadsTo P ~> within K Q` is observed at tick 0 and after each action/stutter;
`Q` at the inclusive deadline succeeds and absence of `Q` fails. Safety is
reported first and separately. A finite unfinished obligation is `pending`, and
unbounded `leadsTo` is listed as unchecked. Schema 1.0/1.1 stays safety-only.

Use native `fslc kernel` as the stable compiler boundary after dialect lowering
and type checking. Do not consume the frozen Python AST JSON or reparse expression
strings: every exported expression has a structural type and span, actions and
properties carry requirement/lowering origin, and partial failures declare
rollback conditions. The default and legacy Rust API remain Public Kernel v1.
Select `--kernel-version 2` only when a consumer needs the queryable provenance
graph; check its `completeness` and per-origin assurance rather than assuming v2
means every dialect is source-complete. Requirement relations remain separate
from origin targets. Use `fslc conformance` with the same major and the matching
`schemas/fslc/kernel/conformance.v{1,2}.schema.json` to test an independent runtime.
The compatibility policy and field contract are in
`docs/DESIGN-kernel-contract.md`; v2 provenance is in
`docs/DESIGN-kernel-origin-v2.md`.

For an induction `unknown_cti`, first try `--engine explicit` — if exploration
closes it returns `proved` with **no lemmas at all** (the invariant being
non-inductive is irrelevant to exhaustive search). Only when explicit is
rejected or returns `unknown_budget`, fall back to lemma hunting: pass
candidate auxiliary invariants with repeatable `--lemma "EXPR"`. fslc proves each candidate independently (original
init/actions + implicit bounds, without original user invariants), rejects
false/non-inductive/invalid candidates with their own evidence, and makes only
`proved` candidates available to the target proof. A candidate is used only
when it is false on the current CTI; `lemma_cti_exclusions` records the target,
CTI, and violated steps. On final `proved`, copy the declarations from
`auxiliary_invariant_recommendation` into the spec and review that source edit.
There is no flag for injecting an unverified assumption, and `--lemma` is an
error with the BMC engine.

`--engine explicit` enumerates the concrete state space (Z3-free BFS). It is
the fastest route on small-state-space specs and, when exploration **closes**
(no new states within `--depth`), returns `result:"proved"` with
`closure:true` — a complete, unbounded proof that needs no lemmas, including
for true-but-not-inductive invariants where induction returns `unknown_cti`.
Depth exhaustion without closure returns bounded `verified` (same strength as
BMC); exceeding `--explicit-budget` returns `unknown_budget` (exit 1) — never
a silent `verified`. Violations return the same shortest-counterexample trace
schema as BMC. Results carry `states_explored`, `max_frontier_width`, and
`depth_reached`. Fail-closed rejections (kind `semantics`, exit 2): `leadsTo`
properties, nondeterministic `init` (every state variable must be definitely
assigned), and `init forall` binder domains that reference state variables
(range bounds and collections must be compile-time constants) — use
`--engine bmc` for those specs. A definitely-assigned but contradictory
`init` (an `init forall` that writes different concrete values to the same
non-indexed location across binder values, e.g. `forall k: K { x = k }` with
`|K| > 1`) is also rejected, matching BMC exactly: `result:"error"`,
`kind:"vacuous"`, `message:"init constraints are unsatisfiable"`, exit 2 —
never `proved`. `--from-state`, `--lemma`, and `--k` do not apply to this
engine.

An explicit-decided envelope also carries `action_profile` separately from
`cost`: each action has non-negative `enabled`, `fired`, and `no_op` counts.
`enabled` counts unique explored states in which the action has an enabled
instance; `fired` counts successful unique `(state, action instance)` edges;
and `no_op` is the fired self-loop subset. The profile is deterministic under
the canonical BFS traversal and is not emitted for BMC or induction. An
`--engine auto` result includes it only when `engine` is `"explicit"`.

`--engine auto` tries explicit first and falls back to bmc transparently
when explicit can't decide the spec (a fail-closed rejection above, or
`unknown_budget`); everywhere else explicit's own verdict is final. Every
result carries `engine: "explicit"` or `engine: "bmc"` naming whichever
engine decided; a fallback additionally carries `engine_fallback: {from:
"explicit", reason: "...", kind: "unsupported"|"budget"}` — `kind`
distinguishes a permanent gate from one a larger `--explicit-budget` might
clear. `auto` shares its cache entries with plain `--engine explicit`/`bmc`
runs of the same spec (the cache key is always the engine that actually
decided, never `auto` itself), does not change the default engine, and is
Rust-only.

**Literate Markdown FSL.** `fslc check`, `fslc verify`, and `fslc scenarios`
accept `.md` files containing ` ```fsl ` fenced code blocks directly — no
extraction step or flag needed. Non-fsl lines are blanked in place so all
diagnostic positions point to the Markdown document's own line numbers.
Multiple fsl blocks form one compilation unit (split definitions across
sections). Files without fsl fences are rejected; non-fsl fences
(` ```python ` etc.) are ignored. A literate `.md` may `use`/compose `.fsl`
files relative to its own directory; using another `.md` as a compose target
is not supported. Most other spec-reading commands (`lint`, `migrate`, `fmt`,
`kernel`, `conformance`, `explain`, `mutate`, `typestate`, `testgen`, `testplan`, `html`,
`ledger`, `analyze`, `diff`, `refine`, `replay`, `sweep`, `counterexample export`,
`db check`/`observe`, `compat check`, `domain check`/`analyze`/`expand`/`generate`/`replay`/`testgen`,
`ai check`/`replay`/`compat`,
`causal check`/`analyze`/`diff`/`ledger`/`observe-expectations`/`verify-expectations`,
`document generate`/`claims`/`check`) reject `.md` input as an input-kind
error (`kind:"usage"`, `diagnostic_code:"FSL-INPUT-LITERATE-UNSUPPORTED"`,
`loc` naming the input file, not a spec position) instead of handing it to
the parser (issue #665). `chain` (project manifest, not a spec) and `db import`
(SQL/Prisma schema artifact) are not spec-path commands in this sense.
`approval create` cannot write a record whose `spec.path` is `.md`; when
`approval check`/`diff` see a matching record they parse the positional as FSL
and reproduce the `1:2` lie (measured with a forged record) -- excluded pending
issue #980. `ai eval`/`regress`/`drift` already parse `.md` through `load_ai_project`
(valid literate AI project succeeds; otherwise a clean semantic error) and are
unaffected. See `rust/fslc/src/literate_access.rs`'s `LITERATE_EXCLUDED`.

`diff` uses bidirectional bounded refinement for behavior changes, implication
between the OLD/NEW user-invariant conjunctions, and replay of OLD `forbidden`
scenarios against NEW. Its stable finding kinds are `behavior_added`,
`behavior_removed`, `invariant_weakened`, `invariant_strengthened`,
`forbidden_relaxed`, `scope_changed`, and `unknown`; an empty report uses
`no_semantic_change`. A changed `verify` scope is explicit and comparison uses
NEW's shared entity/number bounds. Findings exit 0 because the command is an
analysis; use `--forbid` to turn selected kinds into an exit-1 CI gate. Every
verdict is bounded by `--depth`, and a mapping only resolves the direction
declared in its `impl`/`abs` fields (it is never inverted).

Use `verify --from-state` for bounded prediction from a current concrete state,
not for proof. The input must be the complete logical JSON emitted by
`Monitor.state`/replay (enum names, Option as value/`null`, complete Map keys,
Set/Seq arrays, relation pairs). It replaces `init`, bypasses the verdict cache,
disables symmetry reduction for concrete identities, and is rejected with the
induction engine. Results always stamp
`faithfulness.scope:"bounded_from_snapshot"`, `spec_init:"not_used"`, and
`induction:"not_applicable"`. A step-zero invariant violation is a valid
predictive result. Do not fill missing variables: partial snapshots are a
different, weaker existential query and are rejected.

For production-log replay, each non-empty JSONL line is an object with
`action`, `params`, and the observed post-action `state`. The mapping file is
parsed by the same `parse_refinement` path as `fslc refine`: `impl` names the
external log schema, `abs` names the target spec, `map` covers every target
state variable, and `action external(args) -> target(exprs)` (or `stutter`)
maps events. The Monitor executes the target action and compares its state with
the mapped observed state. This v1 requires complete observed state; missing
fields/keys are `log_mapping` nonconformance. The first divergence includes
`failed_at_record` (0-based), `log_line` (1-based), and the action/state
mismatch. Finite replay does not check `leadsTo`.

`verify` is backed by a persistent verdict cache (issue #169) keyed on every
input that can affect its output (the post-desugaring kernel AST, the raw
entry-file text, and every flag/override) plus an implementation fingerprint,
so an unchanged re-run in the same write→verify→repair loop returns instantly
instead of re-solving. A hit adds one additive field,
`"cache":{"hit":true,"key":...,"source":"exact"|"cross_depth"}`; a miss looks
exactly like today's output. `"source":"cross_depth"` means a prior
`violated` result at a shallower depth was reused, because a counterexample's
earliest step does not depend on the requested search bound. Comment/
whitespace-only edits still miss (diagnostics quote source by line number,
so entry-file text is hashed verbatim) — that is a deliberate hit-rate/
staleness trade-off, never a soundness one. `--no-cache` (or `FSLC_CACHE=off`)
opts a run out entirely. Cache writes are atomic, so running `fslc verify` on
many files as concurrent processes (e.g. `xargs -P`, a CI job matrix) is safe —
concurrent runs at worst duplicate solving, never corrupt the cache. When
verifying a whole project's specs, prefer process-level parallelism over a
sequential per-file loop. See `docs/DESIGN-incremental-verify.md`.

`analyze` is a structural observation layer, not a verifier. `--projection tsg`
emits a stable Typed Semantic Graph over requirements, actions, state variables,
properties, acceptance/forbidden scenarios, and traceability metadata.
`--projection action_state_graph`, `action_dependency_graph`,
`impact_graph --focus NODE`, `requirement_property_graph`, and
`property_state_graph` summarize deterministic components/SCCs/cycles, degree,
and metrics over that graph. It accepts multiple files/directories in batch mode;
directories expand recursively to sorted `*.fsl` files (the `*.fsl` filter applies
only to that expansion — an explicitly named file is always kept, whatever its
extension) and partial failures stay visible in `files[]`/`errors[]`; a batch
that analyzed nothing never reports `result:"analyzed"`/exit 0. Standalone
refinement mappings use `--projection
refinement_graph`, project manifests use `--projection traceability_graph`, and
graph projections can export DOT or Mermaid with `--format dot|mermaid`. A
node's TSG `label` is its `fsl_core::display_name` (a db-dialect internal
separator sentinel is converted back to `__`, matching what `verify`
reports); `--focus` accepts either a node's raw id or its displayed name.
`action_dependency_graph`'s `enables`/`conflicts_with` edges carry every
shared read/write state bridge for the action pair in `states` (plural);
`state` (singular) is only the first one, kept for backward compatibility.
`--projection code_audit --code PATH` is the single-spec, JSON-only bridge from
exact executable Kernel requirement targets to `@fsl.trace` implementation
locations. Treat missing, orphan, and target-mismatch findings as review signals,
not proof. `origin_assurance` describes Public Kernel provenance
(`source_backed|generated_from_source|generated_only|unknown`), never formal
verification strength. See `docs/DESIGN-code-audit.md`.
`--profile ai-review` emits AI-readable review findings such as
`disconnected_requirement`, `unanchored_property`, `progressless_cycle`,
`unwritten_state`, `unread_state`, `unguarded_action`, and
`conservation_candidate`. It also runs a fixed depth-4 bounded semantic probe
for `divergent_choice` (two same-state enabled actions split an
invariant/acceptance outcome) and `unconstrained_effect` (an unread state can
receive different next values from two enabled actions). These add
`evidence_basis:"bounded_bmc"` (frozen v0 vocabulary for a bounded reachability
witness; the native probe is solver-free explicit-state exploration, not
symbolic BMC), a reachable witness, and `spec_question` ending
in `?`. Ask that question; do not invent which branch is intended. Bounded-witness
findings supersede duplicate `unread_state`/`unguarded_action` approximations.
No finding means only “not witnessed within depth 4,” not proof of determinism.
Treat all findings as review signals: they carry
`formal_status:"not_a_violation"` unless a future finding explicitly cites
`verify`/`refine`/`replay` evidence. Versioned schemas live under
`schemas/fslc/analysis/`.

Natural-language interpretation on top of `analyze` is agent-side only. The core
analyzer must not infer semantics from English, Japanese, or other free text.
The deterministic tag checks compare only exact code-shaped identifiers:
`tag_stale_reference` and `tag_formula_disjoint`. For meaning review, run
`fslc analyze file.fsl --export tag-review`, compare each `tag.text` with its
`formal_definition`, cite the declaration tuple, and keep conclusions marked
`formal_status:not_a_violation`; never silently rewrite intent from this export.
If an agent reviews requirement text, comments, or source excerpts together with
the TSG, it must cite the exact text and graph node ids it used, keep
`formal_status:"not_a_violation"`, and never convert that suggestion into an
`fslc` violation, proof result, or CI failure. Non-English text should be handled
by the agent's language capability or a user-approved reviewer, not by hard-coded
keywords in this repository. External model calls are an agent privacy decision:
do not send source text, requirement text, comments, or analysis JSON outside the
local environment unless the user or execution environment has explicitly opted
in.

`ledger` (issue #24) re-organizes `verify`/`scenarios`/`replay` findings **by
requirement id** into a Markdown audit ledger a PM / governance / internal-audit
reader can decide approve/reject/risk-accept from. It is a presentation layer
(no new verification): the `trace_type` discriminator drives a per-finding
business translation, governance columns (risk/decider) come from `control`
metadata when present (fill-in otherwise), and the guarantee limit is stated in
positive form. Raw JSON is demoted to a collapsed appendix. `--impl-log
<trace.json>` folds a `run_replay` conformance row into the ledger, but a
replay **error** (missing file, malformed JSON, wrong-spec trace,
schema-invalid trace) fails the whole `ledger` command through the standard
error envelope and exit code, the same as `--evidence`, rather than rendering
a ledger with the implementation-log row silently missing. A **failing**
`--evidence` source (a definitive nonconformant/mismatch/unsupported verdict,
not a verdict-less gate failure) adds a 🔴 要確認 finding for every
requirement it attaches to — its own root `requirements`/`requirement.id`, or
a `requirement.id` nested inside a `findings`/`checks` array item — or a
spec-level（仕様全体）finding when it fails with no attribution at all; it
never silently renders green while failing evidence sits unread in the
appendix, and never changes assurance class (that stays orthogonal to
verdict). See `docs/DESIGN-ledger.md`.

Digest-bound approvals (issue #190) are separate from assurance class and from
the ledger's empty human-decision checkbox. `approval create` must be run from a
clean tracked Git baseline and only accepts a reviewed artifact that matches a
fresh rendering under the recorded inputs. The sidecar uses a lowered-kernel
digest that ignores source locations plus a normalized artifact digest for
`ledger`, `html`, or `scenarios`. `approval check` and `ledger --approval`
report `approved` or `drifted`; drift reasons distinguish spec, rendering, and
renderer changes. A drifted row carries the complete baseline digest and an
`approval diff` command, which compares the approved commit to the current
working spec. Treat `approver` as attribution; authenticity comes from the
repository's signed-commit/review/branch-protection policy. See
`docs/DESIGN-approval.md`.

Every requirement id in the ledger (and every property row in `fslc html`)
carries an **assurance class** (issue #171): `proved(induction)` (k-induction,
all depths) / `bounded(BMC depth k)` (BMC, depth k) / `replay-observed`
(concrete log/trace checked, not a universal claim) / `statistical(Wilson c%)`
(precomputed eval JSONL, aggregate not per-case) / `not_run` (no formal
evidence — structural analysis, profiles, comparisons). The class is
**per element, not per report**: in an `--engine induction` report only
invariants and transitions reach `proved(induction)`, while `reachable` rows,
action coverage, and an unranked `leadsTo` stay `bounded(BMC depth k)` because
k-induction ranks neither. `--engine induction`
is required for a requirement to ever show `proved`; `--evidence
<result.json>` folds a saved fsl-ai/fsl-db/fsl-domain `formal_result:"not_run"`
producer's output (tagged via a top-level `requirements: [...]` list) into the
per-requirement classification. Class is method coverage, not verdict — a
`violated` BMC run is still `bounded`. See `docs/DESIGN-assurance-classes.md`.

`chain` reads `fsl-project.toml` by default. Each `[business]`,
`[requirements]`, and `[design]` table has `file = "..."`; adding `depth = K`
runs `verify`, while omitting `depth` runs `check`. A layer with
`refine_against = "requirements"` must also set `mapping = "..."`. `[impl]`
runs its shell `command` from the manifest directory. JSON is stdout; the
consolidated table is stderr. Without `--keep-going`, execution stops after the
first failed layer and later layers are marked `skipped`. The manifest reader
is fail-closed: an unrecognized top-level section name, zero recognized
sections (including an empty file), or a present-but-unparseable `depth` /
`refine_depth` value (e.g. one followed by an inline comment) is a `kind:
"parse"` error at exit 2 rather than a silently dropped layer or a silently
substituted default — only an *absent* `depth`/`refine_depth` key defaults.

- `mutate` applies a deterministic single mutation to the kernel AST (requires
  deletion/negation, assignment deletion, enum swap, integer/type-bound ±1,
  then/else swap, fair deletion), re-runs `build_spec` on each mutant, and reports
  whether it is killed by BMC/acceptance/forbidden/refinement. exit is always 0.
  `summary.kill_rate = killed / (killed + survived)` is bounded mutant-set
  sensitivity: it depends on the operator mix, `--max-mutants` cap, depth, and
  oracle, and a high value is not a real-bug detection probability, spec
  correctness, or completeness. A survivor is not a failure and not
  automatically a missing invariant: it may be an equivalent mutant, behavior
  dead at baseline, a beyond-depth effect, or genuine under-constraint —
  triage it as a review queue. If the baseline is not clean at depth K, no mutation is done and
  the baseline result is returned. `--by-requirement` aggregates by the requirement
  tag of the killed property or failed acceptance/forbidden trace declaration
  and warns on zero kills as `empty_formalization` (a lower bound observed for
  this mutant set and depth). Trace attribution uses explicit requirement
  annotations; AC/FB case IDs are not implicit requirements and are unique
  within each declaration kind. `--oracle-attribution` (opt-in) adds per-mutant
  `killers` arrays and `by_obligation` sole/shared counts keyed by oracle display
  names; default output is unchanged and these counts are observed lower bounds,
  not completeness or correctness measures.
  `--from` appends external JSONL mutants. Each line supplies either full
  `mutated_spec` source (`spec` alias accepted) or an exact
  `replace:{target,replacement,occurrence?}` instruction. Valid records use the
  same oracle; malformed JSON/instructions and parse/name/type/construction
  errors are `invalid` rather than killed. `summary.kill_rate` and
  `summary.by_source` exclude invalid records from their denominator, and each
  mutant carries `source:"builtin"|"external"`. `--max-mutants` applies only
  to the built-in catalog (`0` gives an external-only run).
  `mutate` also accepts `domain` documents: it mutates the same rendered
  kernel text `domain expand` emits, so `target`/`loc` use generated action
  names and kernel-text line numbers rather than domain source lines, and
  the envelope carries that text as `kernel_source` so a witness is
  resolvable on its own. `--from` external mutants for a domain document
  must target `kernel_source` text, not the `.fsl` domain source. Absolute
  domain kill-rates are not comparable to other dialects (domain lowering
  emits few properties); read the dead-note-carrying survivor set as the
  primary signal instead — a saga whose compensations are structurally
  unreachable at baseline should show all of their mutants surviving with
  the existing dead-action note, not killed.
- `verify --property Name` resolves across invariant, `trans`, `leadsTo`, and
  `reachable` declarations and checks only the named property kind in isolation.
  Under `--engine induction --property <trans>`, the named transition is the
  only transition obligation, while every user invariant and implicit type
  bound remains in the base case and induction hypothesis. This is the one
  dependency-preserving exception to model restriction. Existing selected-
  invariant behavior is unchanged and still drops sibling invariants. Selected
  `leadsTo` and `reachable` remain rejected by the induction selector.
  `--exclude-property Name` is repeatable and acts as the cross-kind inverse:
  it removes named invariants, `trans`, `leadsTo`, and `reachable` checks from
  the run and from checked-property outputs. If both options name the same
  property, exclusion wins.
- `verify --instances NAME=N` / `--values NAME=LO..HI` (both repeatable)
  override the matching `entity`/`number` bound from a `verify { ... }` block
  without editing the spec — the CLI equivalent of hand-shrinking the model
  for the liveness strategy above. `NAME` must be a declared `entity`/`number`
  (in the business/requirements dialects, or a kernel `spec` using
  `entity`/`number`); an undeclared `NAME` or a malformed value (`Case=abc`,
  `N=5..1`) is a spec error, and it does not apply to a kernel `spec` whose
  domain is a raw `type X = lo..hi` literal. The effective override is echoed
  back as `bounds_overrides` in the JSON envelope. When an override is active,
  an `acceptance`/`forbidden` scenario that no longer fits the shrunken world
  (a hardcoded id/number outside the overridden bounds, in a step argument or
  inside its `expect`) is skipped per-scenario instead of hard-erroring the
  whole `verify`, with a `warnings` entry (`kind: "acceptance_skipped"` /
  `"forbidden_skipped"`) naming it; other scenarios still replay normally.
  Without an override, or for a failure unrelated to bounds, the scenario
  still hard-errors as before. When the spec has an inline `implements`, the
  override also propagates into the abstract spec (restricted to the
  entity/number names the abstract declares) so refinement is checked at the
  same world size on both sides — otherwise a shrunken impl vs a full-size
  abstract fails `map_out_of_bounds`; an impl-only carried number applies to
  the impl only.
- `sweep` is opt-in bounded honesty for scope exploration. It calls normal
  `verify` repeatedly over instance/value/depth ranges and returns
  `result:"sweep_passed"` or `"sweep_failed"`, with every run under
  `sweep.results` and the first failing scope under
  `sweep.minimal_counterexample`. For `--values NAME=LO..HI`, it fixes `LO` and
  expands `LO..LO`, `LO..LO+1`, ..., `LO..HI`. A spec `error` from any scope
  (parse/type/semantics/io/vacuous, a mistyped `--instances`/`--values` name,
  a missing file) is returned verbatim — exit code and `kind` unchanged —
  instead of being folded into `sweep_passed`/`sweep_failed`.
- `explain` is deterministic formatting with no LLM. JSON mode enumerates
  state/action/requires/writes/properties/implicit checks by source loc and
  structural traversal, and attaches to each user invariant the shortest
  counterfactual trace that breaks it under requires/assignment/fair removal.
  `skeleton.spec_kind` names the source dialect (`kernel`/`requirements`/…);
  `skeleton.auto_checks` lists both `type_bound` and one `partial_op` entry
  per syntactic `pop`/`head`/`at`/`/`/`%` site. A `branches { when P { … }
  maps Q }` action and a generated SLA `tick`/`_deadline_*` declaration each
  carry an `origin` (`generated:true`, plus a `branch` lowering step naming
  the guard/correspondence for the former) so `name` still resolves to the
  authored identity rather than the lowered `name.bN`/synthetic form.
  `--readable` emits a text view that surfaces verification bounds, fairness,
  KPI projections, branch lowering (one `branch:` line per split action),
  and a synthesized `Implements:` refinement mapping when the source
  declares one. Invariants for which none is found are explicitly marked
  `no counterfactual within depth K`. Counterfactual weakening checks
  invariants and reachability only; `counterfactual_scope.liveness:"skipped"`
  and the readable header make explicit that `leadsTo` remains the normal
  `verify` command's responsibility.
- `--strict-tags` on `check` / `verify` adds traceability warnings only to
  ok/verified/proved success results. The targets are untagged
  action/invariant/trans/reachable/leadsTo, and IDs declared via
  `--requirements ids.txt` or a `requirement` block in the requirements dialect but
  never referenced. A declaration linked by `@requirement("MODEL-SCOPE-001", ...)`
  / `@requirement("ASSUME-SCOPE-001", ...)` does not become a warning. This gate
  only asks whether a tag exists, not whether it is canonical: a legacy
  `"REQ-1: text"` string counts as tagged here, and `fslc lint` is what reports
  it (`legacy_string_metadata`, exit 1, machine-applicable), so run both.
- `typestate`: determines how far a state machine (a struct field with enum values,
  scoped by field name **and** owning struct type so two structs with a same-named
  field stay independent machines / a state variable / an `Option<_>` slot) can be
  mapped onto the host language's **typestate (ghost types)**. Each action is
  classified as
  `derivable` (the from-state is the entity's own local guard — for a compound guard,
  `or` is the union of what each disjunct implies, but only when every disjunct
  constrains the entity; a disjunct silent about the entity, e.g. an unrelated flag,
  drops the guard entirely rather than narrowing it) /
  `branching` (data-dependent inside an `if`) /
  `relational` (no local guard, the premise lives in an external structure — cannot
  be expressed in the type and remains a runtime/verification obligation).
  A locally guarded action that leaves the entity state unchanged is reported
  as an explicit self-loop and emitted with a generic TypeScript return state
  (`S → S`); an action unrelated to that entity is not part of its machine.
  An entity's `applicability` is `full` only when all transitions are
  derivable/branching. `relational` ones carry a reason (diagnostics) and a
  requirement ID. `--ts` outputs only the TypeScript for the derivable portion.
  The native Rust path consumes public Kernel JSON v1 and rejects unsupported
  schema versions; private Rust AST/model shapes are not a generator API. The
  Python reference implementation remains frozen.
- Counterexample trace: `[{step, state, action{name,params,loc}, changes{path:{from,to}}}]`.
  Shortest guaranteed. State is the logical representation (enum name / Option as
  null|value / Seq as an array / composition as `alias.var` keys). Internal names
  (`__`) do not appear.
- `unknown_cti`: `cti.states` (k+1 states) + `violated_at`. The starting state is an
  unreachable phantom — before hunting for an auxiliary invariant, try
  `--engine explicit` (closure proves without lemmas); if that is rejected or
  exceeds budget, add an auxiliary invariant to exclude the phantom. For invariant
  CTIs (not `leadsTo_rank`), a monotone `Int`/`Map<K, Int>` counter whose CTI
  start lies on the unreachable side of its concrete init value gets a concrete
  candidate in `suggested_invariants: [<expr>, ...]` (also appended to `hint`) —
  a heuristic from trace-monotonicity, not a proof; absent when no such counter
  is found.
- `verified` / `reachable_failed` / `violated` from BMC are bounded and include
  `completeness:"bounded"`, `checked_to_depth`, and fixed-shape `cost` with
  total `elapsed_s`, solver check statistics, and deterministic per-property
  check counts/times. Native/Worker keys and nullability match; Z3 counters are
  maximum observed snapshots. Explicit verification emits zero/null solver
  statistics in the same shape. See `docs/DESIGN-verification-cost.md`.
  Bounded `verified` may include a saturation `hint` when the depth-K frontier
  first witnesses a reachable/vacuity/coverage fact during normal exploration.
- `proved`: `completeness:"unbounded"`, `checked_to_depth` (the base BMC depth),
  `cost`, and `k_used` (the k used per invariant); reachables/coverage come from
  the base case. Ranked leadsTo entries add
  `{proved: true, completeness: "unbounded", proof: "ranking", decreases: ...}`.
  From `--engine explicit`, `proved` instead carries `closure: true` plus
  exploration stats (`states_explored`, `max_frontier_width`, `depth_reached`)
  and no `k_used`; reachables/coverage are definitive (full reachable set).
- `reachable_failed`: each `unreached[]` has `classification`:
  `insufficient_depth` (target satisfiable as a state predicate, no witness by K)
  or `over_constrained` (target unsat under type bounds/invariants, with
  `blocking_requires` naming an irreducible blocking core). Native and browser
  frontends compute this in a solver session independent of BMC witness
  projection; path or deadlock UNSAT state never participates in the
  classification, and diagnostic query history cannot perturb witnesses.
- faithfulness diagnostics may add `faithfulness_class` and
  `recommended_action`: `partial_op_unguarded`, `frozen_only_invariant`,
  `intent_unexercised`, or `liveness_not_refined`.
- **blame assignment** (issue #170, additive): a `violated` result with
  `violation_kind` `invariant`/`type_bound` carries top-level
  `blame.conjuncts[]` (`{index, text, holds, violating_bindings?}`) — which
  AND-conjunct of the invariant is false — and each action-bearing `trace[k]`
  (k≥1) carries its own `blame: {guards[], effects[]}` naming the `requires`
  clauses and state-writing statements that fed the blamed conjunct(s) at
  that step (a backward slice over the concrete counterexample; no new
  solver query). `fslc explain`'s `counterfactuals[].violation`/`.trace`
  inherit both automatically. `reachable_failed`'s `unreached[]` gains no new
  fields, but `vacuous_implication`/`vacuous_leadsto` warnings/findings gain
  the same `classification` + a `blocking` list (empty when merely unreached
  within depth, not structurally impossible). Blame identifies; it never
  proposes a repair — do not turn a `blame` entry into a suggested guard
  weakening (that cuts against the anti-hollowing principle).
- **repair routing (`trace_type`)**: every counterexample/failure result carries a
  `trace_type` discriminator — one of `invariant` | `sla` | `type_bound` | `trans`
  | `ensures` | `partial_op` | `deadlock` | `leadsTo` | `leadsTo_rank` |
  `reachable` | `refinement` | `acceptance` | `forbidden` | `vacuity` |
  `conformance` | `induction_cti` — so an agent can route a fix by channel (and
  tell an `sla` deadline from a structural `invariant`). Passing results and spec
  (parse/type/…) errors carry no `trace_type`. The remaining repair inputs already
  exist — no separate field is added for them: `requirement: {id, text}` (now also
  at the `refinement_failed` root) localizes intent; `trace` / `impl_trace` /
  `cti.states` / `accepted_trace` are the counterexample steps; `checked_to_depth`
  + `completeness` are the guarantee bound; `hint` / `recommended_action` are the
  suggested fix; `unreached[].blocking_requires` is the dead-reachable core.
- coverage diagnostic:
  `{covered: false, name, display_name?, blocking_requires: [{loc, text}], hint}`.
- leadsTo violation: `pending_since` + `loop_start` (lasso) or `stutter: true`.
- progress-preserving refinement failure: `refinement_failed`,
  `kind:"progress_lost"`, `violation_kind:"leadsTo"`, `impl_trace`,
  `progress_failure:"lasso_blocks_progress"|"deadlock_or_stall_blocks_progress"`,
  `progress:{leadsTo, actions}`, and `faithfulness_class:"liveness_not_refined"`.
- **impl self-violation** (checked before any correspondence, `refine`'s own
  input precondition): if the impl spec violates its own type bounds or
  invariants within `--depth` — independent of the abstraction and mapping —
  `refine` reports `result:"violated"` with the impl's own `violation_kind`
  and a `note` that this is a property of the refinement input, not a
  fidelity failure. Never `refines`, never folded into `refinement_failed`.
  `fslc diff` surfaces the same condition as an `impl_violated` finding and
  fails its gate unconditionally (not `--forbid`-gated).
- **action-correspondence argument partial_op (#512)**: an
  action-correspondence argument expression (`impl_action(a) -> abs_action(a
  / c)`) dividing by an impl state variable that can be zero is action
  context, not the "no check" property context a refinement state map gets —
  `kind:"map_partial_op"` (`refinement_failed`, exit 1), distinct from
  `map_out_of_bounds` (a range problem) and from the impl's own body dividing
  by zero (caught by the impl self-violation precondition above). A
  correspondence whose divisor is always guarded on every reachable impl
  step still `refines`.
- **nondeterministic `init` (#493)**: a state variable `init` never assigns
  on any path (an `init if` reading an unassigned `Bool`) is a genuinely free
  initial value across its type domain, not a silently defaulted one. The
  self-violation precondition and init correspondence both check *every*
  concrete initial valuation on both impl and abs, not one materialized
  default — an impl-side valuation must not violate the impl itself, and its
  mapped state must be a member of the abs's own set of valid initial
  valuations, for every valuation on both sides. Breaking: a mapping
  previously `refines` only because one impl initial branch was checked can
  now be `refinement_failed`; a mapping previously `refinement_failed` only
  because α(s₀) missed the abs's single materialized default can now be
  `refines` if it matches a different valid abs branch. A variable init
  assigns on only some paths (not on any) keeps the prior single-value
  behavior.
- leadsTo ranking failure: `unknown_cti` / `violation_kind:"leadsTo_rank"` with
  `rank_failure` (`unbounded_below`, `deadlock`, `non_decreasing_action`, or
  `pending_not_preserved`; with `helpful`, also `progress_action_not_fair`,
  `helpful_action_not_enabled`, `non_decreasing_helpful_action`,
  `non_helpful_action_increases_measure`, and — with two or more distinct
  helpful actions — `helpful_action_enabledness_not_sticky`).

### ⚠ Liveness scales differently from safety — verify it on a reduced model

`leadsTo` is a lasso search: the cost grows roughly **exponentially in the number
of concurrent entities** (the textbook BMC-liveness state explosion), because each
added entity multiplies the interleavings the loop search must consider. Safety
(`invariant` / `trans` / `reachable`) does **not** behave this way — it stays cheap
even at large depth. Observed shape: a single entity verifies in seconds even at
depth 16, but three concurrent entities with `leadsTo` can blow past minutes by
depth ~12. This is a known limit, not a pathological encoding.

Practical strategy:
- Verify **liveness on the smallest model that still exhibits the interleaving** —
  shrink the entity-count range (e.g. `0..1` instead of `0..3`) and use a shallow
  `--depth`. One entity is often enough to find a real `leadsTo` bug. If the
  bound is an `entity`/`number` (not a raw `type` literal), shrink it from the
  CLI with `fslc verify spec.fsl --instances Case=1` instead of editing the
  spec (see §7) — the file keeps its normal verify-block size for everything
  else. If the spec has `acceptance`/`forbidden` scenarios hardcoding ids from
  the original (larger) world, they are not a blocker: under an active
  override, a scenario that no longer fits is skipped with a `warnings` entry
  rather than hard-erroring the run (see §7), so `--instances Case=1
  --property <Liveness>` stays usable without editing those scenarios too.
- Verify **safety separately on the full-size model** at the depth you need.
- Use `--property <leadsToName>` to run a single liveness property in isolation
  while iterating (see §7), so a slow `leadsTo` does not gate the safety checks.
