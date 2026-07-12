# SPDX-License-Identifier: Apache-2.0
# Copyright 2026 Ryoichi Izumita

"""Behavioral parity checks for the native Rust CLI."""

from __future__ import annotations

import json
import os
import subprocess
from pathlib import Path


ROOT = Path(__file__).resolve().parents[1]
RUST = ROOT / "rust" / "target" / "debug" / "fslc"

COUNTER_SPEC = """
spec Counter {
  type Count = 0..2
  state { count: Count }
  init { count = 0 }
  action add(delta: Count) {
    requires delta == 1
    count = count + delta
  }
  invariant WithinBound { count <= 2 }
}
"""

MAPPING = """
refinement ProdCounterToCounter {
  impl ProdCounter
  abs Counter
  map count = raw_count
  action increment(amount) -> add(amount)
}
"""


def _run_rust(
    *args: str, cwd: Path = ROOT, env: dict[str, str] | None = None
) -> tuple[int, dict]:
    proc = subprocess.run(
        [str(RUST), *args],
        cwd=cwd,
        text=True,
        capture_output=True,
        check=False,
        env=env,
    )
    assert proc.stdout, proc.stderr
    return proc.returncode, json.loads(proc.stdout)


def _run_rust_raw(*args: str, cwd: Path = ROOT) -> tuple[int, str, str]:
    proc = subprocess.run(
        [str(RUST), *args],
        cwd=cwd,
        text=True,
        capture_output=True,
        check=False,
    )
    return proc.returncode, proc.stdout, proc.stderr


def _log_fixture(tmp_path: Path, records: list[dict]) -> tuple[Path, Path, Path]:
    spec = tmp_path / "counter.fsl"
    mapping = tmp_path / "mapping.fsl"
    log = tmp_path / "events.jsonl"
    spec.write_text(COUNTER_SPEC, encoding="utf-8")
    mapping.write_text(MAPPING, encoding="utf-8")
    log.write_text(
        "".join(json.dumps(record) + "\n" for record in records),
        encoding="utf-8",
    )
    return spec, mapping, log


def test_rust_log_replay_maps_actions_and_state(tmp_path):
    spec, mapping, log = _log_fixture(
        tmp_path,
        [
            {"action": "increment", "params": {"amount": 1}, "state": {"raw_count": 1}},
            {"action": "increment", "params": {"amount": 1}, "state": {"raw_count": 2}},
        ],
    )

    status, result = _run_rust(
        "replay", str(spec), "--from-log", str(log), "--mapping", str(mapping)
    )

    assert status == 0, result
    assert result["result"] == "conformant"
    assert result["source"] == "jsonl_mapping"
    assert result["steps_checked"] == 2
    assert result["final_state"] == {"count": 2}


def test_rust_log_replay_reports_state_mismatch(tmp_path):
    spec, mapping, log = _log_fixture(
        tmp_path,
        [{"action": "increment", "params": {"amount": 1}, "state": {"raw_count": 0}}],
    )

    status, result = _run_rust(
        "replay", str(spec), "--from-log", str(log), "--mapping", str(mapping)
    )

    assert status == 1
    assert result["result"] == "nonconformant"
    assert result["failed_at_record"] == 0
    assert result["log_line"] == 1
    assert result["violation"] == {
        "kind": "state_mismatch",
        "source_action": "increment",
        "action": "add",
        "expected_state": {"count": 1},
        "observed_state": {"count": 0},
        "mismatches": [{"path": "count", "expected": 1, "observed": 0}],
    }


def test_rust_log_replay_reports_requires_rejection(tmp_path):
    spec, mapping, log = _log_fixture(
        tmp_path,
        [{"action": "increment", "params": {"amount": 2}, "state": {"raw_count": 2}}],
    )

    status, result = _run_rust(
        "replay", str(spec), "--from-log", str(log), "--mapping", str(mapping)
    )

    assert status == 1
    assert result["violation"]["kind"] == "requires_failed"
    assert result["violation"]["source_action"] == "increment"
    assert result["violation"]["mapped_action"] == "add"


