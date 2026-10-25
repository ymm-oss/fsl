Fixed (#1337): `fslc testgen` now refuses failed inline `implements` seams
and governance preservations on its kernel path as well as its verify fallback
path. Both return the verify owner's failure envelope and exit 1, or the same
located contract error and exit 2, and write no harness. The gate shares the
scenarios fold and runs before the walk, path handling, and generation.
Migration: these documents previously generated a harness with exit 0 on the
kernel path; automation must now handle exit 1/2 and no output artifact. Fix
the seam or preservation before generating tests. Passing documents and
non-strict partially reachable scenarios keep their existing generation behavior.
