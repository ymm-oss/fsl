from pathlib import Path

from fslc.cli import run_analyze


ROOT = Path(__file__).resolve().parent
CHAIN = ROOT / "fixtures" / "chain" / "fsl-project.toml"


def test_analyze_project_traceability_graph_reads_chain_manifest():
    out = run_analyze(str(CHAIN), projection="traceability_graph")

    assert out["result"] == "analyzed"
    assert out["projection"] == "traceability_graph"
    assert out["schema_version"] == "analysis-graph.v0"
    assert out["formal_status"] == "not_a_violation"
    assert any(n["kind"] == "requirements_spec" for n in out["nodes"])
    assert any(n["kind"] == "design_spec" for n in out["nodes"])
    assert any(e["kind"] == "maps_action" for e in out["edges"])
    assert any(
        e["kind"] == "lower_anchor"
        and e["from"] == "requirements:requirement:REQ-1"
        and e["to"] == "design:action:finish"
        for e in out["edges"]
    )
    assert out["cycles"] == []
    assert not any(f["involved_nodes"] == ["requirements:requirement:REQ-1"] for f in out["findings"])
    assert out == run_analyze(str(CHAIN), projection="traceability_graph")


def test_analyze_project_traceability_graph_accepts_named_toml_manifest():
    manifest = CHAIN.with_name("fsl-project-broken.toml")

    out = run_analyze(str(manifest), projection="traceability_graph")

    assert out["result"] == "analyzed"
    assert out["projection"] == "traceability_graph"
    assert out["manifest"].endswith("fsl-project-broken.toml")


def test_analyze_project_traceability_graph_reports_upper_id_without_lower_anchor(tmp_path):
    (tmp_path / "requirements.fsl").write_text("""
requirements ReqLayer {
  type Item = 0..0
  state { x: Map<Item, Bool> }
  init { forall i: Item { x[i] = false } }
  requirement REQ-LOST "must finish" {
    action finish(i: Item) {
      requires not x[i]
      x[i] = true
    }
  }
}
""", encoding="utf-8")
    (tmp_path / "design.fsl").write_text("""
spec DesignLayer {
  type Item = 0..0
  state { y: Map<Item, Bool> }
  init { forall i: Item { y[i] = false } }
  action noop(i: Item) {
    y[i] = y[i]
  }
}
""", encoding="utf-8")
    (tmp_path / "map.fsl").write_text("""
refinement DesignRefinesReq {
  impl DesignLayer
  abs ReqLayer
  map x[i: Item] = y[i]
  action noop(i) -> stutter
}
""", encoding="utf-8")
    manifest = tmp_path / "fsl-project.toml"
    manifest.write_text("""
[requirements]
file = "requirements.fsl"

[design]
file = "design.fsl"
refine_against = "requirements"
mapping = "map.fsl"
""", encoding="utf-8")

    out = run_analyze(str(manifest), projection="traceability_graph")

    assert out["result"] == "analyzed"
    gaps = [f for f in out["findings"] if f["finding_type"] == "traceability_gap"]
    assert any("requirements:requirement:REQ-LOST" in f["involved_nodes"] for f in gaps)
    assert all(f["formal_status"] == "not_a_violation" for f in gaps)
