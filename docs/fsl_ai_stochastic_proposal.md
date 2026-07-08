# FSL AI / Stochastic Output Specification Proposal

作成日: 2026-07-07
対象: FSL / `fslc` の拡張提案
提案名: **fsl-ai / fsl-stochastic dialect**
目的: AIコンポーネント、統計的出力、プロンプト・モデル・RAG・tool call・評価器・本番観測・AI migration を、FSLの検証・評価・修復ループで扱えるようにする。

Phase 1 の採用設計は [`DESIGN-ai-hard.md`](DESIGN-ai-hard.md) に切り出した。
この提案書は Phase 2 以降（dataset / slice / statistical eval、AI migration、
observed drift、multi-environment統合）も含む全体案として残す。

---

## 1. 要約

FSLに、AIのような統計的・非決定的な出力を記述するための dialect を追加する。

この提案では、AIを通常の決定的な関数として扱わない。AIを次の性質を持つコンポーネントとして扱う。

```text
AI component
  = nondeterministic output producer
  + probabilistic / statistical behavior
  + hard safety contract
  + evaluator-backed quality contract
  + observed runtime behavior
  + fallback / human-review transition
  + versioned artifact in multi-environment compatibility
```

FSLの既存の中心は、state / action / invariant / trans / leadsTo / refinement / Monitor / replay / testgen / JSON repair protocol である。AI記述はこのkernelを置き換えるものではない。`fsl-ai` はAI向けの表層dialectとして導入し、以下へ展開する。

```text
fsl-ai
  ai_component / ai_action / ai_contract / evaluator / dataset / failure_mode / ai_migration
      ↓
fsl-stochastic
  statistical_property / observed_property / estimate / slice / confidence interval / drift
      ↓
FSL kernel + statistical evaluator + runtime replay
```

これにより、以下が可能になる。

- AI出力のhard constraintsを検証する。
- JSON schema、tool schema、権限境界、PII禁止、引用範囲などを契約化する。
- 評価セットとsliceに対して、信頼区間つきの品質条件を記述する。
- hallucination、wrong tool call、missed escalation、prompt injection following などの失敗モードを仕様化する。
- model / prompt / retriever / tool schema の変更を migration として扱い、no-regressionを検証する。
- AI evaluator自体の校正・信頼性を仕様化する。
- 本番ログ上のdrift、失敗率、観測されたdeprecated usageを検出する。
- AI componentをDB / server / mobile / API / feature flag と同じmulti-environment compatibility modelに載せる。
- AIがFSL仕様・migration・PR・テストを生成する場合、そのAI生成物も評価対象にする。

---

## 2. 背景

FSLはAI-nativeな形式仕様言語として設計されており、AIが仕様を書き、`fslc` が検証し、JSON結果をAIが読んで修復する write → verify → repair loop を前提にしている。公開READMEでは、`fslc` がLark + Z3によるBMCおよびk-inductionを行い、結果を機械可読JSONとして返すこと、またscenarios / testgen / replay へつながることが説明されている。

FSLの三層設計では、consulting / requirements / design のdialectをshared kernelへ展開し、refinement chainで接続する。docsの設計説明では、dialect frontendがAST transformでshared kernelへ展開され、BMC、k-induction、scenarios、Monitor、refineが適用される構成が示されている。

この構造は、AI記述にもそのまま適用できる。

```text
AI-specific concepts
  model / prompt / retriever / tool / evaluator / dataset / confidence / fallback
      ↓ dialect expansion
FSL shared kernel
  state / action / invariant / trans / reachable / leadsTo / refinement / compose
      ↓ execution/evidence layer
statistical evaluation / runtime replay / drift monitoring
```

AIは形式手法と相性が悪い対象に見えるが、それはAIを完全に決定的な関数として証明しようとする場合である。FSLが扱うべきなのは、AI内部の完全証明ではなく、AIコンポーネントの外部契約・評価条件・失敗時遷移・観測可能性である。

---

## 3. 問題設定

AIを含むアプリケーションでは、従来のソフトウェア仕様だけでは足りない。

通常のコードなら、次のように考えられる。

```text
input + deterministic code -> output
```

しかしAIでは、実際には次のようになる。

```text
input
+ system instruction
+ prompt
+ model version
+ decoding parameters
+ retrieval result
+ tool schemas
+ policy layer
+ runtime context
+ evaluator / guardrail
    -> distribution over outputs and actions
```

このため、AIシステムでは以下が壊れやすい。

- 同じ入力でも異なる出力が返る。
- 出力はJSON schemaに合うが意味的に誤っている。
- 引用はあるがsourceが主張を支持していない。
- tool callの形式は合っているが実行条件を満たしていない。
- prompt更新で特定sliceだけ品質が低下する。
- retriever index更新で必要文書が取れなくなる。
- evaluatorが誤判定する。
- 低confidenceなのに自動実行される。
- AIが本来は提案だけすべき操作を実行してしまう。
- 本番入力分布が評価セットからdriftする。
- model migration後に拒否率や幻覚率が変わる。

