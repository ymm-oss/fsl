# SPDX-License-Identifier: Apache-2.0
"""Compare the stable Phase-3 native tool contracts with Python."""
from __future__ import annotations

import json
import os
import re
import subprocess
import sys
import tempfile
from html.parser import HTMLParser
from pathlib import Path
from typing import Any

ROOT = Path(__file__).resolve().parents[1]
RUST = ROOT / "rust" / "target" / "debug" / "fslc"


class _HtmlShape(HTMLParser):
    def __init__(self) -> None:
        super().__init__(convert_charrefs=False)
        self.tokens: list[tuple[Any, ...]] = []

    def handle_starttag(self, tag: str, attrs: list[tuple[str, str | None]]) -> None:
        self.tokens.append(("start", tag, tuple(attrs)))

    def handle_startendtag(self, tag: str, attrs: list[tuple[str, str | None]]) -> None:
        self.tokens.append(("empty", tag, tuple(attrs)))

    def handle_endtag(self, tag: str) -> None:
        self.tokens.append(("end", tag))


def html_shape(path: Path) -> list[tuple[Any, ...]]:
    parser = _HtmlShape()
    parser.feed(path.read_text())
    return parser.tokens


def html_static_bytes(path: Path) -> bytes:
    content = artifact_bytes("html", path)
    return re.sub(
        rb'(<section class="section" id="(?:traces|witnesses|counterfactuals|raw-data)">).*?(</section>)',
        rb'\1<DYNAMIC>\2',
        content,
        flags=re.DOTALL,
    )


def artifact_bytes(name: str, path: Path) -> bytes:
    content = path.read_bytes()
    if name == "html":
        content = re.sub(rb'(&quot;elapsed_s&quot;: )[0-9.]+', rb'\1<TIMING>', content)
    return content


def normalized_raw(name: str, content: str) -> str:
    if name == "html-stdout":
        content = re.sub(r'(&quot;elapsed_s&quot;: )[0-9.]+', r'\1<TIMING>', content)
    return content


def invoke(executable: list[str], arguments: list[str], *, raw: bool = False) -> tuple[Any, int]:
    environment = os.environ.copy()
    environment["PYTHONPATH"] = str(ROOT / "src") + os.pathsep + environment.get("PYTHONPATH", "")
    process = subprocess.run([*executable, *arguments], cwd=ROOT, env=environment, text=True, stdout=subprocess.PIPE, stderr=subprocess.PIPE, check=False)
    return (process.stdout if raw else json.loads(process.stdout), process.returncode)


def project(name: str, value: dict[str, Any]) -> Any:
    findings = [item.get("kind") for item in value.get("findings", [])]
    if name == "ai-check":
        return {key: value.get(key) for key in ("result", "dialect", "ai_component", "formal_result")} | {"findings": findings}
    if name == "ai-project-check":
        return {key: value.get(key) for key in ("result", "formal_result", "components", "statistical_properties", "observed_properties", "migrations")}
    if name == "ai-replay":
        return {"result": value.get("result"), "events_checked": value.get("events_checked"), "violations": [item.get("violation") for item in value.get("findings", [])]}
    if name in {"ai-eval", "ai-regress", "ai-drift"}:
        return {"result": value.get("result"), "formal_result": value.get("formal_result"), "findings": findings}
    if name == "ai-compare":
        return {"result": value.get("result"), "formal_result": value.get("formal_result"), "metrics": [item["metric"] for item in value.get("comparisons", [])], "delta_signs": [(item.get("delta") or 0) < 0 for item in value.get("comparisons", [])]}
    if name == "ai-compat":
        profiles=value.get("profiles",[])
        return {"result":value.get("result"),"formal_result":value.get("formal_result"),"components":[item.get("component") for item in profiles],"requires":[item.get("requires") for item in profiles],"provides":[item.get("provides") for item in profiles]}
    if name == "compat-check":
        return {"result":value.get("result"),"compat":value.get("compat")}
    if name == "domain-check":
        return {"result": value.get("result"), "formal_result": value.get("formal_result"), "findings": findings, "generated_actions": sorted(value.get("generated_actions", []))}
    if name == "domain-analyze":
        return {"result": value.get("result"), "aggregates": [item["name"] for item in value.get("aggregates", [])], "effects": [item["name"] for item in value.get("effects", [])], "sagas": [item["name"] for item in value.get("sagas", [])], "findings": findings}
    if name == "domain-replay":
        return {"result": value.get("result"), "events_observed": value.get("events_observed"), "findings": findings}
    if name == "explain":
        return value
    if name == "typestate":
        return value
    if name == "mutate":
        return value
    if name in {"testgen", "html", "ledger"}:
        return {key: value.get(key) for key in ("result", "kind", "spec", "target") if key in value}
    if name == "analyze":
        return value
    raise AssertionError(name)


