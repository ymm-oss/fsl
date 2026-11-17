// SPDX-License-Identifier: Apache-2.0
// Written by a model through prove.mjs; accepted only because build.sh's gates pass.

include "../out/JobPipeline.gen.dfy"

module JobPipelineProof refines JobPipelineSpec {

  lemma QueuedAt(c: Config, st: State, i: int)
    requires Inv(c, st)
    requires 0 <= i < |st.queue|
    ensures 0 <= st.queue[i] < |st.jobs|
    ensures st.jobs[st.queue[i]].st == JSt.Queued
  {
    assert i <= 2;
    assert Inv_QueuedAreQueued(c, st);
  }

  lemma NoDupAt(c: Config, st: State, a: int, b: int)
    requires Inv(c, st)
    requires 0 <= a < b < |st.queue|
    ensures st.queue[a] != st.queue[b]
  {
    assert a <= 2 && b <= 2;
    assert Inv_NoDupQueue(c, st);
  }

  lemma NotInQueue(c: Config, st: State, j: int)
    requires Inv(c, st)
    requires 0 <= j < c.N_JobId
    requires st.jobs[j].st != JSt.Queued
    ensures forall i :: 0 <= i < |st.queue| ==> st.queue[i] != j
  {
    forall i | 0 <= i < |st.queue|
      ensures st.queue[i] != j
    {
      QueuedAt(c, st, i);
    }
  }

  lemma InitEstablishes(c: Config)
  {
    var s := Init(c);
    assert |s.jobs| == c.N_JobId;
    assert forall k :: 0 <= k < |s.jobs| ==> s.jobs[k] == Job(JSt.New, 0);
    assert |s.queue| == 0;
    assert s.running == None;
    assert Wf(c, s);
    assert Inv_NoDupQueue(c, s);
    assert Inv_QueuedAreQueued(c, s);
    assert Inv_RunningMatches(c, s);
  }

  lemma finish_fail_Preserves(c: Config, st: State)
  {
    var j := st.running.value;
    NotInQueue(c, st, j);
    var s2 := finish_fail_Apply(c, st);
    assert s2.jobs == st.jobs[j := st.jobs[j].(st := JSt.Failed)];
    assert s2.queue == st.queue;
    assert s2.running == None;
    assert |s2.jobs| == |st.jobs|;
    forall k | 0 <= k < |s2.jobs|
      ensures 0 <= s2.jobs[k].tries <= 2
    {
      if k == j {
        assert s2.jobs[k].tries == st.jobs[j].tries;
      } else {
        assert s2.jobs[k] == st.jobs[k];
      }
    }
    assert Wf(c, s2);
    forall a, b | 0 <= a <= 2 && 0 <= b <= 2 && a < b && b < |s2.queue|
      ensures s2.queue[a] != s2.queue[b]
    {
      NoDupAt(c, st, a, b);
    }
    assert Inv_NoDupQueue(c, s2);
    forall i | 0 <= i <= 2 && i < |s2.queue|
      ensures s2.jobs[s2.queue[i]].st == JSt.Queued
    {
      QueuedAt(c, st, i);
      assert st.queue[i] != j;
      assert s2.jobs[st.queue[i]] == st.jobs[st.queue[i]];
    }
    assert Inv_QueuedAreQueued(c, s2);
    assert Inv_RunningMatches(c, s2);
  }

