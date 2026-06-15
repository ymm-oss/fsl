# refine 連鎖モード — end-to-end の忠実性を1コマンドで

層連鎖(業務 ⊒ 要件 ⊒ 設計 …)の伝播は、隣接ペアを個別に `refine` しても
**末端 ⊒ 最上位** を直接確かめる手段が無く、推移律を暗黙に信頼するしかなかった
(写像を手書きで合成すれば確かめられるが、取り違えやすい)。

`fslc refine` は **(spec, 写像) を並べて連鎖検査**できる。隣接写像を合成
(α_AC = α_BC ∘ α_AB、アクション対応 a→b→c / stutter)し、最下位 ⊒ 最上位を
**直接**検査する。有界 refinement は同一深さで推移的なので、これは
「全隣接リンクが成り立つ」ことと等価で健全(`docs/DESIGN-refinement.md` §7)。

## 登場人物(3層: 業務 ⊒ 要件 ⊒ 設計)

| ファイル | 層 | 追加した詳細 |
|---|---|---|
| `top.fsl` | 業務 (`ChainTop`) | Open → Done |
| `mid.fsl` | 要件 (`ChainMid`) | 審査ステップ `Review` を追加 |
| `bot.fsl` | 設計 (`ChainBot`) | 監査ステップ `Audit` をさらに追加 |
| `bot_refines_mid.fsl` | 設計 ⊒ 要件 | `audit` は要件層では stutter |
| `mid_refines_top.fsl` | 要件 ⊒ 業務 | `start_review` は業務層では stutter |

## 実行

```bash
E=examples/refinement_chain

# 隣接(従来どおり1ペアずつ)
fslc refine $E/bot.fsl $E/mid.fsl $E/bot_refines_mid.fsl --depth 6   # refines
fslc refine $E/mid.fsl $E/top.fsl $E/mid_refines_top.fsl --depth 6   # refines

# 連鎖: (spec 写像) を続けて並べると end-to-end で合成検査する
fslc refine $E/bot.fsl \
            $E/mid.fsl $E/bot_refines_mid.fsl \
            $E/top.fsl $E/mid_refines_top.fsl --depth 6
```

連鎖検査の出力(成功):

```json
{ "result": "refines", "impl": "ChainBot", "abs": "ChainTop",
  "action_map": { "start_review": "stutter", "audit": "stutter", "finish": "finish" },
  "chain": ["ChainBot", "ChainMid", "ChainTop"] }
```

`action_map` は合成済み(`audit`/`start_review` は最上位では stutter、`finish` は
最上位 `finish` に対応)。`chain` に層の並びが出る。

## 見どころ

- **直接 end-to-end**: 最下位 `ChainBot` の振る舞いが、合成 α で最上位
  `ChainTop` の語彙に写され、業務契約を破らないことを1コマンドで確認できる。
- **壊れたリンクの特定**: どこかの隣接写像が忠実性を破ると、結果は
  `refinement_failed` に加えて `failed_link: {from, to, kind}` で**最初に
  壊れたリンク**を指す(`tests/test_refinement_chain_example.py` 参照)。
- **合成の中身**: indexed map(`st[c]`)も parameterized action(`finish(c)`)も
  合成される。引数式が中間層の状態を読む場合のみ未対応(その旨の型エラー)。

検査: `tests/test_refinement_chain_example.py`。
