"""FSL v2.0 runtime monitor, replay, and testgen tests (DESIGN-bridge §6)."""
import sys
import ast
import copy
import json
import re
import shutil
import subprocess
import tempfile
from pathlib import Path

import pytest

from fslc import parse, build_spec, verify, scenarios, FslError, Monitor
from fslc.cli import run_replay, run_testgen, exit_code
from fslc.testgen import generate_test_file, generate_test_bundle, default_output_name

ROOT = Path(__file__).resolve().parent.parent
SPECS = ROOT / "specs"
PY = sys.executable

SAMPLE_SPECS = [
    "cart_v1.fsl",
    "order_workflow.fsl",
    "auth_lockout.fsl",
    "inventory_reservation.fsl",
    "payment.fsl",
    "rate_limiter.fsl",
    "mutex_queue.fsl",
    "job_pipeline.fsl",
    "audit_log.fsl",
    "cart_fixed.fsl",
]


def _run_verify(name, depth=8):
    src = (SPECS / name).read_text(encoding="utf-8")
    return verify(build_spec(parse(src)), depth, source_lines=src.splitlines())


def _run_scenarios(name, depth=8):
    src = (SPECS / name).read_text(encoding="utf-8")
    return scenarios(build_spec(parse(src)), depth, source_lines=src.splitlines())


def _replay_trace(mon, trace):
    """Replay witness or scenario trace entries; return list of mismatches."""
    mismatches = []
    mon.reset()
    for i, entry in enumerate(trace):
        if i == 0:
            expected = entry["state"]
            if mon.state != expected:
                mismatches.append((i, "initial", mon.state, expected))
            continue
        if "action" not in entry:
            expected = entry["state"]
            if mon.state != expected:
                mismatches.append((i, "no-action", mon.state, expected))
            continue
        act = entry["action"]
        result = mon.step(act["name"], act["params"])
        if not result["ok"]:
            mismatches.append((i, "step-failed", result, entry))
            continue
        expected = entry["state"]
        if mon.state != expected:
            mismatches.append((i, act["name"], mon.state, expected))
    return mismatches


def _replay_scenario_steps(mon, steps, expected_states):
    mismatches = []
    mon.reset()
    for i, (step, expected) in enumerate(zip(steps, expected_states)):
        result = mon.step(step["action"], step["params"])
        if not result["ok"]:
            mismatches.append((i, "step-failed", result))
            continue
        if mon.state != expected:
            mismatches.append((i, step["action"], mon.state, expected))
    return mismatches


@pytest.mark.parametrize("spec_name", SAMPLE_SPECS)
def test_differential_witness_and_scenarios_match_monitor(spec_name):
    """§6.1: Z3 witness traces and scenario steps match concrete Monitor replay."""
    vr = _run_verify(spec_name)
    assert vr["result"] == "verified", f"{spec_name}: {vr.get('result')}"

    mon = Monitor(str(SPECS / spec_name))
    all_mismatches = []

    for rname, rdata in vr.get("reachables", {}).items():
        witness = rdata["witness"]
        mm = _replay_trace(mon, witness)
        if mm:
            all_mismatches.append(("witness", rname, mm))

    sc = _run_scenarios(spec_name)
    assert sc["result"] == "scenarios"
    for scen in sc["scenarios"]:
        mm = _replay_scenario_steps(mon, scen["steps"], scen["expected_states"])
        if mm:
            all_mismatches.append(("scenario", scen["name"], mm))

    assert not all_mismatches, (
        f"{spec_name}: Monitor/BMC semantic mismatch — likely runtime bug if witness "
        f"was correct: {all_mismatches[:3]}"
    )


def test_requires_failed_guard_unchanged_state():
    mon = Monitor(str(SPECS / "cart_v1.fsl"))
    mon.reset()
    mon.step("add_to_cart", {"u": 0, "i": 0})
    before = copy.deepcopy(mon.state)
    r = mon.step("add_to_cart", {"u": 0, "i": 1})
    assert r["ok"] is False
    assert r["kind"] == "requires_failed"
    assert mon.state == before


