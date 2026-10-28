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
The single-root extension completes 21 further modes: six AI readers, the five
non-check domain commands, the three document commands, database observation,
log replay, counterexample export ordering, mutate success warnings, sweep capture
and direct HTML/ledger raw-warning delivery. Root and mapping preparation makes
mapping IO precede an ordinary root parse/model error in log replay. Document
check admits before artifact IO/frontmatter and projects captured bytes under
the recorded label without rereading its path. Direct raw reports warn on stderr;
HTML/ledger file-output envelopes and approval-only stderr remain unchanged.
Eight modes across seven leaves remain: analyze batch/project, approval
check/create/diff (including ledger --approval overlay root rereads), chain
admission, live diff and refine. Their deferred-mode controls still execute;
this remains a partial delivery and does not close #1449.
Git-revision roots (#1513), public-example warning cleanup (#608), lint dialect
support (#609) and other ledger warning classifications (#1497) are separate.
Breaking consumer removals: four next-edition migration forms (enum union,
metadata, colon quantifier, inline maps) now refuse in analyze/conformance/explain/
html/kernel/ledger/mutate/scenarios/sweep/testgen/typestate, and enum union in
testplan (45 command/form observations already rejected by check). A named
BASE/implementation `check` audit covers 597 corpus files plus 309 inline
occurrences in both materialized trees with an empty ok-to-error set; see the
migration design amendment for SHAs, command and attribution.
The initial extension audit uses BASE c79016efeb630c717292e9392901cfe65ab40915 and
implementation ceb03268e567487f6ec080afcf3ab034d4a5ea0c, built in separate clean
archives. Cache-disabled `fslc check FORM`, 597 corpus files plus eight affected
required-input occurrences per tree, both binaries in both trees: 2,420 checks,
empty ok-to-error and full stdout/stderr/exit difference sets, no excluded fields.
The repeated 21-mode consumer comparison executes 772 observations and records
17 distinct breaking command/source removals (19 control occurrences per repeat,
including repeated requirements metadata): domain analyze/expand/generate/replay/
testgen refuse enum union (5); document claims refuses metadata in spec and
requirements, colon quantifier and inline mapping (4); document generate refuses
metadata in spec and requirements (2); document check refuses requirements
metadata (1); replay --from-log refuses enum union, metadata in spec and
requirements, colon quantifier and inline mapping (5). These previously non-error
consumer inputs are not grammar removals or proof claims; the mapping-only control
was already an error at BASE and is not counted as a removal.
The sweep grid now uses the same admission owner as the other single-root modes,
retaining per-cell warning ownership without finishing the aggregate. Success
controls compare one complete finding where the dialect has migration forms and
require warning absence for AI/DB dialects without those forms. No planner or
language behavior is added by those controls.
