# fsl-db examples

These files exercise the `dbsystem` dialect. The dialect lowers database
schema/artifact/environment compatibility into the existing FSL kernel and
`fslc db check` translates violations back into stable fsl-db findings.

Run:

```bash
fslc db check examples/db/safe_dual_write_backfill_switch_read_drop_old.fsl --engine induction
fslc db check examples/db/unsafe_drop_column_with_old_server.fsl
fslc db check examples/db/unsafe_api_response_field_removed.fsl
fslc db check examples/db/safe_feature_flag_kill_switch_compat.fsl
fslc db observe examples/db/runtime_observation_target.fsl --trace examples/db/runtime_observation_mismatch.json
fslc db import examples/db/minimal_import.sql --name ImportedFromSql
fslc db import examples/db/minimal_prisma_schema.prisma --name ImportedFromPrisma
```

Coverage highlights:

- safe and unsafe read/write compatibility across environment windows
- destructive drop annotation enforcement
- bounded rename/split/merge preservation and rollback checks
- API response and offline payload compatibility across artifacts
- finite feature flag variants and kill-switch compatibility windows
- runtime observation evidence separated from formal verification
- minimal SQL DDL and Prisma schema import with explicit unsupported-construct warnings
- external preservation and engine evidence fixtures that keep `formal_result: "not_run"`

The model is intentionally finite: schema ranges are reachable rollout
snapshots, feature flags are declared variants, offline TTLs are logical ticks,
and preservation/rollback checks use a bounded abstract row model rather than
full production-data proof.
