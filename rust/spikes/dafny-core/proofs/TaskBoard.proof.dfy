// SPDX-License-Identifier: Apache-2.0
// Written by a model through prove.mjs; accepted only because build.sh's gates pass.

include "../out/TaskBoard.gen.dfy"
module TaskBoardProof refines TaskBoardSpec {

  predicate Has(st: State, j: int, p: int)
  {
    0 <= j < |st.owner| && st.owner[j].Some? && st.owner[j].value == p
  }

  lemma CountInitZero(c: Config, p: int, i: int)
    requires Wf(c, Init(c)) && 0 <= i
    ensures Count0(c, Init(c), p, i) == 0
    decreases c.N_TaskId - i
  {
    if i < c.N_TaskId {
      assert Init(c).owner[i] == None;
      CountInitZero(c, p, i + 1);
    }
  }

  lemma CountLe(c: Config, a: State, b: State, p: int, i: int)
    requires Wf(c, a) && Wf(c, b) && 0 <= i
    requires forall j :: i <= j < c.N_TaskId ==> (Has(b, j, p) ==> Has(a, j, p))
    ensures Count0(c, b, p, i) <= Count0(c, a, p, i)
    decreases c.N_TaskId - i
  {
    if i < c.N_TaskId {
      assert Has(b, i, p) ==> Has(a, i, p);
      CountLe(c, a, b, p, i + 1);
    }
  }

  lemma CountPlusOne(c: Config, a: State, b: State, p: int, i: int, t: int)
    requires Wf(c, a) && Wf(c, b) && 0 <= i
    requires forall j :: i <= j < c.N_TaskId && j != t ==> (Has(b, j, p) ==> Has(a, j, p))
    ensures Count0(c, b, p, i) <= Count0(c, a, p, i) + 1
    decreases c.N_TaskId - i
  {
    if i < c.N_TaskId {
      if i == t {
        CountLe(c, a, b, p, i + 1);
      } else {
        assert Has(b, i, p) ==> Has(a, i, p);
        CountPlusOne(c, a, b, p, i + 1, t);
      }
    }
  }

  lemma InitEstablishes(c: Config)
  {
    var s := Init(c);
    assert |s.done| == c.N_TaskId;
    assert |s.owner| == c.N_TaskId;
    assert forall k :: 0 <= k < |s.owner| ==> s.owner[k] == None;
    assert Wf(c, s);
    forall w | 0 <= w < c.N_WorkerId
      ensures Count0(c, s, w, 0) <= WIP_LIMIT
    {
      CountInitZero(c, w, 0);
    }
    assert Inv_WipLimit(c, s);
    assert Inv_DoneIsFree(c, s);
  }

  lemma claim_Preserves(c: Config, st: State, t: int, w: int)
  {
    var s2 := claim_Apply(c, st, t, w);
    assert s2.owner == st.owner[t := Some(w)];
    assert s2.done == st.done;
    forall k | 0 <= k < |s2.owner|
      ensures s2.owner[k].Some? ==> 0 <= s2.owner[k].value < c.N_WorkerId
    {
      if k != t { assert s2.owner[k] == st.owner[k]; }
    }
    assert Wf(c, s2);
    forall w2 | 0 <= w2 < c.N_WorkerId
      ensures Count0(c, s2, w2, 0) <= WIP_LIMIT
    {
      if w2 == w {
        forall j | 0 <= j < c.N_TaskId && j != t
          ensures Has(s2, j, w2) ==> Has(st, j, w2)
        {
          assert s2.owner[j] == st.owner[j];
        }
        CountPlusOne(c, st, s2, w2, 0, t);
      } else {
        forall j | 0 <= j < c.N_TaskId
          ensures Has(s2, j, w2) ==> Has(st, j, w2)
        {
          if j != t { assert s2.owner[j] == st.owner[j]; }
        }
        CountLe(c, st, s2, w2, 0);
      }
    }
    assert Inv_WipLimit(c, s2);
    forall t2 | 0 <= t2 < c.N_TaskId
      ensures s2.done[t2] ==> s2.owner[t2] == None
    {
      if t2 != t { assert s2.owner[t2] == st.owner[t2]; }
    }
    assert Inv_DoneIsFree(c, s2);
  }

  lemma finish_Preserves(c: Config, st: State, t: int, w: int)
  {
    var s2 := finish_Apply(c, st, t, w);
    assert s2.owner == st.owner[t := None];
    assert s2.done == st.done[t := true];
    forall k | 0 <= k < |s2.owner|
      ensures s2.owner[k].Some? ==> 0 <= s2.owner[k].value < c.N_WorkerId
    {
      if k != t { assert s2.owner[k] == st.owner[k]; }
    }
    assert Wf(c, s2);
    forall w2 | 0 <= w2 < c.N_WorkerId
      ensures Count0(c, s2, w2, 0) <= WIP_LIMIT
    {
      forall j | 0 <= j < c.N_TaskId
        ensures Has(s2, j, w2) ==> Has(st, j, w2)
      {
        if j != t { assert s2.owner[j] == st.owner[j]; }
      }
      CountLe(c, st, s2, w2, 0);
    }
    assert Inv_WipLimit(c, s2);
    forall t2 | 0 <= t2 < c.N_TaskId
      ensures s2.done[t2] ==> s2.owner[t2] == None
    {
      if t2 != t {
        assert s2.owner[t2] == st.owner[t2];
        assert s2.done[t2] == st.done[t2];
      }
    }
    assert Inv_DoneIsFree(c, s2);
  }

  lemma release_Preserves(c: Config, st: State, t: int)
  {
    var s2 := release_Apply(c, st, t);
    assert s2.owner == st.owner[t := None];
    assert s2.done == st.done;
    forall k | 0 <= k < |s2.owner|
      ensures s2.owner[k].Some? ==> 0 <= s2.owner[k].value < c.N_WorkerId
    {
      if k != t { assert s2.owner[k] == st.owner[k]; }
    }
    assert Wf(c, s2);
    forall w2 | 0 <= w2 < c.N_WorkerId
      ensures Count0(c, s2, w2, 0) <= WIP_LIMIT
    {
      forall j | 0 <= j < c.N_TaskId
        ensures Has(s2, j, w2) ==> Has(st, j, w2)
      {
        if j != t { assert s2.owner[j] == st.owner[j]; }
      }
      CountLe(c, st, s2, w2, 0);
    }
    assert Inv_WipLimit(c, s2);
    forall t2 | 0 <= t2 < c.N_TaskId
      ensures s2.done[t2] ==> s2.owner[t2] == None
    {
      if t2 != t { assert s2.owner[t2] == st.owner[t2]; }
    }
    assert Inv_DoneIsFree(c, s2);
  }
}
