# SPDX-License-Identifier: Apache-2.0
# Copyright 2026 Ryoichi Izumita

"""Command-line entry point for fslc."""
import sys
import json
import argparse
import re

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
    return _envelope({
        "result": "error",
        "kind": "parse",
        "loc": loc,
        "message": str(e).split("\n")[0],
        "expected": _parse_expected(e),
    })


def _parse_file(file, src):
    return parse_src(src, str(Path(file).parent))


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


def _acceptance_error(spec):
    checked = validate_acceptance(spec)
    if checked.get("ok"):
        return None
    out = dict(checked)
    out.pop("ok", None)
    return {"result": "error", **out}


def _forbidden_error(spec):
    checked = validate_forbidden(spec)
    if checked.get("ok"):
        return None
    out = dict(checked)
    out.pop("ok", None)
    return {"result": "error", **out}


def run_check(file, strict_tags=False, requirements=None):
    try:
        src = open(file, encoding="utf-8").read()
        ast, display_names = _parse_file(file, src)
        spec = build_spec(ast, display_names, semantic_check=False)
        acc = _acceptance_error(spec)
        if acc:
            return _envelope(acc)
        forb = _forbidden_error(spec)
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


def _read_spec(file):
    src = open(file, encoding="utf-8").read()
    ast, display_names = _parse_file(file, src)
    return build_spec(ast, display_names), src.splitlines()


def run_verify(
        file, depth, deadlock_mode, engine="bmc", k_ind=1, vacuity_mode="warn",
        strict_tags=False, requirements=None, property_name=None,
        exclude_property_names=None):
    try:
        spec, source_lines = _read_spec(file)
        acc = _acceptance_error(spec)
        if acc:
            return _envelope(acc)
        forb = _forbidden_error(spec)
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


def exit_code(result):
    r = result.get("result")
    if r in ("verified", "proved", "scenarios", "conformant", "generated",
             "refines", "typestate", "mutated", "explained"):
        return 0
    if r in ("violated", "reachable_failed", "unknown_cti", "nonconformant", "refinement_failed"):
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
    v.add_argument("--strict-tags", action="store_true")
    v.add_argument("--requirements", default=None)

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
    else:
        result = run_verify(args.file, args.depth, args.deadlock,
                            engine=args.engine, k_ind=args.k_ind,
                            vacuity_mode=args.vacuity,
                            strict_tags=args.strict_tags,
                            requirements=args.requirements,
                            property_name=args.property_name,
                            exclude_property_names=args.exclude_property_names)
        print(json.dumps(result, indent=2, ensure_ascii=False))

    sys.exit(exit_code(result))


def main(argv=None):
    ap = _build_arg_parser()
    args = ap.parse_args(argv)
    return _dispatch(args)


if __name__ == "__main__":
    main()
