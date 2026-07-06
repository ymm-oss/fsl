from fslc.analysis.graph import (
    connected_components,
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
    assert out == run_analyze(str(path), projection="action_state_graph")
