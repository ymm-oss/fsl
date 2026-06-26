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


def _snake_case(spec_name):
    """PascalCase/camelCase -> snake_case (e.g. ShoppingCart -> shopping_cart)."""
    if not spec_name:
        return "spec"
    s = re.sub(r"(.)([A-Z][a-z]+)", r"\1_\2", spec_name)
    s = re.sub(r"([a-z0-9])([A-Z])", r"\1_\2", s)
    return s.lower()


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
# Swift Testing emitter
# --------------------------------------------------------------------------
def _swift_string(s):
    """Render a Python str as a Swift string literal (Swift escape rules, which
    differ from JSON: ``\\u{XX}`` rather than ``\\uXXXX``, no ``\\b``/``\\f``)."""
    out = ['"']
    for ch in s:
        if ch == '"':
            out.append('\\"')
        elif ch == "\\":
            out.append("\\\\")
        elif ch == "\n":
            out.append("\\n")
        elif ch == "\t":
            out.append("\\t")
        elif ch == "\r":
            out.append("\\r")
        elif ch == "\0":
            out.append("\\0")
        elif ord(ch) < 0x20:
            out.append(f"\\u{{{ord(ch):x}}}")
        else:
            out.append(ch)
    out.append('"')
    return "".join(out)


def _swift_literal(obj):
    """Render a JSON-serialisable scenario value as a Swift literal in the
    ``[String: Any]`` world. ``None`` -> ``FSLNull.instance`` (a self-contained
    sentinel so the harness depends only on ``Testing``); int and float stay
    distinct (``1`` is ``Int``, ``1.0`` is ``Double``) and ``fslEqual``
    discriminates them. bool is checked before int (``True`` is an ``int`` in
    Python)."""
    if obj is None:
        return "FSLNull.instance"
    if obj is True:
        return "true"
    if obj is False:
        return "false"
    if isinstance(obj, str):
        return _swift_string(obj)
    if isinstance(obj, int):
        return str(obj)
    if isinstance(obj, float):
        s = repr(obj)
        if not any(c in s for c in ".eEnN"):  # ensure a Double literal, not Int
            s += ".0"
        return s
    if isinstance(obj, list):
        return "[" + ", ".join(_swift_literal(x) for x in obj) + "]"
    if isinstance(obj, dict):
        if not obj:
            return "[String: Any]()"  # `[:]` is ambiguous outside an annotated slot
        items = ", ".join(
            f"{_swift_string(str(k))}: {_swift_literal(v)}" for k, v in obj.items())
        return "[" + items + "]"
    raise TypeError(f"cannot render {type(obj).__name__} as a Swift literal")


def _sanitize_ident(display_name, fallback="scenario"):
    return re.sub(r"[^A-Za-z0-9_]+", "_", display_name).strip("_") or fallback


def _unique_ident(display_name, seen_names, prefix):
    safe = _sanitize_ident(display_name)
    count = seen_names.get(safe, 0) + 1
    seen_names[safe] = count
    if count > 1:
        safe = f"{safe}_{count}"
    return f"{prefix}{safe}"


