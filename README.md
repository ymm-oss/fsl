# FSL — AI-Native Formal Specification Language (v1.1)

FSL は、**生成AIが書き・検証し・修正する**ことを第一目標に設計した、
アプリ開発向けの形式仕様言語です。検証器 `fslc` は Lark + Z3 により
**有界モデル検査(BMC)** と **k 帰納法による無限深度証明** を行い、結果を常に
**機械可読な JSON** で返します（LLM の write→verify→repair ループ用）。
仕様から統合テスト雛形を生成する `fslc scenarios` も備えます。

言語仕様・意味論・出力 JSON の詳細は [`docs/LANGUAGE.md`](docs/LANGUAGE.md) を参照。
次版の言語設計(型システム・reachable・修復プロトコル等)は
[`docs/DESIGN-v1.md`](docs/DESIGN-v1.md) を参照。

## ディレクトリ構成

```
fsl/
├── README.md
├── pyproject.toml          # 依存 (lark, z3-solver) と fslc コマンドの定義
├── docs/
│   ├── LANGUAGE.md         # 言語リファレンス (v1.1) — 仕様を書くならまずこれ
│   ├── DESIGN-v1.md        # v1 言語設計書
│   ├── DESIGN-induction.md # k 帰納法エンジン実装設計
│   ├── DESIGN-scenarios.md # coverage 診断 / scenarios 実装設計
│   ├── DESIGN-seq.md       # Seq<T, N> 実装設計
│   └── DOGFOOD-1.md / -2.md# ドッグフーディング所見
├── specs/                  # サンプル仕様 (*.fsl) — 正しい10本は全て k=1 で proved
│   ├── cart_v1.fsl         #   Option / ensures / reachable の基本形
│   ├── cart_v1_buggy.fsl   #   ガード欠落 — type_bound 違反の最短反例が返る
│   ├── order_workflow.fsl  #   enum / struct / Set / sum
│   ├── auth_lockout.fsl    #   ロックアウト + ゴースト変数 + 補助 invariant
│   ├── inventory_reservation.fsl  # 保存則 invariant
│   ├── payment.fsl         #   部分返金 + 台帳 + 補助 invariant
│   ├── rate_limiter.fsl    #   トークンバケット
│   ├── mutex_queue.fsl     #   FIFO ミューテックス (Option + Seq)
│   ├── job_pipeline.fsl    #   リトライ付きジョブキュー (Seq + struct)
│   ├── audit_log.fsl       #   追記ログ + Seq 集約イディオム
│   └── cart_{buggy,fixed}.fsl     # v0 互換サンプル
├── src/fslc/               # 検証器パッケージ
│   ├── __init__.py         #   公開API: parse / build_spec / verify
│   ├── __main__.py         #   python -m fslc 用
│   ├── grammar.py          #   Lark 文法 + AST トランスフォーマ
│   ├── parser.py           #   parse(src) -> AST
│   ├── model.py            #   build_spec / 型→Z3sort / 定数評価 / FslError
│   ├── bmc.py              #   verify / prove(k帰納法) / scenarios / トレース生成
│   ├── runtime.py          #   Monitor 具象インタプリタ (Z3 不要)
│   ├── testgen.py          #   pytest 適合性テスト雛形生成
│   └── cli.py              #   CLI と JSON 出力・エラー封筒
└── tests/                  # pytest (v0互換 / v1 / induction / scenarios / runtime)
```

## セットアップ

依存は `lark`（純Python）と `z3-solver`（ネイティブ libz3 を同梱した
ビルド済み wheel）の2つだけ。**C++ コンパイラや別途の Z3 インストールは不要**で、
Mac / Windows / Linux いずれも `pip install` だけで完結します（要 Python 3.9+）。

**Mac / Linux:**

```bash
python3 -m venv .venv
source .venv/bin/activate         # fish の場合: source .venv/bin/activate.fish
pip install -e ".[dev]"           # lark, z3-solver, pytest を導入し fslc を editable インストール
```

**Windows (PowerShell):**

```powershell
py -m venv .venv
.venv\Scripts\Activate.ps1        # cmd の場合: .venv\Scripts\activate.bat
pip install -e ".[dev]"
```

venv を有効化せずに直接実行することもできます:
`./.venv/bin/python -m fslc ...`（Windows は `.venv\Scripts\python -m fslc ...`）。

## 使い方

```bash
fslc check  specs/cart_v1.fsl                    # 構文・型のみ(高速ループ)
fslc verify specs/cart_v1.fsl --depth 8          # BMC: verified + 最短反例/witness
fslc verify specs/cart_v1.fsl --engine induction # k帰納法: proved(無限深度)
fslc scenarios specs/cart_v1.fsl                 # 統合テスト雛形 JSON を生成
fslc replay specs/cart_v1.fsl --trace events.json  # イベントログの適合性検査
fslc testgen specs/cart_v1.fsl -o test_cart_v1.py  # pytest 適合性テスト雛形を生成

# インストールせずモジュール実行でも可
python -m fslc verify specs/cart_v1_buggy.fsl
```

出力は常に JSON（stdout）。終了コード: 0 = verified / proved / conformant /
generated、1 = violated / reachable_failed / unknown_cti / nonconformant、
2 = 仕様エラー、3 = 内部エラー。
`cart_v1_buggy.fsl` は自動境界チェック（`type_bound`）の最短反例トレースを返します。

## テスト

```bash
pytest
```

60 テストが v0 互換・v1 型システム・k 帰納法・scenarios・Seq の
全機能を検証します（約6秒）。

## ライブラリ API

```python
from fslc import parse, build_spec, verify, prove

spec   = build_spec(parse(open("specs/cart_v1.fsl").read()))
result = verify(spec, depth=8)              # BMC。dict（CLI と同じ構造）
result = prove(spec, k_ind=1, base_depth=8) # k帰納法（proved / unknown_cti）
```
