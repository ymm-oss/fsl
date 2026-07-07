# FSL DB / Multi-Environment Compatibility Verification Proposal

作成日: 2026-07-06

対象: FSL / `fslc` の拡張提案

提案名: **fsl-db / fsl-compat dialect**

目的: DBスキーマ、マイグレーション、アプリケーションバージョン、多環境デプロイを同一の状態遷移系として扱い、AIが検証・修復に利用できる診断を返す。

> Status: この提案から採用したMVP設計は [`DESIGN-db.md`](DESIGN-db.md) に固定した。
> `environment schema lo..hi` は宣言順migration planで到達可能な有限snapshot群を意味し、
> rollout percentage / TTL / runtime observation / data preservation は同文書の仮定・後続フェーズ境界に従う。

---

## 1. 要約

FSLに、DBスキーマとマイグレーション、およびサーバー・モバイル・Web・worker・batch・外部クライアントなどの多環境互換性を検証するための dialect を追加する。

設計の中心は、DBやアプリの「バージョン番号」ではなく、各バージョンが提供または要求する **capability** である。

```text
schema migration = DB状態を変えるaction
application deployment = 実行環境状態を変えるaction
feature flag / rollout = capabilityの有効範囲を変えるaction
compatibility = active artifactのrequired capabilitiesがenvironmentのprovided capabilitiesに含まれること
```

この設計により、以下の事故をFSLで検出できるようにする。

- NOT NULL制約をbackfill前に追加する。
- UNIQUE制約を重複データ排除前に追加する。
- 旧サーバー、旧worker、旧モバイルアプリがまだ読むカラムをdropする。
- 新サーバーが書くカラムをDB migration前に参照する。
- mobile offline writeが旧payloadで同期される可能性があるのに、新サーバーが旧APIを拒否する。
- 旧API response fieldをまだ期待するモバイルアプリが残っているのに、サーバーがfieldを返さなくなる。
- rename / split / merge migrationで、アプリケーション上の意味が保存されていない。
- rollback可能と宣言しているmigrationが、実際には不可逆なデータ損失を起こす。

FSL本体の検証kernelを拡張するのではなく、既存のFSL設計に合わせて、dialect frontendからshared kernelへdesugarする。

---

## 2. 背景と前提

FSLは、公開ドキュメント上では、business / requirements / design の三層dialectをshared kernelへ展開する設計を持つ。shared kernelはbounded transition system、invariant、reachable、leadsTo、fairness、BMC、k-induction、scenarios、refinement、compose、JSON repair protocol、Monitor/replay/testgenを扱う構成として説明されている。

この方向性に対して、DB migrationは非常に相性がよい。

```text
DB schema        = state
migration        = action
migration order  = transition relation
schema rule      = invariant
data preservation = refinement / simulation
runtime log      = replay / conformance input
AI repair        = JSON finding + repair candidates
```

ただし、DB migrationを実用的に検証するには、DB単体を見るだけでは不十分である。実運用での破壊は、しばしば次のような組合せで発生する。

```text
DB schema version
× server version
× mobile app version
× web frontend version
× worker / batch version
× external client version
× feature flag
× rollout percentage
× offline sync window
```

したがって、FSLに追加すべきなのは単なるDB schema dialectではなく、**DB + multi-environment compatibility verification dialect** である。

---

## 3. 設計原則

### 3.1 Kernelを太らせない

FSLの既存方針に合わせて、DBや多環境の概念はdialect frontendで受け取り、既存kernelの `state` / `action` / `invariant` / `trans` / `reachable` / `leadsTo` / `refinement` へ展開する。

```text
fsl-db / fsl-compat dialect
  database / table / column / migration / artifact / environment / capability
      ↓ expand
FSL shared kernel
  enum / domain / state / action / invariant / trans / refinement / scenario
```

### 3.2 バージョン番号ではなくcapabilityを中心にする

悪い設計:

```text
app_version >= 2.4 なら安全
```

良い設計:

```text
この環境でactiveなartifactは users.email をreadしない
この環境でactiveなartifactは CreateUserV1 payloadを送らない
この環境でactiveなserverは CreateUserV1 をまだacceptする
このschemaでは users.email_normalized がreadableである
```

バージョン番号は、capability profileのラベルにすぎない。

### 3.3 「存在する」と「使ってよい」を分ける

DB要素について、以下は別概念である。

```text
column exists
column is readable
column is writable
column is queryable with acceptable index support
column is required / not null
column is deprecated for write
column is deprecated for read
column is unused
column is removable
column is removed
```

単に `introduced_in` / `deprecated_in` だけでは粗い。FSLはこれらを区別して検証できるべきである。

### 3.4 多環境は後付けではなく第一級概念にする

DB migrationの正しさは、DBだけでは決まらない。

```text
serverが何を読むか
mobile appが何を送るか
workerが何を参照するか
batchがいつまで旧カラムを使うか
external clientがどのAPI contractを期待するか
feature flagがどちらのread pathを有効にするか
```

これらを表現できないDB migration verificationは、実用上の重要な事故を見逃す。

### 3.5 AI-readable findingを第一成果物にする

FSLはAIが仕様を書き、検証結果を読み、修復する流れと相性がよい。したがって、DB互換性検証も人間向けテキストだけでなく、AIが修復に使えるJSONを第一成果物にする。

---

## 4. 対象範囲

### 4.1 対象にするもの

- DB schema
  - database
  - table
  - column
  - type
  - nullable / not null
  - default
  - primary key
  - foreign key
  - unique constraint
  - index
  - enum values
  - view / generated column / triggerは後続phaseで対応
- Migration
  - add / drop / rename / alter column
  - add / drop index
  - add / drop constraint
  - backfill
  - data transform
  - up / down migration
  - destructive change declaration
- Application artifacts
  - server
  - web frontend
  - iOS app
  - Android app
  - worker
  - batch
  - admin console
  - external client
- API contracts
  - request payload
  - response payload
  - required / optional / deprecated fields
  - accepted versions
- Deployment environment
  - production / stagingなど
  - active app versions
  - supported client versions
  - may-exist client versions
  - feature flags
  - rollout windows
  - offline write TTL
- Runtime observation
  - DB access log
  - API access log
  - event log
  - application version telemetry
  - declared unusedとobserved usageの矛盾検出

### 4.2 最初は対象にしないもの

- SQL engine固有の完全なロック意味論
- DB optimizerやクエリ性能の完全保証
- 本番全データに対する完全証明
- Prisma / Rails / Drizzle / SQL parserの完全実装
- distributed transactionの完全モデル
- online schema change toolの置き換え

ただし、これらを抽象化したrule packとして扱う余地は残す。

---

## 5. 中核概念

### 5.1 DB要素のlifecycle

DB要素ごとに、読み・書き・必須化・削除可能性を分けて表現する。

```text
exists_from
readable_from
writable_from
queryable_from
index_available_from
constraint_enforced_from
required_from
write_deprecated_in
read_deprecated_in
reference_deprecated_in
unused_from
removable_from
removed_in
```

意味:

| 項目 | 意味 |
|---|---|
| `exists_from` | schema上、要素が存在し始めるmigrationまたはschema version |
| `readable_from` | appがread sourceとして使ってよい時点 |
| `writable_from` | appがwrite targetとして使ってよい時点 |
| `queryable_from` | query条件やjoin対象として使ってよい時点 |
| `index_available_from` | indexを前提にしたqueryを出してよい時点 |
| `constraint_enforced_from` | DB制約としてenforceされる時点 |
| `required_from` | appやDBが必須値として扱ってよい時点 |
| `write_deprecated_in` | 新規write targetとして使うべきではなくなる時点 |
| `read_deprecated_in` | primary read sourceとして使うべきではなくなる時点 |
| `reference_deprecated_in` | API / job / query / external contractで参照すべきではなくなる時点 |
| `unused_from` | active artifactからの参照が存在しないと宣言できる時点 |
| `removable_from` | dropしても互換性を壊さないとみなせる時点 |
| `removed_in` | 実際にschemaから削除される時点 |

