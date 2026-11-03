Changed (#1421): New approval records use unsigned v5 or signed v6
with an absolute creation-directory base and the original source label. Checks
resolve stored relative inputs and reviewed document targets from that base
while preserving literal paths, rendering labels and bytes. Legacy v1–v4
records retain their closed formats, signatures and invocation-relative paths;
older binaries reject new records as unsupported schemas. Checkout relocation
and document source-label admission remain unchanged.
Authoritative document approval checks and ledger evaluations reject a raw
input-digest mismatch as an I/O error only when the complete existing evaluation
would approve. Earlier errors and full ordered drift results keep precedence;
the first mismatch follows glossary then recorded evidence order, using the
same loaded bytes and literal paths.
Relative declared AI `dataset.source` paths now resolve from the declaring FSL
file's directory. Absolute declarations and caller-relative `--records` overrides
retain their behavior, with no fallback search. The documented support-quality
example uses `support_eval_v3.jsonl`; existing repository-prefixed declarations
need explicit migration or a records override.
