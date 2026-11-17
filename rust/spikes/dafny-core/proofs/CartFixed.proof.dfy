// SPDX-License-Identifier: Apache-2.0
// Written by a model through prove.mjs; accepted only because build.sh's gates pass.

include "../out/CartFixed.gen.dfy"
module CartFixedProof refines CartFixedSpec {
  lemma InitEstablishes(c: Config)
  {
    var st := Init(c);
    assert |st.cart| == c.N_UserId;
    assert |st.stock| == c.N_ItemId;
    assert forall k0 :: 0 <= k0 < |st.cart| ==> st.cart[k0] == None;
    assert forall k0 :: 0 <= k0 < |st.stock| ==> st.stock[k0] == 1;
    assert Wf(c, st);
    assert Inv_NoNegativeStock(c, st);
  }

  lemma add_to_cart_Preserves(c: Config, st: State, u: int, i: int)
  {
    var st' := add_to_cart_Apply(c, st, u, i);
    assert st'.stock == st.stock;
    assert |st'.cart| == |st.cart|;
    assert forall k0 :: 0 <= k0 < |st'.cart| ==> ((st'.cart[k0].Some? ==> (0 <= st'.cart[k0].value < c.N_ItemId)));
    assert Wf(c, st');
    assert Inv_NoNegativeStock(c, st');
  }

  lemma checkout_Preserves(c: Config, st: State, u: int)
  {
    var i := st.cart[u].value;
    assert 0 <= i < c.N_ItemId;
    assert st.stock[i] > 0;
    var st' := checkout_Apply(c, st, u);
    assert st'.stock == st.stock[i := st.stock[i] - 1];
    assert st'.cart == st.cart[u := None];
    assert |st'.cart| == |st.cart|;
    assert |st'.stock| == |st.stock|;
    assert forall k0 :: 0 <= k0 < |st'.cart| ==> ((st'.cart[k0].Some? ==> (0 <= st'.cart[k0].value < c.N_ItemId)));
    assert Wf(c, st');
    assert forall j :: 0 <= j < c.N_ItemId ==> st'.stock[j] >= 0;
    assert Inv_NoNegativeStock(c, st');
  }

  lemma remove_from_cart_Preserves(c: Config, st: State, u: int)
  {
    var st' := remove_from_cart_Apply(c, st, u);
    assert st'.stock == st.stock;
    assert st'.cart == st.cart[u := None];
    assert |st'.cart| == |st.cart|;
    assert forall k0 :: 0 <= k0 < |st'.cart| ==> ((st'.cart[k0].Some? ==> (0 <= st'.cart[k0].value < c.N_ItemId)));
    assert Wf(c, st');
    assert Inv_NoNegativeStock(c, st');
  }
}
