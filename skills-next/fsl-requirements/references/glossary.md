# Writing a glossary

A `glossary` gives canonical names their aliases, so that a spec reads in the
source's language while the kernel, generated tests and implementations keep
the canonical (ASCII) names. With the Japanese keywords (`keywords.md`) and
the particles, a whole spec can read in Japanese and still be the English spec:
the Japanese spelling below lowers to exactly the kernel of the English
example in `../SKILL.md`.

## Write the English spec first

Write and check the spec in English (`../SKILL.md`). The glossary is the
memo's term list, written down once the names are settled; add it last and
re-run the whole check loop. Renaming a canonical name later changes every
design mapping and implementation that uses it, so choose the canonical names
before the aliases.

## Rules of the block

```fsl-fragment
glossary {
  Return        = 返品
  Return.amount = 返品金額
  requests      = 申請する
}
```

- **Place.** One `glossary` per spec, as its first item
  (`misplaced_glossary` otherwise). Requirements specs only.
- **Left: a canonical name** declared in the spec — a type, constant, actor,
  entity, stage, concept, measure, verb, the spec's own name — or a field as
  `Entity.field` (several entities can have a field `amount`). Canonical
  names are ASCII: a non-ASCII name with no glossary entry is
  `canonical_name_not_ascii`, which tells you to declare an ASCII name and
  alias it.
- **Right: one alias**, one word with no spaces. One alias per canonical name
  (`duplicate_alias`), one canonical name per alias (`alias_collision`), and an
  alias must not be spelled like a canonical name of the spec
  (`alias_shadows_name`) or like any keyword in either language, or a
  particle (`alias_shadows_keyword`: choose a longer word, such as `申請済み`
  rather than `申請` when `申請` is already taken).
- **Use.** After the block, an alias can be written wherever its canonical
  name can: declarations, expressions, rules, decision rows, scenario steps,
  `verify`, and kernel members such as `terminal`. Both spellings may be
  mixed; `fslc lint --strict-names` warns `mixed_names` where one name is
  written both ways.
- **Not in `implements`.** The `implements` block and refinement files are
  written in canonical names only (`alias_in_implements`): the other layer
  does not read this glossary.
- **Declare canonically what reaches code.** Event names
  (`出来事 request = …`), property names (`AmountPositive:`), requirement IDs
  and the spec name are declared with their canonical name, even in a file
  written in Japanese; an event or the spec may still have an alias for the
  prose. Scenario instance names (`r1`) are local to the trace.
- Diagnostics and `fslc explain --readable` show the alias where there is one,
  with the canonical name next to it in messages (`'申請（Claim）'`); the
  lowered state names (`return_stage`) stay canonical.

## Phrases with particles

