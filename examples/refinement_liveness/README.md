# refinement は安全性を伝播し、活性を伝播しない

`fslc refine`(詳細仕様 ⊒ 抽象仕様の忠実性検査)が**何を保証し、何を保証
しないか**を一目で示す例。結論: refinement は前方シミュレーションなので
**安全性**(invariant・統制ガード・観測可能な振る舞いの包含)は下層へ
伝播するが、**活性**(`leadsTo`/`responds`)は伝播しない —
refinement は stutter(下位が上位状態を変えない内部ステップ)を許すため。

詳細は `docs/DESIGN-layers.md` §6 の注、`docs/LANGUAGE.md` §10。

## 登場人物

| ファイル | 役割 |
|---|---|
| `policy.fsl` | 上位層の契約。安全性(支払いは承認後のみ)+ 活性(`leadsTo EveryClaimDecided`)。`fair` な裁定で活性を担保 |
| `design_drops_liveness.fsl` | 忠実に refine するが、裁定の `fair` を落とし内部 stutter ループを持つ設計 |
| `design_keeps_liveness.fsl` | 上と裁定の `fair` だけが違う設計(活性を回復) |
| `design_bypasses_control.fsl` | 承認を飛ばして支払う設計(安全性違反) |
| `*_refines.fsl` | 各設計 ⊒ policy の写像 |

## 実行と期待結果

```bash
E=examples/refinement_liveness

# 契約は単体で健全(活性 leadsTo が成立、支払い到達も可能)
fslc verify $E/policy.fsl --engine induction --deadlock ignore        # proved

# ① 活性は伝播しない: refine は通るのに、同じ policy を設計層で verify すると壊れる
fslc refine $E/design_drops_liveness.fsl $E/policy.fsl \
            $E/design_drops_liveness_refines.fsl --depth 8            # refines
fslc verify $E/design_drops_liveness.fsl --depth 8 --deadlock ignore  # violated / leadsTo(ラッソ)

# ② 解決: 進行アクションに fair を付けて各層で再 verify すれば活性も成立
fslc refine $E/design_keeps_liveness.fsl $E/policy.fsl \
            $E/design_keeps_liveness_refines.fsl --depth 8            # refines
fslc verify $E/design_keeps_liveness.fsl --depth 8 --deadlock ignore  # verified

# ③ 安全性は伝播する: 承認を飛ばす設計は refine が捕まえる
fslc refine $E/design_bypasses_control.fsl $E/policy.fsl \
            $E/design_bypasses_control_refines.fsl --depth 8          # refinement_failed / abs_requires_failed
```

## 見どころ

- **①が肝**: `design_drops_liveness` は `refines`(安全性 OK)を返すのに、
  上位の活性 policy `EveryClaimDecided` は設計層で `violated`。`refine` の
  写像に `fair` は現れないので、上位で `fair` が担保していた進行を下位が
  落としても忠実な refinement のままになる。**業務層で proved にした
  `leadsTo`/`responds` policy は、`refine` が通っても自動継承されない。**
- **②**: `design_keeps_liveness` は `design_drops_liveness` と裁定の `fair`
  注釈だけが違う。refine からは2つの設計は区別できない(写像は同一)。
  活性を保つには各層で `leadsTo` を再 verify し、進行アクションに `fair` を付ける。
- **③**: 安全性の逸脱(`abs_requires_failed`)は refine が確実に検出する。
  `fast_pay` は終端状態 `DPaid` に飛ぶが全深さで検出される(終端へ至る違反を
  取りこぼしていた健全性バグの修正・回帰例も兼ねる)。

検査: `tests/test_refinement_liveness_example.py`。
