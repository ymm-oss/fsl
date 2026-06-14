# FSL — fsl-ui(画面遷移・UIステート方言)スパイク所見と設計

issue #9。デザイン系方言の検討。**スコープはインタラクション設計(画面遷移・UIステート)
に限定**(ビジュアルデザインは遷移系の意味論を持たずカーネルで検査できないため対象外。
DESIGN-layers の原則と整合)。本書はスパイク(素の fsl で手書き → 検証 → 要件層へ refine)
の所見と、go の場合の展開規則案。実走資産は `examples/ui_spike/`。

## スパイクの結論: 技術的実現性は確認(conditional GO)

返品ドメインの申請画面フロー(Form → Submitting → ReadyToPay/MgrPending → Done/Error)を
**素の fsl で手書きし、検証も要件層への refine も通った**。カーネルの意味論変更は不要で、
fsl-ui は AST 展開(糖衣)として成立する見込み。

## 確認できたこと

### 1. 素の fsl が画面フローを完全に表現する(新意味論ゼロ)

| デザイン上の問い | カーネル機能 | スパイク結果 |
|---|---|---|
| 全画面に到達できるか(デッドスクリーン) | `reachable` | CanDone/CanError/CanMgr 全 witness |
| 袋小路・無限ローディングがないか | `leadsTo` | `SubmitResolves: Submitting ~> not Submitting` proved |
| ガード付き遷移の整合 | `requires` | フォーム空で submit 不可 等 |
| 二重送信防止 | `invariant` | `submitting => screen == Submitting` proved |
| 画面=状態 / 遷移=操作 | `enum` / `action` | そのまま |

`ReturnUI` は **verified + proved(k=1)**。

### 2. UI フローは要件層を refine する(アーキテクチャ検証)

`fsl-req ⊒ fsl-ui`(設計層の兄弟として並走)が機械的に成立。UI フロー(impl)が
要件エッセンス(abs)を **refines**:
- UI 専用ステップ(enter_amount/submit/resp_mgr/resp_error/retry)→ `stutter`
- ドメインをコミットするステップ(resp_auto/mgr_approved → approve、pay → pay)→ 要件アクション

これが #9 の核心価値「要件の受け入れ基準ステップ列に画面パスが存在するか」を機械検査で
担保することの実証(「要件は定義したが UI に導線がない」を反例付きで検出できる)。

## 見つかった落とし穴(展開器が吸収すべきもの)

### F-UI-1(バグ・修正済み): refinement の 0引数 abstract アクション写像

`action pay() -> pay()`(0引数 impl → 0引数 abstract)が `expects 0 arguments` の偽エラーで
落ちた。原因は Lark `maybe_placeholders` が空括弧を `(None,)` にし、引数1個と数えていたこと。
既存 refinement は 0引数 impl を全て `stutter` に写していたため未発覚。`grammar.py` の
`mapped_action_target` / `req_mapped_action_target` で None を除去して修正(本スパイクの副産物)。

### F-UI-2: 下書き(フォーム入力)状態は写像でゲートせよ

`map amt = amount` と直結すると、フォーム入力(未コミット)が抽象ビューに漏れ
`stutter_changed_abs`。`map amt = if screen == ReadyToPay or screen == Done then amount else 0`
と**コミット済み画面でのみ可視化**する状態タグ写像が必要(seat_booking と同型)。
UI フロー特有の「下書き vs 確定」の区別。

### F-UI-3(重要): 画面名とドメイン状態名の enum 衝突

UI の `Screen.Paid` と要件の `St.Paid` が同名だと、写像式 `if screen == Paid then Paid` の
右辺が impl 側 enum に解決され `abs_state_mismatch`。**画面名はドメイン状態名と被りやすい**
ので fsl-ui の頻出ハザード。スパイクでは `Done` に改名して回避。
→ 展開器は画面 enum を名前空間化(例 `ui_Screen`)して衝突を構造的に防ぐべき。

### F-UI-4: back stack は Seq でなく Map+depth

戻る(LIFO)に対し `Seq<T,N>` は FIFO で不向き。`Map<Depth,Screen> + depth` イディオムで
表現でき(`NavStack` が verified)、満杯は depth のガード/type_bound、空 back は
`requires depth > 0`。同時代入のため `cur = hist[depth - 1]` は depth の旧値を読む点に注意。
新カーネル不要だが**展開器がこのイディオムを生成すべき**(手書きは煩雑)。

## 展開規則案(go の場合、`expand_ui`、compose/expand_business と同型)

| 方言構文(案) | カーネル展開 |
|---|---|
| `screen S { A, B, ... }` | `enum ui_S { A, ... }`(名前空間化、F-UI-3 対策)+ `state { screen: ui_S }` |
| `navigate <act> A -> B [requires …]` | `action <act> { requires screen == A … screen = B }` |
| `back`(有効時) | `Map<Depth,Screen> + depth` イディオム生成(F-UI-4) |
| `modal M over A { … }` | サブ画面 enum + 復帰遷移 |
| `loading`/`async` 状態 | 画面 × 非同期状態の struct/直積(糖衣の効かせ方は要検討) |
| `implements <Req> from "…" { map … }` | 要件層への refinement 自動生成(F-UI-2/3 を写像生成で吸収) |

## この層が扱わないもの(FSL の外)

ビジュアルデザイン(色・タイポ・レイアウト・美的判断)、ユーザビリティ、アニメーション。
遷移系の意味論を持たないものは各層の文書に書く(カーネルに展開しない大原則)。

## 進め方 / go-no-go

- 技術リスクはスパイクで除去済み(表現可能・refine 可能・新カーネル不要)。
- 残る設計作業は **back stack 糖衣** と **画面 enum の名前空間化**(F-UI-3/4。いずれも特定済み)。
- 想定ユーザーは デザインエンジニア/ハンドオフ検証、または **AI が Figma フロー図を fsl-ui に
  転記して検査**(AI-Native の立ち位置)。デザイナー直書きは非現実的。
- **判断**: UI フロー検査の需要があれば go(`expand_ui` + テスト + ドッグフーディング)。
  需要が薄ければ、当面は「素の fsl で画面フローを書く」運用(本スパイクの ReturnUI が雛形)で
  十分価値が出る。**F-UI-1 の修正は方言と独立に有用なので先行マージ済み。**

## 不変の原則

カーネルの意味論に変更を加えない。方言は AST 展開のみ。表現できないものは層の文書に書く。
