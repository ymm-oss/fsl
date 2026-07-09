# SPDX-License-Identifier: Apache-2.0
# Copyright 2026 Ryoichi Izumita

"""Command-line entry point for fslc."""
import sys
import json
import argparse
import re
import itertools

from lark.exceptions import UnexpectedInput, VisitError

from pathlib import Path

from .diagnostics import with_faithfulness, trace_type_for
from .parser import parse, parse_src, parse_refinement
from .model import build_spec, check_spec, FslError, strict_tag_warnings
from .bmc import verify, prove, scenarios
from .refine import build_refinement, refine, refine_chain
from .runtime import Monitor
from .acceptance import validate_acceptance, validate_forbidden
from .testgen import TestgenScenarioError, generate_test_bundle, default_output_name
from .typestate import analyze as analyze_typestate
from .mutate import DEFAULT_MAX_MUTANTS, mutate_file
from .explain import explain_file
from .html_report import (
    default_output_name as default_html_output_name,
    render_html_report,
)
from .ledger import (
    default_output_name as default_ledger_output_name,
    render_ledger,
)
from .analysis import (
    analyze as analyze_structure,
    analyze_projection,
    analyze_project_manifest,
    analyze_refinement_ast,
    build_tsg,
    export_graph,
)
from .analysis.projections import SUPPORTED_PROJECTIONS
from .analysis.schema import FINDINGS_SCHEMA_VERSION
from .db_check import check_dbsystem, load_dbsystem, observe_dbsystem
from .db_import import import_db_file
from .ai_check import check_ai_source, load_ai_source, replay_ai_events, select_ai_component
from .ai_parser import is_ai_agent_source
from .ai_project import AiProject, is_ai_project_source
from .ai_stochastic import (
    ai_compat_profile_from_file,
    compare_ai_records,
    drift_ai_project,
    evaluate_ai_project,
    load_ai_project_file,
    regress_ai_project,
)
from .domain_check import (
    analyze_domain,
    check_domain_source,
    expand_domain_source,
    generate_domain_scaffold,
    generate_domain_tests,
    load_domain,
    replay_domain_logs,
)
from .domain_testgen import default_domain_testgen_output

FSL_VERSION = "1.0"


def _envelope(result):
    out = {"fsl": FSL_VERSION}
    out.update(result)
    out = with_faithfulness(out)
    tt = trace_type_for(out)
    if tt is not None:
        out.setdefault("trace_type", tt)
    return out


def _error_envelope(kind, message, loc=None, expected=None, hint=None):
    err = {"result": "error", "kind": kind, "message": message}
    if kind in ("parse", "name", "type", "semantics") and loc:
        err["loc"] = loc
    elif loc:
        err["loc"] = loc
    if expected:
        err["expected"] = expected
    if hint:
        err["hint"] = hint
    return _envelope(err)


def _loc_from_exc(e):
    if getattr(e, "loc", None):
        return e.loc
    return None


def _parse_expected(e):
    for attr in ("expected", "allowed", "char_expected"):
        exp = getattr(e, attr, None)
        if exp:
            return "one of: " + ", ".join(sorted(str(x) for x in exp))
    return "valid FSL syntax"


_IDENTIFIER_CHARS = re.compile(r"[A-Za-z0-9_]")


def _invalid_identifier_near(e):
    src = getattr(e, "source", None)
    pos = getattr(e, "pos_in_stream", None)
    if src is None or pos is None or pos < 0 or pos >= len(src):
        return None

    bad = src[pos]
    if bad.isspace() or _IDENTIFIER_CHARS.fullmatch(bad):
        return None

    start = pos
    while start > 0 and _IDENTIFIER_CHARS.fullmatch(src[start - 1]):
        start -= 1
    end = pos + 1
    while end < len(src) and _IDENTIFIER_CHARS.fullmatch(src[end]):
        end += 1
    if start == pos and end == pos + 1:
        return None

    return src[start:end]


def _decreases_inside_forall(e):
    """True if the error token is a standalone 'decreases' keyword — the
    signature left when someone nests it inside a leadsTo's forall braces
    instead of placing it directly inside the leadsTo block."""
    src = getattr(e, "source", None)
    pos = getattr(e, "pos_in_stream", None)
    if src is None or pos is None or pos < 0:
        return False
    end = pos + len("decreases")
    if src[pos:end] != "decreases":
        return False
    if pos > 0 and _IDENTIFIER_CHARS.fullmatch(src[pos - 1]):
        return False
    if end < len(src) and _IDENTIFIER_CHARS.fullmatch(src[end]):
        return False
    return True


def _parse_error_result(e):
    loc = {"line": e.line, "column": e.column}
    near = _invalid_identifier_near(e)
    if near:
        return _envelope({
            "result": "error",
            "kind": "parse",
            "loc": loc,
            "message": (
                f"invalid identifier near '{near}': identifiers may contain "
                "letters, digits and '_', and must start with a letter or '_'"
            ),
        })
    if _decreases_inside_forall(e):
        return _envelope({
            "result": "error",
            "kind": "parse",
            "loc": loc,
            "message": "unexpected 'decreases' here",
            "hint": (
                "decreases belongs directly inside the leadsTo block, after the "
                "closing '}' of forall — not inside the forall body. Example: "
                "leadsTo L { forall c: Case { P ~> Q } decreases M }"
            ),
        })
    return _envelope({
        "result": "error",
        "kind": "parse",
        "loc": loc,
        "message": str(e).split("\n")[0],
        "expected": _parse_expected(e),
    })


def _parse_file(file, src, bounds_overrides=None):
    return parse_src(src, str(Path(file).parent), bounds_overrides)


def _read_requirement_ids(path):
    if path is None:
        return None
    try:
        with open(path, encoding="utf-8") as fh:
            return [line.strip() for line in fh if line.strip()]
    except FileNotFoundError:
        raise FslError(f"file not found: {path}", kind="io")


def _add_strict_tag_warnings(out, spec, strict_tags=False, requirements=None):
    if not strict_tags or out.get("result") not in ("ok", "verified", "proved"):
        return out
    out = dict(out)
    out.setdefault("warnings", [])
    out["warnings"] = list(out["warnings"]) + strict_tag_warnings(
        spec,
        _read_requirement_ids(requirements),
    )
    return out


def _implements_result(spec, depth=8):
    impl = spec.get("implements")
    if not impl:
        return None
    abs_spec = build_spec(impl["abs_ast"], impl.get("abs_display_names"))
    mapping = build_refinement(impl["mapping_ast"], spec, abs_spec)
    result = refine(spec, abs_spec, mapping, depth)
    if result.get("result") == "refines":
        return {"abs": abs_spec["name"], "result": "refines"}
    return {"abs": abs_spec["name"], "result": result.get("result"), "violation": result}


def _build_spec_from_file(path):
    src = open(path, encoding="utf-8").read()
    ast, display_names = _parse_file(path, src)
    return build_spec(ast, display_names)


def _governance_result(spec, depth=8):
    gov = spec.get("governance")
    if not gov:
        return None
    out = {
        "name": gov["name"],
        "controls": [control["id"] for control in gov.get("controls", [])],
        "delegates": [
            {
                "business": delegate["business"],
                "required": delegate["required"],
                "satisfied": delegate["satisfied"],
            }
            for delegate in gov.get("delegates", [])
        ],
        "preservations": [],
    }
    for preservation in gov.get("preservations", []):
        before_spec = _build_spec_from_file(preservation["before"]["path"])
        after_spec = _build_spec_from_file(preservation["after"]["path"])
        mapping_src = open(preservation["refinement"]["path"], encoding="utf-8").read()
        mapping_ast = parse_refinement(mapping_src)
        mapping = build_refinement(mapping_ast, after_spec, before_spec)
        result = refine(after_spec, before_spec, mapping, depth)
        entry = {
            "name": preservation["name"],
            "before": before_spec["name"],
            "after": after_spec["name"],
            "preserve": preservation["preserve"],
            "result": result.get("result"),
        }
        if result.get("result") != "refines":
            entry["violation"] = result
        out["preservations"].append(entry)
    return out


def _acceptance_error(spec, skip_out_of_range=False):
    """Returns (error_envelope_or_None, skipped_ids). `skipped_ids` lists
    acceptance ids downgraded from hard-error to skip because they reference
    ids/numbers outside CLI-overridden bounds (only possible when
    skip_out_of_range is True, i.e. --instances/--values are active)."""
    checked = validate_acceptance(spec, skip_out_of_range=skip_out_of_range)
    if checked.get("ok"):
        return None, checked.get("skipped") or []
    out = dict(checked)
    out.pop("ok", None)
    out.pop("skipped", None)
    return {"result": "error", **out}, []


def _forbidden_error(spec, skip_out_of_range=False):
    """Mirror of `_acceptance_error` for `forbidden` scenarios."""
    checked = validate_forbidden(spec, skip_out_of_range=skip_out_of_range)
    if checked.get("ok"):
        return None, checked.get("skipped") or []
    out = dict(checked)
    out.pop("ok", None)
    out.pop("skipped", None)
    return {"result": "error", **out}, []


def _bounds_overrides_desc(bounds_overrides):
    parts = [f"{name}={n}" for name, n in bounds_overrides["instances"].items()]
    parts += [f"{name}={lo}..{hi}" for name, (lo, hi) in bounds_overrides["values"].items()]
    return ", ".join(parts)


def _bounds_skip_warnings(ids, kind, bounds_overrides):
    if not ids:
        return []
    desc = _bounds_overrides_desc(bounds_overrides)
    return [
        {
            "kind": f"{kind}_skipped",
            "id": scenario_id,
            "message": (
                f"{kind} '{scenario_id}' skipped: references values outside "
                f"overridden bounds ({desc})"
            ),
        }
        for scenario_id in ids
    ]


