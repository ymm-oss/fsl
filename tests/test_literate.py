# SPDX-License-Identifier: Apache-2.0
# Copyright 2026 Ryoichi Izumita

"""Markdown-embedded literate FSL parsing (issue #193)."""
from __future__ import annotations

from fslc.cli import run_check, run_verify
from fslc.literate import is_literate_source
from fslc.lsp.index import build_index


def _write(tmp_path, name, source):
    path = tmp_path / name
    path.write_text(source, encoding="utf-8")
    return path


def _strip_unstable(value):
    if isinstance(value, dict):
        return {
            key: _strip_unstable(item)
            for key, item in value.items()
            if key not in ("cost", "cache")
        }
    if isinstance(value, list):
        return [_strip_unstable(item) for item in value]
    return value


def test_syntax_error_location_is_markdown_line(tmp_path):
    source = """# Requirement

The formal rule is embedded below.

```fsl
spec BadDoc {
  state { flag: Bool }
  init { flag = false }
  action stay() { flag = flag }
  invariant Safe { flag and }
}
```
"""
    path = _write(tmp_path, "bad.md", source)

    out = run_check(str(path))

    assert out["result"] == "error"
    assert out["kind"] == "parse"
    assert out["loc"] == {"line": 10, "column": 29}


def test_multiple_blocks_equal_one_compilation_unit(tmp_path):
    literate = _write(tmp_path, "split.md", """# Split spec

```fsl
spec Split {
  state { flag: Bool }
  init { flag = false }
  action enable() { requires not flag  flag = true }
```

The invariant stays beside its prose requirement.

```fsl
  invariant BoolFlag { flag or not flag }
}
```
""")
    plain = _write(tmp_path, "plain.fsl", """spec Split {
  state { flag: Bool }
  init { flag = false }
  action enable() { requires not flag  flag = true }
  invariant BoolFlag { flag or not flag }
}
""")

    literate_result = run_verify(str(literate), 2, "ignore", use_cache=False)
    plain_result = run_verify(str(plain), 2, "ignore", use_cache=False)

    assert _strip_unstable(literate_result) == _strip_unstable(plain_result)


def test_non_fsl_markdown_is_not_sniffed(tmp_path):
    no_fence = "# prose only\n\nNothing formal here.\n"
    python_fence = "# example\n\n```python\nprint('not fsl')\n```\n"

    assert is_literate_source(no_fence) is False
    assert is_literate_source(python_fence) is False
    assert run_check(str(_write(tmp_path, "prose.md", no_fence)))["kind"] == "parse"
    assert run_check(str(_write(tmp_path, "python.md", python_fence)))["kind"] == "parse"


def test_counterexample_action_location_is_markdown_line(tmp_path):
    path = _write(tmp_path, "violated.md", """# Safety

The action below must preserve zero.

```fsl
spec ViolatedDoc {
  type X = 0..1
  state { x: X }
  init { x = 0 }
  action break_rule() { x = 1 }
  invariant StaysZero { x == 0 }
}
```
""")

    out = run_verify(str(path), 1, "ignore", use_cache=False)

    assert out["result"] == "violated"
    assert out["last_action"]["name"] == "break_rule"
    assert out["last_action"]["loc"] == {"line": 10, "column": 3}


def test_lsp_indexes_original_markdown_positions():
    source = """# Review

```fsl
spec IndexedDoc {
  state { flag: Bool }
  init { flag = false }
  action enable() { flag = true }
  invariant Safe { flag }
}
```
"""

    index = build_index(source, "indexed.md")
    spec = next(item for item in index.symbols if item.role == "spec")
    action = next(item for item in index.symbols if item.role == "action")

    assert spec.selection_range.start.line == 3
    assert action.selection_range.start.line == 6


def test_lsp_dispatches_embedded_business_dialect_at_document_positions():
    source = """# Business flow

```fsl
business ReturnHandling {
  actor Customer, Manager
  entity Return

  process Return {
    stages Requested, Approved
    initial Requested
    transition approve Requested -> Approved by Manager
  }
}
verify { instances Return = 1 }
```
"""

    index = build_index(source, "business.md")
    business = next(item for item in index.symbols if item.name == "ReturnHandling")
    transition = next(item for item in index.symbols if item.name == "approve")

    assert business.selection_range.start.line == 3
    assert transition.selection_range.start.line == 10


def test_compose_import_resolves_from_markdown_directory(tmp_path):
    _write(tmp_path, "child.fsl", """spec Child {
  type X = 0..0
  state { flag: Bool }
  init { flag = false }
  action enable(x: X) { requires not flag  flag = true }
}
""")
    root = _write(tmp_path, "root.md", """# Composite

```fsl
compose Root {
  use Child as child from "child.fsl"
  action enable(x: child.X) = child.enable(x) { }
  internal child.enable
}
```
""")

    out = run_check(str(root))

    assert out["result"] == "ok"
    assert out["spec"] == "Root"


def test_prose_edit_changes_raw_verify_cache_key(tmp_path, monkeypatch):
    monkeypatch.setenv("FSLC_CACHE", "on")
    monkeypatch.setenv("FSLC_CACHE_DIR", str(tmp_path / "cache"))
    source = """# First prose

```fsl
spec CachedDoc {
  state { flag: Bool }
  init { flag = false }
  action enable() { requires not flag  flag = true }
  invariant Safe { flag or not flag }
}
```
"""
    path = _write(tmp_path, "cached.md", source)
    first = run_verify(str(path), 2, "ignore")
    second = run_verify(str(path), 2, "ignore")
    path.write_text(source.replace("First prose", "Edited prose"), encoding="utf-8")
    edited = run_verify(str(path), 2, "ignore")

    assert "cache" not in first
    assert second["cache"]["hit"] is True
    assert "cache" not in edited


def test_unclosed_fsl_fence_is_parse_error(tmp_path):
    path = _write(tmp_path, "unclosed.md", """# Broken

```fsl
spec Broken { state { x: Bool } }
""")

    out = run_check(str(path))

    assert out["result"] == "error"
    assert out["kind"] == "parse"
    assert out["loc"] == {"line": 3, "column": 1}