  lemma finish_ok_Preserves(c: Config, st: State)
  {
    var j := st.running.value;
    NotInQueue(c, st, j);
    var s2 := finish_ok_Apply(c, st);
    assert s2.jobs == st.jobs[j := st.jobs[j].(st := JSt.Done)];
    assert s2.queue == st.queue;
    assert s2.running == None;
    assert |s2.jobs| == |st.jobs|;
    forall k | 0 <= k < |s2.jobs|
      ensures 0 <= s2.jobs[k].tries <= 2
    {
      if k == j {
        assert s2.jobs[k].tries == st.jobs[j].tries;
      } else {
        assert s2.jobs[k] == st.jobs[k];
      }
    }
    assert Wf(c, s2);
    forall a, b | 0 <= a <= 2 && 0 <= b <= 2 && a < b && b < |s2.queue|
      ensures s2.queue[a] != s2.queue[b]
    {
      NoDupAt(c, st, a, b);
    }
    assert Inv_NoDupQueue(c, s2);
    forall i | 0 <= i <= 2 && i < |s2.queue|
      ensures s2.jobs[s2.queue[i]].st == JSt.Queued
    {
      QueuedAt(c, st, i);
      assert st.queue[i] != j;
      assert s2.jobs[st.queue[i]] == st.jobs[st.queue[i]];
    }
    assert Inv_QueuedAreQueued(c, s2);
    assert Inv_RunningMatches(c, s2);
  }

  lemma finish_retry_Preserves(c: Config, st: State)
  {
    var j := st.running.value;
    NotInQueue(c, st, j);
    var s2 := finish_retry_Apply(c, st);
    var A := st.jobs[j].(st := JSt.Queued);
    var J1 := st.jobs[j := A];
    assert J1[j] == A;
    assert s2.jobs == J1[j := A.(tries := st.jobs[j].tries + 1)];
    assert |s2.jobs| == |st.jobs|;
    assert s2.jobs[j] == Job(JSt.Queued, st.jobs[j].tries + 1);
    assert forall k :: 0 <= k < |st.jobs| && k != j ==> s2.jobs[k] == st.jobs[k];
    assert s2.queue == st.queue + [j];
    assert |s2.queue| == |st.queue| + 1;
    assert s2.running == None;
    assert 0 <= st.jobs[j].tries < 1;
    forall k | 0 <= k < |s2.jobs|
      ensures 0 <= s2.jobs[k].tries <= 2
    {
      if k == j {
      } else {
        assert s2.jobs[k] == st.jobs[k];
      }
    }
    forall i | 0 <= i < |s2.queue|
      ensures 0 <= s2.queue[i] < c.N_JobId
    {
      if i < |st.queue| {
        assert s2.queue[i] == st.queue[i];
      } else {
        assert s2.queue[i] == j;
      }
    }
    assert Wf(c, s2);
    forall a, b | 0 <= a <= 2 && 0 <= b <= 2 && a < b && b < |s2.queue|
      ensures s2.queue[a] != s2.queue[b]
    {
      assert s2.queue[a] == st.queue[a];
      if b < |st.queue| {
        assert s2.queue[b] == st.queue[b];
        NoDupAt(c, st, a, b);
      } else {
        assert s2.queue[b] == j;
        assert st.queue[a] != j;
      }
    }
    assert Inv_NoDupQueue(c, s2);
    forall i | 0 <= i <= 2 && i < |s2.queue|
      ensures s2.jobs[s2.queue[i]].st == JSt.Queued
    {
      if i < |st.queue| {
        assert s2.queue[i] == st.queue[i];
        QueuedAt(c, st, i);
        assert st.queue[i] != j;
        assert s2.jobs[st.queue[i]] == st.jobs[st.queue[i]];
      } else {
        assert s2.queue[i] == j;
      }
    }
    assert Inv_QueuedAreQueued(c, s2);
    assert Inv_RunningMatches(c, s2);
  }