def run_check(file, strict_tags=False, requirements=None):
    try:
        src = open(file, encoding="utf-8").read()
        if is_ai_agent_source(src) or is_ai_project_source(src):
            analysis = check_ai_source(load_ai_source(file))
            spec = analysis.get("ai_agent") or analysis.get("ai_project")
            out = {
                "result": "ok",
                "spec": spec,
                "dialect": analysis["dialect"],
                "warnings": [],
                "ai_analysis_result": analysis["result"],
            }
            if analysis.get("ai_agent"):
                out["agent_analysis_result"] = analysis["result"]
            return _envelope(out)
        ast, display_names = _parse_file(file, src)
        spec = build_spec(ast, display_names, semantic_check=False)
        acc, _ = _acceptance_error(spec)
        if acc:
            return _envelope(acc)
        forb, _ = _forbidden_error(spec)
        if forb:
            return _envelope(forb)
        out = {
            "result": "ok",
            "spec": spec["name"],
            "warnings": spec["warnings"],
        }
        impl = _implements_result(spec)
        if impl:
            out["implements"] = impl
        gov = _governance_result(spec)
        if gov:
            out["governance"] = gov
        out = _add_strict_tag_warnings(out, spec, strict_tags, requirements)
        return _envelope(out)
    except UnexpectedInput as e:
        return _parse_error_result(e)
    except VisitError as e:
        orig = e.orig_exc
        return _error_envelope(
            getattr(orig, "kind", "semantics"),
            str(orig),
            _loc_from_exc(orig),
            getattr(orig, "expected", None),
            getattr(orig, "hint", None),
        )
    except FslError as e:
        return _error_envelope(e.kind, str(e), _loc_from_exc(e),
                               getattr(e, "expected", None), getattr(e, "hint", None))
    except FileNotFoundError:
        return _envelope({"result": "error", "kind": "io",
                          "message": f"file not found: {file}"})


def run_typestate(file):
    try:
        src = open(file, encoding="utf-8").read()
        ast, display_names = _parse_file(file, src)
        spec = build_spec(ast, display_names)
        return _envelope(analyze_typestate(spec))
    except UnexpectedInput as e:
        return _parse_error_result(e)
    except VisitError as e:
        orig = e.orig_exc
        return _error_envelope(
            getattr(orig, "kind", "semantics"),
            str(orig),
            _loc_from_exc(orig),
            getattr(orig, "expected", None),
            getattr(orig, "hint", None),
        )
    except FslError as e:
        return _error_envelope(e.kind, str(e), _loc_from_exc(e),
                               getattr(e, "expected", None), getattr(e, "hint", None))
    except FileNotFoundError:
        return _envelope({"result": "error", "kind": "io",
                          "message": f"file not found: {file}"})


ANALYZE_FORMATS = {"json", "dot", "mermaid"}
ANALYZE_PROJECTIONS = [
    "tsg",
    *sorted(SUPPORTED_PROJECTIONS),
    "refinement_graph",
    "traceability_graph",
]


def run_analyze(file, projection="tsg", profile=None, output_format="json", focus=None):
    paths = list(file) if isinstance(file, (list, tuple)) else [file]
    if len(paths) != 1 or any(Path(p).is_dir() for p in paths):
        return _run_analyze_batch(paths, projection, profile, output_format, focus)
    return _run_analyze_one_enveloped(paths[0], projection, profile, output_format, focus)


def _run_analyze_batch(paths, projection="tsg", profile=None, output_format="json", focus=None):
    try:
        if output_format != "json":
            raise FslError("batch analyze supports only --format json", kind="semantics")
        if focus:
            raise FslError("batch analyze does not support --focus; run impact_graph per file", kind="semantics")
        files = _expand_analyze_paths(paths)
        entries = []
        errors = []
        for path in files:
            result = _run_analyze_one_enveloped(str(path), projection, profile, output_format, focus)
            entry = _batch_file_entry(path, result)
            entries.append(entry)
            if result.get("result") != "analyzed":
                errors.append({
                    "file": _display_path(path),
                    "result": result.get("result"),
                    "kind": result.get("kind"),
                    "message": result.get("message"),
                    "loc": result.get("loc"),
                })
        out = {
            "result": "analyzed" if not errors else "error",
            "analysis": "structure",
            "mode": "batch",
            "projection": projection,
            "profile": profile,
            "files": entries,
            "errors": errors,
        }
        if errors:
            out["kind"] = "batch"
            out["message"] = "one or more files failed structural analysis"
        return _envelope(out)
    except UnexpectedInput as e:
        return _parse_error_result(e)
    except VisitError as e:
        orig = e.orig_exc
        return _error_envelope(
            getattr(orig, "kind", "semantics"),
            str(orig),
            _loc_from_exc(orig),
            getattr(orig, "expected", None),
            getattr(orig, "hint", None),
        )
    except FslError as e:
        return _error_envelope(e.kind, str(e), _loc_from_exc(e),
                               getattr(e, "expected", None), getattr(e, "hint", None))
    except FileNotFoundError as e:
        return _envelope({"result": "error", "kind": "io",
                          "message": f"file not found: {e.filename}"})
    except Exception as e:
        return _envelope({"result": "error", "kind": "internal", "message": str(e)})


def _run_analyze_one_enveloped(file, projection="tsg", profile=None, output_format="json", focus=None):
    try:
        out = _run_analyze_one(file, projection, profile, output_format, focus)
        return _envelope(out)
    except UnexpectedInput as e:
        return _parse_error_result(e)
    except VisitError as e:
        orig = e.orig_exc
        return _error_envelope(
            getattr(orig, "kind", "semantics"),
            str(orig),
            _loc_from_exc(orig),
            getattr(orig, "expected", None),
            getattr(orig, "hint", None),
        )
    except FslError as e:
        return _error_envelope(e.kind, str(e), _loc_from_exc(e),
                               getattr(e, "expected", None), getattr(e, "hint", None))
    except FileNotFoundError:
        return _envelope({"result": "error", "kind": "io",
                          "message": f"file not found: {file}"})
    except Exception as e:
        return _envelope({"result": "error", "kind": "internal", "message": str(e)})


def _run_analyze_one(file, projection="tsg", profile=None, output_format="json", focus=None):
    if output_format not in ANALYZE_FORMATS:
        raise FslError(f"unsupported analyze format: {output_format}", kind="semantics")
    if output_format != "json" and profile:
        raise FslError("DOT/Mermaid export is supported for graph projections, not profiles", kind="semantics")
    if focus and profile:
        raise FslError("--focus is supported only with graph projections, not profiles", kind="semantics")
    if focus and projection != "impact_graph":
        raise FslError("--focus is supported only with --projection impact_graph", kind="semantics")
    if projection == "impact_graph" and not focus:
        raise FslError("--projection impact_graph requires --focus <node-id>", kind="semantics")

    path = Path(file)
    if _is_project_manifest(path):
        if profile:
            raise FslError("project traceability analysis does not support --profile", kind="semantics")
        if projection != "traceability_graph":
            raise FslError(
                "project manifests support only --projection traceability_graph",
                kind="semantics",
            )
        out = {
            "result": "analyzed",
            **analyze_project_manifest(str(path), projection),
        }
    else:
        src = open(file, encoding="utf-8").read()
        ast, display_names = _parse_file(file, src)
        if ast[0] == "refinement":
            if profile:
                return {
                    "result": "analyzed",
                    "refinement": ast[1],
                    "analysis": "structure",
                    "profile": profile,
                    "schema_version": FINDINGS_SCHEMA_VERSION,
                    "findings": [],
                }
            if projection not in ("tsg", "refinement_graph"):
                raise FslError(
                    "refinement mappings support only --projection refinement_graph (or the default tsg alias)",
                    kind="semantics",
                )
            out = {
                "result": "analyzed",
                "refinement": ast[1],
                **analyze_refinement_ast(ast, projection),
            }
        else:
            spec = build_spec(ast, display_names)
            if profile:
                out = {
                    "result": "analyzed",
                    "spec": spec["name"],
                    **analyze_structure(spec, profile=profile),
                }
            elif projection == "tsg":
                out = {
                    "result": "analyzed",
                    "spec": spec["name"],
                    **build_tsg(spec),
                }
            elif projection in SUPPORTED_PROJECTIONS:
                out = {
                    "result": "analyzed",
                    "spec": spec["name"],
                    **analyze_projection(spec, projection, focus=focus),
                }
            else:
                raise FslError(f"unsupported analyze projection: {projection}", kind="semantics")

    if output_format == "json":
        return out
    if not out.get("nodes") or "edges" not in out:
        raise FslError(f"--format {output_format} requires a graph projection", kind="semantics")
    return {
        "result": "analyzed",
        "analysis": out.get("analysis", "structure"),
        "projection": out.get("projection", projection),
        "format": output_format,
        "content": export_graph(out, output_format),
    }


def _expand_analyze_paths(paths):
    files = []
    for raw in paths:
        p = Path(raw)
        if p.is_dir():
            files.extend(sorted((item for item in p.rglob("*.fsl") if item.is_file()), key=_display_path))
        else:
            if not p.exists():
                raise FileNotFoundError(2, "No such file or directory", str(p))
            files.append(p)
    seen = set()
    out = []
    for p in files:
        marker = p.resolve(strict=False)
        if marker in seen:
            continue
        seen.add(marker)
        out.append(p)
    return sorted(out, key=_display_path)


def _batch_file_entry(path, result):
    entry = {
        "file": _display_path(path),
        "result": result.get("result"),
    }
    for key in ("spec", "refinement", "projection", "profile", "schema_version", "formal_status"):
        if key in result:
            entry[key] = result[key]
    if result.get("result") == "analyzed":
        entry["summary"] = _analysis_summary(result)
        if result.get("profile") == "ai-review":
            entry["findings"] = result.get("findings", [])
    else:
        for key in ("kind", "message", "loc", "expected", "hint"):
            if key in result:
                entry[key] = result[key]
    return entry


