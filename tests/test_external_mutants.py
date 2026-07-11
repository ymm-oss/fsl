# SPDX-License-Identifier: Apache-2.0
# Copyright 2026 Ryoichi Izumita

"""External JSONL mutation adjudication (issue #178)."""
from __future__ import annotations

import json
import subprocess
import sys
import textwrap
from pathlib import Path

from fslc.cli import _build_arg_parser, run_mutate
from fslc.mutate import mutate_file


SNAPSHOT = Path(__file__).parent / "snapshots" / "external_mutants.json"
ROOT = Path(__file__).resolve().parents[1]


BASE = """
spec Guarded {
  type Count = 0..1
  state { x: Count }
  init { x = 0 }
  action inc() {
    requires x == 0
    x = 1
  }
  invariant Bound { x <= 1 }
}
"""


def _write(tmp_path, name, text):
    path = tmp_path / name
    path.write_text(textwrap.dedent(text), encoding="utf-8")
    return path


def _jsonl(tmp_path, records):
    path = tmp_path / "mutants.jsonl"
    lines = [json.dumps(record) if not isinstance(record, str) else record for record in records]
    path.write_text("\n".join(lines) + "\n", encoding="utf-8")
    return path


def _records():
    return [
        {
            "id": "llm-killed",
            "op": "weaken_guard_and_increment",
            "replace": {
                "target": "    requires x == 0\n    x = 1",
                "replacement": "    x = x + 1",
            },
            "description": "allow repeat increments beyond Count",
        },
        {
            "id": "llm-survived",
            "op": "remove_guard",
            "replace": {
                "target": "    requires x == 0\n",
                "replacement": "",
            },
            "description": "repeat assignment remains inside the bound",
        },
        {
            "id": "llm-invalid",
            "op": "broken_syntax",
            "mutated_spec": "spec Broken {",
        },
    ]


def test_external_jsonl_distinguishes_killed_survived_and_invalid(tmp_path):
    spec = _write(tmp_path, "guarded.fsl", BASE)
    mutants = _jsonl(tmp_path, _records())

    out = mutate_file(
        str(spec),
        depth=3,
        max_mutants=0,
        external_mutants=str(mutants),
    )

    assert out["result"] == "mutated"
    assert out["summary"] == {
        "total": 3,
        "killed": 1,
        "survived": 1,
        "invalid": 1,
        "kill_rate": 0.5,
        "by_source": {
            "builtin": {"total": 0, "killed": 0, "survived": 0, "invalid": 0, "kill_rate": None},
            "external": {"total": 3, "killed": 1, "survived": 1, "invalid": 1, "kill_rate": 0.5},
        },
    }
    by_id = {item["id"]: item for item in out["mutants"]}
    assert by_id["llm-killed"]["status"] == "killed"
    assert by_id["llm-killed"]["source"] == "external"
    assert by_id["llm-survived"]["status"] == "survived"
    assert by_id["llm-invalid"]["status"] == "invalid"
    assert by_id["llm-invalid"]["invalid"]["kind"] == "parse"
    assert by_id["llm-invalid"]["killed_by"] is None


def test_external_full_spec_mutant_is_supported(tmp_path):
    spec = _write(tmp_path, "guarded.fsl", BASE)
    mutated = textwrap.dedent(BASE).replace(
        "    requires x == 0\n    x = 1",
        "    x = x + 1",
    )
    mutants = _jsonl(tmp_path, [{"id": "full", "mutated_spec": mutated}])

    out = mutate_file(str(spec), depth=3, max_mutants=0, external_mutants=str(mutants))

    assert out["mutants"][0]["id"] == "full"
    assert out["mutants"][0]["input_kind"] == "full_spec"
    assert out["mutants"][0]["status"] == "killed"


def test_external_mutation_json_snapshot(tmp_path):
    spec = _write(tmp_path, "guarded.fsl", BASE)
    mutants = _jsonl(tmp_path, _records())
    out = mutate_file(str(spec), depth=3, max_mutants=0, external_mutants=str(mutants))
    snapshot = {
        "result": out["result"],
        "summary": out["summary"],
        "mutants": [
            {
                "id": item["id"],
                "op": item["op"],
                "source": item["source"],
                "input_kind": item["input_kind"],
                "status": item["status"],
                "killed_by": item["killed_by"],
                **(
                    {"invalid_kind": item["invalid"]["kind"]}
                    if item["status"] == "invalid"
                    else {}
                ),
            }
            for item in out["mutants"]
        ],
    }

    assert snapshot == json.loads(SNAPSHOT.read_text(encoding="utf-8"))


def test_external_mutants_combine_with_builtin_catalog(tmp_path):
    spec = _write(tmp_path, "guarded.fsl", BASE)
    mutants = _jsonl(tmp_path, _records())

    out = mutate_file(
        str(spec),
        depth=3,
        max_mutants=1,
        external_mutants=str(mutants),
    )

    assert out["summary"]["total"] == 4
    assert out["summary"]["by_source"]["builtin"]["total"] == 1
    assert out["summary"]["by_source"]["external"]["total"] == 3
    assert {item["source"] for item in out["mutants"]} == {"builtin", "external"}


