# FSL v3 — 共通カーネル + 3方言アーキテクチャ設計

コンサルティング(業務)・要求/要件定義・設計/実装の3層で FSL を使い、
層間を透過的に連携させる。結論: **可能。カーネルは既に存在し、3層の背骨
(refinement 連鎖)は現行 fslc で動く**(§2 のスパイクで実証済み)。
必要なのは各層の語彙を与える方言フロントエンドと、トレーサビリティの
メタデータ配管のみ。

## 1. アーキテクチャ

```
 方言1: fsl-biz(コンサル)   方言2: fsl-req(要件)   方言3: fsl(設計; 現行)
   actor/process/stage/        requirement/usecase/      spec/state/action/
   policy/kpi/handoff          acceptance/actor          invariant/...
        │ 展開(AST変換)            │ 展開(AST変換)           │ そのまま
        ▼                          ▼                         ▼
 ┌───────────────────────────── 共通カーネル ─────────────────────────────┐
 │ 有界遷移系 + invariant / reachable / leadsTo(+fair)+ 自動検査         │
 │ BMC / k帰納法 / unsat core 診断 / scenarios / refinement / compose     │
 │ JSON 修復プロトコル / 具象 Monitor / replay / testgen                  │
 └────────────────────────────────────────────────────────────────────────┘
        ▲                          ▲                         ▲
        └── refinement ────────────┴── refinement ───────────┘
            (業務 ⊒ 要件)              (要件 ⊒ 設計)        + testgen/replay → 実装
```

- **カーネル = 現行 fslc の意味論そのもの**。新しい検証機能は不要。
- **方言 = フロントエンドの AST 変換**。compose で実証済みのパターン
  (`expand_compose`)と同型: 展開後は通常のカーネル仕様なので、
  BMC・帰納法・scenarios・Monitor・refine が**無修正で**全方言に効く。
- **層間連携 = refinement 連鎖**。業務層を先に proved にし、要件層が
  業務層を refine し、設計層が要件層を refine し、実装は testgen/replay で
  設計層に適合する。各層の検証成果が下の層に「忠実性」として伝播する。

## 2. 実証スパイク(現行カーネルでの2層連携)

返品承認ドメインで実施(2026-06-11、無修正の fslc v2.x):

- **コンサル層** `ReturnPolicy`: 業務ステージ(Requested→Approved/Rejected→
  Refunded)+ ポリシー2本(会計整合 invariant、「申請は必ず裁定される」
  leadsTo)→ **proved**
- **要件層** `ReturnSystem`: 金額・自動承認閾値・マネージャ承認キューを追加
  → **proved**
- **層間** `SystemRefinesPolicy`: enum→enum のネスト条件写像
  (`if st == New then Requested else if ...`)で **refines**。
  自動承認は業務の「承認」に、キュー投入は stutter に対応

スパイクから得た方言設計への入力:
- **(L1) 条件付きアクション対応が要る**: 「submit は金額次第で業務上の
  approve または何も起きない」を、現行は submit_small/submit_large への
  **アクション分割**で表現した。req 方言の展開器はこの分割を自動化する
  (§4.2 の `branches`)。
- **(L2) 業務語彙はそのままカーネルに落ちる**: process=enum+Map、
  policy=invariant/leadsTo、actor=ドメイン型、KPI=ゴーストカウンタ。
  新しい意味論はひとつも要らなかった。

## 3. 方言1: fsl-biz(コンサル)

対象成果物: 業務プロセス定義、ポリシー(業務ルール)、As-Is/To-Be 比較、
プロセス健全性の検査(「この規程ではこの状態に到達できない」の機械検出)。

```fsl-biz
business ReturnHandling {
  actor Customer, Manager
  case Return = 0..2                       // 業務ケース(→ ドメイン型)

  process Return {                         // → enum Stage + Map<Case, Stage>
    stage Requested -> Approved  by Manager : approve
    stage Requested -> Rejected  by Manager : reject
    stage Approved  -> Refunded  by System  : refund
  }

  kpi refunded counts Return in Refunded   // → ゴーストカウンタ + 整合 invariant

  policy NoRefundWithoutApproval invariant { ... }   // 式はカーネル式
  policy EveryRequestDecided responds {              // → leadsTo + fair
    Return in Requested ~> Return in Approved or Rejected
  }
}
```

展開規則: `process` → enum + `Map<CaseId, Stage>` + 遷移ごとの action
(`by <actor>` はアクションのメタデータ。actor がパラメータを持つ遷移は
actor 型のパラメータに)。`kpi ... counts` → Int ゴースト + 自動 invariant
`kpi == count(...)`。`responds` → fair + leadsTo。

**この層が扱わないもの(明文化)**: 実時間・SLA 時間値、確率、金額の
連続量、組織図・文書の散文部分。FSL は「コンサル成果物のうち検査可能な
骨格」を担い、文書を置き換えない。