def _analysis_summary(result):
    summary = {}
    if "nodes" in result:
        summary["nodes"] = len(result.get("nodes") or [])
    if "edges" in result:
        summary["edges"] = len(result.get("edges") or [])
    if "findings" in result:
        summary["findings"] = len(result.get("findings") or [])
    if "components" in result:
        summary["components"] = len(result.get("components") or [])
    if "cycles" in result:
        summary["cycles"] = len(result.get("cycles") or [])
    if "errors" in result:
        summary["errors"] = len(result.get("errors") or [])
    return summary


def _is_project_manifest(path):
    return path.suffix == ".toml"


def _display_path(path):
    p = Path(path)
    try:
        return p.resolve(strict=False).relative_to(Path.cwd().resolve()).as_posix()
    except ValueError:
        return p.as_posix()


def _read_spec(file, bounds_overrides=None):
    src = open(file, encoding="utf-8").read()
    ast, display_names = _parse_file(file, src, bounds_overrides)
    return build_spec(ast, display_names), src.splitlines()


def _parse_instances_override(raw):
    name, sep, val = raw.partition("=")
    name = name.strip()
    if not sep or not name:
        raise FslError(
            f"invalid --instances value '{raw}': expected NAME=N", kind="semantics")
    try:
        n = int(val.strip())
    except ValueError:
        raise FslError(
            f"invalid --instances value '{raw}': '{val.strip()}' is not an integer",
            kind="semantics")
    return name, n


def _parse_values_override(raw):
    name, sep, rng = raw.partition("=")
    name = name.strip()
    lo_s, dots, hi_s = rng.partition("..")
    if not sep or not name or not dots:
        raise FslError(
            f"invalid --values value '{raw}': expected NAME=LO..HI", kind="semantics")
    try:
        lo, hi = int(lo_s.strip()), int(hi_s.strip())
    except ValueError:
        raise FslError(
            f"invalid --values value '{raw}': bounds must be integers", kind="semantics")
    return name, (lo, hi)


def _parse_sweep_int_range(raw, flag):
    name, sep, rng = raw.partition("=")
    name = name.strip()
    lo_s, dots, hi_s = rng.partition("..")
    if not sep or not name or not dots:
        raise FslError(
            f"invalid {flag} value '{raw}': expected NAME=LO..HI", kind="semantics")
    try:
        lo, hi = int(lo_s.strip()), int(hi_s.strip())
    except ValueError:
        raise FslError(
            f"invalid {flag} value '{raw}': bounds must be integers", kind="semantics")
    if lo > hi:
        raise FslError(
            f"invalid {flag} value '{raw}': lower bound must be <= upper bound",
            kind="semantics",
        )
    return name, (lo, hi)


def _parse_sweep_depth(raw):
    lo_s, dots, hi_s = raw.partition("..")
    if not dots:
        raise FslError(f"invalid --depth value '{raw}': expected LO..HI", kind="semantics")
    try:
        lo, hi = int(lo_s.strip()), int(hi_s.strip())
    except ValueError:
        raise FslError("invalid --depth value: bounds must be integers", kind="semantics")
    if lo < 0 or lo > hi:
        raise FslError(
            f"invalid --depth value '{raw}': expected 0 <= LO <= HI", kind="semantics")
    return lo, hi


def _build_bounds_overrides(instances, values):
    overrides = {"instances": {}, "values": {}}
    for raw in instances or []:
        name, n = _parse_instances_override(raw)
        overrides["instances"][name] = n
    for raw in values or []:
        name, bounds = _parse_values_override(raw)
        overrides["values"][name] = bounds
    return overrides


def _build_sweep_ranges(instances, values):
    instance_ranges = {}
    for raw in instances or []:
        name, bounds = _parse_sweep_int_range(raw, "--instances")
        if bounds[0] < 1:
            raise FslError(
                f"invalid --instances value '{raw}': instance lower bound must be >= 1",
                kind="semantics",
            )
        instance_ranges[name] = bounds
    value_ranges = {}
    for raw in values or []:
        name, bounds = _parse_sweep_int_range(raw, "--values")
        value_ranges[name] = bounds
    return instance_ranges, value_ranges


def _sweep_counterexample(result):
    return result.get("result") in {
        "violated",
        "reachable_failed",
        "unknown_cti",
        "nonconformant",
        "refinement_failed",
    }


def _sweep_summary(result):
    out = {
        "result": result.get("result"),
        "checked_to_depth": result.get("checked_to_depth"),
    }
    for key in ("invariant", "trans", "violation_kind", "violated_at_step", "rank_failure"):
        if key in result:
            out[key] = result[key]
    return {k: v for k, v in out.items() if v is not None}


def run_sweep(
        file, depth_range, deadlock_mode, engine="bmc", k_ind=1,
        vacuity_mode="warn", strict_tags=False, requirements=None,
        property_name=None, instances=None, values=None):
    try:
        depth_lo, depth_hi = _parse_sweep_depth(depth_range)
        instance_ranges, value_ranges = _build_sweep_ranges(instances, values)
        instance_names = sorted(instance_ranges)
        value_names = sorted(value_ranges)
        instance_options = [
            range(instance_ranges[name][0], instance_ranges[name][1] + 1)
            for name in instance_names
        ]
        value_options = [
            [(value_ranges[name][0], hi)
             for hi in range(value_ranges[name][0], value_ranges[name][1] + 1)]
            for name in value_names
        ]

        results = []
        minimal = None
        spec_name = None
        instance_product = itertools.product(*instance_options) if instance_names else [()]
        value_product_template = list(itertools.product(*value_options)) if value_names else [()]
        for instance_combo in instance_product:
            instance_scope = dict(zip(instance_names, instance_combo))
            instance_args = [f"{name}={value}" for name, value in instance_scope.items()]
            for value_combo in value_product_template:
                value_scope = dict(zip(value_names, value_combo))
                value_args = [f"{name}={lo}..{hi}" for name, (lo, hi) in value_scope.items()]
                for depth in range(depth_lo, depth_hi + 1):
                    verification = run_verify(
                        file,
                        depth,
                        deadlock_mode,
                        engine=engine,
                        k_ind=k_ind,
                        vacuity_mode=vacuity_mode,
                        strict_tags=strict_tags,
                        requirements=requirements,
                        property_name=property_name,
                        instances=instance_args,
                        values=value_args,
                    )
                    spec_name = spec_name or verification.get("spec")
                    entry = {
                        "scope": {
                            "instances": instance_scope,
                            "values": {
                                name: [bounds[0], bounds[1]]
                                for name, bounds in value_scope.items()
                            },
                            "depth": depth,
                        },
                        "summary": _sweep_summary(verification),
                        "verification": verification,
                    }
                    results.append(entry)
                    if minimal is None and _sweep_counterexample(verification):
                        minimal = entry

        out = {
            "result": "sweep_failed" if minimal else "sweep_passed",
            "spec": spec_name,
            "sweep": {
                "minimality_order": ["instances", "values", "depth"],
                "ranges": {
                    "instances": {
                        name: [lo, hi] for name, (lo, hi) in instance_ranges.items()
                    },
                    "values": {
                        name: [lo, hi] for name, (lo, hi) in value_ranges.items()
                    },
                    "depth": [depth_lo, depth_hi],
                },
                "results": results,
                "minimal_counterexample": minimal,
            },
        }
        return _envelope(out)
    except FslError as e:
        return _error_envelope(e.kind, str(e), _loc_from_exc(e),
                               getattr(e, "expected", None), getattr(e, "hint", None))
    except Exception as e:
        return _envelope({"result": "error", "kind": "internal", "message": str(e)})


def run_verify(
        file, depth, deadlock_mode, engine="bmc", k_ind=1, vacuity_mode="warn",
        strict_tags=False, requirements=None, property_name=None,
        exclude_property_names=None, instances=None, values=None):
    try:
        bounds_overrides = _build_bounds_overrides(instances, values)
        has_bounds_overrides = bool(bounds_overrides["instances"] or bounds_overrides["values"])
        spec, source_lines = _read_spec(file, bounds_overrides)
        acc, acc_skipped = _acceptance_error(spec, skip_out_of_range=has_bounds_overrides)
        if acc:
            return _envelope(acc)
        forb, forb_skipped = _forbidden_error(spec, skip_out_of_range=has_bounds_overrides)
        if forb:
            return _envelope(forb)
        if engine == "induction":
            out = prove(
                spec, k_ind, depth,
                deadlock_mode=deadlock_mode,
                vacuity_mode=vacuity_mode,
                property_name=property_name,
                exclude_property_names=exclude_property_names,
            )
        else:
            out = verify(
                spec,
                depth,
                deadlock_mode=deadlock_mode,
                source_lines=source_lines,
                vacuity_mode=vacuity_mode,
                property_name=property_name,
                exclude_property_names=exclude_property_names,
            )
        impl = _implements_result(spec, depth)
        if impl:
            out = dict(out)
            out["implements"] = impl
        out = _add_strict_tag_warnings(out, spec, strict_tags, requirements)
        skip_warnings = (
            _bounds_skip_warnings(acc_skipped, "acceptance", bounds_overrides)
            + _bounds_skip_warnings(forb_skipped, "forbidden", bounds_overrides)
        )
        if skip_warnings:
            out = dict(out)
            out["warnings"] = list(out.get("warnings") or []) + skip_warnings
        if has_bounds_overrides:
            out = dict(out)
            out["bounds_overrides"] = {
                "instances": dict(bounds_overrides["instances"]),
                "values": {k: [lo, hi] for k, (lo, hi) in bounds_overrides["values"].items()},
            }
        return _envelope(out)
    except UnexpectedInput as e:
        return _parse_error_result(e)
    except VisitError as e:
        orig = e.orig_exc
        return _error_envelope(
            getattr(orig, "kind", "semantics"),
            str(orig),
            _loc_from_exc(orig),
            getattr(orig, "expected", None),
            getattr(orig, "hint", None),
        )
    except FslError as e:
        return _error_envelope(e.kind, str(e), _loc_from_exc(e),
                               getattr(e, "expected", None), getattr(e, "hint", None))
    except FileNotFoundError:
        return _envelope({"result": "error", "kind": "io",
                          "message": f"file not found: {file}"})
    except Exception as e:
        return _envelope({"result": "error", "kind": "internal", "message": str(e)})


