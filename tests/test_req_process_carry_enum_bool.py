# SPDX-License-Identifier: Apache-2.0
# Copyright 2026 Ryoichi Izumita

"""Bool/enum carried process fields (requirements `process ... with`, #70).

Carried fields historically had to be a `number` type — a single history flag
or classification needed the full kernel-wrapper form. Bool/enum fields are
now allowed, but only with an explicit initializer: there is no invented
default for a Bool or an enum member, matching the numeric-field default
(domain `lo`) only ever being implicit for numbers.
"""
import pytest

from fslc import FslError, build_spec, parse, verify
from fslc.runtime import Monitor


BOOL_ENUM_SRC = r'''requirements CaseReq {
  entity Case
  enum Kind { Statement, Question }

  process Case with retried: Bool = false, kind: Kind = Statement {
    stages New, Done
    initial New

    transition finish New -> Done by System
      when retried == false
      set retried = true, kind = Question
  }

  requirement REQ-1 "finishing flips retried and kind" {
    reachable Finished {
      case_retried[0] == true and case_kind[0] == Question
    }
  }

  acceptance AC-1 "finish flips retried and kind" {
    finish(0)
    expect case_retried[0] == true and case_kind[0] == Question
  }
}
verify {
  instances Case = 2
}
'''


def test_bool_and_enum_carry_check_and_verify_agree():
    spec = build_spec(parse(BOOL_ENUM_SRC))

    result = verify(spec, 4, deadlock_mode="ignore")

    assert result["result"] == "verified", result
    witness = result["reachables"]["Finished"]["witness"]
    assert witness[-1]["state"]["case_retried"]["0"] is True
    assert witness[-1]["state"]["case_kind"]["0"] == "Question"


def test_bool_and_enum_carry_witness_replays_the_same_in_monitor():
    """BMC's witness for REQ-1 and the concrete Monitor must agree step by step."""
    spec = build_spec(parse(BOOL_ENUM_SRC))
    result = verify(spec, 4, deadlock_mode="ignore")
    witness = result["reachables"]["Finished"]["witness"]

    mon = Monitor(spec)
    mon.reset()
    for entry in witness[1:]:
        action = entry["action"]
        step = mon.step(action["name"], action["params"])
        assert step["ok"], step
        assert step["state"] == entry["state"], (step["state"], entry["state"])


def test_bool_carry_without_initializer_is_an_error():
    src = r'''requirements CaseReq {
  entity Case
  process Case with retried: Bool {
    stages New, Done
    initial New
    transition finish New -> Done by System
  }
}
verify {
  instances Case = 1
}
'''
    with pytest.raises(FslError) as exc:
        parse(src)
    assert exc.value.kind == "type"
    assert str(exc.value) == "carried Bool field requires an explicit initializer (= true / = false)"


def test_enum_carry_without_initializer_is_an_error():
    src = r'''requirements CaseReq {
  entity Case
  enum Kind { Statement, Question }
  process Case with kind: Kind {
    stages New, Done
    initial New
    transition finish New -> Done by System
  }
}
verify {
  instances Case = 1
}
'''
    with pytest.raises(FslError) as exc:
        parse(src)
    assert exc.value.kind == "type"
    assert "carried enum field 'kind' requires an explicit initializer" in str(exc.value)
    assert "member of enum 'Kind'" in str(exc.value)


def test_enum_carry_initializer_must_be_a_member():
    src = r'''requirements CaseReq {
  entity Case
  enum Kind { Statement, Question }
  process Case with kind: Kind = Bogus {
    stages New, Done
    initial New
    transition finish New -> Done by System
  }
}
verify {
  instances Case = 1
}
'''
    with pytest.raises(FslError) as exc:
        parse(src)
    assert exc.value.kind == "type"
    assert "carried enum field 'kind' requires an explicit initializer" in str(exc.value)


def test_unknown_carry_type_error_message_lists_all_three_kinds():
    src = r'''requirements CaseReq {
  entity Case
  process Case with foo: NoSuchType {
    stages New, Done
    initial New
    transition finish New -> Done by System
  }
}
verify {
  instances Case = 1
}
'''
    with pytest.raises(FslError) as exc:
        parse(src)
    assert exc.value.kind == "type"
    assert str(exc.value) == "carried process field must be a number, Bool, or enum type"


def test_number_carry_without_initializer_is_unchanged():
    """Regression guard: the pre-#70 numeric carry (implicit domain `lo` default)."""
    src = r'''requirements CaseReq {
  entity Case
  number Amount
  process Case with amount: Amount {
    stages New, Done
    initial New
    transition finish New -> Done by System
      set amount = amount
  }
}
verify {
  instances Case = 1
  values Amount = 2..5
}
'''
    spec = build_spec(parse(src))
    mon = Monitor(spec)
    state = mon.reset()
    assert state["case_amount"]["0"] == 2


def test_number_carry_with_explicit_const_initializer():
    src = r'''requirements CaseReq {
  entity Case
  number Amount
  const START = 3
  process Case with amount: Amount = START {
    stages New, Done
    initial New
    transition finish New -> Done by System
      set amount = amount
  }
}
verify {
  instances Case = 1
  values Amount = 0..5
}
'''
    spec = build_spec(parse(src))
    mon = Monitor(spec)
    state = mon.reset()
    assert state["case_amount"]["0"] == 3