def test_ensures_violation_kind():
    src = """
spec EnsuresBug {
  state { x: Int }
  init { x = 0 }
  action bad() {
    x = x + 2
    ensures x == old(x) + 1
  }
}
"""
    vr = verify(build_spec(parse(src)), 4)
    assert vr["result"] == "violated"
    assert vr["violation_kind"] == "ensures"
    trace = vr["trace"]
    last = trace[-1]
    mon = Monitor(build_spec(parse(src)))
    mon.reset()
    for entry in trace[1:-1]:
        mon.step(entry["action"]["name"], entry["action"]["params"])
    r = mon.step(last["action"]["name"], last["action"]["params"])
    assert r["ok"] is False
    assert r["kind"] == "ensures"


def test_type_bound_violation_kind():
    vr = _run_verify("cart_v1_buggy.fsl")
    assert vr["result"] == "violated"
    assert vr["violation_kind"] == "type_bound"
    trace = vr["trace"]
    last = trace[-1]
    mon = Monitor(str(SPECS / "cart_v1_buggy.fsl"))
    mon.reset()
    for entry in trace[1:-1]:
        mon.step(entry["action"]["name"], entry["action"]["params"])
    r = mon.step(last["action"]["name"], last["action"]["params"])
    assert r["ok"] is False
    assert r["kind"] == "type_bound"


def test_invariant_violation_kind():
    src = """
spec InvBug {
  state { x: Int }
  init { x = 0 }
  action inc() { x = x + 1 }
  invariant Pos { x >= 0 }
  invariant Max { x <= 0 }
}
"""
    vr = verify(build_spec(parse(src)), 4)
    assert vr["result"] == "violated"
    assert vr["violation_kind"] == "invariant"
    trace = vr["trace"]
    last = trace[-1]
    mon = Monitor(build_spec(parse(src)))
    mon.reset()
    for entry in trace[1:-1]:
        mon.step(entry["action"]["name"], entry["action"]["params"])
    r = mon.step(last["action"]["name"], last["action"]["params"])
    assert r["ok"] is False
    assert r["kind"] == "invariant"


def test_partial_op_violation_kind():
    src = """
spec PartialUnguarded {
  type JobId = 0..1
  state { queue: Seq<JobId, 2> }
  init { queue = Seq {} }
  action bad_pop() {
    queue = queue.pop()
  }
}
"""
    vr = verify(build_spec(parse(src)), 2)
    assert vr["result"] == "violated"
    assert vr["violation_kind"] == "partial_op"
    trace = vr["trace"]
    last = trace[-1]
    mon = Monitor(build_spec(parse(src)))
    mon.reset()
    r = mon.step(last["action"]["name"], last["action"]["params"])
    assert r["ok"] is False
    assert r["kind"] == "partial_op"
    assert r["name"] == "_partial_bad_pop"


def test_enabled_short_circuits_requires_before_guarded_let():
    mon = Monitor(str(SPECS / "job_pipeline.fsl"))
    mon.reset()
    initial_enabled = mon.enabled()
    assert {entry["action"] for entry in initial_enabled} == {"submit"}

    submit = mon.step("submit", {"j": 0})
    assert submit["ok"] is True
    enabled_after_submit = mon.enabled()
    assert "start" in {entry["action"] for entry in enabled_after_submit}


def test_nondeterministic_init_raises_semantics():
    src = """
spec NoInit {
  state { x: Int, y: Int }
  init { x = 0 }
  action noop() { }
  invariant I { true }
}
"""
    with pytest.raises(FslError) as exc:
        Monitor(build_spec(parse(src)))
    assert exc.value.kind == "semantics"
    assert "deterministic init" in (exc.value.hint or "")


def test_monitor_missing_fsl_path_raises_io():
    with pytest.raises(FslError) as exc:
        Monitor("specs/nonexistent.fsl")
    assert exc.value.kind == "io"
    assert str(exc.value) == "file not found: specs/nonexistent.fsl"


def test_monitor_accepts_direct_fsl_source_string():
    src = """
spec DirectSource {
  state { x: Int }
  init { x = 0 }
  action inc() { x = x + 1 }
  invariant NonNegative { x >= 0 }
}
"""
    mon = Monitor(src)
    assert mon.state == {"x": 0}


