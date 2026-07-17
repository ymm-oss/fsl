# FSL 言語リファレンス

FSL は、**生成 AI によって書かれ、検証され、修復される**ことを第一の設計目標とする、
アプリケーション開発のための形式仕様言語です。本書は仕様を書くときに参照する言語
リファレンスです(常に最新の実装に追随します)。設計判断の背景と各機能の実装設計は
[`README.md`](README.md)(ドキュメントマップ)から辿れます。

## 設計原則

| 原則 | 既存言語 (TLA+/Alloy) | FSL |
|---|---|---|
| 構文 | 数学記法 (∀, □, ◇) | TypeScript/Python 風。LLM の学習分布に近い |
| 反例 | 人間向けテキスト | **構造化 JSON**(状態差分と、違反した束縛変数つき) |
| エラー | 人間向けメッセージ | 機械可読(行・列・分類・修復ヒント) |
| 検証 | 完全検証が前提 | デフォルトで有界かつ高速。**k-induction による深さ非有界の証明**も可能 |
| Vacuity | 専門家の直感で発見 | action の到達可能性 + **ブロックしている requires の unsat-core 診断** |
| 落とし穴 | 規律で回避 | **構造的に排除**(自動の範囲チェック、部分演算の暗黙チェック) |

## 1. 仕様の構造

```fsl
spec <Name> ["<kind>: <intent>"] {   // optional spec-level tag → metadata badge (explain/html); never verified
  const <NAME> = <constant expr>
  type  <Name> = <lo>..<hi>            // domain type (bounded integer)
  symmetric type <Name> = <lo>..<hi>   // domain whose values are interchangeable identities
  enum  <Name> { <Member>, ... }
  symmetric enum <Name> { <Member>, ... }
  struct <Name> { <field>: <scalar type | Option<scalar type>>, ... }

  def <name>(<p>: <type name>, ...) = <expr> // non-recursive named predicate; frontend-inlined

  state { <var>: <type> [= <deterministic expr>], ... }
  init  ["undecided: reason"] { <stmt>... }

  [fair] action <name>(<p>: <type name>, ...) {
    requires <expr>                     // guard. multiple allowed (conjunction)
    let <x> = <expr>                    // local binding
    <stmt>...                           // assignment / if-else / forall
    ensures <expr>                      // postcondition. reference the old state with old(expr)
  }

  invariant <Name> { <expr> }           // holds in all reachable states (safety)
  trans <Name> { <expr> }               // holds across all reachable transitions (two-state safety)
  reachable <Name> { <expr> }           // is reachable (returns a witness)
  leadsTo <Name> { <response property> }// bounded response, or ranked induction with decreases (see §1)
  terminal { <expr> }                   // intended terminal states (excluded from deadlock checking)
}
```

spec 名の後の省略可能な文字列は **spec レベルタグ**です
(`"<kind>: <intent>"`、例: `spec ReturnUI "ui: screen flow" { … }`)。宣言ごとの
タグと同様、これは**メタデータのみ**であり — 決して検証されず — `fslc explain` /
`fslc html` が spec 名の横の分類バッジとして表示します(JSON:
`skeleton.spec_kind = {id, text}`)。spec 全体がどんな種類のものかを記録するために
使います(例: 振る舞いのスライスだけをモデル化した画面フロー spec なら `ui`)。
カーネル意味論は持たず、何にも脱糖されません。`docs/DESIGN-ui.md` を参照。
内部的には、このレガシーなバッジは共有の型付き `Kind` アノテーションへ適合されます。
ソース文法は変わりません。`DESIGN-annotations.md` を参照。

任意のレイヤー — カーネル `spec` を含む — は、有限サイズをインラインの
`type X = lo..hi` 範囲ではなく、隣接するトップレベルの `verify` ブロックから得る
identity/number ソートを宣言できます。これによりドメイン宣言(何が存在するか)と
検証の世界サイズ(どれだけ検査するか)が分離されます:

```fsl
spec <Name> {
  entity <Entity>          // identity sort; size from verify { instances <Entity> = N }
  number <Number>          // numeric sort; range from verify { values <Number> = lo..hi }
}
business <Name> {
  entity <Entity>
}
requirements <Name> {
  entity <Entity>          // optional explicit identity sort
  number <Number>
  process <Entity> with f: <Number>, g: Bool = <bool>, h: <Enum> = <Member> { ... }
                            // process also declares the entity kind; Bool/enum
                            // carried fields require an explicit `= ...` initializer
}
verify {
  instances <Entity> = <N>
  values <Number> = <lo>..<hi>
}
```

`entity`/`number` は検証の前に `type <Name> = lo..hi` へ脱糖されるので、有界型を
直接書くのと完全に等価です — 違いは可読性だけです(設計 spec が、実際にはモデルの
境界にすぎないドメインサイズを主張する代わりに、ドキュメントとして読めるように
なります)。`docs/DESIGN-spec-domains.md` を参照。

データベース互換ダイアレクトも、同じカーネルの上の別のフロントエンドです:

```fsl
dbsystem <Name> {
  database <db> {
    schema <initial_version>
    table <table> {
      column <column>: <db_type> present backfilled not_null;
      column <future_column>: <db_type> absent;
    }
  }

  migration <name> from <v0> to <v1> [rollbackable] {
    add <table>.<column> nullable;
    backfill <table>.<column>;
    set_not_null <table>.<column>;
    rename <table>.<old> to <table>.<new>;
    split <table>.<source> into <table>.<a>, <table>.<b> lossless|lossy|irreversible;
    merge <table>.<a>, <table>.<b> into <table>.<target> lossless|lossy|irreversible;
    drop <table>.<column> destructive|irreversible;
  }

  artifact <version> {
    reads <table>.<column>, ...;
    writes <table>.<column>, ...;
    requires <capability_namespace>.<capability>, ...;
    provides <capability_namespace>.<capability>, ...;
    calls api.<operation>, ...;
    accepts api.<operation>, ...;
    expects response.<field>, ...;
    responds response.<field>, ...;
    emits_offline api.<operation> ttl <finite_ticks>;
  }

  environment <env> {
    schema <lo>..<hi>;
    flag <flag_name> { <variant>, ... } default <variant>;
    active <version> when schema <lo>..<hi> when flag <flag_name>=<variant>;
    supported <version> when schema <lo>..<hi>;
    may_exist <version> when schema <lo>..<hi>;
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
    rule artifact_capabilities_provided;
    rule data_preserved;
    rule rollback_equivalent;
  }
}
```

`dbsystem` は、スカラー状態と `Map<Column, Bool>` のカラムライフサイクルマップを
持つカーネル spec に展開されます。ネストした `Map<_, Set<_>>` 状態は決して使いません。
展開後は `fslc check` と `fslc verify` がそのまま動作します。`fslc db check` はさらに
安定した fsl-db findings を出力し、形式検査の成功に対して
`verified_under_assumptions` を返します。環境の schema 範囲は、宣言されたマイグレー
ション順序における有限の到達可能スナップショットです。ロールアウトのパーセンテージ
とオフライン TTL は、有限の共存ウィンドウ/tick としてモデル化しなければなりません。
API/オフラインおよび有界の preservation/rollback 検査は、ダイアレクトレベルの互換性
検査です。フィーチャーフラグは環境内の有限の宣言済みバリアントであり、
`DB-ASSUME-FINITE-FLAG-STATE` を追加します。ロールアウトのパーセンテージを証明する
ものではありません。汎用の `requires` / `provides` ケイパビリティは、AI モデル/
プロンプト/リトリーバー、ツールスキーマ、出力スキーマ、モバイル/サーバー、その他の
アーティファクトプロファイルを同じスナップショットモデルでカバーします。
`docs/DESIGN-db.md` を参照。

関数型 DDD / 非同期エフェクトダイアレクト(v0。同じカーネルへ展開され、安定した
fsl-domain findings を報告します):

```fsl
domain <Name> {
  implementation_profile functional_ddd

  enum OrderStatus { Pending, Approved, Cancelled }

  aggregate Order {
    id OrderId
    state { status: OrderStatus = Pending; }

    command ApproveOrder {}
    event OrderApproved {}
    event PaymentCaptureRequested { payment_request_id: PaymentRequestId }
    event PaymentCaptured { payment_request_id: PaymentRequestId }
    event PaymentFailed { payment_request_id: PaymentRequestId }
    event PaymentCaptureTimedOut { payment_request_id: PaymentRequestId }
    error CannotApprove

    decide ApproveOrder {
      requires status == Pending
      emits OrderApproved
    }

    evolve OrderApproved {
      status = Approved
    }
    evolve PaymentCaptureRequested { }
    evolve PaymentCaptured { }
    evolve PaymentFailed { }
    evolve PaymentCaptureTimedOut { }

    invariant noLateApprove {
      status == Cancelled -> not can(ApproveOrder)
    }
  }

  effect CapturePayment {
    async
    irreversible
    idempotency_key Order.id
    correlation_id PaymentCaptureRequested.payment_request_id
    handles PaymentCaptureRequested
    emits one_of [PaymentCaptured, PaymentFailed, PaymentCaptureTimedOut]
    retry { max_attempts 3 }
    timeout after 10m emits PaymentCaptureTimedOut
    compensation { emits PaymentFailed }
  }

  saga OrderFulfillment {
    starts_on OrderApproved
    outbox OrderOutbox
    inbox FulfillmentInbox

    step RequestPayment {
      async
      emits PaymentCaptureRequested
      awaits one_of [PaymentCaptured, PaymentFailed, PaymentCaptureTimedOut]
      timeout after 10m emits PaymentCaptureTimedOut
    }
  }
}
```

`domain` は、aggregate の一貫性境界、コマンドの意図、受理されるイベント、ドメイン
エラー、純粋な `decide`/`evolve`、非同期エフェクトのライフサイクル、saga/プロセス
マネージャの協調をモデル化します。各 command+decide+evolve のパスをカーネルの
`action` へ、aggregate 状態をプレフィックス付きのカーネル状態へ、saga の step を
イベントフラグでガードされた action へ、エフェクトライフサイクル状態を有限の
`Map<CorrelationId, EffectStatus>` / `Map<CorrelationId, Attempt>` マップへと
lowering します。domain の enum メンバーは lowering 時に名前空間化されるので、
2 つの domain enum が両方とも `Pending` を含んでもかまいません。domain の式では
`X in [A, B]` と `can(Command)` が使えます。これらは型付き domain ツリーから解決
され、カーネル式へ構造的に lowering されます。裸の enum メンバーは期待される論理型
を使います。複数の enum に共有される型なしメンバーはエラーです。有限のメンバー
シップは等値の選言になります(`X in []` は `false`)。`can(Command)` は現在の
aggregate 内で解決され、そのコマンドの `requires` 節の連言と、各拒否条件の否定に
なります。未知のシンボル、aggregate をまたぐコマンド、型の不一致、未対応の呼び出し
は、元の domain 式の位置で報告されます。

domain 宣言では、有限のバリアントに `enum Name { Member, ... }` を、有界の数値範囲
に `type Name = lo..hi` を使います。レガシーな書き方 `type Name = A | B` は現行の
2.x エディションでは引き続き受理され、宣言全体に対して安定した
`deprecated_domain_enum_union` 警告(正準の置き換えを含む)を生成します。
`fslc check`、`fslc verify`、`fslc domain check` は `--edition current|next` を
受け付けます。`next` はレガシーの union 記法を拒否します。`fslc lint` は変更を
加えずにエディション finding を報告し、`fslc migrate --edition next` は検査済みの
正準編集を提供します。空の enum と重複メンバーはエラーで、それぞれ宣言と重複
メンバーの位置で報告されます。

ネイティブ Rust フロントエンドはまた、domain の状態・action・ガード・文・
プロパティについて非公開の origin チェーンを保持します。検証、反例、`explain` の
診断は、元の domain 宣言を第一の表示名として使い、生成された Kernel 名は機械向けの
詳細として保持します。チェーンは、ソースファイル/完全なスパン、宣言パス、lowering
のステップ、primary/secondary のソース、明示的な generated-only ノードを記録します。
要件 ID は独立したトレーサビリティ関係のままです。Public Kernel v1 の出力は不変です。
オプトインの Public Kernel v2 は、このチェーンを、ポータブルなソース同一性、正確な
バイト/行座標、ターゲットバインディング、ソースノードの逆引き、明示的な
assurance/completeness を持つトップレベルの provenance グラフとして公開します。

domain aggregate の状態フィールドが初期化子を省略した場合、現行エディションは
確立された Bool `false`、enum 先頭メンバー、範囲下限、external-placeholder `0` の
選択を維持し、`implicit_initial_value` を出力します。この警告には、選択された値、
理由、current/next の深刻度、フィールドのスパン、機械適用可能な明示初期化子の挿入
が含まれます。次のエディションでは初期化子の明示が必須になります。

安定した fsl-domain findings とネストされたカーネル結果(成功時は
`verified_under_assumptions`)には `fslc domain check` を、aggregate/effect の
サマリーには `fslc domain analyze` を、生成されたカーネル FSL のデバッグビューの
確認には `fslc domain expand` を、Functional DDD スキャフォールドには
`fslc domain generate --target typescript|python|kotlin|swift|rust` を、
アダプタ/コンフォーマンスのスキャフォールドには `fslc domain testgen` を、
ランタイムの command / event / effect エビデンスには
`fslc domain replay --logs events.jsonl` を使います。v0 実装が証明するのは有限の
モデル化されたライフサイクルです。replay は観測エビデンスであり、saga 履歴は
`DOMAIN-ASSUME-SAGA-OBSERVED-HISTORY` を追加します。
ネイティブのスキャフォールドエミッタは、検査済みの Public Kernel v1 JSON と
バージョン付きの `domain-scaffold-metadata.v1` 互換ブリッジを消費します。非公開の
domain AST を受け取ることも、ソース式を再パースすることもありません。Kernel/メタ
データのバージョンやダイアレクトの不一致、重複した Kernel メンバー、lowering された
type/state/action の対応物の欠落は fail closed です。ソース式と effect/saga の
トポロジーはコンパニオン側で引き続き権威です。Public Kernel v1 はそれらをエンコード
しないためです。5 つのターゲット出力はすべて移行前のゴールデンに固定されており、
valid な domain コーパスはすべてのターゲットについて生成されます。
実際のゲートウェイの挙動、実時間のタイムアウト、キューの配送、本番の exactly-once
意味論は証明しません。`docs/DESIGN-domain.md` と `docs/DESIGN-effect.md` を参照。

`fslc verify` は、spec を編集することなく、`verify` ブロックの `instances`/`values`
境界をコマンドラインから上書きできます:

```bash
fslc verify spec.fsl --instances Case=1 --property EventuallyLeavesInProgress
fslc verify spec.fsl --values Amount=0..3
```

どちらのフラグも繰り返し指定でき(`--instances A=1 --instances B=2`)、一致する
`entity`/`number` 名の境界を置き換えます — ファイルに触れずに、liveness/induction
の実行(§7 参照)向けに 1 エンティティのモデルへ縮小するのに便利です。
一致する `entity`/`number` 宣言のない `NAME` や、境界が `entity`/`number` 由来では
ない spec(カーネルの `type X = lo..hi` リテラル)は spec エラー(exit code 2)で、
不正な値(`Case=abc`, `N=5..1`)も同様です。実効の上書き後の境界は、JSON
エンベロープの `bounds_overrides` フィールドにエコーバックされます。

spec がインラインの `implements` を持つ場合、上書きは抽象 spec にも伝播します —
抽象側自身が宣言する entity/number 名に限定されます — ので、refinement 検査は両側
で同じ世界サイズで実行されます(refinement は同サイズの forward simulation です。
これがないと、縮小した impl とフルサイズの抽象は `map_out_of_bounds` で失敗します)。
impl 側だけの carried number(例: business の抽象には存在しない `Amount`)は impl
のみに適用されます。

`acceptance`/`forbidden` シナリオは、spec の元の世界の id や数値をハードコード
しがちで(`accept(2)`)、それが縮小された上書き(`--instances Case=1`)の外に出る
ことがあります。上書きが有効なとき、replay の失敗が*純粋に*上書き後の境界の外の値を
参照したことによる場合(範囲外の action 引数、または `expect` 内の範囲外インデックス)、
そのシナリオはシナリオ単位でハードエラーからスキップへ降格され、エンベロープの
`warnings`(`{"kind": "acceptance_skipped"/"forbidden_skipped",
"id": ..., "message": ...}`)で報告されます。残りのシナリオは引き続き実行されます。
上書きなしの場合 — あるいはそれ以外の失敗(偽の `expect`、満たされない `requires`)
の場合 — 挙動は変わらずハードエラーです。これにより、spec の acceptance シナリオが
元の `verify { instances Case = N }` 境界向けに書かれていても、`--instances Case=1
--property <Liveness>` が使えます。

`fair` は弱い公平性(weak fairness)のアノテーションです: その action インスタンス
が継続的に enabled であり続けるなら、いずれ実行される、という仮定です。
公平性は action インスタンス全体に適用されます。条件付きでのみ公平にしたい場合は、
その条件を個別に guard した `fair action` へ分割してください。

**action パラメータの型**(`<p>: <type name>`): ドメイン型、enum、または組み込みの
`Bool` — BMC が列挙できるものすべてです。`Bool` は `Bool` の状態変数とまったく同じ
ように振る舞います: 真偽値のガードとして裸で使う(`requires b`、
`requires not b`)か、`Bool` 型の状態へ代入します
(`flag[i] = b`)。組み込みの `Int` は拒否されます(非有界のパラメータは列挙でき
ません)。代わりに範囲パラメータを使ってください: `p in <lo>..<hi>`(名前付きの
ドメイン型の宣言を必要としない、`<p>: <type name>` のインライン代替)。

