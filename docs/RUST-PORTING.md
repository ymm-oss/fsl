# fslc Rust porting method and evidence gates

This is the execution procedure for issue #195. The accepted architecture is
in [`DESIGN-rust-port.md`](DESIGN-rust-port.md); this document defines how a
rewrite slice earns equivalence rather than merely compiling.

## 1. Authority order

During the migration, evidence is interpreted in this order:

1. language and CLI contracts in `docs/LANGUAGE.md`, design notes, and tests;
2. observable Python behavior, including exit status and emitted bytes;
3. the Python post-frontend tuple AST exported by `tools/export_ast.py`;
4. Python implementation structure, used only to explain the behavior above.

Python is a reference implementation and arbiter, not a template that Rust must
copy line for line. A disagreement with Python is a migration failure unless a
language/CLI contract proves Python wrong and both implementations plus their
goldens are deliberately changed in the same commit.

## 2. Rewrite loop

Every vertical slice follows the same loop:

1. **Inventory the seam.** Record inputs, outputs, errors, locations, ordering,
   and external reads. Do not combine semantic cleanup with translation.
2. **Freeze reference observations.** Add positive, negative, and boundary cases
   to the smallest suitable differential fixture. Generate expected behavior by
   running Python; do not transcribe large expected ASTs by hand.
3. **Write a typed Rust representation.** Legacy tuple AST and JSON dictionaries
   are compatibility projections at the boundary, not the internal Rust model.
4. **Run the differential gate.** Compare the stable contract byte-for-byte or
   field-for-field. Traces are checked by semantic replay when witnesses are not
   unique.
5. **Triangulate semantics.** A solver result alone is insufficient. Compare the
   Rust symbolic evaluator, Rust Monitor, solver-free Rust BFS, retained Python
   BFS, and cross-implementation replay as soon as the slice reaches them.
6. **Promote only the covered surface.** A crate compiling does not advance a
   phase. The phase table in `DESIGN-rust-port.md` advances only when its gate is
   green over the declared corpus.

The translation order follows semantic dependency rather than Python file size:

```text
tokens/locations
  → expressions/types/binders
  → declarations and surface AST
  → dialect/file lowering
  → typed kernel model
  → concrete runtime + BFS
  → solver trait/backend + BMC
  → CLI/envelope
  → refinement/liveness/tools
  → Web Worker/WASM
```

This keeps failures attributable. For example, BMC work cannot hide a parser
drift because kernel AST parity is already a prerequisite.

## 3. Differential gates

| Gate | Reference side | Rust side | Equality rule |
|---|---|---|---|
| expression AST | `tools/export_ast.py --expr` | `fsl-parse-expr` | exact canonical JSON |
| corpus kernel AST | `tools/export_ast.py --corpus` | future Rust corpus exporter | exact AST and locations; explicit evidence-only entries |
| verdict snapshot | `tests/test_corpus_snapshot.py` | Rust CLI output projection | exact canonical JSON |
| full envelope | Python CLI | Rust CLI/Worker shared emitter | structural equality plus reviewed allowlist |
| counterexample | producing implementation | other implementation's Monitor | actions replay and reproduce the reported violation |
| evaluator | concrete reachable state | symbolic state pinned to it | expression values equal |
| verifier | solver-free BFS | BMC | decision table in `tests/oracle.py` |

`tools/export_ast.py` records every `.fsl` under `specs/` and `examples/` as
`ok`, `error`, or `evidence_only`; dropping a file is itself a test failure.
`tools/inventory_rust_port.py` freezes the measured frontend/top-level/node
surface in `rust/phase0-inventory.json`, so a Python language addition cannot
silently expand the port's moving target.

An allowlist may cover only nondeterministic witness content and parser wording.
Each entry must name the exact JSON path, the source of nondeterminism, and the
test that semantically validates the difference. Verdicts, source locations,
exit codes, assurance classes, missing fields, and file-resolution behavior are
never allowlisted.

## 4. Phase-0 decisions and evidence

Status as of 2026-07-12:

| Decision / spike | Evidence | Status |
|---|---|---|
| shared Z3 release | Python reports 4.16.0; official npm package is pinned by lockfile to 4.16.0 | **decided: 4.16.0** |
| integer representation | corpus uses bounded small domains; Rust lexer rejects literals outside `i64` instead of wrapping | **decided: checked `i64`**, revisit only with a corpus counterexample |
| expression parser | 20 cross-feature cases compare Rust typed AST's Python projection with `parse_expr` | **20/20 exact parity** |
| kernel `spec` surface parser | typed declarations/statements/actions/properties over every kernel spec in the corpus | **79/79 exact parity** |
| refinement surface parser | typed maps, nested conditional/struct expressions, action correspondence, stutter, and progress preservation | **26/26 exact parity** |
| business surface parser | typed actors, processes, controls, policies, and goals | **7/7 exact parity** |
| governance surface parser | typed authority, delegation, satisfaction, and preservation declarations | **1/1 exact parity** |
| requirements surface parser | typed requirements/actions/branches, acceptance/forbidden scenarios, process profile, time/deadline, and implements mappings | **36/36 exact parity** |
| compose surface parser | typed imports, synchronized actions, internal actions, and shared kernel declarations | **2/2 exact parity** |
| database surface parser | typed database, migration, artifact, environment, flag, and compatibility-check IR | **22/22 exact parity** |
| domain surface parser | typed aggregate, effect, await, saga, projection, and raw domain-expression IR | **4/4 exact parity** |
| AI component surface parser | typed tools, authority, fallback, and hard-check IR | **1/1 exact parity** |
| shared parse-error location | intentional missing-expression gallery case | **1/1 kind + line/column parity** |
| complete parseable surface corpus | all six shared top levels plus db/domain/AI-component frontends | **178/178 exact valid AST parity; 1/1 parse error; 2 evidence-only files explicitly inventoried** |
| spec/compose kernel lowering | capture-safe named predicate expansion, entity/number verify-bound lowering, and resolver-backed compose namespacing/synchronization in `fsl-core` | **81/81 exact kernel AST parity** |
| typed kernel + concrete runtime | typed finite domains, options, structs, maps, sets, sequences, relations, simultaneous actions, and a solver-free BFS oracle | **Python BFS = Rust BFS for all 20 monitorable `specs/` cases at depth 3** |
| backend-neutral solver contract | synchronous typed term construction with async-only checks; native implementation pins `z3` 0.20.2 / Z3 4.16.0 | **term, array, substitution, stack, assumptions, unsat core, and model projection tests pass** |
| native symbolic BMC | incremental push/check/pop verifier over the shared typed kernel, with finite dense structured-state encoding | **Python BFS = Rust BFS = Rust BMC for all 20 `specs/` cases at depth 3** |
| witness replay | native violation/reachable/deadlock witnesses are replayed before output; cross-language gates replay both producer directions | **Rust BMC → Rust/Python Monitor and Python BMC → Rust Monitor for all 20 cases at depth 3** |
| native CLI stable projection | `check`/`verify` native envelope projected through the existing corpus snapshot contract | **23/23 `specs/` files at declared depth 5** |
| native CLI full envelope | all `check`/`verify` fields, diagnostics, locations, warnings, and assurance routing with reviewed witness-only normalization | **43/43 `specs/` command envelopes at depth 5** |
| bounded liveness | deadlock stutter, deadline, fair-lasso `leadsTo` checking plus cross-implementation witness replay | **3/3 focused passing/adversarial/violating cases at depth 5** |
| k-induction | bounded base, implicit/user invariants, multi-k CTIs, and explicit ranking proof/failures | **11/11 focused full-envelope cases** |
| refinement | bounded action simulation, stutter, guard/state mismatch, and pulled-back progress preservation | **6/6 focused full-envelope cases** |
| scenarios | reachable, leadsTo response, action-cover, and deadlock scenarios with concrete replay | **20/20 identity parity; 86 Rust scenarios replayed by Python and Python scenarios replayed by Rust at depth 5** |
| public replay command | native `replay --trace` accepts array and `{events:[...]}` forms and reports first rejection | **3/3 conformant/nonconformant contract cases** |
| Phase-2 command surface | sweep, direct refinement chains, and project manifests including short-circuit/keep-going | **5/5 focused full-envelope parity** |
| Phase-3 native surface | DB/AI/domain engines plus mutate, explain, testgen, typestate, HTML, ledger, analyze, and semantic diff | **107/107 command cases; exact typestate, full direct/requirements-profile built-in and external mutation adjudication with acceptance/forbidden/refinement and by-requirement attribution, native invariant/reachable counterfactual diagnostics with source-backed blame, byte-identical pytest plus five alternate testgen targets (positive and forbidden cases), core/standalone-refinement/project traceability graph JSON plus DOT/Mermaid exports, declaration-level tag-review export, mixed spec/refinement and AI-review batch analysis, project missing-anchor and structural/progress-cycle/unconstrained-effect/conservation/acceptance-divergence AI-review findings, byte-identical focused HTML/ledger artifacts, broad HTML static/structural parity, and byte-identical raw stdout for version, typestate, testgen, HTML, ledger, readable explain, domain expansion, and domain conformance generation; solver-selected dynamic witnesses are checked by bidirectional replay rather than unstable raw bytes** |
| full surface corpus | generic `check` and `verify --depth 3` over every `.fsl` in `specs/` and `examples/` | **181/181 stable verdict projections and exit statuses match; no Python command fallback** |
| production browser Worker | Rust WASM + official npm Z3 bridge, isolated Worker, in-memory files, progress, forced cancellation, and playground assets | **verified and violated samples match native CLI; cancellation recreates a healthy solver context** |
| npm Worker round trip | disposable Node Worker returns `sat`, model `x=42`, and can terminate Emscripten threads | **proved for Node Worker** |
| browser Worker asset loading / COOP+COEP | isolated Chrome loaded the bundled Worker, separate Emscripten JS/WASM and pthread script with no console errors; `crossOriginIsolated=true`, `sat`, model `x=42` | **proved in browser** |
| JS term bridge | observed roughly 29k–42k terms/s at 1,000 terms and 60k terms/s at 10,000 terms on the development machine | **start with typed per-term calls** |
| v1 native scope | issue #195 requests a full replacement, including db/AI/domain and report generators in Phase 3 | **decided: no permanent Python command fallback** |
| CI and native targets | PR parity runs Rust stable + Python 3.12 + Node 22 + Chrome on Ubuntu; main/scheduled runs add native Z3 tests on Linux, macOS, and Windows | **implemented for the current kernel slice** |