### 5.2 Capability

Capabilityは、artifactやschemaが「要求するもの」または「提供するもの」を表す。

代表例:

```text
DBRead(table.column)
DBWrite(table.column)
DBQuery(table.column)
DBRequireNotNull(table.column)
DBProvideColumn(table.column)
DBProvideIndex(index)
DBEnforceConstraint(constraint)
APICall(endpoint, request_schema)
APIAccept(endpoint, request_schema)
APIRespond(endpoint, response_schema)
APIExpectField(response.field)
OfflineEmit(request_schema, ttl)
FeatureFlagProvide(flag, variant)
```

互換性の一般形:

```text
forall artifact in active_artifacts:
  artifact.required_capabilities ⊆ environment.provided_capabilities
```

ただし、fallbackやconditional capabilityがあるため、実装上は単純な集合包含ではなく、条件付き述語としてdesugarする。

### 5.3 Artifact

Artifactは、実行環境上に存在しうるアプリケーション単位である。

```text
server
web
mobile_ios
mobile_android
worker
batch
admin_console
external_client
```

Artifact versionは、capability profileを持つ。

```text
artifact ServerApp server_v3_1 {
  reads users.email_normalized
  writes users.email_normalized
  accepts api CreateUserV2
  responds api UserResponseV2
}

artifact iOSApp ios_v2_1 {
  calls api CreateUserV1
  expects response UserResponseV1
  emits_offline api CreateUserV1 ttl days(14)
}
```

### 5.4 Environment

Environmentは、ある時点で混在しうるschema、artifact、feature flag、rollout状態を表す。

```text
environment Production {
  schema schema_v20

  active server ServerApp {
    versions server_v3_1, server_v3_2
  }

  may_exist mobile iOSApp {
    versions ios_v2_0..ios_v3_0
  }

  supported mobile iOSApp {
    versions ios_v2_4..ios_v3_0
  }

  active worker UserWorker {
    versions worker_v1_8
  }

  active batch LegacyExportJob {
    versions batch_v1_2
  }

  flag UseNormalizedEmail = gradual(0..100)
}
```

`active` は確実に存在するもの、`supported` は互換性を保証すべきもの、`may_exist` は現実に残っている可能性があるものを表す。モバイルアプリや外部クライアントでは `may_exist` が重要になる。

---

## 6. 提案DSL例

以下は構文案であり、FSL本体の最終構文ではない。重要なのは、これらをshared kernelへ機械的に展開できることである。

