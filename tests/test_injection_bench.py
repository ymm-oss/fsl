from __future__ import annotations

import json
import subprocess
import sys
from collections import Counter, defaultdict
from pathlib import Path
from typing import Any


ROOT = Path(__file__).resolve().parents[1]
INJECTED = ROOT / "examples" / "gallery" / "injected"
MATRIX_PATH = INJECTED / "MATRIX.json"
PY = ROOT / ".venv" / "bin" / "python"

DEPTH = "4"
MUTATE_DEPTH = "2"
MUTATE_MAX = "8"
DIFF_MUTATE_DEPTH = "3"
DIFF_MUTATE_MAX = "30"

EXPECTED_TYPES = {
    "omission",
    "boundary-flip",
    "guard-weakening",
    "invariant-weakening",
    "unreachable-antecedent",
    "fabricated-constraint",
    "over-strengthened-guard",
}

BASELINE_FOR = {
    "specs/bank.fsl": ROOT / "specs" / "bank.fsl",
    "specs/order_workflow.fsl": ROOT / "specs" / "order_workflow.fsl",
    "examples/layers/return_system.fsl": ROOT / "examples" / "layers" / "return_system.fsl",
}

PRIMARY_DETECTOR = {
    "omission": "strict_tags_requirements",
    "boundary-flip": "forbidden_acceptance",
    "guard-weakening": "forbidden_acceptance",
    "invariant-weakening": "mutate",
    "unreachable-antecedent": "vacuity",
    "fabricated-constraint": "strict_tags",
    "over-strengthened-guard": "verify",
}

BLIND_DETECTOR = {
    "omission": "strict_tags",
    "boundary-flip": "strict_tags",
    "guard-weakening": "strict_tags",
    "invariant-weakening": "verify",
    "unreachable-antecedent": "strict_tags",
    "fabricated-constraint": "vacuity",
    "over-strengthened-guard": "strict_tags",
}


def _run(args: list[str]) -> dict[str, Any]:
    proc = subprocess.run(
        [str(PY), "-m", "fslc", *args],
        cwd=ROOT,
        text=True,
        stdout=subprocess.PIPE,
        stderr=subprocess.PIPE,
        check=False,
    )
    try:
        out = json.loads(proc.stdout)
    except json.JSONDecodeError as exc:
        raise AssertionError(
            f"fslc returned non-JSON for {' '.join(args)}; "
            f"exit={proc.returncode}; stdout={proc.stdout!r}; stderr={proc.stderr!r}"
        ) from exc
    out["_exit"] = proc.returncode
    return out


def _headers(path: Path) -> dict[str, str]:
    headers: dict[str, str] = {}
    for line in path.read_text(encoding="utf-8").splitlines()[:8]:
        stripped = line.strip()
        if not stripped.startswith("//"):
            continue
        body = stripped[2:].strip()
        if ":" in body:
            key, value = body.split(":", 1)
            headers[key.strip()] = value.strip()
    return headers


def _warning_names(out: dict[str, Any], kind: str) -> set[str]:
    return {
        str(w.get("name"))
        for w in out.get("warnings", [])
        if isinstance(w, dict) and w.get("kind") == kind
    }


def _registry_for(case: dict[str, str], tmp_path: Path) -> Path:
    expected = case["expect-signal"].split(":", 1)[-1]
    prefix = expected.split("-", 1)[0] + "-"
    ids = [
        line.strip()
        for line in (INJECTED / "ids.txt").read_text(encoding="utf-8").splitlines()
        if line.strip().startswith(prefix)
    ]
    registry = tmp_path / f"{prefix.lower().strip('-')}_ids.txt"
    registry.write_text("\n".join(ids) + "\n", encoding="utf-8")
    return registry


def _summarize_mutate(out: dict[str, Any]) -> dict[str, Any]:
    summary = out.get("summary") or {}
    return {
        "result": out.get("result"),
        "survived": summary.get("survived"),
        "killed": summary.get("killed"),
        "total": summary.get("total"),
    }


def _cell(status: str, signal: str | None = None, condition: str | None = None) -> dict[str, Any]:
    cell: dict[str, Any] = {"status": status}
    if signal:
        cell["signal"] = signal
    if condition:
        cell["condition"] = condition
    return cell


