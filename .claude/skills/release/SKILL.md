---
name: release
description: Operate FSL's GitLab Flow-inspired lifecycle from short-lived branches through main and production to an exact vX.Y.Z release tag. Use when adopting the branch flow, integrating post-merge quality improvements, promoting a release, cutting a release, or handling a production hotfix.
disable-model-invocation: true
argument-hint: "[adopt|integrate|promote|cut|hotfix] [version]"
---

# Operate the release flow

Treat CI-green pull requests as integration evidence and release promotion as a
separate, stronger decision:

```text
short-lived branch -> main -> production -> vX.Y.Z
                         ^          ^
                  integration   release readiness
```

Keep `main` as the default branch. Do not create `develop` or `pre-production`.
Use a temporary `release/vX.Y` branch only when `main` must advance while an
exact candidate is stabilized.

Read `docs/RELEASE.md` completely before `promote` or `cut`; it is the
authoritative command-level procedure. Keep it and this lifecycle contract
aligned in the same pull request.

## Non-negotiable rules

- Merge product and quality changes into `main` through pull requests.
- Accept changes into `production` only through a release-promotion pull request.
- Never tag `main`; tag the exact `production` commit approved for release.
- Never selectively promote commits from `main`. Promote its complete tree.
- Never force-move or reuse a published tag.
- Publish only the tag-driven GitHub Release. Do not publish the frozen Python
  compatibility reference to PyPI or the Rust crates to crates.io.
- Release native binaries for macOS arm64, Linux x64, Linux arm64, and Windows
  x64 only. Intel macOS (`macos-x64`) is not supported.
- Populate the GitHub Release body from the matching non-empty changelog section.
- Fix defects upstream in `main` first, then promote downstream. Use the hotfix
  exception only when unreleased `main` cannot safely ship.
- Stop before a push, production merge, tag, release publication, or branch
  protection change unless the user explicitly authorized that exact action.

## Route the invocation

Interpret `$ARGUMENTS` as one of these operations. If the operation or version
is missing, inspect the repository state and ask only for the decision that
cannot be derived safely.

| Operation | Outcome |
|---|---|
| `adopt` | Bootstrap and protect `production` |
| `integrate` | Merge a feature, fix, or quality improvement into `main` |
| `promote` | Prove the current release candidate and propose `main -> production` |
| `cut X.Y.Z` | Prepare, promote, tag, and observe version `X.Y.Z` |
| `hotfix X.Y.Z` | Patch the released line without pulling unreleased `main` into production |

## Adopt the flow

1. Fetch remote branches and tags. Require a clean worktree.
2. Identify the latest published `vX.Y.Z` tag and verify its release exists.
3. Create `production` at that tag, not at a newer unreleased `main` commit.
4. Protect `main` and `production` from direct pushes and force pushes.
5. Require normal CI and review on `main` pull requests.
6. Add a required policy status check for pull requests whose base is
   `production`. Make it reject every head except `main`, `release/vX.Y`, or
   `hotfix/vX.Y.Z`; branch protection alone cannot restrict the source branch.
7. Require the release gate and explicit approval on `production` promotion
   pull requests. Treat adoption as incomplete until the source-policy check is
   required by the `production` ruleset.
8. Keep tag creation as the publication trigger in `.github/workflows/release.yml`.

If no release has ever been published, require the user to identify the initial
production baseline. Do not guess it from branch age.

## Integrate and improve on main

1. Start from current `main` on a short-lived branch named for the change, such
   as `feat/...`, `fix/...`, `quality/...`, or `refactor/...`.
2. Apply the narrowest relevant checks before the repository-required gate.
3. Open a pull request to `main` with the contract, risk, and replayable evidence.
4. Merge when the integration gate passes. Do not claim that this makes the
   commit release-ready.
5. Continue audits, formalization, tests, refactoring, and defect fixes through
   new pull requests to `main` until the complete `main` tree meets the release
   gate.

Keep `main` releasable as a whole. Leave incomplete behavior unmerged unless it
is isolated by an already-required explicit configuration boundary. Do not add
a feature-flag system solely to accommodate this flow.

## Promote main to production

1. Verify `production` is an ancestor of, or can be merged cleanly with, `main`.
2. Freeze the candidate by recording the exact `main` SHA in the promotion pull
   request. If unrelated changes land, rerun the gate or replace the candidate.
3. Confirm the release commit preparation is already on `main`:
   - bump `rust/Cargo.toml`'s workspace version and regenerate `rust/Cargo.lock`
     with Cargo; do not bump the frozen `pyproject.toml` package for a native
     GitHub Release;
   - move `[Unreleased]` entries under `## [X.Y.Z] - YYYY-MM-DD` while retaining
     an empty `[Unreleased]` section;
   - update comparison links and confirm the matching changelog section is a
     non-empty Release body.
4. Run `./tools/check-native-integration.sh` and dispatch the release workflow's
   manual artifact build. Verify the completed run's `head_sha` equals the
   recorded candidate SHA; discard and rerun evidence produced from a moving
   branch after it advances. Confirm all four native targets, the VS Code
   extension, and both Kernel bundles pass. `workflow_dispatch` is
   non-publishing and must never attach Release assets, even for a tag ref. Add
   focused formal, mutation, platform, or compatibility evidence when the
   changed contract requires it.
