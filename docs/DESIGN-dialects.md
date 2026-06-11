# FSL v3 — 方言実装設計(DESIGN-layers.md の段階1〜3の詳細)

アーキテクチャと動機は DESIGN-layers.md。本書は実装可能レベルの仕様。
不変の大原則: **カーネルの意味論に変更を加えない**。方言は AST 展開
(compose と同型)、メタデータは表示への配管のみ。

## 段階1: トレーサビリティ・メタデータ配管

### 1.1 カーネル構文: 宣言タグ

invariant / reachable / leadsTo / action(fair 含む)の宣言で、
**ブロック開始 `{` の直前に省略可能な文字列リテラル**を許す:

```fsl
invariant PerUserCap "REQ-7: ユーザー毎の購入上限" { ... }
reachable SoldOut "AC-3: 売り切れに到達できる" { ... }
leadsTo Served "REQ-9: カートは必ず処理される" { P ~> Q }
action submit(c: Case, a: Amount) "REQ-1: 閾値以下は自動承認" { ... }
```

- 文字列は `"ID: 原文"` 形式。**最初の `:` で分割**して
  `meta = {"id": "REQ-7", "text": "ユーザー毎の購入上限"}`。
  `:` が無ければ `{"id": 全文, "text": null}`。前後空白は strip。
- AST → spec dict の各要素(invariant/reachable/leadsto/action)に
  `meta` キー(無タグなら None)。
- 既存仕様(タグなし)の出力は**バイト単位で不変**であること。

### 1.2 JSON 出力への透過

`meta` を持つ要素が関与する出力に `"requirement": {"id", "text"}` を付与:

| 出力 | requirement の出所 |
|---|---|
| violated(invariant / leadsTo) | 違反した invariant / leadsTo の meta |
| violated(ensures / partial_op) | 当該 action の meta |
| unknown_cti | 帰納的でなかった invariant の meta |
| coverage 診断オブジェクト(covered: false) | 当該 action の meta |
| scenarios の reach_* / respond_* | 性質の meta |
| scenarios の cover_* | action の meta |

- `_bounds_*` / `_partial_*` などの自動生成要素は meta なし(従来どおり)。
- refine 出力への透過は段階2(implements)で行う。
- 新サブコマンドは作らない(`fslc trace` は段階2以降で検討)。

### 1.3 テスト(tests/test_meta.py)

タグ付き invariant 違反 / coverage false / unknown_cti / scenarios の
requirement フィールド、`:` なしタグ、タグなし仕様の出力不変(既存全テスト
green がその証明)、check がタグ付き構文を受理。

## 段階2: fsl-req 方言

ファイル拡張子は `.fsl` のまま(トップレベルキーワードで判別)。
`requirements <Name> { ... }` をトップレベルに追加し、**展開器
`src/fslc/dialects.py` の `expand_requirements(ast, base_dir) -> ast`** が
カーネル AST(通常の spec)に変換する。compose と同じ配線位置。

### 2.1 構文

```fsl
requirements ReturnSystemReq {
  implements ReturnPolicy from "return_policy.fsl" {   // 省略可
    map cases[c: CaseId] = if sys[c].st == New then Requested else ...
    map refunded = paid_count
    // action 対応は branches の maps 句と通常 action の maps 句から自動収集
  }

  // 型・状態・init はカーネル構文そのまま(暗黙の状態は作らない)
  type CaseId = 0..2
  type Amount = 0..3
  const AUTO_LIMIT = 1
  enum SSt { New, AutoApproved, MgrQueue, MgrApproved, MgrRejected, Paid }
  struct RCase { st: SSt, amount: Amount }
  state { sys: Map<CaseId, RCase>, paid_count: Int }
  init { ... }

  requirement REQ-1 "閾値以下の返品は自動承認される" {
    action submit(c: CaseId, a: Amount) {
      requires sys[c].st == New
      requires a > 0
      branches {
        when a <= AUTO_LIMIT {
          sys[c] = RCase { st: AutoApproved, amount: a }
        } maps approve(c)
        when a > AUTO_LIMIT {
          sys[c] = RCase { st: MgrQueue, amount: a }
        } maps stutter
      }
    }
  }

  requirement REQ-2 "支払いは承認後のみ" {
    fair action pay(c: CaseId) maps refund(c) {
      requires sys[c].st == AutoApproved or sys[c].st == MgrApproved
      sys[c].st = Paid
      paid_count = paid_count + 1
    }
    invariant PaidLedger { paid_count == count(c: CaseId where sys[c].st == Paid) }
  }

  acceptance AC-1 "小額は自動承認され支払われる" {
    submit(0, 1)
    pay(0)
    expect sys[0].st == Paid
  }
}
```

### 2.2 展開規則

1. `requirement <ID> "<text>" { items }` → 中身の action / invariant /
   reachable / leadsTo をトップレベルへ持ち上げ、各要素に
   `meta = {id: ID, text}` を付与(段階1の機構)。ID は `REQ-1` 形式の
   識別子トークン(英数字とハイフン)。
2. `branches { when <cond> { 文... } maps <abs対応> ... }` →
   アクションを分岐ごとに分割: `submit__b1`, `submit__b2`(表示名は
   `submit[a <= AUTO_LIMIT]` 形式 — 表示名マップは compose の機構を流用)。
   各分割アクション = 元の requires + when 条件 + 分岐本体。
   when 条件は**網羅・排他をチェックしない**(enabled の通常意味論に任せる。
   重なれば両方 enabled、漏れれば disabled — coverage 診断が検出する)。
3. `maps <abs_action>(<args>) | stutter`(action 修飾 / branches 内)→
   `implements` ブロックの map 群と合成して **refinement AST を内部生成**。
