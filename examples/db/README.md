# fsl-db MVP examples

These files exercise the `dbsystem` dialect. The dialect lowers database
schema/artifact/environment compatibility into the existing FSL kernel and
`fslc db check` translates violations back into stable fsl-db findings.

Run:

```bash
fslc db check examples/db/safe_dual_write_backfill_switch_read_drop_old.fsl --engine induction
fslc db check examples/db/unsafe_drop_column_with_old_server.fsl
```

The MVP intentionally excludes SQL/ORM importers, rollback equivalence, runtime
observation, and data-preservation proofs; see `docs/DESIGN-db.md`.