プロパティの階層: `invariant` は 1 状態の安全性、`trans` は 2 状態の安全性(遷移前
状態は `old()` で参照できます)、`leadsTo` は応答 liveness です。ランキング関数が
なければ、`leadsTo` は有界に検査されます。`--engine induction` と
`decreases <int expr>` があれば、`leadsTo` は well-founded なランキング論法によって
非有界に証明できます。

FSL が検証するのは有限遷移系であり、refinement 関係そのものに対する高階の公理は
宣言しません。refinement の反射律や推移律のような法則は、代数公理として量化する
のではなく、具体的な refinement chain または有限状態機械のモデルを通して検査します。

`leadsTo` ブロック内の応答プロパティ:

```fsl
leadsTo <Name> {
  <expr> ~> <expr>                      // once P holds (including the same instant), Q eventually holds
  <expr> ~> within K <expr>             // Q must hold within K steps after P
  forall x: T { <expr> ~> <expr> }      // checked independently per binding (only an outer forall may nest)
  helpful <action>(<binding expr>, ...)  // optional; per-binding progress action for ranked induction
  decreases <int expr>                  // optional; induction-only ranking measure
}
```

`~>` は **leadsTo ブロック専用**です — 一般の式では使えません。
`within K` は応答に対する有界の締め切りで、`K` は非負の定数式でなければなりません。
これは BMC によって検査され、ランク付き induction 証明では検査されません。
`decreases` は省略可能で、整数値の式でなければなりません。
`fslc verify --engine induction` の下では、検証器は、証明済みの invariant の下で、
`P` が pending かつ `Q` が偽であるときには常に、測度が非負であること、進行が可能で
あること、enabled-action の規律が満たされることを検査して、ランク付き応答を証明
します。`helpful` がなければ、enabled なすべての action は、`Q` を真にするか、
測度を狭義に減少させながら `P` を真に保たなければなりません。1 つ以上の
`helpful action(args...)` 行があるときは、一致する helpful action インスタンスだけ
が発火時に測度を狭義に減少させればよく、無関係な action インスタンスは pending の
義務を真に保ち(`Q` を真にする場合を除く)、測度を増加させてはなりません --
helpful の発火の合間の非有界な増加は、保証された減少を上回り、helpful action が
公平性の下で発火し続けても `Q` に決して到達できなくする可能性があるためです。
一致する helpful action は下位レイヤーの `fair action` でなければならず、義務が
pending の間は常に enabled でなければなりません。`helpful` はランク付き証明のための
メタデータであり、公平性の仮定を作りません。ランク付き証明の成功は `--depth` から
独立しています。`--depth` はベースの BMC 検査と reachable/coverage のエビデンスには
引き続き使われます。

**2 つ以上の異なる `helpful` action 名があるとき**、各インスタンスの enabled 状態は
ちらついて(flicker)はなりません: 義務が pending の間に helpful インスタンスが
一度 enabled になったら、それが発火する(または `Q` が成立する)まで enabled で
あり続けなければなりません。「pending のすべての状態でいずれかの helpful マッチが
enabled」だけでは、それ単体では不十分です -- *どの*インスタンスが enabled かが
変わり続ける場合(例: 交互の条件で enabled になる 2 つの helpful action)、単一の
インスタンスが*継続的に* enabled になることは決してないため、その `fair` 宣言は
実際にはその実行を義務づけず、選言的な enabledness 検査が通っても leadsTo は本当に
偽でありえます。これは
`rank_failure:"helpful_action_enabledness_not_sticky"` として報告されます。上の
一般的な、バインディングごとに単一の `helpful step(c)` イディオムは影響を受けません:
helpful action が 1 つなら、「pending の間は常に enabled」がすでに継続的に enabled
であることを含意します。

**配置.** `decreases` は、`leadsTo` ブロックの内側で応答本体の兄弟であり、`forall`
ラッパーの*外側*です — forall の中括弧の内側にネストしてはなりません。`forall` の
内側へのネストはパースエラーであって、forall 下のランキングの制限ではありません:

```fsl
// valid: decreases after the forall's closing }
leadsTo Responds {
  forall c: Case { level[c] > 0 ~> level[c] == 0 }
  decreases level[0] + level[1]
}

// parse error: decreases nested inside the forall body
leadsTo Responds {
  forall c: Case { level[c] > 0 ~> level[c] == 0 decreases level[0] + level[1] }
}
```

**インターリービング下のエンティティ単位の測度には `helpful` が必要.** 束縛された
エンティティだけに言及する測度、例えば
`forall c: Case { level[c] > 0 ~> level[c] == 0 }` の内側の `decreases level[c]`
は、それ単体では不十分です: 別のエンティティを進める action は `level[c]` を変え
ないことがあります。バインディングごとの進行 action を追加してください:

```fsl
leadsTo Responds {
  forall c: Case { level[c] > 0 ~> level[c] == 0 }
  helpful step(c)
  decreases level[c]
}
```

この形式では、`step(c)` は `fair action` と宣言されていなければならず、その
バインディングについて pending なすべての状態で enabled でなければならず、発火時に
`level[c]` を狭義に減少させなければなりません。他のインターリービングは、この
エンティティ単位の測度を減少させずに pending の義務を保存してもかまいませんが、
増加させてはなりません。診断は `progress_action_not_fair`、
`helpful_action_not_enabled`、`non_decreasing_helpful_action`、
`non_helpful_action_increases_measure`、`pending_not_preserved`、および(2 つ以上の
helpful action がある場合)`helpful_action_enabledness_not_sticky` を区別します。

**動くイディオム: グローバルな総和測度.** 組み込みの `sum()` 集約(§3)で、追跡
したい量をドメイン全体にわたって合計します: `decreases sum(k: Case of
level[k])`。`sum()` は有界の `Case` ドメイン自体を列挙するので、この測度は
instances 数から独立です — `Case` のサイズが `verify { instances Case = N }` で
指定されていても、CLI の `--instances` 上書きで縮小されていても、同じ `decreases`
節が変更なしで機能します。fair な `step` はどれもちょうど 1 つの `level[k]` を
デクリメントするので、enabled なすべての action が合計を狭義に減少させ、induction
は `"proved"` を `"completeness": "unbounded"` とともに返します。このイディオムが
カバーするのは、*すべての* enabled action が合計を減少させる設計だけです。
インターリービング下のエンティティ単位の進行には、上の `helpful` 形式を選んで
ください。(条件付きの測度は整数式の間の選択はできますが、それ単体では `helpful`
形式が要求するエンティティ単位の減少を証明しません。)

## 2. 型

| 型 | 例 | 説明 |
|---|---|---|
| `Int` / `Bool` | `count: Int` | 非有界の整数 / 真偽値 |
| ドメイン型 | `type Qty = 0..5` | 有界の整数。**範囲は自動で検査される**(§6) |
| インライン状態ドメイン | `state { qty: 0..5 }` | 状態変数宣言における名前付きドメイン型の省略記法 |
| インライン状態初期化子 | `state { qty: Qty = 0 }` | `init` 内の等価なルート代入の決定的なシュガー |
| エンティティ種 | `entity Claim` / `process Claim ...` | 有限の同一性ソート。カーネル `spec` を含む任意のレイヤーで使用可。サイズは `verify { instances Claim = N }` で設定。`type Claim = 0..N-1` に脱糖 |
| 数値種 | `number Amount` | 有限の数値ソート。カーネル `spec` を含む任意のレイヤーで使用可。範囲は `verify { values Amount = lo..hi }` で設定。`type` に脱糖 |
| enum | `enum St { Open, Closed }` | メンバーは式の中で裸の名前で参照する |
| struct | `struct Order { st: St, item: Option<ItemId>, qty: Qty }` | フィールドはスカラーまたは `Option<スカラー>` |
| `Option<T>` | `cart: Option<ItemId>` | `none` / `some(e)`。番兵値の代わりに使う |
| `Map<K, V>` | `stock: Map<ItemId, Qty>` | K は有界スカラー(ドメイン型 / enum / Bool)を推奨 |
| `Set<T>` | `shipped: Set<OrderId>` | T は有界スカラー |
| `Seq<T, N>` | `queue: Seq<JobId, 3>` | 容量 N の列(FIFO)。T はスカラー、N は定数 |
| `relation A -> B` | `delegates: relation User -> User` | 有界スカラーの端点上の有界な二項関係 |

**スカラー** = Int / Bool / ドメイン型 / enum。`state` 宣言では、`x: lo..hi` が
匿名のドメイン型として受理され、`type X = lo..hi` を宣言して `x: X` と書くのと
等価です。

Kernel の状態フィールドは、単純で決定的な初期値のために `name: Type = expr` を
使えます。検査の前に通常のルート代入へ正規化されるので、Monitor、明示的探索、BMC、
induction、Public Kernel v1 は、等価な `init` ブロックと同じ意味論を観測します。
式には定数、enum メンバー、コンストラクタ、`none`、決定的なコレクションリテラルを
使えますが、状態ルートや別の初期化子を読んではなりません。インデックス付き、
フィールド、条件文、量化、関係、一括の初期化は `init` に残ります。同じルートを
インラインと `init` の両方で代入するのは意味論エラーで、両方のソース位置を報告
します。[`DESIGN-initialization.md`](DESIGN-initialization.md) を参照。

**状態変数として合法な型**(それ以外は `check` が型エラーとして拒否します):
スカラー | `Option<scalar>` | struct(スカラー / `Option<scalar>` フィールド)
| `Map<bounded scalar, scalar | Option<scalar> | struct>`
| `Set<bounded scalar>` | `Seq<scalar, N>` | `relation bounded-scalar -> bounded-scalar`

- struct のネスト、struct フィールドの中の Set/Map/Seq、
  `Option<Option<...>>`、`Option<Set/Map/Seq/struct>` は許されません
  (check 時にヒント付きで拒否されます)。省略可能なスカラーのフィールドは、
  v2.1 以降 struct の中に直接書けます。
- `Map<Int, V>` は動作しますが非推奨の警告を出します。ドメイン型のキーを使って
  ください。
- `symmetric type` と `symmetric enum` は、liveness の対称性簡約のために、値を
  交換可能なエンティティ同一性としてマークします。`leadsTo` の lasso/停滞探索の
  間、fslc は `Map<SymmetricType, V>` と `Set<SymmetricType>` の状態から作られる
  エンティティごとの行に 1 つの正準の代表を使います。`V` は対称な同一性型を含まない
  ときだけ使われます。これは、どのタスクの同一性も特別ではない
  `Map<TaskId, Status>` のようなモデルを意図しています。

## 3. 式

名前付き述語は、繰り返される式や業務上重要な式をくくり出します:

```fsl
def eligible(c: Claim) = submitted[c] and amount[c] <= AUTO_LIMIT
invariant OnlyEligible { forall c: Claim { approved[c] => eligible(c) } }
```

呼び出しは、同一ソースファイル内の `def` を名指しし、そのアリティに一致しなければ
なりません。定義は前後の定義を呼べますが、直接・相互の再帰はできません。定義は意味
検査の前に展開され、検証器とランタイムは手で展開したのと同じカーネル式を見ます。
展開は、内部のバインダー名を発明する代わりに変数捕獲を拒否します。
[`DESIGN-def.md`](DESIGN-def.md) を参照。

- 算術: `+ - * / %`、単項 `-`、`min(a, b)` / `max(a, b)` / `abs(a)`
  (`a//b` は `//` 以降がすべてコメントになってしまうので、除算は空白を入れて
  `a / b` と書きます)
- 比較: `== != < <= > >=`
- 論理: `and or not =>`
- 条件: `if condition then when_true else when_false`。条件は `Bool` で、両方の
  分岐が検査され、同じ論理型を持たなければなりません。ガード、代入、プロパティ、
  関数引数、ランキング式、定数、refinement マッピングを含む、式が受理される任意の
  場所で使えます。条件式は右結合し、各分岐は囲むデリミタまで延びます。後続の演算子
  を条件式全体に適用したいときは括弧を使ってください。具象評価は選択された分岐だけ
  を実行しますが、名前と型の検査は常に両方の分岐を訪れます。
- 有限バインダー: `x: T`、`x in lo..hi`、または `x in set_or_seq`。それぞれ省略
  可能な `where predicate` を後置できます。述語は `Bool` で、`x` が束縛された後に
  スコープされます。Map と非有界のコレクションはバインダーのドメインになりません。
- 量化(有界): 正準形は `forall binder { expr }` と `exists binder { expr }`
  です。`forall i in lo..hi: expr` のような 2.x レガシーのコロン/中括弧なしの記法
  は引き続き受理されますが、非正準です。Seq のバインダーはその live なプレフィックス
  を位置順に訪れ、重複した値を保存します。
- 集約: `count(binder)` と `sum(binder of value)`。例は
  `count(x: T where p)`、`count(x in queue where p)`、
  `sum(x in queue of x.amount where p)` と、それらの範囲の等価形。空のドメインは
  `0` を生みます。Seq の重複は live な位置ごとに 1 回ずつ寄与し、Set のメンバー
  シップは異なるメンバーごとに 1 回寄与します。
- 濃度の述語: `unique(binder)` / `exactlyOne(binder)`。`unique` は一致する束縛が
  高々 1 つ、`exactlyOne` はちょうど 1 つという意味です。
- Option: `x == none` / `x != none` / `x == some(e)` / `x != some(e)`。
  等値は構造的です: `none` は `none` とだけ等しく、2 つの `some` 値はペイロードが
  等しいときにちょうど等しくなります。`v` を束縛する形は引き続き `x is some(v)`
  であり、等値が束縛を導入することはありません。束縛が有効なのは match が真である
  論理的な後続部分だけです。例えば `(x is some(v)) => ...` や
  `(x is some(v)) and ...` の右辺では使えますが、グローバルな束縛にはなりません。
  順序は定義されません。
- struct: リテラル `Order { st: Open, qty: 0 }`、フィールド参照 `o.st`、
  `==` はフィールドごとの等値
- Set: `Set {}` / `Set { 1, 2 }`、`.add(e)` `.remove(e)` `.contains(e)` `.size()`
- Seq: `Seq {}` / `Seq { 1, 2 }`、`.push(e)` `.pop()` `.head()` `.at(i)`
  `.contains(e)` `.size()`、`==` は長さとすべての要素の等値
- Relation: `r.contains(a, b)`、`r.add(a, b)`、`r.remove(a, b)`、
  `reachable(r, a, b)`、`acyclic(r)`、`functional(r)`、`injective(r)`、
  `domain(r)`、`range(r)`。`reachable` と `acyclic` は自己関係
  (`relation T -> T`)を要求します。端点の型/アリティのエラーは修復ヒントを
  含みます。
- ensures / trans の内側のみ: `old(expr)` で遷移前の状態を読む
- leadsTo ブロックの内側のみ: `P ~> Q`(応答プロパティ。一般式の演算子階層の一部ではありません)。
  Q の前に有界の締め切り `within K` を、応答本体の後に induction のランキング用の
  `decreases <int expr>` を置けます

トップレベルの時相シュガー:

```fsl
unless Name { P unless Q }   // while P holds and Q is false, the next state must keep P or make Q true
until  Name { P until Q }    // unless safety plus a leadsTo P ~> Q progress obligation
```

このシュガーは、「解放されるまで保持される」「完了するまで保留」のような持続する
ワークフロー状態に使ってください。任意の履歴の事実には、明示的な ghost 変数を
使います。

## 4. 文(init / action 本体)

- 代入: `x = expr`、`m[k] = expr`、`m[k].field = expr`、`o.field = expr`
- Set/Seq/relation の更新は**再代入イディオム**を使います:
  `s = s.add(x)`、`q = q.pop()`、`r = r.add(a, b)`
- `if expr { stmt... } else { stmt... }` は `init` と action 本体の両方で使えます
  (else の内側の if でネストできます)
- `forall x: T { stmt... }`(一括初期化 / 一括更新)

## 5. 意味論

- **遷移系**: 1 ステップ = ちょうど 1 つの action インスタンス
  (action 名 × パラメータ値)がアトミックに実行される。
- **同時代入**: action 本体のすべての右辺は**旧状態**を読む。代入されない変数は
  変化しない(フレーム条件は自動)。
- **二重代入は意味論エラー**: 同じ実行パス上で同じ変数(またはフィールド)に 2 回
  代入するのは意味論エラーです。if の then/else は別々のパスなので、両方で代入して
  かまいません。if の**後**に同じ変数へ代入するのもエラーです(分岐の内側の書き込み
  が失われるのを防ぐため)。
- `Map<K, Struct>` の値については、フィールドの書き込みはフィールド単位で追跡され
  ます。1 つの action の中で同じ要素の異なる 2 つのフィールドを更新すること、例えば
  `m[k].f1 = 1` に続く `m[k].f2 = 2` は許されます。同じパスで同じフィールドを繰り返す
  のは意味論エラーです。インデックス付きの書き込みは、そのインデックスが証明可能に
  相異なる定数でない限り拒否されます。`requires k != j` のようなガードやローカルの
  定数束縛は相異性を確立しません。検査済みの Kernel モデルは、どの検証器バックエンド
  が走るよりも前にこれを拒否するので、ネイティブの `check`/`verify` とブラウザの
  Worker は同じ `kind:"semantics"` 分類を返します。

  ```fsl
  type K = 0..1
  type V = 0..3
  struct Pair { f1: V, f2: V }
  state { m: Map<K, Pair> }
  action update(k: K) {
    m[k].f1 = 1
    m[k].f2 = 2
  }
  ```

  観測された結果: `fslc check struct_fields_ok.fsl` は `result:"ok"` を返し、
  `fslc verify struct_fields_ok.fsl --depth 1` は `result:"verified"` を返しました。
  action を `m[k].f1` へ 2 回代入するよう変えると、`fslc check` と `fslc verify` の
  両方から `result:"error"`、`kind:"semantics"` が返りました。
