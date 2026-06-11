# ドッグフーディング第3回 — フルワークフロー実証 (2026-06-11)

v2.0/v2.1 の全レイヤを貫通する「FSL の想定する開発の流れ」を、新ドメイン
(二層台帳の銀行口座+監査ログ)で最初から最後まで実走した。

## ワークフローと結果

| 段階 | 成果物 | 結果 |
|---|---|---|
| 1. 抽象仕様 | `specs/bank.fsl`(即時残高の口座) | **proved (k=1)** 一発 |
| 2. 詳細化 | `specs/bank_impl.fsl`(cleared + pending の二層台帳) | **proved (k=1)** 一発 |
| 3. 忠実性検査 | `specs/bank_refines.fsl`(`balance = cleared + pending`) | **refines** 一発。settle は stutter、withdraw のガード強化(cleared のみ)も正しく許容 |
| 4. 合成 | `specs/bank_system.fsl`(bank_impl + audit_log、同期アクション+internal) | verified + **proved (k=1)**。横断 invariant `audit.balance == cleared + pending + withdrawn` がコンポーネントの Seq 集約 invariant と共存して帰納的 |
| 5. 実装接続 | `examples/bank/`(素の Python 実装 + testgen 生成ハーネス + Adapter 結線) | **8/8 passed**(シナリオ再生7 + Monitor をオラクルとする100ステップランダムウォーク) |

`examples/bank/bank.py` は FSL を一切知らない普通のアプリコード。Adapter
(約20行)の結線だけで、仕様から生成された適合テストが実装の正しさを検査する
— これが DESIGN-v1 以来の「仕様と実装の橋」の完成形。

## 発見(2件 — いずれも修正済み)

### BUG16: testgen の生成関数名に表示名のドットが混入(SyntaxError)

合成仕様のシナリオ名(`reach_bank.Settled`)がそのまま関数名になり、生成
ファイルが import 不能だった。識別子サニタイズ+衝突連番+docstring に元名
保持で修正。compose の表示名対応(`__` → `.`)の波及漏れという、第2回 F6 と
同系の「表示レイヤの境界」バグ。

### BUG17: testgen の cwd 相対パス埋め込み / Monitor のパス・ソース誤判別

生成物が `SPEC_PATH = 'specs/...'` を埋め込むためリポジトリルート以外から
実行不能。さらに Monitor が存在しないパス文字列を FSL ソースとして parse し、
io エラーであるべき失敗が UnexpectedCharacters になる(修復プロトコル違反)。
生成ファイル起点の相対パス解決+Monitor のパス判別で修正。

## 所見

- **F8: ワークフロー全段が「一発」で通った。** 第1・2回と異なり仕様起因の
  CTI も反例も出ていない。抽象→詳細→合成の各段で proved を維持したまま
  進める「段階的詳細化」が、このツールチェーンの実際の使い心地として成立する。
- **F9: refinement の写像式に条件式が書けない。**(v2.2 で解消)当初候補
  だった座席予約ドメインでは `map seats[s] = (st == Sold ? some(holder) : none)`
  相当が必要で、FSL に条件式が無いため写像で表現できず、ドメインを変えた。
  → **写像式限定の `if-then-else` 式**として実装(DESIGN-refinement §2.5)。
  断念した座席予約ドメインそのものが2件目の実例となり、
  `specs/seat_booking{,_impl}.fsl` + `seat_refines.fsl` で
  `map seats[s] = if slots[s].st == Sold then slots[s].holder else none` が
  refines を通ることを確認(抽象側の count 集約が条件付き写像値の上で
  正しく評価される)。通常仕様の式文法には開放していない。
- **F10: Adapter の結線規約は十分明確。** observe() の射影(表示名キー、
  Seq は list、Option は None|値)は LANGUAGE.md の規約どおりで迷いなし。
  ランダムウォークが settle の「nothing to settle」ガードと spec の
  `requires pending > 0` の一致を自動で突き合わせてくれるのが実用上強い。

## 統計

- 新規仕様5本(bank / bank_impl / bank_refines / bank_system / examples)、
  リポジトリの proved 仕様は計13本(buggy サンプル2本を除く全部)
- 新規バグ2件(BUG16/17)はいずれも生成系・橋渡し系。検証コア(BMC /
  induction / refine / compose の意味論)の欠陥は今回ゼロ