コンサル価値(カーネル機能の言い換え): 規程の矛盾 = invariant 違反、
死んだプロセスステップ = action coverage false + 阻害規程の unsat core、
業務ゴール到達不能 = reachable_failed、放置されるケース = leadsTo 反例、
As-Is/To-Be の整合 = refinement。

## 4. 方言2: fsl-req(要求・要件定義)

対象成果物: 要件(ID + 原文 + 形式化)、ユースケース、受け入れ基準。

### 4.1 構文

```fsl-req
requirements ReturnSystemReq {
  implements ReturnHandling from "return_policy.fslb"   // 上位層への refinement 宣言

  actor Customer, Manager
  id Case = 0..2
  value Amount = 0..3
  const AUTO_LIMIT = 1

  requirement REQ-1 "閾値以下の返品は自動承認される" {
    action submit(c: Case, a: Amount) by Customer {
      requires state(c) == New
      requires a > 0
      branches {                                  // ← L1: 条件分岐対応の自動分割
        when a <= AUTO_LIMIT -> AutoApproved  maps approve(c)
        when a >  AUTO_LIMIT -> MgrQueue      maps stutter
      }
    }
  }

  requirement REQ-3 "全ての申請はいつか裁定される" responds { ... }

  acceptance AC-1 "小額は即時承認" {
    submit(0, 1)  expect state(0) == AutoApproved
  }
}
```

### 4.2 展開規則

- `requirement` ブロック → 中身のカーネル要素(action/invariant/leadsTo)に
  **`req_id` / `req_text` メタデータを付与**。全 JSON 出力(violated /
  unknown_cti / coverage 診断 / scenarios)に `requirement: {id, text}` が
  載る — 「どの要件が壊れたか」が反例に原文付きで現れる(§6)。
- `branches` → when 条件を requires に足した複数アクションへ自動分割
  (`submit__1`, `submit__2`; 表示は `submit[a<=AUTO_LIMIT]`)。`maps` 句から
  上位層への refinement 写像の action 対応を**自動生成**。
- `implements ... from` → 状態写像(`maps` 句と stage 対応宣言)から
  refinement ファイル相当を合成し、`fslc verify` 時に**上位層への refine 検査を
  同時実行**(検査結果 JSON に `refines_upper: true/false`)。
- `acceptance` → 既知の steps + expect を持つ**確定シナリオ**。replay 機構で
  検査し、scenarios 出力にもそのまま入る(= 受け入れテストが下流の
  testgen に流れて実装の適合テストになる)。

**この層が扱わないもの**: (執筆当時の記述 — その後 DESIGN-nfr.md で権限・
監査・容量・信頼性挙動・離散時刻 SLA まで対応済み。残る対象外は確率・
パーセンタイル・実時間 ms・ユーザビリティ)。要件文書の
うち状態と振る舞いに還元できるものだけを形式化する。

## 5. 方言3: fsl(設計; 現行)

現行言語そのまま。要件層に対して `fslc refine` し、実装に対して
testgen/replay/Monitor で接続する(全部実装済み)。

## 6. 透過連携の3メカニズム

1. **refinement 連鎖**(実証済み): 業務 ⊒ 要件 ⊒ 設計。違反は
   どの層の遷移がどの上位対応を破ったかを、**上位層の語彙で**表示
   (`abs_before/after` は業務ステージ名で出る — 既存の表示機構)。
2. **トレーサビリティ・メタデータ**(新規・配管のみ): `req_id`/`policy_id` を
   カーネル AST のノードに載せ、全 JSON 出力へ透過。設計層の反例から
   「REQ-1(原文)に違反」が直接出る。横断クエリ
   `fslc trace REQ-1`(どの層のどの要素が REQ-1 に由来するか)も同じ
   メタデータから生成。
3. **成果物の下方流動**: 業務層の leadsTo → 要件層の respond 要件の雛形、
   要件層の acceptance → 設計層の scenarios → 実装の testgen。
   逆方向は反例の上方表示(設計層の CTI を要件 ID で注釈)。

## 7. 段階計画

| 段階 | 内容 | 規模感 |
|---|---|---|
| 0 | スパイク(§2)を `examples/layers/` として整備 + 本設計書 | 済/小 |
| 1 | **メタデータ配管**: req_id/text を AST→全 JSON 出力に透過(方言に先行して価値が出る: 現行 fsl でも `// @req REQ-1` 注釈で使える) | 小 |
| 2 | **fsl-req 方言**: requirement/acceptance/branches/implements。展開器は compose と同型。`branches` の自動分割と refinement 自動合成が中核 | 中(compose 1ラウンド相当) |
| 3 | **fsl-biz 方言**: process/policy/kpi。展開器 + 業務語彙での表示 | 中 |
| 4 | 3層ドッグフーディング(コンサル文書を起点に3層+実装まで通す) | 中 |

リスクと退路: 方言が漏れる抽象(検証失敗時にカーネル概念が露出する)に
なる懸念には、層ごとの修復プロトコル表(skills の方言版)で対応する。
方言を作りすぎない原則: **新しい意味論をカーネルに足さない**。方言で
表現できないものは「その層の文書に書く(FSL の外)」と整理する。
