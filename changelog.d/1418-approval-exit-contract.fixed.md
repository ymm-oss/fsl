Fixed (#1418, breaking): Approval artifact failures preserve their complete downstream
JSON envelopes and exit codes through create and check.
Their exit moves from 2 to the downstream exit: 1 for scenarios or HTML verification
failures and 3 for internal failures. `ledger --approval` keeps exit 2 when an `html` or
`scenarios` record's rerender stops on a failed baseline, now with `result:"error"` naming
the record instead of that record's envelope, and leaves the `-o` file untouched. Approval status applies to completed evaluations.
`approved` confirms digest binding, not a verification pass, so `verify` must be gated
separately; an approval gate requires exit 0, `approval_check` and `approved`. Debug builds check final normal-JSON envelope/exit
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
`approval create --kind ledger` and `approval check` on a ledger record now return a
verification `result:"error"` with its envelope and exit (2, or 3 for internal). On a spec
that fails `check`, they previously reported an artifact mismatch and `drifted` at exit 0.
Approving a ledger with a failed or unestablished verdict stays allowed.