def run_scenarios(file, depth, deadlock_mode="warn"):
    try:
        spec, source_lines = _read_spec(file)
        return _envelope(scenarios(spec, depth, deadlock_mode=deadlock_mode, source_lines=source_lines))
    except UnexpectedInput as e:
        return _parse_error_result(e)
    except VisitError as e:
        orig = e.orig_exc
        return _error_envelope(
            getattr(orig, "kind", "semantics"),
            str(orig),
            _loc_from_exc(orig),
            getattr(orig, "expected", None),
            getattr(orig, "hint", None),
        )
    except FslError as e:
        return _error_envelope(e.kind, str(e), _loc_from_exc(e),
                               getattr(e, "expected", None), getattr(e, "hint", None))
    except FileNotFoundError:
        return _envelope({"result": "error", "kind": "io",
                          "message": f"file not found: {file}"})
    except Exception as e:
        return _envelope({"result": "error", "kind": "internal", "message": str(e)})


def run_replay(file, trace_path):
    try:
        spec, _ = _read_spec(file)
        mon = Monitor(spec)
        raw = open(trace_path, encoding="utf-8").read()
        data = json.loads(raw)
        if isinstance(data, list):
            events = data
        elif isinstance(data, dict) and "events" in data:
            events = data["events"]
        else:
            return _envelope({
                "result": "error",
                "kind": "io",
                "message": "trace JSON must be an array or {\"events\": [...]}",
            })

        mon.reset()
        for i, ev in enumerate(events):
            action = ev.get("action")
            params = ev.get("params", {})
            state_before = mon.state
            result = mon.step(action, params)
            if not result.get("ok"):
                return _envelope({
                    "result": "nonconformant",
                    "spec": spec["name"],
                    "failed_at_event": i,
                    "violation": result,
                    "state_before": state_before,
                    "hint": (
                        "the implementation performed an action the spec forbids at this state "
                        "(or reached a state violating an invariant)"
                    ),
                    "note": "leadsTo properties are not checked by replay (finite logs only)",
                })

        return _envelope({
            "result": "conformant",
            "spec": spec["name"],
            "steps_checked": len(events),
            "final_state": mon.state,
            "note": "leadsTo properties are not checked by replay (finite logs only)",
        })
    except json.JSONDecodeError as e:
        return _envelope({"result": "error", "kind": "io", "message": f"invalid JSON: {e}"})
    except UnexpectedInput as e:
        return _parse_error_result(e)
    except VisitError as e:
        orig = e.orig_exc
        return _error_envelope(
            getattr(orig, "kind", "semantics"),
            str(orig),
            _loc_from_exc(orig),
            getattr(orig, "expected", None),
            getattr(orig, "hint", None),
        )
    except FslError as e:
        return _error_envelope(e.kind, str(e), _loc_from_exc(e),
                               getattr(e, "expected", None), getattr(e, "hint", None))
    except FileNotFoundError as e:
        return _envelope({"result": "error", "kind": "io", "message": str(e)})
    except Exception as e:
        return _envelope({"result": "error", "kind": "internal", "message": str(e)})


def run_refine(impl_file, abs_file, mapping_file, depth=8, rest=None):
    try:
        impl_spec, _ = _read_spec(impl_file)
        abs_spec, _ = _read_spec(abs_file)
        mapping_src = open(mapping_file, encoding="utf-8").read()
        mapping = build_refinement(parse_refinement(mapping_src), impl_spec, abs_spec)
        if not rest:
            return _envelope(refine(impl_spec, abs_spec, mapping, depth))
        # Chain: rest = [abs2, map2, abs3, map3, ...] folded as (abs, map) pairs
        if len(rest) % 2 != 0:
            return _envelope({
                "result": "error", "kind": "io",
                "message": "refine chain must list (abs map) pairs after the first mapping",
            })
        specs = [impl_spec, abs_spec]
        mappings = [mapping]
        prev = abs_spec
        i = 0
        while i < len(rest):
            nxt_spec, _ = _read_spec(rest[i])
            nxt_map = build_refinement(
                parse_refinement(open(rest[i + 1], encoding="utf-8").read()),
                prev, nxt_spec)
            specs.append(nxt_spec)
            mappings.append(nxt_map)
            prev = nxt_spec
            i += 2
        return _envelope(refine_chain(specs, mappings, depth))
    except UnexpectedInput as e:
        return _parse_error_result(e)
    except VisitError as e:
        orig = e.orig_exc
        return _error_envelope(
            getattr(orig, "kind", "semantics"),
            str(orig),
            _loc_from_exc(orig),
            getattr(orig, "expected", None),
            getattr(orig, "hint", None),
        )
    except FslError as e:
        return _error_envelope(e.kind, str(e), _loc_from_exc(e),
                               getattr(e, "expected", None), getattr(e, "hint", None))
    except ValueError as e:
        return _envelope({"result": "error", "kind": "parse", "message": str(e)})
    except FileNotFoundError as e:
        return _envelope({"result": "error", "kind": "io", "message": str(e)})
    except Exception as e:
        return _envelope({"result": "error", "kind": "internal", "message": str(e)})


def run_testgen(file, depth=8, output=None, deadlock_mode="warn", write_file=True, strict=False,
                target="pytest"):
    try:
        out_path = output or default_output_name(file, target=target)
        bundle = generate_test_bundle(
            file,
            depth=depth,
            deadlock_mode=deadlock_mode,
            output_path=output if output else None,
            strict=strict,
            target=target,
        )
        content = bundle["content"]
        if write_file and output:
            open(output, "w", encoding="utf-8").write(content)
        result = {
            "result": "generated",
            "spec": bundle["spec"],
            "target": bundle.get("target", target),
            "output": out_path,
            "content": content,
        }
        if bundle.get("warnings"):
            result["warnings"] = bundle["warnings"]
        return _envelope(result)
    except TestgenScenarioError as e:
        return _envelope(e.scenario_result)
    except UnexpectedInput as e:
        return _parse_error_result(e)
    except VisitError as e:
        orig = e.orig_exc
        return _error_envelope(
            getattr(orig, "kind", "semantics"),
            str(orig),
            _loc_from_exc(orig),
            getattr(orig, "expected", None),
            getattr(orig, "hint", None),
        )
    except FslError as e:
        return _error_envelope(e.kind, str(e), _loc_from_exc(e),
                               getattr(e, "expected", None), getattr(e, "hint", None))
    except FileNotFoundError:
        return _envelope({"result": "error", "kind": "io",
                          "message": f"file not found: {file}"})
    except Exception as e:
        return _envelope({"result": "error", "kind": "internal", "message": str(e)})


def run_mutate(file, depth=8, by_requirement=False, max_mutants=DEFAULT_MAX_MUTANTS):
    try:
        return _envelope(mutate_file(
            file,
            depth=depth,
            by_requirement=by_requirement,
            max_mutants=max_mutants,
        ))
    except UnexpectedInput as e:
        return _parse_error_result(e)
    except VisitError as e:
        orig = e.orig_exc
        return _error_envelope(
            getattr(orig, "kind", "semantics"),
            str(orig),
            _loc_from_exc(orig),
            getattr(orig, "expected", None),
            getattr(orig, "hint", None),
        )
    except FslError as e:
        return _error_envelope(e.kind, str(e), _loc_from_exc(e),
                               getattr(e, "expected", None), getattr(e, "hint", None))
    except FileNotFoundError:
        return _envelope({"result": "error", "kind": "io",
                          "message": f"file not found: {file}"})
    except Exception as e:
        return _envelope({"result": "error", "kind": "internal", "message": str(e)})


def run_explain(file, depth=8, readable=False):
    try:
        return _envelope(explain_file(file, depth=depth, readable=readable))
    except UnexpectedInput as e:
        return _parse_error_result(e)
    except VisitError as e:
        orig = e.orig_exc
        return _error_envelope(
            getattr(orig, "kind", "semantics"),
            str(orig),
            _loc_from_exc(orig),
            getattr(orig, "expected", None),
            getattr(orig, "hint", None),
        )
    except FslError as e:
        return _error_envelope(e.kind, str(e), _loc_from_exc(e),
                               getattr(e, "expected", None), getattr(e, "hint", None))
    except FileNotFoundError:
        return _envelope({"result": "error", "kind": "io",
                          "message": f"file not found: {file}"})
    except Exception as e:
        return _envelope({"result": "error", "kind": "internal", "message": str(e)})


def run_html(file, depth=8, output=None, deadlock_mode="warn", write_file=True):
    try:
        explained = explain_file(file, depth=depth)
        verification = run_verify(file, depth, deadlock_mode)
        source = open(file, encoding="utf-8").read()
        content = render_html_report(file, source, explained, verification)
        out_path = output or default_html_output_name(file)
        if write_file and output:
            open(output, "w", encoding="utf-8").write(content)
        return _envelope({
            "result": "generated",
            "kind": "html_report",
            "spec": explained.get("spec"),
            "output": out_path,
            "content": content,
        })
    except UnexpectedInput as e:
        return _parse_error_result(e)
    except VisitError as e:
        orig = e.orig_exc
        return _error_envelope(
            getattr(orig, "kind", "semantics"),
            str(orig),
            _loc_from_exc(orig),
            getattr(orig, "expected", None),
            getattr(orig, "hint", None),
        )
    except FslError as e:
        return _error_envelope(e.kind, str(e), _loc_from_exc(e),
                               getattr(e, "expected", None), getattr(e, "hint", None))
    except FileNotFoundError:
        return _envelope({"result": "error", "kind": "io",
                          "message": f"file not found: {file}"})
    except Exception as e:
        return _envelope({"result": "error", "kind": "internal", "message": str(e)})


