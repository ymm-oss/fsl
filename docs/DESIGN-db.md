# FSL DB / Multi-Environment Compatibility Dialect Design

Status: adopted. The first slice shipped issues #122-#128; bounded
compatibility extensions cover issues #129-#134, and DB/multi-environment
follow-ups cover issues #144-#147.

## Decision

`dbsystem` is a frontend dialect, not a new verifier kernel. It parses database
schema, migration, artifact, API/offline, observation, importer, and environment
declarations into a typed IR, then lowers the DB lifecycle portion to the
existing kernel state machine:

- `schema_version: SchemaVersion`
- `column_exists: Map<Column, Bool>`
- `column_backfilled: Map<Column, Bool>`
- `column_not_null: Map<Column, Bool>`
- one generated action per migration, or a no-op snapshot action when no
  migration is needed
- generated invariants for read/write compatibility and not-null/backfill order

The remaining compatibility dimensions are checked in the fsl-db layer and
reported with the same stable finding envelope. This keeps the shared kernel
unchanged and avoids unsupported state shapes such as
`Map<ArtifactVersion, Set<Column>>`. Static artifact capabilities are represented
by generated invariants and metadata, not by nested runtime state.

## Semantic Modes

### 1. Compatibility Snapshot

A snapshot is `(environment, schema_version)`. An environment declares a finite
schema range, and each `active` / `supported` / `may_exist` artifact may further
restrict itself with `when schema lo..hi`.

`environment schema lo..hi` means exactly the set of schema versions in that
environment that are reachable in the declared migration order. It does not mean
every Cartesian product of arbitrary schema and artifact versions; artifact
coexistence is explicit in the artifact windows.

Rules checked in snapshot mode:

- `all_active_reads_exist`: every applicable artifact read targets an existing
  column.
- `all_active_writes_exist`: every applicable artifact write targets an existing
  column.
- `removed_only_after_unused`: covered by the same read/write compatibility
  facts.
- `not_null_after_backfill`: a column can be `not_null` only after it exists and
  is backfilled.
- `api_calls_accepted`: an artifact call must be accepted by an active or
  supported provider in the same environment snapshot.
- `api_responses_expected`: an expected response field must be produced by an
  active or supported provider in the same environment snapshot.
- `offline_payloads_accepted`: an offline payload that may be emitted by a
  client must still be accepted by an active or supported provider during the
  declared finite TTL window.

### 2. Rollout Plan

Migrations are a single declared, monotonic sequence. Each migration is lowered
to one kernel action guarded by `schema_version == from`, then updates the column
lifecycle maps and moves to `to`.

Rollout percentages are not probabilities. A `10% rollout` is modeled as a
finite coexistence window: both old and new artifacts may be present for the same
schema snapshot. A kill switch is modeled by widening or reintroducing the old
artifact's window, then rechecking compatibility.

Destructive operations must be explicit. A `drop` of an existing column requires
`destructive` or `irreversible`; this annotation is operational approval
metadata, not a spec weakening. An annotated drop can still fail read/write/API
compatibility.

### 3. Feature-Flag Snapshots

Feature flags are first-class finite environment dimensions. They are not
probabilities, percentage-rollout proofs, or experimentation population models.
An environment may declare finite variants:

```fsl
environment prod {
  schema 0..1;
  flag email_v2 { off, on } default off;
  active server_legacy when schema 0..1 when flag email_v2=off;
  active server_new when schema 1..1 when flag email_v2=on;
  may_exist ios_new when schema 1..1 when flag email_v2=on;
}
```

`fslc db check` enumerates `(environment, schema_version, flag variants)` and
applies the same DB/API/offline compatibility rules inside each finite snapshot.
Existing artifact/window modeling remains the simple form: omit `flag` and
`when flag` when rollout variants are irrelevant.

A kill switch is checkable by keeping or reintroducing the old artifact's
`when flag ...=off` window. The result includes
`DB-ASSUME-FINITE-FLAG-STATE` whenever any environment declares flags.

### 4. Preservation and Rollback

`data_preserved` and `rollback_equivalent` are bounded abstract checks, not full
production-data proofs. They model whether the migration preserves observable
row information in a finite abstract row model and whether a `rollbackable`
migration has an observationally equivalent `up; down` shape.

Supported preservation transforms:

- `rename old to new`
- `split source into target_a, target_b, ...`
- `merge source_a, source_b, ... into target`

`split` and `merge` must declare whether the mapping is `lossless`, `lossy`, or
`irreversible`. A lossy transform can be operationally intentional, but it still
violates `data_preserved`; the annotation prevents silent acceptance.

