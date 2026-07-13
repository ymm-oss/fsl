# SPDX-License-Identifier: Apache-2.0
# Copyright 2026 Ryoichi Izumita

"""Export the authoritative Python FSL AST as deterministic JSON.

The Rust port uses this program as a differential oracle.  It intentionally
exports the post-frontend kernel AST returned by :func:`fslc.parser.parse_src`:
that is the representation consumed by ``build_spec`` and therefore the most
useful compatibility seam between the two implementations.

Evidence-only AI project/agent files have no kernel AST by design.  They are
recorded explicitly instead of being silently skipped.
"""
from __future__ import annotations

import argparse
import dataclasses
import hashlib
import json
from pathlib import Path
from typing import Any, Iterable

from lark.exceptions import UnexpectedInput, VisitError

from fslc.ai_parser import is_ai_agent_source, is_ai_component_source, parse_ai_component
from fslc.ai_project import is_ai_project_source
from fslc.db_parser import is_dbsystem_source, parse_dbsystem
from fslc.domain_parser import is_domain_source, parse_domain
from fslc.model import FslError
from fslc.parser import parse_expr, parse_src, parse_surface_src


SCHEMA = "fsl-python-ast.v1"


def canonical(value: Any) -> Any:
    """Convert tuple AST values into a stable JSON-compatible representation."""
    if isinstance(value, tuple):
        return [canonical(item) for item in value]
    if isinstance(value, list):
        return [canonical(item) for item in value]
    if isinstance(value, dict):
        return {str(key): canonical(value[key]) for key in sorted(value, key=str)}
    if isinstance(value, set):
        return sorted((canonical(item) for item in value), key=_sort_key)
    if dataclasses.is_dataclass(value) and not isinstance(value, type):
        return {
            "$type": type(value).__name__,
            **{
                field.name: canonical(getattr(value, field.name))
                for field in dataclasses.fields(value)
            },
        }
    if value is None or isinstance(value, (bool, int, float, str)):
        return value
    raise TypeError(f"AST exporter does not support {type(value).__name__}")


def _sort_key(value: Any) -> str:
    return json.dumps(value, sort_keys=True, ensure_ascii=True, separators=(",", ":"))


def _source_hash(source: str) -> str:
    return hashlib.sha256(source.encode("utf-8")).hexdigest()


def _error_entry(base: dict[str, Any], exc: FslError | UnexpectedInput) -> dict[str, Any]:
    if isinstance(exc, UnexpectedInput):
        loc = {"line": exc.line, "column": exc.column}
        kind = "parse"
    else:
        loc = exc.loc
        kind = exc.kind
    return {
        **base,
        "status": "error",
        "error": {"kind": kind, "loc": canonical(loc)},
    }


def export_expression(source: str) -> dict[str, Any]:
    return {
        "schema": SCHEMA,
        "kind": "expression",
        "source_sha256": _source_hash(source),
        "ast": canonical(parse_expr(source)),
    }


def export_file(
    path: Path,
    *,
    root: Path | None = None,
    stage: str = "kernel",
) -> dict[str, Any]:
    source = path.read_text(encoding="utf-8")
    rel = path.resolve().as_posix()
    if root is not None:
        try:
            rel = path.resolve().relative_to(root.resolve()).as_posix()
        except ValueError:
            pass

    base = {
        "path": rel,
        "source_sha256": _source_hash(source),
    }
    if is_ai_project_source(source):
        return {**base, "status": "evidence_only", "frontend": "ai-project"}
    if is_ai_agent_source(source):
        return {**base, "status": "evidence_only", "frontend": "ai-agent"}

    try:
        if stage == "surface":
            if is_dbsystem_source(source):
                ast = parse_dbsystem(source)
                frontend = "db"
            elif is_ai_component_source(source):
                ast = parse_ai_component(source)
                frontend = "ai-component"
            elif is_domain_source(source):
                ast = parse_domain(source)
                frontend = "domain"
            else:
                ast = parse_surface_src(source)
                frontend = "shared"
            display_names = {}
        elif stage == "kernel":
            ast, display_names = parse_src(source, base_dir=path.parent)
            frontend = "kernel"
        else:
            raise ValueError(f"unknown AST stage: {stage}")
    except VisitError as exc:
        if isinstance(exc.orig_exc, FslError):
            return _error_entry(base, exc.orig_exc)
        raise
    except (FslError, UnexpectedInput) as exc:
        return _error_entry(base, exc)

    return {
        **base,
        "status": "ok",
        "stage": stage,
        "frontend": frontend,
        "ast": canonical(ast),
        "display_names": canonical(display_names),
    }


def corpus_paths(root: Path) -> list[Path]:
    return sorted({*(root / "specs").glob("*.fsl"), *(root / "examples").rglob("*.fsl")})


def export_corpus(
    root: Path,
    paths: Iterable[Path] | None = None,
    *,
    stage: str = "kernel",
) -> dict[str, Any]:
    selected = list(paths) if paths is not None else corpus_paths(root)
    files = [export_file(path, root=root, stage=stage) for path in selected]
    counts: dict[str, int] = {}
    for entry in files:
        status = str(entry["status"])
        counts[status] = counts.get(status, 0) + 1
    return {
        "schema": SCHEMA,
        "kind": "corpus",
        "stage": stage,
        "root": ".",
        "counts": {key: counts[key] for key in sorted(counts)},
        "files": files,
    }


def _parser() -> argparse.ArgumentParser:
    parser = argparse.ArgumentParser(description=__doc__)
    mode = parser.add_mutually_exclusive_group(required=True)
    mode.add_argument("--expr", help="export one standalone expression")
    mode.add_argument("--file", type=Path, help="export one FSL file")
    mode.add_argument("--corpus", action="store_true", help="export specs/ and examples/")
    parser.add_argument("--root", type=Path, default=Path.cwd(), help="repository root")
    parser.add_argument(
        "--stage",
        choices=("surface", "kernel"),
        default="kernel",
        help="surface parser IR or lowered kernel AST (file/corpus modes)",
    )
    parser.add_argument("--compact", action="store_true", help="emit compact JSON")
    return parser


def main(argv: list[str] | None = None) -> int:
    args = _parser().parse_args(argv)
    if args.expr is not None:
        result = export_expression(args.expr)
    elif args.file is not None:
        result = {
            "schema": SCHEMA,
            "kind": "file",
            **export_file(args.file, root=args.root, stage=args.stage),
        }
    else:
        result = export_corpus(args.root, stage=args.stage)
    if args.compact:
        print(json.dumps(result, sort_keys=True, ensure_ascii=False, separators=(",", ":")))
    else:
        print(json.dumps(result, indent=2, sort_keys=True, ensure_ascii=False))
    return 0


if __name__ == "__main__":
    raise SystemExit(main())