def run_ledger(file, depth=8, output=None, deadlock_mode="ignore", impl_log=None, write_file=True):
    """Generate a business audit ledger (markdown) from verifier evidence (#24)."""
    try:
        src = Path(file).read_text(encoding="utf-8")
        ast, display_names = parse_src(src, str(Path(file).parent))
        spec = build_spec(ast, display_names)
        verification = run_verify(file, depth, deadlock_mode)
        scenarios_result = run_scenarios(file, depth, deadlock_mode)
        replay_result = run_replay(file, impl_log) if impl_log else None
        content = render_ledger(file, spec, verification, scenarios_result, replay_result)
        out_path = output or default_ledger_output_name(file)
        if write_file and output:
            open(output, "w", encoding="utf-8").write(content)
        return _envelope({
            "result": "generated",
            "kind": "audit_ledger",
            "spec": spec["name"],
            "output": out_path,
            "content": content,
        })
    except UnexpectedInput as e:
        return _parse_error_result(e)
    except VisitError as e:
        orig = e.orig_exc
        return _error_envelope(
            getattr(orig, "kind", "semantics"), str(orig), _loc_from_exc(orig),
            getattr(orig, "expected", None), getattr(orig, "hint", None),
        )
    except FslError as e:
        return _error_envelope(e.kind, str(e), _loc_from_exc(e),
                               getattr(e, "expected", None), getattr(e, "hint", None))
    except FileNotFoundError:
        return _envelope({"result": "error", "kind": "io",
                          "message": f"file not found: {file}"})
    except Exception as e:
        return _envelope({"result": "error", "kind": "internal", "message": str(e)})


def run_db_check(file, depth=8, engine="bmc", deadlock_mode="warn"):
    try:
        system = load_dbsystem(file)
        return _envelope(check_dbsystem(
            system,
            depth=depth,
            engine=engine,
            deadlock_mode=deadlock_mode,
        ))
    except UnexpectedInput as e:
        return _parse_error_result(e)
    except VisitError as e:
        orig = e.orig_exc
        return _error_envelope(
            getattr(orig, "kind", "semantics"),
            str(orig),
            _loc_from_exc(orig),
            getattr(orig, "expected", None),
            getattr(orig, "hint", None),
        )
    except FslError as e:
        return _error_envelope(e.kind, str(e), _loc_from_exc(e),
                               getattr(e, "expected", None), getattr(e, "hint", None))
    except FileNotFoundError:
        return _envelope({"result": "error", "kind": "io",
                          "message": f"file not found: {file}"})
    except Exception as e:
        return _envelope({"result": "error", "kind": "internal", "message": str(e)})


def run_db_observe(file, trace):
    try:
        system = load_dbsystem(file)
        return _envelope(observe_dbsystem(system, trace))
    except UnexpectedInput as e:
        return _parse_error_result(e)
    except VisitError as e:
        orig = e.orig_exc
        return _error_envelope(
            getattr(orig, "kind", "semantics"),
            str(orig),
            _loc_from_exc(orig),
            getattr(orig, "expected", None),
            getattr(orig, "hint", None),
        )
    except FslError as e:
        return _error_envelope(e.kind, str(e), _loc_from_exc(e),
                               getattr(e, "expected", None), getattr(e, "hint", None))
    except FileNotFoundError as e:
        return _envelope({"result": "error", "kind": "io", "message": str(e)})
    except Exception as e:
        return _envelope({"result": "error", "kind": "internal", "message": str(e)})


def run_db_import(file, name="ImportedDb", output=None, source_format="auto"):
    try:
        imported = import_db_file(file, name=name, source_format=source_format)
        result = {
            "result": "imported_with_warnings" if imported.warnings else "imported",
            "dialect": "fsl-db-mvp.v0",
            "source_format": imported.source_format,
            "dbsystem": imported.system.name,
            "warnings": imported.warnings,
            "dbsystem_source": imported.source,
        }
        if output:
            Path(output).write_text(imported.source, encoding="utf-8")
            result["output"] = output
            result.pop("dbsystem_source", None)
        return _envelope(result)
    except FileNotFoundError as e:
        return _envelope({"result": "error", "kind": "io", "message": str(e)})
    except Exception as e:
        return _envelope({"result": "error", "kind": "internal", "message": str(e)})


def run_ai_check(file, depth=8, engine="bmc", deadlock_mode="warn"):
    try:
        source = load_ai_source(file)
        return _envelope(check_ai_source(
            source,
            depth=depth,
            engine=engine,
            deadlock_mode=deadlock_mode,
        ))
    except UnexpectedInput as e:
        return _parse_error_result(e)
    except VisitError as e:
        orig = e.orig_exc
        return _error_envelope(
            getattr(orig, "kind", "semantics"),
            str(orig),
            _loc_from_exc(orig),
            getattr(orig, "expected", None),
            getattr(orig, "hint", None),
        )
    except FslError as e:
        return _error_envelope(e.kind, str(e), _loc_from_exc(e),
                               getattr(e, "expected", None), getattr(e, "hint", None))
    except FileNotFoundError:
        return _envelope({"result": "error", "kind": "io",
                          "message": f"file not found: {file}"})
    except Exception as e:
        return _envelope({"result": "error", "kind": "internal", "message": str(e)})


def run_ai_replay(file, logs, component=None):
    try:
        source = load_ai_source(file)
        selected = select_ai_component(source, component)
        return _envelope(replay_ai_events(selected, logs))
    except UnexpectedInput as e:
        return _parse_error_result(e)
    except VisitError as e:
        orig = e.orig_exc
        return _error_envelope(
            getattr(orig, "kind", "semantics"),
            str(orig),
            _loc_from_exc(orig),
            getattr(orig, "expected", None),
            getattr(orig, "hint", None),
        )
    except FslError as e:
        return _error_envelope(e.kind, str(e), _loc_from_exc(e),
                               getattr(e, "expected", None), getattr(e, "hint", None))
    except FileNotFoundError as e:
        return _envelope({"result": "error", "kind": "io", "message": str(e)})
    except Exception as e:
        return _envelope({"result": "error", "kind": "internal", "message": str(e)})


def run_ai_eval(file, records=None, dataset=None, slice_name=None, property_name=None):
    try:
        project = load_ai_project_file(file)
        return _envelope(evaluate_ai_project(
            project,
            records_path=records,
            dataset_name=dataset,
            slice_name=slice_name,
            property_name=property_name,
        ))
    except UnexpectedInput as e:
        return _parse_error_result(e)
    except VisitError as e:
        orig = e.orig_exc
        return _error_envelope(
            getattr(orig, "kind", "semantics"),
            str(orig),
            _loc_from_exc(orig),
            getattr(orig, "expected", None),
            getattr(orig, "hint", None),
        )
    except FslError as e:
        return _error_envelope(e.kind, str(e), _loc_from_exc(e),
                               getattr(e, "expected", None), getattr(e, "hint", None))
    except FileNotFoundError as e:
        return _envelope({"result": "error", "kind": "io", "message": str(e)})
    except Exception as e:
        return _envelope({"result": "error", "kind": "internal", "message": str(e)})


def run_ai_regress(file, migration=None, before_records=None, after_records=None, dataset=None):
    try:
        if not before_records or not after_records:
            raise FslError("ai regress requires --before-records and --after-records", kind="semantics")
        project = load_ai_project_file(file)
        return _envelope(regress_ai_project(
            project,
            migration_name=migration,
            before_records_path=before_records,
            after_records_path=after_records,
            dataset_name=dataset,
        ))
    except FslError as e:
        return _error_envelope(e.kind, str(e), _loc_from_exc(e),
                               getattr(e, "expected", None), getattr(e, "hint", None))
    except FileNotFoundError as e:
        return _envelope({"result": "error", "kind": "io", "message": str(e)})
    except Exception as e:
        return _envelope({"result": "error", "kind": "internal", "message": str(e)})


def run_ai_compare(before_records, after_records, dataset=None, from_label=None, to_label=None):
    try:
        return _envelope(compare_ai_records(
            before_records,
            after_records,
            dataset_name=dataset,
            from_label=from_label,
            to_label=to_label,
        ))
    except FslError as e:
        return _error_envelope(e.kind, str(e), _loc_from_exc(e),
                               getattr(e, "expected", None), getattr(e, "hint", None))
    except FileNotFoundError as e:
        return _envelope({"result": "error", "kind": "io", "message": str(e)})
    except Exception as e:
        return _envelope({"result": "error", "kind": "internal", "message": str(e)})


def run_ai_drift(file, logs, baseline_logs=None, property_name=None, window=None, baseline=None):
    try:
        project = load_ai_project_file(file)
        return _envelope(drift_ai_project(
            project,
            current_logs_path=logs,
            baseline_logs_path=baseline_logs,
            property_name=property_name,
            window=window,
            baseline_label=baseline,
        ))
    except FslError as e:
        return _error_envelope(e.kind, str(e), _loc_from_exc(e),
                               getattr(e, "expected", None), getattr(e, "hint", None))
    except FileNotFoundError as e:
        return _envelope({"result": "error", "kind": "io", "message": str(e)})
    except Exception as e:
        return _envelope({"result": "error", "kind": "internal", "message": str(e)})


def run_ai_compat(file, environment=None):
    try:
        return _envelope(ai_compat_profile_from_file(file, environment=environment))
    except UnexpectedInput as e:
        return _parse_error_result(e)
    except VisitError as e:
        orig = e.orig_exc
        return _error_envelope(
            getattr(orig, "kind", "semantics"),
            str(orig),
            _loc_from_exc(orig),
            getattr(orig, "expected", None),
            getattr(orig, "hint", None),
        )
    except FslError as e:
        return _error_envelope(e.kind, str(e), _loc_from_exc(e),
                               getattr(e, "expected", None), getattr(e, "hint", None))
    except FileNotFoundError as e:
        return _envelope({"result": "error", "kind": "io", "message": str(e)})
    except Exception as e:
        return _envelope({"result": "error", "kind": "internal", "message": str(e)})