Production-data preservation evidence can be attached outside the formal result
using `schemas/fslc/db/preservation-evidence.v0.schema.json`. Supported evidence
families include sampled/offline diff jobs, shadow reads, dual-read comparisons,
and post-migration audits. These results use `formal_result: "not_run"` and
statuses such as `evidence_supported` or `evidence_failed`; they must not be
reported as `verified` or `proved`.

Evidence payloads must identify schemas, tables, columns, migration IDs, sample
counts, and aggregate mismatch counts only. Row values, SQL literals, secrets,
payload bodies, and raw production records are outside the schema and must be
redacted before storage.

### 5. Runtime Observation

Runtime observation is evidence, not proof. `fslc db observe` compares an
observation log to a `dbsystem` and emits `observed_mismatch` findings such as:

- `declared_unused_but_observed`
- `unsupported_artifact_observed`
- `legacy_api_still_called`

Absence from logs is not proof of unused behavior. Observation results include
`DB-ASSUME-OBSERVABILITY-COVERAGE` and `formal_result: "not_run"` to keep them
separate from formal compatibility verification.

### 6. Importer Boundary

`fslc db import` provides a deliberately small SQL DDL importer to establish the
typed IR boundary. It supports:

- `CREATE TABLE`
- `ALTER TABLE ... ADD COLUMN`
- `ALTER TABLE ... DROP COLUMN`
- `ALTER TABLE ... RENAME COLUMN ... TO ...`
- `UPDATE ... SET ...` as a backfill signal

Unsupported constructs are reported as `unsupported_sql` warnings and are not
silently ignored.

The first ORM-specific importer is `prisma-schema-minimal.v0`. It imports
Prisma `model` scalar fields into the same typed IR and reports relation/list or
model-level constructs as `unsupported_prisma` warnings. Additional importers
(Rails, Drizzle, Django, Alembic, vendor DSLs) must follow the same rule:
source-specific constructs either become fsl-db IR or explicit warnings; no DB
engine runtime semantics are inferred without a separate evidence artifact.

### 7. DB-Engine Evidence Boundary

fsl-db formal compatibility is engine-agnostic by default. It does not model:

- lock acquisition order, lock wait timing, or online DDL blocking behavior
- optimizer plans, index selection, query latency, or vacuum/analyze effects
- transaction isolation anomalies or vendor-specific migration executor details
- cloud-provider maintenance windows or wall-clock migration duration

Operational engine evidence is represented separately with
`schemas/fslc/db/engine-evidence.v0.schema.json`. A concrete adapter shape is:

```json
{
  "schema_version": "fsl-db-engine-evidence.v0",
  "formal_result": "not_run",
  "engine": {"vendor": "postgresql", "version": "16"},
  "adapter": {"kind": "migration_dry_run", "tool": "vendor-online-ddl-checker"},
  "migration": "add_email_normalized",
  "checks": [
    {"kind": "lock_timeout", "status": "passed", "max_wait_ms": 5000},
    {"kind": "online_ddl_validator", "status": "passed"}
  ],
  "assumptions": [
    {"id": "DB-ENGINE-ASSUME-STAGING-LIKE-PROD", "text": "dry-run environment matches production engine settings relevant to this check"}
  ],
  "redaction": {"policy": "engine evidence contains identifiers and aggregate timings only"}
}
```

Engine evidence may support or reject an operational rollout plan, but it is not
a kernel proof and must not change `fslc db check` from
`verified_under_assumptions` to `verified`/`proved`.

## Syntax

```fsl
dbsystem UserDb {
  database app {
    schema 0
    table users {
      column id: Int present backfilled not_null;
      column legacy_name: Text present backfilled nullable;
      column display_name: Text absent;
    }
  }

  migration rename_legacy_name from 0 to 1 rollbackable {
    rename users.legacy_name to users.display_name;
  }
  migration split_full_name from 1 to 2 {
    split users.display_name into users.first_name, users.last_name lossy;
  }
  migration drop_legacy_name from 2 to 3 {
    drop users.legacy_name irreversible;
  }

  artifact server_v2 {
    reads users.id, users.display_name;
    writes users.display_name;
    accepts api.CreateUserV1, api.CreateUserV2;
    responds response.email;
  }

  artifact ios_v1 {
    calls api.CreateUserV1;
    emits_offline api.CreateUserV1 ttl 2;
    expects response.email;
  }

  environment prod {
    schema 0..3;
    flag email_v2 { off, on } default off;
    active server_v2 when schema 1..3 when flag email_v2=on;
    may_exist ios_v1 when schema 0..3;
  }

  check compatibility {
    rule all_active_reads_exist;
    rule all_active_writes_exist;
    rule removed_only_after_unused;
    rule not_null_after_backfill;
    rule destructive_operations_annotated;
    rule preservation_transforms_annotated;
    rule api_calls_accepted;
    rule api_responses_expected;
    rule offline_payloads_accepted;
    rule data_preserved;
    rule rollback_equivalent;
  }
}
```