def test_replay_conformant_and_nonconformant():
    vr = _run_verify("cart_v1.fsl")
    witness = vr["reachables"]["SoldOut"]["witness"]
    events = [
        {"action": e["action"]["name"], "params": e["action"]["params"]}
        for e in witness[1:]
    ]
    with tempfile.NamedTemporaryFile("w", suffix=".json", delete=False) as f:
        json.dump({"events": events}, f)
        trace_path = f.name
    try:
        good = run_replay(str(SPECS / "cart_v1.fsl"), trace_path)
        assert good["result"] == "conformant"
        assert good["steps_checked"] == len(events)
        assert exit_code(good) == 0

        bad_events = list(events)
        bad_events.append({"action": "checkout", "params": {"u": 0}})
        with tempfile.NamedTemporaryFile("w", suffix=".json", delete=False) as bf:
            json.dump(bad_events, bf)
            bad_path = bf.name
        try:
            bad = run_replay(str(SPECS / "cart_v1.fsl"), bad_path)
            assert bad["result"] == "nonconformant"
            assert bad["violation"]["kind"] == "requires_failed"
            assert exit_code(bad) == 1
        finally:
            Path(bad_path).unlink(missing_ok=True)

        with tempfile.NamedTemporaryFile("w", suffix=".json", delete=False) as af:
            json.dump(events, af)
            arr_path = af.name
        try:
            arr = run_replay(str(SPECS / "cart_v1.fsl"), arr_path)
            assert arr["result"] == "conformant"
        finally:
            Path(arr_path).unlink(missing_ok=True)
    finally:
        Path(trace_path).unlink(missing_ok=True)


def test_testgen_import_skips_without_adapter():
    with tempfile.TemporaryDirectory() as td:
        out = Path(td) / "test_gen.py"
        gen = run_testgen(str(SPECS / "cart_v1.fsl"), output=str(out))
        assert gen["result"] == "generated"
        proc = subprocess.run(
            [str(PY), "-m", "pytest", str(out), "-q"],
            capture_output=True,
            text=True,
            cwd=ROOT,
        )
        assert proc.returncode == 0, proc.stdout + proc.stderr
        assert "skipped" in proc.stdout.lower()


def test_testgen_sanitizes_composed_scenario_function_names():
    with tempfile.TemporaryDirectory() as td:
        out = Path(td) / "test_bank_system.py"
        gen = run_testgen(str(SPECS / "bank_system.fsl"), output=str(out))
        assert gen["result"] == "generated"

        content = out.read_text(encoding="utf-8")
        compile(content, str(out), "exec")
        module = ast.parse(content, filename=str(out))
        test_names = [
            node.name
            for node in module.body
            if isinstance(node, ast.FunctionDef) and node.name.startswith("test_scenario_")
        ]

        assert test_names
        assert all("." not in name for name in test_names)
        assert "test_scenario_reach_bank_Settled" in test_names
        assert "test_scenario_cover_bank_settle" in test_names

        docstrings = {
            node.name: ast.get_docstring(node)
            for node in module.body
            if isinstance(node, ast.FunctionDef) and node.name.startswith("test_scenario_")
        }
        assert docstrings["test_scenario_reach_bank_Settled"] == "Scenario: reach_bank.Settled"
        assert docstrings["test_scenario_cover_bank_settle"] == "Scenario: cover_bank.settle"


def test_testgen_self_conformance_with_monitor_adapter():
    content = generate_test_file(str(SPECS / "cart_v1.fsl"), depth=8)
    adapter_block = '''
class MonitorAdapter:
    def __init__(self):
        self._mon = Monitor(SPEC_PATH)

    def reset(self):
        self._mon.reset()

    def step(self, action, params):
        r = self._mon.step(action, params)
        assert r.get("ok"), r

    def observe(self):
        return self._mon.state

@pytest.fixture
def adapter():
    return MonitorAdapter()
'''
    content = content.replace(
        "@pytest.fixture\ndef adapter():\n    return Adapter()",
        adapter_block.strip(),
    )
    with tempfile.TemporaryDirectory() as td:
        out = Path(td) / "test_self.py"
        out.write_text(content, encoding="utf-8")
        proc = subprocess.run(
            [str(PY), "-m", "pytest", str(out), "-q"],
            capture_output=True,
            text=True,
            cwd=ROOT,
        )
        assert proc.returncode == 0, proc.stdout + proc.stderr
        assert "failed" not in proc.stdout.lower()


