// SPDX-License-Identifier: Apache-2.0
// Written by a model through prove.mjs; accepted only because build.sh's gates pass.

include "../out/SeatBookingImpl.gen.dfy"

module SeatBookingImplProof refines SeatBookingImplSpec {

  function SlotInd(sl: Slot, p: int): int {
    if sl.st == SSt.Sold && sl.holder.Some? && sl.holder.value == p then 1 else 0
  }

  lemma InitCountZero(c: Config, p: int, i: int)
    requires Wf(c, Init(c)) && 0 <= i
    ensures Count0(c, Init(c), p, i) == 0
    decreases c.N_SeatId - i
  {
    if i < c.N_SeatId {
      assert Init(c).slots[i] == Slot(None, SSt.Open);
      InitCountZero(c, p, i + 1);
    }
  }

  lemma CountUpdate(c: Config, st: State, st2: State, p: int, s: int, i: int)
    requires Wf(c, st) && Wf(c, st2) && 0 <= s < c.N_SeatId && 0 <= i
    requires st2.slots == st.slots[s := st2.slots[s]]
    ensures Count0(c, st2, p, i) + (if i <= s then SlotInd(st.slots[s], p) else 0)
         == Count0(c, st, p, i) + (if i <= s then SlotInd(st2.slots[s], p) else 0)
    decreases c.N_SeatId - i
  {
    if i < c.N_SeatId {
      CountUpdate(c, st, st2, p, s, i + 1);
      if i != s {
        assert st2.slots[i] == st.slots[i];
      }
    }
  }

  lemma InitEstablishes(c: Config)
  {
    var st := Init(c);
    assert |st.slots| == c.N_SeatId;
    assert forall k0 :: 0 <= k0 < |st.slots| ==> st.slots[k0] == Slot(None, SSt.Open);
    assert Wf(c, st);
    forall u | 0 <= u < c.N_UserId
      ensures Count0(c, st, u, 0) <= MAX_PER_USER
    {
      InitCountZero(c, u, 0);
    }
    assert Inv_SoldPerUserCap(c, st);
  }

  lemma cancel_sold_Preserves(c: Config, st: State, s: int)
  {
    var st2 := cancel_sold_Apply(c, st, s);
    assert st2.slots == st.slots[s := Slot(None, SSt.Open)];
    assert st2.slots == st.slots[s := st2.slots[s]];
    assert forall k0 :: 0 <= k0 < |st2.slots| && k0 != s ==> st2.slots[k0] == st.slots[k0];
    assert Wf(c, st2);
    forall p | 0 <= p < c.N_UserId
      ensures Count0(c, st2, p, 0) <= MAX_PER_USER
    {
      CountUpdate(c, st, st2, p, s, 0);
      assert Count0(c, st, p, 0) <= MAX_PER_USER;
    }
    assert Inv_SoldPerUserCap(c, st2);
  }

  lemma confirm_Preserves(c: Config, st: State, s: int, u: int)
  {
    var st2 := confirm_Apply(c, st, s, u);
    assert st.slots[s].holder == Some(u);
    assert st2.slots == st.slots[s := Slot(Some(u), SSt.Sold)];
    assert st2.slots == st.slots[s := st2.slots[s]];
    assert forall k0 :: 0 <= k0 < |st2.slots| && k0 != s ==> st2.slots[k0] == st.slots[k0];
    assert Wf(c, st2);
    forall p | 0 <= p < c.N_UserId
      ensures Count0(c, st2, p, 0) <= MAX_PER_USER
    {
      CountUpdate(c, st, st2, p, s, 0);
      assert SlotInd(st.slots[s], p) == 0;
      if p == u {
        assert SlotInd(st2.slots[s], p) == 1;
        assert Count0(c, st, u, 0) < MAX_PER_USER;
      } else {
        assert SlotInd(st2.slots[s], p) == 0;
        assert Count0(c, st, p, 0) <= MAX_PER_USER;
      }
    }
    assert Inv_SoldPerUserCap(c, st2);
  }

  lemma hold_Preserves(c: Config, st: State, s: int, u: int)
  {
    var st2 := hold_Apply(c, st, s, u);
    assert st2.slots == st.slots[s := Slot(Some(u), SSt.Held)];
    assert st2.slots == st.slots[s := st2.slots[s]];
    assert forall k0 :: 0 <= k0 < |st2.slots| && k0 != s ==> st2.slots[k0] == st.slots[k0];
    assert Wf(c, st2);
    forall p | 0 <= p < c.N_UserId
      ensures Count0(c, st2, p, 0) <= MAX_PER_USER
    {
      CountUpdate(c, st, st2, p, s, 0);
      assert SlotInd(st.slots[s], p) == 0;
      assert SlotInd(st2.slots[s], p) == 0;
      assert Count0(c, st, p, 0) <= MAX_PER_USER;
    }
    assert Inv_SoldPerUserCap(c, st2);
  }

  lemma release_Preserves(c: Config, st: State, s: int)
  {
    var st2 := release_Apply(c, st, s);
    assert st2.slots == st.slots[s := Slot(None, SSt.Open)];
    assert st2.slots == st.slots[s := st2.slots[s]];
    assert forall k0 :: 0 <= k0 < |st2.slots| && k0 != s ==> st2.slots[k0] == st.slots[k0];
    assert Wf(c, st2);
    forall p | 0 <= p < c.N_UserId
      ensures Count0(c, st2, p, 0) <= MAX_PER_USER
    {
      CountUpdate(c, st, st2, p, s, 0);
      assert SlotInd(st.slots[s], p) == 0;
      assert SlotInd(st2.slots[s], p) == 0;
      assert Count0(c, st, p, 0) <= MAX_PER_USER;
    }
    assert Inv_SoldPerUserCap(c, st2);
  }
}
