"""Auto-generated conformance tests for FSL spec.
Source: 3_design.fsl
Connect Adapter to your implementation, or use MonitorSelfAdapter for self-check.
"""
import random
from pathlib import Path

import pytest

from fslc.runtime import Monitor

SPEC_PATH = Path(__file__).resolve().parent / '../3_design.fsl'


class Adapter:
    """Wires expense.py, a plain Python implementation, to ExpenseDesign."""

    def reset(self):
        from expense import ExpenseSystem
        self.impl = ExpenseSystem()

    def step(self, action: str, params: dict):
        if action == "submit_small":
            self.impl.submit_small(params["c"], params["a"])
        elif action == "submit_large":
            self.impl.submit_large(params["c"], params["a"])
        elif action == "auto_approve":
            self.impl.auto_approve(params["c"])
        elif action == "mgr_approve":
            self.impl.mgr_approve(params["c"])
        elif action == "mgr_reject":
            self.impl.mgr_reject(params["c"])
        elif action == "pay_submit":
            self.impl.pay_submit(params["c"])
        elif action == "pay_confirm":
            self.impl.pay_confirm(params["c"])
        elif action == "outbox_send":
            self.impl.outbox_send()
        else:
            raise ValueError(f"unknown action {action}")

    def observe(self) -> dict:
        return {
            "design": {
                str(c): {
                    "st": record["st"],
                    "amount": record["amount"],
                }
                for c, record in self.impl.claims.items()
            },
            "paid_count": self.impl.paid_count,
            "outbox": list(self.impl.outbox),
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


def test_scenario_cover_submit_small(adapter):
    'Scenario: cover_submit_small'
    if not _adapter_ready(adapter):
        pytest.skip('Adapter not implemented')
    adapter.reset()
    adapter.step('submit_small', {'c': 0, 'a': 1})
    _assert_partial_expected(adapter.observe(), {'design': {'0': {'st': 'DesignAutoReview', 'amount': 1}, '1': {'st': 'DesignDraft', 'amount': 0}, '2': {'st': 'DesignDraft', 'amount': 0}}, 'paid_count': 0, 'outbox': []})

def test_scenario_cover_submit_large(adapter):
    'Scenario: cover_submit_large'
    if not _adapter_ready(adapter):
        pytest.skip('Adapter not implemented')
    adapter.reset()
    adapter.step('submit_large', {'c': 0, 'a': 2})
    _assert_partial_expected(adapter.observe(), {'design': {'0': {'st': 'DesignManagerReview', 'amount': 2}, '1': {'st': 'DesignDraft', 'amount': 0}, '2': {'st': 'DesignDraft', 'amount': 0}}, 'paid_count': 0, 'outbox': []})

def test_scenario_cover_auto_approve(adapter):
    'Scenario: cover_auto_approve'
    if not _adapter_ready(adapter):
        pytest.skip('Adapter not implemented')
    adapter.reset()
    adapter.step('submit_small', {'c': 0, 'a': 1})
    _assert_partial_expected(adapter.observe(), {'design': {'0': {'st': 'DesignAutoReview', 'amount': 1}, '1': {'st': 'DesignDraft', 'amount': 0}, '2': {'st': 'DesignDraft', 'amount': 0}}, 'paid_count': 0, 'outbox': []})
    adapter.step('auto_approve', {'c': 0})
    _assert_partial_expected(adapter.observe(), {'design': {'0': {'st': 'DesignAutoApproved', 'amount': 1}, '1': {'st': 'DesignDraft', 'amount': 0}, '2': {'st': 'DesignDraft', 'amount': 0}}, 'paid_count': 0, 'outbox': []})

def test_scenario_cover_mgr_approve(adapter):
    'Scenario: cover_mgr_approve'
    if not _adapter_ready(adapter):
        pytest.skip('Adapter not implemented')
    adapter.reset()
    adapter.step('submit_large', {'c': 0, 'a': 3})
    _assert_partial_expected(adapter.observe(), {'design': {'0': {'st': 'DesignManagerReview', 'amount': 3}, '1': {'st': 'DesignDraft', 'amount': 0}, '2': {'st': 'DesignDraft', 'amount': 0}}, 'paid_count': 0, 'outbox': []})
    adapter.step('mgr_approve', {'c': 0})
    _assert_partial_expected(adapter.observe(), {'design': {'0': {'st': 'DesignManagerApproved', 'amount': 3}, '1': {'st': 'DesignDraft', 'amount': 0}, '2': {'st': 'DesignDraft', 'amount': 0}}, 'paid_count': 0, 'outbox': []})

def test_scenario_cover_mgr_reject(adapter):
    'Scenario: cover_mgr_reject'
    if not _adapter_ready(adapter):
        pytest.skip('Adapter not implemented')
    adapter.reset()
    adapter.step('submit_large', {'c': 0, 'a': 3})
    _assert_partial_expected(adapter.observe(), {'design': {'0': {'st': 'DesignManagerReview', 'amount': 3}, '1': {'st': 'DesignDraft', 'amount': 0}, '2': {'st': 'DesignDraft', 'amount': 0}}, 'paid_count': 0, 'outbox': []})
    adapter.step('mgr_reject', {'c': 0})
    _assert_partial_expected(adapter.observe(), {'design': {'0': {'st': 'DesignRejected', 'amount': 3}, '1': {'st': 'DesignDraft', 'amount': 0}, '2': {'st': 'DesignDraft', 'amount': 0}}, 'paid_count': 0, 'outbox': []})

def test_scenario_cover_pay_submit(adapter):
    'Scenario: cover_pay_submit'
    if not _adapter_ready(adapter):
        pytest.skip('Adapter not implemented')
    adapter.reset()
    adapter.step('submit_large', {'c': 0, 'a': 3})
    _assert_partial_expected(adapter.observe(), {'design': {'0': {'st': 'DesignManagerReview', 'amount': 3}, '1': {'st': 'DesignDraft', 'amount': 0}, '2': {'st': 'DesignDraft', 'amount': 0}}, 'paid_count': 0, 'outbox': []})
    adapter.step('mgr_approve', {'c': 0})
    _assert_partial_expected(adapter.observe(), {'design': {'0': {'st': 'DesignManagerApproved', 'amount': 3}, '1': {'st': 'DesignDraft', 'amount': 0}, '2': {'st': 'DesignDraft', 'amount': 0}}, 'paid_count': 0, 'outbox': []})
    adapter.step('pay_submit', {'c': 0})
    _assert_partial_expected(adapter.observe(), {'design': {'0': {'st': 'DesignPaymentSubmitted', 'amount': 3}, '1': {'st': 'DesignDraft', 'amount': 0}, '2': {'st': 'DesignDraft', 'amount': 0}}, 'paid_count': 1, 'outbox': [0]})

def test_scenario_cover_pay_confirm(adapter):
    'Scenario: cover_pay_confirm'
    if not _adapter_ready(adapter):
        pytest.skip('Adapter not implemented')
    adapter.reset()
    adapter.step('submit_small', {'c': 0, 'a': 1})
    _assert_partial_expected(adapter.observe(), {'design': {'0': {'st': 'DesignAutoReview', 'amount': 1}, '1': {'st': 'DesignDraft', 'amount': 0}, '2': {'st': 'DesignDraft', 'amount': 0}}, 'paid_count': 0, 'outbox': []})
    adapter.step('auto_approve', {'c': 0})
    _assert_partial_expected(adapter.observe(), {'design': {'0': {'st': 'DesignAutoApproved', 'amount': 1}, '1': {'st': 'DesignDraft', 'amount': 0}, '2': {'st': 'DesignDraft', 'amount': 0}}, 'paid_count': 0, 'outbox': []})
    adapter.step('pay_submit', {'c': 0})
    _assert_partial_expected(adapter.observe(), {'design': {'0': {'st': 'DesignPaymentSubmitted', 'amount': 1}, '1': {'st': 'DesignDraft', 'amount': 0}, '2': {'st': 'DesignDraft', 'amount': 0}}, 'paid_count': 1, 'outbox': [0]})
    adapter.step('pay_confirm', {'c': 0})
    _assert_partial_expected(adapter.observe(), {'design': {'0': {'st': 'DesignPaid', 'amount': 1}, '1': {'st': 'DesignDraft', 'amount': 0}, '2': {'st': 'DesignDraft', 'amount': 0}}, 'paid_count': 1, 'outbox': [0]})

def test_scenario_cover_outbox_send(adapter):
    'Scenario: cover_outbox_send'
    if not _adapter_ready(adapter):
        pytest.skip('Adapter not implemented')
    adapter.reset()
    adapter.step('submit_large', {'c': 0, 'a': 3})
    _assert_partial_expected(adapter.observe(), {'design': {'0': {'st': 'DesignManagerReview', 'amount': 3}, '1': {'st': 'DesignDraft', 'amount': 0}, '2': {'st': 'DesignDraft', 'amount': 0}}, 'paid_count': 0, 'outbox': []})
    adapter.step('mgr_approve', {'c': 0})
    _assert_partial_expected(adapter.observe(), {'design': {'0': {'st': 'DesignManagerApproved', 'amount': 3}, '1': {'st': 'DesignDraft', 'amount': 0}, '2': {'st': 'DesignDraft', 'amount': 0}}, 'paid_count': 0, 'outbox': []})
    adapter.step('pay_submit', {'c': 0})
    _assert_partial_expected(adapter.observe(), {'design': {'0': {'st': 'DesignPaymentSubmitted', 'amount': 3}, '1': {'st': 'DesignDraft', 'amount': 0}, '2': {'st': 'DesignDraft', 'amount': 0}}, 'paid_count': 1, 'outbox': [0]})
    adapter.step('outbox_send', {})
    _assert_partial_expected(adapter.observe(), {'design': {'0': {'st': 'DesignPaymentSubmitted', 'amount': 3}, '1': {'st': 'DesignDraft', 'amount': 0}, '2': {'st': 'DesignDraft', 'amount': 0}}, 'paid_count': 1, 'outbox': []})

def test_scenario_deadlock_terminal(adapter):
    'Scenario: deadlock_terminal'
    if not _adapter_ready(adapter):
        pytest.skip('Adapter not implemented')
    adapter.reset()
    adapter.step('submit_large', {'c': 1, 'a': 2})
    _assert_partial_expected(adapter.observe(), {'design': {'0': {'st': 'DesignDraft', 'amount': 0}, '1': {'st': 'DesignManagerReview', 'amount': 2}, '2': {'st': 'DesignDraft', 'amount': 0}}, 'paid_count': 0, 'outbox': []})
    adapter.step('submit_large', {'c': 0, 'a': 2})
    _assert_partial_expected(adapter.observe(), {'design': {'0': {'st': 'DesignManagerReview', 'amount': 2}, '1': {'st': 'DesignManagerReview', 'amount': 2}, '2': {'st': 'DesignDraft', 'amount': 0}}, 'paid_count': 0, 'outbox': []})
    adapter.step('mgr_reject', {'c': 1})
    _assert_partial_expected(adapter.observe(), {'design': {'0': {'st': 'DesignManagerReview', 'amount': 2}, '1': {'st': 'DesignRejected', 'amount': 2}, '2': {'st': 'DesignDraft', 'amount': 0}}, 'paid_count': 0, 'outbox': []})
    adapter.step('mgr_reject', {'c': 0})
    _assert_partial_expected(adapter.observe(), {'design': {'0': {'st': 'DesignRejected', 'amount': 2}, '1': {'st': 'DesignRejected', 'amount': 2}, '2': {'st': 'DesignDraft', 'amount': 0}}, 'paid_count': 0, 'outbox': []})
    adapter.step('submit_large', {'c': 2, 'a': 2})
    _assert_partial_expected(adapter.observe(), {'design': {'0': {'st': 'DesignRejected', 'amount': 2}, '1': {'st': 'DesignRejected', 'amount': 2}, '2': {'st': 'DesignManagerReview', 'amount': 2}}, 'paid_count': 0, 'outbox': []})
    adapter.step('mgr_reject', {'c': 2})
    _assert_partial_expected(adapter.observe(), {'design': {'0': {'st': 'DesignRejected', 'amount': 2}, '1': {'st': 'DesignRejected', 'amount': 2}, '2': {'st': 'DesignRejected', 'amount': 2}}, 'paid_count': 0, 'outbox': []})

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
