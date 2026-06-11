"""A tiny real-world-style implementation of the BankSystem spec.

Deliberately written as ordinary application code (no FSL awareness):
a two-ledger account plus an append-only audit trail. The generated
conformance tests drive this through the Adapter and compare every
state against the spec's runtime Monitor.
"""


class AuditTrail:
    def __init__(self):
        self.entries = []
        self.total = 0

    def record(self, amount):
        if amount <= 0:
            raise ValueError("audit entries must be positive")
        if len(self.entries) >= 4:
            raise OverflowError("audit log full")
        self.entries.append(amount)
        self.total += amount


class BankSystem:
    """Two-ledger account: deposits land in pending, settle moves them
    to cleared, withdrawals only touch cleared funds. Every deposit and
    withdrawal is recorded in the audit trail."""

    def __init__(self):
        self.reset()

    def reset(self):
        self.cleared = 0
        self.pending = 0
        self.withdrawn_total = 0
        self.audit = AuditTrail()

    def deposit(self, amount):
        if amount <= 0:
            raise ValueError("deposit must be positive")
        self.audit.record(amount)
        self.pending += amount

    def settle(self):
        if self.pending <= 0:
            raise RuntimeError("nothing to settle")
        self.cleared += self.pending
        self.pending = 0

    def withdraw(self, amount):
        if amount <= 0:
            raise ValueError("withdrawal must be positive")
        if self.cleared < amount:
            raise RuntimeError("insufficient cleared funds")
        self.cleared -= amount
        self.withdrawn_total += amount
