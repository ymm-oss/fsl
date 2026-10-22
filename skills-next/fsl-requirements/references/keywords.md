# Keywords: English and Japanese

A requirements spec headed `fsl 5` reads every keyword below in English or in
Japanese, and the two may be mixed in one file. **English is the canonical
spelling**: write the spec in English first and treat the Japanese keywords as
a way to make it read in the source's language (`glossary.md`).

How the table works:

- **Same position.** A Japanese keyword stands where the English one does
  (`許可 従業員 が 申請 を 提出する 条件 …` is `allow … only when …`). Two
  exceptions are particles: `with` is で, written **after** the input
  (`申請金額 で`, `申請金額 1 で`), and `is` is は (`結果 申請1 は 支払済み`).
- **Per place.** A keyword is read where the grammar expects it, so the same
  English word can be two keywords (`in` after `event … =` and after
  `concept … =` are different rows). Within one place each spelling means one
  keyword.
- **Only these words.** The header `fsl 5`, `implements … from` and its
  mapping, expression operators other than `and` / `or`, kernel members
  (`terminal`, `time`, `state`, …) and the `verify` world's `..` stay English.
- **Reserved rows** are not read yet; they are listed so that no glossary
  alias takes the word (`alias_shadows_keyword`).
- **Particles** are separate words after the word they mark. With particles
  the words of a phrase may come in any order, and then every word but the
  verb takes one (`mixed_particles`). A particle that gives a word the wrong
  role is `particle_role_mismatch`; one written onto the word (`従業員が`) is
  `particle_needs_space`.

The table below is generated from the parser's own table, so it lists exactly
what the parser reads.

<!-- keyword-table:begin (generated; do not edit by hand) -->

**Blocks of the file**

| English | Japanese | Note |
|---|---|---|
| `requirements` | `要件` |  |
| `glossary` | `用語集` |  |
| `verify` | `検証` |  |

**Inside `verify { }`**

| English | Japanese | Note |
|---|---|---|
| `instances` | `個数` |  |
| `values` | `値域` |  |

**The leading word of a member of `requirements`**

| English | Japanese | Note |
|---|---|---|
| `assume` | `前提` | reserved: not read yet |
| `out of scope` | `範囲外` | reserved: not read yet |
| `type` | `型` |  |
| `enum` | `列挙` |  |
| `number` | `数値` |  |
| `const` | `定数` |  |
| `actor` | `アクター` |  |
| `entity` | `エンティティ` |  |
| `concept` | `区分` |  |
| `resource` | `資源` |  |
| `event` | `出来事` |  |
| `rule` | `規則` |  |
| `decide` | `決定表` |  |
| `goal` | `目標` |  |
| `policy` | `方針` |  |
| `measure` | `指標` |  |
| `kpi` | `重要指標` |  |
| `scenario` | `シナリオ` |  |
| `forbidden` | `拒否シナリオ` |  |

**Inside `entity Name { }`**

| English | Japanese | Note |
|---|---|---|
| `lifecycle` | `ライフサイクル` |  |

**`concept C = E in S, … | where cond`**

| English | Japanese | Note |
|---|---|---|
| `in` | `段階` |  |
| `where` | `のうち` |  |

**`R belongs to E as role via event`**

| English | Japanese | Note |
|---|---|---|
| `belongs to` | `所属先` |  |
| `as` | `役名` |  |
| `via` | `契機` |  |

**`[fault] event name = phrase [with …] A -> B | in S [set …] [covers …]`**

| English | Japanese | Note |
|---|---|---|
| `fault` | `異常` |  |
| `environment` | `外部` | reserved: not read yet |
| `in` | `段階` |  |
| `set` | `更新` |  |
| `covers` | `対応要件` |  |
| `with` | `で` | after the word it marks |

**Inside a rule's or an event's expression**

| English | Japanese | Note |
|---|---|---|
| `input` | `入力` |  |

**`allow P only when X | only for C`**

| English | Japanese | Note |
|---|---|---|
| `allow` | `許可` |  |
| `only when` | `条件` |  |
| `only for` | `対象` |  |
| `only as` | `立場` | reserved: not read yet |

**`never P when X | for C`**

| English | Japanese | Note |
|---|---|---|
| `never` | `禁止` |  |
| `when` | `条件` |  |
| `for` | `対象` |  |
| `because` | `理由` | reserved: not read yet |

**`P only after Q`**

| English | Japanese | Note |
|---|---|---|
| `only after` | `先行条件` |  |

**`E enters T only from S, …`**

| English | Japanese | Note |
|---|---|---|
| `enters` | `遷移先` |  |
| `only from` | `遷移元` |  |

**`always C1 is C2`, `always E [not] in S, … has X`**

| English | Japanese | Note |
|---|---|---|
| `always` | `常に` |  |
| `is` | `は` |  |
| `in` | `段階` |  |
| `not in` | `段階以外` |  |
| `has` | `必須` |  |

**`deadline age <= bound` in a rule**

| English | Japanese | Note |
|---|---|---|
| `deadline` | `期限` |  |

**`decide ID "text" for S E { … [otherwise => …] } [assuming Q]`**

| English | Japanese | Note |
|---|---|---|
| `for` | `対象` |  |
| `otherwise` | `それ以外` |  |
| `assuming` | `仮定` |  |

**A decision row's outcome `Actor verb {or verb}`**

| English | Japanese | Note |
|---|---|---|
| `or` | `または` |  |

**`goal ID "text" some E can reach S`**

| English | Japanese | Note |
|---|---|---|
| `some` | `いずれかの` |  |
| `can reach` | `到達可能` |  |

**`policy ID "text" every E in S must eventually be T {or T}`**

| English | Japanese | Note |
|---|---|---|
| `every` | `すべての` |  |
| `in` | `段階` |  |
| `must eventually be` | `いずれ必ず` |  |
| `or` | `または` |  |

**`measure m = count C | count E in S, …`**

| English | Japanese | Note |
|---|---|---|
| `count` | `件数` |  |
| `in` | `段階` |  |

**`scenario` / `forbidden` steps and their `then`**

| English | Japanese | Note |
|---|---|---|
| `then` | `結果` |  |
| `is` | `は` |  |
| `and` | `かつ` |  |
| `rejected` | `拒否` |  |
| `with` | `で` | after the word it marks |

**Boolean operators of an expression**

| English | Japanese | Note |
|---|---|---|
| `and` | `かつ` |  |
| `or` | `または` |  |

**Japanese words shared by several places** (each means the same in all of them)

| Japanese | English |
|---|---|
| `条件` | `only when`, `when` |
| `対象` | `only for`, `for` |

**Particles** (a word of their own, after the word they mark)

| Particle | Marks |
|---|---|
| `が` | actor |
| `を` | entity |
| `に` | counterpart |
| `で` | input |
| `は` | topic |
<!-- keyword-table:end -->

`fslc fmt --lang ja` / `--lang en` respells every keyword of a file into one
language, and `--names alias` / `--names canonical` does the same for glossary
names; a file written in one spelling and respelled lowers to the same kernel.
(An `fslc fmt` that rejects these options predates them: respell by hand with
this table and compare `fslc explain --names canonical --readable` of the two
files.)
