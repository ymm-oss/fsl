Fixed (#1946): the verify cache could store a verdict under the key of other
file contents. The key re-read the entry spec and the `--requirements` file
from disk while verification used the root captured once (#808), and
`--engine auto` rebuilt its store keys after the run, so a spec replaced
during an `auto` run had the old content's verdict served to the next run on
the new content: a false `proved` (exit 0 for a violated spec) or a false
`violated`, also through the cross-depth pointer and to plain
`--engine explicit` runs. A `--requirements` file edited during a
`--strict-tags` run had its later contents' warnings stored under its earlier
contents' key. The key now hashes the captured root bytes (for a literate
`.md`, the Markdown document captured with its source), the recorded
dependencies and the `--requirements` bytes read once, which the strict-tag
pass also uses; `auto` stores under the keys its lookup computed before the
run, and `FSLC_CACHE_VERIFY=1` now also reports an `auto` divergence. The key
prefix moves from `v4` to `v5`, so an entry written before this change, which
may hold such a verdict, is never served; the first run of each spec after
upgrading misses once. `sweep` cells and `chain` spec layers key by their
captured root the same way. Verify output and exit codes are unchanged,
except that `FSLC_CACHE_VERIFY=1` with `--engine auto` now reports a
divergence from the stored entry as an `internal` error (exit 3) where it
used to overwrite the entry and exit 0 or 1.