_SWIFT_PREAMBLE = '''\
// SPDX-License-Identifier: Apache-2.0
//
// Auto-generated FSL conformance tests (Swift Testing).
// Source: __SOURCE__
//
// Wire `makeAdapter()` to your implementation. Until it is wired every test is
// skipped (the `.enabled(if:)` trait checks the adapter and disables the test).
//
// The random-walk trace below was baked at generation time by the FSL Monitor
// (the spec's concrete interpreter) under a fixed seed, so these tests need no
// `fslc`/Python at runtime — they replay the baked oracle states and assert.
import Testing

struct StepResult {
    let ok: Bool
    let kind: String?
}

// A self-contained JSON-null sentinel so the harness depends only on `Testing`
// (no Foundation/NSNull). Project a None/absent Option state field to this.
struct FSLNull: Equatable {
    static let instance = FSLNull()
}

enum FSLNotWired: Error { case notWired }

/// Connect your implementation to the spec actions/state.
///  - reset(): put the implementation in the same initial state as spec `init`
///  - step(action, params): drive one spec action; return a StepResult for
///    forbidden-rejection scenarios (nil is fine for ordinary steps)
///  - observe(): project implementation state onto the spec's logical-state shape
///    (enum = name string, Option = FSLNull.instance|value, Seq = [Any],
///     Map = [String: Any], struct = [String: Any])
protocol Adapter {
    func reset()
    func step(_ action: String, _ params: [String: Any]) -> StepResult?
    func observe() -> [String: Any]
}

// Wire your implementation here. Throwing leaves the suite skipped (not failed).
func makeAdapter() throws -> any Adapter {
    throw FSLNotWired.notWired
}

func isAdapterWired() -> Bool {
    do {
        let a = try makeAdapter()
        a.reset()
        _ = a.observe()
        return true
    } catch {
        return false
    }
}

// Deep equality over the JSON-normal world: Bool/Int/Double/String/FSLNull,
// [Any] (ordered) and [String: Any] (by key). Int and Double do not cross-match.
func fslEqual(_ a: Any, _ b: Any) -> Bool {
    switch (a, b) {
    case (is FSLNull, is FSLNull): return true
    case let (x as Bool, y as Bool): return x == y
    case let (x as Int, y as Int): return x == y
    case let (x as Double, y as Double): return x == y
    case let (x as String, y as String): return x == y
    case let (x as [Any], y as [Any]):
        return x.count == y.count && zip(x, y).allSatisfy { fslEqual($0, $1) }
    case let (x as [String: Any], y as [String: Any]):
        return x.count == y.count && x.allSatisfy { (k, v) in
            guard let w = y[k] else { return false }
            return fslEqual(v, w)
        }
    default: return false
    }
}

// Assert only the fields the spec mentions; recurse into nested (Map/struct) shapes.
func assertPartial(_ observed: [String: Any], _ expected: [String: Any]) {
    for (key, val) in expected {
        let seen = observed[key]
        if let v = val as? [String: Any], let s = seen as? [String: Any] {
            assertPartial(s, v)
        } else {
            #expect(seen != nil, "missing state key \\(key)")
            if let s = seen {
                #expect(fslEqual(s, val), "state \\(key): expected \\(val), got \\(s)")
            }
        }
    }
}

func assertRejected(_ result: StepResult?, _ expectedKind: String?) {
    #expect(result != nil, "forbidden step must return a StepResult")
    #expect(result?.ok == false)
    if let kind = expectedKind {
        #expect(result?.kind == kind)
    }
}
'''


def _swift_scenario_block(scen, seen_names):
    name = scen["name"]
    func = _unique_ident(name, seen_names, "scenario_")
    lines = [
        f"@Test(.enabled(if: isAdapterWired())) func {func}() throws {{",
        "    let a = try makeAdapter()",
        "    a.reset()",
    ]
    steps = scen.get("steps", [])
    expected_states = scen.get("expected_states", [])
    for i, step in enumerate(steps):
        lines.append(
            f"    _ = a.step({_swift_string(step['action'])}, {_swift_literal(step['params'])})")
        exp = expected_states[i] if i < len(expected_states) else {}
        lines.append(f"    assertPartial(a.observe(), {_swift_literal(exp)})")
    if scen.get("kind") == "forbidden" and scen.get("forbidden_step"):
        forbidden_step = scen["forbidden_step"]
        rejected_by = scen.get("rejected_by")
        rejected = _swift_string(rejected_by) if rejected_by is not None else "nil"
        lines.append(
            f"    let result = a.step({_swift_string(forbidden_step['action'])}, "
            f"{_swift_literal(forbidden_step['params'])})")
        lines.append(f"    assertRejected(result, {rejected})")
    lines.append("}")
    return "\n".join(lines)