4. `implements ... { map ... }` がある場合、verify / check 時に
   **上位層への refine 検査を同時実行**し、結果 JSON に
   `"implements": {"abs": "ReturnPolicy", "result": "refines" | {...違反}}` を
   追加する(refine が失敗しても verify 自体の結果は別建てで返す)。
   refine 違反 JSON には関与した impl action の `requirement` を透過(§1.2 拡張)。
5. `acceptance <ID> "<text>" { <action呼び出し>...  expect <式> }` →
   (a) スキーマ: 確定ステップ列。展開時に**具象 Monitor で再生**し、
   各ステップ ok + 最後に expect が真であることを check 時に検証
   (失敗は `kind: "acceptance"` のエラーで AC ID + 失敗ステップを報告)。
   (b) scenarios 出力に `kind: "acceptance"` のシナリオとして埋め込み
   (steps / expected_states は再生結果から構築)→ testgen に自然に流れる。
6. 展開後 spec の名前は `requirements` の名前。それ以外の項目
   (type/enum/struct/state/init/トップレベル action 等)はそのまま透過。

### 2.3 テスト(tests/test_req_dialect.py)

返品要件(§2.1 とほぼ同じ)を fixture に:
- check ok / verify verified(+ implements.refines)/ induction proved
- branches の分割が coverage に表示名で現れる(`submit[a <= AUTO_LIMIT]`)
- 違反を仕込んだ変種で requirement {id, text} が反例に載る
- acceptance: 正例が scenarios に出る / expect を偽にした変種が check 時に
  `kind: "acceptance"` で落ちて AC ID を指す
- implements の写像を壊した変種 → implements.result が refinement_failed
- 既存全テスト不変

## 段階3: fsl-biz 方言

`business <Name> { ... }` をトップレベルに追加。展開器
`expand_business(ast) -> ast`(他ファイル参照なし)。

### 3.1 構文

```fsl
business ReturnHandling {
  actor Customer, Manager                  // → enum Actor { Customer, Manager }(参照されなければ省略可な情報注釈)
  case Return = 0..2                       // → type Return = 0..2

  process Return {                         // case 型ごとに1プロセス
    stages Requested, Approved, Rejected, Refunded   // → enum ReturnStage
    initial Requested
    transition approve  Requested -> Approved by Manager
    transition reject   Requested -> Rejected by Manager
    transition refund   Approved  -> Refunded by Manager
  }

  kpi refunded counts Return in Refunded   // → state refunded: Int +
                                           //   refund 遷移で +1 + 整合 invariant

  policy PAY-1 "返金は承認済みケースのみ" invariant {
    // 式中で stage(c) が使える(c は case 型の束縛変数)
    forall c: Return { stage(c) == Refunded => true }   // 例
  }
  policy PAY-2 "申請は必ず裁定される" responds {
    forall c: Return { stage(c) == Requested ~> not (stage(c) == Requested) }
  }
  goal AllSettled "全件が完了しうる" {
    forall c: Return { stage(c) == Refunded or stage(c) == Rejected }
  }
}
```

### 3.2 展開規則

1. `case X = lo..hi` → `type X = lo..hi`。
2. `process X { stages S1..Sn  initial Si  transition t A -> B by Actor }` →
   - `enum XStage { S1, ..., Sn }`
   - `state { x_stage: Map<X, XStage> }`(変数名はプロセス名の小文字 +
     `_stage`)+ `init { forall c: X { x_stage[c] = Si } }`
   - 遷移ごとに `fair action <t>(c: X) "by <Actor>" {
       requires x_stage[c] == A   x_stage[c] = B  [kpi更新] }`
     (by は meta.text に載せる: `meta = {id: t, text: "by Manager"}`。
      policy 由来でないので requirement とは別フィールド `"actor"` でもよい —
      実装単純さ優先で meta.text に "by Manager" を入れる形でよい)
   - 同名遷移ラベルの重複は type エラー。
3. `kpi k counts X in S` → `state { k: Int }` + init 0 +
   S へ**入る**全遷移で `k = k + 1`(S から出る遷移があれば type エラー —
   v3 では減算 KPI は未対応と明文化)+
   `invariant _kpi_k { k == count(c: X where x_stage(c) == S) }`(自動)。
4. `policy <ID> "<text>" invariant { 式 }` → invariant(meta 付き)。
   `policy ... responds { P ~> Q }` → leadsTo(meta 付き)。
   `goal <ID> "<text>" { 式 }` → reachable(meta 付き)。
   式中の `stage(c)` は当該 case 型の束縛変数 c に対し `x_stage[c]` に
   書き換える(束縛の型からプロセスを特定。曖昧なら type エラー)。
5. `actor` 宣言は名簿(transition の by の検証に使う。未宣言 actor は
   type エラー)。それ以外の意味論なし。

### 3.3 テスト(tests/test_biz_dialect.py)

§3.1 の返品プロセスを fixture に: check ok / verify verified /
induction proved / policy 違反変種で requirement(=policy ID+text)が
反例に載る / kpi 整合 invariant の自動生成 / goal が reachable として
witness / 未宣言 actor・KPI 減算の type エラー / 既存全テスト不変。
さらに examples/layers の return_policy.fsl をこの方言で書き直した
`return_policy_biz.fsl` が、既存 return_refines.fsl(の abs 名調整版)で
**要件層から refine できる**こと(= 方言展開後のカーネル仕様が手書きと
同等であることの実証)。

## 段階4: 3層ドッグフーディング

返品ドメインを3層フルで: fsl-biz(業務)← fsl-req(要件; implements)←
fsl(設計; 永続化や再試行などの実装詳細を追加して refine)← examples の
Adapter 実装。所見は DOGFOOD-4.md。LANGUAGE.md・skills/fsl への方言の
追記もここで行う。