```fsl
dbsystem UserEmailCompatibility {
  database AppDB {
    table users {
      id: uuid primary

      column email: text {
        exists_from schema_v1
        write_deprecated_in server_v3_0
        read_deprecated_in server_v3_1
        unused_from server_v3_2
        removable_from schema_v25
      }

      column email_normalized: text {
        exists_from schema_v20
        writable_from server_v3_0
        readable_from server_v3_1
        required_from schema_v23
      }
    }
  }

  api CreateUserV1 {
    request {
      email: text required
    }
  }

  api CreateUserV2 {
    request {
      email_normalized: text required
      email: text optional deprecated
    }
  }

  api UserResponseV1 {
    response {
      email: text required
    }
  }

  api UserResponseV2 {
    response {
      email_normalized: text required
      email: text optional deprecated
    }
  }

  artifact ServerApp server_v2_9 {
    reads users.email
    writes users.email
    accepts api CreateUserV1
    responds api UserResponseV1
  }

  artifact ServerApp server_v3_0 {
    reads users.email fallback users.email_normalized
    writes users.email
    writes users.email_normalized
    accepts api CreateUserV1
    accepts api CreateUserV2
    responds api UserResponseV1
    responds api UserResponseV2
  }

  artifact ServerApp server_v3_2 {
    reads users.email_normalized
    writes users.email_normalized
    accepts api CreateUserV2
    accepts api CreateUserV1 until days_after(deploy_server_v3_2, 14)
    responds api UserResponseV2
  }

  artifact iOSApp ios_v2_1 {
    calls api CreateUserV1
    expects response UserResponseV1
    emits_offline api CreateUserV1 ttl days(14)
  }

  artifact iOSApp ios_v3_0 {
    calls api CreateUserV2
    expects response UserResponseV2
  }

  artifact UserWorker worker_v1_8 {
    reads users.email
  }

  migration AddNormalizedEmail {
    from schema_v19
    to schema_v20
    add column users.email_normalized text nullable
  }

  migration BackfillNormalizedEmail {
    from schema_v20
    to schema_v21
    backfill users.email_normalized from normalize(users.email)
    ensure no_null users.email_normalized
  }

  migration RequireNormalizedEmail {
    from schema_v21
    to schema_v23
    require no_null users.email_normalized
    alter column users.email_normalized set not_null
  }

  migration DropLegacyEmail {
    from schema_v24
    to schema_v25
    require no_active_reads users.email
    require no_active_writes users.email
    require preserved_as users.email_normalized by NormalizedEmailPreservesEmail
    drop column users.email
  }

  preservation NormalizedEmailPreservesEmail {
    old users.email = denormalize(new users.email_normalized)
  }

  environment ProductionDuringMigration {
    schema schema_v20..schema_v25

    active server ServerApp {
      versions server_v3_0..server_v3_2
    }

    may_exist mobile iOSApp {
      versions ios_v2_1..ios_v3_0
    }

    active worker UserWorker {
      versions worker_v1_8
    }
  }

  check compatibility ProductionDuringMigration {
    require all_active_reads_exist
    require all_active_writes_exist
    require api_request_accepted
    require api_response_expected_fields_present
    require offline_emits_accepted_until_ttl
    require deprecated_not_newly_written
    require removed_only_after_unused
  }
}
```

---

## 7. Kernelへのdesugar方針

### 7.1 Schema version

```fsl
state schema_version: SchemaVersion
```

### 7.2 Column lifecycle

各columnについて、存在・読み取り可能・書き込み可能・必須・削除済みなどを状態または派生述語にする。

```fsl
state column_exists: Map<Column, Bool>
state column_readable: Map<Column, Bool>
state column_writable: Map<Column, Bool>
state column_required: Map<Column, Bool>
state column_removed: Map<Column, Bool>
```

または、有限enumとして持つ。

```fsl
enum ColumnPhase = Missing | Exists | Writable | Readable | Required | Deprecated | Unused | Removed
state column_phase: Map<Column, ColumnPhase>
```

ただし、read/write/deprecatedを独立に扱うため、最終的には複数Boolまたはbitset的表現のほうがよい。

### 7.3 Artifact capabilities

artifact versionはdomain値として定義し、各capabilityをpredicateまたはMapで表現する。

```fsl
domain ArtifactVersion
state active: Map<ArtifactVersion, Bool>

const requires_read: Map<ArtifactVersion, Set<Column>>
const requires_write: Map<ArtifactVersion, Set<Column>>
const accepts_api: Map<ArtifactVersion, Set<ApiContract>>
const calls_api: Map<ArtifactVersion, Set<ApiContract>>
```

### 7.4 Compatibility invariant

```fsl
invariant ActiveReadsExist {
  forall a: ArtifactVersion {
    active[a] => forall c: Column {
      c in requires_read[a] => column_exists[c] && column_readable[c]
    }
  }
}

invariant ActiveWritesExist {
  forall a: ArtifactVersion {
    active[a] => forall c: Column {
      c in requires_write[a] => column_exists[c] && column_writable[c]
    }
  }
}
```

### 7.5 Offline sync invariant

```fsl
invariant OfflineWritesRemainAccepted {
  forall client: ArtifactVersion {
    may_exist[client] && emits_offline_legacy[client] =>
      server_accepts_legacy_until >= offline_write_ttl[client]
  }
}
```

### 7.6 Drop safety

```fsl
invariant RemovedColumnHasNoActiveReference {
  forall c: Column {
    column_removed[c] => forall a: ArtifactVersion {
      active[a] => !(c in requires_read[a]) && !(c in requires_write[a])
    }
  }
}
```

### 7.7 Migration as action

