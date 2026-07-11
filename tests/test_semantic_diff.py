# SPDX-License-Identifier: Apache-2.0
# Copyright 2026 Ryoichi Izumita

"""Semantic specification diff (issue #176)."""
from __future__ import annotations

import json
import subprocess
import textwrap
from pathlib import Path

from fslc.cli import _build_arg_parser, exit_code, run_diff, run_diff_git


SNAPSHOT = Path(__file__).parent / "snapshots" / "semantic_diff_behavior_added.json"
GIT_SNAPSHOT = Path(__file__).parent / "snapshots" / "semantic_diff_git.json"


def _write(tmp_path, name, source):
    path = tmp_path / name
    path.write_text(textwrap.dedent(source), encoding="utf-8")
    return str(path)


def _git(repo, *args):
    return subprocess.run(
        ["git", "-C", str(repo), *args],
        check=True,
        capture_output=True,
        text=True,
    ).stdout.strip()


def _commit(repo, message):
    _git(repo, "add", ".")
    _git(
        repo,
        "-c", "user.name=FSL Test",
        "-c", "user.email=fsl@example.invalid",
        "commit", "-m", message,
    )
    return _git(repo, "rev-parse", "HEAD")


def _flag_spec(name, requires="requires not flag", invariant=""):
    return f"""
    spec {name} {{
      state {{ flag: Bool }}
      init {{ flag = false }}
      action enable() {{
        {requires}
        flag = true
      }}
      {invariant}
    }}
    """


def test_weakened_guard_is_behavior_added_with_witness(tmp_path):
    old = _write(tmp_path, "old.fsl", _flag_spec("Old"))
    new = _write(tmp_path, "new.fsl", _flag_spec("New", requires=""))

    out = run_diff(old, new, depth=3)

    assert out["result"] == "semantic_diff"
    assert "behavior_added" in out["summary"]
    finding = next(item for item in out["findings"] if item["kind"] == "behavior_added")
    assert finding["direction"] == "new_to_old"
    assert finding["witness"]["trace_type"] == "counterexample"
    assert finding["witness"]["trace"]
    assert out["bounded"] == {"depth": 3, "completeness": "bounded"}
    assert exit_code(out) == 0


def test_behavior_added_json_snapshot(tmp_path):
    old = _write(
        tmp_path,
        "old.fsl",
        "spec Old { state { flag: Bool } init { flag = false } "
        "action enable() { requires not flag flag = true } }",
    )
    new = _write(
        tmp_path,
        "new.fsl",
        "spec New { state { flag: Bool } init { flag = false } "
        "action enable() { flag = true } }",
    )

    out = run_diff(old, new, depth=3)
    out["old"]["file"] = "old.fsl"
    out["new"]["file"] = "new.fsl"

    assert out == json.loads(SNAPSHOT.read_text(encoding="utf-8"))


def test_removed_invariant_conjunct_is_invariant_weakened(tmp_path):
    old = _write(
        tmp_path,
        "old.fsl",
        """
        spec Old {
          type X = 0..2
          state { x: X }
          init { x = 0 }
          action advance() { requires x == 0  x = 1 }
          invariant Limit { x >= 0 and x <= 1 }
        }
        """,
    )
    new = _write(
        tmp_path,
        "new.fsl",
        """
        spec New {
          type X = 0..2
          state { x: X }
          init { x = 0 }
          action advance() { requires x == 0  x = 1 }
          invariant Limit { x >= 0 }
        }
        """,
    )

    out = run_diff(old, new, depth=2)

    assert "invariant_weakened" in out["summary"]
    finding = next(item for item in out["findings"] if item["kind"] == "invariant_weakened")
    assert finding["witness"]["state"]["x"] == 2


def test_identical_semantics_report_no_semantic_change(tmp_path):
    old = _write(tmp_path, "old.fsl", _flag_spec("Old"))
    new = _write(tmp_path, "new.fsl", _flag_spec("New"))

    out = run_diff(old, new, depth=2)

    assert out["summary"] == ["no_semantic_change"]
    assert out["findings"] == []
    assert out["directions"]["new_to_old"]["result"] == "refines"
    assert out["directions"]["old_to_new"]["result"] == "refines"