def emit_swift(collected, spec_path, output_path=None):
    spec = collected["spec"]
    scenario_data = collected["scenarios"]
    walk = _bake_random_walk(spec)

    parts = [_SWIFT_PREAMBLE.replace("__SOURCE__", Path(spec_path).name)]

    seen_names = {}
    for scen in scenario_data:
        parts.append(_swift_scenario_block(scen, seen_names))

    walk_type = "[(action: String, params: [String: Any], expected: [String: Any])]"
    if walk["steps"]:
        walk_rows = ",\n".join(
            f"        (action: {_swift_string(s['action'])}, "
            f"params: {_swift_literal(s['params'])}, "
            f"expected: {_swift_literal(s['expected'])})"
            for s in walk["steps"]
        )
        walk_literal = "[\n" + walk_rows + ",\n    ]"
    else:
        walk_literal = "[]"
    walk_section = "\n".join([
        "@Test(.enabled(if: isAdapterWired())) func randomWalkConformance() throws {",
        "    // random-walk conformance (baked oracle trace)",
        "    let a = try makeAdapter()",
        "    a.reset()",
        f"    let initial: [String: Any] = {_swift_literal(walk['initial'])}",
        "    assertPartial(a.observe(), initial)",
        f"    let walk: {walk_type} = {walk_literal}",
        "    for step in walk {",
        "        _ = a.step(step.action, step.params)",
        "        assertPartial(a.observe(), step.expected)",
        "    }",
        "}",
    ])
    parts.append(walk_section)

    return "\n\n".join(parts) + "\n"


# --------------------------------------------------------------------------
# Kotlin emitter (kotlin.test — multiplatform, delegates to JUnit on the JVM)
# --------------------------------------------------------------------------
def _kotlin_string(s):
    """Render a Python str as a Kotlin string literal. Kotlin needs ``$`` escaped
    (it starts a string template) and uses ``\\uXXXX`` (4 hex) like JSON."""
    out = ['"']
    for ch in s:
        if ch == '"':
            out.append('\\"')
        elif ch == "\\":
            out.append("\\\\")
        elif ch == "$":
            out.append("\\$")
        elif ch == "\n":
            out.append("\\n")
        elif ch == "\t":
            out.append("\\t")
        elif ch == "\r":
            out.append("\\r")
        elif ord(ch) < 0x20:
            out.append(f"\\u{ord(ch):04x}")
        else:
            out.append(ch)
    out.append('"')
    return "".join(out)


def _kotlin_literal(obj):
    """Render a JSON-serialisable scenario value as a Kotlin literal in the
    ``Map<String, Any?>`` world. None -> null; int -> Int, float -> Double (kept
    distinct — boxed ``Int`` and ``Double`` are unequal); list -> listOf, dict ->
    mapOf. Empty collections carry explicit type args for inference in ``Any?``
    slots. bool is checked before int (``True`` is an ``int`` in Python)."""
    if obj is None:
        return "null"
    if obj is True:
        return "true"
    if obj is False:
        return "false"
    if isinstance(obj, str):
        return _kotlin_string(obj)
    if isinstance(obj, int):
        return str(obj)
    if isinstance(obj, float):
        s = repr(obj)
        if not any(c in s for c in ".eEnN"):
            s += ".0"
        return s
    if isinstance(obj, list):
        if not obj:
            return "listOf<Any?>()"
        return "listOf(" + ", ".join(_kotlin_literal(x) for x in obj) + ")"
    if isinstance(obj, dict):
        if not obj:
            return "mapOf<String, Any?>()"
        items = ", ".join(
            f"{_kotlin_string(str(k))} to {_kotlin_literal(v)}" for k, v in obj.items())
        return "mapOf(" + items + ")"
    raise TypeError(f"cannot render {type(obj).__name__} as a Kotlin literal")