- **requires**: すべてが成立するときだけ enabled になる。
- **ensures**: 遷移後の状態で検査される。違反は
  `violation_kind: "ensures"`。
- **trans**: 各実行ステップの遷移後の状態で検査され、`old(expr)` は遷移前の状態で
  評価される。違反は `violation_kind: "trans"`。

## 6. 自動検査(書かなくても検査されるもの)

| 検査 | 内容 | 違反時 |
|---|---|---|
| 型境界 | すべての有界型の状態変数(Map の値、struct のフィールド、Seq の要素を含む)が範囲内にある | `violated` / `type_bound` / `_bounds_<var>` |
| 部分演算 | `pop()`/`head()`/`at(i)` の時点で列が非空かつインデックスが範囲内であり、`/` `%` の除数が非ゼロ | `violated` / `partial_op` / `_partial_<action>` |
| action カバレッジ | 各 action が深さ K 以内に少なくとも 1 回 enabled になる | `action_coverage` にブロックしている requires の診断 |
| デッドロック | すべての action が disabled になる状態への到達 | 警告(`--deadlock error` で `violated`) |
| trans | 2 状態の述語が到達可能なすべての遷移で成立するか | `violated` / `trans` / `trans` + トレース |
| leadsTo | `within` 締め切りの超過、深さ K までの lasso、デッドロックによる停滞を通じた P ~> Q 違反(締め切りの超過と停滞は、`--depth` が締め切り/停滞のステップに達した時点で、またそれ以降のすべてのより大きな深さで検出されます。ちょうどそのステップに一致したときだけではありません) | `violated` / `leadsTo` / `bindings` + トレース |

- デッドロック警告には、どの状態で行き詰まったかが含まれます(例:
  `deadlock reachable at step 1 (state: status=ToolFault, ...)`)。完全なトレースは
  JSON の `deadlock.trace` にもあります。
- **意図された終端状態**(処理の完了や最終結果など、停止することが正しい状態)は
  `terminal { <predicate> }` ブロックで宣言します。述語を満たす停止状態はデッド
  ロック検査から除外され、それ以外の予期しないデッドロックは引き続き検出されます。
  `--deadlock ignore` が**すべての停止状態**を一律に無視するのに対し、`terminal` は
  **どの停止が意図的か**を選択できます。
  例: `terminal { status == Done or status == Failed }`。
  - **requirements**: `terminal { }` は `requirements_item` であり、カーネル spec
    へそのまま通ります(§13.2)。`process E { ... }` を使う spec の内側では、
    ソースレベルのアクセサを使います。例えば `terminal { forall c:
    Claim { stage(c) == Approved or stage(c) == Rejected } }`。これは `c` の
    エンティティ型から解決され、生成された stage マップへ lowering されます。生成
    された `claim_stage` という名前は requirements のソース構文の一部ではありません。
  - **business**: `terminal` の構文はまったく存在しません — 各 process のシンク
    ステージ(出て行く `transition` のないステージ)から自動的に導出されます。
    §13.3 を参照。
- 「在庫が 0 以上」のような invariant は**書かないでください** —
  `type Qty = 0..N` にすれば自動で検出されます。
- 満杯の Seq への `push` も `type_bound` として自動で検出されます
  (ガードするには `requires q.size() < N` と書きます)。

## 7. 検証器 `fslc`

```
fslc check     <file.fsl>                        # syntax / names / types only (fast)
fslc lint      <path>... [--edition current|next] [--project fsl-project.toml] # edition + ID-policy diagnostics; never mutates
fslc migrate   <path>... --edition next [--write] # dry-run edits; --write applies validated set
fslc fmt       <file.fsl|-> [--edition current|next] # canonical FSL to stdout; never mutates input
fslc fmt       <path>... --check                 # JSON format_check; exit 0 clean, 1 changed, 2 error
fslc kernel    <file.fsl> [--kernel-version 1|2] # normalized typed Kernel JSON (default v1)
fslc conformance <file.fsl> [--depth K] [--kernel-version 1|2] # matching vectors (default v1)
fslc verify    <file.fsl> [--depth K]            # BMC (default K=8, counterexample is shortest)
               [--engine induction] [--k N]      # k-induction: unbounded-depth proof
               [--engine explicit]               # concrete-state BFS (native fslc): closure ⇒ proved
               [--explicit-budget N]             #   max visited states (default 1000000); over ⇒ unknown_budget
               [--engine auto]                   # explicit first, transparent fallback to bmc when
                                                 #   explicit can't decide (leadsTo, nondeterministic init,
                                                 #   unknown_budget); "engine"/"engine_fallback" in the result
               [--lemma "<expr>"]...             # independently prove auxiliary candidates,
                                                 # then retry CTIs with proved lemmas only
               [--from-state state.json]         # replace init with a complete logical snapshot (BMC only)
               [--deadlock warn|error|ignore]
               [--vacuity warn|error|ignore]     # vacuity check (§15)
               [--property <Name>]               # check one named property in isolation —
                                                 #   invariant / trans / leadsTo / reachable (for probing)
               [--exclude-property <Name>]...    # skip named invariant/trans/leadsTo/reachable
               [--strict-tags] [--requirements ids.txt]  # tag matching (§15)
fslc sweep     <file.fsl> --instances E=lo..hi --depth lo..hi [--property Name]
                                                 # opt-in scope sweep over bounded verification
fslc scenarios <file.fsl> [--depth K]            # generate integration-test scaffold JSON
fslc replay    <file.fsl> --trace <events.json>  # spec-action trace conformance (§12)
fslc replay    <file.fsl> --from-log <events.jsonl> --mapping <mapping.fsl>
                                                 # production log mapping + conformance (§12)
fslc testgen   <file.fsl> [--depth K] [--strict] [--target pytest|vitest|swift|kotlin|dart|phpunit] [-o out]  # implementation-conformance test scaffold (§12)
fslc refine    <impl> <abs> <mapping> [--depth K]# fidelity check of a detailed spec (§10)
fslc diff      <old> <new> [--depth K] [--mapping map.fsl]
               [--forbid behavior_added,invariant_weakened,forbidden_relaxed]
                                                 # bounded semantic change analysis
fslc diff      --git BASE..HEAD [spec.fsl] [--depth K]
                                                 # revision-consistent tree materialization; omit spec for all changed .fsl
fslc chain     [fsl-project.toml] [--keep-going] # manifest-driven cross-layer report (§10)
fslc mutate    <file.fsl> [--by-requirement] [--max-mutants N]
               [--from mutants.jsonl]             # built-in + external spec mutation (§15)
fslc explain   <file.fsl> [--depth K] [--readable] # JSON by default; readable text review view (§15)
fslc analyze   <file-or-dir>... [--projection tsg|action_state_graph|action_dependency_graph|code_audit|impact_graph|requirement_property_graph|property_state_graph|refinement_graph|traceability_graph] [--code FILE_OR_DIR] [--focus NODE] [--profile ai-review] [--export tag-review] [--format json|dot|mermaid]  # structural/tag/code review (§15)
fslc html      <file.fsl> [--depth K] [-o report.html] # self-contained review report (§15)
fslc ledger    <file.fsl> [--depth K] [--impl-log run.json] [--approval record.json] [--trust-key public.pem] [-o ledger.md] # business audit ledger by requirement id (§15)
fslc document generate <file.fsl> [--view requirements] [--lang ja|en] [--strict] [--strict-rendering]
               [--glossary glossary.json] [--evidence evidence.json]... [-o requirements.md]
                                                  # deterministic ja/en requirements document from the Requirement Claim IR (§13);
                                                  # --evidence overlays a per-requirement assurance class from saved
                                                  # external evidence (repeatable; same envelope shape as `fslc ledger --evidence`)
fslc document claims <file.fsl> [--view requirements] [-o requirements.claims.json]
                                                  # emit the Requirement Claim IR (RCIR) claim set as JSON (§13)
fslc document check <file.fsl> <document.md> [--glossary glossary.json] [--evidence evidence.json]...
                                                  # structural drift check: generated claim blocks vs a
                                                  # fresh re-render; never interprets prose (§13)
fslc approval create <file.fsl> --kind ledger|html|scenarios --artifact <reviewed> --approver <name> [--signing-key private.pem] [-o record.json]
fslc approval check  <file.fsl> --record <record.json> [--trust-key public.pem] # approved | drifted | signature-invalid
fslc approval diff   <file.fsl> --record <record.json> [--depth K] [--trust-key public.pem]
fslc typestate <file.fsl> [--ts]                 # decide applicability of state machine → ghost type (§16)
fslc domain check <file.fsl> [--depth K] [--engine bmc|induction] # Functional DDD / effect findings
fslc domain analyze <file.fsl>                                  # aggregate/effect ownership summary
fslc domain expand <file.fsl> [-o out.fsl]                      # generated kernel FSL
fslc domain generate <file.fsl> --target typescript|python|kotlin|swift|rust [-o dir] # Functional DDD scaffold
fslc domain testgen <file.fsl> [--target vitest] [-o out]       # domain adapter/conformance scaffold
fslc domain replay <file.fsl> --logs events.jsonl              # domain runtime replay evidence
fslc db check  <file.fsl> [--depth K] [--engine bmc|induction] # dbsystem compatibility findings (§13.5)
fslc db observe <file.fsl> --trace events.json                  # runtime observation evidence
fslc db import <file.sql> [--name Name] [-o out.fsl]            # minimal SQL DDL -> dbsystem
fslc ai check <file.fsl> [--depth K] [--engine bmc|induction]   # ai_component hard-contract findings (§13.6)
fslc ai replay <file.fsl> --logs events.jsonl                   # AI runtime event replay evidence
```

エディションの lint findings は、安定したタクソノミー `deprecated`、
`non_canonical`、`ambiguous_intent`、`unsupported_in_edition` を使います。
マイグレーションは、レガシーの domain enum/演算子、宣言の文字列メタデータ、コロン
量化子、曖昧さのないローカルなインライン action マッピング、暗黙のデフォルトを
扱います。安全と証明できないソース/コメントの移動は拒否します。`&&` は引き続き
不正なトークンであり、そのため `and` の提案つきで報告されますが、機械適用される
ことはありません。原子性と一括更新の手順は `docs/DESIGN-migration.md` を参照。

ネイティブ Rust 専用の `kernel` コマンドは、ダイアレクトの lowering と意味検査の
後に実行されます。そのバージョン付き JSON は、すべての式の構造的な型、ソースの
スパン、要件/lowering の origin、同時更新の意味論、明示的な部分演算の失敗条件を
含みます。外部のコンパイラが Python の AST や FSL の式パーサーを必要とすることは
ありません。`conformance` は、コンパニオンスキーマの下で、有界の具象
success/disabled/rollback-failure ベクターを出力します。
ネストした Option はタグ付きの `none`/`some` オブジェクトを使うので、到達可能な
状態が潰れることはありません。Public Kernel v1 は `compose` のエクスポートを明示的
に拒否します。現在の lowering がコンポーネントごとの正確なファイル名を保持しない
ためです。direct とその他の lowering されたダイアレクトは引き続きサポートされます。
互換性の規則、スキーマ、フィクスチャ、Rust API のエントリポイントは
[`DESIGN-kernel-contract.md`](DESIGN-kernel-contract.md) で規定されています。
Public Kernel v2 はオプトインで、
[`DESIGN-kernel-origin-v2.md`](DESIGN-kernel-origin-v2.md) により規定されます。
損失のある v1 の origin オブジェクトを、クエリ可能な provenance グラフへのターゲット
参照で置き換えます。ソースカバレッジに依存する前に、その `completeness` フィールド
を確認しなければなりません。

`verify --from-state state.json` は、宣言された `init` を 1 回の有界実行のために
置き換え、その完全な現在状態から何が起こりうるかを問います。JSON の形は正確に
`Monitor.state` / replay の論理状態です: すべての変数と Map のキーが必須で、enum は
メンバー名、Option は値または `null`、Set/Seq は配列、relation はペアの配列です。
欠落/余剰/型不正の値は、解く前に拒否されます。スナップショット実行は verdict
キャッシュをバイパスし、対称性簡約を無効化し(同一性が具体的であるため)、
`--engine induction` を拒否します。結果には `initial_state.source:"snapshot"` と
`faithfulness:{scope:"bounded_from_snapshot",spec_init:"not_used",induction:"not_applicable"}`
が追加されるので、有界の `verified` が spec の init からの検証と取り違えられる
ことはありません。`DESIGN-from-state.md` を参照。

`reachable` と action カバレッジに加えて、`scenarios` は各 `leadsTo P ~> Q` に
ついて `respond_<Name>[_<binding>]` シナリオを出力します。各シナリオは
`kind: "leadsTo"`、`pending_at`、`satisfied_at`、`bindings`、`steps`、
`initial_state`、`expected_states` を持ち、P の成立から深さ K 以内での Q の成立
までの最短トレースを表します。P が決して成立しないバインディングはシナリオに
ならず、`warnings` に現れます。

`verify --property Name` は invariant、`trans`、`leadsTo`、`reachable` の宣言を
横断して解決し、指名されたプロパティ種別だけを単独で検査します。
`--exclude-property Name` は繰り返し指定でき、種別を横断する逆操作として機能します:
指名された invariant、`trans`、`leadsTo`、`reachable` プロパティを、実行と、検査済み
プロパティの出力(`invariants_checked`、`transitions_checked`、`leads_to`、
`reachables`)から取り除きます。`--property` と `--exclude-property` が同じ
プロパティを指名した場合は、除外が勝ちます。

`verify --engine induction --lemma "EXPR"` は、`unknown_cti` の修復ループのための
補助 invariant 候補を繰り返し指定で受け付けます。各式はまず、元の init/actions と
暗黙の型境界に対して、元のユーザー invariant を仮定せずに独立に証明されます。偽の
候補は、その到達可能な反例とともに `rejected` になります。帰納的でない候補は、
それ自身の CTI とともに拒否されます。不正な候補はパース/型エラーを運びます。独立の
結果が `proved` である候補だけが、元の証明に入ることができます。検証器はそれらの
候補を各ターゲット CTI 上で評価し、その CTI 上で実際に偽である最初のものを追加して
リトライします。JSON のフィールド `lemmas` と `lemma_cti_exclusions` は、裁定と
正確な CTI/違反ステップを記録します。ターゲットが `proved` に達すると、
`auxiliary_invariant_recommendation` がソースに永続化すべき宣言を出力します。この
コマンドがファイルを書き換えることは決してありません。未検証の仮定モードは
ありません。BMC エンジンでの `--lemma` は使用法エラーです。候補のテキストと順序は
検証キャッシュのキーの一部です。

`sweep` は `verify` のオプトインのラッパーで、通常の検証を変えません。
`--instances NAME=lo..hi`、`--values NAME=lo..hi`、`--depth lo..hi` の上書きの
決定的なグリッドを評価し、各基盤の検証結果を `sweep.results` に記録し、最初に失敗
したスコープを `sweep.minimal_counterexample` として返します。`--values` に
ついては、sweep は下限を固定して上限を拡大します(`lo..lo`, `lo..lo+1`, ...,
`lo..hi`)。sweep の合格は「このグリッドに反例がない」ことを意味し、非有界の証明
ではありません。

`diff` は、ソーステキストではなく状態機械の意味を比較します。有界の refinement を
両方向に実行します: NEW→OLD の失敗は `behavior_added`、OLD→NEW の失敗は
`behavior_removed` です。ユーザー invariant の連言の間の含意を別途検査し
(`invariant_weakened` / `invariant_strengthened`)、OLD の `forbidden` シナリオを
NEW に対して replay します(`forbidden_relaxed`)。方向性のある失敗は反例の
witness を含みます。同名で互換な state/action は自動でマッピングされ、名前の
不一致は、`--mapping` がその方向を提供しない限り `unknown` です。任意のマッピング
が自動で反転されることはありません。

JSON の結果は、`bounded`、`scope`、`directions`、`summary`、`findings`、`gate` を
持つ `semantic_diff` です。変更された `verify { instances/values }` スコープは
`scope_changed` として報告され、共有された OLD の境界は、`scope.comparison:"new"`
と `scope.applied_to_old` に記録された NEW のスコープの下で再構築されます。findings
がなければ `summary` は `["no_semantic_change"]` です。findings は分析出力なので
デフォルトでは exit 0 です。カンマ区切りの `--forbid` に明示的に列挙された findings
だけがゲートを失敗させ、exit 1 にします。すべての比較は `--depth` に有界のままです。
クリーンな出力は非有界の等価性証明ではありません。

`diff --git BASE..HEAD [spec.fsl]` は VCS/CI アダプタです。両方の完全なコミット
ハッシュを解決・記録し、両方の完全な追跡ツリーを実体化し、そのうえで同じ 2 パスの
比較を呼び出します。これにより、相対 import は自身のリビジョンから解決されます。
spec を省略すると、変更されたすべての `.fsl` パスを比較して `semantic_diff_batch`
を返します。1 つの spec を与えると `semantic_diff` が保たれます。両方の形式が
`vcs.materialization: "git_archive_full_tree"` を含みます。通常の 2 パス形式は
決して Git を呼び出さず、リポジトリの外でも動作します。

