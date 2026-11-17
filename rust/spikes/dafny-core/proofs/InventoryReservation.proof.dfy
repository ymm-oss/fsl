// SPDX-License-Identifier: Apache-2.0
// Written by a model through prove.mjs; accepted only because build.sh's gates pass.

include "../out/InventoryReservation.gen.dfy"

module InventoryReservationProof refines InventoryReservationSpec {

  function T0(c: Config, st: State, p: int, i: int): int
    requires Wf(c, st) && 0 <= i < c.N_ResId
  {
    if ((st.res[i].st == RState.Held) && (st.res[i].item.Some? && (var j_0 := st.res[i].item.value; (j_0 == p)))) then st.res[i].qty else 0
  }

  function T1(c: Config, st: State, p: int, i: int): int
    requires Wf(c, st) && 0 <= i < c.N_ResId
  {
    if ((st.res[i].st == RState.Confirmed) && (st.res[i].item.Some? && (var j_0 := st.res[i].item.value; (j_0 == p)))) then st.res[i].qty else 0
  }

  lemma Sum0Update(c: Config, st: State, st2: State, p: int, r: int, k: int)
    requires Wf(c, st) && Wf(c, st2) && 0 <= r < c.N_ResId && 0 <= k
    requires forall j :: 0 <= j < c.N_ResId && j != r ==> st2.res[j] == st.res[j]
    ensures k <= r ==> Sum0(c, st2, p, k) == Sum0(c, st, p, k) - T0(c, st, p, r) + T0(c, st2, p, r)
    ensures k > r ==> Sum0(c, st2, p, k) == Sum0(c, st, p, k)
    decreases c.N_ResId - k
  {
    if k < c.N_ResId {
      Sum0Update(c, st, st2, p, r, k + 1);
      if k != r {
        assert st2.res[k] == st.res[k];
      }
    }
  }

  lemma Sum1Update(c: Config, st: State, st2: State, p: int, r: int, k: int)
    requires Wf(c, st) && Wf(c, st2) && 0 <= r < c.N_ResId && 0 <= k
    requires forall j :: 0 <= j < c.N_ResId && j != r ==> st2.res[j] == st.res[j]
    ensures k <= r ==> Sum1(c, st2, p, k) == Sum1(c, st, p, k) - T1(c, st, p, r) + T1(c, st2, p, r)
    ensures k > r ==> Sum1(c, st2, p, k) == Sum1(c, st, p, k)
    decreases c.N_ResId - k
  {
    if k < c.N_ResId {
      Sum1Update(c, st, st2, p, r, k + 1);
      if k != r {
        assert st2.res[k] == st.res[k];
      }
    }
  }

  lemma Sum0Ge(c: Config, st: State, p: int, k: int, r: int)
    requires Wf(c, st) && 0 <= k
    ensures Sum0(c, st, p, k) >= 0
    ensures k <= r < c.N_ResId ==> Sum0(c, st, p, k) >= T0(c, st, p, r)
    decreases c.N_ResId - k
  {
    if k < c.N_ResId {
      Sum0Ge(c, st, p, k + 1, r);
      assert 0 <= st.res[k].qty;
    }
  }

  lemma Sum1Ge(c: Config, st: State, p: int, k: int, r: int)
    requires Wf(c, st) && 0 <= k
    ensures Sum1(c, st, p, k) >= 0
    ensures k <= r < c.N_ResId ==> Sum1(c, st, p, k) >= T1(c, st, p, r)
    decreases c.N_ResId - k
  {
    if k < c.N_ResId {
      Sum1Ge(c, st, p, k + 1, r);
      assert 0 <= st.res[k].qty;
    }
  }

  lemma SumZero(c: Config, st: State, p: int, k: int)
    requires Wf(c, st) && 0 <= k
    requires forall j :: k <= j < c.N_ResId ==> st.res[j].st == RState.Free
    ensures Sum0(c, st, p, k) == 0 && Sum1(c, st, p, k) == 0
    decreases c.N_ResId - k
  {
    if k < c.N_ResId {
      SumZero(c, st, p, k + 1);
    }
  }

  lemma InitEstablishes(c: Config)
  {
    var s := Init(c);
    assert forall j :: 0 <= j < c.N_ItemId ==> s.available[j] == 2;
    assert forall j :: 0 <= j < c.N_ResId ==> s.res[j] == Res(None, 0, RState.Free);
    assert Wf(c, s);
    forall p | 0 <= p < c.N_ItemId
      ensures ((s.available[p] + Sum0(c, s, p, 0)) + Sum1(c, s, p, 0)) == 2
    {
      SumZero(c, s, p, 0);
    }
    assert Inv_Conservation(c, s);
    assert Inv_FreeHasNoItem(c, s);
    assert Inv_FreeIsZero(c, s);
    assert Inv_HeldHasItem(c, s);
  }