```fsl
action DropLegacyEmail {
  requires schema_version == schema_v24
  requires no_active_reads(users_email)
  requires no_active_writes(users_email)
  requires data_preserved(users_email, users_email_normalized)

  schema_version = schema_v25
  column_exists[users_email] = false
  column_removed[users_email] = true
}
```

---

## 8. 検証ルール

### 8.1 Static schema rules

- table参照が存在する。
- column参照が存在する。
- primary keyが存在する。
- foreign key参照先が存在する。
- index対象columnが存在する。
- unique constraint対象columnが存在する。
- enum value削除は既存データとapp contractを壊さない。
- generated column / view / triggerは参照先の存在を要求する。

### 8.2 Migration order rules

- `nullable -> not_null` は `no_null` または `default` または `backfill` を要求する。
- unique constraint追加は、重複排除または重複不在の証明を要求する。
- foreign key追加は、既存データが参照整合性を満たすことを要求する。
- drop columnは、active artifactからのread/write/referenceがないことを要求する。
- renameは、drop + addとして扱う場合、data preservation mappingを要求する。
- type narrowingは、既存値が新型範囲に収まることを要求する。
- destructive changeは、明示的な `destructive accepted` または `irreversible` 注釈を要求する。

### 8.3 Multi-environment compatibility rules

- active serverが読むDB要素は、現在のschemaで存在しreadableである。
- active serverが書くDB要素は、現在のschemaで存在しwritableである。
- worker / batch / admin console / external clientも同じread/write規則を満たす。
- mobile appが送るAPI requestは、active serverがacceptする。
- mobile appが期待するAPI response fieldは、server response contractに存在する。
- `may_exist` な古いmobile appがoffline writeをemitしうる期間中、serverは旧requestを受け付ける。
- feature flagの全variantで互換性を検証する。
- gradual rollout中は旧pathと新pathの両方が到達可能とみなす。

### 8.4 Data preservation rules

- split columnは、旧値と新値集合の対応を示すmappingを要求する。
- merge columnは、情報損失がないか、損失を明示的に許容する必要がある。
- normalize / denormalizeは、抽象モデル上の等価性を要求する。
- soft delete導入は、既存query semanticsを壊さないことを要求する。
- tenant_id追加は、既存行がtenantへ正しく割り当てられることを要求する。

### 8.5 Rollback rules

- `rollbackable` と宣言されたmigrationは、`up; down` 後に観測可能等価性を満たす。
- データ損失を起こすdown migrationは、`irreversible` または `lossy` を明示する。
- drop後に再作成するだけのdown migrationは、データ復元がない限りrollbackableではない。

---

## 9. API contractとの接続

DB migrationとAPI互換性は分離しない。多くの移行事故は、DB要素の変更がAPI contractに波及して起こる。

例:

```text
DB: users.email を users.email_normalized に移行
Server: UserResponse.email を返さなくなる
Mobile: iOS v2.1 は UserResponse.email をrequiredとして期待する
```

この場合、DB schemaとしては正しくても、multi-environment compatibilityとしては違反である。

必要な検証:

```text
client.calls_api ⊆ server.accepts_api
client.expected_response_fields ⊆ server.provided_response_fields
server.response_mapping はDB schema上のreadable fieldsから構成できる
server.request_mapping はDB schema上のwritable fieldsへ保存できる
```

---

## 10. Feature flag / rolloutの扱い

Feature flagは、単なる設定値ではなく、到達可能なread/write pathを変える状態である。

```fsl
flag UseNormalizedEmail {
  variants OldPath, DualPath, NewPath
}

artifact ServerApp server_v3_0 when UseNormalizedEmail == OldPath {
  reads users.email
  writes users.email
}

artifact ServerApp server_v3_0 when UseNormalizedEmail == DualPath {
  reads users.email fallback users.email_normalized
  writes users.email
  writes users.email_normalized
}

artifact ServerApp server_v3_0 when UseNormalizedEmail == NewPath {
  reads users.email_normalized
  writes users.email_normalized
}
```

検証上は、rollout中に到達可能な全variantを考慮する。

