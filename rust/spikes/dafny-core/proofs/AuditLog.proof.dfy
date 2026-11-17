// SPDX-License-Identifier: Apache-2.0
// Written by a model through prove.mjs; accepted only because build.sh's gates pass.

include "../out/AuditLog.gen.dfy"

module AuditLogProof refines AuditLogSpec {

  lemma SumAppend(c: Config, st: State, st2: State, a: int, i: int)
    requires Wf(c, st) && Wf(c, st2) && 0 <= i
    requires |st.log| < 4 && st2.log == st.log + [a]
    ensures Sum0(c, st2, i) == Sum0(c, st, i) + (if i <= |st.log| then a else 0)
    decreases 4 - i
  {
    if i < 4 {
      SumAppend(c, st, st2, a, i + 1);
      if i < |st.log| {
        assert st2.log[i] == st.log[i];
      } else if i == |st.log| {
        assert st2.log[i] == a;
      }
    }
  }

  lemma InitEstablishes(c: Config)
  {
    var s := Init(c);
    assert Wf(c, s);
    assert Sum0(c, s, 4) == 0;
    assert Sum0(c, s, 3) == 0;
    assert Sum0(c, s, 2) == 0;
    assert Sum0(c, s, 1) == 0;
    assert Sum0(c, s, 0) == 0;
  }

  lemma deposit_Preserves(c: Config, st: State, a: int)
  {
    var st2 := deposit_Apply(c, st, a);
    assert st2.log == st.log + [a];
    assert |st2.log| == |st.log| + 1;
    assert forall k0 :: 0 <= k0 < |st2.log| ==> (0 <= st2.log[k0] <= 3) by {
      forall k0 | 0 <= k0 < |st2.log|
        ensures 0 <= st2.log[k0] <= 3
      {
        if k0 < |st.log| {
          assert st2.log[k0] == st.log[k0];
        } else {
          assert st2.log[k0] == a;
        }
      }
    }
    assert Wf(c, st2);
    SumAppend(c, st, st2, a, 0);
    assert Sum0(c, st2, 0) == Sum0(c, st, 0) + a;
    assert st2.balance == st.balance + a;
    assert Inv_BalanceMatchesLog(c, st2);
    assert Inv_BalanceNonNegative(c, st2);
    assert forall i_0: int :: 0 <= i_0 <= 3 ==> ((i_0 < |st2.log|) ==> (st2.log[i_0] > 0)) by {
      forall i_0: int | 0 <= i_0 <= 3 && i_0 < |st2.log|
        ensures st2.log[i_0] > 0
      {
        if i_0 < |st.log| {
          assert st2.log[i_0] == st.log[i_0];
        } else {
          assert st2.log[i_0] == a;
        }
      }
    }
    assert Inv_EntriesPositive(c, st2);
  }
}