A phrase (an event's, a rule's, a decision row's outcome, a scenario step)
can mark each word with a particle written as a word of its own:

| Particle | Marks | Example |
|---|---|---|
| が | the actor | `顧客 が` |
| を | the entity, or the instance in a step | `返品 を`, `r1 を` |
| で | an input (this is `with`, written after the input) | `返品金額 で`, `返品金額 1 で` |
| は | the topic of a check (this is `is`) | `結果 r1 は 返金済み` |

With particles, every word but the verb takes one (`mixed_particles`), and
the words may then come in any order. A particle on a word of the wrong kind
is `particle_role_mismatch` (`経理 を …`: an actor marked as the entity); a
particle written onto its word (`返品を`) is read as one unknown word and
reported as `particle_needs_space`. Without particles a phrase is read by
position, as in English (`Customer requests Return`).

## The example in Japanese

The example of `../SKILL.md`, with a glossary, Japanese keywords and
particles. Its kernel equals the English one (spans, origins and annotations
aside), so a design written against either is the same design.

```fsl check
fsl 5
要件 ReturnRequirements {
  用語集 {
    ReturnRequirements = 返品要件
    Amount             = 金額
    AUTO_LIMIT         = 自動承認上限
    Customer           = 顧客
    System             = システム
    Manager            = 上長
    Finance            = 経理
    Return             = 返品
    Return.amount      = 返品金額
    Draft              = 下書き
    Requested          = 申請済み
    Approved           = 承認済み
    Rejected           = 却下済み
    Refunded           = 返金済み
    SmallReturn        = 少額返品
    LargeReturn        = 高額返品
    requests           = 申請する
    approves           = 承認する
    rejects            = 却下する
    refunds            = 返金する
    refunded_returns   = 返金件数
  }

  数値 金額
  定数 自動承認上限 = 1
  アクター 顧客, システム, 上長, 経理

  エンティティ 返品 {
    返品金額: 金額 = 0
    ライフサイクル 下書き -> 申請済み -> 承認済み -> 返金済み
    ライフサイクル 申請済み -> 却下済み
  }

  区分 少額返品 = 返品 のうち 返品金額 <= 自動承認上限
  区分 高額返品 = 返品 のうち 返品金額 > 自動承認上限

  出来事 request      = 顧客 が 返品 を 申請する 返品金額 で   下書き -> 申請済み
  出来事 auto_approve = システム が 返品 を 承認する           申請済み -> 承認済み
  出来事 mgr_approve  = 上長 が 返品 を 承認する               申請済み -> 承認済み
  出来事 reject       = 上長 が 返品 を 却下する               申請済み -> 却下済み
  出来事 refund       = 経理 が 返品 を 返金する               承認済み -> 返金済み

  規則 REQ-RET-001 "返品は正の金額で申請する" {
    許可 顧客 が 返品 を 申請する 条件 入力 返品金額 > 0
  }

  決定表 REQ-RET-002 "申請された返品を誰が決めるか" 対象 申請済み 返品 {
    少額返品 => システム が 承認する
      対応要件 REQ-RET-003 "自動承認上限以下の返品はシステムが承認する"
    高額返品 => 上長 が 承認する または 却下する
      対応要件 REQ-RET-004 "自動承認上限を超える返品は上長が承認または却下する"
  }

  規則 REQ-RET-005 "申請後の返品は正の金額を保つ" {
    AmountPositive: 常に 返品 段階以外 下書き 必須 返品金額 > 0
  }

  terminal { forall r: 返品 { stage(r) == 返金済み or stage(r) == 却下済み } }

  指標 返金件数 = 件数 返品 段階 返金済み
  目標 GOAL-RET-001 "返品は返金まで進める" いずれかの 返品 到達可能 返金済み
  方針 POL-RET-001 "申請された返品はいずれ決まる"
    すべての 返品 段階 申請済み いずれ必ず 承認済み または 却下済み

  シナリオ AC-RET-001 "少額の返品はシステムが承認し返金される" {
    顧客 が r1 を 返品金額 1 で 申請する
    システム が r1 を 承認する
    経理 が r1 を 返金する
    結果 r1 は 返金済み
  }
  シナリオ AC-RET-002 "高額の返品は上長が却下する" {
    顧客 が r1 を 返品金額 2 で 申請する
    上長 が r1 を 却下する
    結果 r1 は 却下済み
  }
  拒否シナリオ FB-RET-001 "金額 0 の返品は申請できない" {
    顧客 が r1 を 返品金額 0 で 申請する
    結果 拒否
  }
  拒否シナリオ FB-RET-002 "少額の返品は上長が決めない" {
    顧客 が r1 を 返品金額 1 で 申請する
    上長 が r1 を 承認する
    結果 拒否
  }
}
検証 {
  個数 返品 = 2
  値域 金額 = 0..3
}
```

Read it against the English example line by line: `要件` is `requirements`,
`許可 … 条件 入力 返品金額 > 0` is `allow … only when input amount > 0`,
`常に 返品 段階以外 下書き 必須 …` is `always Return not in Draft has …`,
`結果 拒否` is `then rejected`. `terminal { }` is a kernel member and stays
English, but the names in it are aliases like everywhere else.

## Checking a glossary

- Run the whole check loop on the Japanese file, not only on the English one.
- `fslc lint --strict-names` should report no `mixed_names`.
- `fslc explain --readable` shows aliases where the file has a glossary;
  `fslc explain --names canonical --readable` shows the canonical names the
  kernel has. Compare the latter with the English file's.
- `fslc fmt --names canonical` / `--names alias` and `--lang en` / `--lang ja`
  respell a file mechanically; the respelled file must lower to the same
  kernel. (An `fslc fmt` that rejects these options predates them; respell by
  hand and compare `explain --names canonical` instead.)