def run_compat_check(file, include_ai=False):
    try:
        result = run_db_check(file)
        if include_ai:
            result.setdefault("compat", {})["include_ai"] = True
            result.setdefault("compat", {})["source"] = "dbsystem artifact capability model"
        return result
    except Exception as e:
        return _envelope({"result": "error", "kind": "internal", "message": str(e)})


def run_domain_check(file, depth=8, engine="bmc", deadlock_mode="warn"):
    try:
        domain = load_domain(file)
        return _envelope(check_domain_source(
            domain,
            depth=depth,
            engine=engine,
            deadlock_mode=deadlock_mode,
        ))
    except UnexpectedInput as e:
        return _parse_error_result(e)
    except VisitError as e:
        orig = e.orig_exc
        return _error_envelope(
            getattr(orig, "kind", "semantics"),
            str(orig),
            _loc_from_exc(orig),
            getattr(orig, "expected", None),
            getattr(orig, "hint", None),
        )
    except FslError as e:
        return _error_envelope(e.kind, str(e), _loc_from_exc(e),
                               getattr(e, "expected", None), getattr(e, "hint", None))
    except FileNotFoundError:
        return _envelope({"result": "error", "kind": "io",
                          "message": f"file not found: {file}"})
    except Exception as e:
        return _envelope({"result": "error", "kind": "internal", "message": str(e)})


def run_domain_analyze(file):
    try:
        return _envelope(analyze_domain(load_domain(file)))
    except UnexpectedInput as e:
        return _parse_error_result(e)
    except VisitError as e:
        orig = e.orig_exc
        return _error_envelope(
            getattr(orig, "kind", "semantics"),
            str(orig),
            _loc_from_exc(orig),
            getattr(orig, "expected", None),
            getattr(orig, "hint", None),
        )
    except FslError as e:
        return _error_envelope(e.kind, str(e), _loc_from_exc(e),
                               getattr(e, "expected", None), getattr(e, "hint", None))
    except FileNotFoundError:
        return _envelope({"result": "error", "kind": "io",
                          "message": f"file not found: {file}"})
    except Exception as e:
        return _envelope({"result": "error", "kind": "internal", "message": str(e)})


def run_domain_expand(file, output=None, write_file=True):
    try:
        out = expand_domain_source(load_domain(file))
        if output and write_file:
            Path(output).write_text(out["kernel_source"], encoding="utf-8")
            out = dict(out)
            out["output"] = output
            out.pop("kernel_source", None)
        return _envelope(out)
    except UnexpectedInput as e:
        return _parse_error_result(e)
    except VisitError as e:
        orig = e.orig_exc
        return _error_envelope(
            getattr(orig, "kind", "semantics"),
            str(orig),
            _loc_from_exc(orig),
            getattr(orig, "expected", None),
            getattr(orig, "hint", None),
        )
    except FslError as e:
        return _error_envelope(e.kind, str(e), _loc_from_exc(e),
                               getattr(e, "expected", None), getattr(e, "hint", None))
    except FileNotFoundError:
        return _envelope({"result": "error", "kind": "io",
                          "message": f"file not found: {file}"})
    except Exception as e:
        return _envelope({"result": "error", "kind": "internal", "message": str(e)})


def run_domain_generate(file, target="typescript", output=None, write_file=True):
    try:
        out = generate_domain_scaffold(load_domain(file), target=target)
        if output and write_file:
            root = Path(output)
            root.mkdir(parents=True, exist_ok=True)
            for item in out["files"]:
                path = root / item["path"]
                path.parent.mkdir(parents=True, exist_ok=True)
                path.write_text(item["content"], encoding="utf-8")
            out = dict(out)
            out["output"] = output
            out["files"] = [{"path": item["path"]} for item in out["files"]]
        return _envelope(out)
    except UnexpectedInput as e:
        return _parse_error_result(e)
    except VisitError as e:
        orig = e.orig_exc
        return _error_envelope(
            getattr(orig, "kind", "semantics"),
            str(orig),
            _loc_from_exc(orig),
            getattr(orig, "expected", None),
            getattr(orig, "hint", None),
        )
    except FslError as e:
        return _error_envelope(e.kind, str(e), _loc_from_exc(e),
                               getattr(e, "expected", None), getattr(e, "hint", None))
    except FileNotFoundError:
        return _envelope({"result": "error", "kind": "io",
                          "message": f"file not found: {file}"})
    except Exception as e:
        return _envelope({"result": "error", "kind": "internal", "message": str(e)})


def run_domain_testgen(file, depth=8, output=None, deadlock_mode="warn",
                       write_file=True, strict=False, target="vitest"):
    try:
        out = generate_domain_tests(
            file,
            depth=depth,
            deadlock_mode=deadlock_mode,
            target=target,
            strict=strict,
        )
        out_path = output or default_domain_testgen_output(file, target=target)
        out = dict(out)
        out["output"] = out_path
        if output and write_file:
            Path(output).write_text(out["content"], encoding="utf-8")
            out.pop("content", None)
        return _envelope(out)
    except TestgenScenarioError as e:
        return _envelope(e.scenario_result)
    except UnexpectedInput as e:
        return _parse_error_result(e)
    except VisitError as e:
        orig = e.orig_exc
        return _error_envelope(
            getattr(orig, "kind", "semantics"),
            str(orig),
            _loc_from_exc(orig),
            getattr(orig, "expected", None),
            getattr(orig, "hint", None),
        )
    except FslError as e:
        return _error_envelope(e.kind, str(e), _loc_from_exc(e),
                               getattr(e, "expected", None), getattr(e, "hint", None))
    except FileNotFoundError:
        return _envelope({"result": "error", "kind": "io",
                          "message": f"file not found: {file}"})
    except Exception as e:
        return _envelope({"result": "error", "kind": "internal", "message": str(e)})


def run_domain_replay(file, logs):
    try:
        return _envelope(replay_domain_logs(file, logs))
    except json.JSONDecodeError as e:
        return _envelope({"result": "error", "kind": "io", "message": f"invalid JSON: {e}"})
    except ValueError as e:
        return _envelope({"result": "error", "kind": "io", "message": str(e)})
    except UnexpectedInput as e:
        return _parse_error_result(e)
    except VisitError as e:
        orig = e.orig_exc
        return _error_envelope(
            getattr(orig, "kind", "semantics"),
            str(orig),
            _loc_from_exc(orig),
            getattr(orig, "expected", None),
            getattr(orig, "hint", None),
        )
    except FslError as e:
        return _error_envelope(e.kind, str(e), _loc_from_exc(e),
                               getattr(e, "expected", None), getattr(e, "hint", None))
    except FileNotFoundError as e:
        return _envelope({"result": "error", "kind": "io", "message": str(e)})
    except Exception as e:
        return _envelope({"result": "error", "kind": "internal", "message": str(e)})


def exit_code(result):
    r = result.get("result")
    if r in ("verified", "proved", "scenarios", "conformant", "generated",
             "refines", "typestate", "mutated", "explained", "analyzed",
             "verified_under_assumptions", "agent_analyzed", "replay_conformant",
             "observed_conformant", "imported", "imported_with_warnings",
             "expanded", "conformance_checked", "ai_project_analyzed",
             "statistically_supported", "observed_supported", "compared",
             "compat_profile_generated"):
        return 0
    if r == "sweep_passed":
        return 0
    if r in ("violated", "reachable_failed", "unknown_cti", "nonconformant",
             "refinement_failed", "sweep_failed", "replay_nonconformant",
             "observed_mismatch", "statistically_unsupported",
             "dataset_invalid", "evaluator_untrusted", "slice_missing",
             "insufficient_samples", "inconclusive"):
        return 1
    if r == "error":
        kind = result.get("kind")
        if kind == "internal":
            return 3
        return 2
    if r == "ok":
        return 0
    return 3


