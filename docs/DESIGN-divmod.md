# FSL — 整数除算 `/`・剰余 `%` 実装設計

動機: DOGFOOD-8 F-B。2次元データを単一キーに平坦化する定石(F-A)で
「セル → 軸の復元」(`c / SLOTS`, `c % SLOTS`)が書けず、境界の
ハードコードを強いられた。算術を `+ - * / %` で完結させる。

## 1. 構文

- 二項演算子 `/`(整数除算)と `%`(剰余)。優先順位は `*` と同じ
  product 段(左結合)。全式文脈で使用可。
- 注意(文書化のみ): `a//b` と空白なしで書くと `//` がコメント開始に
  字句解釈され `a` だけが残る(C と同じ罠)。演算子の両側に空白を書く。
  LANGUAGE.md §3 に1行注意を入れる。

## 2. 意味論(最重要 — 2評価器の完全一致)

1. **ゼロ除算は全域定義**: `a / 0 = 0`、`a % 0 = 0`。
   - Z3 の Int div/mod はゼロ除算が未解釈(モデル依存)なので、符号化では
     `If(b == 0, 0, div(a,b))` / `If(b == 0, 0, mod(a,b))` と**明示的に固定**する。
     これで Z3 側と具象側が必ず一致し、oracle/BFS も安全。
2. **アクション文脈では暗黙の partial_op 検査**: 本体・requires・ensures に
   現れた `/`・`%` について、その遷移で `除数 != 0` を検査(Seq の
   pop/head/at と同じ機構・パス条件考慮・`violation_kind: "partial_op"`、
   invariant 名 `_partial_<action>`、hint「guard the division: requires y != 0」)。
   /0=0 と定義しても**黙って 0 に頼る仕様は violated になる**(G5)。
3. **性質文脈(invariant/reachable/leadsTo/写像式)では検査なし**:
   /0 は 0 に評価される(全域なので不定値ではない)。ガードイディオム
   `y != 0 => P(x / y)` を推奨として文書化。
4. **負数は SMT-LIB(Euclidean)に従う**: `b != 0` のとき
   `a = b * (a / b) + (a % b)` かつ `0 <= a % b < |b|`。
   - Z3 の Int div/mod はこの定義(そのまま使用)。
   - **具象評価器(runtime)は Python の `//`/`%`(floor)と b<0 で食い違う**ため、
     明示式で実装する:
     ```python
     def _euc_div(a, b):
         if b == 0: return 0
         q = a // b
         if a - b * q < 0: q += 1   # Python floor → Euclidean 補正(b<0 のとき)
         return q
     def _euc_mod(a, b):
         if b == 0: return 0
         r = a % b
         if r < 0: r += abs(b)      # 常に 0 <= r < |b|
         return r
     ```
   - 典型仕様(非負ドメイン)では Python と同値だが、負数でも両評価器が
     一致することを**プロパティテストで固定**する(§4)。

## 3. 波及

- grammar.py: product 段に `/` `%`。AST `("bin","/",a,b)` / `("bin","%",a,b)`。
- bmc.py: eval_expr の bin に div/mod(§2.1 の ite 固定)。partial_op 収集
  (既存の Seq 用機構に除数式を追加。パス条件は既存と同じ)。
- runtime.py: `_euc_div`/`_euc_mod`(§2.4)。enabled() の短絡(BUG-020 修正)
  はそのまま効く(requires 成立まで本体未評価)。
- 型: 結果は Int。有界変数への代入は既存の自動境界検査がそのまま守る。
- dialects/refine/compose: 式機構共通のため追加作業なし(回帰のみ)。

## 4. テスト

1. 基本: 商・剰余を使う仕様が verified/proved(例: 平坦化セルから
   `c / SLOTS == r` で「室 r が満杯」を書く会議室仕様 — DOGFOOD-8 の
   盲検仕様の改良版を fixture 化)。
2. **2評価器一致(最重要)**: a ∈ [-7..7] × b ∈ [-3..3](b=0 含む)の全組で
   Z3 符号化の評価値と runtime の `_euc_div`/`_euc_mod` が一致
   (小さい spec を介すか評価器を直接比較)。witness 再生の既存差分テストにも
   除算入り仕様を1本追加。
3. partial_op: 無ガード `x = 10 / d`(d が 0 になり得る)→ violated/partial_op。
   `requires d != 0` 付き → verified。if ガード形も。性質文脈の /0 は検査なし
   (0 に評価される)ことの確認。
4. Euclidean: 負数ケースのピン(例: `-7 / 2 == -4`? → Euclidean では
   -7 = 2*(-4)+1 なので q=-4, r=1。`-7 % 2 == 1`。`7 / -2`: 7 = -2*(-3)+1 →
   q=-3, r=1)。誤実装(truncation/floor)なら落ちる値を選ぶ。
5. ゼロ除算の全域性: invariant 内 `x / 0 == 0` が成り立つ(意図的な仕様で確認)。
6. 既存全テスト(301 passed / 69 skipped)無修正 green。

## 5. ドキュメント

- LANGUAGE.md §3 演算子行 + `a//b` コメント罠の注意 + §6 自動チェック表に
  除算の partial_op を追記。skills/fsl(SKILL.md の partial_op 規則行・
  reference.md §3/§6)も同期。2次元データの定石(F-A)は別途
  イディオム節として追加(本設計の受け入れ後)。