したがって、FSLには以下を記述できる必要がある。

```text
hard constraints:
  必ず守るべき構造・安全・権限制約

statistical constraints:
  評価分布上で統計的に満たすべき品質条件

observed constraints:
  本番ログ上で監視すべきdrift・失敗率・回帰

fallback constraints:
  不確実性や危険条件がある場合に安全側へ遷移する規則

migration constraints:
  model / prompt / retriever / tool schema の変更で壊してはいけない性質
```

---

## 4. 設計原則

### 4.1 AIを決定的関数として扱わない

AI出力は、決定的な `input -> output` ではなく、候補集合または分布として扱う。

```text
AI output = possible values + probabilities/estimates + evidence + fallback behavior
```

FSL上では、hard constraintsは全出力候補に対する安全条件として扱う。一方、品質は統計的評価として扱う。

### 4.2 hard / statistical / observed を分離する

AI仕様で最も重要なのは、性質の強さを混同しないことである。

| 種類 | 意味 | 例 | 検証方法 |
|---|---|---|---|
| `hard` | すべての許容出力で守るべき制約 | JSON schema、tool schema、PII禁止、権限 | static check / runtime guard / invariant |
| `statistical` | 評価分布上で満たすべき品質条件 | groundedness、accuracy、hallucination rate | eval runner / confidence interval |
| `observed` | 本番ログ上で監視する性質 | drift、failure rate、slice劣化 | replay / telemetry aggregation |
| `fallback` | 不確実・危険時の安全側遷移 | human review、refusal、追加検索 | transition / invariant |

`hard` は通常のFSL propertyに近い。`statistical` は証明ではなく、評価データ上の統計的支持を返す。`observed` は運用証拠であり、形式証明ではない。

### 4.3 統計的結果を「証明」と呼ばない

AI品質評価は、通常のinvariant proofとは違う。

```text
invariant result:
  proved / violated / unknown

statistical result:
  statistically_supported / statistically_unsupported / insufficient_samples / inconclusive
```

たとえば `groundedness >= 0.97` は、数学的に証明されるのではなく、指定dataset / slice / evaluator / confidence levelのもとで統計的に支持される。

### 4.4 evaluatorも第一級にする

AI出力を評価するevaluatorもAIである可能性がある。そのため、評価器自体の校正・信頼性を仕様化する。

```text
AI output quality depends on evaluator quality.
```

評価器を暗黙の外部関数にすると、AI評価全体が不透明になる。

### 4.5 AI変更をmigrationとして扱う

AIシステムでは、コード以外も挙動を変える。

```text
model version
prompt version
system instruction
retriever index
tool schema
evaluator rubric
safety policy
temperature / decoding parameter
```

これらの変更は、DB schema migrationと同様にno-regression / compatibility checkの対象にする。

### 4.6 versionではなくcapabilityを中心にする

`model=gpt-x` や `prompt=v8` というラベルだけでは安全性は分からない。重要なのは、そのartifactが何を提供し、何を要求するかである。

```text
provided capabilities:
  emits AnswerSchemaV2
  calls RefundPaymentV2
  cites RetrievedSources
  supports JapaneseRefundPolicy

required capabilities:
  server accepts RefundPaymentV2
  mobile accepts AnswerSchemaV2
  retriever provides ApprovedSupportDocs
  evaluator supports GroundednessV3
```

DB / API / mobile / server と同じmulti-environment compatibility modelにAI componentを載せる。

### 4.7 AIはactorであり、状態遷移を起こす

AIは単にテキストを返すだけではない。tool call、workflow transition、approval request、ticket update、refund、email draftなどの状態遷移を発生させる。

したがって、AI出力はFSLのactionやtransitionと接続する必要がある。

---

## 5. 提案するdialect構成

### 5.1 `fsl-ai`

AIアプリケーション向けの表層dialect。

主な概念:

```text
ai_component
ai_action
ai_contract
ai_migration
prompt
model
retriever
tool
evaluator
failure_mode
fallback
authority
human_review
```

### 5.2 `fsl-stochastic`

AIに限らず、統計的評価・観測的性質を扱う汎用層。

主な概念:

```text
dataset
population
slice
metric
estimate
statistical_property
observed_property
confidence_interval
regression_check
drift_check
```

### 5.3 展開方針

```text
fsl-ai syntax
  ↓
ai semantic IR
  ↓
fsl-stochastic + FSL kernel
  ↓
SMT/BMC/k-induction for hard properties
statistical evaluator for statistical properties
runtime replay / monitor for observed properties
JSON finding for AI repair
```

FSL kernelに確率計算を直接追加するのではない。kernelには、hard constraints、state transition、fallback invariant、tool precondition、environment compatibilityを渡す。統計的評価は周辺runnerが実行し、結果をFSLの診断JSON形式に合わせて返す。

---

## 6. 中核概念

### 6.1 `ai_component`

AIコンポーネントを定義する。

