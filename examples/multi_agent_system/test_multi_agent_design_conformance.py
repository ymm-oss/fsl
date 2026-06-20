"""Auto-generated conformance tests for FSL spec.
Source: multi_agent_design.fsl
Connect Adapter to your implementation, or use MonitorSelfAdapter for self-check.
"""
import random
from pathlib import Path

import pytest

from fslc.runtime import Monitor

SPEC_PATH = Path(__file__).resolve().parent / 'multi_agent_design.fsl'


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


def test_scenario_cover_accept_work(adapter):
    'Scenario: cover_accept_work'
    if not _adapter_ready(adapter):
        pytest.skip('Adapter not implemented')
    adapter.reset()
    adapter.step('accept_work', {'w': 0})
    _assert_partial_expected(adapter.observe(), {'d': {'0': {'phase': 'DAccepted', 'risk': 'Unknown', 'worker_a_done': False, 'worker_b_done': False, 'critic_ok': False, 'approval': 'NoApproval', 'audit': False, 'revision': 0}}, 'active_agents': [], 'planner_q': [], 'worker_q': [], 'critic_q': [], 'tool_q': []})

def test_scenario_cover_write_audit(adapter):
    'Scenario: cover_write_audit'
    if not _adapter_ready(adapter):
        pytest.skip('Adapter not implemented')
    adapter.reset()
    adapter.step('accept_work', {'w': 0})
    _assert_partial_expected(adapter.observe(), {'d': {'0': {'phase': 'DAccepted', 'risk': 'Unknown', 'worker_a_done': False, 'worker_b_done': False, 'critic_ok': False, 'approval': 'NoApproval', 'audit': False, 'revision': 0}}, 'active_agents': [], 'planner_q': [], 'worker_q': [], 'critic_q': [], 'tool_q': []})
    adapter.step('write_audit', {'w': 0})
    _assert_partial_expected(adapter.observe(), {'d': {'0': {'phase': 'DAudited', 'risk': 'Unknown', 'worker_a_done': False, 'worker_b_done': False, 'critic_ok': False, 'approval': 'NoApproval', 'audit': True, 'revision': 0}}, 'active_agents': [], 'planner_q': [], 'worker_q': [], 'critic_q': [], 'tool_q': []})

def test_scenario_cover_classify_low(adapter):
    'Scenario: cover_classify_low'
    if not _adapter_ready(adapter):
        pytest.skip('Adapter not implemented')
    adapter.reset()
    adapter.step('accept_work', {'w': 0})
    _assert_partial_expected(adapter.observe(), {'d': {'0': {'phase': 'DAccepted', 'risk': 'Unknown', 'worker_a_done': False, 'worker_b_done': False, 'critic_ok': False, 'approval': 'NoApproval', 'audit': False, 'revision': 0}}, 'active_agents': [], 'planner_q': [], 'worker_q': [], 'critic_q': [], 'tool_q': []})
    adapter.step('write_audit', {'w': 0})
    _assert_partial_expected(adapter.observe(), {'d': {'0': {'phase': 'DAudited', 'risk': 'Unknown', 'worker_a_done': False, 'worker_b_done': False, 'critic_ok': False, 'approval': 'NoApproval', 'audit': True, 'revision': 0}}, 'active_agents': [], 'planner_q': [], 'worker_q': [], 'critic_q': [], 'tool_q': []})
    adapter.step('classify_low', {'w': 0})
    _assert_partial_expected(adapter.observe(), {'d': {'0': {'phase': 'DScoped', 'risk': 'Low', 'worker_a_done': False, 'worker_b_done': False, 'critic_ok': False, 'approval': 'NoApproval', 'audit': True, 'revision': 0}}, 'active_agents': [], 'planner_q': [], 'worker_q': [], 'critic_q': [], 'tool_q': []})

