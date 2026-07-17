# SPDX-License-Identifier: Apache-2.0

"""Frozen Python compatibility for native Kernel inline initializers."""

from fslc.model import build_spec
from fslc.parser import parse_src


def _build(source: str):
    tree, display_names = parse_src(source)
    return build_spec(tree, display_names)


def test_inline_initializer_projects_to_the_existing_python_init_shape():
    inline = _build(
        """
spec Compat {
  enum Status { Pending, Done }
  state { status: Status = Pending, active: Bool = false }
  action stay() { status = status }
}
"""
    )
    explicit = _build(
        """
spec Compat {
  enum Status { Pending, Done }
  state { status: Status, active: Bool }
  init { status = Pending active = false }
  action stay() { status = status }
}
"""
    )

    assert [statement[:3] for statement in inline["init"]] == [
        statement[:3] for statement in explicit["init"]
    ]
