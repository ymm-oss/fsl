# FSL 5 in English and Japanese — leave requests

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

## Business and design layers

The same leave requests are also written as a business layer above the
requirements and a design layer below them. The three layers are connected by
refinement: each lower layer refines the one above it, checked with
`fslc refine` and a mapping file.

| File | Layer | Form |
|---|---|---|
| [`leave_flow_asis.en.fsl`](leave_flow_asis.en.fsl) / [`.ja.fsl`](leave_flow_asis.ja.fsl) | business, As-Is | `business` with `process` / `transition`, `control`, `policy`, `goal`, `kpi` |
| [`leave_flow_tobe.en.fsl`](leave_flow_tobe.en.fsl) / [`.ja.fsl`](leave_flow_tobe.ja.fsl) | business, To-Be | the same forms |
| [`leave_request.en.fsl`](leave_request.en.fsl) / [`.ja.fsl`](leave_request.ja.fsl) | requirements | `requirements` (above) |
| [`leave_design.en.fsl`](leave_design.en.fsl) / [`.ja.fsl`](leave_design.ja.fsl) | design | kernel `spec` with `struct`, `def`, `fair action`, `invariant`, `trans`, `leadsTo`, `reachable`, `terminal` |
| [`leave_flow_tobe_refines_asis.fsl`](leave_flow_tobe_refines_asis.fsl) | To-Be refines As-Is | `refinement` |
| [`leave_request_refines_tobe.fsl`](leave_request_refines_tobe.fsl) | requirements refine To-Be | `refinement` |
| [`leave_design_refines_request.fsl`](leave_design_refines_request.fsl) | design refines requirements | `refinement` |

- **As-Is**: today a manager decides every request. The flow keeps one
  control, `CTRL-LEAVEFLOW-001` (leave is taken or cancelled only after an
  approval), as a no-bypass `policy`, and states that every approved leave is
  eventually taken or cancelled.
- **To-Be**: the system approves a short leave instead of the manager, and the
  flow adds a second control, `CTRL-LEAVEFLOW-002`, with the progress policy
  that every submitted leave is eventually decided. The refinement to the As-Is
  maps the system's approval to the manager's, so the reform keeps the first
  control.
- **Requirements**: `leave_request` decides *which* leave the system approves
  (the decision table) and adds days and the substitute. Its events have the
  names of the To-Be transitions; arranging a substitute is a stutter.
- **Design**: one action files a request into an automatic queue or a manager
  queue by its length; the requirements see both queues as `Submitted`. A
  `trans` property and a `def` list the arrows a ticket may take.

FSL 5 has no separate design dialect: a design spec is a kernel `spec` headed
`fsl 5`. The Japanese keywords and the glossary are read only in an FSL 5
`requirements` document. In a `business` document, a kernel `spec` and a
`refinement` file the keywords are English and every name is ASCII (a non-ASCII
name is `canonical_name_not_ascii`). The `.ja.fsl` files of these two layers are
therefore the same model as their `.en.fsl` files with Japanese comments and
quoted texts; that is as far as the Japanese spelling goes outside the
requirements layer. One mapping file serves both spellings, because the
canonical names are the same.

```bash
cd examples/fsl5
fslc verify leave_flow_asis.en.fsl --engine induction
fslc verify leave_flow_tobe.en.fsl --engine induction
fslc verify leave_design.en.fsl --engine induction
fslc refine leave_flow_tobe.en.fsl leave_flow_asis.en.fsl leave_flow_tobe_refines_asis.fsl --depth 8
fslc refine leave_request.en.fsl leave_flow_tobe.en.fsl leave_request_refines_tobe.fsl --depth 8
fslc refine leave_design.en.fsl leave_request.en.fsl leave_design_refines_request.fsl --depth 8
```

Each spec checks, lints with no finding and verifies to depth 8; `--engine
induction` reports `proved`, with the note "invariants proved for all depths;
leadsTo checked to depth 8 only". Each `refine` reports `refines`, and the same
with the `.ja.fsl` files in place of the `.en.fsl` ones.

`refine` carries safety only. The progress policies of an upper layer are not
pulled through these mappings: `preserve progress { respond <name> by … }`
reads an identifier, and a policy ID such as `POL-LEAVE-001` is not one. Each
layer states and verifies its own progress instead. The requirements layer
refines the To-Be through a mapping file rather than an inline `implements`, so
that the requirements files above stay as they are.

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

### 業務層と設計層

同じ休暇申請を、要件の上の業務層と、要件の下の設計層でも書いています。三つの層は
詳細化(refinement)でつながっています。下の層が上の層を詳細化することを、
対応付けのファイルと `fslc refine` で確かめます。

- `leave_flow_asis.{en,ja}.fsl`: 業務層の As-Is。現在は上長がすべての申請を決める。
  統制 `CTRL-LEAVEFLOW-001`(承認の後にだけ休暇を取得または取消する)を、迂回を
  禁じる `policy` として持ち、承認された休暇はいずれ取得か取消になると述べる。
- `leave_flow_tobe.{en,ja}.fsl`: 業務層の To-Be。短い休暇は上長に代わってシステムが
  承認し、二つ目の統制 `CTRL-LEAVEFLOW-002` として、申請はいずれ決まるという進行の
  方針を加える。As-Is への対応付けでシステムの承認を上長の承認に写すので、
  見直し後も一つ目の統制が保たれる。
- `leave_request.{en,ja}.fsl`: 要件層(上記)。どの休暇をシステムが承認するかを
  決定表で決め、日数と代行を加える。出来事の名前は To-Be の遷移と同じで、
  代行の手配は業務層から見ると何も変えない一歩(stutter)になる。
- `leave_design.{en,ja}.fsl`: 設計層。申請を長さで自動と上長の二つの待ち行列に
  振り分け、要件からはどちらも `Submitted` に見える。`trans` と `def` で、
  チケットが進める矢印を列挙する。
- `leave_flow_tobe_refines_asis.fsl`、`leave_request_refines_tobe.fsl`、
  `leave_design_refines_request.fsl`: 層の間の対応付け。

FSL 5 には設計層専用のダイアレクトがありません。設計の仕様は `fsl 5` を先頭に置いた
カーネルの `spec` です。日本語のキーワードと用語集を読むのは FSL 5 の `requirements`
文書だけで、`business`、カーネルの `spec`、`refinement` ではキーワードは英語、名前は
ASCII です(ASCII 以外の名前は `canonical_name_not_ascii`)。そのため、この二つの層の
`.ja.fsl` は、`.en.fsl` と同じモデルに日本語のコメントと引用文を付けたものです。
要件層の外で日本語の綴りにできるのはここまでです。正規名が同じなので、対応付けの
ファイルは一つで両方の綴りに使えます。

実行方法は上の「Business and design layers」のコマンドと同じです。どの仕様も check、
lint(指摘なし)、深さ 8 の verify を通り、`--engine induction` は `proved` を
返します(leadsTo は深さ 8 までの確認という注記付き)。どの
`refine` も `refines` を返し、`.en.fsl` を `.ja.fsl` に替えても同じです。

`refine` が運ぶのは安全性だけです。上の層の進行の方針は、この対応付けでは引き継ぎ
ません。`preserve progress { respond <名前> by … }` は識別子を読みますが、
`POL-LEAVE-001` のような方針の ID は識別子ではないためです。代わりに各層が自分の
進行の性質を書いて検証します。要件層は、上の要件ファイルを変えないよう、インラインの
`implements` ではなく対応付けのファイルで To-Be を詳細化します。