```text
10% rollout = OldPathとNewPathが同時に存在しうる
A/B test = variant Aとvariant Bが同時に存在しうる
kill switch = 旧pathへ戻るactionが存在する
```

---

## 11. Runtime observationとの接続

宣言されたlifecycleは、実行ログと照合できるべきである。

例:

```text
users.email は unused_from server_v3_2 と宣言されている
しかし過去7日間のDB access logに worker_v1_8 による users.email read がある
```

この場合、形式仕様そのものは一貫していても、実運用とは不一致である。

FSLのreplay / Monitor系に接続して、以下のfindingを出す。

```json
{
  "kind": "observed_deprecated_usage",
  "severity": "warning",
  "element": "users.email",
  "declared_unused_from": "server_v3_2",
  "observed_usage": {
    "artifact": "UserWorker",
    "version": "worker_v1_8",
    "operation": "read",
    "last_seen": "2026-07-05T02:13:00+09:00"
  },
  "interpretation": "the lifecycle declaration is inconsistent with runtime evidence",
  "repair_candidates": [
    "delay DropLegacyEmail",
    "update worker_v1_8 to stop reading users.email",
    "change unused_from to a later version",
    "keep users.email as generated column until observed usage disappears"
  ]
}
```

---

## 12. CLI案

```bash
# dialect syntax / static checks
fslc db check schema.fsl

# migration sequence verification
fslc db verify schema.fsl --from schema_v19 --to schema_v25

# multi-environment compatibility
fslc db compat schema.fsl --environment ProductionDuringMigration

# rollout plan verification
fslc db plan-check rollout.fsl --depth 12

# rollback verification
fslc db rollback-check schema.fsl --migration DropLegacyEmail

# data preservation / refinement
fslc db preserve schema.fsl --mapping preserve_email.fsl

# runtime conformance / observed usage
fslc db replay schema.fsl --trace db_access_log.json
fslc db replay schema.fsl --trace api_access_log.json

# importers, optional
fslc db import --from prisma ./prisma/schema.prisma --migrations ./prisma/migrations -o db.generated.fsl
fslc db import --from rails ./db/migrate -o db.generated.fsl
fslc db import --from drizzle ./drizzle -o db.generated.fsl
```

---

## 13. AI向けfinding schema

すべてのfindingは、AIが修復に使えるように、以下を持つ。

```json
{
  "fsl": "db-compat-v0",
  "result": "violated",
  "kind": "version_lifecycle_violation",
  "severity": "error",
  "environment": "ProductionDuringMigration",
  "element": "users.email",
  "migration": "DropLegacyEmail",
  "artifact": "UserWorker",
  "artifact_version": "worker_v1_8",
  "violation": "column_removed_while_still_read",
  "witness": [
    "schema_v25 removes users.email",
    "worker_v1_8 still reads users.email",
    "ProductionDuringMigration marks worker_v1_8 as active"
  ],
  "failed_rule": "removed_only_after_unused",
  "minimal_conflict_set": [
    "migration DropLegacyEmail",
    "artifact UserWorker worker_v1_8",
    "environment ProductionDuringMigration"
  ],
  "repair_candidates": [
    {
      "kind": "delay_migration",
      "description": "Delay DropLegacyEmail until worker_v1_8 is no longer active"
    },
    {
      "kind": "update_artifact",
      "description": "Change UserWorker worker_v1_8 to read users.email_normalized"
    },
    {
      "kind": "compatibility_shim",
      "description": "Keep users.email as generated column until worker_v1_8 is removed"
    }
  ]
}
```

重要なのは、単に `failed` と返すのではなく、以下を返すことである。

- どの環境で壊れるか
- どのartifact versionが関与するか
- どのmigrationまたはschema elementが関与するか
- どのruleが破られたか
- 最小conflict setは何か
- どの修復候補があるか

---

## 14. 段階的実装計画

### Phase 0: IR設計

目的:

- DB schema / migration / artifact / environment / capabilityを表すtyped IRを定義する。
- まだSQL parserは作らない。
- 既存FSL kernelへのdesugar設計を固める。