def test_name_mismatch_is_unknown_without_mapping(tmp_path):
    old = _write(tmp_path, "old.fsl", _flag_spec("Old"))
    new = _write(
        tmp_path,
        "new.fsl",
        _flag_spec("New").replace("flag", "enabled"),
    )

    out = run_diff(old, new, depth=2)

    assert "unknown" in out["summary"]
    assert out["directions"]["new_to_old"]["result"] == "unknown"
    assert out["directions"]["old_to_new"]["result"] == "unknown"


def test_mapping_resolves_its_declared_direction_only(tmp_path):
    old = _write(tmp_path, "old.fsl", _flag_spec("Old"))
    new = _write(
        tmp_path,
        "new.fsl",
        _flag_spec("New").replace("enable()", "turn_on()").replace("flag", "enabled"),
    )
    mapping = _write(
        tmp_path,
        "mapping.fsl",
        """
        refinement NewRefinesOld {
          impl New
          abs Old
          map flag = enabled
          action turn_on() -> enable()
        }
        """,
    )

    out = run_diff(old, new, depth=2, mapping=mapping)

    assert out["directions"]["new_to_old"]["result"] == "refines"
    assert out["directions"]["old_to_new"]["result"] == "unknown"


def test_scope_change_uses_new_side_bounds(tmp_path):
    old = _write(
        tmp_path,
        "old.fsl",
        """
        spec Old {
          entity E
          state { current: E }
          init { current = 0 }
          action stay() { current = current }
        }
        verify { instances E = 1 }
        """,
    )
    new = _write(
        tmp_path,
        "new.fsl",
        """
        spec New {
          entity E
          state { current: E }
          init { current = 0 }
          action stay() { current = current }
        }
        verify { instances E = 2 }
        """,
    )

    out = run_diff(old, new, depth=1)

    assert "scope_changed" in out["summary"]
    assert out["scope"]["comparison"] == "new"
    assert out["scope"]["old"]["instances"] == {"E": 1}
    assert out["scope"]["new"]["instances"] == {"E": 2}
    assert out["scope"]["applied_to_old"]["instances"] == {"E": 2}


def test_old_forbidden_accepted_by_new_is_forbidden_relaxed(tmp_path):
    old_source = """
    requirements Old {
      type OrderId = 0..1
      enum OSt { Cart, Paid, Shipped, Cancelled }
      state { order: Map<OrderId, OSt> }
      init { forall o: OrderId { order[o] = Cart } }
      requirement REQ-1 "lifecycle" {
        action pay(o: OrderId) { requires order[o] == Cart  order[o] = Paid }
        action ship(o: OrderId) { requires order[o] == Paid  order[o] = Shipped }
        action cancel(o: OrderId) { requires order[o] == Paid  order[o] = Cancelled }
      }
      forbidden FB-1 "post-shipment cancellation is rejected" {
        pay(0) ship(0) cancel(0)
        expect rejected
      }
    }
    """
    new_source = old_source.replace("requirements Old", "requirements New").replace(
        "requires order[o] == Paid  order[o] = Cancelled",
        "requires order[o] == Paid or order[o] == Shipped  order[o] = Cancelled",
    )
    old = _write(tmp_path, "old.fsl", old_source)
    new = _write(tmp_path, "new.fsl", new_source)

    out = run_diff(old, new, depth=4)

    assert "forbidden_relaxed" in out["summary"]
    finding = next(item for item in out["findings"] if item["kind"] == "forbidden_relaxed")
    assert finding["id"] == "FB-1"
    assert finding["witness"]["trace"][-1]["action"] == "cancel"


def test_forbid_is_the_only_semantic_diff_exit_one(tmp_path):
    old = _write(tmp_path, "old.fsl", _flag_spec("Old"))
    new = _write(tmp_path, "new.fsl", _flag_spec("New", requires=""))

    analysis = run_diff(old, new, depth=2)
    gated = run_diff(old, new, depth=2, forbid=["behavior_added"])

    assert exit_code(analysis) == 0
    assert gated["gate"] == {
        "forbidden": ["behavior_added"],
        "violations": ["behavior_added"],
        "passed": False,
    }
    assert exit_code(gated) == 1