def test_rust_induction_uses_independently_proved_lemma(tmp_path):
    spec = tmp_path / "sync.fsl"
    spec.write_text(
        """
spec Sync {
  state { x: Int, y: Int }
  init { x = 0  y = 0 }
  action step() { requires x < 4  x = x + 1  y = y + 1 }
  invariant Sync { y <= 4 }
}
""",
        encoding="utf-8",
    )

    status, result = _run_rust(
        "verify",
        str(spec),
        "--depth",
        "8",
        "--deadlock",
        "ignore",
        "--engine",
        "induction",
        "--lemma",
        "x == y",
        "--no-cache",
    )

    assert status == 0, result
    assert result["result"] == "proved"
    assert result["lemmas"][0]["status"] == "proved"
    assert result["lemmas"][0]["used"] is True
    assert result["lemma_cti_exclusions"][0]["violated_steps"] == [0, 1]
    assert result["invariants_checked"][-1] == "AuxiliaryLemma1"


def test_rust_induction_rejects_false_lemma(tmp_path):
    spec = tmp_path / "sync.fsl"
    spec.write_text(
        """
spec Sync {
  state { x: Int, y: Int }
  init { x = 0  y = 0 }
  action step() { requires x < 4  x = x + 1  y = y + 1 }
  invariant Sync { y <= 4 }
}
""",
        encoding="utf-8",
    )

    status, result = _run_rust(
        "verify",
        str(spec),
        "--depth",
        "8",
        "--deadlock",
        "ignore",
        "--engine",
        "induction",
        "--lemma",
        "x <= 0",
        "--no-cache",
    )

    assert status == 1
    assert result["result"] == "unknown_cti"
    assert result["lemmas"][0]["status"] == "rejected"
    assert result["lemmas"][0]["proof"]["result"] == "violated"


def test_rust_semantic_diff_finds_behavior_and_enforces_gate(tmp_path):
    old = tmp_path / "old.fsl"
    new = tmp_path / "new.fsl"
    old.write_text(
        "spec Old { state { flag: Bool } init { flag = false } "
        "action enable() { requires not flag flag = true } }",
        encoding="utf-8",
    )
    new.write_text(
        "spec New { state { flag: Bool } init { flag = false } "
        "action enable() { flag = true } }",
        encoding="utf-8",
    )

    status, result = _run_rust(
        "diff",
        str(old),
        str(new),
        "--depth",
        "3",
        "--forbid",
        "behavior_added",
    )

    assert status == 1, result
    assert result["result"] == "semantic_diff"
    assert result["summary"] == ["behavior_added"]
    assert result["gate"] == {
        "forbidden": ["behavior_added"],
        "violations": ["behavior_added"],
        "passed": False,
    }


def test_rust_semantic_diff_finds_weakened_invariant(tmp_path):
    old = tmp_path / "old.fsl"
    new = tmp_path / "new.fsl"
    old.write_text(
        "spec Old { type X = 0..2 state { x: X } init { x = 0 } "
        "action advance() { requires x == 0 x = 1 } invariant Limit { x <= 1 } }",
        encoding="utf-8",
    )
    new.write_text(
        "spec New { type X = 0..2 state { x: X } init { x = 0 } "
        "action advance() { requires x == 0 x = 1 } invariant Limit { x >= 0 } }",
        encoding="utf-8",
    )

    status, result = _run_rust("diff", str(old), str(new), "--depth", "2")

    assert status == 0, result
    assert "invariant_weakened" in result["summary"]
    finding = next(
        finding for finding in result["findings"] if finding["kind"] == "invariant_weakened"
    )
    assert finding["witness"]["state"]["x"] == 2


