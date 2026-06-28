# SPDX-License-Identifier: Apache-2.0
# Copyright 2026 Ryoichi Izumita

"""Inline anonymous range types in state declarations."""

import pytest

from fslc import FslError, build_spec, parse, verify


INLINE_SRC = """
spec InlineRange {
  state { x: 0..3 }
  init { x = 0 }
  action up() {
    requires x < 3
    x = x + 1
  }
  invariant B { x <= 3 }
}
"""

NAMED_SRC = """
spec NamedRange {
  type R = 0..3
  state { x: R }
  init { x = 0 }
  action up() {
    requires x < 3
    x = x + 1
  }
  invariant B { x <= 3 }
}
"""


def test_inline_range_state_type_parses_builds_and_verifies():
    spec = build_spec(parse(INLINE_SRC))

    assert spec["state"]["x"] == ("domain", 0, 3)
    assert spec["state_type_refs"]["x"] == ("domain", 0, 3)

    out = verify(spec, 5)
    assert out["result"] == "verified"
    assert "B" in out["invariants_checked"]
    assert "_bounds_x" in out["invariants_checked"]


def test_inline_range_matches_named_domain_verdict():
    inline = verify(build_spec(parse(INLINE_SRC)), 5)
    named = verify(build_spec(parse(NAMED_SRC)), 5)

    assert inline["result"] == named["result"] == "verified"


def test_inline_range_empty_range_is_type_error():
    src = """
spec EmptyInlineRange {
  state { x: 3..0 }
  init { x = 0 }
}
"""

    with pytest.raises(FslError) as exc:
        build_spec(parse(src))

    assert exc.value.kind == "type"
    assert "inline range type has empty range 3..0" in str(exc.value)
