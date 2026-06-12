---
name: fsl
description: >-
  FSL (AI-Native Formal Specification Language) で仕様を書き・検証し・修復する。
  .fsl ファイルの作成/編集/検証、fslc コマンド(check/verify/scenarios/replay/
  testgen/refine)の実行、形式仕様・モデル検査・invariant 証明・仕様からのテスト
  生成・refinement 検査・実装の適合性検査を行うときに使う。業務フロー/業務
  プロセスの矛盾チェック、As-Is/To-Be の統制検査、要求・要件定義の形式化、
  受け入れ基準のテスト化、SLA・非機能要件の検査も対象。「仕様を書いて」
  「形式検証して」「業務フローを検証して」「要件定義して」「FSL」などが合図。
---

# FSL — 仕様の書き方と write→verify→repair ループ

FSL は学習データに存在しない言語。**記憶で書かず、本書と reference.md に従うこと。**
構文の詳細・全式カタログ・イディオム集は同ディレクトリの `reference.md` を読む
(仕様を書く前に必ず一読)。リポジトリ内なら `docs/LANGUAGE.md` が完全版、
`specs/*.fsl` が動く実例(cart_v1 が基本形、mutex_queue が Seq+leadsTo、
bank_* が refinement+compose の実例)。

## 前提: 検証器 fslc

このスキルは言語知識を供給するだけで、検証は CLI `fslc` が行う。未導入なら
FSL リポジトリ(`pyproject.toml` のあるルート)で `pip install -e .` する
(依存は lark と z3-solver のみ、ネイティブビルド不要)。`fslc` が PATH に
無い環境では `python -m fslc ...` でも同じ。

## 実行方法

```bash
fslc <subcommand> ...            # editable install 済みの場合
python -m fslc <subcommand> ...  # または venv の python で
```

出力は常に stdout への単一 JSON。exit: 0=成功(verified/proved/生成)、
1=性質不成立(violated/reachable_failed/unknown_cti/nonconformant)、
2=仕様エラー(parse/type/semantics/io)、3=内部エラー。

## 仕様を書く前: 形式化メモ(チャットに出す。別ファイルにしない)

自然言語の要件・業務ルール・コードから FSL を起こすとき、**いきなり `.fsl` を
書かない**。まず**形式化メモ**をチャットに出し、人間の確認を受けてから仕様化する。
fslc が保証するのは「書かれた仕様の内部整合」であって「仕様が元の意図に忠実か」では
ない — そのギャップ(AI の取り違え・要件の取りこぼし・勝手な補完)はこのメモで潰す。
メモは思考と確認の足場であって成果物ではないので、**別ファイルにはしない**
(軽量ループを保つ。成果物は `.fsl` 本体だけ):

- **用語集と台帳**: 状態変数の候補、アクション(誰が・いつ enabled か)、
  enum / ドメイン型の候補と値域
- **要件の正規化**: 要件ごとに トリガ / 制約 / 例外 / **境界の含意**(以上か超か、
  以前か以後か、含むか含まないか)を一行ずつ。ここが取り違えの最頻発地点
- **仮定台帳**: 原文が曖昧だった箇所、採った解釈、その理由(下記のとおり仕様へ移す)
- **人間への質問**: 仕様化では決められない判断(業務ルールの優先順位・例外の優先など)

人間が読むのはこのメモと検証器の反例だけでよい — **論理式を直接レビューさせない**。
メモに人間の確認・修正が入ってから `.fsl` を書く。

### 仮定だけは仕様に残す(別メモファイルではなく .fsl に畳む)

メモの大半はチャットで消えてよいが、**仮定台帳だけは消すと後で「なぜこの解釈に
したか」を辿れず困る**。別ファイルにすると仕様との同期が崩れるので、**`.fsl` 本体に
コメント / タグで残す**:

- グローバルな仮定 → 仕様冒頭に台帳ブロック: `// ASSUME-1: 在庫は同時に1ユーザーのみ予約`
- 特定のガード / invariant を正当化する仮定 → その宣言にタグを付ける:
  `invariant OnePerUser "ASSUME-1: 同時予約は1ユーザー" { ... }`

こうすると仮定が仕様と一緒に動き、PR でも見え、将来の `--strict-tags` 検査が
「意図した仮定(タグ付き)」と「根拠のない捏造(タグなし)」を区別できる。

## 自然言語 → 構文の対応(形式化メモから仕様へ)

要件の正規化(上記メモ)で切り出した文を、次の対応で構文に落とす。reference.md §8 の
イディオム集が「FSL → 正しい書き方」なのに対し、これは「自然言語 → どの構文か」の逆引き。
**この表に載らない自由作文の論理式は取り違えやすいので、形式化メモで人間確認の印を付ける。**

