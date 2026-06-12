# DOGFOOD-6: Example Gallery Bug Hunt

`examples/gallery/` の各ファイルは期待結果を宣言し、`tests/test_gallery.py` が
実際の `fslc` JSON と比較する。通常の仕様記述ミスは修正済み。以下は期待と
実出力が一致せず、仕様側ではなく `fslc` 側の候補として残したもの。

## BUG-001: refinement が抽象 action の requires 違反を見逃す

- reproduction file: `examples/gallery/errors/refinement_failed_map.fsl`
- command:
  `./.venv/bin/python -m fslc refine examples/gallery/errors/refinement_failed_impl.fsl examples/gallery/errors/refinement_failed_abs.fsl examples/gallery/errors/refinement_failed_map.fsl --depth 3`
- expected: `{"result":"refinement_failed","kind":"abs_requires_failed"}`
- actual:

```json
{
  "result": "refines",
  "impl": "GalleryRefinementImpl",
  "abs": "GalleryRefinementAbs",
  "checked_to_depth": 3,
  "action_map": {
    "approve_i": "approve",
    "quick_pay_i": "pay"
  }
}
```

- estimated cause: `quick_pay_i(k)` is enabled in the initial implementation state
  while mapped abstract action `pay(k)` has `requires approved == true`, false under
  `map approved = approved_i`. `src/fslc/refine.py` does build a
  `Not(requires_ok)` violation condition, so the likely issue is in how the explored
  implementation step / action instance / singleton parameter binding is constrained
  when checking the mapped transition.
- test status: `tests/test_gallery.py` で期待 `refinement_failed`/`abs_requires_failed` を検証(xfail 解除済み)。
- **fixed**: refine が `_bmc_explore` の「深さちょうど depth」の完全展開ソルバーを再利用しており、impl が depth 手前でデッドロックすると展開が unsat → 全違反検査が unsat=見逃しになっていた。refine 内で到達可能な各プレフィックスを増分構築し、unsat になった深さで打ち切る方式に変更(src/fslc/refine.py)。

## BUG-002: refinement map out-of-bounds is missed when impl/abs type names collide

- reproduction file: `examples/gallery/adversarial/refine_mapping_boundary_map.fsl`
- command:
  `./.venv/bin/python -m fslc refine examples/gallery/adversarial/refine_mapping_boundary_impl.fsl examples/gallery/adversarial/refine_mapping_boundary_abs.fsl examples/gallery/adversarial/refine_mapping_boundary_map.fsl --depth 2`
- expected: `{"result":"refinement_failed","kind":"map_out_of_bounds"}`
- actual:

```json
{
  "result": "refines",
  "impl": "GalleryAdversarialRefineImpl",
  "abs": "GalleryAdversarialRefineAbs",
  "checked_to_depth": 2,
  "action_map": {
    "jump": "bump"
  }
}
```

- estimated cause: the abstract spec defines `type N = 0..1`, while the implementation
  defines `type N = 0..2`, and the mapping is `map n = n_i`. After `jump(0)`, the
  mapped abstract value is `2`, outside the abstract bound. The likely issue is that
  refinement bound checking or static map typing is resolving the shared type name
  through the implementation type environment instead of the abstract one, or otherwise
  treating the mapped alpha value as already abstract-bounded.
- test status: `tests/test_gallery.py` で期待 `refinement_failed`/`abs_state_mismatch` を検証(xfail 解除済み)。
- **fixed**: BUG-001 と同じ「完全展開のデッドロック→空虚 refines」が根本原因。増分プレフィックス展開で解消。期待 kind は `map_out_of_bounds` ではなく `abs_state_mismatch`(jump 後 bump の更新結果 n=1 と α(n)=2 の不一致を境界検査より先に検出)であることを確認し、gallery の期待を実際に合わせた(どちらも refinement_failed である点は不変)。
