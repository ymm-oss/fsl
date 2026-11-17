// SPDX-License-Identifier: Apache-2.0
// Written by a model through prove.mjs; accepted only because build.sh's gates pass.

include "../out/OrderWorkflow.gen.dfy"

module OrderWorkflowProof refines OrderWorkflowSpec {

  function Contrib(x: Order): int {
    if (x.status == Status.Paid) || (x.status == Status.Shipped) then x.qty else 0
  }

  lemma SumUpdate(c: Config, st: State, st2: State, o: int, i: int)
    requires Wf(c, st) && Wf(c, st2)
    requires 0 <= o < c.N_OrderId && 0 <= i
    requires forall k :: 0 <= k < c.N_OrderId && k != o ==> st2.orders[k] == st.orders[k]
    ensures Sum0(c, st2, i) == Sum0(c, st, i)
      + (if i <= o then Contrib(st2.orders[o]) - Contrib(st.orders[o]) else 0)
    decreases c.N_OrderId - i
  {
    if i < c.N_OrderId {
      SumUpdate(c, st, st2, o, i + 1);
    }
  }

  lemma SumNonneg(c: Config, st: State, i: int)
    requires Wf(c, st) && 0 <= i
    ensures Sum0(c, st, i) >= 0
    decreases c.N_OrderId - i
  {
    if i < c.N_OrderId {
      SumNonneg(c, st, i + 1);
    }
  }

  lemma SumInitZero(c: Config, i: int)
    requires Wf(c, Init(c)) && 0 <= i
    ensures Sum0(c, Init(c), i) == 0
    decreases c.N_OrderId - i
  {
    if i < c.N_OrderId {
      SumInitZero(c, i + 1);
    }
  }

  lemma InitEstablishes(c: Config)
  {
    var st := Init(c);
    assert |st.orders| == c.N_OrderId;
    assert forall k0 :: 0 <= k0 < |st.orders| ==> st.orders[k0] == Order(0, Status.Draft);
    assert Wf(c, st);
    SumInitZero(c, 0);
  }

  lemma cancel_Preserves(c: Config, st: State, o: int)
  {
    var st2 := cancel_Apply(c, st, o);
    assert |st2.orders| == c.N_OrderId;
    assert forall k :: 0 <= k < c.N_OrderId && k != o ==> st2.orders[k] == st.orders[k];
    assert st2.orders[o].qty == st.orders[o].qty;
    assert Wf(c, st2);
    SumUpdate(c, st, st2, o, 0);
    SumNonneg(c, st2, 0);
    assert o !in st.shipped by {
      if o in st.shipped {
        assert st.orders[o].status == Status.Shipped;
      }
    }
    assert st2.shipped == st.shipped;
    assert Inv_ShippedWasPaid(c, st2);
  }

  lemma pay_Preserves(c: Config, st: State, o: int)
  {
    var st2 := pay_Apply(c, st, o);
    assert |st2.orders| == c.N_OrderId;
    assert forall k :: 0 <= k < c.N_OrderId && k != o ==> st2.orders[k] == st.orders[k];
    assert st2.orders[o].qty == st.orders[o].qty;
    assert Wf(c, st2);
    SumUpdate(c, st, st2, o, 0);
    SumNonneg(c, st2, 0);
    assert o !in st.shipped by {
      if o in st.shipped {
        assert st.orders[o].status == Status.Shipped;
      }
    }
    assert st2.shipped == st.shipped;
    assert Inv_ShippedWasPaid(c, st2);
  }

  lemma place_Preserves(c: Config, st: State, o: int, q: int)
  {
    var st2 := place_Apply(c, st, o, q);
    assert |st2.orders| == c.N_OrderId;
    assert st2.orders[o] == Order(q, Status.Placed);
    assert forall k :: 0 <= k < c.N_OrderId && k != o ==> st2.orders[k] == st.orders[k];
    assert Wf(c, st2);
    SumUpdate(c, st, st2, o, 0);
    SumNonneg(c, st2, 0);
    assert o !in st.shipped by {
      if o in st.shipped {
        assert st.orders[o].status == Status.Shipped;
      }
    }
    assert st2.shipped == st.shipped;
    assert st2.revenue == st.revenue;
    assert Inv_ShippedWasPaid(c, st2);
  }

  lemma ship_Preserves(c: Config, st: State, o: int)
  {
    var st2 := ship_Apply(c, st, o);
    assert |st2.orders| == c.N_OrderId;
    assert forall k :: 0 <= k < c.N_OrderId && k != o ==> st2.orders[k] == st.orders[k];
    assert st2.orders[o].qty == st.orders[o].qty;
    assert st2.orders[o].status == Status.Shipped;
    assert st2.shipped == st.shipped + {o};
    assert Wf(c, st2);
    SumUpdate(c, st, st2, o, 0);
    SumNonneg(c, st2, 0);
    assert st2.revenue == st.revenue;
    assert Inv_ShippedWasPaid(c, st2);
    assert o in st2.shipped;
  }
}
