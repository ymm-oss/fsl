# bank — 実装適合テストの実例

仕様(`specs/bank_system.fsl`、compose 済み・proved)から `fslc testgen` で
生成したハーネスに、FSL を知らない素の Python 実装(`bank.py`)を
Adapter(約20行)で結線した例。シナリオ再生7本+Monitor をオラクルとする
100ステップランダムウォークの 8/8 が通る。

```bash
cd examples/bank && PYTHONPATH=. ../../.venv/bin/python -m pytest test_bank_conformance.py -q
```