成果物:

```text
src/fslc/db_ir.py
src/fslc/db_expand.py
docs/DESIGN-db.md
examples/db/*.fsl
```

### Phase 1: Static schema verification

対応:

- table / column / index / constraint参照の存在検証
- nullable / default / backfill整合性
- destructive change annotation
- lifecycle syntax check

CLI:

```bash
fslc db check schema.fsl --format json
```

### Phase 2: Migration sequence verification

対応:

- migrationをactionへdesugar
- schema_version / column_stateをstateへdesugar
- migration前提条件をrequiresへ展開
- schema invariantsを通常FSL invariantへ展開

CLI:

```bash
fslc db verify schema.fsl --from schema_v1 --to schema_vN
```

### Phase 3: Multi-environment capability compatibility

対応:

- artifact version profile
- active / supported / may_exist environment
- read/write/API capability invariant
- mobile offline TTL
- feature flag variant

CLI:

```bash
fslc db compat schema.fsl --environment Production
```

### Phase 4: Runtime observation integration

対応:

- DB access log replay
- API access log replay
- declared unused vs observed usage
- unsupported version still observed
- legacy API still called

CLI:

```bash
fslc db replay schema.fsl --trace access.json
```

### Phase 5: Data preservation / refinement

対応:

- split / merge / normalize / denormalize migration
- old schemaとnew schemaを共通抽象モデルへ写す
- `up; down` rollback equivalence

CLI:

```bash
fslc db preserve schema.fsl --mapping mapping.fsl
fslc db rollback-check schema.fsl
```

### Phase 6: Importers and DB-specific rule packs

対応:

- Prisma / Rails / Drizzle / SQL migration importer
- PostgreSQL rule pack
- MySQL rule pack
- SQLite rule pack
- adapter別locking / transactional DDL metadata

方針:

```text
fsl-db-core: DB非依存の抽象migration semantics
fsl-db-postgres: PostgreSQL固有ルール
fsl-db-mysql: MySQL固有ルール
fsl-db-prisma: Prisma importer
fsl-db-rails: Rails migration importer
fsl-db-drizzle: Drizzle importer
```

---

## 15. テスト方針

### 15.1 Golden examples

少なくとも以下のfixtureを作る。

```text
safe_add_nullable_column
unsafe_not_null_before_backfill
safe_expand_contract_rename
unsafe_drop_column_with_old_server
unsafe_drop_column_with_worker
unsafe_drop_api_field_with_old_mobile
unsafe_offline_write_window
safe_dual_write_backfill_switch_read_drop_old
unsafe_unique_before_dedup
rollbackable_add_column
irreversible_drop_column_without_annotation
observed_usage_after_unused
```

### 15.2 Fault injection

正常なmigration planに対して、以下のmutationを行い、検出率を見る。

- backfillを削除する。
- migration順序を入れ替える。
- old app versionをcompatibility windowへ戻す。
- worker参照を残したままdropする。
- `unused_from` を早める。
- `accepts legacy API` の期限を短くする。
- `feature flag` のvariantを片方だけ検証する。

### 15.3 AI repair loop evaluation

AIにfinding JSONを渡し、以下を評価する。

- migration planを安全なexpand-contract sequenceへ修復できるか。
- active artifactのcapability profileを修正できるか。
- `removable_from` を適切に遅らせられるか。
- compatibility shimを提案できるか。
- 修復後に新しい違反を作らないか。

---

## 16. 主要リスクと対策

### 16.1 状態空間爆発

多環境をすべて列挙すると状態が増える。

対策:

- versionではなくcapabilityでまとめる。
- artifact profileを等価類に圧縮する。
- environmentをscenario別に切る。
- mobileはsupported / may_existの範囲を明示する。
- feature flagは全組合せではなく、到達可能variantだけを列挙する。

### 16.2 DB方言の沼

PostgreSQL、MySQL、SQLite、ORMでDDL semanticsが違う。

対策:

- coreはDB非依存の抽象semanticsに留める。
- DB固有挙動はrule packに分離する。
- importerは補助機能とし、最初は手書き抽象DSLを正とする。

