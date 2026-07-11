# FSL — incremental verify: verdict cache, depth hints, differential re-verification

Motivation: issue #169. `fslc` sits inside the LLM write→verify→repair loop, so an agent
re-runs `verify` on identical or near-identical input many times per session; today every run
pays the full Z3 cost. This design adds a persistent verdict cache keyed on a normalized
kernel-AST hash, cross-depth counterexample reuse, and (staged) property-level differential
re-verification.

**Non-negotiable framing**: the verdict is the product. A cache that ever serves a stale
verdict is a soundness regression, strictly worse than no cache. Every decision below is
therefore fail-closed: when in doubt the cache misses, and a cache-layer failure of any kind
degrades to an ordinary uncached run. §7 (the soundness argument) is the contract reviewers
should hold the implementation to.

## 1. Where the cache sits (and where it must not)

The cache is a layer in `cli.run_verify` only — around the span from the engine call
(`bmc.verify` / `bmc.prove`) through the decorations (`_implements_result`, strict-tag
warnings, bounds-skip warnings), i.e. it stores the fully decorated pre-envelope `out` dict
and re-wraps it with `_envelope` on a hit. It is **not** inside `bmc.py` or `runtime.py`:

- `fslc mutate` calls `bmc.verify` directly → mutant verdicts never enter the cache and the
  kill-rate signal is never served from it.
- `fslc verify --from-state` deliberately bypasses lookup and storage. The external
  snapshot changes the initial constraint, and v1 does not include canonical snapshot
  content in the cache key; bypassing is fail-closed and guarded by a signature-classification
  metatest plus a normal-init-vs-snapshot regression.
- The dual-evaluator cross-checks (`tests/test_evaluator_agreement.py`) and the Z3-independent
  oracle (`tests/oracle.py`) exercise `bmc`/`runtime` below the cache — they stay cache-free
  by construction, so the correctness safety net is unaffected.
- Callers that route through `run_verify` — `sweep`, `chain`, and the `explain`/`html`/`ledger`
  internal verifies — inherit caching automatically.

Lookup happens after `_read_spec` and the acceptance/forbidden pre-checks (cheap, concrete
Monitor-based; error paths return before the cache is consulted). Parse + `build_spec` always
run — a hit still costs one parse, which is what makes the key trustworthy.

## 2. Cache key — exhaustive input enumeration

The key is `sha256` over a canonical JSON encoding of every input that can influence the
`run_verify` output. If any component changes, the key changes. The list is closed and must be
kept in one place (`verify_cache.compute_key`); adding a semantics-affecting parameter to
`run_verify` without threading it into the key is the primary review hazard for future changes
(enforced by test, §8).

1. **Cache schema version** — bump invalidates everything.
2. **Implementation fingerprint**: `fslc.__version__`, `z3.get_version_string()`,
   `lark.__version__`, `sys.version_info[:2]`, **and** a sha256 over the sorted
   (relative-path, bytes) of every `*.py` in the installed `fslc` package. The file-content
   fingerprint is what protects editable installs and dev worktrees, where the version string
   does not move when `bmc.py` does. Computed once per process (~1 MB of source; milliseconds).
3. **Kernel AST**: the `(ast, display_names)` pair returned by `parse_src` — i.e.
   *post*-desugaring (compose/requirements/business/governance/db/ai/domain) and *post*
   `--instances`/`--values` override application. Because `expand_compose` /
   `expand_requirements_with_display` inline referenced files into the AST at parse time,
   the AST hash transitively covers every `use`d component and refined abstract spec —
   including the `implements` block's embedded `abs_ast`/`mapping_ast` that
   `_implements_result` verifies. `loc` fields are included (deliberately, see below).
4. **Raw entry-file source** (`sha256` of the bytes). Required because diagnostics quote
   source text by line (`_source_line_text` / `_requires_blocking_entry` slice
   `source_lines[line-1]`), and a text edit can change a quoted line without changing the AST.
5. **All verify options**: `engine`, `depth`, `k_ind`, `deadlock_mode`, `vacuity_mode`,
   `property_name`, sorted `exclude_property_names`, `strict_tags`, the parsed
   `--instances`/`--values` overrides (defense in depth — they are already reflected in the
   AST), and the **content hash** of the `--requirements` file (not its path).

