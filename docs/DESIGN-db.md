# FSL DB Compatibility Dialect Design

Status: adopted for the MVP behind issues #122-#128.

## Decision

`dbsystem` is a frontend dialect, not a new verifier kernel. It parses database
schema, migration, artifact, and environment declarations into a typed IR, then
lowers them to the existing kernel state machine:

- `schema_version: SchemaVersion`
- `column_exists: Map<Column, Bool>`
- `column_backfilled: Map<Column, Bool>`
- `column_not_null: Map<Column, Bool>`
- one action per migration
- generated invariants for read/write compatibility and not-null/backfill order

This keeps the shared kernel unchanged and avoids unsupported state shapes such as
`Map<ArtifactVersion, Set<Column>>`. Static artifact capabilities are represented
by generated invariants and metadata, not by nested runtime state.

## Semantic Modes

### 1. Compatibility Snapshot

A snapshot is `(environment, schema_version)`. An environment declares a finite
schema range, and each `active` / `supported` / `may_exist` artifact may further
restrict itself with `when schema lo..hi`.

In the MVP, `environment schema lo..hi` means exactly the set of schema versions in
that environment that are reachable in the declared migration order. It does not
mean every Cartesian product of arbitrary schema and artifact versions; artifact
coexistence is explicit in the artifact windows.

Rules checked in snapshot mode:

- `all_active_reads_exist`: every applicable artifact read targets an existing column.
- `all_active_writes_exist`: every applicable artifact write targets an existing column.
- `removed_only_after_unused`: covered by the same read/write compatibility facts.
- `not_null_after_backfill`: a column can be `not_null` only after it exists and is
  backfilled.

### 2. Rollout Plan

Migrations are a single declared, monotonic sequence in the MVP. Each migration is
lowered to one kernel action guarded by `schema_version == from`, then updates the
column lifecycle maps and moves to `to`.

Rollout percentages are not probabilities. A `10% rollout` is modeled as a finite
coexistence window: both old and new artifacts may be present for the same schema
snapshot. A kill switch is modeled by widening or reintroducing the old artifact's
window, then rechecking compatibility.

### 3. Runtime Observation

Runtime observation is evidence, not proof. Future log adapters may emit
`observed_mismatch` findings such as declared-unused-but-observed, unsupported
artifact still observed, or legacy API still called. Absence from logs is not a
proof of unused; it requires an explicit observability coverage assumption.

### 4. Data Preservation / Rollback

Data preservation is outside the MVP. The intended model is refinement/simulation:
old and new schemas map to a common abstract row model, then preservation checks
whether observable data is retained. Rollback is `up; down` observational
equivalence under a bounded row model. Lossy rename/split/merge migrations must be
marked irreversible instead of silently accepted.

## MVP Syntax

```fsl
dbsystem UserDb {
  database app {
    schema 0
    table users {
      column id: Int present backfilled not_null;
      column display_name: Text absent;
    }
  }

  migration add_display_name from 0 to 1 {
    add users.display_name nullable;
  }
  migration backfill_display_name from 1 to 2 {
    backfill users.display_name;
  }
  migration require_display_name from 2 to 3 {
    set_not_null users.display_name;
  }

  artifact server_v2 {
    reads users.id, users.display_name;
    writes users.display_name;
  }

  environment prod {
    schema 0..3;
    active server_v2 when schema 1..3;
    supported server_v2 when schema 1..3;
  }

  check compatibility {
    rule all_active_reads_exist;
    rule all_active_writes_exist;
    rule removed_only_after_unused;
    rule not_null_after_backfill;
  }
}
```

`fslc check` and `fslc verify` accept `dbsystem` because it expands to a kernel
spec. `fslc db check` additionally returns fsl-db findings:

```bash
fslc db check examples/db/safe_add_nullable_column.fsl
fslc db check examples/db/safe_dual_write_backfill_switch_read_drop_old.fsl --engine induction
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

Current violation kinds:

- `column_removed_while_still_read`
- `column_removed_while_still_written`
- `not_null_before_backfill`

`repair_candidates` distinguish compatibility-preserving changes from spec
weakening with `weakens_spec: true|false`. Findings emit schema identifiers only;
row values, SQL literals, secrets, and production payloads are not logged.

Successful formal checks return `verified_under_assumptions`, not bare
`verified`, because the DB dialect relies on finite rollout windows and complete
capability declarations. Runtime evidence findings remain separate
(`observed_mismatch`) and are never presented as a formal proof.

## Discrete Rollout / TTL Modeling

FSL does not prove probability, percentages, wall-clock days, DB optimizer timing,
lock timing, or full production data coverage.

Use finite abstractions:

- percentage rollout -> old/new artifacts coexist over a schema window
- A/B test -> finite variant artifacts or actions
- kill switch -> old artifact window remains or returns
- `days(14)` / offline TTL -> bounded tick or TTL step with an assumption that one
  tick represents the operational interval

Assumptions reported by `fslc db check` include:

- `DB-ASSUME-ROLLING-SNAPSHOT`: schema ranges are finite reachable snapshots;
  percentages are coexistence windows
- `DB-ASSUME-CAPABILITY-DECLARATIONS`: artifact read/write declarations are
  complete for the checked window

Future runtime observation should add an observability coverage assumption when it
uses logs to support unused/unsupported claims.

## Out Of MVP

- SQL, Prisma, Rails, Drizzle, or other importers
- DB-engine-specific locking/optimizer semantics
- offline-client replay and TTL implementation
- runtime log adapters
- data preservation, rollback equivalence, destructive annotation enforcement
- rename/split/merge information-preservation proofs
