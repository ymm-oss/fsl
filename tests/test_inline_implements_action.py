# SPDX-License-Identifier: Apache-2.0
# Copyright 2026 Ryoichi Izumita

"""Inline `implements { }` action correspondence (#73).

Confirms `refinement_action` can appear inside the requirements-dialect
inline `implements { }` block (grammar.py `?implements_item`), including
arity-changing correspondences that previously required a separate
refinement file + `fslc refine`. The inline desugar (dialects.py
`_expand_requirements_with_display`) merges the block's items into the
same `("refinement", ..., mapping_items)` AST that the separate-file path
parses, so `refine.py` needs no changes.
"""
from fslc.cli import run_check, run_refine, run_verify


def _write(tmp_path, files):
    for name, src in files.items():
        (tmp_path / name).write_text(src, encoding="utf-8")


ABS_SAME_ARITY = '''spec Abs73Same {
  type CaseId = 0..1
  state { done: Map<CaseId, Bool> }
  init { forall c: CaseId { done[c] = false } }
  fair action settle(c: CaseId) { requires done[c] == false  done[c] = true }
}
'''

IMPL_SAME_ARITY = '''requirements Impl73Same {
  implements Abs73Same from "abs.fsl" {
    map done[c: CaseId] = paid[c]
    action approve(c: CaseId) -> settle(c)
  }
  type CaseId = 0..1
  state { paid: Map<CaseId, Bool> }
  init { forall c: CaseId { paid[c] = false } }
  fair action approve(c: CaseId) { requires paid[c] == false  paid[c] = true }
}
'''


def test_inline_action_map_same_arity_baseline(tmp_path):
    _write(tmp_path, {"abs.fsl": ABS_SAME_ARITY, "impl.fsl": IMPL_SAME_ARITY})

    checked = run_check(str(tmp_path / "impl.fsl"))
    assert checked["result"] == "ok"
    assert checked["implements"] == {"abs": "Abs73Same", "result": "refines"}


ABS_ARITY_CHANGE = '''spec Abs73 {
  type CaseId = 0..1
  state { done: Map<CaseId, Bool> }
  init { forall c: CaseId { done[c] = false } }
  fair action refund(c: CaseId) { requires done[c] == false  done[c] = true }
}
'''

IMPL_ARITY_CHANGE_INLINE = '''requirements Impl73 {
  implements Abs73 from "abs.fsl" {
    map done[c: CaseId] = paid[c]
    action pay(c: CaseId, m: Method) -> refund(c)
  }
  type CaseId = 0..1
  enum Method { Card, Cash }
  state { paid: Map<CaseId, Bool> }
  init { forall c: CaseId { paid[c] = false } }
  fair action pay(c: CaseId, m: Method) { requires paid[c] == false  paid[c] = true }
}
'''

# The impl side of the separate-file path is the same state/action shape the
# requirements dialect desugars `Impl73` to (same spec name — `build_refinement`
# requires `impl <NAME>` to match `impl_spec["name"]`).
IMPL_ARITY_CHANGE_KERNEL = '''spec Impl73 {
  type CaseId = 0..1
  enum Method { Card, Cash }
  state { paid: Map<CaseId, Bool> }
  init { forall c: CaseId { paid[c] = false } }
  fair action pay(c: CaseId, m: Method) { requires paid[c] == false  paid[c] = true }
}
'''

MAPPING_ARITY_CHANGE = '''refinement Impl73RefinesAbs73 {
  impl Impl73
  abs Abs73
  map done[c: CaseId] = paid[c]
  action pay(c: CaseId, m: Method) -> refund(c)
}
'''


def test_inline_action_map_arity_change_matches_separate_file_refine(tmp_path):
    _write(tmp_path, {
        "abs.fsl": ABS_ARITY_CHANGE,
        "impl_inline.fsl": IMPL_ARITY_CHANGE_INLINE,
        "impl_kernel.fsl": IMPL_ARITY_CHANGE_KERNEL,
        "mapping.fsl": MAPPING_ARITY_CHANGE,
    })

    checked = run_check(str(tmp_path / "impl_inline.fsl"))
    assert checked["result"] == "ok"
    inline_verdict = checked["implements"]["result"]

    verified = run_verify(str(tmp_path / "impl_inline.fsl"), 4, "warn")
    assert verified["result"] == "verified"
    assert verified["implements"]["result"] == inline_verdict

    separate = run_refine(
        str(tmp_path / "impl_kernel.fsl"),
        str(tmp_path / "abs.fsl"),
        str(tmp_path / "mapping.fsl"),
        depth=4,
    )
    assert separate["result"] == inline_verdict == "refines"
    assert separate["action_map"] == {"pay": "refund"}


