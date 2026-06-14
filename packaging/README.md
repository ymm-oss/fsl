# スタンドアロンバイナリのビルド

`fslc` を Python 不要の単一実行ファイルとして配布するための仕組み。
リリース時は `.github/workflows/release.yml` が各 OS/アーキで自動ビルドし、
GitHub Release に添付する（`v*` タグの push が契機）。

## 仕組み

- ツール: [PyInstaller](https://pyinstaller.org/) の `--onefile`
- 依存の同梱:
  - `--collect-all z3` … z3 のネイティブ libz3(`.dylib`/`.so`/`.dll`) を取り込む
    （唯一のネイティブ依存。これさえ入れば `verify` まで外部依存なしで動く）
  - `--copy-metadata fslc` … `importlib.metadata.version("fslc")` を frozen 環境でも
    解決できるようにする（無いと `--version` が fallback の `1.0.0` になる）
- エントリ: `packaging/fslc_entry.py`（`fslc.cli.main` を呼ぶだけ）

## ローカルでビルドして試す

```bash
python3 -m venv /tmp/fsl-build && source /tmp/fsl-build/bin/activate
pip install . pyinstaller

pyinstaller --onefile --name fslc \
  --collect-all z3 \
  --copy-metadata fslc \
  packaging/fslc_entry.py

# 生成物: dist/fslc （Windows は dist/fslc.exe）
./dist/fslc --version
./dist/fslc verify examples/pm/cancel_flow.fsl
```

生成されるバイナリは ~37MB（z3 込み）。`--onefile` は起動時に自己展開するため
初回起動がわずかに遅い。

## リリース手順

```bash
git tag v1.1.0
git push origin v1.1.0
```

これで全プラットフォームのビルドが走り、`fslc-<os>-<arch>` と `*.sha256` が
Release に添付される。手元で動作だけ確認したいときは Actions タブから
`workflow_dispatch` で起動（この場合は Release 添付をスキップし artifact として残す）。

## 制約・メモ

- **クロスビルド不可**: 各 OS/アーキは対応する runner 上でビルドする必要がある
  （PyInstaller はクロスコンパイルしない）。matrix で対応済み。
- **macOS の署名/公証なし**: ダウンロードした実行ファイルは Gatekeeper で
  検疫される。利用側で `xattr -d com.apple.quarantine <file>` が必要。
  正式な公証には Apple Developer 証明書が要る（現状は範囲外）。
- **macOS Intel (x86_64) は対象外**: GitHub の `macos-13` (Intel) ランナーが
  枯渇して queued のまま着手されないため matrix から外している。Apple Silicon
  移行が進み Intel mac 需要も縮小中。必要になったら `macos-13` / `target:
  macos-x64` / `asset: fslc-macos-x64` の行を `build` matrix に戻すだけでよい
  (z3 は x86_64 wheel を持つのでビルド自体は他 mac と同一レシピで通る)。
- **z3-solver は wheel 限定** (`--only-binary=z3-solver`): 最新版が当該 OS/アーキの
  wheel を持たない場合に pip がソースビルドへ落ちて失敗するのを防ぐ。pip は
  wheel のある直近バージョンへ自動で後退する。
