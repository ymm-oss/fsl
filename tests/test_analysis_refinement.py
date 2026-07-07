from pathlib import Path

from fslc.cli import run_analyze


SPECS = Path(__file__).resolve().parent.parent / "specs"


def _ids(out, kind=None):
    return {
        n["id"]
        for n in out.get("nodes", [])
        if kind is None or n["kind"] == kind
    }


def test_analyze_refinement_mapping_file_emits_mapping_graph():
    out = run_analyze(str(SPECS / "cart_refines.fsl"))

    assert out["result"] == "analyzed"
    assert out["projection"] == "refinement_graph"
    assert out["schema_version"] == "analysis-graph.v0"
    assert out["formal_status"] == "not_a_violation"
    assert "refinement:CartImplRefinesCart" in _ids(out, "refinement")
    assert "impl_spec:CartImpl" in _ids(out, "impl_spec")
    assert "abs_spec:ShoppingCart" in _ids(out, "abs_spec")
    assert "state_map:stock:i" in _ids(out, "state_map")
    assert "action_map:reserve" in _ids(out, "action_map")
    assert any(e["kind"] == "stutters" for e in out["edges"])
    assert any(e["kind"] == "reads_impl_state" and e["to"] == "map_expr_read:impl_stock" for e in out["edges"])


def test_analyze_refinement_graph_projection_is_explicit(tmp_path):
    path = tmp_path / "mapping.fsl"
    path.write_text("""
refinement ImplRefinesAbs {
  impl Impl
  abs Abs
  maps auto
  map x = y
  action step() -> step()
  preserve progress {
    respond EventuallyDone by step
  }
}
""", encoding="utf-8")

    out = run_analyze(str(path), projection="refinement_graph")

    assert out["result"] == "analyzed"
    assert "maps_auto:ImplRefinesAbs" in _ids(out, "maps_auto")
    assert "preserve_progress:ImplRefinesAbs" in _ids(out, "preserve_progress")
    assert "progress_response:EventuallyDone" in _ids(out, "progress_response")