```fsl
ai_component SupportAnswerAgent {
  model gpt_5_5
  prompt support_answer_prompt_v7
  retriever support_docs_index_v12
  tools [SearchDocs, CreateTicket]
  temperature 0.2

  input SupportTicket
  output Answer
}
```

`ai_component` は、モデル名だけでなく、prompt、retriever、tools、decoding parameters、input/output schemaを持つ。

### 6.2 `ai_action`

AI出力が状態遷移を起こす場合に使う。

```fsl
ai_action ClassifyTicket {
  input ticket: SupportTicket
  output category: TicketCategory
  output confidence: Float

  transitions {
    if category == Urgent -> Escalated
    if confidence < 0.7 -> HumanReviewPending
    otherwise -> AutoAnswerCandidate
  }
}
```

AIの分類結果がworkflowを変えるなら、AI出力は単なる値ではなくtransition guardである。

### 6.3 `ai_contract`

AIコンポーネントの契約を定義する。

```fsl
ai_contract SupportAnswerContract {
  target SupportAnswerAgent

  hard {
    output.language == input.language
    output.citations subset retrieved.sources
    output.tool_calls conform_to tool_schemas
    output.must_not_expose pii
    output.must_not_invent_policy
  }

  statistical {
    dataset SupportEvalV3
    evaluator SupportAnswerJudge
    confidence 0.95

    require ci_lower(metric.groundedness, 0.95) >= 0.97
    require ci_upper(metric.hallucination_rate, 0.95) <= 0.01
    require ci_lower(metric.helpfulness, 0.95) >= 0.82
  }

  fallback {
    when output.confidence < 0.7 require HumanReview
    when evaluator.confidence < 0.8 require HumanReview
    when retrieved.sources.count == 0 require RefuseWithReason
  }
}
```

### 6.4 `dataset` / `population` / `slice`

統計的性質の対象分布を明示する。

```fsl
dataset SupportEvalV3 {
  source "s3://evals/support/v3.jsonl"

  population {
    language in ["ja", "en"]
    channel in ["email", "chat"]
    ticket_type in ["refund", "bug", "account", "urgent"]
  }

  slice UrgentTickets {
    ticket_type == "urgent"
  }

  slice JapaneseRefundTickets {
    language == "ja"
    ticket_type == "refund"
  }
}
```

平均値だけでは危険である。FSLではsliceを第一級にし、重要な利用者群・業務カテゴリ・言語・リスク領域ごとに閾値を設定できるようにする。

### 6.5 `evaluator`

評価器を仕様化する。

```fsl
evaluator GroundednessJudge {
  input question: Question
  input answer: Answer
  input sources: Set<Source>
  output grounded: Bool
  output confidence: Float

  calibration {
    dataset HumanLabeledGroundednessV2
    require agreement_with_human >= 0.90
    require false_negative_rate <= 0.03
    require false_positive_rate <= 0.05
  }
}
```

評価器を指定せずに `groundedness` を測ると、何をもってgroundedとするかが曖昧になる。

### 6.6 `failure_mode`

AI固有の失敗モードを記述する。

```fsl
failure_mode Hallucination {
  condition output.claims not_supported_by retrieved.sources
  severity high
}

failure_mode WrongToolCall {
  condition output.tool_call.args violates tool.schema
  severity high
}

failure_mode PromptInjectionFollowing {
  condition output.follows_untrusted_instruction
  severity critical
}

failure_mode MissedEscalation {
  condition input.urgent && output.escalation_required == false
  severity critical
}
```

これを統計的propertyと接続する。

```fsl
statistical_property FailureBounds {
  target SupportAnswerAgent
  dataset SupportEvalV3
  evaluator SupportAnswerJudge

  require P(Hallucination) <= 0.01
  require P(WrongToolCall) <= 0.001
  require P(PromptInjectionFollowing) <= 0.0001
  require P(MissedEscalation | input.urgent) <= 0.005
}
```

### 6.7 `authority`

AIの権限境界を記述する。

```fsl
authority SupportAgentAuthority {
  target SupportAnswerAgent

  may_suggest [ReplyDraft, TicketCategory, HelpArticle]
  may_execute [SearchDocs, CreateDraft]
  requires_human_approval [SendEmail, RefundPayment, CloseTicket]
  forbidden [DeleteCustomerData, ChangeBillingPlan]
}
```

このauthorityはtool call検証に展開される。

```text
if tool_call in requires_human_approval:
  HumanApproval must precede execution

if tool_call in forbidden:
  violation
```

### 6.8 `ai_migration`

AI componentの変更をmigrationとして扱う。

```fsl
ai_migration PromptV7ToV8 {
  from SupportAnswerAgent {
    model gpt_5_5
    prompt support_answer_prompt_v7
    retriever support_docs_index_v12
  }

  to SupportAnswerAgent {
    model gpt_5_5
    prompt support_answer_prompt_v8
    retriever support_docs_index_v13
  }

  preserve {
    hard_contract SupportAnswerContract.hard

    no_regression {
      dataset SupportEvalV3
      metric groundedness drop <= 0.005
      metric hallucination_rate increase <= 0.002
      metric urgent_escalation_recall drop <= 0.001
    }
  }
}
```

