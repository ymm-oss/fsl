// SPDX-License-Identifier: Apache-2.0
// Written by a model through prove.mjs; accepted only because build.sh's gates pass.

include "../out/MutexQueue.gen.dfy"

module MutexQueueProof refines MutexQueueSpec {

  lemma InitEstablishes(c: Config)
  {
    var st := Init(c);
    assert st.holder == None;
    assert st.waiters == [];
    assert Wf(c, st);
    assert Inv_HolderNotWaiting(c, st);
    assert Inv_NoDuplicateWaiters(c, st);
    assert Inv_WaitersImplyHolder(c, st);
  }

  lemma acquire_free_Preserves(c: Config, st: State, p: int)
  {
    var st' := acquire_free_Apply(c, st, p);
    assert st'.holder == Some(p);
    assert st'.waiters == st.waiters;
    assert |st'.waiters| == 0;
    assert Wf(c, st');
    assert Inv_HolderNotWaiting(c, st');
    assert Inv_NoDuplicateWaiters(c, st');
    assert Inv_WaitersImplyHolder(c, st');
  }

  lemma enqueue_Preserves(c: Config, st: State, p: int)
  {
    var st' := enqueue_Apply(c, st, p);
    var w := st.waiters;
    var w' := st'.waiters;
    assert w' == w + [p];
    assert |w'| == |w| + 1;
    assert |w'| <= 3;
    assert forall k :: 0 <= k < |w| ==> w'[k] == w[k];
    assert w'[|w|] == p;
    assert p !in w;

    forall k0 | 0 <= k0 < |w'|
      ensures 0 <= w'[k0] < c.N_ProcId
    {
      if k0 < |w| {
        assert w'[k0] == w[k0];
      } else {
        assert w'[k0] == p;
      }
    }
    assert st'.holder == st.holder;
    assert Wf(c, st');

    assert st.holder.Some?;
    var h := st.holder.value;
    assert h != p;
    assert h !in w;
    assert h !in w' by {
      if h in w' {
        var k :| 0 <= k < |w'| && w'[k] == h;
        if k < |w| {
          assert w[k] == h;
          assert h in w;
        } else {
          assert w'[k] == p;
        }
      }
    }
    assert Inv_HolderNotWaiting(c, st');

    forall i: int, j: int | 0 <= i <= 2 && 0 <= j <= 2 && i < j && j < |w'|
      ensures w'[i] != w'[j]
    {
      if j < |w| {
        assert w'[i] == w[i];
        assert w'[j] == w[j];
        assert w[i] != w[j];
      } else {
        assert j == |w|;
        assert w'[j] == p;
        assert w'[i] == w[i];
        assert w[i] in w;
      }
    }
    assert Inv_NoDuplicateWaiters(c, st');
    assert Inv_WaitersImplyHolder(c, st');
  }

  lemma release_handoff_Preserves(c: Config, st: State)
  {
    var st' := release_handoff_Apply(c, st);
    var w := st.waiters;
    var w' := st'.waiters;
    assert |w| > 0;
    assert |w| <= 3;
    assert st'.holder == Some(w[0]);
    assert w' == w[1..];
    assert |w'| == |w| - 1;
    assert forall k :: 0 <= k < |w'| ==> w'[k] == w[k + 1];

    assert 0 <= w[0] < c.N_ProcId;
    forall k0 | 0 <= k0 < |w'|
      ensures 0 <= w'[k0] < c.N_ProcId
    {
      assert w'[k0] == w[k0 + 1];
    }
    assert Wf(c, st');

    forall j | 0 <= j < |w'|
      ensures w'[j] != w[0]
    {
      assert w'[j] == w[j + 1];
      assert j + 1 <= 2;
      assert 0 < j + 1 < |w|;
      assert w[0] != w[j + 1];
    }
    assert w[0] !in w';
    assert Inv_HolderNotWaiting(c, st');

    forall i: int, j: int | 0 <= i <= 2 && 0 <= j <= 2 && i < j && j < |w'|
      ensures w'[i] != w'[j]
    {
      assert w'[i] == w[i + 1];
      assert w'[j] == w[j + 1];
      assert j + 1 < |w|;
      assert j + 1 <= 2;
      assert i + 1 <= 2;
      assert i + 1 < j + 1;
      assert w[i + 1] != w[j + 1];
    }
    assert Inv_NoDuplicateWaiters(c, st');
    assert Inv_WaitersImplyHolder(c, st');
  }

  lemma release_idle_Preserves(c: Config, st: State)
  {
    var st' := release_idle_Apply(c, st);
    assert st'.holder == None;
    assert st'.waiters == st.waiters;
    assert |st'.waiters| == 0;
    assert Wf(c, st');
    assert Inv_HolderNotWaiting(c, st');
    assert Inv_NoDuplicateWaiters(c, st');
    assert Inv_WaitersImplyHolder(c, st');
  }
}
