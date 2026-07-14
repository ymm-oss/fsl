# fslc Rust port: native CLI and browser WASM architecture

Status: accepted migration design (issue #195). This document does **not** claim
that a Rust implementation exists. Each phase below has an explicit parity gate;
until that gate passes, the Python implementation remains authoritative for that
surface.

The objective is to produce two distribution targets without changing the FSL
language or its evidence contract:

1. a single-binary native CLI; and
2. a `wasm32-unknown-unknown` kernel running inside a browser Web Worker.

The browser backend calls the official `z3-solver` npm WASM package. A Rust Z3
crate is used only for the native backend because the Rust bindings do not target
`wasm32`.

## 1. Goals, non-goals, and measured constraints

The port must preserve:

- the current JSON envelope, field ordering rules, and exit codes;
- snapshot-projected verdict parity across the corpus;
- the independent Monitor evaluator and solver-free BFS oracle;
- trace replay between implementations; and
- the language accepted by the current parser and dialect frontends.

Version 1 does not add language syntax, output fields, a `wasm32-wasi` CLI, or a
new SMT encoding. Browser-side static linking of native Z3 is explicitly out of
scope.

The design is based on the repository measured on 2026-07-11: `src/fslc` is
approximately 32.9k lines, the grammar is a roughly 150-rule Lark Earley grammar,
and the verifier uses a deliberately small Z3 surface. Only integer, Boolean, and
array sorts are required. The implementation does not depend on Optimize,
tactics, native existential quantification, uninterpreted functions, timeouts, or
random seeds. User quantifiers are finite-domain expansions; native universal
quantification is limited to collection-bound constraints.

These are design inputs, not permanent claims. Phase 0 must regenerate the
inventory before code is ported, and any newly observed API surface must be added
to the abstraction deliberately.

## 2. Workspace and dependency boundaries

The Rust implementation lives in a repository-local `rust/` Cargo workspace so
both implementations can share the corpus, snapshots, and parity harness.

```text
rust/
├── fsl-syntax          lexer, parser, AST, source locations
├── fsl-core            build_spec, types, dialect lowering, diagnostics, resolver
├── fsl-runtime         Monitor, replay, solver-free BFS oracle
├── fsl-solver          backend-neutral SMT trait
├── fsl-solver-z3       native Z3 backend
├── fsl-solver-z3js     wasm-bindgen bridge to z3-solver npm
├── fsl-verifier        BMC, induction, refinement, scenarios
├── fsl-tools           explain, mutate, testgen, reports, analysis
├── fslc                native CLI
└── fsl-wasm            Web Worker API and shared envelope emitter
```

The critical dependency direction is:

```text
fsl-runtime → fsl-core → fsl-syntax

fsl-verifier → fsl-solver
             ↘ fsl-core
```

`fsl-runtime` must not depend directly or transitively on `fsl-solver`, Z3, or a
JavaScript bridge. CI must inspect the Cargo dependency graph and fail if this
boundary is crossed. That makes evaluator independence a structural invariant,
not merely a coding convention.

## 3. Parser and file-resolution strategy

Lark Earley is not mechanically replaced with an LALR generator. The Rust parser
uses recursive descent for declarations/statements and Pratt parsing for
expressions. The existing precedence cascade is the contract:

```text
quantifier → implies → or → and → not → is → compare
           → sum → product → unary → postfix → atom
```

Every AST node retains line and column information. `kind: "parse"` and `loc`
must match the Python result for the corpus; detailed parser messages and expected
token lists may differ only where the parity allowlist says so.

Issue #247 makes the native `fsl-syntax::parse_document` token registry the
authority for document dispatch. Native CLI/library/tool/Worker entrypoints use
that API (or its compatibility adapter) and specialized frontends borrow its one
token stream. The retained Python LSP cannot link the Rust crate, so its parser
selection is explicitly a compatibility adapter with the same ordered keys and
significant-token rules; parity tests, not independent language authority, keep
it aligned. See `DESIGN-token-registry.md`.

Phase 0 adds a temporary Python AST JSON exporter and compares the Rust AST with
the tuple AST for every corpus file. Dialect expansion remains a pure AST-to-AST
stage before `build_spec`, matching the current requirements, business,
governance, compose, database, AI, and domain frontends.

The native domain frontend enforces this boundary concretely: its unresolved
typed syntax is name/type-resolved in `fsl-core` and lowered directly to Kernel
surface AST. Generated Kernel text is available for inspection but is not an
input to parsing, checking, runtime execution, or verification.

File access is behind a `FileResolver` interface:

- native CLI: filesystem-backed, with the current source-relative semantics;
- browser: an in-memory map supplied with the request.

No verifier or dialect component may open a path outside this interface. This is
required for `use ... from` and `implements ... from` parity in a Web Worker.

## 4. Solver interface

The abstraction exposes only the measured operations. It is intentionally not a
generic mirror of the Z3 API.

```rust
enum Sort {
    Bool,
    Int,
    Array(Box<Sort>, Box<Sort>),
}

enum Value {
    Int(i64),
    Bool(bool),
}

enum SatResult {
    Sat,
    Unsat,
    Unknown,
}

trait Smt {
    type Term: Clone;

    // Synchronous term construction.
    fn int_val(&self, value: i64) -> Self::Term;
    fn bool_val(&self, value: bool) -> Self::Term;
    fn constant(&self, name: &str, sort: &Sort) -> Self::Term;
    // Arithmetic, comparisons, Boolean operators, ite, select/store,
    // constant arrays, bounded forall, substitution, and sort inspection.

    fn push(&mut self);
    fn pop(&mut self);
    fn assert(&mut self, term: Self::Term);
    fn assert_and_track(&mut self, term: Self::Term, label: &str);
    async fn check(&mut self) -> SatResult;
    async fn check_assuming(&mut self, terms: &[Self::Term]) -> SatResult;
    fn unsat_core(&self) -> Vec<String>;
    fn model_eval(&self, term: &Self::Term) -> Value;
    fn version(&self) -> String;
}
```

Only `check` and `check_assuming` are asynchronous. The npm implementation uses
Asyncify at those calls; native Z3 returns an immediately-ready future. The
verifier preserves the existing incremental solver shape: transition constraints
are retained across depths and property probes use push/assert/check/pop.

Integer values use `i64` with checked arithmetic. Overflow is an error, never a
silent wrap. Division and remainder preserve SMT-LIB Euclidean semantics. Native
and browser Z3 releases must be pinned to the same version; the concrete release
is a Phase 0 decision. The verification cache key includes both solver version
and backend identity.

## 5. Dual evaluator and state representation

The Rust Monitor and symbolic verifier preserve the same physical layout:

- `__present` / `__value` for options;
- `__data` / `__len` for sequences;
- `__<field>` for structs; and
- dense finite-domain representations for sets, relations, and maps.

Enums remain integer ordinals. The physical-to-logical conversion preserves the
trace schema `{step, state, action:{name,params}, changes}`.

Correctness is triangulated, never inferred from agreement between two code paths
that share a solver:

1. Rust Monitor versus Rust symbolic expression evaluation;
2. Rust solver-free BFS oracle versus Rust BMC;
3. retained Python oracle versus Rust BMC; and
4. counterexamples replayed by the other implementation's Monitor.

## 6. JSON and process contract

CLI and Worker use one envelope emitter. It must preserve the current observable
contract, including non-obvious details:

- `"fsl":"1.0"` is first, JSON is indented by two spaces, and non-ASCII text is
  emitted without ASCII escaping;
- `faithfulness` defaults are applied recursively to nested result dictionaries;
- `trace_type` is a top-level trailing default;
- exit codes 0/1/2/3 retain their current meaning;
- `mutate` always exits zero;
- `chain` keeps its separately assembled envelope and stderr table behavior; and
- raw-output modes remain raw rather than being wrapped opportunistically.

Snapshot parity alone is not enough because corpus snapshots intentionally omit
non-deterministic traces. The migration harness therefore has three layers:

1. exact existing snapshot projection;
2. full-envelope structural diff, with a small reviewed allowlist for trace
   content and parser wording; and
3. bidirectional trace replay to validate non-unique Z3 witnesses semantically.

Any new allowlist entry requires a reason tied to a known nondeterminism. Verdict,
location, assurance, and exit-code differences are never allowlisted.

## 7. Browser execution

`fsl-wasm` is loaded only inside a dedicated Web Worker. Its protocol is:

```text
request:  {id, cmd, source, files?, options}
response: {id, envelope}
progress: {id, progress}
```

The browser exposes an explicit kernel-command allowlist. Commands requiring
subprocesses (`chain`) or host file output (`-o`) are not supported in version 1.

The official `z3-solver` npm package performs term construction and model
evaluation synchronously, while checks are asynchronous. It requires
`SharedArrayBuffer`, hence COOP/COEP isolation. GitHub Pages cannot set arbitrary
response headers, so a future playground must ship and register
`coi-serviceworker` under `docs/intro/`. Z3 is loaded lazily and cached because
the unpacked package is approximately 33 MB.

There is no supported in-flight interrupt API. Cancellation terminates the
entire Worker and creates a fresh Worker/Z3 context. Depth-loop progress is sent
between checks. After a completed run, the bridge terminates residual Emscripten
threads where available.

## 8. Phase gates

No phase is complete merely because its crates compile.

| Phase | Scope | Required gate |
|---|---|---|
| 0: risk spikes | Z3 npm Worker round trip; Rust parser; JS bridge throughput | term→check→model works in Worker; corpus AST diff is understood; backend strategy decided |
| 1: kernel | syntax/core/runtime/verifier BMC; check, verify, replay, scenarios; envelope | `specs/` snapshot projection parity, oracle parity, and bidirectional replay |
| 2: full language | induction, refine, requirements/business/governance, compose, sweep, chain | full corpus projection parity and envelope diff gate |
| 3: tools/frontends | mutate, explain, testgen, typestate, HTML, acceptance, ledger, analyze, db/AI/domain | command-by-command parity plus Rust regression suite |
| 4: browser | wasm package, Worker, npm solver bridge, playground support | browser sample corpus returns the same verdict as native CLI and cancellation recovers cleanly |

Phase 0 must answer four recorded decisions before Phase 1 implementation is
approved:

1. the exact shared Z3 release;
2. whether bridge throughput requires batched SMT-LIB assertions;
3. the first-release boundary for database/AI/domain and large report generators;
4. the Cargo CI jobs and supported native target matrix.

The proposed `i64` checked representation is accepted unless corpus analysis in
Phase 0 produces a concrete counterexample.

## 9. Migration policy

Phases 0-3 are complete. The native Rust implementation is now authoritative;
Python is a frozen compatibility/reference and optional LSP surface, as enforced
by `.github/workflows/ci.yml`. New CLI and semantic work is not backported to
Python. Cross-implementation fixtures remain useful historical evidence but no
longer define the evolving compiler boundary.

External tools must depend on the versioned normalized Kernel JSON described in
[`DESIGN-kernel-contract.md`](DESIGN-kernel-contract.md), not either
implementation's private AST. Rust Monitor/symbolic transition agreement and the
language-neutral conformance vectors replace dual active implementation as the
mechanical semantic gate for this boundary.

The following conditions block migration rather than being accepted as temporary
drift:

- runtime acquiring any solver dependency;
- a verdict or exit-code mismatch;
- a non-replayable cross-implementation counterexample;
- browser cancellation leaving subsequent requests in a corrupt solver state;
- an unreviewed parity allowlist expansion; or
- changing public Kernel semantics without updating its schema, conformance
  vectors, Monitor/verifier agreement gate, and compatibility documentation.

## 10. Risk controls

| Risk | Control |
|---|---|
| Earley and recursive-descent acceptance differ | corpus AST diff plus grammar-derived fuzzing in Phase 0 |
| JS calls dominate term construction | measure before Phase 1; allow a backend-internal batched SMT-LIB path only if required |
| Z3 witnesses differ | compare verdict/envelope structure and replay traces instead of byte-comparing traces |
| npm solver cannot be interrupted | terminate and recreate the Worker and solver context |
| browser payload is large | lazy loading and Service Worker caching |
| Python evolves during port | dual-implementation change policy and permanent parity CI |

This design intentionally separates architectural acceptance from implementation
progress. Follow-up implementation PRs should cite the phase and gate they move;
they must not describe the Rust port as complete before the corresponding gate
passes.