DB schema migrationと同じ発想で、prompt、model、retriever、tool schema、evaluator rubricの変更を扱う。

---

## 7. hard constraintsの設計

AI出力に対して、まず検証すべきはhard constraintsである。

代表例:

```text
output schema validity
JSON schema conformance
tool schema conformance
citation source subset
PII exposure prohibition
authority boundary
human approval before irreversible action
no external ID fabrication
monetary amount bounds
allowed enum value
required fallback transition
```

例:

```fsl
ai_contract RefundAgentContract {
  target RefundAgent

  hard {
    output.tool_calls conform_to tool_schemas

    when tool_call.name == "RefundPayment" {
      require input.user_role == SupportAdmin
      require order.status == Paid
      require amount <= order.refundable_amount
      require HumanApproval before execution
    }

    forbidden tool_call.name == "DeleteCustomerData"
  }
}
```

展開先:

```text
hard contract
  -> action requires
  -> invariant
  -> trans
  -> runtime guard
  -> replay conformance rule
```

AIそのものが必ずこの制約を守ると仮定するのではなく、AI出力を受け取るruntime guard側でも検証する。AIが不正なtool callを出した場合は、実行せずに違反findingを返す。

---

## 8. statistical constraintsの設計

採用済みMVP設計は `docs/DESIGN-stochastic.md` に固定する。この提案書の
統計セクションはPhase 2以降を含む大きな案として残し、MVPでは
precomputed eval JSONL、Bernoulli/proportion metric、Wilson interval、
`formal_result: "not_run"` に限定する。

AI品質は統計的に扱う。

```fsl
statistical_property ClassifierQuality {
  target ClassifyTicket
  dataset SupportEvalV3
  evaluator TicketClassifierJudge
  confidence 0.95

  require ci_lower(metric.accuracy, 0.95) >= 0.92

  slice UrgentTickets {
    require ci_lower(metric.recall, 0.95) >= 0.98
  }

  slice JapaneseRefundTickets {
    require ci_lower(metric.accuracy, 0.95) >= 0.90
  }
}
```

### 8.1 点推定だけを禁止する

`accuracy >= 0.92` のような点推定のみの主張は危険である。原則として、FSLでは次を要求する。

```text
metric estimate
sample count
confidence level
confidence interval
slice definition
```

### 8.2 結果ステータス

統計的評価の結果は、通常のverifyとは分ける。

```text
statistically_supported
statistically_unsupported
insufficient_samples
inconclusive
evaluator_untrusted
dataset_invalid
slice_missing
```

### 8.3 サンプル不足をfailureにするか

重要sliceでは、サンプル不足自体をエラーにする。

```fsl
slice UrgentTickets {
  require min_samples >= 200
  require ci_lower(metric.recall, 0.95) >= 0.98
}
```

これにより、評価セットの穴を検出できる。

---

## 9. observed propertiesの設計

AIは評価セット上では良くても、本番でdriftする。したがって、本番観測を仕様に含める。

```fsl
observed_property SupportAgentOperationalQuality {
  target SupportAnswerAgent
  source production_logs
  window last_7_days

  require observed(metric.hallucination_rate) <= 0.015
  require drift(metric.refusal_rate) <= 0.02 compared_to previous_7_days
  require drift(input.category_distribution) <= 0.10 compared_to training_population

  slice JapaneseUsers {
    require observed(metric.helpfulness) >= 0.80
  }
}
```

これは形式証明ではない。runtime replay / telemetry aggregationの結果として扱う。

### 9.1 観測と仕様の不一致

AI componentでは、宣言されたcapabilityと観測が食い違うことがある。

例:

```text
prompt_v8 は RefundPayment toolを呼ばないと宣言されている
しかし本番ログでRefundPayment tool callが観測された
```

これは `observed_contract_violation` として返す。

---

## 10. fallback / human reviewの設計

AIを使うシステムでは、品質閾値を満たさない場合に安全側へ遷移することが重要である。

```fsl
fallback {
  when output.confidence < 0.7 require HumanReview
  when evaluator.confidence < 0.8 require HumanReview
  when input.category in [Legal, Medical, BillingDispute] require HumanReview
  when retrieved.sources.count == 0 require RefuseWithReason
  when tool_call.irreversible require HumanApproval
}
```

これをFSL kernel上では状態遷移として扱う。

```text
AICompleted
  -> AutoExecuteAllowed
  -> HumanReviewPending
  -> RefusedSafely
  -> NeedsMoreEvidence
```

不変条件:

```fsl
invariant CriticalCasesRequireHumanReview {
  input.category == Legal && output.confidence < 0.9
    -> workflow_state == HumanReviewPending
}
```

---

## 11. tool call安全性

AIエージェントでは、自然言語出力よりtool callの方が危険である。

対象例:

```text
send email
create ticket
refund payment
cancel order
modify database
create GitHub PR
change billing plan
schedule event
```

FSLではtoolを通常のactionと接続する。

