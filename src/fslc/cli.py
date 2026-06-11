"""Command-line entry point for fslc."""
import sys
import json
import argparse

from lark.exceptions import UnexpectedInput, VisitError

from .parser import parse
from .model import build_spec, check_spec, FslError
from .bmc import verify, prove

FSL_VERSION = "1.0"


def _envelope(result):
    out = {"fsl": FSL_VERSION}
    out.update(result)
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


def run_check(file):
    try:
        src = open(file, encoding="utf-8").read()
        ast = parse(src)
        return _envelope(check_spec(ast))
    except UnexpectedInput as e:
        return _envelope({
            "result": "error",
            "kind": "parse",
            "loc": {"line": e.line, "column": e.column},
            "message": str(e).split("\n")[0],
            "expected": _parse_expected(e),
        })
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


def run_verify(file, depth, deadlock_mode, engine="bmc", k_ind=1):
    try:
        src = open(file, encoding="utf-8").read()
        ast = parse(src)
        spec = build_spec(ast)
        if engine == "induction":
            return _envelope(prove(spec, k_ind, depth, deadlock_mode=deadlock_mode))
        return _envelope(verify(spec, depth, deadlock_mode=deadlock_mode))
    except UnexpectedInput as e:
        return _envelope({
            "result": "error",
            "kind": "parse",
            "loc": {"line": e.line, "column": e.column},
            "message": str(e).split("\n")[0],
            "expected": _parse_expected(e),
        })
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
    except Exception as e:
        return _envelope({"result": "error", "kind": "internal", "message": str(e)})
    except FileNotFoundError:
        return _envelope({"result": "error", "kind": "io",
                          "message": f"file not found: {file}"})


def exit_code(result):
    r = result.get("result")
    if r in ("verified", "proved"):
        return 0
    if r in ("violated", "reachable_failed", "unknown_cti"):
        return 1
    if r == "error":
        kind = result.get("kind")
        if kind == "internal":
            return 3
        return 2
    if r == "ok":
        return 0
    return 3


def main(argv=None):
    ap = argparse.ArgumentParser(prog="fslc")
    sub = ap.add_subparsers(dest="cmd", required=True)

    c = sub.add_parser("check")
    c.add_argument("file")

    v = sub.add_parser("verify")
    v.add_argument("file")
    v.add_argument("--depth", type=int, default=8)
    v.add_argument("--engine", choices=["bmc", "induction"], default="bmc")
    v.add_argument("--k", type=int, default=1, dest="k_ind",
                   help="max induction depth (induction engine only)")
    v.add_argument("--deadlock", choices=["warn", "error", "ignore"], default="warn")

    args = ap.parse_args(argv)

    if args.cmd == "check":
        result = run_check(args.file)
    else:
        result = run_verify(args.file, args.depth, args.deadlock,
                            engine=args.engine, k_ind=args.k_ind)

    print(json.dumps(result, indent=2, ensure_ascii=False))
    sys.exit(exit_code(result))


if __name__ == "__main__":
    main()
