# SPDX-License-Identifier: Apache-2.0
# Copyright 2026 Ryoichi Izumita

"""Project-manifest orchestration for cross-layer FSL verification."""
import subprocess
from pathlib import Path

try:
    import tomllib
except ImportError:  # pragma: no cover - exercised on Python 3.9/3.10
    import tomli as tomllib


LAYER_ORDER = ("business", "requirements", "design")
PASSING_RESULTS = {
    "ok",
    "verified",
    "proved",
    "refines",
    "conformant",
    "generated",
    "scenarios",
    "typestate",
    "mutated",
    "explained",
}


def _rel(path, base):
    p = Path(path)
    if not p.is_absolute():
        p = base / p
    return p


def _layer_status(result, code):
    if code == 0 and result.get("result") in PASSING_RESULTS:
        return "passed"
    return "failed"


def _detail_exit_code(detail, exit_code):
    code = exit_code(detail)
    impl = detail.get("implements")
    if not isinstance(impl, dict) or impl.get("result") == "refines":
        return code
    violation = impl.get("violation")
    if isinstance(violation, dict):
        return exit_code(violation)
    return 1


def _chain_result(layers, manifest, keep_going):
    failed = [layer for layer in layers if layer["status"] == "failed"]
    if not failed:
        result = "verified"
    elif any(layer.get("exit_code", 1) in (2, 3) for layer in failed):
        result = "error"
    else:
        result = "violated"

    out = {
        "fsl": "1.0",
        "result": result,
        "manifest": str(manifest),
        "keep_going": keep_going,
        "layers": layers,
    }
    if failed:
        out["failed"] = [layer["layer"] for layer in failed]
        if result == "error":
            out["kind"] = "chain"
            out["message"] = "one or more chain layers returned an error"
    return out


def _error(message, manifest=None, kind="io"):
    out = {"fsl": "1.0", "result": "error", "kind": kind, "message": message}
    if manifest is not None:
        out["manifest"] = str(manifest)
    return out


def _load_manifest(path):
    try:
        with open(path, "rb") as fh:
            data = tomllib.load(fh)
    except FileNotFoundError:
        return None, _error(f"file not found: {path}", path)
    except tomllib.TOMLDecodeError as e:
        return None, _error(f"invalid TOML: {e}", path, kind="parse")
    if not isinstance(data, dict):
        return None, _error("project manifest must be a TOML table", path, kind="parse")
    return data, None


def _runner_functions():
    from .cli import exit_code, run_check, run_refine, run_verify

    return run_check, run_verify, run_refine, exit_code


def _run_spec_layer(layer, cfg, base, run_check, run_verify, exit_code):
    file = cfg.get("file")
    if not file:
        detail = {"result": "error", "kind": "io", "message": f"[{layer}] file is required"}
        return {
            "layer": layer,
            "kind": "check",
            "status": "failed",
            "result": "error",
            "exit_code": 2,
            "detail": detail,
        }

    path = _rel(file, base)
    if "depth" in cfg:
        depth = int(cfg["depth"])
        detail = run_verify(str(path), depth, cfg.get("deadlock", "warn"))
        kind = "verify"
    else:
        depth = None
        detail = run_check(str(path))
        kind = "check"

    code = _detail_exit_code(detail, exit_code)
    entry = {
        "layer": layer,
        "kind": kind,
        "file": str(path),
        "status": _layer_status(detail, code),
        "result": detail.get("result"),
        "exit_code": code,
        "detail": detail,
    }
    if depth is not None:
        entry["depth"] = depth
    return entry


def _run_refinement_layer(layer, cfg, target, target_cfg, base, run_refine, exit_code):
    mapping = cfg.get("mapping")
    if not mapping:
        detail = {
            "result": "error",
            "kind": "io",
            "message": f"[{layer}] mapping is required when refine_against is set",
        }
        return {
            "layer": f"{layer}->{target}",
            "kind": "refine",
            "status": "failed",
            "result": "error",
            "exit_code": 2,
            "detail": detail,
        }

    path = _rel(cfg["file"], base)
    target_path = _rel(target_cfg["file"], base)
    mapping_path = _rel(mapping, base)
    depth = int(cfg.get("refine_depth", cfg.get("depth", target_cfg.get("depth", 8))))
    detail = run_refine(str(path), str(target_path), str(mapping_path), depth=depth)
    code = exit_code(detail)
    return {
        "layer": f"{layer}->{target}",
        "kind": "refine",
        "file": str(path),
        "against": target,
        "abs_file": str(target_path),
        "mapping": str(mapping_path),
        "depth": depth,
        "status": _layer_status(detail, code),
        "result": detail.get("result"),
        "exit_code": code,
        "detail": detail,
    }


