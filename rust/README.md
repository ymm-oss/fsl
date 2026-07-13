# fslc Rust workspace

This workspace is the native implementation for issue #195. Phases 0–4 are
complete over their declared differential, replay, report, and browser gates;
the Rust CLI and Worker replace the Python execution path without a permanent
Python command fallback.

Current covered slice:

- `fsl-syntax`: typed source locations, lexer, Pratt expression parser, and
  typed kernel `spec` surface declarations/statements/actions/properties;
- exact Python↔Rust expression AST projection parity;
- exact surface AST parity for all 79 kernel specs in the corpus;
- exact surface AST parity for all 26 refinement mappings in the corpus;
- exact surface AST parity for all 36 requirements, 7 business, 2 compose,
  and 1 governance documents in the corpus;
- exact typed specialized-IR parity for all 22 database, 4 domain, and 1 AI
  component documents in the corpus;
- `fsl-core`: capture-safe named predicate and entity/number domain lowering,
  plus resolver-backed compose lowering, with exact kernel AST parity for all
  81 corpus spec/compose documents;
- `fsl-runtime`: solver-independent concrete evaluation, Monitor semantics, and
  BFS parity with Python for all 20 monitorable `specs/` programs at depth 3;
- `fsl-solver` and `fsl-solver-z3`: the async-check backend contract and pinned
  native Z3 4.16.0 implementation;
- `fsl-verifier`: incremental symbolic BMC with Python BFS = Rust BFS = Rust BMC
  decision parity for all 20 `specs/` programs at depth 3, plus deadlock,
  deadline, and fair-lasso bounded `leadsTo` checks;
- `fslc-rust`: native `check`, `verify`, and `scenarios` slices with 23/23
  stable `specs/` snapshot projection parity, 43/43 full command-envelope parity,
  and bidirectional replay of BMC, liveness, and scenario witnesses;
- native induction, refinement, sweep, project-chain, database compatibility,
  SQL/Prisma import, AI hard-contract/stochastic evidence, domain structural
  checks/replay/scaffolds, and the report-tool command surface; the committed
  Phase-3 differential matches 107/107 command cases. It covers complete
  typestate, full built-in/external mutation adjudication, invariant and
  reachable counterfactuals with source-backed blame, all five alternate
  testgen targets, core/refinement/project analysis projections and exports,
  tag-review, batch and AI-review profiles, byte-identical focused report
  artifacts, and broad HTML byte-identical static content plus full
  tag/attribute structure. Solver-selected dynamic witnesses are validated by
  bidirectional replay instead of unstable raw bytes;
- `fsl-wasm`: production `wasm32-unknown-unknown` Worker using the official
  `z3-solver` 4.16.0 npm backend, with COOP/COEP playground assets, forced
  cancellation/reinitialization, and native CLI verdict parity in headless
  browser CI;
- parse-error kind and line/column parity for the shared invalid gallery case;
- frozen Python surface inventory in `phase0-inventory.json`; and
- `spikes/z3js-worker`: pinned native-Node and browser Web Worker probes for
  the official `z3-solver` 4.16.0 package.

All phases are complete over their declared gates. Native `replay --trace`
matches the focused conformant and first-rejection contracts, and the complete
gate matrix is documented below and in the porting guide.

See [`../docs/RUST-PORTING.md`](../docs/RUST-PORTING.md) for the rewrite method,
evidence gates, and current decisions.
