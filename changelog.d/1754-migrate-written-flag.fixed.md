Fixed (#1754): `fslc migrate --write` no longer reports `"written": true` when
it wrote nothing. The top-level `written` echoed the `--write` flag, so a run
whose inputs needed no rewrite reported `"written": true` with `"changed": 0`.
`written` now means a file was replaced: the top level is `true` only when at
least one input was replaced, and each `files[]` entry carries its own
`written`, `true` only for a replaced input. Unchanged inputs were already left
untouched (same inode and mtime); dry runs, refusals and
`migrate_manual_required` report `written: false` at both levels. A consumer
that read `written` as "`--write` was given" now sees `false` for a no-op
write.
