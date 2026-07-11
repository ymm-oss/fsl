# SPDX-License-Identifier: Apache-2.0
# Copyright 2026 Ryoichi Izumita

"""Machine adjudication of k-induction lemma candidates (issue #177)."""
from __future__ import annotations

import json
import subprocess
import sys
import textwrap
from pathlib import Path

from fslc.cli import _build_arg_parser, exit_code, run_verify


SNAPSHOT = Path(__file__).parent / "snapshots" / "induction_lemma_fields.json"
ROOT = Path(__file__).resolve().parents[1]


SYNC = """
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


def _write(tmp_path, source=SYNC):
    path = tmp_path / "sync.fsl"
    path.write_text(textwrap.dedent(source), encoding="utf-8")
    return str(path)


def _prove(path, lemmas):
    return run_verify(
        path,
        depth=8,
        deadlock_mode="ignore",
        engine="induction",
        k_ind=1,
        lemmas=lemmas,
        use_cache=False,
    )


def test_verified_lemma_excludes_cti_and_proves_target(tmp_path):
    path = _write(tmp_path)
    assert _prove(path, [])["result"] == "unknown_cti"

    out = _prove(path, ["x == y"])

    assert out["result"] == "proved"
    assert out["lemmas"] == [
        {
            "expression": "x == y",
            "name": "AuxiliaryLemma1",
            "status": "proved",
            "used": True,
            "proof": {
                "result": "proved",
                "k": 1,
                "checked_to_depth": 8,
                "completeness": "unbounded",
            },
        }
    ]
    exclusion = out["lemma_cti_exclusions"][0]
    assert exclusion["lemma"] == "x == y"
    assert exclusion["target"] == "Sync"
    assert exclusion["k"] == 1
    assert exclusion["violated_steps"] == [0, 1]
    assert exclusion["cti"]["violated_at"] == 1
    assert out["auxiliary_invariant_recommendation"] == {
        "message": "write the used proved lemmas into the specification as auxiliary invariants",
        "declarations": ["invariant AuxiliaryLemma1 { x == y }"],
    }
    assert exit_code(out) == 0


def test_false_lemma_is_rejected_with_reachable_counterexample(tmp_path):
    out = _prove(_write(tmp_path), ["x <= 0"])

    assert out["result"] == "unknown_cti"
    lemma = out["lemmas"][0]
    assert lemma["expression"] == "x <= 0"
    assert lemma["status"] == "rejected"
    assert lemma["used"] is False
    assert lemma["proof"]["result"] == "violated"
    assert lemma["proof"]["violation_kind"] == "invariant"
    assert lemma["proof"]["trace"]
    assert out["lemma_cti_exclusions"] == []
    assert "auxiliary_invariant_recommendation" not in out
    assert exit_code(out) == 1


def test_lemma_json_fields_snapshot(tmp_path):
    out = _prove(_write(tmp_path), ["x == y"])
    snapshot = {
        key: out[key]
        for key in (
            "result",
            "engine",
            "completeness",
            "invariants_checked",
            "lemmas",
            "auxiliary_invariant_recommendation",
        )
    }
    snapshot["lemma_cti_exclusions"] = [
        {key: entry[key] for key in ("lemma", "target", "k", "violated_steps")}
        for entry in out["lemma_cti_exclusions"]
    ]

    assert snapshot == json.loads(SNAPSHOT.read_text(encoding="utf-8"))


def test_only_independently_proved_lemmas_enter_target_proof(tmp_path):
    out = _prove(_write(tmp_path), ["x <= 0", "x == y"])

    assert out["result"] == "proved"
    assert [item["status"] for item in out["lemmas"]] == ["rejected", "proved"]
    assert [item["used"] for item in out["lemmas"]] == [False, True]
    assert out["invariants_checked"][-1] == "AuxiliaryLemma2"


def test_invalid_lemma_expression_is_rejected_without_aborting_other_candidates(tmp_path):
    out = _prove(_write(tmp_path), ["x ==", "x == y"])

    assert out["result"] == "proved"
    assert out["lemmas"][0]["status"] == "rejected"
    assert out["lemmas"][0]["proof"]["result"] == "error"
    assert out["lemmas"][0]["proof"]["kind"] == "parse"
    assert out["lemmas"][1]["status"] == "proved"


def test_lemma_requires_induction_engine(tmp_path):
    out = run_verify(
        _write(tmp_path),
        depth=8,
        deadlock_mode="ignore",
        engine="bmc",
        lemmas=["x == y"],
        use_cache=False,
    )

    assert out["result"] == "error"
    assert out["kind"] == "usage"
    assert exit_code(out) == 2


def test_lemma_does_not_make_non_invariant_property_inductively_provable(tmp_path):
    source = SYNC.replace(
        "invariant Sync { y <= 4 }",
        "trans StepBound { y <= 5 }",
    )
    out = run_verify(
        _write(tmp_path, source),
        depth=8,
        deadlock_mode="ignore",
        engine="induction",
        property_name="StepBound",
        lemmas=["x == y"],
        use_cache=False,
    )

    assert out["result"] == "error"
    assert out["kind"] == "usage"
    assert "trans" in out["message"]


def test_repeatable_lemma_cli_contract():
    args = _build_arg_parser().parse_args([
        "verify", "sync.fsl", "--engine", "induction",
        "--lemma", "x == y", "--lemma", "x >= 0",
    ])

    assert args.lemmas == ["x == y", "x >= 0"]


def test_cli_lemma_proves_and_exits_zero(tmp_path):
    proc = subprocess.run(
        [
            sys.executable,
            "-m",
            "fslc",
            "verify",
            _write(tmp_path),
            "--engine",
            "induction",
            "--lemma",
            "x == y",
            "--deadlock",
            "ignore",
            "--no-cache",
        ],
        cwd=ROOT,
        capture_output=True,
        text=True,
        check=False,
    )

    out = json.loads(proc.stdout)
    assert proc.returncode == 0
    assert out["result"] == "proved"
    assert out["lemmas"][0]["used"] is True
