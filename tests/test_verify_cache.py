# SPDX-License-Identifier: Apache-2.0
# Copyright 2026 Ryoichi Izumita

"""Issue #169: the persistent verdict cache (`fslc.verify_cache`).

The cache is only as good as its ability to never lie, so most of this file
is negative tests -- proving specific input changes are *not* served from a
stale cache entry, per the soundness argument in
`docs/DESIGN-incremental-verify.md` §7.
"""
from __future__ import annotations

import inspect
from pathlib import Path

import pytest

from fslc import bmc, verify_cache
from fslc import cli as fslc_cli
from fslc.cli import run_mutate, run_verify

ROOT = Path(__file__).resolve().parents[1]
SPECS = ROOT / "specs"

COUNTER_SRC = """
spec Counter {{
  state {{ x: Int }}
  init {{ x = 0 }}
  action inc() {{ x = x + 1 }}
  invariant Bounded {{ x <= {bound} }}
}}
"""


@pytest.fixture
def cache_dir(tmp_path, monkeypatch):
    monkeypatch.setenv("FSLC_CACHE", "on")
    monkeypatch.setenv("FSLC_CACHE_DIR", str(tmp_path))
    monkeypatch.delenv("FSLC_CACHE_VERIFY", raising=False)
    return tmp_path


def _write(tmp_path, name, src):
    p = tmp_path / name
    p.write_text(src, encoding="utf-8")
    return p


def _counter(tmp_path, bound=20):
    # bound=20 verifies within every depth used below (8, 9, 12, ...) so
    # tests that aren't specifically about cross-depth reuse don't
    # accidentally get served by it. The dedicated cross-depth tests pass
    # bound=2 explicitly (violates at step 3).
    return _write(tmp_path, "counter.fsl", COUNTER_SRC.format(bound=bound))


# --------------------------------------------------------------------------
# hit path
# --------------------------------------------------------------------------
def test_identical_rerun_hits_cache_and_skips_the_engine(cache_dir, tmp_path, monkeypatch):
    spec = _write(tmp_path, "cart.fsl", (SPECS / "cart_v1.fsl").read_text(encoding="utf-8"))
    first = run_verify(str(spec), 4, "warn")
    assert first["result"] == "verified"
    assert "cache" not in first

    def _explode(*args, **kwargs):
        raise AssertionError("engine must not run on a cache hit")

    monkeypatch.setattr(fslc_cli, "verify", _explode)
    second = run_verify(str(spec), 4, "warn")
    assert second["cache"]["hit"] is True
    assert second["cache"]["source"] == "exact"
    # identical modulo the additive cache field
    first_stripped = {k: v for k, v in first.items() if k != "cache"}
    second_stripped = {k: v for k, v in second.items() if k != "cache"}
    assert first_stripped == second_stripped


def test_induction_result_is_cached_too(cache_dir, tmp_path, monkeypatch):
    spec = _write(tmp_path, "counter_latch.fsl", """
spec CounterLatch {
  state { x: Int }
  init { x = 0 }
  action inc() { requires x < 5  x = x + 1 }
  invariant XRange { x >= 0 and x <= 5 }
}
""")
    first = run_verify(str(spec), 8, "ignore", engine="induction")
    assert first["result"] == "proved"

    monkeypatch.setattr(fslc_cli, "prove", lambda *a, **k: (_ for _ in ()).throw(AssertionError("no")))
    second = run_verify(str(spec), 8, "ignore", engine="induction")
    assert second["cache"]["hit"] is True
    assert second["result"] == "proved"


# --------------------------------------------------------------------------
# negative: the soundness protectors
# --------------------------------------------------------------------------
def test_one_character_invariant_edit_misses(cache_dir, tmp_path):
    p = _counter(tmp_path)
    run_verify(str(p), 8, "ignore")
    p.write_text(COUNTER_SRC.format(bound=3), encoding="utf-8")
    out = run_verify(str(p), 8, "ignore")
    assert "cache" not in out


def test_comment_only_edit_misses(cache_dir, tmp_path):
    # Documents the conservative choice: entry-file text is hashed verbatim
    # because diagnostics quote source lines by number.
    p = _counter(tmp_path)
    run_verify(str(p), 8, "ignore")
    p.write_text("// a harmless comment\n" + COUNTER_SRC.format(bound=2), encoding="utf-8")
    out = run_verify(str(p), 8, "ignore")
    assert "cache" not in out