# --------------------------------------------------------------------------
# testgen --target vitest (issue #43)
# --------------------------------------------------------------------------
def _node_supports_ts_check():
    """True if a local `node --check` can syntax-check a typed .ts file."""
    node = shutil.which("node")
    if node is None:
        return False
    with tempfile.NamedTemporaryFile("w", suffix=".ts", delete=False, encoding="utf-8") as f:
        # ESM probe with a type annotation: node only strips TS types when the
        # file is detected as a module (the generated harness always imports vitest).
        f.write('import { a } from "b";\nconst x: number = 1;\nvoid a;\nvoid x;\n')
        probe = f.name
    try:
        proc = subprocess.run([node, "--check", probe], capture_output=True, text=True)
        return proc.returncode == 0
    finally:
        Path(probe).unlink(missing_ok=True)


def _assert_ts_syntax_ok(content):
    if not _node_supports_ts_check():
        pytest.skip("node with TypeScript stripping not available")
    node = shutil.which("node")
    with tempfile.NamedTemporaryFile("w", suffix=".test.ts", delete=False, encoding="utf-8") as f:
        f.write(content)
        path = f.name
    try:
        proc = subprocess.run([node, "--check", path], capture_output=True, text=True)
        assert proc.returncode == 0, proc.stdout + proc.stderr
    finally:
        Path(path).unlink(missing_ok=True)


def test_testgen_vitest_emits_typescript_harness():
    content = generate_test_file(str(SPECS / "cart_v1.fsl"), depth=8, target="vitest")

    # Vitest harness shape, not pytest.
    assert 'import { test, expect } from "vitest";' in content
    assert "export interface Adapter {" in content
    assert "function makeAdapter(): Adapter {" in content
    assert "function assertPartial(" in content
    assert "function assertRejected(" in content

    # Scenarios are emitted as reset + step + partial-match assertions. The
    # concrete witness Z3 returns for reach_/cover_ varies run to run, so assert
    # on the stable scenario names and shape, not on specific step params
    # (exact-step assertions live in the deterministic forbidden test below).
    assert 'scenario("scenario: reach_SoldOut", () => {' in content
    assert 'scenario("scenario: cover_add_to_cart", () => {' in content
    assert 'scenario("scenario: cover_checkout", () => {' in content
    assert "adapter.reset();" in content
    assert 'adapter.step("add_to_cart"' in content
    assert "assertPartial(adapter.observe()," in content

    # Acceptance criterion: the random walk is baked, so the file needs no fslc
    # / Python / Monitor at runtime. The only import is vitest; nothing is
    # require()'d or shelled out to.
    assert "const RANDOM_WALK: WalkStep[] = [" in content
    assert "baked oracle trace" in content
    import_lines = [ln for ln in content.splitlines() if ln.lstrip().startswith("import ")]
    assert import_lines == ['import { test, expect } from "vitest";']
    assert "require(" not in content

    # The baked literals are well-formed JSON (language independent).
    initial = re.search(
        r"const RANDOM_WALK_INITIAL: Record<string, unknown> = (\{.*?\});", content)
    assert initial, "missing baked initial state"
    json.loads(initial.group(1))
    walk = re.search(r"const RANDOM_WALK: WalkStep\[\] = (\[.*?\n\]);", content, re.DOTALL)
    assert walk, "missing baked walk array"
    steps = json.loads(re.sub(r",(\s*\])", r"\1", walk.group(1)))  # drop TS trailing comma
    assert steps, "cart_v1 should bake a non-empty random walk"
    assert all({"action", "params", "expected"} <= set(s) for s in steps)
    assert {s["action"] for s in steps} <= {"add_to_cart", "remove_from_cart", "checkout"}

    _assert_ts_syntax_ok(content)