ABS_STUTTER = '''spec AbsStutter73 {
  type K = 0..1
  state { x: K }
  init { x = 0 }
  fair action tick() { x = 1 }
}
'''

IMPL_STUTTER = '''requirements ImplStutter73 {
  implements AbsStutter73 from "abs.fsl" {
    map x = y
    action internal_tick() -> stutter
  }
  type K = 0..1
  state { y: K }
  init { y = 0 }
  fair action internal_tick() { y = y }
}
'''


def test_inline_action_map_stutter(tmp_path):
    _write(tmp_path, {"abs.fsl": ABS_STUTTER, "impl.fsl": IMPL_STUTTER})

    checked = run_check(str(tmp_path / "impl.fsl"))
    assert checked["result"] == "ok"
    assert checked["implements"] == {"abs": "AbsStutter73", "result": "refines"}


IMPL_DUPLICATE = '''requirements Impl73Dup {
  implements Abs73 from "abs.fsl" {
    map done[c: CaseId] = paid[c]
    action pay(c: CaseId, m: Method) -> refund(c)
  }
  type CaseId = 0..1
  enum Method { Card, Cash }
  state { paid: Map<CaseId, Bool> }
  init { forall c: CaseId { paid[c] = false } }
  fair action pay(c: CaseId, m: Method) maps refund(c) {
    requires paid[c] == false
    paid[c] = true
  }
}
'''


def test_inline_action_map_conflicts_with_requirement_maps_clause_is_an_error(tmp_path):
    # Pinned conflict semantics: an explicit inline `action ... -> ...` item and
    # an auto-derived correspondence (here, the requirement action's own `maps`
    # clause) for the *same* impl action name both land in `build_refinement`'s
    # `items` list. `build_refinement` (refine.py) already rejects a second
    # entry for the same name with `kind: "type"`, "duplicate action map for
    # '<name>'" — this is the same error the separate-file path raises when a
    # mapping file lists an action twice, so no new conflict handling was added;
    # the pre-existing duplicate check just now also sees inline vs. auto-derived
    # duplicates.
    _write(tmp_path, {"abs.fsl": ABS_ARITY_CHANGE, "impl.fsl": IMPL_DUPLICATE})

    checked = run_check(str(tmp_path / "impl.fsl"))
    assert checked["result"] == "error"
    assert checked["kind"] == "type"
    assert "duplicate action map for 'pay'" in checked["message"]


ABS_BRANCH = '''spec AbsBranch73 {
  type CaseId = 0..1
  state { done: Map<CaseId, Bool> }
  init { forall c: CaseId { done[c] = false } }
  fair action settle(c: CaseId) { requires done[c] == false  done[c] = true }
}
'''

IMPL_BRANCH_ORIGINAL_NAME = '''requirements ImplBranch73 {
  implements AbsBranch73 from "abs.fsl" {
    map done[c: CaseId] = paid[c]
    action decide(c: CaseId) -> settle(c)
  }
  type CaseId = 0..1
  state { paid: Map<CaseId, Bool> }
  init { forall c: CaseId { paid[c] = false } }
  fair action decide(c: CaseId) {
    requires paid[c] == false
    branches {
      when true { paid[c] = true } maps settle(c)
    }
  }
}
'''


def test_inline_action_map_referencing_pre_split_branch_name_is_an_error(tmp_path):
    # Documents current behavior: `branches` splits `decide` into aliased kernel
    # actions (`decide__b1`, ...; dialects.py `_split_branch_action`), so the
    # impl spec `build_refinement` type-checks against never has a `decide`
    # action. An inline `action decide(...) -> ...` item referencing the
    # pre-split name is therefore "unknown impl action", matching what the
    # separate-file path would raise for the same reference.
    _write(tmp_path, {"abs.fsl": ABS_BRANCH, "impl.fsl": IMPL_BRANCH_ORIGINAL_NAME})

    checked = run_check(str(tmp_path / "impl.fsl"))
    assert checked["result"] == "error"
    assert checked["kind"] == "type"
    assert "unknown impl action 'decide'" in checked["message"]
