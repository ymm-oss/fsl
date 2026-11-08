# FSL 5 requirements in English and Japanese — leave requests

One small requirements model written twice in FSL 5: once with the English
spellings and once with the Japanese spellings. The two files are the same
model. Only keywords, particles and (through a `glossary`) the names a reader
sees differ; the canonical names behind them are the same, so both files lower
to the same checked model and give the same verification results.

| File | Spelling |
|---|---|
| [`leave_request.en.fsl`](leave_request.en.fsl) | English keywords, English names |
| [`leave_request.ja.fsl`](leave_request.ja.fsl) | Japanese keywords and particles (`が` / `を` / `で` / `は`), Japanese aliases from a `用語集` (glossary) |

The domain: an employee submits a leave request for some days. A one-day leave
is approved by the system; a longer one is approved or rejected by a manager.
An approved leave is taken only after a manager has arranged a substitute, or
it is cancelled.

What each file shows:

- the `fsl 5` header on the first line;
- an `entity` with fields and `lifecycle` paths, `actor`s, `concept`s and
  `event`s (an event with an input, a same-stage event with `set`);
- `rule` with `allow … only when` and `never … when`, and a `decide` table;
- named `always` invariants (`DaysInRange`, `TakenHasSubstitute`,
  `NoEarlySubstitute`, `RejectedIsLong`) and `enters … only from` transition
  properties (`ApprovedFromSubmitted`, `TakenFromApproved`). The lifecycle
  already implies the two transition properties, so `check` reports each as
  `implied_by_lifecycle` with severity `info`; they are kept so that each step
  stays a checked property;
- a `goal` (reachability) and two `policy` declarations (liveness: every
  submitted leave is eventually decided, every approved leave is eventually
  taken or cancelled);
- acceptance `scenario`s and `forbidden … then rejected` traces that probe the
  `allow` and `never` rules and the decision table;
- requirement IDs in the canonical form that `fslc lint` accepts
  (`REQ-LEAVE-001`, `AC-LEAVE-001`, `FB-LEAVE-001`, `GOAL-LEAVE-001`,
  `POL-LEAVE-001`).

## How to run

```bash
fslc check  examples/fsl5/leave_request.en.fsl
fslc lint   examples/fsl5/leave_request.en.fsl
fslc verify examples/fsl5/leave_request.en.fsl --depth 8
fslc verify examples/fsl5/leave_request.en.fsl --engine induction
fslc mutate examples/fsl5/leave_request.en.fsl
```

Run the same commands on `leave_request.ja.fsl`. Both files check, lint with
no finding, verify to depth 8, and report `proved` with `--engine induction`,
whose note reads "invariants proved for all depths; leadsTo checked to depth 8
only": the two policies are liveness properties, checked to the bounded depth
and not proved.

`fslc fmt --lang en --names canonical examples/fsl5/leave_request.ja.fsl`
respells the Japanese file with English keywords and canonical names; the
result differs from `leave_request.en.fsl` only in the comments, the
`glossary` block and the quoted requirement texts. `fslc kernel` gives the same Kernel JSON for both
files apart from source locations and those requirement texts. See the
glossary section of [LANGUAGE.md](../../docs/manual/LANGUAGE.md#fsl5-requirements)
for the Japanese spellings.

Some words stay in English in the Japanese file, because the keyword table
(`rust/fsl-syntax/src/keywords.rs`) has no Japanese spelling for them: `not`,
`Bool`, `true` and `false`. The spec name, the event names (`submit`,
`approve`, …), the property labels (`DaysInRange`, …), the requirement IDs and
the scenario instance labels (`l1`, `l2`) also stay canonical, as the manual
recommends for names that cross into code.

## 日本語

FSL 5 の要件を、英語の綴りと日本語の綴りで一度ずつ書いた例です。二つのファイルは
同じモデルです。違うのはキーワード、助詞、そして用語集(`用語集`)で付けた読み手向けの
名前だけで、その裏の正規名は同じです。どちらも同じ検査済みモデルになり、同じ検証結果を
出します。

題材は休暇申請です。従業員が日数を付けて休暇を申請します。1 日の休暇はシステムが承認し、
それより長い休暇は上長が承認または却下します。承認された休暇は、上長が代行を手配して
から取得するか、取り消します。

- `leave_request.en.fsl`: 英語のキーワードと名前
- `leave_request.ja.fsl`: 日本語のキーワードと助詞(`が` / `を` / `で` / `は`)、
  用語集による日本語の別名

実行方法は上の「How to run」と同じで、ファイル名を `leave_request.ja.fsl` に替えます。
日本語ファイルを `fslc fmt --lang en --names canonical examples/fsl5/leave_request.ja.fsl`
で英語のキーワードと正規名に綴り直すと、英語ファイルとの違いはコメント、用語集、
引用した要件の原文(日本語ファイルでは日本語)だけになります。`fslc kernel` の出力は、
綴り直さなくても、ソース位置と要件の原文を除いて一致します。
二つの方針(policy)は活性(liveness)の性質で、`--engine induction` でも
有界の深さまでの確認であり、証明ではありません。
日本語の綴りの一覧は [LANGUAGE.ja.md](../../docs/manual/LANGUAGE.ja.md#fsl5-requirements)
の「用語集と日本語の綴り」にあります。

日本語ファイルにも英語のまま残る語があります。キーワード表
(`rust/fsl-syntax/src/keywords.rs`)に日本語の綴りがない `not`、`Bool`、`true`、
`false` です。仕様名、出来事の名前(`submit`、`approve` など)、性質のラベル
(`DaysInRange` など)、要件 ID、シナリオのインスタンス名(`l1`、`l2`)も、コードとの
境界で追いやすいようにマニュアルの推奨に従って正規名のままにしています。
