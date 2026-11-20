Documented (#1868, refs #1934): design notes and manuals now match the code.
`DESIGN-verification-cost.md` lists the `key_domain`, `semantics` and
`collection_support` cost kinds and every `partial_op` row name, and says that
with a `terminal` declaration BMC charges its deadlock probes to `partial_op` /
`terminal`. `DESIGN-explain.md` cites `main.rs` by function and test name
instead of drifting line numbers. `DESIGN-html-report.md` describes the native
report inputs and keeps the Python functions as the frozen reference. A
counterfactual's `violation.cost.elapsed_s` is documented as the time of its
weakening's whole analysis, not of that invariant alone. No behavior changes.