5. Open `main -> production`. State the candidate SHA, version, included changes,
   known residual risk, exact gates, and artifact evidence.
6. Merge without squashing away the promoted history. Verify the resulting
   `production` tree matches the approved `main` tree. Record the new production
   merge SHA; it is distinct from the gated candidate SHA unless promotion was a
   true fast-forward.

Do not merge newer `main` work into an open promotion implicitly. Close or update
the promotion and rerun its evidence against the new SHA.

## Cut the release

After the promotion is approved and merged:

1. Verify the requested version is valid SemVer and absent from local and remote
   tags and releases.
2. Verify `production` HEAD has the prepared Rust workspace version, lockfile,
   changelog, and non-empty version section.
3. Rerun `./tools/check-native-integration.sh` on exact `production` HEAD. Dispatch
   the manual release artifact workflow from `production`, verify its `head_sha`
   equals that HEAD, and require every job to pass. Never reuse evidence from the
   pre-merge candidate for a distinct production merge commit.
4. Regenerate a temporary notes file from the exact production HEAD's matching
   changelog section. Stop if it is empty; never reuse notes derived from the
   pre-promotion candidate.
5. Create annotated tag `vX.Y.Z` at the gated `production` HEAD.
6. Before pushing, state the tag, commit SHA, and that the push publishes native
   binaries, the VS Code extension, and Kernel contract bundles.
7. Push the tag only after explicit confirmation for that version.
8. Observe every release job. If a job fails transiently, inspect it and rerun
   only the failed jobs; do not retag.
9. Show the production-derived notes file and obtain explicit confirmation
   before setting the GitHub Release body with
   `gh release edit vX.Y.Z --notes-file ...`.
10. Verify the Release body is non-empty and matches the changelog section.
11. Verify exactly the four supported native `fslc`/`fslc-lsp` asset pairs and
    checksums, the VS Code extension, and both Kernel bundles. Reject any
    `macos-x64` asset. Download one supported binary, verify its checksum, and
    require `fslc --version` to equal the tag; the workflow performs the same
    tag/version assertion on every native runner.
12. If publication has begun and a defect is found, fix it upstream, promote it,
    and cut a new patch version. Never rewrite the published release.

## Stabilize while main advances

Create `release/vX.Y` from a recorded `main` SHA only when candidate validation
must continue while unrelated work lands on `main`.

- Put every generally applicable fix in `main` first, then backport it to the
  release branch with traceable pull requests.
- Accept only release preparation and candidate fixes on the release branch.
- Promote `release/vX.Y -> production`, tag the production merge, then delete
  the temporary branch only after its release metadata is reconciled to `main`.
- After release, open a metadata reconciliation pull request to `main`. Carry
  forward the released version and comparison links, remove only the entries
  shipped by the release branch, and preserve newer `[Unreleased]` entries from
  advancing `main`. Do not merge the older release tree wholesale over `main`.
- Do not introduce permanent `pre-production`; the temporary branch represents
  the stabilized candidate, not an environment.

## Handle an urgent hotfix

Prefer fixing `main` and running the normal promotion. If `main` contains work
that must not ship:

1. Branch `hotfix/vX.Y.Z` from `production`.
2. Make only the minimal patch and release metadata change.
3. Open coordinated pull requests to `production` and `main`; ensure the fix is
   present on `main` before closing the hotfix task.
4. Run the release gate against the hotfix SHA, merge to `production`, and tag a
   new patch version using the normal cut procedure.

Record why normal upstream-first promotion was unsafe. Do not use a hotfix to
ship ordinary feature work.

## Preserve component boundaries

This branch flow does not justify a new runtime component or a branch-aware code
path. Apply these design constraints when a change affects architecture:

- Keep branch names out of product configuration, binaries, schemas, and runtime
  behavior. Git controls promotion; components implement one tested contract.
- Treat all artifacts built from one tag as one atomic release unit. For FSL this
  includes `fslc`, `fslc-lsp`, the VS Code extension, and published Kernel bundles.
  Build jobs upload private workflow artifacts; one final job publishes only after
  every artifact job succeeds.
- Verify cross-component contracts at the promotion SHA. A component cannot be
  declared ready while another artifact from the same tag still depends on an
  incompatible contract.
- Use versioned interfaces and an explicit compatibility policy only when
  components are intentionally deployed or supported independently. Do not use
  a branch per component as a substitute for interface versioning.
- Keep incomplete work outside `main` when it cannot preserve the release unit's
  existing observable behavior. Prefer deletion or consolidation before adding
  configuration or rollout machinery.

If a component needs a different release cadence, stop and propose a separate
release-unit contract before changing this flow.

## Report evidence

At completion, report:

- source and target branches with exact SHAs;
- integration and release gates actually observed;
- promotion pull request and release URLs;
- released tag and whether it equals `production` HEAD;
- non-empty release notes, supported asset inventory, checksum, and version
  smoke-test evidence;
- temporary release/hotfix branches still requiring cleanup;
- any component-contract risk deferred from the release.