def test_rust_semantic_diff_materializes_complete_git_trees(tmp_path):
    repo = tmp_path / "repo"
    repo.mkdir()

    def git(*args: str) -> str:
        return subprocess.run(
            ["git", *args],
            cwd=repo,
            text=True,
            capture_output=True,
            check=True,
        ).stdout.strip()

    git("init", "-q")
    child = repo / "child.fsl"
    root = repo / "root.fsl"
    child.write_text(
        "spec Child { type X = 0..0 state { flag: Bool } init { flag = false } "
        "action enable(x: X) { requires not flag flag = true } }",
        encoding="utf-8",
    )
    root.write_text(
        'compose Root { use Child as child from "child.fsl" '
        "action enable(x: child.X) = child.enable(x) { } internal child.enable }",
        encoding="utf-8",
    )
    git("add", ".")
    git(
        "-c",
        "user.name=FSL Test",
        "-c",
        "user.email=fsl@example.invalid",
        "commit",
        "-qm",
        "base",
    )
    base = git("rev-parse", "HEAD")
    child.write_text(
        "spec Child { type X = 0..0 state { flag: Bool } init { flag = false } "
        "action enable(x: X) { flag = true } }",
        encoding="utf-8",
    )
    git("add", ".")
    git(
        "-c",
        "user.name=FSL Test",
        "-c",
        "user.email=fsl@example.invalid",
        "commit",
        "-qm",
        "head",
    )
    head = git("rev-parse", "HEAD")

    status, result = _run_rust(
        "diff", "--git", f"{base}..{head}", "root.fsl", "--depth", "2", cwd=repo
    )

    assert status == 0, result
    assert result["result"] == "semantic_diff"
    assert result["summary"] == ["behavior_added"]
    assert result["vcs"]["materialization"] == "git_archive_full_tree"


def test_rust_verify_cache_reuses_counterexample_across_depths(tmp_path):
    spec = tmp_path / "violated.fsl"
    spec.write_text(
        "spec Violated { state { flag: Bool } init { flag = false } "
        "action enable() { flag = true } invariant Never { not flag } }",
        encoding="utf-8",
    )
    environment = {"FSLC_CACHE_DIR": str(tmp_path / "cache"), "HOME": str(tmp_path)}

    first_status, first = _run_rust(
        "verify", str(spec), "--depth", "4", env=environment
    )
    second_status, second = _run_rust(
        "verify", str(spec), "--depth", "2", env=environment
    )

    assert first_status == second_status == 1
    assert first["violated_at_step"] == 1
    assert second["cache"]["hit"] is True
    assert second["cache"]["source"] == "cross_depth"


def test_rust_ledger_uses_implementation_log(tmp_path):
    log = tmp_path / "impl.json"
    log.write_text(
        json.dumps(
            [
                {"action": "submit", "params": {"r": 0}},
                {"action": "submit", "params": {"r": 0}},
            ]
        ),
        encoding="utf-8",
    )

    status, output, error = _run_rust_raw(
        "ledger",
        "examples/nfr/sla_worker.fsl",
        "--impl-log",
        str(log),
    )

    assert status == 0, error
    assert "実装ログ適合" in output
    assert "非適合" in output


def test_rust_ledger_groups_native_nfr_evidence_by_requirement(tmp_path):
    source = (ROOT / "examples" / "nfr" / "sla_worker.fsl").read_text(
        encoding="utf-8"
    )
    spec = tmp_path / "sla_no_urgent.fsl"
    spec.write_text(source.replace("    urgent start, finish\n", ""), encoding="utf-8")

    status, output, error = _run_rust_raw("ledger", str(spec), "--depth", "10")

    assert status == 0, error
    assert "NFR-1" in output
    assert "🔴 要確認" in output
    assert "`sla`" in output or "| sla |" in output
    assert "スケジューリング前提" in output


def test_rust_ledger_lists_all_verified_requirement_ids():
    status, output, error = _run_rust_raw(
        "ledger", "examples/nfr/support_sla.fsl", "--depth", "8"
    )

    assert status == 0, error
    assert "🔴 要確認" not in output
    for requirement_id in ("REQ-1", "REQ-3", "REQ-5"):
        assert requirement_id in output


