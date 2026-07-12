# SPDX-License-Identifier: Apache-2.0
# Copyright 2026 Ryoichi Izumita

"""Inventory the Python surface AST that the Rust port must reproduce."""
from __future__ import annotations

import argparse
import json
import sys
from collections import Counter
from functools import lru_cache
from pathlib import Path
from typing import Any


ROOT = Path(__file__).resolve().parents[1]
if str(ROOT) not in sys.path:
    sys.path.insert(0, str(ROOT))

from tools.export_ast import export_corpus  # noqa: E402


SCHEMA = "fsl-rust-port-inventory.v1"
SHARED_AST_TAGS = frozenset(
    """
    __spec_meta abs acceptance acceptance_expect acceptance_expect_stage
    acceptance_step action action_map assign binder_collection binder_range
    binder_typed bin biz_actor biz_control biz_goal biz_goal_all_stage
    biz_goal_some_stage biz_initial biz_kpi biz_policy biz_policy_eventually
    biz_process biz_stages biz_transition bool branch branches business call
    compose const control_applies_to control_owner control_severity count
    deadline decl def def_param ensures entity enum exactly_one exists field
    field_lv forall forall_stmt forbidden gov_authority gov_delegates
    gov_preservation gov_require gov_satisfaction governance if impl implements
    index init int internal invariant is ite leadsto let map maps method min name
    neg none not num number old option param_range param_typed pat_some policy
    preservation_after preservation_before preservation_preserve
    preservation_refinement preserve_progress proc_assign proc_field proc_fields
    progress_respond qname reachable refinement refinement_param req_action
    requirement requirements requires seq seq_lit set set_lit some spec state
    struct struct_lit stutter sum sync_action sync_ref terminal time time_age
    time_urgent trans type unique until use var verify_bounds verify_instances
    verify_values
    """.split()
)


def _node_tags(value: Any, out: Counter[str], *, shared: bool) -> None:
    if isinstance(value, list):
        if shared and value and isinstance(value[0], str) and value[0] in SHARED_AST_TAGS:
            out[value[0]] += 1
        for item in value:
            _node_tags(item, out, shared=shared)
    elif isinstance(value, dict):
        if isinstance(value.get("$type"), str):
            out[f"typed:{value['$type']}"] += 1
        for item in value.values():
            _node_tags(item, out, shared=shared)


@lru_cache(maxsize=4)
def inventory(root: Path) -> dict[str, Any]:
    corpus = export_corpus(root, stage="surface")
    statuses: Counter[str] = Counter()
    frontends: Counter[str] = Counter()
    top_levels: Counter[str] = Counter()
    node_tags: Counter[str] = Counter()
    errors: list[dict[str, Any]] = []
    for entry in corpus["files"]:
        statuses[entry["status"]] += 1
        if entry["status"] == "ok":
            frontends[entry["frontend"]] += 1
            ast = entry["ast"]
            if entry["frontend"] == "shared" and isinstance(ast, list) and ast:
                top_levels[str(ast[0])] += 1
            _node_tags(ast, node_tags, shared=entry["frontend"] == "shared")
        elif entry["status"] == "evidence_only":
            frontends[entry["frontend"]] += 1
        else:
            errors.append({"path": entry["path"], **entry["error"]})
    return {
        "schema": SCHEMA,
        "total_files": len(corpus["files"]),
        "statuses": dict(sorted(statuses.items())),
        "frontends": dict(sorted(frontends.items())),
        "shared_top_levels": dict(sorted(top_levels.items())),
        "surface_node_tags": dict(sorted(node_tags.items())),
        "errors": errors,
    }


def main(argv: list[str] | None = None) -> int:
    parser = argparse.ArgumentParser(description=__doc__)
    parser.add_argument("--root", type=Path, default=ROOT)
    parser.add_argument("--check", type=Path, help="compare with a committed inventory")
    args = parser.parse_args(argv)
    result = inventory(args.root)
    print(json.dumps(result, indent=2, sort_keys=True, ensure_ascii=False))
    if args.check is None:
        return 0
    expected = json.loads(args.check.read_text(encoding="utf-8"))
    if result != expected:
        print(
            "Rust port surface inventory changed; update the parser scope and reviewed inventory",
            file=sys.stderr,
        )
        return 1
    return 0


if __name__ == "__main__":
    raise SystemExit(main())
