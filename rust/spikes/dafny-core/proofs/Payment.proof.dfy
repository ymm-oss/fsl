// SPDX-License-Identifier: Apache-2.0
// Written by a model through prove.mjs; accepted only because build.sh's gates pass.

include "../out/Payment.gen.dfy"

module PaymentProof refines PaymentSpec {

  function Contrib(x: Pmt): int {
    if x.st == PSt.Captured then x.amount - x.refunded else 0
  }

  lemma SumUpdate(c: Config, s1: State, s2: State, p: int, i: int)
    requires Wf(c, s1) && Wf(c, s2) && 0 <= p < c.N_PayId && 0 <= i
    requires forall k :: 0 <= k < c.N_PayId && k != p ==> s1.payments[k] == s2.payments[k]
    ensures Sum0(c, s2, i) == Sum0(c, s1, i) + (if i <= p then Contrib(s2.payments[p]) - Contrib(s1.payments[p]) else 0)
    decreases c.N_PayId - i
  {
    if i < c.N_PayId {
      SumUpdate(c, s1, s2, p, i + 1);
    }
  }

  lemma SumNonNeg(c: Config, st: State, i: int)
    requires Wf(c, st) && 0 <= i
    requires forall k :: 0 <= k < c.N_PayId ==> st.payments[k].refunded <= st.payments[k].amount
    ensures Sum0(c, st, i) >= 0
    decreases c.N_PayId - i
  {
    if i < c.N_PayId {
      SumNonNeg(c, st, i + 1);
    }
  }

  lemma SumZero(c: Config, st: State, i: int)
    requires Wf(c, st) && 0 <= i
    requires forall k :: 0 <= k < c.N_PayId ==> st.payments[k].st != PSt.Captured
    ensures Sum0(c, st, i) == 0
    decreases c.N_PayId - i
  {
    if i < c.N_PayId {
      SumZero(c, st, i + 1);
    }
  }

  lemma InitEstablishes(c: Config)
  {
    var s := Init(c);
    assert |s.payments| == c.N_PayId;
    assert forall k :: 0 <= k < c.N_PayId ==> s.payments[k] == Pmt(0, 0, PSt.Created);
    assert Wf(c, s);
    SumZero(c, s, 0);
  }

  lemma authorize_Preserves(c: Config, st: State, p: int, a: int)
  {
    var st2 := authorize_Apply(c, st, p, a);
    assert st.payments[p].refunded == 0;
    assert st2.payments[p] == Pmt(a, st.payments[p].refunded, PSt.Authorized);
    assert |st2.payments| == |st.payments|;
    assert forall k :: 0 <= k < c.N_PayId && k != p ==> st.payments[k] == st2.payments[k];
    assert Wf(c, st2);
    SumUpdate(c, st, st2, p, 0);
    assert Inv_LedgerMatches(c, st2);
    assert Inv_RefundWithinAmount(c, st2);
    assert Inv_RefundedOnlyWhenCaptured(c, st2);
  }

  lemma capture_Preserves(c: Config, st: State, p: int)
  {
    var st2 := capture_Apply(c, st, p);
    assert st.payments[p].refunded == 0;
    assert st2.payments[p] == st.payments[p].(st := PSt.Captured);
    assert |st2.payments| == |st.payments|;
    assert forall k :: 0 <= k < c.N_PayId && k != p ==> st.payments[k] == st2.payments[k];
    assert Wf(c, st2);
    SumUpdate(c, st, st2, p, 0);
    assert Inv_LedgerMatches(c, st2);
    assert Inv_LedgerNonNegative(c, st2);
    assert Inv_RefundWithinAmount(c, st2);
    assert Inv_RefundedOnlyWhenCaptured(c, st2);
  }

  lemma refund_Preserves(c: Config, st: State, p: int, a: int)
  {
    var st2 := refund_Apply(c, st, p, a);
    assert st2.payments[p] == st.payments[p].(refunded := st.payments[p].refunded + a);
    assert |st2.payments| == |st.payments|;
    assert forall k :: 0 <= k < c.N_PayId && k != p ==> st.payments[k] == st2.payments[k];
    assert Wf(c, st2);
    SumUpdate(c, st, st2, p, 0);
    assert Inv_LedgerMatches(c, st2);
    assert forall k :: 0 <= k < c.N_PayId ==> st2.payments[k].refunded <= st2.payments[k].amount;
    assert Inv_RefundWithinAmount(c, st2);
    SumNonNeg(c, st2, 0);
    assert Inv_LedgerNonNegative(c, st2);
    assert Inv_RefundedOnlyWhenCaptured(c, st2);
  }

  lemma void_Preserves(c: Config, st: State, p: int)
  {
    var st2 := void_Apply(c, st, p);
    assert st.payments[p].refunded == 0;
    assert st2.payments[p] == st.payments[p].(st := PSt.Voided);
    assert |st2.payments| == |st.payments|;
    assert forall k :: 0 <= k < c.N_PayId && k != p ==> st.payments[k] == st2.payments[k];
    assert Wf(c, st2);
    SumUpdate(c, st, st2, p, 0);
    assert Inv_LedgerMatches(c, st2);
    assert Inv_RefundWithinAmount(c, st2);
    assert Inv_RefundedOnlyWhenCaptured(c, st2);
  }
}