def test_rust_ledger_renders_external_evidence(tmp_path):
    spec = tmp_path / "requirements.fsl"
    spec.write_text(
        """
requirements CounterReq {
  state { x: Int }
  init { x = 0 }
  requirement REQ-1 "counter stays non-negative" {
    action inc() { requires x < 5  x = x + 1 }
    invariant XRange { x >= 0 and x <= 5 }
  }
}
""",
        encoding="utf-8",
    )
    evidence = tmp_path / "db_observe.json"
    evidence.write_text(
        json.dumps(
            {
                "result": "observed_mismatch",
                "formal_result": "not_run",
                "requirements": ["REQ-1"],
            }
        ),
        encoding="utf-8",
    )

    status, output, error = _run_rust_raw(
        "ledger", str(spec), "--evidence", str(evidence)
    )

    assert status == 0, error
    external = output.split("## 外部エビデンス", 1)[1]
    assert "replay-observed" in external
    assert "REQ-1" in external


def test_rust_domain_generate_matches_python_for_every_target():
    environment = {**os.environ, "PYTHONHASHSEED": "0"}
    targets = ["typescript", "kotlin", "swift", "python", "rust"]
    for spec in sorted((ROOT / "examples" / "domain").glob("*.fsl")):
        relative = spec.relative_to(ROOT)
        for target in targets:
            python = subprocess.run(
                [
                    str(ROOT / ".venv" / "bin" / "python"),
                    "-m",
                    "fslc",
                    "domain",
                    "generate",
                    str(relative),
                    "--target",
                    target,
                ],
                cwd=ROOT,
                env=environment,
                capture_output=True,
                check=False,
            )
            rust = subprocess.run(
                [
                    str(RUST),
                    "domain",
                    "generate",
                    str(relative),
                    "--target",
                    target,
                ],
                cwd=ROOT,
                capture_output=True,
                check=False,
            )
            assert (rust.returncode, rust.stdout) == (
                python.returncode,
                python.stdout,
            ), (relative, target, python.stderr, rust.stderr)


def test_rust_domain_generate_rejects_invalid_profile_and_target():
    spec = "examples/domain/order_functional_ddd.fsl"

    profile_status, profile = _run_rust(
        "domain", "generate", spec, "--profile", "layered"
    )
    target_status, target = _run_rust(
        "domain", "generate", spec, "--target", "java"
    )

    assert profile_status == 2
    assert profile["result"] == "error"
    assert target_status == 2
    assert target["result"] == "error"


def test_rust_generic_check_recognizes_ai_agent_and_project():
    agent_status, agent = _run_rust(
        "check", "examples/ai/recursive_support_agent.fsl"
    )
    project_status, project = _run_rust(
        "check", "examples/ai/support_answer_quality.fsl"
    )

    assert (agent_status, agent["spec"], agent["dialect"]) == (
        0,
        "SupportOrchestrator",
        "fsl-ai-agent.v0",
    )
    assert (project_status, project["spec"], project["dialect"]) == (
        0,
        "support_answer_quality",
        "fsl-ai-project.v0",
    )


def test_rust_verify_preserves_parse_error_classification():
    for spec in (
        "examples/gallery/errors/parse_missing_expression.fsl",
        "examples/ai/recursive_support_agent.fsl",
        "examples/ai/support_answer_quality.fsl",
    ):
        status, output = _run_rust("verify", spec, "--depth", "3")
        assert status == 2
        assert output["result"] == "error"
        assert output["kind"] == "parse"


def test_rust_verify_executes_db_compatibility_kernel():
    status, output = _run_rust(
        "verify",
        "examples/db/unsafe_drop_column_with_old_server.fsl",
        "--depth",
        "3",
    )

    assert status == 1
    assert output["result"] == "violated"
    assert output["invariant"] == (
        "db_read__prod__supported__server_v1__schema___0___1__users__legacy_name"
    )
    assert output["violated_at_step"] == 1


def test_rust_unique_is_at_most_one_and_exactly_one_remains_exact():
    status, output = _run_rust(
        "verify", "examples/structural/rbac.fsl", "--depth", "3"
    )

    assert status == 0, output
    assert output["result"] == "verified"
    assert output["reachables"]["WellFormedAssignment"]["witnessed_at_step"] == 3
