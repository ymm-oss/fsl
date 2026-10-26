Documented (#251): Move the cancellation example's four action correspondences
into its local `implements` block while preserving its checked Kernel and typed
requirement metadata. `docs/design/DESIGN-migration.md` records the manually
applied edits and that whole-file `fmt --check --edition next` still exits 1,
as it does for the original example.
