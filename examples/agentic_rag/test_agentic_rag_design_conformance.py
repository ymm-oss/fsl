"""Auto-generated conformance tests for FSL spec.
Source: agentic_rag_design.fsl
Connect Adapter to your implementation, or use MonitorSelfAdapter for self-check.
"""
import random
from pathlib import Path

import pytest

from fslc.runtime import Monitor

SPEC_PATH = Path(__file__).resolve().parent / 'agentic_rag_design.fsl'


class Adapter:
    """Connect your implementation to the spec actions/state.

    Wiring convention:
    - reset(): put implementation in the same initial state as spec init
    - step(action, params): drive one spec action on the implementation
    - observe(): return implementation state projected to spec logical state shape
    """

    def reset(self):
        raise NotImplementedError("wire your implementation reset")

    def step(self, action: str, params: dict):
        raise NotImplementedError("wire your implementation step")

    def observe(self) -> dict:
        raise NotImplementedError("wire your implementation observe")


def _adapter_ready(adapter):
    try:
        adapter.reset()
        adapter.observe()
        return True
    except NotImplementedError:
        return False


@pytest.fixture
def adapter():
    return Adapter()


def _assert_partial_expected(observed, expected):
    for key, val in expected.items():
        if isinstance(val, dict) and isinstance(observed.get(key), dict):
            _assert_partial_expected(observed[key], val)
        else:
            assert observed[key] == val


def _assert_rejected(result, expected_kind):
    assert isinstance(result, dict), 'forbidden adapter.step must return a result dict'
    assert result.get('ok') is False
    if expected_kind is not None:
        assert result.get('kind') == expected_kind


def test_scenario_cover_set_operator(adapter):
    'Scenario: cover_set_operator'
    if not _adapter_ready(adapter):
        pytest.skip('Adapter not implemented')
    adapter.reset()
    adapter.step('set_operator', {'r': 0})
    _assert_partial_expected(adapter.observe(), {'d': {'0': {'phase': 'DNew', 'evidence': 'Missing', 'guard': 'Unchecked', 'approval': 'NoApproval', 'role': 'Operator', 'retry': 2, 'needs_tool': False, 'citation_ok': False, 'audit': False}, '1': {'phase': 'DNew', 'evidence': 'Missing', 'guard': 'Unchecked', 'approval': 'NoApproval', 'role': 'Public', 'retry': 2, 'needs_tool': False, 'citation_ok': False, 'audit': False}}})

def test_scenario_cover_accept_request(adapter):
    'Scenario: cover_accept_request'
    if not _adapter_ready(adapter):
        pytest.skip('Adapter not implemented')
    adapter.reset()
    adapter.step('accept_request', {'r': 0})
    _assert_partial_expected(adapter.observe(), {'d': {'0': {'phase': 'DAccepted', 'evidence': 'Missing', 'guard': 'Unchecked', 'approval': 'NoApproval', 'role': 'Public', 'retry': 2, 'needs_tool': False, 'citation_ok': False, 'audit': False}, '1': {'phase': 'DNew', 'evidence': 'Missing', 'guard': 'Unchecked', 'approval': 'NoApproval', 'role': 'Public', 'retry': 2, 'needs_tool': False, 'citation_ok': False, 'audit': False}}})

def test_scenario_cover_write_audit(adapter):
    'Scenario: cover_write_audit'
    if not _adapter_ready(adapter):
        pytest.skip('Adapter not implemented')
    adapter.reset()
    adapter.step('accept_request', {'r': 0})
    _assert_partial_expected(adapter.observe(), {'d': {'0': {'phase': 'DAccepted', 'evidence': 'Missing', 'guard': 'Unchecked', 'approval': 'NoApproval', 'role': 'Public', 'retry': 2, 'needs_tool': False, 'citation_ok': False, 'audit': False}, '1': {'phase': 'DNew', 'evidence': 'Missing', 'guard': 'Unchecked', 'approval': 'NoApproval', 'role': 'Public', 'retry': 2, 'needs_tool': False, 'citation_ok': False, 'audit': False}}})
    adapter.step('write_audit', {'r': 0})
    _assert_partial_expected(adapter.observe(), {'d': {'0': {'phase': 'DReceived', 'evidence': 'Missing', 'guard': 'Unchecked', 'approval': 'NoApproval', 'role': 'Public', 'retry': 2, 'needs_tool': False, 'citation_ok': False, 'audit': True}, '1': {'phase': 'DNew', 'evidence': 'Missing', 'guard': 'Unchecked', 'approval': 'NoApproval', 'role': 'Public', 'retry': 2, 'needs_tool': False, 'citation_ok': False, 'audit': False}}})

