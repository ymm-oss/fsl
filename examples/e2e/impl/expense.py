"""Plain Python implementation for the e2e expense example.

The module deliberately knows nothing about FSL. The generated
conformance tests drive it through an Adapter and compare its state to
examples/e2e/3_design.fsl.
"""


AUTO_LIMIT = 1
OUTBOX_CAP = 3


class ExpenseSystem:
    def __init__(self):
        self.reset()

    def reset(self):
        self.claims = {
            0: {"st": "DesignDraft", "amount": 0},
            1: {"st": "DesignDraft", "amount": 0},
            2: {"st": "DesignDraft", "amount": 0},
        }
        self.paid_count = 0
        self.outbox = []

    def submit_small(self, claim, amount):
        record = self.claims[claim]
        if record["st"] != "DesignDraft":
            raise RuntimeError("claim is not draft")
        if amount <= 0 or amount > AUTO_LIMIT:
            raise ValueError("amount is not in the small lane")
        record["st"] = "DesignAutoReview"
        record["amount"] = amount

    def submit_large(self, claim, amount):
        record = self.claims[claim]
        if record["st"] != "DesignDraft":
            raise RuntimeError("claim is not draft")
        if amount <= AUTO_LIMIT:
            raise ValueError("amount is not in the manager lane")
        record["st"] = "DesignManagerReview"
        record["amount"] = amount

    def auto_approve(self, claim):
        record = self.claims[claim]
        if record["st"] != "DesignAutoReview":
            raise RuntimeError("claim is not waiting for auto approval")
        if record["amount"] > AUTO_LIMIT:
            raise RuntimeError("claim exceeds auto approval limit")
        record["st"] = "DesignAutoApproved"

    def mgr_approve(self, claim):
        record = self.claims[claim]
        if record["st"] != "DesignManagerReview":
            raise RuntimeError("claim is not waiting for manager review")
        if record["amount"] <= AUTO_LIMIT:
            raise RuntimeError("claim does not require manager approval")
        record["st"] = "DesignManagerApproved"

    def mgr_reject(self, claim):
        record = self.claims[claim]
        if record["st"] != "DesignManagerReview":
            raise RuntimeError("claim is not waiting for manager review")
        if record["amount"] <= AUTO_LIMIT:
            raise RuntimeError("claim does not require manager approval")
        record["st"] = "DesignRejected"

    def pay_submit(self, claim):
        record = self.claims[claim]
        if record["st"] not in ("DesignAutoApproved", "DesignManagerApproved"):
            raise RuntimeError("claim is not approved")
        if len(self.outbox) >= OUTBOX_CAP:
            raise OverflowError("outbox is full")
        record["st"] = "DesignPaymentSubmitted"
        self.paid_count += 1
        self.outbox.append(claim)

    def pay_confirm(self, claim):
        record = self.claims[claim]
        if record["st"] != "DesignPaymentSubmitted":
            raise RuntimeError("payment has not been submitted")
        record["st"] = "DesignPaid"

    def outbox_send(self):
        if not self.outbox:
            raise RuntimeError("outbox is empty")
        self.outbox.pop(0)
