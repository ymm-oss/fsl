# Release procedure

This document is the authoritative execution procedure for FSL releases. The
internal `.claude/skills/release/SKILL.md` defines the wider branch lifecycle;
change both in the same pull request when either contract changes.

The release path is:

```text
short-lived branch -> main -> production -> vX.Y.Z
```

FSL is distributed by the tag-driven GitHub Release workflow only. Do not
publish the frozen Python compatibility reference to PyPI or the Rust crates to
crates.io.

## Supported native targets

Each target ships both `fslc` and `fslc-lsp`, with a SHA-256 file for each
binary.

| Target | Runner | Binary suffix |
|---|---|---|
| macOS arm64 (Apple Silicon) | `macos-14` | `macos-arm64` |
| Linux x64 | `ubuntu-latest` | `linux-x64` |
| Linux arm64 | `ubuntu-24.04-arm` | `linux-arm64` |
| Windows x64 | `windows-latest` | `windows-x64.exe` |

Intel macOS (`macos-x64`) is not supported. Releases also contain the VS Code
extension and the Public Kernel contract bundles produced by
`.github/workflows/release.yml`.

## 1. Prepare the release commit on main

1. Start from a clean, current `main`. Fetch `origin` and confirm local `HEAD`
   equals `origin/main`.
2. Review the non-empty `CHANGELOG.md` `[Unreleased]` section and confirm it
   describes every notable change since the previous tag.
3. Choose `X.Y.Z` using SemVer. Confirm that local and remote tag `vX.Y.Z` and
   the corresponding GitHub Release do not exist.
4. On a short-lived branch from `main`, change
   `[workspace.package].version` in `rust/Cargo.toml`. Regenerate
   `rust/Cargo.lock` with Cargo, then prove the lockfile and CLI version agree:

   ```bash
   cargo check --manifest-path rust/Cargo.toml --workspace
   cargo check --manifest-path rust/Cargo.toml --workspace --locked
   test "$(cargo run --manifest-path rust/Cargo.toml -p fslc-rust --bin fslc -- --version)" = "fslc X.Y.Z"
   ```

   Do not hand-edit `rust/Cargo.lock`. Do not bump `pyproject.toml` for a native
   GitHub Release; the Python package is a frozen, unpublished compatibility
   reference.
5. Move all current `[Unreleased]` entries under
   `## [X.Y.Z] - YYYY-MM-DD`, leaving an empty `## [Unreleased]`. Update the
   link references so `[Unreleased]` compares `vX.Y.Z...HEAD` and `[X.Y.Z]`
   compares the previous tag with `vX.Y.Z`.
6. Confirm the complete `X.Y.Z` changelog section is non-empty and suitable for
   the GitHub Release body.
7. Run the complete product gate:

   ```bash
   ./tools/check-native-integration.sh
   ```

8. Commit `chore(release): vX.Y.Z`, open a pull request to `main`, and merge it
   only after required checks pass. Record the exact merged `main` SHA as the
   release candidate.

## 2. Prove and promote the candidate

1. Dispatch `.github/workflows/release.yml` on the recorded candidate. A
   `workflow_dispatch` run builds and smoke-tests every artifact but cannot
   attach files to a GitHub Release, even when the selected ref is a tag.
2. Verify the completed run's `head_sha` equals the recorded candidate SHA and
   all four native targets, the VS Code extension, and both Kernel bundle jobs
   pass. Do not reuse evidence from a moving branch after its SHA changes.
3. Open the release-promotion pull request from `main` to `production`, stating
   the candidate SHA, version, changes, residual risk, gate results, and dry-run
   URL.
4. Merge without squashing away promoted history. Verify the resulting
   `production` tree matches the approved `main` tree and record the new
   `production` HEAD. Never tag `main`.

## 3. Revalidate and tag production

1. On the exact `production` HEAD, verify `rust/Cargo.toml`, `rust/Cargo.lock`,
   the changelog section, and `fslc --version` all identify `X.Y.Z`.
2. Rerun `./tools/check-native-integration.sh` and dispatch the manual release
   workflow from `production`. Require the run's `head_sha` to equal the
   recorded production HEAD and every job to pass; pre-promotion evidence is
   not valid for a distinct production merge commit.
3. Regenerate a temporary notes file from the complete `X.Y.Z` section in the
   exact production HEAD's `CHANGELOG.md`, excluding its version heading. Review
   it and stop if it is empty; do not reuse a file derived from the pre-promotion
   candidate.
4. Show the user the production commit, annotated tag `vX.Y.Z`, and that pushing
   it creates the public GitHub Release and publishes its artifacts. Obtain
   explicit confirmation immediately before running:

   ```bash
   git tag -a vX.Y.Z PRODUCTION_SHA -m "vX.Y.Z"
   git push origin vX.Y.Z
   ```

5. Watch the tag-triggered workflow to completion. The workflow rejects any
   native binary whose `fslc --version` differs from the tag.

## 4. Publish notes and verify the release

1. Show the user the prepared changelog-derived notes and obtain explicit
   confirmation before setting the GitHub Release body:

   ```bash
   gh release edit vX.Y.Z --notes-file RELEASE_NOTES_FILE
   ```

2. Confirm the published body is non-empty and matches the `X.Y.Z` changelog
   section.
3. Confirm `fslc`, `fslc-lsp`, and their checksum files exist for exactly the
   four supported suffixes. Confirm no `macos-x64` asset exists. Also confirm
   the VS Code extension and both Kernel bundle/checksum pairs are present.
4. Download the current machine's supported binary and checksum, verify the
   checksum, and run `fslc --version`. It must print `fslc X.Y.Z`.
5. Report the promotion pull request, production SHA, tag SHA, release URL,
   workflow runs, non-empty notes, asset inventory, checksum, and version smoke
   test.

## Failure handling

- For a transient job failure, inspect it and use
  `gh run rerun RUN_ID --failed`. Do not retag merely to retry the same commit.
- Use `workflow_dispatch` for build diagnosis. It is the only dry-run path and
  never attaches Release assets.
- Never force-move, delete, or reuse a pushed release tag. If the tagged commit
  or artifacts are wrong, fix the defect upstream, promote it, and cut a new
  patch version.
- If release-note publication or asset verification fails, leave the release
  visibly unresolved until repaired. Do not report completion.
- Follow the internal release skill's `release/vX.Y` stabilization and hotfix
  procedures when `main` cannot be promoted as a whole.
