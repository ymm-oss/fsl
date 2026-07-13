---
name: release
description: Cut an FSL release by synchronizing native Rust and Python package versions, rolling the changelog, verifying native artifacts, committing, tagging, and pushing the explicit version.
disable-model-invocation: true
argument-hint: "[version]"
---

# Cut an FSL release

Pushing a `v*` tag triggers native binary builds and a GitHub Release; publishing that Release triggers
the retained Python reference package publication. External publication is irreversible for that version.

1. Confirm a clean worktree, `main`, and an up-to-date remote. Stop otherwise.
2. Confirm `CHANGELOG.md` `[Unreleased]` is non-empty and determine the exact SemVer version.
3. Update both authoritative package version declarations:
   - `rust/Cargo.toml` `[workspace.package].version`
   - `pyproject.toml` `[project].version`
   Regenerate `rust/Cargo.lock` through Cargo if workspace package entries change; do not hand-edit it.
4. Move `[Unreleased]` entries under `## [X.Y.Z] - YYYY-MM-DD`, leaving `[Unreleased]` present and empty.
5. Verify version agreement and run at minimum:
   - `cargo run --manifest-path rust/Cargo.toml -p fslc-rust --bin fslc -- --version`
   - `cargo test --manifest-path rust/Cargo.toml -p fslc-rust --locked`
   - `.venv/bin/python -m pytest tests/test_version.py tests/test_rust_cli_contract.py -q`
   Expand the gate for semantic or distribution changes.
6. Commit only the release files with `chore(release): vX.Y.Z`.
7. Create annotated tag `vX.Y.Z`.
8. Before pushing, state the exact tag and that it triggers native binaries, GitHub Release, and eventual
   Python publication. Push only after explicit confirmation for this version.
9. Report commit, tag, and Actions runs.

Never reuse or force-move a published tag. If publication has begun and a defect is found, cut a new
patch version. Never rewrite an already released changelog section.