def test_malformed_json_and_ambiguous_replacement_are_invalid(tmp_path):
    spec = _write(
        tmp_path,
        "guarded.fsl",
        BASE + "\n// duplicate marker\n// duplicate marker\n",
    )
    mutants = _jsonl(tmp_path, [
        "{not-json",
        {
            "id": "ambiguous",
            "replace": {"target": "// duplicate marker", "replacement": "// changed"},
        },
    ])

    out = mutate_file(str(spec), depth=2, max_mutants=0, external_mutants=str(mutants))

    assert [item["status"] for item in out["mutants"]] == ["invalid", "invalid"]
    assert [item["invalid"]["kind"] for item in out["mutants"]] == ["json", "instruction"]
    assert out["summary"]["kill_rate"] is None


def test_external_mutant_with_different_spec_name_is_invalid(tmp_path):
    spec = _write(tmp_path, "guarded.fsl", BASE)
    other = textwrap.dedent(BASE).replace("spec Guarded", "spec Other")
    mutants = _jsonl(tmp_path, [{"id": "other", "mutated_spec": other}])

    out = mutate_file(str(spec), depth=2, max_mutants=0, external_mutants=str(mutants))

    assert out["mutants"][0]["status"] == "invalid"
    assert out["mutants"][0]["invalid"]["kind"] == "spec_name"


def test_external_type_error_is_invalid_not_killed(tmp_path):
    spec = _write(tmp_path, "guarded.fsl", BASE)
    invalid_type = textwrap.dedent(BASE).replace("x: Count", "x: Missing")
    mutants = _jsonl(tmp_path, [{"id": "bad-type", "mutated_spec": invalid_type}])

    out = mutate_file(str(spec), depth=2, max_mutants=0, external_mutants=str(mutants))

    mutant = out["mutants"][0]
    assert mutant["status"] == "invalid"
    assert mutant["invalid"]["kind"] == "type"
    assert out["summary"]["killed"] == 0


def test_external_mutant_is_killed_by_forbidden_oracle(tmp_path):
    guarded = """
    requirements GuardedReq {
      type OrderId = 0..0
      enum OSt { Cart, Paid, Shipped, Cancelled }
      state { order: Map<OrderId, OSt> }
      init { forall o: OrderId { order[o] = Cart } }
      requirement REQ-1 "lifecycle" {
        action pay(o: OrderId) { requires order[o] == Cart  order[o] = Paid }
        action ship(o: OrderId) { requires order[o] == Paid  order[o] = Shipped }
        action cancel(o: OrderId) { requires order[o] == Paid  order[o] = Cancelled }
      }
      forbidden FB-1 "post-shipment cancellation is rejected" {
        pay(0) ship(0) cancel(0)
        expect rejected
      }
    }
    """
    weakened = textwrap.dedent(guarded).replace(
        "requires order[o] == Paid  order[o] = Cancelled",
        "requires order[o] == Paid or order[o] == Shipped  order[o] = Cancelled",
    )
    spec = _write(tmp_path, "guarded.fsl", guarded)
    mutants = _jsonl(tmp_path, [{"id": "forbidden", "mutated_spec": weakened}])

    out = mutate_file(str(spec), depth=3, max_mutants=0, external_mutants=str(mutants))

    mutant = out["mutants"][0]
    assert mutant["status"] == "killed"
    assert mutant["killed_by"] == "forbidden"


def test_mutate_from_cli_contract_and_output(tmp_path):
    spec = _write(tmp_path, "guarded.fsl", BASE)
    mutants = _jsonl(tmp_path, _records())
    args = _build_arg_parser().parse_args([
        "mutate", str(spec), "--from", str(mutants), "--max-mutants", "0",
    ])

    assert args.mutants_from == str(mutants)
    out = run_mutate(
        str(spec),
        depth=3,
        max_mutants=0,
        external_mutants=str(mutants),
    )
    assert out["result"] == "mutated"
    assert out["summary"]["invalid"] == 1


def test_mutate_from_cli_subprocess(tmp_path):
    spec = _write(tmp_path, "guarded.fsl", BASE)
    mutants = _jsonl(tmp_path, _records())
    proc = subprocess.run(
        [
            sys.executable,
            "-m",
            "fslc",
            "mutate",
            str(spec),
            "--from",
            str(mutants),
            "--max-mutants",
            "0",
            "--depth",
            "3",
        ],
        cwd=ROOT,
        capture_output=True,
        text=True,
        check=False,
    )

    out = json.loads(proc.stdout)
    assert proc.returncode == 0
    assert out["result"] == "mutated"
    assert out["summary"]["by_source"]["external"]["total"] == 3
