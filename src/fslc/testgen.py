"""Generate pytest conformance test stubs from FSL specs."""
from __future__ import annotations

import json
import os
import re
import textwrap
from pathlib import Path

from .parser import parse_src
from .model import build_spec
from .bmc import scenarios


def _py_literal(obj):
    return repr(obj)


def _scenario_function_name(display_name, seen_names):
    safe_name = re.sub(r"[^A-Za-z0-9_]+", "_", display_name).strip("_")
    count = seen_names.get(safe_name, 0) + 1
    seen_names[safe_name] = count
    if count > 1:
        safe_name = f"{safe_name}_{count}"
    return f"test_scenario_{safe_name}"


def _spec_path_line(spec_path, output_path=None):
    if output_path is None:
        return f"SPEC_PATH = {str(Path(spec_path).resolve())!r}"
    relative = os.path.relpath(Path(spec_path).resolve(), Path(output_path).resolve().parent)
    return f"SPEC_PATH = Path(__file__).resolve().parent / {str(relative)!r}"


def generate_test_file(spec_path, depth=8, deadlock_mode="warn", output_path=None):
    path = Path(spec_path)
    src = path.read_text(encoding="utf-8")
    ast, display_names = parse_src(src, str(path.parent))
    spec = build_spec(ast, display_names)
    sc = scenarios(spec, depth, deadlock_mode=deadlock_mode, source_lines=src.splitlines())
    if sc.get("result") != "scenarios":
        raise RuntimeError(f"cannot generate tests: scenarios returned {sc.get('result')}")

    spec_name = spec["name"]
    module_name = spec_name[0].lower() + spec_name[1:] if spec_name else "spec"
    scenario_data = sc.get("scenarios", [])

    lines = [
        '"""Auto-generated conformance tests for FSL spec.',
        f"Source: {Path(spec_path).name}",
        'Connect Adapter to your implementation, or use MonitorSelfAdapter for self-check.',
        '"""',
        "import random",
        "from pathlib import Path",
        "",
        "import pytest",
        "",
        "from fslc.runtime import Monitor",
        "",
        _spec_path_line(spec_path, output_path),
        "",
        "",
        "class Adapter:",
        '    """Connect your implementation to the spec actions/state.',
        "",
        "    Wiring convention:",
        "    - reset(): put implementation in the same initial state as spec init",
        "    - step(action, params): drive one spec action on the implementation",
        "    - observe(): return implementation state projected to spec logical state shape",
        '    """',
        "",
        "    def reset(self):",
        '        raise NotImplementedError("wire your implementation reset")',
        "",
        "    def step(self, action: str, params: dict):",
        '        raise NotImplementedError("wire your implementation step")',
        "",
        "    def observe(self) -> dict:",
        '        raise NotImplementedError("wire your implementation observe")',
        "",
        "",
        "def _adapter_ready(adapter):",
        "    try:",
        "        adapter.reset()",
        "        adapter.observe()",
        "        return True",
        "    except NotImplementedError:",
        "        return False",
        "",
        "",
        "@pytest.fixture",
        "def adapter():",
        "    return Adapter()",
        "",
        "",
        "def _assert_partial_expected(observed, expected):",
        "    for key, val in expected.items():",
        "        if isinstance(val, dict) and isinstance(observed.get(key), dict):",
        "            _assert_partial_expected(observed[key], val)",
        "        else:",
        "            assert observed[key] == val",
        "",
    ]

    scenario_function_names = {}
    for scen in scenario_data:
        name = scen["name"]
        function_name = _scenario_function_name(name, scenario_function_names)
        lines.extend([
            "",
            f"def {function_name}(adapter):",
            f"    {_py_literal(f'Scenario: {name}')}",
            "    if not _adapter_ready(adapter):",
            f"        pytest.skip('Adapter not implemented')",
            "    adapter.reset()",
        ])
        steps = scen.get("steps", [])
        expected_states = scen.get("expected_states", [])
        for i, step in enumerate(steps):
            lines.append(f"    adapter.step({_py_literal(step['action'])}, {_py_literal(step['params'])})")
            exp = expected_states[i] if i < len(expected_states) else {}
            lines.append(f"    _assert_partial_expected(adapter.observe(), {_py_literal(exp)})")

    lines.extend([
        "",
        "def test_random_walk_conformance(adapter):",
        "    if not _adapter_ready(adapter):",
        "        pytest.skip('Adapter not implemented')",
        "    mon = Monitor(SPEC_PATH)",
        "    mon.reset()",
        "    adapter.reset()",
        "    assert adapter.observe() == mon.state",
        "    rng = random.Random(0)",
        "    for _ in range(100):",
        "        enabled = mon.enabled()",
        "        if not enabled:",
        "            break",
        "        choice = enabled[rng.randrange(len(enabled))]",
        "        action, params = choice['action'], dict(choice['params'])",
        "        adapter.step(action, params)",
        "        result = mon.step(action, params)",
        "        if not result.get('ok'):",
        "            pytest.fail(",
        "                f'spec oracle violation at {action} {params}: '",
        "                f\"{result.get('kind')} {result.get('name', '')}\"",
        "            )",
        "        assert adapter.observe() == mon.state",
        "",
    ])

    return "\n".join(lines) + "\n"


def default_output_name(spec_path):
    spec = build_spec(parse(Path(spec_path).read_text(encoding="utf-8")))
    name = spec["name"]
    return f"test_{name[0].lower()}{name[1:]}.py"