def test_scenario_cover_enqueue_router(adapter):
    'Scenario: cover_enqueue_router'
    if not _adapter_ready(adapter):
        pytest.skip('Adapter not implemented')
    adapter.reset()
    adapter.step('accept_request', {'r': 0})
    _assert_partial_expected(adapter.observe(), {'d': {'0': {'phase': 'DAccepted', 'evidence': 'Missing', 'guard': 'Unchecked', 'approval': 'NoApproval', 'role': 'Public', 'retry': 2, 'needs_tool': False, 'citation_ok': False, 'audit': False}, '1': {'phase': 'DNew', 'evidence': 'Missing', 'guard': 'Unchecked', 'approval': 'NoApproval', 'role': 'Public', 'retry': 2, 'needs_tool': False, 'citation_ok': False, 'audit': False}}})
    adapter.step('write_audit', {'r': 0})
    _assert_partial_expected(adapter.observe(), {'d': {'0': {'phase': 'DReceived', 'evidence': 'Missing', 'guard': 'Unchecked', 'approval': 'NoApproval', 'role': 'Public', 'retry': 2, 'needs_tool': False, 'citation_ok': False, 'audit': True}, '1': {'phase': 'DNew', 'evidence': 'Missing', 'guard': 'Unchecked', 'approval': 'NoApproval', 'role': 'Public', 'retry': 2, 'needs_tool': False, 'citation_ok': False, 'audit': False}}})
    adapter.step('enqueue_router', {'r': 0})
    _assert_partial_expected(adapter.observe(), {'d': {'0': {'phase': 'DRouterQueued', 'evidence': 'Missing', 'guard': 'Unchecked', 'approval': 'NoApproval', 'role': 'Public', 'retry': 2, 'needs_tool': False, 'citation_ok': False, 'audit': True}, '1': {'phase': 'DNew', 'evidence': 'Missing', 'guard': 'Unchecked', 'approval': 'NoApproval', 'role': 'Public', 'retry': 2, 'needs_tool': False, 'citation_ok': False, 'audit': False}}})

def test_scenario_cover_route_answer(adapter):
    'Scenario: cover_route_answer'
    if not _adapter_ready(adapter):
        pytest.skip('Adapter not implemented')
    adapter.reset()
    adapter.step('accept_request', {'r': 0})
    _assert_partial_expected(adapter.observe(), {'d': {'0': {'phase': 'DAccepted', 'evidence': 'Missing', 'guard': 'Unchecked', 'approval': 'NoApproval', 'role': 'Public', 'retry': 2, 'needs_tool': False, 'citation_ok': False, 'audit': False}, '1': {'phase': 'DNew', 'evidence': 'Missing', 'guard': 'Unchecked', 'approval': 'NoApproval', 'role': 'Public', 'retry': 2, 'needs_tool': False, 'citation_ok': False, 'audit': False}}})
    adapter.step('write_audit', {'r': 0})
    _assert_partial_expected(adapter.observe(), {'d': {'0': {'phase': 'DReceived', 'evidence': 'Missing', 'guard': 'Unchecked', 'approval': 'NoApproval', 'role': 'Public', 'retry': 2, 'needs_tool': False, 'citation_ok': False, 'audit': True}, '1': {'phase': 'DNew', 'evidence': 'Missing', 'guard': 'Unchecked', 'approval': 'NoApproval', 'role': 'Public', 'retry': 2, 'needs_tool': False, 'citation_ok': False, 'audit': False}}})
    adapter.step('enqueue_router', {'r': 0})
    _assert_partial_expected(adapter.observe(), {'d': {'0': {'phase': 'DRouterQueued', 'evidence': 'Missing', 'guard': 'Unchecked', 'approval': 'NoApproval', 'role': 'Public', 'retry': 2, 'needs_tool': False, 'citation_ok': False, 'audit': True}, '1': {'phase': 'DNew', 'evidence': 'Missing', 'guard': 'Unchecked', 'approval': 'NoApproval', 'role': 'Public', 'retry': 2, 'needs_tool': False, 'citation_ok': False, 'audit': False}}})
    adapter.step('route_answer', {'r': 0})
    _assert_partial_expected(adapter.observe(), {'d': {'0': {'phase': 'DClassified', 'evidence': 'Missing', 'guard': 'Unchecked', 'approval': 'NoApproval', 'role': 'Public', 'retry': 2, 'needs_tool': False, 'citation_ok': False, 'audit': True}, '1': {'phase': 'DNew', 'evidence': 'Missing', 'guard': 'Unchecked', 'approval': 'NoApproval', 'role': 'Public', 'retry': 2, 'needs_tool': False, 'citation_ok': False, 'audit': False}}})

