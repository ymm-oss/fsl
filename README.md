# FSL — AI-Native Formal Specification Language

FSL は、**生成AIが書き・検証し・修正する**ことを第一目標に設計した、
アプリ開発向けの形式仕様言語です。検証器 `fslc` は Lark + Z3 により
**有界モデル検査(BMC)** と **k 帰納法による無限深度証明** を行い、結果を常に
**機械可読な JSON** で返します（LLM の write→verify→repair ループ用）。
仕様から統合テスト雛形を生成する `fslc scenarios` も備えます。

仕様は**コンサル(business)/ 要件(requirements)/ 設計(spec)の3層方言**で書け、
refinement で連鎖して要件 ID が全診断に透過する。非機能要件も SLA(離散時刻)まで対応。
言語仕様・意味論・出力 JSON は [`docs/LANGUAGE.md`](docs/LANGUAGE.md)、
文書全体の見取り図は [`docs/README.md`](docs/README.md) を参照。

## ディレクトリ構成

```
fsl/
├── README.md
├── pyproject.toml          # 依存 (lark, z3-solver) と fslc コマンドの定義
├── docs/
│   ├── README.md           # docs の見取り図(まずここ)
│   ├── LANGUAGE.md         # 言語リファレンス — 仕様を書くならこれ
│   ├── DESIGN-*.md         # 設計書(言語/3層方言/NFR/各機能 — 計12本)
│   └── DOGFOOD-1..5.md     # ドッグフーディング所見(バグ・発見の記録)
├── specs/                  # サンプル仕様 (*.fsl) — 正しいものは全て k=1 で proved
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
│   ├── order_system.fsl    #   compose: cart_v1 + payment 同期 checkout/capture
│   ├── bank{,_impl,_refines,_system}.fsl  # refinement + compose の連鎖
│   ├── seat_booking*.fsl   #   条件写像つき refinement
│   ├── repair_loop.fsl     #   fslc 自身のワークフローの自己仕様
│   └── cart_{buggy,fixed}.fsl     # v0 互換サンプル
├── examples/
│   ├── bank/               # 素の Python 実装への適合テスト(8/8)
│   ├── layers/             # 3層チェーン(business → requirements → 設計)
│   └── nfr/                # 離散時刻 SLA(urgency 規律・時間予算 invariant)
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

## かんたんセットアップ（PM・コンサル・非エンジニアの方）

yumemi/fsl は社内リポジトリのため、GitHub へのログインが1度だけ必要です。

**初回のみ(GitHub CLI の準備)**: ターミナルで `brew install gh` のあと
`gh auth login`(ブラウザが開くので社内アカウントでログイン)。

その後、次の1行でインストールできます:

```bash
gh repo clone yumemi/fsl ~/.fsl && bash ~/.fsl/install.sh
```

リポジトリを zip などで受け取った方・取得済みの方は、展開したフォルダ直下で:

```bash
./install.sh
```

インストールされるもの:

- `fslc` コマンド（`~/.local/bin/fslc` から利用）
- Claude Code 用スキル（`~/.claude/skills/fsl`）
- PM 向け・コンサル向けのサンプル（`examples/pm/`, `examples/consulting/`）

Windows の方は WSL を利用するか、開発者向け手順（PowerShell）を参照してください。

アンインストール:

```bash
rm -rf ~/.fsl ~/.local/bin/fslc ~/.claude/skills/fsl
```

## 開発者向けセットアップ

まずリポジトリを取得します:

```bash
git clone https://github.com/yumemi/fsl && cd fsl
```

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
fslc refine specs/cart_impl.fsl specs/cart_v1.fsl specs/cart_refines.fsl --depth 8
                                                  # 詳細仕様が抽象仕様を refine するか検査
fslc verify specs/order_system.fsl --depth 8    # compose: cart + payment を同期合成

# インストールせずモジュール実行でも可
python -m fslc verify specs/cart_v1_buggy.fsl
```

出力は常に JSON（stdout）。終了コード: 0 = verified / proved / refines /
conformant / generated、1 = violated / refinement_failed / reachable_failed /
unknown_cti / nonconformant、2 = 仕様エラー、3 = 内部エラー。
`cart_v1_buggy.fsl` は自動境界チェック（`type_bound`）の最短反例トレースを返します。

## AI エージェント向けスキル

FSL は学習データに存在しない言語のため、AI エージェント(Claude Code 等)が
仕様を書く際は **Agent Skill** で言語仕様と修復プロトコルを文脈に供給する。
配布・発見しやすいよう正本をリポジトリ直下の [`skills/fsl/`](skills/fsl/) に置く:

- [`skills/fsl/SKILL.md`](skills/fsl/SKILL.md) — ワークフロー・修復プロトコル・最小構文
- [`skills/fsl/reference.md`](skills/fsl/reference.md) — 凝縮版の完全言語リファレンスカード

このリポジトリで作業する Claude Code には `.claude/skills/fsl`(→ `skills/fsl`
へのシンボリックリンク)経由で自動認識される。別プロジェクトで使う場合は
`skills/fsl/` をそのプロジェクトの `.claude/skills/` か `~/.claude/skills/` に
コピーするか、`gh` のスキル拡張で `skills/` を配布元に指定する。
詳細は [`skills/README.md`](skills/README.md)。

## テスト

```bash
pytest
```

169 テストが全機能(v0互換 / 型システム / k帰納法 / leadsTo / scenarios /
runtime / refine / compose / 方言 / NFR)を検証します(約90秒)。
両評価器(Z3・具象 Monitor)は witness 再生の差分テストで相互検証されています。

## ライブラリ API

```python
from fslc import parse, build_spec, verify, prove

spec   = build_spec(parse(open("specs/cart_v1.fsl").read()))
result = verify(spec, depth=8)              # BMC。dict（CLI と同じ構造）
result = prove(spec, k_ind=1, base_depth=8) # k帰納法（proved / unknown_cti）
```
