import json
import subprocess
import sys
from pathlib import Path

from fslc.cli import run_check, run_verify


ROOT = Path(__file__).resolve().parents[1]


def _write(tmp_path, src, name="case.fsl"):
    path = tmp_path / name
    path.write_text(src, encoding="utf-8")
    return path


def _strict_warnings(out):
    return [w for w in out.get("warnings", []) if w.get("kind") in {
        "untagged",
        "unreferenced_requirement",
    }]


def test_strict_tags_warns_for_each_untagged_element_with_name_and_loc(tmp_path):
    path = _write(tmp_path, """
spec UntaggedKinds {
  state { x: Int }
  init { x = 0 }
  action go() { x = 1 }
  invariant Safe { x >= 0 }
  reachable Done { x == 1 }
  leadsTo Eventually { x == 0 ~> x == 1 }
}
""")

    out = run_check(str(path), strict_tags=True)
    warnings = _strict_warnings(out)

    assert out["result"] == "ok"
    assert {(w["element"], w["name"]) for w in warnings} == {
        ("action", "go"),
        ("invariant", "Safe"),
        ("reachable", "Done"),
        ("leadsTo", "Eventually"),
    }
    assert all(w.get("loc", {}).get("line") for w in warnings)


def test_model_and_assume_tags_are_clean(tmp_path):
    path = _write(tmp_path, """
spec TaggedKinds {
  state { x: Int }
  init { x = 0 }
  action go() "MODEL: abstract progress" { x = 1 }
  invariant Safe "ASSUME-1: x is bounded by the model" { x >= 0 }
  reachable Done "MODEL: smoke reachability" { x == 1 }
  leadsTo Eventually "ASSUME-2: scheduler eventually runs go" { x == 0 ~> x == 1 }
}
""")

    out = run_check(str(path), strict_tags=True)

    assert out["result"] == "ok"
    assert _strict_warnings(out) == []


def test_cancel_flow_business_kpis_are_not_flagged():
    out = run_check(str(ROOT / "examples" / "pm" / "cancel_flow.fsl"), strict_tags=True)

    assert out["result"] == "ok"
    assert _strict_warnings(out) == []


def test_time_block_generated_tick_is_not_flagged(tmp_path):
    path = _write(tmp_path, """
requirements TickGenerated {
  state { open: Bool }
  init { open = true }
  time {
    age waiting while open
  }
  requirement REQ-1 "open sessions expire" {
    deadline waiting <= 2
  }
}
""")

    out = run_check(str(path), strict_tags=True)

    assert out["result"] == "ok"
    assert not any(w["kind"] == "untagged" and w["name"] == "tick" for w in out["warnings"])
    assert _strict_warnings(out) == []


def test_user_tick_without_time_block_is_flagged(tmp_path):
    path = _write(tmp_path, """
spec UserTick {
  state { x: Int }
  init { x = 0 }
  action tick() { x = x + 1 }
  invariant Any "MODEL: baseline" { true }
}
""")

    out = run_check(str(path), strict_tags=True)

    assert out["result"] == "ok"
    assert ("action", "tick") in {
        (w["element"], w["name"]) for w in _strict_warnings(out)
    }


def test_requirements_file_unreferenced_id_warns(tmp_path):
    path = _write(tmp_path, """
spec ExternalIds {
  state { x: Int }
  init { x = 0 }
  action idle() "REQ-1: idle is allowed" { x = x }
  invariant Any "MODEL: tautology" { true }
}
""")
    ids = tmp_path / "ids.txt"
    ids.write_text("REQ-1\nREQ-MISSING\n", encoding="utf-8")

    out = run_check(str(path), strict_tags=True, requirements=str(ids))

    assert ("unreferenced_requirement", "REQ-MISSING") in {
        (w["kind"], w["name"]) for w in _strict_warnings(out)
    }


def test_empty_requirement_block_is_auto_collected_and_unreferenced(tmp_path):
    path = _write(tmp_path, """
requirements EmptyBlock {
  state { x: Int }
  init { x = 0 }
  action idle() "MODEL: placeholder transition" { x = x }
  invariant Any "MODEL: tautology" { true }
  requirement REQ-EMPTY "not formalized yet" {
  }
}
""")

    out = run_check(str(path), strict_tags=True)

    assert ("unreferenced_requirement", "REQ-EMPTY") in {
        (w["kind"], w["name"]) for w in _strict_warnings(out)
    }


def test_acceptance_and_forbidden_ids_count_as_referenced(tmp_path):
    path = _write(tmp_path, """
requirements AcceptanceForbidden {
  type K = 0..0
  state { x: Bool }
  init { x = false }
  action go(k: K) "MODEL: state advance" {
    requires not x
    x = true
  }
  invariant Any "MODEL: tautology" { true }
  acceptance AC-1 "go succeeds" {
    go(0)
    expect x == true
  }
  forbidden FB-1 "go cannot repeat" {
    go(0)
    go(0)
    expect rejected
  }
}
""")
    ids = tmp_path / "ids.txt"
    ids.write_text("AC-1\nFB-1\n", encoding="utf-8")

    out = run_check(str(path), strict_tags=True, requirements=str(ids))

    assert out["result"] == "ok"
    assert _strict_warnings(out) == []


def test_violated_spec_does_not_emit_strict_lint(tmp_path):
    path = _write(tmp_path, """
spec ViolatedUntagged {
  state { x: Int }
  init { x = 0 }
  action break_it() { x = 1 }
  invariant StaysZero { x == 0 }
}
""")

    out = run_verify(str(path), 1, "ignore", strict_tags=True)

    assert out["result"] == "violated"
    assert _strict_warnings(out) == []


def test_sla_worker_reports_only_genuine_untagged_findings():
    out = run_check(str(ROOT / "examples" / "nfr" / "sla_worker.fsl"), strict_tags=True)
    untagged = {
        (w["element"], w["name"])
        for w in _strict_warnings(out)
        if w["kind"] == "untagged"
    }

    assert out["result"] == "ok"
    assert untagged == {
        ("action", "submit"),
        ("action", "start"),
        ("action", "finish"),
        ("invariant", "PendingServedExclusive"),
        ("invariant", "ServingIsPending"),
        ("reachable", "BothServed"),
    }
    assert ("action", "tick") not in untagged


def test_default_cli_output_has_no_strict_lint_without_flag(tmp_path):
    path = _write(tmp_path, """
spec DefaultClean {
  state { x: Int }
  init { x = 0 }
  action idle() { x = x }
  invariant Any { true }
}
""")

    proc = subprocess.run(
        [sys.executable, "-m", "fslc", "check", str(path)],
        cwd=ROOT,
        text=True,
        stdout=subprocess.PIPE,
        stderr=subprocess.PIPE,
        check=False,
    )

    assert proc.returncode == 0, proc.stdout + proc.stderr
    out = json.loads(proc.stdout)
    assert out["result"] == "ok"
    assert _strict_warnings(out) == []
