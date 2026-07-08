import json
from pathlib import Path

from fslc.cli import run_analyze


ROOT = Path(__file__).resolve().parent.parent
SCHEMAS = ROOT / "schemas" / "fslc" / "analysis"


def _load_schema(name):
    return json.loads((SCHEMAS / name).read_text(encoding="utf-8"))


def _assert_required(schema, payload):
    assert set(schema["required"]) <= set(payload)
    defs = schema.get("$defs", {})
    if "nodes" in payload and "node" in defs:
        required = set(defs["node"]["required"])
        assert all(required <= set(item) for item in payload["nodes"])
    if "edges" in payload and "edge" in defs:
        required = set(defs["edge"]["required"])
        assert all(required <= set(item) for item in payload["edges"])
    if "findings" in payload and "finding" in defs:
        required = set(defs["finding"]["required"])
        assert all(required <= set(item) for item in payload["findings"])


def test_analysis_schema_files_exist_and_have_stable_ids():
    for filename, version in [
        ("tsg.v0.schema.json", "tsg.v0"),
        ("analysis-graph.v0.schema.json", "analysis-graph.v0"),
        ("analysis-findings.v0.schema.json", "analysis-findings.v0"),
    ]:
        schema = _load_schema(filename)
        assert schema["$id"].endswith(filename)
        assert version in json.dumps(schema)


def test_representative_analyze_outputs_match_schema_required_contracts(tmp_path):
    path = tmp_path / "schema_case.fsl"
    path.write_text("""
spec SchemaCase {
  state { x: Int }
  init { x = 0 }
  action inc() {
    requires x < 2
    x = x + 1
  }
  invariant Any "MODEL: baseline" { true }
}
""", encoding="utf-8")

    tsg = run_analyze(str(path), projection="tsg")
    graph = run_analyze(str(path), projection="action_state_graph")
    findings = run_analyze(str(path), profile="ai-review")

    _assert_required(_load_schema("tsg.v0.schema.json"), tsg)
    _assert_required(_load_schema("analysis-graph.v0.schema.json"), graph)
    _assert_required(_load_schema("analysis-findings.v0.schema.json"), findings)
    assert set(_load_schema("analysis-graph.v0.schema.json")["$defs"]["metrics"]["required"]) <= set(graph["metrics"])