What is normalized away, and what deliberately is not: dialect surface syntax normalizes to
the kernel (two dialect spellings with the same kernel AST + same source text share nothing in
practice because source differs — but a *component* file reformatting that leaves the expanded
AST identical does share). We do **not** normalize away `loc` or entry-file text, because
violated/warning payloads embed both line numbers and quoted source; serving a cached result
with stale locations would misdirect the repair loop. Consequence: comment/whitespace edits to
the entry file miss the cache. That is the accepted trade — hit-rate loss, never staleness.

Canonical encoding: a recursive encoder tagging container types (tuple vs list vs dict —
dicts serialized with sorted keys), then `json.dumps(..., sort_keys=True)` → sha256. The
encoder **raises on any unrecognized type** (fail closed: the run becomes uncacheable rather
than hashing an under-specified representation).

## 3. Storage

- Location: `$FSLC_CACHE_DIR`, else `$XDG_CACHE_HOME/fslc`, else `~/.cache/fslc`. Layout:
  `<root>/verify/v1/<key[:2]>/<key>.json`. Content-addressed keys make a machine-global cache
  safe across projects/worktrees; entries embed no absolute paths (results carry spec names,
  not file paths).
- Entry: `{"schema": 1, "created": iso8601, "fslc": version, "key_inputs": {component
  digests, for debugging}, "result": <decorated pre-envelope out>}`. Only verdict-class
  results are stored (`verified`, `proved`, `violated`, `reachable_failed`, `unknown_cti`);
  `error`/internal results are never cached.
- Writes are atomic (`tempfile` + `os.replace`); concurrent agents at worst duplicate work.
  Unreadable/corrupt entries are treated as a miss and deleted. Entries over 5 MB (huge
  traces) are not stored.
- Eviction: on write, with probability 1/32, prune entries older than 30 days and enforce
  `FSLC_CACHE_MAX_MB` (default 256) LRU-by-mtime; hits touch mtime.
- Escape hatches: `fslc verify --no-cache` (this run neither reads nor writes),
  `FSLC_CACHE=off` (process-wide kill switch), `rm -rf ~/.cache/fslc` (documented; a
  `fslc cache clear|stats` subcommand is optional follow-up surface).

## 4. Output and CLI contract

- On a **hit**, the result dict is byte-identical to the stored run except one **additive**
  field: `"cache": {"hit": true, "key": …, "created": …, "source": "exact"|"cross_depth"}`.
  `cost` keeps the original solve cost (honest: it is the cost of the evidence, not of the
  lookup). Misses emit exactly today's output — no new field — so the corpus snapshot and all
  existing contract tests are unaffected by cache-enabled runs that miss.
- Exit codes are untouched: a hit returns the same `result` value through the same
  `exit_code()` mapping.
- Caching is **on by default** for `fslc verify` (both engines). Rationale: the key
  enumeration in §2 is total over the inputs, the implementation fingerprint covers the
  engine itself, and every failure path degrades to an uncached run; requiring an opt-in flag
  would leave the primary agent loop unaccelerated. `--no-cache`/`FSLC_CACHE=off` remain for
  distrust, and `FSLC_CACHE_VERIFY=1` is a paranoid mode that on every hit *also* re-runs the
  engine and reports any verdict divergence as `{"result":"error","kind":"internal"}` — used
  in CI (§8), available to anyone.
- Test hermeticity: `tests/conftest.py` sets `FSLC_CACHE=off` so the existing suite never
  observes a developer's warm cache; cache tests opt back in with `FSLC_CACHE_DIR=tmp_path`.

## 5. Cross-depth counterexample reuse (the depth hint)

A BMC counterexample is a concrete trace of the transition system; its validity does not
depend on the search bound. `_bmc_explore` scans depths in order, so the first violation it
reports is at the smallest violating step `k`, and the violated result carries
`checked_to_depth = k` (`_checked_to_depth`) without embedding the requested depth anywhere.

Therefore: on lookup, if an entry matches on *every key component except `depth`*, its result
is `violated` with `violated_at_step = k`, and the requested depth is ≥ `k`, the cached result
is returned (`"source": "cross_depth"`). A fresh run at the deeper bound would find the same
earliest step `k` (all shallower steps were checked clean by the recorded run), so this is
behaviorally equivalent, not merely sound. This implements the issue's "a property that
failed at depth k last time is answered from depth k first" without a heuristic scheduler.
No other cross-depth reuse in v1: a `verified`-at-depth-10 entry is *not* served for a
depth-8 request, because verified payloads embed the depth in `depth`/`checked_to_depth`/
warning text; rewriting cached payloads is exactly the kind of cleverness this design bans.
(Index: a small `<root>/verify/v1/xdepth/<depth-agnostic-key>.json` pointer to the violated
entry; same atomicity rules.)