def test_diff_cli_contract():
    args = _build_arg_parser().parse_args([
        "diff", "old.fsl", "new.fsl", "--depth", "5",
        "--forbid", "behavior_added,invariant_weakened",
    ])

    assert args.old == "old.fsl"
    assert args.new == "new.fsl"
    assert args.depth == 5
    assert args.forbid == "behavior_added,invariant_weakened"


def test_git_diff_materializes_imports_from_each_revision(tmp_path, monkeypatch):
    repo = tmp_path / "repo"
    repo.mkdir()
    _git(repo, "init", "-q")
    _write(
        repo,
        "child.fsl",
        """
        spec Child {
          type X = 0..0
          state { flag: Bool }
          init { flag = false }
          action enable(x: X) { requires not flag  flag = true }
        }
        """,
    )
    _write(
        repo,
        "root.fsl",
        """
        compose Root {
          use Child as child from "child.fsl"
          action enable(x: child.X) = child.enable(x) { }
          internal child.enable
        }
        """,
    )
    base = _commit(repo, "base")
    _write(
        repo,
        "child.fsl",
        """
        spec Child {
          type X = 0..0
          state { flag: Bool }
          init { flag = false }
          action enable(x: X) { flag = true }
        }
        """,
    )
    head = _commit(repo, "head")
    monkeypatch.chdir(repo)

    out = run_diff_git(f"{base}..{head}", "root.fsl", depth=2)

    assert out["result"] == "semantic_diff", out
    assert "behavior_added" in out["summary"]
    assert out["old"]["file"] == f"{base}:root.fsl"
    assert out["new"]["file"] == f"{head}:root.fsl"
    assert out["vcs"]["materialization"] == "git_archive_full_tree"

    normalized = json.loads(json.dumps(out))
    normalized["old"]["file"] = "BASE:root.fsl"
    normalized["new"]["file"] = "HEAD:root.fsl"
    normalized["vcs"]["range"] = "BASE..HEAD"
    normalized["vcs"]["base"] = {"revision": "BASE", "commit": "base-commit"}
    normalized["vcs"]["head"] = {"revision": "HEAD", "commit": "head-commit"}
    assert normalized == json.loads(GIT_SNAPSHOT.read_text(encoding="utf-8"))


def test_git_diff_without_spec_enumerates_changed_fsl_files(tmp_path, monkeypatch):
    repo = tmp_path / "repo"
    repo.mkdir()
    _git(repo, "init", "-q")
    _write(repo, "a.fsl", _flag_spec("A"))
    _write(repo, "unchanged.fsl", _flag_spec("Unchanged"))
    base = _commit(repo, "base")
    _write(repo, "a.fsl", _flag_spec("A", requires=""))
    head = _commit(repo, "head")
    monkeypatch.chdir(repo)

    out = run_diff_git(f"{base}..{head}", depth=2)

    assert out["result"] == "semantic_diff_batch"
    assert out["specs"] == ["a.fsl"]
    assert len(out["comparisons"]) == 1
    assert exit_code(out) == 0


def test_two_path_diff_remains_git_independent(tmp_path, monkeypatch):
    outside = tmp_path / "not-a-repository"
    outside.mkdir()
    old = _write(outside, "old.fsl", _flag_spec("Old"))
    new = _write(outside, "new.fsl", _flag_spec("New"))
    monkeypatch.chdir(outside)

    out = run_diff(old, new, depth=1)

    assert out["result"] == "semantic_diff"
    assert out["summary"] == ["no_semantic_change"]


def test_git_diff_cli_contract():
    single = _build_arg_parser().parse_args([
        "diff", "--git", "main..HEAD", "specs/cart.fsl", "--depth", "4",
    ])
    batch = _build_arg_parser().parse_args(["diff", "--git", "main..HEAD"])

    assert single.git_range == "main..HEAD"
    assert single.old == "specs/cart.fsl"
    assert single.new is None
    assert single.depth == 4
    assert batch.git_range == "main..HEAD"
    assert batch.old is None