def _run_impl(cfg, base):
    command = cfg.get("command")
    if not command:
        detail = {"result": "error", "kind": "io", "message": "[impl] command is required"}
        return {
            "layer": "impl",
            "kind": "command",
            "status": "failed",
            "result": "error",
            "exit_code": 2,
            "detail": detail,
        }

    completed = subprocess.run(
        command,
        shell=True,
        cwd=str(base),
        text=True,
        stdout=subprocess.PIPE,
        stderr=subprocess.PIPE,
        check=False,
    )
    passed = completed.returncode == 0
    detail = {
        "result": "passed" if passed else "failed",
        "command": command,
        "returncode": completed.returncode,
        "stdout": completed.stdout,
        "stderr": completed.stderr,
    }
    return {
        "layer": "impl",
        "kind": "command",
        "command": command,
        "status": "passed" if passed else "failed",
        "result": detail["result"],
        "exit_code": 0 if passed else 1,
        "detail": detail,
    }


def _planned_steps(manifest):
    steps = []
    for layer in LAYER_ORDER:
        cfg = manifest.get(layer)
        if cfg is None:
            continue
        steps.append(("spec", layer))
        if cfg.get("refine_against"):
            steps.append(("refine", layer))
    if manifest.get("impl") is not None:
        steps.append(("impl", "impl"))
    return steps


def _skipped_entries(steps, manifest):
    entries = []
    for kind, layer in steps:
        name = layer
        if kind == "refine":
            name = f"{layer}->{manifest.get(layer, {}).get('refine_against')}"
        entries.append({
            "layer": name,
            "kind": kind,
            "status": "skipped",
            "result": "skipped",
            "exit_code": 0,
        })
    return entries


def run_chain(manifest_path="fsl-project.toml", keep_going=False, runners=None):
    manifest_file = Path(manifest_path)
    manifest, error = _load_manifest(manifest_file)
    if error:
        return error

    if runners is None:
        run_check, run_verify, run_refine, exit_code = _runner_functions()
    else:
        run_check, run_verify, run_refine, exit_code = runners

    base = manifest_file.parent
    steps = _planned_steps(manifest)
    layers = []

    for idx, (kind, layer) in enumerate(steps):
        cfg = manifest.get(layer)
        if kind == "spec":
            entry = _run_spec_layer(layer, cfg, base, run_check, run_verify, exit_code)
        elif kind == "refine":
            target = cfg.get("refine_against")
            target_cfg = manifest.get(target)
            if target not in LAYER_ORDER or not target_cfg or not target_cfg.get("file"):
                detail = {
                    "result": "error",
                    "kind": "io",
                    "message": f"[{layer}] unknown refine_against layer: {target}",
                }
                entry = {
                    "layer": f"{layer}->{target}",
                    "kind": "refine",
                    "status": "failed",
                    "result": "error",
                    "exit_code": 2,
                    "detail": detail,
                }
            else:
                entry = _run_refinement_layer(
                    layer, cfg, target, target_cfg, base, run_refine, exit_code)
        else:
            entry = _run_impl(cfg, base)

        layers.append(entry)
        if entry["status"] == "failed" and not keep_going:
            layers.extend(_skipped_entries(steps[idx + 1:], manifest))
            break

    return _chain_result(layers, manifest_file, keep_going)


def format_chain_table(result):
    rows = [("Layer", "Check", "Status", "Result", "Detail")]
    for layer in result.get("layers", []):
        detail = []
        if "depth" in layer:
            detail.append(f"depth={layer['depth']}")
        if layer.get("command"):
            detail.append(f"exit={layer.get('detail', {}).get('returncode', layer.get('exit_code'))}")
        rows.append((
            layer.get("layer", ""),
            layer.get("kind", ""),
            layer.get("status", ""),
            str(layer.get("result", "")),
            ", ".join(detail) if detail else "-",
        ))

    widths = [max(len(str(row[i])) for row in rows) for i in range(len(rows[0]))]
    lines = []
    for i, row in enumerate(rows):
        lines.append("  ".join(str(col).ljust(widths[j]) for j, col in enumerate(row)))
        if i == 0:
            lines.append("  ".join("-" * width for width in widths))
    return "\n".join(lines)