@pytest.mark.parametrize("kwargs", [
    {"depth": 9},
    {"deadlock_mode": "error"},
    {"vacuity_mode": "error"},
    {"k_ind": 2, "engine": "induction"},
    {"strict_tags": True},
    {"exclude_property_names": ["Bounded"]},
], ids=lambda kw: ",".join(f"{k}={v}" for k, v in kw.items()))
def test_option_flip_misses(cache_dir, tmp_path, kwargs):
    p = _counter(tmp_path)
    base = {"depth": 8, "deadlock_mode": "ignore"}
    run_verify(str(p), **base)
    out = run_verify(str(p), **{**base, **kwargs})
    assert "cache" not in out


def test_engine_flip_misses(cache_dir, tmp_path):
    p = _counter(tmp_path)
    run_verify(str(p), 8, "ignore", engine="bmc")
    out = run_verify(str(p), 8, "ignore", engine="induction")
    assert "cache" not in out


def test_lemma_candidates_are_part_of_induction_cache_key(cache_dir, tmp_path):
    p = _counter(tmp_path)
    base = {"depth": 8, "deadlock_mode": "ignore", "engine": "induction"}
    run_verify(str(p), **base)

    first = run_verify(str(p), **base, lemmas=["x >= 0"])
    second = run_verify(str(p), **base, lemmas=["x >= 0"])

    assert "cache" not in first
    assert second["cache"]["hit"] is True


def test_instances_and_values_overrides_change_the_key(cache_dir, tmp_path):
    p = _write(tmp_path, "map_spec.fsl", """
spec MapSpec {
  type Id = 0..2
  state { seen: Map<Id, Bool> }
  init { forall i: Id { seen[i] = false } }
  action mark(i: Id) { seen[i] = true }
  verify { instances Id = 3 }
}
""")
    run_verify(str(p), 6, "ignore")
    out = run_verify(str(p), 6, "ignore", instances=["Id=1"])
    assert "cache" not in out


def test_requirements_file_content_edit_misses(cache_dir, tmp_path):
    p = _counter(tmp_path)
    req = tmp_path / "reqs.txt"
    req.write_text("REQ-1\n", encoding="utf-8")
    run_verify(str(p), 8, "ignore", strict_tags=True, requirements=str(req))
    req.write_text("REQ-1\nREQ-2\n", encoding="utf-8")
    out = run_verify(str(p), 8, "ignore", strict_tags=True, requirements=str(req))
    assert "cache" not in out


def test_implementation_fingerprint_change_misses(cache_dir, tmp_path, monkeypatch):
    p = _counter(tmp_path)
    run_verify(str(p), 8, "ignore")
    monkeypatch.setattr(verify_cache, "_fingerprint_cache", None)
    monkeypatch.setattr(verify_cache, "implementation_fingerprint", lambda: "a-different-fingerprint")
    out = run_verify(str(p), 8, "ignore")
    assert "cache" not in out


def test_corrupt_entry_is_a_miss_not_a_crash(cache_dir, tmp_path):
    p = _counter(tmp_path)
    first = run_verify(str(p), 8, "ignore")
    assert "cache" not in first
    entries = list((cache_dir / "verify" / "v1").rglob("*.json"))
    entries = [e for e in entries if e.parent.name != "xdepth"]
    assert entries, "expected at least one stored entry"
    entries[0].write_text("{not valid json", encoding="utf-8")
    out = run_verify(str(p), 8, "ignore")
    assert "cache" not in out
    assert out["result"] == "verified"


def test_no_cache_flag_never_reads_or_writes(cache_dir, tmp_path):
    p = _counter(tmp_path)
    run_verify(str(p), 8, "ignore", use_cache=False)
    run_verify(str(p), 8, "ignore", use_cache=False)
    out = run_verify(str(p), 8, "ignore", use_cache=False)
    assert "cache" not in out
    verify_root = cache_dir / "verify"
    assert not verify_root.exists() or not any(verify_root.rglob("*.json"))


def test_fslc_cache_off_env_never_reads_or_writes(tmp_path, monkeypatch):
    monkeypatch.setenv("FSLC_CACHE", "off")
    monkeypatch.setenv("FSLC_CACHE_DIR", str(tmp_path))
    p = _counter(tmp_path)
    run_verify(str(p), 8, "ignore")
    out = run_verify(str(p), 8, "ignore")
    assert "cache" not in out
    verify_root = tmp_path / "verify"
    assert not verify_root.exists() or not any(verify_root.rglob("*.json"))


def test_mutate_never_populates_the_cache(cache_dir, tmp_path):
    p = _counter(tmp_path)
    run_mutate(str(p), depth=6)
    verify_root = cache_dir / "verify"
    assert not verify_root.exists() or not any(verify_root.rglob("*.json"))


