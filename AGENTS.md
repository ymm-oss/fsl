# Repository Guidelines

## Project and authority

`fslc` is the verifier for FSL, an AI-native formal specification language. The native Rust
workspace under `rust/` is the authoritative implementation and distribution surface, including
the `fslc-lsp` language server. The Python package under `src/fslc/` is a frozen compatibility
reference; do not add product behavior there unless a compatibility change explicitly requires it.

Interpret evidence in this order:

1. Language and CLI contracts in `docs/LANGUAGE.md`, accepted `docs/DESIGN-*.md`, tests, and CI.
2. The native Rust implementation and its public Kernel/JSON contracts.
3. Observable behavior of the frozen Python reference where a parity contract applies.
4. Proposals, task notes, conversation history, and agent memory.

When sources disagree, stop and resolve the contract conflict instead of silently choosing one.

## Project structure

- `rust/fsl-syntax`: lexer, parsers, source locations, and surface AST.
- `rust/fsl-core`: typed kernel model, validation, resolution, and dialect lowering.
- `rust/fsl-runtime`: solver-independent Monitor and explicit-state/BFS behavior.
- `rust/fsl-solver*`: backend-neutral solver boundary plus native and browser Z3 backends.
- `rust/fsl-verifier`: BMC, induction, refinement, liveness, and scenarios.
- `rust/fsl-tools`: analysis, mutation, report, typestate, and test generation tools.
- `rust/fslc`: native CLI and JSON/process contract.
- `rust/fsl-wasm`: browser Worker surface.
- `rust/fsl-lsp`: native language server and document index.
- `src/fslc`: frozen Python compatibility reference.
- `tests`: Python-driven Rust contract, parity, and compatibility tests.
- `specs` and `examples`: FSL corpus and reproducing cases.
- `skills`: canonical agent skills; `.claude/skills/fsl*` and `.agents/skills/fsl*` symlink here.

## Build and verification commands

Run the native CLI from the working tree:

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

The complete required product gate has one Rust-native entrypoint and does not execute Python:

```bash
./tools/check-native-integration.sh
```

Pull requests into `main` may use the bounded `merge readiness` gate defined in
`docs/DESIGN-ci.md`; it is not product verification. Every merged `main` state and every
production/release promotion must still receive the complete product evidence. Do not hide a
post-merge product-gate failure or treat its automatically created issue as a waiver.

Python is optional and is used only for changes explicitly scoped to the frozen compatibility
reference or Python-based repository hooks. Native solver changes should also run
the focused `fsl-solver-z3`, `fsl-verifier`, and `fslc-rust` tests.
Changes to concrete/symbolic semantics additionally run
`./tools/check-native-integration.sh fsl-logic pr`; generator, comparator,
inventory, and promotion changes run the `scheduled` tier.

## Correctness invariants

- `fsl-runtime` must remain independent of `fsl-solver`, Z3, and JavaScript solver bridges.
- Symbolic verification, the concrete Monitor, and solver-free BFS must agree. A confidently green
  false negative is more dangerous than a crash.
- Native CLI and Worker output must preserve the JSON envelope, exit codes, locations, and replayable
  evidence contract. Do not allowlist verdict, location, assurance, or exit-code differences.