def _build_arg_parser():
    from . import __version__
    ap = argparse.ArgumentParser(prog="fslc")
    ap.add_argument("-V", "--version", action="version",
                    version=f"fslc {__version__}")
    sub = ap.add_subparsers(dest="cmd", required=True)

    sub.add_parser("version", help="show the version")

    c = sub.add_parser("check")
    c.add_argument("file")
    c.add_argument("--strict-tags", action="store_true")
    c.add_argument("--requirements", default=None)

    v = sub.add_parser("verify")
    v.add_argument("file")
    v.add_argument("--depth", type=int, default=8)
    v.add_argument("--engine", choices=["bmc", "induction"], default="bmc")
    v.add_argument("--k", type=int, default=1, dest="k_ind",
                   help="max induction depth (induction engine only)")
    v.add_argument("--deadlock", choices=["warn", "error", "ignore"], default="warn")
    v.add_argument("--vacuity", choices=["warn", "error", "ignore"], default="warn")
    v.add_argument("--property", dest="property_name", default=None,
                   help="check a single named property in isolation; resolves "
                        "across invariant/trans/leadsTo/reachable declarations")
    v.add_argument("--exclude-property", dest="exclude_property_names",
                   action="append", default=None,
                   help="skip a named property; repeat to omit invariants/trans/"
                        "leadsTo/reachable declarations")
    v.add_argument("--instances", action="append", default=None,
                   help="override a verify-block 'instances NAME = N' bound "
                        "(NAME=N; repeatable) — e.g. shrink to 1 entity for liveness")
    v.add_argument("--values", action="append", default=None,
                   help="override a verify-block 'values NAME = LO..HI' bound "
                        "(NAME=LO..HI; repeatable)")
    v.add_argument("--strict-tags", action="store_true")
    v.add_argument("--requirements", default=None)

    sw = sub.add_parser("sweep", help="run bounded verification across a scope grid")
    sw.add_argument("file")
    sw.add_argument("--depth", default="0..8",
                    help="depth range LO..HI to sweep (default: 0..8)")
    sw.add_argument("--engine", choices=["bmc", "induction"], default="bmc")
    sw.add_argument("--k", type=int, default=1, dest="k_ind",
                    help="max induction depth (induction engine only)")
    sw.add_argument("--deadlock", choices=["warn", "error", "ignore"], default="warn")
    sw.add_argument("--vacuity", choices=["warn", "error", "ignore"], default="warn")
    sw.add_argument("--property", dest="property_name", default=None,
                    help="check a single named property in isolation")
    sw.add_argument("--instances", action="append", default=None,
                    help="sweep an entity bound (NAME=LO..HI; repeatable)")
    sw.add_argument("--values", action="append", default=None,
                    help="sweep a number upper bound as LO..LO through LO..HI")
    sw.add_argument("--strict-tags", action="store_true")
    sw.add_argument("--requirements", default=None)

    sc = sub.add_parser("scenarios")
    sc.add_argument("file")
    sc.add_argument("--depth", type=int, default=8)
    sc.add_argument("--deadlock", choices=["warn", "error", "ignore"], default="warn")

    rp = sub.add_parser("replay")
    rp.add_argument("file")
    rp.add_argument("--trace", required=True)

    tg = sub.add_parser("testgen")
    tg.add_argument("file")
    tg.add_argument("--depth", type=int, default=8)
    tg.add_argument("-o", "--output", default=None)
    tg.add_argument("--target",
                    choices=["pytest", "vitest", "swift", "kotlin", "dart", "phpunit"],
                    default="pytest", help="test harness to emit (default: pytest)")
    tg.add_argument("--deadlock", choices=["warn", "error", "ignore"], default="warn")
    tg.add_argument("--strict", action="store_true")

    mt = sub.add_parser("mutate")
    mt.add_argument("file")
    mt.add_argument("--depth", type=int, default=8)
    mt.add_argument("--by-requirement", action="store_true")
    mt.add_argument("--max-mutants", type=int, default=DEFAULT_MAX_MUTANTS)

    ex = sub.add_parser("explain")
    ex.add_argument("file")
    ex.add_argument("--depth", type=int, default=8)
    ex.add_argument("--readable", action="store_true")

    hr = sub.add_parser("html", help="generate a self-contained HTML review report")
    hr.add_argument("file")
    hr.add_argument("--depth", type=int, default=8)
    hr.add_argument("-o", "--output", default=None)
    hr.add_argument("--deadlock", choices=["warn", "error", "ignore"], default="warn")

    lg = sub.add_parser("ledger",
                        help="generate a business audit ledger (markdown) by requirement id")
    lg.add_argument("file")
    lg.add_argument("--depth", type=int, default=8)
    lg.add_argument("-o", "--output", default=None)
    lg.add_argument("--deadlock", choices=["warn", "error", "ignore"], default="ignore")
    lg.add_argument("--impl-log", default=None,
                    help="optional implementation trace JSON to score conformance (fslc replay)")

    rf = sub.add_parser("refine")
    rf.add_argument("impl")
    rf.add_argument("abs")
    rf.add_argument("mapping")
    rf.add_argument("rest", nargs="*",
                    help="chain check: appending more (abs map) pairs runs an end-to-end composed check")
    rf.add_argument("--depth", type=int, default=8)

    ch = sub.add_parser("chain", help="run a project manifest across business, requirements, design, and impl")
    ch.add_argument("path", nargs="?", default="fsl-project.toml")
    ch.add_argument("--keep-going", action="store_true")

    ts = sub.add_parser("typestate",
                        help="decide whether typestate (ghost types) applies to a design spec and emit a TS template")
    ts.add_argument("file")
    ts.add_argument("--ts", action="store_true",
                    help="emit only the derivable entities' TypeScript to stdout instead of JSON")

    an = sub.add_parser("analyze",
                        help="emit structural analysis JSON for a spec")
    an.add_argument("file", nargs="+")
    an.add_argument("--projection",
                    choices=ANALYZE_PROJECTIONS,
                    default="tsg",
                    help="structural projection to emit")
    an.add_argument("--profile", choices=["ai-review"], default=None,
                    help="emit AI-readable structural review findings")
    an.add_argument("--focus",
                    help="node id for --projection impact_graph, e.g. state:stock or action:checkout")
    an.add_argument("--format", choices=sorted(ANALYZE_FORMATS), default="json",
                    dest="output_format")

    db = sub.add_parser("db", help="database compatibility dialect commands")
    db_sub = db.add_subparsers(dest="db_cmd", required=True)
    dbc = db_sub.add_parser("check", help="check a dbsystem and emit fsl-db findings")
    dbc.add_argument("file")
    dbc.add_argument("--depth", type=int, default=8)
    dbc.add_argument("--engine", choices=["bmc", "induction"], default="bmc")
    dbc.add_argument("--deadlock", choices=["warn", "error", "ignore"], default="warn")
    dbo = db_sub.add_parser("observe", help="compare runtime observation logs to dbsystem declarations")
    dbo.add_argument("file")
    dbo.add_argument("--trace", required=True)
    dbi = db_sub.add_parser("import", help="import SQL DDL or a minimal ORM schema into dbsystem")
    dbi.add_argument("file")
    dbi.add_argument("--name", default="ImportedDb")
    dbi.add_argument("--source", choices=["auto", "sql", "prisma"], default="auto",
                     help="source format to import (default: auto by extension)")
    dbi.add_argument("-o", "--output")

    compat = sub.add_parser("compat", help="shared compatibility commands")
    compat_sub = compat.add_subparsers(dest="compat_cmd", required=True)
    compat_check = compat_sub.add_parser("check", help="check dbsystem compatibility, optionally including AI capability profiles")
    compat_check.add_argument("file")
    compat_check.add_argument("--include-ai", action="store_true")

    ai = sub.add_parser("ai", help="AI dialect commands")
    ai_sub = ai.add_subparsers(dest="ai_cmd", required=True)
    aic = ai_sub.add_parser("check", help="check an ai_component hard contract or recursive agent structure")
    aic.add_argument("file")
    aic.add_argument("--depth", type=int, default=8)
    aic.add_argument("--engine", choices=["bmc", "induction"], default="bmc")
    aic.add_argument("--deadlock", choices=["warn", "error", "ignore"], default="warn")
    air = ai_sub.add_parser("replay", help="compare AI runtime JSONL events to ai_component declarations")
    air.add_argument("file")
    air.add_argument("--logs", required=True)
    air.add_argument("--component", help="ai_component to replay when FILE is a project-level fsl-ai file")
    aie = ai_sub.add_parser("eval", help="evaluate precomputed AI eval JSONL against statistical_property thresholds")
    aie.add_argument("file")
    aie.add_argument("--records")
    aie.add_argument("--dataset")
    aie.add_argument("--slice", dest="slice_name")
    aie.add_argument("--property", dest="property_name")
    aig = ai_sub.add_parser("regress", help="check ai_migration no_regression metrics")
    aig.add_argument("file")
    aig.add_argument("--migration")
    aig.add_argument("--before-records", required=True)
    aig.add_argument("--after-records", required=True)
    aig.add_argument("--dataset")
    aicmp = ai_sub.add_parser("compare", help="compare two precomputed AI eval JSONL files")
    aicmp.add_argument("--from", dest="from_records", required=True)
    aicmp.add_argument("--to", dest="to_records", required=True)
    aicmp.add_argument("--from-label")
    aicmp.add_argument("--to-label")
    aicmp.add_argument("--dataset")
    aid = ai_sub.add_parser("drift", help="check observed_property thresholds and drift from runtime telemetry")
    aid.add_argument("file")
    aid.add_argument("--logs", required=True)
    aid.add_argument("--baseline-logs")
    aid.add_argument("--window")
    aid.add_argument("--baseline")
    aid.add_argument("--property", dest="property_name")
    aicp = ai_sub.add_parser("compat", help="generate an AI artifact capability profile for dbsystem")
    aicp.add_argument("file")
    aicp.add_argument("--environment")

    dom = sub.add_parser("domain", help="Functional DDD / async effect dialect commands")
    dom_sub = dom.add_subparsers(dest="domain_cmd", required=True)
    domc = dom_sub.add_parser("check", help="check domain/effect structure and verify generated kernel")
    domc.add_argument("file")
    domc.add_argument("--depth", type=int, default=8)
    domc.add_argument("--engine", choices=["bmc", "induction"], default="bmc")
    domc.add_argument("--deadlock", choices=["warn", "error", "ignore"], default="warn")
    doma = dom_sub.add_parser("analyze", help="emit aggregate/effect ownership findings")
    doma.add_argument("file")
    domx = dom_sub.add_parser("expand", help="expand domain/effect dialect to kernel FSL")
    domx.add_argument("file")
    domx.add_argument("-o", "--output")
    domg = dom_sub.add_parser("generate", help="generate Functional DDD implementation scaffold")
    domg.add_argument("file")
    domg.add_argument("--profile", choices=["functional-ddd"], default="functional-ddd")
    domg.add_argument(
        "--target",
        choices=["typescript", "kotlin", "swift", "python", "rust"],
        default="typescript",
    )
    domg.add_argument("-o", "--output")
    domr = dom_sub.add_parser("replay", help="replay runtime command/event/effect logs")
    domr.add_argument("file")
    domr.add_argument("--logs", required=True)
    domt = dom_sub.add_parser("testgen", help="generate domain adapter/conformance scaffold")
    domt.add_argument("file")
    domt.add_argument("--depth", type=int, default=8)
    domt.add_argument("--target", choices=["vitest", "pytest", "swift", "kotlin", "dart", "phpunit"], default="vitest")
    domt.add_argument("--deadlock", choices=["warn", "error", "ignore"], default="warn")
    domt.add_argument("--strict", action="store_true")
    domt.add_argument("-o", "--output")

    return ap


