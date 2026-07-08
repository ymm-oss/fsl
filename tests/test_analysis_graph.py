from fslc.analysis.graph import (
    connected_components,
    metrics_summary,
    representative_cycle,
    representative_cycles,
    strongly_connected_components,
)
from fslc.cli import run_analyze


def test_connected_components_are_deterministic():
    nodes = ["c", "b", "a", "z"]
    edges = [
        {"from": "b", "to": "a"},
        {"from": "c", "to": "b"},
    ]

    assert connected_components(nodes, edges) == [["a", "b", "c"], ["z"]]
    assert connected_components(nodes, edges) == connected_components(nodes, edges)


def test_strongly_connected_components_and_cycle_witness():
    nodes = ["a", "b", "c", "d"]
    edges = [
        {"from": "a", "to": "b"},
        {"from": "b", "to": "c"},
        {"from": "c", "to": "a"},
        {"from": "c", "to": "d"},
    ]

    assert strongly_connected_components(nodes, edges) == [["a", "b", "c"]]
    assert representative_cycle(nodes, edges) == ["a", "b", "c", "a"]
    assert representative_cycles(nodes, edges) == [["a", "b", "c", "a"]]


def test_acyclic_graph_has_no_cycle():
    nodes = ["a", "b", "c"]
    edges = [{"from": "a", "to": "b"}, {"from": "b", "to": "c"}]

    assert strongly_connected_components(nodes, edges) == []
    assert representative_cycle(nodes, edges) is None


def test_metrics_summary_reports_cycle_rank_and_hubs():
    nodes = ["a", "b", "c"]
    edges = [
        {"from": "a", "to": "b"},
        {"from": "b", "to": "c"},
        {"from": "c", "to": "a"},
        {"from": "a", "to": "c"},
    ]

    metrics = metrics_summary(nodes, edges)

    assert metrics["node_count"] == 3
    assert metrics["edge_count"] == 4
    assert metrics["component_count"] == 1
    assert metrics["scc_count"] == 1
    assert metrics["cycle_rank"] == 4 - 3 + 1
    assert metrics["max_fan_out"]["value"] == 2
    assert metrics["max_fan_out"]["nodes"] == ["a"]
    assert metrics["max_fan_in"]["value"] == 2
    assert metrics["max_fan_in"]["nodes"] == ["c"]


def test_analyze_action_state_graph_projection(tmp_path):
    path = tmp_path / "projection.fsl"
    path.write_text("""
spec Projection {
  state { x: Int }
  init { x = 0 }
  action inc() {
    requires x < 2
    x = x + 1
  }
  invariant Any "MODEL: baseline" { true }
}
""", encoding="utf-8")

    out = run_analyze(str(path), projection="action_state_graph")

    assert out["result"] == "analyzed"
    assert out["projection"] == "action_state_graph"
    assert out["schema_version"] == "analysis-graph.v0"
    assert out["formal_status"] == "not_a_violation"
    assert any(e["kind"] == "writes" for e in out["edges"])
    assert any(e["kind"] == "read_by" for e in out["edges"])
    assert out["metrics"]["node_count"] == len(out["nodes"])
    assert "cycle_rank" in out["metrics"]
    assert out == run_analyze(str(path), projection="action_state_graph")


def test_analyze_action_dependency_graph_projection(tmp_path):
    path = tmp_path / "dependency.fsl"
    path.write_text("""
spec DependencyProjection {
  state { x: Int, y: Int }
  init { x = 0  y = 0 }
  action produce() {
    x = x + 1
  }
  action consume() {
    requires x > 0
    y = y + 1
  }
  action reset() {
    x = x - 1
  }
  invariant Any "MODEL: baseline" { true }
}
""", encoding="utf-8")

    out = run_analyze(str(path), projection="action_dependency_graph")

    assert out["result"] == "analyzed"
    assert out["projection"] == "action_dependency_graph"
    assert {n["id"] for n in out["nodes"]} == {
        "action:consume",
        "action:produce",
        "action:reset",
    }
    assert any(
        e["kind"] == "enables"
        and e["from"] == "action:produce"
        and e["to"] == "action:consume"
        and e["states"] == ["state:x"]
        for e in out["edges"]
    )
    assert any(
        e["kind"] == "conflicts_with"
        and e["from"] == "action:produce"
        and e["to"] == "action:reset"
        and e["states"] == ["state:x"]
        and e["symmetric"] is True
        for e in out["edges"]
    )
    assert out["metrics"]["edge_count"] == len(out["edges"])


def test_analyze_impact_graph_focus_slice(tmp_path):
    path = tmp_path / "impact.fsl"
    path.write_text("""
spec ImpactProjection {
  state { x: Int, y: Int, unrelated: Int }
  init { x = 0  y = 0  unrelated = 0 }
  action produce() {
    x = x + 1
  }
  action consume() "REQ-CONSUME: consume x" {
    requires x > 0
    y = y + 1
  }
  action noop() {
    unrelated = unrelated + 1
  }
  invariant YNonNegative { y >= 0 }
}
""", encoding="utf-8")

    out = run_analyze(str(path), projection="impact_graph", focus="state:x")
    by_id = {n["id"]: n for n in out["nodes"]}

    assert out["result"] == "analyzed"
    assert out["projection"] == "impact_graph"
    assert by_id["state:x"]["direction"] == "focus"
    assert by_id["action:produce"]["direction"] == "upstream"
    assert by_id["guard:consume:0"]["direction"] == "upstream"
    assert "state:unrelated" not in by_id
    assert any(e["from"] == "action:produce" and e["to"] == "state:x" for e in out["edges"])
    assert out == run_analyze(str(path), projection="impact_graph", focus="state:x")


def test_analyze_impact_graph_unknown_focus_uses_name_error(tmp_path):
    path = tmp_path / "impact_error.fsl"
    path.write_text("""
spec ImpactError {
  state { x: Int }
  init { x = 0 }
  action inc() { x = x + 1 }
  invariant Any "MODEL: baseline" { true }
}
""", encoding="utf-8")

    out = run_analyze(str(path), projection="impact_graph", focus="state:missing")

    assert out["result"] == "error"
    assert out["kind"] == "name"
    assert "unknown analyze focus node" in out["message"]