The Phase-0 syntax, dependency, browser, and decision gates are complete. The
Phase-1 semantic kernel is complete: resolver-backed compose lowering, the typed
model, independent concrete runtime/BFS, the native Z3 backend, bounded safety
and liveness verification, full native CLI envelopes, replay, scenarios,
snapshot projection, and bidirectional counterexample gates all pass their
declared corpora. Phase 2 is complete without changing these Phase-1 contracts.
Phase 4's production Worker and cancellation/native-verdict gates are also
complete. Phase 3 is complete: its large raw-output/report bodies and stable
projections pass command-by-command parity without a Python command fallback.
The native release binary is the primary `fslc` distribution; the Python package
remains available as the parity oracle and optional LSP implementation.

The batching decision is evidence-driven: retain per-term calls unless profiling
representative Phase-1 corpus verification shows that JS term construction is at
least 30% of end-to-end wall time. If it crosses that threshold, an SMT-LIB batch
path may exist inside `fsl-solver-z3js`; it must implement the same solver trait
semantics and pass every cross-backend gate.

The supported native release matrix is
`x86_64-unknown-linux-gnu`, `aarch64-apple-darwin`,
`x86_64-apple-darwin`, and `x86_64-pc-windows-msvc`. Pull requests keep one
fast Ubuntu Rust/parity/browser job. Once `fsl-solver-z3` lands, pushes to main
and releases add Linux/macOS/Windows native jobs; browser parity remains an
Ubuntu headless-Chrome job. A target is not advertised until the native Z3
backend and CLI smoke corpus run on that target in CI.

## 5. Commands

```bash
# Python reference AST is deterministic and covers the whole corpus.
PYTHONPATH=src python tools/export_ast.py --corpus --compact > /tmp/fsl-python-ast.json
PYTHONPATH=src python tools/inventory_rust_port.py --check rust/phase0-inventory.json

# Rust syntax checks and live Python↔Rust expression differential.
cd rust
cargo fmt --all -- --check
cargo clippy --workspace --all-targets --locked -- -D warnings
cargo test --workspace --locked
cargo build --workspace --locked
cd ..
PYTHONPATH=src python tools/check_rust_ast_parity.py
PYTHONPATH=src python tools/check_rust_surface_parity.py
PYTHONPATH=src python tools/check_rust_kernel_parity.py
PYTHONPATH=src python tools/check_rust_bfs_parity.py --depth 3
PYTHONPATH=src python tools/check_rust_bmc_parity.py --depth 3
PYTHONPATH=src python tools/check_rust_cli_snapshot.py
PYTHONPATH=src python tools/check_rust_full_envelope.py --depth 5
PYTHONPATH=src python tools/check_rust_leadsto_parity.py --depth 5
PYTHONPATH=src python tools/check_rust_induction_parity.py
PYTHONPATH=src python tools/check_rust_refinement_parity.py
PYTHONPATH=src python tools/check_rust_scenarios_parity.py --depth 5
PYTHONPATH=src python tools/check_rust_replay_parity.py
PYTHONPATH=src python tools/check_rust_corpus_cli_parity.py --depth 3

# Official npm backend spike.
cd rust/spikes/z3js-worker
npm ci
npm run probe
npm run probe:browser
npm run bench
```

Generated measurements are evidence, not committed goldens: timing varies by
machine. The committed contract is the package/version lock, successful solver
round trip, model value, and the explicit performance decision threshold.