### 16.3 偽の安心

抽象モデルが粗すぎると、実DBでは壊れる問題を見逃す。

対策:

- verification resultにassumptionsを必ず出す。
- `verified_under_assumptions` を明示する。
- 実行ログreplayでobserved evidenceと照合する。
- DB-specific rule packがない場合はwarningを出す。

### 16.4 `deprecated` の曖昧さ

`deprecated` は読み、書き、参照、API、batchで意味が違う。

対策:

- `write_deprecated_in`、`read_deprecated_in`、`reference_deprecated_in` に分ける。
- `unused_from` と `removable_from` を別にする。

### 16.5 モバイルの残存バージョン

mobile appは強制アップデートできない場合がある。

対策:

- `supported_versions` と `may_exist_versions` を分ける。
- `minimum_supported_version` だけに依存しない。
- 実行ログでobserved client versionを取り込む。
- offline write TTLを明示する。

---

## 17. 成功基準

MVPとしては、次ができれば価値がある。

1. 手書きのfsl-db DSLから、通常FSL kernel specへdesugarできる。
2. `nullable -> not_null` 前のbackfill不足を検出できる。
3. old serverが読むカラムをdropするmigrationを検出できる。
4. worker / batchが読むカラムをdropするmigrationを検出できる。
5. mobile appが期待するAPI fieldをserverが返さなくなる互換性違反を検出できる。
6. offline write TTLより早くlegacy APIを閉じる違反を検出できる。
7. JSON findingがAIに修復候補を与えられる。
8. HTML reportまたはexplainに、environment / artifact / migration / witnessが表示される。

---

## 18. 推奨する最小MVP構成

最初に実装するなら、以下の範囲に絞る。

```text
Core concepts:
  database
  table
  column
  migration
  artifact
  environment
  capability

Column lifecycle:
  exists_from
  readable_from
  writable_from
  required_from
  write_deprecated_in
  read_deprecated_in
  unused_from
  removable_from
  removed_in

Artifact capability:
  reads
  writes
  calls api
  accepts api
  expects response
  emits_offline ttl

Environment:
  active
  supported
  may_exist
  schema range

Checks:
  all_active_reads_exist
  all_active_writes_exist
  removed_only_after_unused
  not_null_after_backfill
  api_request_accepted
  api_response_expected_fields_present
  offline_emits_accepted_until_ttl
```

このMVPだけでも、DB migration事故のかなり重要な部分を検出できる。

---

## 19. まとめ

FSLにDBスキーマ／マイグレーション検証を追加するなら、DB単体dialectとしてではなく、**multi-environment compatibility verification** として設計するべきである。

中心概念は、アプリバージョンではなく **versioned capability** である。

```text
どのバージョンか
ではなく、
その環境にいるartifactが何を読み、何を書き、何を呼び、何を期待するか
```

この設計により、FSLは以下を同じ状態遷移系で扱える。

```text
DB schema lifecycle
migration sequence
server deployment
mobile supported / may-exist versions
worker / batch / external client
API contract lifecycle
feature flag / rollout
offline sync
runtime observation
```

既存FSLのshared kernel、dialect expansion、refinement、replay/testgen、JSON repair protocolの方向性を活かすなら、`fsl-db` / `fsl-compat` は非常に筋がよい実用拡張である。

---

## 参考資料

- FSL docs map: https://github.com/ymm-oss/fsl/blob/main/docs/README.md
- FSL shared kernel + three dialect architecture: https://github.com/ymm-oss/fsl/blob/main/docs/DESIGN-layers.md
- FSL implementation bridge / Monitor / replay / testgen: https://github.com/ymm-oss/fsl/blob/main/docs/DESIGN-bridge.md
- FSL `trans`: https://github.com/ymm-oss/fsl/blob/main/docs/DESIGN-trans.md
- FSL refinement: https://github.com/ymm-oss/fsl/blob/main/docs/DESIGN-refinement.md
- FSL compose: https://github.com/ymm-oss/fsl/blob/main/docs/DESIGN-compose.md
