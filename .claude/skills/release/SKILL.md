---
name: release
description: Cut a new fslc release. Bumps the version in pyproject.toml, rolls the CHANGELOG.md [Unreleased] section into a dated version section, commits it as chore(release), creates an annotated v* git tag, and pushes so the release-binaries and publish-pypi workflows fire. Invoke only when the user explicitly asks to release/tag/publish a version.
disable-model-invocation: true
---

# release — cut an fslc version

A release here is a fixed ritual driven by a `v*` git tag. Pushing the tag triggers
`.github/workflows/release.yml` (PyInstaller binaries + the VS Code `.vsix`, attached
to a freshly created GitHub Release) which in turn publishes the Release, firing
`.github/workflows/publish.yml` (build sdist/wheel + PyPI trusted publishing). So the
tag is the trigger for the whole public release — get the pre-tag steps right first.

## Inputs

- **Version**: taken from the invocation argument (e.g. `/release 2.6.4`). If none is
  given, read the current `version` in `pyproject.toml` and the contents of the
  CHANGELOG `[Unreleased]` block, propose the next SemVer bump (patch for fixes only,
  minor for new features, major for breaking changes), and confirm it before continuing.
- **Date**: use today's date in `YYYY-MM-DD`.

## Procedure

1. **Pre-flight.** Confirm the working tree is clean (`git status --porcelain` empty)
   and you are on `main` and up to date with `origin/main`. If not, stop and report —
   do not release from a dirty tree or a feature branch.
2. **Confirm `[Unreleased]` is non-empty.** A release with no changelog entries is
   almost always a mistake; if it is empty, stop and ask.
3. **Bump the version** in `pyproject.toml` — the single `version = "x.y.z"` line under
   `[project]`. This is the only place the version string lives (runtime version comes
   from `importlib.metadata`).
4. **Roll the CHANGELOG.** In `CHANGELOG.md`, insert a new `## [x.y.z] - YYYY-MM-DD`
   heading immediately below `## [Unreleased]`, move every entry currently under
   `[Unreleased]` into it, and leave `[Unreleased]` present but empty. Preserve the
   Keep-a-Changelog subsection order (Added / Changed / Fixed / …).
5. **Sanity check.** Run the fast gate — `.venv/bin/python -m fslc --version` reflects
   the install, and a quick `pytest tests/test_version.py -q` — before tagging. (A full
   `pytest -q` is ~8 min; run it if the release contains verifier-semantics changes.)
6. **Commit** exactly the two files with the conventional-commit subject the history
   uses: `chore(release): vX.Y.Z`.
7. **Tag.** Create an *annotated* tag matching the history convention:
   `git tag -a vX.Y.Z -m "vX.Y.Z"`. (Each version corresponds to an annotated `v*` tag.)
8. **Push — the point of no return.** `git push origin main` then
   `git push origin vX.Y.Z`. State clearly, before pushing, that this triggers the
   binary build + GitHub Release + PyPI publish, and that the tag/PyPI version cannot be
   reused once published. Pushing changes the outside world, so do it only when the user
   has confirmed this run (they invoked `/release`, but confirm the exact version).
9. **Report** the tag, the commit, and links to the two Actions runs to watch
   (`release-binaries`, `publish-pypi`).

## Guardrails

- Never reuse or force-move a published tag — PyPI rejects a re-uploaded version, and a
  moved tag desyncs the binaries from the source. If step 8 already ran and something is
  wrong, cut a new patch version instead.
- Do not edit `CHANGELOG.md` history for already-released versions; only touch
  `[Unreleased]` and the new section.
- If `pyproject.toml` version and the new CHANGELOG heading disagree, stop — they must match.
