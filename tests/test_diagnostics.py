# SPDX-License-Identifier: Apache-2.0

from fslc.bmc import verify
from fslc.diagnostics import faithfulness_class_for, with_faithfulness
from fslc.model import build_spec
from fslc.parser import parse


def test_faithfulness_routing_ignores_user_state_payloads():
    state = {
        "kind": {"0": "U"},
        "classification": ["insufficient_depth"],
        "nested": {
            "kind": "partial_op",
            "violation_kind": "partial_op",
            "result": "reachable_failed",
            "covered": False,
            "classification": "over_constrained",
        },
    }
    output = with_faithfulness({
        "result": "verified",
        "trace": [{"step": 0, "state": state}],
        "warnings": [{"kind": "tautology_over_frozen"}],
        "unreached": [{"classification": "insufficient_depth"}],
        "action_coverage": {"blocked": {"covered": False}},
    })

    assert output["trace"][0]["state"] == state
    assert output["warnings"][0]["faithfulness_class"] == "frozen_only_invariant"
    assert output["unreached"][0]["faithfulness_class"] == "intent_unexercised"
    assert output["action_coverage"]["blocked"]["faithfulness_class"] == "intent_unexercised"
    assert faithfulness_class_for({"kind": {"0": "U"}}) is None
    assert faithfulness_class_for({"classification": ["over_constrained"]}) is None


def test_verify_accepts_map_state_named_kind():
    source = """
spec KindCrash {
  type I = 0..1
  enum K { U, R }
  state { kind: Map<I, K>, other: Map<I, K> }
  init { forall i: I { kind[i] = U other[i] = U } }
  action t(i: I) { requires kind[i] == U kind[i] = R }
  invariant Inv { forall i: I { kind[i] == R => other[i] == U } }
  reachable Done { exists i: I { kind[i] == R } }
}
"""

    output = verify(build_spec(parse(source)), 4, deadlock_mode="ignore")

    assert output["result"] == "verified"
    assert output["reachables"]["Done"]["witness"]