_KOTLIN_PREAMBLE = '''\
// SPDX-License-Identifier: Apache-2.0
//
// Auto-generated FSL conformance tests (kotlin.test).
// Source: __SOURCE__
//
// Wire `makeAdapter()` to your implementation. Until it is wired it returns null
// and every test returns early (kotlin.test has no portable runtime skip), so the
// suite passes trivially rather than failing — mirroring the other targets.
//
// The random-walk trace below was baked at generation time by the FSL Monitor
// (the spec's concrete interpreter) under a fixed seed, so these tests need no
// `fslc`/Python at runtime — they replay the baked oracle states and assert.
import kotlin.test.Test
import kotlin.test.assertEquals
import kotlin.test.assertFalse
import kotlin.test.assertNotNull
import kotlin.test.assertTrue

data class StepResult(val ok: Boolean, val kind: String? = null)

/**
 * Connect your implementation to the spec actions/state.
 *  - reset(): put the implementation in the same initial state as spec `init`
 *  - step(action, params): drive one spec action; return a StepResult for
 *    forbidden-rejection scenarios (null is fine for ordinary steps)
 *  - observe(): project implementation state onto the spec's logical-state shape
 *    (enum = name string, Option = null|value, Seq = List, Map = Map with string
 *     keys, struct = Map)
 */
interface Adapter {
    fun reset()
    fun step(action: String, params: Map<String, Any?>): StepResult?
    fun observe(): Map<String, Any?>
}

// Wire your implementation here. Return your adapter instead of null.
fun makeAdapter(): Adapter? = null

// Assert only the fields the spec mentions; recurse into nested (Map/struct)
// shapes. Kotlin's `==` is deep on List/Map and discriminates Int from Double, so
// leaf assertEquals does the right thing.
fun assertPartial(observed: Map<String, Any?>, expected: Map<String, Any?>) {
    for ((key, value) in expected) {
        assertTrue(observed.containsKey(key), "missing state key $key")
        val seen = observed[key]
        if (value is Map<*, *> && seen is Map<*, *>) {
            @Suppress("UNCHECKED_CAST")
            assertPartial(seen as Map<String, Any?>, value as Map<String, Any?>)
        } else {
            assertEquals(value, seen, "state $key")
        }
    }
}

fun assertRejected(result: StepResult?, expectedKind: String?) {
    assertNotNull(result, "forbidden step must return a StepResult")
    assertFalse(result.ok)
    if (expectedKind != null) {
        assertEquals(expectedKind, result.kind)
    }
}
'''


def _kotlin_scenario_block(scen, seen_names):
    name = scen["name"]
    func = _unique_ident(name, seen_names, "scenario_")
    lines = [
        f"    @Test fun {func}() {{",
        "        val a = makeAdapter() ?: return",
        "        a.reset()",
    ]
    steps = scen.get("steps", [])
    expected_states = scen.get("expected_states", [])
    for i, step in enumerate(steps):
        lines.append(
            f"        a.step({_kotlin_string(step['action'])}, {_kotlin_literal(step['params'])})")
        exp = expected_states[i] if i < len(expected_states) else {}
        lines.append(f"        assertPartial(a.observe(), {_kotlin_literal(exp)})")
    if scen.get("kind") == "forbidden" and scen.get("forbidden_step"):
        forbidden_step = scen["forbidden_step"]
        rejected_by = scen.get("rejected_by")
        rejected = _kotlin_string(rejected_by) if rejected_by is not None else "null"
        lines.append(
            f"        val result = a.step({_kotlin_string(forbidden_step['action'])}, "
            f"{_kotlin_literal(forbidden_step['params'])})")
        lines.append(f"        assertRejected(result, {rejected})")
    lines.append("    }")
    return "\n".join(lines)


def emit_kotlin(collected, spec_path, output_path=None):
    spec = collected["spec"]
    spec_name = collected["spec_name"]
    scenario_data = collected["scenarios"]
    walk = _bake_random_walk(spec)

    body = [_KOTLIN_PREAMBLE.replace("__SOURCE__", Path(spec_path).name)]

    class_lines = [f"class {spec_name}ConformanceTest {{"]
    seen_names = {}
    for scen in scenario_data:
        class_lines.append("")
        class_lines.append(_kotlin_scenario_block(scen, seen_names))

    walk_type = "List<Triple<String, Map<String, Any?>, Map<String, Any?>>>"
    if walk["steps"]:
        walk_rows = ",\n".join(
            f"            Triple({_kotlin_string(s['action'])}, "
            f"{_kotlin_literal(s['params'])}, "
            f"{_kotlin_literal(s['expected'])})"
            for s in walk["steps"]
        )
        walk_literal = "listOf(\n" + walk_rows + ",\n        )"
    else:
        walk_literal = "listOf<Triple<String, Map<String, Any?>, Map<String, Any?>>>()"
    class_lines.extend([
        "",
        "    @Test fun randomWalkConformance() {",
        "        // random-walk conformance (baked oracle trace)",
        "        val a = makeAdapter() ?: return",
        "        a.reset()",
        f"        val initial: Map<String, Any?> = {_kotlin_literal(walk['initial'])}",
        "        assertPartial(a.observe(), initial)",
        f"        val walk: {walk_type} = {walk_literal}",
        "        for ((action, params, expected) in walk) {",
        "            a.step(action, params)",
        "            assertPartial(a.observe(), expected)",
        "        }",
        "    }",
        "}",
    ])
    body.append("\n".join(class_lines))

    return "\n\n".join(body) + "\n"