def _measure_case(path: Path, case: dict[str, str], tmp_path: Path) -> dict[str, Any]:
    rel = path.relative_to(ROOT).as_posix()
    cells: dict[str, dict[str, Any]] = {}

    check = _run(["check", str(path)])
    if check.get("result") == "error" and check.get("kind") in {"acceptance", "forbidden"}:
        cells["forbidden_acceptance"] = _cell(
            "caught", f"{check.get('kind')}:{check.get('id')}"
        )
    else:
        cells["forbidden_acceptance"] = _cell("not-caught", str(check.get("result")))

    verify = _run(["verify", str(path), "--depth", DEPTH, "--deadlock", "ignore"])
    if verify.get("result") == "reachable_failed":
        names = ",".join(item.get("name", "") for item in verify.get("unreached", []))
        cells["verify"] = _cell("caught", f"reachable_failed:{names}")
    elif verify.get("result") == "violated":
        cells["verify"] = _cell("caught", f"violated:{verify.get('violation_kind')}")
    else:
        cells["verify"] = _cell("not-caught", str(verify.get("result")))

    vacuity = _run([
        "verify",
        str(path),
        "--depth",
        DEPTH,
        "--deadlock",
        "ignore",
        "--vacuity",
        "error",
    ])
    if vacuity.get("result") == "error" and str(vacuity.get("kind", "")).startswith("vacuous_"):
        names = ",".join(f.get("name", "") for f in vacuity.get("findings", []))
        cells["vacuity"] = _cell("caught", f"{vacuity.get('kind')}:{names}")
    else:
        cells["vacuity"] = _cell("not-caught", str(vacuity.get("kind") or vacuity.get("result")))

    strict = _run(["check", str(path), "--strict-tags"])
    untagged = _warning_names(strict, "untagged")
    cells["strict_tags"] = (
        _cell("caught", "untagged:" + ",".join(sorted(untagged)))
        if untagged
        else _cell("not-caught", str(strict.get("result")))
    )

    registry = _registry_for(case, tmp_path)
    strict_req = _run(["check", str(path), "--strict-tags", "--requirements", str(registry)])
    unreferenced = _warning_names(strict_req, "unreferenced_requirement")
    cells["strict_tags_requirements"] = (
        _cell(
            "caught",
            "unreferenced_requirement:" + ",".join(sorted(unreferenced)),
            "per-domain ids registry derived from examples/gallery/injected/ids.txt",
        )
        if unreferenced
        else _cell(
            "not-caught",
            str(strict_req.get("result")),
            "per-domain ids registry derived from examples/gallery/injected/ids.txt",
        )
    )

    mutate = _run(["mutate", str(path), "--depth", MUTATE_DEPTH, "--max-mutants", MUTATE_MAX])
    cells["mutate"] = _cell("not-caught", json.dumps(_summarize_mutate(mutate), sort_keys=True))

    if case["inject"] == "invariant-weakening":
        baseline = _run([
            "mutate",
            str(BASELINE_FOR[case["base"]]),
            "--depth",
            DIFF_MUTATE_DEPTH,
            "--max-mutants",
            DIFF_MUTATE_MAX,
        ])
        injected = _run([
            "mutate",
            str(path),
            "--depth",
            DIFF_MUTATE_DEPTH,
            "--max-mutants",
            DIFF_MUTATE_MAX,
        ])
        base_survived = (baseline.get("summary") or {}).get("survived", 0)
        injected_survived = (injected.get("summary") or {}).get("survived", 0)
        status = "caught" if injected_survived > base_survived else "not-caught"
        cells["mutate"] = _cell(
            status,
            f"survivors {base_survived}->{injected_survived}",
            "baseline differential",
        )

    return {
        "file": rel,
        "base": case["base"],
        "inject": case["inject"],
        "expect_detector": case["expect-detector"],
        "expect_signal": case["expect-signal"],
        "note": case["note"],
        "detectors": cells,
    }


def _summarize(cases: list[dict[str, Any]]) -> dict[str, Any]:
    by_type: dict[str, dict[str, dict[str, int]]] = defaultdict(lambda: defaultdict(Counter))
    surprises: list[dict[str, str]] = []

    for case in cases:
        inject = case["inject"]
        primary = PRIMARY_DETECTOR[inject]
        for detector, cell in case["detectors"].items():
            by_type[inject][detector][cell["status"]] += 1
        if case["detectors"][primary]["status"] != "caught":
            surprises.append({
                "file": case["file"],
                "kind": "missing-primary",
                "detector": primary,
                "actual": case["detectors"][primary]["status"],
            })
        for detector, cell in case["detectors"].items():
            if detector != primary and cell["status"] == "caught":
                surprises.append({
                    "file": case["file"],
                    "kind": "unexpected-cross-catch",
                    "detector": detector,
                    "signal": str(cell.get("signal")),
                })

    return {
        "summary_by_injection": {
            inject: {
                detector: dict(counts)
                for detector, counts in detectors.items()
            }
            for inject, detectors in sorted(by_type.items())
        },
        "surprises": surprises,
    }


def test_error_injection_benchmark_matrix(tmp_path):
    paths = sorted(INJECTED.glob("*.fsl"))
    cases: list[dict[str, Any]] = []
    failures: list[str] = []

    try:
        headers_by_path = {path: _headers(path) for path in paths}
        bases = {headers.get("base") for headers in headers_by_path.values()}
        injects = {headers.get("inject") for headers in headers_by_path.values()}

        if len(paths) != 21:
            failures.append(f"expected 21 injected specs, found {len(paths)}")
        if len(bases) < 3:
            failures.append(f"expected at least 3 bases, found {sorted(bases)}")
        if injects != EXPECTED_TYPES:
            failures.append(f"unexpected injection types: {sorted(injects)}")

        for path, headers in headers_by_path.items():
            missing = {"base", "inject", "expect-detector", "expect-signal", "note"} - set(headers)
            if missing:
                failures.append(f"{path} missing headers: {sorted(missing)}")
                continue
            measured = _measure_case(path, headers, tmp_path)
            cases.append(measured)

            primary = PRIMARY_DETECTOR[headers["inject"]]
            if measured["detectors"][primary]["status"] != "caught":
                failures.append(
                    f"{path.name}: primary {primary} did not catch "
                    f"{headers['inject']}: {measured['detectors'][primary]}"
                )

            blind = BLIND_DETECTOR[headers["inject"]]
            if measured["detectors"][blind]["status"] == "caught":
                failures.append(
                    f"{path.name}: blind detector {blind} unexpectedly caught "
                    f"{headers['inject']}: {measured['detectors'][blind]}"
                )
    finally:
        matrix = {
            "detectors": [
                "forbidden_acceptance",
                "verify",
                "vacuity",
                "strict_tags",
                "strict_tags_requirements",
                "mutate",
            ],
            "cases": cases,
            **_summarize(cases),
        }
        MATRIX_PATH.write_text(json.dumps(matrix, indent=2, sort_keys=True) + "\n", encoding="utf-8")

    assert not failures, "\n".join(failures)