```fsl
tool RefundPayment {
  input order_id: OrderId
  input amount: Money
  effect PaymentRefunded

  requires order.status == Paid
  requires amount <= order.refundable_amount
  requires not order.already_refunded
}

ai_contract RefundAgentContract {
  target RefundAgent

  hard {
    when tool_call.name == "RefundPayment" {
      require tool_call.args conform_to RefundPayment.input_schema
      require RefundPayment.requires
      require HumanApproval before execution
    }
  }
}
```

出力がtool schemaに合っていても、業務preconditionを満たさなければ実行してはいけない。

---

## 12. RAG / grounding の設計

RAGでは、retriever、source、answerを分けて扱う。

```fsl
retriever SupportDocsRetriever {
  index support_docs_index_v12

  input Query
  output RetrievedSources

  hard {
    output.sources all_from ApprovedSupportDocs
    output.sources.not_expired
  }

  statistical {
    dataset RetrievalEvalV3
    require ci_lower(metric.recall_at_5, 0.95) >= 0.92
    require ci_lower(metric.precision_at_5, 0.95) >= 0.85

    slice RefundPolicy {
      require ci_lower(metric.recall_at_5, 0.95) >= 0.97
    }
  }
}

ai_contract SupportAnswerContract {
  target SupportAnswerAgent

  hard {
    output.citations subset retrieved.sources
    output.claims supported_by retrieved.sources
  }
}
```

RAGの失敗は少なくとも3種類に分ける。

```text
retrieval_miss:
  必要なsourceが取得されない

answer_ungrounded:
  sourceはあるが、回答がsourceに基づかない

evaluator_error:
  評価器がgroundednessを誤判定する
```

findingにはこの切り分けを含める。

---

## 13. prompt injection / untrusted input

AI componentは信頼境界を持つ必要がある。

```fsl
trust_boundary SupportAnswerAgentBoundary {
  trusted [system_instruction, developer_prompt, approved_docs]
  untrusted [user_message, retrieved_external_web, email_body, ticket_description]
}

failure_mode PromptInjectionFollowing {
  condition output.follows_instruction_from untrusted
  severity critical
}

ai_contract InjectionResistance {
  target SupportAnswerAgent

  hard {
    output.must_not_follow untrusted.instructions_that_conflict_with trusted.instructions
  }

  statistical {
    dataset PromptInjectionEvalV2
    require ci_upper(metric.injection_success_rate, 0.95) <= 0.001
  }
}
```

hard propertyとして「untrusted instructionを絶対に無視する」と書いても、実モデルの完全保証はできない。したがって、実行時guard、評価セット、red-team dataset、observed attacksを組み合わせる。

---

## 14. AI migration

AI関連artifactの変更をmigrationとして扱う。

対象:

```text
model migration
prompt migration
retriever index migration
tool schema migration
evaluator migration
rubric migration
safety policy migration
decoding parameter migration
```

例:

```fsl
ai_migration ToolSchemaV2ToV3 {
  from RefundAgent {
    tools [RefundPayment_v2]
  }

  to RefundAgent {
    tools [RefundPayment_v3]
  }

  compatibility {
    require server accepts RefundPayment_v3
    require mobile output_schema supports RefundResult_v3
    require old traces replay against RefundPayment_v3_adapter
  }

  preserve {
    hard_contract RefundAgentContract.hard
    no_regression {
      dataset RefundAgentEvalV4
      metric wrong_tool_call_rate increase <= 0.001
      metric human_approval_bypass_rate increase == 0
    }
  }
}
```

これはDB migration提案と同じmulti-environment compatibilityへ接続できる。

---

## 15. multi-environment compatibilityとの接続

採用済みの接続先は `docs/DESIGN-db.md` の generic artifact capability
modelである。AI component用に別checkerを作らず、`dbsystem artifact` の
`requires` / `provides` capability profileとして、model / prompt /
retriever / tool schema / output schemaを同じenvironment snapshotで検査する。

AI componentは、server、mobile、DB、API contractと同じ環境artifactである。

```fsl
environment Production {
  server server_v3_2
  database schema_v21
  mobile ios_v2_4..ios_v3_0

  ai_component SupportAnswerAgent {
    model gpt_5_5
    prompt support_prompt_v8
    retriever support_docs_index_v14
    tools [CreateTicket_v3, RefundPayment_v2]
  }
}

compatibility AIEnvironmentCompatibility {
  require SupportAnswerAgent.tool_calls compatible_with server_v3_2.tool_api
  require SupportAnswerAgent.output_schema compatible_with ios_v2_4.expected_schema
  require SupportAnswerAgent.citations compatible_with support_docs_index_v14
}
```

検出したい事故:

```text
AIはRefundPayment_v3の引数形式でtool callを出すが、serverはv2しか受け取れない。
AIはAnswerSchemaV3を返すが、mobile v2.4はAnswerSchemaV2しか読めない。
AIは新しいpolicy documentを引用するが、retriever indexにまだ入っていない。
AIはDB schema_v22のfieldを前提に回答するが、本番DBはschema_v21である。
```

