# FSL v3.1 — 非機能要件(NFR)の取り扱い 設計

結論: **NFR の過半は既存カーネルで扱え、時間(SLA/タイムアウト)は離散時刻
構文の追加で扱える。確率・パーセンタイル・実時間は対象外**(正直な線引き)。
カーネル意味論は変更しない(時間構文も方言展開)。

## 1. NFR カテゴリ → FSL 対応表(本設計の全体像)

| NFR カテゴリ | 扱い | 機構 |
|---|---|---|
| セキュリティ/権限(「X できるのは管理者のみ」) | **今日から可** | ロール状態 + requires + invariant(イディオム化) |
| 監査/コンプライアンス(「全操作が記録される」) | **今日から可** | bank_system の監査パターン(横断 invariant) |
| 容量/上限(「キューは N まで」「同時 M 件」) | **今日から可** | 有界型・Seq 容量・count invariant |
| 信頼性の挙動(フェイルオーバー・縮退・復旧) | **今日から可** | 故障注入アクション + モード状態 + 復旧 leadsTo(イディオム化) |
| 性能/SLA(「K tick 以内に完了」)・タイムアウト | **本設計で追加** | `time` ブロック(離散時刻)+ `deadline` |
| スループット率・99.9%・パーセンタイル・実時間(ms) | **対象外** | 確率/定量意味論が必要(PRISM 等の領域)。文書に書く |
| ユーザビリティ・保守性 | 対象外 | 形式化対象外 |

## 2. 実証スパイク(2026-06-12、無修正カーネル)

「ワーカー1・処理2tick・リクエスト2件、SLA: 受理から4tick以内に完了」を
手書きカーネルで構成(tick アクション+age カウンタ+urgency 規律):

- **BMC**: verified(SLA は深さ内で成立)
- **負例**(urgency を外す= tick がいつでも可)→ `violated` /
  `submit → tick×5` の**飢餓トレース** + `requirement: NFR-1(原文)`
- **帰納証明**: 補助 invariant 6本(構造3: 排他・serving⇒pending・
  busy⇒serving / **時間予算3**: `age[serving] + busy <= 4`、待機者の
  予算、サービス開始前は age=0)を CTI 4ラウンドで導出して **proved**

知見: (a) SLA は安全性(age 上限 invariant)として検査できる。
(b) **urgency 規律が本質** — 「緊急アクションが enabled の間は時間が進まない」
を tick のガードに織り込まないと、インターリービングが常に飢餓反例を作る。
(c) BMC 検査は即動く。帰納証明は時間予算 invariant の階梯が必要で、
未時間化仕様(1ラウンド収束)より重い(4ラウンド)— 既定は BMC、証明は
オプトインと位置づける。

## 3. 構文(`requirements` 方言に追加)

```fsl
requirements OrderProcessingReq {
  ...型・state・init・requirement...

  time {
    urgent start, finish                       // enabled の間 tick を禁止
    age waitAge[r: Req] while pending[r]       // tick で +1、条件偽で 0 リセット
    age idleAge while queue.size() == 0        // スカラ形も可
  }

  requirement NFR-1 "受理されたリクエストは4tick以内に完了する" {
    deadline waitAge <= 4
  }
}
```

### 3.1 展開規則(すべて既存カーネル構文へ)

`time` ブロック(`requirements` 内に高々1つ):

1. `age m[x: T] while P` →
   - 上限 `cap = max(その age を参照する deadline の K) + 1`(参照する
     deadline が無ければ type エラー「unused age」)
   - `type _AgeM = 0..cap` 相当のドメイン + `state { m: Map<T, _AgeM> }` +
     init 0(スカラ形は `m: _AgeM`)
2. `urgent a, b, ...` → 列挙された(展開後の)アクション名を検証
   (branches 分割前の名前で書く: `urgent submit` は分割後の全分岐に適用)。
3. tick アクションを自動生成:
   ```
   action tick() {
     requires not (exists <urgent各アクションの全パラメータ束縛> { その requires 連言 })
     forall x: T { if P { if m[x] < cap { m[x] = m[x] + 1 } } else { m[x] = 0 } }
     ...全 age について同様...
   }
   ```
   urgent の requires が is-束縛を含む場合もそのまま exists 内に埋め込める
   (カーネル式)。`tick` という名前のユーザーアクションが既にあれば type エラー。
4. `deadline m <= K`(requirement 内)→ meta 付き invariant
   `forall x: T { m[x] <= K }`(スカラは `m <= K`)。
   deadline は time ブロックで宣言された age のみ参照可。

### 3.2 意味論ノート(ドキュメントに明記すること)

- tick は他のアクションと同じ1ステップ。「K tick 以内」= 「P が連続して
  成立する間に tick は高々 K 回」。
- urgency は**モデリング上の前提**(「システムは暇なときに仕事を先延ばし
  しない」)。urgent を指定しなければ大半の deadline は飢餓反例で落ちる —
  それは検査が正しく「スケジューリング前提が無い」ことを指摘している。
- deadline 違反の反例トレースには tick が並ぶ(待ち時間が見える)。
- 帰納証明には時間予算の補助 invariant(`age + 残り作業 <= K` 型)が
  必要になることが多い。CTI から導出する(§2 の実例を examples に置く)。
- deadlock 検査との関係: tick が requires を持つため「全 urgent が
  disabled かつ時間も進めない」状態は deadlock として検出される(正しい)。

## 4. 既存カーネルで足りる NFR のイディオム化(ドキュメントのみ)

LANGUAGE.md / skills に「NFR の書き方」節を追加:

- **権限**: `requires role[u] == Admin`、invariant
  `forall x { sensitive_done[x] => done_by_admin[x] }`(ゴースト)
- **監査完全性**: bank_system パターン(`audit.balance == ... + withdrawn`)
- **容量**: 型境界 + `requires q.size() < CAP`(枯渇時の挙動も action で明示)
- **信頼性の挙動**: `action crash() { mode = Degraded }` 等の故障注入 +
  `invariant DegradedRefusesWrites` + `fair action recover` +
  `leadsTo CrashRecovers { mode == Degraded ~> mode == Normal }`

## 5. 実装計画

1. `expand_requirements` に time/deadline/urgent を追加(§3.1)。
2. テスト(tests/test_nfr.py): §2 のスパイクを方言で書いた fixture が
   (a) BMC verified、(b) urgent を外した変種が violated + 飢餓トレース +
   requirement、(c) 補助 invariant を足した版が proved、(d) unused age /
   未知 urgent / tick 名衝突 / time ブロック重複の type エラー、
   (e) 既存全テスト不変。
3. examples/nfr/: 手書きカーネル版(証明済み・補助 invariant 込み)と
   方言版を並置 + README。
4. LANGUAGE.md(§13 に time/deadline、新節「NFR の書き方」)、
   skills/fsl(SKILL.md の規則+reference.md)、DOGFOOD-5.md(本スパイクの
   記録)。