def test_testgen_vitest_forbidden_rejection(tmp_path):
    src = """
requirements ForbiddenVitest {
  type OrderId = 0..1
  enum OSt { Cart, Paid, Shipped, Cancelled }
  state { order: Map<OrderId, OSt> }
  init { forall o: OrderId { order[o] = Cart } }
  action pay(o: OrderId) { requires order[o] == Cart order[o] = Paid }
  action ship(o: OrderId) { requires order[o] == Paid order[o] = Shipped }
  action cancel(o: OrderId) { requires order[o] == Paid order[o] = Cancelled }
  forbidden FB-1 "shipped order cannot be cancelled" {
    pay(0)
    ship(0)
    cancel(0)
    expect rejected
  }
}
"""
    path = tmp_path / "forbidden_vitest.fsl"
    path.write_text(src, encoding="utf-8")

    content = generate_test_file(str(path), depth=4, target="vitest")

    assert 'scenario("scenario: forbidden_FB-1", () => {' in content
    assert 'const result = adapter.step("cancel", {"o": 0});' in content
    assert 'assertRejected(result, "requires_failed");' in content
    # Enum values are baked as their member-name strings.
    assert 'assertPartial(adapter.observe(), {"order": {"0": "Paid", "1": "Cart"}});' in content

    _assert_ts_syntax_ok(content)


def test_testgen_vitest_output_name_and_target(tmp_path):
    spec = str(SPECS / "cart_v1.fsl")
    # ShoppingCart -> shoppingCart.test.ts (vitest) ; test_shoppingCart.py (pytest, unchanged)
    assert default_output_name(spec, target="vitest") == "shoppingCart.test.ts"
    assert default_output_name(spec, target="pytest") == "test_shoppingCart.py"
    assert default_output_name(spec) == "test_shoppingCart.py"  # default stays pytest

    out = tmp_path / "cart.test.ts"
    result = run_testgen(spec, output=str(out), target="vitest")
    assert result["result"] == "generated"
    assert result["target"] == "vitest"
    assert result["output"] == str(out)
    written = out.read_text(encoding="utf-8")
    assert written.startswith("// SPDX-License-Identifier: Apache-2.0")
    assert 'import { test, expect } from "vitest";' in written


def test_testgen_unknown_target_rejected():
    with pytest.raises(ValueError):
        generate_test_bundle(str(SPECS / "cart_v1.fsl"), depth=4, target="jest")


# --------------------------------------------------------------------------
# testgen --target swift (issue #44)
# --------------------------------------------------------------------------
def _swiftc_available():
    return shutil.which("swiftc") is not None


def _assert_swift_syntax_ok(content):
    """Syntax-only gate (parse, no module resolution) — the swiftc analog of the
    vitest `node --check`. `import Testing` need not resolve under `-parse`."""
    if not _swiftc_available():
        pytest.skip("swiftc not available")
    swiftc = shutil.which("swiftc")
    with tempfile.NamedTemporaryFile("w", suffix=".swift", delete=False, encoding="utf-8") as f:
        f.write(content)
        path = f.name
    try:
        proc = subprocess.run([swiftc, "-parse", path], capture_output=True, text=True)
        assert proc.returncode == 0, proc.stdout + proc.stderr
    finally:
        Path(path).unlink(missing_ok=True)


def test_testgen_swift_emits_swift_testing_harness():
    content = generate_test_file(str(SPECS / "cart_v1.fsl"), depth=8, target="swift")

    # Swift Testing harness shape (not XCTest, not pytest).
    assert content.startswith("// SPDX-License-Identifier: Apache-2.0")
    assert "import Testing" in content
    assert "protocol Adapter {" in content
    assert "func makeAdapter() throws -> any Adapter {" in content
    assert "func assertPartial(" in content
    assert "func assertRejected(" in content
    assert "struct FSLNull: Equatable {" in content

    # Scenarios -> @Test funcs gated by the adapter-wired trait; assert on the
    # stable scenario names/shape, not specific witness params (the reach_/cover_
    # witness Z3 returns varies run to run — exact steps live in the forbidden test).
    assert "@Test(.enabled(if: isAdapterWired())) func scenario_reach_SoldOut() throws {" in content
    assert "@Test(.enabled(if: isAdapterWired())) func scenario_cover_add_to_cart() throws {" in content
    assert "    let a = try makeAdapter()" in content
    assert "    a.reset()" in content
    assert '    _ = a.step("add_to_cart"' in content
    assert "    assertPartial(a.observe()," in content

    # Acceptance: the random walk is baked, so the file needs no fslc/Python/Monitor
    # at runtime. The only import is Testing (no Foundation); nothing is shelled out.
    assert "func randomWalkConformance() throws {" in content
    assert "baked oracle trace" in content
    assert "let walk: [(action: String, params: [String: Any], expected: [String: Any])] = [" in content
    import_lines = [ln for ln in content.splitlines() if ln.lstrip().startswith("import ")]
    assert import_lines == ["import Testing"]

    # Option None bakes as the self-contained null sentinel; enum members as strings.
    assert "FSLNull.instance" in content

    walk_actions = re.findall(r'\(action: "(\w+)"', content)
    assert walk_actions, "cart_v1 should bake a non-empty random walk"
    assert set(walk_actions) <= {"add_to_cart", "remove_from_cart", "checkout"}

    _assert_swift_syntax_ok(content)


