# Git-aware semantic diff

Issue: #186. This adapter is layered on the VCS-independent comparison in
[`DESIGN-semantic-diff.md`](DESIGN-semantic-diff.md).

## Contract

`fslc diff --git BASE..HEAD [SPEC]` resolves both names to commit hashes and
materializes each complete tracked tree with `git archive`. The old and new
`SPEC` paths are then passed to the ordinary two-path semantic diff. Complete
tree materialization is required: `use`, `implements ... from`, compose, and
project-relative references must resolve against files from their own
revision, never against the current worktree or the other side of the diff.

When `SPEC` is omitted, the adapter obtains the sorted set of added, copied,
modified, or renamed `.fsl` paths from `git diff --name-only` and compares each
path that exists in both trees. A path that is present on only one side is an
explicit IO error; inventing semantics for a created or deleted specification
would be unsound.

The adapter is deliberately thin. `semantic_diff()` accepts two filesystem
paths and never invokes Git. Therefore `fslc diff OLD NEW` continues to work in
a directory that is not a repository.

## Output and gate

A single-spec result keeps `result:"semantic_diff"` and adds:

```json
{
  "vcs": {
    "kind": "git",
    "range": "main..HEAD",
    "base": {"revision": "main", "commit": "<full hash>"},
    "head": {"revision": "HEAD", "commit": "<full hash>"},
    "materialization": "git_archive_full_tree"
  }
}
```

The file labels are revision-qualified (`main:path/to/spec.fsl`). Batch mode
returns `result:"semantic_diff_batch"`, the same `vcs` object, sorted `specs`,
and `comparisons`. Its gate fails if any child comparison violates `--forbid`.
All boundedness and semantic classifications remain those of the core diff.

## CI recipe

The following job turns changed specifications into a review artifact. A bot
may post `semantic-diff.json` as a PR comment; the JSON witness is the review
unit, while `--forbid` remains repository policy.

```yaml
jobs:
  semantic-diff:
    runs-on: ubuntu-latest
    steps:
      - uses: actions/checkout@v4
        with: {fetch-depth: 0}
      - uses: actions/setup-python@v5
        with: {python-version: "3.12"}
      - run: pip install -e .
      - name: Compare changed FSL specifications
        run: >-
          fslc diff --git origin/${{ github.base_ref }}..HEAD
          --depth 8
          --forbid behavior_added,invariant_weakened,forbidden_relaxed
          > semantic-diff.json
      - uses: actions/upload-artifact@v4
        if: always()
        with:
          name: semantic-diff
          path: semantic-diff.json
```

GitHub Pages or a diff driver is not used for materialization. A diff driver
receives isolated temporary files and cannot supply the revision-consistent
import tree required by FSL parsing.
