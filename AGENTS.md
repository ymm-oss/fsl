# FSL repository contract

## Authority and scope

`fslc` is the verifier for FSL. The native Rust workspace under `rust/` is the
authoritative implementation and distribution surface, including `fslc-lsp`.
`src/fslc/` is a frozen Python compatibility reference; add product behavior
there only when a compatibility change explicitly requires it.

Resolve evidence in this order:

1. `docs/LANGUAGE.md`, accepted `docs/DESIGN-*.md`, tests, and CI contracts.
2. Native Rust implementation and public Kernel/JSON contracts.
3. Observable frozen-Python behavior where parity applies.
4. Proposals, task notes, conversations, and agent memory.

Stop on a conflict instead of silently choosing a source. Keep durable
decisions in accepted contracts or executable tests, not temporary reports.

## Work discipline

- Start with `git status --short`, named paths/issues/symbols, and `rg`.
  Read the smallest useful ranges; widen only when evidence requires it.
- Preserve unrelated user changes. Use a dedicated branch/worktree for
  non-trivial work. Do not inspect secrets, unrelated user data, generated
  trees, vendor/build output, snapshots, or full logs without a task-specific
  reason.
- Make the smallest coherent diff. Do not perform opportunistic refactors,
  dependency upgrades, formatting sweeps, or unrelated cleanup.
- `skills/` holds distributable canonical skills; repository-internal workflow
  skills go in `.claude/skills` or `.codex/skills`. `.claude/skills/fsl*` and
  `.agents/skills/fsl*` stay symlinks to the distributable ones.
- For bounded implementation, bug-fix, test, or refactor work, use
  `$focused-change`. It routes soundness-sensitive FSL work to the complete
  repository contract only when needed.
- Use `$task-start` before substantial Codex work and `$checkpoint` before
  compaction, handoff, independent review, or ending the task.
- Delegate broad read-heavy exploration to `evidence_explorer` and independent
  final review to `independent_reviewer`. Keep iterative implementation in the
  main thread unless writes can be isolated.

## Build and verification

Run the working-tree CLI with:

```bash
cargo run --manifest-path rust/Cargo.toml -p fslc-rust --bin fslc -- check specs/cart_v1.fsl
cargo run --manifest-path rust/Cargo.toml -p fslc-rust --bin fslc -- verify specs/cart_v1.fsl --depth 8
cargo run --manifest-path rust/Cargo.toml -p fslc-rust --bin fslc -- verify specs/cart_v1.fsl --engine induction
```

Run the narrowest relevant check first. The Rust CI-equivalent gate is:

```bash
cargo fmt --manifest-path rust/Cargo.toml --all -- --check
cargo clippy --manifest-path rust/Cargo.toml --workspace --all-targets --locked -- -D warnings
cargo test --manifest-path rust/Cargo.toml --workspace --locked
cargo build --manifest-path rust/Cargo.toml --workspace --locked
```

The complete product gate is `./tools/check-native-integration.sh`. Merge
readiness is bounded PR evidence, not a substitute for product verification on
merged `main` or release/production promotion. Python checks are required only
for explicit compatibility-reference or Python-hook changes. Solver changes
also run focused `fsl-solver-z3`, `fsl-verifier`, and `fslc-rust` tests;
semantic changes run the applicable logic/scheduled tier.

Use `./scripts/cx-log <label> -- <command...>` for noisy commands. Keep complete
logs in `.codex/tmp/`; report only exact commands, exit codes, failing tests,
essential lines, and the log path. Do not rerun an unchanged failure without a
new hypothesis, code/config change, or narrower reproducer.

## Non-negotiable correctness

- `fsl-runtime` stays independent of solvers/Z3/JavaScript bridges. Symbolic
  verification, Monitor, and solver-free BFS must agree.
- Preserve native CLI and Worker JSON envelopes, locations, exit codes, replay
  evidence, and public Kernel contracts. Do not allowlist verdict, location,
  assurance, or exit-code drift.
- Do not weaken or hollow out specs to obtain green checks. Every conformance
  anchor needs a rejecting negative control; calibrate mutation/vacuity evidence
  and report produced versus expected results.
- Compare agreement outputs in full. Every excluded field needs an observed,
  written reason and a live exclusion check. Separate stable values from ambient
  cache/environment values and run new controls at least twice.
- Identify the exact commit, binary, worktree, and active mutation behind every
  observation. Evidence measured on another state does not transfer. Before
  scoping an issue or claiming current state, fetch `origin` and read recently
  merged pull requests touching the same contract surface; a cached
  remote-tracking ref is not freshness evidence.
- A language feature moves with grammar/lowering, typed model, symbolic and
  concrete semantics, regression corpus, both language references, FSL skill
  references, design note, changelog fragment, and—when applicable—LSP indexing
  plus role/scope tests and dialect registry coverage.
- Soundness-critical triangulation follows
  `docs/DESIGN-triangulated-assurance.md`; shared parsing/classification is not
  independent evidence and triangulation does not promote assurance or exit
  status.
- ⚠️ **No gate enforces this one.** A change that rewrites a type-boundary gate
  must `check` every `.fsl` under `specs/`, `examples/`, and
  `rust/fslc/tests/fixtures/` with binaries built from the named base SHA and
  from head, in each SHA's own materialized tree, plus the inline fixtures of
  every required test it rewrites, renames, removes, or stops running. Record
  each `ok`→`error` transition as a breaking removal in the design note, both
  language references, `skills/fsl/references/syntax.md`, and a `changed`
  fragment; state an empty transition set with the command, both SHAs, and the
  form count rather than assuming it. `corpus_check_sweep.rs` reports only a
  `specs/`/`examples/` form that stops checking
  (`docs/DESIGN-nested-option-support.md`, "Amendment (#925)").
- Never hand-edit generated compatibility snapshots. An accepted construct with
  missing/placeholder semantics is a soundness defect: fix it in scope or record
  an issue/follow-up with explicit authority.

Gate-enforced detail (coupled-change lists, CI validator inventory, failure
classes, mutation controls, knowledge-distillation rules, task-state) lives in
`.agents/skills/focused-change/references/fsl-repository-contract.md` and must
be read through `$focused-change` for affected work.

## Change and completion

Use `cargo fmt`; keep Clippy warning-free; do not introduce unsafe Rust.
Python uses four-space style. New source files require the Apache-2.0 SPDX
header.

Keep one topic per commit with a Conventional Commit-style subject. Add notable
changes as new `changelog.d/<id>-<slug>.<category>.md` fragments; do not edit
`CHANGELOG.md` directly. PRs state the problem, contract change, test evidence,
linked issue, and documentation/skill impact.

Before finishing, inspect `git diff --stat` and the relevant diff. Completion
requires satisfied acceptance conditions, sufficient checks (or an explained
failure), reviewed scope, current state identity, and explicit remaining risks.
Final responses should report changed behavior, files, checks/results, and
unresolved risk without reproducing source files or full logs.