  lemma start_Preserves(c: Config, st: State)
  {
    var j := st.queue[0];
    var s2 := start_Apply(c, st);
    assert 0 <= j < c.N_JobId;
    assert s2.jobs == st.jobs[j := st.jobs[j].(st := JSt.Running)];
    assert s2.queue == st.queue[1..];
    assert |s2.queue| == |st.queue| - 1;
    assert s2.running == Some(j);
    assert |s2.jobs| == |st.jobs|;
    forall k | 0 <= k < |s2.jobs|
      ensures 0 <= s2.jobs[k].tries <= 2
    {
      if k == j {
        assert s2.jobs[k].tries == st.jobs[j].tries;
      } else {
        assert s2.jobs[k] == st.jobs[k];
      }
    }
    forall i | 0 <= i < |s2.queue|
      ensures 0 <= s2.queue[i] < c.N_JobId
    {
      assert s2.queue[i] == st.queue[i + 1];
    }
    assert Wf(c, s2);
    forall a, b | 0 <= a <= 2 && 0 <= b <= 2 && a < b && b < |s2.queue|
      ensures s2.queue[a] != s2.queue[b]
    {
      assert s2.queue[a] == st.queue[a + 1];
      assert s2.queue[b] == st.queue[b + 1];
      NoDupAt(c, st, a + 1, b + 1);
    }
    assert Inv_NoDupQueue(c, s2);
    forall i | 0 <= i <= 2 && i < |s2.queue|
      ensures s2.jobs[s2.queue[i]].st == JSt.Queued
    {
      assert s2.queue[i] == st.queue[i + 1];
      NoDupAt(c, st, 0, i + 1);
      QueuedAt(c, st, i + 1);
      assert s2.jobs[st.queue[i + 1]] == st.jobs[st.queue[i + 1]];
    }
    assert Inv_QueuedAreQueued(c, s2);
    assert s2.jobs[j].st == JSt.Running;
    assert Inv_RunningMatches(c, s2);
  }

  lemma submit_Preserves(c: Config, st: State, j: int)
  {
    NotInQueue(c, st, j);
    var s2 := submit_Apply(c, st, j);
    assert s2.jobs == st.jobs[j := st.jobs[j].(st := JSt.Queued)];
    assert |s2.jobs| == |st.jobs|;
    assert s2.jobs[j] == Job(JSt.Queued, st.jobs[j].tries);
    assert forall k :: 0 <= k < |st.jobs| && k != j ==> s2.jobs[k] == st.jobs[k];
    assert s2.queue == st.queue + [j];
    assert |s2.queue| == |st.queue| + 1;
    assert s2.running == st.running;
    forall k | 0 <= k < |s2.jobs|
      ensures 0 <= s2.jobs[k].tries <= 2
    {
      if k == j {
      } else {
        assert s2.jobs[k] == st.jobs[k];
      }
    }
    forall i | 0 <= i < |s2.queue|
      ensures 0 <= s2.queue[i] < c.N_JobId
    {
      if i < |st.queue| {
        assert s2.queue[i] == st.queue[i];
      } else {
        assert s2.queue[i] == j;
      }
    }
    assert Wf(c, s2);
    forall a, b | 0 <= a <= 2 && 0 <= b <= 2 && a < b && b < |s2.queue|
      ensures s2.queue[a] != s2.queue[b]
    {
      assert s2.queue[a] == st.queue[a];
      if b < |st.queue| {
        assert s2.queue[b] == st.queue[b];
        NoDupAt(c, st, a, b);
      } else {
        assert s2.queue[b] == j;
        assert st.queue[a] != j;
      }
    }
    assert Inv_NoDupQueue(c, s2);
    forall i | 0 <= i <= 2 && i < |s2.queue|
      ensures s2.jobs[s2.queue[i]].st == JSt.Queued
    {
      if i < |st.queue| {
        assert s2.queue[i] == st.queue[i];
        QueuedAt(c, st, i);
        assert st.queue[i] != j;
        assert s2.jobs[st.queue[i]] == st.jobs[st.queue[i]];
      } else {
        assert s2.queue[i] == j;
      }
    }
    assert Inv_QueuedAreQueued(c, s2);
    if st.running.Some? {
      var r := st.running.value;
      assert st.jobs[r].st == JSt.Running;
      assert r != j;
      assert s2.jobs[r] == st.jobs[r];
    }
    assert Inv_RunningMatches(c, s2);
  }
}
