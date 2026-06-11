import gc
from pathlib import Path

import z3

from fslc import build_spec, parse
from fslc import bmc
from fslc.cli import run_refine


SPECS = Path(__file__).resolve().parent.parent / "specs"


def _minimal_spec():
    return build_spec(parse("""
spec CacheSpec {
  state { x: Int }
  init { x = 0 }
  action noop() { x = x }
}
"""))


def test_eval_expr_cache_recomputes_when_ast_identity_mismatches():
    spec = _minimal_spec()
    state = bmc.make_state(spec, 0)
    expr = ("var", "x")
    stale_expr = ("num", 999)
    token = object()
    cache_key = (token, id(expr), ())
    cache = {
        cache_key: (stale_expr, (), z3.IntVal(999)),
    }

    with bmc._eval_cache_scope(cache, token):
        result = bmc.eval_expr(expr, state, {}, spec)

    assert result.eq(state["x"])
    cached_expr, cached_binds, cached_result = cache[cache_key]
    assert cached_expr is expr
    assert cached_binds == ()
    assert cached_result.eq(state["x"])


def test_eval_expr_cache_recomputes_when_binding_identity_mismatches():
    spec = _minimal_spec()
    state = bmc.make_state(spec, 0)
    expr = ("var", "i")
    current_binding = z3.Int("i_current")
    stale_binding = z3.Int("i_stale")
    binds = {"i": current_binding}
    token = object()
    cache_key = (token, id(expr), bmc._freeze_binds_for_cache(binds))
    cache = {
        cache_key: (expr, (("i", stale_binding),), z3.IntVal(999)),
    }

    with bmc._eval_cache_scope(cache, token):
        result = bmc.eval_expr(expr, state, binds, spec)

    assert result is current_binding
    cached_expr, cached_binds, cached_result = cache[cache_key]
    assert cached_expr is expr
    assert cached_binds[0][1] is current_binding
    assert cached_result is current_binding


def test_refine_repeated_fresh_parse_with_gc():
    for _ in range(20):
        result = run_refine(
            str(SPECS / "cart_impl.fsl"),
            str(SPECS / "cart_v1.fsl"),
            str(SPECS / "cart_refines.fsl"),
            depth=2,
        )
        assert result["result"] == "refines", result
        gc.collect()
