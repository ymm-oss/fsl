# SPDX-License-Identifier: Apache-2.0
# Copyright 2026 Ryoichi Izumita

"""Generate conformance test scaffolds from FSL specs.

The language-independent core is ``scenarios()`` in :mod:`fslc.bmc`; this module
is the *emitter* layer that renders that JSON into a concrete test harness.
``--target pytest`` (default) and ``--target vitest`` share one scenario-collection
pass (:func:`_collect_scenarios`) and differ only in how they render it.
"""
from __future__ import annotations

import json
import os
import random
import re
from pathlib import Path

from .parser import parse_src
from .model import build_spec
from .bmc import scenarios


class TestgenScenarioError(RuntimeError):
    def __init__(self, scenario_result):
        self.scenario_result = scenario_result
        super().__init__(f"cannot generate tests: scenarios returned {scenario_result.get('result')}")


# --------------------------------------------------------------------------
# shared scenario-collection core (language independent)
# --------------------------------------------------------------------------
def _module_name(spec_name):
    return spec_name[0].lower() + spec_name[1:] if spec_name else "spec"


def _collect_scenarios(spec_path, depth=8, deadlock_mode="warn", strict=False):
    """Parse, build, and run ``scenarios`` once; the result feeds every emitter."""
    path = Path(spec_path)
    src = path.read_text(encoding="utf-8")
    ast, display_names = parse_src(src, str(path.parent))
    spec = build_spec(ast, display_names)
    sc = scenarios(
        spec,
        depth,
        deadlock_mode=deadlock_mode,
        source_lines=src.splitlines(),
        allow_unreached=not strict,
    )
    if sc.get("result") != "scenarios":
        raise TestgenScenarioError(sc)
    spec_name = spec["name"]
    return {
        "spec": spec,
        "spec_name": spec_name,
        "module_name": _module_name(spec_name),
        "scenarios": sc.get("scenarios", []),
        "warnings": sc.get("warnings", []),
        "src": src,
    }


# --------------------------------------------------------------------------
# pytest emitter (output intentionally byte-for-byte stable — see CHANGELOG)
# --------------------------------------------------------------------------
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


def emit_pytest(collected, spec_path, output_path=None):
    spec_name = collected["spec_name"]
    scenario_data = collected["scenarios"]

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
        "",
        "def _assert_rejected(result, expected_kind):",
        "    assert isinstance(result, dict), 'forbidden adapter.step must return a result dict'",
        "    assert result.get('ok') is False",
        "    if expected_kind is not None:",
        "        assert result.get('kind') == expected_kind",
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
        if scen.get("kind") == "forbidden" and scen.get("forbidden_step"):
            forbidden_step = scen["forbidden_step"]
            lines.append(
                f"    result = adapter.step({_py_literal(forbidden_step['action'])}, "
                f"{_py_literal(forbidden_step['params'])})"
            )
            lines.append(f"    _assert_rejected(result, {_py_literal(scen.get('rejected_by'))})")

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


# --------------------------------------------------------------------------
# vitest emitter (TypeScript)
# --------------------------------------------------------------------------
def _ts_inline(obj):
    """Render a JSON-serialisable scenario value as a compact TS/JS literal."""
    return json.dumps(obj, ensure_ascii=False)


def _ts_array(items):
    """Render a list as a TS/JS array literal, one compact element per line."""
    if not items:
        return "[]"
    body = ",\n".join("  " + _ts_inline(item) for item in items)
    return "[\n" + body + ",\n]"


def _bake_random_walk(spec, steps=100, seed=0):
    """Replay the seed-fixed random walk against the Python Monitor (the oracle)
    and bake the ``(action, params, expected_state)`` sequence as a static
    fixture, so the generated Vitest file needs no ``fslc`` at runtime.

    The pytest scaffold runs this same walk live with ``random.Random(0)``; the
    Monitor is deterministic, so baking under the same seed yields the identical
    trace — equal coverage, no live oracle dependency.
    """
    from .runtime import Monitor  # lazy: keep module import graph free of cycles

    mon = Monitor(spec)
    mon.reset()
    walk = {"initial": mon.state, "steps": []}
    rng = random.Random(seed)
    for _ in range(steps):
        enabled = mon.enabled()
        if not enabled:
            break
        choice = enabled[rng.randrange(len(enabled))]
        action, params = choice["action"], dict(choice["params"])
        result = mon.step(action, params)
        if not result.get("ok"):
            # The oracle rejected an action it reported as enabled — bake only the
            # consistent prefix rather than emit a step the spec itself refuses.
            break
        walk["steps"].append({"action": action, "params": params, "expected": mon.state})
    return walk