def test_testgen_swift_forbidden_rejection(tmp_path):
    src = """
requirements ForbiddenSwift {
  type OrderId = 0..1
  enum OSt { Cart, Paid, Shipped, Cancelled }
  state { order: Map<OrderId, OSt> }
  init { forall o: OrderId { order[o] = Cart } }
  action pay(o: OrderId) { requires order[o] == Cart order[o] = Paid }
  action ship(o: OrderId) { requires order[o] == Paid order[o] = Shipped }
  action cancel(o: OrderId) { requires order[o] == Paid order[o] = Cancelled }
  forbidden FB-1 "shipped order cannot be cancelled" {
    pay(0)
    ship(0)
    cancel(0)
    expect rejected
  }
}
"""
    path = tmp_path / "forbidden_swift.fsl"
    path.write_text(src, encoding="utf-8")

    content = generate_test_file(str(path), depth=4, target="swift")

    # Hyphen in the forbidden ID is sanitized into a valid Swift identifier.
    assert "func scenario_forbidden_FB_1() throws {" in content
    assert 'let result = a.step("cancel", ["o": 0])' in content
    assert 'assertRejected(result, "requires_failed")' in content
    # Enum values are baked as their member-name strings; Map keys as strings.
    assert 'assertPartial(a.observe(), ["order": ["0": "Paid", "1": "Cart"]])' in content

    _assert_swift_syntax_ok(content)


def test_testgen_swift_output_name_and_target(tmp_path):
    spec = str(SPECS / "cart_v1.fsl")
    # ShoppingCart -> ShoppingCartConformanceTests.swift; pytest/vitest unchanged.
    assert default_output_name(spec, target="swift") == "ShoppingCartConformanceTests.swift"
    assert default_output_name(spec, target="vitest") == "shoppingCart.test.ts"
    assert default_output_name(spec, target="pytest") == "test_shoppingCart.py"

    out = tmp_path / "Cart.swift"
    result = run_testgen(spec, output=str(out), target="swift")
    assert result["result"] == "generated"
    assert result["target"] == "swift"
    assert result["output"] == str(out)
    written = out.read_text(encoding="utf-8")
    assert written.startswith("// SPDX-License-Identifier: Apache-2.0")
    assert "import Testing" in written