# --------------------------------------------------------------------------
# Dart emitter (package:test — also runs under `flutter test`)
# --------------------------------------------------------------------------
def _dart_string(s):
    """Render a Python str as a Dart single-quoted string literal. Dart needs
    ``$`` escaped (string interpolation) and uses ``\\u{XX}``."""
    out = ["'"]
    for ch in s:
        if ch == "'":
            out.append("\\'")
        elif ch == "\\":
            out.append("\\\\")
        elif ch == "$":
            out.append("\\$")
        elif ch == "\n":
            out.append("\\n")
        elif ch == "\t":
            out.append("\\t")
        elif ch == "\r":
            out.append("\\r")
        elif ord(ch) < 0x20:
            out.append(f"\\u{{{ord(ch):x}}}")
        else:
            out.append(ch)
    out.append("'")
    return "".join(out)


def _dart_literal(obj):
    """Render a JSON-serialisable scenario value as a Dart literal in the
    ``Map<String, dynamic>`` world. None -> null; int -> int, float -> double
    (Dart's ``==`` treats ``1 == 1.0`` as true, so they compare equal — this is
    language semantics, not a bug); list/map literals (empty ones carry explicit
    type args). bool is checked before int (``True`` is an ``int`` in Python)."""
    if obj is None:
        return "null"
    if obj is True:
        return "true"
    if obj is False:
        return "false"
    if isinstance(obj, str):
        return _dart_string(obj)
    if isinstance(obj, int):
        return str(obj)
    if isinstance(obj, float):
        s = repr(obj)
        if not any(c in s for c in ".eEnN"):
            s += ".0"
        return s
    if isinstance(obj, list):
        if not obj:
            return "<dynamic>[]"
        return "[" + ", ".join(_dart_literal(x) for x in obj) + "]"
    if isinstance(obj, dict):
        if not obj:
            return "<String, dynamic>{}"
        items = ", ".join(
            f"{_dart_string(str(k))}: {_dart_literal(v)}" for k, v in obj.items())
        return "{" + items + "}"
    raise TypeError(f"cannot render {type(obj).__name__} as a Dart literal")


_DART_PREAMBLE = '''\
// SPDX-License-Identifier: Apache-2.0
//
// Auto-generated FSL conformance tests (package:test).
// Source: __SOURCE__
//
// Wire `makeAdapter()` to your implementation. Until it is wired every test is
// skipped (a top-level probe sets `skip:` on each test), mirroring the other
// targets' skip-when-unwired behaviour.
//
// The random-walk trace below was baked at generation time by the FSL Monitor
// (the spec's concrete interpreter) under a fixed seed, so these tests need no
// `fslc`/Python at runtime — they replay the baked oracle states and assert.
import 'package:test/test.dart';

class StepResult {
  final bool ok;
  final String? kind;
  StepResult(this.ok, [this.kind]);
}

/// Connect your implementation to the spec actions/state.
///  - reset(): put the implementation in the same initial state as spec `init`
///  - step(action, params): drive one spec action; return a StepResult for
///    forbidden-rejection scenarios (null is fine for ordinary steps)
///  - observe(): project implementation state onto the spec's logical-state shape
///    (enum = name string, Option = null|value, Seq = List, Map = Map with string
///     keys, struct = Map)
abstract class Adapter {
  void reset();
  StepResult? step(String action, Map<String, dynamic> params);
  Map<String, dynamic> observe();
}

// Wire your implementation here. Return your adapter instead of throwing.
Adapter makeAdapter() =>
    throw UnimplementedError('wire your implementation: implement makeAdapter()');

bool _adapterWired() {
  try {
    final a = makeAdapter();
    a.reset();
    a.observe();
    return true;
  } catch (_) {
    return false;
  }
}

// Assert only the fields the spec mentions; recurse into nested (Map/struct)
// shapes. `equals` (from package:test's matcher) is deep on List/Map, so leaves
// and sequences are compared structurally with the generated file depending only
// on package:test.
void assertPartial(Map<String, dynamic> observed, Map<String, dynamic> expected) {
  expected.forEach((key, value) {
    expect(observed.containsKey(key), isTrue, reason: 'missing state key $key');
    final seen = observed[key];
    if (value is Map && seen is Map) {
      assertPartial(
        Map<String, dynamic>.from(seen),
        Map<String, dynamic>.from(value),
      );
    } else {
      expect(seen, equals(value), reason: 'state $key');
    }
  });
}

void assertRejected(StepResult? result, String? expectedKind) {
  expect(result, isNotNull, reason: 'forbidden step must return a StepResult');
  expect(result!.ok, isFalse);
  if (expectedKind != null) {
    expect(result.kind, equals(expectedKind));
  }
}
'''