| 自然言語のパターン | FSL 構文 |
|---|---|
| 「〜してはならない」「常に〜である」(禁止・不変) | `invariant`(安全性) |
| 「〜の場合だけ〜できる」(前提条件) | action の `requires` |
| 「〜したら必ずいつか〜する」(応答・進行) | `leadsTo` + 進行を担うアクションに `fair` |
| 「一度〜したら二度と〜できない」(履歴依存) | ゴースト変数(`ever_*`)+ invariant |
| 「〜に到達できる / 到達できてしまう」(可能性) | `reachable`(witness、または過剰制約の検出) |
| 「K 回 / K tick 以内に〜」(期限) | requirements の `time` + `deadline`(reference §11) |
| 数の上限・下限・非負 | ドメイン型 `type T = lo..hi`(境界 invariant は手書きしない) |
| 「以下 / 未満 / 以上 / 超」「以前 / 以後」 | `<= / < / >= / >`。**境界の含意はメモで明示**(最頻発の取り違え) |
| 「合計が〜と一致」「件数が〜」(集計整合) | `sum(...)` / `count(...)` の invariant |

## 標準ワークフロー(proved を標準とする)

1. 仕様を書く → `fslc check file.fsl`(構文・型のみ、速い。エラーの `loc`/`expected`/`hint` に従って修正)
2. `fslc verify file.fsl --depth 8` → 結果ごとの対応は下表
3. verified になったら `fslc verify file.fsl --engine induction` → `proved` で完了
   (注: `--depth K` はステップ K を**含む**。`proved` が無限深度になるのは
   invariant のみで、**leadsTo は深さ K までの有界検査のまま** — 状態が単調にしか
   進まない非循環の仕様なら、最長実行長より大きい `--depth` で再 verify すれば
   全実行を被覆できる)
4. 必要に応じて: `fslc scenarios`(統合テスト雛形 JSON)、`fslc testgen -o test_x.py`
   (実装適合 pytest 雛形)、`fslc replay --trace events.json`(ログ適合性)、
   `fslc refine impl.fsl abs.fsl mapping.fsl`(詳細仕様の忠実性検査)

## 修復プロトコル(結果 → 次の一手)

| result / violation_kind | 意味 | 次の一手 |
|---|---|---|
| `violated` / `invariant` | 反例あり(trace は最短) | trace の `changes` と `violating_bindings` を読み、ガード追加か invariant 修正 |
| `violated` / `type_bound` | 有界型が範囲外(自動検査) | `last_action` のガード不足。`requires` で範囲を守る(invariant を手書きしない) |
| `violated` / `partial_op` | 空 Seq の pop/head、添字範囲外、除数 0 | `requires q.size() > 0` / `requires d != 0` か `if` でガード |
| `violated` / `ensures` | 事後条件不成立 | 本体と ensures のどちらが正かを判断して修正 |
| `violated` / `leadsTo` | 応答性質の反例(ラッソ/停滞) | trace の `loop_start` を確認。進行を担うアクションに `fair` を付けるか仕様を修正 |
| `reachable_failed` | 到達したい状態に届かない | `action_coverage` の `blocking_requires`(unsat core)を読む。ガード緩和/アクション追加/`--depth` 増 |
| `unknown_cti` | invariant は真だが帰納的でない | **CTI の開始状態 = 全 invariant を満たす幽霊状態。それを排除する補助 invariant(ドメインの真実であるもの)を追加して再実行。** 実績: 1ラウンド収束(例: 「キューに重複なし」「返金は Captured のみ」) |
| `error` / `parse` | 構文エラー | `loc` と `expected`(候補トークン)に従う |
| `error` / `type` | 型エラー | `hint` に従う(例: `x == some(e)` → `x is some(v)` で束縛して比較) |
| `error` / `semantics` | 二重代入など | 同一パスで同じ変数に2回代入しない(if の then/else は別パスなので可) |
| `error` / `vacuous` | init が充足不能(矛盾した代入など) | init を見直す。1つの状態変数に矛盾する値を与えていないか確認。範囲外の値による違反は別物で `violated`/`type_bound` になる |
| `refinement_failed` / `abs_requires_failed` | 詳細層の遷移が上位層のガードを破る(例: 承認を飛ばす近道) | `impl_action` と `impl_trace` を読む。詳細層にガードを足すか、対応(`maps`/写像)の解釈を見直す |
| `refinement_failed` / `abs_state_mismatch`・`stutter_changed_abs`・`map_out_of_bounds` | 写像の不整合(更新が対応しない / stutter なのに上位状態が変わる / 写像値が型範囲外) | `mismatch` のパスと `abs_before/after` を比較。写像式か action 対応を修正 |
| verify 内 `implements.result: violated` | 要件層が上位(業務)層から逸脱 | `implements.violation` の中身は refinement_failed と同形。上と同じ手順 + 要件側の `requirement` を確認 |
| `error` / `acceptance` | 受け入れ基準の再生が失敗 | 失敗した AC の ID とステップが返る。手順の前提(状態)か expect のどちらが正かを判断して修正 |
| `error` / `forbidden` | 拒否されるべき操作列が受理された(過小制約。安全性 invariant では沈黙する種類) | `accepted_trace` が受理経路。最後の操作を enabled にしている requires が緩い → ガードを追加するか仕様を見直す |
| `error` / `forbidden_setup` | forbidden の前提(最後以外の)ステップが enabled でない(トレース不正) | セットアップ手順を見直す。最後以外はそこへ到達する手順であり、成功扱いにはならない |