## 6. Property-level differential re-verification (staged; BMC only)

Real solve structure (read from `_bmc_explore`): one shared incremental unrolling
(`transition` constraints per step), and the cost is dominated by per-(step, check) SAT
calls: partial-op guards per action instance, **each invariant** (against a dedicated `inv_s`
that accumulates passed invariants as facts), each `trans`, each `ensures` clause per
instance, reachables/vacuity probes, leadsTo stutter checks, deadlock. Invariants are *not*
asserted into the exploration solver before being checked, so invariant verdicts are mutually
independent in BMC. Honest cost accounting: skipping unchanged invariants/leadsTos removes
their `(depth+1)`-per-property SAT calls but **not** the unrolling construction nor the
model-level checks (partial-op/ensures/deadlock/coverage/requires-vacuity), which dominate in
action-heavy specs. The whole-verdict cache (§1–5) is the guaranteed win; property skipping
is a bounded additional win for property-heavy specs (e.g. requirements-dialect specs that
desugar many `_deadline_*` invariants) and must be benchmarked before being enabled.

Mechanism (stage 2):

- Split the kernel AST into **model core** (everything except the four property item lists:
  invariants, trans, leadsTo, reachable) and **per-property items**, each canonically hashed
  (including `loc` and `meta` — a moved or re-tagged property counts as changed).
- Each cached overall-`verified` BMC entry additionally records
  `{property_hash → name, kind}` for every property it checked, plus its `reachables`
  witnesses, `leads_to` entries, and property-named vacuity warnings.
- On a run whose full key misses but whose *(model core + all options + depth)* matches such
  an entry: properties whose hashes match the entry are **reused** — passed to the existing
  `bmc.verify(..., exclude_property_names=[...])` filter (`_select_properties`), which
  already removes them from checking *and* from their vacuity candidates. Changed/new
  properties are verified normally against the freshly built unrolling; model-level checks
  all re-run.
- Merge on `verified`: union `invariants_checked`/`transitions_checked`, re-attach reused
  `reachables`/`leads_to` entries and property-scoped vacuity warnings from the cached entry;
  additive `cache.properties_reused: [names]`. On `violated`: return as-is — since every
  reused property is known to pass at this depth for this exact model core, the earliest
  violation among changed properties is the earliest overall, matching an uncached run.
- Soundness precondition, all required: engine `bmc`; cached entry overall `verified`; model
  core hash equal; every option and `depth` equal; property reused only on exact item-hash
  match. A "P verified to depth d" verdict is a semantic fact about (model core, P, d) alone
  — other properties cannot invalidate it.

**Induction is excluded from property skipping.** In `prove`, all invariants are asserted as
mutual induction hypotheses at step `k-1`; filtering an unchanged invariant out would weaken
the hypothesis and can turn a provable spec into a spurious `unknown_cti` — not unsound, but
a cache-on/cache-off behavioral divergence, which this design treats as disqualifying.
The sound future path (stage 3, explicitly deferred): `prove(assume_invariants=[...])` that
re-asserts previously-proved unchanged invariants as lemmas *without* re-checking them —
semantically justified because a prior overall-`proved` entry for the same model core
established them as true invariants of this transition system; assuming true lemmas only
strengthens the hypothesis. Until that lands, induction gets whole-verdict caching only.

## 7. Soundness argument — how the cache could lie, and why it can't