def _dispatch(args):
    from . import __version__
    if args.cmd == "version":
        print(f"fslc {__version__}")
        return 0
    if args.cmd == "check":
        result = run_check(args.file, args.strict_tags, args.requirements)
        print(json.dumps(result, indent=2, ensure_ascii=False))
    elif args.cmd == "typestate":
        result = run_typestate(args.file)
        if args.ts and result.get("result") == "typestate":
            blocks = [e["typescript"] for e in result["entities"]
                      if e["applicability"] != "none"]
            sys.stdout.write("\n\n".join(blocks) + ("\n" if blocks else ""))
            sys.exit(0)
        print(json.dumps(result, indent=2, ensure_ascii=False))
    elif args.cmd == "scenarios":
        result = run_scenarios(args.file, args.depth, args.deadlock)
        print(json.dumps(result, indent=2, ensure_ascii=False))
    elif args.cmd == "sweep":
        result = run_sweep(args.file, args.depth, args.deadlock,
                           engine=args.engine, k_ind=args.k_ind,
                           vacuity_mode=args.vacuity,
                           strict_tags=args.strict_tags,
                           requirements=args.requirements,
                           property_name=args.property_name,
                           instances=args.instances,
                           values=args.values)
        print(json.dumps(result, indent=2, ensure_ascii=False))
    elif args.cmd == "replay":
        result = run_replay(args.file, args.trace)
        print(json.dumps(result, indent=2, ensure_ascii=False))
    elif args.cmd == "refine":
        result = run_refine(args.impl, args.abs, args.mapping, args.depth, rest=args.rest)
        print(json.dumps(result, indent=2, ensure_ascii=False))
    elif args.cmd == "chain":
        from .chain import format_chain_table, run_chain

        result = run_chain(args.path, keep_going=args.keep_going)
        print(format_chain_table(result), file=sys.stderr)
        print(json.dumps(result, indent=2, ensure_ascii=False))
    elif args.cmd == "testgen":
        result = run_testgen(args.file, args.depth, args.output, args.deadlock,
                            write_file=bool(args.output), strict=args.strict,
                            target=args.target)
        if result.get("result") == "generated":
            content = result.pop("content")
            out = result.get("output")
            if args.output:
                print(json.dumps(result, indent=2, ensure_ascii=False))
            else:
                sys.stdout.write(content)
                sys.exit(0)
        else:
            print(json.dumps(result, indent=2, ensure_ascii=False))
    elif args.cmd == "mutate":
        result = run_mutate(args.file, args.depth, args.by_requirement, args.max_mutants)
        print(json.dumps(result, indent=2, ensure_ascii=False))
        sys.exit(0)
    elif args.cmd == "explain":
        result = run_explain(args.file, args.depth, readable=args.readable)
        if args.readable and result.get("result") == "explained":
            sys.stdout.write(result["readable"] + "\n")
        else:
            print(json.dumps(result, indent=2, ensure_ascii=False))
    elif args.cmd == "html":
        result = run_html(args.file, args.depth, args.output, args.deadlock,
                          write_file=bool(args.output))
        if result.get("result") == "generated":
            content = result.pop("content")
            if args.output:
                print(json.dumps(result, indent=2, ensure_ascii=False))
            else:
                sys.stdout.write(content)
                sys.exit(0)
        else:
            print(json.dumps(result, indent=2, ensure_ascii=False))
    elif args.cmd == "ledger":
        result = run_ledger(args.file, args.depth, args.output, args.deadlock,
                            impl_log=args.impl_log, write_file=bool(args.output))
        if result.get("result") == "generated":
            content = result.pop("content")
            if args.output:
                print(json.dumps(result, indent=2, ensure_ascii=False))
            else:
                sys.stdout.write(content)
                sys.exit(0)
        else:
            print(json.dumps(result, indent=2, ensure_ascii=False))
    elif args.cmd == "analyze":
        result = run_analyze(args.file, args.projection, args.profile, args.output_format, args.focus)
        if args.output_format != "json" and result.get("result") == "analyzed" and "content" in result:
            sys.stdout.write(result["content"])
        else:
            print(json.dumps(result, indent=2, ensure_ascii=False))
    elif args.cmd == "db":
        if args.db_cmd == "check":
            result = run_db_check(args.file, args.depth, args.engine, args.deadlock)
            print(json.dumps(result, indent=2, ensure_ascii=False))
        elif args.db_cmd == "observe":
            result = run_db_observe(args.file, args.trace)
            print(json.dumps(result, indent=2, ensure_ascii=False))
        elif args.db_cmd == "import":
            result = run_db_import(
                args.file,
                name=args.name,
                output=args.output,
                source_format=args.source,
            )
            if result.get("result") == "imported" and not args.output:
                sys.stdout.write(result["dbsystem_source"])
                sys.exit(0)
            print(json.dumps(result, indent=2, ensure_ascii=False))
        else:
            result = _envelope({
                "result": "error",
                "kind": "parse",
                "message": f"unknown db command: {args.db_cmd}",
            })
            print(json.dumps(result, indent=2, ensure_ascii=False))
    elif args.cmd == "compat":
        if args.compat_cmd == "check":
            result = run_compat_check(args.file, include_ai=args.include_ai)
            print(json.dumps(result, indent=2, ensure_ascii=False))
        else:
            result = _envelope({
                "result": "error",
                "kind": "parse",
                "message": f"unknown compat command: {args.compat_cmd}",
            })
            print(json.dumps(result, indent=2, ensure_ascii=False))
    elif args.cmd == "ai":
        if args.ai_cmd == "check":
            result = run_ai_check(args.file, args.depth, args.engine, args.deadlock)
            print(json.dumps(result, indent=2, ensure_ascii=False))
        elif args.ai_cmd == "replay":
            result = run_ai_replay(args.file, args.logs, component=args.component)
            print(json.dumps(result, indent=2, ensure_ascii=False))
        elif args.ai_cmd == "eval":
            result = run_ai_eval(
                args.file,
                records=args.records,
                dataset=args.dataset,
                slice_name=args.slice_name,
                property_name=args.property_name,
            )
            print(json.dumps(result, indent=2, ensure_ascii=False))
        elif args.ai_cmd == "regress":
            result = run_ai_regress(
                args.file,
                migration=args.migration,
                before_records=args.before_records,
                after_records=args.after_records,
                dataset=args.dataset,
            )
            print(json.dumps(result, indent=2, ensure_ascii=False))
        elif args.ai_cmd == "compare":
            result = run_ai_compare(
                args.from_records,
                args.to_records,
                dataset=args.dataset,
                from_label=args.from_label,
                to_label=args.to_label,
            )
            print(json.dumps(result, indent=2, ensure_ascii=False))
        elif args.ai_cmd == "drift":
            result = run_ai_drift(
                args.file,
                args.logs,
                baseline_logs=args.baseline_logs,
                property_name=args.property_name,
                window=args.window,
                baseline=args.baseline,
            )
            print(json.dumps(result, indent=2, ensure_ascii=False))
        elif args.ai_cmd == "compat":
            result = run_ai_compat(args.file, environment=args.environment)
            print(json.dumps(result, indent=2, ensure_ascii=False))
        else:
            result = _envelope({
                "result": "error",
                "kind": "parse",
                "message": f"unknown ai command: {args.ai_cmd}",
            })
            print(json.dumps(result, indent=2, ensure_ascii=False))
    elif args.cmd == "domain":
        if args.domain_cmd == "check":
            result = run_domain_check(args.file, args.depth, args.engine, args.deadlock)
            print(json.dumps(result, indent=2, ensure_ascii=False))
        elif args.domain_cmd == "analyze":
            result = run_domain_analyze(args.file)
            print(json.dumps(result, indent=2, ensure_ascii=False))
        elif args.domain_cmd == "expand":
            result = run_domain_expand(args.file, args.output, write_file=bool(args.output))
            if result.get("result") == "expanded" and not args.output:
                sys.stdout.write(result["kernel_source"])
                sys.exit(0)
            print(json.dumps(result, indent=2, ensure_ascii=False))
        elif args.domain_cmd == "generate":
            result = run_domain_generate(
                args.file,
                target=args.target,
                output=args.output,
                write_file=bool(args.output),
            )
            print(json.dumps(result, indent=2, ensure_ascii=False))
        elif args.domain_cmd == "replay":
            result = run_domain_replay(args.file, args.logs)
            print(json.dumps(result, indent=2, ensure_ascii=False))
        elif args.domain_cmd == "testgen":
            result = run_domain_testgen(
                args.file,
                depth=args.depth,
                output=args.output,
                deadlock_mode=args.deadlock,
                write_file=bool(args.output),
                strict=args.strict,
                target=args.target,
            )
            if result.get("result") == "generated" and not args.output:
                sys.stdout.write(result["content"])
                sys.exit(0)
            print(json.dumps(result, indent=2, ensure_ascii=False))
        else:
            result = _envelope({
                "result": "error",
                "kind": "parse",
                "message": f"unknown domain command: {args.domain_cmd}",
            })
            print(json.dumps(result, indent=2, ensure_ascii=False))
    else:
        result = run_verify(args.file, args.depth, args.deadlock,
                            engine=args.engine, k_ind=args.k_ind,
                            vacuity_mode=args.vacuity,
                            strict_tags=args.strict_tags,
                            requirements=args.requirements,
                            property_name=args.property_name,
                            exclude_property_names=args.exclude_property_names,
                            instances=args.instances,
                            values=args.values)
        print(json.dumps(result, indent=2, ensure_ascii=False))

    sys.exit(exit_code(result))


def main(argv=None):
    ap = _build_arg_parser()
    args = ap.parse_args(argv)
    return _dispatch(args)


if __name__ == "__main__":
    main()
