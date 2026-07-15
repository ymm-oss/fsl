# Repository Guidelines

## Project and authority

`fslc` is the verifier for FSL, an AI-native formal specification language. The native Rust
workspace under `rust/` is the authoritative implementation and distribution surface. The Python
package under `src/fslc/` is a frozen compatibility reference and the retained LSP surface; do not
add product behavior there unless a compatibility or LSP change explicitly requires it.

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
- `src/fslc`: frozen Python compatibility reference and LSP.
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

Python is optional and is used only for changes explicitly scoped to the frozen compatibility
reference, retained LSP, or Python-based repository hooks. Native solver changes should also run
the focused `fsl-solver-z3`, `fsl-verifier`, and `fslc-rust` tests.

## Correctness invariants

- `fsl-runtime` must remain independent of `fsl-solver`, Z3, and JavaScript solver bridges.
- Symbolic verification, the concrete Monitor, and solver-free BFS must agree. A confidently green
  false negative is more dangerous than a crash.
- Native CLI and Worker output must preserve the JSON envelope, exit codes, locations, and replayable
  evidence contract. Do not allowlist verdict, location, assurance, or exit-code differences.
- A language feature moves with its grammar/lowering, typed model, symbolic and concrete semantics,
  regression cases, `docs/LANGUAGE.md`, `skills/fsl/reference.md`, a design note, and `CHANGELOG.md`.
- Do not weaken or hollow out `.fsl` specs to make checks pass. Verify mutation/vacuity evidence.
- Do not hand-edit generated compatibility snapshots. Regenerate them only when the corresponding
  contract change is intentional and review the resulting diff.

## Coding and change conventions

Use `cargo fmt` and keep Clippy warning-free. The workspace forbids unsafe Rust. Python code follows
standard four-space style, but changes to the frozen implementation require an explicit reason.
New source files must carry the repository's Apache-2.0 SPDX header.

## Codex task state and context discipline

- In Codex sessions, `tasks/active.md` is the worktree-local current task packet. It is ignored by Git
  and must be reconciled with the branch, working tree, implementation, and observed command results.
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

Keep one topic per commit, use Conventional Commit-style subjects, and add notable changes under
`CHANGELOG.md` `[Unreleased]`. A pull request should state the problem, contract change, test evidence,
linked issue, and any documentation or skill updates.
