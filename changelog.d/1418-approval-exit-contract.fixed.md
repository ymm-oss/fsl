Fixed (#1418): Approval artifact failures preserve their complete downstream
JSON envelopes and exit codes through create, check and ledger overlays.
Approval status applies to completed evaluations; usable approval requires exit 0,
`approval_check` and `approved`. Debug builds check final normal-JSON envelope/exit
pairs after internal normalization, with finite HTML/ledger report exceptions;
release and dedicated raw/metadata/fmt delivery retain their existing behavior.
Aborted scenarios approval results attach each root migration finding and each
finding from a distinct dependency resource once; successful artifact bytes,
digests and approval signatures retain their existing form.
The debug exit check recognizes the existing `migrate_manual_required` result
at exactly exit 1, preserving its JSON envelope and no-write behavior.
Batch diff's debug exit check preserves its existing maximum child exit (0/1/2),
separately from the semantic gate, including operational errors at exit 2.
The design-citation CI scanner honors Git's declared binary/nontext attributes
without ignoring malformed UTF-8 text or relaxing heading and tracked-target checks.
