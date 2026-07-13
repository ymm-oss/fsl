# FSL Explicit-State Engine — Implementation Design (`--engine explicit`)

This document is an implementation-level specification of `--engine explicit`
(issue #212): a Z3-free verification engine that exhaustively enumerates the
concrete state space of a spec. Because every FSL domain is bounded and every
quantifier is finite-domain, breadth-first exploration of concrete states is
*equivalent* to BMC up to depth `k`, and reaching closure (no new states) is a
*complete proof* of all invariants — no induction, no lemmas.

The engine is **Rust-native only** (`rust/`), like `fslc approval`: the frozen
Python reference implementation is intentionally unchanged. The Python tree
still supplies the *semantic* reference — `runtime.py Monitor` and the BFS
oracle `tests/oracle.py` are the specification this engine productizes.

## 1. Goals and non-goals

- **Goals**:
  - A verdict engine that is orders of magnitude faster than Z3-based BMC on
    the small-state-space specs that dominate the corpus (state counts in the
    thousands): `violated` with the shortest concrete counterexample,
    `verified` (bounded, BMC-equivalent), and — when exploration closes —
    `proved` (unbounded, subsumes k-induction for finite systems).
  - A **third independent evaluator** of FSL semantics next to the symbolic
    BMC (`bmc.py`) and the concrete step interpreter (`runtime.py Monitor`),
    strengthening the cross-check regime rather than weakening it.
  - Fail-closed truncation: exceeding the state budget yields an explicit
    `unknown_budget` verdict, never a silent `verified`.
- **Non-goals (this design)**:
  - `--engine auto` (explicit first, fall back to symbolic on blow-up) — a
    follow-up once explicit is trusted.
  - Symbolic-side scaling work (symbolic action parameters, cone-of-influence
    slicing) — the explicit engine does not replace the Z3 path; it owns the
    small-state-space regime, the symbolic engines own the rest.
  - SAT bit-blasting and GPU exploration — revisit only when real specs exceed
    ~10⁹ states.
  - Distributed/parallel exploration — the first version is single-threaded in
    both implementations.

## 2. Algorithm

Standard explicit-state reachability with level-synchronous BFS:

```
frontier := canonical initial states (deduplicated)
seen     := frontier
for level in 0..depth:
    check state properties on every state in frontier   # invariants, reachable, deadlock
    if violation found: return violated (trace via parent links; BFS ⇒ shortest)
    next := {}
    for s in frontier, for each enabled action instance a in s:
        s' := step(s, a)          # concrete Monitor semantics
        check edge properties     # ensures
        if s' not in seen: add to seen and next
    if next is empty: return proved (closure)            # complete reachability
    frontier := next
if budget exceeded at any point: return unknown_budget
return verified                                          # depth exhausted, frontier non-empty
```

Key points:

- **State identity.** A state is the full concrete valuation of all state
  variables, canonicalized the same way as `tests/oracle.py` (`state_key` /
  `normalize`) so that dedup is semantic, not representational.
- **Shortest counterexample.** BFS explores by depth level, so the first
  violation found is at minimal depth; `violated_at_step` is the BFS level.
  This matches the BMC contract (`bmc.py` returns the earliest violating
  step of its incremental unrolling).
- **Closure ⇒ proof.** When a level produces no unseen states, `seen` is the
  full reachable set. Invariants checked on every member of `seen` therefore
  hold in *every* reachable state: `result: "proved"`, the same verdict word
  the induction engine uses, plus `closure: true` and exploration stats.
  For specs where k-induction returns `unknown_cti` (the invariant is true but
  not inductive), explicit closure proves it without lemmas.
- **Deterministic init (fail closed).** The concrete interpreter requires
  init to definitely assign every state variable
  (`runtime.py _check_deterministic_init`); specs with underconstrained init
  are rejected at Monitor construction (kind `semantics`, §5). Symbolic BMC
  treats unassigned init variables as free — a strictly larger initial set —
  so the explicit engine must never silently explore only the default-seeded
  state: any spec it accepts has exactly one initial state, and everything
  else is an explicit error. The Rust port must enforce the same gate.

## 3. Verdict and JSON contract

The result dict is shape-compatible with `bmc.verify` output so that
`cli._envelope`, `exit_code()`, `fslc replay`, and downstream tooling work
unchanged:

| Outcome | `result` | Exit | Notes |
|---|---|---|---|
| Violation found | `violated` | 1 | Same trace JSON schema as BMC; `violated_at_step` = BFS depth |
| Depth exhausted, frontier non-empty | `verified` | 0 | Bounded claim, identical strength to BMC `verified` |
| Closure reached, no violation | `proved` | 0 | Unbounded claim; `closure: true` |
| State budget exceeded | `unknown_budget` | 1 | Explicit truncation; never reported as verified |
| Unsupported spec feature | `error` (kind `semantics`) | 2 | Fail closed, see §5 |

Every result carries exploration stats alongside the standard
`cost.elapsed_s`: states explored, maximum frontier width, and whether closure
was reached. `unknown_budget` additionally reports the depth reached when the
budget ran out.

## 4. Property semantics

The engine checks the same property surface as `bmc.verify`, with identical
verdict semantics:

- **Invariants** — evaluated on every visited state (including initial
  states, level 0).
- **`ensures`** — evaluated on every explored transition edge.
- **`reachable`** — witnessed at the earliest level where the goal holds;
  unreached goals are reported exactly as BMC reports them at depth
  exhaustion. Under `proved` (closure), an unreached `reachable` is
  *definitively* unreachable — stronger than BMC's bounded "not reached within
  depth" — and is reported as `reachable_failed`.
- **Deadlock** — a state with no enabled action instance, honoring
  `deadlock_mode` (`warn`/`error`) with BMC-equivalent behavior.
- **Vacuity/coverage** — an action never enabled anywhere explored, honoring
  `--vacuity` the same way BMC does. Under closure this too is definitive.
- **`leadsTo`** — not supported: the concrete runtime has no complete
  liveness decision procedure, so specs with `leadsTo` properties are rejected
  fail-closed (kind `semantics`) unless the property is excluded from the run;
  the error steers the user to `--engine bmc` for the bounded lasso/stutter
  check.

## 5. Support matrix (fail closed)

The engine executes concrete semantics via the ported interpreter
(`fsl-runtime` `Monitor`). Specs using features the concrete interpreter
cannot execute — in particular a nondeterministic init (§2) — are rejected
with the standard JSON error envelope (kind `semantics`, exit 2) and a message
naming the unsupported feature, mirroring the frozen Python reference's
`runtime.py` gates (`_check_deterministic_init` and friends). The engine never
approximates: any spec it accepts is verified under full FSL semantics, any
spec it cannot handle is an explicit error steering the user to
`--engine bmc`/`induction`.

Beyond the definite-assignment rule, `init forall` binder *domains* must not
reference state variables: both reference engines reject non-constant range
bounds (`forall i in 0..n` with `n` a state variable — "ranges must be
compile-time integers") and binders over state collections, so the explicit
engine rejects them too. Accepting them would let the engine concretely
evaluate a domain the symbolic engines call ill-formed, and emit verdicts for
specs the language rejects. Constant bounds (`0..CAP` with `const CAP`) are
resolved before the kernel AST and remain accepted.

## 6. Integration

- **CLI**: `fslc verify <spec> --engine explicit [--explicit-budget N]`.
  `--explicit-budget` caps visited states (default 1,000,000).
  `--from-state` stays BMC-only; `--lemma` stays induction-only; `--k` is
  ignored. Other subcommands keep their `bmc`/`induction` choices for now.
- **Verdict cache**: the persistent verify cache keys already include the
  engine name, so explicit verdicts cache independently of BMC verdicts.
  Cross-depth reuse of `violated` entries applies unchanged.
- **Envelope / exit codes**: `unknown_budget` joins the exit-1 verdict list in
  `exit_code()`; everything else reuses existing vocabulary.

## 7. Soundness argument and gates

The engine's claims reduce to: (a) the concrete step semantics are correct,
and (b) the dedup key captures full state identity. (a) is the already-tested
ported Monitor semantics (`fsl-runtime`) — the same evaluator family the
Python BFS oracle uses to catch Z3-side false negatives. (b) uses the ordered
`State` value itself (`BTreeSet<State>`), the same canonical identity
`fsl_runtime::bfs` already relies on. On top of that, the repo's standing
gates apply:

- Verdict agreement on the corpus: explicit vs the Rust BMC engine (explicit
  `proved` subsumes BMC `verified`; `violated_at_step` equal), as Rust
  integration tests.
- Violated traces must replay through the concrete interpreter
  (`fsl_runtime::replay_trace`) — every counterexample is a checked concrete
  execution.
- The Python corpus snapshot (`tests/test_corpus_snapshot.py`) must be
  byte-identical: the frozen Python tree is untouched.
- The Rust/Python parity harnesses (`tools/check_rust_*_parity.py`) keep
  passing via the established Rust-only-command pattern (as with `approval`).

## 8. Placement in the Rust workspace

The engine lives on the Z3-free path: the exploration core belongs with the
concrete interpreter (`fsl-runtime`, where `bfs`, `find_boundary_violation`,
and `replay_trace` already live), with verdict rendering wired through the
`fslc` crate's existing envelope machinery (`verification.rs` renderers,
`fslc_rust::trace_json`). It must not depend on `fsl-solver`, which also makes
it the cheapest engine to ship to WASM (no Z3 worker, no COOP/COEP
requirement — a later follow-up).

## 9. Performance expectations

For the current corpus (3–5 state variables, tiny domains, state spaces in the
thousands), exploration completes in microseconds-to-milliseconds and replaces
a Z3 session issuing hundreds of incremental `check()` calls — typically a
10²–10⁴× wall-clock win, measured by `tools/bench_explicit.py` (explicit vs
BMC over the corpus). The engine's cost is O(reachable states × transition
degree) with hash-set memory; it deliberately does not try to compete beyond
the budget cap — that regime belongs to the symbolic engines.

## 10. Future work

- `--engine auto`: run explicit first; on `unknown_budget`, fall back to the
  symbolic pipeline transparently.
- Parallel exploration (sharded frontier) if real specs approach the budget.
- WASM exposure of the explicit engine (Z3-free verification in the browser).
- Definitive-verdict upgrades: under closure, report unreachable `reachable`
  goals and never-enabled actions as proofs rather than bounded observations
  (already specified in §4; surfacing this distinction in reports/HTML is
  follow-up polish).
