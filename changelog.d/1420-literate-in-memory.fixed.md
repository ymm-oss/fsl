Fixed (#1420): native `check`, `verify`, and `scenarios` consume extracted
literate Markdown source in memory instead of creating an adjacent
`.literate-<pid>.fsl` file. Extraction no longer requires a writable source
directory or leaves a new sibling after abrupt termination. Existing
diagnostic labels, lexical dependency resolution, edition admission and
Markdown-byte cache identity are retained; verify's `--strict-tags` uses
the captured extracted source. Existing abandoned siblings are left untouched.