_VITEST_PREAMBLE = '''\
// SPDX-License-Identifier: Apache-2.0
/**
 * Auto-generated FSL conformance tests (Vitest).
 * Source: __SOURCE__
 *
 * Wire `makeAdapter()` to your implementation. Until it is wired, every test is
 * skipped (mirroring the pytest scaffold's skip-when-unwired behaviour).
 *
 * The random-walk trace below was baked at generation time by the FSL Monitor
 * (the spec's concrete interpreter) under a fixed seed, so these tests need no
 * `fslc`/Python at runtime — they replay the baked oracle states and assert.
 */
import { test, expect } from "vitest";

export interface StepResult {
  ok: boolean;
  kind?: string;
}

/**
 * Connect your implementation to the spec actions/state.
 *  - reset(): put the implementation in the same initial state as spec `init`
 *  - step(action, params): drive one spec action on the implementation
 *  - observe(): project implementation state onto the spec's logical-state shape
 *    (enum = name string, Option = null|value, Seq = array, Map = object with
 *     string keys, struct = object)
 */
export interface Adapter {
  reset(): void;
  step(action: string, params: Record<string, unknown>): StepResult | void;
  observe(): Record<string, unknown>;
}

// Wire your implementation here. Throwing leaves the suite skipped (not failed).
function makeAdapter(): Adapter {
  throw new Error("wire your implementation: implement makeAdapter()");
}

function isPlainObject(v: unknown): v is Record<string, unknown> {
  return v !== null && typeof v === "object" && !Array.isArray(v);
}

// Assert only the fields the spec mentions; recurse into nested (Map/struct) shapes.
function assertPartial(observed: Record<string, unknown>, expected: Record<string, unknown>): void {
  for (const [key, val] of Object.entries(expected)) {
    const seen = observed[key];
    if (isPlainObject(val) && isPlainObject(seen)) {
      assertPartial(seen, val);
    } else {
      expect(seen).toStrictEqual(val);
    }
  }
}

function assertRejected(result: StepResult | void, expectedKind: string | null): void {
  expect(isPlainObject(result), "forbidden step must return a result object").toBe(true);
  const r = result as StepResult;
  expect(r.ok).toBe(false);
  if (expectedKind !== null) {
    expect(r.kind).toBe(expectedKind);
  }
}

let adapter: Adapter;
let wired = false;
try {
  adapter = makeAdapter();
  adapter.reset();
  adapter.observe();
  wired = true;
} catch {
  // Adapter not wired yet — every test below is skipped.
}
const scenario = wired ? test : test.skip;
'''


def _vitest_scenario_block(scen):
    name = scen["name"]
    lines = [
        f"scenario({_ts_inline(f'scenario: {name}')}, () => {{",
        "  adapter.reset();",
    ]
    steps = scen.get("steps", [])
    expected_states = scen.get("expected_states", [])
    for i, step in enumerate(steps):
        lines.append(f"  adapter.step({_ts_inline(step['action'])}, {_ts_inline(step['params'])});")
        exp = expected_states[i] if i < len(expected_states) else {}
        lines.append(f"  assertPartial(adapter.observe(), {_ts_inline(exp)});")
    if scen.get("kind") == "forbidden" and scen.get("forbidden_step"):
        forbidden_step = scen["forbidden_step"]
        rejected_by = scen.get("rejected_by")
        rejected = _ts_inline(rejected_by) if rejected_by is not None else "null"
        lines.append(
            f"  const result = adapter.step({_ts_inline(forbidden_step['action'])}, "
            f"{_ts_inline(forbidden_step['params'])});"
        )
        lines.append(f"  assertRejected(result, {rejected});")
    lines.append("});")
    return "\n".join(lines)


def emit_vitest(collected, spec_path, output_path=None):
    spec = collected["spec"]
    scenario_data = collected["scenarios"]
    walk = _bake_random_walk(spec)

    parts = [_VITEST_PREAMBLE.replace("__SOURCE__", Path(spec_path).name)]

    for scen in scenario_data:
        parts.append(_vitest_scenario_block(scen))

    walk_section = "\n".join([
        "interface WalkStep {",
        "  action: string;",
        "  params: Record<string, unknown>;",
        "  expected: Record<string, unknown>;",
        "}",
        "",
        f"const RANDOM_WALK_INITIAL: Record<string, unknown> = {_ts_inline(walk['initial'])};",
        f"const RANDOM_WALK: WalkStep[] = {_ts_array(walk['steps'])};",
        "",
        'scenario("random-walk conformance (baked oracle trace)", () => {',
        "  adapter.reset();",
        "  assertPartial(adapter.observe(), RANDOM_WALK_INITIAL);",
        "  for (const step of RANDOM_WALK) {",
        "    adapter.step(step.action, step.params);",
        "    assertPartial(adapter.observe(), step.expected);",
        "  }",
        "});",
    ])
    parts.append(walk_section)

    return "\n\n".join(parts) + "\n"


# --------------------------------------------------------------------------
# emitter dispatch + public API
# --------------------------------------------------------------------------
_EMITTERS = {
    "pytest": emit_pytest,
    "vitest": emit_vitest,
}

_TARGET_EXTENSION = {
    "pytest": "py",
    "vitest": "test.ts",
}


def available_targets():
    return tuple(_EMITTERS)


def generate_test_bundle(
        spec_path, depth=8, deadlock_mode="warn", output_path=None, strict=False,
        target="pytest"):
    emit = _EMITTERS.get(target)
    if emit is None:
        raise ValueError(
            f"unknown testgen target '{target}'; choose one of {', '.join(available_targets())}"
        )
    collected = _collect_scenarios(
        spec_path, depth=depth, deadlock_mode=deadlock_mode, strict=strict)
    content = emit(collected, spec_path, output_path)
    return {
        "content": content,
        "spec": collected["spec_name"],
        "module": collected["module_name"],
        "warnings": collected["warnings"],
        "target": target,
    }


def generate_test_file(
        spec_path, depth=8, deadlock_mode="warn", output_path=None, strict=False,
        target="pytest"):
    return generate_test_bundle(
        spec_path,
        depth=depth,
        deadlock_mode=deadlock_mode,
        output_path=output_path,
        strict=strict,
        target=target,
    )["content"]


def default_output_name(spec_path, target="pytest"):
    path = Path(spec_path)
    src = path.read_text(encoding="utf-8")
    ast, display_names = parse_src(src, str(path.parent))
    spec = build_spec(ast, display_names)
    module = _module_name(spec["name"])
    if target == "vitest":
        return f"{module}.test.ts"
    return f"test_{module}.py"
