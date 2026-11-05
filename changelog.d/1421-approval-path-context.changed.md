Changed (#1421): New approval records use unsigned v5 or signed v6
with a `path_base` relative to the Git root and the original source label. Checks
resolve stored inputs and reviewed document targets from the Git root of the spec
being checked, so a clone, worktree or relocated checkout reads its own files,
while preserving literal paths, rendering labels and bytes. A document record's
creation directory, target and inputs must stay inside that root: paths that are
absolute or lead outside are refused at creation and at check. The root comes from
the directory the spec's path names, ignoring `GIT_DIR` and similar variables, so
`document generate --approval` on these records needs Git, and a spec resolving outside
the root is refused. For solver records `approved` covers the spec and a fresh
rendering, not the reviewed file on disk; a v5 record is unsigned, so a `path_base`
edited to reach a same-content copy still reports `approved`, while v6 catches it. Legacy v1–v4
records retain their closed formats, signatures and invocation-relative paths;
older binaries reject new records as unsupported schemas. Document source-label
admission remains unchanged.
Authoritative document approval checks and ledger evaluations reject a raw
input-digest mismatch as an I/O error only when the complete existing evaluation
would approve. Earlier errors and full ordered drift results keep precedence;
the first mismatch follows glossary then recorded evidence order, using the
same loaded bytes and literal paths.
Relative declared AI `dataset.source` paths now resolve from the declaring FSL
file's directory. Absolute declarations and caller-relative `--records` overrides
retain their behavior, with no fallback search. The base is the file's lexical
parent, so a declaring file reached through a symlink resolves beside the link. The documented support-quality
example uses `support_eval_v3.jsonl`; existing repository-prefixed declarations
need explicit migration or a records override.
After replay and artifact construction, causal observation rejects repeated
actual output destinations before serialization or writes. A key is the canonicalized
parent plus the file name, or the collapsed spelling when the parent does not exist;
names differing only by case do not collide. The usage error names the first repeated
paths. A later write failure now lists the earlier files in `written`. Distinct
output names, bytes, interleaving and inconclusive assurance remain unchanged.
