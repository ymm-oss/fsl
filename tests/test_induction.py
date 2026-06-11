"""k-induction engine tests (DESIGN-induction.md §6)."""
import json
import subprocess
from pathlib import Path

import pytest

from fslc import parse, build_spec, prove, verify

SPECS = Path(__file__).resolve().parent.parent / "specs"
ROOT = Path(__file__).resolve().parent.parent
PY = ROOT / ".venv" / "bin" / "python"
CTI_HINT = (
    "this state sequence satisfies all invariants but leads to a violation; "
    "the start state may be unreachable — add an auxiliary invariant that excludes it, "
    "then re-run"
)


def run_induction(name, depth=8, k_ind=1):
    ast = parse((SPECS / name).read_text(encoding="utf-8"))
    return prove(build_spec(ast), k_ind, depth)


def prove_inline(src, k_ind=1, depth=8):
    return prove(build_spec(parse(src)), k_ind, depth)


def cli_induction(name, depth=8, k_ind=1):
    proc = subprocess.run(
        [str(PY), "-m", "fslc", "verify", str(SPECS / name),
         "--engine", "induction", "--depth", str(depth), "--k", str(k_ind)],
        capture_output=True,
        text=True,
        cwd=ROOT,
    )
    return json.loads(proc.stdout), proc.returncode


def test_cart_v1_induction_proved_with_soldout():
    r = run_induction("cart_v1.fsl")
    assert r["result"] == "proved"
    assert r["engine"] == "induction"
    assert "k_used" in r
    sold = r["reachables"]["SoldOut"]
    assert sold["witnessed_at_step"] == 4
    assert len(sold["witness"]) == 5
    assert sold["witness"][-1]["state"]["stock"] == {"0": 0, "1": 0}
    assert "deadlock" not in r


def test_counter_latch_k1_proved():
    src = """
spec CounterLatch {
  state { x: Int }
  init { x = 0 }
  action inc() {
    requires x < 5
    x = x + 1
  }
  invariant XRange { x >= 0 and x <= 5 }
}
"""
    r = prove_inline(src)
    assert r["result"] == "proved"
    assert r["k_used"]["XRange"] == 1


def test_sync_unknown_cti_then_aux_proved():
    base = """
spec Sync {
  state { x: Int, y: Int }
  init { x = 0  y = 0 }
  action step() {
    requires x < 4
    x = x + 1
    y = y + 1
  }
  invariant Sync { y <= 4 }
}
"""
    r = prove_inline(base)
    assert r["result"] == "unknown_cti"
    assert r["invariant"] == "Sync"
    assert r["hint"] == CTI_HINT
    assert r["k"] == 1
    cti = r["cti"]
    assert cti["violated_at"] == 1
    assert len(cti["states"]) == 2
    assert cti["states"][0]["step"] == 0
    assert "state" in cti["states"][0]
    assert "action" in cti["states"][1]
    blob = json.dumps(r)
    assert "__present" not in blob

    with_aux = base.replace(
        "  invariant Sync { y <= 4 }",
        "  invariant Sync { y <= 4 }\n  invariant Aux { x == y }",
    )
    r2 = prove_inline(with_aux)
    assert r2["result"] == "proved"
    assert r2["k_used"]["Sync"] >= 1
    assert r2["k_used"]["Aux"] >= 1


def test_cart_v1_buggy_induction_same_violated_as_bmc():
    ast = parse((SPECS / "cart_v1_buggy.fsl").read_text(encoding="utf-8"))
    spec = build_spec(ast)
    bmc = verify(spec, 8)
    ind = prove(spec, 1, 8)
    assert bmc["result"] == "violated"
    assert ind["result"] == "violated"
    assert ind["violation_kind"] == bmc["violation_kind"]
    assert ind["invariant"] == bmc["invariant"]
    assert ind["violated_at_step"] == bmc["violated_at_step"]
    # Z3 may pick different (equally shortest) counterexample bindings per
    # run, so only the action name is stable — not its params.
    assert ind["last_action"]["name"] == bmc["last_action"]["name"]


def test_cli_induction_exit_codes_and_fields():
    out, code = cli_induction("cart_v1.fsl")
    assert code == 0
    assert out["result"] == "proved"
    assert out["engine"] == "induction"
    assert out["fsl"] == "1.0"
    assert isinstance(out["k_used"], dict)
    assert out["base_depth"] == 8

    sync_src = """
spec SyncCli {
  state { x: Int, y: Int }
  init { x = 0  y = 0 }
  action step() { requires x < 4  x = x + 1  y = y + 1 }
  invariant Sync { y <= 4 }
}
"""
    path = SPECS / "_sync_cli.fsl"
    path.write_text(sync_src, encoding="utf-8")
    try:
        proc = subprocess.run(
            [str(PY), "-m", "fslc", "verify", str(path),
             "--engine", "induction", "--depth", "4"],
            capture_output=True,
            text=True,
            cwd=ROOT,
        )
        out = json.loads(proc.stdout)
        assert proc.returncode == 1
        assert out["result"] == "unknown_cti"
    finally:
        path.unlink(missing_ok=True)


def test_k_iteration_with_higher_k():
    """Sync without Aux stays unknown_cti through k=1..4 (k loop runs)."""
    src = """
spec SyncK {
  state { x: Int, y: Int }
  init { x = 0  y = 0 }
  action step() {
    requires x < 4
    x = x + 1
    y = y + 1
  }
  invariant Sync { y <= 4 }
}
"""
    r = prove_inline(src, k_ind=4)
    assert r["result"] == "unknown_cti"
    assert r["k"] == 4
