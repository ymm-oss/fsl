"""Tests for enum member names (and const/bool literals) as acceptance/forbidden
action arguments (#67). The expression grammar already parsed these; the only
rejection point was `_literal_value` in `fslc.acceptance`, whose `var` branch
only resolved `spec["consts"]`."""
from fslc.acceptance import _literal_value, validate_acceptance, validate_forbidden
from fslc.cli import run_check
from fslc.model import build_spec
from fslc.parser import parse_src


def _spec_from(src):
    ast, display = parse_src(src, ".")
    return build_spec(ast, display)


SIGNAL_SRC = r'''requirements Signal {{
  type Id = 0..0
  enum Trigger {{ Idle, Triggered }}
  enum Auth {{ Pending, Authorized }}
  state {{ trig: Map<Id, Trigger>, auth: Map<Id, Auth> }}
  init {{ forall i: Id {{ trig[i] = Idle  auth[i] = Pending }} }}

  requirement REQ-1 "answer records trigger and auth" {{
    action answer(i: Id, t: Trigger, a: Auth) {{
      requires trig[i] == Idle
      trig[i] = t
      auth[i] = a
    }}
  }}

  acceptance AC-1 "answer" {{
    answer({args})
    expect trig[0] == Triggered and auth[0] == Authorized
  }}

  forbidden FB-1 "answering twice is rejected" {{
    answer({args})
    answer({args})
    expect rejected
  }}
}}'''

ENUM_NAMES_SRC = SIGNAL_SRC.format(args="0, Triggered, Authorized")
ORDINALS_SRC = SIGNAL_SRC.format(args="0, 1, 1")

BAD_NAME_SRC = SIGNAL_SRC.format(args="0, Bogus, Authorized")


def test_acceptance_enum_member_names_equivalent_to_ordinals():
    enum_spec = _spec_from(ENUM_NAMES_SRC)
    ordinal_spec = _spec_from(ORDINALS_SRC)

    enum_result = validate_acceptance(enum_spec)
    ordinal_result = validate_acceptance(ordinal_spec)

    assert enum_result["ok"] is True
    assert ordinal_result["ok"] is True
    assert enum_result["scenarios"][0]["steps"] == ordinal_result["scenarios"][0]["steps"]
    assert enum_result["scenarios"][0]["steps"][0]["params"] == {"i": 0, "t": 1, "a": 1}


def test_forbidden_enum_member_names_equivalent_to_ordinals():
    enum_spec = _spec_from(ENUM_NAMES_SRC)
    ordinal_spec = _spec_from(ORDINALS_SRC)

    enum_result = validate_forbidden(enum_spec)
    ordinal_result = validate_forbidden(ordinal_spec)

    assert enum_result["ok"] is True
    assert ordinal_result["ok"] is True
    assert enum_result["scenarios"][0]["steps"] == ordinal_result["scenarios"][0]["steps"]
    assert enum_result["scenarios"][0]["rejected_by"] == ordinal_result["scenarios"][0]["rejected_by"]


def test_acceptance_and_forbidden_pass_check_with_enum_member_names(tmp_path):
    path = tmp_path / "signal.fsl"
    path.write_text(ENUM_NAMES_SRC, encoding="utf-8")

    assert run_check(str(path))["result"] == "ok"


def test_undefined_name_in_acceptance_arg_still_errors_with_updated_message(tmp_path):
    path = tmp_path / "signal.fsl"
    path.write_text(BAD_NAME_SRC, encoding="utf-8")

    result = run_check(str(path))

    assert result["result"] == "error"
    assert result["kind"] == "acceptance"
    assert result["id"] == "AC-1"
    assert "undefined const or enum member 'Bogus'" in result["message"]


def test_literal_value_accepts_bool_literal():
    # Bool-typed action parameters are not implemented yet (tracked separately
    # as #68), so a bool literal can't reach `_literal_value` through a real
    # acceptance/forbidden action argument today. This exercises the `bool`
    # branch directly to confirm it already accepts `true`/`false` literals,
    # unaffected by the const/enum fallback added for enum member names.
    spec = _spec_from(ENUM_NAMES_SRC)
    assert _literal_value(("bool", True), spec) is True
    assert _literal_value(("bool", False), spec) is False
