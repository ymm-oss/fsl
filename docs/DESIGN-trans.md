# FSL — `trans`(遷移 invariant / 2状態安全性)実装設計

動機: DOGFOOD-11 F24。self-spec で「Reject 以後は Nonconformant のまま」
「ToolFault 以後は修復不能」のような**遷移禁止**を表明したいが、従来は
ghost 変数 + 1状態 invariant、または action guard への埋め込みで間接表現するしか
なかった。`trans` は action 横断の2状態安全性を直接書く構文。

## 1. 構文

```fsl
trans RejectIsSticky {
  old(status) == Nonconformant => status == Nonconformant
}
```

`trans <Name> [meta_tag] { <expr> }` は `invariant` / `reachable` / `leadsTo` と同じ
トップレベル性質宣言。`old(<expr>)` は `ensures` と同じ構文で使える。

## 2. 意味論

`trans P` は全到達遷移 `σ -> σ'` について、式を `σ'` で評価し、`old(e)` だけを
`σ` で評価する。つまり action 個別の `ensures` を、spec 全体に一般化した
2状態述語である。

- `t = 0` には prior state が無いので検査しない。
- 違反は `result:"violated"`, `violation_kind:"trans"`, `trans:"Name"` と最短 trace。
- 成功出力は既存の `invariants_checked` を保ったまま `transitions_checked` を追加する。

## 3. BMC

`_bmc_explore` は既存 invariant 検査の直後、`t >= 1` の各ステップで全 `trans` を評価する:

```
eval_expr(expr, states[t], {}, spec, old_state=states[t-1], in_ensures=True)
```

`old()` のゲートは `ensures` と同じ評価経路を再利用する。ユーザー向けエラーは
「ensures または trans 内のみ」と表現し、trans 文脈で誤解を招かないようにする。

## 4. Induction

Base case は従来どおり BMC を実行するため、到達可能な `trans` 違反は通常の
`violated` として返る。

Step case では `Inv(σ0)` と `T(σ0, σ1)` の下で各 `trans(σ0, σ1)` を検査する。
`¬trans` が satisfiable なら `proved` にせず、`unknown_cti` として2状態 CTI を返す。
これは「全 invariant を満たすが到達不能かもしれない始状態から、trans を破る遷移がある」
という既存 CTI の読み方と同じ。

## 5. Temporal Hierarchy

- `invariant`: 1状態安全性。全到達状態で成立する。
- `trans`: 2状態安全性。全到達遷移で成立し、`old()` が使える。
- `leadsTo`: liveness。深さ K までのラッソ / 停滞反例を探す。

「一度 X になったら次の全ステップで X を保つ」は `trans`。
「X になったらいつか Y」は `leadsTo`。

## 6. `forbidden` との違い

`forbidden` は具体的な操作列が拒否されることを check 時に再生する負の受け入れ基準。
ガード漏れのような「このトレースは通ってはいけない」を、人間が列挙した trace で検査する。

`trans` は trace を列挙せず、全 action・全パラメータ・全到達遷移に量化された性質を
BMC / induction で検査する。具体例:

```fsl
trans ToolFaultNotRepairable {
  old(status) == ToolFault => status == ToolFault
}
```

これは「ToolFault から抜ける任意の遷移」を禁止する。`forbidden` で同じ意図を書くには
抜け道候補の操作列を個別に列挙する必要があり、新しい action が増えたときに漏れやすい。