def _dart_scenario_block(scen):
    name = scen["name"]
    lines = [
        f"  test({_dart_string('scenario: ' + name)}, () {{",
        "    final a = makeAdapter();",
        "    a.reset();",
    ]
    steps = scen.get("steps", [])
    expected_states = scen.get("expected_states", [])
    for i, step in enumerate(steps):
        lines.append(
            f"    a.step({_dart_string(step['action'])}, {_dart_literal(step['params'])});")
        exp = expected_states[i] if i < len(expected_states) else {}
        lines.append(f"    assertPartial(a.observe(), {_dart_literal(exp)});")
    if scen.get("kind") == "forbidden" and scen.get("forbidden_step"):
        forbidden_step = scen["forbidden_step"]
        rejected_by = scen.get("rejected_by")
        rejected = _dart_string(rejected_by) if rejected_by is not None else "null"
        lines.append(
            f"    final result = a.step({_dart_string(forbidden_step['action'])}, "
            f"{_dart_literal(forbidden_step['params'])});")
        lines.append(f"    assertRejected(result, {rejected});")
    lines.append("  }, skip: wired ? null : 'Adapter not wired');")
    return "\n".join(lines)


def emit_dart(collected, spec_path, output_path=None):
    spec = collected["spec"]
    scenario_data = collected["scenarios"]
    walk = _bake_random_walk(spec)

    parts = [_DART_PREAMBLE.replace("__SOURCE__", Path(spec_path).name)]

    main_lines = ["void main() {", "  final wired = _adapterWired();"]
    for scen in scenario_data:
        main_lines.append("")
        main_lines.append(_dart_scenario_block(scen))

    if walk["steps"]:
        walk_rows = ",\n".join(
            "      {"
            f"'action': {_dart_string(s['action'])}, "
            f"'params': {_dart_literal(s['params'])}, "
            f"'expected': {_dart_literal(s['expected'])}"
            "}"
            for s in walk["steps"]
        )
        walk_literal = "<Map<String, dynamic>>[\n" + walk_rows + ",\n    ]"
    else:
        walk_literal = "<Map<String, dynamic>>[]"
    main_lines.extend([
        "",
        "  test('random-walk conformance (baked oracle trace)', () {",
        "    final a = makeAdapter();",
        "    a.reset();",
        f"    final initial = {_dart_literal(walk['initial'])};",
        "    assertPartial(a.observe(), Map<String, dynamic>.from(initial));",
        f"    final walk = {walk_literal};",
        "    for (final step in walk) {",
        "      a.step(step['action'] as String, step['params'] as Map<String, dynamic>);",
        "      assertPartial(a.observe(), step['expected'] as Map<String, dynamic>);",
        "    }",
        "  }, skip: wired ? null : 'Adapter not wired');",
        "}",
    ])
    parts.append("\n".join(main_lines))

    return "\n\n".join(parts) + "\n"


# --------------------------------------------------------------------------
# PHPUnit emitter (PHP 8.1+ / PHPUnit 10+)
# --------------------------------------------------------------------------
def _php_string(s):
    """Render a Python str as a PHP single-quoted string literal. Single-quoted
    PHP interpolates nothing and only recognises ``\\\\`` and ``\\'``."""
    return "'" + s.replace("\\", "\\\\").replace("'", "\\'") + "'"


