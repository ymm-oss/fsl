// SPDX-License-Identifier: Apache-2.0
// Written by a model through prove.mjs; accepted only because build.sh's gates pass.

include "../out/CartImpl.gen.dfy"

module CartImplProof refines CartImplSpec {
  lemma InitEstablishes(c: Config)
  {
    var s := Init(c);
    assert |s.impl_cart| == c.N_UserId;
    assert forall k :: 0 <= k < |s.impl_cart| ==> s.impl_cart[k] == None;
    assert |s.impl_stock| == c.N_ItemId;
    assert forall k :: 0 <= k < |s.impl_stock| ==> s.impl_stock[k] == 1;
    assert |s.reserved| == c.N_ItemId;
    assert forall k :: 0 <= k < |s.reserved| ==> s.reserved[k] == 0;
  }

  lemma add_to_cart_Preserves(c: Config, st: State, u: int, i: int)
  {
    var s := add_to_cart_Apply(c, st, u, i);
    assert s.impl_cart == st.impl_cart[u := Some(i)];
    assert forall k :: 0 <= k < |s.impl_cart| ==> (s.impl_cart[k].Some? ==> 0 <= s.impl_cart[k].value < c.N_ItemId) by {
      forall k | 0 <= k < |s.impl_cart|
        ensures s.impl_cart[k].Some? ==> 0 <= s.impl_cart[k].value < c.N_ItemId
      {
        if k == u {
        } else {
          assert s.impl_cart[k] == st.impl_cart[k];
        }
      }
    }
  }

  lemma impl_checkout_Preserves(c: Config, st: State, u: int)
  {
    var i := st.impl_cart[u].value;
    assert 0 <= i < c.N_ItemId;
    assert st.impl_stock[i] > st.reserved[i] >= 0;
    var s := impl_checkout_Apply(c, st, u);
    assert s.impl_stock == st.impl_stock[i := st.impl_stock[i] - 1];
    assert s.impl_cart == st.impl_cart[u := None];
    assert s.reserved == st.reserved;
    forall k | 0 <= k < |s.impl_stock|
      ensures 0 <= s.impl_stock[k] <= 3
    {
      if k == i {
      } else {
        assert s.impl_stock[k] == st.impl_stock[k];
      }
    }
    forall k | 0 <= k < |s.impl_cart|
      ensures s.impl_cart[k].Some? ==> 0 <= s.impl_cart[k].value < c.N_ItemId
    {
      if k == u {
      } else {
        assert s.impl_cart[k] == st.impl_cart[k];
      }
    }
  }

  lemma remove_from_cart_Preserves(c: Config, st: State, u: int)
  {
    var s := remove_from_cart_Apply(c, st, u);
    assert s.impl_cart == st.impl_cart[u := None];
    forall k | 0 <= k < |s.impl_cart|
      ensures s.impl_cart[k].Some? ==> 0 <= s.impl_cart[k].value < c.N_ItemId
    {
      if k == u {
      } else {
        assert s.impl_cart[k] == st.impl_cart[k];
      }
    }
  }

  lemma reserve_Preserves(c: Config, st: State, i: int)
  {
    var s := reserve_Apply(c, st, i);
    assert s.impl_stock == st.impl_stock[i := st.impl_stock[i] + 1];
    assert s.reserved == st.reserved[i := st.reserved[i] + 1];
    assert s.impl_cart == st.impl_cart;
    forall k | 0 <= k < |s.impl_stock|
      ensures 0 <= s.impl_stock[k] <= 3
    {
      if k == i {
      } else {
        assert s.impl_stock[k] == st.impl_stock[k];
      }
    }
    forall k | 0 <= k < |s.reserved|
      ensures 0 <= s.reserved[k] <= 3
    {
      if k == i {
      } else {
        assert s.reserved[k] == st.reserved[k];
      }
    }
  }
}
