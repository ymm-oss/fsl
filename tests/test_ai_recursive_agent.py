# SPDX-License-Identifier: Apache-2.0
# Copyright 2026 Ryoichi Izumita

"""fsl-ai recursive agent composition coverage."""

from pathlib import Path

from fslc.cli import run_ai_check, run_check


ROOT = Path(__file__).resolve().parents[1]
EXAMPLES = ROOT / "examples" / "ai"


def _write(tmp_path, name, source):
    path = tmp_path / name
    path.write_text(source, encoding="utf-8")
    return str(path)


def _violations(out):
    return {finding["violation"] for finding in out["findings"]}


def test_recursive_agent_example_produces_deterministic_ir():
    out = run_ai_check(str(EXAMPLES / "recursive_support_agent.fsl"))

    assert out["result"] == "agent_analyzed"
    assert out["dialect"] == "fsl-ai-agent.v0"
    assert out["formal_result"] == "not_run"
    assert out["findings"] == []

    ir = out["agent_ir"]
    assert ir["path"] == "SupportOrchestrator"
    assert [child["path"] for child in ir["children"]] == [
        "SupportOrchestrator.RetrievalAgent",
        "SupportOrchestrator.PolicyCheckAgent",
        "SupportOrchestrator.DraftAnswerAgent",
        "SupportOrchestrator.SendAgent",
    ]
    assert out["graph_summary"]["delegation_graph"] == [
        {
            "parent": "SupportOrchestrator",
            "source": "SupportOrchestrator.RetrievalAgent",
            "target": "SupportOrchestrator.PolicyCheckAgent",
        },
        {
            "parent": "SupportOrchestrator",
            "source": "SupportOrchestrator.PolicyCheckAgent",
            "target": "SupportOrchestrator.DraftAnswerAgent",
        },
        {
            "parent": "SupportOrchestrator",
            "source": "SupportOrchestrator.DraftAnswerAgent",
            "target": "SupportOrchestrator.SendAgent",
        },
    ]
    assert out["graph_summary"]["failure_policy"][0] == {
        "source": "SupportOrchestrator.RetrievalAgent",
        "condition": "failed",
        "action": "retry",
        "target": None,
        "retry_limit": 2,
    }


def test_recursive_agent_is_parseable_by_regular_check_for_corpus_sweeps():
    out = run_check(str(EXAMPLES / "recursive_support_agent.fsl"))

    assert out["result"] == "ok"
    assert out["spec"] == "SupportOrchestrator"
    assert out["dialect"] == "fsl-ai-agent.v0"
    assert out["agent_analysis_result"] == "agent_analyzed"


def test_invalid_authority_grant_rejects_child_outside_parent_boundary(tmp_path):
    path = _write(tmp_path, "bad_grant.fsl", """
agent Parent {
  context [Ticket];
  tools [SearchDocs];

  agent Child {
    grant authority [RefundPayment];
    grant context [Ticket];
  }
}
""")

    out = run_ai_check(path)

    assert out["result"] == "error"
    assert out["kind"] == "semantics"
    assert "grant authority exceeds parent boundary" in out["message"]
    assert out["loc"] == {"line": 7, "column": 5}


def test_visibility_leak_between_siblings_requires_delegation_path(tmp_path):
    path = _write(tmp_path, "visibility_leak.fsl", """
agent Parent {
  context [Ticket];
  tools [Draft, CheckPolicy];
  authority { may_execute [Draft, CheckPolicy]; }

  agent Draft {
    grant authority [Draft];
    grant context [Ticket];
    tools [Draft];
    authority { may_execute [Draft]; }
    output DraftOut visibility Policy;
  }

  agent Policy {
    grant authority [CheckPolicy];
    grant context [Ticket];
    tools [CheckPolicy];
    authority { may_execute [CheckPolicy]; }
  }
}
""")

    out = run_ai_check(path)

    assert out["result"] == "violated"
    assert _violations(out) == {"visibility_leak_across_sibling_agents"}
    finding = out["findings"][0]
    assert finding["guarantee_kind"] == "agent_structural"
    assert finding["evidence"] == {"kind": "static_agent_graph", "formal_proof": False}


def test_child_authority_use_outside_grant_is_structural_finding(tmp_path):
    path = _write(tmp_path, "authority_exceeds_grant.fsl", """
agent Parent {
  context [Ticket];
  tools [SearchDocs, RefundPayment];
  authority { may_execute [SearchDocs, RefundPayment]; }

  agent Child {
    grant authority [SearchDocs];
    grant context [Ticket];
    tools [RefundPayment];
    authority { may_execute [RefundPayment]; }
  }
}
""")

    out = run_ai_check(path)

    assert out["result"] == "violated"
    assert _violations(out) == {"child_authority_exceeds_parent_authority"}
    finding = out["findings"][0]
    assert finding["minimal_conflict_set"]["exceeded_authority"] == ["RefundPayment"]


def test_agent_graph_flags_tool_reachability_and_irreversible_approval(tmp_path):
    path = _write(tmp_path, "unsafe_graph.fsl", """
agent Parent {
  context [Ticket];
  tools [Draft, RefundPayment];
  authority {
    may_execute [Draft];
    requires_human_approval [RefundPayment];
  }

  agent Untrusted {
    trust low;
    grant authority [Draft];
    grant context [Ticket];
    tools [Draft];
    authority { may_execute [Draft]; }
    output DraftOut visibility Payment;
  }

  agent Payment {
    grant authority [RefundPayment];
    grant context [Ticket];
    tool RefundPayment irreversible {
      schema RefundPaymentV1;
    }
    authority { may_execute [RefundPayment]; }
  }

  orchestration {
    Untrusted -> Payment;
  }
}
""")

    out = run_ai_check(path)

    assert out["result"] == "violated"
    assert _violations(out) == {
        "irreversible_operation_without_human_approval_path",
        "low_trust_agent_path_to_high_authority_tool",
    }


def test_policy_review_bypass_in_orchestration_is_reported(tmp_path):
    path = _write(tmp_path, "policy_bypass.fsl", """
agent Parent {
  context [Ticket];
  tools [Draft, CheckPolicy, RefundPayment];
  authority {
    may_execute [Draft, CheckPolicy];
    requires_human_approval [RefundPayment];
  }

  agent Draft {
    grant authority [Draft];
    grant context [Ticket];
    tools [Draft];
    authority { may_execute [Draft]; }
  }

  agent Policy {
    grant authority [CheckPolicy];
    grant context [Ticket];
    tools [CheckPolicy];
    authority { may_execute [CheckPolicy]; }
  }

  agent Payment {
    grant authority [RefundPayment];
    grant context [Ticket];
    tool RefundPayment irreversible {
      schema RefundPaymentV1;
    }
    authority { requires_human_approval [RefundPayment]; }
  }

  review_gate Policy;
  orchestration {
    Draft -> Payment;
  }
}
""")

    out = run_ai_check(path)

    assert out["result"] == "violated"
    assert _violations(out) == {"policy_review_bypass_in_orchestration"}
