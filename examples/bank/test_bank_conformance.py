"""Auto-generated conformance tests for FSL spec.
Source: bank_system.fsl
Connect Adapter to your implementation, or use MonitorSelfAdapter for self-check.
"""
import random
from pathlib import Path

import pytest

from fslc.runtime import Monitor

SPEC_PATH = Path(__file__).resolve().parent / '../../specs/bank_system.fsl'


class Adapter:
    """Wires examples/bank/bank.py (a plain Python implementation with
    no FSL awareness) to the BankSystem spec actions/state."""

    def reset(self):
        from bank import BankSystem
        self.impl = BankSystem()

    def step(self, action: str, params: dict):
        if action == "deposit_audited":
            self.impl.deposit(params["a"])
        elif action == "withdraw_audited":
            self.impl.withdraw(params["a"])
        elif action == "bank.settle":
            self.impl.settle()
        else:
            raise ValueError(f"unknown action {action}")

    def observe(self) -> dict:
        return {
            "bank.cleared": self.impl.cleared,
            "bank.pending": self.impl.pending,
            "audit.balance": self.impl.audit.total,
            "audit.log": list(self.impl.audit.entries),
            "withdrawn": self.impl.withdrawn_total,
        }


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


def test_scenario_reach_bank_Settled(adapter):
    'Scenario: reach_bank.Settled'
    if not _adapter_ready(adapter):
        pytest.skip('Adapter not implemented')
    adapter.reset()
    adapter.step('deposit_audited', {'a': 3})
    _assert_partial_expected(adapter.observe(), {'bank.cleared': 0, 'bank.pending': 3, 'audit.balance': 3, 'audit.log': [3], 'withdrawn': 0})
    adapter.step('bank.settle', {})
    _assert_partial_expected(adapter.observe(), {'bank.cleared': 3, 'bank.pending': 0, 'audit.balance': 3, 'audit.log': [3], 'withdrawn': 0})

def test_scenario_reach_audit_LogFull(adapter):
    'Scenario: reach_audit.LogFull'
    if not _adapter_ready(adapter):
        pytest.skip('Adapter not implemented')
    adapter.reset()
    adapter.step('deposit_audited', {'a': 3})
    _assert_partial_expected(adapter.observe(), {'bank.cleared': 0, 'bank.pending': 3, 'audit.balance': 3, 'audit.log': [3], 'withdrawn': 0})
    adapter.step('deposit_audited', {'a': 3})
    _assert_partial_expected(adapter.observe(), {'bank.cleared': 0, 'bank.pending': 6, 'audit.balance': 6, 'audit.log': [3, 3], 'withdrawn': 0})
    adapter.step('deposit_audited', {'a': 3})
    _assert_partial_expected(adapter.observe(), {'bank.cleared': 0, 'bank.pending': 9, 'audit.balance': 9, 'audit.log': [3, 3, 3], 'withdrawn': 0})
    adapter.step('deposit_audited', {'a': 1})
    _assert_partial_expected(adapter.observe(), {'bank.cleared': 0, 'bank.pending': 10, 'audit.balance': 10, 'audit.log': [3, 3, 3, 1], 'withdrawn': 0})

def test_scenario_reach_FullCycle(adapter):
    'Scenario: reach_FullCycle'
    if not _adapter_ready(adapter):
        pytest.skip('Adapter not implemented')
    adapter.reset()
    adapter.step('deposit_audited', {'a': 3})
    _assert_partial_expected(adapter.observe(), {'bank.cleared': 0, 'bank.pending': 3, 'audit.balance': 3, 'audit.log': [3], 'withdrawn': 0})
    adapter.step('deposit_audited', {'a': 3})
    _assert_partial_expected(adapter.observe(), {'bank.cleared': 0, 'bank.pending': 6, 'audit.balance': 6, 'audit.log': [3, 3], 'withdrawn': 0})
    adapter.step('bank.settle', {})
    _assert_partial_expected(adapter.observe(), {'bank.cleared': 6, 'bank.pending': 0, 'audit.balance': 6, 'audit.log': [3, 3], 'withdrawn': 0})
    adapter.step('withdraw_audited', {'a': 2})
    _assert_partial_expected(adapter.observe(), {'bank.cleared': 4, 'bank.pending': 0, 'audit.balance': 6, 'audit.log': [3, 3], 'withdrawn': 2})