# --------------------------------------------------------------------------
# testgen --target kotlin (issue #45)
# --------------------------------------------------------------------------
# No compiler gate: kotlinc has no dependency-free parse-only mode (a real
# compile needs kotlin-test on the classpath), unlike swiftc -parse / node
# --check. We assert on harness shape and the baked walk instead.
def test_testgen_kotlin_emits_kotlin_test_harness():
    content = generate_test_file(str(SPECS / "cart_v1.fsl"), depth=8, target="kotlin")

    # kotlin.test harness shape.
    assert content.startswith("// SPDX-License-Identifier: Apache-2.0")
    assert "import kotlin.test.Test" in content
    assert "data class StepResult(val ok: Boolean, val kind: String? = null)" in content
    assert "interface Adapter {" in content
    assert "fun makeAdapter(): Adapter? = null" in content
    assert "fun assertPartial(" in content
    assert "fun assertRejected(" in content
    assert "class ShoppingCartConformanceTest {" in content

    # Scenarios -> @Test funcs; assert on stable names/shape (witness params vary).
    assert "@Test fun scenario_reach_SoldOut() {" in content
    assert "@Test fun scenario_cover_add_to_cart() {" in content
    assert "val a = makeAdapter() ?: return" in content
    assert 'a.step("add_to_cart"' in content
    assert "assertPartial(a.observe()," in content

    # Acceptance: random walk baked, no fslc/Python at runtime. The only imports
    # are kotlin.test.* (no fslc/runtime), so nothing is shelled out.
    assert "@Test fun randomWalkConformance() {" in content
    assert "baked oracle trace" in content
    assert ("val walk: List<Triple<String, Map<String, Any?>, Map<String, Any?>>> = listOf("
            in content)
    import_lines = [ln for ln in content.splitlines() if ln.lstrip().startswith("import ")]
    assert import_lines and all(ln.startswith("import kotlin.test.") for ln in import_lines)

    # Option None bakes as Kotlin null; Map keys as strings.
    assert '"cart" to mapOf("0" to null' in content

    walk_actions = re.findall(r'Triple\("(\w+)"', content)
    assert walk_actions, "cart_v1 should bake a non-empty random walk"
    assert set(walk_actions) <= {"add_to_cart", "remove_from_cart", "checkout"}


def test_testgen_kotlin_forbidden_rejection(tmp_path):
    src = """
requirements ForbiddenKotlin {
  type OrderId = 0..1
  enum OSt { Cart, Paid, Shipped, Cancelled }
  state { order: Map<OrderId, OSt> }
  init { forall o: OrderId { order[o] = Cart } }
  action pay(o: OrderId) { requires order[o] == Cart order[o] = Paid }
  action ship(o: OrderId) { requires order[o] == Paid order[o] = Shipped }
  action cancel(o: OrderId) { requires order[o] == Paid order[o] = Cancelled }
  forbidden FB-1 "shipped order cannot be cancelled" {
    pay(0)
    ship(0)
    cancel(0)
    expect rejected
  }
}
"""
    path = tmp_path / "forbidden_kotlin.fsl"
    path.write_text(src, encoding="utf-8")

    content = generate_test_file(str(path), depth=4, target="kotlin")

    assert "class ForbiddenKotlinConformanceTest {" in content
    assert "@Test fun scenario_forbidden_FB_1() {" in content
    assert 'val result = a.step("cancel", mapOf("o" to 0))' in content
    assert 'assertRejected(result, "requires_failed")' in content
    # Enum values baked as member-name strings; Map keys as strings.
    assert ('assertPartial(a.observe(), mapOf("order" to mapOf("0" to "Paid", "1" to "Cart")))'
            in content)


def test_testgen_kotlin_output_name_and_target(tmp_path):
    spec = str(SPECS / "cart_v1.fsl")
    assert default_output_name(spec, target="kotlin") == "ShoppingCartConformanceTest.kt"
    assert default_output_name(spec, target="swift") == "ShoppingCartConformanceTests.swift"
    assert default_output_name(spec, target="pytest") == "test_shoppingCart.py"

    out = tmp_path / "Cart.kt"
    result = run_testgen(spec, output=str(out), target="kotlin")
    assert result["result"] == "generated"
    assert result["target"] == "kotlin"
    assert result["output"] == str(out)
    written = out.read_text(encoding="utf-8")
    assert written.startswith("// SPDX-License-Identifier: Apache-2.0")
    assert "import kotlin.test.Test" in written


