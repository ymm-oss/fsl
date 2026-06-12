from __future__ import annotations

import json
import random
import shlex
import subprocess
import textwrap
from pathlib import Path
from typing import Any

from fslc.cli import exit_code, run_verify
from oracle import PYTHON, ROOT
from test_gallery import GALLERY, GALLERY_CASES
from test_oracle_agreement import oracle_cases


KNOWN_RESULTS = {
    "ok",
    "verified",
    "proved",
    "violated",
    "reachable_failed",
    "unknown_cti",
    "error",
    "refines",
    "refinement_failed",
}

HYPOTHESIS_MODE = "fixed-seed deterministic generator"


def _assert_no_internal_names(value: Any):
    if isinstance(value, dict):
        for key, item in value.items():
            assert "__" not in str(key), value
            _assert_no_internal_names(item)
    elif isinstance(value, list):
        for item in value:
            _assert_no_internal_names(item)
    elif isinstance(value, str):
        assert "__" not in value, value


def _argv_for_gallery(case):
    path = GALLERY / case.path
    parts = shlex.split(case.command)
    if parts[0] == "check":
        return [str(PYTHON), "-m", "fslc", "check", str(path), *parts[1:]]
    if parts[0] == "verify":
        return [str(PYTHON), "-m", "fslc", "verify", str(path), *parts[1:]]
    if parts[0] == "refine":
        return [
            str(PYTHON),
            "-m",
            "fslc",
            "refine",
            str(path.parent / parts[1]),
            str(path.parent / parts[2]),
            str(path.parent / parts[3]),
            *parts[4:],
        ]
    raise AssertionError(parts)


def _run_json(argv):
    proc = subprocess.run(
        argv,
        cwd=ROOT,
        text=True,
        stdout=subprocess.PIPE,
        stderr=subprocess.PIPE,
        check=False,
    )
    out = json.loads(proc.stdout)
    return proc.returncode, out


def test_verify_corpus_results_are_serializable_known_and_public():
    for case in oracle_cases():
        result = run_verify(str(case.path), case.depth, deadlock_mode=case.deadlock, engine=case.engine)
        json.dumps(result, sort_keys=True)
        assert result["result"] in KNOWN_RESULTS, (case.id, result)
        assert exit_code(result) in {0, 1, 2, 3}, (case.id, result)
        _assert_no_internal_names(result)


def test_gallery_cli_exit_codes_match_json_results():
    for case in GALLERY_CASES:
        rc, out = _run_json(_argv_for_gallery(case))
        json.dumps(out, sort_keys=True)
        assert out["result"] in KNOWN_RESULTS, (case.path, out)
        assert rc == exit_code(out), (case.path, rc, out)
        _assert_no_internal_names(out)


def _generated_spec(seed: int) -> str:
    rng = random.Random(seed)
    cap = rng.randint(1, 3)
    include_dec = rng.choice([True, False])
    invariant_name = f"Within{seed}"
    dec = ""
    if include_dec:
        dec = "  action dec() { requires n > 0  n = n - 1 }\n"
    return textwrap.dedent(
        f"""
        spec Generated{seed} {{
          type N = 0..{cap}
          state {{ n: N }}
          init {{ n = 0 }}
          action inc() {{ requires n < {cap}  n = n + 1 }}
        {dec}  invariant {invariant_name} {{ n >= 0 and n <= {cap} }}
          reachable Top {{ n == {cap} }}
        }}
        """
    )


def test_fixed_seed_generated_specs_have_stable_json_contract(tmp_path):
    assert HYPOTHESIS_MODE == "fixed-seed deterministic generator"
    for seed in range(8):
        path = tmp_path / f"generated_{seed}.fsl"
        path.write_text(_generated_spec(seed), encoding="utf-8")
        result = run_verify(str(path), depth=4, deadlock_mode="ignore")
        json.dumps(result, sort_keys=True)
        assert result["result"] in {"verified", "reachable_failed"}, result
        assert exit_code(result) in {0, 1}
        _assert_no_internal_names(result)