  lemma confirm_Preserves(c: Config, st: State, r: int)
  {
    var s2 := confirm_Apply(c, st, r);
    assert s2.available == st.available;
    assert s2.res[r] == st.res[r].(st := RState.Confirmed);
    assert forall j :: 0 <= j < c.N_ResId && j != r ==> s2.res[j] == st.res[j];
    assert Wf(c, s2);
    forall p | 0 <= p < c.N_ItemId
      ensures ((s2.available[p] + Sum0(c, s2, p, 0)) + Sum1(c, s2, p, 0)) == 2
    {
      assert ((st.available[p] + Sum0(c, st, p, 0)) + Sum1(c, st, p, 0)) == 2;
      Sum0Update(c, st, s2, p, r, 0);
      Sum1Update(c, st, s2, p, r, 0);
      assert T0(c, s2, p, r) == 0;
      assert T1(c, st, p, r) == 0;
      assert T0(c, st, p, r) == T1(c, s2, p, r);
    }
    assert Inv_Conservation(c, s2);
    assert Inv_FreeHasNoItem(c, s2);
    assert Inv_FreeIsZero(c, s2);
    assert Inv_HeldHasItem(c, s2);
  }

  lemma hold_Preserves(c: Config, st: State, r: int, i: int, q: int)
  {
    var s2 := hold_Apply(c, st, r, i, q);
    assert s2.available == st.available[i := st.available[i] - q];
    assert s2.res[r] == Res(Some(i), q, RState.Held);
    assert forall j :: 0 <= j < c.N_ResId && j != r ==> s2.res[j] == st.res[j];
    assert 0 <= st.available[i] - q <= 4;
    assert Wf(c, s2);
    forall p | 0 <= p < c.N_ItemId
      ensures ((s2.available[p] + Sum0(c, s2, p, 0)) + Sum1(c, s2, p, 0)) == 2
    {
      assert ((st.available[p] + Sum0(c, st, p, 0)) + Sum1(c, st, p, 0)) == 2;
      Sum0Update(c, st, s2, p, r, 0);
      Sum1Update(c, st, s2, p, r, 0);
      assert T0(c, st, p, r) == 0;
      assert T1(c, st, p, r) == 0;
      assert T1(c, s2, p, r) == 0;
      if p == i {
        assert T0(c, s2, p, r) == q;
        assert s2.available[p] == st.available[p] - q;
      } else {
        assert T0(c, s2, p, r) == 0;
        assert s2.available[p] == st.available[p];
      }
    }
    assert Inv_Conservation(c, s2);
    assert Inv_FreeHasNoItem(c, s2);
    assert Inv_FreeIsZero(c, s2);
    assert Inv_HeldHasItem(c, s2);
  }

  lemma release_Preserves(c: Config, st: State, r: int)
  {
    var i := st.res[r].item.value;
    var qq := st.res[r].qty;
    assert 0 <= i < c.N_ItemId;
    assert 0 <= qq;
    assert ((st.available[i] + Sum0(c, st, i, 0)) + Sum1(c, st, i, 0)) == 2;
    Sum0Ge(c, st, i, 0, r);
    Sum1Ge(c, st, i, 0, r);
    assert T0(c, st, i, r) == qq;
    assert st.available[i] + qq <= 2;
    var s2 := release_Apply(c, st, r);
    assert s2.available == st.available[i := st.available[i] + qq];
    assert s2.res[r] == Res(None, 0, RState.Free);
    assert forall j :: 0 <= j < c.N_ResId && j != r ==> s2.res[j] == st.res[j];
    assert 0 <= st.available[i] + qq <= 4;
    assert Wf(c, s2);
    forall p | 0 <= p < c.N_ItemId
      ensures ((s2.available[p] + Sum0(c, s2, p, 0)) + Sum1(c, s2, p, 0)) == 2
    {
      assert ((st.available[p] + Sum0(c, st, p, 0)) + Sum1(c, st, p, 0)) == 2;
      Sum0Update(c, st, s2, p, r, 0);
      Sum1Update(c, st, s2, p, r, 0);
      assert T0(c, s2, p, r) == 0;
      assert T1(c, s2, p, r) == 0;
      assert T1(c, st, p, r) == 0;
      if p == i {
        assert T0(c, st, p, r) == qq;
        assert s2.available[p] == st.available[p] + qq;
      } else {
        assert T0(c, st, p, r) == 0;
        assert s2.available[p] == st.available[p];
      }
    }
    assert Inv_Conservation(c, s2);
    assert Inv_FreeHasNoItem(c, s2);
    assert Inv_FreeIsZero(c, s2);
    assert Inv_HeldHasItem(c, s2);
  }
}