# --------------------------------------------------------------------------
# testgen --target dart (issue #46)
# --------------------------------------------------------------------------
# No compiler gate: `dart analyze` needs a pub package context (package:test
# resolved), so there is no clean dependency-free parse-only mode like swiftc
# -parse / node --check. We assert on harness shape and the baked walk.
def test_testgen_dart_emits_package_test_harness():
    content = generate_test_file(str(SPECS / "cart_v1.fsl"), depth=8, target="dart")

    # package:test harness shape.
    assert content.startswith("// SPDX-License-Identifier: Apache-2.0")
    assert "import 'package:test/test.dart';" in content
    assert "class StepResult {" in content
    assert "abstract class Adapter {" in content
    assert "Adapter makeAdapter() =>" in content
    assert "void assertPartial(" in content
    assert "void assertRejected(" in content
    assert "void main() {" in content
    assert "final wired = _adapterWired();" in content

    # Scenarios -> test(...) blocks; assert on stable names/shape.
    assert "test('scenario: reach_SoldOut', () {" in content
    assert "test('scenario: cover_add_to_cart', () {" in content
    assert "final a = makeAdapter();" in content
    assert "a.step('add_to_cart'," in content
    assert "assertPartial(a.observe()," in content
    # Skip-when-unwired: every test carries the conditional skip.
    assert "}, skip: wired ? null : 'Adapter not wired');" in content

    # Acceptance: random walk baked, no fslc/Python at runtime. The only import is
    # package:test; nothing is shelled out.
    assert "test('random-walk conformance (baked oracle trace)', () {" in content
    assert "final walk = <Map<String, dynamic>>[" in content
    import_lines = [ln for ln in content.splitlines() if ln.lstrip().startswith("import ")]
    assert import_lines == ["import 'package:test/test.dart';"]

    # Option None bakes as Dart null; Map keys as strings.
    assert "'cart': {'0': null" in content

    walk_actions = re.findall(r"'action': '(\w+)'", content)
    assert walk_actions, "cart_v1 should bake a non-empty random walk"
    assert set(walk_actions) <= {"add_to_cart", "remove_from_cart", "checkout"}


def test_testgen_dart_forbidden_rejection(tmp_path):
    src = """
requirements ForbiddenDart {
  type OrderId = 0..1
  enum OSt { Cart, Paid, Shipped, Cancelled }
  state { order: Map<OrderId, OSt> }
  init { forall o: OrderId { order[o] = Cart } }
  action pay(o: OrderId) { requires order[o] == Cart order[o] = Paid }
  action ship(o: OrderId) { requires order[o] == Paid order[o] = Shipped }
  action cancel(o: OrderId) { requires order[o] == Paid order[o] = Cancelled }
  forbidden FB-1 "shipped order cannot be cancelled" {
    pay(0)
    ship(0)
    cancel(0)
    expect rejected
  }
}
"""
    path = tmp_path / "forbidden_dart.fsl"
    path.write_text(src, encoding="utf-8")

    content = generate_test_file(str(path), depth=4, target="dart")

    assert "test('scenario: forbidden_FB-1', () {" in content
    assert "final result = a.step('cancel', {'o': 0});" in content
    assert "assertRejected(result, 'requires_failed');" in content
    # Enum values baked as member-name strings; Map keys as strings.
    assert "assertPartial(a.observe(), {'order': {'0': 'Paid', '1': 'Cart'}});" in content


def test_testgen_dart_output_name_and_target(tmp_path):
    spec = str(SPECS / "cart_v1.fsl")
    # ShoppingCart -> shopping_cart_conformance_test.dart (snake_case + _test.dart).
    assert default_output_name(spec, target="dart") == "shopping_cart_conformance_test.dart"
    assert default_output_name(spec, target="kotlin") == "ShoppingCartConformanceTest.kt"
    assert default_output_name(spec, target="pytest") == "test_shoppingCart.py"

    out = tmp_path / "cart_test.dart"
    result = run_testgen(spec, output=str(out), target="dart")
    assert result["result"] == "generated"
    assert result["target"] == "dart"
    assert result["output"] == str(out)
    written = out.read_text(encoding="utf-8")
    assert written.startswith("// SPDX-License-Identifier: Apache-2.0")
    assert "import 'package:test/test.dart';" in written


def test_enabled_matches_guarded_instances():
    mon = Monitor(str(SPECS / "cart_v1.fsl"))
    mon.reset()
    mon.step("add_to_cart", {"u": 0, "i": 0})
    enabled = mon.enabled()
    names = {(e["action"], tuple(sorted(e["params"].items()))) for e in enabled}
    assert ("remove_from_cart", (("u", 0),)) in names
    assert ("checkout", (("u", 0),)) in names
    assert all(e["action"] != "add_to_cart" or e["params"]["u"] != 0 for e in enabled)
