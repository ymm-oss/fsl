# SPDX-License-Identifier: Apache-2.0
# Copyright 2026 Ryoichi Izumita

"""Issue #24: the business audit ledger (`fslc ledger`)."""
from pathlib import Path

from fslc.cli import run_ledger


ROOT = Path(__file__).resolve().parents[1]
NFR = ROOT / "examples" / "nfr"
INJECTED = ROOT / "examples" / "gallery" / "injected"


def _write(tmp_path, name, src):
    p = tmp_path / name
    p.write_text(src, encoding="utf-8")
    return p


def _ledger(path, **kw):
    r = run_ledger(str(path), **kw)
    assert r["result"] == "generated", r
    assert r["kind"] == "audit_ledger"
    return r["content"]


def test_ledger_sla_violation_groups_by_requirement(tmp_path):
    src = (NFR / "sla_worker.fsl").read_text(encoding="utf-8").replace(
        "    urgent start, finish\n", "")
    p = _write(tmp_path, "sla_no_urgent.fsl", src)
    md = _ledger(p, depth=10)
    assert "意図ずれ監査台帳" in md
    assert "NFR-1" in md                      # grouped by requirement id
    assert "🔴 要確認" in md
    assert "| sla |" in md or "`sla`" in md    # trace_type drives the row
    assert "スケジューリング前提" in md          # sla business translation
    # decision affordances a PM can act on
    assert "☐ 承認" in md and "☐ 差戻し" in md and "☐ リスク受容" in md


def test_ledger_guarantee_limit_is_positive_framing(tmp_path):
    src = (NFR / "sla_worker.fsl").read_text(encoding="utf-8").replace(
        "    urgent start, finish\n", "")
    p = _write(tmp_path, "sla_no_urgent.fsl", src)
    md = _ledger(p, depth=10)
    # the guarantee limit states what IS covered, not "nothing is guaranteed"
    assert "全実行を網羅" in md or "全実行を証明" in md
    assert "内部整合" in md


def test_ledger_verified_spec_all_confirmed():
    md = _ledger(NFR / "support_sla.fsl", depth=8)
    assert "🟢" in md
    assert "🔴 要確認" not in md               # nothing needs review
    for rid in ("REQ-1", "REQ-3", "REQ-5"):
        assert rid in md


def test_ledger_forbidden_flow_flagged():
    md = _ledger(INJECTED / "order_workflow__guard_weakening.fsl", depth=6)
    assert "🔴 要確認" in md
    assert "forbidden" in md
    assert "OR-FB-CANCEL-SHIPPED" in md


def test_ledger_reachable_dead_path_spec_level(tmp_path):
    p = _write(tmp_path, "unreach.fsl", """
spec Unreach {
  state { x: Bool }
  init { x = false }
  action noop() { requires x  x = true }
  reachable XTrue { x }
}
""")
    md = _ledger(p, depth=5)
    assert "（仕様全体）" in md                 # untagged findings grouped at spec level
    assert "reachable" in md
    assert "到達" in md                         # dead-path translation


def test_ledger_raw_json_demoted_to_appendix(tmp_path):
    src = (NFR / "sla_worker.fsl").read_text(encoding="utf-8").replace(
        "    urgent start, finish\n", "")
    p = _write(tmp_path, "sla_no_urgent.fsl", src)
    md = _ledger(p, depth=10)
    assert "## 付録" in md
    assert "<details>" in md                    # raw JSON is collapsed, not page 1
    # the risk list precedes the raw JSON
    assert md.index("リスク一覧") < md.index("付録")


def test_ledger_writes_file(tmp_path):
    out = tmp_path / "led.md"
    r = run_ledger(str(NFR / "support_sla.fsl"), depth=8, output=str(out))
    assert r["result"] == "generated"
    assert r["output"] == str(out)
    assert out.exists() and "監査台帳" in out.read_text(encoding="utf-8")


def test_ledger_impl_log_nonconformance(tmp_path):
    # a trace that violates the spec → the ledger surfaces non-conformance
    log = tmp_path / "log.json"
    log.write_text('[{"action": "submit", "params": {"r": 0}},'
                   ' {"action": "submit", "params": {"r": 0}}]', encoding="utf-8")
    md = _ledger(NFR / "sla_worker.fsl", depth=8, impl_log=str(log))
    assert "実装ログ適合" in md