def run(binary: Path = RUST) -> dict[str, Any]:
    failures: list[dict[str, Any]] = []
    cases = [
        ("ai-check", ["ai", "check", "examples/ai/refund_agent_tool_safety.fsl", "--depth", "3"]),
        ("ai-project-check", ["ai", "check", "examples/ai/support_answer_quality.fsl"]),
        ("ai-replay", ["ai", "replay", "examples/ai/refund_agent_tool_safety.fsl", "--logs", "examples/ai/runtime_forbidden_tool.jsonl"]),
        ("ai-eval", ["ai","eval","examples/ai/support_answer_quality.fsl","--records","examples/ai/support_eval_v3.jsonl","--dataset","SupportEvalV3","--property","LooseQuality"]),
        ("ai-regress", ["ai","regress","examples/ai/support_answer_quality.fsl","--before-records","examples/ai/support_eval_v7.jsonl","--after-records","examples/ai/support_eval_v8_regressed.jsonl","--dataset","SupportEvalV3","--migration","PromptV7ToV8"]),
        ("ai-compare", ["ai","compare","--from","examples/ai/support_eval_v7.jsonl","--to","examples/ai/support_eval_v8_regressed.jsonl","--dataset","SupportEvalV3"]),
        ("ai-drift", ["ai","drift","examples/ai/support_answer_quality.fsl","--logs","examples/ai/runtime_drift_current.jsonl","--baseline-logs","examples/ai/runtime_drift_baseline.jsonl","--property","SupportAgentOperationalQuality"]),
        ("ai-compat", ["ai","compat","examples/ai/support_answer_quality.fsl","--environment","prod"]),
        ("compat-check", ["compat","check","examples/db/safe_ai_artifact_compat.fsl","--include-ai"]),
        ("domain-check", ["domain", "check", "examples/domain/order_async_effect.fsl", "--depth", "3"]),
        ("domain-analyze", ["domain", "analyze", "examples/domain/order_async_effect.fsl"]),
        ("domain-replay", ["domain", "replay", "examples/domain/order_async_effect.fsl", "--logs", "examples/domain/order_async_effect_replay.jsonl"]),
        ("explain", ["explain", "specs/cart_v1.fsl", "--depth", "3"]),
        ("typestate", ["typestate", "specs/order_workflow.fsl"]),
        ("mutate", ["mutate", "specs/cart_v1.fsl", "--depth", "4", "--max-mutants", "3"]),
        ("analyze", ["analyze", "specs/cart_v1.fsl", "--projection", "tsg"]),
    ]
    with tempfile.TemporaryDirectory() as directory:
        for name, arguments in cases:
            python, python_status = invoke([sys.executable, "-m", "fslc"], arguments)
            rust, rust_status = invoke([str(binary)], arguments)
            left, right = project(name, python), project(name, rust)
            if left != right or (name != "mutate" and python_status != rust_status):
                failures.append({"case": name, "python": left, "rust": right, "exit_codes": [python_status, rust_status]})
        for name, command_name in (("testgen", "testgen"), ("html", "html"), ("ledger", "ledger")):
            py_out = Path(directory) / f"py-{name}"
            rs_out = Path(directory) / f"rs-{name}"
            arguments = [command_name, "specs/cart_v1.fsl", "--depth", "3", "-o"]
            python, python_status = invoke([sys.executable, "-m", "fslc"], [*arguments, str(py_out)])
            rust, rust_status = invoke([str(binary)], [*arguments, str(rs_out)])
            left, right = project(name, python), project(name, rust)
            if left != right or python_status != rust_status:
                failures.append({"case": name, "python": left, "rust": right, "exit_codes": [python_status, rust_status]})
            elif artifact_bytes(name, py_out) != artifact_bytes(name, rs_out):
                failures.append({"case": f"{name}:content", "python": "different bytes", "rust": "different bytes"})
        for target in ("vitest", "swift", "kotlin", "dart", "phpunit"):
            py_out = Path(directory) / f"py-testgen-{target}"
            rs_out = Path(directory) / f"rs-testgen-{target}"
            arguments = ["testgen", "specs/cart_v1.fsl", "--depth", "3", "--target", target, "-o"]
            python, python_status = invoke([sys.executable, "-m", "fslc"], [*arguments, str(py_out)])
            rust, rust_status = invoke([str(binary)], [*arguments, str(rs_out)])
            left, right = project("testgen", python), project("testgen", rust)
            if left != right or python_status != rust_status:
                failures.append({"case": f"testgen:{target}", "python": left, "rust": right, "exit_codes": [python_status, rust_status]})
            elif py_out.read_bytes() != rs_out.read_bytes():
                failures.append({"case": f"testgen:{target}:content", "python": "different bytes", "rust": "different bytes"})
        forbidden_spec = "examples/gallery/valid/small_forbidden_guarded_cancel.fsl"
        for target in ("vitest", "swift", "kotlin", "dart", "phpunit"):
            py_out = Path(directory) / f"py-testgen-forbidden-{target}"
            rs_out = Path(directory) / f"rs-testgen-forbidden-{target}"
            arguments = ["testgen", forbidden_spec, "--depth", "3", "--target", target, "-o"]
            python, python_status = invoke([sys.executable, "-m", "fslc"], [*arguments, str(py_out)])
            rust, rust_status = invoke([str(binary)], [*arguments, str(rs_out)])
            left, right = project("testgen", python), project("testgen", rust)
            if left != right or python_status != rust_status:
                failures.append({"case": f"testgen:forbidden:{target}", "python": left, "rust": right, "exit_codes": [python_status, rust_status]})
            elif py_out.read_bytes() != rs_out.read_bytes():
                failures.append({"case": f"testgen:forbidden:{target}:content", "python": "different bytes", "rust": "different bytes"})
        broad_html_specs = [
            "specs/order_workflow.fsl",
            "specs/inventory_reservation.fsl",
            "examples/gallery/valid/small_vending_machine.fsl",
            "examples/gallery/valid/small_forbidden_guarded_cancel.fsl",
        ]
        for spec in broad_html_specs:
            stem = Path(spec).stem
            py_out = Path(directory) / f"py-html-{stem}"
            rs_out = Path(directory) / f"rs-html-{stem}"
            arguments = ["html", spec, "--depth", "3", "-o"]
            python, python_status = invoke([sys.executable, "-m", "fslc"], [*arguments, str(py_out)])
            rust, rust_status = invoke([str(binary)], [*arguments, str(rs_out)])
            if project("html", python) != project("html", rust) or python_status != rust_status:
                failures.append({"case": f"html:broad:{stem}", "python": project("html", python), "rust": project("html", rust), "exit_codes": [python_status, rust_status]})
            elif html_shape(py_out) != html_shape(rs_out):
                failures.append({"case": f"html:broad:{stem}:shape", "python": "different HTML structure", "rust": "different HTML structure"})
            elif html_static_bytes(py_out) != html_static_bytes(rs_out):
                failures.append({"case": f"html:broad:{stem}:static-content", "python": "different static bytes", "rust": "different static bytes"})
    db_cases = sorted((ROOT / "examples" / "db").glob("*.fsl"))
    typestate_specs = [
        "specs/inventory_reservation.fsl",
        "specs/cart_v1.fsl",
        "specs/job_pipeline.fsl",
        "specs/rate_limiter.fsl",
    ]
    for spec in typestate_specs:
        arguments = ["typestate", spec]
        python, python_status = invoke([sys.executable, "-m", "fslc"], arguments)
        rust, rust_status = invoke([str(binary)], arguments)
        if python != rust or python_status != rust_status:
            failures.append({"case": f"typestate:{Path(spec).name}", "python": python, "rust": rust, "exit_codes": [python_status, rust_status]})
    analysis_specs = [
        "specs/order_workflow.fsl",
        "specs/inventory_reservation.fsl",
        "specs/job_pipeline.fsl",
        "specs/rate_limiter.fsl",
    ]
    for spec in analysis_specs:
        arguments = ["analyze", spec, "--projection", "tsg"]
        python, python_status = invoke([sys.executable, "-m", "fslc"], arguments)
        rust, rust_status = invoke([str(binary)], arguments)
        if python != rust or python_status != rust_status:
            failures.append({"case": f"analyze-tsg:{Path(spec).name}", "python": python, "rust": rust, "exit_codes": [python_status, rust_status]})
    analysis_projections = [
        ("action_state_graph", None),
        ("action_dependency_graph", None),
        ("requirement_property_graph", None),
        ("property_state_graph", None),
        ("impact_graph", "state:stock"),
    ]
    for projection, focus in analysis_projections:
        arguments = ["analyze", "specs/cart_v1.fsl", "--projection", projection]
        if focus:
            arguments.extend(["--focus", focus])
        python, python_status = invoke([sys.executable, "-m", "fslc"], arguments)
        rust, rust_status = invoke([str(binary)], arguments)
        if python != rust or python_status != rust_status:
            failures.append({"case": f"analyze:{projection}", "python": python, "rust": rust, "exit_codes": [python_status, rust_status]})
    analysis_exports = [
        ("tsg", "dot", None),
        ("tsg", "mermaid", None),
        ("action_state_graph", "dot", None),
        ("impact_graph", "mermaid", "state:stock"),
    ]
    for projection, output_format, focus in analysis_exports:
        arguments = ["analyze", "specs/cart_v1.fsl", "--projection", projection, "--format", output_format]
        if focus:
            arguments.extend(["--focus", focus])
        python, python_status = invoke([sys.executable, "-m", "fslc"], arguments, raw=True)
        rust, rust_status = invoke([str(binary)], arguments, raw=True)
        if python != rust or python_status != rust_status:
            failures.append({"case": f"analyze:{projection}:{output_format}", "python": "different bytes", "rust": "different bytes", "exit_codes": [python_status, rust_status]})
    mutation_cases = [
        ("cart-full", ["mutate", "specs/cart_v1.fsl", "--depth", "4", "--max-mutants", "200"]),
        ("vending-full", ["mutate", "examples/gallery/valid/small_vending_machine.fsl", "--depth", "4", "--max-mutants", "100"]),
        ("workflow-full", ["mutate", "specs/order_workflow.fsl", "--depth", "3", "--max-mutants", "100"]),
        ("forbidden-full", ["mutate", "examples/gallery/valid/small_forbidden_guarded_cancel.fsl", "--depth", "3", "--max-mutants", "100", "--by-requirement"]),
        ("requirements-profile", ["mutate", "examples/e2e/2_requirements.fsl", "--depth", "2", "--max-mutants", "100", "--by-requirement"]),
        ("implements-cancel", ["mutate", "examples/pm/cancel_system.fsl", "--depth", "3", "--max-mutants", "100", "--by-requirement"]),
        ("implements-return", ["mutate", "examples/layers/return_system.fsl", "--depth", "3", "--max-mutants", "100", "--by-requirement"]),
        ("external", ["mutate", "tests/fixtures/rust_port/external_guarded.fsl", "--depth", "3", "--max-mutants", "0", "--from", "tests/fixtures/rust_port/external_mutants.jsonl"]),
        ("external-invalid", ["mutate", "tests/fixtures/rust_port/external_guarded.fsl", "--depth", "2", "--max-mutants", "0", "--from", "tests/fixtures/rust_port/external_mutants_invalid.jsonl"]),
        ("external-forbidden", ["mutate", "examples/gallery/valid/small_forbidden_guarded_cancel.fsl", "--depth", "3", "--max-mutants", "0", "--by-requirement", "--from", "tests/fixtures/rust_port/external_forbidden_mutants.jsonl"]),
    ]
    for name, arguments in mutation_cases:
        python, python_status = invoke([sys.executable, "-m", "fslc"], arguments)
        rust, rust_status = invoke([str(binary)], arguments)
        if python != rust or python_status != rust_status:
            failures.append({"case": f"mutate:{name}", "python": python, "rust": rust, "exit_codes": [python_status, rust_status]})
    refinement_analysis_cases = [
        ("json", ["analyze", "examples/e2e/3_refines_2.fsl", "--projection", "refinement_graph"], False),
        ("dot", ["analyze", "examples/e2e/3_refines_2.fsl", "--format", "dot"], True),
    ]
    for name, arguments, raw in refinement_analysis_cases:
        python, python_status = invoke([sys.executable, "-m", "fslc"], arguments, raw=raw)
        rust, rust_status = invoke([str(binary)], arguments, raw=raw)
        if python != rust or python_status != rust_status:
            failures.append({"case": f"analyze:refinement:{name}", "python": python if not raw else "different bytes", "rust": rust if not raw else "different bytes", "exit_codes": [python_status, rust_status]})
    tag_review_arguments = ["analyze", "tests/fixtures/rust_port/tag_review.fsl", "--export", "tag-review"]
    python, python_status = invoke([sys.executable, "-m", "fslc"], tag_review_arguments)
    rust, rust_status = invoke([str(binary)], tag_review_arguments)
    if python != rust or python_status != rust_status:
        failures.append({"case": "analyze:tag-review", "python": python, "rust": rust, "exit_codes": [python_status, rust_status]})
    batch_arguments = ["analyze", "specs/cart_v1.fsl", "examples/e2e/3_refines_2.fsl"]
    python, python_status = invoke([sys.executable, "-m", "fslc"], batch_arguments)
    rust, rust_status = invoke([str(binary)], batch_arguments)
    if python != rust or python_status != rust_status:
        failures.append({"case": "analyze:batch", "python": python, "rust": rust, "exit_codes": [python_status, rust_status]})
    project_analysis_cases = [
        "tests/fixtures/chain/fsl-project.toml",
        "tests/fixtures/rust_port/project_gap/fsl-project.toml",
    ]
    for manifest in project_analysis_cases:
        arguments = ["analyze", manifest, "--projection", "traceability_graph"]
        python, python_status = invoke([sys.executable, "-m", "fslc"], arguments)
        rust, rust_status = invoke([str(binary)], arguments)
        if python != rust or python_status != rust_status:
            failures.append({"case": f"analyze:project:{Path(manifest).parent.name}", "python": python, "rust": rust, "exit_codes": [python_status, rust_status]})
    ai_review_cases = [
        "specs/cart_v1.fsl",
        "specs/order_workflow.fsl",
        "tests/fixtures/rust_port/tag_review.fsl",
        "tests/fixtures/rust_port/ai_review_structural.fsl",
        "tests/fixtures/rust_port/ai_review_retry_loop.fsl",
        "tests/fixtures/rust_port/ai_review_retry_progress.fsl",
        "tests/fixtures/rust_port/ai_review_conservation.fsl",
        "tests/fixtures/rust_port/ai_review_divergent.fsl",
    ]
    for spec in ai_review_cases:
        arguments = ["analyze", spec, "--profile", "ai-review"]
        python, python_status = invoke([sys.executable, "-m", "fslc"], arguments)
        rust, rust_status = invoke([str(binary)], arguments)
        if python != rust or python_status != rust_status:
            failures.append({"case": f"analyze:ai-review:{Path(spec).name}", "python": python, "rust": rust, "exit_codes": [python_status, rust_status]})
    ai_batch_arguments = ["analyze", "tests/fixtures/rust_port/tag_review.fsl", "tests/fixtures/rust_port/ai_review_structural.fsl", "--profile", "ai-review"]
    python, python_status = invoke([sys.executable, "-m", "fslc"], ai_batch_arguments)
    rust, rust_status = invoke([str(binary)], ai_batch_arguments)
    if python != rust or python_status != rust_status:
        failures.append({"case": "analyze:ai-review:batch", "python": python, "rust": rust, "exit_codes": [python_status, rust_status]})
    for path in db_cases:
        arguments = ["db", "check", str(path.relative_to(ROOT)), "--depth", "3"]
        python, _ = invoke([sys.executable, "-m", "fslc"], arguments)
        rust, _ = invoke([str(binary)], arguments)
        left=(python.get("result"),[item["kind"] for item in python.get("findings",[])])
        right=(rust.get("result"),[item["kind"] for item in rust.get("findings",[])])
        if left != right: failures.append({"case":f"db:{path.name}","python":left,"rust":right})
    for source in ("minimal_import.sql", "minimal_prisma_schema.prisma"):
        args=["db","import",f"examples/db/{source}","--name","ImportedDb"]
        python,_=invoke([sys.executable,"-m","fslc"],args,raw=True);rust,_=invoke([str(binary)],args,raw=True)
        if python!=rust: failures.append({"case":f"db-import:{source}","python":"different bytes","rust":"different bytes"})
    raw_cases = [
        ("version", ["--version"]),
        ("typestate-ts", ["typestate", "specs/order_workflow.fsl", "--ts"]),
        ("testgen-stdout", ["testgen", "specs/cart_v1.fsl", "--depth", "3"]),
        ("html-stdout", ["html", "specs/cart_v1.fsl", "--depth", "3"]),
        ("ledger-stdout", ["ledger", "specs/cart_v1.fsl", "--depth", "3"]),
        ("explain-readable", ["explain", "specs/cart_v1.fsl", "--depth", "3", "--readable"]),
        ("domain-expand", ["domain", "expand", "examples/domain/order_async_effect.fsl"]),
        ("domain-testgen", ["domain", "testgen", "examples/domain/order_async_effect.fsl", "--depth", "3"]),
    ]
    for name, arguments in raw_cases:
        python, python_status = invoke([sys.executable, "-m", "fslc"], arguments, raw=True)
        rust, rust_status = invoke([str(binary)], arguments, raw=True)
        if normalized_raw(name, python) != normalized_raw(name, rust) or python_status != rust_status:
            failures.append({"case":name,"python":"different bytes","rust":"different bytes","exit_codes":[python_status,rust_status]})
    total=len(cases)+3+5+5+4+len(db_cases)+2+len(typestate_specs)+len(analysis_specs)+len(analysis_projections)+len(analysis_exports)+len(mutation_cases)+len(refinement_analysis_cases)+3+len(project_analysis_cases)+len(ai_review_cases)+len(raw_cases)
    return {"schema":"fsl-rust-phase3-command-parity.v1","cases":total,"matched":total-len(failures),"failures":failures}


if __name__ == "__main__":
    result=run();print(json.dumps(result,indent=2,ensure_ascii=False,sort_keys=True));raise SystemExit(bool(result["failures"]))