def _php_literal(obj):
    """Render a JSON-serialisable scenario value as a PHP literal. int and float
    are kept distinct (``1`` vs ``1.0``) because leaves are compared with
    ``assertSame`` (``===``), which treats them as unequal — the whole point of the
    PHP target. JSON array -> PHP list, JSON object -> associative array (PHP
    coerces numeric string keys like ``'0'`` to int, consistently on both sides).
    bool is checked before int (``True`` is an ``int`` in Python)."""
    if obj is None:
        return "null"
    if obj is True:
        return "true"
    if obj is False:
        return "false"
    if isinstance(obj, str):
        return _php_string(obj)
    if isinstance(obj, int):
        return str(obj)
    if isinstance(obj, float):
        s = repr(obj)
        if not any(c in s for c in ".eEnN"):
            s += ".0"
        return s
    if isinstance(obj, list):
        return "[" + ", ".join(_php_literal(x) for x in obj) + "]"
    if isinstance(obj, dict):
        if not obj:
            return "[]"
        items = ", ".join(
            f"{_php_string(str(k))} => {_php_literal(v)}" for k, v in obj.items())
        return "[" + items + "]"
    raise TypeError(f"cannot render {type(obj).__name__} as a PHP literal")


_PHP_PREAMBLE = '''\
<?php
// SPDX-License-Identifier: Apache-2.0
//
// Auto-generated FSL conformance tests (PHPUnit, PHP 8.1+ / PHPUnit 10+).
// Source: __SOURCE__
//
// Wire `makeAdapter()` to your implementation. Until it is wired, setUp() marks
// every test skipped (mirroring the other targets' skip-when-unwired behaviour).
//
// The random-walk trace below was baked at generation time by the FSL Monitor
// (the spec's concrete interpreter) under a fixed seed, so these tests need no
// `fslc`/Python at runtime — they replay the baked oracle states and assert.
declare(strict_types=1);

use PHPUnit\\Framework\\TestCase;

interface Adapter
{
    public function reset(): void;
    // Return ['ok' => bool, 'kind' => ?string] for forbidden-rejection scenarios;
    // null is fine for ordinary steps.
    public function step(string $action, array $params): ?array;
    public function observe(): array;
}
'''


_PHP_CLASS_HELPERS = '''\
    private ?Adapter $adapter = null;

    // Wire your implementation here: return your adapter instead of throwing.
    protected function makeAdapter(): Adapter
    {
        throw new \\RuntimeException('wire your implementation: implement makeAdapter()');
    }

    protected function setUp(): void
    {
        try {
            $a = $this->makeAdapter();
            $a->reset();
            $a->observe();
            $this->adapter = $a;
        } catch (\\Throwable $e) {
            $this->markTestSkipped('Adapter not wired: ' . $e->getMessage());
        }
    }

    // Compare the spec's expected state against the observed state. Recurse into
    // arrays by the EXPECTED keys (so maps match by key — order-independent, and
    // only the fields the spec mentions are asserted; PHP coerces numeric string
    // keys like '0' to int consistently on both sides). A list-shaped expected
    // also pins the length, so sequences are exact. Leaves use assertSame (===),
    // so int/float, bool and null never coerce — the point of the PHP target.
    private function assertPartial(mixed $expected, mixed $observed, string $path = 'state'): void
    {
        if (is_array($expected) && is_array($observed)) {
            if (array_is_list($expected)) {
                $this->assertCount(count($expected), $observed, "length at $path");
            }
            foreach ($expected as $key => $value) {
                $this->assertArrayHasKey($key, $observed, "missing $path.$key");
                $this->assertPartial($value, $observed[$key], "$path.$key");
            }
        } else {
            $this->assertSame($expected, $observed, "at $path");
        }
    }

    private function assertRejected(?array $result, ?string $expectedKind): void
    {
        $this->assertIsArray($result, 'forbidden step must return a result array');
        $this->assertFalse($result['ok']);
        if ($expectedKind !== null) {
            $this->assertSame($expectedKind, $result['kind']);
        }
    }
'''