| Failure mode | Prevention |
|---|---|
| Spec edit not detected | Key covers raw source bytes + full post-expansion kernel AST; parse always runs; no mtime/size shortcuts anywhere. |
| Edit to a composed/`use`d/refined component while entry file unchanged | Expansion inlines components into the AST at parse time → AST hash moves. |
| `--instances`/`--values`/`--depth`/`--engine`/`--k`/`--deadlock`/`--vacuity`/`--property`/`--exclude-property`/`--strict-tags` change | Each is an explicit key component; bounds overrides are additionally reflected in the AST they rewrite. |
| `--requirements` file edited at same path | Content hash, not path, in the key. |
| fslc upgraded, or dev edits `bmc.py` in an editable install | Version string **and** package source fingerprint in the key. |
| z3 / lark / Python upgraded | Versions in the key. |
| Key computed over an under-specified value (new AST node type, new option type) | Canonical encoder raises on unknown types → run is uncacheable; new `run_verify` parameters are caught by the key-completeness test (§8). |
| Stale line numbers / quoted source in diagnostics | `loc` is inside the hashed AST; entry-file text is hashed; property items hash includes `loc`. |
| Cross-depth reuse serving a wrong verdict | Only `violated` results, only when `violated_at_step ≤` requested depth; argument in §5 shows behavioral equivalence, not just soundness. |
| Property reuse masking an interaction between properties | BMC invariant checks are independent (verified in §6 against the actual solver structure); induction, where they are *not* independent, is excluded. |
| Corrupt/truncated/concurrently-written cache file | Atomic writes; JSON decode failure ⇒ miss + delete. |
| Cache layer bug of any other kind | `FSLC_CACHE_VERIFY=1` recompute-and-compare mode in CI; `--no-cache`/`FSLC_CACHE=off` escape hatches; layer wraps everything in try/except that degrades to an uncached run and never alters exit codes. |
| Deliberate local tampering with entry files | Out of scope — same trust boundary as editing the installed `fslc` itself. |

Residual honesty: Z3 model values in traces are not guaranteed bit-stable across runs, so a
cached trace may differ from what a fresh run would print — both are valid counterexamples of
the same class; the verdict, violated step, and property name are stable. This is the same
nondeterminism the tool already has across z3 versions.

## 8. Tests and benchmark

- `tests/test_verify_cache.py` — hit path: second identical `run_verify` returns
  `cache.hit == true` and does **not** invoke the engine (monkeypatch `bmc.verify`/`prove` to
  fail after priming); envelope equality modulo `cache` field.
- Negative (the soundness protectors): one-character invariant edit misses; component-file
  edit under an unchanged compose entry file misses; each option flip misses (parametrized
  over the full §2.5 list); requirements-content edit misses; fingerprint change misses;
  comment-only edit misses (documents the conservative choice); corrupt entry ⇒ miss without
  crash; `--no-cache`/`FSLC_CACHE=off` never read nor write; `run_mutate` leaves the cache
  directory empty.
- Key-completeness guard: introspect `run_verify`'s signature and assert every parameter is
  either a declared key component or on an explicit allowlist of non-semantic parameters —
  a new parameter fails the test until classified.
- Cross-depth: prime violated-at-3 with depth 8 ⇒ depth 12 served from cache; depth 2 runs
  the engine.
- Property reuse (stage 2): two-invariant spec, edit one ⇒ engine called with the other in
  `exclude_property_names`, merged result lists both in `invariants_checked`; edit an action
  ⇒ no reuse.
- `FSLC_CACHE_VERIFY=1` itself is exercised by one dedicated unit test
  (`test_fslc_cache_verify_mode_detects_a_forced_divergence`) rather than a
  separate CI job: the mode's whole point is "also re-run the engine and
  compare", which is the opposite of what the "engine must not be called on
  a hit" tests assert, so running the *existing* suite under it would not
  add coverage — it would just make those tests' engine-call assertions
  meaningless. A corpus-wide paranoid CI job (verify every example spec
  twice, cached and fresh, and diff verdicts) is a reasonable follow-up but
  is out of scope here.
- Benchmark (acceptance): `tools/bench_verify_cache.py` — N repeated verifies of
  `specs/order_workflow.fsl`-class specs, report cold vs warm wall time (expected: warm ≈
  parse+build_spec only). Not a pytest assertion (timing flakiness); the "engine not called"
  test is the mechanical guarantee.

## 9. Non-goals

- No caching for `refine`/`scenarios`/`replay`/`testgen`/`typestate` in v1 (chain's verify
  steps benefit; its refine steps do not — refine caching is a natural follow-up using the
  same key discipline over the spec-pair + mapping).
- No rewriting of cached payloads to fit different bounds (§5), no heuristic "probably
  unchanged" matching, no cross-machine cache sharing.
- No change to any existing JSON field, exit code, or engine behavior when the cache is off —
  and, apart from the additive `cache` field, none when it is on.

## Rollout

Stage 1: whole-verdict cache + cross-depth violated reuse (this doc §1–5) — ships first.
Stage 2: BMC property-level reuse (§6) behind `FSLC_CACHE_PROPS=1` until benchmarked, then
default. Stage 3 (deferred): induction lemma reuse via `assume_invariants`.