---

## 16. AIがFSLを書く場合の自己適用

FSLはAIが仕様を書くことを前提にしている。したがって、AI記述はアプリAIだけでなく、FSL生成AIにも適用できる。

```fsl
ai_component FslSpecGenerator {
  input RequirementsDoc
  output FslSpec

  hard {
    output.parses_successfully
    output.no_forbidden_constructs
    output.requirement_ids_preserved
  }

  statistical {
    dataset FslGenerationEvalV1
    require ci_lower(metric.compile_success_rate, 0.95) >= 0.98
    require ci_lower(metric.verification_pass_or_actionable_failure, 0.95) >= 0.95
    require ci_lower(metric.human_review_acceptance, 0.95) >= 0.85
  }
}
```

この自己適用により、AIによるFSL生成・反例修復・property追加・migration生成を継続的に評価できる。

---

## 17. JSON診断スキーマ

AI修復ループ向けに、結果は機械可読JSONで返す。

### 17.1 hard contract violation

```json
{
  "kind": "ai_hard_contract_violation",
  "target": "RefundAgent",
  "contract": "RefundAgentContract",
  "violation": "human_approval_required_before_irreversible_tool",
  "tool_call": {
    "name": "RefundPayment",
    "args": {
      "order_id": "O-123",
      "amount": 5000
    }
  },
  "witness": [
    "RefundPayment is irreversible",
    "tool_call emitted by RefundAgent",
    "HumanApproval state was not reached before execution"
  ],
  "repair_candidates": [
    "insert HumanReviewPending transition before RefundPayment",
    "change RefundAgent authority to may_suggest RefundPayment only",
    "add runtime guard that blocks RefundPayment until approval token exists"
  ]
}
```

### 17.2 statistical unsupported

```json
{
  "kind": "statistical_contract_unsupported",
  "target": "SupportAnswerAgent",
  "property": "Groundedness",
  "dataset": "SupportEvalV3",
  "slice": "JapaneseRefundTickets",
  "metric": "groundedness",
  "threshold": {
    "ci_lower_95": ">= 0.97"
  },
  "observed": {
    "estimate": 0.961,
    "ci_lower_95": 0.948,
    "ci_upper_95": 0.973,
    "samples": 420
  },
  "interpretation": "The groundedness requirement is not statistically supported for this slice.",
  "witness_examples": [
    "eval_case_0192",
    "eval_case_0331"
  ],
  "repair_candidates": [
    "add retrieval requirement for refund policy pages",
    "split Japanese refund prompt from generic support prompt",
    "raise human-review threshold for low-evidence answers",
    "add citation coverage check before final answer"
  ]
}
```

### 17.3 migration regression

```json
{
  "kind": "ai_migration_regression",
  "migration": "PromptV7ToV8",
  "target": "SupportAnswerAgent",
  "metric": "urgent_escalation_recall",
  "allowed_drop": 0.001,
  "observed_drop": 0.014,
  "dataset": "SupportEvalV3",
  "slice": "UrgentTickets",
  "status": "statistically_unsupported",
  "repair_candidates": [
    "restore escalation examples removed from prompt_v7",
    "add explicit urgent detection step before answer generation",
    "route urgent tickets to classifier_v3 before answer prompt",
    "block rollout of prompt_v8 for urgent ticket slice"
  ]
}
```

### 17.4 observed drift

```json
{
  "kind": "ai_observed_drift",
  "target": "SupportAnswerAgent",
  "window": "last_7_days",
  "metric": "refusal_rate",
  "baseline": "previous_7_days",
  "observed_drift": 0.034,
  "allowed_drift": 0.02,
  "affected_slices": ["JapaneseRefundTickets", "BillingDispute"],
  "repair_candidates": [
    "inspect prompt changes in support_prompt_v8",
    "compare retrieved source coverage between current and baseline windows",
    "run regression eval on affected slices",
    "temporarily increase human review routing for affected slices"
  ]
}
```

---

## 18. CLI案

### 18.1 契約検証

```bash
fslc ai check support_agent.fsl
fslc ai check support_agent.fsl --engine induction
```

### 18.2 統計評価

```bash
fslc ai eval support_agent.fsl --dataset SupportEvalV3
fslc ai eval support_agent.fsl --dataset SupportEvalV3 --slice JapaneseRefundTickets
```

### 18.3 migration / no-regression

```bash
fslc ai regress support_agent.fsl --migration PromptV7ToV8
fslc ai compare --from prompt_v7 --to prompt_v8 --dataset SupportEvalV3
```

### 18.4 本番ログreplay / drift

```bash
fslc ai replay support_agent.fsl --logs ./prod_ai_events.jsonl
fslc ai drift support_agent.fsl --window last_7_days --baseline previous_7_days
```

### 18.5 多環境互換性

```bash
fslc compat check production.fsl --include-ai
fslc ai compat support_agent.fsl --environment production
```

---

## 19. 実装方針

### 19.1 Parser / AST

追加ノード:

```text
AIComponent
AIAction
AIContract
Evaluator
Dataset
Slice
FailureMode
Authority
AIMigration
StatisticalProperty
ObservedProperty
Metric
Estimate
```

### 19.2 Desugar

| AI構文 | 展開先 |
|---|---|
| `ai_component` | artifact state / capability metadata |
| `ai_action` | nondeterministic action + output domain + workflow transition |
| `hard` | invariant / ensures / requires / runtime guard |
| `fallback` | transition rule / invariant |
| `authority` | tool call precondition / forbidden action |
| `ai_migration` | artifact transition + no-regression job |
| `statistical` | external eval job definition |
| `observed` | replay / telemetry aggregation job |

### 19.3 Runtime integration

AI実行ログはJSONLで受け取る。

```json
{
  "timestamp": "2026-07-07T12:00:00+09:00",
  "component": "SupportAnswerAgent",
  "model": "gpt_5_5",
  "prompt": "support_prompt_v8",
  "input_hash": "...",
  "retrieved_source_ids": ["doc_12", "doc_45"],
  "output_schema": "AnswerV2",
  "tool_calls": [
    {"name": "CreateTicket", "args": {"priority": "normal"}}
  ],
  "evaluator_results": {
    "groundedness": true,
    "confidence": 0.86
  }
}
```

`fslc ai replay` はこのログを読み、hard contract、observed property、declared capabilityとの整合を確認する。

### 19.4 評価runner

統計評価runnerは以下を行う。

```text
1. datasetを読み込む
2. sliceを展開する
3. target ai_componentを実行する、または既存eval結果を読む
4. evaluatorを適用する
5. metricを集計する
6. confidence intervalを計算する
7. thresholdを判定する
8. JSON findingを返す
```

初期実装では、Wilson interval、bootstrap、または指定された外部統計モジュールのいずれかをサポートすればよい。

---

## 20. 可能になる分析

### 20.1 AI出力契約分析

- 出力schemaが変わったとき、古いmobile / API consumerが壊れないか。
- AIが返してはいけないfieldを返していないか。
- AIが必要なcitationを省略していないか。
- AIがPIIを含む回答を生成していないか。

### 20.2 tool call安全性分析

- irreversible actionにhuman approvalがあるか。
- tool schema versionがserverの受け入れversionと一致するか。
- tool preconditionを満たさないcallが発生していないか。
- AIがforbidden toolを呼んでいないか。

### 20.3 RAG分析

- retrieverが必要sourceを取得できているか。
- answer claimsがretrieved sourcesに支持されているか。
- citationが許可source集合に属するか。
- index migration後に特定sliceのrecallが低下していないか。

### 20.4 migration regression分析

- prompt更新で特定sliceの品質が低下していないか。
- model更新で拒否率が上がっていないか。
- retriever更新でgroundednessが低下していないか。
- tool schema変更でwrong tool callが増えていないか。

### 20.5 operational drift分析

- 本番入力分布が評価セットから乖離していないか。
- refusal rateやhallucination rateが急変していないか。
- 特定言語・カテゴリ・顧客セグメントで悪化していないか。
- deprecated tool / deprecated APIへの依存が残っていないか。

### 20.6 evaluator信頼性分析

- evaluatorがhuman-labeled datasetと一致しているか。
- evaluator更新で判定基準が変わっていないか。
- evaluator confidenceが低いcaseを自動実行していないか。

### 20.7 AI生成物分析

- AIが生成したFSL仕様がparse可能か。
- 要求IDを保持しているか。
- verification errorがactionableか。
- 反例修復で別のrequirementを壊していないか。

---

## 21. MVP計画

### Phase 1: hard contract / tool guard

最初に実装すべき範囲。

採用済みのMVP設計: [`docs/DESIGN-ai-hard.md`](DESIGN-ai-hard.md)。

```text
ai_component
ai_contract.hard
tool schema conformance
authority
fallback
runtime replay for hard violations
JSON findings
```

価値:

- AI tool call事故を防ぎやすい。
- 既存kernelに展開しやすい。
- 統計評価なしでも実用価値がある。

### Phase 2: dataset / slice / statistical eval

```text
dataset
slice
metric
evaluator
statistical_property
confidence interval
```

価値:

- AI品質を定量的・slice別に管理できる。
- 平均値で隠れる劣化を検出できる。

### Phase 3: AI migration / no-regression

```text
ai_migration
prompt/model/retriever/tool schema diff
no_regression
compare eval
```

価値:

- AI変更をDB migrationのように安全に扱える。
- prompt更新・model更新のリスクを明示できる。

### Phase 4: observed property / drift

```text
production logs
observed_property
drift_check
runtime telemetry aggregation
```

価値:

- 評価セット外の本番劣化を検出できる。
- AI運用監視へ接続できる。

### Phase 5: multi-environment compatibility統合

```text
server/mobile/API/DB/tool schema compatibility
AI artifact lifecycle
versioned capability
fsl-db / fsl-compat連携
```

価値:

- AI componentを実運用システム全体の互換性検証に組み込める。