def _php_scenario_block(scen, seen_names):
    name = scen["name"]
    method = _unique_ident(name, seen_names, "testScenario_")
    lines = [
        f"    public function {method}(): void",
        "    {",
        "        $a = $this->adapter;",
        "        $a->reset();",
    ]
    steps = scen.get("steps", [])
    expected_states = scen.get("expected_states", [])
    for i, step in enumerate(steps):
        lines.append(
            f"        $a->step({_php_string(step['action'])}, {_php_literal(step['params'])});")
        exp = expected_states[i] if i < len(expected_states) else {}
        lines.append(f"        $this->assertPartial({_php_literal(exp)}, $a->observe());")
    if scen.get("kind") == "forbidden" and scen.get("forbidden_step"):
        forbidden_step = scen["forbidden_step"]
        rejected_by = scen.get("rejected_by")
        rejected = _php_string(rejected_by) if rejected_by is not None else "null"
        lines.append(
            f"        $result = $a->step({_php_string(forbidden_step['action'])}, "
            f"{_php_literal(forbidden_step['params'])});")
        lines.append(f"        $this->assertRejected($result, {rejected});")
    lines.append("    }")
    return "\n".join(lines)


def emit_phpunit(collected, spec_path, output_path=None):
    spec = collected["spec"]
    spec_name = collected["spec_name"]
    scenario_data = collected["scenarios"]
    walk = _bake_random_walk(spec)

    parts = [_PHP_PREAMBLE.replace("__SOURCE__", Path(spec_path).name)]

    class_lines = [
        f"final class {spec_name}ConformanceTest extends TestCase",
        "{",
        _PHP_CLASS_HELPERS.rstrip("\n"),
    ]
    seen_names = {}
    for scen in scenario_data:
        class_lines.append("")
        class_lines.append(_php_scenario_block(scen, seen_names))

    if walk["steps"]:
        walk_rows = ",\n".join(
            "        ["
            f"'action' => {_php_string(s['action'])}, "
            f"'params' => {_php_literal(s['params'])}, "
            f"'expected' => {_php_literal(s['expected'])}"
            "]"
            for s in walk["steps"]
        )
        walk_literal = "[\n" + walk_rows + ",\n    ]"
    else:
        walk_literal = "[]"
    class_lines.extend([
        "",
        f"    private const INITIAL = {_php_literal(walk['initial'])};",
        f"    private const WALK = {walk_literal};",
        "",
        "    public function testRandomWalkConformance(): void",
        "    {",
        "        // random-walk conformance (baked oracle trace)",
        "        $a = $this->adapter;",
        "        $a->reset();",
        "        $this->assertPartial(self::INITIAL, $a->observe());",
        "        foreach (self::WALK as $step) {",
        "            $a->step($step['action'], $step['params']);",
        "            $this->assertPartial($step['expected'], $a->observe());",
        "        }",
        "    }",
        "}",
    ])
    parts.append("\n".join(class_lines))

    return "\n\n".join(parts) + "\n"


# --------------------------------------------------------------------------
# emitter dispatch + public API
# --------------------------------------------------------------------------
_EMITTERS = {
    "pytest": emit_pytest,
    "vitest": emit_vitest,
    "swift": emit_swift,
    "kotlin": emit_kotlin,
    "dart": emit_dart,
    "phpunit": emit_phpunit,
}

# How each target names its default output file. Conventions diverge (prefix vs
# suffix, camelCase vs PascalCase), so this is a per-target function of the spec
# name and its camelCase module form rather than a bare extension swap.
_TARGET_OUTPUT_NAME = {
    "pytest": lambda name, module: f"test_{module}.py",
    "vitest": lambda name, module: f"{module}.test.ts",
    "swift": lambda name, module: f"{name}ConformanceTests.swift",
    "kotlin": lambda name, module: f"{name}ConformanceTest.kt",
    "dart": lambda name, module: f"{_snake_case(name)}_conformance_test.dart",
    "phpunit": lambda name, module: f"{name}ConformanceTest.php",
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
    name = spec["name"]
    namer = _TARGET_OUTPUT_NAME.get(target, _TARGET_OUTPUT_NAME["pytest"])
    return namer(name, _module_name(name))