def test_scenario_cover_bank_settle(adapter):
    'Scenario: cover_bank.settle'
    if not _adapter_ready(adapter):
        pytest.skip('Adapter not implemented')
    adapter.reset()
    adapter.step('deposit_audited', {'a': 3})
    _assert_partial_expected(adapter.observe(), {'bank.cleared': 0, 'bank.pending': 3, 'audit.balance': 3, 'audit.log': [3], 'withdrawn': 0})
    adapter.step('bank.settle', {})
    _assert_partial_expected(adapter.observe(), {'bank.cleared': 3, 'bank.pending': 0, 'audit.balance': 3, 'audit.log': [3], 'withdrawn': 0})

def test_scenario_cover_deposit_audited(adapter):
    'Scenario: cover_deposit_audited'
    if not _adapter_ready(adapter):
        pytest.skip('Adapter not implemented')
    adapter.reset()
    adapter.step('deposit_audited', {'a': 1})
    _assert_partial_expected(adapter.observe(), {'bank.cleared': 0, 'bank.pending': 1, 'audit.balance': 1, 'audit.log': [1], 'withdrawn': 0})

def test_scenario_cover_withdraw_audited(adapter):
    'Scenario: cover_withdraw_audited'
    if not _adapter_ready(adapter):
        pytest.skip('Adapter not implemented')
    adapter.reset()
    adapter.step('deposit_audited', {'a': 3})
    _assert_partial_expected(adapter.observe(), {'bank.cleared': 0, 'bank.pending': 3, 'audit.balance': 3, 'audit.log': [3], 'withdrawn': 0})
    adapter.step('bank.settle', {})
    _assert_partial_expected(adapter.observe(), {'bank.cleared': 3, 'bank.pending': 0, 'audit.balance': 3, 'audit.log': [3], 'withdrawn': 0})
    adapter.step('withdraw_audited', {'a': 1})
    _assert_partial_expected(adapter.observe(), {'bank.cleared': 2, 'bank.pending': 0, 'audit.balance': 3, 'audit.log': [3], 'withdrawn': 1})

def test_scenario_deadlock_terminal(adapter):
    'Scenario: deadlock_terminal'
    if not _adapter_ready(adapter):
        pytest.skip('Adapter not implemented')
    adapter.reset()
    adapter.step('deposit_audited', {'a': 1})
    _assert_partial_expected(adapter.observe(), {'bank.cleared': 0, 'bank.pending': 1, 'audit.balance': 1, 'audit.log': [1], 'withdrawn': 0})
    adapter.step('deposit_audited', {'a': 2})
    _assert_partial_expected(adapter.observe(), {'bank.cleared': 0, 'bank.pending': 3, 'audit.balance': 3, 'audit.log': [1, 2], 'withdrawn': 0})
    adapter.step('deposit_audited', {'a': 2})
    _assert_partial_expected(adapter.observe(), {'bank.cleared': 0, 'bank.pending': 5, 'audit.balance': 5, 'audit.log': [1, 2, 2], 'withdrawn': 0})
    adapter.step('deposit_audited', {'a': 1})
    _assert_partial_expected(adapter.observe(), {'bank.cleared': 0, 'bank.pending': 6, 'audit.balance': 6, 'audit.log': [1, 2, 2, 1], 'withdrawn': 0})
    adapter.step('bank.settle', {})
    _assert_partial_expected(adapter.observe(), {'bank.cleared': 6, 'bank.pending': 0, 'audit.balance': 6, 'audit.log': [1, 2, 2, 1], 'withdrawn': 0})
    adapter.step('withdraw_audited', {'a': 3})
    _assert_partial_expected(adapter.observe(), {'bank.cleared': 3, 'bank.pending': 0, 'audit.balance': 6, 'audit.log': [1, 2, 2, 1], 'withdrawn': 3})
    adapter.step('withdraw_audited', {'a': 3})
    _assert_partial_expected(adapter.observe(), {'bank.cleared': 0, 'bank.pending': 0, 'audit.balance': 6, 'audit.log': [1, 2, 2, 1], 'withdrawn': 6})

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