If `check compatibility` is omitted, the default rule set includes the DB
read/write lifecycle rules, destructive annotation enforcement, preservation
annotation enforcement, and API/offline compatibility. `data_preserved` and
`rollback_equivalent` remain opt-in because they add the bounded row-model
assumption.

## CLI

`fslc check` and `fslc verify` accept `dbsystem` because it expands to a kernel
spec. `fslc db check` additionally returns fsl-db findings:

```bash
fslc db check examples/db/safe_add_nullable_column.fsl
fslc db check examples/db/safe_dual_write_backfill_switch_read_drop_old.fsl --engine induction
fslc db observe examples/db/runtime_observation_target.fsl --trace examples/db/runtime_observation_mismatch.json
fslc db import examples/db/minimal_import.sql --name ImportedFromSql -o /tmp/imported.fsl
fslc db import examples/db/minimal_prisma_schema.prisma --name ImportedFromPrisma
```

## Finding Contract

The stable finding schema is `fsl-db-finding.v0` (see
`schemas/fslc/db/finding.v0.schema.json`). Required fields:

- `fsl`: dialect/schema version
- `result`
- `kind`
- `severity`
- `environment`
- `migration`
- `schema_element`
- `artifact`
- `artifact_version`
- `failed_rule`
- `witness`
- `minimal_conflict_set`
- `repair_candidates`
- `assumptions`

Violation kinds currently include:

- `column_removed_while_still_read`
- `column_removed_while_still_written`
- `not_null_before_backfill`
- `destructive_migration_unannotated`
- `preservation_transform_unannotated`
- `data_preservation_loss`
- `rollback_not_equivalent`
- `api_call_not_accepted`
- `api_response_field_missing`
- `offline_payload_not_accepted`

Observation-only kinds currently include:

- `declared_unused_but_observed`
- `unsupported_artifact_observed`
- `legacy_api_still_called`

`repair_candidates` distinguish compatibility-preserving changes from spec
weakening with `weakens_spec: true|false`. For destructive operations, adding an
annotation and adding a compatibility shim are separate candidates. Findings emit
schema identifiers only; row values, SQL literals, secrets, and production
payloads are not logged.

Successful formal checks return `verified_under_assumptions`, not bare
`verified`, because the DB dialect relies on finite rollout windows and complete
capability declarations. Runtime evidence findings remain separate
(`observed_mismatch`) and are never presented as a formal proof.

## Discrete Rollout / TTL Modeling

FSL does not prove probability, percentages, wall-clock days, DB optimizer
timing, lock timing, or full production data coverage.

Use finite abstractions:

- percentage rollout -> old/new artifacts coexist over a schema window
- A/B test -> finite variant artifacts or actions
- kill switch -> old artifact window remains or returns
- offline TTL -> finite logical ticks declared on `emits_offline ... ttl N`

Assumptions reported by `fslc db check` may include:

- `DB-ASSUME-ROLLING-SNAPSHOT`: schema ranges are finite reachable snapshots;
  percentages are coexistence windows
- `DB-ASSUME-CAPABILITY-DECLARATIONS`: artifact capability declarations are
  complete for the checked window
- `DB-ASSUME-OFFLINE-TTL-FINITE`: offline TTL values are finite logical ticks
- `DB-ASSUME-BOUNDED-ROW-MODEL`: preservation and rollback checks use a bounded
  abstract row model
- `DB-ASSUME-FINITE-FLAG-STATE`: feature flags are finite declared variants,
  not percentage/probability proofs

`fslc db observe` additionally reports:

- `DB-ASSUME-OBSERVABILITY-COVERAGE`: logs are evidence only; absence from logs
  does not prove unused behavior

## Remaining Boundaries

- probability, wall-clock time, and percentile reasoning
- production-data evidence beyond aggregate/sample/audit artifacts
- DB-engine behavior beyond explicit external evidence adapters
- ORM/vendor importers beyond SQL and the first minimal Prisma schema importer