def test_scenario_cover_classify_high(adapter):
    'Scenario: cover_classify_high'
    if not _adapter_ready(adapter):
        pytest.skip('Adapter not implemented')
    adapter.reset()
    adapter.step('accept_work', {'w': 0})
    _assert_partial_expected(adapter.observe(), {'d': {'0': {'phase': 'DAccepted', 'risk': 'Unknown', 'worker_a_done': False, 'worker_b_done': False, 'critic_ok': False, 'approval': 'NoApproval', 'audit': False, 'revision': 0}}, 'active_agents': [], 'planner_q': [], 'worker_q': [], 'critic_q': [], 'tool_q': []})
    adapter.step('write_audit', {'w': 0})
    _assert_partial_expected(adapter.observe(), {'d': {'0': {'phase': 'DAudited', 'risk': 'Unknown', 'worker_a_done': False, 'worker_b_done': False, 'critic_ok': False, 'approval': 'NoApproval', 'audit': True, 'revision': 0}}, 'active_agents': [], 'planner_q': [], 'worker_q': [], 'critic_q': [], 'tool_q': []})
    adapter.step('classify_high', {'w': 0})
    _assert_partial_expected(adapter.observe(), {'d': {'0': {'phase': 'DScoped', 'risk': 'High', 'worker_a_done': False, 'worker_b_done': False, 'critic_ok': False, 'approval': 'NoApproval', 'audit': True, 'revision': 0}}, 'active_agents': [], 'planner_q': [], 'worker_q': [], 'critic_q': [], 'tool_q': []})

def test_scenario_cover_enqueue_plan(adapter):
    'Scenario: cover_enqueue_plan'
    if not _adapter_ready(adapter):
        pytest.skip('Adapter not implemented')
    adapter.reset()
    adapter.step('accept_work', {'w': 0})
    _assert_partial_expected(adapter.observe(), {'d': {'0': {'phase': 'DAccepted', 'risk': 'Unknown', 'worker_a_done': False, 'worker_b_done': False, 'critic_ok': False, 'approval': 'NoApproval', 'audit': False, 'revision': 0}}, 'active_agents': [], 'planner_q': [], 'worker_q': [], 'critic_q': [], 'tool_q': []})
    adapter.step('write_audit', {'w': 0})
    _assert_partial_expected(adapter.observe(), {'d': {'0': {'phase': 'DAudited', 'risk': 'Unknown', 'worker_a_done': False, 'worker_b_done': False, 'critic_ok': False, 'approval': 'NoApproval', 'audit': True, 'revision': 0}}, 'active_agents': [], 'planner_q': [], 'worker_q': [], 'critic_q': [], 'tool_q': []})
    adapter.step('classify_high', {'w': 0})
    _assert_partial_expected(adapter.observe(), {'d': {'0': {'phase': 'DScoped', 'risk': 'High', 'worker_a_done': False, 'worker_b_done': False, 'critic_ok': False, 'approval': 'NoApproval', 'audit': True, 'revision': 0}}, 'active_agents': [], 'planner_q': [], 'worker_q': [], 'critic_q': [], 'tool_q': []})
    adapter.step('enqueue_plan', {'w': 0})
    _assert_partial_expected(adapter.observe(), {'d': {'0': {'phase': 'DPlanQueued', 'risk': 'High', 'worker_a_done': False, 'worker_b_done': False, 'critic_ok': False, 'approval': 'NoApproval', 'audit': True, 'revision': 0}}, 'active_agents': [], 'planner_q': [0], 'worker_q': [], 'critic_q': [], 'tool_q': []})