def test_scenario_cover_route_tool(adapter):
    'Scenario: cover_route_tool'
    if not _adapter_ready(adapter):
        pytest.skip('Adapter not implemented')
    adapter.reset()
    adapter.step('accept_request', {'r': 0})
    _assert_partial_expected(adapter.observe(), {'d': {'0': {'phase': 'DAccepted', 'evidence': 'Missing', 'guard': 'Unchecked', 'approval': 'NoApproval', 'role': 'Public', 'retry': 2, 'needs_tool': False, 'citation_ok': False, 'audit': False}, '1': {'phase': 'DNew', 'evidence': 'Missing', 'guard': 'Unchecked', 'approval': 'NoApproval', 'role': 'Public', 'retry': 2, 'needs_tool': False, 'citation_ok': False, 'audit': False}}})
    adapter.step('write_audit', {'r': 0})
    _assert_partial_expected(adapter.observe(), {'d': {'0': {'phase': 'DReceived', 'evidence': 'Missing', 'guard': 'Unchecked', 'approval': 'NoApproval', 'role': 'Public', 'retry': 2, 'needs_tool': False, 'citation_ok': False, 'audit': True}, '1': {'phase': 'DNew', 'evidence': 'Missing', 'guard': 'Unchecked', 'approval': 'NoApproval', 'role': 'Public', 'retry': 2, 'needs_tool': False, 'citation_ok': False, 'audit': False}}})
    adapter.step('enqueue_router', {'r': 0})
    _assert_partial_expected(adapter.observe(), {'d': {'0': {'phase': 'DRouterQueued', 'evidence': 'Missing', 'guard': 'Unchecked', 'approval': 'NoApproval', 'role': 'Public', 'retry': 2, 'needs_tool': False, 'citation_ok': False, 'audit': True}, '1': {'phase': 'DNew', 'evidence': 'Missing', 'guard': 'Unchecked', 'approval': 'NoApproval', 'role': 'Public', 'retry': 2, 'needs_tool': False, 'citation_ok': False, 'audit': False}}})
    adapter.step('route_tool', {'r': 0})
    _assert_partial_expected(adapter.observe(), {'d': {'0': {'phase': 'DClassified', 'evidence': 'Missing', 'guard': 'Unchecked', 'approval': 'NoApproval', 'role': 'Public', 'retry': 2, 'needs_tool': True, 'citation_ok': False, 'audit': True}, '1': {'phase': 'DNew', 'evidence': 'Missing', 'guard': 'Unchecked', 'approval': 'NoApproval', 'role': 'Public', 'retry': 2, 'needs_tool': False, 'citation_ok': False, 'audit': False}}})

def test_random_walk_conformance(adapter):
    if not _adapter_ready(adapter):
        pytest.skip('Adapter not implemented')
    mon = Monitor(SPEC_PATH)
    mon.reset()
    adapter.reset()
    assert adapter.observe() == mon.state
    rng = random.Random(0)
    for _ in range(100):
        enabled = mon.enabled()
        if not enabled:
            break
        choice = enabled[rng.randrange(len(enabled))]
        action, params = choice['action'], dict(choice['params'])
        adapter.step(action, params)
        result = mon.step(action, params)
        if not result.get('ok'):
            pytest.fail(
                f'spec oracle violation at {action} {params}: '
                f"{result.get('kind')} {result.get('name', '')}"
            )
        assert adapter.observe() == mon.state