coverage が `false` のアクションは `blocking_requires` が「どの requires が
阻んでいるか」を句単位で特定している。silent に無視しないこと。

反例を受けて**解釈を変えた**(ガードを足した・invariant を緩めた・例外の扱いを
決めた)ときは、その判断を仮定台帳(`.fsl` の `// ASSUME-n:` コメント / タグ)に
追記する。verified にする最短経路はしばしば「仕様を弱めること」なので、何を
なぜ弱めたかが残っていないと、後から骨抜き修復と正当な修正を区別できない。

## 最小構文(詳細・全カタログは reference.md)

下記はそのまま `fslc check` を通る自己完結の雛形(Map/Option/Seq の要素型は
全てドメイン型として宣言してある — **使う型は必ず `type ... = lo..hi` か `enum`
で宣言する**。未宣言だと `unknown type` の型エラーになる):

```fsl
spec Cart {
  const CAP = 3
  type ItemId = 0..1
  type UserId = 0..1
  type JobId  = 0..1
  type Qty    = 0..5                     // ドメイン型 = 有界整数。範囲は自動検査
  enum St { Open, Closed }
  struct Order { st: St, qty: Qty, buyer: Option<UserId> }

  state {
    stock: Map<ItemId, Qty>,
    cart:  Option<ItemId>,
    q:     Seq<JobId, CAP>
  }
  init {
    forall i: ItemId { stock[i] = 1 }
    cart = none
    q = Seq {}
  }

  action add_to_cart(i: ItemId) {
    requires cart == none
    cart = some(i)
  }

  fair action abandon() {                // 常に可能なので Served(下記)が成立する
    requires cart != none
    cart = none
  }

  fair action checkout(u: UserId) {      // fair = 弱公平(leadsTo 用)
    requires cart is some(i)             // i がここで束縛される
    requires stock[i] > 0
    stock[i] = stock[i] - 1              // 右辺は全て旧状態を読む(同時代入)
    cart = none
    ensures stock[i] == old(stock[i]) - 1
  }

  // 「stock[i] >= 0」のような境界 invariant は書かない(Qty=0..5 で自動検査)。
  // 下は非・境界の真の安全性 invariant の例(<式> の位置)。
  invariant QueueStaysEmpty { q.size() == 0 }   // q を触る action が無いので不変
  reachable SoldOut { stock[0] == 0 }           // witness が返る
  leadsTo Served { cart is some(j) ~> cart == none }   // ~> は leadsTo 専用
}
```

## 絶対に守る規則(構造的落とし穴)

- **番兵値(-1 等)禁止 → `Option<T>`**。`x == some(e)` は型エラー —
  `x is some(v)` で取り出す。`== none` / `!= none` は可。
- **「0以上」系 invariant を手書きしない** → `type Qty = 0..N` で自動検査される。
- 同一実行パスでの**二重代入はエラー**。if の後に分岐内と同じ変数へ代入もエラー。
- Set/Seq の更新は**再代入**: `s = s.add(x)`、`q = q.pop()`。
- Seq の `pop/head/at` と `/` `%` の除数は**必ずガード**(requires か if)。忘れは partial_op で検出される。
- invariant で Seq を語るときは添字ガード:
  `forall i in 0..CAP-1 { i < q.size() => P(q.at(i)) }`(範囲は `0..CAP-1` と
  const から導出して書く — リテラルをハードコードすると容量変更に追従しない)。
- **Map のネスト(`Map<K1, Map<K2,V>>`)は不可** → 2軸は積のドメイン型1本に
  平坦化(`type Cell = 0..ROOMS*SLOTS-1`)し、軸は `c / SLOTS`・`c % SLOTS` で復元。
- 「X の後に Y が起きた」という**履歴**は状態で書けない → ゴースト変数
  (`ever_locked` 等)を足すか、応答性質なら `leadsTo`。

## 推奨プラクティス(任意 — リスクに応じて。小さな仕様では省いてよい)

上の「絶対に守る規則」と違い、ここは**義務ではない**。重い手順を全仕様に課すと
軽量ループが死ぬので、重要な制約・高リスク仕様にだけ効かせればよい。

