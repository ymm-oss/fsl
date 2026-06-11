from pathlib import Path

from fslc.cli import run_check, run_verify


ROOT = Path(__file__).resolve().parents[1]
SLA_WORKER = ROOT / "examples" / "nfr" / "sla_worker.fsl"


def _write(tmp_path, src, name="nfr_case.fsl"):
    path = tmp_path / name
    path.write_text(src, encoding="utf-8")
    return path


def _without_urgent(src):
    return src.replace("    urgent start, finish\n", "")


def test_nfr_sla_worker_bmc_verified():
    checked = run_check(str(SLA_WORKER))
    assert checked["result"] == "ok"

    result = run_verify(str(SLA_WORKER), 10, "ignore")

    assert result["result"] == "verified"
    assert "_deadline_NFR_1_age_1" in result["invariants_checked"]
    assert result["action_coverage"]["tick"] is True


def test_nfr_sla_worker_without_urgent_is_violated(tmp_path):
    src = SLA_WORKER.read_text(encoding="utf-8")
    path = _write(tmp_path, _without_urgent(src), "sla_worker_no_urgent.fsl")

    result = run_verify(str(path), 10, "ignore")

    assert result["result"] == "violated"
    assert result["invariant"] == "_deadline_NFR_1_age_1"
    assert result["requirement"] == {
        "id": "NFR-1",
        "text": "submitted requests finish within 4 ticks",
    }
    assert result["last_action"]["name"] == "tick"
    assert [step.get("action", {}).get("name") for step in result["trace"]].count("tick") >= 5


def test_nfr_sla_worker_auxiliary_invariant_induction_proved(tmp_path):
    src = SLA_WORKER.read_text(encoding="utf-8").replace(
        "  requirement NFR-1",
        "  invariant AgeZeroUnderUrgency { forall r: Req { age[r] == 0 } }\n\n"
        "  requirement NFR-1",
    )
    path = _write(tmp_path, src, "sla_worker_aux.fsl")

    result = run_verify(str(path), 10, "ignore", engine="induction")

    assert result["result"] == "proved"
    assert "AgeZeroUnderUrgency" in result["invariants_checked"]


BASE_STATIC = r'''requirements StaticNfr {
  type Req = 0..1
  state { pending: Map<Req, Bool> }
  init { forall r: Req { pending[r] = false } }
  action submit(r: Req) { pending[r] = true }
  invariant I { true }
%s
}
'''


def _static_error(tmp_path, body):
    path = _write(tmp_path, BASE_STATIC % body)
    result = run_check(str(path))
    assert result["result"] == "error"
    assert result["kind"] == "type"
    return result["message"]


def test_nfr_static_error_unused_age(tmp_path):
    message = _static_error(tmp_path, '  time { age age[r: Req] while pending[r] }\n')
    assert "unused age 'age'" in message


def test_nfr_static_error_undeclared_deadline_reference(tmp_path):
    message = _static_error(
        tmp_path,
        '''  time { age age[r: Req] while pending[r] }
  requirement NFR-1 "deadline" { deadline missing <= 4 }
''',
    )
    assert "deadline references undeclared age 'missing'" in message


def test_nfr_static_error_unknown_urgent(tmp_path):
    message = _static_error(
        tmp_path,
        '''  time {
    urgent missing
    age age[r: Req] while pending[r]
  }
  requirement NFR-1 "deadline" { deadline age <= 4 }
''',
    )
    assert "unknown urgent action 'missing'" in message


def test_nfr_static_error_tick_name_collision(tmp_path):
    path = _write(
        tmp_path,
        r'''requirements TickCollision {
  type Req = 0..1
  state { pending: Map<Req, Bool> }
  init { forall r: Req { pending[r] = false } }
  action tick() { }
  time { age age[r: Req] while pending[r] }
  requirement NFR-1 "deadline" { deadline age <= 4 }
  invariant I { true }
}
''',
    )

    result = run_check(str(path))

    assert result["result"] == "error"
    assert result["kind"] == "type"
    assert "action 'tick' already exists" in result["message"]


def test_nfr_static_error_duplicate_time_block(tmp_path):
    message = _static_error(
        tmp_path,
        '''  time { age age[r: Req] while pending[r] }
  time { age other[r: Req] while pending[r] }
  requirement NFR-1 "deadline" { deadline age <= 4 }
''',
    )
    assert "time block only once" in message


def test_nfr_static_error_deadline_without_time(tmp_path):
    message = _static_error(
        tmp_path,
        '  requirement NFR-1 "deadline" { deadline age <= 4 }\n',
    )
    assert "deadline requires a time block" in message