- A language feature moves with its grammar/lowering, typed model, symbolic and concrete semantics,
  regression cases, `docs/LANGUAGE.md`, `docs/LANGUAGE.ja.md`, `skills/fsl/reference.md`, a design
  note, and a `changelog.d/` fragment (see `changelog.d/README.md`; `CHANGELOG.md`'s `[Unreleased]`
  body itself is aggregated from fragments at release time and must not be hand-edited). A new
  declaration, binder, or reference form additionally moves with
  `rust/fsl-lsp/src/index.rs` and a targeted role/scope test, or it silently loses
  definition/references/rename/documentSymbol with no parse failure to surface the gap;
  `rust/fsl-lsp/tests/corpus.rs` only asserts that every identifier is indexed as something.
  `docs/LANGUAGE.ja.md` is a second canonical source kept section-aligned
  1:1 with `docs/LANGUAGE.md` (same count/order of `## ` sections) — `tools/build_site_reference.py`
  fails loudly on drift; see `docs/DESIGN-docs-site.md` D7 (this one *is* a required CI check:
  `.github/workflows/site-reference-freshness.yml`). A new dialect's top-level construct (and any new
  `examples/`/`specs/` directory) additionally moves with `tests/dialect_registry.py` (`DIALECTS`,
  `EVIDENCE_CONSTRUCTS`, or `MONITOR_EXCLUSIONS`). The frozen-Python conformance harness
  (`docs/DESIGN-conformance-harness.md`, `tests/test_dialect_conformance.py`) is written to fail
  loudly on an unregistered construct instead of silently excluding the corpus, but — unlike the
  `docs/LANGUAGE.ja.md` check above — **no CI workflow and no `./tools/check-native-integration.sh`
  lane currently invokes it**; it is a developer-run manual/reference check, not a machine-enforced
  gate (see the design doc's "Cost and CI wiring"). Scope that precisely: registering the construct
  in `tests/dialect_registry.py` and the harness's dual-evaluator (Monitor/BMC/oracle) agreement
  checks have no mechanical enforcement today. A narrower obligation is enforced regardless — every
  `.fsl` under `specs/`/`examples/` must `check` cleanly or declare/exclude its error — by
  `rust/fslc/tests/corpus_check_sweep.rs` inside the required `rust workspace` job
  (`.github/workflows/ci.yml`). That native sweep will not catch a missing `tests/dialect_registry.py`
  entry or a Monitor/native disagreement; register the construct because the rule says so, not
  because something will always catch you if you don't.
- Top-level dialect counts and parser parity do not establish nested semantic coverage. When porting
  or auditing an AST/enum sum type, inventory every behavior-bearing variant and bind each accepted
  variant to executable native semantics with accepting/rejecting controls, or to an explicit
  fail-closed diagnostic. Prefer a total lowering expression whose arms all return the same semantic
  output type; an empty unit arm must not compile as a valid implementation.
- Do not weaken or hollow out `.fsl` specs to make checks pass. Verify mutation/vacuity evidence.
- Every formal-to-implementation conformance anchor must include a negative control that rejects a
  known contract-violating trace, transition, or mutation. A green positive path alone does not
  establish that the anchor can detect drift.
- Label each control by what it establishes, and prove the label by executing the mutation. A
  *detector* fails under the mutation it is cited for; a *preservation control* correctly keeps passing
  when the change is reverted and establishes only that unrelated behavior was not disturbed. A
  preservation control presented as a detector is a false coverage claim. Report the produced value
  beside the expected one — "the test failed" does not establish that it failed for the right reason.
  Isolate mutations unless a compound mutant's compound expectation is stated before it is applied.
  Prove the revert by exact equality to the named baseline — an empty `git diff` against it. A `grep -c`
  returning zero is supporting evidence only when the mutation introduced a unique token: a mutation
  that edits or deletes text leaves a correct revert with a nonzero count, and a pattern can match a
  sibling site rather than the mutated one.
- A comparison control's scope is part of the control. When a control asserts that two outputs agree,
  compare them in full. Give every excluded field a written reason it *cannot* be compared, not a
  category label asserting that it varies; build the exclusion list from the observed output, never
  from a type's field names; and pair it with a check that fails when an excluded key is absent from
  both sides. A dead exclusion weakens nothing while looking deliberate, so reading cannot distinguish
  it from a considered one. A hand-picked field list has let two opposite-direction wrong
  implementations both pass.
- A control whose verdict depends on ambient state is not a control. If an observable it compares can
  vary with cache state, environment, filesystem residue, or execution order, split it: compare stable
  observables exactly and check ambient ones for membership rather than equality. Run a new or changed
  control at least twice in one session before reporting it green — the first fix for this class
  commonly inverts the flake rather than removing it, and a single run cannot tell those apart.
- Confirm what a state *is* before reporting an observation about it. Name the commit, the built
  binary, and any mutation currently applied. A working tree under a calibration mutation is not the
  committed implementation; a worktree behind `origin/main` is not `main`; a binary built before the
  change does not exercise the change; a run's creation timestamp is not a job's elapsed time. Each of
  these has produced a confidently reported defect that did not exist, or a passing verdict that the
  change had not earned.
- A soundness-critical claim marked triangulated must follow
  `docs/DESIGN-triangulated-assurance.md`: preserve the pre-classification raw observation, declare
  two reviewably independent semantic lineages, execute all three agreement edges, and calibrate
  accepting/rejecting controls. Consumer parity through one parser/classifier is not independence,
  and triangulation never promotes the public assurance class or process exit.
- Do not hand-edit generated compatibility snapshots. Regenerate them only when the corresponding
  contract change is intentional and review the resulting diff.
- An accepted construct with absent, placeholder, or hollow semantics is a soundness defect, even if
  it is outside the current edit. Before reporting a task complete, either fix it in scope or record
  an existing/new issue URL in the task packet. If external issue creation is not authorized, leave
  an explicit unresolved follow-up and request authorization; do not let the finding survive only in
  chat, a review transcript, or agent memory.

## Knowledge distillation

Treat field-trial logs, spike notes, task packets, and audit transcripts as temporary evidence, not
repository authority. Before closing the work, promote every durable result to the smallest relevant
authoritative surface: a language/design contract, an agent or contributor rule, an executable test,
or a maintained example. Then remove the temporary report and its backlinks. Keep a standalone
experiment record only when its method or raw data is itself a maintained product artifact; Git and
pull-request history preserve chronology.

For an explicitly requested local Referance semantic-drift audit, follow
`docs/DESIGN-referance-local-audit.md`: begin with a verified task-local Store and provenance-bearing
behavior/freshness evidence, then use the repo-owned bounded CodeReferance profile only as an auxiliary
read-only detector. Confirm both Store path and audited root, keep every observation shadow/local, and
complete authority-ordered triage before filing a finding. Referance is not a CI, merge, product,
promotion, or release gate, and its symbol/parity results never establish nested executable semantics
or authorize automatic ground/promote/issue.

## Coding and change conventions

Use `cargo fmt` and keep Clippy warning-free. The workspace forbids unsafe Rust. Python code follows
standard four-space style, but changes to the frozen implementation require an explicit reason.
New source files must carry the repository's Apache-2.0 SPDX header.

## Codex task state and context discipline

- In Codex sessions, `tasks/active.md` is the worktree-local current task packet. It is ignored by Git
  and must be reconciled with the branch, working tree, implementation, and observed command results.
- When a task packet declares append-only history or a no-amend rule, create a new commit rather than
  using `git commit --amend`; an exception requires explicit approval recorded in the task packet.
- A single successful or partial verification command is not completion evidence. The task packet must
  name every required command, its expected and produced result, and the current commit/binary identity;
  whoever delegated the task judges whether that evidence is sufficient. A delegated worker does
  not declare the task complete on its own.
- Use `$task-start` before substantial Codex work and `$checkpoint` before compaction, clearing,
  handoff, independent review, or ending the task.
- Keep durable decisions in accepted `docs/DESIGN-*.md`; task packets, conversations, plans, and Codex
  memories are not architectural authority.
- Delegate broad read-heavy exploration to `evidence_explorer` and independent final review to
  `independent_reviewer`. Keep iterative implementation in the main thread unless work can be isolated
  without shared write coordination.
- Do not return raw search output or full build logs to the main thread. Preserve exact paths, symbols,
  commands, exit codes, failing test names, and a full-log path when one exists.
- Use `/compact` at investigation, implementation, and verification boundaries. Use `/clear` only after
  checkpointing when switching to unrelated work.

For non-trivial changes, use a dedicated branch/worktree so unrelated local state is not mixed into
the task. Use repository-relative paths in committed files and delegation briefs; never embed a
developer's absolute path or username. Preserve unrelated user changes.

Keep one topic per commit, use Conventional Commit-style subjects, and add a notable change as a new
`changelog.d/<id>-<slug>.<category>.md` fragment (see `changelog.d/README.md`) in the same pull
request, rather than editing `CHANGELOG.md` directly. A pull request should state the problem,
contract change, test evidence, linked issue, and any documentation or skill updates.