- **正例とのペア**: invariant を書いたら、その境界近傍で「許されるべき挙動がまだ
  可能」なことを示す `reachable` か `acceptance` を1本添えると、ガードの掛けすぎ
  (過剰制約)と空虚な invariant を自分で検出できる。修復でガードを強めたときに特に
  有効。例: 在庫を減らす仕様に `reachable SoldOut { stock[0] == 0 }` を添えると
  「売り切りまで到達できる=ガード過剰でない」が確認できる。
- **1要件=1宣言**: 巨大な連言 invariant を避け、要件単位で宣言を分ける。反例の
  `requirement` タグが効き、診断が読みやすく、どの要件が壊れたか1往復で分かる。
- **ドメインサイジング**: 個体間の相互作用を語る性質はエンティティ3個体以上
  (2だと対称性でバグが隠れる)、容量は「上限+1」を試せる値、検査は depth 8 +
  induction を標準にする。
- **交差検証(高リスク仕様のみ)**: 決済・権限など誤りが重大な仕様は、(a) 原文を
  見ていない別エージェントに `.fsl` を自然文へ翻訳させ要件リストと項目別に突合する、
  (b) state スキーマを固定して2エージェントに独立に dynamics+性質を書かせ、互いの
  `scenarios` を相手の仕様で `replay` して不一致を炙り出す。コストが高いので限定運用。

## 役割別の入口(まず実例を読む)

| 立場 | 読む実例 | 主に書く構文 |
|---|---|---|
| コンサル(業務フロー・規程・As-Is/To-Be) | `examples/consulting/`、`examples/pm/cancel_flow.fsl` | `business`(reference.md §10) |
| PM / PdM(要件定義・受け入れ基準) | `examples/pm/`、`examples/e2e/2_requirements.fsl` | `requirements`(同 §10)+ NFR(同 §11) |
| エンジニア(設計・実装接続) | `examples/e2e/`(3役の連鎖全体)、`examples/bank/` | `spec`(本書)+ refine 写像 + Adapter(同 §9) |

3役を1ドメインで貫通する旗艦例は `examples/e2e/`(経費精算)。

## 3層方言(コンサル / 要件 / 設計)

仕様は3つの層で書ける。**業務 ⊒ 要件 ⊒ 設計 ⊒ 実装**を refinement で連鎖させる
(構文は reference.md §10)。どの層もカーネルに展開されるので
verify/induction/scenarios/Monitor は同じに使える:

- `business Name { process/policy/kpi/goal }` — コンサル層。規程の矛盾=invariant
  違反、死んだ業務ステップ=coverage 診断、業務ゴール到達不能=reachable_failed
- `requirements Name { requirement REQ-1 "原文" {...} / acceptance / branches /
  implements Abs from "file" {map ...} }` — 要件層。`implements` があると verify が
  上位層への refine を同時実行(結果 JSON の `implements`)。`acceptance` は
  check 時に再生検証され scenarios → testgen に流れる。`forbidden`(must-forbid)は
  逆に「拒否されるべき操作列」を書き、最後のステップが拒否される(not-enabled か
  違反)ことを check 時に検証する — 受理されたら `kind: "forbidden"`。安全性
  invariant では沈黙する過小制約(ガード漏れ)を捕まえる独立チャネル(別エージェントに
  NL から正負トレースを書かせる交差検証の受け皿)
- 設計層は通常の `spec`(本書の主対象)。要件層へ `fslc refine` で接続
- **トレーサビリティ**: 宣言の `{` 直前に `"ID: 原文"` タグ。violated / CTI /
  coverage / scenarios に `requirement: {id, text}` が載る — 反例を読んだら
  必ず requirement を見て、その要件の意図に沿って修復すること

## 高度な機能(必要になったら reference.md の該当節)

- **非機能要件**: 権限・監査・容量・信頼性挙動は通常の invariant/leadsTo で書ける。SLA/タイムアウトは requirements の `time`+`deadline`(reference.md §11)
- **Seq の集約**: `sum(i: Idx of log.at(i) where i < log.size())`(Idx は容量を覆うドメイン型)
- **合成**: `compose X { use A as a from "a.fsl" ... }`、同期アクション
  `action s(..) = a.act(..) || b.act2(..) { .. }`、`internal a.act`
- **refinement**: 写像ファイル(`map abs_var = 式`、`action impl -> abs(..) | stutter`、
  写像式限定の `if c then a else b`)+ `fslc refine`
- **実装接続**: `fslc testgen` 生成ファイルの Adapter(reset/step/observe)を
  実装に結線。observe は仕様の論理状態と同形(enum は名前、Option は None|値、
  Seq は list、合成は `alias.var` キー)