def test_scenario_cover_start_planner(adapter):
    'Scenario: cover_start_planner'
    if not _adapter_ready(adapter):
        pytest.skip('Adapter not implemented')
    adapter.reset()
    adapter.step('accept_work', {'w': 0})
    _assert_partial_expected(adapter.observe(), {'d': {'0': {'phase': 'DAccepted', 'risk': 'Unknown', 'worker_a_done': False, 'worker_b_done': False, 'critic_ok': False, 'approval': 'NoApproval', 'audit': False, 'revision': 0}}, 'active_agents': [], 'planner_q': [], 'worker_q': [], 'critic_q': [], 'tool_q': []})
    adapter.step('write_audit', {'w': 0})
    _assert_partial_expected(adapter.observe(), {'d': {'0': {'phase': 'DAudited', 'risk': 'Unknown', 'worker_a_done': False, 'worker_b_done': False, 'critic_ok': False, 'approval': 'NoApproval', 'audit': True, 'revision': 0}}, 'active_agents': [], 'planner_q': [], 'worker_q': [], 'critic_q': [], 'tool_q': []})
    adapter.step('classify_high', {'w': 0})
    _assert_partial_expected(adapter.observe(), {'d': {'0': {'phase': 'DScoped', 'risk': 'High', 'worker_a_done': False, 'worker_b_done': False, 'critic_ok': False, 'approval': 'NoApproval', 'audit': True, 'revision': 0}}, 'active_agents': [], 'planner_q': [], 'worker_q': [], 'critic_q': [], 'tool_q': []})
    adapter.step('enqueue_plan', {'w': 0})
    _assert_partial_expected(adapter.observe(), {'d': {'0': {'phase': 'DPlanQueued', 'risk': 'High', 'worker_a_done': False, 'worker_b_done': False, 'critic_ok': False, 'approval': 'NoApproval', 'audit': True, 'revision': 0}}, 'active_agents': [], 'planner_q': [0], 'worker_q': [], 'critic_q': [], 'tool_q': []})
    adapter.step('start_planner', {'w': 0})
    _assert_partial_expected(adapter.observe(), {'d': {'0': {'phase': 'DPlanning', 'risk': 'High', 'worker_a_done': False, 'worker_b_done': False, 'critic_ok': False, 'approval': 'NoApproval', 'audit': True, 'revision': 0}}, 'active_agents': [], 'planner_q': [], 'worker_q': [], 'critic_q': [], 'tool_q': []})

def test_scenario_cover_finish_plan(adapter):
    'Scenario: cover_finish_plan'
    if not _adapter_ready(adapter):
        pytest.skip('Adapter not implemented')
    adapter.reset()
    adapter.step('accept_work', {'w': 0})
    _assert_partial_expected(adapter.observe(), {'d': {'0': {'phase': 'DAccepted', 'risk': 'Unknown', 'worker_a_done': False, 'worker_b_done': False, 'critic_ok': False, 'approval': 'NoApproval', 'audit': False, 'revision': 0}}, 'active_agents': [], 'planner_q': [], 'worker_q': [], 'critic_q': [], 'tool_q': []})
    adapter.step('write_audit', {'w': 0})
    _assert_partial_expected(adapter.observe(), {'d': {'0': {'phase': 'DAudited', 'risk': 'Unknown', 'worker_a_done': False, 'worker_b_done': False, 'critic_ok': False, 'approval': 'NoApproval', 'audit': True, 'revision': 0}}, 'active_agents': [], 'planner_q': [], 'worker_q': [], 'critic_q': [], 'tool_q': []})
    adapter.step('classify_high', {'w': 0})
    _assert_partial_expected(adapter.observe(), {'d': {'0': {'phase': 'DScoped', 'risk': 'High', 'worker_a_done': False, 'worker_b_done': False, 'critic_ok': False, 'approval': 'NoApproval', 'audit': True, 'revision': 0}}, 'active_agents': [], 'planner_q': [], 'worker_q': [], 'critic_q': [], 'tool_q': []})
    adapter.step('enqueue_plan', {'w': 0})
    _assert_partial_expected(adapter.observe(), {'d': {'0': {'phase': 'DPlanQueued', 'risk': 'High', 'worker_a_done': False, 'worker_b_done': False, 'critic_ok': False, 'approval': 'NoApproval', 'audit': True, 'revision': 0}}, 'active_agents': [], 'planner_q': [0], 'worker_q': [], 'critic_q': [], 'tool_q': []})
    adapter.step('start_planner', {'w': 0})
    _assert_partial_expected(adapter.observe(), {'d': {'0': {'phase': 'DPlanning', 'risk': 'High', 'worker_a_done': False, 'worker_b_done': False, 'critic_ok': False, 'approval': 'NoApproval', 'audit': True, 'revision': 0}}, 'active_agents': [], 'planner_q': [], 'worker_q': [], 'critic_q': [], 'tool_q': []})
    adapter.step('finish_plan', {'w': 0})
    _assert_partial_expected(adapter.observe(), {'d': {'0': {'phase': 'DPlanned', 'risk': 'High', 'worker_a_done': False, 'worker_b_done': False, 'critic_ok': False, 'approval': 'NoApproval', 'audit': True, 'revision': 0}}, 'active_agents': [], 'planner_q': [], 'worker_q': [], 'critic_q': [], 'tool_q': []})

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

