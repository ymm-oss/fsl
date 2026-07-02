# SPDX-License-Identifier: Apache-2.0
# Copyright 2026 Ryoichi Izumita

"""`fslc verify --instances`/`--values` bounds overrides (#86).

`verify { instances Item = 3  values Amount = 0..5 }` bounds are hardcoded in
the spec; these flags let the CLI shrink them (e.g. to 1 entity for liveness
runs) without editing the file. The override is applied as an AST rewrite in
`dialects.apply_verify_bounds_overrides`, before dialect desugaring runs
`_collect_verify_bounds` — so an unknown NAME (no matching `entity`/`number`
declaration) surfaces through the *existing* undeclared-entity/-number checks,
and a malformed value (non-integer, or `LO..HI` with `LO > HI`) surfaces
through the existing integer parsing / empty-range checks. Both come back as
a spec-error JSON envelope (exit code 2), never an argparse crash or an
"internal" (exit 3) error.
"""
from fslc.cli import exit_code, run_verify

REQ_SRC = r'''requirements SizeOverrideDemo {
  entity Item
  number Amount

  process Item {
    stages Idle, Done
    initial Idle
    transition finish Idle -> Done by Worker
      covers REQ-1 "finish completes the item"
  }
}
verify {
  instances Item = 3
  values Amount = 0..5
}
'''

KERNEL_SRC = r'''spec KernelDemo {
  type CaseId = 0..2
  state { x: Int }
  init { x = 0 }
  fair action bump() { requires x < 5  x = x + 1 }
  invariant NonNeg { x >= 0 }
}
'''


def _write(tmp_path, src, name="size_override_demo.fsl"):
    path = tmp_path / name
    path.write_text(src, encoding="utf-8")
    return path


def test_instances_override_shrinks_model(tmp_path):
    spec = _write(tmp_path, REQ_SRC)
    out = run_verify(str(spec), 6, "warn", instances=["Item=1"])
    assert out["result"] == "verified"
    assert out["bounds_overrides"] == {"instances": {"Item": 1}, "values": {}}


def test_values_override_shrinks_model(tmp_path):
    spec = _write(tmp_path, REQ_SRC)
    out = run_verify(str(spec), 6, "warn", values=["Amount=0..1"])
    assert out["result"] == "verified"
    assert out["bounds_overrides"] == {"instances": {}, "values": {"Amount": [0, 1]}}


def test_unknown_name_is_spec_error(tmp_path):
    spec = _write(tmp_path, REQ_SRC)
    out = run_verify(str(spec), 6, "warn", instances=["Bogus=1"])
    assert out["result"] == "error"
    assert exit_code(out) == 2
    assert "Bogus" in out["message"]


def test_malformed_instances_value_is_spec_error(tmp_path):
    spec = _write(tmp_path, REQ_SRC)
    out = run_verify(str(spec), 6, "warn", instances=["Item=abc"])
    assert out["result"] == "error"
    assert exit_code(out) == 2
    assert out["kind"] != "internal"


def test_malformed_values_range_is_spec_error(tmp_path):
    spec = _write(tmp_path, REQ_SRC)
    out = run_verify(str(spec), 6, "warn", values=["Amount=5..1"])
    assert out["result"] == "error"
    assert exit_code(out) == 2
    assert out["kind"] != "internal"


def test_kernel_spec_without_entity_number_rejects_override(tmp_path):
    spec = _write(tmp_path, KERNEL_SRC, name="kernel_demo.fsl")
    out = run_verify(str(spec), 6, "warn", instances=["CaseId=1"])
    assert out["result"] == "error"
    assert exit_code(out) == 2


def test_no_flags_behavior_unchanged(tmp_path):
    spec = _write(tmp_path, REQ_SRC)
    out = run_verify(str(spec), 6, "warn")
    assert out["result"] == "verified"
    assert "bounds_overrides" not in out