def test_fslc_cache_verify_mode_detects_a_forced_divergence(cache_dir, tmp_path, monkeypatch):
    p = _counter(tmp_path)
    run_verify(str(p), 8, "ignore")
    monkeypatch.setenv("FSLC_CACHE_VERIFY", "1")
    monkeypatch.setattr(fslc_cli, "verify", lambda *a, **k: {
        "result": "violated", "violation_kind": "invariant", "invariant": "Bounded",
        "violated_at_step": 1, "completeness": "bounded", "checked_to_depth": 1,
    })
    out = run_verify(str(p), 8, "ignore")
    assert out["result"] == "error"
    assert out["kind"] == "internal"
    assert "divergence" in out["message"]


# --------------------------------------------------------------------------
# cross-depth reuse
# --------------------------------------------------------------------------
def test_violated_at_shallow_depth_is_reused_at_a_deeper_depth(cache_dir, tmp_path, monkeypatch):
    p = _counter(tmp_path, bound=2)
    primed = run_verify(str(p), 8, "ignore")
    assert primed["result"] == "violated"
    step = primed["violated_at_step"]

    def _explode(*a, **k):
        raise AssertionError("engine must not run when cross-depth reuse applies")

    monkeypatch.setattr(fslc_cli, "verify", _explode)
    deeper = run_verify(str(p), 12, "ignore")
    assert deeper["cache"]["source"] == "cross_depth"
    assert deeper["violated_at_step"] == step


def test_shallower_depth_than_the_violation_still_runs_the_engine(cache_dir, tmp_path):
    p = _counter(tmp_path, bound=2)
    primed = run_verify(str(p), 8, "ignore")
    step = primed["violated_at_step"]
    assert step > 1
    out = run_verify(str(p), 1, "ignore")
    assert "cache" not in out
    assert out["result"] == "verified"


def test_verified_result_is_not_reused_across_depths(cache_dir, tmp_path):
    spec = _write(tmp_path, "cart.fsl", (SPECS / "cart_v1.fsl").read_text(encoding="utf-8"))
    run_verify(str(spec), 4, "warn")
    out = run_verify(str(spec), 5, "warn")
    assert "cache" not in out


# --------------------------------------------------------------------------
# key-completeness guard
# --------------------------------------------------------------------------
_NON_SEMANTIC_RUN_VERIFY_PARAMS = {"file", "use_cache"}
_CACHE_BYPASSED_SEMANTIC_RUN_VERIFY_PARAMS = {"from_state"}


def test_run_verify_signature_is_fully_classified():
    """Every run_verify parameter must either be threaded into the cache key
    (verify_cache.compute_key's keyword-only parameters, modulo the sha256'd
    forms of src/requirements) or be on the explicit non-semantic allowlist
    above. A new parameter silently affecting output without being in either
    set is exactly the risk this design calls out as the primary review
    hazard for future changes."""
    run_verify_params = set(inspect.signature(run_verify).parameters) - {"self"}
    key_params = set(inspect.signature(verify_cache.compute_key).parameters)
    # run_verify's `requirements` (a path) becomes compute_key's
    # `requirements_sha256`; everything else must match by name.
    mapped = {"requirements": "requirements_sha256"}
    classified = (
        _NON_SEMANTIC_RUN_VERIFY_PARAMS
        | _CACHE_BYPASSED_SEMANTIC_RUN_VERIFY_PARAMS
    )
    for name in run_verify_params - classified:
        target = mapped.get(name, name)
        assert target in key_params or name in ("ast", "display_names", "src"), (
            f"run_verify parameter {name!r} is not threaded into verify_cache.compute_key "
            "and is not on the non-semantic allowlist -- classify it"
        )


def test_from_state_never_reuses_normal_init_cache(cache_dir, tmp_path):
    spec = _counter(tmp_path, bound=1)
    normal = run_verify(str(spec), 0, "ignore")
    assert normal["result"] == "verified"

    snapshot = _write(tmp_path, "state.json", '{"x": 2}')
    predicted = run_verify(str(spec), 0, "ignore", from_state=str(snapshot))

    assert predicted["result"] == "violated"
    assert predicted["violated_at_step"] == 0
    assert "cache" not in predicted


# --------------------------------------------------------------------------
# canonical hash / cache-key plumbing
# --------------------------------------------------------------------------
def test_canonical_hash_rejects_unencodable_values():
    with pytest.raises(verify_cache.CacheKeyError):
        verify_cache.canonical_hash(object())


def test_canonical_hash_is_order_independent_for_dict_keys():
    a = verify_cache.canonical_hash({"a": 1, "b": 2})
    b = verify_cache.canonical_hash({"b": 2, "a": 1})
    assert a == b


def test_canonical_hash_distinguishes_tuple_and_list():
    assert verify_cache.canonical_hash((1, 2)) != verify_cache.canonical_hash([1, 2])
