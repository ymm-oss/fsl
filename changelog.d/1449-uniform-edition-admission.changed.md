Changed (#1449): Native checking and verification classify the root source's
edition before consumption. Verify-shaped results share migration-warning
finalization after governance, including nested verification and report baselines,
instead of repeating it in the CLI verify and test-generation failure paths.
Edition-migration ledger rows now say 移行候補 with their code and message instead of vacuity or mutate advice.
Migration-only rows use status 🟡 移行候補 and risk —; non-migration or mixed rows retain 🔴 要確認 and risk 要確認.
Kernel, conformance, testplan and typestate consumers also admit their captured
source before analysis; closed contracts and raw TypeScript deliver migration
warnings on stderr without changing their output schemas.
Worker check and verify reject unsupported source editions before model or solver
consumption, retain verification-option error priority, and deliver admitted
warnings once after governance.
Editor diagnostics reject unsupported editions before model or analysis work and
retain migration finding ranges for UTF-16 publication.
Replay, explain and single-source analysis also apply admission before consuming
the captured source, with readable and graph output warnings kept on stderr.
Domain check now finalizes each result once instead of duplicating migration
warnings across its specialized result and nested verification result, including
non-definitive verification errors; requested editions and read-error kinds are retained.
Causal consumers admit captured roots and explicit observation mappings before
analysis or output, preserve specialized warnings, and reuse the same source for
expectation parsing and model construction instead of rereading the root.
Causal root preparation intentionally reports a later operand/mapping read error
before an earlier root's parse/model error. Envelope edition metadata now comes
only from the primary root; every slot's warnings are retained once. Native and
Worker check reuse the admitted token for their inner diagnostic preflight.
This is a partial delivery: AI and non-check domain intake, document/approval
intake, live comparison/refinement/log mappings, batch/project/chain preparation,
counterexample ordering, mutate success warnings, sweep capture and raw-success
HTML/ledger stderr remain tracked by #1449 with executed deferred-mode controls.
Git-revision roots (#1513), public-example warning cleanup (#608), lint dialect
support (#609) and other ledger warning classifications (#1497) are separate.
Breaking consumer removals: four next-edition migration forms (enum union,
metadata, colon quantifier, inline maps) now refuse in analyze/conformance/explain/
html/kernel/ledger/mutate/scenarios/sweep/testgen/typestate, and enum union in
testplan (45 command/form observations already rejected by check). A named
BASE/implementation `check` audit covers 597 corpus files plus 309 inline
occurrences in both materialized trees with an empty ok-to-error set; see the
migration design amendment for SHAs, command and attribution.
