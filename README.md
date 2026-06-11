# FSL — AI-Native Formal Specification Language (v0 プロトタイプ)

FSL は、**生成AIが書き・検証し・修正する**ことを第一目標に設計した、
アプリ開発向けの形式仕様言語です。検証器 `fslc` は Lark + Z3 による
**有界モデル検査(BMC)** を行い、結果を常に**機械可読な JSON** で返します
（LLM の write→verify→repair ループ用）。

言語仕様・意味論・出力 JSON の詳細は [`docs/LANGUAGE.md`](docs/LANGUAGE.md) を参照。
次版の言語設計(型システム・reachable・修復プロトコル等)は
[`docs/DESIGN-v1.md`](docs/DESIGN-v1.md) を参照。

## ディレクトリ構成

```
fsl/
├── README.md
├── pyproject.toml          # 依存 (lark, z3-solver) と fslc コマンドの定義
├── docs/
│   ├── LANGUAGE.md         # 言語仕様書 (v0)
│   └── DESIGN-v1.md        # v1 言語設計書
├── specs/                  # サンプル仕様 (*.fsl)
│   ├── cart_buggy.fsl      #   在庫確認なしの checkout — invariant 違反する
│   └── cart_fixed.fsl      #   在庫ガード追加 — 検証を通る
├── src/fslc/               # 検証器パッケージ
│   ├── __init__.py         #   公開API: parse / build_spec / verify
│   ├── __main__.py         #   python -m fslc 用
│   ├── grammar.py          #   Lark 文法 + AST トランスフォーマ
│   ├── parser.py           #   parse(src) -> AST
│   ├── model.py            #   build_spec / 型→Z3sort / 定数評価 / FslError
│   ├── bmc.py              #   verify / transition / 反例トレース生成
│   └── cli.py              #   CLI と JSON 出力・エラー封筒
└── tests/                  # サンプル仕様の期待結果テスト
    └── test_cart.py
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
# editable インストール後はコマンドとして
fslc verify specs/cart_buggy.fsl
fslc verify specs/cart_fixed.fsl --depth 8

# インストールせずモジュール実行でも可
python -m fslc verify specs/cart_buggy.fsl
```

出力は常に JSON（stdout）。`verified` のときのみ終了コード 0。
`cart_buggy.fsl` は `NoNegativeStock` 違反の反例トレースを、
`cart_fixed.fsl` は `action_coverage`（空虚性チェック）つきの検証成功を返します。

## テスト

```bash
pytest
```

`tests/test_cart.py` が `specs/` のサンプルに対して
「バグ版は violated・修正版は verified」を検証します。

## ライブラリ API

```python
from fslc import parse, build_spec, verify

ast  = parse(open("specs/cart_fixed.fsl").read())
spec = build_spec(ast)
result = verify(spec, depth=8)   # dict（CLI が JSON 化するのと同じ構造）
```