---

## 22. out of scope

初期実装では以下を対象外にする。

```text
LLM内部の完全形式証明
任意自然言語主張の完全な真偽判定
全入力空間に対する確率保証
provider固有の隠れたsampling distributionの正確な同定
AI evaluatorの完全な客観性保証
```

FSLが扱うのは、AI内部の真理ではなく、仕様化された契約・評価・観測・fallbackである。

---

## 23. リスクと対策

### 23.1 統計的指標への過信

リスク:

```text
ci_lower >= 0.97 を満たしたので安全だと誤解する
```

対策:

- 統計評価結果は `proved` と表示しない。
- dataset / slice / evaluator / confidence levelを常に結果に含める。
- out-of-distributionや本番driftを別ステータスで扱う。

### 23.2 評価セットへの過適合

リスク:

```text
promptがeval setにだけ最適化される
```

対策:

- holdout datasetをサポートする。
- slice別評価を必須化する。
- production observed propertyと組み合わせる。

### 23.3 evaluator bias

リスク:

```text
AI evaluatorが誤って合格判定する
```

対策:

- evaluator calibrationを必須にする。
- human-labeled datasetとの一致率を管理する。
- evaluator confidenceが低い場合はhuman reviewへ遷移する。

### 23.4 hard constraintをAI任せにする

リスク:

```text
AIに「PIIを出すな」と指示するだけでguardしない
```

対策:

- hard contractはruntime guardとしても評価する。
- tool call実行前にpreconditionを検証する。
- forbidden actionはAI出力後にブロックする。

### 23.5 dialect肥大化

リスク:

```text
fsl-aiがFSL kernelを複雑化する
```

対策:

- kernelにはhard constraintsと状態遷移だけを渡す。
- 統計評価は外部runnerとして実装する。
- dialect expansionの境界を明確にする。

---

## 24. 既存FSL設計との整合

この提案は、FSLの既存思想と整合する。

| FSL既存概念 | AI記述での対応 |
|---|---|
| `state` | AI workflow state、artifact version、capability state |
| `action` | AI action、tool execution、fallback transition |
| `invariant` | hard contract、authority boundary、runtime safety |
| `trans` | AI出力前後の状態条件、migration preservation |
| `leadsTo` | human reviewへ到達、escalation完了、safe refusal |
| `refinement` | abstract business ruleとAI output behaviorの対応 |
| `compose` | AI component + server + DB + mobile + tool API |
| `Monitor` | AI event log replay / runtime conformance |
| `testgen` | AI eval scaffold / tool-call conformance tests |
| JSON repair protocol | AI修復候補、prompt修正候補、fallback追加候補 |

重要なのは、AI記述がFSL kernelを壊さず、FSLの「AIが読みやすい診断JSON」を拡張することである。

---

## 25. 提案するファイル構成

```text
docs/
  DESIGN-ai.md
  DESIGN-stochastic.md
  DESIGN-ai-eval.md
  DESIGN-ai-migration.md

src/fslc/
  ai_dialect.py
  stochastic.py
  eval_runner.py
  ai_replay.py
  ai_json.py

schemas/fslc/
  ai_finding.schema.json
  statistical_result.schema.json
  ai_event.schema.json

examples/ai/
  support_answer_agent.fsl
  refund_agent_tool_safety.fsl
  rag_grounding.fsl
  prompt_migration.fsl
  ai_multienv_compat.fsl
```

---

## 26. まとめ

FSLにAI記述を追加すると、AIを「曖昧なブラックボックス」ではなく、以下を持つ検証対象として扱えるようになる。

```text
contract
statistical quality
failure mode
fallback transition
evaluator calibration
runtime observation
migration safety
environment compatibility
```

最小実装は、AI出力のhard contract、tool call guard、fallback、人間承認、JSON findingから始めるべきである。次にdataset / slice / evaluator / confidence intervalを導入し、AI品質を統計的に扱う。さらにAI migrationとobserved driftを加えることで、prompt、model、RAG、tool schemaの変更管理までFSLの対象にできる。

この拡張により、FSLは次の範囲を同一体系で扱えるようになる。

```text
決定的なアプリケーション仕様
DB / API / 多環境互換性
AI component の統計的・観測的・権限付き振る舞い
```

一言で言えば、`fsl-ai` / `fsl-stochastic` は、FSLを **AIを含むシステムの仕様・評価・移行・運用・修復を扱う言語基盤** へ拡張する提案である。

---

## 参考リンク

- FSL README: https://github.com/ymm-oss/fsl
- FSL docs map: https://github.com/ymm-oss/fsl/blob/main/docs/README.md
- FSL shared kernel + three dialect architecture: https://github.com/ymm-oss/fsl/blob/main/docs/DESIGN-layers.md
- FSL implementation bridge / Monitor / replay / testgen: https://github.com/ymm-oss/fsl/blob/main/docs/DESIGN-bridge.md
- FSL refinement design: https://github.com/ymm-oss/fsl/blob/main/docs/DESIGN-refinement.md