`approval create` は、レビュー済みの `ledger`、`html`、`scenarios` アーティファクト
を、完全に lowering された位置非依存のカーネルダイジェストと、現在のクリーンな Git
コミットに束縛します。バージョン付きの JSON サイドカーは、正規化された
アーティファクトのダイジェスト、ジェネレータ/バージョン/オプション、承認された
要件 ID、承認者、UTC 時刻も記録します。作成はアーティファクトを再生成し、古い
レビューファイルを拒否します。`approval check` は、spec、レンダリング、レンダラの
バインディングがすべて一致している間だけ `approved` を返します。そうでなければ
`spec_changed`、`rendering_changed`、`renderer_changed` を伴う `drifted` を返します。
`ledger --approval` は要件ごとに同じステータスを追加し、完全なベースラインの
ダイジェストを含みます。`approval diff` は承認されたコミットを実体化し、現在の作業
ファイルに対して通常の有界セマンティック diff を呼び出します。
`docs/DESIGN-approval.md` を参照。

`--signing-key` がなければ、`approval create` は従来どおり無署名の
`fslc.approval.v1` レコードを出力します。Ed25519 PKCS#8 PEM の署名鍵があれば
`fslc.approval.v2` を出力します。そのレコードを使うすべての check、diff、ledger は
対応する SPKI PEM の `--trust-key` を要求します。v2 の分離署名は完全な正準レコード
をカバーし、暗号的な不一致は承認ではなく `signature-invalid` です。trust-key の
所持は署名者を認証しますが、認可は依然として組織のポリシーの問題です。正確な正準化
と鍵のフォーマットは `docs/DESIGN-approval.md` で規定されています。

Exit code: `0` = verified / proved / scenarios/testgen の生成 / conformant / refines /
mutated / explained / analyzed / semantic_diff(明示的なゲートが失敗しない限り) /
typestate / sweep_passed / observed_conformant /
imported / imported_with_warnings、
`1` = violated / reachable_failed / unknown_cti / unknown_budget / nonconformant /
refinement_failed / sweep_failed / observed_mismatch、
`2` = spec エラー(parse / type / semantics / io / vacuous / acceptance / forbidden /
`--vacuity error`)、`3` = 内部エラー。`observed_*` は `fslc db observe` の結果、
`imported`/`imported_with_warnings` は `fslc db import` の結果です。

### 結果の種類

| result | 意味 | 次の一手 |
|---|---|---|
| `verified` | 深さ K まで違反なし(+ すべての reachable が満たされた)。`completeness:"bounded"` | 確信度を上げるには `--engine induction` を使う |
| `proved` | **invariant がすべての実行で成立する**(深さ非有界)。`completeness:"unbounded"`。`--engine induction` から、または探索が閉じたとき(`closure:true`)の `--engine explicit` から | 完了 |
| `violated` | 反例が存在する。`violation_kind` と最短トレースつき | トレースを読んで spec を直す |
| `reachable_failed` | reachable が深さ K 以内に到達されなかった | 各 `unreached[].classification` を読む: `insufficient_depth` なら `--depth` を上げ、`over_constrained` ならブロックしている制約を直す |
| `unknown_cti` | invariant は違反されないが帰納的でない | **CTI を読んで補助 invariant を追加する**(§8)か、`--engine explicit` を試す(closure はレンマなしで証明する) |
| `unknown_budget` | `--engine explicit` が閉じる前に `--explicit-budget` を超えた | 予算を上げるか、この spec には `--engine bmc`/`induction` を使う |
| `error` | parse / type / semantics / io | `loc` / `expected` / `hint` に従って直す |

`--engine auto` は explicit と bmc を合成します: まず explicit を試し(より速く、
`proved`+`closure:true` に到達できます)、explicit が単独ではその spec を判定でき
ない場合 — 未対応の機能(`leadsTo`、非決定的/部分的な init)または `unknown_budget`
— に透過的に bmc へフォールバックします。すべての結果は、実際に判定したエンジンを
名指しする `engine: "explicit"` または `engine: "bmc"` を運びます。フォールバックは
さらに
`engine_fallback: {from: "explicit", reason: "...", kind: "unsupported"|"budget"}`
を運ぶので、呼び出し側は再導出することなく、有界の bmc verdict と非有界の explicit
verdict を区別できます。`auto` がデフォルトのエンジンを変えることはなく(依然
`bmc`)、`explicit` 自体と同じく Rust 専用です。
`docs/DESIGN-explicit-engine.md` §6a を参照。

`violation_kind`: `invariant` | `trans` | `ensures` | `type_bound` | `partial_op` | `deadlock` | `leadsTo`。

`violation_kind` が `invariant`/`type_bound` の `violated` 結果は、さらに
**ブレーム割り当て(blame assignment)**(issue #170)を運び、反例をただ見せるの
ではなくローカライズします: トップレベルの `blame.conjuncts[]`(`{index, text,
holds, violating_bindings?}`)は、invariant が複数の AND 連言肢から構成される場合に
どの連言肢が偽であるかを名指しします(そうでなければ 1 要素のリスト)。action を
伴う各 `trace[k]`(k≥1)は、そのステップでブレームされた連言肢に影響した
`requires` 節と状態を書く文を名指しする、それ自身の `blame: {guards[], effects[]}`
を得ます(具象反例上の後方スライスであって、新しいソルバークエリではありません)。
`fslc explain` の counterfactual は両方を自動的に継承します。vacuity findings
(`vacuous_implication` / `vacuous_leadsto`)は `classification`
(`insufficient_depth` | `over_constrained`)と `blocking`(前件/トリガーを不可能に
している他の invariant。深さ内で単に未到達なだけのときは空)を得ます —
`reachable_failed` の `unreached[].blocking_requires` と同じ形です。
blame は特定するだけで、修復(ガードの弱化、連言肢の削除)を提案することは決して
ありません — それは anti-hollowing の原則に反するからです。これらはすべて JSON
コントラクトへの厳密に追加的な変更です。

忠実性/意図のギャップを特定する診断は、`faithfulness_class` と
`recommended_action` も運ぶことがあります。現在のクラスは
`partial_op_unguarded`、`frozen_only_invariant`、`intent_unexercised`、
`liveness_not_refined` です。このタグは既存の `result` / `kind` /
`violation_kind` フィールドから導出される追加的なものです。消費者は詳細のために
元の分類フィールドを読み続けるべきです。

進行を保存する refinement の失敗は、`refinement_failed` として、
`kind:"progress_lost"`、`violation_kind:"leadsTo"`、`impl_trace`、
`progress:{leadsTo, actions}`、`faithfulness_class:"liveness_not_refined"` を伴って
報告されます。

すべての `verify` 結果は、`checked_to_depth` と 1 つの固定された `cost`
オブジェクトを含みます: 合計の `elapsed_s`、`solver` の検査回数/時間と nullable な
共通 Z3 統計、そしてプロパティごとの検査回数と時間を持つ決定的な `properties` 行
です。ネイティブとブラウザの Worker は同じキーと nullability を使います。Z3 の
カウンターは、累積かもしれないスナップショットの合計ではなく、構成する検査を通じて
観測された最大のスナップショットです。タイミングの値は非決定的です。explicit
エンジンは、ソルバー検査ゼロ・Z3 統計 null で同じ形を使います。
[`DESIGN-verification-cost.md`](DESIGN-verification-cost.md) を参照。BMC の
`verified` は明示的に有界です。最終深さが、通常の探索の中で
reachable/vacuity/coverage の事実を最初に目撃したとき、`verified` には、その深さで
状態空間が明らかには飽和していないことを示し、より大きな `--depth` または
induction を提案する `hint` も含まれます。

ネイティブとブラウザの BMC は、Z3 の `random_seed` と `smt.random_seed` を `0` に
設定します。これにより各バックエンドは固定の Z3 ビルドの下で決定的になりますが、
具象反例と reachable/deadlock の witness は、ネイティブと WebAssembly のビルドの間
で一意ではないままです。消費者は、特定の充足割り当てではなく、verdict と再生可能な
witness のコントラクトを使わなければなりません。

leadsTo が宣言されていて結果が `verified` / `proved` のとき、
`leads_to: { "<Name>": { "checked_to_depth": K } }` が付与されます
(反例がないことは深さ K までの有界の保証で、`verified` な invariant と同じ地位
です)。induction がランク付き `leadsTo` を discharge した場合、そのエントリは
`proved: true`、`completeness: "unbounded"`、`proof: "ranking"`、`decreases` で
アップグレードされます。`trans` が宣言されているとき、成功の出力は
`transitions_checked: ["Name", ...]` を運びます。

### カバレッジ診断(決して enabled にならない action)

```json
"action_coverage": {
  "checkout": {
    "covered": false,
    "name": "checkout",
    "blocking_requires": [ {"loc": {"line": 27}, "text": "requires stock[i] > 0"} ],
    "hint": "never enabled within depth 8; blocking requires: requires stock[i] > 0; ...",
    "faithfulness_class": "intent_unexercised",
    "recommended_action": "add a single-shot reachable for the action / raise --depth"
  }
}
```

ブロックしている requires 節は、それが安価な場合、最小化された unsat core によって
特定されます。requirements の `branches` については、偽のカバレッジ診断は内部の
分割 action の `name` を保持し、`display_name` を追加します。

`reachable_failed` については、各 `unreached` エントリは以下を運びます:

```json
{
  "name": "SoldOut",
  "classification": "insufficient_depth",
  "hint": "not witnessed within depth 3; try a larger --depth"
}
```

または、ターゲットの述語が型境界/invariant の下で充足不能な場合:

```json
{
  "name": "TooHigh",
  "classification": "over_constrained",
  "blocking_requires": [{"kind": "type_bound", "name": "_bounds_x"}],
  "hint": "target predicate is unsatisfiable under type bounds/invariants (_bounds_x); ..."
}
```

## 8. 推奨ワークフロー: proved を標準にする

1. spec を書く → `fslc check`(高速な構文/型のループ)
2. `fslc verify --depth 8` → violated ならトレースを使って直す。
   意図したシナリオが reachable によって目撃されることを確認する
3. `fslc verify --engine induction` → `proved` なら完了
4. `unknown_cti` なら、CTI(k+1 状態のトレース)を読む。CTI の開始状態は
   「すべての invariant を満たすが、実際には到達不能」な**ゴースト状態**である。
   それを排除する**補助 invariant**(それ自体がドメインの真実であるもの)を追加して
   ステップ 3 に戻る

有用な補助 invariant は、証明専用の人工物ではなく「attempts == 3 なら locked」
「refund を持つのは Captured だけ」「キューに重複はない」のようなドメインの真実です。

## 9. イディオム集

### 番兵値の代わりに Option

```fsl
cart: Map<UserId, Option<ItemId>>      // do not use a sentinel like -1
struct Reservation { item: Option<ItemId>, qty: Qty }  // optional fields can be written directly too
action checkout(u: UserId) {
  requires cart[u] is some(i)          // i is bound here
  requires stock[i] > 0
  stock[i] = stock[i] - 1
  cart[u] = none
}
```

### 手書きの境界 invariant の代わりにドメイン型

```fsl
type Qty = 0..5
state { stock: Map<ItemId, Qty> }      // do not write NoNegativeStock (automatic)
```

### 部分演算のガード(requires 形式か if 形式のどちらか)

```fsl
action take()  { requires q.size() > 0  x = q.head()  q = q.pop() }
action drain() { if q.size() > 0 { x = q.head()  q = q.pop() } }
```

ガードを忘れると `partial_op` 違反として検出されます(黙って壊れることは
ありません)。

### invariant で Seq について語る: インデックスガード付きの forall

```fsl
invariant QueuedAreQueued {
  forall i in 0..2 {                   // 0..capacity-1
    i < queue.size() => jobs[queue.at(i)].st == Queued
  }
}
```

`at()` はプロパティの文脈では全域です(範囲外は不特定の値を生みます)ので、常に
`i < q.size()` でガードしてください。

### Seq 上の集約: インデックス / ドメイン型のイディオム

```fsl
type Idx = 0..3                        // a domain type covering up to capacity-1
invariant BalanceMatchesLog {
  balance == sum(i: Idx of log.at(i) where i < log.size())
}
```

`sum`/`count` はドメイン型の上を走りますが、`where i < size` で live な
プレフィックスに制限すると **Seq 上の fold** になります。

### 2 次元データ(部屋 × スロットなど): 単一キーへの平坦化

`Map<RoomId, Map<SlotId, …>>` のような **Map のネスト**は許されません(§2)。
2 つの軸を単一の積ドメイン型へ平坦化し、`/` と `%` で軸を復元します:

```fsl
const SLOTS = 4
type RoomId = 0..2
type Cell   = 0..11                       // ROOMS*SLOTS - 1
state { holder: Map<Cell, Option<UserId>> }
// c / SLOTS = room, c % SLOTS = slot
reachable Room1Full {
  forall c: Cell { c / SLOTS == 1 => holder[c] != none }
}
```

軸が少なくて名前があるとき(例: 固定 5 コマの曜日)には struct のフィールドへ分解
する選択肢もありますが、量化が必要なら平坦化がデフォルトです。

### 履歴(過去)について語るには ghost 変数を使う

```fsl
state { ever_locked: Map<UserId, Bool> }   // "was locked at least once"
// in the locking branch, ever_locked[u] = true
reachable RecoveredAfterLock {
  exists u: UserId { ever_locked[u] and session[u] }
}
```

reachable / invariant は状態だけを見るので、「X の後の Y」を**状態の事実**として
語るには、履歴を状態へ押し込みます(ghost 変数)。

### 履歴の ghost 変数と leadsTo の使い分け

| 書きたいこと | 手段 |
|---|---|
| 「少なくとも一度は X だった」(状態の事実) | ghost 変数 + invariant / reachable |
| 「Y まで X を保つ。Y は起きても起きなくてもよい」 | `unless Name { X unless Y }` |
| 「一度 X になったら、いずれ Y」(応答プロパティ) | `leadsTo` + 必要なら `fair action` |
| 「Y まで X を保ち、Y は必ずいずれ起きる」 | `until Name { X until Y }` |

例: FIFO ミューテックスで「待ちキューに入ったプロセスはいずれロックを得る」は
`leadsTo WaiterGetsLock { forall p: ProcId { waiters.contains(p) ~> ... } }` です。
進行が `release_handoff` のような特定の action に依存するなら、`fair` を付けます
(`specs/mutex_queue.fsl` を参照)。

### CTI からの補助 invariant(induction の強化)

`unknown_cti` の CTI の開始状態を見て、「現実には起こらない組み合わせ」を
invariant にします:

```fsl
// CTI: queue = [0, 0, 0] (the same job tripled) → state that there are no duplicates
invariant NoDupQueue {
  forall i in 0..2 { forall j in 0..2 {
    (i < j and j < queue.size()) => not (queue.at(i) == queue.at(j))
  } }
}
```

よくある「単調カウンター」のイディオム — 一方向にしか動かない `Int` または
`Map<K, Int>` の状態変数 — については、CTI のカウンターがその具象初期値の到達不能
側から始まる場合(実際の実行が決して生み出せない巨大または負の「ゴースト」の開始
など)、`unknown_cti` の結果は追加的な `suggested_invariants: [<expr>, ...]`
フィールド(および `hint` に付加される対応する文)を運びます。これは CTI の
トレースを具象の init と diff して計算されるヒューリスティックであり(大域的な
単調性の証明ではありません)、出発点として扱ってください:

```fsl
// CTI: audit = -101 (only increases in this trace, but starts below its
// init value 0) → suggested_invariants: ["audit >= 0"]
invariant AuditNonNeg { audit >= 0 }
```

`Map<K, Int>` のカウンターは、すべてのキーが同じ初期値を共有するとき、`forall` で
量化した形(`forall k: K { audit[k] >= 0 }`)を提案します。単調カウンターが検出
されない場合、または CTI の開始が想定される境界を違反しない場合、提案は追加されず
`hint` は変わりません。

## 10. 詳細化(詳細 spec の忠実性)

抽象 spec(abs)をまず `verify` / `prove` した後、実装に近い詳細 spec(impl)が
abs の振る舞いから逸脱していないことを **`fslc refine`** で検査します
(`DESIGN-refinement.md` を参照)。

マッピングは**別ファイル**に書きます(impl/abs の `.fsl` を汚さないでください):

```fsl
refinement CartImplRefinesCart {
  impl CartImpl
  abs  ShoppingCart

  maps auto
  map stock[i: ItemId] = impl_stock[i] - reserved[i]
  map cart[u: UserId]  = impl_cart[u]

  action add_to_cart(u: UserId, i: ItemId) -> add_to_cart(u, i)
  action impl_checkout(u: UserId)          -> checkout(u)
  action reserve(i: ItemId)                -> stutter
}
```

- `map <abs var> = <impl expr>` — スカラーの抽象変数。
- `map <abs var>[<binder>] = <expr>` — Map の要素ごとのマッピング(キー型を列挙
  します。キー型は有界です)。
- `maps auto` — 省略可能な恒等デフォルト。明示の `map` のない同名で互換な状態変数
  には `map x = x` を合成し、明示の対応のない同名で互換な action には
  `action f(params...) -> f(params...)` を合成します。明示のエントリはデフォルトを
  上書きします。互換でない同名の候補は `kind: "type"` エラーとして報告されます。
- `action <impl>(<formal params>) -> <abs>(<expr>) | stutter` — すべての impl
  action に必須です。形式パラメータは裸の名前でも、impl の action 宣言に一致する
  `name: Type` の注釈でもかまいません。
  `stutter` は、抽象状態が変化しない内部ステップです。

4 つの記述経路すべて — スタンドアロンの `action`、インライン `implements` の
action、requirement-action の `maps`、auto/恒等の合成 — は、同じ型付き action
対応 IR へ解決されます。共通の検証は、impl の同一性と型付きパラメータ、ターゲット
の同一性/アリティ、引数の式、auto マップされた actor の互換性を検査します。重複の
診断は両方の起源種別とソース位置を名指しします。明示のエントリは依然として auto
合成に優先します。

refinement のマップと action の引数は、`if <condition> then <expr> else <expr>` を
含め、通常の spec と同じ式文法と型規則を使います。条件が定数であっても、両方の分岐
が名前と型の検査を受けます。

```bash
fslc refine specs/cart_impl.fsl specs/cart_v1.fsl specs/cart_refines.fsl --depth 8
```

成功: `result: "refines"`(exit 0)。違反: `refinement_failed`(exit 1)で、
`kind`(`abs_requires_failed` / `abs_state_mismatch` / `stutter_changed_abs` /
`map_out_of_bounds`)、`impl_trace`、マッピング後の `abs_before` /
`abs_after_*` を伴います。静的なエラー(マップの欠落、未知の action など)は
`kind: "type"`(exit 2)です。

状態変数のペアが 1:1 でマップされる場合でも、impl と abs には**別々の enum/struct
型名**を与えてください。型メタデータは refinement 検査のために名前でマージされ
ます。両側で異なるメンバーリスト(またはフィールド集合)を持って宣言された同名の
enum(または struct)は、マージされるのではなく `kind: "type"`(exit 2)として拒否
されます — マージすると、impl 専用のメンバーが、同じ順序位置に座っている abs の
メンバーとして黙って再解釈されかねないからです。ドメイン型
(`type X = lo..hi`)は異なる境界で安全に名前を共有できます。そこでの範囲外の値は
依然として `map_out_of_bounds`/`abs_state_mismatch` として捕捉されます。

### チェーン検査(マッピングの合成)

レイヤーのチェーン(business ⊒ requirements ⊒ design …)の端から端までの忠実性は、
`(spec mapping)` を順に並べると、**合成によって直接**検査できます:

```bash
fslc refine bot.fsl  mid.fsl bot_refines_mid.fsl  top.fsl mid_refines_top.fsl --depth 6
#            ^impl    ^abs1   ^map(impl→abs1)      ^abs2   ^map(abs1→abs2)
```

隣接するマッピングを合成し(状態 α_AC = α_BC ∘ α_AB、action は a→b→c /
stutter)、bottom ⊒ top を検査します。成功時は合成された `action_map` とレイヤーの
順序 `chain` を返します。失敗時は最初に壊れたリンク
`failed_link: {from, to, kind}` を返します。有界の refinement は同じ深さで推移的
なので、合成の検査はすべての隣接リンクが成立することと等価です
(`docs/DESIGN-refinement.md` §7、例 `examples/refinement_chain`)。
引数の式が中間レイヤーの状態を読むケースだけが未対応です。

推奨ワークフロー: **人間/LLM が abs をレビュー → LLM が impl を詳細化 →
`refine` が忠実性を保証**。abs の `ensures` / invariant は refine では再検査され
ません。abs 側で別途検証済みであることが前提です。

`refine` が保証するのは**安全性の包含**(impl が abs のガード/invariant を壊さ
ない)です。**liveness(`leadsTo`)は伝播しません** — refine は stutter を許すので、
abs が `fair` で保証した進行を impl が落としても、結果は依然 `refines` になりえます
(マッピングは fair アノテーションを要求しません)。抽象の応答を下位レイヤーで
検査することにオプトインするには、refinement マッピングに `preserve progress` を
書きます:

```fsl
refinement DesignRefinesReq {
  impl Design
  abs  Req
  map st = ...
  action enqueue(c) -> stutter
  action answer(c)  -> answer(c)
  action refuse(c)  -> refuse(c)

  preserve progress {
    respond EveryRequestHandled by answer, refuse
  }
}
```

これは、指名された抽象の `leadsTo` を状態マッピングを通して引き込み、impl の実行の
上で `P(α(impl_state)) ~> Q(α(impl_state))` を検査します。抽象の応答が pending の
まま下位レイヤーが永遠にスピンしたりデッドロックしたりできる場合、結果は
`kind:"progress_lost"` と `violation_kind:"leadsTo"` を伴う `refinement_failed`
です。`progress_failure` は `lasso_blocks_progress` と
`deadlock_or_stall_blocks_progress` を区別します。`by` の action はレビュー用の
メタデータで、impl の action を名指ししなければなりません。公平性を作ることも、
実装のコンフォーマンスを証明することもありません。公平性は依然として下位レイヤーの
`fair action` 宣言から来ます。
非有界の証明には、引き続き下位レイヤーの `leadsTo ... decreases ...` と
`verify --engine induction` を使ってください。

## 11. 合成(compose)

検証済みのいくつかのコンポーネント spec を、**名前空間つきで** 1 つのシステム仕様
にマージします。展開後は通常の単一 spec になるので、`verify` /
`prove` / `scenarios` / `Monitor` / `replay` / `testgen` / `refine` がそのまま
使えます(設計: `DESIGN-compose.md`)。

```fsl
compose OrderSystem {
  use ShoppingCart as cart from "cart_v1.fsl"
  use Payment      as pay  from "payment.fsl"

  state { orders_linked: Int }
  init  { orders_linked = 0 }

  // synchronized action: execute the actions of several components in the same step
  action checkout_and_pay(u: cart.UserId, p: pay.PayId) =
      cart.checkout(u) || pay.capture(p) {
    requires pay.payments[p].st == Authorized
    orders_linked = orders_linked + 1
  }

  // excluded from standalone execution (fires only via synchronization)
  internal cart.checkout
  internal pay.capture

  invariant LinkedNonNeg { orders_linked >= 0 }
  reachable PaidOrder {
    exists p: pay.PayId { pay.payments[p].st == Captured }
  }
}
```

- `use <SpecName> as <alias> from "<relative path>"` — パスは compose ファイルから
  の相対です。spec 名はファイル内の名前と一致しなければなりません。alias は
  compose 内で一意でなければなりません。compose のネストは許されません。
- コンポーネントの型/状態/action は `alias.Name` で参照します。
- **同期 action** `action <name>(...) = <a>.<act>(...) || <b>.<act2>(...) { ... }`:
  各コンポーネント action の requires / 本体 / ensures をマージします。追加の文は
  合成側の状態にのみ代入できます
  (同じコンポーネントの 2 つの action を同期させることはできません)。
- 公平性は同期を通じて継承されません。fair なコンポーネント action が非 fair な
  同期 action から参照されると、`check` / `verify` は JSON の `warnings` に
  `fair_not_inherited` 警告を出します。同期 action が fair でなければならないとき
  は、`fair action <name>(...) = ...` と書いてください。
- 同期 action の引数は、宣言された型名ではなく、有界整数ドメインによって構造的に
  互換です。`core.TaskId` の値を `NoteId` と宣言された action パラメータへ渡すこと
  は、基礎となる値の範囲がターゲット型に収まるとき有効です。これは意図された
  compose の挙動であって、偶発的な命名の偶然ではありません。再現:
  `TaskId = 0..2` と `NoteId = 0..2` のとき、
  `action sync(t: core.TaskId) = core.choose(t) || note.attach(t) { }` は
  `fslc check` から `result:"ok"` を、`fslc verify --depth 1` から
  `result:"verified"` を生みました。`NoteId = 0..1` では、同じ compose は依然
  `check` を通りますが、`verify --depth 1` は `sync(t: 2)` について
  `result:"violated"`、`violation_kind:"type_bound"`、invariant
  `"_bounds_note.last"` を返しました。推奨イディオム: 意図的に共有される ID には
  同じ範囲のコンポーネントローカルなドメイン型を使うこと。ターゲットのドメインが
  より狭い場合は、同期 action に明示の `requires` ガードを追加するか、片方の
  コンポーネントで変換をモデル化してください。
- `internal <alias>.<action>` — その action をインターリービングから除外します。
- (`=` のない)通常の `action` も書けます(グルー action)。
- JSON の表示: 物理名 `alias__x` は `alias.x` として出力されます(状態のキー、
  action 名、invariant / reachable 名、トレース、シナリオ、Monitor — すべて)。

```bash
fslc check  specs/order_system.fsl
fslc verify specs/order_system.fsl --depth 8
fslc scenarios specs/order_system.fsl
```

## 12. 実装への橋

仕様を証明した後、それを実装へ結線するためのエントリポイントは 3 つあります
(`DESIGN-bridge.md` を参照)。

| 手段 | 用途 |
|---|---|
| `fslc.runtime.Monitor` | spec の具象インタープリタ(Z3 不要)。実装に埋め込んでランタイム検査を行う |
| `fslc replay` | 実システムのイベントログ JSON を spec に対して検査する |
| `fslc testgen` | コンフォーマンステストのスキャフォールドを生成する — pytest(デフォルト)、Vitest(`--target vitest`)、Swift Testing(`--target swift`)、kotlin.test(`--target kotlin`)、Dart `package:test`(`--target dart`)、PHPUnit(`--target phpunit`)(実装を Adapter に結線する) |

推奨ワークフロー: **spec を `verify` / `prove` する → `testgen` でスキャフォールド
を生成する → 実装を `Adapter` に結線する → テストを実行する**。`Monitor` は
ランダムウォークテストのオラクルとして使われます。

`testgen` は、言語非依存のシナリオ収集コア(`scenarios`)をターゲットごとの
エミッタから分離しているので、同じシナリオが複数のハーネスへレンダリングされます。
ネイティブ実装では、6 つのエミッタすべてが、Public Kernel v1、シナリオ JSON、
バージョン付き固定シードの `testgen-trace.v1` コンフォーマンストレースから構築
された 1 つの検証済みアダプタを消費します。非公開のモデルや AST は読みません。
スキーマ/バージョン/spec の不一致、未知の state/action/パラメータ名、不正な入力は
fail closed です。compose は、明示の検査済み names/order ブリッジを使います。
Public Kernel は不完全なマルチファイルの provenance を意図的に拒否するためです。
エクスポートエラーの後のフォールバックではなく、同じアダプタに入ります。

- `--target pytest`(デフォルト): `fslc.runtime.Monitor` をインポートし、オラクル
  としてランダムウォークをライブで駆動する Python テストを出力します。
- `--target vitest`: 自己完結の TypeScript(Vitest)ファイルを出力します。決定的な
  シナリオと forbidden 拒否のアサーションは直接翻訳され、ランダムウォークは
  **生成時に焼き込まれます** — 具象の Monitor が固定シードのウォークを実行し、
  `(action, params, expected_state)` のトレースが静的なフィクスチャとして埋め込ま
  れるので、生成されたテストは実行時に **`fslc`/Python を必要としません**。出力の
  拡張子のデフォルトは `<spec>.test.ts` です。
- `--target swift`: 自己完結の Swift Testing ファイルを出力します(`import Testing`、
  `@Test`、`#expect`。**XCTest ではありません**)。Vitest と同じ焼き込みウォーク
  設計です。動的な状態は `[String: Any]` で、深い等値/部分一致のヘルパーが同梱
  されます。Option の `None` は自己完結の `FSLNull.instance` 番兵として焼き込まれ
  ます(Foundation 不要)。`makeAdapter()` が結線されるまで、すべてのテストは
  `@Test(.enabled(if:))` で無効化されます。出力のデフォルトは
  `<SpecName>ConformanceTests.swift` です。
- `--target kotlin`: 自己完結の kotlin.test ファイルを出力します(マルチ
  プラットフォーム。JVM は JUnit に委譲します)。同じ焼き込みウォーク設計です。
  動的な状態は `Map<String, Any?>` で、Kotlin の構造的な `==` は `List`/`Map` 上で
  深く、`Int` と `Double` を区別するので、部分一致ヘルパーは素朴な再帰です。
  kotlin.test にはポータブルなランタイムスキップがないので、`makeAdapter()` が
  結線される(`null` を返します)まで各テストは早期リターンします。出力のデフォルト
  は `<SpecName>ConformanceTest.kt` です。
- `--target dart`: 自己完結の `package:test` ファイルを出力します(`flutter test`
  でも動きます)。同じ焼き込みウォーク設計です。動的な状態は
  `Map<String, dynamic>` です。Dart の `==` はコレクション上で参照ベースなので、
  同梱の `assertPartial` は期待されるキーで再帰し、葉/列を `equals` マッチャー
  (`package:test` が再エクスポートするので、唯一の依存は `package:test`)で比較
  します。トップレベルのプローブが、`makeAdapter()` が結線されるまで各 `test()` に
  `skip:` を設定します。出力のデフォルトは `<spec_name>_conformance_test.dart`
  (snake_case。ランナーが期待する `_test.dart` サフィックス)です。
- `--target phpunit`: 自己完結の PHPUnit ファイルを出力します(PHP 8.1+ /
  PHPUnit 10+、`declare(strict_types=1)`)。同じ焼き込みウォーク設計です。動的な
  状態は連想 `array` です。葉は `assertSame`(`===`)で比較され、`int`/`float`、
  `bool`、`null` の強制変換を防ぎます(PHP の緩い `==` は `0 == "0"` などを混同
  します)。`assertPartial` は期待されるキーで再帰します(マップは順序に依存せず
  一致し、リスト形の値は長さも固定します)。`setUp()` は `makeAdapter()` が結線
  されるまですべてのテストをスキップします。出力のデフォルトは
  `<SpecName>ConformanceTest.php`(PSR-4 のクラス = ファイル名)です。

```python
from fslc import Monitor

mon = Monitor("specs/cart_v1.fsl")
mon.reset()
r = mon.step("add_to_cart", {"u": 0, "i": 0})   # ok / kind / state / changes
```

```bash
fslc replay specs/cart_v1.fsl --trace events.json   # conformant / nonconformant
fslc replay specs/cart_v1.fsl --from-log production.jsonl --mapping log_mapping.fsl
fslc testgen specs/cart_v1.fsl -o test_cart_v1.py            # pytest (default); partial reachability warnings unless --strict
fslc testgen specs/cart_v1.fsl --target vitest -o cart.test.ts  # self-contained Vitest (TypeScript) scaffold
fslc testgen specs/cart_v1.fsl --target swift -o CartConformanceTests.swift  # self-contained Swift Testing scaffold
fslc testgen specs/cart_v1.fsl --target kotlin -o CartConformanceTest.kt  # self-contained kotlin.test scaffold
fslc testgen specs/cart_v1.fsl --target dart -o cart_conformance_test.dart  # self-contained package:test scaffold
fslc testgen specs/cart_v1.fsl --target phpunit -o CartConformanceTest.php  # self-contained PHPUnit scaffold
```

外部のコンパイラは、ネイティブの replay コントラクトを、閉じたバージョン付き JSON
オブジェクト(`schemas/fslc/kernel/replay-trace.v1.schema.json`)として出力します:

```json
{"$schema":"https://fsl.dev/schemas/fslc/kernel/replay-trace.v1.schema.json","schema_version":"1.2.0","kernel_schema_version":"1.0.0","spec":"ShoppingCart","initial":{"stock":{"0":1},"cart":{"0":0}},"events":[{"tick":1,"action":null,"params":{},"state":{"stock":{"0":1},"cart":{"0":0}}},{"tick":2,"action":"add_to_cart","params":{"u":0,"i":0},"state":{"stock":{"0":0},"cart":{"0":1}}}]}
```

`initial` は完全な tick-0 の状態で、すべてのイベントは、正確な Public Kernel の
action/パラメータ名と、その完全な事後状態を持ちます。トレーススキーマ 1.1 以降、
`{}` の params を伴う `action:null` は明示的な stutter の観測で、その完全な状態は
現在の論理状態と等しくなければなりません。1.0 は action のみのままです。
等しい状態の stutter の挿入や削除は、射影された action トレースと最終状態を保存
します。tick は stutter を含めて正確に `1..N` です。省略可能な非空の
`timestamp` は、無視される不透明なプロデューサのメタデータであり、形式的な時間では
ありません。トレース v1 は Kernel `1.0.0` と `2.0.0` を受理します。不正な
コントラクトは入力エラーとして失敗します。型付きの initial/事後状態の乖離は、葉の
不一致のエビデンスを伴う nonconformant です。invariant は観測点とアトミックな
Monitor の後続に適用され、報告されない実装の中間には適用されません。裸の配列と
`{events:[...]}` は引き続き明示の
非バージョンの action-only アダプタです。testgen と検証器のトレース JSON は replay
の入力ではありません。`docs/DESIGN-replay-trace.md` を参照。

トレーススキーマ 1.2 はさらに、初期状態と各 action/stutter の観測において、
すべての `leadsTo P ~> within K Q` を検査します。締め切り `p + K` は境界を含み
ます: そこでの `Q` は成功し、`Q` の不在は有界 liveness のエビデンスを伴って失敗
します。安全性が先に評価されます。成功の出力は `checks.safety` を
`checks.bounded_liveness` から分離します。未完の有限の義務は `pending` で、非有界の
`leadsTo` プロパティは名指しされますが、検査済みとは主張されません。スキーマ
1.0/1.1 は以前の安全性のみの意味を保ちます。

`--from-log` 形式は、外部の JSONL レコードを spec の action と論理状態へ翻訳する
ために、refinement マッピングの文法を正確に再利用します。第二のマッピング言語を
追加しません。各非空行は
`{"action":"external_name","params":{...},"state":{...}}` で、`state` は観測された
action 後の状態です。マッピングでは、`impl` は外部ログのスキーマにラベルを付け、
`abs` は replay される spec を名指ししなければならず、`map` エントリはすべての
spec 状態変数をカバーし、`action external(params) -> spec_action(exprs)`(または
`stutter`)がイベントをマップします。Monitor はマップされた action を実行し、その
結果を、すべての行でマップされた観測状態と比較します。最初の乖離は、0 始まりの
`failed_at_record` / `failed_at_event`、1 始まりの `log_line`、そして Monitor の
違反または葉のパス付きの `state_mismatch` のどちらかを報告します。

この初版は完全な観測を要求します: フィールドや Map のキーの欠落は `log_mapping`
の nonconformance であって、制約のない値ではありません。レコードのスキーマ、
マッピングの例、`db observe` / `ai replay` / `domain replay` との境界は
`DESIGN-log-replay.md` を参照。

`replay` は有限のログしか検査しないので、**`leadsTo` は範囲外**です(出力の
`note` に明記されます)。`Monitor` は init が決定的であることを要求します(forall
の一括代入は許されます)。Map/インデックスのターゲット(`m[K] = ...`)については、
「ちょうど 1 回の代入」は変数単位ではなく具象キー単位です: 2 つの*異なる*キーへの
別々のフラットな `m[K1] = ...` / `m[K2] = ...` 文は問題ありません。
同じキーへの 2 回の代入、またはキー自体が束縛されたループ変数であるもの
(2 つの反復がエイリアスしうる)は、引き続き拒否されます。

## 13. 3層ダイアレクト(業務コンサルティング / 要件 / 設計)とトレーサビリティ

設計の背景は `DESIGN-layers.md` に、実装の仕様は `DESIGN-dialects.md` にあります。
カーネルは 1 つ(本書の §1–12)で、レイヤーごとのダイアレクトは AST へ展開される
フロントエンドです。レイヤーは refinement で接続されます: **business ⊒
requirements ⊒ design ⊒ implementation (testgen/replay)**。

### 13.1 宣言タグ(全レイヤー共通のトレーサビリティ)

正準な関係構文は、リンクされる宣言の直前に置く型付きアノテーションです。違反、
CTI、カバレッジ診断、シナリオは `requirement: {id, text}` を運びます:

```fsl
@requirement("REQ-LEDGER-003", "the ledger matches the number of payments")
invariant PaidLedger { ... }

@requirement("REQ-EXPENSE-001", "amounts at or below the threshold are auto-approved")
action submit(c: Case, a: Amount) { ... }
```

`requirement`、`acceptance`、`forbidden`、`policy`、`goal`、`control` の各宣言は、
キーワード直後の ID を所有します。`@requirement(...)` は別の宣言をその ID へリンクし、
process の `covers ID "text"` は同じ型付き関係に対する正準なダイアレクト糖衣です。
宣言本体直前の旧形式 `"ID: original text"` は移行入力として引き続き受理されますが、
`fslc lint` は `legacy_string_metadata` を報告し、`fslc migrate --edition next` は
安全に変換できる場合に `@requirement(...)` へ変換します。

予約された `"undecided: reason"` タグは、レビュー済みで意図的に先送りされた決定を
マークします。これはメタデータであり、プロパティとして検証されることはありません。
`init`、`action`、`invariant`、`trans`、`reachable`、`leadsTo` に付けられます。例:

```fsl
init "undecided: initial operating mode is pending" { mode = Manual }
action route() "undecided: routing policy is pending" { ... }
```

`fslc ledger` と `fslc html` は、これらの宣言と、状態の依存がそれらと重なる要件 ID
を列挙します。`analyze --profile ai-review` は underspecification の finding を保持
しつつ、宣言との完全一致を `acknowledged:true` とマークします。レガシーの直接宣言
構文は依然として 1 つの文字列スロットを持ちますが、ネイティブの lowering はそれを、
要件、undecided マーカー、kind、名前空間付きのカスタムアノテーションを一緒に保持
できる型付きアノテーションキャリアへ適合させます — 下の正準な `@...` 構文が
埋めるのと同じキャリアです。
したがって requirement ブロックは、外側の要件を内側のレガシーマーカーで上書きする
のではなく、マージします。明示の `covers` と requirement ブロックのアノテーション
は、それ自身の正確なソーススパンを保持し、`undecided` は明示の要件 ID として受理
されるのではなく予約されています。複数の関係を公開する JSON の消費者は、既存の
単数フィールドを字句互換の射影として保持しつつ、`requirements` 配列を使います。
`DESIGN-undecided.md` と `DESIGN-annotations.md` を参照。これは権威あるネイティブ
Rust CLI が実装しており、凍結された Python 参照実装にはバックポートされていません。

ドキュメント自体が、そのダイアレクトキーワードの前に型付きアノテーションを運べ
ます。また(issue #241 以降)ドキュメント内の宣言は、その直前に型付き
アノテーションを運べます。同じ宣言をターゲットとするレガシーメタデータとの順序は
どちらでもかまいません:

```fsl
@requirement("REQ-CHECKOUT-001", "this document owns the checkout contract")
@acme.review(owner.platform, 2, true)
spec Checkout {
  state { paid: Bool }

  @requirement("REQ-CHECKOUT-003", "the ledger matches payments")
  @undecided("late gateway completion policy is pending")
  @kind("safety")
  invariant PaidLedger { paid == paid }
}
```

共有レキサーは、アノテーショングループ(トップレベルまたはネスト)の前の先頭
BOM、空白、`//` コメントをスキップします。積み重ねたアノテーションの間、または
最後のアノテーションとそのターゲットの間のコメントや空行は、付着を壊しません。
組み込みは `@requirement(id, text?)`、`@undecided(reason)`、
`@kind(id, text?)`、そして文字列/整数/真偽値/シンボルパスの引数を持つカスタムの
マルチセグメント名前空間(`@acme.review.owner(...)`)です。複数のアノテーションは、
検査結果を変えることなく、任意の順序で 1 つの宣言に積み重ねられます。トップ
レベルでは、アノテーション引数の中のキーワードはダイアレクトのディスパッチに関与
しません。空のドキュメントと未知のドキュメントは、正確な位置と決定的なサポート
済みキーワードのリストとともに `FSL-DIALECT-EMPTY` / `FSL-DIALECT-UNKNOWN` を報告
します(`DESIGN-dialect-dispatch.md` を参照)。

#### 13.1.1 正準 ID ポリシー

既存の分類体系を表現できるよう、文法自体は幅広い ID を受理します。`fslc lint` は
構文解析や検証とは独立して、次の種別別組み込みポリシーを適用します:

| 種別 | 組み込みテンプレート |
|---|---|
| requirement | `REQ-{scope}-{number:3}`, `NFR-{scope}-{number:3}`, `INV-{scope}-{number:3}` |
| acceptance | `AC-{scope}-{number:3}` |
| forbidden | `FB-{scope}-{number:3}` |
| policy | `POL-{scope}-{number:3}` |
| goal | `GOAL-{scope}-{number:3}` |
| control | `CTRL-{scope}-{number:3}` |
| model | `MODEL-{scope}-{number:3}` |
| assumption | `ASSUME-{scope}-{number:3}` |

`scope` は大文字 ASCII 英数字で、ハイフン区切りの複数セグメントを持てます。
`number:3` はちょうど 3 桁の十進数です。不一致は機械適用不可の
`non_canonical_id` finding になります。参照はソース、テスト、コード、telemetry、
外部証跡を横断しうるため、ID の rename は自動実行されません。

`--project fsl-project.toml` を渡すと、指定した種別だけを置き換え、省略した種別には
組み込み既定値を残せます:

```toml
[id_policy.patterns]
requirement = ["PAY-{number}", "NFR-{scope}-{number:3}"]
acceptance = "TEST-{number}"
```

値は文字列または空でない文字列配列です。テンプレートは `{scope}`、`{number}`、
正の幅を持つ `{number:N}` をサポートします。不正なポリシー設定は exit 2 で失敗します。
値には manifest reader の閉じた部分集合を使います。すなわち、double quote の
JSON-compatible な文字列/配列を trailing comma と inline comment なしで記述し、
single quote の TOML 文字列は拒否されます。model と assumption のテンプレートは、
互いにも requirement template にも重ならない別々の literal prefix で始める必要があります。lint は親ディレクトリを
探索しません。manifest は明示的に渡し、JSON 出力はその source と解決済み pattern
全体を記録します。`DESIGN-id-policy.md` を参照してください。

宣言レベルのアノテーションは、`init`、`action`(sync とマップされた/requirement
の action を含む)、`invariant`、`trans`、`reachable`、`until`、
`unless`、`leadsTo`、process の `transition`(`covers` と並んで)、
`requirement`/`acceptance`/`forbidden` ブロックに付着します(`requirement`
ブロック自身のアノテーションは、その `id`/`text` がすでにそうしているのと同じ
ように、それが含むすべての action/プロパティへファンアウトします)。同じブロック
内に後続の宣言のないアノテーション、またはアノテーションを受理しない宣言種別に
先行するアノテーションは、アノテーションのスパンで `FSL-ANNOTATION-TARGET` を報告
します。不正な引数、名前空間、構文は `FSL-ANNOTATION-ARGUMENTS` /
`FSL-ANNOTATION-PATH` / `FSL-ANNOTATION-SYNTAX` を報告します。新しい構文と
レガシーの文字列/`covers` 形式は、どちらも同じ型付きの関係へ脱糖され、1 つの宣言に
両方が存在する場合は単に union されます(同一の `(id, text)` ペアは重複排除され
ます。1 つの要件 ID に対する矛盾するテキストは、検査済みモデルのエラーです)。
issue #281 以降、同じ `@...` 文法は、このパーサーを共有しない 3 つの特化
フロントエンドのネストした宣言にも付着します: `domain`(aggregate の `command`、
`decide`、`evolve`、`invariant`、`projection`、`effect`、saga の `step`)、
`ai_component`(`tool`、`tools [a, b];` の短縮形、`authority` ブロックとその各
ルール行、`fallback` と各 `when` 項目、`check`)、そして `dbsystem`(`migration`、
および `check compatibility { ... }` の中の各 `rule` 行)。`command`/`decide` の
ペア(および decide の emit するイベントが到達する任意の `evolve` ブロック)は、
それらが一緒に生成する 1 つの action へ union されます。`effect` や saga の `step`
のアノテーションは、それが生成するすべての action へブロードキャストされます
(step 自身の action とその `_timeout` 変種。effect の成果ごとの `_complete_*`
action、その `_retry` action、その success-sticky な進行プロパティ)。これら
3 つのダイアレクトの他のあらゆる場所 — migration の操作行、`dbsystem`/
`ai_component` のスカラー宣言、受理リストの外にある domain の宣言 — では、はぐれた
アノテーションは引き続きコード付きの `FSL-ANNOTATION-TARGET` 診断を報告します。
完全な付着テーブル、`AiAuthority` ルールの再構造化、そして `dbsystem` の
migration/互換性ルールのアノテーションが、損失のあるレガシーの `quote_meta` 文字列
規約を通らずに検査済みモデルへ到達する方法は、`DESIGN-annotations.md` の
"domain/db/ai nested declaration syntax" 節を参照。ネストした宣言の上の `@...` は
ネイティブ専用の表層追加のままです — 凍結された Python 参照はこれをパースしない
ので、これを使う spec は構成上 Python パリティコーパスの外にあります。

#### 13.1.2 ツール・AI 消費のための根拠(rationale)

`//` コメントは字句解析上の trivia(`rust/fsl-syntax/src/lexer.rs`)であり、
AST にも `KernelModel` にも `python_ast()` にも JSON の result envelope にも
LSP のインデックスにも監査台帳にも到達しません。補助不変条件が k-induction の
CTI を閉じるために存在する、あるいは実装のガードが抽象側より意図的に強い、と
いった、下流のツールや AI エージェントが失ってはならない事実は、散文だけに
留めるのではなく、既存のアノテーションキャリアに載せるべきです:

- 組み込みの `@kind(id, text?)` を使って、宣言を 1 行で分類しつつ説明します。
  例: `@kind("aux_invariant", "closes the k-induction CTI for
  attempts_bounded")`。`Kind` は guard・action・プロパティの種類・検証・
  lowering を一切変更しません。JSON envelope や LSP を含む、
  `KernelModel::annotations_for` を読むあらゆる消費者がこれを見られます。
- 分類にうまく当てはまらない短い根拠には、推奨されるカスタム名前空間
  `@doc.rationale("...")` を使います(通常の `Custom` アノテーション。13.1 を
  参照)。他のカスタム名前空間と同じ、検証に無害でクエリ可能という保証を持ち、
  文法変更は不要です。
- 複数文にわたる説明 — spec が何を示すか、設計がなぜうまくいくかの解説、
  意図的に壊れた例における教育目的のバグマーカー — は、通常の `//` コメントの
  ままにします。アノテーション引数の文字列にはエスケープ構文がなく、最初の
  `"` または改行で止まる(`lex_string`)ため、散文を保持できません。物語的な
  文章をアノテーション引数に押し込むと、spec はかえって読みにくくなります。

### 13.2 要件レイヤー: `requirements`(fsl-req ダイアレクト)

```fsl
requirements ExpenseRequirements {
  implements ExpenseToBe from "1_business.fsl" { }

  number Amount
  const AUTO_LIMIT = 1

  process Claim with amount: Amount {
    stages Draft, Submitted, Approved, Rejected, Paid
    initial Draft
    transition submit       Draft     -> Submitted by Employee with a: Amount when a > 0 set amount = a covers REQ-EXPENSE-001 "The applicant submits an expense claim by entering an amount"
    transition auto_approve Submitted -> Approved  by System  when amount <= AUTO_LIMIT covers REQ-EXPENSE-002 "Claims at or below AUTO_LIMIT are auto-approved by the system"
    transition mgr_approve  Submitted -> Approved  by Manager when amount >  AUTO_LIMIT covers REQ-EXPENSE-003 "Claims above AUTO_LIMIT are approved by a manager"
    transition reject       Submitted -> Rejected  by Manager when amount >  AUTO_LIMIT covers REQ-EXPENSE-003 "Claims above AUTO_LIMIT may be rejected by a manager"
    transition pay          Approved  -> Paid      by Finance covers REQ-EXPENSE-004 "Only approved claims are paid"
  }

  kpi paid_claims = count Claim in Paid

  acceptance AC-EXPENSE-001 "Approval flow: a low-amount claim is auto-approved and paid" {
    submit(0, 1) auto_approve(0) pay(0)
    expect Claim 0 in Paid
  }
  acceptance AC-EXPENSE-002 "Rejection flow: a high-amount claim ends in manager rejection" {
    submit(1, 2) reject(1)
    expect Claim 1 in Rejected
  }
}
verify {
  instances Claim = 3
  values Amount = 0..3
}
```

- process+data プロファイルは、単一エンティティのライフサイクルに対する第一の
  requirements 形式です。`process E with f: T { ... }` は、エンティティの stage
  マップと carried フィールドを作ります。transition は、入力(`with a: T`)、
  ガード(`when`)、フィールドの更新(`set f = expr`)、トレーサビリティ
  (`covers REQ-n "text"`)を追加できます。carried フィールドの型 `T` は
  `number`、`Bool`、または同じ requirements spec で宣言された enum です:
  - `number` フィールドは、そのドメインの `lo` 境界がデフォルトです。
    `f: T = <expr>` は省略可能な明示の初期化子(コンパイル時の定数式)です。省略は
    現行エディションでは、安定した `implicit_initial_value` 警告と、選択された下限
    の挿入編集つきで保持されます。
  - `Bool` と enum のフィールドには、発明されたデフォルトはありません —
    `f: Bool = true/false` と `f: T = Member` は**必須**です。初期化子の省略は
    check 時のエラーです(黙って選ばれる `false` や先頭の enum メンバーは
    ありません)。
- `number Amount` は値の種を宣言します。有限の検証器の範囲は
  `verify { values Amount = lo..hi }` に住みます。エンティティのサイズは
  `verify { instances Entity = N }` に住みます。
- `kpi NAME = count ENTITY in STAGE` は、business と requirements の両方における
  宣言的な射影です。ghost のカウンターや自動の `_kpi_*` invariant を作ることは
  ありません。
- `implements` があると、`fslc verify` は**上位レイヤーへの refine も同時に実行**
  し、結果は `implements: {abs, result}` を運びます。空のボディ
  (`implements X from "..." { }`)は、process/action/stage の名前が一致するとき、
  恒等の refinement を自動生成します。`implements { }` ブロックの内側には、状態の
  `map` エントリ、`maps auto`、`preserve progress`、そして — #73 以降 —
  `action <impl_act>(<params>) -> <abs_act>(<args>) | stutter` を書きます。これは
  別の refinement ファイルの `refinement_action`
  (`docs/DESIGN-refinement.md` §1)と同じ対応の構文で、impl と abs の action の
  間のアリティ変更を含みます。action↔action の対応は、requirement レベルの action
  の上の `maps <abs_act>(...)` 節としても引き続き書けます。`maps auto` は
  同名のカーネルラッパーの state/action をカバーし、明示の map はそれを上書き
  します。`maps` 節と、それに一致するインラインの `action ...` 項目の両方を持つ
  impl の action は、対応の重複であり、両方の起源と位置を名指しする
  `kind: "type"` の check 時エラーです(同じ action を 2 回列挙するマッピング
  ファイルと同じです)。auto マップされた process の
  transition は actor 検査されます。actor が business の action の actor と異なる
  transition は check 時エラーです。
- `acceptance` は、check 時に具象の Monitor によって replay 検証されます(失敗は
  `kind: "acceptance"`)。`expect <expr>` と並んで `expect <Entity> <id> in
  <Stage>` をサポートし、scenarios / testgen へ直接流れ込みます(= 受け入れ基準が
  実装のコンフォーマンステストになります)。
  `acceptance`/`forbidden` のステップの action 引数は、数値リテラルに加えて enum
  メンバー名と const 名を受け付けます(例: `answer(0, Triggered)` は
  `Triggered` が `Trigger` の第 2 メンバーであるとき `answer(0, 1)` と等価です)
  — 未定義の名前は check 時エラーです。
- カーネルラッパー形式は、難しいケースにだけ使ってください: 複数エンティティの
  要件、保存則、SLA/時間、carried フィールドとして表現できない履歴、明示の
  カーネル状態を必要とする振る舞い。そのフォールバックは引き続き
  `struct` / `state` / `init`、`fair action`、`branches`、明示の `maps` をサポート
  します。`branches` は action を各 when 条件で自動的に分割し
  (`submit[a <= AUTO_LIMIT]` と表示されます)、`maps` 節が上位レイヤーへの action
  の対応を提供します。
- `terminal { <expr> }` は `requirements` spec のトップレベルで許され、カーネルへ
  lowering されます(§6) — カーネルと同じく、spec ごとに `terminal` ブロックは
  ちょうど 1 つです。`stage(c)` は、`c` が型付きのエンティティパラメータまたは
  バインダーであるとき、requirements のすべての式文脈で利用できます。エンティティ
  型がその process と stage の enum を選択し、検査済みの式は生成された stage
  マップのインデックスへ lowering されます。requirements では、シンクステージが
  自動的に terminal になることはありません。
- 1 つのエンティティが複数の process に参加するとき、process は修飾パスを使えます:
  `process claims.Claim { ... }`。複数の process が `Claim` に対応するとき、
  非修飾の `stage(c)` はエラーです。
  `claims.Claim.stage(c)`(任意深さの `SymbolPath`)で 1 つを選択してください。
  存在しない process、非エンティティの引数、未知の stage メンバー、未解決の修飾子
  は、ソース位置付きの型エラーです。

### 13.3 業務コンサルティングレイヤー: `business`(fsl-biz ダイアレクト)

業務プロセス、ポリシー、KPI を、実装の語彙ゼロで書きます(構文の詳細は
`DESIGN-dialects.md` §3)。process は enum+Map+transition の action へ、policy は
invariant / leadsTo へ、kpi はメタデータとして記録される宣言的な count の射影へ
展開されます。規程の矛盾 = invariant 違反、死んだプロセスステップ = カバレッジ
診断、到達不能な業務ゴール = reachable_failed、放置された案件 = leadsTo の反例 —
すべて機械的に検出できます。

PM/コンサルタント向けのファイルでは、よくある応答ポリシーとゴールに、読みやすい
stage 構文を使ってください:

```fsl
business ReturnHandling {
  actor Customer, Manager
  entity Return
  process Return {
    stages Requested, Approved, Rejected, Refunded
    initial Requested
    transition approve Requested -> Approved by Manager
    transition reject Requested -> Rejected by Manager
    transition refund Approved -> Refunded by Manager
  }

  kpi refunded = count Return in Refunded

  control CTRL-DECISION
    "Every return must preserve an adjudication control"
    owner Manager
    severity high
    applies_to Return

  policy PAY-2 "every request is eventually decided"
    satisfies CTRL-DECISION
    every Return in Requested must eventually be Approved or Rejected or Refunded
  goal AllSettled "all cases can be completed"
    all Return can be Refunded or Rejected
}

verify {
  instances Return = 3
}
```

規則が単なる stage の進行でないときは、明示の形式が引き続き使えます:
`policy ... responds { forall c: Return { stage(c) == Requested ~> ... } }` と
`goal ... { exists c: Return { stage(c) == Refunded } }`。

**バイパス禁止(no-bypass)**のコントロール — 必須の経由地点を先に通らずには決して
到達してはならないターゲット stage — には、precedence 形式を使います
(#75。設計の根拠は `DESIGN-precedence-policy.md`):

```fsl
policy CTRL-APPROVAL "承認を経ずに完了しない"
  every Return reaching Refunded must have passed through Approved
```

これは、不可視の `Map<Return, Bool>` 履歴フラグ(トレースで読めるように
`return_stage_via_Approved` と命名されます)を合成し、`Approved` に着地する任意の
transition でそれを `true` に設定し、policy を
`forall c: Return { stage(c) == Refunded => return_stage_via_Approved[c] }` へ
コンパイルします。`Approved` をスキップする `Requested -> Refunded` の transition
は、そのとき本物の invariant 違反となり、トレースはどのバイパス transition が発火
したかを正確に示します。両側は選言を受け付けます —
`every Return reaching Refunded or Closed must have passed through Approved
or Rejected` — そして、同じ `(process, waypoint-set)` の上の 2 つの policy は、
1 つの合成された履歴フラグを共有します。

履歴フラグと並んで、第二の invariant `<PolicyId>_stability` が、process の stage
グラフから自動合成されます(#85。設計の根拠は
`DESIGN-precedence-policy.md`)。これにより、**準拠している** precedence policy は
`--engine induction` の下で、手動の invariant なしに最初から証明されます —
履歴フラグが任意の induction ステップで「真だが、まだ証明可能に真ではない」ことに
よるゴーストの counterexample-to-induction は生じません。

```json
{"result": "proved", "k_used": {"CTRL-APPROVAL": 1, "CTRL-APPROVAL_stability": 1}}
```

business には独自の `terminal` 構文はありません。代わりに、各 process の
**シンクステージ**(出て行く `transition` のないステージ)が自動的に収集されます:
すべての process が少なくとも 1 つのシンクを持つなら、process にわたる
`forall c: <Entity> { stage(c) in {Sink1, Sink2, ...} }` の連言としてカーネルの
`terminal { }` が生成されます — つまり、すべての process のすべてのエンティティが
同時に自身のシンクの 1 つに停まっているときにだけ、デッドロックは「意図された」
ものになります。したがって上の `ReturnHandling` は、`--deadlock ignore` なしで
`Rejected`/`Refunded` においてクリーンに検証されます。いずれかの process が循環的
(すべての stage が出て行く transition を持ち、シンクがない)なら、terminal は
まったく生成されず、デッドロック検査は影響を受けません — 循環プロセスはそもそも
デッドロックしません。常にいずれかの transition が enabled だからです。

ガバナンス/コントロールのメタデータは、`business` の内側に保持することも、
スタンドアロンのカタログへ持ち上げることもできます。
`control ID "text" owner NAME severity NAME applies_to Entity`
は、それ自体ではプロパティを生成しません。カタログのエントリです。`policy` や
`goal` は `satisfies CTRL-ID` を宣言でき、そのとき違反は、policy/goal の要件と
満たされたコントロールの両方を JSON で運びます:

```fsl
policy PAY-2 "every request is eventually decided"
  satisfies CTRL-DECISION
  every Return in Requested must eventually be Approved or Rejected or Refunded
```

business の spec をまたいで再利用されるコントロールには、`governance` カタログを
使います:

```fsl
governance EnterpriseReturnControls {
  authority Operations owns CTRL-DECISION
  control CTRL-DECISION "Every return must preserve an adjudication control"

  delegates ReturnHandling from "return_policy.fsl" {
    require CTRL-DECISION
    // optional if the business policy already says `satisfies CTRL-DECISION`
    CTRL-DECISION is satisfied_by policy PAY-2
  }

  preservation ReturnReform {
    before AsIsReturn from "asis_return.fsl"
    after  ToBeReturn from "tobe_return.fsl"
    preserve CTRL-DECISION
    checked_by refinement "tobe_refines_asis.fsl"
  }
}
```

`fslc check governance.fsl` は、参照されるすべてのコントロール、business ファイル、
policy/goal、preservation ファイルを検証します。preservation ブロックはさらに、
宣言された refinement を深さ 8 で実行し、結果を `governance.preservations` の下で
報告します。

### 13.4 非機能要件(NFR)の書き方

NFR の大半は、機能要件と同じ機構で書けます(詳細とデモは `DESIGN-nfr.md`):

| NFR | 書き方 |
|---|---|
| 権限(管理者だけが X する) | `requires role[u] == Admin` + ghost の `done_by_admin` の上の invariant |
| 監査の完全性 | 横断的な invariant(例: `audit.balance == cleared + pending + withdrawn`) |
| 容量 / 上限 | 有界型 / Seq の容量 / `count(...) <= N` の invariant |
| 信頼性の振る舞い | 障害注入の action(`crash`)+ モード状態 + `fair recover` + 回復の leadsTo |
| **SLA / タイムアウト** | `time` ブロック + `deadline`(下記) |
| 確率、パーセンタイル、ms の実時間 | **範囲外**(散文で書く) |

SLA は、離散時間における安全性として検査されます(`requirements` の内側):

```fsl
time {
  urgent start, finish                    // while enabled, time (tick) does not advance
  age waitAge[r: Req] while pending[r]    // +1 per tick, 0 if the condition is false
}
requirement NFR-1 "complete within 4 ticks of acceptance" {
  deadline waitAge <= 4
}
```

- tick は自動生成され、緊急性の規律(「システムは、暇なときに仕事を先送りしない」)
  がそのガードです。urgent を指定しなければ、ほとんどの deadline は飢餓トレースで
  violated になります — これは「スケジューリングの仮定が存在しない」ことの正しい
  表示です。
- 配置: requirements の直下に `time` は高々 1 つ、`deadline` は requirement の
  内側(要件 ID が違反に結び付きます)。age は tick ごとに +1(while が偽なら 0 に
  リセット)で、通常の状態変数としてガードから読めます。
- `tick` は生成されます — 自前の `action tick` を宣言しないでください(check
  エラーになります)。age のカウンターだけを進め、refinement の下では自動的に
  `stutter` へマップされます。`tick()` として参照します(例: `acceptance`
  シナリオの中)。tick 側の仕事(サービス時間など)には、カーネルラッパー形式が
  必要です。
- **レイヤーをまたぐと、`deadline` は共有クロックの上でのみ refine します。**
  `deadline` はそれを宣言するクロックの安全性プロパティなので、設計が*時間付き*の
  requirements spec を refine するのは、その `tick` が生成されたものをミラーする
  ときだけです。より細かいクロック(サービス時間も消費する `tick`)を持つ設計は、
  それらのステップの抽象像を持たず、`abs_requires_failed` で `fslc refine` に失敗
  します — liveness と同じ非伝播です。時間付きのプロパティはクロックを所有する
  レイヤーで検証するか、上位のコントラクトを時間なしに保ってクロックを設計の
  カーネルに置いてください(`tick → stutter`)。`docs/DESIGN-nfr.md` §6 と
  `examples/nfr/sla_worker_design.fsl` を参照。
- **⚠ vacuous な SLA の罠**: 常に enabled になりうる action を urgent にすると
  時間が凍結し、任意の K が deadline を空虚に満たします(`<= 0` でも green です)。
  正しい形式は、**締め切りの到来時に enabled になるガード付きの action だけを
  urgent にすること(`requires age >= K` を持つ respond_due 形式)**です。
  非空虚性を確認するには、`K-1` へ下げて violated になることを確認します。
  `fslc verify --vacuity` は、この罠の証明可能な形式(urgent の条件が初期かつ
  帰納的)に対して `kind:"urgency_freeze"` を出します。警告の不在は非空虚性の証明
  ではありません。
- BMC の検査はすぐに機能します。帰納的な証明はしばしば、時間予算の補助 invariant
  (`age + remaining work <= K` の形)を必要とします(CTI から導出します。実例は
  `examples/nfr/`)。

### 13.5 データベース互換レイヤー: `dbsystem`(fsl-db)

`dbsystem` は、データベース、アプリケーションのアーティファクト、API/オフラインの
ペイロード、環境にまたがるマイグレーションの互換性をモデル化します。これは
ダイアレクトの展開であって、DB エンジンのモデルではありません: オプティマイザの
挙動、ロックのタイミング、実時間の TTL、確率、本番データの完全性は、形式モデルの
外にあります。

中核の形:

```fsl
dbsystem <Name> {
  database <db> {
    schema <initial_version>
    table <table> {
      column <column>: <db_type> present backfilled not_null;
      column <future_column>: <db_type> absent;
    }
  }

  migration <name> from <v0> to <v1> [rollbackable] {
    add <table>.<column> nullable;
    backfill <table>.<column>;
    set_not_null <table>.<column>;
    rename <table>.<old> to <table>.<new>;
    split <table>.<source> into <table>.<a>, <table>.<b> lossless|lossy|irreversible;
    merge <table>.<a>, <table>.<b> into <table>.<target> lossless|lossy|irreversible;
    drop <table>.<column> destructive|irreversible;
  }

  artifact <version> {
    reads <table>.<column>, ...;
    writes <table>.<column>, ...;
    requires <capability_namespace>.<capability>, ...;
    provides <capability_namespace>.<capability>, ...;
    calls api.<operation>, ...;
    accepts api.<operation>, ...;
    expects response.<field>, ...;
    responds response.<field>, ...;
    emits_offline api.<operation> ttl <finite_ticks>;
  }

  environment <env> {
    schema <lo>..<hi>;
    flag <flag_name> { <variant>, ... } default <variant>;
    active <version> when schema <lo>..<hi> when flag <flag_name>=<variant>;
    supported <version> when schema <lo>..<hi>;
    may_exist <version> when schema <lo>..<hi>;
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
    rule artifact_capabilities_provided;
    rule data_preserved;
    rule rollback_equivalent;
  }
}
```

`check compatibility` を省略すると、デフォルトのルールが read/write の
ライフサイクル、destructive のアノテーション、preservation-transform の
アノテーション、API/オフラインの互換性をカバーします。`data_preserved` と
`rollback_equivalent` はオプトインの有界検査で、`DB-ASSUME-BOUNDED-ROW-MODEL` を
報告します。

フィーチャーフラグは有限の環境次元です。`fslc db check` は、宣言されたバリアント
を schema のスナップショットとともに列挙し、
`DB-ASSUME-FINITE-FLAG-STATE` を報告します。これはロールアウト/キルスイッチの
互換性検査であって、パーセンテージや確率の証明ではありません。`flag` /
`when flag` を省略すると、既存のアーティファクト/ウィンドウのみのモデルが保たれ
ます。

現在の形式的な違反種別は、以下を含みます:

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
- `required_capability_missing`

fsl-db の語彙が欲しいときは、`fslc db check` を使います:

```bash
fslc db check examples/db/safe_add_nullable_column.fsl
fslc db check examples/db/safe_dual_write_backfill_switch_read_drop_old.fsl --engine induction
fslc db observe examples/db/runtime_observation_target.fsl --trace examples/db/runtime_observation_mismatch.json
fslc db import examples/db/minimal_import.sql --name ImportedFromSql -o /tmp/imported.fsl
fslc db import examples/db/minimal_prisma_schema.prisma --name ImportedFromPrisma
```

成功した検査は、有限ロールアウトとケイパビリティ完全性の仮定を伴う
`verified_under_assumptions` を返します。互換性の失敗は
`finding_schema_version: "fsl-db-finding.v0"` と、環境、アーティファクト、
migration/schema の要素、最小の競合集合、修復候補を含む `findings[]` を返します。
ランタイムの観測は `formal_result: "not_run"` の `observed_mismatch` を返します。
ログからの不在は、未使用の振る舞いの証明ではありません。
生成されたカーネルの反例を直接調べたいときは、通常の `fslc verify` を使って
ください。

汎用の `requires` / `provides` ケイパビリティにより、AI モデル/プロンプト/
リトリーバー/ツールスキーマと出力スキーマのプロファイルが、DB/API/モバイル/
サーバーのアーティファクトと同じ互換性検査を共有します。
`requires tool.RefundPaymentV2` の宣言が安全なのは、同じ環境スナップショットの中の
active または supported なアーティファクトが `provides tool.RefundPaymentV2` する
ときだけです。これらのプロファイルは有限の共存の事実であって、評価器や統計的な
品質の主張ではありません。

インポーターの境界: SQL DDL のインポートは `sql-ddl-minimal.v0` です。最初の
ソース特化の ORM インポーターは `prisma-schema-minimal.v0` で、Prisma の
`model` のスカラーフィールドをインポートし、relation/list/model の属性を
`unsupported_prisma` 警告として報告します。本番データの保存エビデンスと
DB エンジンのエビデンスは、`schemas/fslc/db/` の下の別々の JSON スキーマに住み、
`formal_result: "not_run"` を使います。サンプリング/監査されたエビデンスを
`verified` や `proved` へ昇格させることは決してありません。

### 13.6 AI ハードコントラクトレイヤー: `ai_component`(fsl-ai)

`ai_component` は、AI コンポーネントの決定的でガードに裏付けられたスライスをモデル
化します: ツールの宣言、シンボリックなツールスキーマ、業務の事前条件のエビデンス、
権限、人間の承認、禁止ツール、フォールバックのルーティング。これはダイアレクトの
展開であって、確率的なカーネルではありません。確率、評価器のスコアリング、
groundedness の判断、プロンプトインジェクションの意味論的な判断、信頼区間は、この
形式カーネルモデルの外に留まり、外部のエビデンスとして扱われます。

中核の形:

```fsl
ai_component RefundAgentToolSafety {
  model refund_model_v1;
  prompt refund_prompt_v1;
  input RefundRequestV1;
  output RefundDecisionV1;

  tool SearchOrder {
    schema SearchOrderV1;
    precondition order_exists;
  }

  tool RefundPayment irreversible {
    schema RefundPaymentV1;
    precondition order_paid;
    precondition amount_refundable;
  }

  tool DeleteCustomerData irreversible {
    schema DeleteCustomerDataV1;
  }

  authority {
    may_execute SearchOrder;
    requires_human_approval RefundPayment;
    forbidden DeleteCustomerData;
  }

  fallback {
    when low_confidence require human_review;
  }
}
```

`model`/`prompt`/`input`/`output`/`tool`/`authority`/`fallback` が、ほとんどの
spec が必要とするフィールドです。さらに 3 つが省略可能で、それぞれ高々 1 回です:
`retriever <id>;`、`temperature <number>;`、そして schema/precondition/effect の
ない裸のツールを宣言する `tools [Name, ...]` の短縮形。`tool` ブロックの
`precondition <name>;` 行は繰り返し可能(0 回以上)で、1 つの `effect <name>;` も
宣言できます。これらのフィールドのいずれも — `authority`、`fallback`、下の
`check hard { }` ブロックも — `"description text"` タグを受け付けません。ここの
すべてのフィールドは、§13.1 の宣言タグの規約とは違い、裸の識別子または数値です。

`ai_component` はまた、どのハードルールが明示的で個別に報告される invariant を
得るかを宣言できます:

```fsl
  check hard {
    rule tool_authority;
    rule human_approval_required;
    rule forbidden_tool_blocked;
    rule tool_schema_declared;
    rule tool_precondition_declared;
  }
```

`check hard { }` を省略すると、5 つのルールすべてが検査されます(デフォルト)。
未知のルールを名指しするのは check 時エラーです。集合を狭めることは、
`forbidden_tool_blocked` / `human_approval_required` について、明示的で個別に報告
される invariant を取り除くだけです — 構造的なガード自体(forbidden なツールに
execute の action が生成されることは決してない。承認必須のツールの execute action
は常に `requires human_approved` ガードを運ぶ)は、どちらにせよ無条件に生成され
ます。`tool_authority`、
`tool_schema_declared`、`tool_precondition_declared` は、このブロックに関係なく
無条件に検査されます。

`ai_component` は、有限のツール状態を持つカーネル spec へ lowering されます:

- `Tool` enum
- `human_approved: Map<Tool, Bool>`
- `tool_executed: Map<Tool, Bool>`
- 生成された `approve_*` / `execute_*` の action
- 実行前の承認と禁止ツールのための、生成された invariant

fsl-ai の語彙が欲しいときは、`fslc ai check` を使います:

```bash
fslc ai check examples/ai/refund_agent_tool_safety.fsl
fslc ai check examples/ai/recursive_support_agent.fsl
fslc ai replay examples/ai/refund_agent_tool_safety.fsl --logs examples/ai/runtime_human_approval_bypass.jsonl
fslc ai eval examples/ai/support_answer_quality.fsl --property LooseQuality
fslc ai regress examples/ai/support_answer_quality.fsl --migration PromptV7ToV8 --before-records examples/ai/support_eval_v7.jsonl --after-records examples/ai/support_eval_v8_regressed.jsonl
fslc ai drift examples/ai/support_answer_quality.fsl --logs examples/ai/runtime_drift_current.jsonl --baseline-logs examples/ai/runtime_drift_baseline.jsonl
fslc ai compat examples/ai/support_answer_quality.fsl --environment prod
```

成功した `ai_component` のハードコントラクト検査は
`verified_under_assumptions` を返します。再帰的な `agent` の検査は
`agent_analyzed` を `formal_result: "not_run"` とともに返します。それらはカーネル
の証明ではなく、構造的なグラフ分析だからです。`fslc ai replay` は JSONL または
`{ "events": [...] }` を受理し、
`replay_conformant` / `replay_nonconformant` を
`formal_result: "not_run"` とともに返します。replay は観測のエビデンスだから
です。findings は `finding_schema_version: "fsl-ai-finding.v0"` を使い、
`guarantee_kind` を含みます:

- `syntactic_hard`: schema/authority/approval/forbidden/precondition のガードの事実。
- `agent_structural`: 再帰的エージェントのスコープ、grant、可視性、委譲、ツール
  到達可能性の findings。
- `runtime_observed`: 宣言されたコンポーネントのケイパビリティが観測イベントと
  異なる。
- `statistically_supported` / `statistically_unsupported`: `fslc ai eval` からの
  事前計算済み eval JSONL と Wilson 信頼限界のエビデンス。決して `proved` として
  表示されない。
- `evaluator_supported`: 外部の評価器に裏付けられたエビデンスのために予約。
  `proved` として表示してはならない。

プロジェクトレベルの fsl-ai エビデンス宣言は、`ai_component`、
`dataset`、`evaluator`、`failure_mode`、`statistical_property`、
`ai_migration`、`observed_property` を組み合わせられます。`fslc ai check` は
これらのファイルをパースして `ai_project_analyzed` を返します。`fslc ai eval` は
Wilson 区間つきで、事前計算された JSONL から Bernoulli/比率のメトリクスを検査
します。`fslc ai regress` は集約の `no_regression` のメトリクス低下/増加の節を
検査します。`fslc ai compare` は閾値の主張なしにメトリクスの差分を報告します。
`fslc ai drift` はランタイムのテレメトリの閾値とドリフトを検査します。そして
`fslc ai compat` は有限の
`dbsystem artifact` ケイパビリティプロファイルを出力します。これらはすべて
`formal_result:"not_run"` を使います。

再帰的な `agent` の形:

```fsl
agent SupportOrchestrator {
  context [CustomerTicket, ApprovedSupportDocs];
  tools [SearchDocs, CheckPolicy, CreateDraft];
  authority {
    may_execute [SearchDocs, CheckPolicy, CreateDraft];
  }
  review_gate PolicyCheckAgent;

  agent RetrievalAgent {
    trust medium;
    grant authority [SearchDocs];
    grant context [ApprovedSupportDocs];
    tools [SearchDocs];
    authority { may_execute [SearchDocs]; }
    output RetrievedSources visibility [parent, PolicyCheckAgent];
  }

  agent PolicyCheckAgent {
    trust high;
    grant authority [CheckPolicy];
    grant context [CustomerTicket, ApprovedSupportDocs];
    tools [CheckPolicy];
    authority { may_execute [CheckPolicy]; }
    contract { hard { rule PolicyMustCiteSource; } }
    output PolicyDecision visibility parent;
  }

  orchestration {
    RetrievalAgent -> PolicyCheckAgent;
  }

  failure_policy {
    when RetrievalAgent.failed -> retry up_to 2;
    when RetrievalAgent.failed_after_retry -> HumanReviewPending;
  }
}
```

ネストされた agent は、親によってスコープされる通常の agent であり、別個の
`sub_agent` 型ではありません。ネストは
`SupportOrchestrator.RetrievalAgent` のような字句名を作ります。ランタイムの協調は
`orchestration` で別途宣言されます。親の authority/context が暗黙に継承されること
はありません。子は明示の `grant authority` と `grant context` を受け取らなければ
ならず、各 grant は直接の親の境界の内側に留まらなければなりません。`model`/
`prompt` は任意の agent レベル(ルートまたは子)でも有効で、直接の
`tool { }` ブロックは、`ai_component` の内側と同じように agent の内側で機能します。
`review_gate <Child>;` は、高権限ツールを持つ子孫へのすべての orchestration パスが
通過しなければならない**直接の子** agent を名指しします。
宣言されたすべての review gate をスキップするパスは
`policy_review_bypass_in_orchestration` としてフラグされます。`trust` は自由な
識別子であり、検証された enum ではありません — 現在、専用の検査を駆動するのは
リテラルの `low` だけです(低信頼の agent から高権限ツールへのパス)。他の値は
パースされますが、まだ個別の検査を持ちません。`contract { hard { rule <Name>; } }`
はパースされ、agent ごとに列挙されますが — `ai_component` の `check hard { }` とは
違い — そのルール名は既知の集合に対して検証されず、まだ何とも突合されません。
前方宣言されたメタデータとして扱ってください。

設計の詳細: `docs/DESIGN-ai-hard.md`。

### 13.7 扱わないもの(レイヤーの境界)

非機能要件の大半(権限、監査、容量、信頼性の振る舞い、離散時間の SLA)は扱えます
(§13.4)。FSL の外に残るのは: **確率、パーセンタイル(99.9% など)、実時間
(実時計の ms)、ユーザビリティ、DB のオプティマイザ/ロックのタイミング、完全な
本番データの証明、評価器の真偽判断、そして散文の根拠**(これらは各レイヤーの
ドキュメントに書いてください)。FSL が責任を持つのは、各アーティファクトの
**検査可能な骨格**です。

## 14. ライブラリ API

```python
from fslc import parse, build_spec, verify, prove, Monitor

spec   = build_spec(parse(src))
result = verify(spec, depth=8)            # BMC
result = prove(spec, k_ind=1, base_depth=8)   # k-induction
```

CLI と同じ構造の dict を返します(CLI はそれを `"fsl": "1.0"` エンベロープで包み
ます)。ネイティブ CLI とブラウザ Worker の `check`/`verify` エンベロープは、
`versions.verifier`、`versions.core`、`versions.solver` オブジェクトも含みます。
各オブジェクトは `name` と `version` を持ち、solver オブジェクトはさらに
`backend` を持ちます。ソルバーのバージョンは、CLI の定数ではなく、ロードされた
ネイティブまたはブラウザの Z3 ランタイムから来ます。機械可読なコントラクトは
`schemas/fslc/envelope.v1.schema.json` です。

## 15. 妥当性確認スイート(spec ≠ 意図のギャップ)

`fslc` が保証するのは「書かれた spec の内部整合性」であって、「spec が元の意図に
忠実かどうか」ではありません。AI に spec を書かせると、エラーはこの妥当性確認の
レイヤーに集中します。以下は、そうしたエラーを**機械的な不一致として表面化させる**
検査の集合です(設計の全体像はロードマップ issue #1。各機能には対応する
DESIGN-*.md があります)。

- **`forbidden`(否定の受け入れ基準)** — requirements ダイアレクトの構成物です。
  「拒否されるべき操作列」を書くと、check 時に、最後のステップが拒否される
  (not-enabled または違反)ことが replay 検証されます。受理されてしまった場合は
  `kind:"forbidden"` です(制約不足 = ガードの欠落の検出。安全性の invariant は
  これについて沈黙します)。`acceptance`(must-allow)の双対です。
  → [`DESIGN-forbidden.md`](DESIGN-forbidden.md)
- **Vacuity 検査(`--vacuity`)** — verified/proved のパスの上で、
  `vacuous_implication`(含意の前件が到達不能)、
  `vacuous_leadsto`(トリガーが到達不能)、`always_true_requires`
  (先行する節の文脈の下で常に真であるガード)、
  `tautology_over_frozen`(どの action も変えない状態の上の、動的に
  トートロジーである invariant)、`urgency_freeze`(urgency が時間を凍結するために
  死んでいると証明された、生成された deadline の `tick`)を警告します。
  `error` は exit 2 です。→
  [`DESIGN-vacuity.md`](DESIGN-vacuity.md)
- **`--strict-tags`** — 成功の結果の上で、タグのない宣言(捏造の候補)と、参照
  されない要件(欠落の候補。空の requirement ブロックを含む)を警告します。存在
  レベルの突合です。→ [`DESIGN-strict-tags.md`](DESIGN-strict-tags.md)
- **`fslc mutate`** — spec を機械的に変異させ、各ミュータントが既存の検査の網に
  よって殺されるかを測定します。生き残ったミュータント = どのプロパティにも制約
  されない振る舞い = invariant が欠けている場所です。
  `--from mutants.jsonl` はさらに、完全な `mutated_spec`、または正確な
  `replace:{target,replacement,occurrence?}` 命令として表現された、外部で生成
  された変異を裁定します。valid な外部ミュータントは、同じ
  verify/acceptance/forbidden/refinement のオラクルを使います。JSON、命令、
  パース、名前、型、構築のエラーは `invalid` で、決して killed にはならず、
  combined/per-source のキル率の分母から除外されます。
  すべてのエントリは `source:"builtin"|"external"` を運びます。`--max-mutants` は
  組み込みのカタログだけを上限にするので、`--max-mutants 0 --from ...` は外部のみ
  を実行します。
  `--by-requirement` は「どの振る舞いミュータントも殺さない要件」を
  `empty_formalization` 警告としてフラグします(`--strict-tags` の意味レベルの
  拡張)。→ [`DESIGN-mutate.md`](DESIGN-mutate.md)
- **`fslc explain --readable`** — 骨格の列挙(状態、action の誰が/いつ/何を変える
  か、検証の境界、公平性、KPI の射影、branch の lowering、合成された refinement
  マッピング、自動検査、タグ)+ counterfactual(「この規則がなければ、この手順で
  それを壊せる」)+ witness のナレーションの上のテキストビュー。人間のレビューを、
  論理式を読むことから、具体例を裁定することへ移します。`--readable` なしでは
  JSON モードが引き続き利用できます。→
  [`DESIGN-explain.md`](DESIGN-explain.md)
- **`fslc analyze`** — 構造的な観測 JSON を出力します。`--projection tsg` は、
  要件、action、状態、プロパティ、シナリオの Typed Semantic Graph を返します。
  グラフの射影は、連結成分、SCC、代表的なサイクル、次数、そしてサイクルランクや
  ファンイン/ファンアウトのハブのような構造メトリクスを返します。
  `--projection action_dependency_graph` は、構造的な action の enable/競合の
  エッジを公開します。`--projection impact_graph --focus NODE` は、TSG のノードの
  周りの上流/下流のスライスを出力します。バッチモードで複数のファイルや
  ディレクトリを受け付けます。
  ディレクトリは `*.fsl` について再帰的に展開され、決定的にソートされます。
  スタンドアロンの refinement マッピングは `--projection
  refinement_graph` で見られます。このコマンドは impl/abs のモデルのパスを持たない
  ため、これは未解決の構造ビューです。プロジェクトのマニフェストは `--projection
  traceability_graph` で見られます。その action のエッジは検査済みの対応 IR を
  使い、合成された auto マッピングを含みます。`--projection code_audit --code
  <path>` は、正確な実行可能 Kernel 要件ターゲットを、言語非依存の
  `@fsl.trace` ソースアノテーションへマップします。missing/orphan/mismatched の
  ペアは `formal_status:not_a_violation` のレビュー findings のままです。これは
  単一 spec かつ JSON のみです。
  `--format dot` と `--format mermaid` は、JSON をデフォルトに保ちつつ、レビュー
  図のためにグラフ形の射影をエクスポートします。`--profile
  ai-review` は、`disconnected_requirement`、
  `unanchored_property`、`progressless_cycle`、`unwritten_state`、
  `unread_state`、`unguarded_action`、`conservation_candidate`、
  `divergent_choice`、`unconstrained_effect` のようなレビュー findings を出力
  します。最後の 2 つは固定の深さ 4 の BMC プローブを使います:
  `evidence_basis:"bounded_bmc"`、reachable な分岐の witness、どの成果が意図されて
  いるかを問う疑問形の `spec_question` を含みます。`undecided:` の宣言との完全一致
  は `acknowledged:true` と `acknowledged_by` つきで可視のままです。一致しない
  意味的な findings は acknowledgement のフィールドを運びません。BMC に裏付け
  られた `unconstrained_effect` は、同じ状態の
  構造的な `unread_state` を抑制します。意味的な action の witness は、同様に重複
  する `unguarded_action` を抑制します。不在は、境界を超えた決定性の証明では
  ありません。[`DESIGN-underspecification.md`](DESIGN-underspecification.md) を
  参照。
  正確な識別子の検査はさらに、コード形のタグのトークンがもはや存在しないときに
  `tag_stale_reference` を、タグがその宣言の形式的な定義に存在しない現在の
  状態/定数を名指しするときに `tag_formula_disjoint` を出力します。
  `--export tag-review` は、タグ付きの宣言を 1 つずつ、そのレンダリングされた形式
  的な定義とともに、スキーマ `tag-review.v0` の下で出力します。自然言語の意味を
  判断したり、モデルを呼んだりはしません。
  これらの findings は `formal_status: "not_a_violation"` を運びます。構造的な
  サイクルや非連結の成分は、証明の失敗ではありません。TSG、グラフの射影、findings
  のバージョン付きスキーマは `schemas/fslc/analysis/` の下で公開されています。→
  [`DESIGN-analysis.md`](DESIGN-analysis.md)
- **`fslc html`** — 同じ explain/verify のエビデンスの上の、自己完結の HTML
  レポート: ステータスのサマリー、state/action/プロパティのテーブル、action から
  状態への書き込みグラフ、トレースのタイムライン、witness の例、counterfactual、
  ソース、生の JSON。読み手に CLI の実行を要求することなく、PR、設計レビュー、
  非専門家によるプロジェクトレビューに使うためのものです。→
  [`DESIGN-html-report.md`](DESIGN-html-report.md)

書く前の規律(形式化メモ、自然言語→構文の逆引き、推奨プラクティス)は
`skills/` の下の AI エージェントスキルにあり、共有の言語リファレンスは
`skills/fsl/SKILL.md` にあります。保守される実例は
[`examples/validation/`](https://github.com/ymm-oss/fsl/tree/main/examples/validation) にあります。

## 16. ghost 型への昇格判定(typestate)

`fslc typestate <file.fsl> [--ts]` は、設計 spec の状態機械(enum 値の struct
フィールド / 状態変数 / `Option<_>` スロット)を、ホスト言語の typestate
(ghost 型)へどれだけ健全にマップできるかを判定します。各
`(entity, action)` を、`derivable`(from の状態がエンティティ自身のローカルな
ガード)/ `branching`(`if` の内側でデータ依存)/ `relational`(ローカルな
ガードがなく、事前条件が外部の構造に住む — 型では表現できず、ランタイム/検証の
義務として残る)に分類します。エンティティの
`applicability` が `full` になるのは、すべての遷移が derivable/branching のとき
だけです。`--ts` を付けると、derivable な部分の TypeScript スキャフォールドを出力
します。
ネイティブ Rust のコマンドは、この判定を、非公開のパーサー/モデルの構造ではなく、
バージョン付きの公開 Kernel JSON v1 コントラクトから行い、未対応の Kernel
スキーマバージョンでは fail closed します。その JSON レポートと TypeScript の
バイト列は、凍結された参照実装の出力と互換のままです。
→ [`DESIGN-typestate.md`](DESIGN-typestate.md)
